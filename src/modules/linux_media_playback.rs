//! 第三方 MPRIS App v3 的领域行为：一次控制、单一总期限与独立状态观察。

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{
    capabilities,
    components::{
        cancellation,
        media_playback_contract::{self as contract, Action, Control, Plan},
    },
    domain::{AppControlError, AppResult, CommandRequest},
    mpris_runtime_client,
};

pub(crate) use contract::is_capability;

pub(crate) fn perform(request: &CommandRequest) -> AppResult<Value> {
    let plan = contract::parse(request)?;
    execute_with(plan, run_backend)
}

fn run_backend(
    action: Action,
    target: Option<&str>,
    maximum_items: u32,
    timeout_ms: u32,
) -> AppResult<Value> {
    match action {
        Action::Discover => mpris_runtime_client::discover_current(maximum_items, timeout_ms),
        Action::State => mpris_runtime_client::state_current(
            target.unwrap_or_default(),
            maximum_items,
            timeout_ms,
        ),
        Action::Control(control) => mpris_runtime_client::control_current(
            target.unwrap_or_default(),
            control.operation(),
            true,
            maximum_items,
            timeout_ms,
        ),
    }
}

fn execute_with<F>(plan: Plan<'_>, mut backend: F) -> AppResult<Value>
where
    F: FnMut(Action, Option<&str>, u32, u32) -> AppResult<Value>,
{
    let deadline = Instant::now() + Duration::from_millis(u64::from(plan.timeout_ms));
    let result = backend(
        plan.action,
        plan.session_id,
        plan.maximum_items,
        remaining_ms(deadline)?,
    )?;
    let Some(control) = (match plan.action {
        Action::Control(control) => Some(control),
        _ => None,
    }) else {
        remaining_ms(deadline)?;
        return project(plan.action, plan.session_id, result);
    };
    // v2 的 ok=true 包含业务前拒绝；不能把这种外壳升级为 v3 控制成功。
    check_control_reply(&result)?;
    let mut completed = project(plan.action, plan.session_id, result.clone()).map_err(|_| {
        AppControlError::with_details("OUTCOME_UNKNOWN", "The MPRIS method receipt has an invalid request binding.",
            json!({"acceptedMayHaveOccurred": true, "automaticRetryProhibited": true, "providerResult": result}))
    })?;
    loop {
        let timeout =
            remaining_ms(deadline).map_err(|error| verification_error(&result, control, error))?;
        let observation = backend(Action::State, plan.session_id, plan.maximum_items, timeout)
            .and_then(|state| project(Action::State, plan.session_id, state))
            .map_err(|error| verification_error(&result, control, error))?;
        // 过期之后返回的匹配观察不能冒充在原期限内完成。
        remaining_ms(deadline).map_err(|error| verification_error(&result, control, error))?;
        if observation["data"]["playbackStatus"] == control.expected_status() {
            completed["data"]["verification"] = json!({
                "level": "observed-provider-state",
                "expectedPlaybackStatus": control.expected_status(),
                "observedPlaybackStatus": observation["data"]["playbackStatus"],
                "conditionObserved": true,
                "causalityConfirmed": false,
                "domainCommitVerified": false,
            });
            return Ok(completed);
        }
        // 只重读属性，不重发控制；总期限跨全部 worker 调用保持不变。
        let left =
            remaining_ms(deadline).map_err(|error| verification_error(&result, control, error))?;
        std::thread::sleep(Duration::from_millis(u64::from(left.min(50))));
    }
}

fn remaining_ms(deadline: Instant) -> AppResult<u32> {
    if cancellation::is_cancelled() {
        return Err(AppControlError::new(
            "CANCELLED",
            "The MPRIS operation was cancelled.",
        ));
    }
    deadline
        .checked_duration_since(Instant::now())
        .and_then(|remaining| u32::try_from(remaining.as_millis()).ok())
        .filter(|millis| *millis > 0)
        .ok_or_else(|| AppControlError::new("TIMEOUT", "The MPRIS total deadline expired."))
}

fn check_control_reply(result: &Value) -> AppResult<()> {
    match (
        result["data"]["dispatchOutcome"].as_str(),
        result["data"]["accepted"].as_bool(),
    ) {
        (Some("replied"), Some(true)) if result["data"]["finalStateReached"] == true => Ok(()),
        (Some("pre-dispatch-rejected"), Some(false)) => Err(AppControlError::with_details(
            "OPERATION_FAILED",
            "MPRIS rejected the control before method dispatch.",
            json!({"accepted": false, "targetMayHaveMutated": false, "automaticRetryProhibited": false, "providerResult": result}),
        )),
        (Some("provider-error"), Some(true)) => Err(AppControlError::with_details(
            "OPERATION_FAILED",
            "The MPRIS provider replied with an error; do not repeat the method.",
            json!({"accepted": true, "targetMayHaveMutated": true, "automaticRetryProhibited": true, "providerResult": result}),
        )),
        _ => Err(AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The MPRIS control has no trustworthy completed method receipt.",
            json!({"acceptedMayHaveOccurred": true, "targetMayHaveMutated": true, "automaticRetryProhibited": true, "providerResult": result}),
        )),
    }
}

