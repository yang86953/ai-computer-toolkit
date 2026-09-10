//! 冻结 UIX 精确窗口关闭请求与同连接关闭终态同步输入。

use serde::Deserialize;
use serde_json::Value;

const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
const DEFAULT_TIMEOUT_MS: u32 = MAXIMUM_TIMEOUT_MS;

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存一次关闭请求与精确 generation 终态等待的总 deadline。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixWindowCloseTransitionInput {
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixWindowCloseTransitionInput {
    /// 严格解析输入且不回显非法原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let input = serde_json::from_value::<Self>(value.clone()).map_err(
            |_| "UIX window close transition input violates schema://window/close-transition/v1.",
        )?;
        if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms) {
            return Err("UIX window close transition timeout is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回覆盖发现、认证、关闭请求与终态等待的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn close_transition_input_is_closed_and_bounded() {
        let Ok(input) = UixWindowCloseTransitionInput::parse(&json!({})) else {
            panic!("默认关闭 transition 输入必须有效");
        };
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
        assert!(UixWindowCloseTransitionInput::parse(&json!({ "timeoutMs": 99 })).is_err());
        assert!(
            UixWindowCloseTransitionInput::parse(&json!({
                "timeoutMs": 100,
                "acceptConnectionCloseAsProof": true
            }))
            .is_err()
        );
    }
}
