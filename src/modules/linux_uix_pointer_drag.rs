//! 协调确认、固定左键原子拖拽与失败释放事实投影。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, PointerDragFailure, PointerDragOutcome, WindowActionFailure,
    },
    capabilities,
    components::uix_pointer_drag_contract::UixPointerDragInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

trait UixPointerDragPort {
    fn drag(
        &self,
        session_id: &str,
        input: &UixPointerDragInput,
    ) -> Result<PointerDragOutcome, PointerDragFailure>;
}

struct SystemUixPointerDrag;

impl UixPointerDragPort for SystemUixPointerDrag {
    fn drag(
        &self,
        session_id: &str,
        input: &UixPointerDragInput,
    ) -> Result<PointerDragOutcome, PointerDragFailure> {
        uix_agent::perform_pointer_drag(session_id, input)
    }
}

/// 对精确协作式窗口执行一次已确认、请求内配平的左键拖拽。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    perform_with(&SystemUixPointerDrag, session_id, confirmed, value)
}

fn perform_with(
    port: &impl UixPointerDragPort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX application pointer drag requires explicit confirmation.",
        ));
    }
    let input = UixPointerDragInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .drag(session_id, &input)
        .map_err(|failure| drag_error(failure, &input))?;
    Ok(json!({
        "capability": capabilities::UI_INPUT_POINTER_DRAG,
        "targetId": session_id,
        "action": input.action(),
        "button": input.button(),
        "coordinateSpace": input.coordinate_space().as_str(),
        "start": { "x": input.start().x(), "y": input.start().y() },
        "end": { "x": input.end().x(), "y": input.end().y() },
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "pointerDownAccepted": true,
        "pointerUpAccepted": true,
        "buttonReleaseConfirmed": true,
        "requestScopedBalancedButtons": true,
        "samplesRequested": input.samples_requested(),
        "moveSamplesAccepted": outcome.move_samples_accepted,
        "durationMs": input.duration_ms(),
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
            "desktopPointerMoved": false,
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

fn drag_error(failure: PointerDragFailure, input: &UixPointerDragInput) -> AppControlError {
    if failure.pointer_down_may_have_occurred
        || failure.pointer_down_accepted
        || failure.move_samples_accepted > 0
        || failure.button_release_attempted
    {
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer drag may have partially executed; automatic retry is prohibited.",
            failure_details(
                input,
                &failure,
                "partial-drag-or-release-state-indeterminate",
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
            "The authenticated UIX Agent does not publish the complete pointer drag action set.",
            failure_details(input, &failure, "agent-drag-actions-not-published", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before pointer drag dispatch.",
            failure_details(input, &failure, "window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids application-internal pointer drag.",
            failure_details(input, &failure, "application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for pointer drag.",
            failure_details(input, &failure, "window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer drag lost a trusted final after dispatch may have begun.",
            failure_details(input, &failure, "trusted-final-lost", true),
        ),
    }
}

fn failure_details(
    input: &UixPointerDragInput,
    failure: &PointerDragFailure,
    reason: &'static str,
    accepted_may_have_occurred: bool,
) -> Value {
    json!({
        "action": input.action(),
        "button": input.button(),
        "coordinateSpace": input.coordinate_space().as_str(),
        "reason": reason,
        "pointerDownMayHaveOccurred": failure.pointer_down_may_have_occurred,
        "pointerDownAccepted": failure.pointer_down_accepted,
        "moveSamplesAccepted": failure.move_samples_accepted,
        "buttonReleaseAttempted": failure.button_release_attempted,
        "buttonReleaseConfirmed": failure.button_release_confirmed,
        "requestScopedBalancedButtons": failure.button_release_confirmed,
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
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
        result: Result<PointerDragOutcome, PointerDragFailure>,
    }

    impl UixPointerDragPort for FixturePort {
        fn drag(
            &self,
            _: &str,
            _: &UixPointerDragInput,
        ) -> Result<PointerDragOutcome, PointerDragFailure> {
            self.result
        }
    }

    fn input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 10, "y": 20 },
            "end": { "x": 30, "y": 40 },
            "samples": 2,
            "durationMs": 0,
            "timeoutMs": 1000
        })
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let Err(error) = perform("not-a-target", false, &Value::Null) else {
            panic!("未确认拖拽必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn balanced_drag_matches_public_result_schema() {
        let target = "s2:w:0123456789abcdef";
        let port = FixturePort {
            result: Ok(PointerDragOutcome {
                revision: 9,
                presented_revision: 9,
                settled: true,
                move_samples_accepted: 2,
            }),
        };
        let Ok(result) = perform_with(&port, target, true, &input()) else {
            panic!("测试拖拽必须成功");
        };
        let envelope = json!({
            "ok": true,
            "app": "app",
            "verb": "apply",
            "capability": capabilities::UI_INPUT_POINTER_DRAG,
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
            "../../contracts/v1/uix-pointer-drag-result.schema.json"
        )) else {
            panic!("拖拽结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "测试拖拽结果必须匹配公开 schema"
        );
    }

    #[test]
    fn failed_release_is_non_retryable_unknown() {
        let input_value = input();
        let Ok(parsed) = UixPointerDragInput::parse(&input_value) else {
            panic!("测试输入必须有效");
        };
        let error = drag_error(
            PointerDragFailure {
                source: WindowActionFailure::OutcomeUnknown,
                pointer_down_may_have_occurred: true,
                pointer_down_accepted: true,
                move_samples_accepted: 1,
                button_release_attempted: true,
                button_release_confirmed: false,
            },
            &parsed,
        );
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["buttonReleaseConfirmed"], false);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
