#![cfg(target_os = "windows")]

// 导入公开 System、严格输入类型与绑定硬边界。
use ai_computer_toolkit::{
    // 导入公开 System 协调入口。
    AppControlService,
    // 导入 sequence 输入与绑定常量。
    service::{
        MAX_SEQUENCE_BINDING_FIELD_BYTES, MAX_SEQUENCE_BINDING_POINTER_BYTES,
        MAX_SEQUENCE_BINDINGS, MAX_SEQUENCE_BOUND_VALUE_BYTES, SequenceInput,
    },
};
// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 构造一个只读且不依赖真实窗口的 desktop 状态步骤。
fn status_step(name: &str) -> Value {
    // 返回严格 step schema 接受的最小只读输入。
    json!({
        // 使用可关联名称。
        "name": name,
        // 状态读取不会修改外部状态。
        "verb": "status",
        // desktop 状态由确定性 Rust adapter 提供。
        "app": "desktop",
    })
}

// 验证绑定输入拒绝冲突来源形状和未知目标区域。
#[test]
fn binding_input_rejects_unknown_shapes() {
    // 构造同时携带直接来源和错误模板形状的绑定输入。
    let unknown_field = serde_json::from_value::<SequenceInput>(json!({
        // 第二步声明绑定。
        "steps": [status_step("source"), {
            // 使用只读状态动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // 预先声明目标字段。
            "target": { "slot": null },
            // 注入与直接来源冲突且类型错误的 template 字段。
            "bindings": [{
                // 引用第一步。
                "sourceStep": 1,
                // 读取来源 app。
                "sourcePointer": "/app",
                // 写入 target。
                "destination": "target",
                // 替换已声明 slot。
                "destinationField": "slot",
                // 非数组模板形状必须拒绝。
                "template": "{}"
            }]
        }]
    }));
    // 严格模板字段不得接受错误类型。
    assert!(unknown_field.is_err());

    // 构造未知 destination 枚举值。
    let unknown_destination = serde_json::from_value::<SequenceInput>(json!({
        // 第二步声明绑定。
        "steps": [status_step("source"), {
            // 使用只读状态动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // 预先声明目标字段。
            "target": { "slot": null },
            // 尝试写入不允许的 operation 区域。
            "bindings": [{
                // 引用第一步。
                "sourceStep": 1,
                // 读取来源 app。
                "sourcePointer": "/app",
                // operation 不属于封闭目标。
                "destination": "operation",
                // 提供目标字段。
                "destinationField": "slot"
            }]
        }]
    }));
    // 绑定不得扩展到控制字段。
    assert!(unknown_destination.is_err());
}

