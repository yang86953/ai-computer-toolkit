//! UIX 窗口生命周期 transition 的严格输入 Component。

use serde::Deserialize;
use serde_json::Value;

use super::{
    uix_window_lifecycle_contract::UixWindowLifecycleAction,
    uix_window_state_wait_contract::UixWindowStateWaitCondition,
};

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

/// 保存一个动作与其框架当前终态条件的严格请求。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixWindowLifecycleTransitionInput {
    action: UixWindowLifecycleAction,
    condition: UixWindowStateWaitCondition,
    #[serde(default = "default_poll_interval_ms")]
    poll_interval_ms: u32,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixWindowLifecycleTransitionInput {
    /// 严格解析动作、条件和总 deadline，不回显非法原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let Some(condition) = value.get("condition") else {
            return Err("UIX window lifecycle transition input violates its bounded contract.");
        };
        UixWindowStateWaitCondition::parse(condition)
            .map_err(|_| "UIX window lifecycle transition condition is invalid.")?;
        let input = serde_json::from_value::<Self>(value.clone()).map_err(|_| {
            "UIX window lifecycle transition input violates schema://window/lifecycle-transition/v1."
        })?;
        if input.action.validate().is_err()
            || input.condition.validate().is_err()
            || !(MINIMUM_POLL_INTERVAL_MS..=MAXIMUM_POLL_INTERVAL_MS)
                .contains(&input.poll_interval_ms)
            || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
        {
            return Err("UIX window lifecycle transition input is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回严格复用的生命周期动作。
    pub(crate) const fn action(&self) -> UixWindowLifecycleAction {
        self.action
    }

    /// 返回严格复用的框架当前状态条件。
    pub(crate) const fn condition(&self) -> UixWindowStateWaitCondition {
        self.condition
    }

    /// 返回有界框架状态轮询间隔。
    pub(crate) const fn poll_interval_ms(&self) -> u32 {
        self.poll_interval_ms
    }

    /// 返回覆盖 resolve、认证、动作和观察的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn valid_input() -> Value {
        json!({
            "action": { "type": "resize", "coordinateSpace": "client-logical-px", "width": 800, "height": 600 },
            "condition": { "type": "client-size", "width": 800, "height": 600 }
        })
    }

    #[test]
    fn transition_reuses_strict_action_and_state_condition_defaults() {
        let Ok(input) = UixWindowLifecycleTransitionInput::parse(&valid_input()) else {
            panic!("有效 transition 输入必须解析");
        };
        assert_eq!(input.action().provider_action(), "resize_window");
        assert_eq!(input.action().requested_size(), Some((800, 600)));
        assert_eq!(input.condition().public_value()["type"], "client-size");
        assert_eq!(input.poll_interval_ms(), DEFAULT_POLL_INTERVAL_MS);
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn transition_enforces_poll_and_timeout_bounds() {
        let mut too_fast = valid_input();
        too_fast["pollIntervalMs"] = json!(19);
        assert!(UixWindowLifecycleTransitionInput::parse(&too_fast).is_err());

        let mut too_slow = valid_input();
        too_slow["pollIntervalMs"] = json!(501);
        assert!(UixWindowLifecycleTransitionInput::parse(&too_slow).is_err());

        let mut too_short = valid_input();
        too_short["timeoutMs"] = json!(99);
        assert!(UixWindowLifecycleTransitionInput::parse(&too_short).is_err());

        let mut too_long = valid_input();
        too_long["timeoutMs"] = json!(30_001);
        assert!(UixWindowLifecycleTransitionInput::parse(&too_long).is_err());
    }

    #[test]
    fn transition_is_closed_and_rejects_unsupported_action_or_condition() {
        let mut unknown = valid_input();
        unknown["unexpected"] = json!(true);
        assert!(UixWindowLifecycleTransitionInput::parse(&unknown).is_err());

        let mut move_action = valid_input();
        move_action["action"] = json!({ "type": "move", "x": 1, "y": 2 });
        assert!(UixWindowLifecycleTransitionInput::parse(&move_action).is_err());

        let mut malformed_condition = valid_input();
        malformed_condition["condition"] = json!({ "type": "visibility" });
        assert!(UixWindowLifecycleTransitionInput::parse(&malformed_condition).is_err());

        let mut null_condition = valid_input();
        null_condition["condition"] = json!({ "type": "focus", "focused": null });
        assert!(UixWindowLifecycleTransitionInput::parse(&null_condition).is_err());
    }
}
