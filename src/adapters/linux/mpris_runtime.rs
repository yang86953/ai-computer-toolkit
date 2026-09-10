//! Linux MPRIS current-user bus 与稳定父代际解析 Adapter。

use std::{
    future::Future,
    time::{Duration, Instant},
};

use futures_lite::future;
use zbus::{
    Connection, Proxy,
    connection::Builder,
    proxy::MethodFlags,
    proxy::{Builder as ProxyBuilder, CacheProperties},
};

use crate::components::linux_user_bus_endpoint::{
    LinuxUserBusEndpointFailure, resolve_current as resolve_current_endpoint,
};

use super::mpris::Failure;

const DBUS_DEST: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
#[cfg(feature = "linux-mpris-candidate")]
const PRIVATE_RUNTIME_GENERATION: &str = "fixture-user-bus-v1";

/// 只向 Module 交付内部 worker 所需的地址和稳定父代际，不进入公共 JSON。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MprisRuntimeIdentity {
    address: String,
    broker_epoch: String,
    bus_guid: String,
}

impl MprisRuntimeIdentity {
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    pub(crate) fn broker_epoch(&self) -> &str {
        &self.broker_epoch
    }

    /// 返回跨进程复核使用的规范化 D-Bus GUID，不进入公开 JSON。
    pub(crate) fn bus_guid(&self) -> &str {
        &self.bus_guid
    }
}

struct Deadline(Instant);

impl Deadline {
    fn new(timeout_ms: u32) -> Result<Self, Failure> {
        if !(1..=30_000).contains(&timeout_ms) {
            return Err(Failure::Protocol);
        }
        Ok(Self(
            Instant::now() + Duration::from_millis(u64::from(timeout_ms)),
        ))
    }

    fn left(&self) -> Result<Duration, Failure> {
        self.0
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(Failure::Timeout)
    }
}

/// 解析固定 current-user endpoint；只连接总线服务并读取 GUID，不访问播放器。
pub(crate) fn resolve_current(timeout_ms: u32) -> Result<MprisRuntimeIdentity, Failure> {
    let endpoint = resolve_current_endpoint().map_err(map_endpoint_failure)?;
    async_io::block_on(resolve(
        endpoint.address(),
        endpoint.runtime_generation(),
        timeout_ms,
    ))
}

/// 仅供私有 D-Bus fixture 验证跨请求 GUID 代际稳定性。
#[cfg(feature = "linux-mpris-candidate")]
pub(crate) fn resolve_private(
    address: &str,
    timeout_ms: u32,
) -> Result<MprisRuntimeIdentity, Failure> {
    async_io::block_on(resolve(address, PRIVATE_RUNTIME_GENERATION, timeout_ms))
}

async fn resolve(
    address: &str,
    runtime_generation: &str,
    timeout_ms: u32,
) -> Result<MprisRuntimeIdentity, Failure> {
    if !valid_address(address) || !valid_generation(runtime_generation) {
        return Err(Failure::Protocol);
    }
    let deadline = Deadline::new(timeout_ms)?;
    let connection = within(
        &deadline,
        Builder::address(address)
            .map_err(|_| Failure::Protocol)?
            .method_timeout(deadline.left()?)
            .build(),
    )
    .await?;
    let dbus = proxy(&connection, &deadline).await?;
    let guid = within(
        &deadline,
        dbus.call_with_flags::<_, _, String>("GetId", MethodFlags::NoAutoStart.into(), &()),
    )
    .await?
    .ok_or(Failure::ProviderError)?
    .to_ascii_lowercase();
    if guid.len() != 32 || !guid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Failure::Protocol);
    }
    let broker_epoch = format!("{runtime_generation}:{guid}");
    if broker_epoch.len() > 128 {
        return Err(Failure::Protocol);
    }
    Ok(MprisRuntimeIdentity {
        address: address.to_owned(),
        broker_epoch,
        bus_guid: guid,
    })
}

async fn proxy(connection: &Connection, deadline: &Deadline) -> Result<Proxy<'static>, Failure> {
    let builder = ProxyBuilder::<Proxy<'static>>::new(connection)
        .destination(DBUS_DEST.to_owned())
        .map_err(|_| Failure::Protocol)?
        .path(DBUS_PATH.to_owned())
        .map_err(|_| Failure::Protocol)?
        .interface(DBUS_DEST.to_owned())
        .map_err(|_| Failure::Protocol)?
        .cache_properties(CacheProperties::No);
    within(deadline, builder.build()).await
}

async fn within<T, F>(deadline: &Deadline, operation: F) -> Result<T, Failure>
where
    F: Future<Output = zbus::Result<T>>,
{
    let remaining = deadline.left()?;
    future::race(async { operation.await.map_err(map_zbus_failure) }, async {
        async_io::Timer::after(remaining).await;
        Err(Failure::Timeout)
    })
    .await
}

fn valid_address(value: &str) -> bool {
    value.starts_with("unix:path=")
        && value.len() > "unix:path=".len()
        && value.len() <= 4096
        && !value
            .bytes()
            .any(|byte| matches!(byte, b'\0' | b'\n' | b'\r' | b';'))
}

fn valid_generation(value: &str) -> bool {
    (1..=95).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

fn map_endpoint_failure(failure: LinuxUserBusEndpointFailure) -> Failure {
    match failure {
        LinuxUserBusEndpointFailure::IdentityUnavailable
        | LinuxUserBusEndpointFailure::UnsafeRuntime
        | LinuxUserBusEndpointFailure::BusUnavailable => Failure::Unavailable,
    }
}

fn map_zbus_failure(error: zbus::Error) -> Failure {
    match error {
        zbus::Error::InputOutput(ref source) if source.kind() == std::io::ErrorKind::TimedOut => {
            Failure::Timeout
        }
        zbus::Error::MethodError(_, _, _) => Failure::ProviderError,
        _ => Failure::Unavailable,
    }
}
