#![cfg(target_os = "windows")]

// 导入公开 System、严格输入和模板硬边界。
use ai_computer_toolkit::{
    // 导入公开 System 协调入口。
    AppControlService,
    // 导入 sequence 输入与模板常量。
    service::{
        MAX_SEQUENCE_BOUND_VALUE_BYTES, MAX_SEQUENCE_TEMPLATE_LITERAL_BYTES,
        MAX_SEQUENCE_TEMPLATE_SEGMENTS, MAX_SEQUENCE_TEMPLATE_SOURCES,
        MAX_SEQUENCE_TEMPLATE_STATIC_BYTES, SequenceInput,
    },
};
// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 构造不依赖真实窗口的只读 desktop 状态步骤。
fn status_step(name: &str) -> Value {
    // 返回严格 step schema 接受的最小输入。
    json!({
        // 保留调用方关联名称。
        "name": name,
        // 状态读取没有外部变更。
        "verb": "status",
        // desktop 状态由确定性 Rust Adapter 提供。
        "app": "desktop"
    })
}

// 验证只读步骤可在统一 Policy 前应用结构化模板。
#[test]
fn structured_template_materializes_for_read_step() -> Result<(), Box<dyn std::error::Error>> {
    // 构造第二步从首步 app 字符串派生一个目标叶字段。
    let input: SequenceInput = serde_json::from_value(json!({
        // 两步都保持只读。
        "steps": [status_step("source"), {
            // 保留结果关联名称。
            "name": "templated-read",
            // 首批模板允许 status。
            "verb": "status",
            // 使用确定性 desktop Adapter。
            "app": "desktop",
            // 静态声明全部父对象。
            "target": { "derived": {} },
            // 声明一个结构化模板绑定。
            "bindings": [{
                // 按顺序拼接 literal、source 和 literal。
                "template": [
                    // 逐字前缀。
                    { "kind": "literal", "text": "prefix-" },
                    // 读取第一步 app 字符串。
                    { "kind": "source", "sourceStep": 1, "sourcePointer": "/app" },
                    // 逐字后缀。
                    { "kind": "literal", "text": "-suffix" }
                ],
                // 只写 target 区域。
                "destination": "target",
                // 允许在静态对象父路径下创建叶字段。
                "destinationPointer": "/derived/value"
            }]
        }]
    }))?;
    // 通过公开 System 执行 Workflow。
    let result = AppControlService::new().sequence(input)?;
    // 两个 provider 步骤都成功完成。
    assert_eq!(result["count"], 2);
    // Workflow 保持成功。
    assert_eq!(result["ok"], true);
    // 第二步记录一个已应用绑定且不回显最终值。
    assert_eq!(result["results"][1]["bindings"]["applied"], 1);
    // 完整物化请求仍由统一 Policy 标记执行域。
    assert_eq!(
        // 读取第二步结果执行域。
        result["results"][1]["result"]["executionRealm"],
        // 对比 host 只读执行域。
        "host-headless"
    );
    // 测试正常完成。
    Ok(())
}

// 验证 run 模板在任何 provider 启动前失败闭合。
#[test]
fn run_template_is_rejected_before_provider_execution() -> Result<(), Box<dyn std::error::Error>> {
    // 首步使用未知 app 证明全量静态验证先发生。
    let input: SequenceInput = serde_json::from_value(json!({
        // 第二步声明当前禁止的 run 模板。
        "steps": [
            // 若先执行就会产生不同 provider 错误。
            { "verb": "status", "app": "never-execute" },
            // run 必须等待稳定 resume 和动态确认。
            {
                // 使用变更入口动词。
                "verb": "run",
                // Adapter 在本测试中不应被调用。
                "app": "desktop",
                // 静态声明目标叶字段。
                "target": { "slot": null },
                // 声明结构化模板来源。
                "bindings": [{
                    // 模板至少包含一个来源段。
                    "template": [{ "kind": "source", "sourceStep": 1, "sourcePointer": "/app" }],
                    // 只选择 target 区域。
                    "destination": "target",
                    // 替换静态字段。
                    "destinationField": "slot"
                }]
            }
        ]
    }))?;
    // System 必须返回静态参数错误而不是 provider 结果。
    let error = AppControlService::new()
        // 调用公开 Workflow。
        .sequence(input)
        // 取得失败分支。
        .err()
        // 意外成功时转换为测试错误。
        .ok_or_else(|| std::io::Error::other("run template unexpectedly succeeded"))?;
    // 当前发布范围使用稳定参数错误码。
    assert_eq!(error.code, "INVALID_ARGUMENT");
    // 错误精确定位第二步模板字段。
    assert_eq!(error.details["field"], "steps[1].bindings[0].template");
    // 证据明确列出两个后续依赖。
    assert_eq!(error.details["blockedBy"][0], "GC-FLOW-003A");
    // 测试正常完成。
    Ok(())
}

