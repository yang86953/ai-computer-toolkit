//! Wayland 与 Desktop Portal 的无副作用就绪度探针 Component。

use std::{
    collections::HashMap,
    fs,
    io::Read,
    os::unix::fs::FileTypeExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use futures_lite::{StreamExt, future};
use zbus::{
    MatchRule, MessageStream, Proxy,
    connection::Builder as ConnectionBuilder,
    message::Type as MessageType,
    proxy::MethodFlags,
    zvariant::{OwnedObjectPath, OwnedValue, Value as ZValue},
};

const BUSCTL: &str = "/usr/bin/busctl";
const PORTAL_SERVICE: &str = "org.freedesktop.portal.Desktop";
const PORTAL_OBJECT: &str = "/org/freedesktop/portal/desktop";
const SCREENSHOT_INTERFACE: &str = "org.freedesktop.portal.Screenshot";
const REQUEST_INTERFACE: &str = "org.freedesktop.portal.Request";
const RESPONSE_MEMBER: &str = "Response";
static NEXT_REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// 只包含 provider-neutral 可用性事实，不携带 D-Bus 或 compositor 私有类型。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WaylandPortalFacts {
    pub(crate) wayland_socket_count: usize,
    pub(crate) user_bus_socket_present: bool,
    pub(crate) user_bus_reachable: bool,
    pub(crate) portal_service_registered: bool,
    pub(crate) screen_cast_version: Option<u32>,
    pub(crate) remote_desktop_version: Option<u32>,
    pub(crate) screenshot_version: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QueryOutput {
    success: bool,
    stdout: String,
}

trait ProbeSource {
    fn runtime_socket_names(&self) -> Vec<String>;
    fn user_bus_socket_present(&self) -> bool;
    fn busctl(&self, arguments: &[&str]) -> Option<QueryOutput>;
}

struct SystemProbeSource;

impl SystemProbeSource {
    fn user_id() -> Option<String> {
        // `/proc/self/status` 是当前进程自己的内核快照，不读取其他用户会话。
        let status = fs::read_to_string("/proc/self/status").ok()?;
        let uid = status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:").map(str::trim))?
            .split_whitespace()
            .next()?;
        // 为固定运行时目录只保留 ASCII 数字，拒绝路径材料注入。
        if uid.is_empty() || !uid.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        Some(uid.to_owned())
    }

    fn runtime_directory() -> Option<PathBuf> {
        Self::user_id().map(|uid| PathBuf::from(format!("/run/user/{uid}")))
    }
}

impl ProbeSource for SystemProbeSource {
    fn runtime_socket_names(&self) -> Vec<String> {
        let Some(directory) = Self::runtime_directory() else {
            return Vec::new();
        };
        let Ok(entries) = fs::read_dir(directory) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_str()?.to_owned();
                let suffix = name.strip_prefix("wayland-")?;
                if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                let file_type = entry.file_type().ok()?;
                file_type.is_socket().then_some(name)
            })
            .collect()
    }

    fn user_bus_socket_present(&self) -> bool {
        Self::runtime_directory()
            .map(|directory| directory.join("bus"))
            .and_then(|path| fs::metadata(path).ok())
            .is_some_and(|metadata| metadata.file_type().is_socket())
    }

    fn busctl(&self, arguments: &[&str]) -> Option<QueryOutput> {
        // 固定绝对路径、固定参数且不经过 shell；调用方不能注入 service、object 或 interface。
        let bus = Self::runtime_directory()?.join("bus");
        let output = Command::new(BUSCTL)
            // 禁止继承环境变量把探针重定向到其他总线或加载额外配置。
            .env_clear()
            // 地址只由当前内核 UID 的固定运行时目录生成。
            .arg(format!("--address=unix:path={}", bus.display()))
            .args(arguments)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        Some(QueryOutput {
            success: output.status.success(),
            stdout: String::from_utf8(output.stdout).ok()?,
        })
    }
}

