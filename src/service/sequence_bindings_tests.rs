// 导入待测绑定契约与应用函数。
use super::{
    SequenceBinding, SequenceBindingDestination, apply_sequence_bindings,
    validate_sequence_bindings,
};
// 导入统一请求和控制字段类型。
use crate::domain::{CommandRequest, IsolationRequirement, Verb};
// 导入 JSON 构造宏。
use serde_json::json;

// 验证绑定精确替换 target/args 且保持控制字段不变。
#[test]
fn bindings_only_replace_declared_request_maps() -> Result<(), Box<dyn std::error::Error>> {
    // 构造一个已完成且成功的来源步骤记录。
    let results = vec![json!({
        // 保持真实 provider 成功事实。
        "ok": true,
        // 提供嵌套来源值。
        "result": { "session": { "id": "s2:w:opaque" }, "limit": 7 },
    })];
    // 构造带显式控制事实的最终请求。
    let mut request = CommandRequest::read(Verb::Run, "window");
    // 保存不可由绑定修改的 operation。
    request.operation = Some("close".to_owned());
    // 保存不可由绑定授予的确认事实。
    request.confirmed = true;
    // 保存不可由绑定授予的前台同意。
    request.foreground_consent = true;
    // 保存不可由绑定放宽的隔离要求。
    request.isolation_requirement = IsolationRequirement::Strict;
    // 预先声明 target 顶层占位字段。
    request.target.insert("sessionId".to_owned(), json!(null));
    // 预先声明 args 顶层占位字段。
    request.args.insert("limit".to_owned(), json!(0));
    // 构造两个互不重复的绑定。
    let bindings = vec![
        // 把嵌套 session ID 绑定到 target。
        SequenceBinding {
            // 引用第一步。
            source_step: Some(1),
            // 读取嵌套 ID。
            source_pointer: Some("/session/id".to_owned()),
            // 不使用结构化模板来源。
            template: None,
            // 写入 target 区域。
            destination: SequenceBindingDestination::Target,
            // 替换已声明字段。
            destination_field: Some("sessionId".to_owned()),
            // 不使用新嵌套 Pointer 形状。
            destination_pointer: None,
        },
        // 把数值绑定到 args。
        SequenceBinding {
            // 引用第一步。
            source_step: Some(1),
            // 读取数值字段。
            source_pointer: Some("/limit".to_owned()),
            // 不使用结构化模板来源。
            template: None,
            // 写入 args 区域。
            destination: SequenceBindingDestination::Args,
            // 替换已声明字段。
            destination_field: Some("limit".to_owned()),
            // 不使用新嵌套 Pointer 形状。
            destination_pointer: None,
        },
    ];
    // 结构边界验证必须接受两个显式目标。
    validate_sequence_bindings(1, request.verb, &request.target, &request.args, &bindings)?;
    // 应用绑定并取得成功证据。
    let evaluation = apply_sequence_bindings(&results, &mut request, &bindings);
    // 不得产生绑定失败。
    assert!(evaluation.failure.is_none());
    // 绑定数量证据必须为二。
    assert_eq!(evaluation.evidence, Some(json!({ "applied": 2 })));
    // target 字段被精确替换。
    assert_eq!(request.target["sessionId"], "s2:w:opaque");
    // args 字段被精确替换且保持 JSON 数字类型。
    assert_eq!(request.args["limit"], 7);
    // verb 不得被绑定修改。
    assert_eq!(request.verb, Verb::Run);
    // app 不得被绑定修改。
    assert_eq!(request.app, "window");
    // operation 不得被绑定修改。
    assert_eq!(request.operation.as_deref(), Some("close"));
    // 确认事实不得被绑定修改。
    assert!(request.confirmed);
    // 前台同意不得被绑定修改。
    assert!(request.foreground_consent);
    // 隔离要求不得被绑定放宽。
    assert_eq!(request.isolation_requirement, IsolationRequirement::Strict);
    // 测试正常完成。
    Ok(())
}

