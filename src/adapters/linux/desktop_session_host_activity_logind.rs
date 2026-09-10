//! 通过公开 systemd-logind D-Bus 接口监视当前 Wayland 用户会话活动状态。

use std::{collections::HashMap, time::Duration};

use futures_lite::{StreamExt, future};
use zbus::{
    MatchRule, Message, MessageStream, Proxy,
    connection::Builder as ConnectionBuilder,
    message::Type as MessageType,
    proxy::MethodFlags,
    zvariant::{OwnedObjectPath, OwnedValue},
};

use crate::components::desktop_session_input_liveness::{
    DesktopLoginSessionActivity, DesktopSessionInputLivenessFailure,
};

const DBUS_SERVICE: &str = "org.freedesktop.DBus";
const DBUS_OBJECT: &str = "/org/freedesktop/DBus";
const DBUS_INTERFACE: &str = "org.freedesktop.DBus";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const LOGIN_SERVICE: &str = "org.freedesktop.login1";
const LOGIN_OBJECT: &str = "/org/freedesktop/login1";
const LOGIN_MANAGER_INTERFACE: &str = "org.freedesktop.login1.Manager";
const LOGIN_SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";

/// 只在 Portal lease 的 owner 线程持有当前登录会话的公开活动投影。
pub(crate) struct SystemLoginSessionMonitor {
    _connection: zbus::Connection,
    session_path: OwnedObjectPath,
    owner_changes: MessageStream,
    session_removed: MessageStream,
    properties_changed: MessageStream,
    activity: DesktopLoginSessionActivity,
    unavailable: bool,
}

impl SystemLoginSessionMonitor {
    pub(crate) fn same_session(&self, other: &Self) -> bool {
        self.session_path == other.session_path
    }

    pub(crate) async fn open() -> Result<Self, DesktopSessionInputLivenessFailure> {
        let connection = ConnectionBuilder::system()
            .map_err(|_| unavailable_failure())?
            .method_timeout(Duration::from_secs(2))
            .build()
            .await
            .map_err(|_| unavailable_failure())?;
        let mut owner_changes =
            signal_stream(&connection, DBUS_INTERFACE, "NameOwnerChanged", DBUS_OBJECT).await?;
        ensure_login_owner(&connection).await?;
        let session_removed = signal_stream(
            &connection,
            LOGIN_MANAGER_INTERFACE,
            "SessionRemoved",
            LOGIN_OBJECT,
        )
        .await?;
        let manager = Proxy::new(
            &connection,
            LOGIN_SERVICE,
            LOGIN_OBJECT,
            LOGIN_MANAGER_INTERFACE,
        )
        .await
        .map_err(|_| unavailable_failure())?;
        // user@.service 下的桌面应用和代理通常不属于 session scope。
        // logind 的 auto 依据调用方凭据选择其自身会话，或同一用户的主显示会话；
        // 返回具体对象路径后固定监视该代际，不猜测/枚举其他用户或重定向活跃 lease。
        let session_path = manager
            .call_with_flags::<_, _, OwnedObjectPath>(
                "GetSession",
                MethodFlags::NoAutoStart.into(),
                &("auto"),
            )
            .await
            .map_err(|_| unavailable_failure())?
            .ok_or_else(unavailable_failure)?;
        let properties_changed = signal_stream(
            &connection,
            PROPERTIES_INTERFACE,
            "PropertiesChanged",
            session_path.as_str(),
        )
        .await?;
        let properties = Proxy::new(
            &connection,
            LOGIN_SERVICE,
            session_path.as_str(),
            PROPERTIES_INTERFACE,
        )
        .await
        .map_err(|_| unavailable_failure())?
        .call_with_flags::<_, _, HashMap<String, OwnedValue>>(
            "GetAll",
            MethodFlags::NoAutoStart.into(),
            &(LOGIN_SESSION_INTERFACE),
        )
        .await
        .map_err(|_| unavailable_failure())?
        .ok_or_else(unavailable_failure)?;
        let activity = initial_activity(&properties)?;
        // NameHasOwner 后若服务已经切换，预订阅流会保留该失效事实。
        if poll_login_owner_changed(&mut owner_changes).await? {
            return Err(unavailable_failure());
        }
        Ok(Self {
            _connection: connection,
            session_path,
            owner_changes,
            session_removed,
            properties_changed,
            activity,
            unavailable: false,
        })
    }

    pub(crate) fn poll(&mut self) -> Result<(), DesktopSessionInputLivenessFailure> {
        if self.unavailable || login_owner_changed(&mut self.owner_changes).unwrap_or(true) {
            return self.fail_unavailable();
        }
        loop {
            let message = match poll_message(&mut self.session_removed) {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(()) => return self.fail_unavailable(),
            };
            let removed = message
                .body()
                .deserialize::<(String, OwnedObjectPath)>()
                .map(|(_, path)| path == self.session_path)
                .unwrap_or(true);
            if removed {
                return self.fail_unavailable();
            }
        }
        loop {
            let message = match poll_message(&mut self.properties_changed) {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(()) => return self.fail_unavailable(),
            };
            let Ok((interface, changed, invalidated)) =
                message
                    .body()
                    .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
            else {
                return self.fail_unavailable();
            };
            if interface != LOGIN_SESSION_INTERFACE {
                continue;
            }
            if invalidated.iter().any(|name| required_property(name))
                || apply_changes(&mut self.activity, &changed).is_err()
            {
                return self.fail_unavailable();
            }
        }
        self.activity.ensure_input_allowed()
    }

    fn fail_unavailable(&mut self) -> Result<(), DesktopSessionInputLivenessFailure> {
        self.unavailable = true;
        Err(unavailable_failure())
    }
}