fn parse_version(output: Option<QueryOutput>) -> Option<u32> {
    let output = output.filter(|output| output.success)?;
    let mut fields = output.stdout.split_whitespace();
    match (fields.next(), fields.next(), fields.next()) {
        (Some("u"), Some(version), None) => version.parse().ok(),
        _ => None,
    }
}

fn portal_version(source: &impl ProbeSource, interface: &str) -> Option<u32> {
    parse_version(source.busctl(&[
        "--auto-start=no",
        "--timeout=1s",
        "get-property",
        PORTAL_SERVICE,
        PORTAL_OBJECT,
        interface,
        "version",
    ]))
}

fn probe_with(source: &impl ProbeSource) -> WaylandPortalFacts {
    let wayland_socket_count = source.runtime_socket_names().len();
    let names = source.busctl(&[
        "--auto-start=no",
        "--timeout=1s",
        "--no-pager",
        "--no-legend",
        "list",
    ]);
    let user_bus_reachable = names.as_ref().is_some_and(|output| output.success);
    let portal_service_registered = names.as_ref().is_some_and(|output| {
        output
            .stdout
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .any(|name| name == PORTAL_SERVICE)
    });
    let (screen_cast_version, remote_desktop_version, screenshot_version) =
        if portal_service_registered {
            (
                portal_version(source, "org.freedesktop.portal.ScreenCast"),
                portal_version(source, "org.freedesktop.portal.RemoteDesktop"),
                portal_version(source, "org.freedesktop.portal.Screenshot"),
            )
        } else {
            (None, None, None)
        };
    WaylandPortalFacts {
        wayland_socket_count,
        user_bus_socket_present: source.user_bus_socket_present(),
        user_bus_reachable,
        portal_service_registered,
        screen_cast_version,
        remote_desktop_version,
        screenshot_version,
    }
}

/// 探测标准运行时套接字与已注册 Portal 接口；不连接 Wayland、不创建 Portal session。
pub(crate) fn probe() -> WaylandPortalFacts {
    probe_with(&SystemProbeSource)
}

/// Portal Screenshot Adapter 向 Module 返回的中立制品引用。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PortalScreenshotArtifact {
    pub(crate) uri: String,
    pub(crate) interface_version: u32,
}

/// Portal Screenshot Adapter 的封闭失败类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PortalScreenshotFailure {
    Unavailable,
    ProtocolViolation,
    OwnerDisconnected,
    Cancelled,
    Dismissed,
    Timeout,
}

/// Portal 请求协议的封闭状态；只描述交互顺序，不携带 D-Bus 对象。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PortalRequestState {
    Prepared,
    Connected,
    Subscribed,
    Requested,
    HandleVerified,
    Responded,
    Aborted,
    Closed,
    Completed,
}

/// 对生产与 fixture 共用的 Portal 请求状态迁移执行严格门禁。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PortalRequestMachine {
    state: PortalRequestState,
}

impl PortalRequestMachine {
    fn new() -> Self {
        Self {
            state: PortalRequestState::Prepared,
        }
    }

    fn transition(
        &mut self,
        expected: PortalRequestState,
        next: PortalRequestState,
    ) -> Result<(), PortalScreenshotFailure> {
        if self.state != expected {
            return Err(PortalScreenshotFailure::ProtocolViolation);
        }
        self.state = next;
        Ok(())
    }

    fn connected(&mut self) -> Result<(), PortalScreenshotFailure> {
        self.transition(PortalRequestState::Prepared, PortalRequestState::Connected)
    }

    fn subscribed(&mut self) -> Result<(), PortalScreenshotFailure> {
        self.transition(
            PortalRequestState::Connected,
            PortalRequestState::Subscribed,
        )
    }

    fn requested(&mut self) -> Result<(), PortalScreenshotFailure> {
        self.transition(
            PortalRequestState::Subscribed,
            PortalRequestState::Requested,
        )
    }

    fn handle_verified(&mut self) -> Result<(), PortalScreenshotFailure> {
        self.transition(
            PortalRequestState::Requested,
            PortalRequestState::HandleVerified,
        )
    }

