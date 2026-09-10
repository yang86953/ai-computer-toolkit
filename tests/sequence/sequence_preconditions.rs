#![cfg(target_os = "windows")]

// 导入公开 System、严格输入类型和前置条件边界常量。
use ai_computer_toolkit::{
    // 导入公开 System 协调入口。
    AppControlService,
    // 导入 sequence 输入与边界常量。
    service::{
        MAX_SEQUENCE_POSTCONDITION_EXPECTED_BYTES, MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES,
        MAX_SEQUENCE_PRECONDITIONS, SequenceInput,
    },
};
// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 构造一个只读且不依赖真实窗口的 desktop 状态步骤。
fn status_step(name: &str) -> Value {
    // 返回严格 step schema 接受的最小只读输入。
    json!({
        // 使用可断言的调用方名称。
        "name": name,
        // 状态读取不会修改外部状态。
        "verb": "status",
        // desktop 状态由确定性 Rust adapter 提供。
        "app": "desktop",
    })
}

// 验证前置条件 schema 保持封闭且 equals 必须携带 expected。
#[test]
fn precondition_input_rejects_unknown_shapes() {
    // 构造未知 operator。
    let unknown_operator = serde_json::from_value::<SequenceInput>(json!({
        // 第二步声明当前契约不存在的 contains 条件。
        "steps": [status_step("source"), {
            // 使用只读状态动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // 未知 operator 必须拒绝。
            "preconditions": [{
                // 使用未声明的 contains。
                "operator": "contains",
                // 引用严格更早步骤。
                "sourceStep": 1,
                // 使用合法 Pointer。
                "pointer": "/app"
            }]
        }]
    }));
    // 未知 operator 不得静默退化。
    assert!(unknown_operator.is_err());

    // 构造 exists 变体的未知 expected 字段。
    let unknown_field = serde_json::from_value::<SequenceInput>(json!({
        // 第二步携带 exists 不允许的 expected。
        "steps": [status_step("source"), {
            // 使用只读状态动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // exists 必须保持封闭字段集。
            "preconditions": [{
                // 使用 exists operator。
                "operator": "exists",
                // 引用第一步。
                "sourceStep": 1,
                // 指向稳定 app 字段。
                "pointer": "/app",
                // 注入不允许的 expected。
                "expected": "desktop"
            }]
        }]
    }));
    // 变体未知字段必须由 deny_unknown_fields 拒绝。
    assert!(unknown_field.is_err());

    // 构造缺少 expected 的 equals 条件。
    let missing_expected = serde_json::from_value::<SequenceInput>(json!({
        // 第二步携带不完整 equals。
        "steps": [status_step("source"), {
            // 使用只读状态动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // equals 必须要求 expected。
            "preconditions": [{
                // 使用 equals operator。
                "operator": "equals",
                // 引用第一步。
                "sourceStep": 1,
                // 指向稳定 app 字段。
                "pointer": "/app"
            }]
        }]
    }));
    // 缺少 expected 必须拒绝。
    assert!(missing_expected.is_err());
}

