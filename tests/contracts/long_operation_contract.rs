#![cfg(target_os = "windows")]

//! 验证长操作公开 schema 与内部 broker schema 的冻结字段。

// 导入 JSON 值类型。
use serde_json::Value;

// 解析公开状态 schema。
fn status_schema() -> serde_json::Result<Value> {
    // 读取编译期固定契约文件。
    serde_json::from_str(include_str!(
        // 使用公开 v1 状态 schema。
        "../../contracts/v1/long-operation-status.schema.json"
    ))
}

// 解析内部 broker frame schema。
fn broker_schema() -> serde_json::Result<Value> {
    // 读取编译期固定内部契约文件。
    serde_json::from_str(include_str!(
        // 使用内部 broker v1 schema。
        "../../contracts/internal/long-operation-broker-v1.schema.json"
    ))
}

// 验证公开状态只使用 opaque operation handle 与封闭状态集合。
#[test]
// 覆盖版本、额外字段、handle pattern 和六个状态。
fn public_status_schema_is_closed_and_versioned() -> serde_json::Result<()> {
    // 解析公开 schema。
    let schema = status_schema()?;
    // 顶层拒绝未知字段。
    assert_eq!(schema["additionalProperties"], false);
    // 固定公开契约版本。
    assert_eq!(
        schema["properties"]["contractVersion"]["const"],
        "act/long-operation/v1"
    );
    // 句柄只允许 canonical operation 类别。
    assert_eq!(
        schema["properties"]["operationId"]["pattern"],
        "^s2:o:[0-9a-f]{16}$"
    );
    // 冻结全部生命周期状态文本。
    assert_eq!(
        schema["properties"]["status"]["enum"],
        serde_json::json!([
            // broker 已接受。
            "accepted",
            // worker 已 dispatch。
            "running",
            // 已请求取消但非终态。
            "cancel-requested",
            // 已证明成功。
            "completed",
            // 已证明失败。
            "failed",
            // dispatch 后结果未知。
            "outcome-unknown"
        ])
    );
    // 已建立 handle 总是意味着业务接受可能发生。
    assert_eq!(
        schema["properties"]["acceptedMayHaveOccurred"]["const"],
        true
    );
    // 返回测试成功。
    Ok(())
}

// 验证终态约束不会把未知结果伪装成可重试或成功。
#[test]
// 覆盖 completed 与 outcome-unknown 的事实约束。
fn terminal_status_contract_preserves_outcome_truth() -> serde_json::Result<()> {
    // 解析公开 schema。
    let schema = status_schema()?;
    // 成功终态必须要求结果。
    assert!(
        schema["$defs"]["completed"]["required"]
            // 读取 required 数组。
            .as_array()
            // 检查 result 字段。
            .is_some_and(|fields| fields.contains(&Value::String("result".to_owned())))
    );
    // 成功终态必须发生在 dispatch 后。
    assert_eq!(
        schema["$defs"]["completed"]["properties"]["dispatchStarted"]["const"],
        true
    );
    // 未知终态不得自动重试。
    assert_eq!(
        schema["$defs"]["outcomeUnknown"]["properties"]["retrySafe"]["const"],
        false
    );
    // 未知终态使用独立稳定错误码。
    assert_eq!(
        schema["$defs"]["outcomeUnknown"]["properties"]["error"]["allOf"][1]["properties"]["code"]
            ["const"],
        "OUTCOME_UNKNOWN"
    );
    // 非终态不得伪造固定保留截止时间。
    assert_eq!(
        schema["$defs"]["nonTerminal"]["properties"]["expiresAt"]["const"],
        Value::Null
    );
    // completed 终态必须携带 RFC 3339 到期时间。
    assert_eq!(
        schema["$defs"]["completed"]["properties"]["expiresAt"]["format"],
        "date-time"
    );
    // 返回测试成功。
    Ok(())
}

// 验证固定 broker 在触碰 journal 前先取得首实例且不接受可注入 endpoint。
#[test]
fn broker_startup_owns_endpoint_before_recovery() {
    // 读取编译期固定 broker System 组合源码。
    let source = include_str!("../../src/long_operation_broker.rs");
    // 定位固定首实例创建调用。
    let endpoint = source
        // 搜索生产 endpoint 入口。
        .find("ServerPipe::create_for")
        // 源码漂移时给出明确测试失败。
        .unwrap_or_else(|| panic!("fixed broker endpoint creation is missing"));
    // 定位 Known Folder journal 打开调用。
    let journal = source[endpoint..]
        // 从启动创建点后搜索固定存储调用。
        .find("open_long_operation_journal")
        // 源码漂移时给出明确测试失败。
        .map(|offset| endpoint + offset)
        // 源码漂移时给出明确测试失败。
        .unwrap_or_else(|| panic!("broker journal recovery is missing after endpoint ownership"));
    // 第二 broker 必须先被内核首实例拒绝，不能并发恢复 live journal。
    assert!(endpoint < journal);
    // broker 只能选择封闭长操作 endpoint kind。
    assert!(source.contains("FixedLocalEndpointKind::LongOperation"));
    // 生产 broker 不得从参数解析 pipe、路径或命令行。
    assert!(!source.contains("std::env::args"));
}