    fn responded(&mut self) -> Result<(), PortalScreenshotFailure> {
        self.transition(
            PortalRequestState::HandleVerified,
            PortalRequestState::Responded,
        )
    }

    fn closed(&mut self) -> Result<(), PortalScreenshotFailure> {
        self.transition(
            PortalRequestState::HandleVerified,
            PortalRequestState::Closed,
        )
    }

    fn aborted(&mut self) -> Result<(), PortalScreenshotFailure> {
        self.transition(
            PortalRequestState::HandleVerified,
            PortalRequestState::Aborted,
        )
    }

    fn completed(&mut self) -> Result<(), PortalScreenshotFailure> {
        self.transition(PortalRequestState::Responded, PortalRequestState::Completed)
    }
}

fn request_token() -> Result<String, PortalScreenshotFailure> {
    let mut random = [0_u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|_| PortalScreenshotFailure::Unavailable)?;
    let sequence = NEXT_REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut token = format!("act{sequence:016x}");
    for byte in random {
        token.push_str(&format!("{byte:02x}"));
    }
    Ok(token)
}

fn request_path(sender: &str, token: &str) -> Result<OwnedObjectPath, PortalScreenshotFailure> {
    let sender = sender
        .strip_prefix(':')
        .ok_or(PortalScreenshotFailure::ProtocolViolation)?
        .replace('.', "_");
    OwnedObjectPath::try_from(format!(
        "/org/freedesktop/portal/desktop/request/{sender}/{token}"
    ))
    .map_err(|_| PortalScreenshotFailure::ProtocolViolation)
}

async fn response_stream(
    connection: &zbus::Connection,
    path: &OwnedObjectPath,
) -> Result<MessageStream, PortalScreenshotFailure> {
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface(REQUEST_INTERFACE)
        .map_err(|_| PortalScreenshotFailure::ProtocolViolation)?
        .member(RESPONSE_MEMBER)
        .map_err(|_| PortalScreenshotFailure::ProtocolViolation)?
        .path(path.as_str())
        .map_err(|_| PortalScreenshotFailure::ProtocolViolation)?
        .build();
    MessageStream::for_match_rule(rule, connection, Some(4))
        .await
        .map_err(|_| PortalScreenshotFailure::Unavailable)
}

fn response_artifact(
    response: u32,
    results: HashMap<String, OwnedValue>,
    interface_version: u32,
) -> Result<PortalScreenshotArtifact, PortalScreenshotFailure> {
    match response {
        0 => {
            let uri = results
                .get("uri")
                .cloned()
                .and_then(|value| String::try_from(value).ok())
                .filter(|value| !value.is_empty())
                .ok_or(PortalScreenshotFailure::ProtocolViolation)?;
            Ok(PortalScreenshotArtifact {
                uri,
                interface_version,
            })
        }
        1 => Err(PortalScreenshotFailure::Cancelled),
        2 => Err(PortalScreenshotFailure::Dismissed),
        _ => Err(PortalScreenshotFailure::ProtocolViolation),
    }
}

async fn close_request(connection: &zbus::Connection, path: &OwnedObjectPath) {
    let Ok(proxy) = Proxy::new(connection, PORTAL_SERVICE, path.as_str(), REQUEST_INTERFACE).await
    else {
        return;
    };
    let _ = proxy
        .call_with_flags::<_, _, ()>("Close", MethodFlags::NoAutoStart.into(), &())
        .await;
}

