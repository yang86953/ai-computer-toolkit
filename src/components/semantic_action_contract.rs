//! 冻结语义元素动作的 provider-neutral 输入与封闭动作集合。

// 导入严格 JSON 编解码。
use serde::{Deserialize, Serialize};

// 导入共享语义 selector。
use super::accessibility_selector::AccessibilitySelector;

// 固定默认 worker deadline。
const DEFAULT_TIMEOUT_MS: u32 = 2_000;
// 固定默认查询深度。
const DEFAULT_MAXIMUM_DEPTH: usize = 8;
// 固定默认查询节点数。
const DEFAULT_MAXIMUM_ITEMS: usize = 1_024;
// 限制一次 Value 写入的 UTF-8 字节数。
const MAXIMUM_VALUE_BYTES: usize = 64 * 1_024;

// 返回默认 worker deadline。
const fn default_timeout_ms() -> u32 {
    // 使用公开默认值。
    DEFAULT_TIMEOUT_MS
}

// 返回默认查询深度。
const fn default_maximum_depth() -> usize {
    // 使用公开默认值。
    DEFAULT_MAXIMUM_DEPTH
}

// 返回默认查询节点数。
const fn default_maximum_items() -> usize {
    // 使用公开默认值。
    DEFAULT_MAXIMUM_ITEMS
}

// 返回默认 ControlView。
fn default_view() -> String {
    // 创建独立字符串所有权。
    "control".to_owned()
}

// 返回默认无滚动量。
const fn default_scroll_amount() -> SemanticScrollAmount {
    // 未提供轴不会产生隐式滚动。
    SemanticScrollAmount::NoAmount
}

// 表示公开输入的封闭验证失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SemanticActionInputError {
    // 表示 selector 不符合共享精确匹配契约。
    InvalidSelector,
    // 表示搜索、视图或 deadline 超出硬边界。
    InvalidBounds,
    // 表示 Value 文本超过固定资源边界。
    InvalidValue,
    // 表示 Scroll 没有请求任一轴变化。
    EmptyScroll,
}

// 保存 UIA ScrollPattern 可表达的 provider-neutral 相对滚动量。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
// 使用公开 kebab-case 文本并拒绝开放枚举。
#[serde(rename_all = "kebab-case")]
pub(crate) enum SemanticScrollAmount {
    // 表示该轴不滚动。
    NoAmount,
    // 表示大幅向负方向滚动。
    LargeDecrement,
    // 表示小幅向负方向滚动。
    SmallDecrement,
    // 表示大幅向正方向滚动。
    LargeIncrement,
    // 表示小幅向正方向滚动。
    SmallIncrement,
}

// 保存五种公开语义动作。
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
// 使用 `type` 判别并拒绝每个变体的未知字段。
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum SemanticAction {
    // 调用元素唯一默认动作。
    Invoke {},
    // 设置可编辑 ValuePattern 文本，允许空串用于清空。
    Value {
        // 保存待设置的调用方文本。
        value: String,
    },
    // 切换 TogglePattern 状态。
    Toggle {},
    // 选择 SelectionItemPattern 元素。
    Select {},
    // 按相对量滚动 ScrollPattern 容器。
    Scroll {
        // 保存水平相对滚动量。
        #[serde(default = "default_scroll_amount")]
        horizontal: SemanticScrollAmount,
        // 保存垂直相对滚动量。
        #[serde(default = "default_scroll_amount")]
        vertical: SemanticScrollAmount,
    },
}

// 提供动作名称与封闭校验。
impl SemanticAction {
    // 返回稳定公开动作文本。
    pub(crate) const fn as_str(&self) -> &'static str {
        // 穷举五种动作。
        match self {
            // 映射默认调用。
            Self::Invoke {} => "invoke",
            // 映射值设置。
            Self::Value { .. } => "value",
            // 映射切换。
            Self::Toggle {} => "toggle",
            // 映射选择。
            Self::Select {} => "select",
            // 映射滚动。
            Self::Scroll { .. } => "scroll",
        }
    }

    // 验证动作自有资源边界。
    fn validate(&self) -> Result<(), SemanticActionInputError> {
        // Value 允许空串，但禁止超过固定字节数。
        if let Self::Value { value } = self
            // 直接按 UTF-8 字节长度核对。
            && value.len() > MAXIMUM_VALUE_BYTES
        {
            // 返回值边界错误。
            return Err(SemanticActionInputError::InvalidValue);
        }
        // Scroll 至少要求一个轴发生变化。
        if let Self::Scroll {
            // 借用水平量。
            horizontal,
            // 借用垂直量。
            vertical,
        } = self
            // 两轴均无动作时拒绝空 mutation。
            && *horizontal == SemanticScrollAmount::NoAmount
            // 同时核对垂直轴。
            && *vertical == SemanticScrollAmount::NoAmount
        {
            // 返回空滚动错误。
            return Err(SemanticActionInputError::EmptyScroll);
        }
        // 其他动作无需额外输入。
        Ok(())
    }

    // 返回 Value 动作的文本。
    pub(crate) fn value(&self) -> Option<&str> {
        // 只为 Value 变体返回文本。
        match self {
            // 借用调用方文本。
            Self::Value { value } => Some(value),
            // 其他动作不含文本。
            _ => None,
        }
    }

    // 返回 Scroll 动作的两个轴。
    pub(crate) const fn scroll_amounts(
        // 借用当前动作。
        &self,
    ) -> Option<(SemanticScrollAmount, SemanticScrollAmount)> {
        // 只为 Scroll 变体返回封闭量。
        match self {
            // 复制两个小枚举值。
            Self::Scroll {
                // 借用水平量。
                horizontal,
                // 借用垂直量。
                vertical,
            } => Some((*horizontal, *vertical)),
            // 其他动作没有滚动量。
            _ => None,
        }
    }
}

