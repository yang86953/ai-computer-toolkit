//! 通过 Portal 授权 PipeWire remote 消费一个有界 raw frame。

use std::{cell::RefCell, io::Cursor, os::fd::OwnedFd, rc::Rc, sync::Once, time::Duration};

use pipewire::{self as pw, properties::properties, spa};
use spa::{
    buffer::{ChunkFlags, DataType},
    pod::Pod,
};

use crate::components::desktop_session_frame_capture::{
    DesktopCapturedFrame, DesktopFrameDataError, DesktopPackedPixelFormat, encode_mapped_frame,
};

pub(super) static PIPEWIRE_INITIALIZED: Once = Once::new();

/// ScreenCast 原生目标只在 Linux Adapter 内部存活。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PipeWireStreamTarget {
    NodeId(u32),
    Serial(u64),
}

/// 不泄漏 PipeWire 状态文本的稳定失败分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PipeWireCaptureFailure {
    code: &'static str,
    stage: &'static str,
    pixels_may_have_been_consumed: bool,
}

impl PipeWireCaptureFailure {
    const fn new(
        code: &'static str,
        stage: &'static str,
        pixels_may_have_been_consumed: bool,
    ) -> Self {
        Self {
            code,
            stage,
            pixels_may_have_been_consumed,
        }
    }

    pub(super) const fn code(self) -> &'static str {
        self.code
    }

    pub(super) const fn stage(self) -> &'static str {
        self.stage
    }

    pub(super) const fn pixels_may_have_been_consumed(self) -> bool {
        self.pixels_may_have_been_consumed
    }
}

#[derive(Default)]
struct CaptureState {
    format: Option<DesktopPackedPixelFormat>,
    width: u32,
    height: u32,
    result: Option<Result<DesktopCapturedFrame, PipeWireCaptureFailure>>,
}

/// 使用一次授权 remote 连接一条精确 stream，首帧到达后立即退出。
pub(super) fn capture_frame(
    remote: OwnedFd,
    target: PipeWireStreamTarget,
    timeout: Duration,
    max_dimension: Option<u32>,
) -> Result<DesktopCapturedFrame, PipeWireCaptureFailure> {
    PIPEWIRE_INITIALIZED.call_once(pw::init);
    let main_loop = pw::main_loop::MainLoopRc::new(None)
        .map_err(|_| failure("CAPTURE_UNAVAILABLE", "pipewire-main-loop", false))?;
    let context = pw::context::ContextRc::new(&main_loop, None)
        .map_err(|_| failure("CAPTURE_UNAVAILABLE", "pipewire-context", false))?;
    let core = context
        .connect_fd_rc(remote, None)
        .map_err(|_| failure("CAPTURE_UNAVAILABLE", "pipewire-connect-fd", false))?;
    let mut stream_properties = properties! {
        *pw::keys::MEDIA_TYPE => "Video",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Screen",
    };
    let connect_target = match target {
        PipeWireStreamTarget::NodeId(node_id) => Some(node_id),
        PipeWireStreamTarget::Serial(serial) => {
            stream_properties.insert(*pw::keys::TARGET_OBJECT, serial.to_string());
            None
        }
    };
    let stream = pw::stream::StreamBox::new(
        &core,
        "ai-computer-toolkit-screen-capture",
        stream_properties,
    )
    .map_err(|_| failure("CAPTURE_START_FAILED", "pipewire-create-stream", false))?;
    let state = Rc::new(RefCell::new(CaptureState::default()));
    let parameter_state = Rc::clone(&state);
    let parameter_loop = main_loop.clone();
    let process_state = Rc::clone(&state);
    let process_loop = main_loop.clone();
    let error_state = Rc::clone(&state);
    let error_loop = main_loop.clone();
    let _listener = stream
        .add_local_listener::<()>()
        .state_changed(move |_, _, _, new| {
            if let pw::stream::StreamState::Error(_) = new {
                set_failure_once(
                    &error_state,
                    failure("CAPTURE_START_FAILED", "pipewire-stream-state", false),
                );
                error_loop.quit();
            }
        })
        .param_changed(move |_, _, id, parameter| {
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Some(parameter) = parameter else {
                set_failure_once(
                    &parameter_state,
                    failure("CAPTURE_FRAME_METADATA_FAILED", "pipewire-format", false),
                );
                parameter_loop.quit();
                return;
            };
            let parsed = spa::param::format_utils::parse_format(parameter);
            let Ok((media_type, media_subtype)) = parsed else {
                set_failure_once(
                    &parameter_state,
                    failure("CAPTURE_FRAME_METADATA_FAILED", "pipewire-format", false),
                );
                parameter_loop.quit();
                return;
            };
            if media_type != spa::param::format::MediaType::Video
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                set_failure_once(
                    &parameter_state,
                    failure("CAPTURE_FRAME_METADATA_FAILED", "pipewire-format", false),
                );
                parameter_loop.quit();
                return;
            }
            let mut info = spa::param::video::VideoInfoRaw::new();
            if info.parse(parameter).is_err() {
                set_failure_once(
                    &parameter_state,
                    failure("CAPTURE_FRAME_METADATA_FAILED", "pipewire-format", false),
                );
                parameter_loop.quit();
                return;
            }
            let Some(format) = packed_format(info.format()) else {
                set_failure_once(
                    &parameter_state,
                    failure("CAPTURE_FRAME_METADATA_FAILED", "pipewire-format", false),
                );
                parameter_loop.quit();
                return;
            };
            let size = info.size();
            let mut state = parameter_state.borrow_mut();
            state.format = Some(format);
            state.width = size.width;
            state.height = size.height;
        })
        .process(move |stream, _| {
            if process_state.borrow().result.is_some() {
                return;
            }
            let (format, width, height) = {
                let state = process_state.borrow();
                let Some(format) = state.format else {
                    return;
                };
                (format, state.width, state.height)
            };
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let data_planes = buffer.datas_mut();
            if data_planes.len() != 1 {
                set_failure_once(
                    &process_state,
                    failure("CAPTURE_READBACK_FAILED", "pipewire-frame", true),
                );
                process_loop.quit();
                return;
            }
            let data = &mut data_planes[0];
            if !matches!(data.type_(), DataType::MemPtr | DataType::MemFd)
                || data.chunk().flags().contains(ChunkFlags::CORRUPTED)
            {
                set_failure_once(
                    &process_state,
                    failure("CAPTURE_READBACK_FAILED", "pipewire-frame", true),
                );
                process_loop.quit();
                return;
            }
            let offset = data.chunk().offset();
            let chunk_size = data.chunk().size();
            let stride = data.chunk().stride();
            let Some(allocation) = data.data() else {
                set_failure_once(
                    &process_state,
                    failure("CAPTURE_READBACK_FAILED", "pipewire-frame", true),
                );
                process_loop.quit();
                return;
            };
            let result = encode_mapped_frame(
                allocation, offset, chunk_size, stride, (width, height), format, max_dimension,
            )
            .map_err(data_error);
            process_state.borrow_mut().result = Some(result);
            process_loop.quit();
        })
        .register()
        .map_err(|_| failure("CAPTURE_START_FAILED", "pipewire-listener", false))?;

    let parameter_bytes = format_parameter()?;
    let parameter = Pod::from_bytes(&parameter_bytes)
        .ok_or_else(|| failure("CAPTURE_FRAME_METADATA_FAILED", "pipewire-format", false))?;
    let mut parameters = [parameter];
    stream
        .connect(
            spa::utils::Direction::Input,
            connect_target,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::DONT_RECONNECT,
            &mut parameters,
        )
        .map_err(|_| failure("CAPTURE_START_FAILED", "pipewire-connect-stream", false))?;
    let timeout_loop = main_loop.clone();
    let timer = main_loop.loop_().add_timer(move |_| timeout_loop.quit());
    timer
        .update_timer(Some(timeout), None)
        .into_result()
        .map_err(|_| failure("CAPTURE_START_FAILED", "pipewire-timeout-arm", false))?;
    main_loop.run();
    let result = state.borrow_mut().result.take();
    result.unwrap_or_else(|| Err(failure("TIMEOUT", "pipewire-first-frame", false)))
}

