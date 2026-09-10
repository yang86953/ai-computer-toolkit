// 导入 JSON 构造宏。
use serde_json::json;

// 导入被测 worker 私有函数。
use super::*;

// 构造最小合法 worker 请求。
fn request(action: Value) -> String {
    // 序列化固定协议夹具。
    serde_json::to_string(&json!({
        // 使用固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用唯一 operation。
        "operation": OPERATION,
        // 使用 canonical 窗口 ID 形状。
        "sessionId": "s2:w:0000000000000000",
        // 使用最小 selector。
        "selector": { "automationId": "ready" },
        // 注入封闭动作。
        "action": action,
        // 只搜索 root。
        "maximumDepth": 0,
        // 只访问一个节点。
        "maximumItems": 1,
        // 使用 control view。
        "view": "control",
    }))
    // 测试 JSON 必须可序列化。
    .unwrap_or_else(|error| panic!("worker request fixture failed: {error}"))
}

// 验证协议接受五种动作且拒绝 snapshot element ID。
#[test]
fn worker_protocol_accepts_five_actions_and_only_window_targets() {
    // 固定五种动作。
    let actions = [
        // 调用动作。
        json!({ "type": "invoke" }),
        // Value 动作。
        json!({ "type": "value", "value": "text" }),
        // Toggle 动作。
        json!({ "type": "toggle" }),
        // Select 动作。
        json!({ "type": "select" }),
        // Scroll 动作。
        json!({ "type": "scroll", "vertical": "small-increment" }),
    ];
    // 逐项核对协议解析。
    for action in actions {
        // 每种封闭动作都应解析成功。
        assert!(parse_request(&request(action)).is_ok());
    }
    // 把 canonical 窗口类别改成 snapshot element 类别。
    let element_target = request(json!({ "type": "invoke" }))
        // 仅替换类别，不改变摘要形状。
        .replace("s2:w:", "s2:e:");
    // snapshot ID 不能成为写目标。
    assert_eq!(
        // 读取稳定错误码。
        parse_request(&element_target).err().map(|error| error.code),
        // 必须在 provider 前拒绝。
        Some("INVALID_ARGUMENT")
    );
}

// 验证写调用后异常固定禁止自动重试。
#[test]
fn provider_exception_is_always_outcome_unknown() {
    // 构造任意 mutation 动作。
    let action = SemanticAction::Toggle {};
    // 映射 provider 写调用异常。
    let error = outcome_unknown(&action);
    // 保持稳定结果未知码。
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    // 保守承认动作可能被接受。
    assert_eq!(error.details["acceptedMayHaveOccurred"], true);
    // 禁止自动重试。
    assert_eq!(error.details["automaticRetryProhibited"], true);
    // 明确没有安全重试证明。
    assert_eq!(error.details["retrySafe"], false);
    // 明确没有指针降级。
    assert_eq!(error.details["pointerFallbackUsed"], false);
}

// 验证滚动量到 UIA 私有枚举的封闭映射。
#[test]
fn scroll_amount_mapping_is_complete() {
    // 固定全部 provider-neutral 量。
    let amounts = [
        // 不滚动。
        SemanticScrollAmount::NoAmount,
        // 大幅负向。
        SemanticScrollAmount::LargeDecrement,
        // 小幅负向。
        SemanticScrollAmount::SmallDecrement,
        // 大幅正向。
        SemanticScrollAmount::LargeIncrement,
        // 小幅正向。
        SemanticScrollAmount::SmallIncrement,
    ];
    // 映射必须保持五个不同 UIA 值。
    let mapped = amounts.map(|amount| native_scroll_amount(amount).0);
    // 排序后与 UIA 封闭整数集合一致。
    let mut sorted = mapped;
    // 对小数组执行确定性排序。
    sorted.sort_unstable();
    // UIA ScrollAmount 使用 0..=4 的封闭集合。
    assert_eq!(sorted, [0, 1, 2, 3, 4]);
}
