#![cfg(target_os = "windows")]

// 导入 System、严格输入类型与公开预算常量。
use ai_computer_toolkit::{
    // 导入公开 System 协调入口。
    AppControlService,
    // 导入 sequence 契约类型和边界常量。
    service::{
        DEFAULT_SEQUENCE_RESULT_BYTES, DEFAULT_SEQUENCE_STEP_TIMEOUT_MS,
        DEFAULT_SEQUENCE_TOTAL_TIMEOUT_MS, MAX_SEQUENCE_POSTCONDITION_EXPECTED_BYTES,
        MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES, MAX_SEQUENCE_POSTCONDITIONS,
        MAX_SEQUENCE_PRECONDITIONS, MAX_SEQUENCE_RESULT_BYTES, MAX_SEQUENCE_STEP_TIMEOUT_MS,
        MAX_SEQUENCE_STEPS, MAX_SEQUENCE_TOTAL_TIMEOUT_MS, MIN_SEQUENCE_RESULT_BYTES,
        MIN_SEQUENCE_STEP_TIMEOUT_MS, MIN_SEQUENCE_TOTAL_TIMEOUT_MS, SequenceInput,
    },
};
// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 强制 Cargo 为 integration Workflow 回归构建固定 sequence step worker。
const _SEQUENCE_STEP_WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-sequence-step-worker");

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

// 验证顶层和 step 未知字段都在反序列化阶段拒绝。
#[test]
fn sequence_input_rejects_unknown_fields() {
    // 顶层未知控制项不能被静默忽略。
    let unknown_top_level = serde_json::from_value::<SequenceInput>(json!({
        // 提供一个有效步骤以隔离未知字段行为。
        "steps": [status_step("one")],
        // 模拟调用方误拼的预算字段。
        "resultBudget": 1024,
    }));
    // 严格输入必须拒绝顶层未知字段。
    assert!(unknown_top_level.is_err());

    // step 未知控制项同样不能被静默忽略。
    let unknown_step = serde_json::from_value::<SequenceInput>(json!({
        // 在步骤中加入未声明的伪 deadline 字段。
        "steps": [{
            // 使用只读状态动词。
            "verb": "status",
            // 使用已注册 adapter。
            "app": "desktop",
            // 当前版本不允许别名字段绕过唯一 timeoutMs 契约。
            "deadlineMs": 10,
        }],
    }));
    // 严格 step 必须拒绝未知字段。
    assert!(unknown_step.is_err());

    // 后置断言未知 operator 必须在反序列化阶段拒绝。
    let unknown_operator = serde_json::from_value::<SequenceInput>(json!({
        // 构造 provider 本身有效的只读步骤。
        "steps": [{
            // 使用状态读取动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // 声明当前契约不存在的 contains operator。
            "postconditions": [{ "operator": "contains", "pointer": "/app" }],
        }],
    }));
    // 未知 operator 不得静默退化。
    assert!(unknown_operator.is_err());

    // exists 变体的未知 expected 字段必须拒绝。
    let unknown_condition_field = serde_json::from_value::<SequenceInput>(json!({
        // 构造只读步骤和伪 expected 字段。
        "steps": [{
            // 使用状态读取动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // exists 不允许携带 expected。
            "postconditions": [{ "operator": "exists", "pointer": "/app", "expected": true }],
        }],
    }));
    // 变体未知字段必须由 deny_unknown_fields 拒绝。
    assert!(unknown_condition_field.is_err());

    // equals 缺少 required expected 必须拒绝。
    let missing_expected = serde_json::from_value::<SequenceInput>(json!({
        // 构造缺少 expected 的 equals 断言。
        "steps": [{
            // 使用状态读取动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // 只提供 operator 和 Pointer。
            "postconditions": [{ "operator": "equals", "pointer": "/app" }],
        }],
    }));
    // equals 必须要求 expected 字段存在。
    assert!(missing_expected.is_err());

    // 显式 JSON null 是合法 equals 期望值而不是缺失字段。
    let null_expected = serde_json::from_value::<SequenceInput>(json!({
        // 构造携带 null 期望值的合法断言。
        "steps": [{
            // 使用状态读取动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // expected 字段明确存在且值为 null。
            "postconditions": [{ "operator": "equals", "pointer": "/optional", "expected": null }],
        }],
    }));
    // null 期望值必须成功解析。
    assert!(null_expected.is_ok());
}

