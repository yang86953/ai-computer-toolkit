#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 请求内配平拖拽 transition 的输入、结果和安全边界契约回归。

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
        "requestScopedBalancedButtons": true,
        "buttonReleaseConfirmed": true,
        "independentButtonOwnership": false,
        "x11Used": false,
        "fallback": "none"
    })
}

fn envelope(data: Value) -> Value {
    json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.pointer.drag.transition@1",
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

fn result_data(condition: &str, wait_count: u64, drag_settled: bool) -> Value {
    let missing = condition == "missing";
    let element = if missing {
        Value::Null
    } else {
        json!({
            "elementId": "s2:e:0000000000000001",
            "identityFreshness": "snapshot-revision",
            "automationId": "drag-target",
            "role": "status",
            "name": "拖拽目标",
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
        "contractVersion": "act/ui-pointer-drag-transition/v1",
        "capability": "ui.input.pointer.drag.transition@1",
        "targetId": "s2:w:0000000000000000",
        "action": "drag",
        "button": "left",
        "coordinateSpace": "client-logical-px",
        "start": { "x": 10.5, "y": 20.25 },
        "end": { "x": 100.0, "y": 200.0 },
        "samplesRequested": 4,
        "moveSamplesAccepted": 4,
        "durationMs": 250,
        "pointerDownAccepted": true,
        "pointerUpAccepted": true,
        "buttonReleaseConfirmed": true,
        "requestScopedBalancedButtons": true,
        "dragRevision": 6,
        "dragPresentedRevision": 6,
        "dragSettled": drag_settled,
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
        "applicationConsumptionConfirmed": false,
        "causalityConfirmed": false,
        "finalStateReached": false,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "hostForegroundActivationRequested": false,
        "effectObservation": "semantic-postcondition-after-balanced-drag-release-no-causal-claim",
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
fn input_and_result_schema_accept_unique_missing_and_unsettled_drag()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "coordinateSpace": "client-logical-px",
        "start": { "x": 10.5, "y": 20.25 },
        "end": { "x": 100, "y": 200 },
        "samples": 4,
        "durationMs": 250,
        "timeoutMs": 1000,
        "postcondition": {
            "selector": { "automationId": "drag-target" },
            "condition": "unique"
        }
    });
    let input_schema = schema(include_str!(
        "../contracts/v1/uix-pointer-drag-transition-input.schema.json"
    ))?;
    assert_valid(&input_schema, &input)?;

    let result_schema = schema(include_str!(
        "../contracts/v1/uix-pointer-drag-transition-result.schema.json"
    ))?;
    assert_valid(&result_schema, &envelope(result_data("unique", 1, false)))?;
    assert_valid(&result_schema, &envelope(result_data("missing", 0, false)))?;
    Ok(())
}

#[test]
fn input_schema_rejects_unknown_independent_button_and_invalid_drag_or_condition()
-> Result<(), Box<dyn std::error::Error>> {
    let input_schema = schema(include_str!(
        "../contracts/v1/uix-pointer-drag-transition-input.schema.json"
    ))?;
    let postcondition = json!({
        "selector": { "role": "status" },
        "condition": "unique"
    });
    for invalid in [
        json!({ "unexpected": true }),
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 1, "y": 2 },
            "end": { "x": 3, "y": 4 },
            "postcondition": postcondition.clone(),
            "button": "right"
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 1, "y": 2, "pointerDown": true },
            "end": { "x": 3, "y": 4 },
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": -0.1, "y": 2 },
            "end": { "x": 3, "y": 4 },
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 1, "y": 2 },
            "end": { "x": 65_535.1, "y": 4 },
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 1, "y": 2 },
            "end": { "x": 3, "y": 4 },
            "samples": 0,
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 1, "y": 2 },
            "end": { "x": 3, "y": 4 },
            "durationMs": 5001,
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 1, "y": 2 },
            "end": { "x": 3, "y": 4 },
            "timeoutMs": 100.5,
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 1, "y": 2 },
            "end": { "x": 3, "y": 4 },
            "postcondition": { "selector": {}, "condition": "unique" }
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 1, "y": 2 },
            "end": { "x": 3, "y": 4 },
            "postcondition": { "selector": { "role": null }, "condition": "unique" }
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 1, "y": 2 },
            "end": { "x": 3, "y": 4 },
            "postcondition": { "selector": { "role": "status" }, "condition": "other" }
        }),
    ] {
        assert!(jsonschema::draft202012::validate(&input_schema, &invalid).is_err());
    }
    Ok(())
}

