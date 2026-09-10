//! 显式私有地址 MPRIS 候选 fixture API；不进入生产 CLI 路由。

use std::time::{Duration, Instant};

use crate::{
    adapters::linux::{
        mpris::{Config, Failure},
        mpris_runtime::{self, MprisRuntimeIdentity},
    },
    modules::media_session,
};
use serde_json::Value;

fn config(address: &str, broker_epoch: &str, timeout_ms: u32) -> Config {
    Config {
        address: address.to_owned(),
        broker_epoch: broker_epoch.to_owned(),
        expected_bus_guid: None,
        timeout_ms,
        maximum_items: 128,
        fixture_public_id: None,
    }
}

fn resolved_config(identity: &MprisRuntimeIdentity, timeout_ms: u32) -> Config {
    let mut config = config(identity.address(), identity.broker_epoch(), timeout_ms);
    config.expected_bus_guid = Some(identity.bus_guid().to_owned());
    config
}

/// 仅探测受认证 current-user bus 与 GUID；不列出名称、不读取播放器、不进入公开路由。
pub fn current_user_runtime_candidate_ready(timeout_ms: u32) -> Result<bool, &'static str> {
    mpris_runtime::resolve_current(timeout_ms)
        .map(|_| true)
        .map_err(code)
}

/// 仅供私有总线证明：父代际由受限 endpoint generation 与 D-Bus GUID 解析。
pub fn discover_private_with_resolved_epoch(
    address: &str,
    timeout_ms: u32,
) -> Result<Value, &'static str> {
    let deadline = resolved_deadline(timeout_ms).map_err(code)?;
    let identity = mpris_runtime::resolve_private(address, remaining_ms(&deadline).map_err(code)?)
        .map_err(code)?;
    media_session::discover(&resolved_config(
        &identity,
        remaining_ms(&deadline).map_err(code)?,
    ))
    .map_err(code)
}

/// 仅供私有总线证明：跨请求用同一 GUID 父代际重新解析 opaque 目标。
pub fn state_private_with_resolved_epoch(
    address: &str,
    target: &str,
    timeout_ms: u32,
) -> Result<Value, &'static str> {
    let deadline = resolved_deadline(timeout_ms).map_err(code)?;
    let identity = mpris_runtime::resolve_private(address, remaining_ms(&deadline).map_err(code)?)
        .map_err(code)?;
    media_session::state(
        &resolved_config(&identity, remaining_ms(&deadline).map_err(code)?),
        target,
    )
    .map_err(code)
}

/// 未确认分支在 endpoint/GUID 解析前结束；确认后才解析稳定父代际并控制。
pub fn control_private_with_resolved_epoch(
    address: &str,
    target: &str,
    operation: &str,
    confirmed: bool,
    timeout_ms: u32,
) -> Value {
    if !confirmed {
        return media_session::pre_dispatch_rejected(target, operation);
    }
    let deadline = match resolved_deadline(timeout_ms) {
        Ok(deadline) => deadline,
        Err(_) => return media_session::pre_dispatch_rejected(target, operation),
    };
    let resolver_timeout = match remaining_ms(&deadline) {
        Ok(timeout) => timeout,
        Err(_) => return media_session::pre_dispatch_rejected(target, operation),
    };
    match mpris_runtime::resolve_private(address, resolver_timeout) {
        Ok(identity) => {
            let operation_timeout = match remaining_ms(&deadline) {
                Ok(timeout) => timeout,
                Err(_) => return media_session::pre_dispatch_rejected(target, operation),
            };
            media_session::control(
                &resolved_config(&identity, operation_timeout),
                target,
                operation,
                true,
            )
        }
        Err(_) => media_session::pre_dispatch_rejected(target, operation),
    }
}

fn resolved_deadline(timeout_ms: u32) -> Result<Instant, Failure> {
    if !(1..=30_000).contains(&timeout_ms) {
        return Err(Failure::Protocol);
    }
    Ok(Instant::now() + Duration::from_millis(u64::from(timeout_ms)))
}

fn remaining_ms(deadline: &Instant) -> Result<u32, Failure> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(Failure::Timeout)?;
    let millis = remaining.as_millis();
    u32::try_from(millis)
        .ok()
        .filter(|millis| *millis > 0)
        .ok_or(Failure::Timeout)
}

/// 仅供合成夹具证明 FNV/多 provider 同 ID 时整组失败闭合。
pub fn discover_private_with_forced_collision(
    address: &str,
    broker_epoch: &str,
    timeout_ms: u32,
) -> Result<Value, &'static str> {
    let config = Config {
        address: address.to_owned(),
        broker_epoch: broker_epoch.to_owned(),
        expected_bus_guid: None,
        timeout_ms,
        maximum_items: 128,
        fixture_public_id: Some("s2:m:0000000000000000".to_owned()),
    };
    media_session::discover(&config).map_err(code)
}

/// 仅对注入的私有总线执行无属性发现。
pub fn discover_private(
    address: &str,
    broker_epoch: &str,
    timeout_ms: u32,
) -> Result<Value, &'static str> {
    media_session::discover(&config(address, broker_epoch, timeout_ms)).map_err(code)
}
/// 仅对同一私有 broker 生命周期的 opaque 目标读取白名单状态。
pub fn state_private(
    address: &str,
    broker_epoch: &str,
    target: &str,
    timeout_ms: u32,
) -> Result<Value, &'static str> {
    media_session::state(&config(address, broker_epoch, timeout_ms), target).map_err(code)
}
/// 确认在任何总线访问前检查；结果不声称实际媒体效果。
pub fn control_private(
    address: &str,
    broker_epoch: &str,
    target: &str,
    operation: &str,
    confirmed: bool,
    timeout_ms: u32,
) -> Value {
    media_session::control(
        &config(address, broker_epoch, timeout_ms),
        target,
        operation,
        confirmed,
    )
}
fn code(f: Failure) -> &'static str {
    match f {
        Failure::Stale | Failure::BusEpochStale => "STALE_SESSION",
        Failure::Ambiguous => "AMBIGUOUS_TARGET",
        Failure::Timeout => "TIMEOUT",
        Failure::ProviderError => "PROCESS_SNAPSHOT_FAILED",
        Failure::OutcomeUnknown => "OUTCOME_UNKNOWN",
        Failure::Unavailable | Failure::Protocol => "CAPABILITY_UNAVAILABLE",
    }
}
