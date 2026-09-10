#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 语义元素 revision 等待的公开契约、只读门禁与平台边界回归。

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
        "verb": "read",
        "capability": "ui.element.wait@2",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": data,
        "meta": {
            "foreground": { "activationRequested": false },
            "targeting": "opaque exact window generation"
        }
    })
}

fn safety(revision_wait_used: bool) -> Value {
    json!({
        "provider": "uix-agent-v1",
        "providerCache": "disabled",
        "sameAuthenticatedConnectionUsed": true,
        "revisionWaitUsed": revision_wait_used,
        "providerPolling": false,
        "stabilitySemantics": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
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
fn schemas_freeze_exact_selector_and_actual_revision_wait_use()
-> Result<(), Box<dyn std::error::Error>> {
    let input_schema = schema(include_str!(
        "../contracts/v2/ui-element-wait-input.schema.json"
    ))?;
    assert_valid(
        &input_schema,
        &json!({
            "selector": { "automationId": "save", "enabled": true, "action": "invoke" },
            "condition": "unique",
            "timeoutMs": 500
        }),
    )?;
    for invalid in [
        json!({ "selector": {}, "condition": "unique" }),
        json!({ "selector": { "role": null }, "condition": "missing" }),
        json!({ "selector": { "role": "button" }, "condition": "present" }),
        json!({
            "selector": { "role": "button" },
            "condition": "unique",
            "pollIntervalMs": 50
        }),
        json!({
            "selector": { "role": "button" },
            "condition": "unique",
            "stableForMs": 100
        }),
    ] {
        assert!(assert_valid(&input_schema, &invalid).is_err());
    }

    let result_schema = schema(include_str!("../contracts/v2/ui-element-wait.schema.json"))?;
    let unique = envelope(json!({
        "ok": true,
        "contractVersion": "act/ui-element-wait/v2",
        "capability": "ui.element.wait@2",
        "targetId": "s2:w:0000000000000000",
        "snapshotId": "as3:0000000000000000",
        "condition": "unique",
        "selectorSemantics": "exact-and",
        "matchState": "unique",
        "matchCount": 1,
        "sampleCount": 2,
        "waitCount": 1,
        "revision": 6,
        "presentedRevision": 6,
        "revisionWaitUsed": true,
        "targetReResolved": true,
        "sameAuthenticatedConnectionUsed": true,
        "element": {
            "elementId": "s2:e:0000000000000000",
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
                "frame": { "x": 1, "y": 2, "width": 80, "height": 30 },
                "visibleBounds": null
            }
        },
        "executionRealm": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "retrySafe": true,
        "safety": safety(true)
    }));
    assert_valid(&result_schema, &unique)?;

    let missing = envelope(json!({
        "ok": true,
        "contractVersion": "act/ui-element-wait/v2",
        "capability": "ui.element.wait@2",
        "targetId": "s2:w:0000000000000000",
        "snapshotId": "as3:0000000000000000",
        "condition": "missing",
        "selectorSemantics": "exact-and",
        "matchState": "missing",
        "matchCount": 0,
        "sampleCount": 1,
        "waitCount": 0,
        "revision": 6,
        "presentedRevision": 6,
        "revisionWaitUsed": false,
        "targetReResolved": true,
        "sameAuthenticatedConnectionUsed": true,
        "element": null,
        "executionRealm": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "retrySafe": true,
        "safety": safety(false)
    }));
    assert_valid(&result_schema, &missing)?;

    let mut false_wait_claim = missing;
    false_wait_claim["data"]["revisionWaitUsed"] = json!(true);
    assert!(assert_valid(&result_schema, &false_wait_claim).is_err());
    Ok(())
}

#[test]
fn read_route_rejects_mutation_flags_before_input_file_access()
-> Result<(), Box<dyn std::error::Error>> {
    let missing = "/tmp/ai-computer-toolkit-element-wait-must-not-read.json";
    let (code, result) = run(&[
        "run",
        "app",
        "read",
        "--capability",
        "ui.element.wait@2",
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
fn adapter_reuses_snapshot_and_revision_wait_without_polling_or_desktop_fallback() {
    let adapter = include_str!("../src/adapters/linux/uix_agent_element_wait.rs");
    let module = include_str!("../src/modules/linux_uix_element_wait.rs");
    assert!(adapter.contains(".snapshot("));
    assert!(adapter.contains(".wait_revision("));
    assert!(adapter.contains("window.window_id"));
    assert!(adapter.contains("window.generation"));
    assert!(adapter.contains("fixed_window.session_id != target"));
    assert!(!adapter.contains("thread::sleep"));
    assert!(module.contains("\"providerPolling\": false"));
    assert!(module.contains("\"stabilitySemantics\": false"));
    assert!(module.contains("\"desktopInputInjected\": false"));
    for forbidden in ["xdotool", "XTest", "Xlib", "xcb_", "org.kde.KWin"] {
        assert!(!adapter.contains(forbidden));
        assert!(!module.contains(forbidden));
    }
}