// 验证步骤数量与预算边界在任何 provider 调用前失败。
#[test]
fn sequence_validates_all_workflow_bounds_before_execution()
-> Result<(), Box<dyn std::error::Error>> {
    // 解析空步骤工作流。
    let empty: SequenceInput = serde_json::from_value(json!({ "steps": [] }))?;
    // 执行 System 前置验证并取得错误。
    let empty_error = AppControlService::new()
        .sequence(empty)
        .err()
        .ok_or_else(|| {
            // 构造测试失败原因。
            std::io::Error::other("empty sequence unexpectedly succeeded")
        })?;
    // 空步骤使用稳定参数错误码。
    assert_eq!(empty_error.code, "INVALID_ARGUMENT");
    // 错误证据必须指出步骤字段。
    assert_eq!(empty_error.details["field"], "steps");

    // 构造超过硬上限的一组未知 app 步骤。
    let steps = (0..=MAX_SEQUENCE_STEPS)
        // 每项若被执行都会产生不同于前置验证的 adapter 错误。
        .map(|index| {
            // 构造严格 schema 接受但 app 不存在的步骤。
            json!({ "name": format!("step-{index}"), "verb": "status", "app": "never-execute" })
        })
        // 收集为 JSON 数组负载。
        .collect::<Vec<_>>();
    // 解析超上限输入。
    let too_many: SequenceInput = serde_json::from_value(json!({ "steps": steps }))?;
    // 执行 System 前置验证并取得错误。
    let count_error = AppControlService::new()
        // 调用工作流入口。
        .sequence(too_many)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("oversized sequence unexpectedly succeeded"))?;
    // 超步骤数必须仍是前置参数错误，而不是 adapter 错误。
    assert_eq!(count_error.code, "INVALID_ARGUMENT");
    // 报告实际步骤数。
    assert_eq!(count_error.details["actual"], MAX_SEQUENCE_STEPS + 1);
    // 报告固定最大步骤数。
    assert_eq!(count_error.details["maximum"], MAX_SEQUENCE_STEPS);

    // 解析低于结果预算下界的输入。
    let too_small: SequenceInput = serde_json::from_value(json!({
        // 保留一个有效只读步骤。
        "steps": [status_step("one")],
        // 使用下界前一个字节。
        "maxResultBytes": MIN_SEQUENCE_RESULT_BYTES - 1,
    }))?;
    // 执行预算前置验证并取得错误。
    let budget_error = AppControlService::new()
        // 调用工作流入口。
        .sequence(too_small)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("invalid budget unexpectedly succeeded"))?;
    // 预算越界必须使用稳定参数错误码。
    assert_eq!(budget_error.code, "INVALID_ARGUMENT");
    // 错误证据必须指出预算字段。
    assert_eq!(budget_error.details["field"], "maxResultBytes");

    // 解析低于总执行预算下界的输入。
    let invalid_total: SequenceInput = serde_json::from_value(json!({
        // 保留一个有效只读步骤。
        "steps": [status_step("one")],
        // 使用公开下界前一个毫秒。
        "totalTimeoutMs": MIN_SEQUENCE_TOTAL_TIMEOUT_MS - 1,
    }))?;
    // 执行总预算前置验证并取得错误。
    let total_error = AppControlService::new()
        // 调用工作流入口。
        .sequence(invalid_total)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("invalid total timeout unexpectedly succeeded"))?;
    // 总预算越界必须使用稳定参数错误码。
    assert_eq!(total_error.code, "INVALID_ARGUMENT");
    // 错误证据必须指出总预算字段。
    assert_eq!(total_error.details["field"], "totalTimeoutMs");

    // 解析超过逐步执行预算上界的输入。
    let invalid_step: SequenceInput = serde_json::from_value(json!({
        // 在首步设置越界预算以证明 provider 不会启动。
        "steps": [{
            // 使用只读状态动词。
            "verb": "status",
            // 使用不会在前置验证前解析的未知 app。
            "app": "never-execute",
            // 使用公开上界后一个毫秒。
            "timeoutMs": MAX_SEQUENCE_STEP_TIMEOUT_MS + 1,
        }],
    }))?;
    // 执行步骤预算前置验证并取得错误。
    let step_error = AppControlService::new()
        // 调用工作流入口。
        .sequence(invalid_step)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("invalid step timeout unexpectedly succeeded"))?;
    // 步骤预算越界必须使用稳定参数错误码。
    assert_eq!(step_error.code, "INVALID_ARGUMENT");
    // 错误证据必须携带零基步骤路径。
    assert_eq!(step_error.details["field"], "steps[0].timeoutMs");
    // 测试正常完成。
    Ok(())
}

