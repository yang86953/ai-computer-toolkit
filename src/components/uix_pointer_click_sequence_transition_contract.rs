//! UIX 应用内左键点击序列及提交后语义条件的版本一输入契约 Component。

use serde_json::Value;

use super::{
    uix_element_transition_contract::UixElementTransitionPostcondition,
    uix_pointer_click_sequence_contract::UixPointerClickSequenceInput,
    uix_pointer_input_contract::{UixPointerCoordinateSpace, UixPointerPoint},
};

/// 保存严格验证后的应用内普通左键点击序列 transition 请求。
#[derive(Clone, Debug)]
pub(crate) struct UixPointerClickSequenceTransitionInput {
    sequence: UixPointerClickSequenceInput,
    postcondition: UixElementTransitionPostcondition,
}

impl UixPointerClickSequenceTransitionInput {
    /// 严格解析封闭输入，并把点击点及时序校验委托给既有 Component。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let Some(object) = value.as_object() else {
            return Err("UIX pointer click sequence transition input must be an object.");
        };
        if object.keys().any(|field| {
            !matches!(
                field.as_str(),
                "coordinateSpace" | "clicks" | "intervalMs" | "timeoutMs" | "postcondition"
            )
        }) || contains_null(value)
        {
            return Err(
                "UIX pointer click sequence transition input violates its closed contract.",
            );
        }

        let Some(postcondition_value) = object.get("postcondition") else {
            return Err("UIX pointer click sequence transition requires a postcondition.");
        };
        let Some(postcondition_object) = postcondition_value.as_object() else {
            return Err("UIX pointer click sequence transition postcondition must be an object.");
        };
        let Some(selector) = postcondition_object
            .get("selector")
            .and_then(Value::as_object)
        else {
            return Err("UIX pointer click sequence transition requires a selector object.");
        };
        if selector.is_empty() {
            return Err("UIX pointer click sequence transition selector cannot be empty.");
        }
        let postcondition = serde_json::from_value::<UixElementTransitionPostcondition>(
            postcondition_value.clone(),
        )
        .map_err(
            |_| "UIX pointer click sequence transition postcondition violates its contract.",
        )?;
        postcondition.selector().validate()?;

        let mut sequence_object = object.clone();
        sequence_object.remove("postcondition");
        let sequence = UixPointerClickSequenceInput::parse(&Value::Object(sequence_object))?;
        Ok(Self {
            sequence,
            postcondition,
        })
    }

    /// 返回严格验证后的基础 pointer click sequence 输入。
    pub(crate) fn sequence(&self) -> &UixPointerClickSequenceInput {
        &self.sequence
    }

    /// 返回提交后必须满足的 exact-AND 语义条件。
    pub(crate) fn postcondition(&self) -> &UixElementTransitionPostcondition {
        &self.postcondition
    }

    /// 返回 provider-neutral 的完整点击点顺序。
    pub(crate) fn clicks(&self) -> &[UixPointerPoint] {
        self.sequence.clicks()
    }

    /// 返回请求中的点击数量。
    pub(crate) fn clicks_requested(&self) -> usize {
        self.sequence.clicks_requested()
    }

    /// 返回固定的 provider-neutral 动作名。
    pub(crate) const fn action(&self) -> &'static str {
        "click-sequence"
    }

    /// 返回固定的客户区 logical px 坐标空间。
    pub(crate) const fn coordinate_space(&self) -> UixPointerCoordinateSpace {
        self.sequence.coordinate_space()
    }

    /// 返回相邻点击之间的有界间隔。
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
            "clicks": [
                { "x": 10.5, "y": 20.25 },
                { "x": 30.0, "y": 40.0 }
            ],
            "postcondition": {
                "selector": { "automationId": "result" },
                "condition": condition
            }
        })
    }

    #[test]
    fn transition_reuses_click_sequence_and_exact_postcondition() {
        let Ok(input) = UixPointerClickSequenceTransitionInput::parse(&valid_input("unique"))
        else {
            panic!("有效 pointer click sequence transition 必须解析");
        };
        assert_eq!(input.action(), "click-sequence");
        assert_eq!(input.coordinate_space().as_str(), "client-logical-px");
        assert_eq!(input.clicks_requested(), 2);
        assert_eq!(input.clicks()[0].x(), 10.5);
        assert_eq!(input.clicks()[1].y(), 40.0);
        assert_eq!(input.postcondition().condition().as_str(), "unique");
        assert_eq!(input.interval_ms(), 0);
        assert_eq!(input.planned_duration_ms(), 0);
        assert_eq!(input.timeout_ms(), 30_000);
    }

    #[test]
    fn transition_accepts_missing_and_bounded_timing() {
        let mut value = valid_input("missing");
        value["intervalMs"] = json!(100);
        value["timeoutMs"] = json!(200);
        let Ok(input) = UixPointerClickSequenceTransitionInput::parse(&value) else {
            panic!("有界 missing transition 必须解析");
        };
        assert_eq!(input.postcondition().condition().as_str(), "missing");
        assert_eq!(input.interval_ms(), 100);
        assert_eq!(input.planned_duration_ms(), 100);
        assert_eq!(input.timeout_ms(), 200);
    }

    #[test]
    fn transition_rejects_unknown_null_invalid_point_and_postcondition() {
        let mut unknown = valid_input("unique");
        unknown["unexpected"] = json!(true);
        assert!(UixPointerClickSequenceTransitionInput::parse(&unknown).is_err());

        let mut nested_null = valid_input("unique");
        nested_null["clicks"][0]["x"] = Value::Null;
        assert!(UixPointerClickSequenceTransitionInput::parse(&nested_null).is_err());

        let mut invalid_point = valid_input("unique");
        invalid_point["clicks"][0]["x"] = json!(65_535.1);
        assert!(UixPointerClickSequenceTransitionInput::parse(&invalid_point).is_err());

        let mut invalid_condition = valid_input("unique");
        invalid_condition["postcondition"]["condition"] = json!("stable");
        assert!(UixPointerClickSequenceTransitionInput::parse(&invalid_condition).is_err());

        let mut empty_selector = valid_input("unique");
        empty_selector["postcondition"]["selector"] = json!({});
        assert!(UixPointerClickSequenceTransitionInput::parse(&empty_selector).is_err());

        let mut too_short = valid_input("unique");
        too_short["intervalMs"] = json!(100);
        too_short["timeoutMs"] = json!(199);
        assert!(UixPointerClickSequenceTransitionInput::parse(&too_short).is_err());
    }

    #[test]
    fn transition_rejects_non_click_fields_and_empty_sequence() {
        let mut empty = valid_input("unique");
        empty["clicks"] = json!([]);
        assert!(UixPointerClickSequenceTransitionInput::parse(&empty).is_err());

        let mut wrong_action = valid_input("unique");
        wrong_action["clicks"][0]["button"] = json!("right");
        assert!(UixPointerClickSequenceTransitionInput::parse(&wrong_action).is_err());

        let mut wrong_space = valid_input("unique");
        wrong_space["coordinateSpace"] = json!("screen-physical-px");
        assert!(UixPointerClickSequenceTransitionInput::parse(&wrong_space).is_err());
    }
}