fn verification_error(result: &Value, control: Control, cause: AppControlError) -> AppControlError {
    AppControlError::with_details(
        "TASK_VERIFICATION_FAILED",
        "The MPRIS method replied, but its requested playback state could not be verified.",
        json!({
            "accepted": true, "providerCompleted": true, "targetMayHaveMutated": true,
            "automaticRetryProhibited": true, "providerResult": result,
            "verification": {"conditionObserved": false, "expectedPlaybackStatus": control.expected_status(), "domainCommitVerified": false},
            "cause": {"code": cause.code, "message": cause.message, "details": cause.details},
        }),
    )
}

fn project(action: Action, target: Option<&str>, result: Value) -> AppResult<Value> {
    let expected_capability = match action {
        Action::Discover => capabilities::MEDIA_SESSION_DISCOVER_V2,
        Action::State => capabilities::MEDIA_PLAYBACK_STATE_READ_V2,
        Action::Control(_) => capabilities::MEDIA_PLAYBACK_CONTROL_V2,
    };
    if result["ok"] != true
        || result["capability"] != expected_capability
        || !result["data"].is_object()
        || target.is_some_and(|target| result["data"]["sessionId"] != target)
    {
        return Err(AppControlError::new(
            "OPERATION_FAILED",
            "The MPRIS result does not match its bound request.",
        ));
    }
    let mut data = result["data"].clone();
    if action == Action::Discover {
        data["coverage"] = json!("current-user-mpris-owned-names");
    }
    if action == Action::State {
        // v3 只宣告自身可执行的三种控制，不把 v2 的跳曲/切换能力伪装成已发布操作。
        let mut controls = serde_json::Map::new();
        for operation in ["play", "pause", "stop"] {
            let available = data["availableControls"][operation]
                .as_bool()
                .ok_or_else(|| {
                    AppControlError::new(
                        "OPERATION_FAILED",
                        "MPRIS state is missing a boolean control property.",
                    )
                })?;
            controls.insert(operation.into(), json!(available));
        }
        data["availableControls"] = Value::Object(controls);
    }
    Ok(json!({
        "ok": true, "contractVersion": "act/control/v2", "capability": action.capability(),
        "executionRealm": "isolated-worker", "windowIndependent": true, "imageDependency": "none",
        "fallback": "none", "data": data,
    }))
}