// 验证所有后置断言边界都在未知 provider 解析前失败。
#[test]
fn postcondition_bounds_are_validated_before_provider_execution()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造超过单步骤硬上限的 exists 断言列表。
    let too_many_conditions = (0..=MAX_SEQUENCE_POSTCONDITIONS)
        // 每项使用合法且很小的根指针。
        .map(|_| json!({ "operator": "exists", "pointer": "" }))
        // 收集为严格输入数组。
        .collect::<Vec<_>>();
    // 使用未知 app 证明断言数量在 adapter 解析前拒绝。
    let too_many: SequenceInput = serde_json::from_value(json!({
        // 首步若执行会返回未知应用错误。
        "steps": [{
            // 使用状态读取动词。
            "verb": "status",
            // 使用不应被解析的未知 app。
            "app": "never-execute",
            // 注入超过硬上限的断言。
            "postconditions": too_many_conditions,
        }],
    }))?;
    // 执行 System 前置验证并取得错误。
    let count_error = AppControlService::new()
        // 调用 sequence 公开入口。
        .sequence(too_many)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("too many postconditions unexpectedly succeeded"))?;
    // 数量超限必须是参数错误而非未知 app 错误。
    assert_eq!(count_error.code, "INVALID_ARGUMENT");
    // 错误字段必须定位 postconditions 数组。
    assert_eq!(count_error.details["field"], "steps[0].postconditions");

    // 构造 UTF-8 字节数超过 Pointer 上限的多字节字符串。
    let oversized_pointer = "界".repeat(MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES);
    // 解析包含超长 Pointer 的未知 app 步骤。
    let pointer_input: SequenceInput = serde_json::from_value(json!({
        // 首步若执行会返回未知应用错误。
        "steps": [{
            // 使用状态读取动词。
            "verb": "status",
            // 使用不应被解析的未知 app。
            "app": "never-execute",
            // Pointer 字符数不大但 UTF-8 字节数超限。
            "postconditions": [{ "operator": "exists", "pointer": oversized_pointer }],
        }],
    }))?;
    // 执行 System 前置验证并取得错误。
    let pointer_error = AppControlService::new()
        // 调用 sequence 公开入口。
        .sequence(pointer_input)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("oversized pointer unexpectedly succeeded"))?;
    // Pointer 字节超限必须是参数错误。
    assert_eq!(pointer_error.code, "INVALID_ARGUMENT");
    // 错误字段必须精确定位 Pointer。
    assert_eq!(
        // 读取公开字段路径。
        pointer_error.details["field"],
        // 对比零基步骤和断言索引。
        "steps[0].postconditions[0].pointer"
    );

    // 解析包含非法 RFC 6901 转义的未知 app 步骤。
    let invalid_pointer: SequenceInput = serde_json::from_value(json!({
        // 首步若执行会返回未知应用错误。
        "steps": [{
            // 使用状态读取动词。
            "verb": "status",
            // 使用不应被解析的未知 app。
            "app": "never-execute",
            // `~2` 不是合法 JSON Pointer 转义。
            "postconditions": [{ "operator": "exists", "pointer": "/bad~2escape" }],
        }],
    }))?;
    // 执行 System 前置验证并取得错误。
    let syntax_error = AppControlService::new()
        // 调用 sequence 公开入口。
        .sequence(invalid_pointer)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("invalid pointer unexpectedly succeeded"))?;
    // 非法 Pointer 必须在 adapter 前失败。
    assert_eq!(syntax_error.code, "INVALID_ARGUMENT");
    // 错误字段必须精确定位 Pointer。
    assert_eq!(
        // 读取公开字段路径。
        syntax_error.details["field"],
        // 对比零基步骤和断言索引。
        "steps[0].postconditions[0].pointer"
    );

    // 构造序列化后超过 equals 期望负载上限的字符串。
    let oversized_expected = "x".repeat(MAX_SEQUENCE_POSTCONDITION_EXPECTED_BYTES);
    // 解析包含超大 expected 的未知 app 步骤。
    let expected_input: SequenceInput = serde_json::from_value(json!({
        // 首步若执行会返回未知应用错误。
        "steps": [{
            // 使用状态读取动词。
            "verb": "status",
            // 使用不应被解析的未知 app。
            "app": "never-execute",
            // JSON 字符串引号使紧凑负载超过常量上限。
            "postconditions": [{ "operator": "equals", "pointer": "", "expected": oversized_expected }],
        }],
    }))?;
    // 执行 System 前置验证并取得错误。
    let expected_error = AppControlService::new()
        // 调用 sequence 公开入口。
        .sequence(expected_input)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("oversized expected unexpectedly succeeded"))?;
    // expected 字节超限必须是参数错误。
    assert_eq!(expected_error.code, "INVALID_ARGUMENT");
    // 错误字段必须精确定位 expected。
    assert_eq!(
        // 读取公开字段路径。
        expected_error.details["field"],
        // 对比零基步骤和断言索引。
        "steps[0].postconditions[0].expected"
    );
    // 测试正常完成。
    Ok(())
}

