// 导入 JSON 构造宏。
use serde_json::json;

// 导入被测 Module 私有函数。
use super::*;

// 构造最小合法输入。
fn input() -> Value {
    // 返回严格 provider-neutral 对象。
    json!({
        // 使用单一精确 selector。
        "selector": { "automationId": "ready" },
        // 使用 Invoke 动作。
        "action": { "type": "invoke" },
        // 使用固定边界。
        "maximumDepth": 8,
        // 使用固定数量边界。
        "maximumItems": 1024,
        // 使用 control view。
        "view": "control",
        // 使用默认 deadline 值。
        "timeoutMs": 2000,
    })
}

// 验证输入拒绝原生与指针字段。
#[test]
fn public_input_rejects_native_and_pointer_fields() {
    // 合法输入必须解析。
    assert!(parse_input(&input()).is_ok());
    // 注入原生句柄。
    let mut native = input();
    // 测试夹具固定为对象。
    native
        // 取得对象。
        .as_object_mut()
        // 对象夹具必须存在。
        .unwrap_or_else(|| panic!("semantic action input fixture must be an object"))
        // 注入禁止字段。
        .insert("hwnd".to_owned(), Value::from(42));
    // 原生字段必须在 provider 前拒绝。
    assert_eq!(
        // 读取错误码。
        parse_input(&native).err().map(|error| error.code),
        // 使用稳定参数错误。
        Some("INVALID_ARGUMENT")
    );
    // 在 action 内注入屏幕坐标。
    let pointer = json!({
        // 使用合法 selector。
        "selector": { "name": "ready" },
        // Invoke 不接受坐标。
        "action": { "type": "invoke", "x": 10, "y": 10 },
    });
    // 禁止静默指针路由。
    assert_eq!(
        // 读取错误码。
        parse_input(&pointer).err().map(|error| error.code),
        // 使用稳定参数错误。
        Some("INVALID_ARGUMENT")
    );
}

// 验证未确认请求在任何 target 或输入解析前失败。
#[test]
fn confirmation_precedes_input_and_target_resolution() {
    // 使用故意无效 target 与输入。
    let error = perform("not-a-target", false, &Value::Null)
        // 未确认必须失败。
        .err()
        // 测试必须取得错误。
        .unwrap_or_else(|| panic!("unconfirmed semantic action must fail"));
    // 确认错误优先。
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

// 验证 parent worker 边界失败保守禁止重试。
#[test]
fn parent_unknown_outcome_is_never_retry_safe() {
    // 构造 parent 不确定结果。
    let error = parent_outcome_unknown("toggle", Some(true));
    // 保持稳定错误码。
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    // 保守承认动作可能发生。
    assert_eq!(error.details["acceptedMayHaveOccurred"], true);
    // 禁止自动重试。
    assert_eq!(error.details["automaticRetryProhibited"], true);
    // 明确无重试证明。
    assert_eq!(error.details["retrySafe"], false);
    // 不得静默指针降级。
    assert_eq!(error.details["pointerFallbackUsed"], false);
}

// 验证成功 data 只接受固定不重试形状。
#[test]
fn success_data_rejects_retry_or_native_drift() {
    // 解析合法输入。
    let input = parse_input(&input())
        // 测试夹具必须有效。
        .unwrap_or_else(|error| panic!("semantic action input failed: {error}"));
    // 构造合法 worker data。
    let data = json!({
        // 回显动作。
        "action": "invoke",
        // 完成结果。
        "outcome": "completed",
        // 完成 dispatch。
        "dispatchState": "completed",
        // mutation 已接受。
        "acceptedMayHaveOccurred": true,
        // 禁止自动重试。
        "automaticRetryProhibited": true,
        // 不可安全重试。
        "retrySafe": false,
        // 窗口已重新解析。
        "windowReResolved": true,
        // 元素已重新解析。
        "elementReResolved": true,
        // 精确 AND selector。
        "selectorSemantics": "exact-and",
        // 有界访问数。
        "visited": 2,
        // 无指针 fallback。
        "pointerFallbackUsed": false,
        // 执行一次写调用。
        "writePerformed": true,
    });
    // 合法 data 必须通过。
    assert!(success_data(&data, &input).is_ok());
    // 注入 native provider 字段。
    let mut native = data;
    // 测试夹具固定为对象。
    native
        // 取得对象。
        .as_object_mut()
        // 对象必须存在。
        .unwrap_or_else(|| panic!("worker data fixture must be an object"))
        // 注入禁止字段。
        .insert("patternId".to_owned(), Value::from(10_000));
    // 原生字段必须拒绝。
    assert_eq!(
        // 读取错误码。
        success_data(&native, &input).err().map(|error| error.code),
        // 使用内部协议失败。
        Some("WORKER_PROTOCOL_FAILED")
    );
}
