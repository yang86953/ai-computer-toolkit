#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX 窗口生命周期序列 transition 的公开契约、双前置门禁与安全边界回归。

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

fn action_sequence() -> Value {
    json!([
        { "type": "restore" },
        {
            "type": "resize",
            "coordinateSpace": "client-logical-px",
            "width": 800,
            "height": 600
        },
        { "type": "maximize" }
    ])
}

fn condition() -> Value {
    json!({ "type": "window-flags", "maximized": true })
}

fn safety() -> Value {
    json!({
        "provider": "uix-agent-v1",
        "sameAuthenticatedConnectionUsed": true,
        "windowGenerationFixed": true,
        "revisionChainUsed": true,
        "providerPolling": true,
        "waitProtocolUsed": false,
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "arbitraryWindowMoveSupported": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "x11Used": false,
        "fallback": "none"
    })
}

fn envelope(data: Value) -> Value {
    json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "window.lifecycle.sequence.transition@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "host-foreground",
        "requiredExecutionRealm": "host-foreground",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": data,
        "meta": {
            "foreground": {
                "activationRequested": false,
                "visibleMutationAuthorized": true,
                "frameworkConditionObservationRequested": true
            },
            "targeting": "opaque exact window generation"
        }
    })
}

fn result_data(sequence_settled: bool, poll_count: u64) -> Value {
    json!({
        "capability": "window.lifecycle.sequence.transition@1",
        "targetId": "s2:w:0000000000000000",
        "action": "lifecycle-sequence",
        "actions": action_sequence(),
        "actionsRequested": 3,
        "actionsAccepted": 3,
        "distinctActionKinds": 3,
        "intervalMs": 20,
        "plannedDurationMs": 40,
        "sequenceRevision": 8,
        "sequencePresentedRevision": 8,
        "sequenceSettled": sequence_settled,
        "condition": condition(),
        "observationSource": "uix-framework-current",
        "coordinateSpace": "client-logical-px",
        "visible": true,
        "presentable": true,
        "focused": false,
        "clientSize": { "width": 800, "height": 600 },
        "windowState": {
            "maximized": true,
            "minimized": false,
            "fullscreen": false
        },
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "frameworkConditionMatchedAfterCompleteSequence": true,
        "revision": 8,
        "presentedRevision": 8,
        "pollCount": poll_count,
        "sameAuthenticatedConnectionUsed": true,
        "sameConnectionRevisionChain": true,
        "providerPolling": true,
        "waitProtocolUsed": false,
        "transactionSemantics": false,
        "rollbackSemantics": false,
        "effectConfirmed": false,
        "finalStateReached": false,
        "causalityConfirmed": false,
        "compositorFinalStateConfirmed": false,
        "windowReResolved": true,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "foregroundConsentEvaluatedBeforeDiscovery": true,
        "foregroundConsentRequired": true,
        "foregroundImpactAuthorized": true,
        "hostForegroundActivationRequested": false,
        "effectObservation": "framework-condition-after-complete-sequence-no-causal-claim",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "pollIntervalMs": 20,
        "timeoutMs": 1000,
        "executionRealm": "host-foreground",
        "safety": safety()
    })
}

#[test]
fn schemas_accept_complete_sequence_and_unsettled_framework_observation()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "actions": action_sequence(),
        "condition": condition(),
        "intervalMs": 20,
        "pollIntervalMs": 20,
        "timeoutMs": 1000
    });
    let input_schema = schema(include_str!(
        "../contracts/v1/window-lifecycle-sequence-transition-input.schema.json"
    ))?;
    assert_valid(&input_schema, &input)?;

    let result_schema = schema(include_str!(
        "../contracts/v1/window-lifecycle-sequence-transition.schema.json"
    ))?;
    assert_valid(&result_schema, &envelope(result_data(false, 1)))?;
    assert_valid(&result_schema, &envelope(result_data(true, 2)))?;
    Ok(())
}

#[test]
fn input_schema_rejects_unknown_null_invalid_action_condition_and_timing()
-> Result<(), Box<dyn std::error::Error>> {
    let input_schema = schema(include_str!(
        "../contracts/v1/window-lifecycle-sequence-transition-input.schema.json"
    ))?;
    for invalid in [
        json!({ "unexpected": true }),
        json!({
            "actions": action_sequence(),
            "condition": null
        }),
        json!({
            "actions": action_sequence(),
            "condition": condition(),
            "pollIntervalMs": null
        }),
        json!({
            "actions": [
                { "type": "restore" },
                { "type": "move", "x": 1, "y": 2 }
            ],
            "condition": condition()
        }),
        json!({
            "actions": [{ "type": "restore" }],
            "condition": condition()
        }),
        json!({
            "actions": [
                { "type": "restore" },
                { "type": "resize", "coordinateSpace": "client-logical-px", "width": 0, "height": 600 }
            ],
            "condition": condition()
        }),
        json!({
            "actions": [
                { "type": "restore" },
                { "type": "resize", "coordinateSpace": "screen-physical-px", "width": 800, "height": 600 }
            ],
            "condition": condition()
        }),
        json!({
            "actions": action_sequence(),
            "condition": { "type": "visibility" }
        }),
        json!({
            "actions": action_sequence(),
            "condition": { "type": "focus", "focused": null }
        }),
        json!({
            "actions": action_sequence(),
            "condition": { "type": "window-flags", "extra": true }
        }),
        json!({
            "actions": action_sequence(),
            "condition": condition(),
            "pollIntervalMs": 19
        }),
        json!({
            "actions": action_sequence(),
            "condition": condition(),
            "pollIntervalMs": 501
        }),
        json!({
            "actions": action_sequence(),
            "condition": condition(),
            "intervalMs": 100.5
        }),
        json!({
            "actions": action_sequence(),
            "condition": condition(),
            "timeoutMs": 100.5
        }),
    ] {
        assert!(
            jsonschema::draft202012::validate(&input_schema, &invalid).is_err(),
            "非法 lifecycle sequence transition 输入不应通过: {invalid}"
        );
    }
    Ok(())
}

