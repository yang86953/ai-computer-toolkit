#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 普通左键点击序列公开 schema 回归。

use serde_json::{Value, json};

fn schema(source: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(source)?)
}

fn assert_valid(schema: &Value, instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    jsonschema::draft202012::validate(schema, instance)
        .map_err(|error| format!("JSON Schema validation failed: {error}").into())
}

#[test]
fn schemas_accept_ordinary_left_clicks_without_double_click_semantics()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "coordinateSpace": "client-logical-px",
        "clicks": [
            { "x": 10.0, "y": 20.0 },
            { "x": 30.0, "y": 40.0 }
        ],
        "intervalMs": 20,
        "timeoutMs": 5000
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/uix-pointer-click-sequence-input.schema.json"
        ))?,
        &input,
    )?;
    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.pointer.click.sequence@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "capability": "ui.input.pointer.click.sequence@1",
            "targetId": "s2:w:0000000000000000",
            "action": "click-sequence",
            "coordinateSpace": "client-logical-px",
            "clicks": [
                { "x": 10.0, "y": 20.0 },
                { "x": 30.0, "y": 40.0 }
            ],
            "clicksRequested": 2,
            "clicksAccepted": 2,
            "intervalMs": 20,
            "plannedDurationMs": 20,
            "outcome": "completed",
            "dispatchState": "completed",
            "accepted": true,
            "allClicksBalanced": true,
            "doubleClickSemantics": false,
            "clickCountSemantics": false,
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
                "requestScopedBalancedClicks": true,
                "independentButtonOwnership": false,
                "doubleClickSupported": false,
                "clickCountSupported": false,
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
            "../contracts/v1/uix-pointer-click-sequence-result.schema.json"
        ))?,
        &result,
    )?;
    Ok(())
}
