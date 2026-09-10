//! CLI 与 MCP 共用的桌面授权、输入、观察和收尾生命周期。

use std::{collections::HashMap, path::PathBuf, time::Duration};

use serde_json::{Value, json};

use crate::{
    components::{
        atomic_file::{AtomicFileError, StagedFile},
        desktop_session_frame_capture::{self, DesktopCapturedFrame},
        desktop_session_identity,
        desktop_session_input_cancellation::DesktopInputCancellation,
        desktop_session_keyboard_input, desktop_session_pointer_input,
        desktop_session_pointer_input::DesktopPointerInput,
        keyboard_input_contract::KeyboardInput,
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
        output_guard::{OutputGuardError, guard_file_output},
    },
    domain::{AppControlError, AppResult, IsolationRequirement},
};

const MINIMUM_OPEN_TIMEOUT: Duration = Duration::from_secs(10);
const MAXIMUM_OPEN_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const MAXIMUM_LIVE_SESSIONS: usize = 8;

#[path = "desktop_interaction.rs"]
mod interaction;
#[path = "desktop_subscription.rs"]
mod subscription;

/// 保存 Portal/EIS 建立后允许跨层公开的 provider-neutral 事实。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSessionFacts {
    backend: &'static str,
    remote_desktop_version: u32,
    screen_cast_version: u32,
    authorized_device_classes: Vec<&'static str>,
    stream_count: usize,
    mapping_id_count: usize,
}

impl DesktopSessionFacts {
    /// 由 Adapter 从原生结果投影最小事实，不保存路径、FD、node 或 mapping 身份。
    pub(crate) fn new(
        remote_desktop_version: u32,
        screen_cast_version: u32,
        authorized_device_classes: Vec<&'static str>,
        stream_count: usize,
        mapping_id_count: usize,
    ) -> Self {
        Self {
            backend: "portal-eis",
            remote_desktop_version,
            screen_cast_version,
            authorized_device_classes,
            stream_count,
            mapping_id_count,
        }
    }

    pub(crate) const fn remote_desktop_version(&self) -> u32 {
        self.remote_desktop_version
    }

    pub(crate) const fn screen_cast_version(&self) -> u32 {
        self.screen_cast_version
    }

    pub(crate) fn authorized_device_classes(&self) -> &[&'static str] {
        &self.authorized_device_classes
    }

    pub(crate) const fn stream_count(&self) -> usize {
        self.stream_count
    }

    pub(crate) const fn mapping_id_count(&self) -> usize {
        self.mapping_id_count
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn windows(stream_count: usize) -> Self {
        Self {
            backend: "windows-wgc",
            remote_desktop_version: 0,
            screen_cast_version: 0,
            authorized_device_classes: vec!["keyboard", "pointer"],
            stream_count,
            mapping_id_count: 1,
        }
    }

    pub(crate) fn backend(&self) -> &'static str {
        self.backend
    }

    fn is_valid_l1_projection(&self) -> bool {
        let provider_valid = match self.backend {
            "portal-eis" => self.remote_desktop_version >= 2 && self.screen_cast_version >= 5,
            "windows-wgc" => self.remote_desktop_version == 0 && self.screen_cast_version == 0,
            _ => false,
        };
        provider_valid
            && self.authorized_device_classes == ["keyboard", "pointer"]
            && self.stream_count > 0
            && (1..=self.stream_count).contains(&self.mapping_id_count)
    }
}

/// 保存 Adapter 不含原生字符串的稳定失败分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSessionPortFailure {
    code: &'static str,
    stage: &'static str,
    accepted_may_have_occurred: bool,
    cleanup_confirmed: bool,
}

impl DesktopSessionPortFailure {
    /// 创建尚未产生 Portal session 的确定失败。
    pub(crate) const fn before_session(code: &'static str, stage: &'static str) -> Self {
        Self {
            code,
            stage,
            accepted_may_have_occurred: false,
            cleanup_confirmed: false,
        }
    }

    /// 创建 session 已可能产生、并携带清理确认状态的失败。
    pub(crate) const fn after_session(
        code: &'static str,
        stage: &'static str,
        cleanup_confirmed: bool,
    ) -> Self {
        Self {
            code,
            stage,
            accepted_may_have_occurred: true,
            cleanup_confirmed,
        }
    }

    pub(crate) const fn code(self) -> &'static str {
        self.code
    }

    pub(crate) const fn stage(self) -> &'static str {
        self.stage
    }

    pub(crate) const fn accepted_may_have_occurred(self) -> bool {
        self.accepted_may_have_occurred
    }

    pub(crate) const fn cleanup_confirmed(self) -> bool {
        self.cleanup_confirmed
    }
}

/// Adapter 返回的 lease 由单线程 broker 内的 Module 唯一持有并负责显式关闭。
pub(crate) trait DesktopSessionLease {
    fn frame_mapping(
        &mut self,
    ) -> Result<
        Option<crate::components::desktop_interaction::FrameMapping>,
        DesktopSessionInputFailure,
    > {
        Ok(None)
    }
    fn send_frame_point(
        &mut self,
        _: &crate::components::desktop_interaction::FramePoint,
        _: u32,
        _: &DesktopInputCancellation,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
        Err(DesktopSessionInputFailure::before_dispatch(
            "INPUT_MAPPING_UNAVAILABLE",
            "frame-point",
        ))
    }
    fn send_keyboard(
        &mut self,
        input: &KeyboardInput,
        cancellation: &DesktopInputCancellation,
    ) -> Result<DesktopKeyboardDispatchFacts, DesktopSessionInputFailure>;

