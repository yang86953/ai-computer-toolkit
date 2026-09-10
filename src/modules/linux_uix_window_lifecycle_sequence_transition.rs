//! 协调确认、前景同意与 UIX 窗口生命周期序列后的框架状态观察。

use serde_json::{Map, Value, json};

use crate::{
    adapters::linux::uix_agent::{
        WindowActionFailure, WindowLifecycleSequenceFailure,
        WindowLifecycleSequenceTransitionFailure, WindowLifecycleSequenceTransitionFailureSource,
        WindowLifecycleSequenceTransitionOutcome, perform_window_lifecycle_sequence_transition,
    },
    capabilities,
    components::uix_window_lifecycle_sequence_transition_contract::UixWindowLifecycleSequenceTransitionInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

const CAPABILITY: &str = capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION;
const PROVIDER: &str = "uix-agent-v1";

trait UixWindowLifecycleSequenceTransitionPort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixWindowLifecycleSequenceTransitionInput,
    ) -> Result<WindowLifecycleSequenceTransitionOutcome, WindowLifecycleSequenceTransitionFailure>;
}

struct SystemUixWindowLifecycleSequenceTransition;

impl UixWindowLifecycleSequenceTransitionPort for SystemUixWindowLifecycleSequenceTransition {
    fn perform(
        &self,
        session_id: &str,
        input: &UixWindowLifecycleSequenceTransitionInput,
    ) -> Result<WindowLifecycleSequenceTransitionOutcome, WindowLifecycleSequenceTransitionFailure>
    {
        perform_window_lifecycle_sequence_transition(session_id, input)
    }
}

/// 对精确协作式窗口执行一次已确认、已同意前景影响的生命周期序列，并观察最终状态。
pub(crate) fn perform(
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    value: &Value,
) -> AppResult<Value> {
    perform_with(
        &SystemUixWindowLifecycleSequenceTransition,
        session_id,
        confirmed,
        foreground_consent,
        value,
    )
}

fn perform_with(
    port: &impl UixWindowLifecycleSequenceTransitionPort,
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认与可见窗口前景影响同意必须先于 input、target 和 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX window lifecycle sequence transition requires explicit confirmation.",
        ));
    }
    if !foreground_consent {
        return Err(AppControlError::new(
            "FOREGROUND_CONSENT_REQUIRED",
            "UIX window lifecycle sequence transition requires upfront foreground-impact consent.",
        ));
    }
    let input = UixWindowLifecycleSequenceTransitionInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .perform(session_id, &input)
        .map_err(|failure| transition_error(failure, &input))?;
    debug_assert_eq!(
        outcome.sequence.actions_accepted,
        input.actions_requested() as u16
    );

    let actions = input
        .actions()
        .iter()
        .map(|action| action.public_value())
        .collect::<Vec<_>>();
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
        "provider": PROVIDER,
        "sameAuthenticatedConnectionUsed": true,
        "windowGenerationFixed": true,
        "revisionChainUsed": true,
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
        "action": input.action(),
        "actions": actions,
        "actionsRequested": input.actions_requested(),
        "actionsAccepted": outcome.sequence.actions_accepted,
        "distinctActionKinds": input.distinct_action_kinds(),
        "intervalMs": input.interval_ms(),
        "plannedDurationMs": input.planned_duration_ms(),
        "condition": input.public_condition(),
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
        "frameworkConditionMatchedAfterCompleteSequence": true,
        "sameAuthenticatedConnectionUsed": true,
        "sequenceRevision": outcome.sequence.revision,
        "sequencePresentedRevision": outcome.sequence.presented_revision,
        "sequenceSettled": outcome.sequence.settled,
        "revision": observation.revision,
        "presentedRevision": observation.presented_revision,
        "pollCount": observation.poll_count,
    }) else {
        unreachable!("窗口生命周期序列 transition 事实投影固定为 JSON 对象");
    };
    let Value::Object(execution) = json!({
        "providerPolling": true,
        "waitProtocolUsed": false,
        "sameConnectionRevisionChain": true,
        "transactionSemantics": false,
        "rollbackSemantics": false,
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
        "effectObservation": "framework-condition-after-complete-sequence-no-causal-claim",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "pollIntervalMs": input.poll_interval_ms(),
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "host-foreground",
        "safety": safety,
    }) else {
        unreachable!("窗口生命周期序列 transition 执行投影固定为 JSON 对象");
    };
    result.extend(execution);
    Ok(Value::Object(result))
}

