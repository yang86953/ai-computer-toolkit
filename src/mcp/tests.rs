//! MCP 控制面的协议与清单回归。
//!
//! 这些测试不连接桌面：它们只验证工具清单是封闭的、协议状态机按契约拒绝，
//! 以及未连接时的调用失败闭合。实机截图与输入验收不在此列。

use serde_json::{Value, json};

use super::server::Session;
use super::tools::{input_schema, is_known_tool, tool_catalog};
use crate::components::desktop_interaction::MAXIMUM_INTERACTION_STEPS;

/// 取出响应中的 `error.code`。
fn error_code(response: &Value) -> Option<i64> {
    response.get("error")?.get("code")?.as_i64()
}

/// 构造一次带 id 的请求。
fn request(id: i64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

/// 表单内必须包含的九个工具，顺序即公开顺序。
const EXPECTED_TOOLS: [&str; 9] = [
    "computer_connect",
    "computer_status",
    "computer_observe",
    "computer_interact",
    "computer_keys",
    "computer_pointer",
    "computer_run",
    "computer_disconnect",
    "computer_authorization",
];

#[test]
fn catalog_is_closed_and_ordered() {
    let names: Vec<String> = tool_catalog()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(names, EXPECTED_TOOLS, "tool catalog must stay closed");
    for name in EXPECTED_TOOLS {
        assert!(is_known_tool(name), "{name} must be addressable");
        assert!(input_schema(name).is_some(), "{name} must publish a schema");
        assert!(
            !is_known_tool(&format!("{name}_extra")),
            "catalog must reject variants"
        );
    }
}

#[test]
fn every_tool_schema_is_a_closed_object() {
    for tool in tool_catalog() {
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object", "tool schema must be an object");
        assert_eq!(
            schema["additionalProperties"], false,
            "tool schema must reject undeclared fields"
        );
        let properties = schema["properties"].as_object().expect("properties");
        for required in schema["required"].as_array().expect("required") {
            let key = required.as_str().expect("required entry");
            assert!(
                properties.contains_key(key),
                "{key} is required but not declared"
            );
        }
    }
}

/// 长流程工具与单批工具共用同一输入契约边界；MCP 不得比 broker 更严或更松。
#[test]
fn run_batches_share_the_interaction_input_contract() {
    let run = input_schema("computer_run").expect("schema");
    let batches = &run["properties"]["batches"];
    assert_eq!(batches["minItems"], 1);
    assert_eq!(batches["maxItems"], 64);
    let step = &batches["items"]["properties"]["steps"];
    assert_eq!(step["minItems"], 1);
    // 直接对齐 broker 的常量：写死数字正是「MCP 64 / broker 128」那次漂移的成因。
    assert_eq!(
        step["maxItems"],
        MAXIMUM_INTERACTION_STEPS,
        "每批不得超出 broker 的步数契约"
    );
    // 单批工具与批次内单批必须给出同一个上限，否则同一批输入会在两条路上表现不同。
    let interact = input_schema("computer_interact").expect("schema");
    assert_eq!(
        interact["properties"]["steps"]["maxItems"],
        MAXIMUM_INTERACTION_STEPS
    );
}

#[test]
fn run_requires_session_and_batches_but_not_consent_fields() {
    let schema = input_schema("computer_run").expect("schema");
    let required = schema["required"].as_array().expect("required");
    for key in ["sessionId", "batches"] {
        assert!(
            required.iter().any(|entry| entry == key),
            "computer_run must require {key}"
        );
    }
    // 确认字段改为声明可选：session 授权会话可省略，显式传值仍被校验。
    for key in ["confirmed", "foregroundConsent", "strictIsolation"] {
        assert!(
            !required.iter().any(|entry| entry == key),
            "computer_run must not hard-require {key}"
        );
        assert!(
            schema["properties"]
                .as_object()
                .expect("properties")
                .contains_key(key),
            "{key} stays declared for explicit confirmation"
        );
    }
    // 长流程不接收调用方帧：帧必须由服务端在每批前自己补。
    assert!(
        !schema["properties"]
            .as_object()
            .expect("properties")
            .contains_key("frameId"),
        "computer_run must not accept a caller-supplied frameId"
    );
}

#[test]
fn input_tools_publish_optional_foreground_authorization() {
    // connect 仍要求一次性显式确认；其余工具声明字段但不再强制。
    let connect = input_schema("computer_connect").expect("schema");
    for key in ["confirmed", "foregroundConsent", "strictIsolation"] {
        assert!(
            connect["required"]
                .as_array()
                .expect("required")
                .iter()
                .any(|entry| entry == key),
            "computer_connect must require {key}"
        );
    }
    assert_eq!(
        connect["properties"]["authorizationMode"]["enum"],
        json!(["operation", "session"]),
        "connect must publish the authorization mode enum"
    );
    assert!(
        connect["properties"]
            .as_object()
            .expect("properties")
            .contains_key("rememberAuthorization"),
        "connect must publish rememberAuthorization"
    );
    for name in [
        "computer_observe",
        "computer_interact",
        "computer_keys",
        "computer_pointer",
        "computer_run",
    ] {
        let schema = input_schema(name).expect("schema");
        let required = schema["required"].as_array().expect("required");
        for key in ["confirmed", "strictIsolation"] {
            assert!(
                !required.iter().any(|entry| entry == key),
                "{name} must allow omitting {key} in session mode"
            );
            assert!(
                schema["properties"]
                    .as_object()
                    .expect("properties")
                    .contains_key(key),
                "{name} keeps {key} declared for explicit calls"
            );
        }
    }
}

#[test]
fn authorization_basis_matrix_matches_broker_inheritance_rules() {
    use super::desktop::authorization_basis_probe as basis;
    fn ok(result: Result<&'static str, &'static str>) -> &'static str {
        match result {
            Ok(value) => value,
            Err(code) => panic!("expected basis, got {code}"),
        }
    }
    // 全显式放行；connect 之外的部分显式按 session 会话继承。
    assert_eq!(
        ok(basis(
            Some(true),
            Some(true),
            Some(false),
            true,
            false,
            true
        )),
        "explicit"
    );
    assert_eq!(
        ok(basis(Some(true), None, Some(false), true, true, true)),
        "inherited"
    );
    assert_eq!(ok(basis(None, None, None, false, true, true)), "inherited");
    // 显式拒绝优先闭合，session 会话也不能覆盖。
    for (confirmed, foreground, strict, write) in [
        (Some(false), Some(true), Some(false), true),
        (Some(true), Some(true), Some(true), true),
        (Some(true), Some(false), Some(false), true),
    ] {
        let Err(code) = basis(confirmed, foreground, strict, write, true, true) else {
            panic!("explicit refusal must be rejected even in session mode");
        };
        assert_eq!(code, "CONSENT_REQUIRED");
    }
    // 省略字段在未连接或非 session 会话下保持显式要求。
    for (scoped, connected) in [(false, true), (true, false), (false, false)] {
        let Err(code) = basis(None, None, None, true, scoped, connected) else {
            panic!("omitted fields must not pass without a session-scoped live broker");
        };
        assert_eq!(code, "CONSENT_REQUIRED");
    }
}

