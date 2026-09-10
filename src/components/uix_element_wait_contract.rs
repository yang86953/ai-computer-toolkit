//! 冻结 UIX 精确语义元素等待的 provider-neutral 输入契约。

use serde::Deserialize;
use serde_json::Value;

use super::uix_element_location_contract::UixElementSelector;

const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 表示仅等待唯一匹配或完全缺失的封闭元素条件。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum UixElementWaitCondition {
    Unique,
    Missing,
}

impl UixElementWaitCondition {
    /// 返回稳定公开条件名。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Unique => "unique",
            Self::Missing => "missing",
        }
    }
}

/// 保存通过严格验证的 UIX 元素等待请求。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixElementWaitInput {
    selector: UixElementSelector,
    condition: UixElementWaitCondition,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixElementWaitInput {
    /// 严格解析输入且不回显 selector 或其他非法原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let Some(selector) = value.get("selector").and_then(Value::as_object) else {
            return Err("UIX element wait input violates its bounded contract.");
        };
        if selector.values().any(Value::is_null) {
            return Err("UIX element wait selector fields cannot be null.");
        }
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX element wait input violates schema://ui/element-wait/v2.")?;
        input.selector.validate()?;
        if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms) {
            return Err("UIX element wait timeout is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回复用的严格 exact-AND selector。
    pub(crate) fn selector(&self) -> &UixElementSelector {
        &self.selector
    }

    /// 返回等待条件。
    pub(crate) const fn condition(&self) -> UixElementWaitCondition {
        self.condition
    }

    /// 返回覆盖 resolve、snapshot 和 revision wait 的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn element_wait_reuses_exact_selector_and_defaults_timeout() {
        let Ok(input) = UixElementWaitInput::parse(&json!({
            "selector": { "automationId": "save", "action": "invoke" },
            "condition": "unique"
        })) else {
            panic!("有效元素等待输入必须解析");
        };
        assert_eq!(input.condition().as_str(), "unique");
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
        assert!(input.selector().matches(
            Some("save"),
            "button",
            "保存",
            false,
            true,
            &["invoke".to_owned()]
        ));
    }

    #[test]
    fn element_wait_accepts_missing_and_enforces_timeout_bounds() {
        let Ok(input) = UixElementWaitInput::parse(&json!({
            "selector": { "role": "button" },
            "condition": "missing",
            "timeoutMs": 100
        })) else {
            panic!("缺失条件必须解析");
        };
        assert_eq!(input.condition(), UixElementWaitCondition::Missing);
        assert_eq!(input.timeout_ms(), MINIMUM_TIMEOUT_MS);
        assert!(
            UixElementWaitInput::parse(&json!({
                "selector": { "role": "button" },
                "condition": "unique",
                "timeoutMs": 99
            }))
            .is_err()
        );
        assert!(
            UixElementWaitInput::parse(&json!({
                "selector": { "role": "button" },
                "condition": "unique",
                "timeoutMs": 30_001
            }))
            .is_err()
        );
    }

    #[test]
    fn element_wait_is_closed_and_rejects_stability_poll_and_null_fields() {
        assert!(
            UixElementWaitInput::parse(&json!({
                "selector": { "role": "button" },
                "condition": "unique",
                "stableForMs": 100
            }))
            .is_err()
        );
        assert!(
            UixElementWaitInput::parse(&json!({
                "selector": { "role": "button" },
                "condition": "unique",
                "pollIntervalMs": 50
            }))
            .is_err()
        );
        assert!(
            UixElementWaitInput::parse(&json!({
                "selector": { "role": "button", "unexpected": true },
                "condition": "unique"
            }))
            .is_err()
        );
        assert!(
            UixElementWaitInput::parse(&json!({
                "selector": { "role": null },
                "condition": "unique"
            }))
            .is_err()
        );
        assert!(
            UixElementWaitInput::parse(&json!({
                "selector": { "role": "button" },
                "condition": "unique",
                "timeoutMs": null
            }))
            .is_err()
        );
    }
}
