//! UIX 窗口生命周期序列及框架当前条件的版本一输入契约 Component。

use serde_json::{Value, json};

use super::{
    uix_window_lifecycle_sequence_contract::UixWindowLifecycleSequenceInput,
    uix_window_state_wait_contract::UixWindowStateWaitCondition,
};

const DEFAULT_POLL_INTERVAL_MS: u32 = 50;
const MINIMUM_POLL_INTERVAL_MS: u32 = 20;
const MAXIMUM_POLL_INTERVAL_MS: u32 = 500;

/// 保存严格验证后的生命周期序列与框架当前状态条件。
#[derive(Clone, Debug)]
pub(crate) struct UixWindowLifecycleSequenceTransitionInput {
    sequence: UixWindowLifecycleSequenceInput,
    condition: UixWindowStateWaitCondition,
    poll_interval_ms: u32,
}

impl UixWindowLifecycleSequenceTransitionInput {
    /// 严格解析封闭输入，并复用动作序列与状态条件 Component 的校验。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let Some(object) = value.as_object() else {
            return Err("UIX window lifecycle sequence transition input must be an object.");
        };
        if object.keys().any(|field| {
            !matches!(
                field.as_str(),
                "actions" | "intervalMs" | "timeoutMs" | "condition" | "pollIntervalMs"
            )
        }) || contains_null(value)
        {
            return Err(
                "UIX window lifecycle sequence transition input violates its closed contract.",
            );
        }

        let Some(condition_value) = object.get("condition") else {
            return Err("UIX window lifecycle sequence transition requires a condition.");
        };
        let condition = UixWindowStateWaitCondition::parse(condition_value)
            .map_err(|_| "UIX window lifecycle sequence transition condition is invalid.")?;

        let poll_interval_value = match object.get("pollIntervalMs") {
            Some(value) => value.clone(),
            None => json!(DEFAULT_POLL_INTERVAL_MS),
        };
        let poll_interval_ms = serde_json::from_value::<u32>(poll_interval_value)
            .map_err(|_| "UIX window lifecycle sequence transition poll interval is invalid.")?;
        if !(MINIMUM_POLL_INTERVAL_MS..=MAXIMUM_POLL_INTERVAL_MS).contains(&poll_interval_ms) {
            return Err(
                "UIX window lifecycle sequence transition poll interval is outside its bounded contract.",
            );
        }

        let mut sequence_object = object.clone();
        sequence_object.remove("condition");
        sequence_object.remove("pollIntervalMs");
        let sequence = UixWindowLifecycleSequenceInput::parse(&Value::Object(sequence_object))?;
        Ok(Self {
            sequence,
            condition,
            poll_interval_ms,
        })
    }

    /// 返回严格验证后的基础生命周期序列。
    pub(crate) fn sequence(&self) -> &UixWindowLifecycleSequenceInput {
        &self.sequence
    }

    /// 返回 provider-neutral 的完整动作顺序。
    pub(crate) fn actions(
        &self,
    ) -> &[super::uix_window_lifecycle_contract::UixWindowLifecycleAction] {
        self.sequence.actions()
    }

    /// 返回请求中的动作数量。
    pub(crate) fn actions_requested(&self) -> usize {
        self.sequence.actions_requested()
    }

    /// 返回不同生命周期动作类型数量。
    pub(crate) fn distinct_action_kinds(&self) -> usize {
        self.sequence.distinct_action_kinds()
    }

    /// 返回固定的 provider-neutral 动作名。
    pub(crate) const fn action(&self) -> &'static str {
        "lifecycle-sequence"
    }

    /// 返回严格复用的框架当前状态条件。
    pub(crate) const fn condition(&self) -> UixWindowStateWaitCondition {
        self.condition
    }

    /// 返回不含 provider 身份的公开条件值。
    pub(crate) fn public_condition(&self) -> Value {
        self.condition.public_value()
    }

    /// 返回相邻动作之间的有界间隔。
    pub(crate) const fn interval_ms(&self) -> u32 {
        self.sequence.interval_ms()
    }

    /// 返回序列计划时长。
    pub(crate) fn planned_duration_ms(&self) -> u32 {
        self.sequence.planned_duration_ms()
    }

    /// 返回框架状态有界轮询间隔。
    pub(crate) const fn poll_interval_ms(&self) -> u32 {
        self.poll_interval_ms
    }

    /// 返回覆盖完整序列与状态观察的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.sequence.timeout_ms()
    }
}

