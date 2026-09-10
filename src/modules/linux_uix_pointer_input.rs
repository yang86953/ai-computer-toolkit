//! 协调确认与 UIX 应用内指针 mutation。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{self, WindowActionFailure},
    components::uix_pointer_input_contract::{UixPointerAction, UixPointerInput},
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

/// 对当前协作式窗口执行一次已确认的应用内指针动作。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    // 工具确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX application pointer input requires explicit confirmation.",
        ));
    }
    let input = UixPointerInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = uix_agent::perform_pointer(session_id, &input)
        .map_err(|failure| pointer_error(failure, &input))?;
    Ok(json!({
        "capability": crate::capabilities::UI_INPUT_POINTER_V2,
        "targetId": session_id,
        "action": input.action().as_str(),
        "coordinateSpace": input.coordinate_space().as_str(),
        "x": input.x(),
        "y": input.y(),
        "handledEvents": input.action().handled_events(),
        "outcome": if outcome.settled { "completed" } else { "completed-unsettled" },
        "dispatchState": "completed",
        "accepted": true,
        "effectConfirmed": false,
        "finalStateReached": false,
        "effectObservation": "not-exposed-by-uix-agent-v1",
        "revision": outcome.revision,
        "presentedRevision": outcome.presented_revision,
        "settled": outcome.settled,
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
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "desktopInputInjected": false,
            "desktopPointerMoved": false,
            "applicationInternalEventsDispatched": true,
            "requestScopedBalancedButtons": true,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

fn pointer_error(failure: WindowActionFailure, input: &UixPointerInput) -> AppControlError {
    match failure {
        WindowActionFailure::Transport(failure) => uix_window::public_error(failure),
        WindowActionFailure::UnsupportedAction => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish the requested pointer action.",
            safe_failure_details(input, "agent-pointer-action-not-published", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before pointer dispatch.",
            safe_failure_details(input, "window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids application-internal pointer input.",
            safe_failure_details(input, "application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable.",
            safe_failure_details(input, "window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable if input.action() == UixPointerAction::Move => {
            AppControlError::with_details(
                "POINTER_DISPATCH_FAILED",
                "No UIX component handled the application-internal pointer move.",
                safe_failure_details(input, "pointer-move-not-handled", false),
            )
        }
        WindowActionFailure::NotInteractable => outcome_unknown(
            input,
            "pointer-click-not-fully-handled",
            "The UIX pointer click may have been partially handled.",
        ),
        WindowActionFailure::WindowOperationFailed => outcome_unknown(
            input,
            "unexpected-window-operation-failure",
            "The UIX pointer action returned an indeterminate platform failure.",
        ),
        WindowActionFailure::DidNotSettle => outcome_unknown(
            input,
            "application-did-not-settle",
            "The UIX pointer action may have completed but the application did not settle.",
        ),
        WindowActionFailure::OutcomeUnknown => outcome_unknown(
            input,
            "trusted-final-lost",
            "The UIX pointer action lost its trusted final after dispatch may have begun.",
        ),
    }
}

fn safe_failure_details(
    input: &UixPointerInput,
    reason: &'static str,
    accepted_may_have_occurred: bool,
) -> Value {
    json!({
        "action": input.action().as_str(),
        "coordinateSpace": input.coordinate_space().as_str(),
        "reason": reason,
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
        "automaticRetryProhibited": accepted_may_have_occurred,
        "retrySafe": false,
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "requiredNextStep": "inspect-current-uix-window",
    })
}

fn outcome_unknown(
    input: &UixPointerInput,
    reason: &'static str,
    message: &'static str,
) -> AppControlError {
    AppControlError::with_details(
        "OUTCOME_UNKNOWN",
        message,
        safe_failure_details(input, reason, true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(action: &str) -> UixPointerInput {
        UixPointerInput::parse(&json!({
            "action": action,
            "coordinateSpace": "client-logical-px",
            "x": 10,
            "y": 20
        }))
        .expect("fixture input must parse")
    }

    #[test]
    fn confirmation_precedes_input_and_provider() {
        let error = perform("not-a-target", false, &Value::Null)
            .expect_err("confirmation must fail before input");
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn click_partial_is_unknown_but_unhandled_move_is_safe_failure() {
        let click = pointer_error(WindowActionFailure::NotInteractable, &input("click"));
        assert_eq!(click.code, "OUTCOME_UNKNOWN");
        assert_eq!(click.details["acceptedMayHaveOccurred"], true);
        let movement = pointer_error(WindowActionFailure::NotInteractable, &input("move"));
        assert_eq!(movement.code, "POINTER_DISPATCH_FAILED");
        assert_eq!(movement.details["acceptedMayHaveOccurred"], false);
    }
}
