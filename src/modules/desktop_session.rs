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

/// open 请求显式选择的授权作用域。
///
/// `Session` 表示 connect/open 处的一次确认覆盖整条会话；`PerOperation` 保持
/// 逐操作显式确认的旧模式（缺省兼容）。显式拒绝不能被任何作用域继承覆盖。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DesktopAuthorizationScope {
    #[default]
    PerOperation,
    Session,
}

impl DesktopAuthorizationScope {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::PerOperation => "operation",
            Self::Session => "session",
        }
    }
}

/// open 请求携带的完整授权意图。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DesktopSessionAuthorization {
    pub(crate) scope: DesktopAuthorizationScope,
    /// 请求按平台原生机制记住授权（Linux Portal persist_mode=2 + restore token）。
    pub(crate) remember: bool,
}

/// Adapter 报告的持久化授权事实；只有脱敏布尔与原因，绝不携带 token。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DesktopAuthorizationPersistenceState {
    /// 本次 open 是否请求了记住授权。
    pub(crate) requested: bool,
    /// 本次 open 是否向 Portal 提交了已保存 token（恢复尝试）。
    ///
    /// 只代表提交动作，不代表免提示恢复成功：Portal 在无法恢复时按官方
    /// 语义忽略 token 并正常弹窗，客户端无法观测是否真的免提示恢复；
    /// 不以 token 存在或连接耗时作为恢复成功的证据。
    pub(crate) restore_attempted: bool,
    /// 本次 open 后本地是否持有可用 restore token。
    pub(crate) token_retained: bool,
    /// 请求记住但未保存成功的原因；None 表示无异常或未请求。
    pub(crate) note: Option<&'static str>,
}

/// 传给 Adapter 的持久化意图；Adapter 只在 Remember 时读写自有存储。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DesktopAuthorizationPersistence {
    #[default]
    None,
    Remember,
}

/// 单次操作请求携带的显式确认字段；`None` 字段在 session 授权下继承已授予作用域。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct DesktopConsent {
    pub(crate) confirmed: Option<bool>,
    pub(crate) foreground_consent: Option<bool>,
    pub(crate) strict_isolation: Option<bool>,
}

impl DesktopConsent {
    /// 全显式构造（旧调用与测试用）：三个字段都有确定值。
    #[cfg(test)]
    pub(crate) const fn explicit(confirmed: bool, foreground: bool, strict: bool) -> Self {
        Self {
            confirmed: Some(confirmed),
            foreground_consent: Some(foreground),
            strict_isolation: Some(strict),
        }
    }

    fn isolation_of(strict: bool) -> IsolationRequirement {
        if strict {
            IsolationRequirement::Strict
        } else {
            IsolationRequirement::Standard
        }
    }
}

/// 本工具保存授权的脱敏状态；不区分具体 token 内容。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopSavedAuthorizationState {
    /// 平台后端不支持记住授权（如 Windows）。
    Unsupported,
    /// 没有保存任何授权。
    Absent,
    /// 持有一条可尝试恢复的已保存授权。
    Saved,
    /// 存储存在但未通过私有性校验，拒绝使用。
    Unreadable,
}

impl DesktopSavedAuthorizationState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Absent => "absent",
            Self::Saved => "saved",
            Self::Unreadable => "unreadable",
        }
    }
}

/// 查询本工具保存授权得到的脱敏事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSavedAuthorization {
    state: DesktopSavedAuthorizationState,
    backend: &'static str,
}

impl DesktopSavedAuthorization {
    pub(crate) const fn unsupported() -> Self {
        Self {
            state: DesktopSavedAuthorizationState::Unsupported,
            backend: "none",
        }
    }

    pub(crate) const fn of(state: DesktopSavedAuthorizationState, backend: &'static str) -> Self {
        Self { state, backend }
    }

    pub(crate) const fn state(&self) -> DesktopSavedAuthorizationState {
        self.state
    }

    pub(crate) const fn backend(&self) -> &'static str {
        self.backend
    }
}

/// 清除本工具保存授权的结果事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSavedAuthorizationForget {
    had_saved: bool,
    cleared: bool,
}

impl DesktopSavedAuthorizationForget {
    pub(crate) const fn nothing_to_clear() -> Self {
        Self {
            had_saved: false,
            cleared: true,
        }
    }

