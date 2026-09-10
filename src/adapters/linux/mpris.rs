//! 显式私有总线 MPRIS Adapter；zbus 与原生身份都止于本文件。

use crate::components::opaque_id::{OpaqueTargetId, OpaqueTargetKind};
use futures_lite::{StreamExt, future};
use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    time::{Duration, Instant},
};
use zbus::{
    Connection, MatchRule, MessageStream, Proxy,
    connection::Builder,
    message::Type,
    proxy::{Builder as ProxyBuilder, CacheProperties},
};

const DBUS_DEST: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const MAXIMUM_BUS_NAMES: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Failure {
    Unavailable,
    Timeout,
    Stale,
    BusEpochStale,
    Ambiguous,
    Protocol,
    ProviderError,
    OutcomeUnknown,
}

#[derive(Clone, Debug)]
pub(crate) struct Config {
    pub(crate) address: String,
    pub(crate) broker_epoch: String,
    // 由 runtime resolver 交付的规范化 GUID；旧显式 raw fixture 可保持 None。
    pub(crate) expected_bus_guid: Option<String>,
    pub(crate) timeout_ms: u32,
    pub(crate) maximum_items: usize,
    // 只供非默认私有 fixture 强制制造公开指纹碰撞。
    pub(crate) fixture_public_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Session {
    pub(crate) session_id: String,
    name: String,
    owner: String,
    generation: String,
}

/// 保存有界媒体会话目录及其完整性，不向 Module 暴露总线身份。
#[derive(Clone, Debug)]
pub(crate) struct Inventory {
    pub(crate) sessions: Vec<Session>,
    pub(crate) total: usize,
    pub(crate) truncated: bool,
    pub(crate) warnings: Vec<&'static str>,
    pub(crate) audit: Audit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Controls {
    pub(crate) play: bool,
    pub(crate) pause: bool,
    pub(crate) toggle: bool,
    pub(crate) stop: bool,
    pub(crate) next: bool,
    pub(crate) previous: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct State {
    pub(crate) status: &'static str,
    pub(crate) controls: Controls,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Audit {
    pub(crate) list_names: u64,
    pub(crate) get_name_owner: u64,
    pub(crate) property_get: u64,
    pub(crate) method: u64,
    pub(crate) get_all: u64,
    pub(crate) metadata: u64,
    pub(crate) properties_changed: u64,
    pub(crate) activation: u64,
}

struct Deadline(Instant);
impl Deadline {
    fn new(ms: u32) -> Result<Self, Failure> {
        if !(1..=30_000).contains(&ms) {
            return Err(Failure::Protocol);
        }
        Ok(Self(Instant::now() + Duration::from_millis(u64::from(ms))))
    }
    fn left(&self) -> Result<Duration, Failure> {
        self.0
            .checked_duration_since(Instant::now())
            .filter(|v| !v.is_zero())
            .ok_or(Failure::Timeout)
    }
}

pub(crate) async fn discover(config: &Config) -> Result<Inventory, Failure> {
    let deadline = Deadline::new(config.timeout_ms)?;
    validate_config(config)?;
    let conn = connect(config, &deadline).await?;
    discover_on_connection(config, &conn, &deadline).await
}

fn validate_config(config: &Config) -> Result<(), Failure> {
    if config.address.is_empty()
        || config.broker_epoch.is_empty()
        || !(1..=128).contains(&config.maximum_items)
        || config
            .expected_bus_guid
            .as_deref()
            .is_some_and(|guid| !valid_bus_guid(guid))
    {
        return Err(Failure::Protocol);
    }
    Ok(())
}

async fn connect(config: &Config, deadline: &Deadline) -> Result<Connection, Failure> {
    within(
        deadline,
        Builder::address(config.address.as_str())
            .map_err(|_| Failure::Protocol)?
            .method_timeout(deadline.left()?)
            .build(),
    )
    .await
}

async fn discover_on_connection(
    config: &Config,
    conn: &Connection,
    deadline: &Deadline,
) -> Result<Inventory, Failure> {
    let dbus = proxy(conn, DBUS_DEST, DBUS_PATH, DBUS_DEST, deadline).await?;
    // 先建立 owner 变化订阅，再取得快照，避免 list-before-subscribe 竞态。
    let mut owner_changes = owner_change_stream(conn, deadline).await?;
    let guid = normalize_bus_guid(within(deadline, dbus.call("GetId", &())).await?)?;
    // 这是跨进程 runtime identity 的第一道门禁；错配必须早于 ListNames 与播放器访问。
    if config
        .expected_bus_guid
        .as_deref()
        .is_some_and(|expected| expected != guid)
    {
        return Err(Failure::BusEpochStale);
    }
    let names: Vec<String> = within(deadline, dbus.call("ListNames", &())).await?;
    if names.len() > MAXIMUM_BUS_NAMES {
        return Err(Failure::Protocol);
    }
    let mut audit = Audit {
        list_names: 1,
        ..Audit::default()
    };
    let mut by_owner: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in names.into_iter().filter(|n| n.starts_with(MPRIS_PREFIX)) {
        let owner: String = within(deadline, dbus.call("GetNameOwner", &(name.as_str(),))).await?;
        audit.get_name_owner += 1;
        if owner.starts_with(':') {
            by_owner.entry(owner).or_default().push(name);
        }
    }
    // 双次 owner 快照是权威一致性门禁；信号流另行覆盖 A→B→A 回环变化。
    for (expected_owner, names) in &by_owner {
        for name in names {
            let current_owner: String =
                within(deadline, dbus.call("GetNameOwner", &(name.as_str(),))).await?;
            audit.get_name_owner += 1;
            if &current_owner != expected_owner {
                return Err(Failure::Stale);
            }
        }
    }
    // 同一总线服务的回复形成屏障；屏障前的相关 owner 变化必须已经进入订阅队列。
    let barrier_guid = normalize_bus_guid(within(deadline, dbus.call("GetId", &())).await?)?;
    if barrier_guid != guid || owner_changed_during_enumeration(&mut owner_changes).await? {
        return Err(Failure::Stale);
    }
    let mut warnings = Vec::new();
    let mut sessions: Vec<Session> = Vec::new();
    let mut ids = HashMap::new();
    for (owner, names) in by_owner {
        if names.len() != 1 {
            warnings.push("ambiguous-owner-alias");
            continue;
        }
        let name = names.into_iter().next().ok_or(Failure::Protocol)?;
        let generation = format!("{}|{}|{}|{}", config.broker_epoch, guid, name, owner);
        let id = config.fixture_public_id.clone().unwrap_or_else(|| {
            OpaqueTargetId::new(OpaqueTargetKind::Media, &generation).to_string()
        });
        if ids.insert(id.clone(), ()).is_some() {
            warnings.push("ambiguous-public-id");
            sessions.retain(|s| s.session_id != id);
            continue;
        }
        sessions.push(Session {
            session_id: id,
            name,
            owner,
            generation,
        });
    }
    sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));
    warnings.sort();
    warnings.dedup();
    let total = sessions.len();
    let truncated = total > config.maximum_items;
    sessions.truncate(config.maximum_items);
    Ok(Inventory {
        sessions,
        total,
        truncated,
        warnings,
        audit,
    })
}

fn normalize_bus_guid(value: String) -> Result<String, Failure> {
    let normalized = value.to_ascii_lowercase();
    if valid_bus_guid(&normalized) {
        Ok(normalized)
    } else {
        Err(Failure::Protocol)
    }
}

fn valid_bus_guid(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

async fn owner_change_stream(
    conn: &Connection,
    deadline: &Deadline,
) -> Result<MessageStream, Failure> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(DBUS_DEST)
        .map_err(|_| Failure::Protocol)?
        .path(DBUS_PATH)
        .map_err(|_| Failure::Protocol)?
        .interface(DBUS_DEST)
        .map_err(|_| Failure::Protocol)?
        .member("NameOwnerChanged")
        .map_err(|_| Failure::Protocol)?
        .build();
    within(
        deadline,
        MessageStream::for_match_rule(rule, conn, Some(MAXIMUM_BUS_NAMES)),
    )
    .await
}

async fn owner_changed_during_enumeration(
    owner_changes: &mut MessageStream,
) -> Result<bool, Failure> {
    loop {
        match future::poll_once(owner_changes.next()).await {
            None => return Ok(false),
            Some(None) => return Err(Failure::ProviderError),
            Some(Some(message)) => {
                let message = message.map_err(map_error)?;
                let (name, _old_owner, _new_owner): (String, String, String) = message
                    .body()
                    .deserialize()
                    .map_err(|_| Failure::Protocol)?;
                if name.starts_with(MPRIS_PREFIX) {
                    return Ok(true);
                }
            }
        }
    }
}

pub(crate) async fn state(config: &Config, target: &str) -> Result<(State, Audit), Failure> {
    let deadline = Deadline::new(config.timeout_ms)?;
    let (conn, session, mut audit) = resolve(config, target, &deadline).await?;
    let player = proxy(&conn, &session.owner, PLAYER_PATH, PLAYER_IFACE, &deadline).await?;
    let status: String = get_string(&player, "PlaybackStatus", &deadline, &mut audit).await?;
    let can_control: bool = get_bool(&player, "CanControl", &deadline, &mut audit).await?;
    let can_play: bool = get_bool(&player, "CanPlay", &deadline, &mut audit).await?;
    let can_pause: bool = get_bool(&player, "CanPause", &deadline, &mut audit).await?;
    let can_next: bool = get_bool(&player, "CanGoNext", &deadline, &mut audit).await?;
    let can_previous: bool = get_bool(&player, "CanGoPrevious", &deadline, &mut audit).await?;
    ensure_owner(&conn, &session, &deadline, &mut audit).await?;
    let status = match status.as_str() {
        "Playing" => "playing",
        "Paused" => "paused",
        "Stopped" => "stopped",
        _ => "unknown",
    };
    Ok((
        State {
            status,
            controls: Controls {
                play: can_control && can_play,
                pause: can_control && can_pause,
                toggle: can_control && can_play && can_pause,
                stop: can_control,
                next: can_control && can_next,
                previous: can_control && can_previous,
            },
        },
        audit,
    ))
}

/// 在全部只读门禁通过且即将调用 MPRIS method 时发布一次 accepted。
pub(crate) async fn control_with_acceptance<F>(
    config: &Config,
    target: &str,
    operation: &str,
    on_accepted: F,
) -> Result<Audit, Failure>
where
    F: FnOnce() -> Result<(), Failure>,
{
    let deadline = Deadline::new(config.timeout_ms)?;
    // 封闭 operation 必须在建立总线连接前完成解析。
    let (method, gate) = match operation {
        "play" => ("Play", Some("CanPlay")),
        "pause" => ("Pause", Some("CanPause")),
        "toggle-play-pause" => ("PlayPause", None),
        "stop" => ("Stop", None),
        "skip-next" => ("Next", Some("CanGoNext")),
        "skip-previous" => ("Previous", Some("CanGoPrevious")),
        _ => return Err(Failure::Protocol),
    };
    let (conn, session, mut audit) = resolve(config, target, &deadline).await?;
    let player = proxy(&conn, &session.owner, PLAYER_PATH, PLAYER_IFACE, &deadline).await?;
    let control: bool = get_bool(&player, "CanControl", &deadline, &mut audit).await?;
    if !control {
        return Err(Failure::Unavailable);
    }
    if operation == "toggle-play-pause" {
        let play: bool = get_bool(&player, "CanPlay", &deadline, &mut audit).await?;
        let pause: bool = get_bool(&player, "CanPause", &deadline, &mut audit).await?;
        if !(play && pause) {
            return Err(Failure::Unavailable);
        }
    } else if let Some(property) = gate {
        let allowed: bool = get_bool(&player, property, &deadline, &mut audit).await?;
        if !allowed {
            return Err(Failure::Unavailable);
        }
    }
    ensure_owner(&conn, &session, &deadline, &mut audit).await?;
    // 隔离 worker 必须先发布并 flush accepted；进程内 fixture 使用无 I/O 回调。
    on_accepted()?;
    audit.method += 1;
    match within(&deadline, player.call::<_, _, ()>(method, &())).await {
        Ok(()) => {}
        Err(Failure::ProviderError) => return Err(Failure::ProviderError),
        Err(_) => return Err(Failure::OutcomeUnknown),
    }
    ensure_owner(&conn, &session, &deadline, &mut audit)
        .await
        .map_err(|_| Failure::OutcomeUnknown)?;
    Ok(audit)
}

async fn resolve(
    config: &Config,
    target: &str,
    deadline: &Deadline,
) -> Result<(Connection, Session, Audit), Failure> {
    validate_config(config)?;
    // 重新发现、目标解析、能力门禁与最终 method 共享同一连接和 deadline。
    let conn = connect(config, deadline).await?;
    let inventory = discover_on_connection(config, &conn, deadline).await?;
    let mut matches = inventory
        .sessions
        .into_iter()
        .filter(|session| session.session_id == target);
    let one = match matches.next() {
        Some(session) => session,
        None if inventory.truncated => return Err(Failure::Unavailable),
        None => return Err(Failure::Stale),
    };
    if matches.next().is_some() {
        return Err(Failure::Ambiguous);
    }
    Ok((conn, one, inventory.audit))
}

async fn ensure_owner(
    conn: &Connection,
    s: &Session,
    d: &Deadline,
    a: &mut Audit,
) -> Result<(), Failure> {
    let dbus = proxy(conn, DBUS_DEST, DBUS_PATH, DBUS_DEST, d).await?;
    let owner: String = within(d, dbus.call("GetNameOwner", &(s.name.as_str(),))).await?;
    a.get_name_owner += 1;
    if owner != s.owner {
        return Err(Failure::Stale);
    };
    Ok(())
}
async fn get_bool(p: &Proxy<'_>, name: &str, d: &Deadline, a: &mut Audit) -> Result<bool, Failure> {
    a.property_get += 1;
    within(d, p.get_property::<bool>(name)).await
}
async fn get_string(
    p: &Proxy<'_>,
    name: &str,
    d: &Deadline,
    a: &mut Audit,
) -> Result<String, Failure> {
    a.property_get += 1;
    within(d, p.get_property::<String>(name)).await
}
async fn proxy(
    c: &Connection,
    dest: &str,
    path: &str,
    iface: &str,
    d: &Deadline,
) -> Result<Proxy<'static>, Failure> {
    let b = ProxyBuilder::<Proxy<'static>>::new(c)
        .destination(dest.to_owned())
        .map_err(|_| Failure::Protocol)?
        .path(path.to_owned())
        .map_err(|_| Failure::Protocol)?
        .interface(iface.to_owned())
        .map_err(|_| Failure::Protocol)?
        .cache_properties(CacheProperties::No);
    within(d, b.build()).await
}
async fn within<T, F>(d: &Deadline, f: F) -> Result<T, Failure>
where
    F: Future<Output = zbus::Result<T>>,
{
    let left = d.left()?;
    future::race(async { f.await.map_err(map_error) }, async {
        async_io::Timer::after(left).await;
        Err(Failure::Timeout)
    })
    .await
}
fn map_error(e: zbus::Error) -> Failure {
    match e {
        zbus::Error::MethodError(name, _, _) if name.as_str().contains("NameHasNoOwner") => {
            Failure::Stale
        }
        zbus::Error::MethodError(_, _, _) => Failure::ProviderError,
        zbus::Error::InputOutput(ref s) if s.kind() == std::io::ErrorKind::TimedOut => {
            Failure::Timeout
        }
        _ => Failure::Unavailable,
    }
}