// 验证来源方向和条件资源边界在任何 provider 启动前失败。
#[test]
fn precondition_bounds_are_validated_before_provider_execution()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造第一步非法引用自身的输入。
    let first_step_reference: SequenceInput = serde_json::from_value(json!({
        // 唯一步骤若执行会产生未知应用错误。
        "steps": [{
            // 使用状态读取动词。
            "verb": "status",
            // 使用不应被解析的未知 app。
            "app": "never-execute",
            // 第一项步骤不能声明任何来源。
            "preconditions": [{ "operator": "exists", "sourceStep": 1, "pointer": "" }]
        }]
    }))?;
    // 调用 System 前置验证并取得错误。
    let first_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(first_step_reference)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("first-step precondition unexpectedly succeeded"))?;
    // 错误必须来自来源边界而不是 adapter。
    assert_eq!(first_error.code, "INVALID_ARGUMENT");
    // 字段必须精确定位来源索引。
    assert_eq!(
        // 读取公开字段路径。
        first_error.details["field"],
        // 对比零基步骤和条件位置。
        "steps[0].preconditions[0].sourceStep"
    );

    // 构造第二步引用自身的输入，首步同样不得执行。
    let forward_reference: SequenceInput = serde_json::from_value(json!({
        // 第一项使用未知 app 证明全量预验证先发生。
        "steps": [
            // 首步若执行会产生不同错误。
            { "verb": "status", "app": "never-execute" },
            // 第二步非法引用当前步骤。
            {
                // 使用只读状态动词。
                "verb": "status",
                // 使用确定性 desktop adapter。
                "app": "desktop",
                // sourceStep 必须严格小于当前一基索引二。
                "preconditions": [{ "operator": "exists", "sourceStep": 2, "pointer": "" }]
            }
        ]
    }))?;
    // 调用 System 前置验证并取得错误。
    let reference_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(forward_reference)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("forward precondition unexpectedly succeeded"))?;
    // 报告允许的最大来源索引一。
    assert_eq!(reference_error.details["maximum"], 1);

    // 构造超过单步骤硬上限的条件列表。
    let too_many_conditions = (0..=MAX_SEQUENCE_PRECONDITIONS)
        // 每项引用第一步并使用合法根 Pointer。
        .map(|_| json!({ "operator": "exists", "sourceStep": 1, "pointer": "" }))
        // 收集为严格输入数组。
        .collect::<Vec<_>>();
    // 首步未知 app 证明数量验证发生在 provider 前。
    let too_many: SequenceInput = serde_json::from_value(json!({
        // 第二步携带超上限条件。
        "steps": [
            // 首步不应执行。
            { "verb": "status", "app": "never-execute" },
            // 第二步声明过多条件。
            { "verb": "status", "app": "desktop", "preconditions": too_many_conditions }
        ]
    }))?;
    // 调用 System 前置验证并取得错误。
    let count_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(too_many)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("too many preconditions unexpectedly succeeded"))?;
    // 错误字段必须定位前置条件数组。
    assert_eq!(count_error.details["field"], "steps[1].preconditions");

    // 构造字节数超过 Pointer 上限的多字节字符串。
    let oversized_pointer = "界".repeat(MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES);
    // 构造含超长 Pointer 的输入。
    let pointer_input: SequenceInput = serde_json::from_value(json!({
        // 首步使用未知 app 证明全量预验证。
        "steps": [
            // 首步不应执行。
            { "verb": "status", "app": "never-execute" },
            // 第二步携带超长 Pointer。
            {
                // 使用只读状态动词。
                "verb": "status",
                // 使用确定性 desktop adapter。
                "app": "desktop",
                // Pointer 字符数不大但 UTF-8 字节数超限。
                "preconditions": [{
                    // 使用 exists operator。
                    "operator": "exists",
                    // 引用第一步。
                    "sourceStep": 1,
                    // 注入超长 Pointer。
                    "pointer": oversized_pointer
                }]
            }
        ]
    }))?;
    // 调用 System 前置验证并取得错误。
    let pointer_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(pointer_input)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("oversized pointer unexpectedly succeeded"))?;
    // 字段必须精确定位 Pointer。
    assert_eq!(
        // 读取公开字段路径。
        pointer_error.details["field"],
        // 对比零基步骤和条件索引。
        "steps[1].preconditions[0].pointer"
    );

    // 构造序列化后超过 equals 期望负载上限的字符串。
    let oversized_expected = "x".repeat(MAX_SEQUENCE_POSTCONDITION_EXPECTED_BYTES);
    // 构造含超大 expected 的输入。
    let expected_input: SequenceInput = serde_json::from_value(json!({
        // 首步使用未知 app 证明全量预验证。
        "steps": [
            // 首步不应执行。
            { "verb": "status", "app": "never-execute" },
            // 第二步携带超大 expected。
            {
                // 使用只读状态动词。
                "verb": "status",
                // 使用确定性 desktop adapter。
                "app": "desktop",
                // equals 期望 JSON 必须满足字节上限。
                "preconditions": [{
                    // 使用 equals operator。
                    "operator": "equals",
                    // 引用第一步。
                    "sourceStep": 1,
                    // 使用根 Pointer。
                    "pointer": "",
                    // 注入超大期望值。
                    "expected": oversized_expected
                }]
            }
        ]
    }))?;
    // 调用 System 前置验证并取得错误。
    let expected_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(expected_input)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("oversized expected unexpectedly succeeded"))?;
    // 字段必须精确定位 expected。
    assert_eq!(
        // 读取公开字段路径。
        expected_error.details["field"],
        // 对比零基步骤和条件索引。
        "steps[1].preconditions[0].expected"
    );
    // 测试正常完成。
    Ok(())
}

// 验证前置条件通过后当前步骤正常执行并返回通过证据。
#[test]
fn preconditions_pass_on_earlier_success_result() -> Result<(), Box<dyn std::error::Error>> {
    // 构造两个无副作用的 desktop 状态步骤。
    let input: SequenceInput = serde_json::from_value(json!({
        // 第二步读取第一步已建立结果。
        "steps": [status_step("source"), {
            // 使用可关联名称。
            "name": "guarded",
            // 使用只读状态动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // 依次验证字段存在和精确值。
            "preconditions": [
                // exists 验证来源 app 字段存在。
                { "operator": "exists", "sourceStep": 1, "pointer": "/app" },
                // equals 验证来源 app 字段值。
                { "operator": "equals", "sourceStep": 1, "pointer": "/app", "expected": "desktop" }
            ]
        }]
    }))?;
    // 执行只读 Workflow。
    let result = AppControlService::new().sequence(input)?;
    // 两个 provider 步骤都成功完成。
    assert_eq!(result["count"], 2);
    // 整体工作流成功。
    assert_eq!(result["ok"], true);
    // 第二步返回两项通过证据。
    assert_eq!(result["results"][1]["preconditions"]["checked"], 2);
    // 整组前置条件报告通过。
    assert_eq!(result["results"][1]["preconditions"]["passed"], true);
    // 第二个 provider 结果实际存在。
    assert_eq!(result["results"][1]["result"]["app"], "desktop");
    // 测试正常完成。
    Ok(())
}