// 验证全部绑定结构边界在首个 provider 调用前失败。
#[test]
fn binding_bounds_are_validated_before_provider_execution() -> Result<(), Box<dyn std::error::Error>>
{
    // 构造第一步非法引用自身的绑定。
    let first_step_reference: SequenceInput = serde_json::from_value(json!({
        // 唯一步骤若执行会产生未知应用错误。
        "steps": [{
            // 使用状态读取动词。
            "verb": "status",
            // 使用不应被解析的未知 app。
            "app": "never-execute",
            // 预先声明目标字段。
            "target": { "slot": null },
            // 第一项步骤不能引用任何来源。
            "bindings": [{
                // 非法引用当前第一步。
                "sourceStep": 1,
                // 使用合法根 Pointer。
                "sourcePointer": "",
                // 写入 target。
                "destination": "target",
                // 替换已声明 slot。
                "destinationField": "slot"
            }]
        }]
    }))?;
    // 调用 System 前置验证并取得错误。
    let source_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(first_step_reference)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("first-step binding unexpectedly succeeded"))?;
    // 错误必须来自绑定来源而不是 adapter。
    assert_eq!(source_error.code, "INVALID_ARGUMENT");
    // 字段必须精确定位来源索引。
    assert_eq!(
        // 读取公开字段路径。
        source_error.details["field"],
        // 对比零基步骤和绑定位置。
        "steps[0].bindings[0].sourceStep"
    );

    // 构造第二步非法引用当前步骤的绑定。
    let forward_reference: SequenceInput = serde_json::from_value(json!({
        // 第一项使用未知 app 证明全量预验证先发生。
        "steps": [
            // 首步若执行会产生不同错误。
            { "verb": "status", "app": "never-execute" },
            // 第二步非法引用一基索引二。
            {
                // 使用只读状态动词。
                "verb": "status",
                // 使用确定性 desktop adapter。
                "app": "desktop",
                // 预先声明目标字段。
                "target": { "slot": null },
                // sourceStep 必须严格小于当前步骤。
                "bindings": [{
                    // 非法引用当前第二步。
                    "sourceStep": 2,
                    // 使用合法根 Pointer。
                    "sourcePointer": "",
                    // 写入 target。
                    "destination": "target",
                    // 替换已声明 slot。
                    "destinationField": "slot"
                }]
            }
        ]
    }))?;
    // 调用 System 前置验证并取得错误。
    let forward_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(forward_reference)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("forward binding unexpectedly succeeded"))?;
    // 第二步允许的最大来源索引只能为一。
    assert_eq!(forward_error.details["maximum"], 1);

    // 构造超过单步骤硬上限的绑定列表。
    let too_many_bindings = (0..=MAX_SEQUENCE_BINDINGS)
        // 每项使用合法来源与互不相关的目标字段名。
        .map(|index| {
            // 构造一个绑定对象。
            json!({
                // 引用第一步。
                "sourceStep": 1,
                // 使用合法根 Pointer。
                "sourcePointer": "",
                // 写入 target。
                "destination": "target",
                // 目标字段名随索引变化。
                "destinationField": format!("slot-{index}")
            })
        })
        // 收集为严格输入数组。
        .collect::<Vec<_>>();
    // 构造与绑定字段相同数量的目标模板。
    let target = (0..=MAX_SEQUENCE_BINDINGS)
        // 产生字段名和值。
        .map(|index| (format!("slot-{index}"), Value::Null))
        // 收集为 JSON 对象 Map。
        .collect::<serde_json::Map<_, _>>();
    // 首步未知 app 证明数量验证发生在 provider 前。
    let too_many: SequenceInput = serde_json::from_value(json!({
        // 第二步携带超上限绑定。
        "steps": [
            // 首步不应执行。
            { "verb": "status", "app": "never-execute" },
            // 第二步声明完整目标模板和过多绑定。
            { "verb": "status", "app": "desktop", "target": target, "bindings": too_many_bindings }
        ]
    }))?;
    // 调用 System 前置验证并取得错误。
    let count_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(too_many)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("too many bindings unexpectedly succeeded"))?;
    // 错误字段必须定位绑定数组。
    assert_eq!(count_error.details["field"], "steps[1].bindings");

    // 构造非法 RFC 6901 转义。
    let invalid_pointer: SequenceInput = serde_json::from_value(json!({
        // 首步未知 app 证明全量预验证。
        "steps": [
            // 首步不应执行。
            { "verb": "status", "app": "never-execute" },
            // 第二步携带非法来源 Pointer。
            {
                // 使用只读状态动词。
                "verb": "status",
                // 使用确定性 desktop adapter。
                "app": "desktop",
                // 预先声明目标字段。
                "target": { "slot": null },
                // 使用未知 `~2` 转义。
                "bindings": [{
                    // 引用第一步。
                    "sourceStep": 1,
                    // 注入非法 Pointer。
                    "sourcePointer": "/bad~2escape",
                    // 写入 target。
                    "destination": "target",
                    // 替换已声明 slot。
                    "destinationField": "slot"
                }]
            }
        ]
    }))?;
    // 调用 System 前置验证并取得错误。
    let pointer_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(invalid_pointer)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("invalid binding pointer unexpectedly succeeded"))?;
    // 字段必须精确定位来源 Pointer。
    assert_eq!(
        // 读取公开字段路径。
        pointer_error.details["field"],
        // 对比零基步骤和绑定索引。
        "steps[1].bindings[0].sourcePointer"
    );

    // 构造 UTF-8 字节数超过 Pointer 上限的多字节字符串。
    let oversized_pointer = "界".repeat(MAX_SEQUENCE_BINDING_POINTER_BYTES);
    // 构造带超长来源 Pointer 的输入。
    let oversized_pointer_input: SequenceInput = serde_json::from_value(json!({
        // 首步未知 app 证明全量预验证。
        "steps": [
            // 首步不应执行。
            { "verb": "status", "app": "never-execute" },
            // 第二步携带超长来源 Pointer。
            {
                // 使用只读状态动词。
                "verb": "status",
                // 使用确定性 desktop adapter。
                "app": "desktop",
                // 预先声明目标字段。
                "target": { "slot": null },
                // 注入超长来源 Pointer。
                "bindings": [{
                    // 引用第一步。
                    "sourceStep": 1,
                    // 使用多字节超长 Pointer。
                    "sourcePointer": oversized_pointer,
                    // 写入 target。
                    "destination": "target",
                    // 替换已声明 slot。
                    "destinationField": "slot"
                }]
            }
        ]
    }))?;
    // 调用 System 前置验证并取得错误。
    let oversized_pointer_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(oversized_pointer_input)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("oversized binding pointer unexpectedly succeeded"))?;
    // 证据必须报告真实 UTF-8 字节上限。
    assert_eq!(
        // 读取公开上限。
        oversized_pointer_error.details["maximumBytes"],
        // 对比 Rust 常量。
        MAX_SEQUENCE_BINDING_POINTER_BYTES
    );

    // 构造引用未声明目标字段的绑定。
    let missing_destination: SequenceInput = serde_json::from_value(json!({
        // 首步未知 app 证明全量预验证。
        "steps": [
            // 首步不应执行。
            { "verb": "status", "app": "never-execute" },
            // 第二步没有 target.slot。
            {
                // 使用只读状态动词。
                "verb": "status",
                // 使用确定性 desktop adapter。
                "app": "desktop",
                // 绑定试图隐式创建字段。
                "bindings": [{
                    // 引用第一步。
                    "sourceStep": 1,
                    // 使用合法根 Pointer。
                    "sourcePointer": "",
                    // 写入 target。
                    "destination": "target",
                    // 目标字段并不存在。
                    "destinationField": "slot"
                }]
            }
        ]
    }))?;
    // 调用 System 前置验证并取得错误。
    let destination_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(missing_destination)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| {
            std::io::Error::other("missing binding destination unexpectedly succeeded")
        })?;
    // 错误必须来自目标声明而不是 adapter。
    assert_eq!(destination_error.code, "INVALID_ARGUMENT");
    // 证据报告 target 区域。
    assert_eq!(destination_error.details["destination"], "target");

    // 构造两个绑定写入同一目标字段。
    let duplicate_destination: SequenceInput = serde_json::from_value(json!({
        // 首步未知 app 证明全量预验证。
        "steps": [
            // 首步不应执行。
            { "verb": "status", "app": "never-execute" },
            // 第二步携带重复目标。
            {
                // 使用只读状态动词。
                "verb": "status",
                // 使用确定性 desktop adapter。
                "app": "desktop",
                // 预先声明目标字段。
                "args": { "slot": null },
                // 两项都写入 args.slot。
                "bindings": [
                    // 第一项绑定。
                    { "sourceStep": 1, "sourcePointer": "/a", "destination": "args", "destinationField": "slot" },
                    // 第二项重复绑定。
                    { "sourceStep": 1, "sourcePointer": "/b", "destination": "args", "destinationField": "slot" }
                ]
            }
        ]
    }))?;
    // 调用 System 前置验证并取得错误。
    let duplicate_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(duplicate_destination)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("duplicate binding unexpectedly succeeded"))?;
    // 后出现的重复字段必须精确定位。
    assert_eq!(
        // 读取公开字段路径。
        duplicate_error.details["field"],
        // 对比第二项绑定位置。
        "steps[1].bindings[1].destinationField"
    );

    // 构造 UTF-8 字节数超过目标字段上限的名称。
    let oversized_field = "界".repeat(MAX_SEQUENCE_BINDING_FIELD_BYTES);
    // 构造带超长字段名的 target 模板。
    let oversized_target = serde_json::Map::from_iter([(oversized_field.clone(), Value::Null)]);
    // 构造超长目标字段绑定。
    let field_input: SequenceInput = serde_json::from_value(json!({
        // 首步未知 app 证明全量预验证。
        "steps": [
            // 首步不应执行。
            { "verb": "status", "app": "never-execute" },
            // 第二步携带超长字段名。
            {
                // 使用只读状态动词。
                "verb": "status",
                // 使用确定性 desktop adapter。
                "app": "desktop",
                // 预先声明同名目标字段。
                "target": oversized_target,
                // 引用超长目标字段。
                "bindings": [{
                    // 引用第一步。
                    "sourceStep": 1,
                    // 使用合法根 Pointer。
                    "sourcePointer": "",
                    // 写入 target。
                    "destination": "target",
                    // 注入超长字段名。
                    "destinationField": oversized_field
                }]
            }
        ]
    }))?;
    // 调用 System 前置验证并取得错误。
    let field_error = AppControlService::new()
        // 执行 sequence 公开入口。
        .sequence(field_input)
        // 只接受失败结果。
        .err()
        // 转换意外成功为测试错误。
        .ok_or_else(|| std::io::Error::other("oversized binding field unexpectedly succeeded"))?;
    // 字段必须精确定位目标字段名。
    assert_eq!(
        // 读取公开字段路径。
        field_error.details["field"],
        // 对比零基步骤和绑定索引。
        "steps[1].bindings[0].destinationField"
    );
    // 测试正常完成。
    Ok(())
}

