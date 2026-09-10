//! 协调确认与 UIX 应用内按键 mutation。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{self, WindowActionFailure},
    components::uix_key_input_contract::UixKeyInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

/// 对当前协作式窗口执行一次已确认的应用内完整按键。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    // 工具确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX application key input requires explicit confirmation.",
        ));
    }
    let input = UixKeyInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome =
        uix_agent::perform_key(session_id, &input).map_err(|failure| key_error(failure, &input))?;
    Ok(json!({
        "capability": crate::capabilities::UI_INPUT_KEY_V2,
        "targetId": session_id,
        "key": input.key(),
        "modifiers": input.modifiers(),
        "phase": "press",
        "outcome": if outcome.settled { "completed" } else { "completed-unsettled" },
        "dispatchState": "completed",
        "accepted": true,
        "keyDownHandled": true,
        "keyUpHandled": true,
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
            "applicationInternalEventsDispatched": true,
            "requestScopedBalancedPress": true,
            "textInputSupported": false,
            "keyHoldSupported": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

fn key_error(failure: WindowActionFailure, input: &UixKeyInput) -> AppControlError {
    match failure {
        WindowActionFailure::Transport(failure) => uix_window::public_error(failure),
        WindowActionFailure::UnsupportedAction => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish the requested key action.",
            safe_failure_details(input, "agent-key-not-published", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before key dispatch.",
            safe_failure_details(input, "window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids application-internal key input.",
            safe_failure_details(input, "application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable.",
            safe_failure_details(input, "window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable => outcome_unknown(
            input,
            "key-events-not-fully-handled",
            "The UIX key press may have been partially handled.",
        ),
        WindowActionFailure::WindowOperationFailed => outcome_unknown(
            input,
            "unexpected-window-operation-failure",
            "The UIX key press returned an indeterminate platform failure.",
        ),
        WindowActionFailure::DidNotSettle => outcome_unknown(
            input,
            "application-did-not-settle",
            "The UIX key press may have completed but the application did not settle.",
        ),
        WindowActionFailure::OutcomeUnknown => outcome_unknown(
            input,
            "trusted-final-lost",
            "The UIX key press lost its trusted final after dispatch may have begun.",
        ),
    }
}

fn safe_failure_details(
    input: &UixKeyInput,
    reason: &'static str,
    accepted_may_have_occurred: bool,
) -> Value {
    json!({
        "key": input.key(),
        "modifiers": input.modifiers(),
        "phase": "press",
        "reason": reason,
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
        "automaticRetryProhibited": accepted_may_have_occurred,
        "retrySafe": false,
        "desktopInputInjected": false,
        "requiredNextStep": "inspect-current-uix-window",
    })
}

fn outcome_unknown(
    input: &UixKeyInput,
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

    #[test]
    fn confirmation_precedes_input_and_provider() {
        let error = perform("not-a-target", false, &Value::Null)
            .expect_err("confirmation must fail before input");
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn partial_key_handling_is_non_retryable_unknown() {
        let input =
            UixKeyInput::parse(&json!({ "key": "enter" })).expect("fixture input must parse");
        let error = key_error(WindowActionFailure::NotInteractable, &input);
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