/// 递归拒绝显式 null，避免嵌套动作或条件绕过闭合契约。
fn contains_null(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(values) => values.iter().any(contains_null),
        Value::Object(object) => object.values().any(contains_null),
        Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn valid_input(condition: Value) -> Value {
        json!({
            "actions": [
                { "type": "restore" },
                { "type": "resize", "coordinateSpace": "client-logical-px", "width": 800, "height": 600 },
                { "type": "maximize" }
            ],
            "condition": condition
        })
    }

    #[test]
    fn transition_reuses_sequence_and_all_state_conditions() {
        let conditions = [
            json!({ "type": "visibility", "visible": true }),
            json!({ "type": "focus", "focused": false }),
            json!({ "type": "client-size", "width": 800, "height": 600 }),
            json!({ "type": "window-flags", "maximized": true }),
        ];
        for condition in conditions {
            let Ok(input) =
                UixWindowLifecycleSequenceTransitionInput::parse(&valid_input(condition))
            else {
                panic!("有效 lifecycle sequence transition 必须解析");
            };
            assert_eq!(input.action(), "lifecycle-sequence");
            assert_eq!(input.actions_requested(), 3);
            assert_eq!(input.distinct_action_kinds(), 3);
            assert_eq!(input.actions()[0].provider_action(), "restore_window");
            assert_eq!(input.poll_interval_ms(), DEFAULT_POLL_INTERVAL_MS);
            assert_eq!(input.interval_ms(), 0);
            assert_eq!(input.planned_duration_ms(), 0);
            assert_eq!(input.timeout_ms(), 30_000);
            assert!(input.condition().validate().is_ok());
        }
    }

    #[test]
    fn transition_enforces_poll_and_total_timeout_bounds() {
        let mut too_fast = valid_input(json!({ "type": "focus", "focused": true }));
        too_fast["pollIntervalMs"] = json!(19);
        assert!(UixWindowLifecycleSequenceTransitionInput::parse(&too_fast).is_err());

        let mut too_slow = valid_input(json!({ "type": "focus", "focused": true }));
        too_slow["pollIntervalMs"] = json!(501);
        assert!(UixWindowLifecycleSequenceTransitionInput::parse(&too_slow).is_err());

        let mut too_short = valid_input(json!({ "type": "focus", "focused": true }));
        too_short["intervalMs"] = json!(500);
        too_short["timeoutMs"] = json!(1_099);
        assert!(UixWindowLifecycleSequenceTransitionInput::parse(&too_short).is_err());

        let mut valid_timing = valid_input(json!({ "type": "focus", "focused": true }));
        valid_timing["intervalMs"] = json!(500);
        valid_timing["timeoutMs"] = json!(1_100);
        let Ok(input) = UixWindowLifecycleSequenceTransitionInput::parse(&valid_timing) else {
            panic!("动作计划时长加余量后必须允许有效 timeout");
        };
        assert_eq!(input.planned_duration_ms(), 1_000);
    }

    #[test]
    fn transition_is_closed_and_recursively_rejects_null() {
        let mut unknown = valid_input(json!({ "type": "focus", "focused": true }));
        unknown["unexpected"] = json!(true);
        assert!(UixWindowLifecycleSequenceTransitionInput::parse(&unknown).is_err());

        let mut nested_null = valid_input(json!({ "type": "focus", "focused": true }));
        nested_null["actions"][1]["width"] = Value::Null;
        assert!(UixWindowLifecycleSequenceTransitionInput::parse(&nested_null).is_err());

        let mut null_condition = valid_input(json!({ "type": "focus", "focused": true }));
        null_condition["condition"]["focused"] = Value::Null;
        assert!(UixWindowLifecycleSequenceTransitionInput::parse(&null_condition).is_err());

        let mut null_poll = valid_input(json!({ "type": "focus", "focused": true }));
        null_poll["pollIntervalMs"] = Value::Null;
        assert!(UixWindowLifecycleSequenceTransitionInput::parse(&null_poll).is_err());
    }

    #[test]
    fn transition_rejects_one_kind_actions_and_invalid_condition() {
        let mut one_kind = valid_input(json!({ "type": "focus", "focused": true }));
        one_kind["actions"] = json!([{ "type": "restore" }, { "type": "restore" }]);
        assert!(UixWindowLifecycleSequenceTransitionInput::parse(&one_kind).is_err());

        let malformed = valid_input(json!({ "type": "visibility" }));
        assert!(UixWindowLifecycleSequenceTransitionInput::parse(&malformed).is_err());

        let unsupported = valid_input(json!({ "type": "move", "x": 1, "y": 2 }));
        assert!(UixWindowLifecycleSequenceTransitionInput::parse(&unsupported).is_err());

        let mut extra_condition = valid_input(json!({ "type": "focus", "focused": true }));
        extra_condition["condition"]["extra"] = json!(true);
        assert!(UixWindowLifecycleSequenceTransitionInput::parse(&extra_condition).is_err());
    }
}
