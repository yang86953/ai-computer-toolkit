//! UIX 应用内键盘与指针混合序列及提交后语义条件的版本一输入契约 Component。

use serde_json::Value;

use super::{
    uix_element_transition_contract::UixElementTransitionPostcondition,
    uix_input_sequence_contract::{UixInputSequenceInput, UixInputSequenceStep},
    uix_pointer_input_contract::UixPointerCoordinateSpace,
};

/// 保存严格验证后的应用内混合输入序列 transition 请求。
#[derive(Clone, Debug)]
pub(crate) struct UixInputSequenceTransitionInput {
    sequence: UixInputSequenceInput,
    postcondition: UixElementTransitionPostcondition,
}

impl UixInputSequenceTransitionInput {
    /// 严格解析封闭输入，并把序列、按键、坐标校验委托给既有 Component。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let Some(object) = value.as_object() else {
            return Err("UIX input sequence transition input must be an object.");
        };
        if object.keys().any(|field| {
            !matches!(
                field.as_str(),
                "coordinateSpace" | "steps" | "intervalMs" | "timeoutMs" | "postcondition"
            )
        }) || contains_null(value)
        {
            return Err("UIX input sequence transition input violates its closed contract.");
        }

        let Some(postcondition_value) = object.get("postcondition") else {
            return Err("UIX input sequence transition requires a postcondition.");
        };
        let Some(postcondition_object) = postcondition_value.as_object() else {
            return Err("UIX input sequence transition postcondition must be an object.");
        };
        let Some(selector) = postcondition_object
            .get("selector")
            .and_then(Value::as_object)
        else {
            return Err("UIX input sequence transition requires a selector object.");
        };
        if selector.is_empty() {
            return Err("UIX input sequence transition selector cannot be empty.");
        }
        let postcondition = serde_json::from_value::<UixElementTransitionPostcondition>(
            postcondition_value.clone(),
        )
        .map_err(|_| "UIX input sequence transition postcondition violates its contract.")?;
        postcondition.selector().validate()?;

        let mut sequence_object = object.clone();
        sequence_object.remove("postcondition");
        let sequence = UixInputSequenceInput::parse(&Value::Object(sequence_object))?;
        Ok(Self {
            sequence,
            postcondition,
        })
    }

    /// 返回严格验证后的基础 input sequence 输入。
    pub(crate) fn sequence(&self) -> &UixInputSequenceInput {
        &self.sequence
    }

    /// 返回提交后必须满足的 exact-AND 语义条件。
    pub(crate) fn postcondition(&self) -> &UixElementTransitionPostcondition {
        &self.postcondition
    }

    /// 返回完整步骤顺序。
    pub(crate) fn steps(&self) -> &[UixInputSequenceStep] {
        self.sequence.steps()
    }

    /// 返回请求步骤总数。
    pub(crate) fn steps_requested(&self) -> usize {
        self.sequence.steps_requested()
    }

    /// 返回请求中的 press 步骤数量。
    pub(crate) fn presses_requested(&self) -> usize {
        self.sequence.presses_requested()
    }

    /// 返回请求中的 move 步骤数量。
    pub(crate) fn moves_requested(&self) -> usize {
        self.sequence.moves_requested()
    }

    /// 返回请求中的 click 步骤数量。
    pub(crate) fn clicks_requested(&self) -> usize {
        self.sequence.clicks_requested()
    }

    /// 返回固定的 provider-neutral 动作名。
    pub(crate) const fn action(&self) -> &'static str {
        "input-sequence"
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

/// 递归拒绝输入任何位置的显式 null，避免可选字段绕过闭合契约。
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

    fn valid_input(condition: &str) -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "press", "key": "enter", "modifiers": ["control"] },
                { "type": "move", "x": 10.5, "y": 20.25 },
                { "type": "click", "x": 30.0, "y": 40.0 }
            ],
            "postcondition": {
                "selector": { "automationId": "result" },
                "condition": condition
            }
        })
    }

    #[test]
    fn transition_reuses_mixed_sequence_and_exact_postcondition() {
        let Ok(input) = UixInputSequenceTransitionInput::parse(&valid_input("unique")) else {
            panic!("有效 input sequence transition 必须解析");
        };
        assert_eq!(input.action(), "input-sequence");
        assert_eq!(input.coordinate_space().as_str(), "client-logical-px");
        assert_eq!(input.steps_requested(), 3);
        assert_eq!(input.presses_requested(), 1);
        assert_eq!(input.moves_requested(), 1);
        assert_eq!(input.clicks_requested(), 1);
        assert_eq!(input.postcondition().condition().as_str(), "unique");
        assert_eq!(input.steps()[0].provider_action(), "press_key");
        assert_eq!(input.steps()[2].provider_action(), "click_at");
    }

    #[test]
    fn transition_accepts_missing_and_bounded_timing() {
        let mut value = valid_input("missing");
        value["intervalMs"] = json!(100);
        value["timeoutMs"] = json!(300);
        let Ok(input) = UixInputSequenceTransitionInput::parse(&value) else {
            panic!("有界 missing transition 必须解析");
        };
        assert_eq!(input.postcondition().condition().as_str(), "missing");
        assert_eq!(input.interval_ms(), 100);
        assert_eq!(input.planned_duration_ms(), 200);
        assert_eq!(input.timeout_ms(), 300);
    }

    #[test]
    fn transition_rejects_unknown_null_invalid_sequence_and_postcondition() {
        let mut unknown = valid_input("unique");
        unknown["unexpected"] = json!(true);
        assert!(UixInputSequenceTransitionInput::parse(&unknown).is_err());

        let mut nested_null = valid_input("unique");
        nested_null["steps"][1]["x"] = Value::Null;
        assert!(UixInputSequenceTransitionInput::parse(&nested_null).is_err());

        let mut only_press = valid_input("unique");
        only_press["steps"] = json!([
            { "type": "press", "key": "a" },
            { "type": "press", "key": "b" }
        ]);
        assert!(UixInputSequenceTransitionInput::parse(&only_press).is_err());

        let mut invalid_condition = valid_input("unique");
        invalid_condition["postcondition"]["condition"] = json!("stable");
        assert!(UixInputSequenceTransitionInput::parse(&invalid_condition).is_err());

        let mut empty_selector = valid_input("unique");
        empty_selector["postcondition"]["selector"] = json!({});
        assert!(UixInputSequenceTransitionInput::parse(&empty_selector).is_err());

        let mut too_short = valid_input("unique");
        too_short["timeoutMs"] = json!(299);
        too_short["intervalMs"] = json!(100);
        assert!(UixInputSequenceTransitionInput::parse(&too_short).is_err());
    }

    #[test]
    fn transition_reuses_base_component_coordinate_and_key_bounds() {
        let mut invalid_coordinate = valid_input("unique");
        invalid_coordinate["steps"][1]["x"] = json!(65_535.1);
        assert!(UixInputSequenceTransitionInput::parse(&invalid_coordinate).is_err());

        let mut invalid_key = valid_input("unique");
        invalid_key["steps"][0]["key"] = json!("f13");
        assert!(UixInputSequenceTransitionInput::parse(&invalid_key).is_err());
    }
}
