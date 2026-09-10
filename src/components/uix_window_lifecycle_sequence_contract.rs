//! UIX 协作式窗口生命周期序列版本一输入契约 Component。

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::Value;

use super::uix_window_lifecycle_contract::UixWindowLifecycleAction;

const DEFAULT_INTERVAL_MS: u32 = 0;
const MAXIMUM_INTERVAL_MS: u32 = 500;
const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_ACTIONS: usize = 2;
const MAXIMUM_ACTIONS: usize = 16;
const MAXIMUM_PLANNED_DURATION_MS: u32 = 5_000;

const fn default_interval_ms() -> u32 {
    DEFAULT_INTERVAL_MS
}

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存严格验证后的同一连接窗口生命周期序列请求。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixWindowLifecycleSequenceInput {
    actions: Vec<UixWindowLifecycleAction>,
    #[serde(default = "default_interval_ms")]
    interval_ms: u32,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixWindowLifecycleSequenceInput {
    /// 严格解析公开输入且不回显非法原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let input = serde_json::from_value::<Self>(value.clone()).map_err(|_| {
            "UIX window lifecycle sequence input violates schema://window/lifecycle-sequence/v1."
        })?;
        let planned_duration_ms = input.planned_duration_ms();
        if !(MINIMUM_ACTIONS..=MAXIMUM_ACTIONS).contains(&input.actions.len())
            || input
                .actions
                .iter()
                .any(|action| action.validate().is_err())
            || input.distinct_action_kinds() < 2
            || input.interval_ms > MAXIMUM_INTERVAL_MS
            || planned_duration_ms > MAXIMUM_PLANNED_DURATION_MS
            || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
            || input.timeout_ms < planned_duration_ms.saturating_add(MINIMUM_TIMEOUT_MS)
        {
            return Err("UIX window lifecycle sequence input is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回完整的、已复用生命周期动作 Component 的动作顺序。
    pub(crate) fn actions(&self) -> &[UixWindowLifecycleAction] {
        &self.actions
    }

    /// 返回请求中的动作数量。
    pub(crate) fn actions_requested(&self) -> usize {
        self.actions.len()
    }

    /// 返回不同 provider-neutral 动作类型的数量。
    pub(crate) fn distinct_action_kinds(&self) -> usize {
        self.actions
            .iter()
            .map(|action| action.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// 返回固定的 provider-neutral 动作名。
    pub(crate) const fn action(&self) -> &'static str {
        "lifecycle-sequence"
    }

    /// 返回相邻动作之间的有界间隔。
    pub(crate) const fn interval_ms(&self) -> u32 {
        self.interval_ms
    }

    /// 返回 `(actions.len() - 1) * intervalMs` 的计划时长。
    pub(crate) fn planned_duration_ms(&self) -> u32 {
        (self.actions.len().saturating_sub(1) as u32).saturating_mul(self.interval_ms)
    }

    /// 返回覆盖发现、认证、调度与响应的总 deadline。
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
            "actions": [
                { "type": "restore" },
                { "type": "resize", "coordinateSpace": "client-logical-px", "width": 800, "height": 600 },
                { "type": "maximize" }
            ]
        })
    }

    #[test]
    fn lifecycle_sequence_reuses_action_validation_and_defaults() {
        let Ok(input) = UixWindowLifecycleSequenceInput::parse(&valid_input()) else {
            panic!("有效生命周期序列必须解析");
        };
        assert_eq!(input.action(), "lifecycle-sequence");
        assert_eq!(input.actions_requested(), 3);
        assert_eq!(input.distinct_action_kinds(), 3);
        assert_eq!(input.interval_ms(), DEFAULT_INTERVAL_MS);
        assert_eq!(input.planned_duration_ms(), 0);
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
        assert_eq!(input.actions()[0].provider_action(), "restore_window");
        assert_eq!(
            input.actions()[1].public_value()["coordinateSpace"],
            "client-logical-px"
        );
        assert_eq!(input.actions()[1].provider_value()["kind"], "resize_window");
    }

    #[test]
    fn lifecycle_sequence_requires_two_distinct_actions_and_bounded_timing() {
        let only_one_kind = json!({
            "actions": [
                { "type": "restore" },
                { "type": "restore" }
            ]
        });
        assert!(UixWindowLifecycleSequenceInput::parse(&only_one_kind).is_err());

        let mut too_short = valid_input();
        too_short["intervalMs"] = json!(500);
        too_short["timeoutMs"] = json!(1099);
        assert!(UixWindowLifecycleSequenceInput::parse(&too_short).is_err());

        let mut too_long = valid_input();
        too_long["intervalMs"] = json!(500);
        too_long["timeoutMs"] = json!(30_000);
        let actions = (0..16)
            .map(|index| {
                if index == 0 {
                    json!({ "type": "restore" })
                } else {
                    json!({ "type": "maximize" })
                }
            })
            .collect::<Vec<_>>();
        too_long["actions"] = json!(actions);
        assert!(UixWindowLifecycleSequenceInput::parse(&too_long).is_err());
    }

    #[test]
    fn lifecycle_sequence_is_closed_and_rejects_invalid_actions() {
        let mut unknown = valid_input();
        unknown["unexpected"] = json!(true);
        assert!(UixWindowLifecycleSequenceInput::parse(&unknown).is_err());

        let mut invalid_resize = valid_input();
        invalid_resize["actions"][1]["width"] = json!(0);
        assert!(UixWindowLifecycleSequenceInput::parse(&invalid_resize).is_err());

        let mut invalid_action_field = valid_input();
        invalid_action_field["actions"][0]["x"] = json!(1);
        assert!(UixWindowLifecycleSequenceInput::parse(&invalid_action_field).is_err());

        let mut invalid_coordinate_space = valid_input();
        invalid_coordinate_space["actions"][1]["coordinateSpace"] = json!("screen-physical-px");
        assert!(UixWindowLifecycleSequenceInput::parse(&invalid_coordinate_space).is_err());
    }
}