/// 普通 facade 评估只读取同一精确目标的白名单状态，不为 method 自授确认。
pub(crate) fn assess(capability: &str, target: &str) -> AppResult<Value> {
    if !matches!(
        capability,
        capabilities::MEDIA_PLAYBACK_STATE_READ_V3 | capabilities::MEDIA_PLAYBACK_CONTROL_V3
    ) || !contract::valid_target(target)
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "MPRIS assessment requires a state/control capability and exact media target.",
        ));
    }
    let state = mpris_runtime_client::state_current(
        target,
        contract::MAXIMUM_ITEMS,
        contract::DEFAULT_TIMEOUT_MS,
    )?;
    let state = project(Action::State, Some(target), state)?;
    let operations = ["play", "pause", "stop"]
        .into_iter()
        .filter(|operation| state["data"]["availableControls"][*operation] == true)
        .collect::<Vec<_>>();
    let mutation = capability == capabilities::MEDIA_PLAYBACK_CONTROL_V3;
    Ok(json!({
        "ok": true, "contractVersion": "act/control/v1", "capability": capability, "targetId": target,
        "decision": if mutation && operations.is_empty() { "unavailable" } else if mutation { "confirmation-required" } else { "executable-background" },
        "executionRealm": "isolated-worker", "requiresConfirmation": mutation, "requiresForegroundConsent": false,
        "availableOperations": operations, "reasons": ["current-mpris-owner-and-player-state-observed"],
        "constraints": {"windowIndependent": true, "noFallback": true, "revalidateAtDispatch": true},
        "evidence": state["data"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: &str = "s2:m:0123456789abcdef";

    fn plan() -> Plan<'static> {
        Plan {
            action: Action::Control(Control::Pause),
            session_id: Some(TARGET),
            maximum_items: 128,
            timeout_ms: 1000,
        }
    }

    fn reply(outcome: &str, accepted: bool) -> Value {
        json!({"ok": true, "capability": capabilities::MEDIA_PLAYBACK_CONTROL_V2, "data": {
            "sessionId": TARGET, "dispatchOutcome": outcome, "accepted": accepted,
            "finalStateReached": outcome == "replied", "effectConfirmed": false,
        }})
    }

    fn state(status: &str) -> Value {
        json!({"ok": true, "contractVersion": "act/control/v2", "capability": capabilities::MEDIA_PLAYBACK_STATE_READ_V2,
            "data": {"sessionId": TARGET, "targetKind": "media-session", "playbackStatus": status,
                "availableControls": {"play": true, "pause": true, "stop": true, "togglePlayPause": true, "skipNext": false, "skipPrevious": false}}})
    }

    #[test]
    fn one_method_and_independent_observation_are_required_for_success() {
        let mut actions = Vec::new();
        let result = execute_with(plan(), |action, target, maximum_items, timeout| {
            actions.push(action);
            assert_eq!(target, Some(TARGET));
            assert_eq!(maximum_items, 128);
            assert!(timeout <= 1000);
            Ok(if matches!(action, Action::Control(_)) {
                reply("replied", true)
            } else {
                state("paused")
            })
        })
        .unwrap();
        assert_eq!(actions, [Action::Control(Control::Pause), Action::State]);
        assert_eq!(
            result["capability"],
            capabilities::MEDIA_PLAYBACK_CONTROL_V3
        );
        assert_eq!(result["data"]["verification"]["conditionObserved"], true);
        assert_eq!(result["data"]["verification"]["causalityConfirmed"], false);
        assert_eq!(
            result["data"]["verification"]["domainCommitVerified"],
            false
        );
        assert_eq!(result["data"]["effectConfirmed"], false);
    }

    #[test]
    fn v2_success_shell_does_not_hide_rejection_or_unknown_dispatch() {
        for (outcome, accepted, code) in [
            ("pre-dispatch-rejected", false, "OPERATION_FAILED"),
            ("provider-error", true, "OPERATION_FAILED"),
            ("outcome-unknown", true, "OUTCOME_UNKNOWN"),
        ] {
            let mut calls = 0;
            let failure = execute_with(plan(), |_, _, _, _| {
                calls += 1;
                Ok(reply(outcome, accepted))
            })
            .unwrap_err();
            assert_eq!(calls, 1);
            assert_eq!(failure.code, code);
            assert_eq!(failure.details["automaticRetryProhibited"], accepted);
        }
    }

    #[test]
    fn stale_after_reply_preserves_dispatch_and_stops_retries() {
        let mut calls = 0;
        let failure = execute_with(plan(), |action, _, _, _| {
            calls += 1;
            if action == Action::State {
                Err(AppControlError::new("STALE_SESSION", "stale"))
            } else {
                Ok(reply("replied", true))
            }
        })
        .unwrap_err();
        assert_eq!(calls, 2);
        assert_eq!(failure.code, "TASK_VERIFICATION_FAILED");
        assert_eq!(failure.details["providerCompleted"], true);
        assert_eq!(failure.details["automaticRetryProhibited"], true);
        assert_eq!(failure.details["cause"]["code"], "STALE_SESSION");
    }

    #[test]
    fn verification_consumes_original_deadline_without_a_second_method() {
        let mut plan = plan();
        plan.timeout_ms = 10;
        let mut controls = 0;
        let failure = execute_with(plan, |action, _, _, _| {
            if matches!(action, Action::Control(_)) {
                controls += 1;
                Ok(reply("replied", true))
            } else {
                Ok(state("playing"))
            }
        })
        .unwrap_err();
        assert_eq!(controls, 1);
        assert_eq!(failure.code, "TASK_VERIFICATION_FAILED");
        assert_eq!(failure.details["cause"]["code"], "TIMEOUT");
        assert_eq!(failure.details["providerCompleted"], true);
    }

    #[test]
    fn observation_cannot_be_rebound_to_another_target() {
        let mut observation = state("paused");
        observation["data"]["sessionId"] = json!("s2:m:fedcba9876543210");
        assert!(project(Action::State, Some(TARGET), observation).is_err());
    }
}