// 验证超大来源值在目标写入和 provider 启动前失败。
#[test]
fn oversized_bound_value_is_rejected_before_request_mutation()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造紧凑 JSON 字节数超过单值上限的来源字符串。
    let oversized = "x".repeat(super::MAX_SEQUENCE_BOUND_VALUE_BYTES);
    // 构造一个成功来源步骤记录。
    let results = vec![json!({
        // 保持真实 provider 成功事实。
        "ok": true,
        // 提供超大来源值。
        "result": { "value": oversized },
    })];
    // 构造只读最终请求。
    let mut request = CommandRequest::read(Verb::Status, "desktop");
    // 预先声明不会被失败绑定改写的占位字段。
    request.args.insert("slot".to_owned(), json!("original"));
    // 构造读取超大值的单一绑定。
    let bindings = vec![SequenceBinding {
        // 引用第一步。
        source_step: Some(1),
        // 读取超大值字段。
        source_pointer: Some("/value".to_owned()),
        // 不使用结构化模板来源。
        template: None,
        // 写入 args 区域。
        destination: SequenceBindingDestination::Args,
        // 替换已声明 slot。
        destination_field: Some("slot".to_owned()),
        // 不使用新嵌套 Pointer 形状。
        destination_pointer: None,
    }];
    // 应用绑定并取得失败证据。
    let evaluation = apply_sequence_bindings(&results, &mut request, &bindings);
    // 绑定不得产生成功证据。
    assert!(evaluation.evidence.is_none());
    // 取得首个失败定位。
    let failure = evaluation
        // 读取失败。
        .failure
        // 缺失失败时让测试明确报错。
        .ok_or_else(|| std::io::Error::other("oversized binding unexpectedly succeeded"))?;
    // 使用稳定超限原因。
    assert_eq!(failure.reason, "bound-value-too-large");
    // 实际字节数必须超过公开上限。
    assert!(failure.attempted_bytes > Some(super::MAX_SEQUENCE_BOUND_VALUE_BYTES));
    // 目标字段保持原值，证明失败发生在写入前。
    assert_eq!(request.args["slot"], "original");
    // 测试正常完成。
    Ok(())
}

// 验证新 Pointer 形状解码嵌套对象路径并创建最终叶字段。
#[test]
fn pointer_binding_creates_nested_leaf_with_rfc6901_decoding()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造包含待绑定标量的成功来源步骤。
    let results = vec![json!({
        // 保持真实 provider 成功事实。
        "ok": true,
        // 提供待写入的数值。
        "result": { "value": 42 },
    })];
    // 构造只读候选请求。
    let mut request = CommandRequest::read(Verb::Status, "desktop");
    // 静态声明包含斜杠与波浪号字段的全部父对象。
    request.args = serde_json::from_value(json!({
        // 提供第一层父对象。
        "options": {
            // 该字段由 `~1` 解码得到。
            "a/b": {
                // 该字段由 `~0` 解码得到。
                "~key": {}
            }
        }
    }))?;
    // 构造允许创建最终 value 叶字段的嵌套绑定。
    let bindings = vec![SequenceBinding {
        // 引用第一步。
        source_step: Some(1),
        // 读取来源数值。
        source_pointer: Some("/value".to_owned()),
        // 不使用结构化模板来源。
        template: None,
        // 写入 args 区域。
        destination: SequenceBindingDestination::Args,
        // 不使用旧顶层字段形状。
        destination_field: None,
        // 使用非根嵌套目标 Pointer。
        destination_pointer: Some("/options/a~1b/~0key/value".to_owned()),
    }];
    // 序列化新形状时不得注入与 schema 冲突的旧 null 字段。
    let serialized_binding = serde_json::to_value(&bindings[0])?;
    // 新 Pointer 必须保持逐字公开形状。
    assert_eq!(
        serialized_binding["destinationPointer"],
        "/options/a~1b/~0key/value"
    );
    // 未选择的旧字段必须完全省略而不是序列化为 null。
    assert!(serialized_binding.get("destinationField").is_none());
    // 结构验证必须接受静态对象父路径和缺失叶字段。
    validate_sequence_bindings(1, request.verb, &request.target, &request.args, &bindings)?;
    // 应用绑定并取得成功证据。
    let evaluation = apply_sequence_bindings(&results, &mut request, &bindings);
    // 不得产生运行时目标失败。
    assert!(evaluation.failure.is_none());
    // 应用数量必须为一。
    assert_eq!(evaluation.evidence, Some(json!({ "applied": 1 })));
    // 解码后的嵌套路径必须包含新建叶字段和值。
    assert_eq!(request.args["options"]["a/b"]["~key"]["value"], 42);
    // 测试正常完成。
    Ok(())
}
