//! 提供与 provider 无关的 JSON Pointer 条件判定。

// 导入公开 JSON 值类型。
use serde_json::Value;

// 描述一次 JSON 条件的窄输入。
#[derive(Clone, Copy, Debug)]
pub(crate) enum JsonCondition<'a> {
    // 要求 JSON Pointer 指向任意现存值，包括 null。
    Exists {
        // 借用已经通过 System 边界验证的指针。
        pointer: &'a str,
    },
    // 要求 JSON Pointer 指向与期望值完全相等的值。
    Equals {
        // 借用已经通过 System 边界验证的指针。
        pointer: &'a str,
        // 借用调用方提供的有界期望 JSON。
        expected: &'a Value,
    },
}

// 描述不携带实际值或期望值的稳定失败原因。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JsonConditionFailure {
    // JSON Pointer 没有指向现存值。
    Missing,
    // 现存值与期望 JSON 不完全相等。
    NotEqual,
}

// 对完整 provider 结果执行一次无副作用断言。
pub(crate) fn evaluate(
    // 借用已经成功建立的完整 provider 结果。
    result: &Value,
    // 接收只包含指针和可选期望值的窄断言。
    condition: JsonCondition<'_>,
) -> Result<(), JsonConditionFailure> {
    // 按断言种类执行确定性 JSON Pointer 查询。
    match condition {
        // exists 只要求目标存在，不区分其值是否为 null。
        JsonCondition::Exists { pointer } => result
            // 使用 serde_json 的 RFC 6901 Pointer 实现。
            .pointer(pointer)
            // 存在时丢弃借用值并返回成功。
            .map(|_| ())
            // 缺失时返回不泄漏结果内容的窄原因。
            .ok_or(JsonConditionFailure::Missing),
        // equals 要求目标存在且与期望 JSON 完全相等。
        JsonCondition::Equals { pointer, expected } => {
            // 先解析目标，缺失与不相等必须保持不同语义。
            let actual = result
                // 使用已经验证语法的 JSON Pointer。
                .pointer(pointer)
                // 缺失时返回稳定原因。
                .ok_or(JsonConditionFailure::Missing)?;
            // 对 JSON 值执行类型敏感的精确相等比较。
            if actual == expected {
                // 完全相等时断言通过。
                Ok(())
            } else {
                // 不回显任一值，只报告稳定不相等原因。
                Err(JsonConditionFailure::NotEqual)
            }
        }
    }
}

// 验证 RFC 6901 JSON Pointer 的根、前导斜杠和转义语法。
pub(crate) fn is_valid_json_pointer(pointer: &str) -> bool {
    // 空字符串是指向完整 JSON 文档的根指针。
    if pointer.is_empty() {
        // 根指针有效。
        return true;
    }
    // 非根指针必须以斜杠开始。
    if !pointer.starts_with('/') {
        // 拒绝相对路径和 JSONPath 等其他语法。
        return false;
    }
    // 使用字节扫描即可安全识别 ASCII 转义标记。
    let bytes = pointer.as_bytes();
    // 从第一个 segment 字节开始扫描。
    let mut index = 1usize;
    // 检查整个 Pointer 的每个转义。
    while index < bytes.len() {
        // 只对波浪号执行双字节转义校验。
        if bytes[index] == b'~' {
            // `~` 后必须存在 0 或 1。
            if index + 1 >= bytes.len() || !matches!(bytes[index + 1], b'0' | b'1') {
                // 拒绝悬空或未知转义。
                return false;
            }
            // 跳过已经验证的转义尾字节。
            index += 1;
        }
        // 继续扫描下一个 UTF-8 字节。
        index += 1;
    }
    // 全部语法约束成立。
    true
}

