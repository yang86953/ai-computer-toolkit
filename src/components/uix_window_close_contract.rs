//! 冻结 UIX 协作式精确窗口关闭请求输入。

use serde::Deserialize;
use serde_json::Value;

const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
const DEFAULT_TIMEOUT_MS: u32 = MAXIMUM_TIMEOUT_MS;

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存通过严格验证的 UIX 窗口关闭输入。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixWindowCloseInput {
    /// 覆盖重新解析、认证和关闭请求响应的总 deadline。
    #[serde(default = "default_timeout_ms")]
    pub(crate) timeout_ms: u32,
}

impl UixWindowCloseInput {
    /// 严格解析输入，且不把原始内容写入错误消息。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX window close input violates schema://window/close/v2.")?;
        if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms) {
            return Err("UIX window close timeout is outside its bounded contract.");
        }
        Ok(input)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn close_input_is_closed_and_bounded() {
        let input = UixWindowCloseInput::parse(&json!({})).expect("default close input must parse");
        assert_eq!(input.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert!(UixWindowCloseInput::parse(&json!({ "timeoutMs": 99 })).is_err());
        assert!(
            UixWindowCloseInput::parse(&json!({
                "timeoutMs": 100,
                "waitForClosed": true
            }))
            .is_err()
        );
    }
}
