#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 混合输入序列 transition 的输入、结果与安全边界契约回归。

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
        "x11Used": false,
        "fallback": "none"
    })
}

fn envelope(data: Value) -> Value {
    json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.sequence.transition@1",
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
            "name": "输入序列结果",
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
        "contractVersion": "act/ui-input-sequence-transition/v1",
        "capability": "ui.input.sequence.transition@1",
        "targetId": "s2:w:0000000000000000",
        "action": "input-sequence",
        "coordinateSpace": "client-logical-px",
        "steps": [
            { "type": "press", "key": "enter", "modifiers": ["control"] },
            { "type": "move", "x": 10.0, "y": 20.0 },
            { "type": "click", "x": 30.0, "y": 40.0 }
        ],
        "stepsRequested": 3,
        "stepsAccepted": 3,
        "pressesAccepted": 1,
        "movesAccepted": 1,
        "clicksAccepted": 1,
        "intervalMs": 20,
        "plannedDurationMs": 40,
        "allPressesBalanced": true,
        "allClicksBalanced": true,
        "transactionSemantics": false,
        "rollbackSemantics": false,
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
        "effectObservation": "semantic-postcondition-after-complete-input-sequence-no-causal-claim",
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
fn schemas_accept_unique_missing_and_unsettled_sequence_results()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "coordinateSpace": "client-logical-px",
        "steps": [
            { "type": "press", "key": "enter", "modifiers": [] },
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
        "../../contracts/v1/uix-input-sequence-transition-input.schema.json"
    ))?;
    assert_valid(&input_schema, &input)?;

    let result_schema = schema(include_str!(
        "../../contracts/v1/uix-input-sequence-transition-result.schema.json"
    ))?;
    let unique = envelope(result_data("unique", 1, false));
    assert_eq!(unique["data"]["transactionSemantics"], false);
    assert_eq!(unique["data"]["rollbackSemantics"], false);
    assert_valid(&result_schema, &unique)?;
    assert_valid(&result_schema, &envelope(result_data("missing", 0, true)))?;
    Ok(())
}

#[test]
fn input_schema_rejects_unknown_null_invalid_modality_and_postcondition()
-> Result<(), Box<dyn std::error::Error>> {
    let input_schema = schema(include_str!(
        "../../contracts/v1/uix-input-sequence-transition-input.schema.json"
    ))?;
    let postcondition = json!({
        "selector": { "role": "button" },
        "condition": "unique"
    });
    for invalid in [
        json!({ "unexpected": true }),
        json!({
            "coordinateSpace": null,
            "steps": [
                { "type": "press", "key": "a" },
                { "type": "move", "x": 1, "y": 2 }
            ],
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "screen-physical-px",
            "steps": [
                { "type": "press", "key": "a" },
                { "type": "move", "x": 1, "y": 2 }
            ],
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "press", "key": "a" },
                { "type": "move", "x": 1, "y": 2 }
            ],
            "timeoutMs": 100.5,
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "press", "key": "a" },
                { "type": "press", "key": "b" }
            ],
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "press", "key": "a" },
                { "type": "move", "x": 1, "y": 2 }
            ],
            "postcondition": { "selector": {}, "condition": "unique" }
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "press", "key": "a" },
                { "type": "move", "x": 1, "y": 2 }
            ],
            "postcondition": { "selector": { "role": "button" }, "condition": "stable" }
        }),
    ] {
        assert!(
            jsonschema::draft202012::validate(&input_schema, &invalid).is_err(),
            "非法 input sequence transition 输入不应通过: {invalid}"
        );
    }
    Ok(())
}

#[test]
fn confirmation_source_order_is_checked_when_transition_module_exists() {
    let Some(module) = source("src/modules/linux_uix_input_sequence_transition.rs") else {
        eprintln!("跳过：input sequence transition Module 尚未出现");
        return;
    };
    let Some(confirmation) = module.find("if !confirmed") else {
        panic!("Module 必须先检查 confirmation");
    };
    let Some(input) = module.find("UixInputSequenceTransitionInput::parse") else {
        panic!("Module 必须解析 input sequence transition 输入");
    };
    let Some(provider) = module.find(".perform(session_id") else {
        panic!("Module 必须通过 provider port 执行混合序列");
    };
    assert!(confirmation < input && input < provider);
}

#[test]
fn partial_ambiguity_and_unknown_results_prohibit_automatic_retry_when_implemented() {
    let Some(adapter) = source("src/adapters/linux/uix_agent_input_sequence_transition.rs") else {
        eprintln!("跳过：input sequence transition Adapter 尚未出现");
        return;
    };
    for marker in [
        "accepted_may_have_occurred",
        "sequence_completed",
        "Postcondition(ElementWaitFailure)",
    ] {
        assert!(
            adapter.contains(marker),
            "Adapter 缺少不确定性事实: {marker}"
        );
    }
    let Some(module) = source("src/modules/linux_uix_input_sequence_transition.rs") else {
        eprintln!("跳过 Module 不确定性断言：input sequence transition Module 尚未出现");
        return;
    };
    for marker in [
        "AMBIGUOUS_TARGET",
        "OUTCOME_UNKNOWN",
        "automaticRetryProhibited",
        "retrySafe",
        "accepted_may_have_occurred",
    ] {
        assert!(module.contains(marker), "Module 缺少不确定性边界: {marker}");
    }
}

#[test]
fn result_and_implementation_forbid_secrets_x11_and_host_input() {
    let schema = include_str!("../../contracts/v1/uix-input-sequence-transition-result.schema.json");
    for marker in [
        "\"desktopInputInjected\": { \"const\": false }",
        "\"desktopPointerMoved\": { \"const\": false }",
        "\"nativeIdentityExposed\": { \"const\": false }",
        "\"transportIdentityExposed\": { \"const\": false }",
        "\"x11Used\": { \"const\": false }",
        "\"fallback\": { \"const\": \"none\" }",
        "\"independentKeyOwnership\": { \"const\": false }",
        "\"independentButtonOwnership\": { \"const\": false }",
        "\"dragSupported\": { \"const\": false }",
        "\"doubleClickSupported\": { \"const\": false }",
        "\"clickCountSupported\": { \"const\": false }",
        "\"scrollSupported\": { \"const\": false }",
    ] {
        assert!(schema.contains(marker), "结果契约缺少安全事实: {marker}");
    }

    for relative_path in [
        "src/adapters/linux/uix_agent_input_sequence_transition.rs",
        "src/modules/linux_uix_input_sequence_transition.rs",
    ] {
        let Some(implementation) = source(relative_path) else {
            eprintln!("跳过安全源码断言：{relative_path} 尚未出现");
            continue;
        };
        let implementation = implementation
            .split_once("#[cfg(test)]")
            .map_or(implementation.as_str(), |(production, _)| production);
        for forbidden in [
            "process_id",
            "descriptor.token",
            "xdotool",
            "XTest",
            "Xlib",
            "xcb_",
        ] {
            assert!(
                !implementation.contains(forbidden),
                "{relative_path} 不得暴露或使用 {forbidden}"
            );
        }
    }
}

#[test]
fn unconfirmed_cli_fails_before_input_target_or_provider_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.sequence.transition@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("未确认 input sequence transition 必须优先失败"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}