// 验证 broker submit 只允许确认后的固定窗口录制。
#[test]
// 覆盖协议版本、nonce、capability、目标、输入和确认。
fn broker_submit_schema_is_fixed_and_confirmation_only() -> serde_json::Result<()> {
    // 解析内部 broker schema。
    let schema = broker_schema()?;
    // 借用 submit 定义。
    let submit = &schema["$defs"]["submitRequest"];
    // submit 拒绝任意 envelope 扩展。
    assert_eq!(submit["additionalProperties"], false);
    // 固定 broker 协议版本。
    assert_eq!(
        submit["properties"]["contractVersion"]["const"],
        "act/long-operation-broker/v1"
    );
    // 首个 operation capability 固定为窗口录制。
    assert_eq!(
        submit["properties"]["capabilityId"]["const"],
        "window.record@1"
    );
    // 只允许显式确认 true。
    assert_eq!(submit["properties"]["confirmed"]["const"], true);
    // 复用既有封闭窗口录制输入 schema。
    assert_eq!(
        submit["properties"]["input"]["$ref"],
        "../../v1/window-record-input.schema.json"
    );
    // 请求 nonce 固定为三十二位小写十六进制。
    assert_eq!(schema["$defs"]["requestNonce"]["pattern"], "^[0-9a-f]{32}$");
    // 返回测试成功。
    Ok(())
}

// 验证 status 与 cancel 都是 operation-handle-only frame。
#[test]
// 覆盖 action 常量、operation pattern 与封闭字段。
fn broker_queries_accept_only_operation_handles() -> serde_json::Result<()> {
    // 解析内部 broker schema。
    let schema = broker_schema()?;
    // 逐一核对 status Query 与 cancel Command。
    for (definition, action) in [("statusRequest", "status"), ("cancelRequest", "cancel")] {
        // 借用当前 action 定义。
        let request = &schema["$defs"][definition];
        // 拒绝 target、input、path 或 provider 扩展。
        assert_eq!(request["additionalProperties"], false);
        // 固定 action 文本。
        assert_eq!(request["properties"]["action"]["const"], action);
        // operationId 必须存在。
        assert!(
            request["required"]
                // 读取 required 数组。
                .as_array()
                // 检查 operationId 字段。
                .is_some_and(|fields| fields.contains(&Value::String("operationId".to_owned())))
        );
    }
    // operation handle pattern 不接受其他 opaque 类别。
    assert_eq!(
        schema["$defs"]["operationId"]["pattern"],
        "^s2:o:[0-9a-f]{16}$"
    );
    // 返回测试成功。
    Ok(())
}

// 验证 broker response 区分 transport、业务接受和任务终态。
#[test]
// 覆盖成功与两个接受前拒绝层级。
fn broker_responses_separate_acceptance_boundaries() -> serde_json::Result<()> {
    // 解析内部 broker schema。
    let schema = broker_schema()?;
    // 成功 frame 必须携带公开任务快照。
    assert_eq!(
        schema["$defs"]["successResponse"]["properties"]["operation"]["$ref"],
        "../../v1/long-operation-status.schema.json"
    );
    // 成功 frame 表示 transport 已接受。
    assert_eq!(
        schema["$defs"]["successResponse"]["properties"]["transportAccepted"]["const"],
        true
    );
    // 成功 frame 表示业务已建立 handle。
    assert_eq!(
        schema["$defs"]["successResponse"]["properties"]["businessAccepted"]["const"],
        true
    );
    // envelope 拒绝不得冒充 transport 接受。
    assert_eq!(
        schema["$defs"]["rejectionBeforeTransport"]["properties"]["transportAccepted"]["const"],
        false
    );
    // envelope 通过后的参数/容量拒绝仍不得冒充业务接受。
    assert_eq!(
        schema["$defs"]["rejectionBeforeBusinessAcceptance"]["properties"]["businessAccepted"]["const"],
        false
    );
    // 返回测试成功。
    Ok(())
}