    fn send_pointer(
        &mut self,
        input: &DesktopPointerInput,
        cancellation: &DesktopInputCancellation,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure>;

    #[allow(unused_variables)]
    fn capture_frame(
        &mut self,
        timeout: Duration,
        max_dimension: Option<u32>,
    ) -> Result<DesktopCapturedFrame, DesktopSessionFrameFailure> {
        Err(DesktopSessionFrameFailure::new(
            "CAPABILITY_UNAVAILABLE",
            "capture-frame",
            false,
            false,
        ))
    }

    fn subscription_stats(&self) -> (u64, u64) {
        (0, 0)
    }
    fn subscribe_frames(
        &mut self,
        _id: &str,
        _lifetime: Duration,
    ) -> Result<(), DesktopSessionFrameFailure> {
        Err(DesktopSessionFrameFailure::new(
            "CAPABILITY_UNAVAILABLE",
            "subscribe-frames",
            false,
            false,
        ))
    }
    fn next_frame_update(
        &mut self,
        _id: &str,
        _after: u64,
        _wait: Duration,
    ) -> Result<
        Option<crate::components::desktop_frame_stream::FrameUpdate>,
        DesktopSessionFrameFailure,
    > {
        Err(DesktopSessionFrameFailure::new(
            "STALE_SUBSCRIPTION",
            "next-frame",
            false,
            false,
        ))
    }
    fn unsubscribe_frames(&mut self, _id: &str) -> Result<(), DesktopSessionFrameFailure> {
        Err(DesktopSessionFrameFailure::new(
            "STALE_SUBSCRIPTION",
            "unsubscribe-frames",
            false,
            false,
        ))
    }
    fn close(self: Box<Self>) -> Result<(), DesktopSessionPortFailure>;
}

/// 保存单帧 Adapter 的稳定失败事实，不公开 PipeWire 或 Portal 原生身份。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSessionFrameFailure {
    code: &'static str,
    stage: &'static str,
    pixels_may_have_been_consumed: bool,
    invalidates_session: bool,
}

impl DesktopSessionFrameFailure {
    pub(crate) const fn new(
        code: &'static str,
        stage: &'static str,
        pixels_may_have_been_consumed: bool,
        invalidates_session: bool,
    ) -> Self {
        Self {
            code,
            stage,
            pixels_may_have_been_consumed,
            invalidates_session,
        }
    }

    pub(crate) const fn code(self) -> &'static str {
        self.code
    }

    pub(crate) const fn stage(self) -> &'static str {
        self.stage
    }

    pub(crate) const fn pixels_may_have_been_consumed(self) -> bool {
        self.pixels_may_have_been_consumed
    }

    pub(crate) const fn invalidates_session(self) -> bool {
        self.invalidates_session
    }
}

/// 保存会话级输入 Adapter 的稳定失败事实，不暴露 EIS 原生对象或消息。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSessionInputFailure {
    code: &'static str,
    stage: &'static str,
    accepted_may_have_occurred: bool,
    releases_confirmed: bool,
    completed_steps: usize,
    input_events_sent: usize,
}

impl DesktopSessionInputFailure {
    pub(crate) const fn before_dispatch(code: &'static str, stage: &'static str) -> Self {
        Self {
            code,
            stage,
            accepted_may_have_occurred: false,
            releases_confirmed: true,
            completed_steps: 0,
            input_events_sent: 0,
        }
    }

    pub(crate) const fn after_dispatch(
        code: &'static str,
        stage: &'static str,
        releases_confirmed: bool,
        completed_steps: usize,
        input_events_sent: usize,
    ) -> Self {
        Self {
            code,
            stage,
            accepted_may_have_occurred: true,
            releases_confirmed,
            completed_steps,
            input_events_sent,
        }
    }

    /// 创建协作取消终态；仅在已经 flush 输入事件时声明可能被应用接受。
    pub(crate) const fn cancelled(
        releases_confirmed: bool,
        completed_steps: usize,
        input_events_sent: usize,
    ) -> Self {
        Self {
            code: "CANCELLED",
            stage: "input-cancel",
            accepted_may_have_occurred: input_events_sent > 0,
            releases_confirmed,
            completed_steps,
            input_events_sent,
        }
    }

    pub(crate) const fn code(self) -> &'static str {
        self.code
    }

    pub(crate) const fn stage(self) -> &'static str {
        self.stage
    }

    pub(crate) const fn accepted_may_have_occurred(self) -> bool {
        self.accepted_may_have_occurred
    }

    pub(crate) const fn releases_confirmed(self) -> bool {
        self.releases_confirmed
    }

    pub(crate) const fn completed_steps(self) -> usize {
        self.completed_steps
    }

    pub(crate) const fn input_events_sent(self) -> usize {
        self.input_events_sent
    }
}

/// 保存 EIS 已接受到本地发送缓冲区的有界调度事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopKeyboardDispatchFacts {
    completed_steps: usize,
    input_events_sent: usize,
}

impl DesktopKeyboardDispatchFacts {
    pub(crate) const fn new(completed_steps: usize, input_events_sent: usize) -> Self {
        Self {
            completed_steps,
            input_events_sent,
        }
    }

    pub(crate) const fn completed_steps(self) -> usize {
        self.completed_steps
    }

    pub(crate) const fn input_events_sent(self) -> usize {
        self.input_events_sent
    }
}

/// 保存 EIS 已接受到本地发送缓冲区的有界相对指针调度事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopPointerDispatchFacts {
    completed_steps: usize,
    input_events_sent: usize,
}

impl DesktopPointerDispatchFacts {
    pub(crate) const fn new(completed_steps: usize, input_events_sent: usize) -> Self {
        Self {
            completed_steps,
            input_events_sent,
        }
    }

    pub(crate) const fn completed_steps(self) -> usize {
        self.completed_steps
    }

    pub(crate) const fn input_events_sent(self) -> usize {
        self.input_events_sent
    }
}

