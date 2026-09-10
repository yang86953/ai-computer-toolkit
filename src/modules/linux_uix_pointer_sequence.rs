//! 协调确认、悬停—普通左键点击混合序列与部分执行事实投影。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, PointerSequenceFailure, PointerSequenceOutcome, WindowActionFailure,
    },
    capabilities,
    components::uix_pointer_sequence_contract::UixPointerSequenceInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

trait UixPointerSequencePort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixPointerSequenceInput,
    ) -> Result<PointerSequenceOutcome, PointerSequenceFailure>;
}

struct SystemUixPointerSequence;

impl UixPointerSequencePort for SystemUixPointerSequence {
    fn perform(
        &self,
        session_id: &str,
        input: &UixPointerSequenceInput,
    ) -> Result<PointerSequenceOutcome, PointerSequenceFailure> {
        uix_agent::perform_pointer_sequence(session_id, input)
    }
}

/// 对精确协作式窗口执行一次已确认的悬停—普通点击混合序列。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    perform_with(&SystemUixPointerSequence, session_id, confirmed, value)
}

fn perform_with(
    port: &impl UixPointerSequencePort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX application pointer sequence requires explicit confirmation.",
        ));
    }
    let input = UixPointerSequenceInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .perform(session_id, &input)
        .map_err(|failure| sequence_error(failure, &input))?;
    let steps = input
        .steps()
        .iter()
        .map(|step| {
            json!({
                "type": step.as_str(),
                "x": step.x(),
                "y": step.y(),
            })
        })
        .collect::<Vec<_>>();
    let safety = json!({
        "provider": "uix-agent-v1",
        "applicationInternalEventsDispatched": true,
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "sameConnectionUsed": true,
        "windowGenerationFixed": true,
        "revisionChainUsed": true,
        "requestScopedBalancedClicks": true,
        "independentButtonOwnership": false,
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
        "capability": capabilities::UI_INPUT_POINTER_SEQUENCE,
        "targetId": session_id,
        "action": input.action(),
        "coordinateSpace": input.coordinate_space().as_str(),
        "steps": steps,
        "stepsRequested": input.steps_requested(),
        "stepsAccepted": outcome.steps_accepted,
        "movesAccepted": outcome.moves_accepted,
        "clicksAccepted": outcome.clicks_accepted,
        "intervalMs": input.interval_ms(),
        "plannedDurationMs": input.planned_duration_ms(),
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "allClicksBalanced": true,
        "doubleClickSemantics": false,
        "clickCountSemantics": false,
        "sameConnectionRevisionChain": true,
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

fn sequence_error(
    failure: PointerSequenceFailure,
    input: &UixPointerSequenceInput,
) -> AppControlError {
    if failure.accepted_may_have_occurred || failure.steps_accepted > 0 {
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer sequence may have partially executed; automatic retry is prohibited.",
            failure_details(
                input,
                &failure,
                "partial-pointer-sequence-or-final-state-indeterminate",
                true,
            ),
        );
    }
    match failure.source {
        WindowActionFailure::Transport(source) => {
            let mut error = uix_window::public_error(source);
            if let Some(details) = error.details.as_object_mut() {
                details.insert("acceptedMayHaveOccurred".to_owned(), Value::Bool(false));
                details.insert("automaticRetryProhibited".to_owned(), Value::Bool(false));
                details.insert("retrySafe".to_owned(), Value::Bool(false));
            }
            error
        }
        WindowActionFailure::UnsupportedAction => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish both pointer_move and click_at.",
            failure_details(input, &failure, "agent-move-or-click-not-published", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before pointer sequence dispatch.",
            failure_details(input, &failure, "window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids the pointer sequence.",
            failure_details(input, &failure, "application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for pointer sequence input.",
            failure_details(input, &failure, "window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer sequence lost a trusted final after dispatch may have begun.",
            failure_details(input, &failure, "trusted-final-lost", true),
        ),
    }
}

fn failure_details(
    input: &UixPointerSequenceInput,
    failure: &PointerSequenceFailure,
    reason: &'static str,
    accepted_may_have_occurred: bool,
) -> Value {
    json!({
        "action": input.action(),
        "reason": reason,
        "stepsRequested": input.steps_requested(),
        "stepsAccepted": failure.steps_accepted,
        "movesAccepted": failure.moves_accepted,
        "clicksAccepted": failure.clicks_accepted,
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
        "allClicksBalanced": false,
        "doubleClickSemantics": false,
        "clickCountSemantics": false,
        "automaticRetryProhibited": accepted_may_have_occurred,
        "retrySafe": false,
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "requiredNextStep": "inspect-current-uix-window-before-any-new-pointer-input",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixturePort {
        result: Result<PointerSequenceOutcome, PointerSequenceFailure>,
    }

    impl UixPointerSequencePort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixPointerSequenceInput,
        ) -> Result<PointerSequenceOutcome, PointerSequenceFailure> {
            self.result
        }
    }

    fn input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "move", "x": 10.0, "y": 20.0 },
                { "type": "click", "x": 30.0, "y": 40.0 }
            ],
            "intervalMs": 20,
            "timeoutMs": 1000
        })
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let Err(error) = perform("not-a-target", false, &Value::Null) else {
            panic!("未确认混合指针序列必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn mixed_sequence_matches_public_result_schema() {
        let target = "s2:w:0123456789abcdef";
        let port = FixturePort {
            result: Ok(PointerSequenceOutcome {
                revision: 8,
                presented_revision: 8,
                settled: true,
                steps_accepted: 2,
                moves_accepted: 1,
                clicks_accepted: 1,
            }),
        };
        let Ok(result) = perform_with(&port, target, true, &input()) else {
            panic!("测试混合指针序列必须成功");
        };
        let envelope = json!({
            "ok": true,
            "app": "app",
            "verb": "apply",
            "capability": capabilities::UI_INPUT_POINTER_SEQUENCE,
            "targetId": target,
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
            "../../contracts/v1/uix-pointer-sequence-result.schema.json"
        )) else {
            panic!("混合指针序列结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "测试混合指针序列结果必须匹配公开 schema"
        );
    }

    #[test]
    fn partial_sequence_is_non_retryable_unknown() {
        let Ok(parsed) = UixPointerSequenceInput::parse(&input()) else {
            panic!("测试输入必须有效");
        };
        let error = sequence_error(
            PointerSequenceFailure {
                source: WindowActionFailure::OutcomeUnknown,
                accepted_may_have_occurred: true,
                steps_accepted: 1,
                moves_accepted: 1,
                clicks_accepted: 0,
            },
            &parsed,
        );
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["stepsAccepted"], 1);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