// 验证根指针、转义、缺失和精确相等语义。
#[cfg(test)]
mod tests {
    // 导入待测窄契约。
    use super::{JsonCondition, JsonConditionFailure, evaluate, is_valid_json_pointer};
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 验证 exists 接受根和值为 null 的目标。
    #[test]
    fn exists_accepts_root_and_null_values() {
        // 构造包含 null 的确定性结果。
        let result = json!({ "value": null });
        // 空指针按 RFC 6901 指向完整根值。
        assert_eq!(
            // 执行根存在断言。
            evaluate(&result, JsonCondition::Exists { pointer: "" }),
            // 根必须存在。
            Ok(())
        );
        // null 仍是现存 JSON 值而不是缺失。
        assert_eq!(
            // 执行 null 字段存在断言。
            evaluate(
                // 传递完整结果。
                &result,
                // 指向 null 字段。
                JsonCondition::Exists { pointer: "/value" },
            ),
            // null 字段必须视为存在。
            Ok(())
        );
    }

    // 验证 Pointer 转义和缺失原因保持确定。
    #[test]
    fn exists_supports_pointer_escaping_and_reports_missing() {
        // 构造键名包含斜杠的结果。
        let result = json!({ "a/b": true });
        // `~1` 必须按 RFC 6901 解码为斜杠。
        assert_eq!(
            // 执行转义指针断言。
            evaluate(&result, JsonCondition::Exists { pointer: "/a~1b" }),
            // 转义后的键必须存在。
            Ok(())
        );
        // 不存在字段返回稳定 Missing 原因。
        assert_eq!(
            // 执行缺失目标断言。
            evaluate(
                &result,
                JsonCondition::Exists {
                    pointer: "/missing"
                }
            ),
            // 不得把缺失改写为不相等。
            Err(JsonConditionFailure::Missing)
        );
    }

    // 验证 equals 区分通过、缺失和不相等。
    #[test]
    fn equals_reports_exact_outcomes() {
        // 构造包含数字一的稳定结果。
        let result = json!({ "value": 1 });
        // 相同 JSON 数字必须通过。
        assert_eq!(
            // 执行相等断言。
            evaluate(
                // 传递完整结果。
                &result,
                // 指向现存数字并提供相同期望值。
                JsonCondition::Equals {
                    // 使用字段指针。
                    pointer: "/value",
                    // 借用相同 JSON 数字。
                    expected: &json!(1),
                },
            ),
            // 完全相等时通过。
            Ok(())
        );
        // JSON 字符串不得与数字相等。
        assert_eq!(
            // 执行类型不同的相等断言。
            evaluate(
                // 传递完整结果。
                &result,
                // 提供字符串期望值。
                JsonCondition::Equals {
                    // 使用同一字段指针。
                    pointer: "/value",
                    // 借用不同类型的 JSON 值。
                    expected: &json!("1"),
                },
            ),
            // 类型不同必须报告不相等。
            Err(JsonConditionFailure::NotEqual)
        );
        // 缺失字段优先报告 Missing。
        assert_eq!(
            // 执行缺失字段相等断言。
            evaluate(
                // 传递完整结果。
                &result,
                // 指向不存在字段。
                JsonCondition::Equals {
                    // 使用缺失字段指针。
                    pointer: "/missing",
                    // 期望值不影响缺失判定。
                    expected: &json!(null),
                },
            ),
            // 缺失必须与不相等区分。
            Err(JsonConditionFailure::Missing)
        );
    }

    // 验证 Pointer 语法接受根与标准转义并拒绝相对或未知转义。
    #[test]
    fn pointer_syntax_is_strict_rfc_6901() {
        // 空字符串是合法根 Pointer。
        assert!(is_valid_json_pointer(""));
        // 斜杠开头且使用标准转义的 Pointer 合法。
        assert!(is_valid_json_pointer("/a~1b/~0value"));
        // 相对路径不属于 JSON Pointer。
        assert!(!is_valid_json_pointer("a/b"));
        // 未知转义必须拒绝。
        assert!(!is_valid_json_pointer("/bad~2escape"));
        // 悬空波浪号必须拒绝。
        assert!(!is_valid_json_pointer("/bad~"));
    }
}