pub(super) fn format_parameter() -> Result<Vec<u8>, PipeWireCaptureFailure> {
    let object = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::xBGR,
            spa::param::video::VideoFormat::xRGB,
        )
    );
    spa::pod::serialize::PodSerializer::serialize(
        Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .map_err(|_| failure("CAPTURE_FRAME_METADATA_FAILED", "pipewire-format", false))
}

pub(super) fn packed_format(format: spa::param::video::VideoFormat) -> Option<DesktopPackedPixelFormat> {
    match format {
        spa::param::video::VideoFormat::RGBA => Some(DesktopPackedPixelFormat::Rgba),
        spa::param::video::VideoFormat::BGRA => Some(DesktopPackedPixelFormat::Bgra),
        spa::param::video::VideoFormat::RGBx => Some(DesktopPackedPixelFormat::Rgbx),
        spa::param::video::VideoFormat::BGRx => Some(DesktopPackedPixelFormat::Bgrx),
        spa::param::video::VideoFormat::xRGB => Some(DesktopPackedPixelFormat::Xrgb),
        spa::param::video::VideoFormat::xBGR => Some(DesktopPackedPixelFormat::Xbgr),
        _ => None,
    }
}

fn data_error(error: DesktopFrameDataError) -> PipeWireCaptureFailure {
    match error {
        DesktopFrameDataError::ResourceExhausted => {
            failure("OPERATION_RESULT_TOO_LARGE", "pipewire-frame", true)
        }
        DesktopFrameDataError::InvalidFrame | DesktopFrameDataError::EncodingFailed => {
            failure("CAPTURE_READBACK_FAILED", "pipewire-frame", true)
        }
    }
}

fn set_failure_once(state: &RefCell<CaptureState>, failure: PipeWireCaptureFailure) {
    let mut state = state.borrow_mut();
    if state.result.is_none() {
        state.result = Some(Err(failure));
    }
}

const fn failure(
    code: &'static str,
    stage: &'static str,
    pixels_may_have_been_consumed: bool,
) -> PipeWireCaptureFailure {
    PipeWireCaptureFailure::new(code, stage, pixels_may_have_been_consumed)
}
