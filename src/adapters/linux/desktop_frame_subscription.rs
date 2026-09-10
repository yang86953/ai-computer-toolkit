//! 一条已授权 PipeWire stream 的常驻消费者。worker 独占原生资源，停止时 join。
use super::desktop_frame_pipewire::{
    PIPEWIRE_INITIALIZED, PipeWireStreamTarget, format_parameter, packed_format,
};
use super::desktop_session_host_activity_logind::SystemLoginSessionMonitor;
use crate::components::{
    desktop_frame_changes::Region,
    desktop_frame_stream::{FrameMailbox, FrameUpdate, MappedFrame},
    desktop_session_frame_capture::{DesktopFrameDataError, DesktopPackedPixelFormat},
};
use pipewire::{self as pw, properties::properties, spa};
use spa::{
    buffer::{
        ChunkFlags, DataType,
        meta::{MetaHeader, MetaHeaderFlags, MetaVideoDamage},
    },
    pod::{Object, Pod, Property, Value},
};
use std::{
    cell::RefCell,
    io::Cursor,
    os::fd::OwnedFd,
    rc::Rc,
    sync::Arc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[cfg(test)]
#[path = "desktop_frame_fixture_source.rs"]
mod fixture_source;

#[cfg(test)]
#[path = "desktop_frame_subscription_tests.rs"]
mod tests;

struct ProducerGuard(Arc<FrameMailbox>);
impl Drop for ProducerGuard {
    fn drop(&mut self) {
        self.0.finish("CAPTURE_WORKER_FAILED");
    }
}

pub(super) struct PipeWireSubscription {
    id: String,
    mailbox: Arc<FrameMailbox>,
    worker: Option<JoinHandle<()>>,
}
impl PipeWireSubscription {
    pub(super) fn start(
        id: &str,
        remote: OwnedFd,
        target: PipeWireStreamTarget,
        lifetime: Duration,
        monitor: SystemLoginSessionMonitor,
    ) -> Result<Self, &'static str> {
        let mailbox = Arc::new(FrameMailbox::default());
        let producer = mailbox.clone();
        let worker = std::thread::Builder::new()
            .name("desktop-frame-stream".into())
            .spawn(move || {
                let _guard = ProducerGuard(producer.clone());
                let result = run(remote, target, lifetime, &producer, monitor);
                producer.finish(result.err().unwrap_or("SUBSCRIPTION_CLOSED"));
            })
            .map_err(|_| "CAPTURE_START_FAILED")?;
        Ok(Self {
            id: id.to_owned(),
            mailbox,
            worker: Some(worker),
        })
    }
    pub(super) fn stats(&self) -> (u64, u64) {
        self.mailbox.stats()
    }
    pub(super) fn stop(mut self) -> (u64, u64) {
        self.mailbox.finish("SUBSCRIPTION_CLOSED");
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.mailbox.stats()
    }
    pub(super) fn matches(&self, id: &str) -> bool {
        self.id == id
    }
    pub(super) fn next(
        &self,
        id: &str,
        after: u64,
        wait: Duration,
    ) -> Result<Option<FrameUpdate>, &'static str> {
        if !self.matches(id) {
            return Err("STALE_SUBSCRIPTION");
        }
        self.mailbox.next(after, wait)
    }
}
impl Drop for PipeWireSubscription {
    fn drop(&mut self) {
        self.mailbox.finish("SUBSCRIPTION_CLOSED");
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Default)]
struct Format {
    packed: Option<DesktopPackedPixelFormat>,
    width: u32,
    height: u32,
    renegotiated: bool,
}

fn meta_parameter(kind: u32, size: i32) -> Result<Vec<u8>, &'static str> {
    let object = Object {
        type_: spa::sys::SPA_TYPE_OBJECT_ParamMeta,
        id: spa::sys::SPA_PARAM_Meta,
        properties: vec![
            Property::new(
                spa::sys::SPA_PARAM_META_type,
                Value::Id(spa::utils::Id(kind)),
            ),
            Property::new(spa::sys::SPA_PARAM_META_size, Value::Int(size)),
        ],
    };
    spa::pod::serialize::PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(object))
        .map(|(c, _)| c.into_inner())
        .map_err(|_| "CAPTURE_FRAME_METADATA_FAILED")
}

