//! UIX 应用内指针序列与提交后语义条件的版本一输入契约 Component。

use serde_json::Value;

use super::{
    uix_element_transition_contract::UixElementTransitionPostcondition,
    uix_pointer_input_contract::UixPointerCoordinateSpace,
    uix_pointer_sequence_contract::{UixPointerSequenceInput, UixPointerSequenceStep},
};

/// 保存严格验证后的应用内指针序列 transition 请求。
#[derive(Clone, Debug)]
pub(crate) struct UixPointerSequenceTransitionInput {
    sequence: UixPointerSequenceInput,
    postcondition: UixElementTransitionPostcondition,
}

impl UixPointerSequenceTransitionInput {
    /// 严格解析封闭输入，并把序列校验委托给既有 sequence Component。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let Some(object) = value.as_object() else {
            return Err("UIX pointer sequence transition input must be an object.");
        };
        if object.values().any(Value::is_null)
            || object.keys().any(|field| {
                !matches!(
                    field.as_str(),
                    "coordinateSpace" | "steps" | "intervalMs" | "timeoutMs" | "postcondition"
                )
            })
        {
            return Err("UIX pointer sequence transition input violates its closed contract.");
        }

        let Some(postcondition_value) = object.get("postcondition") else {
            return Err("UIX pointer sequence transition requires a postcondition.");
        };
        let Some(postcondition_object) = postcondition_value.as_object() else {
            return Err("UIX pointer sequence transition postcondition must be an object.");
        };
        if postcondition_object.values().any(Value::is_null) {
            return Err("UIX pointer sequence transition postcondition cannot contain null.");
        }
        let Some(selector) = postcondition_object
            .get("selector")
            .and_then(Value::as_object)
        else {
            return Err("UIX pointer sequence transition requires a selector object.");
        };
        if selector.values().any(Value::is_null) {
            return Err("UIX pointer sequence transition selector fields cannot be null.");
        }
        let postcondition = serde_json::from_value::<UixElementTransitionPostcondition>(
            postcondition_value.clone(),
        )
        .map_err(|_| "UIX pointer sequence transition postcondition violates its contract.")?;
        postcondition.selector().validate()?;

        let mut sequence_object = object.clone();
        sequence_object.remove("postcondition");
        let sequence = UixPointerSequenceInput::parse(&Value::Object(sequence_object))?;
        Ok(Self {
            sequence,
            postcondition,
        })
    }

    /// 返回严格验证后的基础 pointer sequence 输入。
    pub(crate) fn sequence(&self) -> &UixPointerSequenceInput {
        &self.sequence
    }

    /// 返回提交后必须满足的 exact-AND 语义条件。
    pub(crate) fn postcondition(&self) -> &UixElementTransitionPostcondition {
        &self.postcondition
    }

    /// 返回完整步骤顺序。
    pub(crate) fn steps(&self) -> &[UixPointerSequenceStep] {
        self.sequence.steps()
    }

    /// 返回请求步骤总数。
    pub(crate) fn steps_requested(&self) -> usize {
        self.sequence.steps_requested()
    }

    /// 返回 move 步骤数。
    pub(crate) fn moves_requested(&self) -> usize {
        self.sequence.moves_requested()
    }

    /// 返回普通左键 click 步骤数。
    pub(crate) fn clicks_requested(&self) -> usize {
        self.sequence.clicks_requested()
    }

    /// 返回固定的客户区 logical px 坐标空间。
    pub(crate) const fn coordinate_space(&self) -> UixPointerCoordinateSpace {
        self.sequence.coordinate_space()
    }

    /// 返回相邻步骤之间的有界间隔。
    pub(crate) const fn interval_ms(&self) -> u32 {
        self.sequence.interval_ms()
    }

    /// 返回序列计划时长。
    pub(crate) fn planned_duration_ms(&self) -> u32 {
        self.sequence.planned_duration_ms()
    }

    /// 返回覆盖序列与后置观测的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.sequence.timeout_ms()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn valid_input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "move", "x": 10.0, "y": 20.0 },
                { "type": "click", "x": 30.0, "y": 40.0 }
            ],
            "postcondition": {
                "selector": { "automationId": "result" },
                "condition": "unique"
            }
        })
    }

    #[test]
    fn transition_reuses_sequence_and_exact_postcondition_accessors() {
        let Ok(input) = UixPointerSequenceTransitionInput::parse(&valid_input()) else {
            panic!("有效 pointer sequence transition 必须解析");
        };
        assert_eq!(input.coordinate_space().as_str(), "client-logical-px");
        assert_eq!(input.steps_requested(), 2);
        assert_eq!(input.moves_requested(), 1);
        assert_eq!(input.clicks_requested(), 1);
        assert_eq!(input.interval_ms(), 0);
        assert_eq!(input.planned_duration_ms(), 0);
        assert_eq!(input.timeout_ms(), 30_000);
        assert_eq!(input.postcondition().condition().as_str(), "unique");
        assert_eq!(input.steps()[1].provider_action(), "click_at");
    }

    #[test]
    fn transition_accepts_missing_postcondition_and_explicit_bounded_timing() {
        let mut value = valid_input();
        value["postcondition"]["condition"] = json!("missing");
        value["intervalMs"] = json!(100);
        value["timeoutMs"] = json!(300);
        let Ok(input) = UixPointerSequenceTransitionInput::parse(&value) else {
            panic!("有界 missing transition 必须解析");
        };
        assert_eq!(input.postcondition().condition().as_str(), "missing");
        assert_eq!(input.interval_ms(), 100);
        assert_eq!(input.planned_duration_ms(), 100);
        assert_eq!(input.timeout_ms(), 300);
    }

    #[test]
    fn transition_rejects_unknown_null_empty_selector_and_invalid_timing() {
        let mut unknown = valid_input();
        unknown["unexpected"] = json!(true);
        assert!(UixPointerSequenceTransitionInput::parse(&unknown).is_err());

        let mut null_field = valid_input();
        null_field["intervalMs"] = Value::Null;
        assert!(UixPointerSequenceTransitionInput::parse(&null_field).is_err());

        let mut empty_selector = valid_input();
        empty_selector["postcondition"]["selector"] = json!({});
        assert!(UixPointerSequenceTransitionInput::parse(&empty_selector).is_err());

        let mut null_selector_field = valid_input();
        null_selector_field["postcondition"]["selector"]["role"] = Value::Null;
        assert!(UixPointerSequenceTransitionInput::parse(&null_selector_field).is_err());

        let mut float_timeout = valid_input();
        float_timeout["timeoutMs"] = json!(100.5);
        assert!(UixPointerSequenceTransitionInput::parse(&float_timeout).is_err());
    }

    #[test]
    fn transition_rejects_sequence_without_both_move_and_click() {
        let mut only_move = valid_input();
        only_move["steps"] = json!([
            { "type": "move", "x": 1, "y": 2 },
            { "type": "move", "x": 3, "y": 4 }
        ]);
        assert!(UixPointerSequenceTransitionInput::parse(&only_move).is_err());

        let mut invalid_step = valid_input();
        invalid_step["steps"][0]["type"] = json!("drag");
        assert!(UixPointerSequenceTransitionInput::parse(&invalid_step).is_err());
    }
}