#[test]
fn confirmation_drag_release_and_semantic_wait_keep_the_required_source_order() {
    let module = include_str!("../src/modules/linux_uix_pointer_drag_transition.rs");
    let adapter = include_str!("../src/adapters/linux/uix_agent_pointer_drag_transition.rs");
    let drag_adapter = include_str!("../src/adapters/linux/uix_agent_pointer_drag.rs");
    let Some(confirmation) = module.find("if !confirmed") else {
        panic!("pointer drag transition 必须先检查 confirmation");
    };
    let Some(input) = module.find("UixPointerDragTransitionInput::parse") else {
        panic!("pointer drag transition 必须解析输入");
    };
    let Some(provider) = module.find(".drag(session_id") else {
        panic!("pointer drag transition 必须通过 provider port 执行动作");
    };
    assert!(confirmation < input && input < provider);

    let Some(down) = drag_adapter.find("pointer_action(\"pointer_down\"") else {
        panic!("Adapter 必须先执行 pointer_down");
    };
    let Some(moves) = drag_adapter.find("pointer_action(\"pointer_move\"") else {
        panic!("Adapter 必须在 down 后执行 pointer_move 序列");
    };
    let Some(release) = drag_adapter.find("pointer_action(\"pointer_up\"") else {
        panic!("Adapter 必须执行 pointer_up release");
    };
    let Some(wait) = adapter.find("execute_wait_from_baseline") else {
        panic!("Adapter 必须在 confirmed release 后执行 semantic wait");
    };
    assert!(down < moves && moves < release);
    let Some(drag) = adapter.find("execute_drag") else {
        panic!("transition Adapter 必须复用请求内配平拖拽");
    };
    assert!(drag < wait);
}

#[test]
fn partial_release_unknown_and_postcondition_ambiguity_prohibit_retry_and_wait() {
    let module = include_str!("../src/modules/linux_uix_pointer_drag_transition.rs");
    let adapter = include_str!("../src/adapters/linux/uix_agent_pointer_drag_transition.rs");
    assert!(module.contains("OUTCOME_UNKNOWN"));
    assert!(module.contains("AMBIGUOUS_TARGET"));
    assert!(module.contains("automaticRetryProhibited"));
    assert!(module.contains("retrySafe"));
    assert!(module.contains("automatic retry is prohibited"));
    assert!(module.contains("accepted_may_have_occurred"));
    assert!(module.contains("ElementWaitFailure::Ambiguous"));
    assert!(adapter.contains("button_release_confirmed"));
    assert!(adapter.contains("button_release_attempted"));
    assert!(adapter.contains("pointer_down_may_have_occurred"));
    assert!(adapter.contains("Postcondition(ElementWaitFailure)"));

    let Some(wait) = adapter.find("execute_wait_from_baseline") else {
        panic!("Adapter 必须只在完整 release 成功后进入 semantic wait");
    };
    assert!(adapter[..wait].contains("pointer_up"));
}

#[test]
fn result_and_implementation_forbid_desktop_pointer_x11_and_fallback() {
    let schema = include_str!("../contracts/v1/uix-pointer-drag-transition-result.schema.json");
    let module = include_str!("../src/modules/linux_uix_pointer_drag_transition.rs");
    let adapter = include_str!("../src/adapters/linux/uix_agent_pointer_drag_transition.rs");
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
    assert!(module.contains("\"applicationConsumptionConfirmed\": false"));
    assert!(module.contains("\"fallback\": \"none\""));
    assert!(schema.contains("\"executionRealm\": { \"const\": \"same-session-no-focus\" }"));
    assert!(schema.contains("\"applicationConsumptionConfirmed\": { \"const\": false }"));
    assert!(schema.contains("\"desktopInputInjected\": { \"const\": false }"));
    assert!(schema.contains("\"desktopPointerMoved\": { \"const\": false }"));
    assert!(schema.contains("\"requestScopedBalancedButtons\": { \"const\": true }"));
    assert!(schema.contains("\"buttonReleaseConfirmed\": { \"const\": true }"));
    assert!(schema.contains("\"independentButtonOwnership\": { \"const\": false }"));
    assert!(schema.contains("\"x11Used\": { \"const\": false }"));
    assert!(schema.contains("\"fallback\": { \"const\": \"none\" }"));
}
