//! 协调确认、键盘与指针混合序列及部分执行事实投影。

use serde_json::{Map, Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, InputSequenceFailure, InputSequenceOutcome, WindowActionFailure,
    },
    capabilities,
    components::uix_input_sequence_contract::UixInputSequenceInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

trait UixInputSequencePort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixInputSequenceInput,
    ) -> Result<InputSequenceOutcome, InputSequenceFailure>;
}

struct SystemUixInputSequence;

impl UixInputSequencePort for SystemUixInputSequence {
    fn perform(
        &self,
        session_id: &str,
        input: &UixInputSequenceInput,
    ) -> Result<InputSequenceOutcome, InputSequenceFailure> {
        uix_agent::perform_input_sequence(session_id, input)
    }
}

/// 对精确协作式窗口执行一次已确认的键盘与指针混合序列。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    perform_with(&SystemUixInputSequence, session_id, confirmed, value)
}

fn perform_with(
    port: &impl UixInputSequencePort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX application input sequence requires explicit confirmation.",
        ));
    }
    let input = UixInputSequenceInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .perform(session_id, &input)
        .map_err(|failure| sequence_error(failure, &input))?;
    let steps = input
        .steps()
        .iter()
        .map(|step| step.public_value())
        .collect::<Vec<_>>();
    let safety = json!({
        "provider": "uix-agent-v1",
        "applicationInternalEventsDispatched": true,
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "sameConnectionUsed": true,
        "windowGenerationFixed": true,
        "revisionChainUsed": true,
        "requestScopedBalancedPresses": true,
        "requestScopedBalancedClicks": true,
        "independentKeyOwnership": false,
        "independentButtonOwnership": false,
        "textInputSupported": false,
        "keyHoldSupported": false,
        "keyRepeatSupported": false,
        "dragSupported": false,
        "doubleClickSupported": false,
        "clickCountSupported": false,
        "scrollSupported": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "x11Used": false,
        "fallback": "none",
    });
    Ok(json!({
        "capability": capabilities::UI_INPUT_SEQUENCE,
        "targetId": session_id,
        "action": input.action(),
        "coordinateSpace": input.coordinate_space().as_str(),
        "steps": steps,
        "stepsRequested": input.steps_requested(),
        "stepsAccepted": outcome.steps_accepted,
        "pressesAccepted": outcome.presses_accepted,
        "movesAccepted": outcome.moves_accepted,
        "clicksAccepted": outcome.clicks_accepted,
        "intervalMs": input.interval_ms(),
        "plannedDurationMs": input.planned_duration_ms(),
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "allPressesBalanced": true,
        "allClicksBalanced": true,
        "sameConnectionRevisionChain": true,
        "transactionSemantics": false,
        "rollbackSemantics": false,
        "revision": outcome.revision,
        "presentedRevision": outcome.presented_revision,
        "settled": outcome.settled,
        "effectConfirmed": false,
        "finalStateReached": false,
        "windowReResolved": true,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "foregroundConsentRequired": false,
        "hostForegroundActivationRequested": false,
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "same-session-no-focus",
        "safety": safety,
    }))
}

