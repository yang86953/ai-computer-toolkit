//! 协调确认、前景同意与 UIX 窗口生命周期 mutation。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{self, WindowActionFailure},
    components::uix_window_lifecycle_contract::{
        UixWindowLifecycleAction, UixWindowLifecycleInput,
    },
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

/// 对当前协作式窗口执行一次已确认且已同意前景影响的生命周期动作。
pub(crate) fn perform(
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    value: &Value,
) -> AppResult<Value> {
    // 工具确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX window lifecycle requires explicit confirmation.",
        ));
    }
    // 可见窗口 mutation 的前景影响同意必须同样在发现前成立。
    if !foreground_consent {
        return Err(AppControlError::new(
            "FOREGROUND_CONSENT_REQUIRED",
            "UIX window lifecycle requires upfront foreground-impact consent.",
        ));
    }
    let input = UixWindowLifecycleInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = uix_agent::perform_window(session_id, input.action, input.timeout_ms)
        .map_err(|failure| lifecycle_error(failure, input.action))?;
    let requested_size = input
        .action
        .requested_size()
        .map(|(width, height)| json!({ "width": width, "height": height }));
    Ok(json!({
        "capability": crate::capabilities::WINDOW_LIFECYCLE_V2,
        "targetId": session_id,
        "action": input.action.as_str(),
        "outcome": if outcome.settled { "completed" } else { "completed-unsettled" },
        "dispatchState": "completed",
        "accepted": true,
        "effectConfirmed": false,
        "finalStateReached": false,
        "effectObservation": "not-exposed-by-uix-agent-v1",
        "revision": outcome.revision,
        "presentedRevision": outcome.presented_revision,
        "settled": outcome.settled,
        "coordinateSpace": input.action.coordinate_space().map(|space| space.as_str()),
        "requestedClientSize": requested_size,
        "windowReResolved": true,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "foregroundConsentEvaluatedBeforeDiscovery": true,
        "foregroundImpactAuthorized": true,
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout_ms,
        "executionRealm": "host-foreground",
        "safety": {
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "hostForegroundActivationRequested": false,
            "desktopInputInjected": false,
            "moveSupportedOnLinux": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

fn lifecycle_error(
    failure: WindowActionFailure,
    action: UixWindowLifecycleAction,
) -> AppControlError {
    match failure {
        WindowActionFailure::Transport(failure) => uix_window::public_error(failure),
        WindowActionFailure::UnsupportedAction => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish the requested window action.",
            safe_failure_details(action, "agent-action-not-published", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before lifecycle dispatch.",
            safe_failure_details(action, "window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids window lifecycle mutation.",
            safe_failure_details(action, "application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable.",
            safe_failure_details(action, "window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable => outcome_unknown(
            action,
            "unexpected-window-action-partial-dispatch",
            "The UIX window action returned an indeterminate interaction failure.",
        ),
        WindowActionFailure::WindowOperationFailed => outcome_unknown(
            action,
            "window-operation-failed-after-dispatch",
            "The UIX platform window operation failed after dispatch began.",
        ),
        WindowActionFailure::DidNotSettle => outcome_unknown(
            action,
            "application-did-not-settle",
            "The UIX window action may have completed but the application did not settle.",
        ),
        WindowActionFailure::OutcomeUnknown => outcome_unknown(
            action,
            "trusted-final-lost",
            "The UIX window lifecycle action lost its trusted final after dispatch may have begun.",
        ),
    }
}

fn safe_failure_details(
    action: UixWindowLifecycleAction,
    reason: &'static str,
    accepted_may_have_occurred: bool,
) -> Value {
    json!({
        "action": action.as_str(),
        "reason": reason,
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
        "automaticRetryProhibited": accepted_may_have_occurred,
        "retrySafe": false,
        "moveFallbackUsed": false,
        "requiredNextStep": "inspect-current-uix-window",
    })
}

fn outcome_unknown(
    action: UixWindowLifecycleAction,
    reason: &'static str,
    message: &'static str,
) -> AppControlError {
    AppControlError::with_details(
        "OUTCOME_UNKNOWN",
        message,
        safe_failure_details(action, reason, true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_and_foreground_consent_precede_input_and_provider() {
        let unconfirmed = perform("not-a-target", false, false, &Value::Null)
            .expect_err("confirmation must fail first");
        assert_eq!(unconfirmed.code, "CONFIRMATION_REQUIRED");
        let no_foreground = perform("not-a-target", true, false, &Value::Null)
            .expect_err("foreground consent must fail before input");
        assert_eq!(no_foreground.code, "FOREGROUND_CONSENT_REQUIRED");
    }

    #[test]
    fn platform_failure_after_dispatch_is_non_retryable_unknown() {
        let error = lifecycle_error(
            WindowActionFailure::WindowOperationFailed,
            UixWindowLifecycleAction::Maximize {},
        );
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
