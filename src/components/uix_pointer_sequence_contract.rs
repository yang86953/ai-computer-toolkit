//! UIX 协作式应用内混合指针序列版本一输入契约 Component。

use serde::Deserialize;
use serde_json::{Value, json};

use super::uix_pointer_input_contract::{UixPointerCoordinateSpace, UixPointerPoint};

const DEFAULT_INTERVAL_MS: u32 = 0;
const MAXIMUM_INTERVAL_MS: u32 = 500;
const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_STEPS: usize = 2;
const MAXIMUM_STEPS: usize = 64;
const MAXIMUM_PLANNED_DURATION_MS: u32 = 5_000;

const fn default_interval_ms() -> u32 {
    DEFAULT_INTERVAL_MS
}

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 表示应用内混合指针序列唯一允许的步骤类型。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum UixPointerSequenceStepKind {
    Move,
    Click,
}

impl UixPointerSequenceStepKind {
    /// 返回稳定公开步骤名。
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
}

/// 保存一个只含 type/x/y 的 provider-neutral 指针步骤。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixPointerSequenceStep {
    #[serde(rename = "type")]
    kind: UixPointerSequenceStepKind,
    #[serde(flatten)]
    point: UixPointerPoint,
}

impl UixPointerSequenceStep {
    /// 返回步骤类型。
    pub(crate) const fn kind(&self) -> UixPointerSequenceStepKind {
        self.kind
    }

    /// 返回稳定公开步骤名。
    pub(crate) const fn as_str(&self) -> &'static str {
        self.kind.as_str()
    }

    /// 返回 provider-neutral 横坐标。
    pub(crate) const fn x(&self) -> f32 {
        self.point.x()
    }

    /// 返回 provider-neutral 纵坐标。
    pub(crate) const fn y(&self) -> f32 {
        self.point.y()
    }

    /// 返回 UIX Agent hello 使用的窗口动作名。
    pub(crate) const fn provider_action(&self) -> &'static str {
        self.kind.provider_action()
    }

    /// 生成仅供认证 Agent Adapter 使用的 targetless action。
    pub(crate) fn provider_value(&self) -> Value {
        match self.kind {
            UixPointerSequenceStepKind::Move => json!({
                "kind": self.provider_action(),
                "x": self.x(),
                "y": self.y(),
            }),
            UixPointerSequenceStepKind::Click => self.point.provider_click_value(),
        }
    }

    fn validate(&self) -> Result<(), &'static str> {
        self.point.validate()
    }
}