fn sequence_error(failure: InputSequenceFailure, input: &UixInputSequenceInput) -> AppControlError {
    if failure.accepted_may_have_occurred || failure.steps_accepted > 0 {
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX input sequence may have partially executed; automatic retry is prohibited.",
            failure_details(
                input,
                &failure,
                "partial-input-sequence-or-final-state-indeterminate",
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
            "The authenticated UIX Agent does not publish the complete requested input sequence.",
            failure_details(input, &failure, "agent-action-or-key-not-published", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before input sequence dispatch.",
            failure_details(input, &failure, "window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids the input sequence.",
            failure_details(input, &failure, "application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for input sequence dispatch.",
            failure_details(input, &failure, "window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX input sequence lost a trusted final after dispatch may have begun.",
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
    input: &UixInputSequenceInput,
    failure: &InputSequenceFailure,
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
        "stepsRequested".to_owned(),
        Value::from(input.steps_requested()),
    );
    details.insert(
        "stepsAccepted".to_owned(),
        Value::from(failure.steps_accepted),
    );
    details.insert(
        "pressesAccepted".to_owned(),
        Value::from(failure.presses_accepted),
    );
    details.insert(
        "movesAccepted".to_owned(),
        Value::from(failure.moves_accepted),
    );
    details.insert(
        "clicksAccepted".to_owned(),
        Value::from(failure.clicks_accepted),
    );
    details.insert(
        "acceptedMayHaveOccurred".to_owned(),
        Value::Bool(accepted_may_have_occurred),
    );
    details.insert("transactionSemantics".to_owned(), Value::Bool(false));
    details.insert("rollbackSemantics".to_owned(), Value::Bool(false));
    details.insert(
        "automaticRetryProhibited".to_owned(),
        Value::Bool(accepted_may_have_occurred),
    );
    details.insert("retrySafe".to_owned(), Value::Bool(false));
    details.insert("desktopInputInjected".to_owned(), Value::Bool(false));
    details.insert("desktopPointerMoved".to_owned(), Value::Bool(false));
    details.insert(
        "requiredNextStep".to_owned(),
        Value::String("inspect-current-uix-window-before-any-new-input".to_owned()),
    );
    Value::Object(details)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixturePort {
        result: Result<InputSequenceOutcome, InputSequenceFailure>,
    }

    impl UixInputSequencePort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixInputSequenceInput,
        ) -> Result<InputSequenceOutcome, InputSequenceFailure> {
            self.result
        }
    }

    fn input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "press", "key": "a", "modifiers": ["control"] },
                { "type": "move", "x": 10, "y": 20 },
                { "type": "click", "x": 30, "y": 40 }
            ],
            "intervalMs": 20,
            "timeoutMs": 1000
        })
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let Err(error) = perform("not-a-target", false, &Value::Null) else {
            panic!("未确认混合输入序列必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn successful_sequence_publishes_all_counts_and_safety_facts() {
        let port = FixturePort {
            result: Ok(InputSequenceOutcome {
                revision: 8,
                presented_revision: 8,
                settled: true,
                steps_accepted: 3,
                presses_accepted: 1,
                moves_accepted: 1,
                clicks_accepted: 1,
            }),
        };
        let Ok(result) = perform_with(&port, "s2:w:0123456789abcdef", true, &input()) else {
            panic!("测试混合输入序列必须成功");
        };
        assert_eq!(result["action"], "input-sequence");
        assert_eq!(result["stepsRequested"], 3);
        assert_eq!(result["stepsAccepted"], 3);
        assert_eq!(result["pressesAccepted"], 1);
        assert_eq!(result["movesAccepted"], 1);
        assert_eq!(result["clicksAccepted"], 1);
        assert_eq!(result["allPressesBalanced"], true);
        assert_eq!(result["allClicksBalanced"], true);
        assert_eq!(result["transactionSemantics"], false);
        assert_eq!(result["rollbackSemantics"], false);
        assert_eq!(result["safety"]["desktopInputInjected"], false);
        let envelope = json!({
            "ok": true,
            "app": "app",
            "verb": "apply",
            "capability": capabilities::UI_INPUT_SEQUENCE,
            "targetId": "s2:w:0123456789abcdef",
            "executionRealm": "same-session-no-focus",
            "requiredExecutionRealm": "same-session-no-focus",
            "executionRealmCertified": true,
            "isolationRequirement": "standard",
            "hostImpactPolicy": "background-preferred",
            "data": result,
            "meta": {
                "foreground": { "activationRequested": false },
                "targeting": "opaque exact window generation"
            }
        });
        let Ok(schema) = serde_json::from_str::<Value>(include_str!(
            "../../contracts/v1/uix-input-sequence-result.schema.json"
        )) else {
            panic!("混合输入序列结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "混合输入序列结果必须匹配公开 schema"
        );
    }

    #[test]
    fn partial_failure_is_unknown_and_carries_non_retryable_counts() {
        let Ok(parsed) = UixInputSequenceInput::parse(&input()) else {
            panic!("测试输入必须有效");
        };
        let error = sequence_error(
            InputSequenceFailure {
                source: WindowActionFailure::OutcomeUnknown,
                accepted_may_have_occurred: true,
                steps_accepted: 1,
                presses_accepted: 1,
                moves_accepted: 0,
                clicks_accepted: 0,
            },
            &parsed,
        );
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["stepsAccepted"], 1);
        assert_eq!(error.details["pressesAccepted"], 1);
        assert_eq!(error.details["movesAccepted"], 0);
        assert_eq!(error.details["clicksAccepted"], 0);
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["transactionSemantics"], false);
        assert_eq!(error.details["rollbackSemantics"], false);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