// 验证绑定成功后步骤正常执行并返回应用证据。
#[test]
fn bindings_apply_before_unified_request_execution() -> Result<(), Box<dyn std::error::Error>> {
    // 构造两个无副作用的 desktop 状态步骤。
    let input: SequenceInput = serde_json::from_value(json!({
        // 第二步从第一步结果绑定两个值。
        "steps": [status_step("source"), {
            // 使用可关联名称。
            "name": "bound-read",
            // 使用只读状态动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // 预先声明 target 顶层字段。
            "target": { "boundApp": null },
            // 预先声明 args 顶层字段。
            "args": { "boundRealm": null },
            // 声明两个互不重复的绑定。
            "bindings": [
                // 把来源 app 绑定到 target。
                { "sourceStep": 1, "sourcePointer": "/app", "destination": "target", "destinationField": "boundApp" },
                // 把来源执行域绑定到 args。
                { "sourceStep": 1, "sourcePointer": "/executionRealm", "destination": "args", "destinationField": "boundRealm" }
            ]
        }]
    }))?;
    // 执行只读 Workflow。
    let result = AppControlService::new().sequence(input)?;
    // 两个 provider 步骤都成功完成。
    assert_eq!(result["count"], 2);
    // 整体工作流成功。
    assert_eq!(result["ok"], true);
    // 第二步报告应用两个绑定。
    assert_eq!(result["results"][1]["bindings"]["applied"], 2);
    // 第二步仍由统一 Policy 附加执行域证据。
    assert_eq!(
        result["results"][1]["result"]["executionRealm"],
        "host-headless"
    );
    // 测试正常完成。
    Ok(())
}