async fn ensure_login_owner(
    connection: &zbus::Connection,
) -> Result<(), DesktopSessionInputLivenessFailure> {
    let dbus = Proxy::new(connection, DBUS_SERVICE, DBUS_OBJECT, DBUS_INTERFACE)
        .await
        .map_err(|_| unavailable_failure())?;
    let owned = dbus
        .call_with_flags::<_, _, bool>(
            "NameHasOwner",
            MethodFlags::NoAutoStart.into(),
            &(LOGIN_SERVICE),
        )
        .await
        .map_err(|_| unavailable_failure())?
        .ok_or_else(unavailable_failure)?;
    if owned {
        Ok(())
    } else {
        Err(unavailable_failure())
    }
}

async fn signal_stream(
    connection: &zbus::Connection,
    interface: &'static str,
    member: &'static str,
    path: &str,
) -> Result<MessageStream, DesktopSessionInputLivenessFailure> {
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface(interface)
        .map_err(|_| unavailable_failure())?
        .member(member)
        .map_err(|_| unavailable_failure())?
        .path(path)
        .map_err(|_| unavailable_failure())?
        .build();
    MessageStream::for_match_rule(rule, connection, Some(8))
        .await
        .map_err(|_| unavailable_failure())
}

fn initial_activity(
    properties: &HashMap<String, OwnedValue>,
) -> Result<DesktopLoginSessionActivity, DesktopSessionInputLivenessFailure> {
    if property_text(properties, "Type")? != "wayland"
        || property_text(properties, "Class")? != "user"
    {
        return Err(unavailable_failure());
    }
    let activity = DesktopLoginSessionActivity::new(
        property_bool(properties, "Active")?,
        property_bool(properties, "LockedHint")?,
        property_bool(properties, "CanLock")?,
    );
    activity.ensure_input_allowed()?;
    Ok(activity)
}

fn apply_changes(
    activity: &mut DesktopLoginSessionActivity,
    changed: &HashMap<String, OwnedValue>,
) -> Result<(), DesktopSessionInputLivenessFailure> {
    if let Some(value) = changed.get("Type")
        && value.downcast_ref::<&str>().ok() != Some("wayland")
    {
        return Err(unavailable_failure());
    }
    if let Some(value) = changed.get("Class")
        && value.downcast_ref::<&str>().ok() != Some("user")
    {
        return Err(unavailable_failure());
    }
    if let Some(value) = changed.get("Active") {
        activity.update_active(
            value
                .downcast_ref::<bool>()
                .map_err(|_| unavailable_failure())?,
        );
    }
    if let Some(value) = changed.get("LockedHint") {
        activity.update_locked(
            value
                .downcast_ref::<bool>()
                .map_err(|_| unavailable_failure())?,
        );
    }
    if let Some(value) = changed.get("CanLock") {
        activity.update_lock_state_available(
            value
                .downcast_ref::<bool>()
                .map_err(|_| unavailable_failure())?,
        );
    }
    Ok(())
}

fn property_bool(
    properties: &HashMap<String, OwnedValue>,
    name: &str,
) -> Result<bool, DesktopSessionInputLivenessFailure> {
    properties
        .get(name)
        .ok_or_else(unavailable_failure)?
        .downcast_ref::<bool>()
        .map_err(|_| unavailable_failure())
}

fn property_text<'a>(
    properties: &'a HashMap<String, OwnedValue>,
    name: &str,
) -> Result<&'a str, DesktopSessionInputLivenessFailure> {
    properties
        .get(name)
        .ok_or_else(unavailable_failure)?
        .downcast_ref::<&str>()
        .map_err(|_| unavailable_failure())
}

fn login_owner_changed(
    stream: &mut MessageStream,
) -> Result<bool, DesktopSessionInputLivenessFailure> {
    async_io::block_on(poll_login_owner_changed(stream))
}

async fn poll_login_owner_changed(
    stream: &mut MessageStream,
) -> Result<bool, DesktopSessionInputLivenessFailure> {
    loop {
        let Some(next) = future::poll_once(stream.next()).await else {
            return Ok(false);
        };
        let message = next
            .ok_or_else(unavailable_failure)?
            .map_err(|_| unavailable_failure())?;
        let (name, old_owner, new_owner) = message
            .body()
            .deserialize::<(String, String, String)>()
            .map_err(|_| unavailable_failure())?;
        if name == LOGIN_SERVICE && old_owner != new_owner {
            return Ok(true);
        }
    }
}

fn poll_message(stream: &mut MessageStream) -> Result<Option<Message>, ()> {
    match async_io::block_on(future::poll_once(stream.next())) {
        None => Ok(None),
        Some(Some(Ok(message))) => Ok(Some(message)),
        Some(_) => Err(()),
    }
}

fn required_property(name: &str) -> bool {
    matches!(name, "Active" | "LockedHint" | "CanLock" | "Type" | "Class")
}

const fn unavailable_failure() -> DesktopSessionInputLivenessFailure {
    DesktopSessionInputLivenessFailure::new(
        "HOST_SESSION_STATE_UNAVAILABLE",
        "host-session-liveness",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an active unlocked Wayland user display and system logind; read-only"]
    fn live_monitor_resolves_the_current_users_display_without_portal_or_input() {
        let mut monitor = async_io::block_on(SystemLoginSessionMonitor::open())
            .expect("resolve and validate the caller's current display session");
        assert_ne!(
            monitor.session_path.as_str(),
            "/org/freedesktop/login1/session/auto"
        );
        assert_ne!(
            monitor.session_path.as_str(),
            "/org/freedesktop/login1/session/self"
        );
        monitor
            .poll()
            .expect("the resolved session is still active and unlocked");
    }
}
