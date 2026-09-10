#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 纯 pointer_move 序列的公开契约、确认门禁与平台边界回归。

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
fn schemas_accept_pure_hover_path_without_interpolation_or_desktop_pointer()
-> Result<(), Box<dyn std::error::Error>> {
    let moves = json!([
        { "x": 10.0, "y": 20.0 },
        { "x": 30.0, "y": 40.0 },
        { "x": 50.0, "y": 60.0 }
    ]);
    let input = json!({
        "coordinateSpace": "client-logical-px",
        "moves": moves,
        "intervalMs": 20,
        "timeoutMs": 5000
    });
    assert_valid(
        &schema(include_str!(
            "../../contracts/v1/uix-pointer-move-sequence-input.schema.json"
        ))?,
        &input,
    )?;

    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.pointer.move.sequence@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "capability": "ui.input.pointer.move.sequence@1",
            "targetId": "s2:w:0000000000000000",
            "action": "pointer-move-sequence",
            "coordinateSpace": "client-logical-px",
            "moves": moves,
            "movesRequested": 3,
            "movesAccepted": 3,
            "intervalMs": 20,
            "plannedDurationMs": 40,
            "outcome": "completed",
            "dispatchState": "completed",
            "accepted": true,
            "sameConnectionRevisionChain": true,
            "interpolationSemantics": false,
            "revision": 8,
            "presentedRevision": 8,
            "settled": true,
            "effectConfirmed": false,
            "finalStateReached": false,
            "windowReResolved": true,
            "applicationPolicyEvaluated": true,
            "confirmationEvaluatedBeforeDiscovery": true,
            "foregroundConsentRequired": false,
            "hostForegroundActivationRequested": false,
            "automaticRetryProhibited": true,
            "retrySafe": false,
            "timeoutMs": 5000,
            "executionRealm": "same-session-no-focus",
            "safety": {
                "provider": "uix-agent-v1",
                "applicationInternalEventsDispatched": true,
                "desktopInputInjected": false,
                "desktopPointerMoved": false,
                "sameConnectionUsed": true,
                "windowGenerationFixed": true,
                "revisionChainUsed": true,
                "interpolationSemantics": false,
                "smoothingSemantics": false,
                "dragSupported": false,
                "clickSupported": false,
                "scrollSupported": false,
                "independentButtonOwnership": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": { "activationRequested": false },
            "targeting": "opaque exact window generation"
        }
    });
    assert_valid(
        &schema(include_str!(
            "../../contracts/v1/uix-pointer-move-sequence-result.schema.json"
        ))?,
        &result,
    )?;
    Ok(())
}

#[test]
fn confirmation_precedes_input_file_and_target_access() -> Result<(), Box<dyn std::error::Error>> {
    let missing = "/tmp/ai-computer-toolkit-pointer-move-sequence-must-not-read.json";
    let (code, confirmation) = run(&[
        "run",
        "app",
        "apply",
        "--capability",
        "ui.input.pointer.move.sequence@1",
        "--input",
        missing,
    ])?;
    assert_eq!(code, 2);
    assert_eq!(confirmation["error"]["code"], "CONFIRMATION_REQUIRED");
    Ok(())
}

#[test]
fn adapter_remains_application_internal_and_has_no_desktop_fallback() {
    let adapter = include_str!("../../src/adapters/linux/uix_agent_pointer_move_sequence.rs");
    let module = include_str!("../../src/modules/linux_uix_pointer_move_sequence.rs");
    assert!(adapter.contains("同一认证连接"));
    assert!(adapter.contains("window.generation"));
    assert!(adapter.contains("latest_revision"));
    assert!(module.contains("\"desktopInputInjected\": false"));
    assert!(module.contains("\"desktopPointerMoved\": false"));
    assert!(module.contains("\"interpolationSemantics\": false"));
    for forbidden in ["xdotool", "XTest", "Xlib", "xcb_", "org.kde.KWin"] {
        assert!(!adapter.contains(forbidden));
        assert!(!module.contains(forbidden));
    }
}