async fn screenshot_async(
    timeout: Duration,
) -> Result<PortalScreenshotArtifact, PortalScreenshotFailure> {
    let runtime =
        SystemProbeSource::runtime_directory().ok_or(PortalScreenshotFailure::Unavailable)?;
    let address = format!("unix:path={}", runtime.join("bus").display());
    let mut machine = PortalRequestMachine::new();
    let connection = ConnectionBuilder::address(address.as_str())
        .map_err(|_| PortalScreenshotFailure::Unavailable)?
        .method_timeout(Duration::from_secs(2))
        .build()
        .await
        .map_err(|_| PortalScreenshotFailure::Unavailable)?;
    machine.connected()?;

    let sender = connection
        .unique_name()
        .ok_or(PortalScreenshotFailure::ProtocolViolation)?;
    let token = request_token()?;
    let expected_path = request_path(sender.as_str(), &token)?;
    let mut stream = response_stream(&connection, &expected_path).await?;
    machine.subscribed()?;

    let screenshot = Proxy::new(
        &connection,
        PORTAL_SERVICE,
        PORTAL_OBJECT,
        SCREENSHOT_INTERFACE,
    )
    .await
    .map_err(|_| PortalScreenshotFailure::Unavailable)?;
    let interface_version = screenshot
        .get_property::<u32>("version")
        .await
        .map_err(|_| PortalScreenshotFailure::Unavailable)?;
    if interface_version < 2 {
        return Err(PortalScreenshotFailure::Unavailable);
    }
    // Portal owner 换代会使既有 Request 失去可信所有者，必须与 timeout 分开闭合。
    let mut owner_changes = screenshot
        .receive_owner_changed()
        .await
        .map_err(|_| PortalScreenshotFailure::Unavailable)?;
    let mut options = HashMap::<&str, ZValue<'_>>::new();
    options.insert("handle_token", ZValue::from(token.as_str()));
    options.insert("modal", ZValue::from(true));
    options.insert("interactive", ZValue::from(true));
    let returned_path = screenshot
        .call_with_flags::<_, _, OwnedObjectPath>(
            "Screenshot",
            MethodFlags::NoAutoStart.into(),
            &("", options),
        )
        .await
        .map_err(|_| PortalScreenshotFailure::Unavailable)?
        .ok_or(PortalScreenshotFailure::ProtocolViolation)?;
    machine.requested()?;

    let active_path = if returned_path == expected_path {
        expected_path
    } else {
        stream = response_stream(&connection, &returned_path).await?;
        returned_path
    };
    machine.handle_verified()?;

    let response = future::race(
        async {
            let message = stream
                .next()
                .await
                .ok_or(PortalScreenshotFailure::OwnerDisconnected)?
                .map_err(|_| PortalScreenshotFailure::OwnerDisconnected)?;
            message
                .body()
                .deserialize::<(u32, HashMap<String, OwnedValue>)>()
                .map_err(|_| PortalScreenshotFailure::ProtocolViolation)
        },
        future::race(
            async {
                let _ = owner_changes.next().await;
                Err(PortalScreenshotFailure::OwnerDisconnected)
            },
            async {
                async_io::Timer::after(timeout).await;
                Err(PortalScreenshotFailure::Timeout)
            },
        ),
    )
    .await;
    let (response, results) = match response {
        Ok(response) => response,
        Err(PortalScreenshotFailure::Timeout) => {
            close_request(&connection, &active_path).await;
            machine.closed()?;
            return Err(PortalScreenshotFailure::Timeout);
        }
        Err(PortalScreenshotFailure::OwnerDisconnected) => {
            machine.aborted()?;
            return Err(PortalScreenshotFailure::OwnerDisconnected);
        }
        Err(error) => return Err(error),
    };
    machine.responded()?;
    let artifact = response_artifact(response, results, interface_version)?;
    machine.completed()?;
    Ok(artifact)
}