fn transition_error(
    failure: WindowLifecycleSequenceTransitionFailure,
    input: &UixWindowLifecycleSequenceTransitionInput,
) -> AppControlError {
    if failure.accepted_may_have_occurred {
        let reason = if failure.sequence_completed {
            "trusted-framework-state-observation-lost-after-complete-lifecycle-sequence"
        } else {
            "partial-lifecycle-sequence-or-final-state-indeterminate"
        };
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX lifecycle sequence transition may have executed without a trusted final framework state; automatic retry is prohibited.",
            failure_details(&failure, input, reason, true),
        );
    }

    match failure.source {
        WindowLifecycleSequenceTransitionFailureSource::Transport(source) => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(&failure, input, "transport-before-dispatch", false),
            );
            error
        }
        WindowLifecycleSequenceTransitionFailureSource::NegotiationUnavailable => {
            AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The authenticated UIX Agent does not publish perform, the requested lifecycle actions, list_windows, and all six window state fields required for this transition.",
                failure_details(&failure, input, "agent-preflight-missing", false),
            )
        }
        WindowLifecycleSequenceTransitionFailureSource::Sequence(sequence) => {
            safe_sequence_error(sequence, &failure, input)
        }
        WindowLifecycleSequenceTransitionFailureSource::Observation(_) => {
            AppControlError::with_details(
                "OUTCOME_UNKNOWN",
                "The complete UIX lifecycle sequence lost its trusted framework state observation; automatic retry is prohibited.",
                failure_details(
                    &failure,
                    input,
                    "framework-state-observation-untrusted",
                    true,
                ),
            )
        }
    }
}

