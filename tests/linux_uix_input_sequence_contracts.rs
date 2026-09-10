#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 应用内键盘与指针混合序列公开 schema 回归。

use serde_json::{Value, json};

fn schema(source: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(source)?)
}

fn assert_valid(schema: &Value, instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    jsonschema::draft202012::validate(schema, instance)
        .map_err(|error| format!("JSON Schema validation failed: {error}").into())
}

#[test]
fn schemas_accept_same_connection_key_and_pointer_sequence_without_transaction_claim()
-> Result<(), Box<dyn std::error::Error>> {
    let steps = json!([
        { "type": "move", "x": 10.0, "y": 20.0 },
        { "type": "click", "x": 30.0, "y": 40.0 },
        { "type": "press", "key": "enter", "modifiers": [] }
    ]);
    let input = json!({
        "coordinateSpace": "client-logical-px",
        "steps": steps,
        "intervalMs": 20,
        "timeoutMs": 5000
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/uix-input-sequence-input.schema.json"
        ))?,
        &input,
    )?;

    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.sequence@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "capability": "ui.input.sequence@1",
            "targetId": "s2:w:0000000000000000",
            "action": "input-sequence",
            "coordinateSpace": "client-logical-px",
            "steps": steps,
            "stepsRequested": 3,
            "stepsAccepted": 3,
            "pressesAccepted": 1,
            "movesAccepted": 1,
            "clicksAccepted": 1,
            "intervalMs": 20,
            "plannedDurationMs": 40,
            "outcome": "completed",
            "dispatchState": "completed",
            "accepted": true,
            "allPressesBalanced": true,
            "allClicksBalanced": true,
            "sameConnectionRevisionChain": true,
            "transactionSemantics": false,
            "rollbackSemantics": false,
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
                "requestScopedBalancedPresses": true,
                "requestScopedBalancedClicks": true,
                "independentKeyOwnership": false,
                "independentButtonOwnership": false,
                "textInputSupported": false,
                "keyHoldSupported": false,
                "keyRepeatSupported": false,
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
            "../contracts/v1/uix-input-sequence-result.schema.json"
        ))?,
        &result,
    )?;
    Ok(())
}
