//! Test-only RGBA source in a private daemon; never connects to the user's PipeWire remote.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub(super) struct Source {
    pub id: u32,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Source {
    pub(super) fn start(remote: OwnedFd) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            PIPEWIRE_INITIALIZED.call_once(pw::init);
            let main_loop = pw::main_loop::MainLoopRc::new(None).unwrap();
            let context = pw::context::ContextRc::new(&main_loop, None).unwrap();
            let core = context.connect_fd_rc(remote, None).unwrap();
            let stream=pw::stream::StreamRc::new(core.clone(),"act-generated-rgba",properties!{*pw::keys::MEDIA_TYPE=>"Video",*pw::keys::MEDIA_CATEGORY=>"Capture",*pw::keys::MEDIA_CLASS=>"Video/Source",*pw::keys::NODE_NAME=>"act-testsrc"}).unwrap();
            let sent = std::cell::Cell::new(false);
            let _listener = stream
                .add_local_listener_with_user_data(0u8)
                .state_changed(move |stream, _, _, new| {
                    if matches!(new, pw::stream::StreamState::Paused) && !sent.replace(true) {
                        let _ = tx.send(stream.node_id());
                    }
                })
                .param_changed(|stream, _, id, _| {
                    if id != spa::param::ParamType::Format.as_raw() {
                        return;
                    }
                    let object = Object {
                        type_: spa::sys::SPA_TYPE_OBJECT_ParamBuffers,
                        id: spa::sys::SPA_PARAM_Buffers,
                        properties: vec![
                            Property::new(spa::sys::SPA_PARAM_BUFFERS_buffers, Value::Int(8)),
                            Property::new(spa::sys::SPA_PARAM_BUFFERS_blocks, Value::Int(1)),
                            Property::new(
                                spa::sys::SPA_PARAM_BUFFERS_size,
                                Value::Int(64 * 48 * 4),
                            ),
                            Property::new(spa::sys::SPA_PARAM_BUFFERS_stride, Value::Int(64 * 4)),
                        ],
                    };
                    let bytes = spa::pod::serialize::PodSerializer::serialize(
                        Cursor::new(Vec::new()),
                        &Value::Object(object),
                    )
                    .unwrap()
                    .0
                    .into_inner();
                    stream
                        .update_params(&mut [Pod::from_bytes(&bytes).unwrap()])
                        .unwrap();
                })
                .process(|stream, color| {
                    let Some(mut buffer) = stream.dequeue_buffer() else {
                        return;
                    };
                    let planes = buffer.datas_mut();
                    if planes.is_empty() {
                        return;
                    }
                    let data = &mut planes[0];
                    let Some(bytes) = data.data() else {
                        return;
                    };
                    if bytes.len() < 64 * 48 * 4 {
                        return;
                    }
                    *color = color.wrapping_add(1);
                    bytes[..64 * 48 * 4].fill(*color);
                    let chunk = data.chunk_mut();
                    *chunk.offset_mut() = 0;
                    *chunk.stride_mut() = 64 * 4;
                    *chunk.size_mut() = 64 * 48 * 4;
                })
                .register()
                .unwrap();
            let object = Object {
                type_: spa::sys::SPA_TYPE_OBJECT_Format,
                id: spa::sys::SPA_PARAM_EnumFormat,
                properties: vec![
                    Property::new(
                        spa::sys::SPA_FORMAT_mediaType,
                        Value::Id(spa::utils::Id(spa::sys::SPA_MEDIA_TYPE_video)),
                    ),
                    Property::new(
                        spa::sys::SPA_FORMAT_mediaSubtype,
                        Value::Id(spa::utils::Id(spa::sys::SPA_MEDIA_SUBTYPE_raw)),
                    ),
                    Property::new(
                        spa::sys::SPA_FORMAT_VIDEO_format,
                        Value::Id(spa::utils::Id(spa::sys::SPA_VIDEO_FORMAT_RGBA)),
                    ),
                    Property::new(
                        spa::sys::SPA_FORMAT_VIDEO_size,
                        Value::Rectangle(spa::utils::Rectangle {
                            width: 64,
                            height: 48,
                        }),
                    ),
                    Property::new(
                        spa::sys::SPA_FORMAT_VIDEO_framerate,
                        Value::Fraction(spa::utils::Fraction { num: 25, denom: 1 }),
                    ),
                ],
            };
            let bytes = spa::pod::serialize::PodSerializer::serialize(
                Cursor::new(Vec::new()),
                &Value::Object(object),
            )
            .unwrap()
            .0
            .into_inner();
            stream
                .connect(
                    spa::utils::Direction::Output,
                    None,
                    pw::stream::StreamFlags::DRIVER | pw::stream::StreamFlags::MAP_BUFFERS,
                    &mut [Pod::from_bytes(&bytes).unwrap()],
                )
                .unwrap();
            let timer_loop = main_loop.clone();
            let trigger = stream.clone();
            let timer = main_loop.loop_().add_timer(move |_| {
                if cancelled.load(Ordering::Acquire) {
                    timer_loop.quit();
                } else {
                    let _ = trigger.trigger_process();
                }
            });
            timer
                .update_timer(
                    Some(Duration::from_millis(40)),
                    Some(Duration::from_millis(40)),
                )
                .into_result()
                .unwrap();
            main_loop.run();
        });
        let mut source = Self {
            id: 0,
            stop,
            worker: Some(worker),
        };
        source.id = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("fixture source must register");
        source
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