// 验证 exists 与 equals 通过后允许工作流正常完成。
#[test]
fn postconditions_pass_on_complete_success_result() -> Result<(), Box<dyn std::error::Error>> {
    // 构造带两项断言的确定性 desktop 状态步骤。
    let input: SequenceInput = serde_json::from_value(json!({
        // 只执行一个无副作用步骤。
        "steps": [{
            // 使用可关联名称。
            "name": "assert-status",
            // 使用状态读取动词。
            "verb": "status",
            // desktop 结果稳定包含 app 字段。
            "app": "desktop",
            // 依次验证字段存在和精确值。
            "postconditions": [
                // exists 验证 app 字段存在。
                { "operator": "exists", "pointer": "/app" },
                // equals 验证 app 字段值。
                { "operator": "equals", "pointer": "/app", "expected": "desktop" }
            ],
        }],
    }))?;
    // 执行无副作用工作流。
    let result = AppControlService::new().sequence(input)?;
    // provider 和断言均成功时整体成功。
    assert_eq!(result["ok"], true);
    // 不产生工作流错误。
    assert_eq!(result["workflowErrorCount"], 0);
    // 两项断言都已执行。
    assert_eq!(result["results"][0]["postconditions"]["checked"], 2);
    // 整组断言报告通过。
    assert_eq!(result["results"][0]["postconditions"]["passed"], true);
    // 完整 provider 结果仍然返回。
    assert_eq!(result["results"][0]["result"]["app"], "desktop");
    // 测试正常完成。
    Ok(())
}