// 验证嵌套 Pointer 可创建叶字段且最终请求仍进入统一 Policy。
#[test]
fn nested_pointer_binding_applies_before_unified_policy() -> Result<(), Box<dyn std::error::Error>>
{
    // 构造两个无副作用的 desktop 状态步骤。
    let input: SequenceInput = serde_json::from_value(json!({
        // 第二步把第一步 app 写入缺失的嵌套叶字段。
        "steps": [status_step("source"), {
            // 使用可关联名称。
            "name": "nested-bound-read",
            // 使用只读状态动词。
            "verb": "status",
            // 使用确定性 desktop adapter。
            "app": "desktop",
            // 静态声明全部父对象但不声明最终叶字段。
            "args": { "options": { "derived": {} } },
            // 使用新 Pointer 形状创建最终叶字段。
            "bindings": [{
                // 引用第一步。
                "sourceStep": 1,
                // 读取来源 app。
                "sourcePointer": "/app",
                // 写入 args 区域。
                "destination": "args",
                // 创建嵌套 leaf 字段。
                "destinationPointer": "/options/derived/app"
            }]
        }]
    }))?;
    // 执行只读 Workflow。
    let result = AppControlService::new().sequence(input)?;
    // 两个步骤都必须完成。
    assert_eq!(result["count"], 2);
    // Workflow 必须保持成功。
    assert_eq!(result["ok"], true);
    // 新 Pointer 绑定必须计入应用证据。
    assert_eq!(result["results"][1]["bindings"]["applied"], 1);
    // 统一 Policy 仍附加 host-headless 执行域。
    assert_eq!(
        // 读取第二步 provider 结果。
        result["results"][1]["result"]["executionRealm"],
        // 对比统一 Policy 的只读执行域。
        "host-headless"
    );
    // 测试正常完成。
    Ok(())
}