// 验证前置条件缺失或不相等时当前步骤不启动并硬停止。
#[test]
fn precondition_failure_stops_before_current_provider() -> Result<(), Box<dyn std::error::Error>> {
    // 构造首步成功、第二步条件缺失且 app 本身不可执行的输入。
    let missing_input: SequenceInput = serde_json::from_value(json!({
        // 第二步若绕过条件会产生未知应用错误。
        "steps": [status_step("source"), {
            // 使用可关联名称。
            "name": "must-not-start",
            // 使用状态读取动词。
            "verb": "status",
            // 使用不应被解析的未知 app。
            "app": "never-execute",
            // 指向来源结果中不存在字段。
            "preconditions": [{
                // 使用 exists operator。
                "operator": "exists",
                // 引用第一步。
                "sourceStep": 1,
                // 指向缺失字段。
                "pointer": "/missing"
            }]
        }],
        // 普通 provider 错误继续策略不能放宽前置失败。
        "continueOnError": true
    }))?;
    // 执行只读 Workflow。
    let missing_result = AppControlService::new().sequence(missing_input)?;
    // 只有第一步 provider 实际完成。
    assert_eq!(missing_result["count"], 1);
    // provider 失败数保持零。
    assert_eq!(missing_result["failedCount"], 0);
    // 前置失败计为唯一 Workflow 错误。
    assert_eq!(missing_result["workflowErrorCount"], 1);
    // 顶层使用稳定前置条件错误码。
    assert_eq!(
        // 读取顶层错误码。
        missing_result["workflowError"]["code"],
        // 对比公开稳定码。
        "SEQUENCE_PRECONDITION_FAILED"
    );
    // 当前 provider 明确未启动。
    assert_eq!(
        // 读取启动事实。
        missing_result["workflowError"]["details"]["stepStarted"],
        // 期望未启动。
        false
    );
    // 缺失使用封闭原因。
    assert_eq!(
        // 读取失败原因。
        missing_result["workflowError"]["details"]["reason"],
        // 对比稳定原因。
        "missing"
    );
    // 被阻止步骤不进入 provider results。
    assert!(missing_result["results"].get(1).is_none());

    // 构造 equals 不相等的第二个输入。
    let mismatch_input: SequenceInput = serde_json::from_value(json!({
        // 第二步仍不得启动。
        "steps": [status_step("source"), {
            // 使用状态读取动词。
            "verb": "status",
            // 使用不应被解析的未知 app。
            "app": "never-execute",
            // 期望与来源 app 不同的值。
            "preconditions": [{
                // 使用 equals operator。
                "operator": "equals",
                // 引用第一步。
                "sourceStep": 1,
                // 指向稳定 app 字段。
                "pointer": "/app",
                // 提供确定不相等期望值。
                "expected": "browser"
            }]
        }]
    }))?;
    // 执行只读 Workflow。
    let mismatch_result = AppControlService::new().sequence(mismatch_input)?;
    // 不相等使用独立封闭原因。
    assert_eq!(
        // 读取失败原因。
        mismatch_result["workflowError"]["details"]["reason"],
        // 对比稳定原因。
        "not-equal"
    );
    // 测试正常完成。
    Ok(())
}

// 验证来源 provider 失败时后续前置条件不会读取错误负载。
#[test]
fn failed_source_step_cannot_satisfy_precondition() -> Result<(), Box<dyn std::error::Error>> {
    // 构造首步 provider 失败且允许继续的输入。
    let input: SequenceInput = serde_json::from_value(json!({
        // 第二步尝试读取失败来源。
        "steps": [
            // 首步产生确定性未知应用错误。
            { "name": "failed-source", "verb": "status", "app": "unknown-source" },
            // 第二步若绕过条件也会产生未知应用错误。
            {
                // 使用状态读取动词。
                "verb": "status",
                // 使用不应被解析的未知 app。
                "app": "never-execute",
                // 尝试读取失败步骤的根结果。
                "preconditions": [{ "operator": "exists", "sourceStep": 1, "pointer": "" }]
            }
        ],
        // 允许普通 provider 错误后进入条件判定。
        "continueOnError": true
    }))?;
    // 执行 Workflow。
    let result = AppControlService::new().sequence(input)?;
    // 只有失败来源 provider 实际完成。
    assert_eq!(result["count"], 1);
    // 保留真实 provider 失败数一。
    assert_eq!(result["failedCount"], 1);
    // 来源失败使用独立封闭原因。
    assert_eq!(
        // 读取失败原因。
        result["workflowError"]["details"]["reason"],
        // 对比稳定原因。
        "source-step-failed"
    );
    // 前置条件错误仍只计一个 Workflow 错误。
    assert_eq!(result["workflowErrorCount"], 1);
    // 测试正常完成。
    Ok(())
}