#[test]
fn calls_are_rejected_before_initialize() {
    let mut session = Session::new();
    let responses = session.handle(&request(1, "tools/list", json!({})));
    assert_eq!(error_code(&responses[0]), Some(-32002));
    session.dispose();
}

#[test]
fn initialize_is_single_shot_and_negotiates_version() {
    let mut session = Session::new();
    let first = session.handle(&request(
        1,
        "initialize",
        json!({ "protocolVersion": "2024-11-05" }),
    ));
    assert_eq!(first[0]["result"]["protocolVersion"], "2024-11-05");
    let second = session.handle(&request(2, "initialize", json!({})));
    assert_eq!(error_code(&second[0]), Some(-32600));
    session.dispose();
}

#[test]
fn unknown_protocol_version_falls_back_to_latest_supported() {
    let mut session = Session::new();
    let response = session.handle(&request(
        1,
        "initialize",
        json!({ "protocolVersion": "1999-01-01" }),
    ));
    assert_eq!(response[0]["result"]["protocolVersion"], "2025-06-18");
    session.dispose();
}

#[test]
fn tools_list_is_withheld_until_initialized_notification() {
    let mut session = Session::new();
    session.handle(&request(1, "initialize", json!({})));
    let before = session.handle(&request(2, "tools/list", json!({})));
    assert_eq!(
        error_code(&before[0]),
        Some(-32002),
        "tools require the initialized notification"
    );
    session.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let after = session.handle(&request(3, "tools/list", json!({})));
    let tools = after[0]["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), EXPECTED_TOOLS.len());
    session.dispose();
}

#[test]
fn unknown_method_and_malformed_requests_fail_closed() {
    let mut session = Session::new();
    session.handle(&request(1, "initialize", json!({})));
    session.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let unknown = session.handle(&request(4, "tools/execute", json!({})));
    assert_eq!(error_code(&unknown[0]), Some(-32601));
    let bad_version = session.handle(&json!({ "jsonrpc": "1.0", "id": 5, "method": "ping" }));
    assert_eq!(error_code(&bad_version[0]), Some(-32600));
    let bad_params = session.handle(&request(6, "tools/list", json!([])));
    assert_eq!(error_code(&bad_params[0]), Some(-32602));
    session.dispose();
}

#[test]
fn notifications_never_produce_responses() {
    let mut session = Session::new();
    let responses = session.handle(&json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": { "requestId": 99 }
    }));
    assert!(responses.is_empty(), "notifications must not be answered");
    session.dispose();
}