/// Metadata iterator is bounded before use; malformed damage falls back to full reads.
fn damage_regions(buffer: &pw::buffer::Buffer<'_>, width: u32, height: u32) -> Option<Vec<Region>> {
    let meta = buffer.find_meta::<MetaVideoDamage>()?;
    let raw = meta.as_raw();
    let unit = std::mem::size_of::<spa::sys::spa_meta_region>();
    if raw.data.is_null()
        || raw.size as usize > 256 * unit
        || !(raw.size as usize).is_multiple_of(unit)
    {
        return None;
    }
    let mut result = Vec::new();
    for r in meta.iter() {
        let p = r.position();
        let size = r.size();
        let (Ok(x), Ok(y)) = (u32::try_from(p.x), u32::try_from(p.y)) else {
            return None;
        };
        if size.width == 0
            || size.height == 0
            || x.checked_add(size.width).is_none_or(|end| end > width)
            || y.checked_add(size.height).is_none_or(|end| end > height)
        {
            return None;
        }
        result.push(Region {
            x,
            y,
            width: size.width,
            height: size.height,
        });
    }
    Some(result)
}

fn run(
    remote: OwnedFd,
    target: PipeWireStreamTarget,
    lifetime: Duration,
    mailbox: &Arc<FrameMailbox>,
    mut monitor: SystemLoginSessionMonitor,
) -> Result<(), &'static str> {
    run_stream(remote, target, lifetime, mailbox, move || {
        monitor.poll().map_err(|e| e.code())
    })
}

