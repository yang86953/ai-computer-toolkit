#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 普通左键 click transition 的输入、结果和安全边界契约回归。

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
        "desktopPointerMoved": false,
        "requestScopedBalancedClicks": true,
        "independentButtonOwnership": false,
        "doubleClickSupported": false,
        "clickCountSupported": false,
        "x11Used": false,
        "fallback": "none"
    })
}

fn envelope(data: Value) -> Value {
    json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.pointer.click.transition@1",
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

fn result_data(condition: &str, wait_count: u64, action_settled: bool) -> Value {
    let missing = condition == "missing";
    let element = if missing {
        Value::Null
    } else {
        json!({
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
        })
    };
    json!({
        "ok": true,
        "contractVersion": "act/ui-pointer-click-transition/v1",
        "capability": "ui.input.pointer.click.transition@1",
        "targetId": "s2:w:0000000000000000",
        "action": "click",
        "coordinateSpace": "client-logical-px",
        "x": 10.5,
        "y": 20.25,
        "actionAccepted": true,
        "actionRevision": 6,
        "actionPresentedRevision": 6,
        "actionSettled": action_settled,
        "postcondition": condition,
        "selectorSemantics": "exact-and",
        "postconditionMatchedAfterDispatch": true,
        "snapshotId": "as3:0000000000000001",
        "matchState": if missing { "missing" } else { "unique" },
        "matchCount": if missing { 0 } else { 1 },
        "sampleCount": 2,
        "waitCount": wait_count,
        "revision": 7,
        "presentedRevision": 7,
        "revisionWaitUsed": wait_count > 0,
        "element": element,
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
        "effectObservation": "semantic-postcondition-after-click-dispatch-no-causal-claim",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": 1000,
        "executionRealm": "same-session-no-focus",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety(wait_count > 0)
    })
}

#[test]
fn input_and_result_schema_accept_unsettled_click_and_nullable_missing_variant()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "coordinateSpace": "client-logical-px",
        "x": 10.5,
        "y": 20.25,
        "postcondition": {
            "selector": { "automationId": "saved" },
            "condition": "unique"
        },
        "timeoutMs": 1000
    });
    let input_schema = schema(include_str!(
        "../../contracts/v1/uix-pointer-click-transition-input.schema.json"
    ))?;
    assert_valid(&input_schema, &input)?;

    let result_schema = schema(include_str!(
        "../../contracts/v1/uix-pointer-click-transition-result.schema.json"
    ))?;
    assert_valid(&result_schema, &envelope(result_data("unique", 1, false)))?;
    assert_valid(&result_schema, &envelope(result_data("missing", 0, true)))?;
    Ok(())
}

#[test]
fn input_schema_rejects_unknown_null_float_coordinate_and_non_click_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let input_schema = schema(include_str!(
        "../../contracts/v1/uix-pointer-click-transition-input.schema.json"
    ))?;
    let postcondition = json!({
        "selector": { "role": "button" },
        "condition": "unique"
    });
    for invalid in [
        json!({ "unexpected": true }),
        json!({ "coordinateSpace": null, "x": 1, "y": 2, "postcondition": postcondition.clone() }),
        json!({ "coordinateSpace": "screen-physical-px", "x": 1, "y": 2, "postcondition": postcondition.clone() }),
        json!({ "coordinateSpace": "client-logical-px", "x": -0.1, "y": 2, "postcondition": postcondition.clone() }),
        json!({ "coordinateSpace": "client-logical-px", "x": 1, "y": 65_535.1, "postcondition": postcondition.clone() }),
        json!({ "coordinateSpace": "client-logical-px", "x": 1, "y": 2, "postcondition": postcondition.clone(), "timeoutMs": 100.5 }),
        json!({ "coordinateSpace": "client-logical-px", "x": 1, "y": 2, "postcondition": postcondition.clone(), "button": "right" }),
        json!({ "coordinateSpace": "client-logical-px", "x": 1, "y": 2, "postcondition": postcondition.clone(), "clickCount": 2 }),
        json!({ "coordinateSpace": "client-logical-px", "x": 1, "y": 2, "postcondition": postcondition.clone(), "intervalMs": 100 }),
        json!({ "coordinateSpace": "client-logical-px", "x": 1, "y": 2, "postcondition": { "selector": {}, "condition": "unique" } }),
        json!({ "coordinateSpace": "client-logical-px", "x": 1, "y": 2, "postcondition": { "selector": { "role": "button" }, "condition": "other" } }),
    ] {
        assert!(jsonschema::draft202012::validate(&input_schema, &invalid).is_err());
    }
    Ok(())
}

