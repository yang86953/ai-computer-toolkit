#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 完整指针序列 transition 的输入、结果与安全边界契约回归。

use std::{fs, path::PathBuf};

use ai_computer_toolkit::cli;
use serde_json::{Value, json};

fn schema(source: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(source)?)
}

fn assert_valid(schema: &Value, instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    jsonschema::draft202012::validate(schema, instance)
        .map_err(|error| format!("JSON Schema validation failed: {error}").into())
}

fn source(relative_path: &str) -> Option<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    fs::read_to_string(path).ok()
}

fn safety(semantic_revision_wait_used: bool) -> Value {
    json!({
        "provider": "uix-agent-v1",
        "providerCache": "disabled",
        "sameAuthenticatedConnectionUsed": true,
        "windowGenerationFixed": true,
        "revisionChainUsed": true,
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
        "dragSupported": false,
        "doubleClickSupported": false,
        "clickCountSupported": false,
        "scrollSupported": false,
        "x11Used": false,
        "fallback": "none"
    })
}

fn envelope(data: Value) -> Value {
    json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.pointer.sequence.transition@1",
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

fn result_data(condition: &str, wait_count: u64, sequence_settled: bool) -> Value {
    let missing = condition == "missing";
    let element = if missing {
        Value::Null
    } else {
        json!({
            "elementId": "s2:e:0000000000000001",
            "identityFreshness": "snapshot-revision",
            "automationId": "result",
            "role": "status",
            "name": "序列结果",
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
        "contractVersion": "act/ui-pointer-sequence-transition/v1",
        "capability": "ui.input.pointer.sequence.transition@1",
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
        "intervalMs": 0,
        "plannedDurationMs": 0,
        "allClicksBalanced": true,
        "sequenceRevision": 7,
        "sequencePresentedRevision": 7,
        "sequenceSettled": sequence_settled,
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
        "effectObservation": "semantic-postcondition-after-complete-pointer-sequence-no-causal-claim",
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
fn input_and_result_schema_accept_unique_missing_and_unsettled_sequence()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "coordinateSpace": "client-logical-px",
        "steps": [
            { "type": "move", "x": 10.5, "y": 20.25 },
            { "type": "click", "x": 30.0, "y": 40.0 }
        ],
        "intervalMs": 20,
        "timeoutMs": 1000,
        "postcondition": {
            "selector": { "automationId": "result" },
            "condition": "unique"
        }
    });
    let input_schema = schema(include_str!(
        "../../contracts/v1/uix-pointer-sequence-transition-input.schema.json"
    ))?;
    assert_valid(&input_schema, &input)?;

    let result_schema = schema(include_str!(
        "../../contracts/v1/uix-pointer-sequence-transition-result.schema.json"
    ))?;
    assert_valid(&result_schema, &envelope(result_data("unique", 1, false)))?;
    assert_valid(&result_schema, &envelope(result_data("missing", 0, true)))?;
    Ok(())
}

#[test]
fn input_schema_rejects_unknown_null_float_and_non_sequence_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let input_schema = schema(include_str!(
        "../../contracts/v1/uix-pointer-sequence-transition-input.schema.json"
    ))?;
    let postcondition = json!({
        "selector": { "role": "button" },
        "condition": "unique"
    });
    for invalid in [
        json!({ "unexpected": true }),
        json!({
            "coordinateSpace": null,
            "steps": [],
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "screen-physical-px",
            "steps": [
                { "type": "move", "x": 1, "y": 2 },
                { "type": "click", "x": 3, "y": 4 }
            ],
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "move", "x": 1, "y": 2 },
                { "type": "click", "x": 3, "y": 4 }
            ],
            "timeoutMs": 100.5,
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "pointer-down", "x": 1, "y": 2 },
                { "type": "click", "x": 3, "y": 4 }
            ],
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "move", "x": 1, "y": 2 },
                { "type": "click", "x": 3, "y": 4 }
            ],
            "button": "right",
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "move", "x": 1, "y": 2 },
                { "type": "click", "x": 3, "y": 4 }
            ],
            "postcondition": { "selector": {}, "condition": "unique" }
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "move", "x": 1, "y": 2 },
                { "type": "click", "x": 3, "y": 4 }
            ],
            "postcondition": { "selector": { "role": "button" }, "condition": "other" }
        }),
    ] {
        assert!(
            jsonschema::draft202012::validate(&input_schema, &invalid).is_err(),
            "非法 pointer sequence transition 输入不应通过: {invalid}"
        );
    }
    Ok(())
}

