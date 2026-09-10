//! 协调 UIX 精确窗口前台激活、确认与焦点证据投影。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{self, WindowActionFailure, WindowActivationOutcome},
    capabilities,
    components::uix_window_activation_contract::UixWindowActivationInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

trait UixWindowActivationPort {
    fn activate(
        &self,
        session_id: &str,
        timeout_ms: u32,
    ) -> Result<WindowActivationOutcome, WindowActionFailure>;
}

struct SystemUixWindowActivation;

impl UixWindowActivationPort for SystemUixWindowActivation {
    fn activate(
        &self,
        session_id: &str,
        timeout_ms: u32,
    ) -> Result<WindowActivationOutcome, WindowActionFailure> {
        uix_agent::perform_activation(session_id, timeout_ms)
    }
}

/// 在确认与前景同意后，为精确 UIX 窗口提交一次激活请求。
pub(crate) fn perform(
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    value: &Value,
) -> AppResult<Value> {
    perform_with(
        &SystemUixWindowActivation,
        session_id,
        confirmed,
        foreground_consent,
        value,
    )
}

fn perform_with(
    port: &impl UixWindowActivationPort,
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX window activation requires explicit confirmation.",
        ));
    }
    // 激活会影响主机前台，必须在读取 input 和发现目标前授权。
    if !foreground_consent {
        return Err(AppControlError::new(
            "FOREGROUND_CONSENT_REQUIRED",
            "UIX window activation requires upfront foreground-impact consent.",
        ));
    }
    let input = UixWindowActivationInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .activate(session_id, input.timeout_ms())
        .map_err(activation_error)?;
    let focus_confirmed = outcome.focus_observed_after_dispatch == Some(true)
        && outcome.target_generation_current_after_dispatch == Some(true);
    Ok(json!({
        "capability": capabilities::WINDOW_ACTIVATE,
        "targetId": session_id,
        "outcome": "request-accepted",
        "activationRequestAccepted": true,
        "focusObservedAfterDispatch": outcome.focus_observed_after_dispatch,
        "focusConfirmed": focus_confirmed,
        "targetGenerationCurrentAfterDispatch": outcome.target_generation_current_after_dispatch,
        "finalFocusStateGuaranteed": false,
        "windowReResolved": true,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeInputAndDiscovery": true,
        "foregroundConsentEvaluatedBeforeInputAndDiscovery": true,
        "foregroundConsentRequired": true,
        "foregroundActivationRequested": true,
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "host-foreground",
        "safety": {
            "provider": "uix-agent-v1",
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "desktopInputInjected": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

fn activation_error(failure: WindowActionFailure) -> AppControlError {
    match failure {
        WindowActionFailure::Transport(failure) => uix_window::public_error(failure),
        WindowActionFailure::UnsupportedAction => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish window activation.",
            safe_failure_details("agent-action-not-published", false),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before activation dispatch.",
            safe_failure_details("window-revision-changed", false),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids window activation.",
            safe_failure_details("application-policy-forbidden", false),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for activation.",
            safe_failure_details("window-not-presentable", false),
        ),
        WindowActionFailure::NotInteractable => outcome_unknown(
            "unexpected-interaction-failure",
            "The UIX activation returned an indeterminate interaction failure.",
        ),
        WindowActionFailure::WindowOperationFailed => outcome_unknown(
            "window-operation-failed-after-dispatch",
            "The platform activation request failed after dispatch may have begun.",
        ),
        WindowActionFailure::DidNotSettle => outcome_unknown(
            "application-did-not-settle",
            "The activation may have been accepted but the application did not settle.",
        ),
        WindowActionFailure::OutcomeUnknown => outcome_unknown(
            "trusted-final-lost",
            "The UIX window activation lost its trusted final after dispatch may have begun.",
        ),
    }
}

fn safe_failure_details(reason: &'static str, accepted_may_have_occurred: bool) -> Value {
    json!({
        "action": "activate",
        "reason": reason,
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
        "focusConfirmed": false,
        "finalFocusStateGuaranteed": false,
        "automaticRetryProhibited": accepted_may_have_occurred,
        "retrySafe": false,
        "desktopInputInjected": false,
        "requiredNextStep": "inspect-current-uix-window",
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

    struct FixturePort;

    impl UixWindowActivationPort for FixturePort {
        fn activate(
            &self,
            _: &str,
            _: u32,
        ) -> Result<WindowActivationOutcome, WindowActionFailure> {
            Ok(WindowActivationOutcome {
                focus_observed_after_dispatch: Some(true),
                target_generation_current_after_dispatch: Some(true),
            })
        }
    }

    #[test]
    fn confirmation_and_foreground_consent_precede_input_and_provider() {
        let Err(unconfirmed) = perform("not-a-target", false, false, &Value::Null) else {
            panic!("未确认激活必须优先失败");
        };
        assert_eq!(unconfirmed.code, "CONFIRMATION_REQUIRED");
        let Err(no_foreground) = perform("not-a-target", true, false, &Value::Null) else {
            panic!("前景同意必须先于输入校验");
        };
        assert_eq!(no_foreground.code, "FOREGROUND_CONSENT_REQUIRED");
    }

    #[test]
    fn accepted_activation_only_confirms_focus_from_same_generation_observation() {
        let target = "s2:w:0123456789abcdef";
        let Ok(result) = perform_with(&FixturePort, target, true, true, &json!({})) else {
            panic!("测试激活必须成功");
        };
        assert_eq!(result["activationRequestAccepted"], true);
        assert_eq!(result["focusObservedAfterDispatch"], true);
        assert_eq!(result["focusConfirmed"], true);
        assert_eq!(result["finalFocusStateGuaranteed"], false);

        let envelope = json!({
            "ok": true,
            "app": "app",
            "verb": "apply",
            "capability": capabilities::WINDOW_ACTIVATE,
            "targetId": target,
            "executionRealm": "host-foreground",
            "requiredExecutionRealm": "host-foreground",
            "executionRealmCertified": true,
            "isolationRequirement": "standard",
            "hostImpactPolicy": "background-preferred",
            "data": result,
            "meta": {
                "foreground": {
                    "activationRequested": true,
                    "focusConfirmed": true
                },
                "targeting": "opaque exact window generation"
            }
        });
        let Ok(schema) = serde_json::from_str::<Value>(include_str!(
            "../../contracts/v1/window-activate-result.schema.json"
        )) else {
            panic!("激活结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "测试激活结果必须匹配公开 schema"
        );
    }

    #[test]
    fn post_dispatch_platform_failure_is_non_retryable_unknown() {
        let error = activation_error(WindowActionFailure::WindowOperationFailed);
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["focusConfirmed"], false);
    }
}