/// Module 面向平台 Adapter 的窄端口，不暴露 D-Bus、EIS 或 FD 类型。
pub(crate) trait DesktopSessionPort {
    fn open(
        &self,
        timeout: Duration,
    ) -> Result<(Box<dyn DesktopSessionLease>, DesktopSessionFacts), DesktopSessionPortFailure>;
}

/// 保存可返回给 System 的 live 会话只读投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSessionView {
    session_id: String,
    facts: DesktopSessionFacts,
    input_events_sent: usize,
    frames_captured: usize,
    pixels_consumed: u64,
}

impl DesktopSessionView {
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) const fn facts(&self) -> &DesktopSessionFacts {
        &self.facts
    }

    pub(crate) const fn input_events_sent(&self) -> usize {
        self.input_events_sent
    }

    pub(crate) const fn frames_captured(&self) -> usize {
        self.frames_captured
    }

    pub(crate) const fn pixels_consumed(&self) -> u64 {
        self.pixels_consumed
    }
}

/// 保存一次会话级键盘调度的公开 provider-neutral 结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopKeyboardReport {
    session_id: String,
    completed_steps: usize,
    input_events_sent: usize,
}

impl DesktopKeyboardReport {
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) const fn completed_steps(&self) -> usize {
        self.completed_steps
    }

    pub(crate) const fn input_events_sent(&self) -> usize {
        self.input_events_sent
    }

    pub(crate) const fn effect_confirmed(&self) -> bool {
        false
    }
}

/// 保存一次会话级相对指针调度的公开 provider-neutral 结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopPointerReport {
    session_id: String,
    coordinate_space: &'static str,
    completed_steps: usize,
    input_events_sent: usize,
}

/// 保存一次同 lease 单帧 PNG 提交的中立事实。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopFrameCaptureReport {
    rgba: Vec<u8>,
    session_id: String,
    path: String,
    bytes: usize,
    width: u32,
    height: u32,
    pixel_digest: String,
    source_size: (u32, u32),
    replaced_existing: bool,
}

impl DesktopFrameCaptureReport {
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) const fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) const fn width(&self) -> u32 {
        self.width
    }

    pub(crate) const fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn pixel_digest(&self) -> &str {
        &self.pixel_digest
    }

    pub(crate) const fn source_size(&self) -> (u32, u32) {
        self.source_size
    }

    pub(crate) const fn replaced_existing(&self) -> bool {
        self.replaced_existing
    }
}

impl DesktopPointerReport {
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) const fn coordinate_space(&self) -> &'static str {
        self.coordinate_space
    }

    pub(crate) const fn completed_steps(&self) -> usize {
        self.completed_steps
    }

    pub(crate) const fn input_events_sent(&self) -> usize {
        self.input_events_sent
    }

    pub(crate) const fn effect_confirmed(&self) -> bool {
        false
    }

    pub(crate) const fn final_position_confirmed(&self) -> bool {
        false
    }
}

/// 保存显式关闭的确定完成事实。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSessionCloseReport {
    session_id: String,
}

impl DesktopSessionCloseReport {
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) const fn completed(&self) -> bool {
        true
    }
}

fn subscription_view(entry: &DesktopSessionEntry) -> DesktopSessionView {
    let mut view = entry.view.clone();
    let (frames, pixels) = entry.lease.subscription_stats();
    view.frames_captured = view
        .frames_captured
        .saturating_add(usize::try_from(frames).unwrap_or(usize::MAX));
    view.pixels_consumed = view.pixels_consumed.saturating_add(pixels);
    view
}

struct DesktopSessionEntry {
    view: DesktopSessionView,
    lease: Box<dyn DesktopSessionLease>,
    observation: Option<interaction::Observation>,
}

/// 拥有同一进程代际内全部 live Portal 桌面会话。
pub(crate) struct DesktopSessionModule<P: DesktopSessionPort> {
    port: P,
    sessions: HashMap<String, DesktopSessionEntry>,
}

impl<P: DesktopSessionPort> DesktopSessionModule<P> {
    pub(crate) fn new(port: P) -> Self {
        Self {
            port,
            sessions: HashMap::new(),
        }
    }

    /// 建立并接管一条持续存活的 Portal/EIS/PipeWire lease。
    pub(crate) fn open(
        &mut self,
        confirmed: bool,
        foreground_consent: bool,
        isolation_requirement: IsolationRequirement,
        timeout: Duration,
    ) -> AppResult<DesktopSessionView> {
        validate_open_permissions(confirmed, foreground_consent, isolation_requirement)?;
        validate_open_timeout(timeout)?;
        if self.sessions.len() >= MAXIMUM_LIVE_SESSIONS {
            return Err(AppControlError::with_details(
                "RESOURCE_EXHAUSTED",
                "The desktop session limit has been reached.",
                json!({
                    "maximumLiveSessions": MAXIMUM_LIVE_SESSIONS,
                    "portalRequestIssued": false,
                    "retrySafe": false,
                }),
            ));
        }
        // 在发出可见 Portal 请求前生成公开身份，避免成功后随机源失败。
        let session_id = desktop_session_identity::new_session_target()?;
        let (lease, facts) = self.port.open(timeout).map_err(port_error)?;
        if !facts.is_valid_l1_projection() {
            return match lease.close() {
                Ok(()) => Err(AppControlError::new(
                    "INTERNAL_PROTOCOL_ERROR",
                    "The desktop session provider returned an invalid L1 projection.",
                )),
                Err(failure) => Err(port_error(failure)),
            };
        }
        if self.sessions.contains_key(&session_id) {
            return match lease.close() {
                Ok(()) => Err(AppControlError::new(
                    "INTERNAL_PROTOCOL_ERROR",
                    "The desktop session identity was duplicated.",
                )),
                Err(failure) => Err(port_error(failure)),
            };
        }
        let view = DesktopSessionView {
            session_id: session_id.clone(),
            facts,
            input_events_sent: 0,
            frames_captured: 0,
            pixels_consumed: 0,
        };
        self.sessions.insert(
            session_id,
            DesktopSessionEntry {
                view: view.clone(),
                lease,
                observation: None,
            },
        );
        Ok(view)
    }

