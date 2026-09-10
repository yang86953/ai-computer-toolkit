//! 协调确认、普通左键点击序列与部分执行事实投影。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, PointerClickSequenceFailure, PointerClickSequenceOutcome, WindowActionFailure,
    },
    capabilities,
    components::uix_pointer_click_sequence_contract::UixPointerClickSequenceInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

trait UixPointerClickSequencePort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixPointerClickSequenceInput,
    ) -> Result<PointerClickSequenceOutcome, PointerClickSequenceFailure>;
}

struct SystemUixPointerClickSequence;

impl UixPointerClickSequencePort for SystemUixPointerClickSequence {
    fn perform(
        &self,
        session_id: &str,
        input: &UixPointerClickSequenceInput,
    ) -> Result<PointerClickSequenceOutcome, PointerClickSequenceFailure> {
        uix_agent::perform_pointer_click_sequence(session_id, input)
    }
}

/// 对精确协作式窗口执行一次已确认的普通左键点击序列。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    perform_with(&SystemUixPointerClickSequence, session_id, confirmed, value)
}

fn perform_with(
    port: &impl UixPointerClickSequencePort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX application pointer click sequence requires explicit confirmation.",
        ));
    }
    let input = UixPointerClickSequenceInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .perform(session_id, &input)
        .map_err(|failure| sequence_error(failure, &input))?;
    let clicks = input
        .clicks()
        .iter()
        .map(|click| json!({ "x": click.x(), "y": click.y() }))
        .collect::<Vec<_>>();
    let safety = json!({
        "provider": "uix-agent-v1",
        "applicationInternalEventsDispatched": true,
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "requestScopedBalancedClicks": true,
        "independentButtonOwnership": false,
        "doubleClickSupported": false,
        "clickCountSupported": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "x11Used": false,
        "fallback": "none",
    });
    Ok(json!({
        "capability": capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE,
        "targetId": session_id,
        "action": input.action(),
        "coordinateSpace": input.coordinate_space().as_str(),
        "clicks": clicks,
        "clicksRequested": input.clicks_requested(),
        "clicksAccepted": outcome.clicks_accepted,
        "intervalMs": input.interval_ms(),
        "plannedDurationMs": input.planned_duration_ms(),
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "allClicksBalanced": true,
        "doubleClickSemantics": false,
        "clickCountSemantics": false,
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
    failure: PointerClickSequenceFailure,
    input: &UixPointerClickSequenceInput,
) -> AppControlError {
    if failure.accepted_may_have_occurred || failure.clicks_accepted > 0 {
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer click sequence may have partially executed; automatic retry is prohibited.",
            failure_details(
                input,
                &failure,
                "partial-click-sequence-or-final-state-indeterminate",
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
            "The authenticated UIX Agent does not publish ordinary application-internal clicks.",
            failure_details(input, &failure, "agent-click-at-not-published", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before pointer click sequence dispatch.",
            failure_details(input, &failure, "window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids the pointer click sequence.",
            failure_details(input, &failure, "application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for pointer clicks.",
            failure_details(input, &failure, "window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer click sequence lost a trusted final after dispatch may have begun.",
            failure_details(input, &failure, "trusted-final-lost", true),
        ),
    }
}

fn failure_details(
    input: &UixPointerClickSequenceInput,
    failure: &PointerClickSequenceFailure,
    reason: &'static str,
    accepted_may_have_occurred: bool,
) -> Value {
    json!({
        "action": input.action(),
        "reason": reason,
        "clicksRequested": input.clicks_requested(),
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
        result: Result<PointerClickSequenceOutcome, PointerClickSequenceFailure>,
    }

    impl UixPointerClickSequencePort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixPointerClickSequenceInput,
        ) -> Result<PointerClickSequenceOutcome, PointerClickSequenceFailure> {
            self.result
        }
    }

    fn input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "clicks": [
                { "x": 10.0, "y": 20.0 },
                { "x": 30.0, "y": 40.0 }
            ],
            "intervalMs": 20,
            "timeoutMs": 1000
        })
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let Err(error) = perform("not-a-target", false, &Value::Null) else {
            panic!("未确认点击序列必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn balanced_click_sequence_matches_public_result_schema() {
        let target = "s2:w:0123456789abcdef";
        let port = FixturePort {
            result: Ok(PointerClickSequenceOutcome {
                revision: 8,
                presented_revision: 8,
                settled: true,
                clicks_accepted: 2,
            }),
        };
        let Ok(result) = perform_with(&port, target, true, &input()) else {
            panic!("测试点击序列必须成功");
        };
        let envelope = json!({
            "ok": true,
            "app": "app",
            "verb": "apply",
            "capability": capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE,
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
            "../../contracts/v1/uix-pointer-click-sequence-result.schema.json"
        )) else {
            panic!("点击序列结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "测试点击序列结果必须匹配公开 schema"
        );
    }

    #[test]
    fn partial_click_sequence_is_non_retryable_unknown() {
        let Ok(parsed) = UixPointerClickSequenceInput::parse(&input()) else {
            panic!("测试输入必须有效");
        };
        let error = sequence_error(
            PointerClickSequenceFailure {
                source: WindowActionFailure::OutcomeUnknown,
                accepted_may_have_occurred: true,
                clicks_accepted: 1,
            },
            &parsed,
        );
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["clicksAccepted"], 1);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
