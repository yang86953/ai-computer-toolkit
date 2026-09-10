//! 冻结 UIX 协作式应用内按键版本二输入。

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{Value, json};

const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

const KEY_NAMES: &[&str] = &[
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "a",
    "alt",
    "b",
    "backspace",
    "c",
    "control",
    "d",
    "delete",
    "down",
    "e",
    "end",
    "enter",
    "escape",
    "f",
    "f1",
    "f10",
    "f11",
    "f12",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "g",
    "h",
    "home",
    "i",
    "insert",
    "j",
    "k",
    "l",
    "left",
    "m",
    "n",
    "o",
    "p",
    "page-down",
    "page-up",
    "q",
    "r",
    "right",
    "s",
    "shift",
    "space",
    "super",
    "t",
    "tab",
    "u",
    "up",
    "v",
    "w",
    "x",
    "y",
    "z",
];

const MODIFIERS: &[&str] = &["alt", "control", "shift", "super"];

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存一个可复用、严格校验的 provider-neutral 按键与修饰键组合。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixKeyPress {
    key: String,
    #[serde(default)]
    modifiers: Vec<String>,
}

impl UixKeyPress {
    /// 严格解析并校验单个按键组合，不回显原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let press = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX key press violates its bounded contract.")?;
        press.validate()?;
        Ok(press)
    }

    /// 校验键名、修饰键白名单与无重复修饰键约束。
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        let modifiers = self.modifiers.iter().map(String::as_str);
        if !KEY_NAMES.contains(&self.key.as_str())
            || self.modifiers.len() > MODIFIERS.len()
            || !self
                .modifiers
                .iter()
                .all(|modifier| MODIFIERS.contains(&modifier.as_str()))
            || modifiers.clone().collect::<BTreeSet<_>>().len() != self.modifiers.len()
        {
            return Err("UIX key press is outside its bounded contract.");
        }
        Ok(())
    }

    /// 返回 provider-neutral 键名。
    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    /// 返回 provider-neutral 修饰键顺序。
    pub(crate) fn modifiers(&self) -> &[String] {
        &self.modifiers
    }

    /// 返回 UIX Agent hello 使用的键名。
    pub(crate) fn provider_key(&self) -> &str {
        provider_name(&self.key)
    }

    /// 返回 UIX Agent hello 使用的修饰键集合。
    pub(crate) fn provider_modifiers(&self) -> Vec<&str> {
        self.modifiers
            .iter()
            .map(|modifier| provider_name(modifier))
            .collect()
    }

    /// 生成仅供认证 Agent Adapter 使用的 targetless action。
    pub(crate) fn provider_value(&self) -> Value {
        json!({
            "kind": "press_key",
            "key": self.provider_key(),
            "modifiers": self.provider_modifiers(),
        })
    }
}

/// 保存严格解析后的单次应用内按键请求。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixKeyInput {
    #[serde(flatten)]
    press: UixKeyPress,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixKeyInput {
    /// 严格解析公开输入且不回显非法内容。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        if !has_only_key_input_fields(value) {
            return Err("UIX key input violates schema://ui/key-input/v2.");
        }
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX key input violates schema://ui/key-input/v2.")?;
        input.press.validate()?;
        if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms) {
            return Err("UIX key input is outside its bounded contract.");
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

    /// 返回覆盖发现、认证、调度与响应的总预算。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }

    /// 返回 UIX Agent hello 使用的键名。
    pub(crate) fn provider_key(&self) -> &str {
        self.press.provider_key()
    }

    /// 返回 UIX Agent hello 使用的修饰键集合。
    pub(crate) fn provider_modifiers(&self) -> Vec<&str> {
        self.press.provider_modifiers()
    }

    /// 生成仅供认证 Agent Adapter 使用的 targetless action。
    pub(crate) fn provider_value(&self) -> Value {
        self.press.provider_value()
    }
}

fn has_only_key_input_fields(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object
            .keys()
            .all(|field| matches!(field.as_str(), "key" | "modifiers" | "timeoutMs"))
    })
}

fn provider_name(name: &str) -> &str {
    match name {
        "control" => "ctrl",
        "page-up" => "page_up",
        "page-down" => "page_down",
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_v2_is_single_press_bounded_and_provider_neutral() {
        let Ok(input) = UixKeyInput::parse(&json!({
            "key": "page-down",
            "modifiers": ["control", "shift"]
        })) else {
            panic!("valid key press must parse");
        };
        assert_eq!(input.provider_key(), "page_down");
        assert_eq!(input.provider_modifiers(), ["ctrl", "shift"]);
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
        assert!(
            UixKeyInput::parse(&json!({
                "key": "enter",
                "modifiers": ["alt", "alt"]
            }))
            .is_err()
        );
        assert!(UixKeyInput::parse(&json!({ "key": "f13" })).is_err());
        assert!(UixKeyInput::parse(&json!({ "key": "enter", "phase": "down" })).is_err());
        assert!(
            serde_json::from_value::<UixKeyInput>(json!({
                "key": "enter",
                "phase": "down"
            }))
            .is_err()
        );
    }

    #[test]
    fn reusable_key_press_keeps_one_validation_and_mapping_directory() {
        let Ok(press) = UixKeyPress::parse(&json!({
            "key": "page-up",
            "modifiers": ["control"]
        })) else {
            panic!("valid reusable key press must parse");
        };
        assert_eq!(press.key(), "page-up");
        assert_eq!(press.modifiers(), ["control"]);
        assert_eq!(press.provider_key(), "page_up");
        assert_eq!(press.provider_modifiers(), ["ctrl"]);
        assert!(
            UixKeyPress::parse(&json!({
                "key": "enter",
                "unexpected": true
            }))
            .is_err()
        );
    }
}