// 验证新目标形状、父对象和路径重叠在任何 provider 前失败闭合。
#[test]
fn nested_pointer_structure_is_validated_before_provider_execution()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造覆盖二选一、非根、父对象和规范重叠的非法步骤。
    let cases = vec![
        // 两种目标形状不能同时声明。
        (
            "both-shapes",
            json!({
                "verb": "status", "app": "desktop", "args": { "slot": null },
                "bindings": [{ "sourceStep": 1, "sourcePointer": "/app", "destination": "args", "destinationField": "slot", "destinationPointer": "/slot" }]
            }),
            "steps[1].bindings[0]",
        ),
        // 两种目标形状不能同时缺失。
        (
            "missing-shape",
            json!({
                "verb": "status", "app": "desktop",
                "bindings": [{ "sourceStep": 1, "sourcePointer": "/app", "destination": "args" }]
            }),
            "steps[1].bindings[0]",
        ),
        // 根 Pointer 不能替换整个 target 或 args 对象。
        (
            "root-pointer",
            json!({
                "verb": "status", "app": "desktop",
                "bindings": [{ "sourceStep": 1, "sourcePointer": "/app", "destination": "args", "destinationPointer": "" }]
            }),
            "steps[1].bindings[0].destinationPointer",
        ),
        // 缺失父对象不能被递归创建。
        (
            "missing-parent",
            json!({
                "verb": "status", "app": "desktop",
                "bindings": [{ "sourceStep": 1, "sourcePointer": "/app", "destination": "args", "destinationPointer": "/config/value" }]
            }),
            "steps[1].bindings[0].destinationPointer",
        ),
        // 数组父值不能被当作对象字段容器。
        (
            "array-parent",
            json!({
                "verb": "status", "app": "desktop", "args": { "config": [] },
                "bindings": [{ "sourceStep": 1, "sourcePointer": "/app", "destination": "args", "destinationPointer": "/config/0" }]
            }),
            "steps[1].bindings[0].destinationPointer",
        ),
        // 旧字段与转义 Pointer 的规范别名不能重复写入。
        (
            "escaped-alias",
            json!({
                "verb": "status", "app": "desktop", "args": { "a/b": null },
                "bindings": [
                    { "sourceStep": 1, "sourcePointer": "/app", "destination": "args", "destinationField": "a/b" },
                    { "sourceStep": 1, "sourcePointer": "/executionRealm", "destination": "args", "destinationPointer": "/a~1b" }
                ]
            }),
            "steps[1].bindings[1].destinationPointer",
        ),
        // 祖先与后代路径不能依赖声明顺序相互覆盖。
        (
            "ancestor-overlap",
            json!({
                "verb": "status", "app": "desktop", "args": { "config": {} },
                "bindings": [
                    { "sourceStep": 1, "sourcePointer": "/app", "destination": "args", "destinationPointer": "/config" },
                    { "sourceStep": 1, "sourcePointer": "/executionRealm", "destination": "args", "destinationPointer": "/config/value" }
                ]
            }),
            "steps[1].bindings[1].destinationPointer",
        ),
    ];
    // 逐项证明全量 Workflow 预验证先于首个 provider。
    for (name, step, expected_field) in cases {
        // 解析严格输入，目标结构错误由 System 返回稳定证据。
        let input: SequenceInput = serde_json::from_value(json!({
            // 首步未知 app，若执行就会产生不同错误。
            "steps": [{ "verb": "status", "app": "never-execute" }, step]
        }))?;
        // 执行公开入口并只接受参数错误。
        let error = AppControlService::new()
            // 调用 sequence Workflow。
            .sequence(input)
            // 取得失败分支。
            .err()
            // 意外成功时保留用例名称。
            .ok_or_else(|| std::io::Error::other(format!("{name} unexpectedly succeeded")))?;
        // 全部结构错误都必须使用统一参数错误码。
        assert_eq!(error.code, "INVALID_ARGUMENT", "{name}");
        // 错误字段必须精确指向当前目标声明。
        assert_eq!(error.details["field"], expected_field, "{name}");
    }
    // 测试正常完成。
    Ok(())
}

