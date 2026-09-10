//! UIX 协作式应用内拖拽与提交后条件同步输入契约 Component。

use serde_json::Value;

use super::{
    uix_element_transition_contract::UixElementTransitionPostcondition,
    uix_pointer_drag_contract::{
        UixPointerDragCoordinateSpace, UixPointerDragInput, UixPointerDragPoint,
    },
};

const INPUT_SCHEMA_ERROR: &str =
    "UIX pointer drag transition input violates schema://ui/pointer-drag-transition/v1.";
const POSTCONDITION_ERROR: &str =
    "UIX pointer drag transition postcondition violates its bounded contract.";

/// 保存严格验证后的固定左键拖拽与提交后语义条件。
#[derive(Clone, Debug)]
pub(crate) struct UixPointerDragTransitionInput {
    drag: UixPointerDragInput,
    postcondition: UixElementTransitionPostcondition,
}

impl UixPointerDragTransitionInput {
    /// 严格解析封闭输入，复用 drag Component 的坐标和时限校验。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let Some(object) = value.as_object() else {
            return Err(INPUT_SCHEMA_ERROR);
        };
        if object.keys().any(|field| {
            !matches!(
                field.as_str(),
                "coordinateSpace"
                    | "start"
                    | "end"
                    | "samples"
                    | "durationMs"
                    | "timeoutMs"
                    | "postcondition"
            )
        }) {
            return Err(INPUT_SCHEMA_ERROR);
        }
        let Some(raw_postcondition) = object.get("postcondition") else {
            return Err(POSTCONDITION_ERROR);
        };
        let Some(selector) = raw_postcondition.get("selector").and_then(Value::as_object) else {
            return Err(POSTCONDITION_ERROR);
        };
        if selector.values().any(Value::is_null) {
            return Err("UIX pointer drag transition selector fields cannot be null.");
        }
        let postcondition =
            serde_json::from_value::<UixElementTransitionPostcondition>(raw_postcondition.clone())
                .map_err(|_| POSTCONDITION_ERROR)?;
        postcondition.selector().validate()?;

        // postcondition 由本 Component 独立验证，剩余字段交给既有 drag parser。
        let mut drag_object = object.clone();
        drag_object.remove("postcondition");
        let drag = UixPointerDragInput::parse(&Value::Object(drag_object))?;
        Ok(Self {
            drag,
            postcondition,
        })
    }

    /// 返回通过既有 drag Component 验证的完整拖拽参数。
    pub(crate) const fn drag(&self) -> &UixPointerDragInput {
        &self.drag
    }

    /// 返回动作提交后必须匹配的 exact-AND 条件。
    pub(crate) fn postcondition(&self) -> &UixElementTransitionPostcondition {
        &self.postcondition
    }

    /// 返回固定的 UIX 客户区 logical px 坐标空间。
    pub(crate) const fn coordinate_space(&self) -> UixPointerDragCoordinateSpace {
        self.drag.coordinate_space()
    }

    /// 返回拖拽起点。
    pub(crate) const fn start(&self) -> UixPointerDragPoint {
        self.drag.start()
    }

    /// 返回拖拽终点。
    pub(crate) const fn end(&self) -> UixPointerDragPoint {
        self.drag.end()
    }

    /// 返回请求的有界移动采样数。
    pub(crate) const fn samples(&self) -> u16 {
        self.drag.samples()
    }

    /// 返回拖拽持续时间。
    pub(crate) const fn duration_ms(&self) -> u32 {
        self.drag.duration_ms()
    }

    /// 返回覆盖拖拽提交与条件等待的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.drag.timeout_ms()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn valid_input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 10.5, "y": 20.25 },
            "end": { "x": 100, "y": 200 },
            "postcondition": {
                "selector": { "automationId": "drag-target", "role": "status" },
                "condition": "unique"
            }
        })
    }

    #[test]
    fn drag_transition_reuses_bounded_drag_and_exact_postcondition() {
        let Ok(input) = UixPointerDragTransitionInput::parse(&valid_input()) else {
            panic!("有效 drag transition 必须解析");
        };
        assert_eq!(input.coordinate_space().as_str(), "client-logical-px");
        assert_eq!(input.start().x(), 10.5);
        assert_eq!(input.end().y(), 200.0);
        assert_eq!(input.samples(), 12);
        assert_eq!(input.duration_ms(), 250);
        assert_eq!(input.timeout_ms(), 30_000);
        assert_eq!(input.drag().button(), "left");
        assert_eq!(input.postcondition().condition().as_str(), "unique");
        assert!(input.postcondition().selector().matches(
            Some("drag-target"),
            "status",
            "拖拽目标",
            false,
            true,
            &[]
        ));
    }

    #[test]
    fn drag_transition_accepts_missing_and_duration_timeout_boundary() {
        let mut input = valid_input();
        input["postcondition"]["condition"] = json!("missing");
        input["samples"] = json!(64);
        input["durationMs"] = json!(5_000);
        input["timeoutMs"] = json!(5_100);
        let Ok(parsed) = UixPointerDragTransitionInput::parse(&input) else {
            panic!("missing 条件与 duration+100 timeout 边界必须解析");
        };
        assert_eq!(parsed.postcondition().condition().as_str(), "missing");
        assert_eq!(parsed.samples(), 64);
        assert_eq!(parsed.duration_ms(), 5_000);
        assert_eq!(parsed.timeout_ms(), 5_100);
    }

    #[test]
    fn drag_transition_rejects_closed_postcondition_and_invalid_drag_fields() {
        let mut unknown = valid_input();
        unknown["unexpected"] = json!(true);
        assert!(UixPointerDragTransitionInput::parse(&unknown).is_err());

        let mut null_postcondition = valid_input();
        null_postcondition["postcondition"] = Value::Null;
        assert!(UixPointerDragTransitionInput::parse(&null_postcondition).is_err());

        let mut empty_selector = valid_input();
        empty_selector["postcondition"]["selector"] = json!({});
        assert!(UixPointerDragTransitionInput::parse(&empty_selector).is_err());

        let mut null_selector_field = valid_input();
        null_selector_field["postcondition"]["selector"]["role"] = Value::Null;
        assert!(UixPointerDragTransitionInput::parse(&null_selector_field).is_err());

        let mut float_timeout = valid_input();
        float_timeout["timeoutMs"] = json!(300.5);
        assert!(UixPointerDragTransitionInput::parse(&float_timeout).is_err());

        let mut unbalanced = valid_input();
        unbalanced["start"]["pointerDown"] = json!(true);
        assert!(UixPointerDragTransitionInput::parse(&unbalanced).is_err());

        let mut insufficient_timeout = valid_input();
        insufficient_timeout["durationMs"] = json!(5_000);
        insufficient_timeout["timeoutMs"] = json!(5_099);
        assert!(UixPointerDragTransitionInput::parse(&insufficient_timeout).is_err());
    }
}