// 保存完整版本一语义动作输入。
#[derive(Clone, Debug, Deserialize, Serialize)]
// 使用 camelCase 并拒绝协议外字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SemanticActionInput {
    // 保存精确 AND 语义 selector。
    pub(crate) selector: AccessibilitySelector,
    // 保存五种封闭动作之一。
    pub(crate) action: SemanticAction,
    // 保存查询深度边界。
    #[serde(default = "default_maximum_depth")]
    pub(crate) maximum_depth: usize,
    // 保存查询节点边界。
    #[serde(default = "default_maximum_items")]
    pub(crate) maximum_items: usize,
    // 保存 UIA view。
    #[serde(default = "default_view")]
    pub(crate) view: String,
    // 保存 parent worker deadline。
    #[serde(default = "default_timeout_ms")]
    pub(crate) timeout_ms: u32,
}

// 提供完整输入的统一硬边界验证。
impl SemanticActionInput {
    // 验证 selector、动作和有界查询参数。
    pub(crate) fn validate(&self) -> Result<(), SemanticActionInputError> {
        // 复用定位与等待共享 selector 契约。
        if !matches!(self.selector.validate(), Ok(())) {
            // 隐藏 selector 具体内容。
            return Err(SemanticActionInputError::InvalidSelector);
        }
        // 验证动作自有边界。
        self.action.validate()?;
        // 深度、数量、视图和 deadline 必须全部位于封闭范围。
        if self.maximum_depth > 20
            // 数量必须为正且受硬上限约束。
            || !(1..=4_096).contains(&self.maximum_items)
            // 只允许 UIA ControlView 或 RawView。
            || !matches!(self.view.as_str(), "control" | "raw")
            // worker deadline 保持统一边界。
            || !(1..=30_000).contains(&self.timeout_ms)
        {
            // 返回统一范围错误。
            return Err(SemanticActionInputError::InvalidBounds);
        }
        // 所有门禁通过。
        Ok(())
    }
}

// 验证封闭动作与资源边界。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 导入被测契约。
    use super::*;

    // 验证五种动作均可严格解析。
    #[test]
    fn parses_all_five_provider_neutral_actions() {
        // 固定全部合法动作夹具。
        let actions = [
            // 调用动作无需额外字段。
            json!({ "type": "invoke" }),
            // Value 允许空串清空。
            json!({ "type": "value", "value": "" }),
            // Toggle 无参数。
            json!({ "type": "toggle" }),
            // Select 无参数。
            json!({ "type": "select" }),
            // Scroll 使用封闭相对量。
            json!({ "type": "scroll", "vertical": "small-increment" }),
        ];
        // 逐项解析并验证。
        for action in actions {
            // 构造完整输入。
            let input = serde_json::from_value::<SemanticActionInput>(json!({
                // 使用最小合法 selector。
                "selector": { "automationId": "ready" },
                // 注入当前动作。
                "action": action,
            }))
            // 合法夹具必须可解析。
            .unwrap_or_else(|error| panic!("semantic action fixture failed: {error}"));
            // 合法夹具必须通过硬边界。
            assert_eq!(input.validate(), Ok(()));
        }
    }

    // 验证未知字段与空滚动失败闭合。
    #[test]
    fn rejects_open_action_shapes_and_empty_scroll() {
        // 未知动作字段必须在反序列化边界拒绝。
        assert!(
            serde_json::from_value::<SemanticActionInput>(json!({
                // 使用合法 selector。
                "selector": { "name": "ready" },
                // Invoke 不接受坐标或 provider 字段。
                "action": { "type": "invoke", "x": 1 },
            }))
            // 断言严格失败。
            .is_err()
        );
        // 两轴均为空的 Scroll 可以解析但必须校验失败。
        let input = serde_json::from_value::<SemanticActionInput>(json!({
            // 使用合法 selector。
            "selector": { "name": "ready" },
            // 省略两个轴即默认 no-amount。
            "action": { "type": "scroll" },
        }))
        // 解析形状本身必须成功。
        .unwrap_or_else(|error| panic!("empty scroll fixture failed: {error}"));
        // 空 mutation 必须被拒绝。
        assert_eq!(input.validate(), Err(SemanticActionInputError::EmptyScroll));
    }
}