// 验证绑定后的 run 请求仍经过现有确认门禁。
#[test]
fn bound_run_request_does_not_bypass_confirmation_policy() -> Result<(), Box<dyn std::error::Error>>
{
    // 构造首步只读、第二步未确认截图请求。
    let input: SequenceInput = serde_json::from_value(json!({
        // 第二步不会通过确认门禁，因此不会产生截图。
        "steps": [status_step("source"), {
            // 使用统一 run 动词。
            "verb": "run",
            // 使用 desktop surface。
            "app": "desktop",
            // 使用需要确认的截图操作。
            "operation": "screenshot",
            // 预先声明 sessionId 占位字段。
            "target": { "sessionId": null },
            // 把来源 app 文本绑定到 sessionId。
            "bindings": [{
                // 引用第一步。
                "sourceStep": 1,
                // 读取来源 app。
                "sourcePointer": "/app",
                // 写入 target。
                "destination": "target",
                // 替换 sessionId 占位字段。
                "destinationField": "sessionId"
            }],
            // 显式保持未确认。
            "confirmed": false
        }]
    }))?;
    // 执行 Workflow 并取得结构化步骤失败。
    let result = AppControlService::new().sequence(input)?;
    // 第二个统一请求已进入 Policy 并作为失败步骤记录。
    assert_eq!(result["count"], 2);
    // 真实执行失败数为一。
    assert_eq!(result["failedCount"], 1);
    // 绑定在 Policy 前已经应用。
    assert_eq!(result["results"][1]["bindings"]["applied"], 1);
    // 现有确认门禁仍返回稳定错误。
    assert_eq!(
        result["results"][1]["error"]["code"],
        "CONFIRMATION_REQUIRED"
    );
    // 测试正常完成。
    Ok(())
}

// 验证运行时来源缺口在当前 provider 启动前硬停止。
#[test]
fn binding_failure_stops_before_current_provider() -> Result<(), Box<dyn std::error::Error>> {
    // 构造来源成功但 Pointer 缺失的输入。
    let missing_pointer: SequenceInput = serde_json::from_value(json!({
        // 第二步若绕过绑定会产生未知应用错误。
        "steps": [status_step("source"), {
            // 使用状态读取动词。
            "verb": "status",
            // 使用不应被解析的未知 app。
            "app": "never-execute",
            // 预先声明目标字段。
            "target": { "slot": null },
            // 指向来源结果中不存在字段。
            "bindings": [{
                // 引用第一步。
                "sourceStep": 1,
                // 指向缺失字段。
                "sourcePointer": "/missing",
                // 写入 target。
                "destination": "target",
                // 替换已声明 slot。
                "destinationField": "slot"
            }]
        }],
        // 普通 provider 错误继续策略不能放宽绑定失败。
        "continueOnError": true
    }))?;
    // 执行只读 Workflow。
    let missing_result = AppControlService::new().sequence(missing_pointer)?;
    // 只有第一步 provider 实际完成。
    assert_eq!(missing_result["count"], 1);
    // provider 失败数保持零。
    assert_eq!(missing_result["failedCount"], 0);
    // 绑定失败计为唯一 Workflow 错误。
    assert_eq!(missing_result["workflowErrorCount"], 1);
    // 顶层使用稳定绑定错误码。
    assert_eq!(
        missing_result["workflowError"]["code"],
        "SEQUENCE_BINDING_FAILED"
    );
    // 当前 provider 明确未启动。
    assert_eq!(
        // 读取启动事实。
        missing_result["workflowError"]["details"]["stepStarted"],
        // 期望未启动。
        false
    );
    // 来源 Pointer 缺失使用封闭原因。
    assert_eq!(
        // 读取失败原因。
        missing_result["workflowError"]["details"]["reason"],
        // 对比稳定原因。
        "source-pointer-missing"
    );

    // 构造首步 provider 失败且允许继续的输入。
    let failed_source: SequenceInput = serde_json::from_value(json!({
        // 第二步尝试绑定失败来源。
        "steps": [
            // 首步产生确定性未知应用错误。
            { "verb": "status", "app": "unknown-source" },
            // 第二步若绕过绑定也会产生未知应用错误。
            {
                // 使用状态读取动词。
                "verb": "status",
                // 使用不应被解析的未知 app。
                "app": "never-execute",
                // 预先声明 args 字段。
                "args": { "slot": null },
                // 尝试读取失败来源的根。
                "bindings": [{
                    // 引用第一步。
                    "sourceStep": 1,
                    // 使用根 Pointer。
                    "sourcePointer": "",
                    // 写入 args。
                    "destination": "args",
                    // 替换已声明 slot。
                    "destinationField": "slot"
                }]
            }
        ],
        // 允许普通 provider 错误后进入绑定阶段。
        "continueOnError": true
    }))?;
    // 执行 Workflow。
    let failed_result = AppControlService::new().sequence(failed_source)?;
    // 只有失败来源 provider 实际完成。
    assert_eq!(failed_result["count"], 1);
    // 保留真实 provider 失败数一。
    assert_eq!(failed_result["failedCount"], 1);
    // 来源失败使用独立封闭原因。
    assert_eq!(
        // 读取失败原因。
        failed_result["workflowError"]["details"]["reason"],
        // 对比稳定原因。
        "source-step-failed"
    );
    // 测试正常完成。
    Ok(())
}