fn safe_sequence_error(
    sequence: WindowLifecycleSequenceFailure,
    failure: &WindowLifecycleSequenceTransitionFailure,
    input: &UixWindowLifecycleSequenceTransitionInput,
) -> AppControlError {
    match sequence.source {
        WindowActionFailure::Transport(source) if !failure.accepted_may_have_occurred => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(failure, input, "transport-before-dispatch", false),
            );
            error
        }
        WindowActionFailure::UnsupportedAction => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish the complete requested lifecycle sequence.",
            failure_details(failure, input, "agent-actions-not-published", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before lifecycle sequence transition dispatch.",
            failure_details(failure, input, "window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids the lifecycle sequence transition.",
            failure_details(failure, input, "application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for lifecycle sequence transition dispatch.",
            failure_details(failure, input, "window-not-presentable", false),
        ),
        WindowActionFailure::Transport(_)
        | WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX lifecycle sequence transition may have partially executed; automatic retry is prohibited.",
            failure_details(failure, input, "unexpected-unsafe-sequence-failure", true),
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
    failure: &WindowLifecycleSequenceTransitionFailure,
    input: &UixWindowLifecycleSequenceTransitionInput,
    reason: &'static str,
    accepted_may_have_occurred: bool,
) -> Value {
    let mut details = Map::new();
    details.insert(
        "action".to_owned(),
        Value::String(input.action().to_owned()),
    );
    details.insert(
        "actions".to_owned(),
        Value::Array(
            input
                .actions()
                .iter()
                .map(|action| action.public_value())
                .collect(),
        ),
    );
    details.insert(
        "actionsRequested".to_owned(),
        Value::from(input.actions_requested()),
    );
    details.insert(
        "actionsAccepted".to_owned(),
        Value::from(failure.actions_accepted),
    );
    details.insert(
        "distinctActionKinds".to_owned(),
        Value::from(input.distinct_action_kinds()),
    );
    details.insert("intervalMs".to_owned(), Value::from(input.interval_ms()));
    details.insert(
        "plannedDurationMs".to_owned(),
        Value::from(input.planned_duration_ms()),
    );
    details.insert("condition".to_owned(), input.public_condition());
    details.insert(
        "observationSource".to_owned(),
        Value::String("uix-framework-current".to_owned()),
    );
    details.insert("reason".to_owned(), Value::String(reason.to_owned()));
    details.insert(
        "sequenceCompleted".to_owned(),
        Value::Bool(failure.sequence_completed),
    );
    details.insert(
        "sequenceRevision".to_owned(),
        failure.sequence_revision.map_or(Value::Null, Value::from),
    );
    details.insert(
        "sequencePresentedRevision".to_owned(),
        failure
            .sequence_presented_revision
            .map_or(Value::Null, Value::from),
    );
    details.insert(
        "sequenceSettled".to_owned(),
        failure.sequence_settled.map_or(Value::Null, Value::Bool),
    );
    details.insert(
        "frameworkConditionMatchedAfterCompleteSequence".to_owned(),
        Value::Bool(false),
    );
    details.insert(
        "acceptedMayHaveOccurred".to_owned(),
        Value::Bool(accepted_may_have_occurred),
    );
    details.insert("transactionSemantics".to_owned(), Value::Bool(false));
    details.insert("rollbackSemantics".to_owned(), Value::Bool(false));
    details.insert("effectConfirmed".to_owned(), Value::Bool(false));
    details.insert("causalityConfirmed".to_owned(), Value::Bool(false));
    details.insert("finalStateReached".to_owned(), Value::Bool(false));
    details.insert(
        "automaticRetryProhibited".to_owned(),
        Value::Bool(accepted_may_have_occurred),
    );
    details.insert("retrySafe".to_owned(), Value::Bool(false));
    details.insert(
        "requiredNextStep".to_owned(),
        Value::String(
            if accepted_may_have_occurred {
                "inspect-current-uix-window-before-any-new-lifecycle-sequence-transition"
            } else {
                "refresh-target-and-reassess"
            }
            .to_owned(),
        ),
    );
    details.insert("pollCount".to_owned(), Value::from(failure.poll_count));
    details.insert(
        "pollIntervalMs".to_owned(),
        Value::from(input.poll_interval_ms()),
    );
    details.insert("timeoutMs".to_owned(), Value::from(input.timeout_ms()));
    details.insert("foregroundImpactAuthorized".to_owned(), Value::Bool(true));
    details.insert(
        "hostForegroundActivationRequested".to_owned(),
        Value::Bool(false),
    );
    details.insert("desktopInputInjected".to_owned(), Value::Bool(false));
    details.insert("desktopPointerMoved".to_owned(), Value::Bool(false));
    details.insert("fallback".to_owned(), Value::String("none".to_owned()));
    Value::Object(details)
}

#[cfg(test)]
mod tests {
    use crate::adapters::linux::uix_agent::{
        Failure, WindowLifecycleSequenceOutcome, WindowStateWaitObservation,
    };

    use super::*;

    struct FixturePort {
        result: Result<
            WindowLifecycleSequenceTransitionOutcome,
            WindowLifecycleSequenceTransitionFailure,
        >,
    }

    impl UixWindowLifecycleSequenceTransitionPort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixWindowLifecycleSequenceTransitionInput,
        ) -> Result<
            WindowLifecycleSequenceTransitionOutcome,
            WindowLifecycleSequenceTransitionFailure,
        > {
            self.result
        }
    }

    fn input() -> Value {
        json!({
            "actions": [
                { "type": "restore" },
                { "type": "resize", "coordinateSpace": "client-logical-px", "width": 800, "height": 600 },
                { "type": "maximize" }
            ],
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
            revision: 8,
            presented_revision: 8,
            poll_count: 1,
        }
    }

    fn success(settled: bool) -> WindowLifecycleSequenceTransitionOutcome {
        WindowLifecycleSequenceTransitionOutcome {
            sequence: WindowLifecycleSequenceOutcome {
                revision: 8,
                presented_revision: 8,
                settled,
                actions_accepted: 3,
            },
            observation: observation(),
        }
    }

    #[test]
    fn confirmation_and_foreground_consent_precede_input_target_and_provider() {
        let port = FixturePort {
            result: Ok(success(true)),
        };
        let Err(error) = perform_with(&port, "not-a-target", false, false, &Value::Null) else {
            panic!("未确认生命周期序列 transition 必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");

        let Err(error) = perform_with(&port, "not-a-target", true, false, &Value::Null) else {
            panic!("未同意前景影响必须在输入解析前失败");
        };
        assert_eq!(error.code, "FOREGROUND_CONSENT_REQUIRED");
    }

    #[test]
    fn complete_sequence_can_succeed_with_trusted_unsettled_state_observation() {
        let port = FixturePort {
            result: Ok(success(false)),
        };
        let Ok(result) = perform_with(&port, "s2:w:0123456789abcdef", true, true, &input()) else {
            panic!("可信框架条件必须允许 settled=false 的完整序列成功");
        };
        assert_eq!(result["sequenceSettled"], false);
        assert_eq!(result["actionsAccepted"], 3);
        assert_eq!(
            result["frameworkConditionMatchedAfterCompleteSequence"],
            true
        );
        assert_eq!(result["sameAuthenticatedConnectionUsed"], true);
        assert_eq!(result["causalityConfirmed"], false);
        assert_eq!(result["finalStateReached"], false);
        assert_eq!(result["safety"]["desktopInputInjected"], false);
        assert_eq!(result["safety"]["fallback"], "none");
    }

    #[test]
    fn partial_sequence_is_unknown_and_non_retryable() {
        let Ok(parsed) = UixWindowLifecycleSequenceTransitionInput::parse(&input()) else {
            panic!("测试输入必须有效");
        };
        let failure = WindowLifecycleSequenceTransitionFailure {
            source: WindowLifecycleSequenceTransitionFailureSource::Sequence(
                WindowLifecycleSequenceFailure {
                    source: WindowActionFailure::OutcomeUnknown,
                    accepted_may_have_occurred: true,
                    actions_accepted: 1,
                },
            ),
            accepted_may_have_occurred: true,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            actions_accepted: 1,
            poll_count: 0,
        };
        let error = transition_error(failure, &parsed);
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["actionsAccepted"], 1);
        assert_eq!(error.details["automaticRetryProhibited"], true);
        assert_eq!(error.details["retrySafe"], false);
    }

    #[test]
    fn pre_dispatch_timeout_preserves_specific_error() {
        let Ok(parsed) = UixWindowLifecycleSequenceTransitionInput::parse(&input()) else {
            panic!("测试输入必须有效");
        };
        let failure = WindowLifecycleSequenceTransitionFailure {
            source: WindowLifecycleSequenceTransitionFailureSource::Transport(Failure::Timeout),
            accepted_may_have_occurred: false,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            actions_accepted: 0,
            poll_count: 0,
        };
        let error = transition_error(failure, &parsed);
        assert_eq!(error.code, "TIMEOUT");
        assert_eq!(error.details["acceptedMayHaveOccurred"], false);
        assert_eq!(error.details["automaticRetryProhibited"], false);
    }
}
