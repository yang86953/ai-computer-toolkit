//! UIX 精确窗口前台激活输入契约 Component。

use serde::Deserialize;
use serde_json::Value;

const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
const DEFAULT_TIMEOUT_MS: u32 = MAXIMUM_TIMEOUT_MS;

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存通过严格验证的 UIX 窗口激活输入。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixWindowActivationInput {
    /// 覆盖目标重新解析、认证、激活请求和焦点观察的总 deadline。
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixWindowActivationInput {
    /// 严格解析输入，且不把原始 JSON 写入错误消息。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX window activation input violates schema://window/activate/v1.")?;
        if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms) {
            return Err("UIX window activation timeout is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回覆盖本次激活全流程的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn activation_input_is_closed_and_defaults_to_thirty_seconds() {
        let Ok(input) = UixWindowActivationInput::parse(&json!({})) else {
            panic!("空激活输入必须使用默认 deadline");
        };
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
        assert_eq!(
            UixWindowActivationInput::parse(&json!({ "timeoutMs": 100 }))
                .map(|value| value.timeout_ms()),
            Ok(MINIMUM_TIMEOUT_MS)
        );
        assert_eq!(
            UixWindowActivationInput::parse(&json!({ "timeoutMs": 30_000 }))
                .map(|value| value.timeout_ms()),
            Ok(MAXIMUM_TIMEOUT_MS)
        );
    }

    #[test]
    fn activation_input_rejects_unknown_or_out_of_range_fields() {
        assert!(UixWindowActivationInput::parse(&json!({ "timeoutMs": 99 })).is_err());
        assert!(UixWindowActivationInput::parse(&json!({ "timeoutMs": 30_001 })).is_err());
        assert!(
            UixWindowActivationInput::parse(&json!({
                "timeoutMs": 100,
                "unexpected": true
            }))
            .is_err()
        );
        assert!(UixWindowActivationInput::parse(&json!({ "timeoutMs": null })).is_err());
        assert!(UixWindowActivationInput::parse(&json!({ "timeoutMs": 100.5 })).is_err());
    }
}
