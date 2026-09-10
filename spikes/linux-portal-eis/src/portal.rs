//! 只验证标准 Portal 会话、EIS sender 握手与 PipeWire remote FD。

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
use reis::ei::{self, handshake::ContextType};
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

use crate::contract::{Failure, RunConfig, Verification};

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

/// 把内部错误连同已确认的 session 清理事实投影给 main。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RunFailure {
    pub(crate) failure: Failure,
    pub(crate) session_closed: bool,
}

impl RunFailure {
    const fn new(failure: Failure, session_closed: bool) -> Self {
        Self {
            failure,
            session_closed,
        }
    }
}

/// 为全部业务调用共享唯一 deadline。
struct Deadline {
    expires: Instant,
}

impl Deadline {
    fn new(timeout_ms: u32) -> Self {
        Self {
            expires: Instant::now() + Duration::from_millis(u64::from(timeout_ms)),
        }
    }

    fn remaining(&self, stage: &'static str) -> Result<Duration, Failure> {
        self.expires
            .checked_duration_since(Instant::now())
            .filter(|value| !value.is_zero())
            .ok_or_else(|| Failure::new("TIMEOUT", stage))
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
}

/// 对 Portal Request 的合法状态顺序执行显式门禁。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestState {
    Prepared,
    Subscribed,
    Requested,
    HandleVerified,
    Responded,
    Closed,
}

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

    fn transition(&mut self, expected: RequestState, next: RequestState) -> Result<(), Failure> {
        if self.state != expected {
            return Err(Failure::new("PORTAL_PROTOCOL_ERROR", self.stage));
        }
        self.state = next;
        Ok(())
    }

    fn subscribed(&mut self) -> Result<(), Failure> {
        self.transition(RequestState::Prepared, RequestState::Subscribed)
    }

    fn requested(&mut self) -> Result<(), Failure> {
        self.transition(RequestState::Subscribed, RequestState::Requested)
    }

    fn handle_verified(&mut self) -> Result<(), Failure> {
        self.transition(RequestState::Requested, RequestState::HandleVerified)
    }

    fn responded(&mut self) -> Result<(), Failure> {
        self.transition(RequestState::HandleVerified, RequestState::Responded)
    }

    fn closed(&mut self) -> Result<(), Failure> {
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
    ) -> Result<Self, Failure> {
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

    fn requested(&mut self) -> Result<(), Failure> {
        self.machine.requested()
    }

    async fn response<T>(
        mut self,
        connection: &zbus::Connection,
        returned_path: OwnedObjectPath,
        owner_changes: &mut MessageStream,
        deadline: &Deadline,
    ) -> Result<T, Failure>
    where
        T: for<'de> Deserialize<'de> + Type,
    {
        if returned_path != self.expected_path {
            close_request(connection, &returned_path).await;
            return Err(Failure::new("PORTAL_PROTOCOL_ERROR", self.machine.stage));
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
                    .ok_or_else(|| Failure::new("PORTAL_OWNER_DISCONNECTED", self.machine.stage))?
                    .map_err(|_| Failure::new("PORTAL_OWNER_DISCONNECTED", self.machine.stage))?;
                Ok::<WaitOutcome, Failure>(WaitOutcome::Response(message))
            },
            future::race(
                async {
                    wait_for_portal_owner_change(owner_changes, self.machine.stage).await?;
                    Ok::<WaitOutcome, Failure>(WaitOutcome::OwnerChanged)
                },
                async {
                    async_io::Timer::after(timeout).await;
                    Ok::<WaitOutcome, Failure>(WaitOutcome::Timeout)
                },
            ),
        )
        .await?;
        let message = match outcome {
            WaitOutcome::Response(message) => message,
            WaitOutcome::OwnerChanged => {
                return Err(Failure::new(
                    "PORTAL_OWNER_DISCONNECTED",
                    self.machine.stage,
                ));
            }
            WaitOutcome::Timeout => {
                close_request(connection, &self.expected_path).await;
                self.machine.closed()?;
                return Err(Failure::new("TIMEOUT", self.machine.stage));
            }
        };
        let (response, results) = message
            .body()
            .deserialize::<(u32, T)>()
            .map_err(|_| Failure::new("PORTAL_PROTOCOL_ERROR", self.machine.stage))?;
        self.machine.responded()?;
        match response {
            0 => Ok(results),
            1 => Err(Failure::new("PORTAL_CANCELLED", self.machine.stage)),
            2 => Err(Failure::new("PORTAL_DISMISSED", self.machine.stage)),
            _ => Err(Failure::new("PORTAL_PROTOCOL_ERROR", self.machine.stage)),
        }
    }
}

