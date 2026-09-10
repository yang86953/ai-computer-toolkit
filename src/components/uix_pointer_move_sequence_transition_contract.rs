//! UIX 应用内多点悬停序列及提交后语义条件的版本一输入契约 Component。

use serde_json::Value;

use super::{
    uix_element_transition_contract::UixElementTransitionPostcondition,
    uix_pointer_input_contract::{UixPointerCoordinateSpace, UixPointerPoint},
    uix_pointer_move_sequence_contract::UixPointerMoveSequenceInput,
};

/// 保存严格验证后的应用内 pointer_move 序列 transition 请求。
#[derive(Clone, Debug)]
pub(crate) struct UixPointerMoveSequenceTransitionInput {
    sequence: UixPointerMoveSequenceInput,
    postcondition: UixElementTransitionPostcondition,
}

impl UixPointerMoveSequenceTransitionInput {
    /// 严格解析封闭输入，并把移动点及时序校验委托给既有 Component。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let Some(object) = value.as_object() else {
            return Err("UIX pointer move sequence transition input must be an object.");
        };
        if object.keys().any(|field| {
            !matches!(
                field.as_str(),
                "coordinateSpace" | "moves" | "intervalMs" | "timeoutMs" | "postcondition"
            )
        }) || contains_null(value)
        {
            return Err("UIX pointer move sequence transition input violates its closed contract.");
        }

        let Some(postcondition_value) = object.get("postcondition") else {
            return Err("UIX pointer move sequence transition requires a postcondition.");
        };
        let Some(postcondition_object) = postcondition_value.as_object() else {
            return Err("UIX pointer move sequence transition postcondition must be an object.");
        };
        let Some(selector) = postcondition_object
            .get("selector")
            .and_then(Value::as_object)
        else {
            return Err("UIX pointer move sequence transition requires a selector object.");
        };
        if selector.is_empty() {
            return Err("UIX pointer move sequence transition selector cannot be empty.");
        }
        let postcondition = serde_json::from_value::<UixElementTransitionPostcondition>(
            postcondition_value.clone(),
        )
        .map_err(|_| "UIX pointer move sequence transition postcondition violates its contract.")?;
        postcondition.selector().validate()?;

        let mut sequence_object = object.clone();
        sequence_object.remove("postcondition");
        let sequence = UixPointerMoveSequenceInput::parse(&Value::Object(sequence_object))?;
        Ok(Self {
            sequence,
            postcondition,
        })
    }

    /// 返回严格验证后的基础 pointer move sequence 输入。
    pub(crate) fn sequence(&self) -> &UixPointerMoveSequenceInput {
        &self.sequence
    }

    /// 返回提交后必须满足的 exact-AND 语义条件。
    pub(crate) fn postcondition(&self) -> &UixElementTransitionPostcondition {
        &self.postcondition
    }

    /// 返回 provider-neutral 的完整移动点顺序。
    pub(crate) fn moves(&self) -> &[UixPointerPoint] {
        self.sequence.moves()
    }

    /// 返回请求中的移动数量。
    pub(crate) fn moves_requested(&self) -> usize {
        self.sequence.moves_requested()
    }

    /// 返回固定的 provider-neutral 动作名。
    pub(crate) const fn action(&self) -> &'static str {
        "pointer-move-sequence"
    }

    /// 返回固定的客户区 logical px 坐标空间。
    pub(crate) const fn coordinate_space(&self) -> UixPointerCoordinateSpace {
        self.sequence.coordinate_space()
    }

    /// 返回相邻移动之间的有界间隔。
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
            "moves": [
                { "x": 10.5, "y": 20.25 },
                { "x": 30.0, "y": 40.0 },
                { "x": 50.0, "y": 60.0 }
            ],
            "postcondition": {
                "selector": { "automationId": "result" },
                "condition": condition
            }
        })
    }

    #[test]
    fn transition_reuses_move_sequence_and_exact_postcondition() {
        let Ok(input) = UixPointerMoveSequenceTransitionInput::parse(&valid_input("unique")) else {
            panic!("有效 pointer move sequence transition 必须解析");
        };
        assert_eq!(input.action(), "pointer-move-sequence");
        assert_eq!(input.coordinate_space().as_str(), "client-logical-px");
        assert_eq!(input.moves_requested(), 3);
        assert_eq!(input.moves()[0].x(), 10.5);
        assert_eq!(input.moves()[2].y(), 60.0);
        assert_eq!(input.postcondition().condition().as_str(), "unique");
        assert_eq!(input.interval_ms(), 0);
        assert_eq!(input.planned_duration_ms(), 0);
        assert_eq!(input.timeout_ms(), 30_000);
    }

    #[test]
    fn transition_accepts_missing_and_bounded_timing() {
        let mut value = valid_input("missing");
        value["intervalMs"] = json!(100);
        value["timeoutMs"] = json!(300);
        let Ok(input) = UixPointerMoveSequenceTransitionInput::parse(&value) else {
            panic!("有界 missing transition 必须解析");
        };
        assert_eq!(input.postcondition().condition().as_str(), "missing");
        assert_eq!(input.interval_ms(), 100);
        assert_eq!(input.planned_duration_ms(), 200);
        assert_eq!(input.timeout_ms(), 300);
    }

    #[test]
    fn transition_rejects_unknown_null_invalid_point_and_postcondition() {
        let mut unknown = valid_input("unique");
        unknown["unexpected"] = json!(true);
        assert!(UixPointerMoveSequenceTransitionInput::parse(&unknown).is_err());

        let mut nested_null = valid_input("unique");
        nested_null["moves"][0]["x"] = Value::Null;
        assert!(UixPointerMoveSequenceTransitionInput::parse(&nested_null).is_err());

        let mut invalid_point = valid_input("unique");
        invalid_point["moves"][0]["x"] = json!(65_535.1);
        assert!(UixPointerMoveSequenceTransitionInput::parse(&invalid_point).is_err());

        let mut invalid_condition = valid_input("unique");
        invalid_condition["postcondition"]["condition"] = json!("stable");
        assert!(UixPointerMoveSequenceTransitionInput::parse(&invalid_condition).is_err());

        let mut empty_selector = valid_input("unique");
        empty_selector["postcondition"]["selector"] = json!({});
        assert!(UixPointerMoveSequenceTransitionInput::parse(&empty_selector).is_err());

        let mut too_short = valid_input("unique");
        too_short["intervalMs"] = json!(100);
        too_short["timeoutMs"] = json!(299);
        assert!(UixPointerMoveSequenceTransitionInput::parse(&too_short).is_err());
    }

    #[test]
    fn transition_rejects_non_move_fields_and_short_sequence() {
        let mut too_short = valid_input("unique");
        too_short["moves"] = json!([{ "x": 1, "y": 2 }]);
        assert!(UixPointerMoveSequenceTransitionInput::parse(&too_short).is_err());

        let mut wrong_kind = valid_input("unique");
        wrong_kind["moves"][0]["kind"] = json!("pointer_move");
        assert!(UixPointerMoveSequenceTransitionInput::parse(&wrong_kind).is_err());

        let mut wrong_space = valid_input("unique");
        wrong_space["coordinateSpace"] = json!("screen-physical-px");
        assert!(UixPointerMoveSequenceTransitionInput::parse(&wrong_space).is_err());
    }
}