    /// 列出本 Module 代际仍由工具持有的 live 会话。
    pub(crate) fn sessions(&self) -> Vec<DesktopSessionView> {
        let mut sessions = self
            .sessions
            .values()
            .map(subscription_view)
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        sessions
    }

    /// 只读检查当前精确 opaque 会话；不存在即 stale。
    pub(crate) fn inspect(&self, session_id: &str) -> AppResult<DesktopSessionView> {
        validate_session_target(session_id)?;
        self.sessions
            .get(session_id)
            .map(subscription_view)
            .ok_or_else(stale_session_error)
    }

    /// 在当前 live lease 上执行有界、请求内配平的键盘序列。
    pub(crate) fn send_keyboard(
        &mut self,
        session_id: &str,
        confirmed: bool,
        foreground_consent: bool,
        isolation_requirement: IsolationRequirement,
        value: &Value,
        cancellation: &DesktopInputCancellation,
    ) -> AppResult<DesktopKeyboardReport> {
        validate_input_permissions(confirmed, foreground_consent, isolation_requirement)?;
        let input = desktop_session_keyboard_input::parse(value)?;
        validate_session_target(session_id)?;
        let dispatch = {
            let entry = self
                .sessions
                .get_mut(session_id)
                .ok_or_else(stale_session_error)?;
            entry.lease.send_keyboard(&input, cancellation)
        };
        match dispatch {
            Ok(facts) => {
                let entry = self
                    .sessions
                    .get_mut(session_id)
                    .ok_or_else(stale_session_error)?;
                entry.view.input_events_sent = entry
                    .view
                    .input_events_sent
                    .saturating_add(facts.input_events_sent());
                Ok(DesktopKeyboardReport {
                    session_id: session_id.to_owned(),
                    completed_steps: facts.completed_steps(),
                    input_events_sent: facts.input_events_sent(),
                })
            }
            Err(failure) => {
                let entry = self
                    .sessions
                    .remove(session_id)
                    .ok_or_else(stale_session_error)?;
                let cleanup_confirmed = entry.lease.close().is_ok();
                Err(input_port_error(failure, cleanup_confirmed))
            }
        }
    }

    /// 在当前 live lease 上执行有界、请求内配平的相对指针序列。
    pub(crate) fn send_pointer(
        &mut self,
        session_id: &str,
        confirmed: bool,
        foreground_consent: bool,
        isolation_requirement: IsolationRequirement,
        value: &Value,
        cancellation: &DesktopInputCancellation,
    ) -> AppResult<DesktopPointerReport> {
        validate_input_permissions(confirmed, foreground_consent, isolation_requirement)?;
        let input = desktop_session_pointer_input::parse(value)?;
        validate_session_target(session_id)?;
        let dispatch = {
            let entry = self
                .sessions
                .get_mut(session_id)
                .ok_or_else(stale_session_error)?;
            entry.lease.send_pointer(&input, cancellation)
        };
        match dispatch {
            Ok(facts) => {
                let entry = self
                    .sessions
                    .get_mut(session_id)
                    .ok_or_else(stale_session_error)?;
                entry.view.input_events_sent = entry
                    .view
                    .input_events_sent
                    .saturating_add(facts.input_events_sent());
                Ok(DesktopPointerReport {
                    session_id: session_id.to_owned(),
                    coordinate_space: input.coordinate_space.as_str(),
                    completed_steps: facts.completed_steps(),
                    input_events_sent: facts.input_events_sent(),
                })
            }
            Err(failure) => {
                let entry = self
                    .sessions
                    .remove(session_id)
                    .ok_or_else(stale_session_error)?;
                let cleanup_confirmed = entry.lease.close().is_ok();
                Err(input_port_error(failure, cleanup_confirmed))
            }
        }
    }

    /// 在既有授权 stream 上消费一帧，并以同目录 staging 原子提交 PNG。
    pub(crate) fn capture_frame(
        &mut self,
        session_id: &str,
        confirmed: bool,
        isolation_requirement: IsolationRequirement,
        value: &Value,
    ) -> AppResult<DesktopFrameCaptureReport> {
        validate_capture_permissions(confirmed, isolation_requirement)?;
        let input = desktop_session_frame_capture::parse_input(value)?;
        validate_session_target(session_id)?;
        if !self.sessions.contains_key(session_id) {
            return Err(stale_session_error());
        }
        let destination = PathBuf::from(input.path());
        guard_file_output(&destination, input.overwrite()).map_err(capture_output_guard_error)?;
        let mut staged = StagedFile::reserve(&destination).map_err(capture_atomic_file_error)?;
        let capture = {
            let entry = self
                .sessions
                .get_mut(session_id)
                .ok_or_else(stale_session_error)?;
            entry
                .lease
                .capture_frame(input.timeout(), input.max_dimension())
        };
        let frame = match capture {
            Ok(frame) => frame,
            Err(failure) if failure.invalidates_session() => {
                let entry = self
                    .sessions
                    .remove(session_id)
                    .ok_or_else(stale_session_error)?;
                let cleanup_confirmed = entry.lease.close().is_ok();
                return Err(frame_port_error(failure, Some(cleanup_confirmed)));
            }
            Err(failure) => return Err(frame_port_error(failure, None)),
        };
        let entry = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(stale_session_error)?;
        entry.view.frames_captured = entry.view.frames_captured.saturating_add(1);
        entry.view.pixels_consumed = entry.view.pixels_consumed.saturating_add(frame.pixels());
        staged
            .write_all(frame.png())
            .map_err(capture_atomic_file_error)?;
        let commit = staged
            .commit(input.overwrite())
            .map_err(capture_atomic_file_error)?;
        Ok(DesktopFrameCaptureReport {
            session_id: session_id.to_owned(),
            path: input.path().to_owned(),
            bytes: frame.png().len(),
            width: frame.width(),
            height: frame.height(),
            pixel_digest: frame.pixel_digest().to_owned(),
            source_size: frame.source_size(),
            replaced_existing: commit.replaced_existing,
            rgba: frame.into_rgba(),
        })
    }

