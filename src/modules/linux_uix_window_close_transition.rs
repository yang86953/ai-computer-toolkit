//! 协调确认、UIX 窗口关闭请求与精确 generation 终态同步。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, WindowActionFailure, WindowCloseTransitionFailure,
        WindowCloseTransitionFailureSource, WindowCloseTransitionOutcome,
    },
    capabilities,
    components::uix_window_close_transition_contract::UixWindowCloseTransitionInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

const CAPABILITY: &str = capabilities::WINDOW_CLOSE_TRANSITION;

trait UixWindowCloseTransitionPort {
    fn close_and_wait(
        &self,
        session_id: &str,
        input: &UixWindowCloseTransitionInput,
    ) -> Result<WindowCloseTransitionOutcome, WindowCloseTransitionFailure>;
}

struct SystemUixWindowCloseTransition;

impl UixWindowCloseTransitionPort for SystemUixWindowCloseTransition {
    fn close_and_wait(
        &self,
        session_id: &str,
        input: &UixWindowCloseTransitionInput,
    ) -> Result<WindowCloseTransitionOutcome, WindowCloseTransitionFailure> {
        uix_agent::perform_window_close_transition(session_id, input)
    }
}

/// 提交一次已确认关闭请求，并只接受同连接显式 closed 终态作为成功。
pub(crate) fn close(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    close_with(
        &SystemUixWindowCloseTransition,
        session_id,
        confirmed,
        value,
    )
}

fn close_with(
    port: &impl UixWindowCloseTransitionPort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 与任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX window close transition requires explicit confirmation.",
        ));
    }
    let input = UixWindowCloseTransitionInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .close_and_wait(session_id, &input)
        .map_err(|failure| close_transition_error(failure, &input))?;
    let safety = json!({
        "provider": "uix-agent-v1",
        "providerCache": "disabled",
        "sameAuthenticatedConnectionUsed": true,
        "windowGenerationFixed": true,
        "waitProtocolUsed": true,
        "providerPolling": false,
        "terminalReplyReceived": true,
        "connectionCloseAcceptedAsProof": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "hostForegroundActivationRequested": false,
        "desktopInputInjected": false,
        "windowInventoryPolled": false,
        "x11Used": false,
        "fallback": "none",
    });
    // 分段构造固定字段，避免深层 JSON 宏展开改变公开事实。
    let Value::Object(mut result) = json!({
        "ok": true,
        "contractVersion": "act/window-close-transition/v1",
        "capability": CAPABILITY,
        "targetId": session_id,
        "outcome": "closed",
        "dispatchState": "completed",
        "accepted": true,
        "actionAccepted": true,
        "actionRevision": outcome.action.revision,
        "actionPresentedRevision": outcome.action.presented_revision,
        "actionSettled": outcome.action.settled,
        "revision": outcome.closed.revision,
        "presentedRevision": outcome.closed.presented_revision,
        "closed": true,
        "closeConfirmed": true,
        "exactGenerationConfirmed": true,
        "closedObservedAfterDispatch": true,
    }) else {
        unreachable!("窗口 close transition 事实投影固定为 JSON 对象");
    };
    let Value::Object(execution) = json!({
        "causalityConfirmed": false,
        "applicationClosedConfirmed": false,
        "finalStateReached": true,
        "effectObservation": "same-connection-exact-generation-closed-after-dispatch-no-causal-claim",
        "sameAuthenticatedConnectionUsed": true,
        "waitProtocolUsed": true,
        "providerPolling": false,
        "windowReResolved": true,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "same-session-no-focus",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety,
    }) else {
        unreachable!("窗口 close transition 执行投影固定为 JSON 对象");
    };
    result.extend(execution);
    Ok(Value::Object(result))
}

fn close_transition_error(
    failure: WindowCloseTransitionFailure,
    input: &UixWindowCloseTransitionInput,
) -> AppControlError {
    if failure.accepted_may_have_occurred {
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX window close transition lost a trusted explicit closed terminal after dispatch may have begun; automatic retry is prohibited.",
            failure_details(&failure, input, "trusted-closed-terminal-lost"),
        );
    }
    match failure.source {
        WindowCloseTransitionFailureSource::TransportBeforeDispatch(source) => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(&failure, input, "transport-before-dispatch"),
            );
            error
        }
        WindowCloseTransitionFailureSource::NegotiationUnavailable => {
            AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The authenticated UIX Agent does not publish close_window and wait for a close transition.",
                failure_details(&failure, input, "agent-preflight-missing"),
            )
        }
        WindowCloseTransitionFailureSource::Action(source) => {
            safe_action_error(source, &failure, input)
        }
        WindowCloseTransitionFailureSource::ClosedObservation(_) => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX close request was accepted but an explicit exact-generation closed terminal was not received.",
            failure_details(&failure, input, "closed-observation-untrusted"),
        ),
    }
}

