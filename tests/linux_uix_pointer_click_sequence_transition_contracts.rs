#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 普通左键点击序列 transition 的输入、结果与安全边界契约回归。

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
        "capability": "ui.input.pointer.click.sequence.transition@1",
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
            "name": "点击序列结果",
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
        "contractVersion": "act/ui-pointer-click-sequence-transition/v1",
        "capability": "ui.input.pointer.click.sequence.transition@1",
        "targetId": "s2:w:0000000000000000",
        "action": "click-sequence",
        "coordinateSpace": "client-logical-px",
        "clicks": [
            { "x": 10.5, "y": 20.25 },
            { "x": 30.0, "y": 40.0 }
        ],
        "clicksRequested": 2,
        "clicksAccepted": 2,
        "intervalMs": 20,
        "plannedDurationMs": 20,
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
        "effectObservation": "semantic-postcondition-after-complete-pointer-click-sequence-no-causal-claim",
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
fn schemas_accept_unique_missing_and_unsettled_click_sequence_results()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "coordinateSpace": "client-logical-px",
        "clicks": [
            { "x": 10.5, "y": 20.25 },
            { "x": 30.0, "y": 40.0 }
        ],
        "intervalMs": 20,
        "timeoutMs": 1000,
        "postcondition": {
            "selector": { "automationId": "result" },
            "condition": "unique"
        }
    });
    let input_schema = schema(include_str!(
        "../contracts/v1/uix-pointer-click-sequence-transition-input.schema.json"
    ))?;
    assert_valid(&input_schema, &input)?;

    let result_schema = schema(include_str!(
        "../contracts/v1/uix-pointer-click-sequence-transition-result.schema.json"
    ))?;
    let unique = envelope(result_data("unique", 1, false));
    assert_eq!(unique["data"]["transactionSemantics"], false);
    assert_eq!(unique["data"]["rollbackSemantics"], false);
    assert_valid(&result_schema, &unique)?;
    assert_valid(&result_schema, &envelope(result_data("missing", 0, true)))?;
    Ok(())
}

#[test]
fn input_schema_rejects_unknown_null_invalid_point_and_postcondition()
-> Result<(), Box<dyn std::error::Error>> {
    let input_schema = schema(include_str!(
        "../contracts/v1/uix-pointer-click-sequence-transition-input.schema.json"
    ))?;
    let postcondition = json!({
        "selector": { "role": "button" },
        "condition": "unique"
    });
    for invalid in [
        json!({ "unexpected": true }),
        json!({
            "coordinateSpace": null,
            "clicks": [{ "x": 1, "y": 2 }],
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "screen-physical-px",
            "clicks": [{ "x": 1, "y": 2 }],
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "clicks": [],
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "clicks": [{ "x": 65_535.1, "y": 2 }],
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "clicks": [{ "x": 1, "y": null }],
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "clicks": [{ "x": 1, "y": 2, "button": "right" }],
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "clicks": [{ "x": 1, "y": 2 }, { "x": 3, "y": 4 }],
            "intervalMs": 100.5,
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "clicks": [{ "x": 1, "y": 2 }],
            "timeoutMs": null,
            "postcondition": postcondition.clone()
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "clicks": [{ "x": 1, "y": 2 }],
            "postcondition": { "selector": {}, "condition": "unique" }
        }),
        json!({
            "coordinateSpace": "client-logical-px",
            "clicks": [{ "x": 1, "y": 2 }],
            "postcondition": { "selector": { "role": "button" }, "condition": "stable" }
        }),
    ] {
        assert!(
            jsonschema::draft202012::validate(&input_schema, &invalid).is_err(),
            "非法 pointer click sequence transition 输入不应通过: {invalid}"
        );
    }
    Ok(())
}

#[test]
fn confirmation_source_order_is_checked_when_transition_module_exists() {
    let Some(module) = source("src/modules/linux_uix_pointer_click_sequence_transition.rs") else {
        eprintln!("跳过：pointer click sequence transition Module 尚未出现");
        return;
    };
    let Some(confirmation) = module.find("if !confirmed") else {
        panic!("Module 必须先检查 confirmation");
    };
    let Some(input) = module.find("UixPointerClickSequenceTransitionInput::parse") else {
        panic!("Module 必须解析 pointer click sequence transition 输入");
    };
    let Some(provider) = module.find(".perform(session_id") else {
        panic!("Module 必须通过 provider port 执行点击序列");
    };
    assert!(confirmation < input && input < provider);
}

#[test]
fn partial_ambiguity_and_unknown_results_prohibit_automatic_retry_when_implemented() {
    let Some(adapter) = source("src/adapters/linux/uix_agent_pointer_click_sequence_transition.rs")
    else {
        eprintln!("跳过：pointer click sequence transition Adapter 尚未出现");
        return;
    };
    for marker in [
        "accepted_may_have_occurred",
        "sequence_completed",
        "Postcondition(ElementWaitFailure)",
    ] {
        assert!(
            adapter.contains(marker),
            "Adapter 缺少失败边界标记: {marker}"
        );
    }

    let Some(module) = source("src/modules/linux_uix_pointer_click_sequence_transition.rs") else {
        eprintln!("跳过：pointer click sequence transition Module 尚未出现");
        return;
    };
    for marker in [
        "AMBIGUOUS_TARGET",
        "OUTCOME_UNKNOWN",
        "automaticRetryProhibited",
        "retrySafe",
        "accepted_may_have_occurred",
        "ElementWaitFailure::Ambiguous",
    ] {
        assert!(module.contains(marker), "Module 缺少失败边界标记: {marker}");
    }
}

#[test]
fn result_security_contract_forbids_foreground_desktop_and_extra_pointer_semantics() {
    let schema =
        include_str!("../contracts/v1/uix-pointer-click-sequence-transition-result.schema.json");
    let adapter = source("src/adapters/linux/uix_agent_pointer_click_sequence_transition.rs");
    let module = source("src/modules/linux_uix_pointer_click_sequence_transition.rs");
    for implementation in [Some(schema), adapter.as_deref(), module.as_deref()] {
        let Some(implementation) = implementation else {
            continue;
        };
        // 测试夹具可使用进程连接构造，但生产路径不得发布宿主或传输身份。
        let production = implementation
            .split_once("#[cfg(test)]")
            .map_or(implementation, |(production, _)| production);
        for forbidden in ["xdotool", "XTest", "Xlib", "xcb_"] {
            assert!(!production.contains(forbidden));
        }
        assert!(!production.contains("process_id"));
        assert!(!production.contains("descriptor.token"));
    }
    for marker in [
        "\"applicationConsumptionConfirmed\": { \"const\": false }",
        "\"causalityConfirmed\": { \"const\": false }",
        "\"finalStateReached\": { \"const\": false }",
        "\"desktopInputInjected\": { \"const\": false }",
        "\"desktopPointerMoved\": { \"const\": false }",
        "\"independentButtonOwnership\": { \"const\": false }",
        "\"doubleClickSupported\": { \"const\": false }",
        "\"clickCountSupported\": { \"const\": false }",
        "\"dragSupported\": { \"const\": false }",
        "\"scrollSupported\": { \"const\": false }",
        "\"foregroundActivationRequested\": { \"const\": false }",
        "\"fallback\": { \"const\": \"none\" }",
    ] {
        assert!(
            schema.contains(marker),
            "结果 schema 缺少安全冻结字段: {marker}"
        );
    }
}

#[test]
fn unconfirmed_cli_fails_before_target_input_or_provider_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.pointer.click.sequence.transition@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("未确认 pointer click sequence transition 必须优先失败"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}
