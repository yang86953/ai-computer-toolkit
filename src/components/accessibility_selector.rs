//! 提供 Accessibility 只读查询共用的 provider-neutral 语义 selector。

// 导入严格 JSON 序列化与反序列化。
use serde::{Deserialize, Serialize};
// 导入无平台类型的 JSON 节点视图。
use serde_json::Value;

// 限制单个语义字符串的 UTF-8 字节数。
const MAXIMUM_SELECTOR_BYTES: usize = 512;

// 表示语义 selector 的封闭验证失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccessibilitySelectorError {
    // 表示没有提供任何语义条件。
    Empty,
    // 表示字符串为空或超过固定边界。
    InvalidString,
    // 表示 control type 不是正整数。
    InvalidControlType,
}

// 保存 provider-neutral 语义 selector。
#[derive(Clone, Debug, Deserialize, Serialize)]
// 使用 camelCase 并拒绝未知字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccessibilitySelector {
    // 保存可选精确公开名称。
    name: Option<String>,
    // 保存可选精确 automation ID。
    automation_id: Option<String>,
    // 保存可选精确类名。
    class_name: Option<String>,
    // 保存可选精确 framework ID。
    framework_id: Option<String>,
    // 保存可选精确 control type ID。
    control_type: Option<i32>,
}

// 提供统一验证与 AND 匹配语义。
impl AccessibilitySelector {
    // 验证 selector 至少包含一个有界语义字段。
    pub(crate) fn validate(&self) -> Result<(), AccessibilitySelectorError> {
        // 收集全部字符串 selector。
        let strings = [
            // 加入名称。
            self.name.as_deref(),
            // 加入 automation ID。
            self.automation_id.as_deref(),
            // 加入类名。
            self.class_name.as_deref(),
            // 加入 framework ID。
            self.framework_id.as_deref(),
        ];
        // 拒绝空字符串和超长输入。
        if strings
            // 遍历已提供字段。
            .into_iter()
            // 丢弃缺失字段。
            .flatten()
            // 检查每个 UTF-8 边界。
            .any(|value| value.is_empty() || value.len() > MAXIMUM_SELECTOR_BYTES)
        {
            // 返回封闭字符串错误。
            return Err(AccessibilitySelectorError::InvalidString);
        }
        // control type 必须为正数。
        if self.control_type.is_some_and(|value| value <= 0) {
            // 返回封闭数值错误。
            return Err(AccessibilitySelectorError::InvalidControlType);
        }
        // 至少要求一个语义字段。
        if strings.into_iter().all(|value| value.is_none()) && self.control_type.is_none() {
            // 拒绝匹配整个树的空 selector。
            return Err(AccessibilitySelectorError::Empty);
        }
        // 返回已验证状态。
        Ok(())
    }

    // 核对一个无平台类型的节点是否满足 AND selector。
    pub(crate) fn matches_node(&self, node: &Value) -> bool {
        // 精确匹配名称。
        self.name
            // 借用可选名称。
            .as_ref()
            // 已提供时要求节点字符串完全相等。
            .is_none_or(|value| node.get("name").and_then(Value::as_str) == Some(value))
            // 精确匹配 automation ID。
            && self.automation_id.as_ref().is_none_or(|value| {
                // 读取公开 automation ID。
                node.get("automationId").and_then(Value::as_str) == Some(value)
            })
            // 精确匹配类名。
            && self.class_name.as_ref().is_none_or(|value| {
                // 读取公开类名。
                node.get("className").and_then(Value::as_str) == Some(value)
            })
            // 精确匹配 framework ID。
            && self.framework_id.as_ref().is_none_or(|value| {
                // 读取公开 framework ID。
                node.get("frameworkId").and_then(Value::as_str) == Some(value)
            })
            // 精确匹配 control type。
            && self.control_type.is_none_or(|value| {
                // 读取公开 control type。
                node.get("controlType").and_then(Value::as_i64) == Some(i64::from(value))
            })
    }
}

// 验证 selector 的封闭输入与 AND 语义。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 导入被测类型。
    use super::*;

    // 验证未知字段、空值和数值边界均被拒绝。
    #[test]
    fn selector_validation_is_closed_and_bounded() {
        // 解析合法 selector。
        let valid = serde_json::from_value::<AccessibilitySelector>(json!({
            // 使用单一 automation ID。
            "automationId": "ready",
        }))
        // 测试夹具必须可反序列化。
        .unwrap_or_else(|error| panic!("valid selector fixture failed: {error}"));
        // 合法 selector 必须通过验证。
        assert_eq!(valid.validate(), Ok(()));
        // 未知字段必须在反序列化边界拒绝。
        assert!(
            serde_json::from_value::<AccessibilitySelector>(json!({ "xpath": "//x" })).is_err()
        );
        // 空 selector 必须返回封闭错误。
        let empty = serde_json::from_value::<AccessibilitySelector>(json!({}))
            // 测试夹具必须可反序列化。
            .unwrap_or_else(|error| panic!("empty selector fixture failed: {error}"));
        // 空 selector 不得匹配整个树。
        assert_eq!(empty.validate(), Err(AccessibilitySelectorError::Empty));
    }

    // 验证多个字段使用精确 AND 匹配。
    #[test]
    fn selector_matches_provider_neutral_nodes_with_and_semantics() {
        // 解析双字段 selector。
        let selector = serde_json::from_value::<AccessibilitySelector>(json!({
            // 精确 automation ID。
            "automationId": "ready",
            // 精确 control type。
            "controlType": 50000,
        }))
        // 测试夹具必须可反序列化。
        .unwrap_or_else(|error| panic!("selector fixture failed: {error}"));
        // 两个字段同时命中才算匹配。
        assert!(selector.matches_node(&json!({ "automationId": "ready", "controlType": 50000 })));
        // 任一字段不一致必须拒绝。
        assert!(!selector.matches_node(&json!({ "automationId": "ready", "controlType": 50001 })));
    }
}