// 验证模板来源不是 JSON string 时当前 provider 不启动。
#[test]
fn template_source_type_mismatch_stops_current_provider() -> Result<(), Box<dyn std::error::Error>>
{
    // 第二步读取第一步根对象并要求字符串模板语义。
    let input: SequenceInput = serde_json::from_value(json!({
        // 第二步若越过绑定会产生未知 app 错误。
        "steps": [status_step("source"), {
            // 使用只读动词进入首批模板范围。
            "verb": "status",
            // 当前 provider 不应启动。
            "app": "never-execute",
            // 预先声明目标字段。
            "args": { "slot": null },
            // 根 Pointer 返回对象而不是字符串。
            "bindings": [{
                // 单个 source 段读取完整结果对象。
                "template": [{ "kind": "source", "sourceStep": 1, "sourcePointer": "" }],
                // 只写 args 区域。
                "destination": "args",
                // 替换静态字段。
                "destinationField": "slot"
            }]
        }]
    }))?;
    // 执行只读 Workflow 并取得结构化终止结果。
    let result = AppControlService::new().sequence(input)?;
    // 只有第一步 provider 实际完成。
    assert_eq!(result["count"], 1);
    // 模板失败计为唯一 Workflow 错误。
    assert_eq!(result["workflowErrorCount"], 1);
    // 使用统一绑定失败错误码。
    assert_eq!(result["workflowError"]["code"], "SEQUENCE_BINDING_FAILED");
    // 原因区分严格字符串类型错误。
    assert_eq!(
        // 读取公开封闭原因。
        result["workflowError"]["details"]["reason"],
        // 对比模板专属类型错误。
        "template-source-type-mismatch"
    );
    // 一基段索引定位首个来源段。
    assert_eq!(result["workflowError"]["details"]["segmentIndex"], 1);
    // 当前 provider 明确未启动。
    assert_eq!(result["workflowError"]["details"]["stepStarted"], false);
    // 测试正常完成。
    Ok(())
}

// 验证静态 literal 合法但最终模板输出超限时不泄漏内容。
#[test]
fn template_output_limit_stops_before_current_provider() -> Result<(), Box<dyn std::error::Error>> {
    // 构造恰好达到静态总上限的四个 literal 段。
    let literal = "a".repeat(MAX_SEQUENCE_TEMPLATE_LITERAL_BYTES);
    // 第二步追加来源 app 后突破最终 4096 字节上限。
    let input: SequenceInput = serde_json::from_value(json!({
        // 首步提供字符串来源。
        "steps": [status_step("source"), {
            // 使用允许模板的只读动词。
            "verb": "status",
            // 当前 provider 不应启动。
            "app": "never-execute",
            // 预先声明目标字段。
            "target": { "slot": null },
            // 静态文本总计 4096 字节，动态来源令最终值超限。
            "bindings": [{
                // 使用四个上限 literal 和一个字符串来源。
                "template": [
                    // 第一个静态段。
                    { "kind": "literal", "text": literal },
                    // 第二个静态段。
                    { "kind": "literal", "text": literal },
                    // 第三个静态段。
                    { "kind": "literal", "text": literal },
                    // 第四个静态段。
                    { "kind": "literal", "text": literal },
                    // 动态字符串来源触发最终上限。
                    { "kind": "source", "sourceStep": 1, "sourcePointer": "/app" }
                ],
                // 只写 target 区域。
                "destination": "target",
                // 替换静态字段。
                "destinationField": "slot"
            }]
        }]
    }))?;
    // 执行 Workflow 并取得结构化终止结果。
    let result = AppControlService::new().sequence(input)?;
    // 输出超限使用模板专属原因。
    assert_eq!(
        // 读取终止原因。
        result["workflowError"]["details"]["reason"],
        // 对比稳定原因。
        "template-output-too-large"
    );
    // 实际字节数必须大于统一最终上限。
    assert!(
        // 读取实际字节数。
        result["workflowError"]["details"]["attemptedBytes"]
            // 转换为无符号整数。
            .as_u64()
            // 证据缺失时测试失败。
            .is_some_and(|bytes| bytes > MAX_SEQUENCE_BOUND_VALUE_BYTES as u64)
    );
    // 公开错误不得回显任何模板内容或最终值。
    assert!(!result["workflowError"].to_string().contains(&literal));
    // 测试正常完成。
    Ok(())
}

// 验证公开 JSON Schema 与 Rust 模板硬边界一致。
#[test]
fn template_schema_matches_runtime_bounds() -> Result<(), Box<dyn std::error::Error>> {
    // 解析仓库内版本化输入 schema。
    let schema: Value = serde_json::from_str(include_str!(
        // 使用编译期路径避免依赖工作目录。
        "../contracts/v1/sequence-input.schema.json"
    ))?;
    // 取得模板数组契约。
    let template = &schema["$defs"]["binding"]["properties"]["template"];
    // 核对模板总段数上限。
    assert_eq!(template["maxItems"], MAX_SEQUENCE_TEMPLATE_SEGMENTS);
    // 核对动态来源段上限。
    assert_eq!(template["maxContains"], MAX_SEQUENCE_TEMPLATE_SOURCES);
    // 核对单 literal 数值上限。
    assert_eq!(
        // 读取 literal 文本 schema 上限。
        schema["$defs"]["literalTemplateSegment"]["properties"]["text"]["maxLength"],
        // 对比 Rust UTF-8 字节硬上限。
        MAX_SEQUENCE_TEMPLATE_LITERAL_BYTES
    );
    // 静态总字节与最终输出上限保持相同且明确。
    assert_eq!(
        MAX_SEQUENCE_TEMPLATE_STATIC_BYTES,
        MAX_SEQUENCE_BOUND_VALUE_BYTES
    );
    // 来源形状与模板必须由第一个 allOf 分支恰好二选一。
    assert_eq!(
        // 读取来源 oneOf 分支数量。
        schema["$defs"]["binding"]["allOf"][0]["oneOf"]
            // 转换为数组引用。
            .as_array()
            // 读取分支数量。
            .map(Vec::len),
        // 直接来源和模板来源两个形状。
        Some(2)
    );
    // 测试正常完成。
    Ok(())
}