// 验证缺失断言保持步骤成功并硬停止后续步骤。
#[test]
fn missing_postcondition_preserves_success_and_stops_later_steps()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造首步缺失断言和一个不应执行的后续步骤。
    let input: SequenceInput = serde_json::from_value(json!({
        // 第二步用于证明断言失败立即停止。
        "steps": [
            // 首步 provider 成功但 Pointer 缺失。
            {
                // 使用状态读取动词。
                "verb": "status",
                // 使用确定性 desktop adapter。
                "app": "desktop",
                // 指向不存在字段。
                "postconditions": [{ "operator": "exists", "pointer": "/missing" }]
            },
            // 后续步骤不得执行。
            { "verb": "status", "app": "never-execute" }
        ],
        // 即使允许普通 provider 错误继续也不能放宽断言失败。
        "continueOnError": true,
    }))?;
    // 执行无副作用工作流。
    let result = AppControlService::new().sequence(input)?;
    // 断言失败使整体失败。
    assert_eq!(result["ok"], false);
    // 只执行首个步骤。
    assert_eq!(result["count"], 1);
    // 保留调用方请求的步骤总数。
    assert_eq!(result["total"], 2);
    // provider 实际成功，因此失败数保持零。
    assert_eq!(result["failedCount"], 0);
    // 断言失败计为一个工作流错误。
    assert_eq!(result["workflowErrorCount"], 1);
    // 步骤成功事实保持为 true。
    assert_eq!(result["results"][0]["ok"], true);
    // 完整 provider 结果必须保留。
    assert_eq!(result["results"][0]["result"]["app"], "desktop");
    // 使用稳定后置断言错误码。
    assert_eq!(
        // 读取工作流错误码。
        result["results"][0]["workflowError"]["code"],
        // 对比公开稳定码。
        "SEQUENCE_POSTCONDITION_FAILED"
    );
    // 错误证据保持 provider 已成功。
    assert_eq!(
        // 读取步骤成功证据。
        result["results"][0]["workflowError"]["details"]["stepSucceeded"],
        // 期望真实成功。
        true
    );
    // 缺失使用封闭原因。
    assert_eq!(
        // 读取失败原因。
        result["results"][0]["workflowError"]["details"]["reason"],
        // 对比稳定原因。
        "missing"
    );
    // 测试正常完成。
    Ok(())
}

// 验证 equals 不相等返回独立原因且不计 provider 失败。
#[test]
fn equals_mismatch_reports_not_equal() -> Result<(), Box<dyn std::error::Error>> {
    // 构造预期 app 值不匹配的单步骤工作流。
    let input: SequenceInput = serde_json::from_value(json!({
        // 只执行一个无副作用步骤。
        "steps": [{
            // 使用状态读取动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // 期望一个确定不相等的字符串。
            "postconditions": [{ "operator": "equals", "pointer": "/app", "expected": "browser" }],
        }],
    }))?;
    // 执行无副作用工作流。
    let result = AppControlService::new().sequence(input)?;
    // provider 失败数保持零。
    assert_eq!(result["failedCount"], 0);
    // 步骤成功事实保持为 true。
    assert_eq!(result["results"][0]["ok"], true);
    // 不相等使用独立封闭原因。
    assert_eq!(
        // 读取失败原因。
        result["results"][0]["workflowError"]["details"]["reason"],
        // 对比稳定原因。
        "not-equal"
    );
    // 操作符证据保持 equals。
    assert_eq!(
        // 读取操作符。
        result["results"][0]["workflowError"]["details"]["operator"],
        // 对比公开名称。
        "equals"
    );
    // 测试正常完成。
    Ok(())
}