enum WaitOutcome {
    Response(Message),
    OwnerChanged,
    Timeout,
}

/// 在成功 close 前持有全部 session 资源，避免 FD drop 提前结束会话。
struct LiveEvidence {
    verification: Verification,
    _eis_connection: reis::event::Connection,
    _eis_events: reis::async_io::EiConvertEventStream,
    _pipe_wire_remote: OwnedFd,
}

/// 从 CreateSession 前开始监听 Closed，避免超时关闭时丢失清理证据。
struct SessionLease {
    path: OwnedObjectPath,
    closed_signals: MessageStream,
}

/// 执行一次真实、零输入、零像素的 L1 连通验证。
pub(crate) async fn verify(config: RunConfig) -> Result<Verification, RunFailure> {
    if let Err(failure) = verify_wayland_session() {
        return Err(RunFailure::new(failure, false));
    }
    let deadline = Deadline::new(config.timeout_ms);
    let connection = match connect_user_bus().await {
        Ok(connection) => connection,
        Err(failure) => return Err(RunFailure::new(failure, false)),
    };
    let sender = match connection.unique_name() {
        Some(sender) => sender.as_str().to_owned(),
        None => {
            return Err(RunFailure::new(
                Failure::new("PORTAL_PROTOCOL_ERROR", "connect"),
                false,
            ));
        }
    };
    let mut owner_changes = match portal_owner_stream(&connection).await {
        Ok(stream) => stream,
        Err(failure) => return Err(RunFailure::new(failure, false)),
    };
    if let Err(failure) = ensure_portal_owner(&connection).await {
        return Err(RunFailure::new(failure, false));
    }
    let remote = match portal_proxy(&connection, REMOTE_DESKTOP_INTERFACE, "preflight").await {
        Ok(proxy) => proxy,
        Err(failure) => return Err(RunFailure::new(failure, false)),
    };
    let screen_cast = match portal_proxy(&connection, SCREEN_CAST_INTERFACE, "preflight").await {
        Ok(proxy) => proxy,
        Err(failure) => return Err(RunFailure::new(failure, false)),
    };
    let (remote_desktop_version, screen_cast_version) =
        match portal_versions(&remote, &screen_cast).await {
            Ok(versions) => versions,
            Err(failure) => return Err(RunFailure::new(failure, false)),
        };
    if let Err(failure) = ensure_available_devices(&remote).await {
        return Err(RunFailure::new(failure, false));
    }

    let mut session =
        match create_session(&connection, &remote, &sender, &mut owner_changes, &deadline).await {
            Ok(session) => session,
            Err(failure) => return Err(RunFailure::new(failure, false)),
        };

    let live = establish_live_evidence(
        &connection,
        (&remote, &screen_cast),
        &sender,
        &session.path,
        &mut owner_changes,
        &deadline,
        (remote_desktop_version, screen_cast_version),
    )
    .await;
    let evidence = match live {
        Ok(evidence) => evidence,
        Err(failure) => {
            let session_closed = close_session(&connection, &mut session, &mut owner_changes).await;
            return Err(RunFailure::new(failure, session_closed));
        }
    };
    if !close_session(&connection, &mut session, &mut owner_changes).await {
        return Err(RunFailure::new(
            Failure::new("SESSION_CLOSE_UNCONFIRMED", "close-session"),
            false,
        ));
    }
    Ok(evidence.verification)
}