/// 保存严格验证后的应用内混合悬停—点击序列请求。
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixPointerSequenceInput {
    coordinate_space: UixPointerCoordinateSpace,
    steps: Vec<UixPointerSequenceStep>,
    #[serde(default = "default_interval_ms")]
    interval_ms: u32,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixPointerSequenceInput {
    /// 严格解析公开输入且不回显非法原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX pointer sequence input violates schema://ui/pointer-sequence/v1.")?;
        let planned_duration_ms = input.planned_duration_ms();
        let move_count = input.moves_requested();
        let click_count = input.clicks_requested();
        if !(MINIMUM_STEPS..=MAXIMUM_STEPS).contains(&input.steps.len())
            || move_count == 0
            || click_count == 0
            || input.steps.iter().any(|step| step.validate().is_err())
            || input.interval_ms > MAXIMUM_INTERVAL_MS
            || planned_duration_ms > MAXIMUM_PLANNED_DURATION_MS
            || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
            || input.timeout_ms < planned_duration_ms.saturating_add(MINIMUM_TIMEOUT_MS)
        {
            return Err("UIX pointer sequence input is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回 provider-neutral 的完整步骤顺序。
    pub(crate) fn steps(&self) -> &[UixPointerSequenceStep] {
        &self.steps
    }

    /// 返回请求中的步骤数量。
    pub(crate) fn steps_requested(&self) -> usize {
        self.steps.len()
    }

    /// 返回请求中的 move 步骤数量。
    pub(crate) fn moves_requested(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.kind() == UixPointerSequenceStepKind::Move)
            .count()
    }

    /// 返回请求中的 click 步骤数量。
    pub(crate) fn clicks_requested(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.kind() == UixPointerSequenceStepKind::Click)
            .count()
    }

    /// 返回固定的 provider-neutral 动作名。
    pub(crate) const fn action(&self) -> &'static str {
        "pointer-sequence"
    }

    /// 返回固定的客户区 logical px 坐标空间。
    pub(crate) const fn coordinate_space(&self) -> UixPointerCoordinateSpace {
        self.coordinate_space
    }

    /// 返回相邻步骤之间的有界间隔。
    pub(crate) const fn interval_ms(&self) -> u32 {
        self.interval_ms
    }

    /// 返回 `(steps.len() - 1) * intervalMs` 的计划时长。
    pub(crate) fn planned_duration_ms(&self) -> u32 {
        (self.steps.len().saturating_sub(1) as u32).saturating_mul(self.interval_ms)
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

    fn mixed_input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "move", "x": 10.5, "y": 20.25 },
                { "type": "click", "x": 100, "y": 200 },
                { "type": "move", "x": 300, "y": 400 }
            ]
        })
    }

    #[test]
    fn pointer_sequence_reuses_point_and_applies_defaults() {
        let Ok(input) = UixPointerSequenceInput::parse(&mixed_input()) else {
            panic!("valid pointer sequence must parse");
        };
        assert_eq!(input.action(), "pointer-sequence");
        assert_eq!(input.coordinate_space().as_str(), "client-logical-px");
        assert_eq!(input.steps_requested(), 3);
        assert_eq!(input.moves_requested(), 2);
        assert_eq!(input.clicks_requested(), 1);
        assert_eq!(input.interval_ms(), DEFAULT_INTERVAL_MS);
        assert_eq!(input.planned_duration_ms(), 0);
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
        assert_eq!(input.steps()[0].as_str(), "move");
        assert_eq!(input.steps()[0].provider_action(), "pointer_move");
        assert_eq!(input.steps()[0].provider_value()["kind"], "pointer_move");
        assert_eq!(input.steps()[1].provider_action(), "click_at");
        assert_eq!(input.steps()[1].provider_value()["kind"], "click_at");
    }

    #[test]
    fn pointer_sequence_enforces_planned_duration_and_timeout_slack() {
        let mut input = mixed_input();
        input["intervalMs"] = json!(100);
        input["timeoutMs"] = json!(300);
        let Ok(parsed) = UixPointerSequenceInput::parse(&input) else {
            panic!("bounded pointer sequence must parse");
        };
        assert_eq!(parsed.planned_duration_ms(), 200);

        let mut input = mixed_input();
        input["intervalMs"] = json!(100);
        input["timeoutMs"] = json!(299);
        assert!(UixPointerSequenceInput::parse(&input).is_err());

        let long_steps = (0..12)
            .map(|index| {
                if index == 0 {
                    json!({ "type": "move", "x": 1, "y": 2 })
                } else {
                    json!({ "type": "click", "x": 1, "y": 2 })
                }
            })
            .collect::<Vec<_>>();
        let too_long = json!({
            "coordinateSpace": "client-logical-px",
            "steps": long_steps,
            "intervalMs": 500,
            "timeoutMs": 30_000
        });
        assert!(UixPointerSequenceInput::parse(&too_long).is_err());
    }

    #[test]
    fn pointer_sequence_requires_both_move_and_click_and_rejects_unknown_fields() {
        let only_move = json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "move", "x": 1, "y": 2 },
                { "type": "move", "x": 3, "y": 4 }
            ]
        });
        assert!(UixPointerSequenceInput::parse(&only_move).is_err());

        let only_click = json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "click", "x": 1, "y": 2 },
                { "type": "click", "x": 3, "y": 4 }
            ]
        });
        assert!(UixPointerSequenceInput::parse(&only_click).is_err());

        assert!(
            UixPointerSequenceInput::parse(&json!({
                "coordinateSpace": "client-logical-px",
                "steps": [
                    { "type": "move", "x": 1, "y": 2 },
                    { "type": "click", "x": 3, "y": 4, "button": "right" }
                ]
            }))
            .is_err()
        );
        assert!(
            UixPointerSequenceInput::parse(&json!({
                "coordinateSpace": "client-logical-px",
                "steps": [
                    { "type": "move", "x": 1, "y": 2 },
                    { "type": "drag", "x": 3, "y": 4 }
                ]
            }))
            .is_err()
        );
        assert!(
            UixPointerSequenceInput::parse(&json!({
                "coordinateSpace": "screen-physical-px",
                "steps": [
                    { "type": "move", "x": 1, "y": 2 },
                    { "type": "click", "x": 3, "y": 4 }
                ]
            }))
            .is_err()
        );
        assert!(
            UixPointerSequenceInput::parse(&json!({
                "coordinateSpace": "client-logical-px",
                "steps": [
                    { "type": "move", "x": 1, "y": 2 },
                    { "type": "click", "x": 3, "y": 4 }
                ],
                "unknown": true
            }))
            .is_err()
        );
    }
}
