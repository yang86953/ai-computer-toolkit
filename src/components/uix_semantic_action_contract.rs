//! 冻结协作式语义快照上的精确元素动作输入。

use serde::Deserialize;
use serde_json::{Value, json};

use super::opaque_id::{OpaqueTargetId, OpaqueTargetKind};

const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MAXIMUM_TEXT_BYTES: usize = 64 * 1024;
const MAXIMUM_SCROLL_DELTA: f32 = 10_000.0;

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// UIX 协作式 provider 可以无损承接的语义动作集合。
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum UixSemanticAction {
    Invoke {},
    Focus {},
    SetValue { value: String },
    InsertText { text: String },
    Select { value: String },
    Toggle {},
    Increment {},
    Decrement {},
    Scroll { delta_x: f32, delta_y: f32 },
}

impl UixSemanticAction {
    /// 返回公开动作类别。
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Self::Invoke {} => "invoke",
            Self::Focus {} => "focus",
            Self::SetValue { .. } => "set-value",
            Self::InsertText { .. } => "insert-text",
            Self::Select { .. } => "select",
            Self::Toggle {} => "toggle",
            Self::Increment {} => "increment",
            Self::Decrement {} => "decrement",
            Self::Scroll { .. } => "scroll",
        }
    }

    /// 返回 UIX hello 与节点动作清单使用的稳定动作名。
    pub(crate) const fn provider_action(&self) -> &'static str {
        match self {
            Self::SetValue { .. } => "set_value",
            Self::InsertText { .. } => "insert_text",
            _ => self.as_str(),
        }
    }

    /// 生成仅供认证 Adapter 使用的有界动作对象。
    pub(crate) fn provider_value(&self) -> Value {
        match self {
            Self::Invoke {} => json!({ "kind": "invoke" }),
            Self::Focus {} => json!({ "kind": "focus" }),
            Self::SetValue { value } => json!({ "kind": "set_value", "value": value }),
            Self::InsertText { text } => json!({ "kind": "insert_text", "text": text }),
            Self::Select { value } => json!({ "kind": "select", "value": value }),
            Self::Toggle {} => json!({ "kind": "toggle" }),
            Self::Increment {} => json!({ "kind": "increment" }),
            Self::Decrement {} => json!({ "kind": "decrement" }),
            Self::Scroll { delta_x, delta_y } => {
                json!({ "kind": "scroll", "delta_x": delta_x, "delta_y": delta_y })
            }
        }
    }

    pub(crate) fn validate(&self) -> bool {
        match self {
            Self::SetValue { value } => value.len() <= MAXIMUM_TEXT_BYTES,
            Self::InsertText { text } => text.len() <= MAXIMUM_TEXT_BYTES,
            Self::Select { value } => !value.is_empty() && value.len() <= MAXIMUM_TEXT_BYTES,
            Self::Scroll { delta_x, delta_y } => {
                delta_x.is_finite()
                    && delta_y.is_finite()
                    && (*delta_x != 0.0 || *delta_y != 0.0)
                    && delta_x.abs() <= MAXIMUM_SCROLL_DELTA
                    && delta_y.abs() <= MAXIMUM_SCROLL_DELTA
            }
            _ => true,
        }
    }
}

/// 保存版本二动作的公开输入。
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixSemanticActionInput {
    pub(crate) snapshot_id: String,
    pub(crate) element_id: String,
    pub(crate) action: UixSemanticAction,
    #[serde(default = "default_timeout_ms")]
    pub(crate) timeout_ms: u32,
}

impl UixSemanticActionInput {
    /// 严格解析输入且不把原始文本写入错误消息。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX semantic action input violates schema://ui/element-action/v2.")?;
        if !canonical_snapshot_id(&input.snapshot_id)
            || OpaqueTargetId::parse(&input.element_id)
                .is_none_or(|target| target.kind() != OpaqueTargetKind::Element)
            || !(1..=30_000).contains(&input.timeout_ms)
            || !input.action.validate()
        {
            return Err("UIX semantic action input is outside its bounded contract.");
        }
        Ok(input)
    }
}

pub(crate) fn canonical_snapshot_id(value: &str) -> bool {
    value.strip_prefix("as3:").is_some_and(|suffix| {
        suffix.len() == 16
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_snapshot_action_contract_is_closed_and_bounded() {
        let input = UixSemanticActionInput::parse(&json!({
            "snapshotId": "as3:0123456789abcdef",
            "elementId": "s2:e:0123456789abcdef",
            "action": { "type": "insert-text", "text": "hello" }
        }))
        .expect("valid fixture must parse");
        assert_eq!(input.action.provider_action(), "insert_text");
        assert_eq!(input.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert!(
            UixSemanticActionInput::parse(&json!({
                "snapshotId": "as3:0123456789abcdef",
                "elementId": "s2:e:0123456789abcdef",
                "action": { "type": "scroll", "deltaX": 0.0, "deltaY": 0.0 }
            }))
            .is_err()
        );
    }
}
