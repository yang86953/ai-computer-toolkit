//! 协作式 UIX Agent 精确窗口关闭请求 Module。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{self, WindowActionFailure},
    capabilities,
    components::uix_window_close_contract::UixWindowCloseInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

/// 对当前精确窗口提交一次已确认的平台关闭请求。
pub(crate) fn close(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    // 工具确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX window close requires explicit confirmation.",
        ));
    }
    let input = UixWindowCloseInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = uix_agent::perform_close(session_id, input.timeout_ms).map_err(close_error)?;
    Ok(json!({
        "ok": true,
        "contractVersion": "act/window-close/v2",
        "capability": capabilities::WINDOW_CLOSE_V2,
        "targetId": session_id,
        "outcome": "accepted",
        "dispatchState": "completed",
        "accepted": true,
        "closeConfirmed": false,
        "finalStateReached": false,
        "effectObservation": "separate-window.closed.wait@2",
        "followUpCapability": capabilities::WINDOW_CLOSED_WAIT_V2,
        "revision": outcome.revision,
        "presentedRevision": outcome.presented_revision,
        "settled": outcome.settled,
        "windowReResolved": true,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout_ms,
        "executionRealm": "same-session-no-focus",
        "safety": {
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "hostForegroundActivationRequested": false,
            "desktopInputInjected": false,
            "windowInventoryPolled": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

fn close_error(failure: WindowActionFailure) -> AppControlError {
    match failure {
        WindowActionFailure::Transport(failure) => uix_window::public_error(failure),
        WindowActionFailure::UnsupportedAction => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish close_window.",
            safe_failure_details("agent-action-not-published", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before close dispatch.",
            safe_failure_details("window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids window close.",
            safe_failure_details("application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable.",
            safe_failure_details("window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable => outcome_unknown(
            "unexpected-close-interaction-failure",
            "The UIX window close returned an indeterminate interaction failure.",
        ),
        WindowActionFailure::WindowOperationFailed => outcome_unknown(
            "window-operation-failed-after-dispatch",
            "The UIX platform close request failed after dispatch began.",
        ),
        WindowActionFailure::DidNotSettle => outcome_unknown(
            "application-did-not-settle",
            "The UIX close request may have completed but the application did not settle.",
        ),
        WindowActionFailure::OutcomeUnknown => outcome_unknown(
            "trusted-final-lost",
            "The UIX close request lost its trusted final after dispatch may have begun.",
        ),
    }
}

fn safe_failure_details(reason: &'static str, accepted_may_have_occurred: bool) -> Value {
    json!({
        "reason": reason,
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
        "automaticRetryProhibited": accepted_may_have_occurred,
        "retrySafe": false,
        "requiredNextStep": "window.closed.wait@2",
        "fallback": "none",
    })
}

fn outcome_unknown(reason: &'static str, message: &'static str) -> AppControlError {
    AppControlError::with_details(
        "OUTCOME_UNKNOWN",
        message,
        safe_failure_details(reason, true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_precedes_input_and_provider() {
        let error = close("not-a-target", false, &Value::Null)
            .expect_err("confirmation must fail before input");
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn post_dispatch_failure_is_non_retryable_unknown() {
        let error = close_error(WindowActionFailure::WindowOperationFailed);
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["requiredNextStep"], "window.closed.wait@2");
    }
}
