#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 窗口生命周期过渡的公开契约、双前置门禁与同连接边界回归。

use std::process::Command;

use serde_json::{Value, json};

const TOOLKIT: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit");

fn schema(source: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(source)?)
}

fn assert_valid(schema: &Value, instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    jsonschema::draft202012::validate(schema, instance)
        .map_err(|error| format!("JSON Schema validation failed: {error}").into())
}

fn run(arguments: &[&str]) -> Result<(i32, Value), Box<dyn std::error::Error>> {
    let output = Command::new(TOOLKIT)
        .args(arguments)
        .env_clear()
        .env("LANG", "C")
        .env("PATH", "/usr/bin")
        .output()?;
    let value = serde_json::from_slice::<Value>(&output.stdout)?;
    Ok((output.status.code().unwrap_or(-1), value))
}

#[test]
fn schemas_accept_action_and_post_dispatch_framework_observation_without_causal_claim()
-> Result<(), Box<dyn std::error::Error>> {
    let action = json!({ "type": "maximize" });
    let condition = json!({ "type": "window-flags", "maximized": true });
    let input = json!({
        "action": action,
        "condition": condition,
        "pollIntervalMs": 20,
        "timeoutMs": 1000
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/window-lifecycle-transition-input.schema.json"
        ))?,
        &input,
    )?;

    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "window.lifecycle.transition@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "host-foreground",
        "requiredExecutionRealm": "host-foreground",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "capability": "window.lifecycle.transition@1",
            "targetId": "s2:w:0000000000000000",
            "action": action,
            "condition": condition,
            "actionAccepted": true,
            "actionRevision": 6,
            "actionPresentedRevision": 6,
            "actionSettled": false,
            "observationSource": "uix-framework-current",
            "coordinateSpace": "client-logical-px",
            "visible": true,
            "presentable": true,
            "focused": false,
            "clientSize": { "width": 800, "height": 600 },
            "windowState": {
                "maximized": true,
                "minimized": false,
                "fullscreen": false
            },
            "outcome": "completed",
            "dispatchState": "completed",
            "accepted": true,
            "frameworkConditionMatchedAfterDispatch": true,
            "revision": 6,
            "presentedRevision": 6,
            "pollCount": 1,
            "sameAuthenticatedConnectionUsed": true,
            "providerPolling": true,
            "waitProtocolUsed": false,
            "effectConfirmed": false,
            "causalityConfirmed": false,
            "finalStateReached": false,
            "compositorFinalStateConfirmed": false,
            "windowReResolved": true,
            "applicationPolicyEvaluated": true,
            "confirmationEvaluatedBeforeDiscovery": true,
            "foregroundConsentEvaluatedBeforeDiscovery": true,
            "foregroundConsentRequired": true,
            "foregroundImpactAuthorized": true,
            "hostForegroundActivationRequested": false,
            "effectObservation": "framework-condition-after-dispatch-no-causal-claim",
            "automaticRetryProhibited": true,
            "retrySafe": false,
            "pollIntervalMs": 20,
            "timeoutMs": 1000,
            "executionRealm": "host-foreground",
            "safety": {
                "provider": "uix-agent-v1",
                "sameAuthenticatedConnectionUsed": true,
                "windowGenerationFixed": true,
                "providerPolling": true,
                "waitProtocolUsed": false,
                "desktopInputInjected": false,
                "desktopPointerMoved": false,
                "arbitraryWindowMoveSupported": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": {
                "activationRequested": false,
                "visibleMutationAuthorized": true,
                "frameworkConditionObservationRequested": true
            },
            "targeting": "opaque exact window generation"
        }
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/window-lifecycle-transition.schema.json"
        ))?,
        &result,
    )?;
    Ok(())
}

#[test]
fn confirmation_and_foreground_consent_precede_input_file_access()
-> Result<(), Box<dyn std::error::Error>> {
    let missing = "/tmp/ai-computer-toolkit-lifecycle-transition-must-not-read.json";
    let (code, confirmation) = run(&[
        "run",
        "app",
        "apply",
        "--capability",
        "window.lifecycle.transition@1",
        "--input",
        missing,
    ])?;
    assert_eq!(code, 2);
    assert_eq!(confirmation["error"]["code"], "CONFIRMATION_REQUIRED");

    let (code, foreground) = run(&[
        "run",
        "app",
        "apply",
        "--capability",
        "window.lifecycle.transition@1",
        "--input",
        missing,
        "--confirm",
    ])?;
    assert_eq!(code, 4);
    assert_eq!(foreground["error"]["code"], "FOREGROUND_CONSENT_REQUIRED");
    Ok(())
}

#[test]
fn adapter_dispatches_then_polls_on_one_connection_without_desktop_fallback() {
    let adapter = include_str!("../src/adapters/linux/uix_agent_window_lifecycle_transition.rs");
    let module = include_str!("../src/modules/linux_uix_lifecycle_transition.rs");
    assert!(adapter.contains("perform_targetless_with_request_id"));
    assert!(adapter.contains("client.list_windows()"));
    assert!(adapter.contains("matching_observation"));
    assert!(!adapter.contains("wait_revision"));
    assert!(module.contains("\"frameworkConditionMatchedAfterDispatch\": true"));
    assert!(module.contains("\"causalityConfirmed\": false"));
    assert!(module.contains("\"desktopInputInjected\": false"));
    for forbidden in ["xdotool", "XTest", "Xlib", "xcb_", "org.kde.KWin"] {
        assert!(!adapter.contains(forbidden));
        assert!(!module.contains(forbidden));
    }
}
