//! UIX 精确窗口激活与焦点观察 transition 的严格输入契约 Component。

use serde::Deserialize;
use serde_json::Value;

const DEFAULT_POLL_INTERVAL_MS: u32 = 50;
const MINIMUM_POLL_INTERVAL_MS: u32 = 20;
const MAXIMUM_POLL_INTERVAL_MS: u32 = 500;
const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

const fn default_poll_interval_ms() -> u32 {
    DEFAULT_POLL_INTERVAL_MS
}

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存一次窗口激活请求与同连接焦点观察的有界调度参数。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixWindowActivationTransitionInput {
    /// 激活后同连接 list_windows 观察之间的间隔。
    #[serde(default = "default_poll_interval_ms")]
    poll_interval_ms: u32,
    /// 覆盖目标解析、认证、激活请求和焦点观察的总 deadline。
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixWindowActivationTransitionInput {
    /// 严格解析封闭输入，拒绝未知字段、null、浮点和越界值。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let input = serde_json::from_value::<Self>(value.clone()).map_err(|_| {
            "UIX window activation transition input violates schema://window/activate-transition/v1."
        })?;
        if !(MINIMUM_POLL_INTERVAL_MS..=MAXIMUM_POLL_INTERVAL_MS).contains(&input.poll_interval_ms)
            || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
        {
            return Err("UIX window activation transition input is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回同连接焦点观察的有界轮询间隔。
    pub(crate) const fn poll_interval_ms(&self) -> u32 {
        self.poll_interval_ms
    }

    /// 返回覆盖本次 transition 的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn activation_transition_input_defaults_and_exposes_bounded_values() {
        let Ok(input) = UixWindowActivationTransitionInput::parse(&json!({})) else {
            panic!("空激活 transition 输入必须使用默认参数");
        };
        assert_eq!(input.poll_interval_ms(), DEFAULT_POLL_INTERVAL_MS);
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
        let Ok(input) = UixWindowActivationTransitionInput::parse(&json!({
            "pollIntervalMs": 20,
            "timeoutMs": 100
        })) else {
            panic!("边界激活 transition 输入必须有效");
        };
        assert_eq!(input.poll_interval_ms(), MINIMUM_POLL_INTERVAL_MS);
        assert_eq!(input.timeout_ms(), MINIMUM_TIMEOUT_MS);
    }

    #[test]
    fn activation_transition_input_rejects_unknown_null_float_and_bounds() {
        for invalid in [
            json!({ "unexpected": true }),
            json!({ "pollIntervalMs": null }),
            json!({ "timeoutMs": 100.5 }),
            json!({ "pollIntervalMs": 19 }),
            json!({ "pollIntervalMs": 501 }),
            json!({ "timeoutMs": 99 }),
            json!({ "timeoutMs": 30_001 }),
        ] {
            assert!(UixWindowActivationTransitionInput::parse(&invalid).is_err());
        }
    }
}
