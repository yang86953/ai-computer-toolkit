#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 窗口生命周期序列的公开契约、门禁与平台边界回归。

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
fn schemas_accept_same_connection_lifecycle_sequence_without_final_state_claim()
-> Result<(), Box<dyn std::error::Error>> {
    let actions = json!([
        { "type": "restore" },
        { "type": "resize", "coordinateSpace": "client-logical-px", "width": 800, "height": 600 },
        { "type": "maximize" }
    ]);
    let input = json!({
        "actions": actions,
        "intervalMs": 20,
        "timeoutMs": 5000
    });
    assert_valid(
        &schema(include_str!(
            "../../contracts/v1/uix-window-lifecycle-sequence-input.schema.json"
        ))?,
        &input,
    )?;

    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "window.lifecycle.sequence@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "host-foreground",
        "requiredExecutionRealm": "host-foreground",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "capability": "window.lifecycle.sequence@1",
            "targetId": "s2:w:0000000000000000",
            "action": "lifecycle-sequence",
            "actions": actions,
            "actionsRequested": 3,
            "actionsAccepted": 3,
            "distinctActionKinds": 3,
            "intervalMs": 20,
            "plannedDurationMs": 40,
            "outcome": "completed",
            "dispatchState": "completed",
            "accepted": true,
            "sameConnectionRevisionChain": true,
            "transactionSemantics": false,
            "rollbackSemantics": false,
            "revision": 8,
            "presentedRevision": 8,
            "settled": true,
            "effectConfirmed": false,
            "finalStateReached": false,
            "compositorFinalStateConfirmed": false,
            "windowReResolved": true,
            "applicationPolicyEvaluated": true,
            "confirmationEvaluatedBeforeDiscovery": true,
            "foregroundConsentEvaluatedBeforeDiscovery": true,
            "foregroundConsentRequired": true,
            "foregroundImpactAuthorized": true,
            "hostForegroundActivationRequested": false,
            "effectObservation": "not-exposed-by-uix-agent-v1",
            "automaticRetryProhibited": true,
            "retrySafe": false,
            "timeoutMs": 5000,
            "executionRealm": "host-foreground",
            "safety": {
                "provider": "uix-agent-v1",
                "sameConnectionUsed": true,
                "windowGenerationFixed": true,
                "revisionChainUsed": true,
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
                "visibleMutationAuthorized": true
            },
            "targeting": "opaque exact window generation"
        }
    });
    assert_valid(
        &schema(include_str!(
            "../../contracts/v1/uix-window-lifecycle-sequence-result.schema.json"
        ))?,
        &result,
    )?;
    Ok(())
}

#[test]
fn confirmation_and_foreground_consent_precede_input_file_access()
-> Result<(), Box<dyn std::error::Error>> {
    let missing = "/tmp/ai-computer-toolkit-lifecycle-sequence-must-not-read.json";
    let (code, confirmation) = run(&[
        "run",
        "app",
        "apply",
        "--capability",
        "window.lifecycle.sequence@1",
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
        "window.lifecycle.sequence@1",
        "--input",
        missing,
        "--confirm",
    ])?;
    assert_eq!(code, 4);
    assert_eq!(foreground["error"]["code"], "FOREGROUND_CONSENT_REQUIRED");
    Ok(())
}

#[test]
fn adapter_remains_wayland_standard_and_does_not_inject_desktop_input() {
    let adapter = include_str!("../../src/adapters/linux/uix_agent_window_lifecycle_sequence.rs");
    let module = include_str!("../../src/modules/linux_uix_lifecycle_sequence.rs");
    assert!(adapter.contains("same connection") || adapter.contains("同一认证连接"));
    assert!(adapter.contains("window.generation"));
    assert!(adapter.contains("latest_revision"));
    assert!(module.contains("\"desktopInputInjected\": false"));
    assert!(module.contains("\"desktopPointerMoved\": false"));
    for forbidden in ["xdotool", "XTest", "Xlib", "xcb_", "org.kde.KWin"] {
        assert!(!adapter.contains(forbidden));
        assert!(!module.contains(forbidden));
    }
}