// 验证断言和预算同时失败时预算错误优先且断言已执行。
#[test]
fn result_budget_error_takes_precedence_after_postcondition_evaluation()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造结果超最小预算且断言不相等的步骤。
    let input: SequenceInput = serde_json::from_value(json!({
        // 第二步用于证明优先错误仍然硬停止。
        "steps": [
            // 首步产生大于最小预算的 desktop 状态结果。
            {
                // 使用状态读取动词。
                "verb": "status",
                // 使用确定性 desktop adapter。
                "app": "desktop",
                // 同时制造一个可判定不相等断言。
                "postconditions": [{ "operator": "equals", "pointer": "/app", "expected": "browser" }]
            },
            // 后续步骤不得执行。
            { "verb": "status", "app": "never-execute" }
        ],
        // 普通 provider 错误继续策略不能放宽两种硬错误。
        "continueOnError": true,
        // 使用公开最小结果预算制造冲突。
        "maxResultBytes": MIN_SEQUENCE_RESULT_BYTES,
    }))?;
    // 执行无副作用工作流。
    let result = AppControlService::new().sequence(input)?;
    // 只执行首个步骤。
    assert_eq!(result["count"], 1);
    // 预算优先时仍只报告一个终止工作流错误。
    assert_eq!(result["workflowErrorCount"], 1);
    // 顶层预算证据明确耗尽。
    assert_eq!(result["budget"]["exhausted"], true);
    // 完整结果无法接纳时必须省略而不截断。
    assert_eq!(result["results"][0]["resultOmitted"], true);
    // provider 成功事实保持为 true。
    assert_eq!(result["results"][0]["ok"], true);
    // 断言在预算判定前已经执行并报告失败。
    assert_eq!(result["results"][0]["postconditions"]["passed"], false);
    // 对外终止错误由结果预算优先占用。
    assert_eq!(
        // 读取工作流错误码。
        result["results"][0]["workflowError"]["code"],
        // 对比预算稳定错误码。
        "SEQUENCE_RESULT_BUDGET_EXCEEDED"
    );
    // 测试正常完成。
    Ok(())
}

// 验证结果预算耗尽保持步骤事实并覆盖继续策略。
#[test]
fn result_budget_exhaustion_stops_even_when_continue_is_enabled()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造两个只读步骤和最小结果预算。
    let input: SequenceInput = serde_json::from_value(json!({
        // 第二步用于证明预算耗尽后没有继续执行。
        "steps": [status_step("first"), status_step("second")],
        // 普通 provider 错误允许继续，但预算错误不允许。
        "continueOnError": true,
        // desktop 状态结果稳定大于该最小预算。
        "maxResultBytes": MIN_SEQUENCE_RESULT_BYTES,
    }))?;
    // 执行只读工作流。
    let result = AppControlService::new().sequence(input)?;
    // 工作流因为结果无法完整收集而失败。
    assert_eq!(result["ok"], false);
    // 只执行并记录首个步骤。
    assert_eq!(result["count"], 1);
    // 保留调用方原始步骤总数。
    assert_eq!(result["total"], 2);
    // 成功步骤的结果省略不计为 provider 失败。
    assert_eq!(result["failedCount"], 0);
    // 记录唯一工作流预算错误。
    assert_eq!(result["workflowErrorCount"], 1);
    // 预算证据明确标记耗尽。
    assert_eq!(result["budget"]["exhausted"], true);
    // 首份负载未能完整接纳，因此已用量仍为零。
    assert_eq!(result["budget"]["resultBytes"], 0);
    // 已执行步骤的 provider 成功事实保持为 true。
    assert_eq!(result["results"][0]["ok"], true);
    // 明确报告成功结果已省略。
    assert_eq!(result["results"][0]["resultOmitted"], true);
    // 不返回不完整 result 字段。
    assert!(result["results"][0].get("result").is_none());
    // 使用稳定工作流错误码。
    assert_eq!(
        // 读取嵌套错误码。
        result["results"][0]["workflowError"]["code"],
        // 对比公开契约常量文本。
        "SEQUENCE_RESULT_BUDGET_EXCEEDED"
    );
    // 错误证据明确说明步骤已经成功完成。
    assert_eq!(
        // 读取步骤成功证据。
        result["results"][0]["workflowError"]["details"]["stepSucceeded"],
        // 期望保持真实成功语义。
        true
    );
    // 测试正常完成。
    Ok(())
}