async fn establish_live_evidence(
    connection: &zbus::Connection,
    portals: (&Proxy<'_>, &Proxy<'_>),
    sender: &str,
    session: &OwnedObjectPath,
    owner_changes: &mut MessageStream,
    deadline: &Deadline,
    versions: (u32, u32),
) -> Result<LiveEvidence, Failure> {
    let (remote, screen_cast) = portals;
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
    let verification = project_start(start, versions.0, versions.1)?;
    let eis_fd = connect_to_eis(remote, session).await?;
    let context = ei::Context::new(UnixStream::from(OwnedFd::from(eis_fd)))
        .map_err(|_| Failure::new("EIS_HANDSHAKE_FAILED", "eis-handshake"))?;
    let timeout = deadline.remaining("eis-handshake")?;
    let handshake = future::race(
        async {
            context
                .handshake_async_io("ai-computer-toolkit-spike", ContextType::Sender)
                .await
                .map_err(|_| Failure::new("EIS_HANDSHAKE_FAILED", "eis-handshake"))
        },
        async {
            async_io::Timer::after(timeout).await;
            Err(Failure::new("TIMEOUT", "eis-handshake"))
        },
    )
    .await?;
    let pipe_wire_remote = open_pipe_wire_remote(screen_cast, session).await?;
    Ok(LiveEvidence {
        verification,
        _eis_connection: handshake.0,
        _eis_events: handshake.1,
        _pipe_wire_remote: OwnedFd::from(pipe_wire_remote),
    })
}

fn project_start(
    start: StartResponse,
    remote_desktop_version: u32,
    screen_cast_version: u32,
) -> Result<Verification, Failure> {
    if start.devices & REQUESTED_DEVICES != REQUESTED_DEVICES {
        return Err(Failure::new("REQUIRED_DEVICES_NOT_GRANTED", "start"));
    }
    if start.streams.is_empty() {
        return Err(Failure::new("SCREEN_CAST_STREAM_MISSING", "start"));
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
        return Err(Failure::new("MAPPING_ID_MISSING", "start"));
    }
    let _restore_token_was_discarded = start.restore_token.is_some();
    Ok(Verification {
        remote_desktop_version,
        screen_cast_version,
        authorized_device_classes: vec!["keyboard", "pointer"],
        stream_count: start.streams.len(),
        mapping_id_count,
    })
}

fn verify_wayland_session() -> Result<(), Failure> {
    if env::var("XDG_SESSION_TYPE").ok().as_deref() != Some("wayland") {
        return Err(Failure::new("WAYLAND_SESSION_REQUIRED", "preflight"));
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
        .ok_or_else(|| Failure::new("WAYLAND_SESSION_REQUIRED", "preflight"))?;
    let runtime = runtime_directory("preflight")?;
    let metadata = fs::metadata(runtime.join(display))
        .map_err(|_| Failure::new("WAYLAND_SESSION_REQUIRED", "preflight"))?;
    if !metadata.file_type().is_socket() {
        return Err(Failure::new("WAYLAND_SESSION_REQUIRED", "preflight"));
    }
    Ok(())
}

async fn connect_user_bus() -> Result<zbus::Connection, Failure> {
    let address = format!(
        "unix:path={}",
        runtime_directory("connect")?.join("bus").display()
    );
    ConnectionBuilder::address(address.as_str())
        .map_err(|_| Failure::new("USER_BUS_UNAVAILABLE", "connect"))?
        .method_timeout(Duration::from_secs(2))
        .build()
        .await
        .map_err(|_| Failure::new("USER_BUS_UNAVAILABLE", "connect"))
}

fn runtime_directory(stage: &'static str) -> Result<PathBuf, Failure> {
    let mut status = String::new();
    fs::File::open("/proc/self/status")
        .and_then(|mut file| file.read_to_string(&mut status))
        .map_err(|_| Failure::new("RUNTIME_IDENTITY_UNAVAILABLE", stage))?;
    let uid = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|value| value.split_whitespace().next())
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| Failure::new("RUNTIME_IDENTITY_UNAVAILABLE", stage))?;
    Ok(PathBuf::from(format!("/run/user/{uid}")))
}