    /// 先使公开身份 stale，再消费唯一 lease 并确认 Portal Close。
    pub(crate) fn close(&mut self, session_id: &str) -> AppResult<DesktopSessionCloseReport> {
        validate_session_target(session_id)?;
        let entry = self
            .sessions
            .remove(session_id)
            .ok_or_else(stale_session_error)?;
        entry.lease.close().map_err(port_error)?;
        Ok(DesktopSessionCloseReport {
            session_id: session_id.to_owned(),
        })
    }
}

impl<P: DesktopSessionPort> Drop for DesktopSessionModule<P> {
    fn drop(&mut self) {
        // 组合根退出时逆序消费所有 lease；失败也不得恢复已经 stale 的公开身份。
        for (_, entry) in self.sessions.drain() {
            let _ = entry.lease.close();
        }
    }
}

fn validate_open_timeout(timeout: Duration) -> AppResult<()> {
    if !(MINIMUM_OPEN_TIMEOUT..=MAXIMUM_OPEN_TIMEOUT).contains(&timeout) {
        return Err(AppControlError::with_details(
            "INVALID_ARGUMENT",
            "Desktop session timeout must be 10000..300000ms.",
            json!({
                "portalRequestIssued": false,
                "minimumTimeoutMs": MINIMUM_OPEN_TIMEOUT.as_millis(),
                "maximumTimeoutMs": MAXIMUM_OPEN_TIMEOUT.as_millis(),
            }),
        ));
    }
    Ok(())
}

fn validate_open_permissions(
    confirmed: bool,
    foreground_consent: bool,
    isolation_requirement: IsolationRequirement,
) -> AppResult<()> {
    if !confirmed {
        return Err(AppControlError::with_details(
            "CONFIRMATION_REQUIRED",
            "Desktop session creation requires explicit confirmation.",
            json!({
                "executionRealm": "host-foreground",
                "portalRequestIssued": false,
                "fallback": "none",
            }),
        ));
    }
    if isolation_requirement == IsolationRequirement::Strict {
        return Err(AppControlError::with_details(
            "ISOLATION_REQUIRED",
            "Interactive Portal authorization cannot satisfy strict isolation.",
            json!({
                "executionRealm": "host-foreground",
                "visibleEffect": "system-portal-selection-dialog",
                "portalRequestIssued": false,
                "fallback": "none",
            }),
        ));
    }
    if !foreground_consent {
        return Err(AppControlError::with_details(
            "FOREGROUND_CONSENT_REQUIRED",
            "Desktop session creation requires consent for a visible Portal dialog.",
            json!({
                "executionRealm": "host-foreground",
                "visibleEffect": "system-portal-selection-dialog",
                "portalRequestIssued": false,
                "fallback": "none",
            }),
        ));
    }
    Ok(())
}

fn validate_input_permissions(
    confirmed: bool,
    foreground_consent: bool,
    isolation_requirement: IsolationRequirement,
) -> AppResult<()> {
    if !confirmed {
        return Err(AppControlError::with_details(
            "CONFIRMATION_REQUIRED",
            "Desktop-session input requires explicit confirmation.",
            json!({"inputAttempted": false, "fallback": "none"}),
        ));
    }
    if isolation_requirement == IsolationRequirement::Strict {
        return Err(AppControlError::with_details(
            "ISOLATION_REQUIRED",
            "Desktop-session input cannot satisfy strict isolation.",
            json!({"inputAttempted": false, "executionRealm": "host-foreground", "fallback": "none"}),
        ));
    }
    if !foreground_consent {
        return Err(AppControlError::with_details(
            "FOREGROUND_CONSENT_REQUIRED",
            "Desktop-session input requires host-foreground consent.",
            json!({"inputAttempted": false, "executionRealm": "host-foreground", "fallback": "none"}),
        ));
    }
    Ok(())
}

fn validate_capture_permissions(
    confirmed: bool,
    isolation_requirement: IsolationRequirement,
) -> AppResult<()> {
    if !confirmed {
        return Err(AppControlError::with_details(
            "CONFIRMATION_REQUIRED",
            "Screen capture requires explicit confirmation.",
            json!({
                "pixelsConsumed": false,
                "outputTouched": false,
                "fallback": "none",
            }),
        ));
    }
    if isolation_requirement == IsolationRequirement::Strict {
        return Err(AppControlError::with_details(
            "ISOLATION_REQUIRED",
            "Screen capture from a host Portal lease cannot satisfy strict isolation.",
            json!({
                "pixelsConsumed": false,
                "outputTouched": false,
                "executionRealm": "host-background",
                "fallback": "none",
            }),
        ));
    }
    Ok(())
}

fn validate_session_target(session_id: &str) -> AppResult<()> {
    if OpaqueTargetId::parse(session_id).map(OpaqueTargetId::kind)
        != Some(OpaqueTargetKind::InteractiveSession)
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "Desktop session requires a canonical s2:i target.",
        ));
    }
    Ok(())
}

