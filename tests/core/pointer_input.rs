#![cfg(target_os = "windows")]

// 导入统一服务与请求领域类型。
use ai_computer_toolkit::{
    // 导入统一服务。
    AppControlService,
    // 导入请求字段类型。
    domain::{CommandRequest, IsolationRequirement, Verb},
};
// 导入 JSON 构造与值类型。
use serde_json::{Value, json};

// 构造不会命中真实窗口的统一指针请求。
fn pointer_request() -> CommandRequest {
    // 构造 app.apply 请求。
    let mut request = CommandRequest::read(Verb::Run, "app");
    // 固定统一 apply operation。
    request.operation = Some("apply".to_owned());
    // 提供 canonical 但不存在的精确窗口目标。
    request
        // 访问 target 对象。
        .target
        // 插入稳定 sessionId。
        .insert("sessionId".to_owned(), json!("s2:w:0000000000000000"));
    // 指定版本化指针 capability。
    request
        // 访问 args 对象。
        .args
        // 插入 capability。
        .insert("capability".to_owned(), json!("ui.input.pointer@1"));
    // 提供合法旧兼容输入以隔离策略优先级。
    request
        // 访问 args 对象。
        .args
        // 插入 input。
        .insert("input".to_owned(), json!({ "x": 10, "y": 20 }));
    // 显式允许前景影响。
    request.foreground_consent = true;
    // 返回请求。
    request
}

// 验证逐操作确认在任何实时目标解析前失败。
#[test]
fn pointer_confirmation_is_enforced_before_stale_target_resolution() {
    // 保持未确认请求。
    let request = pointer_request();
    // 执行统一 System 路由并显式区分结果。
    let error = match AppControlService::new().execute(request) {
        // 成功表示确认门禁失效。
        Ok(_) => panic!("pointer confirmation must precede target resolution"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 禁止先返回 stale 或 provider 错误。
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

// 验证 strict 零打扰在前台 provider 解析前失败。
#[test]
fn strict_pointer_request_is_rejected_before_provider_resolution() {
    // 构造标准请求。
    let mut request = pointer_request();
    // 提供逐操作确认。
    request.confirmed = true;
    // 要求严格零打扰。
    request.isolation_requirement = IsolationRequirement::Strict;
    // 执行统一 System 路由并显式区分结果。
    let error = match AppControlService::new().execute(request) {
        // 成功表示严格隔离门禁失效。
        Ok(_) => panic!("strict pointer request must not reach a foreground provider"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对稳定隔离错误。
    assert_eq!(error.code, "ISOLATION_REQUIRED");
}

// 验证输入与成功 schema 冻结通用原语、所有权和物理坐标语义。
#[test]
fn schemas_freeze_pointer_primitives_ownership_and_result_safety()
-> Result<(), Box<dyn std::error::Error>> {
    // 解析版本化输入 schema。
    let input: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../../contracts/v1/pointer-input.schema.json"
    ))?;
    // 解析版本化成功结果 schema。
    let result: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../../contracts/v1/pointer-input-result.schema.json"
    ))?;
    // 正式序列必须拒绝未知字段。
    assert_eq!(input["$defs"]["sequence"]["additionalProperties"], false);
    // 动作序列上限必须与 Rust 常量一致。
    assert_eq!(
        input["$defs"]["sequence"]["properties"]["steps"]["maxItems"],
        64
    );
    // 两种坐标空间保持封闭。
    assert_eq!(
        input["$defs"]["sequence"]["properties"]["coordinateSpace"]["enum"],
        json!(["screen-physical-px", "window-client-physical-px"])
    );
    // 五种通用步骤保持封闭。
    assert_eq!(
        input["$defs"]["step"]["oneOf"].as_array().map(Vec::len),
        Some(5)
    );
    // 三类按钮保持封闭。
    assert_eq!(
        input["$defs"]["button"]["enum"],
        json!(["left", "right", "middle"])
    );
    // 旧单击形状仍拒绝 native 扩展字段。
    assert_eq!(input["$defs"]["legacyClick"]["additionalProperties"], false);
    // 成功结果必须声明请求内平衡所有权。
    assert_eq!(
        result["properties"]["data"]["properties"]["buttonOwnership"]["const"],
        "request-scoped-balanced"
    );
    // 成功结果不得遗留工具持有按钮。
    assert_eq!(
        result["properties"]["data"]["properties"]["buttonsHeldByTool"]["maxItems"],
        0
    );
    // 成功结果仍禁止自动重放。
    assert_eq!(
        result["properties"]["data"]["properties"]["automaticRetryProhibited"]["const"],
        true
    );
    // 前景 capability 的 mapper 允许真实前景变化证据。
    assert_eq!(
        result["properties"]["meta"]["properties"]["foreground"]["properties"]["unchanged"]["type"],
        "boolean"
    );
    // 完成 schema 测试。
    Ok(())
}
