#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 悬停—普通左键点击混合序列公开 schema 回归。

use serde_json::{Value, json};

fn schema(source: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(source)?)
}

fn assert_valid(schema: &Value, instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    jsonschema::draft202012::validate(schema, instance)
        .map_err(|error| format!("JSON Schema validation failed: {error}").into())
}

#[test]
fn schemas_accept_move_then_ordinary_click_without_desktop_pointer_semantics()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "coordinateSpace": "client-logical-px",
        "steps": [
            { "type": "move", "x": 10.0, "y": 20.0 },
            { "type": "click", "x": 30.0, "y": 40.0 }
        ],
        "intervalMs": 20,
        "timeoutMs": 5000
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/uix-pointer-sequence-input.schema.json"
        ))?,
        &input,
    )?;
    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.pointer.sequence@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "capability": "ui.input.pointer.sequence@1",
            "targetId": "s2:w:0000000000000000",
            "action": "pointer-sequence",
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "move", "x": 10.0, "y": 20.0 },
                { "type": "click", "x": 30.0, "y": 40.0 }
            ],
            "stepsRequested": 2,
            "stepsAccepted": 2,
            "movesAccepted": 1,
            "clicksAccepted": 1,
            "intervalMs": 20,
            "plannedDurationMs": 20,
            "outcome": "completed",
            "dispatchState": "completed",
            "accepted": true,
            "allClicksBalanced": true,
            "doubleClickSemantics": false,
            "clickCountSemantics": false,
            "sameConnectionRevisionChain": true,
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
                "requestScopedBalancedClicks": true,
                "independentButtonOwnership": false,
                "dragSupported": false,
                "doubleClickSupported": false,
                "clickCountSupported": false,
                "scrollSupported": false,
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
            "../contracts/v1/uix-pointer-sequence-result.schema.json"
        ))?,
        &result,
    )?;
    Ok(())
}