#[test]
fn confirmation_and_foreground_source_order_is_checked_when_module_exists() {
    let Some(module) = source("src/modules/linux_uix_window_lifecycle_sequence_transition.rs")
    else {
        eprintln!("跳过：lifecycle sequence transition Module 尚未出现");
        return;
    };
    let Some(confirmation) = module.find("if !confirmed") else {
        panic!("Module 必须先检查 confirmation");
    };
    let Some(foreground) = module.find("if !foreground_consent") else {
        panic!("Module 必须先检查 foreground consent");
    };
    let Some(input) = module.find("UixWindowLifecycleSequenceTransitionInput::parse") else {
        panic!("Module 必须解析 lifecycle sequence transition 输入");
    };
    let Some(provider) = module.find(".perform(session_id") else {
        panic!("Module 必须通过 provider port 执行动作序列");
    };
    assert!(confirmation < foreground && foreground < input && input < provider);

    let Some(adapter) =
        source("src/adapters/linux/uix_agent_window_lifecycle_sequence_transition.rs")
    else {
        eprintln!("跳过 Adapter 顺序断言：lifecycle sequence transition Adapter 尚未出现");
        return;
    };
    let Some(dispatch) = adapter.find("perform_targetless_with_request_id") else {
        panic!("Adapter 必须在首个 dispatch 前完成动作预检并发送序列");
    };
    let Some(poll) = adapter.find("list_windows") else {
        panic!("Adapter 必须在完整序列后使用 list_windows 观察条件");
    };
    assert!(dispatch < poll);
}

#[test]
fn partial_and_unknown_results_prohibit_automatic_retry_when_implemented() {
    let Some(adapter) =
        source("src/adapters/linux/uix_agent_window_lifecycle_sequence_transition.rs")
    else {
        eprintln!("跳过：lifecycle sequence transition Adapter 尚未出现");
        return;
    };
    for marker in ["accepted_may_have_occurred", "actions_accepted"] {
        assert!(
            adapter.contains(marker),
            "Adapter 缺少不确定性字段: {marker}"
        );
    }
    let Some(module) = source("src/modules/linux_uix_window_lifecycle_sequence_transition.rs")
    else {
        eprintln!("跳过：lifecycle sequence transition Module 尚未出现");
        return;
    };
    for marker in ["OUTCOME_UNKNOWN", "automaticRetryProhibited", "retrySafe"] {
        assert!(module.contains(marker), "Module 缺少失败边界标记: {marker}");
    }
}

#[test]
fn result_and_implementation_forbid_compositor_desktop_and_identity_fallbacks() {
    let schema = include_str!("../contracts/v1/window-lifecycle-sequence-transition.schema.json");
    let adapter = source("src/adapters/linux/uix_agent_window_lifecycle_sequence_transition.rs");
    let module = source("src/modules/linux_uix_window_lifecycle_sequence_transition.rs");
    for implementation in [Some(schema), adapter.as_deref(), module.as_deref()] {
        let Some(implementation) = implementation else {
            continue;
        };
        // 测试夹具可以构造连接，但生产路径不能发布宿主/传输身份或桌面后门。
        let production = implementation
            .split_once("#[cfg(test)]")
            .map_or(implementation, |(production, _)| production);
        for forbidden in ["xdotool", "XTest", "Xlib", "xcb_", "org.kde.KWin"] {
            assert!(!production.contains(forbidden));
        }
        assert!(!production.contains("process_id"));
        assert!(!production.contains("descriptor.token"));
    }
    for marker in [
        "\"observationSource\": { \"const\": \"uix-framework-current\" }",
        "\"frameworkConditionMatchedAfterCompleteSequence\": { \"const\": true }",
        "\"transactionSemantics\": { \"const\": false }",
        "\"rollbackSemantics\": { \"const\": false }",
        "\"compositorFinalStateConfirmed\": { \"const\": false }",
        "\"desktopInputInjected\": { \"const\": false }",
        "\"desktopPointerMoved\": { \"const\": false }",
        "\"arbitraryWindowMoveSupported\": { \"const\": false }",
        "\"nativeIdentityExposed\": { \"const\": false }",
        "\"transportIdentityExposed\": { \"const\": false }",
        "\"x11Used\": { \"const\": false }",
        "\"fallback\": { \"const\": \"none\" }",
    ] {
        assert!(
            schema.contains(marker),
            "结果 schema 缺少安全冻结字段: {marker}"
        );
    }
}

#[test]
fn unconfirmed_cli_fails_before_input_target_or_provider_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "window.lifecycle.sequence.transition@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("未确认 lifecycle sequence transition 必须优先失败"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}
