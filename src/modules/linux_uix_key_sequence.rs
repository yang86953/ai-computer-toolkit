//! 协调确认、请求级成对按键序列与部分执行事实投影。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, KeySequenceFailure, KeySequenceOutcome, WindowActionFailure,
    },
    capabilities,
    components::uix_key_sequence_contract::UixKeySequenceInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

trait UixKeySequencePort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixKeySequenceInput,
    ) -> Result<KeySequenceOutcome, KeySequenceFailure>;
}

struct SystemUixKeySequence;

impl UixKeySequencePort for SystemUixKeySequence {
    fn perform(
        &self,
        session_id: &str,
        input: &UixKeySequenceInput,
    ) -> Result<KeySequenceOutcome, KeySequenceFailure> {
        uix_agent::perform_key_sequence(session_id, input)
    }
}

/// 对精确协作式窗口执行一次已确认的完整 press 序列。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    perform_with(&SystemUixKeySequence, session_id, confirmed, value)
}

fn perform_with(
    port: &impl UixKeySequencePort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX application key sequence requires explicit confirmation.",
        ));
    }
    let input = UixKeySequenceInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .perform(session_id, &input)
        .map_err(|failure| sequence_error(failure, &input))?;
    let presses = input
        .presses()
        .iter()
        .map(|press| {
            json!({
                "key": press.key(),
                "modifiers": press.modifiers(),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "capability": capabilities::UI_INPUT_KEY_SEQUENCE,
        "targetId": session_id,
        "action": input.action(),
        "presses": presses,
        "pressesRequested": input.presses_requested(),
        "pressesAccepted": outcome.presses_accepted,
        "intervalMs": input.interval_ms(),
        "plannedDurationMs": input.planned_duration_ms(),
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "allPressesBalanced": true,
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
        "safety": {
            "provider": "uix-agent-v1",
            "applicationInternalEventsDispatched": true,
            "desktopInputInjected": false,
            "requestScopedBalancedPresses": true,
            "textInputSupported": false,
            "keyHoldSupported": false,
            "keyRepeatSupported": false,
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

fn sequence_error(failure: KeySequenceFailure, input: &UixKeySequenceInput) -> AppControlError {
    if failure.accepted_may_have_occurred || failure.presses_accepted > 0 {
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX key sequence may have partially executed; automatic retry is prohibited.",
            failure_details(
                input,
                &failure,
                "partial-key-sequence-or-final-state-indeterminate",
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
            "The authenticated UIX Agent does not publish the complete requested key sequence.",
            failure_details(
                input,
                &failure,
                "agent-key-or-modifier-not-published",
                false,
            ),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before key sequence dispatch.",
            failure_details(input, &failure, "window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids the key sequence.",
            failure_details(input, &failure, "application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for key sequence input.",
            failure_details(input, &failure, "window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX key sequence lost a trusted final after dispatch may have begun.",
            failure_details(input, &failure, "trusted-final-lost", true),
        ),
    }
}

fn failure_details(
    input: &UixKeySequenceInput,
    failure: &KeySequenceFailure,
    reason: &'static str,
    accepted_may_have_occurred: bool,
) -> Value {
    json!({
        "action": "key-sequence",
        "reason": reason,
        "pressesRequested": input.presses_requested(),
        "pressesAccepted": failure.presses_accepted,
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
        "allPressesBalanced": false,
        "automaticRetryProhibited": accepted_may_have_occurred,
        "retrySafe": false,
        "desktopInputInjected": false,
        "requiredNextStep": "inspect-current-uix-window-before-any-new-key-input",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixturePort {
        result: Result<KeySequenceOutcome, KeySequenceFailure>,
    }

    impl UixKeySequencePort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixKeySequenceInput,
        ) -> Result<KeySequenceOutcome, KeySequenceFailure> {
            self.result
        }
    }

    fn input() -> Value {
        json!({
            "presses": [
                { "key": "a", "modifiers": ["control"] },
                { "key": "enter" }
            ],
            "intervalMs": 20,
            "timeoutMs": 1000
        })
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let Err(error) = perform("not-a-target", false, &Value::Null) else {
            panic!("未确认按键序列必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn balanced_sequence_matches_public_result_schema() {
        let target = "s2:w:0123456789abcdef";
        let port = FixturePort {
            result: Ok(KeySequenceOutcome {
                revision: 8,
                presented_revision: 8,
                settled: true,
                presses_accepted: 2,
            }),
        };
        let Ok(result) = perform_with(&port, target, true, &input()) else {
            panic!("测试按键序列必须成功");
        };
        let envelope = json!({
            "ok": true,
            "app": "app",
            "verb": "apply",
            "capability": capabilities::UI_INPUT_KEY_SEQUENCE,
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
            "../../contracts/v1/uix-key-sequence-result.schema.json"
        )) else {
            panic!("按键序列结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "测试按键序列结果必须匹配公开 schema"
        );
    }

    #[test]
    fn partial_sequence_is_non_retryable_unknown() {
        let Ok(parsed) = UixKeySequenceInput::parse(&input()) else {
            panic!("测试输入必须有效");
        };
        let error = sequence_error(
            KeySequenceFailure {
                source: WindowActionFailure::OutcomeUnknown,
                accepted_may_have_occurred: true,
                presses_accepted: 1,
            },
            &parsed,
        );
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["pressesAccepted"], 1);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