// 验证超预算错误负载仍保留 provider 失败事实。
#[test]
fn oversized_provider_error_is_omitted_without_becoming_success()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造达到 worker route 上限的未知 app ID。
    let unknown_app = "x".repeat(128);
    // 构造先失败再只读的两个步骤。
    let input: SequenceInput = serde_json::from_value(json!({
        // 首步产生包含长 app ID 的确定性参数错误。
        "steps": [
            // 错误负载会超过预算。
            { "name": "failure", "verb": "status", "app": unknown_app },
            // 该步骤用于证明预算错误立即停止。
            status_step("must-not-run")
        ],
        // 即使允许普通错误继续，预算错误也必须停止。
        "continueOnError": true,
        // 使用公开最小结果预算。
        "maxResultBytes": MIN_SEQUENCE_RESULT_BYTES,
    }))?;
    // 执行不会修改外部状态的工作流。
    let result = AppControlService::new().sequence(input)?;
    // 整体工作流失败。
    assert_eq!(result["ok"], false);
    // 预算耗尽后第二步没有执行。
    assert_eq!(result["count"], 1);
    // provider 失败仍计入失败数。
    assert_eq!(result["failedCount"], 1);
    // 首步保持真实失败状态。
    assert_eq!(result["results"][0]["ok"], false);
    // 明确报告错误负载已省略。
    assert_eq!(result["results"][0]["errorOmitted"], true);
    // 不返回不完整 error 字段。
    assert!(result["results"][0].get("error").is_none());
    // 预算错误证据保持 provider 失败事实。
    assert_eq!(
        // 读取步骤成功证据。
        result["results"][0]["workflowError"]["details"]["stepSucceeded"],
        // 期望明确为失败。
        false
    );
    // 测试正常完成。
    Ok(())
}

// 验证预算内结果与公开字节证据一致。
#[test]
fn result_budget_reports_exact_included_payload_bytes() -> Result<(), Box<dyn std::error::Error>> {
    // 构造使用默认预算的单步骤只读工作流。
    let input: SequenceInput = serde_json::from_value(json!({
        // 只读取 desktop 状态。
        "steps": [status_step("status")],
    }))?;
    // 执行工作流。
    let result = AppControlService::new().sequence(input)?;
    // 预算内工作流成功。
    assert_eq!(result["ok"], true);
    // 回显默认一 MiB 预算。
    assert_eq!(
        // 读取公开最大结果字节数。
        result["budget"]["maxResultBytes"],
        // 对比 Rust 单一来源常量。
        DEFAULT_SEQUENCE_RESULT_BYTES
    );
    // 读取完整 provider 结果。
    let payload = &result["results"][0]["result"];
    // 公开已用量等于紧凑 JSON 的 UTF-8 字节数。
    assert_eq!(result["budget"]["resultBytes"], payload.to_string().len());
    // 测试正常完成。
    Ok(())
}

