//! 冻结 UIX 协作式应用内指针版本二输入。

use serde::Deserialize;
use serde_json::{Value, json};

const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
const MAXIMUM_CLIENT_COORDINATE: f32 = 65_535.0;

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 表示版本二唯一认证的指针坐标空间。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub(crate) enum UixPointerCoordinateSpace {
    /// 使用 UIX 跨平台客户区 logical px。
    #[serde(rename = "client-logical-px")]
    ClientLogicalPx,
}

impl UixPointerCoordinateSpace {
    /// 返回稳定公开坐标空间名称。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ClientLogicalPx => "client-logical-px",
        }
    }
}

/// 保存 provider-neutral 的客户区 logical px 点。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct UixPointerPoint {
    x: f32,
    y: f32,
}

impl UixPointerPoint {
    /// 严格解析并校验单个客户区点，不回显原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let point = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX pointer point violates its bounded contract.")?;
        point.validate()?;
        Ok(point)
    }

    /// 校验坐标 finite 且处于 provider-neutral 客户区范围。
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if !self.x.is_finite()
            || !self.y.is_finite()
            || !(0.0..=MAXIMUM_CLIENT_COORDINATE).contains(&self.x)
            || !(0.0..=MAXIMUM_CLIENT_COORDINATE).contains(&self.y)
        {
            return Err("UIX pointer point is outside its bounded contract.");
        }
        Ok(())
    }

    /// 返回 provider-neutral 横坐标。
    pub(crate) const fn x(&self) -> f32 {
        self.x
    }

    /// 返回 provider-neutral 纵坐标。
    pub(crate) const fn y(&self) -> f32 {
        self.y
    }

    /// 生成仅供认证 Agent Adapter 使用的普通左键 click value。
    pub(crate) fn provider_click_value(&self) -> Value {
        json!({
            "kind": "click_at",
            "x": self.x,
            "y": self.y,
        })
    }

    /// 生成仅供认证 Agent Adapter 使用的应用内 pointer_move value。
    pub(crate) fn provider_move_value(&self) -> Value {
        json!({
            "kind": "pointer_move",
            "x": self.x,
            "y": self.y,
        })
    }

    /// 返回固定 click_at provider value 的兼容别名。
    pub(crate) fn provider_value(&self) -> Value {
        self.provider_click_value()
    }
}

/// 表示无跨请求按钮所有权的应用内指针动作。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum UixPointerAction {
    Move,
    Click,
}

impl UixPointerAction {
    /// 返回稳定公开动作名。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Click => "click",
        }
    }

    /// 返回 UIX Agent hello 使用的窗口动作名。
    pub(crate) const fn provider_action(self) -> &'static str {
        match self {
            Self::Move => "pointer_move",
            Self::Click => "click_at",
        }
    }

    /// 返回成功所证明已经消费的应用内部事件集合。
    pub(crate) const fn handled_events(self) -> &'static [&'static str] {
        match self {
            Self::Move => &["pointer-move"],
            Self::Click => &["pointer-down", "pointer-up"],
        }
    }
}

/// 保存严格解析后的单次应用内指针请求。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixPointerInput {
    action: UixPointerAction,
    coordinate_space: UixPointerCoordinateSpace,
    #[serde(flatten)]
    point: UixPointerPoint,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixPointerInput {
    /// 严格解析公开输入且不回显非法坐标。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        if !has_only_pointer_input_fields(value) {
            return Err("UIX pointer input violates schema://ui/pointer-input/v2.");
        }
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX pointer input violates schema://ui/pointer-input/v2.")?;
        if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms) {
            return Err("UIX pointer input is outside its bounded contract.");
        }
        input.point.validate()?;
        Ok(input)
    }

    pub(crate) const fn action(&self) -> UixPointerAction {
        self.action
    }

    pub(crate) const fn coordinate_space(&self) -> UixPointerCoordinateSpace {
        self.coordinate_space
    }

    pub(crate) const fn x(&self) -> f32 {
        self.point.x()
    }

    pub(crate) const fn y(&self) -> f32 {
        self.point.y()
    }

    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }

    /// 生成仅供认证 Agent Adapter 使用的 targetless action。
    pub(crate) fn provider_value(&self) -> Value {
        match self.action {
            UixPointerAction::Click => self.point.provider_click_value(),
            UixPointerAction::Move => json!({
                "kind": self.action.provider_action(),
                "x": self.x(),
                "y": self.y(),
            }),
        }
    }
}

fn has_only_pointer_input_fields(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.keys().all(|field| {
            matches!(
                field.as_str(),
                "action" | "coordinateSpace" | "x" | "y" | "timeoutMs"
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_v2_is_logical_bounded_and_has_no_button_ownership() {
        let Ok(input) = UixPointerInput::parse(&json!({
            "action": "click",
            "coordinateSpace": "client-logical-px",
            "x": 20.5,
            "y": 30.25
        })) else {
            panic!("valid logical click must parse");
        };
        assert_eq!(input.action().provider_action(), "click_at");
        assert_eq!(
            input.action().handled_events(),
            ["pointer-down", "pointer-up"]
        );
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
        assert!(
            UixPointerInput::parse(&json!({
                "action": "pointer-down",
                "coordinateSpace": "client-logical-px",
                "x": 1,
                "y": 2
            }))
            .is_err()
        );
        assert!(
            UixPointerInput::parse(&json!({
                "action": "move",
                "coordinateSpace": "screen-physical-px",
                "x": 1,
                "y": 2
            }))
            .is_err()
        );
        assert_eq!(input.provider_value()["kind"], "click_at");
    }

    #[test]
    fn flattened_pointer_point_keeps_unknown_fields_closed() {
        assert!(
            UixPointerPoint::parse(&json!({
                "x": 1,
                "y": 2,
                "unexpected": true
            }))
            .is_err()
        );
        assert!(
            UixPointerInput::parse(&json!({
                "action": "click",
                "coordinateSpace": "client-logical-px",
                "x": 1,
                "y": 2,
                "unexpected": true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<UixPointerInput>(json!({
                "action": "click",
                "coordinateSpace": "client-logical-px",
                "x": 1,
                "y": 2,
                "unexpected": true
            }))
            .is_err()
        );
    }

    #[test]
    fn pointer_point_rejects_non_finite_and_out_of_range_coordinates() {
        assert!(UixPointerPoint::parse(&json!({ "x": -0.1, "y": 1 })).is_err());
        assert!(UixPointerPoint::parse(&json!({ "x": 65_535.1, "y": 1 })).is_err());
        assert!(UixPointerPoint::parse(&json!({ "x": "NaN", "y": 1 })).is_err());
    }
}