    pub(crate) const fn outcome(had_saved: bool, cleared: bool) -> Self {
        Self { had_saved, cleared }
    }

    pub(crate) const fn had_saved(&self) -> bool {
        self.had_saved
    }

    pub(crate) const fn cleared(&self) -> bool {
        self.cleared
    }
}

/// 撤销入口的完整报告：清除保存凭据并停止本客户端 live 会话。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopAuthorizationForgetReport {
    had_saved: bool,
    cleared: bool,
    sessions_closed: usize,
    close_failures: usize,
}

impl DesktopAuthorizationForgetReport {
    pub(crate) const fn had_saved(&self) -> bool {
        self.had_saved
    }

    pub(crate) const fn cleared(&self) -> bool {
        self.cleared
    }

    pub(crate) const fn sessions_closed(&self) -> usize {
        self.sessions_closed
    }

    pub(crate) const fn close_failures(&self) -> usize {
        self.close_failures
    }
}

/// 保存 Portal/EIS 建立后允许跨层公开的 provider-neutral 事实。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSessionFacts {
    backend: &'static str,
    remote_desktop_version: u32,
    screen_cast_version: u32,
    authorized_device_classes: Vec<&'static str>,
    stream_count: usize,
    mapping_id_count: usize,
    authorization_persistence: DesktopAuthorizationPersistenceState,
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
            authorization_persistence: DesktopAuthorizationPersistenceState::default(),
        }
    }

    /// 附加 Adapter 报告的持久化授权事实（仅 Linux Portal Remember 路径设置）。
    pub(crate) fn with_persistence(
        mut self,
        persistence: DesktopAuthorizationPersistenceState,
    ) -> Self {
        self.authorization_persistence = persistence;
        self
    }

    pub(crate) const fn authorization_persistence(&self) -> DesktopAuthorizationPersistenceState {
        self.authorization_persistence
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
            authorization_persistence: DesktopAuthorizationPersistenceState::default(),
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
    /// 结束一段连续点派发。
    ///
    /// 后端可能在点之间维持一个跨越整批的 emulation/注入会话（EIS 绝对指针就是如此：
    /// 逐点开关会被 compositor 断开），所以批结束时必须显式收尾。默认后端不持有会话。
    fn finish_frame_points(&mut self) -> Result<(), DesktopSessionInputFailure> {
        Ok(())
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

/// 一条脱敏的输入后端事件事实：只含事件类别词、单调序号与事件后的
/// 代际/可用设备计数，不含设备 ID、区域内容、按键或坐标。
///
/// 供失败诊断区分「哪个服务端事件、按什么顺序」；由生产事件处理路径
/// 记录，固定容量、只随失败详情输出。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopProviderEventFact {
    pub(crate) sequence: u64,
    pub(crate) event: &'static str,
    pub(crate) generation_after: u64,
    pub(crate) absolute_devices_after: usize,
}

/// 失败详情携带的最近事件数上限；环形轨迹容量更大，输出取最近这些。
pub(crate) const PROVIDER_EVENT_FACT_SNAPSHOT: usize = 8;

/// 保存会话级输入 Adapter 的稳定失败事实，不暴露 EIS 原生对象或消息。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSessionInputFailure {
    code: &'static str,
    stage: &'static str,
    accepted_may_have_occurred: bool,
    releases_confirmed: bool,
    completed_steps: usize,
    input_events_sent: usize,
    /// 最近的后端事件事实（脱敏、固定容量）；空表示无记录或非相关失败。
    provider_events: [Option<DesktopProviderEventFact>; PROVIDER_EVENT_FACT_SNAPSHOT],
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
            provider_events: [None; PROVIDER_EVENT_FACT_SNAPSHOT],
        }
    }

    /// 附上最近的后端事件事实（脱敏快照）；只影响诊断详情。
    pub(crate) fn with_provider_events(
        mut self,
        events: [Option<DesktopProviderEventFact>; PROVIDER_EVENT_FACT_SNAPSHOT],
    ) -> Self {
        self.provider_events = events;
        self
    }

    pub(crate) const fn provider_recent_events(
        &self,
    ) -> &[Option<DesktopProviderEventFact>; PROVIDER_EVENT_FACT_SNAPSHOT] {
        &self.provider_events
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
            provider_events: [None; PROVIDER_EVENT_FACT_SNAPSHOT],
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
            provider_events: [None; PROVIDER_EVENT_FACT_SNAPSHOT],
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
    /// 平台是否支持跨进程记住授权；Windows 沿用 OS 自身边界，保持 false。
    const PERSISTENT_AUTHORIZATION_SUPPORTED: bool = false;

    fn open(
        &self,
        timeout: Duration,
        persistence: DesktopAuthorizationPersistence,
    ) -> Result<(Box<dyn DesktopSessionLease>, DesktopSessionFacts), DesktopSessionPortFailure>;

    /// 读取本工具保存授权的脱敏状态；默认平台无存储。
    fn saved_authorization(&self) -> DesktopSavedAuthorization {
        DesktopSavedAuthorization::unsupported()
    }

    /// 清除本工具保存的授权凭据；默认平台没有任何可清除内容。
    fn forget_saved_authorization(
        &self,
    ) -> Result<DesktopSavedAuthorizationForget, DesktopSessionPortFailure> {
        Ok(DesktopSavedAuthorizationForget::nothing_to_clear())
    }
}

