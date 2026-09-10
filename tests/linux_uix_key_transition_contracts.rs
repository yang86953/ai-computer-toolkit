#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 完整按键 transition 的结果 schema、确认顺序和不确定性边界回归。

use serde_json::{Value, json};

fn schema(source: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(source)?)
}

fn assert_valid(schema: &Value, instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    jsonschema::draft202012::validate(schema, instance)
        .map_err(|error| format!("JSON Schema validation failed: {error}").into())
}

fn safety(semantic_revision_wait_used: bool) -> Value {
    json!({
        "provider": "uix-agent-v1",
        "providerCache": "disabled",
        "sameAuthenticatedConnectionUsed": true,
        "windowGenerationFixed": true,
        "semanticRevisionWaitUsed": semantic_revision_wait_used,
        "providerPolling": false,
        "stabilitySemantics": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "sensitiveSnapshotPublished": false,
        "hostCoordinateMappingPublished": false,
        "foregroundActivationRequested": false,
        "desktopInputInjected": false,
        "independentKeyOwnershipExposed": false,
        "textInputSupported": false,
        "x11Used": false,
        "fallback": "none"
    })
}

fn envelope(data: Value) -> Value {
    json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.key.transition@1",
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

#[test]
fn result_schema_accepts_unsettled_press_with_verified_postcondition()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "key": "enter",
        "modifiers": ["control"],
        "postcondition": {
            "selector": { "automationId": "saved" },
            "condition": "unique"
        },
        "timeoutMs": 1000
    });
    let input_schema = schema(include_str!(
        "../contracts/v1/uix-key-transition-input.schema.json"
    ))?;
    assert_valid(&input_schema, &input)?;

    // actionSettled=false 只表示动作尚未给出 settle 事实，不否定同连接后置观察。
    let result = envelope(json!({
        "ok": true,
        "contractVersion": "act/ui-key-transition/v1",
        "capability": "ui.input.key.transition@1",
        "targetId": "s2:w:0000000000000000",
        "action": "press",
        "key": "enter",
        "modifiers": ["control"],
        "actionAccepted": true,
        "actionRevision": 6,
        "actionPresentedRevision": 6,
        "actionSettled": false,
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
            "automationId": "saved",
            "role": "status",
            "name": "保存",
            "focused": false,
            "enabled": true,
            "actions": [],
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
        "effectObservation": "semantic-postcondition-after-key-dispatch-no-causal-claim",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": 1000,
        "executionRealm": "same-session-no-focus",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety(true)
    }));
    let result_schema = schema(include_str!(
        "../contracts/v1/uix-key-transition.schema.json"
    ))?;
    assert_valid(&result_schema, &result)?;
    Ok(())
}

#[test]
fn input_schema_rejects_unknown_null_float_invalid_key_modifier_and_selector_values()
-> Result<(), Box<dyn std::error::Error>> {
    let input_schema = schema(include_str!(
        "../contracts/v1/uix-key-transition-input.schema.json"
    ))?;
    let postcondition = json!({
        "selector": { "role": "button" },
        "condition": "unique"
    });
    for invalid in [
        json!({ "unexpected": true }),
        json!({ "key": null, "postcondition": postcondition.clone() }),
        json!({ "key": "f13", "postcondition": postcondition.clone() }),
        json!({ "key": "enter", "modifiers": ["alt", "alt"], "postcondition": postcondition.clone() }),
        json!({ "key": "enter", "timeoutMs": 100.5, "postcondition": postcondition.clone() }),
        json!({ "key": "enter", "timeoutMs": 99, "postcondition": postcondition.clone() }),
        json!({ "key": "enter", "timeoutMs": 30001, "postcondition": postcondition.clone() }),
        json!({
            "key": "enter",
            "postcondition": { "selector": {}, "condition": "unique" }
        }),
        json!({
            "key": "enter",
            "postcondition": { "selector": { "role": "button" }, "condition": "other" }
        }),
    ] {
        assert!(jsonschema::draft202012::validate(&input_schema, &invalid).is_err());
    }
    Ok(())
}

#[test]
fn confirmation_dispatch_and_postcondition_failures_remain_non_retryable() {
    let module = include_str!("../src/modules/linux_uix_key_transition.rs");
    let adapter = include_str!("../src/adapters/linux/uix_agent_key_transition.rs");
    let Some(confirmation) = module.find("if !confirmed") else {
        panic!("key transition 必须先检查 confirmation");
    };
    let Some(input) = module.find("UixKeyTransitionInput::parse") else {
        panic!("key transition 必须解析输入");
    };
    let Some(provider) = module.find(".perform(session_id") else {
        panic!("key transition 必须通过 provider port 执行动作");
    };
    assert!(confirmation < input && input < provider);

    let Some(dispatch) = adapter.find("perform_targetless_with_request_id") else {
        panic!("Adapter 必须先执行完整 press_key");
    };
    let Some(wait) = adapter.find("execute_wait_from_baseline") else {
        panic!("Adapter 必须在 dispatch 后执行 semantic wait");
    };
    assert!(dispatch < wait);
    assert!(adapter.contains("Postcondition(ElementWaitFailure)"));
    assert!(module.contains("ElementWaitFailure::Ambiguous"));
    assert!(module.contains("AMBIGUOUS_TARGET"));
    assert!(module.contains("OUTCOME_UNKNOWN"));
    assert!(module.contains("automaticRetryProhibited"));
    assert!(module.contains("retrySafe"));
    assert!(module.contains("automatic retry is prohibited"));
}

#[test]
fn result_and_implementation_forbid_foreground_desktop_input_and_fallback() {
    let schema = include_str!("../contracts/v1/uix-key-transition.schema.json");
    let module = include_str!("../src/modules/linux_uix_key_transition.rs");
    let adapter = include_str!("../src/adapters/linux/uix_agent_key_transition.rs");
    for source in [schema, module, adapter] {
        assert!(!source.contains("xdotool"));
        assert!(!source.contains("XTest"));
        assert!(!source.contains("Xlib"));
        assert!(!source.contains("xcb_"));
    }
    assert!(module.contains("\"executionRealm\": \"same-session-no-focus\""));
    assert!(module.contains("\"hostForegroundActivationRequested\": false"));
    assert!(module.contains("\"desktopInputInjected\": false"));
    assert!(module.contains("\"fallback\": \"none\""));
    assert!(schema.contains("\"executionRealm\": { \"const\": \"same-session-no-focus\" }"));
    assert!(schema.contains("\"desktopInputInjected\": { \"const\": false }"));
    assert!(schema.contains("\"fallback\": { \"const\": \"none\" }"));
}
