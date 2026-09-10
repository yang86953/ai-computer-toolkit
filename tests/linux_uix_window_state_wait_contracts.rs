#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX framework-current 窗口状态条件等待的公开契约、只读门禁与平台边界回归。

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
fn schemas_accept_same_connection_framework_state_observation_without_compositor_claim()
-> Result<(), Box<dyn std::error::Error>> {
    let condition = json!({ "type": "focus", "focused": true });
    let input = json!({
        "condition": condition,
        "pollIntervalMs": 20,
        "timeoutMs": 500
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/window-state-wait-input.schema.json"
        ))?,
        &input,
    )?;

    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "read",
        "capability": "window.state.wait@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "ok": true,
            "contractVersion": "act/window-state-wait/v1",
            "capability": "window.state.wait@1",
            "targetId": "s2:w:0000000000000000",
            "observationSource": "uix-framework-current",
            "condition": condition,
            "coordinateSpace": "client-logical-px",
            "visible": true,
            "presentable": true,
            "focused": true,
            "clientSize": { "width": 800, "height": 600 },
            "windowState": {
                "maximized": false,
                "minimized": false,
                "fullscreen": false
            },
            "revision": 5,
            "presentedRevision": 5,
            "pollCount": 2,
            "frameworkConditionMatched": true,
            "compositorFinalStateConfirmed": false,
            "targetReResolved": true,
            "sameAuthenticatedConnectionUsed": true,
            "providerPolling": true,
            "waitProtocolUsed": false,
            "pollIntervalMs": 20,
            "timeoutMs": 500,
            "executionRealm": "same-session-no-focus",
            "readOnly": true,
            "mutationAllowed": false,
            "retrySafe": true,
            "safety": {
                "provider": "uix-agent-v1",
                "providerCache": "disabled",
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "foregroundActivationRequested": false,
                "desktopInputInjected": false,
                "desktopPointerMoved": false,
                "globalWindowStateClaimed": false,
                "sameAuthenticatedConnectionUsed": true,
                "providerPolling": true,
                "waitProtocolUsed": false,
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
            "../contracts/v1/window-state-wait.schema.json"
        ))?,
        &result,
    )?;
    Ok(())
}

#[test]
fn read_route_rejects_mutation_flags_before_input_file_access()
-> Result<(), Box<dyn std::error::Error>> {
    let missing = "/tmp/ai-computer-toolkit-state-wait-must-not-read.json";
    let (code, result) = run(&[
        "run",
        "app",
        "read",
        "--capability",
        "window.state.wait@1",
        "--target",
        "sessionId=s2:w:0000000000000000",
        "--input",
        missing,
        "--confirm",
    ])?;
    assert_eq!(code, 2);
    assert_eq!(result["error"]["code"], "INVALID_ARGUMENT");
    Ok(())
}

#[test]
fn adapter_uses_bounded_provider_polling_and_exact_opaque_target_without_desktop_fallback() {
    let adapter = include_str!("../src/adapters/linux/uix_agent_window_state_wait.rs");
    let module = include_str!("../src/modules/linux_uix_state_wait.rs");
    assert!(adapter.contains("client.list_windows()"));
    assert!(adapter.contains("window_session_id(&fixed_window.endpoint, &wire)"));
    assert!(adapter.contains("same authenticated connection") || adapter.contains("同一认证连接"));
    assert!(!adapter.contains("wait_revision"));
    assert!(module.contains("\"providerPolling\": true"));
    assert!(module.contains("\"waitProtocolUsed\": false"));
    assert!(module.contains("\"desktopInputInjected\": false"));
    for forbidden in ["xdotool", "XTest", "Xlib", "xcb_", "org.kde.KWin"] {
        assert!(!adapter.contains(forbidden));
        assert!(!module.contains(forbidden));
    }
}