async fn ensure_portal_owner(connection: &zbus::Connection) -> Result<(), Failure> {
    let dbus = Proxy::new(connection, DBUS_SERVICE, DBUS_OBJECT, DBUS_INTERFACE)
        .await
        .map_err(|_| Failure::new("PORTAL_UNAVAILABLE", "preflight"))?;
    let owner = dbus
        .call_with_flags::<_, _, bool>(
            "NameHasOwner",
            MethodFlags::NoAutoStart.into(),
            &(PORTAL_SERVICE),
        )
        .await
        .map_err(|_| Failure::new("PORTAL_UNAVAILABLE", "preflight"))?
        .ok_or_else(|| Failure::new("PORTAL_PROTOCOL_ERROR", "preflight"))?;
    if !owner {
        return Err(Failure::new("PORTAL_UNAVAILABLE", "preflight"));
    }
    Ok(())
}

async fn portal_proxy<'a>(
    connection: &'a zbus::Connection,
    interface: &'static str,
    stage: &'static str,
) -> Result<Proxy<'a>, Failure> {
    Proxy::new(connection, PORTAL_SERVICE, PORTAL_OBJECT, interface)
        .await
        .map_err(|_| Failure::new("PORTAL_UNAVAILABLE", stage))
}

async fn portal_versions(
    remote: &Proxy<'_>,
    screen_cast: &Proxy<'_>,
) -> Result<(u32, u32), Failure> {
    let remote_version = remote
        .get_property::<u32>("version")
        .await
        .map_err(|_| Failure::new("PORTAL_UNAVAILABLE", "preflight"))?;
    let screen_cast_version = screen_cast
        .get_property::<u32>("version")
        .await
        .map_err(|_| Failure::new("PORTAL_UNAVAILABLE", "preflight"))?;
    if remote_version < 2 || screen_cast_version < 5 {
        return Err(Failure::new("PORTAL_VERSION_UNSUPPORTED", "preflight"));
    }
    Ok((remote_version, screen_cast_version))
}

async fn ensure_available_devices(remote: &Proxy<'_>) -> Result<(), Failure> {
    let devices = remote
        .get_property::<u32>("AvailableDeviceTypes")
        .await
        .map_err(|_| Failure::new("PORTAL_UNAVAILABLE", "preflight"))?;
    if devices & REQUESTED_DEVICES != REQUESTED_DEVICES {
        return Err(Failure::new("REQUIRED_DEVICES_UNAVAILABLE", "preflight"));
    }
    Ok(())
}

async fn create_session(
    connection: &zbus::Connection,
    remote: &Proxy<'_>,
    sender: &str,
    owner_changes: &mut MessageStream,
    deadline: &Deadline,
) -> Result<SessionLease, Failure> {
    let session_token = request_token("create-session")?;
    let expected_session = session_path(sender, &session_token, "create-session")?;
    let closed_signals = signal_stream(
        connection,
        SESSION_INTERFACE,
        CLOSED_MEMBER,
        expected_session.as_str(),
        "create-session",
    )
    .await?;
    let mut pending = PendingRequest::prepare(connection, sender, "create-session").await?;
    let mut options = HashMap::<&str, ZValue<'_>>::new();
    options.insert("handle_token", ZValue::from(pending.token()));
    options.insert("session_handle_token", ZValue::from(session_token.as_str()));
    let returned = remote
        .call_with_flags::<_, _, OwnedObjectPath>(
            "CreateSession",
            MethodFlags::NoAutoStart.into(),
            &(options),
        )
        .await
        .map_err(|_| Failure::new("PORTAL_UNAVAILABLE", "create-session"))?
        .ok_or_else(|| Failure::new("PORTAL_PROTOCOL_ERROR", "create-session"))?;
    pending.requested()?;
    let mut results = pending
        .response::<HashMap<String, OwnedValue>>(connection, returned, owner_changes, deadline)
        .await?;
    if results.len() != 1 {
        return Err(Failure::new("PORTAL_PROTOCOL_ERROR", "create-session"));
    }
    let value = results
        .remove("session_handle")
        .ok_or_else(|| Failure::new("PORTAL_PROTOCOL_ERROR", "create-session"))?;
    let returned_session = parse_session_path(&value, "create-session")?;
    if returned_session != expected_session {
        return Err(Failure::new("PORTAL_PROTOCOL_ERROR", "create-session"));
    }
    Ok(SessionLease {
        path: returned_session,
        closed_signals,
    })
}