/// 发起一次明确授权后的交互式 Screenshot Portal 请求。
pub(crate) fn screenshot(
    timeout: Duration,
) -> Result<PortalScreenshotArtifact, PortalScreenshotFailure> {
    async_io::block_on(screenshot_async(timeout))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use zbus::zvariant::{OwnedValue, Str};

    use super::{
        PortalRequestMachine, PortalRequestState, PortalScreenshotFailure, ProbeSource,
        QueryOutput, probe_with, response_artifact,
    };

    struct FixtureProbeSource;

    impl ProbeSource for FixtureProbeSource {
        fn runtime_socket_names(&self) -> Vec<String> {
            vec!["wayland-0".to_owned()]
        }

        fn user_bus_socket_present(&self) -> bool {
            true
        }

        fn busctl(&self, arguments: &[&str]) -> Option<QueryOutput> {
            let stdout = if arguments.last() == Some(&"list") {
                "org.freedesktop.portal.Desktop 42 portal fixture\n"
            } else {
                match arguments.get(arguments.len().saturating_sub(2)).copied() {
                    Some("org.freedesktop.portal.ScreenCast") => "u 5\n",
                    Some("org.freedesktop.portal.RemoteDesktop") => "u 2\n",
                    Some("org.freedesktop.portal.Screenshot") => "u 2\n",
                    _ => return None,
                }
            };
            Some(QueryOutput {
                success: true,
                stdout: stdout.to_owned(),
            })
        }
    }

    #[test]
    fn fixture_reports_wayland_and_portal_interfaces_without_requests() {
        let facts = probe_with(&FixtureProbeSource);
        assert_eq!(facts.wayland_socket_count, 1);
        assert!(facts.user_bus_socket_present);
        assert!(facts.user_bus_reachable);
        assert!(facts.portal_service_registered);
        assert_eq!(facts.screen_cast_version, Some(5));
        assert_eq!(facts.remote_desktop_version, Some(2));
        assert_eq!(facts.screenshot_version, Some(2));
    }

    #[test]
    fn fixture_protocol_state_machine_requires_subscribe_before_request() {
        let mut machine = PortalRequestMachine::new();
        assert_eq!(
            machine.requested(),
            Err(PortalScreenshotFailure::ProtocolViolation)
        );
        assert_eq!(machine.state, PortalRequestState::Prepared);

        machine
            .connected()
            .unwrap_or_else(|error| panic!("connect fixture: {error:?}"));
        machine
            .subscribed()
            .unwrap_or_else(|error| panic!("subscribe fixture: {error:?}"));
        machine
            .requested()
            .unwrap_or_else(|error| panic!("request fixture: {error:?}"));
        machine
            .handle_verified()
            .unwrap_or_else(|error| panic!("handle fixture: {error:?}"));
        machine
            .responded()
            .unwrap_or_else(|error| panic!("response fixture: {error:?}"));
        machine
            .completed()
            .unwrap_or_else(|error| panic!("complete fixture: {error:?}"));
        assert_eq!(machine.state, PortalRequestState::Completed);

        for (terminal, expected) in [
            (
                PortalRequestMachine::closed
                    as fn(&mut PortalRequestMachine) -> Result<(), PortalScreenshotFailure>,
                PortalRequestState::Closed,
            ),
            (PortalRequestMachine::aborted, PortalRequestState::Aborted),
        ] {
            let mut terminal_machine = PortalRequestMachine::new();
            terminal_machine
                .connected()
                .and_then(|_| terminal_machine.subscribed())
                .and_then(|_| terminal_machine.requested())
                .and_then(|_| terminal_machine.handle_verified())
                .unwrap_or_else(|error| panic!("terminal fixture setup: {error:?}"));
            terminal(&mut terminal_machine)
                .unwrap_or_else(|error| panic!("terminal fixture: {error:?}"));
            assert_eq!(terminal_machine.state, expected);
        }
    }

    #[test]
    fn fixture_protocol_maps_success_cancel_dismiss_and_unknown_response() {
        let mut results = HashMap::new();
        results.insert(
            "uri".to_owned(),
            OwnedValue::from(Str::from("file:///fixture/screenshot.png")),
        );
        let success = response_artifact(0, results, 2)
            .unwrap_or_else(|error| panic!("success fixture: {error:?}"));
        assert_eq!(success.uri, "file:///fixture/screenshot.png");
        assert_eq!(success.interface_version, 2);
        assert_eq!(
            response_artifact(1, HashMap::new(), 2),
            Err(PortalScreenshotFailure::Cancelled)
        );
        assert_eq!(
            response_artifact(2, HashMap::new(), 2),
            Err(PortalScreenshotFailure::Dismissed)
        );
        assert_eq!(
            response_artifact(7, HashMap::new(), 2),
            Err(PortalScreenshotFailure::ProtocolViolation)
        );
    }
}