#[test]
fn confirmation_dispatch_and_semantic_wait_keep_the_required_source_order() {
    let module = include_str!("../../src/modules/linux_uix_pointer_click_transition.rs");
    let adapter = include_str!("../../src/adapters/linux/uix_agent_pointer_click_transition.rs");
    let Some(confirmation) = module.find("if !confirmed") else {
        panic!("pointer click transition 必须先检查 confirmation");
    };
    let Some(input) = module.find("UixPointerClickTransitionInput::parse") else {
        panic!("pointer click transition 必须解析输入");
    };
    let Some(provider) = module.find(".perform(session_id") else {
        panic!("pointer click transition 必须通过 provider port 执行动作");
    };
    assert!(confirmation < input && input < provider);

    let Some(dispatch) = adapter.find("perform_targetless_with_request_id") else {
        panic!("Adapter 必须先执行普通左键 click_at");
    };
    let Some(wait) = adapter.find("execute_wait_from_baseline") else {
        panic!("Adapter 必须在 click dispatch 后执行 semantic wait");
    };
    assert!(dispatch < wait);
    assert!(!adapter[..dispatch].contains("snapshot("));
}

#[test]
fn ambiguity_and_unknown_results_prohibit_automatic_retry() {
    let module = include_str!("../../src/modules/linux_uix_pointer_click_transition.rs");
    let adapter = include_str!("../../src/adapters/linux/uix_agent_pointer_click_transition.rs");
    assert!(module.contains("AMBIGUOUS_TARGET"));
    assert!(module.contains("OUTCOME_UNKNOWN"));
    assert!(module.contains("automaticRetryProhibited"));
    assert!(module.contains("retrySafe"));
    assert!(module.contains("automatic retry is prohibited"));
    assert!(module.contains("accepted_may_have_occurred"));
    assert!(adapter.contains("Postcondition(ElementWaitFailure)"));
    assert!(module.contains("ElementWaitFailure::Ambiguous"));
}

#[test]
fn result_and_implementation_forbid_foreground_desktop_input_and_fallback() {
    let schema = include_str!("../../contracts/v1/uix-pointer-click-transition-result.schema.json");
    let module = include_str!("../../src/modules/linux_uix_pointer_click_transition.rs");
    let adapter = include_str!("../../src/adapters/linux/uix_agent_pointer_click_transition.rs");
    for source in [schema, module, adapter] {
        assert!(!source.contains("xdotool"));
        assert!(!source.contains("XTest"));
        assert!(!source.contains("Xlib"));
        assert!(!source.contains("xcb_"));
    }
    assert!(module.contains("\"executionRealm\": \"same-session-no-focus\""));
    assert!(module.contains("\"hostForegroundActivationRequested\": false"));
    assert!(module.contains("\"desktopInputInjected\": false"));
    assert!(module.contains("\"desktopPointerMoved\": false"));
    assert!(module.contains("\"fallback\": \"none\""));
    assert!(schema.contains("\"executionRealm\": { \"const\": \"same-session-no-focus\" }"));
    assert!(schema.contains("\"desktopInputInjected\": { \"const\": false }"));
    assert!(schema.contains("\"desktopPointerMoved\": { \"const\": false }"));
    assert!(schema.contains("\"fallback\": { \"const\": \"none\" }"));
}