async fn select_devices(
    connection: &zbus::Connection,
    remote: &Proxy<'_>,
    sender: &str,
    session: &OwnedObjectPath,
    owner_changes: &mut MessageStream,
    deadline: &Deadline,
) -> Result<(), Failure> {
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
        .map_err(|_| Failure::new("PORTAL_UNAVAILABLE", "select-devices"))?
        .ok_or_else(|| Failure::new("PORTAL_PROTOCOL_ERROR", "select-devices"))?;
    pending.requested()?;
    let results = pending
        .response::<HashMap<String, OwnedValue>>(connection, returned, owner_changes, deadline)
        .await?;
    if !results.is_empty() {
        return Err(Failure::new("PORTAL_PROTOCOL_ERROR", "select-devices"));
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
) -> Result<(), Failure> {
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
        .map_err(|_| Failure::new("PORTAL_UNAVAILABLE", "select-sources"))?
        .ok_or_else(|| Failure::new("PORTAL_PROTOCOL_ERROR", "select-sources"))?;
    pending.requested()?;
    let results = pending
        .response::<HashMap<String, OwnedValue>>(connection, returned, owner_changes, deadline)
        .await?;
    if !results.is_empty() {
        return Err(Failure::new("PORTAL_PROTOCOL_ERROR", "select-sources"));
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
) -> Result<StartResponse, Failure> {
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
        .map_err(|_| Failure::new("PORTAL_UNAVAILABLE", "start"))?
        .ok_or_else(|| Failure::new("PORTAL_PROTOCOL_ERROR", "start"))?;
    pending.requested()?;
    pending
        .response::<StartResponse>(connection, returned, owner_changes, deadline)
        .await
}

async fn connect_to_eis(
    remote: &Proxy<'_>,
    session: &OwnedObjectPath,
) -> Result<ZOwnedFd, Failure> {
    let options = HashMap::<&str, ZValue<'_>>::new();
    remote
        .call_with_flags::<_, _, ZOwnedFd>(
            "ConnectToEIS",
            MethodFlags::NoAutoStart.into(),
            &(session, options),
        )
        .await
        .map_err(|_| Failure::new("EIS_CONNECTION_FAILED", "connect-to-eis"))?
        .ok_or_else(|| Failure::new("PORTAL_PROTOCOL_ERROR", "connect-to-eis"))
}

async fn open_pipe_wire_remote(
    screen_cast: &Proxy<'_>,
    session: &OwnedObjectPath,
) -> Result<ZOwnedFd, Failure> {
    let options = HashMap::<&str, ZValue<'_>>::new();
    screen_cast
        .call_with_flags::<_, _, ZOwnedFd>(
            "OpenPipeWireRemote",
            MethodFlags::NoAutoStart.into(),
            &(session, options),
        )
        .await
        .map_err(|_| Failure::new("PIPEWIRE_REMOTE_FAILED", "open-pipewire-remote"))?
        .ok_or_else(|| Failure::new("PORTAL_PROTOCOL_ERROR", "open-pipewire-remote"))
}

async fn close_session(
    connection: &zbus::Connection,
    session: &mut SessionLease,
    owner_changes: &mut MessageStream,
) -> bool {
    let call_succeeded = if let Ok(proxy) = Proxy::new(
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
    if session_close_confirmed(call_succeeded, false) {
        return true;
    }
    let asynchronous_close = future::race(
        async {
            let message = session.closed_signals.next().await;
            let Some(Ok(message)) = message else {
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
) -> Result<MessageStream, Failure> {
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface(interface)
        .map_err(|_| Failure::new("PORTAL_PROTOCOL_ERROR", stage))?
        .member(member)
        .map_err(|_| Failure::new("PORTAL_PROTOCOL_ERROR", stage))?
        .path(path)
        .map_err(|_| Failure::new("PORTAL_PROTOCOL_ERROR", stage))?
        .build();
    MessageStream::for_match_rule(rule, connection, Some(8))
        .await
        .map_err(|_| Failure::new("PORTAL_UNAVAILABLE", stage))
}

async fn portal_owner_stream(connection: &zbus::Connection) -> Result<MessageStream, Failure> {
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
) -> Result<(), Failure> {
    loop {
        let message = owner_changes
            .next()
            .await
            .ok_or_else(|| Failure::new("PORTAL_OWNER_DISCONNECTED", stage))?
            .map_err(|_| Failure::new("PORTAL_OWNER_DISCONNECTED", stage))?;
        let (name, old_owner, new_owner) = message
            .body()
            .deserialize::<(String, String, String)>()
            .map_err(|_| Failure::new("PORTAL_PROTOCOL_ERROR", stage))?;
        if name == PORTAL_SERVICE && old_owner != new_owner {
            return Ok(());
        }
    }
}

fn parse_session_path(value: &OwnedValue, stage: &'static str) -> Result<OwnedObjectPath, Failure> {
    let path = if let Ok(text) = value.downcast_ref::<&str>() {
        ObjectPath::try_from(text).map_err(|_| Failure::new("PORTAL_PROTOCOL_ERROR", stage))?
    } else if let Ok(path) = value.downcast_ref::<ObjectPath<'_>>() {
        path
    } else {
        return Err(Failure::new("PORTAL_PROTOCOL_ERROR", stage));
    };
    Ok(path.into())
}

fn request_token(stage: &'static str) -> Result<String, Failure> {
    let mut random = [0_u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|_| Failure::new("RANDOMNESS_UNAVAILABLE", stage))?;
    let sequence = NEXT_REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut token = format!("act{sequence:016x}");
    for byte in random {
        token.push_str(&format!("{byte:02x}"));
    }
    Ok(token)
}

fn sender_segment(sender: &str, stage: &'static str) -> Result<String, Failure> {
    sender
        .strip_prefix(':')
        .filter(|value| !value.is_empty())
        .map(|value| value.replace('.', "_"))
        .ok_or_else(|| Failure::new("PORTAL_PROTOCOL_ERROR", stage))
}

fn request_path(
    sender: &str,
    token: &str,
    stage: &'static str,
) -> Result<OwnedObjectPath, Failure> {
    OwnedObjectPath::try_from(format!(
        "/org/freedesktop/portal/desktop/request/{}/{token}",
        sender_segment(sender, stage)?
    ))
    .map_err(|_| Failure::new("PORTAL_PROTOCOL_ERROR", stage))
}

fn session_path(
    sender: &str,
    token: &str,
    stage: &'static str,
) -> Result<OwnedObjectPath, Failure> {
    OwnedObjectPath::try_from(format!(
        "/org/freedesktop/portal/desktop/session/{}/{token}",
        sender_segment(sender, stage)?
    ))
    .map_err(|_| Failure::new("PORTAL_PROTOCOL_ERROR", stage))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_state_rejects_out_of_order_completion() {
        let mut machine = RequestMachine::new("test");
        assert_eq!(
            machine.responded(),
            Err(Failure::new("PORTAL_PROTOCOL_ERROR", "test"))
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
                    },
                )],
                restore_token: None,
            },
            2,
            5,
        );
        assert_eq!(
            missing_pointer,
            Err(Failure::new("REQUIRED_DEVICES_NOT_GRANTED", "start"))
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
            Err(Failure::new("MAPPING_ID_MISSING", "start"))
        );
    }

    #[test]
    fn start_projection_discards_restore_token_and_node_identity() {
        let result = project_start(
            StartResponse {
                devices: REQUESTED_DEVICES,
                streams: vec![PortalStream(
                    9001,
                    PortalStreamProperties {
                        mapping_id: Some("private-mapping".to_owned()),
                    },
                )],
                restore_token: Some("discard-me".to_owned()),
            },
            2,
            5,
        );
        assert_eq!(
            result,
            Ok(Verification {
                remote_desktop_version: 2,
                screen_cast_version: 5,
                authorized_device_classes: vec!["keyboard", "pointer"],
                stream_count: 1,
                mapping_id_count: 1,
            })
        );
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
}
