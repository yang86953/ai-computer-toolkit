//! 通过标准 XDG Desktop Portal 建立持续存活的 Wayland 桌面授权 lease。

use std::{
    collections::HashMap,
    env, fs,
    io::Read,
    os::{fd::OwnedFd, unix::fs::FileTypeExt, unix::net::UnixStream},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use futures_lite::{StreamExt, future};
use serde::Deserialize;
use zbus::{
    MatchRule, Message, MessageStream, Proxy,
    connection::Builder as ConnectionBuilder,
    message::Type as MessageType,
    proxy::MethodFlags,
    zvariant::{
        ObjectPath, OwnedFd as ZOwnedFd, OwnedObjectPath, OwnedValue, Type, Value as ZValue,
        as_value::{self, optional},
    },
};

use crate::{
    adapters::linux::{
        desktop_frame_pipewire::{self, PipeWireStreamTarget},
        desktop_input_eis::DesktopEisInput,
        desktop_session_host_activity_logind::SystemLoginSessionMonitor,
    },
    components::{
        desktop_session_input_cancellation::DesktopInputCancellation,
        desktop_session_input_liveness::{
            DesktopSessionInputLiveness, DesktopSessionInputLivenessFailure,
        },
        desktop_session_pointer_input::DesktopPointerInput,
        keyboard_input_contract::KeyboardInput,
    },
    modules::desktop_session::{
        DesktopKeyboardDispatchFacts, DesktopPointerDispatchFacts, DesktopSessionFacts,
        DesktopSessionFrameFailure, DesktopSessionInputFailure, DesktopSessionLease,
        DesktopSessionPort, DesktopSessionPortFailure,
    },
};

const PORTAL_SERVICE: &str = "org.freedesktop.portal.Desktop";
const PORTAL_OBJECT: &str = "/org/freedesktop/portal/desktop";
const REMOTE_DESKTOP_INTERFACE: &str = "org.freedesktop.portal.RemoteDesktop";
const SCREEN_CAST_INTERFACE: &str = "org.freedesktop.portal.ScreenCast";
const REQUEST_INTERFACE: &str = "org.freedesktop.portal.Request";
const SESSION_INTERFACE: &str = "org.freedesktop.portal.Session";
const DBUS_SERVICE: &str = "org.freedesktop.DBus";
const DBUS_OBJECT: &str = "/org/freedesktop/DBus";
const DBUS_INTERFACE: &str = "org.freedesktop.DBus";
const RESPONSE_MEMBER: &str = "Response";
const CLOSED_MEMBER: &str = "Closed";
const DEVICE_KEYBOARD: u32 = 1;
const DEVICE_POINTER: u32 = 2;
const REQUESTED_DEVICES: u32 = DEVICE_KEYBOARD | DEVICE_POINTER;
const SOURCE_MONITOR: u32 = 1;
static NEXT_REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// 生产 Module 使用的无状态 Portal Adapter。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SystemDesktopSessionPortal;

impl DesktopSessionPort for SystemDesktopSessionPortal {
    fn open(
        &self,
        timeout: Duration,
    ) -> Result<(Box<dyn DesktopSessionLease>, DesktopSessionFacts), DesktopSessionPortFailure>
    {
        async_io::block_on(open_live_session(timeout))
            .map(|(lease, facts)| (Box::new(lease) as Box<dyn DesktopSessionLease>, facts))
    }
}

/// 原生资源只存在于 Adapter 内部，不进入 Module 或 JSON 边界。
struct PortalDesktopSessionLease {
    connection: zbus::Connection,
    session: SessionLease,
    owner_changes: MessageStream,
    live: LiveEvidence,
}

impl DesktopSessionLease for PortalDesktopSessionLease {
    fn frame_mapping(
        &mut self,
    ) -> Result<
        Option<crate::components::desktop_interaction::FrameMapping>,
        DesktopSessionInputFailure,
    > {
        let mut liveness = PortalInputLiveness {
            session: &mut self.session,
            owner_changes: &mut self.owner_changes,
            host_session: &mut self.live.host_session,
        };
        self.live.input.frame_mapping(&mut liveness)
    }

    fn send_frame_point(
        &mut self,
        point: &crate::components::desktop_interaction::FramePoint,
        timeout_ms: u32,
        cancellation: &DesktopInputCancellation,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
        let mut liveness = PortalInputLiveness {
            session: &mut self.session,
            owner_changes: &mut self.owner_changes,
            host_session: &mut self.live.host_session,
        };
        self.live
            .input
            .send_frame_point(point, timeout_ms, cancellation, &mut liveness)
    }
    fn finish_frame_points(&mut self) -> Result<(), DesktopSessionInputFailure> {
        self.live.input.finish_frame_points()
    }
    fn send_keyboard(
        &mut self,
        request: &KeyboardInput,
        cancellation: &DesktopInputCancellation,
    ) -> Result<DesktopKeyboardDispatchFacts, DesktopSessionInputFailure> {
        let Self {
            session,
            owner_changes,
            live,
            ..
        } = self;
        let mut liveness = PortalInputLiveness {
            session,
            owner_changes,
            host_session: &mut live.host_session,
        };
        live.input
            .send_keyboard(request, cancellation, &mut liveness)
    }

    fn send_pointer(
        &mut self,
        request: &DesktopPointerInput,
        cancellation: &DesktopInputCancellation,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
        let Self {
            session,
            owner_changes,
            live,
            ..
        } = self;
        let mut liveness = PortalInputLiveness {
            session,
            owner_changes,
            host_session: &mut live.host_session,
        };
        live.input
            .send_pointer(request, cancellation, &mut liveness)
    }

    fn capture_frame(
        &mut self,
        timeout: Duration,
        max_dimension: Option<u32>,
    ) -> Result<
        crate::components::desktop_session_frame_capture::DesktopCapturedFrame,
        DesktopSessionFrameFailure,
    > {
        let Self {
            connection,
            session,
            owner_changes,
            live,
        } = self;
        {
            let mut liveness = PortalInputLiveness {
                session,
                owner_changes,
                host_session: &mut live.host_session,
            };
            liveness.poll_input_allowed().map_err(|failure| {
                DesktopSessionFrameFailure::new(failure.code(), failure.stage(), false, true)
            })?;
        }
        let remote = match live.pipe_wire_remote.take() {
            Some(remote) => remote,
            None => async_io::block_on(async {
                let screen_cast =
                    portal_proxy(connection, SCREEN_CAST_INTERFACE, "capture-frame").await?;
                open_pipe_wire_remote(&screen_cast, &session.path).await
            })
            .map(OwnedFd::from)
            .map_err(|failure| {
                DesktopSessionFrameFailure::new(failure.code, failure.stage, false, false)
            })?,
        };
        let frame = desktop_frame_pipewire::capture_frame(
            remote,
            live.stream_target,
            timeout,
            max_dimension,
        )
        .map_err(|failure| {
            DesktopSessionFrameFailure::new(
                failure.code(),
                failure.stage(),
                failure.pixels_may_have_been_consumed(),
                false,
            )
        })?;
        let mut liveness = PortalInputLiveness {
            session,
            owner_changes,
            host_session: &mut live.host_session,
        };
        liveness.poll_input_allowed().map_err(|failure| {
            DesktopSessionFrameFailure::new(failure.code(), failure.stage(), true, true)
        })?;
        Ok(frame)
    }

    fn subscription_stats(&self) -> (u64, u64) {
        let (frames, pixels) = self.live.subscription.as_ref().map(|s| s.stats()).unwrap_or((0, 0));
        (self.live.subscription_totals.0.saturating_add(frames), self.live.subscription_totals.1.saturating_add(pixels))
    }
    fn subscribe_frames(&mut self, id: &str, lifetime: Duration) -> Result<(), DesktopSessionFrameFailure> {
        if self.live.subscription.is_some() {
            return Err(DesktopSessionFrameFailure::new("SUBSCRIPTION_ALREADY_ACTIVE", "subscribe-frames", false, false));
        }
        let mut liveness = PortalInputLiveness {session: &mut self.session, owner_changes: &mut self.owner_changes, host_session: &mut self.live.host_session};
        liveness.poll_input_allowed().map_err(|e| DesktopSessionFrameFailure::new(e.code(), e.stage(), false, true))?;
        let monitor = async_io::block_on(SystemLoginSessionMonitor::open())
            .map_err(|e| DesktopSessionFrameFailure::new(e.code(), e.stage(), false, true))?;
        if !monitor.same_session(&self.live.host_session) {
            return Err(DesktopSessionFrameFailure::new("HOST_SESSION_UNAVAILABLE", "subscribe-host-generation", false, true));
        }
        let remote = match self.live.pipe_wire_remote.take() {
            Some(remote) => remote,
            None => async_io::block_on(async {
                let screen_cast = portal_proxy(&self.connection, SCREEN_CAST_INTERFACE, "subscribe-frames").await?;
                open_pipe_wire_remote(&screen_cast, &self.session.path).await
            }).map(OwnedFd::from).map_err(|e| DesktopSessionFrameFailure::new(e.code, e.stage, false, false))?,
        };
        self.live.subscription = Some(super::desktop_frame_subscription::PipeWireSubscription::start(
            id, remote, self.live.stream_target, lifetime, monitor,
        ).map_err(|code| DesktopSessionFrameFailure::new(code, "subscribe-worker", false, false))?);
        Ok(())
    }

    fn next_frame_update(&mut self, id: &str, after: u64, wait: Duration) -> Result<Option<crate::components::desktop_frame_stream::FrameUpdate>, DesktopSessionFrameFailure> {
        {
            let mut liveness = PortalInputLiveness {session: &mut self.session, owner_changes: &mut self.owner_changes, host_session: &mut self.live.host_session};
            liveness.poll_input_allowed().map_err(|e| DesktopSessionFrameFailure::new(e.code(), e.stage(), true, true))?;
        }
        let result = self.live.subscription.as_ref().ok_or_else(|| DesktopSessionFrameFailure::new("STALE_SUBSCRIPTION", "next-frame", false, false))?
            .next(id, after, wait).map_err(|code| DesktopSessionFrameFailure::new(code, "subscription-next", true, false))?;
        let mut liveness = PortalInputLiveness {session: &mut self.session, owner_changes: &mut self.owner_changes, host_session: &mut self.live.host_session};
        liveness.poll_input_allowed().map_err(|e| DesktopSessionFrameFailure::new(e.code(), e.stage(), true, true))?;
        Ok(result)
    }

    fn unsubscribe_frames(&mut self, id: &str) -> Result<(), DesktopSessionFrameFailure> {
        if !self.live.subscription.as_ref().is_some_and(|s| s.matches(id)) {
            return Err(DesktopSessionFrameFailure::new("STALE_SUBSCRIPTION", "unsubscribe-frames", false, false));
        }
        if let Some(subscription) = self.live.subscription.take() {
            let (frames, pixels) = subscription.stop();
            self.live.subscription_totals.0 = self.live.subscription_totals.0.saturating_add(frames);
            self.live.subscription_totals.1 = self.live.subscription_totals.1.saturating_add(pixels);
        }
        Ok(())
    }

    fn close(self: Box<Self>) -> Result<(), DesktopSessionPortFailure> {
        let Self {
            connection,
            mut session,
            mut owner_changes,
            live: mut _live,
        } = *self;
        drop(_live.subscription.take());
        if async_io::block_on(close_session(&connection, &mut session, &mut owner_changes)) {
            Ok(())
        } else {
            Err(DesktopSessionPortFailure::after_session(
                "SESSION_CLOSE_UNCONFIRMED",
                "close-session",
                false,
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PortalFailure {
    code: &'static str,
    stage: &'static str,
}

impl PortalFailure {
    const fn new(code: &'static str, stage: &'static str) -> Self {
        Self { code, stage }
    }

    const fn before_session(self) -> DesktopSessionPortFailure {
        DesktopSessionPortFailure::before_session(self.code, self.stage)
    }

    const fn after_session(self, cleanup_confirmed: bool) -> DesktopSessionPortFailure {
        DesktopSessionPortFailure::after_session(self.code, self.stage, cleanup_confirmed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CreateSessionFailure {
    failure: PortalFailure,
    accepted_may_have_occurred: bool,
    cleanup_confirmed: bool,
}

impl CreateSessionFailure {
    const fn before_dispatch(failure: PortalFailure) -> Self {
        Self {
            failure,
            accepted_may_have_occurred: false,
            cleanup_confirmed: false,
        }
    }

    const fn after_dispatch(failure: PortalFailure, cleanup_confirmed: bool) -> Self {
        Self {
            failure,
            accepted_may_have_occurred: true,
            cleanup_confirmed,
        }
    }

    const fn into_port_failure(self) -> DesktopSessionPortFailure {
        if self.accepted_may_have_occurred {
            self.failure.after_session(self.cleanup_confirmed)
        } else {
            self.failure.before_session()
        }
    }
}

/// 为打开序列的全部 Portal 调用共享唯一 deadline。
struct Deadline {
    expires: Instant,
}

impl Deadline {
    fn new(timeout: Duration) -> Self {
        Self {
            expires: Instant::now() + timeout,
        }
    }

    fn remaining(&self, stage: &'static str) -> Result<Duration, PortalFailure> {
        self.expires
            .checked_duration_since(Instant::now())
            .filter(|value| !value.is_zero())
            .ok_or_else(|| PortalFailure::new("TIMEOUT", stage))
    }
}

/// Portal Start 响应中只读取 L1 门禁所需字段。
#[derive(Debug, Default, Deserialize, Type)]
#[zvariant(signature = "dict")]
struct StartResponse {
    #[serde(default, with = "as_value")]
    devices: u32,
    #[serde(default, with = "as_value")]
    streams: Vec<PortalStream>,
    #[serde(default, with = "optional")]
    restore_token: Option<String>,
}

/// PipeWire node ID 只为完整反序列化而保留，绝不进入公开结果。
#[derive(Clone, Debug, Deserialize, Type)]
struct PortalStream(u32, PortalStreamProperties);

/// 只保存跨 EIS/ScreenCast 对齐所需的 mapping 存在性。
#[derive(Clone, Debug, Default, Deserialize, Type)]
#[zvariant(signature = "dict")]
struct PortalStreamProperties {
    #[serde(default, with = "optional")]
    mapping_id: Option<String>,
    #[serde(default, rename = "pipewire-serial", with = "optional")]
    pipewire_serial: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestState {
    Prepared,
    Subscribed,
    Requested,
    HandleVerified,
    Responded,
    Closed,
}

/// 对 Portal Request 的合法状态顺序执行显式门禁。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RequestMachine {
    state: RequestState,
    stage: &'static str,
}

impl RequestMachine {
    const fn new(stage: &'static str) -> Self {
        Self {
            state: RequestState::Prepared,
            stage,
        }
    }

    fn transition(
        &mut self,
        expected: RequestState,
        next: RequestState,
    ) -> Result<(), PortalFailure> {
        if self.state != expected {
            return Err(PortalFailure::new("PORTAL_PROTOCOL_ERROR", self.stage));
        }
        self.state = next;
        Ok(())
    }

    fn subscribed(&mut self) -> Result<(), PortalFailure> {
        self.transition(RequestState::Prepared, RequestState::Subscribed)
    }

    fn requested(&mut self) -> Result<(), PortalFailure> {
        self.transition(RequestState::Subscribed, RequestState::Requested)
    }

    fn handle_verified(&mut self) -> Result<(), PortalFailure> {
        self.transition(RequestState::Requested, RequestState::HandleVerified)
    }

    fn responded(&mut self) -> Result<(), PortalFailure> {
        self.transition(RequestState::HandleVerified, RequestState::Responded)
    }

    fn closed(&mut self) -> Result<(), PortalFailure> {
        self.transition(RequestState::HandleVerified, RequestState::Closed)
    }
}

/// 保存已预订阅 Response 的单个 Portal Request。
struct PendingRequest {
    token: String,
    expected_path: OwnedObjectPath,
    responses: MessageStream,
    machine: RequestMachine,
}

impl PendingRequest {
    async fn prepare(
        connection: &zbus::Connection,
        sender: &str,
        stage: &'static str,
    ) -> Result<Self, PortalFailure> {
        let token = request_token(stage)?;
        let expected_path = request_path(sender, &token, stage)?;
        let responses = signal_stream(
            connection,
            REQUEST_INTERFACE,
            RESPONSE_MEMBER,
            expected_path.as_str(),
            stage,
        )
        .await?;
        let mut machine = RequestMachine::new(stage);
        machine.subscribed()?;
        Ok(Self {
            token,
            expected_path,
            responses,
            machine,
        })
    }

    fn token(&self) -> &str {
        &self.token
    }

    fn requested(&mut self) -> Result<(), PortalFailure> {
        self.machine.requested()
    }

    async fn response<T>(
        mut self,
        connection: &zbus::Connection,
        returned_path: OwnedObjectPath,
        owner_changes: &mut MessageStream,
        deadline: &Deadline,
    ) -> Result<T, PortalFailure>
    where
        T: for<'de> Deserialize<'de> + Type,
    {
        if returned_path != self.expected_path {
            close_request(connection, &returned_path).await;
            return Err(PortalFailure::new(
                "PORTAL_PROTOCOL_ERROR",
                self.machine.stage,
            ));
        }
        self.machine.handle_verified()?;
        let timeout = match deadline.remaining(self.machine.stage) {
            Ok(timeout) => timeout,
            Err(failure) => {
                close_request(connection, &self.expected_path).await;
                self.machine.closed()?;
                return Err(failure);
            }
        };
        let outcome = future::race(
            async {
                let message = self
                    .responses
                    .next()
                    .await
                    .ok_or_else(|| {
                        PortalFailure::new("PORTAL_OWNER_DISCONNECTED", self.machine.stage)
                    })?
                    .map_err(|_| {
                        PortalFailure::new("PORTAL_OWNER_DISCONNECTED", self.machine.stage)
                    })?;
                Ok::<WaitOutcome, PortalFailure>(WaitOutcome::Response(message))
            },
            future::race(
                async {
                    wait_for_portal_owner_change(owner_changes, self.machine.stage).await?;
                    Ok::<WaitOutcome, PortalFailure>(WaitOutcome::OwnerChanged)
                },
                async {
                    async_io::Timer::after(timeout).await;
                    Ok::<WaitOutcome, PortalFailure>(WaitOutcome::Timeout)
                },
            ),
        )
        .await?;
        let message = match outcome {
            WaitOutcome::Response(message) => message,
            WaitOutcome::OwnerChanged => {
                return Err(PortalFailure::new(
                    "PORTAL_OWNER_DISCONNECTED",
                    self.machine.stage,
                ));
            }
            WaitOutcome::Timeout => {
                close_request(connection, &self.expected_path).await;
                self.machine.closed()?;
                return Err(PortalFailure::new("TIMEOUT", self.machine.stage));
            }
        };
        let (response, results) = message
            .body()
            .deserialize::<(u32, T)>()
            .map_err(|_| PortalFailure::new("PORTAL_PROTOCOL_ERROR", self.machine.stage))?;
        self.machine.responded()?;
        match response {
            0 => Ok(results),
            1 => Err(PortalFailure::new("PORTAL_CANCELLED", self.machine.stage)),
            2 => Err(PortalFailure::new("PORTAL_DISMISSED", self.machine.stage)),
            _ => Err(PortalFailure::new(
                "PORTAL_PROTOCOL_ERROR",
                self.machine.stage,
            )),
        }
    }
}

enum WaitOutcome {
    Response(Message),
    OwnerChanged,
    Timeout,
}

/// 组合 Portal 生命周期与宿主活动，但不让 EIS 认识任何 D-Bus 身份。
struct PortalInputLiveness<'a> {
    session: &'a mut SessionLease,
    owner_changes: &'a mut MessageStream,
    host_session: &'a mut SystemLoginSessionMonitor,
}

impl DesktopSessionInputLiveness for PortalInputLiveness<'_> {
    fn poll_input_allowed(&mut self) -> Result<(), DesktopSessionInputLivenessFailure> {
        match poll_message(&mut self.session.closed_signals) {
            Ok(Some(_)) => {
                self.session.closed_confirmed = true;
                return Err(DesktopSessionInputLivenessFailure::new(
                    "PORTAL_SESSION_CLOSED",
                    "portal-session-liveness",
                ));
            }
            Ok(None) => {}
            Err(()) => {
                return Err(DesktopSessionInputLivenessFailure::new(
                    "PORTAL_SESSION_STATE_UNAVAILABLE",
                    "portal-session-liveness",
                ));
            }
        }
        match portal_owner_changed(self.owner_changes) {
            Ok(true) => {
                self.session.closed_confirmed = true;
                return Err(DesktopSessionInputLivenessFailure::new(
                    "PORTAL_OWNER_DISCONNECTED",
                    "portal-session-liveness",
                ));
            }
            Ok(false) => {}
            Err(()) => {
                return Err(DesktopSessionInputLivenessFailure::new(
                    "PORTAL_SESSION_STATE_UNAVAILABLE",
                    "portal-session-liveness",
                ));
            }
        }
        self.host_session.poll()
    }
}

fn poll_message(stream: &mut MessageStream) -> Result<Option<Message>, ()> {
    match async_io::block_on(future::poll_once(stream.next())) {
        None => Ok(None),
        Some(Some(Ok(message))) => Ok(Some(message)),
        Some(_) => Err(()),
    }
}

fn portal_owner_changed(stream: &mut MessageStream) -> Result<bool, ()> {
    loop {
        let Some(message) = poll_message(stream)? else {
            return Ok(false);
        };
        let (name, old_owner, new_owner) = message
            .body()
            .deserialize::<(String, String, String)>()
            .map_err(|_| ())?;
        if name == PORTAL_SERVICE && old_owner != new_owner {
            return Ok(true);
        }
    }
}

/// 在成功 close 前持有全部 live 原生资源，避免 FD drop 提前结束会话。
struct LiveEvidence {
    subscription: Option<super::desktop_frame_subscription::PipeWireSubscription>,
    subscription_totals: (u64, u64),
    facts: DesktopSessionFacts,
    input: DesktopEisInput,
    host_session: SystemLoginSessionMonitor,
    pipe_wire_remote: Option<OwnedFd>,
    stream_target: PipeWireStreamTarget,
}

/// 聚合建立 live 证据所需的私有 Portal 与宿主依赖。
struct LiveEvidenceDependencies<'proxy, 'connection> {
    remote: &'proxy Proxy<'connection>,
    screen_cast: &'proxy Proxy<'connection>,
    host_session: SystemLoginSessionMonitor,
}

/// 从 CreateSession 前开始监听 Closed，避免主动关闭时丢失清理证据。
struct SessionLease {
    path: OwnedObjectPath,
    closed_signals: MessageStream,
    closed_confirmed: bool,
}

/// 完成真实零输入、零像素的打开序列，并保留 live lease。
async fn open_live_session(
    timeout: Duration,
) -> Result<(PortalDesktopSessionLease, DesktopSessionFacts), DesktopSessionPortFailure> {
    verify_wayland_session().map_err(PortalFailure::before_session)?;
    let deadline = Deadline::new(timeout);
    let connection = connect_user_bus()
        .await
        .map_err(PortalFailure::before_session)?;
    let sender = connection
        .unique_name()
        .map(|name| name.as_str().to_owned())
        .ok_or_else(|| PortalFailure::new("PORTAL_PROTOCOL_ERROR", "connect").before_session())?;
    let mut owner_changes = portal_owner_stream(&connection)
        .await
        .map_err(PortalFailure::before_session)?;
    ensure_portal_owner(&connection)
        .await
        .map_err(PortalFailure::before_session)?;
    let host_session = SystemLoginSessionMonitor::open()
        .await
        .map_err(|failure| PortalFailure::new(failure.code(), failure.stage()).before_session())?;

    let (session, live) = {
        let remote = portal_proxy(&connection, REMOTE_DESKTOP_INTERFACE, "preflight")
            .await
            .map_err(PortalFailure::before_session)?;
        let screen_cast = portal_proxy(&connection, SCREEN_CAST_INTERFACE, "preflight")
            .await
            .map_err(PortalFailure::before_session)?;
        let versions = portal_versions(&remote, &screen_cast)
            .await
            .map_err(PortalFailure::before_session)?;
        ensure_available_devices(&remote)
            .await
            .map_err(PortalFailure::before_session)?;
        let mut session =
            create_session(&connection, &remote, &sender, &mut owner_changes, &deadline)
                .await
                .map_err(CreateSessionFailure::into_port_failure)?;
        let live = establish_live_evidence(
            &connection,
            LiveEvidenceDependencies {
                remote: &remote,
                screen_cast: &screen_cast,
                host_session,
            },
            &sender,
            &session.path,
            &mut owner_changes,
            &deadline,
            versions,
        )
        .await;
        let live = match live {
            Ok(live) => live,
            Err(failure) => {
                let cleanup_confirmed =
                    close_session(&connection, &mut session, &mut owner_changes).await;
                return Err(failure.after_session(cleanup_confirmed));
            }
        };
        (session, live)
    };
    let facts = live.facts.clone();
    Ok((
        PortalDesktopSessionLease {
            connection,
            session,
            owner_changes,
            live,
        },
        facts,
    ))
}

async fn establish_live_evidence(
    connection: &zbus::Connection,
    dependencies: LiveEvidenceDependencies<'_, '_>,
    sender: &str,
    session: &OwnedObjectPath,
    owner_changes: &mut MessageStream,
    deadline: &Deadline,
    versions: (u32, u32),
) -> Result<LiveEvidence, PortalFailure> {
    let LiveEvidenceDependencies {
        remote,
        screen_cast,
        host_session,
    } = dependencies;
    select_devices(connection, remote, sender, session, owner_changes, deadline).await?;
    select_sources(
        connection,
        screen_cast,
        sender,
        session,
        owner_changes,
        deadline,
    )
    .await?;
    let start = start_session(connection, remote, sender, session, owner_changes, deadline).await?;
    let projection = project_start(start, versions.0, versions.1)?;
    let eis_fd = connect_to_eis(remote, session).await?;
    let input_timeout = deadline.remaining("eis-handshake")?;
    let mut input =
        DesktopEisInput::connect(UnixStream::from(OwnedFd::from(eis_fd)), input_timeout)
            .await
            .map_err(|failure| PortalFailure::new(failure.code(), failure.stage()))?;
    input.set_capture_mapping(projection.capture_mapping_id);
    let pipe_wire_remote = open_pipe_wire_remote(screen_cast, session).await?;
    Ok(LiveEvidence {
        subscription: None,
        subscription_totals: (0, 0),
        facts: projection.facts,
        input,
        host_session,
        pipe_wire_remote: Some(OwnedFd::from(pipe_wire_remote)),
        stream_target: projection.stream_target,
    })
}

#[derive(Debug, Eq, PartialEq)]
struct StartProjection {
    facts: DesktopSessionFacts,
    stream_target: PipeWireStreamTarget,
    capture_mapping_id: Option<String>,
}

fn project_start(
    start: StartResponse,
    remote_desktop_version: u32,
    screen_cast_version: u32,
) -> Result<StartProjection, PortalFailure> {
    if start.devices & REQUESTED_DEVICES != REQUESTED_DEVICES {
        return Err(PortalFailure::new("REQUIRED_DEVICES_NOT_GRANTED", "start"));
    }
    if start.streams.is_empty() {
        return Err(PortalFailure::new("SCREEN_CAST_STREAM_MISSING", "start"));
    }
    let mapping_id_count = start
        .streams
        .iter()
        .filter(|stream| {
            let _private_node_id = stream.0;
            stream
                .1
                .mapping_id
                .as_deref()
                .is_some_and(|value| !value.is_empty())
        })
        .count();
    if mapping_id_count == 0 {
        return Err(PortalFailure::new("MAPPING_ID_MISSING", "start"));
    }
    let _restore_token_was_discarded = start.restore_token.is_some();
    let first_stream = start
        .streams
        .first()
        .ok_or_else(|| PortalFailure::new("SCREEN_CAST_STREAM_MISSING", "start"))?;
    let stream_target = if screen_cast_version >= 6 {
        first_stream
            .1
            .pipewire_serial
            .map(PipeWireStreamTarget::Serial)
            .ok_or_else(|| PortalFailure::new("PIPEWIRE_STREAM_TARGET_MISSING", "start"))?
    } else {
        PipeWireStreamTarget::NodeId(first_stream.0)
    };
    Ok(StartProjection {
        capture_mapping_id: first_stream.1.mapping_id.clone(),
        facts: DesktopSessionFacts::new(
            remote_desktop_version,
            screen_cast_version,
            vec!["keyboard", "pointer"],
            start.streams.len(),
            mapping_id_count,
        ),
        stream_target,
    })
}

fn verify_wayland_session() -> Result<(), PortalFailure> {
    if env::var("XDG_SESSION_TYPE").ok().as_deref() != Some("wayland") {
        return Err(PortalFailure::new("WAYLAND_SESSION_REQUIRED", "preflight"));
    }
    let display = env::var("WAYLAND_DISPLAY")
        .ok()
        .filter(|value| {
            !value.is_empty()
                && !value.contains('/')
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .ok_or_else(|| PortalFailure::new("WAYLAND_SESSION_REQUIRED", "preflight"))?;
    let runtime = runtime_directory("preflight")?;
    let metadata = fs::metadata(runtime.join(display))
        .map_err(|_| PortalFailure::new("WAYLAND_SESSION_REQUIRED", "preflight"))?;
    if !metadata.file_type().is_socket() {
        return Err(PortalFailure::new("WAYLAND_SESSION_REQUIRED", "preflight"));
    }
    Ok(())
}

async fn connect_user_bus() -> Result<zbus::Connection, PortalFailure> {
    let address = format!(
        "unix:path={}",
        runtime_directory("connect")?.join("bus").display()
    );
    ConnectionBuilder::address(address.as_str())
        .map_err(|_| PortalFailure::new("USER_BUS_UNAVAILABLE", "connect"))?
        .method_timeout(Duration::from_secs(2))
        .build()
        .await
        .map_err(|_| PortalFailure::new("USER_BUS_UNAVAILABLE", "connect"))
}

fn runtime_directory(stage: &'static str) -> Result<PathBuf, PortalFailure> {
    let mut status = String::new();
    fs::File::open("/proc/self/status")
        .and_then(|mut file| file.read_to_string(&mut status))
        .map_err(|_| PortalFailure::new("RUNTIME_IDENTITY_UNAVAILABLE", stage))?;
    let uid = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|value| value.split_whitespace().next())
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| PortalFailure::new("RUNTIME_IDENTITY_UNAVAILABLE", stage))?;
    Ok(PathBuf::from(format!("/run/user/{uid}")))
}

async fn ensure_portal_owner(connection: &zbus::Connection) -> Result<(), PortalFailure> {
    let dbus = Proxy::new(connection, DBUS_SERVICE, DBUS_OBJECT, DBUS_INTERFACE)
        .await
        .map_err(|_| PortalFailure::new("PORTAL_UNAVAILABLE", "preflight"))?;
    let owner = dbus
        .call_with_flags::<_, _, bool>(
            "NameHasOwner",
            MethodFlags::NoAutoStart.into(),
            &(PORTAL_SERVICE),
        )
        .await
        .map_err(|_| PortalFailure::new("PORTAL_UNAVAILABLE", "preflight"))?
        .ok_or_else(|| PortalFailure::new("PORTAL_PROTOCOL_ERROR", "preflight"))?;
    if !owner {
        return Err(PortalFailure::new("PORTAL_UNAVAILABLE", "preflight"));
    }
    Ok(())
}

async fn portal_proxy<'a>(
    connection: &'a zbus::Connection,
    interface: &'static str,
    stage: &'static str,
) -> Result<Proxy<'a>, PortalFailure> {
    Proxy::new(connection, PORTAL_SERVICE, PORTAL_OBJECT, interface)
        .await
        .map_err(|_| PortalFailure::new("PORTAL_UNAVAILABLE", stage))
}

async fn portal_versions(
    remote: &Proxy<'_>,
    screen_cast: &Proxy<'_>,
) -> Result<(u32, u32), PortalFailure> {
    let remote_version = remote
        .get_property::<u32>("version")
        .await
        .map_err(|_| PortalFailure::new("PORTAL_UNAVAILABLE", "preflight"))?;
    let screen_cast_version = screen_cast
        .get_property::<u32>("version")
        .await
        .map_err(|_| PortalFailure::new("PORTAL_UNAVAILABLE", "preflight"))?;
    if remote_version < 2 || screen_cast_version < 5 {
        return Err(PortalFailure::new(
            "PORTAL_VERSION_UNSUPPORTED",
            "preflight",
        ));
    }
    Ok((remote_version, screen_cast_version))
}

async fn ensure_available_devices(remote: &Proxy<'_>) -> Result<(), PortalFailure> {
    let devices = remote
        .get_property::<u32>("AvailableDeviceTypes")
        .await
        .map_err(|_| PortalFailure::new("PORTAL_UNAVAILABLE", "preflight"))?;
    if devices & REQUESTED_DEVICES != REQUESTED_DEVICES {
        return Err(PortalFailure::new(
            "REQUIRED_DEVICES_UNAVAILABLE",
            "preflight",
        ));
    }
    Ok(())
}

async fn create_session(
    connection: &zbus::Connection,
    remote: &Proxy<'_>,
    sender: &str,
    owner_changes: &mut MessageStream,
    deadline: &Deadline,
) -> Result<SessionLease, CreateSessionFailure> {
    let session_token =
        request_token("create-session").map_err(CreateSessionFailure::before_dispatch)?;
    let expected_session = session_path(sender, &session_token, "create-session")
        .map_err(CreateSessionFailure::before_dispatch)?;
    let closed_signals = signal_stream(
        connection,
        SESSION_INTERFACE,
        CLOSED_MEMBER,
        expected_session.as_str(),
        "create-session",
    )
    .await
    .map_err(CreateSessionFailure::before_dispatch)?;
    let mut pending = PendingRequest::prepare(connection, sender, "create-session")
        .await
        .map_err(CreateSessionFailure::before_dispatch)?;
    let mut lease = SessionLease {
        path: expected_session.clone(),
        closed_signals,
        closed_confirmed: false,
    };
    let handle_token = pending.token().to_owned();
    let mut options = HashMap::<&str, ZValue<'_>>::new();
    options.insert("handle_token", ZValue::from(handle_token.as_str()));
    options.insert("session_handle_token", ZValue::from(session_token.as_str()));
    let dispatched = async {
        let returned = remote
            .call_with_flags::<_, _, OwnedObjectPath>(
                "CreateSession",
                MethodFlags::NoAutoStart.into(),
                &(options),
            )
            .await
            .map_err(|_| PortalFailure::new("PORTAL_UNAVAILABLE", "create-session"))?
            .ok_or_else(|| PortalFailure::new("PORTAL_PROTOCOL_ERROR", "create-session"))?;
        pending.requested()?;
        let mut results = pending
            .response::<HashMap<String, OwnedValue>>(connection, returned, owner_changes, deadline)
            .await?;
        if results.len() != 1 {
            return Err(PortalFailure::new(
                "PORTAL_PROTOCOL_ERROR",
                "create-session",
            ));
        }
        let value = results
            .remove("session_handle")
            .ok_or_else(|| PortalFailure::new("PORTAL_PROTOCOL_ERROR", "create-session"))?;
        let returned_session = parse_session_path(&value, "create-session")?;
        if returned_session != expected_session {
            return Err(PortalFailure::new(
                "PORTAL_PROTOCOL_ERROR",
                "create-session",
            ));
        }
        Ok(())
    }
    .await;
    match dispatched {
        Ok(()) => Ok(lease),
        Err(failure) => {
            let cleanup_confirmed = close_session(connection, &mut lease, owner_changes).await;
            Err(CreateSessionFailure::after_dispatch(
                failure,
                cleanup_confirmed,
            ))
        }
    }
}

async fn select_devices(
    connection: &zbus::Connection,
    remote: &Proxy<'_>,
    sender: &str,
    session: &OwnedObjectPath,
    owner_changes: &mut MessageStream,
    deadline: &Deadline,
) -> Result<(), PortalFailure> {
    let mut pending = PendingRequest::prepare(connection, sender, "select-devices").await?;
    let mut options = HashMap::<&str, ZValue<'_>>::new();
    options.insert("handle_token", ZValue::from(pending.token()));
    options.insert("types", ZValue::from(REQUESTED_DEVICES));
    let returned = remote
        .call_with_flags::<_, _, OwnedObjectPath>(
            "SelectDevices",
            MethodFlags::NoAutoStart.into(),
            &(session, options),
        )
        .await
        .map_err(|_| PortalFailure::new("PORTAL_UNAVAILABLE", "select-devices"))?
        .ok_or_else(|| PortalFailure::new("PORTAL_PROTOCOL_ERROR", "select-devices"))?;
    pending.requested()?;
    let results = pending
        .response::<HashMap<String, OwnedValue>>(connection, returned, owner_changes, deadline)
        .await?;
    if !results.is_empty() {
        return Err(PortalFailure::new(
            "PORTAL_PROTOCOL_ERROR",
            "select-devices",
        ));
    }
    Ok(())
}

async fn select_sources(
    connection: &zbus::Connection,
    screen_cast: &Proxy<'_>,
    sender: &str,
    session: &OwnedObjectPath,
    owner_changes: &mut MessageStream,
    deadline: &Deadline,
) -> Result<(), PortalFailure> {
    let mut pending = PendingRequest::prepare(connection, sender, "select-sources").await?;
    let mut options = HashMap::<&str, ZValue<'_>>::new();
    options.insert("handle_token", ZValue::from(pending.token()));
    options.insert("types", ZValue::from(SOURCE_MONITOR));
    options.insert("multiple", ZValue::from(false));
    let returned = screen_cast
        .call_with_flags::<_, _, OwnedObjectPath>(
            "SelectSources",
            MethodFlags::NoAutoStart.into(),
            &(session, options),
        )
        .await
        .map_err(|_| PortalFailure::new("PORTAL_UNAVAILABLE", "select-sources"))?
        .ok_or_else(|| PortalFailure::new("PORTAL_PROTOCOL_ERROR", "select-sources"))?;
    pending.requested()?;
    let results = pending
        .response::<HashMap<String, OwnedValue>>(connection, returned, owner_changes, deadline)
        .await?;
    if !results.is_empty() {
        return Err(PortalFailure::new(
            "PORTAL_PROTOCOL_ERROR",
            "select-sources",
        ));
    }
    Ok(())
}

async fn start_session(
    connection: &zbus::Connection,
    remote: &Proxy<'_>,
    sender: &str,
    session: &OwnedObjectPath,
    owner_changes: &mut MessageStream,
    deadline: &Deadline,
) -> Result<StartResponse, PortalFailure> {
    let mut pending = PendingRequest::prepare(connection, sender, "start").await?;
    let mut options = HashMap::<&str, ZValue<'_>>::new();
    options.insert("handle_token", ZValue::from(pending.token()));
    let returned = remote
        .call_with_flags::<_, _, OwnedObjectPath>(
            "Start",
            MethodFlags::NoAutoStart.into(),
            &(session, "", options),
        )
        .await
        .map_err(|_| PortalFailure::new("PORTAL_UNAVAILABLE", "start"))?
        .ok_or_else(|| PortalFailure::new("PORTAL_PROTOCOL_ERROR", "start"))?;
    pending.requested()?;
    pending
        .response::<StartResponse>(connection, returned, owner_changes, deadline)
        .await
}

async fn connect_to_eis(
    remote: &Proxy<'_>,
    session: &OwnedObjectPath,
) -> Result<ZOwnedFd, PortalFailure> {
    let options = HashMap::<&str, ZValue<'_>>::new();
    remote
        .call_with_flags::<_, _, ZOwnedFd>(
            "ConnectToEIS",
            MethodFlags::NoAutoStart.into(),
            &(session, options),
        )
        .await
        .map_err(|_| PortalFailure::new("EIS_CONNECTION_FAILED", "connect-to-eis"))?
        .ok_or_else(|| PortalFailure::new("PORTAL_PROTOCOL_ERROR", "connect-to-eis"))
}

async fn open_pipe_wire_remote(
    screen_cast: &Proxy<'_>,
    session: &OwnedObjectPath,
) -> Result<ZOwnedFd, PortalFailure> {
    let options = HashMap::<&str, ZValue<'_>>::new();
    screen_cast
        .call_with_flags::<_, _, ZOwnedFd>(
            "OpenPipeWireRemote",
            MethodFlags::NoAutoStart.into(),
            &(session, options),
        )
        .await
        .map_err(|_| PortalFailure::new("PIPEWIRE_REMOTE_FAILED", "open-pipewire-remote"))?
        .ok_or_else(|| PortalFailure::new("PORTAL_PROTOCOL_ERROR", "open-pipewire-remote"))
}

async fn close_session(
    connection: &zbus::Connection,
    session: &mut SessionLease,
    owner_changes: &mut MessageStream,
) -> bool {
    if session.closed_confirmed {
        return true;
    }
    let method_reply = if let Ok(proxy) = Proxy::new(
        connection,
        PORTAL_SERVICE,
        session.path.as_str(),
        SESSION_INTERFACE,
    )
    .await
    {
        matches!(
            proxy
                .call_with_flags::<_, _, ()>("Close", MethodFlags::NoAutoStart.into(), &())
                .await,
            Ok(Some(()))
        )
    } else {
        false
    };
    if session_close_confirmed(method_reply, false) {
        return true;
    }
    let asynchronous_close = future::race(
        async {
            let Some(Ok(message)) = session.closed_signals.next().await else {
                return false;
            };
            message
                .body()
                .deserialize::<(HashMap<String, OwnedValue>,)>()
                .is_ok()
        },
        future::race(
            async {
                wait_for_portal_owner_change(owner_changes, "close-session")
                    .await
                    .is_ok()
            },
            async {
                async_io::Timer::after(Duration::from_secs(1)).await;
                false
            },
        ),
    )
    .await;
    session_close_confirmed(false, asynchronous_close)
}

/// 主动 Close 的成功回复与后端主动 Closed/owner 变化都是有效清理证据。
const fn session_close_confirmed(method_reply: bool, asynchronous_close: bool) -> bool {
    method_reply || asynchronous_close
}

async fn close_request(connection: &zbus::Connection, request: &OwnedObjectPath) {
    let Ok(proxy) = Proxy::new(
        connection,
        PORTAL_SERVICE,
        request.as_str(),
        REQUEST_INTERFACE,
    )
    .await
    else {
        return;
    };
    let _ = proxy
        .call_with_flags::<_, _, ()>("Close", MethodFlags::NoAutoStart.into(), &())
        .await;
}

async fn signal_stream(
    connection: &zbus::Connection,
    interface: &'static str,
    member: &'static str,
    path: &str,
    stage: &'static str,
) -> Result<MessageStream, PortalFailure> {
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface(interface)
        .map_err(|_| PortalFailure::new("PORTAL_PROTOCOL_ERROR", stage))?
        .member(member)
        .map_err(|_| PortalFailure::new("PORTAL_PROTOCOL_ERROR", stage))?
        .path(path)
        .map_err(|_| PortalFailure::new("PORTAL_PROTOCOL_ERROR", stage))?
        .build();
    MessageStream::for_match_rule(rule, connection, Some(8))
        .await
        .map_err(|_| PortalFailure::new("PORTAL_UNAVAILABLE", stage))
}

async fn portal_owner_stream(
    connection: &zbus::Connection,
) -> Result<MessageStream, PortalFailure> {
    signal_stream(
        connection,
        DBUS_INTERFACE,
        "NameOwnerChanged",
        DBUS_OBJECT,
        "preflight",
    )
    .await
}

async fn wait_for_portal_owner_change(
    owner_changes: &mut MessageStream,
    stage: &'static str,
) -> Result<(), PortalFailure> {
    loop {
        let message = owner_changes
            .next()
            .await
            .ok_or_else(|| PortalFailure::new("PORTAL_OWNER_DISCONNECTED", stage))?
            .map_err(|_| PortalFailure::new("PORTAL_OWNER_DISCONNECTED", stage))?;
        let (name, old_owner, new_owner) = message
            .body()
            .deserialize::<(String, String, String)>()
            .map_err(|_| PortalFailure::new("PORTAL_PROTOCOL_ERROR", stage))?;
        if name == PORTAL_SERVICE && old_owner != new_owner {
            return Ok(());
        }
    }
}

fn parse_session_path(
    value: &OwnedValue,
    stage: &'static str,
) -> Result<OwnedObjectPath, PortalFailure> {
    let path = if let Ok(text) = value.downcast_ref::<&str>() {
        ObjectPath::try_from(text)
            .map_err(|_| PortalFailure::new("PORTAL_PROTOCOL_ERROR", stage))?
    } else if let Ok(path) = value.downcast_ref::<ObjectPath<'_>>() {
        path
    } else {
        return Err(PortalFailure::new("PORTAL_PROTOCOL_ERROR", stage));
    };
    Ok(path.into())
}

fn request_token(stage: &'static str) -> Result<String, PortalFailure> {
    let mut random = [0_u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|_| PortalFailure::new("RANDOMNESS_UNAVAILABLE", stage))?;
    let sequence = NEXT_REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut token = format!("act{sequence:016x}");
    for byte in random {
        token.push_str(&format!("{byte:02x}"));
    }
    Ok(token)
}

fn sender_segment(sender: &str, stage: &'static str) -> Result<String, PortalFailure> {
    sender
        .strip_prefix(':')
        .filter(|value| !value.is_empty())
        .map(|value| value.replace('.', "_"))
        .ok_or_else(|| PortalFailure::new("PORTAL_PROTOCOL_ERROR", stage))
}

fn request_path(
    sender: &str,
    token: &str,
    stage: &'static str,
) -> Result<OwnedObjectPath, PortalFailure> {
    OwnedObjectPath::try_from(format!(
        "/org/freedesktop/portal/desktop/request/{}/{token}",
        sender_segment(sender, stage)?
    ))
    .map_err(|_| PortalFailure::new("PORTAL_PROTOCOL_ERROR", stage))
}

fn session_path(
    sender: &str,
    token: &str,
    stage: &'static str,
) -> Result<OwnedObjectPath, PortalFailure> {
    OwnedObjectPath::try_from(format!(
        "/org/freedesktop/portal/desktop/session/{}/{token}",
        sender_segment(sender, stage)?
    ))
    .map_err(|_| PortalFailure::new("PORTAL_PROTOCOL_ERROR", stage))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_state_rejects_out_of_order_completion() {
        let mut machine = RequestMachine::new("test");
        assert_eq!(
            machine.responded(),
            Err(PortalFailure::new("PORTAL_PROTOCOL_ERROR", "test"))
        );
        assert_eq!(machine.subscribed(), Ok(()));
        assert_eq!(machine.requested(), Ok(()));
        assert_eq!(machine.handle_verified(), Ok(()));
        assert_eq!(machine.responded(), Ok(()));
    }

    #[test]
    fn start_projection_requires_both_devices_and_mapping() {
        let missing_pointer = project_start(
            StartResponse {
                devices: DEVICE_KEYBOARD,
                streams: vec![PortalStream(
                    42,
                    PortalStreamProperties {
                        mapping_id: Some("private".to_owned()),
                        pipewire_serial: None,
                    },
                )],
                restore_token: None,
            },
            2,
            5,
        );
        assert_eq!(
            missing_pointer,
            Err(PortalFailure::new("REQUIRED_DEVICES_NOT_GRANTED", "start"))
        );
        let missing_mapping = project_start(
            StartResponse {
                devices: REQUESTED_DEVICES,
                streams: vec![PortalStream(42, PortalStreamProperties::default())],
                restore_token: None,
            },
            2,
            5,
        );
        assert_eq!(
            missing_mapping,
            Err(PortalFailure::new("MAPPING_ID_MISSING", "start"))
        );
    }

    #[test]
    fn start_projection_discards_restore_token_and_native_identities() {
        let result = project_start(
            StartResponse {
                devices: REQUESTED_DEVICES,
                streams: vec![PortalStream(
                    9001,
                    PortalStreamProperties {
                        mapping_id: Some("private-mapping".to_owned()),
                        pipewire_serial: None,
                    },
                )],
                restore_token: Some("discard-me".to_owned()),
            },
            2,
            5,
        )
        .unwrap_or_else(|failure| panic!("start projection failed: {failure:?}"));
        assert_eq!(result.facts.remote_desktop_version(), 2);
        assert_eq!(result.facts.screen_cast_version(), 5);
        assert_eq!(
            result.facts.authorized_device_classes(),
            &["keyboard", "pointer"]
        );
        assert_eq!(result.facts.stream_count(), 1);
        assert_eq!(result.facts.mapping_id_count(), 1);
        assert_eq!(result.stream_target, PipeWireStreamTarget::NodeId(9001));
    }

    #[test]
    fn screen_cast_v6_requires_the_private_pipewire_serial() {
        let projection = project_start(
            StartResponse {
                devices: REQUESTED_DEVICES,
                streams: vec![PortalStream(
                    9001,
                    PortalStreamProperties {
                        mapping_id: Some("private-mapping".to_owned()),
                        pipewire_serial: Some(42),
                    },
                )],
                restore_token: None,
            },
            2,
            6,
        )
        .unwrap_or_else(|failure| panic!("v6 start projection failed: {failure:?}"));
        assert_eq!(projection.stream_target, PipeWireStreamTarget::Serial(42));

        let Err(missing_serial) = project_start(
            StartResponse {
                devices: REQUESTED_DEVICES,
                streams: vec![PortalStream(
                    9001,
                    PortalStreamProperties {
                        mapping_id: Some("private-mapping".to_owned()),
                        pipewire_serial: None,
                    },
                )],
                restore_token: None,
            },
            2,
            6,
        ) else {
            panic!("ScreenCast v6 缺少 serial 必须失败闭合");
        };
        assert_eq!(missing_serial.code, "PIPEWIRE_STREAM_TARGET_MISSING");
        assert_eq!(missing_serial.stage, "start");
    }

    #[test]
    fn opaque_paths_are_derived_from_unique_sender_and_random_token() {
        let path = request_path(":1.42", "act0001", "test");
        assert_eq!(
            path.as_ref().map(|value| value.as_str()),
            Ok("/org/freedesktop/portal/desktop/request/1_42/act0001")
        );
    }

    #[test]
    fn successful_close_reply_confirms_client_initiated_cleanup() {
        assert!(session_close_confirmed(true, false));
        assert!(session_close_confirmed(false, true));
        assert!(!session_close_confirmed(false, false));
    }

    #[test]
    fn create_failure_preserves_dispatch_and_cleanup_truth() {
        let before = CreateSessionFailure::before_dispatch(PortalFailure::new(
            "RANDOMNESS_UNAVAILABLE",
            "create-session",
        ))
        .into_port_failure();
        assert!(!before.accepted_may_have_occurred());
        assert!(!before.cleanup_confirmed());
        let cleaned = CreateSessionFailure::after_dispatch(
            PortalFailure::new("TIMEOUT", "create-session"),
            true,
        )
        .into_port_failure();
        assert!(cleaned.accepted_may_have_occurred());
        assert!(cleaned.cleanup_confirmed());
    }
}