// 验证公开 schema 与 Rust 绑定硬边界保持一致。
#[test]
fn binding_schema_matches_runtime_bounds() -> Result<(), Box<dyn std::error::Error>> {
    // 解析仓库内版本化输入 schema。
    let schema: Value = serde_json::from_str(include_str!(
        // 使用编译期路径防止测试依赖工作目录。
        "../contracts/v1/sequence-input.schema.json"
    ))?;
    // binding 对象必须拒绝未知字段。
    assert_eq!(schema["$defs"]["binding"]["additionalProperties"], false);
    // 核对单步骤绑定数量上限。
    assert_eq!(
        // 读取 bindings 数组上限。
        schema["$defs"]["step"]["properties"]["bindings"]["maxItems"],
        // 对比 Rust 常量。
        MAX_SEQUENCE_BINDINGS
    );
    // 核对来源 Pointer 数值上限。
    assert_eq!(
        // 读取 schema Pointer 上限。
        schema["$defs"]["binding"]["properties"]["sourcePointer"]["maxLength"],
        // 对比 Rust 字节硬上限。
        MAX_SEQUENCE_BINDING_POINTER_BYTES
    );
    // 核对目标字段数值上限。
    assert_eq!(
        // 读取 schema 字段上限。
        schema["$defs"]["binding"]["properties"]["destinationField"]["maxLength"],
        // 对比 Rust 字节硬上限。
        MAX_SEQUENCE_BINDING_FIELD_BYTES
    );
    // 核对嵌套目标 Pointer 数值上限。
    assert_eq!(
        // 读取 schema 新 Pointer 上限。
        schema["$defs"]["binding"]["properties"]["destinationPointer"]["maxLength"],
        // 与来源 Pointer 共享同一 Rust 字节硬上限。
        MAX_SEQUENCE_BINDING_POINTER_BYTES
    );
    // 目标形状必须在第二个 allOf 分支通过 oneOf 表达恰好二选一。
    assert_eq!(
        // 读取目标形状 oneOf 数量。
        schema["$defs"]["binding"]["allOf"][1]["oneOf"]
            // 转换为数组引用。
            .as_array()
            // 读取分支数量。
            .map(Vec::len),
        // 旧字段和新 Pointer 两种目标形状。
        Some(2)
    );
    // 单值运行时字节上限保持明确且非零。
    assert_eq!(MAX_SEQUENCE_BOUND_VALUE_BYTES, 4_096);
    // 测试正常完成。
    Ok(())
}