// 验证公开 schema 与 Rust 边界常量保持一致。
#[test]
fn input_schema_matches_runtime_bounds() -> Result<(), Box<dyn std::error::Error>> {
    // 解析仓库内版本化输入 schema。
    let schema: Value = serde_json::from_str(include_str!(
        // 使用编译期路径防止测试依赖工作目录。
        "../../contracts/v1/sequence-input.schema.json"
    ))?;
    // 顶层对象必须拒绝未知字段。
    assert_eq!(schema["additionalProperties"], false);
    // step 对象同样必须拒绝未知字段。
    assert_eq!(schema["$defs"]["step"]["additionalProperties"], false);
    // 两个断言变体必须分别拒绝未知字段。
    assert_eq!(
        // 读取 exists 变体封闭标记。
        schema["$defs"]["existsPostcondition"]["additionalProperties"],
        // 期望封闭对象。
        false
    );
    // 两个前置条件变体必须分别拒绝未知字段。
    assert_eq!(
        // 读取 exists 前置条件封闭标记。
        schema["$defs"]["existsPrecondition"]["additionalProperties"],
        // 期望封闭对象。
        false
    );
    // equals 前置条件同样必须封闭。
    assert_eq!(
        // 读取 equals 前置条件封闭标记。
        schema["$defs"]["equalsPrecondition"]["additionalProperties"],
        // 期望封闭对象。
        false
    );
    // equals 变体同样必须封闭。
    assert_eq!(
        // 读取 equals 变体封闭标记。
        schema["$defs"]["equalsPostcondition"]["additionalProperties"],
        // 期望封闭对象。
        false
    );
    // 核对步骤数硬上限。
    assert_eq!(
        schema["properties"]["steps"]["maxItems"],
        MAX_SEQUENCE_STEPS
    );
    // 核对结果预算下界。
    assert_eq!(
        // 读取 schema 最小值。
        schema["properties"]["maxResultBytes"]["minimum"],
        // 对比 Rust 常量。
        MIN_SEQUENCE_RESULT_BYTES
    );
    // 核对结果预算默认值。
    assert_eq!(
        // 读取 schema 默认值。
        schema["properties"]["maxResultBytes"]["default"],
        // 对比 Rust 常量。
        DEFAULT_SEQUENCE_RESULT_BYTES
    );
    // 核对结果预算上界。
    assert_eq!(
        // 读取 schema 最大值。
        schema["properties"]["maxResultBytes"]["maximum"],
        // 对比 Rust 常量。
        MAX_SEQUENCE_RESULT_BYTES
    );
    // 核对总执行预算 schema 下界。
    assert_eq!(
        // 读取总预算最小值。
        schema["properties"]["totalTimeoutMs"]["minimum"],
        // 对比 Rust 公开常量。
        MIN_SEQUENCE_TOTAL_TIMEOUT_MS
    );
    // 核对总执行预算 schema 默认值。
    assert_eq!(
        // 读取总预算默认值。
        schema["properties"]["totalTimeoutMs"]["default"],
        // 对比 Rust 公开常量。
        DEFAULT_SEQUENCE_TOTAL_TIMEOUT_MS
    );
    // 核对总执行预算 schema 上界。
    assert_eq!(
        // 读取总预算最大值。
        schema["properties"]["totalTimeoutMs"]["maximum"],
        // 对比 Rust 公开常量。
        MAX_SEQUENCE_TOTAL_TIMEOUT_MS
    );
    // 核对逐步执行预算 schema 下界。
    assert_eq!(
        // 读取步骤预算最小值。
        schema["$defs"]["step"]["properties"]["timeoutMs"]["minimum"],
        // 对比 Rust 公开常量。
        MIN_SEQUENCE_STEP_TIMEOUT_MS
    );
    // 核对逐步执行预算 schema 默认值。
    assert_eq!(
        // 读取步骤预算默认值。
        schema["$defs"]["step"]["properties"]["timeoutMs"]["default"],
        // 对比 Rust 公开常量。
        DEFAULT_SEQUENCE_STEP_TIMEOUT_MS
    );
    // 核对逐步执行预算 schema 上界。
    assert_eq!(
        // 读取步骤预算最大值。
        schema["$defs"]["step"]["properties"]["timeoutMs"]["maximum"],
        // 对比 Rust 公开常量。
        MAX_SEQUENCE_STEP_TIMEOUT_MS
    );
    // 核对单步骤后置断言数量上限。
    assert_eq!(
        // 读取 postconditions 数组上限。
        schema["$defs"]["step"]["properties"]["postconditions"]["maxItems"],
        // 对比 Rust 常量。
        MAX_SEQUENCE_POSTCONDITIONS
    );
    // 核对单步骤前置条件数量上限。
    assert_eq!(
        // 读取 preconditions 数组上限。
        schema["$defs"]["step"]["properties"]["preconditions"]["maxItems"],
        // 对比 Rust 常量。
        MAX_SEQUENCE_PRECONDITIONS
    );
    // 核对 exists Pointer schema 字符上限与 Rust 字节硬上限数值同源。
    assert_eq!(
        // 读取 exists Pointer 上限。
        schema["$defs"]["existsPostcondition"]["properties"]["pointer"]["maxLength"],
        // 对比 Rust 运行时字节上限。
        MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES
    );
    // 核对 equals Pointer 使用相同上限。
    assert_eq!(
        // 读取 equals Pointer 上限。
        schema["$defs"]["equalsPostcondition"]["properties"]["pointer"]["maxLength"],
        // 对比 Rust 运行时字节上限。
        MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES
    );
    // 测试正常完成。
    Ok(())
}
