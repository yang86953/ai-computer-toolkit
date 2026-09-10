//! 协调确认、应用内悬停序列与部分执行事实投影。

use serde_json::{Map, Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, PointerMoveSequenceFailure, PointerMoveSequenceOutcome, WindowActionFailure,
    },
    capabilities,
    components::uix_pointer_move_sequence_contract::UixPointerMoveSequenceInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

trait UixPointerMoveSequencePort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixPointerMoveSequenceInput,
    ) -> Result<PointerMoveSequenceOutcome, PointerMoveSequenceFailure>;
}

struct SystemUixPointerMoveSequence;

impl UixPointerMoveSequencePort for SystemUixPointerMoveSequence {
    fn perform(
        &self,
        session_id: &str,
        input: &UixPointerMoveSequenceInput,
    ) -> Result<PointerMoveSequenceOutcome, PointerMoveSequenceFailure> {
        uix_agent::perform_pointer_move_sequence(session_id, input)
    }
}

/// 对精确协作式窗口执行一次已确认的应用内 pointer_move 序列。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    perform_with(&SystemUixPointerMoveSequence, session_id, confirmed, value)
}

fn perform_with(
    port: &impl UixPointerMoveSequencePort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX application pointer move sequence requires explicit confirmation.",
        ));
    }
    let input = UixPointerMoveSequenceInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .perform(session_id, &input)
        .map_err(|failure| sequence_error(failure, &input))?;
    let safety = json!({
        "provider": "uix-agent-v1",
        "applicationInternalEventsDispatched": true,
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "sameConnectionUsed": true,
        "windowGenerationFixed": true,
        "revisionChainUsed": true,
        "interpolationSemantics": false,
        "smoothingSemantics": false,
        "dragSupported": false,
        "clickSupported": false,
        "scrollSupported": false,
        "independentButtonOwnership": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "x11Used": false,
        "fallback": "none",
    });
    Ok(json!({
        "capability": capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE,
        "targetId": session_id,
        "action": input.action(),
        "coordinateSpace": input.coordinate_space().as_str(),
        "moves": input.public_moves(),
        "movesRequested": input.moves_requested(),
        "movesAccepted": outcome.moves_accepted,
        "intervalMs": input.interval_ms(),
        "plannedDurationMs": input.planned_duration_ms(),
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "sameConnectionRevisionChain": true,
        "interpolationSemantics": false,
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
    failure: PointerMoveSequenceFailure,
    input: &UixPointerMoveSequenceInput,
) -> AppControlError {
    if failure.accepted_may_have_occurred || failure.moves_accepted > 0 {
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer move sequence may have partially executed; automatic retry is prohibited.",
            failure_details(
                input,
                &failure,
                "partial-pointer-move-sequence-or-final-state-indeterminate",
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
            "The authenticated UIX Agent does not publish pointer_move.",
            failure_details(input, &failure, "agent-pointer-move-not-published", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before pointer move sequence dispatch.",
            failure_details(input, &failure, "window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids the pointer move sequence.",
            failure_details(input, &failure, "application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for pointer move sequence input.",
            failure_details(input, &failure, "window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer move sequence lost a trusted final after dispatch may have begun.",
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
    input: &UixPointerMoveSequenceInput,
    failure: &PointerMoveSequenceFailure,
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
        "movesRequested".to_owned(),
        Value::from(input.moves_requested()),
    );
    details.insert(
        "movesAccepted".to_owned(),
        Value::from(failure.moves_accepted),
    );
    details.insert(
        "acceptedMayHaveOccurred".to_owned(),
        Value::Bool(accepted_may_have_occurred),
    );
    details.insert(
        "automaticRetryProhibited".to_owned(),
        Value::Bool(accepted_may_have_occurred),
    );
    details.insert("retrySafe".to_owned(), Value::Bool(false));
    details.insert("interpolationSemantics".to_owned(), Value::Bool(false));
    details.insert("desktopInputInjected".to_owned(), Value::Bool(false));
    details.insert("desktopPointerMoved".to_owned(), Value::Bool(false));
    details.insert(
        "requiredNextStep".to_owned(),
        Value::String("inspect-current-uix-window-before-any-new-pointer-move".to_owned()),
    );
    Value::Object(details)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixturePort {
        result: Result<PointerMoveSequenceOutcome, PointerMoveSequenceFailure>,
    }

    impl UixPointerMoveSequencePort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixPointerMoveSequenceInput,
        ) -> Result<PointerMoveSequenceOutcome, PointerMoveSequenceFailure> {
            self.result
        }
    }

    fn input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "moves": [
                { "x": 10, "y": 20 },
                { "x": 30, "y": 40 },
                { "x": 50, "y": 60 }
            ],
            "intervalMs": 20,
            "timeoutMs": 1000
        })
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let Err(error) = perform("not-a-target", false, &Value::Null) else {
            panic!("未确认移动序列必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn successful_sequence_matches_public_result_schema() {
        let target = "s2:w:0123456789abcdef";
        let port = FixturePort {
            result: Ok(PointerMoveSequenceOutcome {
                revision: 8,
                presented_revision: 8,
                settled: true,
                moves_accepted: 3,
            }),
        };
        let Ok(result) = perform_with(&port, target, true, &input()) else {
            panic!("测试移动序列必须成功");
        };
        let envelope = json!({
            "ok": true,
            "app": "app",
            "verb": "apply",
            "capability": capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE,
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
            "../../contracts/v1/uix-pointer-move-sequence-result.schema.json"
        )) else {
            panic!("移动序列结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "测试移动序列结果必须匹配公开 schema"
        );
    }

    #[test]
    fn partial_failure_is_unknown_and_non_retryable() {
        let Ok(parsed) = UixPointerMoveSequenceInput::parse(&input()) else {
            panic!("测试输入必须有效");
        };
        let error = sequence_error(
            PointerMoveSequenceFailure {
                source: WindowActionFailure::OutcomeUnknown,
                accepted_may_have_occurred: true,
                moves_accepted: 1,
            },
            &parsed,
        );
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["movesAccepted"], 1);
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
