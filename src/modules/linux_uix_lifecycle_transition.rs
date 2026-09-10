//! 协调确认、前景同意与 UIX 窗口生命周期 transition 事实投影。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, WindowActionFailure, WindowLifecycleTransitionFailure,
        WindowLifecycleTransitionOutcome,
    },
    capabilities,
    components::uix_window_lifecycle_transition_contract::UixWindowLifecycleTransitionInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

const CAPABILITY: &str = capabilities::WINDOW_LIFECYCLE_TRANSITION;

trait UixWindowLifecycleTransitionPort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixWindowLifecycleTransitionInput,
    ) -> Result<WindowLifecycleTransitionOutcome, WindowLifecycleTransitionFailure>;
}

struct SystemUixWindowLifecycleTransition;

impl UixWindowLifecycleTransitionPort for SystemUixWindowLifecycleTransition {
    fn perform(
        &self,
        session_id: &str,
        input: &UixWindowLifecycleTransitionInput,
    ) -> Result<WindowLifecycleTransitionOutcome, WindowLifecycleTransitionFailure> {
        uix_agent::perform_window_lifecycle_transition(session_id, input)
    }
}

/// 对精确协作式窗口执行一次已授权动作，并等待框架当前条件命中。
pub(crate) fn perform(
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    value: &Value,
) -> AppResult<Value> {
    perform_with(
        &SystemUixWindowLifecycleTransition,
        session_id,
        confirmed,
        foreground_consent,
        value,
    )
}

fn perform_with(
    port: &impl UixWindowLifecycleTransitionPort,
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX window lifecycle transition requires explicit confirmation.",
        ));
    }
    // 可见窗口 mutation 的前景影响同意必须同样在发现前成立。
    if !foreground_consent {
        return Err(AppControlError::new(
            "FOREGROUND_CONSENT_REQUIRED",
            "UIX window lifecycle transition requires upfront foreground-impact consent.",
        ));
    }
    let input = UixWindowLifecycleTransitionInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .perform(session_id, &input)
        .map_err(|failure| transition_error(failure, &input))?;
    let observation = outcome.observation;
    let client_size = json!({
        "width": observation.logical_width,
        "height": observation.logical_height,
    });
    let window_state = json!({
        "maximized": observation.maximized,
        "minimized": observation.minimized,
        "fullscreen": observation.fullscreen,
    });
    let safety = json!({
        "provider": "uix-agent-v1",
        "sameAuthenticatedConnectionUsed": true,
        "windowGenerationFixed": true,
        "providerPolling": true,
        "waitProtocolUsed": false,
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "arbitraryWindowMoveSupported": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "x11Used": false,
        "fallback": "none",
    });
    let Value::Object(mut result) = json!({
        "capability": CAPABILITY,
        "targetId": session_id,
        "action": input.action().public_value(),
        "condition": input.condition().public_value(),
        "actionAccepted": true,
        "actionRevision": outcome.action_revision,
        "actionPresentedRevision": outcome.action_presented_revision,
        "actionSettled": outcome.action_settled,
        "observationSource": "uix-framework-current",
        "coordinateSpace": "client-logical-px",
        "visible": observation.visible,
        "presentable": observation.presentable,
        "focused": observation.focused,
        "clientSize": client_size,
        "windowState": window_state,
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "frameworkConditionMatchedAfterDispatch": true,
        "revision": observation.revision,
        "presentedRevision": observation.presented_revision,
        "pollCount": observation.poll_count,
    }) else {
        unreachable!("transition 事实投影固定为 JSON 对象");
    };
    let Value::Object(execution) = json!({
        "sameAuthenticatedConnectionUsed": true,
        "providerPolling": true,
        "waitProtocolUsed": false,
        "effectConfirmed": false,
        "causalityConfirmed": false,
        "finalStateReached": false,
        "compositorFinalStateConfirmed": false,
        "windowReResolved": true,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "foregroundConsentEvaluatedBeforeDiscovery": true,
        "foregroundConsentRequired": true,
        "foregroundImpactAuthorized": true,
        "hostForegroundActivationRequested": false,
        "effectObservation": "framework-condition-after-dispatch-no-causal-claim",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "pollIntervalMs": input.poll_interval_ms(),
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "host-foreground",
        "safety": safety,
    }) else {
        unreachable!("transition 执行投影固定为 JSON 对象");
    };
    result.extend(execution);
    Ok(Value::Object(result))
}