#[test]
fn cancellation_does_not_apply_to_a_future_request() {
    let mut session = Session::new();
    session.handle(&request(1, "initialize", json!({})));
    session.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    session.handle(&json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": { "requestId": 7 }
    }));
    let responses = session.handle(&request(
        7,
        "tools/call",
        json!({
            "name": "computer_connect",
            "arguments": {}
        }),
    ));
    assert_eq!(error_code(&responses[0]), None);
    assert!(responses[0]["result"]["isError"].as_bool().unwrap());
    session.dispose();
}

#[test]
fn unknown_tool_is_a_tool_error_not_a_protocol_error() {
    let mut session = Session::new();
    session.handle(&request(1, "initialize", json!({})));
    session.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let responses = session.handle(&request(
        8,
        "tools/call",
        json!({
            "name": "computer_execute",
            "arguments": {}
        }),
    ));
    assert!(
        error_code(&responses[0]).is_none(),
        "tool errors stay in the result envelope"
    );
    assert_eq!(responses[0]["result"]["isError"], true);
    session.dispose();
}

#[test]
fn connect_without_authorization_is_refused_before_any_broker() {
    for arguments in [
        json!({}),
        json!({ "confirmed": true }),
        json!({ "confirmed": true, "foregroundConsent": true }),
        // strictIsolation=true 表示要求后台隔离，此路线必须拒绝而不是降级。
        json!({ "confirmed": true, "foregroundConsent": true, "strictIsolation": true }),
        // session 模式不是隐式授权：connect 仍要求完整显式确认。
        json!({ "authorizationMode": "session" }),
    ] {
        let mut session = Session::new();
        session.handle(&request(1, "initialize", json!({})));
        session.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        let responses = session.handle(&request(
            9,
            "tools/call",
            json!({
                "name": "computer_connect",
                "arguments": arguments
            }),
        ));
        assert_eq!(
            responses[0]["result"]["isError"], true,
            "must refuse {arguments}"
        );
        session.dispose();
    }
}

#[test]
fn authorization_tool_rejects_unknown_actions_before_any_broker() {
    let mut session = Session::new();
    session.handle(&request(1, "initialize", json!({})));
    session.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    for arguments in [
        json!({}),
        json!({ "action": "revoke" }),
        json!({ "action": 7 }),
    ] {
        let responses = session.handle(&request(
            12,
            "tools/call",
            json!({
                "name": "computer_authorization",
                "arguments": arguments
            }),
        ));
        assert_eq!(
            responses[0]["result"]["isError"], true,
            "must refuse {arguments}"
        );
        let text = responses[0]["result"]["content"][0]["text"]
            .as_str()
            .expect("text payload");
        assert!(
            text.contains("INVALID_ARGUMENT"),
            "unknown action must fail closed, got {text}"
        );
    }
    session.dispose();
}

#[test]
fn session_bound_tools_require_the_returned_session_id() {
    let mut session = Session::new();
    session.handle(&request(1, "initialize", json!({})));
    session.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    for (name, arguments) in [
        (
            "computer_observe",
            json!({ "sessionId": "s2:i:0000000000000000", "confirmed": true, "strictIsolation": false }),
        ),
        (
            "computer_disconnect",
            json!({ "sessionId": "s2:i:0000000000000000" }),
        ),
    ] {
        let responses = session.handle(&request(
            10,
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        ));
        assert_eq!(
            responses[0]["result"]["isError"], true,
            "{name} must reject a foreign session"
        );
        let text = responses[0]["result"]["content"][0]["text"]
            .as_str()
            .expect("text payload");
        assert!(
            text.contains("STALE_SESSION"),
            "{name} must report STALE_SESSION, got {text}"
        );
    }
    session.dispose();
}

#[test]
fn status_without_connection_reports_no_sessions() {
    let mut session = Session::new();
    session.handle(&request(1, "initialize", json!({})));
    session.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    let responses = session.handle(&request(
        11,
        "tools/call",
        json!({
            "name": "computer_status",
            "arguments": {}
        }),
    ));
    let text = responses[0]["result"]["content"][0]["text"]
        .as_str()
        .expect("text payload");
    assert!(
        text.contains("sessions"),
        "status must report the session list"
    );
    assert_eq!(responses[0]["result"]["isError"], false);
    session.dispose();
}
