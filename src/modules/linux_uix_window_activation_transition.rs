//! 协调确认、前景同意、UIX 窗口激活与提交后焦点观察。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        WindowActionFailure, WindowActivationTransitionFailure, WindowActivationTransitionOutcome,
        perform_window_activation_transition,
    },
    capabilities,
    components::uix_window_activation_transition_contract::UixWindowActivationTransitionInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

const CAPABILITY: &str = capabilities::WINDOW_ACTIVATE_TRANSITION;

trait UixWindowActivationTransitionPort {
    fn activate_and_wait(
        &self,
        session_id: &str,
        input: &UixWindowActivationTransitionInput,
    ) -> Result<WindowActivationTransitionOutcome, WindowActivationTransitionFailure>;
}

struct SystemUixWindowActivationTransition;

impl UixWindowActivationTransitionPort for SystemUixWindowActivationTransition {
    fn activate_and_wait(
        &self,
        session_id: &str,
        input: &UixWindowActivationTransitionInput,
    ) -> Result<WindowActivationTransitionOutcome, WindowActivationTransitionFailure> {
        perform_window_activation_transition(session_id, input)
    }
}

/// 提交一次已确认激活请求，并等待同一精确窗口代际报告焦点。
pub(crate) fn perform(
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    value: &Value,
) -> AppResult<Value> {
    perform_with(
        &SystemUixWindowActivationTransition,
        session_id,
        confirmed,
        foreground_consent,
        value,
    )
}

fn perform_with(
    port: &impl UixWindowActivationTransitionPort,
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认与前景影响同意必须先于 input、target 与任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX window activation transition requires explicit confirmation.",
        ));
    }
    if !foreground_consent {
        return Err(AppControlError::new(
            "FOREGROUND_CONSENT_REQUIRED",
            "UIX window activation transition requires upfront foreground-impact consent.",
        ));
    }
    let input = UixWindowActivationTransitionInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .activate_and_wait(session_id, &input)
        .map_err(|failure| transition_error(failure, &input))?;
    let observation = outcome.observation;
    let safety = json!({
        "provider": "uix-agent-v1",
        "providerCache": "disabled",
        "sameAuthenticatedConnectionUsed": true,
        "windowGenerationFixed": true,
        "focusedFieldNegotiated": true,
        "providerPolling": true,
        "waitProtocolUsed": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "hostForegroundActivationRequested": true,
        "desktopInputInjected": false,
        "x11Used": false,
        "fallback": "none",
    });
    let Value::Object(mut result) = json!({
        "ok": true,
        "contractVersion": "act/window-activation-transition/v1",
        "capability": CAPABILITY,
        "targetId": session_id,
        "outcome": "focused",
        "dispatchState": "completed",
        "accepted": true,
        "activationRequestAccepted": true,
        "actionAccepted": true,
        "actionRevision": outcome.action.revision,
        "actionPresentedRevision": outcome.action.presented_revision,
        "actionSettled": outcome.action.settled,
        "revision": observation.revision,
        "presentedRevision": observation.presented_revision,
        "focused": true,
        "focusObservedAfterDispatch": true,
        "focusConfirmed": true,
        "focusMatchedAfterDispatch": true,
        "targetGenerationCurrentAfterDispatch": true,
        "pollCount": observation.poll_count,
    }) else {
        unreachable!("窗口 activation transition 事实投影固定为 JSON 对象");
    };
    let Value::Object(execution) = json!({
        "sameAuthenticatedConnectionUsed": true,
        "providerPolling": true,
        "waitProtocolUsed": false,
        "effectConfirmed": false,
        "causalityConfirmed": false,
        "finalFocusStateGuaranteed": false,
        "finalStateReached": false,
        "effectObservation": "same-connection-exact-generation-focused-after-dispatch-no-causal-or-persistence-claim",
        "windowReResolved": true,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "foregroundConsentEvaluatedBeforeDiscovery": true,
        "foregroundConsentRequired": true,
        "foregroundActivationRequested": true,
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "pollIntervalMs": input.poll_interval_ms(),
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "host-foreground",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety,
    }) else {
        unreachable!("窗口 activation transition 执行投影固定为 JSON 对象");
    };
    result.extend(execution);
    Ok(Value::Object(result))
}

