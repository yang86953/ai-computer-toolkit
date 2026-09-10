//! 协调确认、前景同意与 UIX 窗口生命周期序列事实投影。

use serde_json::{Map, Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, WindowActionFailure, WindowLifecycleSequenceFailure, WindowLifecycleSequenceOutcome,
    },
    components::uix_window_lifecycle_sequence_contract::UixWindowLifecycleSequenceInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

const CAPABILITY: &str = crate::capabilities::WINDOW_LIFECYCLE_SEQUENCE;

trait UixWindowLifecycleSequencePort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixWindowLifecycleSequenceInput,
    ) -> Result<WindowLifecycleSequenceOutcome, WindowLifecycleSequenceFailure>;
}

struct SystemUixWindowLifecycleSequence;

impl UixWindowLifecycleSequencePort for SystemUixWindowLifecycleSequence {
    fn perform(
        &self,
        session_id: &str,
        input: &UixWindowLifecycleSequenceInput,
    ) -> Result<WindowLifecycleSequenceOutcome, WindowLifecycleSequenceFailure> {
        uix_agent::perform_window_lifecycle_sequence(session_id, input)
    }
}

/// 对精确协作式窗口执行一次已确认且已同意前景影响的生命周期序列。
pub(crate) fn perform(
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    value: &Value,
) -> AppResult<Value> {
    perform_with(
        &SystemUixWindowLifecycleSequence,
        session_id,
        confirmed,
        foreground_consent,
        value,
    )
}

fn perform_with(
    port: &impl UixWindowLifecycleSequencePort,
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX window lifecycle sequence requires explicit confirmation.",
        ));
    }
    // 可见窗口 mutation 的前景影响同意必须同样在发现前成立。
    if !foreground_consent {
        return Err(AppControlError::new(
            "FOREGROUND_CONSENT_REQUIRED",
            "UIX window lifecycle sequence requires upfront foreground-impact consent.",
        ));
    }
    let input = UixWindowLifecycleSequenceInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .perform(session_id, &input)
        .map_err(|failure| sequence_error(failure, &input))?;
    let actions = input
        .actions()
        .iter()
        .map(|action| action.public_value())
        .collect::<Vec<_>>();
    let safety = json!({
        "provider": "uix-agent-v1",
        "sameConnectionUsed": true,
        "windowGenerationFixed": true,
        "revisionChainUsed": true,
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "arbitraryWindowMoveSupported": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "x11Used": false,
        "fallback": "none",
    });
    Ok(json!({
        "capability": CAPABILITY,
        "targetId": session_id,
        "action": input.action(),
        "actions": actions,
        "actionsRequested": input.actions_requested(),
        "actionsAccepted": outcome.actions_accepted,
        "distinctActionKinds": input.distinct_action_kinds(),
        "intervalMs": input.interval_ms(),
        "plannedDurationMs": input.planned_duration_ms(),
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "sameConnectionRevisionChain": true,
        "transactionSemantics": false,
        "rollbackSemantics": false,
        "revision": outcome.revision,
        "presentedRevision": outcome.presented_revision,
        "settled": outcome.settled,
        "effectConfirmed": false,
        "finalStateReached": false,
        "compositorFinalStateConfirmed": false,
        "windowReResolved": true,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "foregroundConsentEvaluatedBeforeDiscovery": true,
        "foregroundConsentRequired": true,
        "foregroundImpactAuthorized": true,
        "hostForegroundActivationRequested": false,
        "effectObservation": "not-exposed-by-uix-agent-v1",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "host-foreground",
        "safety": safety,
    }))
}