fn safe_action_error(
    source: WindowActionFailure,
    failure: &WindowCloseTransitionFailure,
    input: &UixWindowCloseTransitionInput,
) -> AppControlError {
    match source {
        WindowActionFailure::UnsupportedAction => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish close_window.",
            failure_details(failure, input, "agent-action-not-published"),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before close dispatch.",
            failure_details(failure, input, "window-revision-changed"),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids window close.",
            failure_details(failure, input, "application-policy-forbidden"),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable.",
            failure_details(failure, input, "window-not-presentable"),
        ),
        WindowActionFailure::Transport(_)
        | WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX close request may have begun without a trusted closed terminal.",
            failure_details(failure, input, "unexpected-unsafe-action-failure"),
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
    failure: &WindowCloseTransitionFailure,
    input: &UixWindowCloseTransitionInput,
    reason: &'static str,
) -> Value {
    json!({
        "reason": reason,
        "actionAccepted": failure.action_accepted,
        "actionRevision": failure.action_revision,
        "actionPresentedRevision": failure.action_presented_revision,
        "actionSettled": failure.action_settled,
        "acceptedMayHaveOccurred": failure.accepted_may_have_occurred,
        "explicitClosedTerminalReceived": false,
        "connectionCloseAcceptedAsProof": false,
        "automaticRetryProhibited": failure.accepted_may_have_occurred,
        "retrySafe": false,
        "requiredNextStep": if failure.accepted_may_have_occurred {
            "discover-current-uix-windows-before-any-new-close"
        } else {
            "refresh-target-and-reassess"
        },
        "timeoutMs": input.timeout_ms(),
        "fallback": "none",
    })
}

#[cfg(test)]
mod tests {
    use crate::adapters::linux::uix_agent::{
        ActionOutcome, Failure, RevisionWaitOutcome, RevisionWaitOutcomeKind,
    };

    use super::*;

    struct FixturePort {
        result: Result<WindowCloseTransitionOutcome, WindowCloseTransitionFailure>,
    }

    impl UixWindowCloseTransitionPort for FixturePort {
        fn close_and_wait(
            &self,
            _: &str,
            _: &UixWindowCloseTransitionInput,
        ) -> Result<WindowCloseTransitionOutcome, WindowCloseTransitionFailure> {
            self.result
        }
    }

    fn success(settled: bool) -> WindowCloseTransitionOutcome {
        WindowCloseTransitionOutcome {
            action: ActionOutcome {
                revision: 6,
                presented_revision: 6,
                settled,
                application_confirmation_performed: false,
            },
            closed: RevisionWaitOutcome {
                kind: RevisionWaitOutcomeKind::Closed,
                revision: 7,
                presented_revision: 6,
                closed: true,
            },
        }
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let port = FixturePort {
            result: Ok(success(true)),
        };
        let Err(error) = close_with(&port, "not-a-target", false, &Value::Null) else {
            panic!("未确认 close transition 必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn explicit_closed_terminal_succeeds_without_causal_claim() {
        let port = FixturePort {
            result: Ok(success(false)),
        };
        let Ok(result) = close_with(
            &port,
            "s2:w:0123456789abcdef",
            true,
            &json!({ "timeoutMs": 1000 }),
        ) else {
            panic!("同连接显式 closed 终态必须成功");
        };
        assert_eq!(result["capability"], CAPABILITY);
        assert_eq!(result["actionSettled"], false);
        assert_eq!(result["closeConfirmed"], true);
        assert_eq!(result["exactGenerationConfirmed"], true);
        assert_eq!(result["causalityConfirmed"], false);
        assert_eq!(result["applicationClosedConfirmed"], false);
        assert_eq!(result["safety"]["terminalReplyReceived"], true);
        assert_eq!(result["safety"]["connectionCloseAcceptedAsProof"], false);
    }

    #[test]
    fn closed_observation_loss_is_unknown_and_non_retryable() {
        let failure = WindowCloseTransitionFailure {
            source: WindowCloseTransitionFailureSource::ClosedObservation(Failure::Unavailable),
            accepted_may_have_occurred: true,
            action_accepted: true,
            action_revision: Some(6),
            action_presented_revision: Some(6),
            action_settled: Some(true),
        };
        let port = FixturePort {
            result: Err(failure),
        };
        let Err(error) = close_with(&port, "s2:w:0123456789abcdef", true, &json!({})) else {
            panic!("动作后失去 closed 终态必须失败");
        };
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["automaticRetryProhibited"], true);
        assert_eq!(error.details["explicitClosedTerminalReceived"], false);
    }
}