#[test]
fn confirmation_sequence_and_semantic_wait_have_required_source_order() {
    let Some(adapter) = source("src/adapters/linux/uix_agent_pointer_sequence_transition.rs")
    else {
        eprintln!("跳过：pointer sequence transition Adapter 尚未出现");
        return;
    };
    let Some(sequence_adapter) = source("src/adapters/linux/uix_agent_pointer_sequence.rs") else {
        eprintln!("跳过：既有 pointer sequence Adapter 尚未出现");
        return;
    };
    let Some(dispatch) = sequence_adapter.find("perform_targetless_with_request_id") else {
        panic!("sequence Adapter 必须发送 targetless pointer action");
    };
    let Some(wait) = adapter.find("execute_wait_from_baseline") else {
        panic!("transition Adapter 必须在完整序列后执行 semantic wait");
    };
    assert!(adapter.contains("execute_sequence"));
    assert!(
        adapter
            .find("execute_sequence")
            .is_some_and(|index| index < wait)
    );
    assert!(!sequence_adapter[..dispatch].contains("snapshot("));

    let Some(module) = source("src/modules/linux_uix_pointer_sequence_transition.rs") else {
        eprintln!("跳过 Module 顺序断言：pointer sequence transition Module 尚未出现");
        return;
    };
    let Some(confirmation) = module.find("if !confirmed") else {
        panic!("Module 必须先检查 confirmation");
    };
    let Some(input) = module.find("UixPointerSequenceTransitionInput::parse") else {
        panic!("Module 必须解析 pointer sequence transition 输入");
    };
    let Some(provider) = module.find(".perform(session_id") else {
        panic!("Module 必须通过 provider port 执行完整序列");
    };
    assert!(confirmation < input && input < provider);
}

#[test]
fn unconfirmed_cli_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.pointer.sequence.transition@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not-read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("未确认 pointer sequence transition 必须优先失败"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn partial_ambiguity_and_unknown_results_prohibit_automatic_retry() {
    let Some(adapter) = source("src/adapters/linux/uix_agent_pointer_sequence_transition.rs")
    else {
        eprintln!("跳过：pointer sequence transition Adapter 尚未出现");
        return;
    };
    assert!(adapter.contains("accepted_may_have_occurred"));
    assert!(adapter.contains("sequence_completed"));
    assert!(adapter.contains("Postcondition(ElementWaitFailure)"));

    let Some(module) = source("src/modules/linux_uix_pointer_sequence_transition.rs") else {
        eprintln!("跳过：pointer sequence transition Module 尚未出现");
        return;
    };
    assert!(module.contains("AMBIGUOUS_TARGET"));
    assert!(module.contains("OUTCOME_UNKNOWN"));
    assert!(module.contains("automaticRetryProhibited"));
    assert!(module.contains("retrySafe"));
    assert!(module.contains("accepted_may_have_occurred"));
    assert!(module.contains("ElementWaitFailure::Ambiguous"));
}

#[test]
fn result_and_implementation_forbid_foreground_desktop_input_and_fallback() {
    let schema = include_str!("../../contracts/v1/uix-pointer-sequence-transition-result.schema.json");
    let Some(adapter) = source("src/adapters/linux/uix_agent_pointer_sequence_transition.rs")
    else {
        eprintln!("跳过 Adapter 安全源码断言：pointer sequence transition Adapter 尚未出现");
        return;
    };
    let sequence_adapter = source("src/adapters/linux/uix_agent_pointer_sequence.rs");
    let Some(module) = source("src/modules/linux_uix_pointer_sequence_transition.rs") else {
        eprintln!("跳过 Module 安全源码断言：pointer sequence transition Module 尚未出现");
        return;
    };
    assert!(!module.contains("process_id"));
    assert!(!module.contains("descriptor.token"));
    for implementation in [
        Some(adapter.as_str()),
        sequence_adapter.as_deref(),
        Some(module.as_str()),
    ] {
        let Some(implementation) = implementation else {
            continue;
        };
        assert!(!implementation.contains("xdotool"));
        assert!(!implementation.contains("XTest"));
        assert!(!implementation.contains("Xlib"));
        assert!(!implementation.contains("xcb_"));
    }
    assert!(
        schema.contains("semantic-postcondition-after-complete-pointer-sequence-no-causal-claim")
    );
    assert!(schema.contains("\"applicationConsumptionConfirmed\": { \"const\": false }"));
    assert!(schema.contains("\"desktopInputInjected\": { \"const\": false }"));
    assert!(schema.contains("\"desktopPointerMoved\": { \"const\": false }"));
    assert!(schema.contains("\"requestScopedBalancedClicks\": { \"const\": true }"));
    assert!(schema.contains("\"fallback\": { \"const\": \"none\" }"));
}