fn transition_error(
    failure: WindowActivationTransitionFailure,
    input: &UixWindowActivationTransitionInput,
) -> AppControlError {
    if failure.accepted_may_have_occurred {
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX activation transition lost a trusted exact-generation focus observation after dispatch may have begun; automatic retry is prohibited.",
            failure_details(&failure, input, "trusted-focus-observation-lost"),
        );
    }
    match failure.source {
        WindowActionFailure::Transport(source) => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(&failure, input, "transport-before-dispatch"),
            );
            error
        }
        WindowActionFailure::UnsupportedAction => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish activate_window, list_windows, and focused for an activation transition.",
            failure_details(&failure, input, "agent-preflight-missing"),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before activation dispatch.",
            failure_details(&failure, input, "window-revision-changed"),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids window activation.",
            failure_details(&failure, input, "application-policy-forbidden"),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for activation.",
            failure_details(&failure, input, "window-not-presentable"),
        ),
        WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX activation request may have begun without a trusted focus observation.",
            failure_details(&failure, input, "unexpected-unsafe-action-failure"),
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
    failure: &WindowActivationTransitionFailure,
    input: &UixWindowActivationTransitionInput,
    reason: &'static str,
) -> Value {
    json!({
        "reason": reason,
        "actionAccepted": failure.action_accepted,
        "actionRevision": failure.action_revision,
        "actionPresentedRevision": failure.action_presented_revision,
        "actionSettled": failure.action_settled,
        "acceptedMayHaveOccurred": failure.accepted_may_have_occurred,
        "pollCount": failure.poll_count,
        "focusObservedAfterDispatch": false,
        "focusConfirmed": false,
        "finalFocusStateGuaranteed": false,
        "providerPolling": true,
        "waitProtocolUsed": false,
        "automaticRetryProhibited": failure.accepted_may_have_occurred,
        "retrySafe": false,
        "requiredNextStep": if failure.accepted_may_have_occurred {
            "discover-current-uix-windows-and-observe-focus-before-any-new-activation"
        } else {
            "refresh-target-and-reassess"
        },
        "pollIntervalMs": input.poll_interval_ms(),
        "timeoutMs": input.timeout_ms(),
        "fallback": "none",
    })
}

#[cfg(test)]
mod tests {
    use crate::adapters::linux::uix_agent::{ActionOutcome, Failure, WindowFocusObservation};

    use super::*;

    struct FixturePort {
        result: Result<WindowActivationTransitionOutcome, WindowActivationTransitionFailure>,
    }

    impl UixWindowActivationTransitionPort for FixturePort {
        fn activate_and_wait(
            &self,
            _: &str,
            _: &UixWindowActivationTransitionInput,
        ) -> Result<WindowActivationTransitionOutcome, WindowActivationTransitionFailure> {
            self.result
        }
    }

    fn success(settled: bool) -> WindowActivationTransitionOutcome {
        WindowActivationTransitionOutcome {
            action: ActionOutcome {
                revision: 6,
                presented_revision: 6,
                settled,
                application_confirmation_performed: false,
            },
            observation: WindowFocusObservation {
                focused: true,
                revision: 6,
                presented_revision: 6,
                poll_count: 2,
            },
        }
    }

    #[test]
    fn confirmation_and_foreground_consent_precede_input_target_and_provider() {
        let port = FixturePort {
            result: Ok(success(true)),
        };
        let Err(unconfirmed) = perform_with(&port, "not-a-target", false, false, &Value::Null)
        else {
            panic!("未确认激活 transition 必须优先失败");
        };
        assert_eq!(unconfirmed.code, "CONFIRMATION_REQUIRED");
        let Err(no_foreground) = perform_with(&port, "not-a-target", true, false, &Value::Null)
        else {
            panic!("缺失前景同意必须先于 input 失败");
        };
        assert_eq!(no_foreground.code, "FOREGROUND_CONSENT_REQUIRED");
    }

    #[test]
    fn focused_observation_succeeds_without_causal_or_persistence_claim() {
        let port = FixturePort {
            result: Ok(success(false)),
        };
        let Ok(result) = perform_with(
            &port,
            "s2:w:0123456789abcdef",
            true,
            true,
            &json!({ "pollIntervalMs": 20, "timeoutMs": 1000 }),
        ) else {
            panic!("同连接精确代际焦点观察必须成功");
        };
        assert_eq!(result["actionSettled"], false);
        assert_eq!(result["focusConfirmed"], true);
        assert_eq!(result["causalityConfirmed"], false);
        assert_eq!(result["finalFocusStateGuaranteed"], false);
        assert_eq!(result["waitProtocolUsed"], false);
        assert_eq!(result["pollCount"], 2);
    }

    #[test]
    fn post_activation_observation_loss_is_unknown_and_non_retryable() {
        let failure = WindowActivationTransitionFailure {
            source: WindowActionFailure::Transport(Failure::Unavailable),
            accepted_may_have_occurred: true,
            action_accepted: true,
            action_revision: Some(6),
            action_presented_revision: Some(6),
            action_settled: Some(true),
            poll_count: 1,
        };
        let port = FixturePort {
            result: Err(failure),
        };
        let Err(error) = perform_with(&port, "s2:w:0123456789abcdef", true, true, &json!({}))
        else {
            panic!("激活后失去焦点观察必须失败");
        };
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