fn sequence_error(
    failure: WindowLifecycleSequenceFailure,
    input: &UixWindowLifecycleSequenceInput,
) -> AppControlError {
    if failure.accepted_may_have_occurred || failure.actions_accepted > 0 {
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX window lifecycle sequence may have partially executed; automatic retry is prohibited.",
            failure_details(
                input,
                &failure,
                "partial-lifecycle-sequence-or-final-state-indeterminate",
                true,
            ),
        );
    }
    match failure.source {
        WindowActionFailure::Transport(source) => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(input, &failure, "transport-before-dispatch", false),
            );
            error
        }
        WindowActionFailure::UnsupportedAction => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish the complete requested lifecycle sequence.",
            failure_details(input, &failure, "agent-actions-not-published", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before lifecycle sequence dispatch.",
            failure_details(input, &failure, "window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids the lifecycle sequence.",
            failure_details(input, &failure, "application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for lifecycle sequence dispatch.",
            failure_details(input, &failure, "window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX lifecycle sequence lost a trusted final after dispatch may have begun.",
            failure_details(input, &failure, "trusted-final-lost", true),
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
    input: &UixWindowLifecycleSequenceInput,
    failure: &WindowLifecycleSequenceFailure,
    reason: &'static str,
    accepted_may_have_occurred: bool,
) -> Value {
    let mut details = Map::new();
    details.insert(
        "action".to_owned(),
        Value::String(input.action().to_owned()),
    );
    details.insert("reason".to_owned(), Value::String(reason.to_owned()));
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
    details.insert(
        "acceptedMayHaveOccurred".to_owned(),
        Value::Bool(accepted_may_have_occurred),
    );
    details.insert("transactionSemantics".to_owned(), Value::Bool(false));
    details.insert("rollbackSemantics".to_owned(), Value::Bool(false));
    details.insert("foregroundImpactAuthorized".to_owned(), Value::Bool(true));
    details.insert(
        "automaticRetryProhibited".to_owned(),
        Value::Bool(accepted_may_have_occurred),
    );
    details.insert("retrySafe".to_owned(), Value::Bool(false));
    details.insert(
        "requiredNextStep".to_owned(),
        Value::String("inspect-current-uix-window-before-any-new-lifecycle-action".to_owned()),
    );
    Value::Object(details)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixturePort {
        result: Result<WindowLifecycleSequenceOutcome, WindowLifecycleSequenceFailure>,
    }

    impl UixWindowLifecycleSequencePort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixWindowLifecycleSequenceInput,
        ) -> Result<WindowLifecycleSequenceOutcome, WindowLifecycleSequenceFailure> {
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
            "intervalMs": 20,
            "timeoutMs": 1000
        })
    }

    #[test]
    fn confirmation_and_foreground_consent_precede_input_target_and_provider() {
        let Err(error) = perform("not-a-target", false, false, &Value::Null) else {
            panic!("未确认生命周期序列必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");

        let Err(error) = perform("not-a-target", true, false, &Value::Null) else {
            panic!("未同意前景影响必须在解析前失败");
        };
        assert_eq!(error.code, "FOREGROUND_CONSENT_REQUIRED");
    }

    #[test]
    fn successful_sequence_publishes_counts_authorization_and_safety_facts() {
        let port = FixturePort {
            result: Ok(WindowLifecycleSequenceOutcome {
                revision: 8,
                presented_revision: 8,
                settled: true,
                actions_accepted: 3,
            }),
        };
        let Ok(result) = perform_with(&port, "s2:w:0123456789abcdef", true, true, &input()) else {
            panic!("测试生命周期序列必须成功");
        };
        assert_eq!(result["capability"], CAPABILITY);
        assert_eq!(result["action"], "lifecycle-sequence");
        assert_eq!(result["actionsRequested"], 3);
        assert_eq!(result["actionsAccepted"], 3);
        assert_eq!(result["distinctActionKinds"], 3);
        assert_eq!(result["foregroundConsentRequired"], true);
        assert_eq!(result["foregroundImpactAuthorized"], true);
        assert_eq!(result["confirmationEvaluatedBeforeDiscovery"], true);
        assert_eq!(result["transactionSemantics"], false);
        assert_eq!(result["rollbackSemantics"], false);
        assert_eq!(result["safety"]["desktopInputInjected"], false);
        assert_eq!(result["safety"]["arbitraryWindowMoveSupported"], false);
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
                    "visibleMutationAuthorized": true
                },
                "targeting": "opaque exact window generation"
            }
        });
        let Ok(schema) = serde_json::from_str::<Value>(include_str!(
            "../../contracts/v1/uix-window-lifecycle-sequence-result.schema.json"
        )) else {
            panic!("生命周期序列结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "生命周期序列结果必须匹配公开 schema"
        );
    }

    #[test]
    fn partial_failure_is_unknown_and_non_retryable() {
        let Ok(parsed) = UixWindowLifecycleSequenceInput::parse(&input()) else {
            panic!("测试输入必须有效");
        };
        let error = sequence_error(
            WindowLifecycleSequenceFailure {
                source: WindowActionFailure::OutcomeUnknown,
                accepted_may_have_occurred: true,
                actions_accepted: 1,
            },
            &parsed,
        );
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["actionsAccepted"], 1);
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["transactionSemantics"], false);
        assert_eq!(error.details["rollbackSemantics"], false);
        assert_eq!(error.details["foregroundImpactAuthorized"], true);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
