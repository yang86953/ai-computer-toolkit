//! MPRIS worker 的 v2 领域投影；私有候选与生产 App v3 共用，不改变公开 Media v2 路由。

use serde_json::{Value, json};

use crate::adapters::linux::mpris::{self, Config, Failure};

/// 从内部已解析的总线投影有界媒体目录，不发布 MPRIS 名称或总线身份。
pub(crate) fn discover(config: &Config) -> Result<Value, Failure> {
    let inventory = async_io::block_on(mpris::discover(config))?;
    let sessions = inventory
        .sessions
        .into_iter()
        .map(|session| {
            json!({
                "sessionId": session.session_id,
                "targetKind": "media-session",
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": true,
        "contractVersion": "act/control/v2",
        "capability": "media.session.discover@2",
        "data": {
            "count": sessions.len(),
            "total": inventory.total,
            "truncated": inventory.truncated,
            "coverage": "mpris-owned-names-private-broker-generation",
            "complete": inventory.warnings.is_empty() && !inventory.truncated,
            "warnings": inventory.warnings,
            "sessions": sessions,
        }
    }))
}

/// 重新解析有界 opaque 媒体目标并只读取播放状态与控制可用性白名单。
pub(crate) fn state(config: &Config, target: &str) -> Result<Value, Failure> {
    let (state, _audit) = async_io::block_on(mpris::state(config, target))?;
    Ok(json!({
        "ok": true,
        "contractVersion": "act/control/v2",
        "capability": "media.playback.state.read@2",
        "data": {
            "sessionId": target,
            "targetKind": "media-session",
            "playbackStatus": state.status,
            "availableControls": {
                "play": state.controls.play,
                "pause": state.controls.pause,
                "togglePlayPause": state.controls.toggle,
                "stop": state.controls.stop,
                "skipNext": state.controls.next,
                "skipPrevious": state.controls.previous,
            }
        }
    }))
}

#[cfg(feature = "linux-mpris-candidate")]
pub(crate) fn control(config: &Config, target: &str, operation: &str, confirmed: bool) -> Value {
    match control_with_acceptance(config, target, operation, confirmed, || Ok(())) {
        Ok(value) => value,
        // 进程内旧 fixture 保持既有成功外壳；隔离 worker 会保留精确结构化失败。
        Err(_) => pre_dispatch_rejected(target, operation),
    }
}

/// 在 MPRIS method 调用前通过回调发布 accepted，供隔离 worker 区分不确定结果。
pub(crate) fn control_with_acceptance<F>(
    config: &Config,
    target: &str,
    operation: &str,
    confirmed: bool,
    on_accepted: F,
) -> Result<Value, Failure>
where
    F: FnOnce() -> Result<(), Failure>,
{
    // confirmation-first：未确认分支绝不调用 Adapter，也不会建立总线连接。
    if !confirmed {
        return Ok(pre_dispatch_rejected(target, operation));
    }
    match async_io::block_on(mpris::control_with_acceptance(
        config,
        target,
        operation,
        on_accepted,
    )) {
        Ok(_) => Ok(control_result(
            target, operation, true, false, true, "replied",
        )),
        // 父/子总线代际错配必须保留精确 stale，不能伪装成一般业务前拒绝。
        Err(Failure::BusEpochStale) => Err(Failure::BusEpochStale),
        Err(
            Failure::Unavailable
            | Failure::Protocol
            | Failure::Stale
            | Failure::Ambiguous
            | Failure::Timeout,
        ) => Ok(control_result(
            target,
            operation,
            false,
            true,
            false,
            "pre-dispatch-rejected",
        )),
        Err(Failure::ProviderError) => Ok(control_result(
            target,
            operation,
            true,
            false,
            true,
            "provider-error",
        )),
        Err(Failure::OutcomeUnknown) => Ok(control_result(
            target,
            operation,
            true,
            false,
            false,
            "outcome-unknown",
        )),
    }
}

/// 供生产候选组合根在 endpoint/代际解析失败时复用同一零 mutation 投影。
pub(crate) fn pre_dispatch_rejected(target: &str, operation: &str) -> Value {
    control_result(
        target,
        operation,
        false,
        true,
        false,
        "pre-dispatch-rejected",
    )
}

fn control_result(
    target: &str,
    operation: &str,
    accepted: bool,
    retry_safe: bool,
    final_state: bool,
    outcome: &str,
) -> Value {
    json!({
        "ok": true,
        "contractVersion": "act/control/v2",
        "capability": "media.playback.control@2",
        "data": {
            "sessionId": target,
            "targetKind": "media-session",
            "operation": operation,
            "accepted": accepted,
            "finalStateReached": final_state,
            "dispatchOutcome": outcome,
            "effectConfirmed": false,
            "retrySafe": retry_safe,
            "targetMayHaveMutated": accepted,
            "automaticRetryProhibited": accepted,
        }
    })
}