/// 保存可返回给 System 的 live 会话只读投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSessionView {
    session_id: String,
    facts: DesktopSessionFacts,
    authorization: DesktopSessionAuthorization,
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

    pub(crate) const fn authorization(&self) -> DesktopSessionAuthorization {
        self.authorization
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
    authorization: DesktopSessionAuthorization,
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
        authorization: DesktopSessionAuthorization,
    ) -> AppResult<DesktopSessionView> {
        validate_open_permissions(confirmed, foreground_consent, isolation_requirement)?;
        validate_open_timeout(timeout)?;
        if authorization.remember && !P::PERSISTENT_AUTHORIZATION_SUPPORTED {
            // 平台没有可用的原生记住授权机制时如实拒绝，不静默降级也不伪造。
            return Err(AppControlError::with_details(
                "DESKTOP_AUTHORIZATION_PERSISTENCE_UNSUPPORTED",
                "Remembering desktop authorization requires the Linux Portal backend.",
                json!({
                    "portalRequestIssued": false,
                    "retrySafe": false,
                    "fallback": "none",
                }),
            ));
        }
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
        let persistence = if authorization.remember {
            DesktopAuthorizationPersistence::Remember
        } else {
            DesktopAuthorizationPersistence::None
        };
        let (lease, facts) = self.port.open(timeout, persistence).map_err(port_error)?;
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
            authorization,
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
                authorization,
            },
        );
        Ok(view)
    }

    /// 读取本工具保存授权的脱敏状态。
    pub(crate) fn saved_authorization(&self) -> DesktopSavedAuthorization {
        self.port.saved_authorization()
    }

    /// 清除本工具保存的授权凭据，并停止本 Module 持有的全部 live 会话。
    pub(crate) fn forget_authorization(&mut self) -> AppResult<DesktopAuthorizationForgetReport> {
        let mut ids = self.sessions.keys().cloned().collect::<Vec<_>>();
        ids.sort();
        let mut sessions_closed = 0;
        let mut close_failures = 0;
        for id in ids {
            if self.close(&id).is_ok() {
                sessions_closed += 1;
            } else {
                close_failures += 1;
            }
        }
        let forget = self.port.forget_saved_authorization().map_err(port_error)?;
        Ok(DesktopAuthorizationForgetReport {
            had_saved: forget.had_saved(),
            cleared: forget.cleared(),
            sessions_closed,
            close_failures,
        })
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
        consent: DesktopConsent,
        value: &Value,
        cancellation: &DesktopInputCancellation,
    ) -> AppResult<DesktopKeyboardReport> {
        self.resolve_input_consent(session_id, consent)?;
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
        consent: DesktopConsent,
        value: &Value,
        cancellation: &DesktopInputCancellation,
    ) -> AppResult<DesktopPointerReport> {
        self.resolve_input_consent(session_id, consent)?;
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
        consent: DesktopConsent,
        value: &Value,
    ) -> AppResult<DesktopFrameCaptureReport> {
        self.resolve_capture_consent(session_id, consent)?;
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

    /// 解析输入类操作的确认字段。
    ///
    /// 全显式请求保持旧门禁语义；任何省略字段只能来自 session 授权继承，
    /// 显式拒绝先于继承闭合，不能被已授予作用域覆盖。
    fn resolve_input_consent(&self, session_id: &str, consent: DesktopConsent) -> AppResult<()> {
        let DesktopConsent {
            confirmed,
            foreground_consent,
            strict_isolation,
        } = consent;
        if let (Some(confirmed), Some(foreground), Some(strict)) =
            (confirmed, foreground_consent, strict_isolation)
        {
            validate_input_permissions(
                confirmed,
                foreground,
                DesktopConsent::isolation_of(strict),
            )?;
            return Ok(());
        }
        if let Some(confirmed) = confirmed {
            validate_input_permissions(confirmed, true, IsolationRequirement::Standard)?;
        }
        if let Some(strict) = strict_isolation {
            validate_input_permissions(true, true, DesktopConsent::isolation_of(strict))?;
        }
        if let Some(foreground) = foreground_consent {
            validate_input_permissions(true, foreground, IsolationRequirement::Standard)?;
        }
        self.inherit_session_consent(session_id, true)
    }

    /// 解析截图/观察类操作的确认字段；规则与输入一致，只缺前景同意维度。
    fn resolve_capture_consent(&self, session_id: &str, consent: DesktopConsent) -> AppResult<()> {
        let DesktopConsent {
            confirmed,
            strict_isolation,
            ..
        } = consent;
        if let (Some(confirmed), Some(strict)) = (confirmed, strict_isolation) {
            validate_capture_permissions(confirmed, DesktopConsent::isolation_of(strict))?;
            return Ok(());
        }
        if let Some(confirmed) = confirmed {
            validate_capture_permissions(confirmed, IsolationRequirement::Standard)?;
        }
        if let Some(strict) = strict_isolation {
            validate_capture_permissions(true, DesktopConsent::isolation_of(strict))?;
        }
        self.inherit_session_consent(session_id, false)
    }

    /// 省略字段只能从仍持有的 session 授权会话继承；其余情况保持显式要求。
    fn inherit_session_consent(&self, session_id: &str, input: bool) -> AppResult<()> {
        validate_session_target(session_id)?;
        match self
            .sessions
            .get(session_id)
            .map(|entry| entry.authorization.scope)
        {
            None => Err(stale_session_error()),
            Some(DesktopAuthorizationScope::Session) => Ok(()),
            Some(DesktopAuthorizationScope::PerOperation) => Err(if input {
                AppControlError::with_details(
                    "CONFIRMATION_REQUIRED",
                    "Desktop-session input requires explicit confirmation fields unless the session was opened with authorizationScope=session.",
                    json!({
                        "inputAttempted": false,
                        "confirmationFieldsOmitted": true,
                        "fallback": "none",
                    }),
                )
            } else {
                AppControlError::with_details(
                    "CONFIRMATION_REQUIRED",
                    "Screen capture requires explicit confirmation fields unless the session was opened with authorizationScope=session.",
                    json!({
                        "pixelsConsumed": false,
                        "outputTouched": false,
                        "confirmationFieldsOmitted": true,
                        "fallback": "none",
                    }),
                )
            }),
        }
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
    let mut details = json!({
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
    });
    // 脱敏有界的事件轨迹：让「哪个服务端事件、什么顺序」可辨，空则省略。
    let provider_events = failure
        .provider_recent_events()
        .iter()
        .flatten()
        .map(|fact| {
            json!({
                "sequence": fact.sequence,
                "event": fact.event,
                "generationAfter": fact.generation_after,
                "absoluteDevicesAfter": fact.absolute_devices_after,
            })
        })
        .collect::<Vec<_>>();
    if !provider_events.is_empty() {
        details["providerRecentEvents"] = json!(provider_events);
    }
    AppControlError::with_details(
        failure.code(),
        "The desktop-session input provider did not complete the requested sequence.",
        details,
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
            _: DesktopAuthorizationPersistence,
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
                DesktopSessionAuthorization::default(),
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
                DesktopSessionAuthorization::default(),
            )
            .unwrap_or_else(|error| panic!("desktop session open failed: {error}"));
        let report = module
            .send_keyboard(
                opened.session_id(),
                DesktopConsent::explicit(true, true, false),
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
                DesktopSessionAuthorization::default(),
            )
            .unwrap_or_else(|error| panic!("desktop session open failed: {error}"));
        let report = module
            .send_pointer(
                opened.session_id(),
                DesktopConsent::explicit(true, true, false),
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
                DesktopSessionAuthorization::default(),
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
                DesktopConsent::explicit(true, true, false),
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
                DesktopSessionAuthorization::default(),
            )
            .unwrap_or_else(|error| panic!("desktop session open failed: {error}"));
        let cancellation = DesktopInputCancellation::new();
        cancellation.cancel();
        let error = required_error(
            module.send_keyboard(
                opened.session_id(),
                DesktopConsent::explicit(true, true, false),
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
                DesktopSessionAuthorization::default(),
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
                    DesktopSessionAuthorization::default(),
                )
                .unwrap_or_else(|error| panic!("first desktop session open failed: {error}"));
            let _second = module
                .open(
                    true,
                    true,
                    IsolationRequirement::Standard,
                    Duration::from_secs(120),
                    DesktopSessionAuthorization::default(),
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
                DesktopSessionAuthorization::default(),
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
                DesktopSessionAuthorization::default(),
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
                DesktopSessionAuthorization::default(),
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
                DesktopSessionAuthorization::default(),
            ),
            "strict isolation must reject visible Portal flow",
        );
        assert_eq!(isolation.code, "ISOLATION_REQUIRED");
        assert_eq!(opens.load(Ordering::Relaxed), 0);
    }

    /// 携带轨迹的输入失败经真实模块错误映射输出 providerRecentEvents。
    struct TrailFailingLease {
        trail: [Option<DesktopProviderEventFact>; PROVIDER_EVENT_FACT_SNAPSHOT],
        closes: Arc<AtomicUsize>,
    }

    impl DesktopSessionLease for TrailFailingLease {
        fn send_keyboard(
            &mut self,
            _: &KeyboardInput,
            _: &DesktopInputCancellation,
        ) -> Result<DesktopKeyboardDispatchFacts, DesktopSessionInputFailure> {
            Err(DesktopSessionInputFailure::after_dispatch(
                "EIS_DEVICE_UNAVAILABLE",
                "eis-test-stage",
                true,
                0,
                0,
            )
            .with_provider_events(self.trail))
        }

        fn send_pointer(
            &mut self,
            _: &DesktopPointerInput,
            _: &DesktopInputCancellation,
        ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
            Err(DesktopSessionInputFailure::after_dispatch(
                "EIS_DEVICE_UNAVAILABLE",
                "eis-test-stage",
                true,
                0,
                0,
            )
            .with_provider_events(self.trail))
        }

        fn close(self: Box<Self>) -> Result<(), DesktopSessionPortFailure> {
            self.closes.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    struct TrailFailingPort {
        trail: [Option<DesktopProviderEventFact>; PROVIDER_EVENT_FACT_SNAPSHOT],
        closes: Arc<AtomicUsize>,
    }

    impl DesktopSessionPort for TrailFailingPort {
        fn open(
            &self,
            _: Duration,
            _: DesktopAuthorizationPersistence,
        ) -> Result<(Box<dyn DesktopSessionLease>, DesktopSessionFacts), DesktopSessionPortFailure>
        {
            Ok((
                Box::new(TrailFailingLease {
                    trail: self.trail,
                    closes: Arc::clone(&self.closes),
                }),
                valid_facts(),
            ))
        }
    }

    fn fact(sequence: u64, event: &'static str) -> Option<DesktopProviderEventFact> {
        Some(DesktopProviderEventFact {
            sequence,
            event,
            generation_after: sequence + 1,
            absolute_devices_after: 0,
        })
    }

    #[test]
    fn input_failure_event_trail_reaches_public_details_in_order() {
        let mut trail = [None; PROVIDER_EVENT_FACT_SNAPSHOT];
        trail[0] = fact(1, "device-paused");
        trail[1] = fact(2, "device-resumed");
        trail[2] = fact(3, "device-removed");
        let port = TrailFailingPort {
            trail,
            closes: Arc::new(AtomicUsize::new(0)),
        };
        let closes = Arc::clone(&port.closes);
        let mut module = DesktopSessionModule::new(port);
        let opened = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
                DesktopSessionAuthorization::default(),
            )
            .unwrap_or_else(|error| panic!("open failed: {error}"));
        let error = required_error(
            module.send_keyboard(
                opened.session_id(),
                DesktopConsent::explicit(true, true, false),
                &json!({"steps": [{"type": "key", "key": "enter"}]}),
                &DesktopInputCancellation::new(),
            ),
            "trail-carrying input failure must surface",
        );
        assert_eq!(error.code, "EIS_DEVICE_UNAVAILABLE");
        let events = error.details["providerRecentEvents"]
            .as_array()
            .unwrap_or_else(|| panic!("providerRecentEvents missing: {error:?}"));
        assert_eq!(events.len(), 3);
        let sequences = events
            .iter()
            .map(|event| event["sequence"].as_u64())
            .collect::<Vec<_>>();
        // 从旧到新；事实字段脱敏完整。
        assert_eq!(sequences, vec![Some(1), Some(2), Some(3)]);
        assert_eq!(events[0]["event"], "device-paused");
        assert_eq!(events[0]["generationAfter"], 2);
        assert_eq!(events[0]["absoluteDevicesAfter"], 0);
        // 安全字段保持既有语义，不因轨迹改变。
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["completedSteps"], 0);
        assert_eq!(error.details["inputEventsSent"], 0);
        assert_eq!(error.details["targetInvalidated"], true);
        assert_eq!(error.details["retrySafe"], false);
        assert_eq!(error.details["sessionCleanupConfirmed"], true);
        // 失败仍作废会话并收尾 lease。
        assert!(module.sessions().is_empty());
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn input_failure_without_trail_omits_the_details_field() {
        let port = TrailFailingPort {
            trail: [None; PROVIDER_EVENT_FACT_SNAPSHOT],
            closes: Arc::new(AtomicUsize::new(0)),
        };
        let mut module = DesktopSessionModule::new(port);
        let opened = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
                DesktopSessionAuthorization::default(),
            )
            .unwrap_or_else(|error| panic!("open failed: {error}"));
        let error = required_error(
            module.send_pointer(
                opened.session_id(),
                DesktopConsent::explicit(true, true, false),
                &json!({
                    "coordinateSpace": "relative-logical-px",
                    "steps": [{"type": "move", "delta": {"x": 1, "y": 1}}]
                }),
                &DesktopInputCancellation::new(),
            ),
            "plain input failure must still fail",
        );
        assert_eq!(error.code, "EIS_DEVICE_UNAVAILABLE");
        assert!(
            error.details.get("providerRecentEvents").is_none(),
            "空轨迹必须省略字段: {error:?}"
        );
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

    fn session_authorization() -> DesktopSessionAuthorization {
        DesktopSessionAuthorization {
            scope: DesktopAuthorizationScope::Session,
            remember: false,
        }
    }

    #[test]
    fn session_scope_inherits_consent_for_input_and_capture() {
        let (port, opens, _) = fake(false);
        let mut module = DesktopSessionModule::new(port);
        let opened = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
                session_authorization(),
            )
            .unwrap_or_else(|error| panic!("session-scope open failed: {error}"));
        assert_eq!(
            opened.authorization().scope,
            DesktopAuthorizationScope::Session
        );
        // 省略全部确认字段：输入继承 open 处的一次确认。
        module
            .send_keyboard(
                opened.session_id(),
                DesktopConsent::default(),
                &json!({"steps": [{"type": "key", "key": "enter"}]}),
                &DesktopInputCancellation::new(),
            )
            .unwrap_or_else(|error| panic!("inherited keyboard dispatch failed: {error}"));
        // 部分显式（confirmed=true、省略其余）同样允许继承剩余字段。
        module
            .send_pointer(
                opened.session_id(),
                DesktopConsent {
                    confirmed: Some(true),
                    ..DesktopConsent::default()
                },
                &json!({
                    "coordinateSpace": "relative-logical-px",
                    "steps": [{"type": "move", "delta": {"x": 1, "y": 1}}]
                }),
                &DesktopInputCancellation::new(),
            )
            .unwrap_or_else(|error| panic!("partial explicit pointer dispatch failed: {error}"));
        // 显式拒绝不能被已授予作用域覆盖。
        let refused = required_error(
            module.send_keyboard(
                opened.session_id(),
                DesktopConsent::explicit(false, true, false),
                &json!({"steps": [{"type": "key", "key": "enter"}]}),
                &DesktopInputCancellation::new(),
            ),
            "explicit refusal must not be overridden by session scope",
        );
        assert_eq!(refused.code, "CONFIRMATION_REQUIRED");
        assert_eq!(refused.details["inputAttempted"], false);
        let strict = required_error(
            module.send_keyboard(
                opened.session_id(),
                DesktopConsent::explicit(true, true, true),
                &json!({"steps": [{"type": "key", "key": "enter"}]}),
                &DesktopInputCancellation::new(),
            ),
            "explicit strict isolation must not be overridden by session scope",
        );
        assert_eq!(strict.code, "ISOLATION_REQUIRED");
        assert_eq!(strict.details["inputAttempted"], false);
        assert_eq!(opens.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn per_operation_scope_still_requires_explicit_confirmation_fields() {
        let (port, opens, _) = fake(false);
        let mut module = DesktopSessionModule::new(port);
        let opened = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
                DesktopSessionAuthorization::default(),
            )
            .unwrap_or_else(|error| panic!("open failed: {error}"));
        let omitted = required_error(
            module.send_keyboard(
                opened.session_id(),
                DesktopConsent::default(),
                &json!({"steps": [{"type": "key", "key": "enter"}]}),
                &DesktopInputCancellation::new(),
            ),
            "omitted fields must not inherit in per-operation mode",
        );
        assert_eq!(omitted.code, "CONFIRMATION_REQUIRED");
        assert_eq!(omitted.details["inputAttempted"], false);
        assert_eq!(omitted.details["confirmationFieldsOmitted"], true);
        let omitted_capture = required_error(
            module.capture_frame(
                opened.session_id(),
                DesktopConsent::default(),
                &json!({"path": "/tmp/act-omitted-consent.png"}),
            ),
            "omitted capture fields must not inherit in per-operation mode",
        );
        assert_eq!(omitted_capture.code, "CONFIRMATION_REQUIRED");
        assert_eq!(omitted_capture.details["confirmationFieldsOmitted"], true);
        // 全显式仍可用（旧调用兼容）。
        module
            .send_keyboard(
                opened.session_id(),
                DesktopConsent::explicit(true, true, false),
                &json!({"steps": [{"type": "key", "key": "enter"}]}),
                &DesktopInputCancellation::new(),
            )
            .unwrap_or_else(|error| panic!("explicit dispatch failed: {error}"));
        assert_eq!(opens.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn omitted_fields_without_live_session_report_stale_target() {
        let (port, _, _) = fake(false);
        let mut module = DesktopSessionModule::new(port);
        let error = required_error(
            module.send_keyboard(
                "s2:i:0123456789abcdef",
                DesktopConsent::default(),
                &json!({"steps": [{"type": "key", "key": "enter"}]}),
                &DesktopInputCancellation::new(),
            ),
            "omitted consent without a live session must be stale",
        );
        assert_eq!(error.code, "STALE_SESSION");
    }

    #[test]
    fn remember_on_unsupported_backend_fails_before_provider_access() {
        let (port, opens, _) = fake(false);
        let mut module = DesktopSessionModule::new(port);
        let error = required_error(
            module.open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
                DesktopSessionAuthorization {
                    scope: DesktopAuthorizationScope::Session,
                    remember: true,
                },
            ),
            "remember must fail on backends without native persistence",
        );
        assert_eq!(error.code, "DESKTOP_AUTHORIZATION_PERSISTENCE_UNSUPPORTED");
        assert_eq!(error.details["portalRequestIssued"], false);
        assert_eq!(opens.load(Ordering::Relaxed), 0);
    }

    /// 模拟支持持久化的端口：保存/轮换 fake token，用于验证模块报告的脱敏事实。
    struct RememberingPort {
        grants_token: bool,
        saved: std::sync::Mutex<Option<String>>,
    }

    impl DesktopSessionPort for RememberingPort {
        const PERSISTENT_AUTHORIZATION_SUPPORTED: bool = true;

        fn open(
            &self,
            _: Duration,
            persistence: DesktopAuthorizationPersistence,
        ) -> Result<(Box<dyn DesktopSessionLease>, DesktopSessionFacts), DesktopSessionPortFailure>
        {
            let mut state = DesktopAuthorizationPersistenceState::default();
            if persistence == DesktopAuthorizationPersistence::Remember {
                state.requested = true;
                let saved = self
                    .saved
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .clone();
                state.restore_attempted = saved.is_some();
                if self.grants_token {
                    *self
                        .saved
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner()) =
                        Some("fake-portal-restore-token".to_owned());
                    state.token_retained = true;
                } else {
                    state.note = Some("portal-did-not-grant-persistence");
                }
            }
            let closes = Arc::new(AtomicUsize::new(0));
            Ok((
                Box::new(FakeLease {
                    closes,
                    close_fails: false,
                }),
                valid_facts().with_persistence(state),
            ))
        }

        fn saved_authorization(&self) -> DesktopSavedAuthorization {
            let state = if self
                .saved
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .is_some()
            {
                DesktopSavedAuthorizationState::Saved
            } else {
                DesktopSavedAuthorizationState::Absent
            };
            DesktopSavedAuthorization::of(state, "fake-store")
        }

        fn forget_saved_authorization(
            &self,
        ) -> Result<DesktopSavedAuthorizationForget, DesktopSessionPortFailure> {
            let mut saved = self
                .saved
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let had = saved.is_some();
            *saved = None;
            Ok(DesktopSavedAuthorizationForget::outcome(had, true))
        }
    }

    #[test]
    fn remembered_authorization_reports_desensitized_retention_facts() {
        let port = RememberingPort {
            grants_token: true,
            saved: std::sync::Mutex::new(None),
        };
        let mut module = DesktopSessionModule::new(port);
        let remember = DesktopSessionAuthorization {
            scope: DesktopAuthorizationScope::Session,
            remember: true,
        };
        let first = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
                remember,
            )
            .unwrap_or_else(|error| panic!("first remembered open failed: {error}"));
        let retention = first.facts().authorization_persistence();
        assert!(retention.requested);
        assert!(!retention.restore_attempted);
        assert!(retention.token_retained);
        assert_eq!(
            module.saved_authorization().state(),
            DesktopSavedAuthorizationState::Saved
        );
        // 第二次连接提交已保存 token（恢复尝试）并轮换出新 token。
        let second = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
                remember,
            )
            .unwrap_or_else(|error| panic!("second remembered open failed: {error}"));
        let rotation = second.facts().authorization_persistence();
        assert!(rotation.restore_attempted);
        assert!(rotation.token_retained);
        // 未请求记住时 Adapter 不读写存储，事实保持缺省。
        let plain = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
                DesktopSessionAuthorization::default(),
            )
            .unwrap_or_else(|error| panic!("plain open failed: {error}"));
        assert_eq!(
            plain.facts().authorization_persistence(),
            DesktopAuthorizationPersistenceState::default()
        );
        // 撤销入口：清除保存凭据并停止本 Module 的 live 会话。
        let report = module
            .forget_authorization()
            .unwrap_or_else(|error| panic!("forget authorization failed: {error}"));
        assert!(report.had_saved());
        assert!(report.cleared());
        assert_eq!(report.sessions_closed(), 3);
        assert_eq!(report.close_failures(), 0);
        assert!(module.sessions().is_empty());
        assert_eq!(
            module.saved_authorization().state(),
            DesktopSavedAuthorizationState::Absent
        );
    }

    #[test]
    fn ungranted_persistence_reports_honest_note() {
        let port = RememberingPort {
            grants_token: false,
            saved: std::sync::Mutex::new(None),
        };
        let mut module = DesktopSessionModule::new(port);
        let opened = module
            .open(
                true,
                true,
                IsolationRequirement::Standard,
                Duration::from_secs(120),
                DesktopSessionAuthorization {
                    scope: DesktopAuthorizationScope::Session,
                    remember: true,
                },
            )
            .unwrap_or_else(|error| panic!("open without granted persistence failed: {error}"));
        let retention = opened.facts().authorization_persistence();
        assert!(retention.requested);
        assert!(!retention.token_retained);
        assert_eq!(retention.note, Some("portal-did-not-grant-persistence"));
        assert_eq!(
            module.saved_authorization().state(),
            DesktopSavedAuthorizationState::Absent
        );
    }
}
