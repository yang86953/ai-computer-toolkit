#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 语义元素 transition 的严格结果契约、确认顺序与平台边界回归。

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
        "capability": "ui.element.transition@1",
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
                "semanticPostconditionObservationRequested": true
            },
            "targeting": "opaque exact window generation"
        }
    })
}

fn safety(revision_wait_used: bool) -> Value {
    json!({
        "provider": "uix-agent-v1",
        "providerCache": "disabled",
        "sameAuthenticatedConnectionUsed": true,
        "windowGenerationFixed": true,
        "revisionWaitUsed": revision_wait_used,
        "providerPolling": false,
        "stabilitySemantics": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "confirmIdentityExposed": false,
        "sensitiveSnapshotPublished": false,
        "hostCoordinateMappingPublished": false,
        "clickPointInferred": false,
        "foregroundActivationRequested": false,
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "x11Used": false,
        "fallback": "none"
    })
}

#[test]
fn input_and_result_schemas_accept_an_unsettled_action_with_verified_postcondition()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "snapshotId": "as3:0000000000000000",
        "elementId": "s2:e:0000000000000000",
        "action": { "type": "invoke" },
        "postcondition": {
            "selector": { "automationId": "save" },
            "condition": "unique"
        },
        "timeoutMs": 1000
    });
    let input_schema = schema(include_str!(
        "../../contracts/v1/ui-element-transition-input.schema.json"
    ))?;
    assert_valid(&input_schema, &input)?;

    // 动作响应未 settled 仍可成功：成功条件是响应与提交后语义观察可信。
    let result = envelope(json!({
        "ok": true,
        "contractVersion": "act/ui-element-transition/v1",
        "capability": "ui.element.transition@1",
        "targetId": "s2:w:0000000000000000",
        "sourceSnapshotId": "as3:0000000000000000",
        "sourceElementId": "s2:e:0000000000000000",
        "action": "invoke",
        "actionAccepted": true,
        "actionRevision": 6,
        "actionPresentedRevision": 6,
        "actionSettled": false,
        "applicationConfirmation": "not-required",
        "postcondition": "unique",
        "selectorSemantics": "exact-and",
        "postconditionMatchedAfterDispatch": true,
        "snapshotId": "as3:0000000000000001",
        "matchState": "unique",
        "matchCount": 1,
        "sampleCount": 2,
        "waitCount": 1,
        "revision": 7,
        "presentedRevision": 7,
        "revisionWaitUsed": true,
        "element": {
            "elementId": "s2:e:0000000000000001",
            "identityFreshness": "snapshot-revision",
            "automationId": "save",
            "role": "button",
            "name": "保存",
            "focused": false,
            "enabled": true,
            "actions": ["invoke"],
            "geometry": {
                "coordinateSpace": "application-client",
                "unit": "logical-px",
                "frame": { "x": 1, "y": 2, "width": 20, "height": 10 },
                "visibleBounds": null
            }
        },
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "sameAuthenticatedConnectionUsed": true,
        "effectConfirmed": false,
        "causalityConfirmed": false,
        "finalStateReached": false,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "hostForegroundActivationRequested": false,
        "effectObservation": "semantic-postcondition-after-dispatch-no-causal-claim",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": 1000,
        "executionRealm": "same-session-no-focus",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety(true)
    }));
    let result_schema = schema(include_str!(
        "../../contracts/v1/ui-element-transition.schema.json"
    ))?;
    assert_valid(&result_schema, &result)?;
    Ok(())
}

#[test]
fn confirmation_precedes_missing_input_file_access() -> Result<(), Box<dyn std::error::Error>> {
    let missing = "/tmp/ai-computer-toolkit-element-transition-must-not-read.json";
    let (code, result) = run(&[
        "run",
        "app",
        "apply",
        "--capability",
        "ui.element.transition@1",
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
fn adapter_dispatches_snapshot_then_perform_then_postcondition_wait_on_one_connection() {
    let adapter = include_str!("../../src/adapters/linux/uix_agent_element_transition.rs");
    let module = include_str!("../../src/modules/linux_uix_element_transition.rs");
    let Some(snapshot) = adapter.find(".snapshot(") else {
        panic!("Adapter 必须先获取 snapshot");
    };
    let Some(perform) = adapter.find(".perform(") else {
        panic!("Adapter 必须执行精确 perform");
    };
    let Some(wait) = adapter.find("execute_wait_from_baseline") else {
        panic!("Adapter 必须在动作后执行 baseline wait");
    };
    assert!(snapshot < perform && perform < wait);
    assert!(module.contains("\"sameAuthenticatedConnectionUsed\": true"));
    assert!(module.contains("\"confirmationEvaluatedBeforeDiscovery\": true"));
    assert!(module.contains("\"providerPolling\": false"));
    assert!(module.contains("\"desktopInputInjected\": false"));
    assert!(module.contains("\"causalityConfirmed\": false"));
    assert!(module.contains("\"finalStateReached\": false"));
    for forbidden in ["xdotool", "XTest", "Xlib", "xcb_", "org.kde.KWin"] {
        assert!(!adapter.contains(forbidden));
        assert!(!module.contains(forbidden));
    }
}