fn run_stream(
    remote: OwnedFd,
    target: PipeWireStreamTarget,
    lifetime: Duration,
    mailbox: &Arc<FrameMailbox>,
    mut liveness: impl FnMut() -> Result<(), &'static str> + 'static,
) -> Result<(), &'static str> {
    liveness()?;
    PIPEWIRE_INITIALIZED.call_once(pw::init);
    let main_loop = pw::main_loop::MainLoopRc::new(None).map_err(|_| "CAPTURE_UNAVAILABLE")?;
    let context =
        pw::context::ContextRc::new(&main_loop, None).map_err(|_| "CAPTURE_UNAVAILABLE")?;
    let core = context
        .connect_fd_rc(remote, None)
        .map_err(|_| "CAPTURE_UNAVAILABLE")?;
    let failure_mailbox = mailbox.clone();
    let failure_loop = main_loop.clone();
    let _core_listener = core
        .add_listener_local()
        .error(move |_, _, _, _| {
            failure_mailbox.finish("CAPTURE_STREAM_CLOSED");
            failure_loop.quit();
        })
        .register();
    let mut props = properties! {*pw::keys::MEDIA_TYPE=>"Video",*pw::keys::MEDIA_CATEGORY=>"Capture",*pw::keys::MEDIA_ROLE=>"Screen"};
    let target = match target {
        PipeWireStreamTarget::NodeId(id) => Some(id),
        PipeWireStreamTarget::Serial(serial) => {
            props.insert(*pw::keys::TARGET_OBJECT, serial.to_string());
            None
        }
    };
    let stream = pw::stream::StreamBox::new(&core, "ai-computer-toolkit-frame-subscription", props)
        .map_err(|_| "CAPTURE_START_FAILED")?;
    let format = Rc::new(RefCell::new(Format::default()));
    let parameter_format = format.clone();
    let process_format = format.clone();
    let error_mailbox = mailbox.clone();
    let error_loop = main_loop.clone();
    let parameter_mailbox = mailbox.clone();
    let parameter_loop = main_loop.clone();
    let process_mailbox = mailbox.clone();
    let process_loop = main_loop.clone();
    let _listener = stream
        .add_local_listener::<()>()
        .state_changed(move |_, _, old, new| {
            if matches!(new, pw::stream::StreamState::Error(_))
                || (matches!(new, pw::stream::StreamState::Unconnected)
                    && !matches!(old, pw::stream::StreamState::Unconnected))
            {
                error_mailbox.finish("CAPTURE_STREAM_CLOSED");
                error_loop.quit();
            }
        })
        .param_changed(move |stream, _, id, parameter| {
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let parsed = (|| {
                let parameter = parameter.ok_or("CAPTURE_FRAME_METADATA_FAILED")?;
                let (kind, subtype) = spa::param::format_utils::parse_format(parameter)
                    .map_err(|_| "CAPTURE_FRAME_METADATA_FAILED")?;
                if kind != spa::param::format::MediaType::Video
                    || subtype != spa::param::format::MediaSubtype::Raw
                {
                    return Err("CAPTURE_FRAME_METADATA_FAILED");
                }
                let mut info = spa::param::video::VideoInfoRaw::new();
                info.parse(parameter)
                    .map_err(|_| "CAPTURE_FRAME_METADATA_FAILED")?;
                let packed = packed_format(info.format()).ok_or("CAPTURE_FRAME_METADATA_FAILED")?;
                let size = info.size();
                let mut f = parameter_format.borrow_mut();
                f.renegotiated = f.packed.is_some();
                f.packed = Some(packed);
                f.width = size.width;
                f.height = size.height;
                let header = meta_parameter(
                    spa::sys::SPA_META_Header,
                    std::mem::size_of::<spa::sys::spa_meta_header>() as i32,
                )?;
                let damage = meta_parameter(
                    spa::sys::SPA_META_VideoDamage,
                    (256 * std::mem::size_of::<spa::sys::spa_meta_region>()) as i32,
                )?;
                let mut params = [
                    Pod::from_bytes(&header).ok_or("CAPTURE_FRAME_METADATA_FAILED")?,
                    Pod::from_bytes(&damage).ok_or("CAPTURE_FRAME_METADATA_FAILED")?,
                ];
                // Metadata is optional: producers without it still supply full buffers.
                stream
                    .update_params(&mut params)
                    .map_err(|_| "CAPTURE_FRAME_METADATA_FAILED")?;
                Ok(())
            })();
            if let Err(code) = parsed {
                parameter_mailbox.finish(code);
                parameter_loop.quit();
            }
        })
        .process(move |stream, _| {
            if process_mailbox.is_finished() {
                process_loop.quit();
                return;
            }
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let (packed, width, height, renegotiated) = {
                let mut f = process_format.borrow_mut();
                let Some(packed) = f.packed else {
                    return;
                };
                let r = f.renegotiated;
                f.renegotiated = false;
                (packed, f.width, f.height, r)
            };
            let header = buffer.find_meta::<MetaHeader>();
            let sequence = header.map(MetaHeader::seq);
            let flags = header
                .map(MetaHeader::flags)
                .unwrap_or(MetaHeaderFlags::empty());
            if flags.intersects(
                MetaHeaderFlags::CORRUPTED | MetaHeaderFlags::GAP | MetaHeaderFlags::DELTA_UNIT,
            ) {
                // Cannot reconstruct a complete source from an invalid/partial buffer.
                process_mailbox.finish("CAPTURE_READBACK_FAILED");
                process_loop.quit();
                return;
            }
            let damage = damage_regions(&buffer, width, height);
            let planes = buffer.datas_mut();
            let result = (|| {
                if planes.len() != 1 {
                    return Err(DesktopFrameDataError::InvalidFrame);
                }
                let data = &mut planes[0];
                if !matches!(data.type_(), DataType::MemPtr | DataType::MemFd)
                    || data.chunk().flags().contains(ChunkFlags::CORRUPTED)
                {
                    return Err(DesktopFrameDataError::InvalidFrame);
                }
                let offset = data.chunk().offset();
                let chunk_size = data.chunk().size();
                let stride = data.chunk().stride();
                let allocation = data.data().ok_or(DesktopFrameDataError::InvalidFrame)?;
                process_mailbox.ingest(
                    MappedFrame {
                        allocation,
                        offset,
                        chunk_size,
                        stride,
                        width,
                        height,
                        format: packed,
                    },
                    sequence,
                    renegotiated || flags.contains(MetaHeaderFlags::DISCONT),
                    damage.as_deref(),
                )
            })();
            if let Err(error) = result {
                process_mailbox.finish(if error == DesktopFrameDataError::ResourceExhausted {
                    "OPERATION_RESULT_TOO_LARGE"
                } else {
                    "CAPTURE_READBACK_FAILED"
                });
                process_loop.quit();
            }
        })
        .register()
        .map_err(|_| "CAPTURE_START_FAILED")?;
    let bytes = format_parameter().map_err(|_| "CAPTURE_FRAME_METADATA_FAILED")?;
    let mut params = [Pod::from_bytes(&bytes).ok_or("CAPTURE_FRAME_METADATA_FAILED")?];
    stream
        .connect(
            spa::utils::Direction::Input,
            target,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::DONT_RECONNECT,
            &mut params,
        )
        .map_err(|_| "CAPTURE_START_FAILED")?;
    let deadline = Instant::now() + lifetime;
    let timer_mailbox = mailbox.clone();
    let timer_loop = main_loop.clone();
    let liveness = RefCell::new(liveness);
    let timer = main_loop.loop_().add_timer(move |_| {
        if let Err(code) = liveness.borrow_mut()() {
            timer_mailbox.finish(code);
        }
        if Instant::now() >= deadline {
            timer_mailbox.finish("SUBSCRIPTION_EXPIRED");
        }
        if timer_mailbox.is_finished() {
            timer_loop.quit();
        }
    });
    timer
        .update_timer(
            Some(Duration::from_millis(50)),
            Some(Duration::from_millis(50)),
        )
        .into_result()
        .map_err(|_| "CAPTURE_START_FAILED")?;
    main_loop.run();
    Ok(())
}
