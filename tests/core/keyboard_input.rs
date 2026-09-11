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

// 构造不会命中真实窗口的统一键盘请求。
fn keyboard_request() -> CommandRequest {
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
    // 指定版本化键盘 capability。
    request
        // 访问 args 对象。
        .args
        // 插入 capability。
        .insert("capability".to_owned(), json!("ui.input.key@1"));
    // 提供合法旧兼容输入以隔离策略优先级。
    request
        // 访问 args 对象。
        .args
        // 插入 input。
        .insert("input".to_owned(), json!({ "key": "ENTER" }));
    // 显式允许前景影响。
    request.foreground_consent = true;
    // 返回请求。
    request
}

// 验证逐操作确认在任何实时目标解析前失败。
#[test]
fn keyboard_confirmation_is_enforced_before_stale_target_resolution() {
    // 保持未确认请求。
    let request = keyboard_request();
    // 执行统一 System 路由并显式区分结果。
    let error = match AppControlService::new().execute(request) {
        // 成功表示确认门禁失效。
        Ok(_) => panic!("keyboard confirmation must precede target resolution"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 禁止先返回 stale 或 provider 错误。
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

// 验证 strict 零打扰在前台 provider 解析前失败。
#[test]
fn strict_keyboard_request_is_rejected_before_provider_resolution() {
    // 构造标准请求。
    let mut request = keyboard_request();
    // 提供逐操作确认。
    request.confirmed = true;
    // 要求严格零打扰。
    request.isolation_requirement = IsolationRequirement::Strict;
    // 执行统一 System 路由并显式区分结果。
    let error = match AppControlService::new().execute(request) {
        // 成功表示严格隔离门禁失效。
        Ok(_) => panic!("strict keyboard request must not reach a foreground provider"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对稳定隔离错误。
    assert_eq!(error.code, "ISOLATION_REQUIRED");
}

// 验证统一精确目标门禁在 provider dispatch 前失败闭合。
#[test]
fn keyboard_exact_target_gate_precedes_provider_dispatch() {
    // 构造标准请求。
    let mut request = keyboard_request();
    // 提供逐操作确认。
    request.confirmed = true;
    // 替换为含未知键的正式序列以证明不会越过目标门禁调度。
    request.args.insert(
        // 写入 input 字段。
        "input".to_owned(),
        // provider 输入不会在不存在目标上执行。
        json!({ "steps": [{ "type": "key", "key": "vk-255", "phase": "down" }] }),
    );
    // 执行统一 System 路由并显式区分结果。
    let error = match AppControlService::new().execute(request) {
        // 成功表示精确目标门禁失效。
        Ok(_) => panic!("missing keyboard target must fail closed"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 统一 System 必须在任何 provider dispatch 前拒绝不存在目标。
    assert_eq!(error.code, "TARGET_NOT_FOUND");
}

// 验证输入与成功 schema 冻结完整键集、所有权和结果安全。
#[test]
fn schemas_freeze_keyboard_primitives_ownership_and_result_safety()
-> Result<(), Box<dyn std::error::Error>> {
    // 解析版本化输入 schema。
    let input: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../../contracts/v1/key-input.schema.json"
    ))?;
    // 解析版本化成功结果 schema。
    let result: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../../contracts/v1/key-input-result.schema.json"
    ))?;
    // 正式序列必须拒绝未知字段。
    assert_eq!(input["$defs"]["sequence"]["additionalProperties"], false);
    // 动作序列上限必须与 Rust 常量一致。
    assert_eq!(
        input["$defs"]["sequence"]["properties"]["steps"]["maxItems"],
        128
    );
    // 三种通用步骤保持封闭。
    assert_eq!(
        input["$defs"]["step"]["oneOf"].as_array().map(Vec::len),
        Some(3)
    );
    // 快捷键按键数量保持有界。
    assert_eq!(
        input["$defs"]["chordStep"]["properties"]["keys"]["maxItems"],
        8
    );
    // 旧形状只允许安全 press。
    assert_eq!(
        input["$defs"]["legacyKey"]["properties"]["phase"]["const"],
        "press"
    );
    // 成功结果必须声明请求内平衡所有权。
    assert_eq!(
        result["properties"]["data"]["properties"]["keyOwnership"]["const"],
        "request-scoped-balanced"
    );
    // 成功结果不得遗留工具持有按键。
    assert_eq!(
        result["properties"]["data"]["properties"]["keysHeldByTool"]["maxItems"],
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

// 验证迁移清单只认证 Rust 生产路由且禁止跨请求按键状态。
#[test]
fn migration_policy_freezes_rust_route_and_release_lifecycle()
-> Result<(), Box<dyn std::error::Error>> {
    // 解析仓库内版本化迁移清单。
    let policy: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../contracts/foreground-input-migration-policy.json"
    ))?;
    // 统一 capability 必须直接走 Rust app.apply。
    assert_eq!(policy["productionRoute"], "app.apply");
    // 生产状态必须是已确认可用。
    assert_eq!(policy["rustExecutionStatus"], "available-confirmed");
    // 不得保留 C++ 生产路由。
    assert_eq!(policy["cppExecutionStatus"], "retired-no-route");
    // 跨请求 down/up 必须关闭。
    assert_eq!(policy["lifecycle"]["downUpAcrossRequestsAllowed"], false);
    // 取消和 timeout 不得阻止释放。
    assert_eq!(
        policy["lifecycle"]["releaseBypassedByCancelTimeoutOrForegroundChange"],
        false
    );
    // 人类视觉交互门禁仍由 owner 独占。
    assert_eq!(policy["formalGate"], "owner-only-gc-va-001");
    // 完成迁移清单测试。
    Ok(())
}