fn stale_session_error() -> AppControlError {
    AppControlError::with_details(
        "STALE_SESSION",
        "The desktop session is not live in this owner generation.",
        json!({
            "targetInvalidated": true,
            "retrySafe": false,
            "fallback": "none",
        }),
    )
}

fn port_error(failure: DesktopSessionPortFailure) -> AppControlError {
    let outcome_unknown = failure.accepted_may_have_occurred() && !failure.cleanup_confirmed();
    // 已进入 Portal session 的失败即使清理确认，也不得自动重触发可见授权。
    let retry_safe = !failure.accepted_may_have_occurred();
    AppControlError::with_details(
        failure.code(),
        "The desktop session provider did not complete the requested lifecycle transition.",
        json!({
            "stage": failure.stage(),
            "outcome": if outcome_unknown { "unknown" } else { "failed" },
            "acceptedMayHaveOccurred": failure.accepted_may_have_occurred(),
            "sessionCleanupConfirmed": failure.cleanup_confirmed(),
            "retrySafe": retry_safe,
            "automaticRetryProhibited": !retry_safe,
            "targetInvalidated": failure.accepted_may_have_occurred(),
            "fallback": "none",
        }),
    )
}

fn input_port_error(
    failure: DesktopSessionInputFailure,
    session_cleanup_confirmed: bool,
) -> AppControlError {
    let cancelled = failure.code() == "CANCELLED";
    let outcome_unknown = failure.accepted_may_have_occurred();
    AppControlError::with_details(
        failure.code(),
        "The desktop-session input provider did not complete the requested sequence.",
        json!({
            "stage": failure.stage(),
            "outcome": if cancelled {
                "cancelled"
            } else if outcome_unknown {
                "unknown"
            } else {
                "failed"
            },
            "acceptedMayHaveOccurred": failure.accepted_may_have_occurred(),
            "releasesConfirmed": failure.releases_confirmed(),
            "completedSteps": failure.completed_steps(),
            "inputEventsSent": failure.input_events_sent(),
            "sessionCleanupConfirmed": session_cleanup_confirmed,
            "retrySafe": false,
            "automaticRetryProhibited": true,
            "targetInvalidated": true,
            "fallback": "none",
        }),
    )
}

fn frame_port_error(
    failure: DesktopSessionFrameFailure,
    session_cleanup_confirmed: Option<bool>,
) -> AppControlError {
    AppControlError::with_details(
        failure.code(),
        "The desktop-session screen capture provider did not complete one frame.",
        json!({
            "stage": failure.stage(),
            "outcome": if failure.pixels_may_have_been_consumed() { "unknown" } else { "failed" },
            "acceptedMayHaveOccurred": failure.pixels_may_have_been_consumed(),
            "pixelsMayHaveBeenConsumed": failure.pixels_may_have_been_consumed(),
            "sessionCleanupConfirmed": session_cleanup_confirmed,
            "targetInvalidated": failure.invalidates_session(),
            "retrySafe": false,
            "automaticRetryProhibited": true,
            "fallback": "none",
        }),
    )
}

fn capture_output_guard_error(error: OutputGuardError) -> AppControlError {
    match error {
        OutputGuardError::ConfirmationRequired => AppControlError::new(
            "OVERWRITE_CONFIRMATION_REQUIRED",
            "The screen capture output exists; set overwrite=true after confirmation.",
        ),
        OutputGuardError::InspectionFailed | OutputGuardError::InvalidTargetType => {
            AppControlError::new(
                "INVALID_OUTPUT_PATH",
                "The screen capture output is not a trusted regular file destination.",
            )
        }
    }
}

