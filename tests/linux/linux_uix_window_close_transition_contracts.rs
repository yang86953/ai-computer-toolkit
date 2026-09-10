#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 窗口 close transition 的结果契约、终态证据与确认顺序回归。

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

fn envelope(data: Value) -> Value {
    json!({
        "ok": true,
        "app": "app",
        "verb": "close",
        "capability": "window.close.transition@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": data,
        "meta": {
            "foreground": {
                "activationRequested": false,
                "terminalClosedObservationRequested": true
            },
            "targeting": "opaque exact window generation"
        }
    })
}

fn safety() -> Value {
    json!({
        "provider": "uix-agent-v1",
        "providerCache": "disabled",
        "sameAuthenticatedConnectionUsed": true,
        "windowGenerationFixed": true,
        "waitProtocolUsed": true,
        "providerPolling": false,
        "terminalReplyReceived": true,
        "connectionCloseAcceptedAsProof": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "hostForegroundActivationRequested": false,
        "desktopInputInjected": false,
        "windowInventoryPolled": false,
        "x11Used": false,
        "fallback": "none"
    })
}

#[test]
fn input_and_result_schemas_accept_unsettled_action_with_exact_closed_terminal()
-> Result<(), Box<dyn std::error::Error>> {
    let input_schema = schema(include_str!(
        "../../contracts/v1/window-close-transition-input.schema.json"
    ))?;
    assert_valid(&input_schema, &json!({}))?;
    assert_valid(&input_schema, &json!({ "timeoutMs": 100 }))?;

    // actionSettled=false 不否定已收到的动作响应；成功仍要求精确 closed 终态。
    let result = envelope(json!({
        "ok": true,
        "contractVersion": "act/window-close-transition/v1",
        "capability": "window.close.transition@1",
        "targetId": "s2:w:0000000000000000",
        "outcome": "closed",
        "dispatchState": "completed",
        "accepted": true,
        "actionAccepted": true,
        "actionRevision": 6,
        "actionPresentedRevision": 6,
        "actionSettled": false,
        "revision": 7,
        "presentedRevision": 6,
        "closed": true,
        "closeConfirmed": true,
        "exactGenerationConfirmed": true,
        "closedObservedAfterDispatch": true,
        "causalityConfirmed": false,
        "applicationClosedConfirmed": false,
        "finalStateReached": true,
        "effectObservation": "same-connection-exact-generation-closed-after-dispatch-no-causal-claim",
        "sameAuthenticatedConnectionUsed": true,
        "waitProtocolUsed": true,
        "providerPolling": false,
        "windowReResolved": true,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": 1000,
        "executionRealm": "same-session-no-focus",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety()
    }));
    let result_schema = schema(include_str!(
        "../../contracts/v1/window-close-transition.schema.json"
    ))?;
    assert_valid(&result_schema, &result)?;
    Ok(())
}

#[test]
fn confirmation_precedes_missing_input_file_access() -> Result<(), Box<dyn std::error::Error>> {
    let missing = "/tmp/ai-computer-toolkit-window-close-transition-must-not-read.json";
    let (code, result) = run(&[
        "run",
        "app",
        "close",
        "--capability",
        "window.close.transition@1",
        "--target",
        "sessionId=s2:w:0000000000000000",
        "--input",
        missing,
    ])?;
    assert_eq!(code, 2);
    assert_eq!(result["error"]["code"], "CONFIRMATION_REQUIRED");
    Ok(())
}

#[test]
fn protocol_fixture_requires_same_connection_closed_reply_and_never_infers_app_close() {
    let adapter = include_str!("../../src/adapters/linux/uix_agent_window_close_transition.rs");
    let action = include_str!("../../src/adapters/linux/uix_agent_window_action.rs");
    let module = include_str!("../../src/modules/linux_uix_window_close_transition.rs");
    let Some(perform) = adapter.find(".perform_close(") else {
        panic!("close transition 必须先发送精确 perform close_window");
    };
    let Some(wait) = adapter.find(".wait_closed(") else {
        panic!("close transition 必须在动作后等待 closed 终态");
    };
    assert!(perform < wait);
    assert!(adapter.contains("RevisionWaitOutcomeKind::Closed"));
    assert!(adapter.contains("closed.closed"));
    assert!(module.contains("\"connectionCloseAcceptedAsProof\": false"));
    assert!(module.contains("\"applicationClosedConfirmed\": false"));
    assert!(action.contains("\"app_closed\" => WindowActionFailure::OutcomeUnknown"));
    for forbidden in ["xdotool", "XTest", "Xlib", "xcb_", "org.kde.KWin"] {
        assert!(!adapter.contains(forbidden));
        assert!(!module.contains(forbidden));
    }
}
