//! UIX 协作式应用内单次完整按键与提交后条件同步输入契约 Component。

use serde::Deserialize;
use serde_json::Value;

use super::{
    uix_element_transition_contract::UixElementTransitionPostcondition,
    uix_key_input_contract::UixKeyPress,
};

const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存严格验证后的单次完整 key press 与提交后语义条件。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixKeyTransitionInput {
    #[serde(flatten)]
    press: UixKeyPress,
    postcondition: UixElementTransitionPostcondition,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixKeyTransitionInput {
    /// 严格解析封闭输入，不回显非法按键、selector 或原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        if !has_only_key_transition_fields(value) {
            return Err("UIX key transition input violates schema://ui/key-transition/v1.");
        }
        let Some(selector) = value
            .get("postcondition")
            .and_then(|postcondition| postcondition.get("selector"))
            .and_then(Value::as_object)
        else {
            return Err("UIX key transition postcondition violates its bounded contract.");
        };
        if selector.values().any(Value::is_null) {
            return Err("UIX key transition selector fields cannot be null.");
        }
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX key transition input violates schema://ui/key-transition/v1.")?;
        input.press.validate()?;
        input.postcondition.selector().validate()?;
        if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms) {
            return Err("UIX key transition timeout is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回 provider-neutral 键名。
    pub(crate) fn key(&self) -> &str {
        self.press.key()
    }

    /// 返回 provider-neutral 修饰键顺序。
    pub(crate) fn modifiers(&self) -> &[String] {
        self.press.modifiers()
    }

    /// 返回 UIX Agent 使用的 provider 名称。
    pub(crate) const fn provider(&self) -> &'static str {
        "uix-agent-v1"
    }

    /// 返回 UIX Agent hello 使用的键名。
    pub(crate) fn provider_key(&self) -> &str {
        self.press.provider_key()
    }

    /// 返回 UIX Agent hello 使用的修饰键集合。
    pub(crate) fn provider_modifiers(&self) -> Vec<&str> {
        self.press.provider_modifiers()
    }

    /// 生成仅供认证 Agent Adapter 使用的 targetless press action。
    pub(crate) fn provider_value(&self) -> Value {
        self.press.provider_value()
    }

    /// 返回动作提交后必须满足的 exact-AND 条件。
    pub(crate) fn postcondition(&self) -> &UixElementTransitionPostcondition {
        &self.postcondition
    }

    /// 返回覆盖解析、认证、按键提交与条件等待的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }
}

fn has_only_key_transition_fields(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.keys().all(|field| {
            matches!(
                field.as_str(),
                "key" | "modifiers" | "postcondition" | "timeoutMs"
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn valid_input() -> Value {
        json!({
            "key": "s",
            "modifiers": ["control"],
            "postcondition": {
                "selector": { "automationId": "saved", "role": "button" },
                "condition": "unique"
            }
        })
    }

    #[test]
    fn key_transition_reuses_press_provider_and_exact_postcondition() {
        let Ok(input) = UixKeyTransitionInput::parse(&valid_input()) else {
            panic!("有效 key transition 必须解析");
        };
        assert_eq!(input.key(), "s");
        assert_eq!(input.modifiers(), ["control"]);
        assert_eq!(input.provider(), "uix-agent-v1");
        assert_eq!(input.provider_key(), "s");
        assert_eq!(input.provider_modifiers(), ["ctrl"]);
        assert_eq!(input.provider_value()["kind"], "press_key");
        assert_eq!(input.postcondition().condition().as_str(), "unique");
        assert!(input.postcondition().selector().matches(
            Some("saved"),
            "button",
            "保存",
            false,
            true,
            &["invoke".to_owned()]
        ));
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn key_transition_accepts_missing_modifiers_and_bounded_missing_condition() {
        let Ok(input) = UixKeyTransitionInput::parse(&json!({
            "key": "escape",
            "postcondition": {
                "selector": { "focused": false },
                "condition": "missing"
            },
            "timeoutMs": 100
        })) else {
            panic!("带 missing 条件的 key transition 必须解析");
        };
        assert!(input.modifiers().is_empty());
        assert_eq!(input.postcondition().condition().as_str(), "missing");
        assert_eq!(input.timeout_ms(), MINIMUM_TIMEOUT_MS);
    }

    #[test]
    fn key_transition_rejects_closed_invalid_and_ambiguous_input() {
        let mut unknown = valid_input();
        unknown["unexpected"] = json!(true);
        assert!(UixKeyTransitionInput::parse(&unknown).is_err());

        for invalid in [
            json!({
                "key": null,
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" }
            }),
            json!({
                "key": "f13",
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" }
            }),
            json!({
                "key": "enter",
                "modifiers": ["alt", "alt"],
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" }
            }),
            json!({
                "key": "enter",
                "modifiers": null,
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" }
            }),
            json!({
                "key": "enter",
                "timeoutMs": 100.5,
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" }
            }),
            json!({
                "key": "enter",
                "postcondition": { "selector": {}, "condition": "unique" }
            }),
        ] {
            assert!(UixKeyTransitionInput::parse(&invalid).is_err());
        }
    }
}
