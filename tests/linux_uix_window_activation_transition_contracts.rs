#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 窗口 activation transition 的结果契约、前置确认和同连接观察边界回归。

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
        "verb": "apply",
        "capability": "window.activate.transition@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "host-foreground",
        "requiredExecutionRealm": "host-foreground",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": data,
        "meta": {
            "foreground": {
                "activationRequested": true,
                "focusObservationRequested": true
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
        "focusedFieldNegotiated": true,
        "providerPolling": true,
        "waitProtocolUsed": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "hostForegroundActivationRequested": true,
        "desktopInputInjected": false,
        "x11Used": false,
        "fallback": "none"
    })
}

#[test]
fn input_and_result_schemas_accept_unsettled_action_with_exact_focus_observation()
-> Result<(), Box<dyn std::error::Error>> {
    let input_schema = schema(include_str!(
        "../contracts/v1/window-activate-transition-input.schema.json"
    ))?;
    assert_valid(&input_schema, &json!({}))?;
    assert_valid(
        &input_schema,
        &json!({ "pollIntervalMs": 20, "timeoutMs": 100 }),
    )?;

    // actionSettled=false 不否定已收到的动作响应；成功仍要求同连接焦点事实。
    let result = envelope(json!({
        "ok": true,
        "contractVersion": "act/window-activation-transition/v1",
        "capability": "window.activate.transition@1",
        "targetId": "s2:w:0000000000000000",
        "outcome": "focused",
        "dispatchState": "completed",
        "accepted": true,
        "activationRequestAccepted": true,
        "actionAccepted": true,
        "actionRevision": 6,
        "actionPresentedRevision": 6,
        "actionSettled": false,
        "revision": 6,
        "presentedRevision": 6,
        "focused": true,
        "focusObservedAfterDispatch": true,
        "focusConfirmed": true,
        "focusMatchedAfterDispatch": true,
        "targetGenerationCurrentAfterDispatch": true,
        "pollCount": 2,
        "sameAuthenticatedConnectionUsed": true,
        "providerPolling": true,
        "waitProtocolUsed": false,
        "effectConfirmed": false,
        "causalityConfirmed": false,
        "finalFocusStateGuaranteed": false,
        "finalStateReached": false,
        "effectObservation": "same-connection-exact-generation-focused-after-dispatch-no-causal-or-persistence-claim",
        "windowReResolved": true,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "foregroundConsentEvaluatedBeforeDiscovery": true,
        "foregroundConsentRequired": true,
        "foregroundActivationRequested": true,
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "pollIntervalMs": 20,
        "timeoutMs": 1000,
        "executionRealm": "host-foreground",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety()
    }));
    let result_schema = schema(include_str!(
        "../contracts/v1/window-activate-transition.schema.json"
    ))?;
    assert_valid(&result_schema, &result)?;
    Ok(())
}

#[test]
fn input_schema_rejects_unknown_null_float_and_out_of_bounds_values()
-> Result<(), Box<dyn std::error::Error>> {
    let input_schema = schema(include_str!(
        "../contracts/v1/window-activate-transition-input.schema.json"
    ))?;
    for invalid in [
        json!({ "unexpected": true }),
        json!({ "pollIntervalMs": null }),
        json!({ "timeoutMs": 100.5 }),
        json!({ "pollIntervalMs": 19 }),
        json!({ "pollIntervalMs": 501 }),
        json!({ "timeoutMs": 99 }),
        json!({ "timeoutMs": 30001 }),
    ] {
        assert!(jsonschema::draft202012::validate(&input_schema, &invalid).is_err());
    }
    Ok(())
}

#[test]
fn confirmation_and_foreground_consent_precede_input_file_access()
-> Result<(), Box<dyn std::error::Error>> {
    let missing = "/tmp/ai-computer-toolkit-window-activate-transition-must-not-read.json";
    let (code, confirmation) = run(&[
        "run",
        "app",
        "apply",
        "--capability",
        "window.activate.transition@1",
        "--target",
        "sessionId=not-a-target",
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
        "window.activate.transition@1",
        "--target",
        "sessionId=not-a-target",
        "--input",
        missing,
        "--confirm",
    ])?;
    assert_eq!(code, 4);
    assert_eq!(foreground["error"]["code"], "FOREGROUND_CONSENT_REQUIRED");
    Ok(())
}

#[test]
fn adapter_dispatches_before_focused_only_polling_and_unknown_forbids_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let adapter = include_str!("../src/adapters/linux/uix_agent_window_activation_transition.rs");
    let module = include_str!("../src/modules/linux_uix_window_activation_transition.rs");
    let Some(dispatch) = adapter.find("perform_targetless_with_request_id") else {
        panic!("activation transition 必须先提交阶段专属 perform 请求");
    };
    let Some(poll) = adapter.find("client.list_windows()") else {
        panic!("activation transition 必须在动作后轮询 list_windows");
    };
    assert!(dispatch < poll);
    assert!(adapter.contains("client.window_state_fields.contains(\"focused\")"));
    assert!(adapter.contains("let Some(focused) = wire.focused else"));
    assert!(!adapter.contains("wait_revision"));
    assert!(module.contains("\"sameAuthenticatedConnectionUsed\": true"));
    assert!(module.contains("\"providerPolling\": true"));
    assert!(module.contains("\"waitProtocolUsed\": false"));
    assert!(module.contains("\"OUTCOME_UNKNOWN\""));
    assert!(module.contains("\"automaticRetryProhibited\": failure.accepted_may_have_occurred"));
    assert!(module.contains("automatic retry is prohibited"));
    for forbidden in ["xdotool", "XTest", "Xlib", "xcb_", "org.kde.KWin"] {
        assert!(!adapter.contains(forbidden));
        assert!(!module.contains(forbidden));
    }
    Ok(())
}