fn transition_error(
    failure: WindowLifecycleTransitionFailure,
    input: &UixWindowLifecycleTransitionInput,
) -> AppControlError {
    if failure.accepted_may_have_occurred {
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX lifecycle transition lost a trusted final after dispatch may have begun; automatic retry is prohibited.",
            failure_details(&failure, input, "trusted-final-lost", true),
        );
    }
    match failure.source {
        WindowActionFailure::Transport(source) => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(&failure, input, "transport-before-dispatch", false),
            );
            error
        }
        WindowActionFailure::UnsupportedAction => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish the complete lifecycle transition contract.",
            failure_details(&failure, input, "agent-preflight-missing", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before lifecycle transition dispatch.",
            failure_details(&failure, input, "window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids the lifecycle transition.",
            failure_details(&failure, input, "application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for lifecycle transition dispatch.",
            failure_details(&failure, input, "window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX lifecycle transition lost a trusted final after dispatch may have begun.",
            failure_details(&failure, input, "trusted-final-lost", true),
        ),
    }
}

fn merge_failure_details(target: &mut Value, extra: Value) {
    let (Some(target), Some(extra)) = (target.as_object_mut(), extra.as_object()) else {
        return;
    };
    target.extend(extra.clone());
}

fn failure_details(
    failure: &WindowLifecycleTransitionFailure,
    input: &UixWindowLifecycleTransitionInput,
    reason: &'static str,
    accepted_may_have_occurred: bool,
) -> Value {
    json!({
        "action": input.action().public_value(),
        "condition": input.condition().public_value(),
        "reason": reason,
        "actionAccepted": failure.action_accepted,
        "actionRevision": failure.action_revision,
        "actionPresentedRevision": failure.action_presented_revision,
        "actionSettled": failure.action_settled,
        "pollCount": failure.poll_count,
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
        "foregroundImpactAuthorized": true,
        "transactionSemantics": false,
        "rollbackSemantics": false,
        "automaticRetryProhibited": accepted_may_have_occurred,
        "retrySafe": false,
        "requiredNextStep": "inspect-current-uix-window-before-any-new-lifecycle-transition",
    })
}

#[cfg(test)]
mod tests {
    use crate::adapters::linux::uix_agent::{Failure, WindowStateWaitObservation};

    use super::*;

    struct FixturePort {
        result: Result<WindowLifecycleTransitionOutcome, WindowLifecycleTransitionFailure>,
    }

    impl UixWindowLifecycleTransitionPort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixWindowLifecycleTransitionInput,
        ) -> Result<WindowLifecycleTransitionOutcome, WindowLifecycleTransitionFailure> {
            self.result
        }
    }

    fn input() -> Value {
        json!({
            "action": { "type": "maximize" },
            "condition": { "type": "window-flags", "maximized": true },
            "pollIntervalMs": 20,
            "timeoutMs": 1000
        })
    }

    fn observation() -> WindowStateWaitObservation {
        WindowStateWaitObservation {
            visible: true,
            presentable: true,
            focused: false,
            logical_width: 800,
            logical_height: 600,
            maximized: true,
            minimized: false,
            fullscreen: false,
            revision: 6,
            presented_revision: 6,
            poll_count: 1,
        }
    }

    #[test]
    fn confirmation_and_foreground_consent_precede_input_target_and_provider() {
        let Err(error) = perform("not-a-target", false, false, &Value::Null) else {
            panic!("未确认 transition 必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");

        let Err(error) = perform("not-a-target", true, false, &Value::Null) else {
            panic!("未同意前景影响必须在输入解析前失败");
        };
        assert_eq!(error.code, "FOREGROUND_CONSENT_REQUIRED");
    }

    #[test]
    fn success_publishes_action_observation_and_non_final_state_facts() {
        let port = FixturePort {
            result: Ok(WindowLifecycleTransitionOutcome {
                action_revision: 6,
                action_presented_revision: 6,
                action_settled: false,
                observation: observation(),
            }),
        };
        let Ok(result) = perform_with(&port, "s2:w:0123456789abcdef", true, true, &input()) else {
            panic!("transition 成功事实必须投影");
        };
        assert_eq!(result["capability"], CAPABILITY);
        assert_eq!(result["action"]["type"], "maximize");
        assert_eq!(result["condition"]["type"], "window-flags");
        assert_eq!(result["actionAccepted"], true);
        assert_eq!(result["actionSettled"], false);
        assert_eq!(result["observationSource"], "uix-framework-current");
        assert_eq!(result["coordinateSpace"], "client-logical-px");
        assert_eq!(result["visible"], true);
        assert_eq!(result["presentable"], true);
        assert_eq!(result["focused"], false);
        assert_eq!(result["clientSize"]["width"], 800);
        assert_eq!(result["windowState"]["maximized"], true);
        assert_eq!(result["frameworkConditionMatchedAfterDispatch"], true);
        assert_eq!(result["actionRevision"], 6);
        assert_eq!(result["revision"], 6);
        assert_eq!(result["pollCount"], 1);
        assert_eq!(result["sameAuthenticatedConnectionUsed"], true);
        assert_eq!(result["providerPolling"], true);
        assert_eq!(result["effectConfirmed"], false);
        assert_eq!(result["causalityConfirmed"], false);
        assert_eq!(result["finalStateReached"], false);
        assert_eq!(result["safety"]["desktopInputInjected"], false);
        assert_eq!(result["safety"]["fallback"], "none");

        let envelope = json!({
            "ok": true,
            "app": "app",
            "verb": "apply",
            "capability": CAPABILITY,
            "targetId": "s2:w:0123456789abcdef",
            "executionRealm": "host-foreground",
            "requiredExecutionRealm": "host-foreground",
            "executionRealmCertified": true,
            "isolationRequirement": "standard",
            "hostImpactPolicy": "background-preferred",
            "data": result,
            "meta": {
                "foreground": {
                    "activationRequested": false,
                    "visibleMutationAuthorized": true,
                    "frameworkConditionObservationRequested": true
                },
                "targeting": "opaque exact window generation"
            }
        });
        let Ok(schema) = serde_json::from_str::<Value>(include_str!(
            "../../contracts/v1/window-lifecycle-transition.schema.json"
        )) else {
            panic!("生命周期 transition 结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "生命周期 transition 成功结果必须匹配公开 schema"
        );
    }

    #[test]
    fn dispatch_after_failure_maps_unknown_and_prohibits_retry() {
        let Ok(parsed) = UixWindowLifecycleTransitionInput::parse(&input()) else {
            panic!("测试 transition 输入必须有效");
        };
        let error = transition_error(
            WindowLifecycleTransitionFailure {
                source: WindowActionFailure::Transport(Failure::Timeout),
                accepted_may_have_occurred: true,
                action_accepted: true,
                action_revision: Some(6),
                action_presented_revision: Some(6),
                action_settled: Some(false),
                poll_count: 1,
            },
            &parsed,
        );
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["automaticRetryProhibited"], true);
        assert_eq!(error.details["retrySafe"], false);
        assert_eq!(error.details["actionAccepted"], true);
        assert_eq!(error.details["pollCount"], 1);
    }
}
