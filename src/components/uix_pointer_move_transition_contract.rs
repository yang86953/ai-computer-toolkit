//! UIX 协作式应用内单次 pointer_move 与提交后条件同步输入契约 Component。

use serde::Deserialize;
use serde_json::Value;

use super::{
    uix_element_transition_contract::UixElementTransitionPostcondition,
    uix_pointer_input_contract::{UixPointerCoordinateSpace, UixPointerPoint},
};

const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存严格验证后的单次 pointer_move 与提交后语义条件。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixPointerMoveTransitionInput {
    coordinate_space: UixPointerCoordinateSpace,
    #[serde(flatten)]
    point: UixPointerPoint,
    postcondition: UixElementTransitionPostcondition,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixPointerMoveTransitionInput {
    /// 严格解析封闭输入，不回显非法坐标、selector 或原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        if !has_only_pointer_move_transition_fields(value) {
            return Err(
                "UIX pointer move transition input violates schema://ui/pointer-move-transition/v1.",
            );
        }
        let Some(selector) = value
            .get("postcondition")
            .and_then(|postcondition| postcondition.get("selector"))
            .and_then(Value::as_object)
        else {
            return Err("UIX pointer move transition postcondition violates its bounded contract.");
        };
        if selector.values().any(Value::is_null) {
            return Err("UIX pointer move transition selector fields cannot be null.");
        }
        let input = serde_json::from_value::<Self>(value.clone()).map_err(|_| {
            "UIX pointer move transition input violates schema://ui/pointer-move-transition/v1."
        })?;
        input.point.validate()?;
        input.postcondition.selector().validate()?;
        if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms) {
            return Err("UIX pointer move transition timeout is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回固定的 UIX 客户区 logical px 坐标空间。
    pub(crate) const fn coordinate_space(&self) -> UixPointerCoordinateSpace {
        self.coordinate_space
    }

    /// 返回 provider-neutral 横坐标。
    pub(crate) const fn x(&self) -> f32 {
        self.point.x()
    }

    /// 返回 provider-neutral 纵坐标。
    pub(crate) const fn y(&self) -> f32 {
        self.point.y()
    }

    /// 生成仅供认证 Agent Adapter 使用的应用内 pointer_move value。
    pub(crate) fn provider_value(&self) -> Value {
        self.point.provider_move_value()
    }

    /// 返回动作提交后必须满足的 exact-AND 条件。
    pub(crate) fn postcondition(&self) -> &UixElementTransitionPostcondition {
        &self.postcondition
    }

    /// 返回覆盖解析、认证、移动提交与条件等待的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }
}

fn has_only_pointer_move_transition_fields(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.keys().all(|field| {
            matches!(
                field.as_str(),
                "coordinateSpace" | "x" | "y" | "postcondition" | "timeoutMs"
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn valid_input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "x": 12.5,
            "y": 34.25,
            "postcondition": {
                "selector": { "automationId": "hover-target", "role": "button" },
                "condition": "unique"
            }
        })
    }

    #[test]
    fn move_transition_reuses_bounded_point_and_provider_move_value() {
        let Ok(input) = UixPointerMoveTransitionInput::parse(&valid_input()) else {
            panic!("有效 pointer move transition 必须解析");
        };
        assert_eq!(input.coordinate_space().as_str(), "client-logical-px");
        assert_eq!(input.x(), 12.5);
        assert_eq!(input.y(), 34.25);
        assert_eq!(input.provider_value()["kind"], "pointer_move");
        assert_eq!(input.provider_value()["x"], 12.5);
        assert_eq!(input.provider_value()["y"], 34.25);
        assert!(input.provider_value().get("button").is_none());
        assert_eq!(input.postcondition().condition().as_str(), "unique");
        assert!(input.postcondition().selector().matches(
            Some("hover-target"),
            "button",
            "悬停目标",
            false,
            true,
            &["invoke".to_owned()]
        ));
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn move_transition_accepts_missing_condition_and_timeout_boundaries() {
        let mut input = valid_input();
        input["postcondition"]["condition"] = json!("missing");
        input["timeoutMs"] = json!(MINIMUM_TIMEOUT_MS);
        let Ok(parsed) = UixPointerMoveTransitionInput::parse(&input) else {
            panic!("missing 条件与 timeout 下界必须解析");
        };
        assert_eq!(parsed.postcondition().condition().as_str(), "missing");
        assert_eq!(parsed.timeout_ms(), MINIMUM_TIMEOUT_MS);
    }

    #[test]
    fn move_transition_rejects_closed_invalid_and_non_move_fields() {
        for invalid in [
            json!({
                "coordinateSpace": "client-logical-px",
                "x": 1,
                "y": 2,
                "postcondition": { "selector": {}, "condition": "unique" }
            }),
            json!({
                "coordinateSpace": "client-logical-px",
                "x": -0.1,
                "y": 2,
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" }
            }),
            json!({
                "coordinateSpace": "client-logical-px",
                "x": 1,
                "y": 65_535.1,
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" }
            }),
            json!({
                "coordinateSpace": "screen-physical-px",
                "x": 1,
                "y": 2,
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" }
            }),
            json!({
                "coordinateSpace": "client-logical-px",
                "x": 1,
                "y": 2,
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" },
                "timeoutMs": 100.5
            }),
            json!({
                "coordinateSpace": "client-logical-px",
                "x": 1,
                "y": 2,
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" },
                "action": "move"
            }),
            json!({
                "coordinateSpace": "client-logical-px",
                "x": 1,
                "y": 2,
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" },
                "interpolation": true
            }),
            json!({
                "coordinateSpace": "client-logical-px",
                "x": 1,
                "y": 2,
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" },
                "drag": true
            }),
            json!({
                "coordinateSpace": "client-logical-px",
                "x": 1,
                "y": 2,
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" },
                "scroll": true
            }),
            json!({
                "coordinateSpace": "client-logical-px",
                "x": 1,
                "y": 2,
                "postcondition": { "selector": { "role": "button" }, "condition": "unique" },
                "intervalMs": 100
            }),
        ] {
            assert!(UixPointerMoveTransitionInput::parse(&invalid).is_err());
        }
    }
}