fn capture_atomic_file_error(error: AtomicFileError) -> AppControlError {
    match error {
        AtomicFileError::TargetExists => AppControlError::new(
            "OVERWRITE_CONFIRMATION_REQUIRED",
            "The screen capture output appeared before commit; retry with overwrite=true.",
        ),
        AtomicFileError::InvalidDestination => AppControlError::new(
            "INVALID_OUTPUT_PATH",
            "The screen capture output is not a trusted regular file destination.",
        ),
        #[cfg(target_os = "linux")]
        AtomicFileError::WriteFailed => AppControlError::new(
            "SCREENSHOT_WRITE_FAILED",
            "The screen capture PNG could not be written.",
        ),
        #[cfg(target_os = "windows")]
        AtomicFileError::InvalidStaging => AppControlError::new(
            "SCREENSHOT_WRITE_FAILED",
            "The staging file could not be written.",
        ),
        AtomicFileError::StagingCreationFailed
        | AtomicFileError::SyncFailed
        | AtomicFileError::CommitFailed => AppControlError::new(
            "SCREENSHOT_WRITE_FAILED",
            "The screen capture PNG could not be atomically written.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    #[derive(Clone)]
    struct FakePort {
        opens: Arc<AtomicUsize>,
        closes: Arc<AtomicUsize>,
        close_fails: bool,
    }

    struct FakeLease {
        closes: Arc<AtomicUsize>,
        close_fails: bool,
    }

    impl DesktopSessionLease for FakeLease {
        fn send_keyboard(
            &mut self,
            input: &KeyboardInput,
            cancellation: &DesktopInputCancellation,
        ) -> Result<DesktopKeyboardDispatchFacts, DesktopSessionInputFailure> {
            if cancellation.is_cancelled() {
                return Err(DesktopSessionInputFailure::cancelled(true, 0, 0));
            }
            Ok(DesktopKeyboardDispatchFacts::new(
                input.steps.len(),
                input.steps.len().saturating_mul(2),
            ))
        }

        fn send_pointer(
            &mut self,
            input: &DesktopPointerInput,
            cancellation: &DesktopInputCancellation,
        ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
            if cancellation.is_cancelled() {
                return Err(DesktopSessionInputFailure::cancelled(true, 0, 0));
            }
            Ok(DesktopPointerDispatchFacts::new(
                input.steps.len(),
                input.steps.len(),
            ))
        }

        fn capture_frame(
            &mut self,
            _: Duration,
            max_dimension: Option<u32>,
        ) -> Result<DesktopCapturedFrame, DesktopSessionFrameFailure> {
            desktop_session_frame_capture::encode_mapped_frame(
                &[3, 2, 1, 255],
                0,
                4,
                4,
                (1, 1),
                desktop_session_frame_capture::DesktopPackedPixelFormat::Bgra,
                max_dimension,
            )
            .map_err(|_| {
                DesktopSessionFrameFailure::new(
                    "CAPTURE_READBACK_FAILED",
                    "fixture-frame",
                    false,
                    false,
                )
            })
        }

        fn close(self: Box<Self>) -> Result<(), DesktopSessionPortFailure> {
            self.closes.fetch_add(1, Ordering::Relaxed);
            if self.close_fails {
                Err(DesktopSessionPortFailure::after_session(
                    "SESSION_CLOSE_UNCONFIRMED",
                    "close-session",
                    false,
                ))
            } else {
                Ok(())
            }
        }
    }

    impl DesktopSessionPort for FakePort {
        fn open(
            &self,
            _: Duration,
        ) -> Result<(Box<dyn DesktopSessionLease>, DesktopSessionFacts), DesktopSessionPortFailure>
        {
            self.opens.fetch_add(1, Ordering::Relaxed);
            Ok((
                Box::new(FakeLease {
                    closes: Arc::clone(&self.closes),
                    close_fails: self.close_fails,
                }),
                valid_facts(),
            ))
        }
    }

    fn valid_facts() -> DesktopSessionFacts {
        DesktopSessionFacts::new(2, 5, vec!["keyboard", "pointer"], 1, 1)
    }

    fn fake(close_fails: bool) -> (FakePort, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let opens = Arc::new(AtomicUsize::new(0));
        let closes = Arc::new(AtomicUsize::new(0));
        (
            FakePort {
                opens: Arc::clone(&opens),
                closes: Arc::clone(&closes),
                close_fails,
            },
            opens,
            closes,
        )
    }

    fn required_error<T>(
        result: Result<T, crate::domain::AppControlError>,
        message: &str,
    ) -> crate::domain::AppControlError {
        let Err(error) = result else {
            panic!("{message}");
        };
        error
    }

    #[test]
    fn open_inspect_and_close_preserve_only_opaque_live_identity() {
        let (port, opens, closes) = fake(false);
        let mut module = DesktopSessionModule::new(port);
        let opened = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
            )
            .unwrap_or_else(|error| panic!("desktop session open failed: {error}"));
        assert!(opened.session_id().starts_with("s2:i:"));
        assert_eq!(opened.facts(), &valid_facts());
        assert_eq!(module.sessions(), vec![opened.clone()]);
        assert_eq!(
            module
                .inspect(opened.session_id())
                .unwrap_or_else(|error| panic!("desktop session inspect failed: {error}")),
            opened
        );
        let closed = module
            .close(opened.session_id())
            .unwrap_or_else(|error| panic!("desktop session close failed: {error}"));
        assert!(closed.completed());
        assert_eq!(closed.session_id(), opened.session_id());
        assert!(module.sessions().is_empty());
        assert_eq!(opens.load(Ordering::Relaxed), 1);
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn keyboard_dispatch_updates_only_public_event_count() {
        let (port, _, closes) = fake(false);
        let mut module = DesktopSessionModule::new(port);
        let opened = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
            )
            .unwrap_or_else(|error| panic!("desktop session open failed: {error}"));
        let report = module
            .send_keyboard(
                opened.session_id(),
                true,
                true,
                IsolationRequirement::Standard,
                &json!({"steps": [{"type": "key", "key": "enter"}]}),
                &DesktopInputCancellation::new(),
            )
            .unwrap_or_else(|error| panic!("desktop keyboard dispatch failed: {error}"));
        assert_eq!(report.completed_steps(), 1);
        assert_eq!(report.input_events_sent(), 2);
        assert!(!report.effect_confirmed());
        assert_eq!(
            module
                .inspect(opened.session_id())
                .unwrap_or_else(|error| panic!("desktop session inspect failed: {error}"))
                .input_events_sent(),
            2
        );
        drop(module);
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn pointer_dispatch_updates_only_relative_public_facts() {
        let (port, _, closes) = fake(false);
        let mut module = DesktopSessionModule::new(port);
        let opened = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
            )
            .unwrap_or_else(|error| panic!("desktop session open failed: {error}"));
        let report = module
            .send_pointer(
                opened.session_id(),
                true,
                true,
                IsolationRequirement::Standard,
                &json!({
                    "coordinateSpace": "relative-logical-px",
                    "steps": [{"type": "move", "delta": {"x": 12, "y": -4}}]
                }),
                &DesktopInputCancellation::new(),
            )
            .unwrap_or_else(|error| panic!("desktop pointer dispatch failed: {error}"));
        assert_eq!(report.coordinate_space(), "relative-logical-px");
        assert_eq!(report.completed_steps(), 1);
        assert_eq!(report.input_events_sent(), 1);
        assert!(!report.effect_confirmed());
        assert!(!report.final_position_confirmed());
        assert_eq!(
            module
                .inspect(opened.session_id())
                .unwrap_or_else(|error| panic!("desktop session inspect failed: {error}"))
                .input_events_sent(),
            1
        );
        drop(module);
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn single_frame_capture_commits_png_and_updates_session_facts() {
        let (port, _, closes) = fake(false);
        let mut module = DesktopSessionModule::new(port);
        let opened = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
            )
            .unwrap_or_else(|error| panic!("desktop session open failed: {error}"));
        let directory = std::env::temp_dir().join(format!(
            "act-desktop-frame-{}-{}",
            std::process::id(),
            opened.session_id().replace(':', "-")
        ));
        std::fs::create_dir(&directory)
            .unwrap_or_else(|error| panic!("建立单帧 fixture 目录失败：{error}"));
        let destination = directory.join("frame.png");
        let report = module
            .capture_frame(
                opened.session_id(),
                true,
                IsolationRequirement::Standard,
                &json!({"path": destination.to_string_lossy(), "timeoutMs": 1000}),
            )
            .unwrap_or_else(|error| panic!("desktop frame capture failed: {error}"));
        assert_eq!((report.width(), report.height()), (1, 1));
        assert!(report.bytes() > 0);
        assert!(!report.replaced_existing());
        let persisted_bytes = std::fs::read(&destination)
            .unwrap_or_else(|error| panic!("读取单帧 fixture 失败：{error}"));
        assert_eq!(persisted_bytes.len(), report.bytes());
        let inspected = module
            .inspect(opened.session_id())
            .unwrap_or_else(|error| panic!("desktop session inspect failed: {error}"));
        assert_eq!(inspected.frames_captured(), 1);
        assert_eq!(inspected.pixels_consumed(), 1);
        std::fs::remove_file(&destination)
            .unwrap_or_else(|error| panic!("清理单帧 fixture 失败：{error}"));
        std::fs::remove_dir(&directory)
            .unwrap_or_else(|error| panic!("清理单帧 fixture 目录失败：{error}"));
        drop(module);
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cancellation_before_dispatch_invalidates_the_session_without_input_effects() {
        let (port, _, closes) = fake(false);
        let mut module = DesktopSessionModule::new(port);
        let opened = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
            )
            .unwrap_or_else(|error| panic!("desktop session open failed: {error}"));
        let cancellation = DesktopInputCancellation::new();
        cancellation.cancel();
        let error = required_error(
            module.send_keyboard(
                opened.session_id(),
                true,
                true,
                IsolationRequirement::Standard,
                &json!({"steps": [{"type": "key", "key": "enter"}]}),
                &cancellation,
            ),
            "cancelled input must return a terminal failure",
        );
        assert_eq!(error.code, "CANCELLED");
        assert_eq!(error.details["outcome"], "cancelled");
        assert_eq!(error.details["acceptedMayHaveOccurred"], false);
        assert_eq!(error.details["inputEventsSent"], 0);
        assert_eq!(error.details["targetInvalidated"], true);
        assert!(module.inspect(opened.session_id()).is_err());
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn close_failure_invalidates_target_and_prohibits_retry() {
        let (port, _, closes) = fake(true);
        let mut module = DesktopSessionModule::new(port);
        let opened = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
            )
            .unwrap_or_else(|error| panic!("desktop session open failed: {error}"));
        let error = required_error(
            module.close(opened.session_id()),
            "uncertain close must fail",
        );
        assert_eq!(error.code, "SESSION_CLOSE_UNCONFIRMED");
        assert_eq!(error.details["outcome"], "unknown");
        assert_eq!(error.details["retrySafe"], false);
        assert_eq!(error.details["targetInvalidated"], true);
        assert!(module.sessions().is_empty());
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn module_drop_closes_each_remaining_lease_once() {
        let (port, _, closes) = fake(false);
        {
            let mut module = DesktopSessionModule::new(port);
            let _first = module
                .open(
                    true,
                    true,
                    IsolationRequirement::Standard,
                    Duration::from_secs(120),
                )
                .unwrap_or_else(|error| panic!("first desktop session open failed: {error}"));
            let _second = module
                .open(
                    true,
                    true,
                    IsolationRequirement::Standard,
                    Duration::from_secs(120),
                )
                .unwrap_or_else(|error| panic!("second desktop session open failed: {error}"));
        }
        assert_eq!(closes.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn invalid_target_and_timeout_fail_before_provider_access() {
        let (port, opens, _) = fake(false);
        let mut module = DesktopSessionModule::new(port);
        let timeout_error = required_error(
            module.open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(9),
            ),
            "short timeout must fail",
        );
        assert_eq!(timeout_error.code, "INVALID_ARGUMENT");
        let target_error = required_error(
            module.inspect("/org/freedesktop/portal/desktop/session/private"),
            "native path must never be accepted",
        );
        assert_eq!(target_error.code, "INVALID_ARGUMENT");
        assert_eq!(opens.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn permissions_and_strict_isolation_fail_before_provider_access() {
        let (port, opens, _) = fake(false);
        let mut module = DesktopSessionModule::new(port);
        let confirmation = required_error(
            module.open(
                false,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
            ),
            "missing confirmation must fail",
        );
        assert_eq!(confirmation.code, "CONFIRMATION_REQUIRED");
        let foreground = required_error(
            module.open(
                true,
                false,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
            ),
            "missing foreground consent must fail",
        );
        assert_eq!(foreground.code, "FOREGROUND_CONSENT_REQUIRED");
        let isolation = required_error(
            module.open(
                true,
                true,
                IsolationRequirement::Strict,
                Duration::from_secs(120),
            ),
            "strict isolation must reject visible Portal flow",
        );
        assert_eq!(isolation.code, "ISOLATION_REQUIRED");
        assert_eq!(opens.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn accepted_open_failure_never_allows_automatic_reauthorization() {
        let error = port_error(DesktopSessionPortFailure::after_session(
            "TIMEOUT", "start", true,
        ));
        assert_eq!(error.details["outcome"], "failed");
        assert_eq!(error.details["sessionCleanupConfirmed"], true);
        assert_eq!(error.details["retrySafe"], false);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
