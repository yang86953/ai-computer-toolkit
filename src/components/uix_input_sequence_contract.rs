//! UIX 协作式应用内键盘与指针混合序列版本一输入契约 Component。

use serde::{Deserialize, de::Error as DeError};
use serde_json::{Map, Value, json};

use super::{
    uix_key_input_contract::UixKeyPress,
    uix_pointer_input_contract::{UixPointerCoordinateSpace, UixPointerPoint},
};

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

/// 混合序列唯一允许的步骤类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UixInputSequenceStepKind {
    Press,
    Move,
    Click,
}

impl UixInputSequenceStepKind {
    /// 返回稳定公开步骤名。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Press => "press",
            Self::Move => "move",
            Self::Click => "click",
        }
    }

    /// 返回 UIX Agent hello 使用的窗口动作名。
    pub(crate) const fn provider_action(self) -> &'static str {
        match self {
            Self::Press => "press_key",
            Self::Move => "pointer_move",
            Self::Click => "click_at",
        }
    }
}

/// 保存一个严格闭合且已复用基础 Component 校验的混合步骤。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum UixInputSequenceStep {
    Press(UixKeyPress),
    Move(UixPointerPoint),
    Click(UixPointerPoint),
}

/// 用于识别步骤标签与字段集合的闭合 wire 结构；具体键和坐标仍由基础 Component 校验。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UixInputSequenceStepWire {
    #[serde(rename = "type")]
    kind: String,
    key: Option<Value>,
    modifiers: Option<Value>,
    x: Option<Value>,
    y: Option<Value>,
}

impl UixInputSequenceStep {
    /// 严格解析一个 press、move 或 click 步骤，不回显非法原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let wire = serde_json::from_value::<UixInputSequenceStepWire>(value.clone())
            .map_err(|_| "UIX input sequence step violates its bounded contract.")?;
        match wire.kind.as_str() {
            "press" => {
                if wire.key.is_none() || wire.x.is_some() || wire.y.is_some() {
                    return Err("UIX input sequence press step is outside its bounded contract.");
                }
                let Some(key) = wire.key else {
                    return Err("UIX input sequence press step requires key.");
                };
                let mut object = Map::new();
                object.insert("key".to_owned(), key);
                if let Some(modifiers) = wire.modifiers {
                    object.insert("modifiers".to_owned(), modifiers);
                }
                let press = UixKeyPress::parse(&Value::Object(object))?;
                Ok(Self::Press(press))
            }
            "move" | "click" => {
                if wire.key.is_some() || wire.modifiers.is_some() {
                    return Err("UIX input sequence pointer step contains forbidden key fields.");
                }
                let (Some(x), Some(y)) = (wire.x, wire.y) else {
                    return Err("UIX input sequence pointer step requires x and y.");
                };
                let mut object = Map::new();
                object.insert("x".to_owned(), x);
                object.insert("y".to_owned(), y);
                let point = UixPointerPoint::parse(&Value::Object(object))?;
                if wire.kind == "move" {
                    Ok(Self::Move(point))
                } else {
                    Ok(Self::Click(point))
                }
            }
            _ => Err("UIX input sequence step has an unsupported type."),
        }
    }

    /// 返回步骤类别。
    pub(crate) const fn kind(&self) -> UixInputSequenceStepKind {
        match self {
            Self::Press(_) => UixInputSequenceStepKind::Press,
            Self::Move(_) => UixInputSequenceStepKind::Move,
            Self::Click(_) => UixInputSequenceStepKind::Click,
        }
    }

    /// 返回稳定公开步骤名。
    pub(crate) const fn as_str(&self) -> &'static str {
        self.kind().as_str()
    }

    /// 返回 UIX Agent hello 使用的窗口动作名。
    pub(crate) const fn provider_action(&self) -> &'static str {
        self.kind().provider_action()
    }

    /// 返回 press 步骤的 provider-neutral 键名；指针步骤返回 None。
    pub(crate) fn key(&self) -> Option<&str> {
        match self {
            Self::Press(press) => Some(press.key()),
            Self::Move(_) | Self::Click(_) => None,
        }
    }

    /// 返回 press 步骤的 provider-neutral 修饰键；指针步骤返回 None。
    pub(crate) fn modifiers(&self) -> Option<&[String]> {
        match self {
            Self::Press(press) => Some(press.modifiers()),
            Self::Move(_) | Self::Click(_) => None,
        }
    }

    /// 返回 press 步骤的 UIX Agent 键名；指针步骤返回 None。
    pub(crate) fn provider_key(&self) -> Option<&str> {
        match self {
            Self::Press(press) => Some(press.provider_key()),
            Self::Move(_) | Self::Click(_) => None,
        }
    }

    /// 返回 press 步骤的 UIX Agent 修饰键；指针步骤返回 None。
    pub(crate) fn provider_modifiers(&self) -> Option<Vec<&str>> {
        match self {
            Self::Press(press) => Some(press.provider_modifiers()),
            Self::Move(_) | Self::Click(_) => None,
        }
    }

    /// 返回指针步骤的 provider-neutral横坐标；press 步骤返回 None。
    pub(crate) const fn x(&self) -> Option<f32> {
        match self {
            Self::Press(_) => None,
            Self::Move(point) | Self::Click(point) => Some(point.x()),
        }
    }

    /// 返回指针步骤的 provider-neutral 纵坐标；press 步骤返回 None。
    pub(crate) const fn y(&self) -> Option<f32> {
        match self {
            Self::Press(_) => None,
            Self::Move(point) | Self::Click(point) => Some(point.y()),
        }
    }

    /// 生成仅供认证 Agent Adapter 使用的 targetless action。
    pub(crate) fn provider_value(&self) -> Value {
        match self {
            Self::Press(press) => press.provider_value(),
            Self::Move(point) => json!({
                "kind": "pointer_move",
                "x": point.x(),
                "y": point.y(),
            }),
            Self::Click(point) => point.provider_click_value(),
        }
    }

    /// 生成不含 provider 身份的公开步骤值。
    pub(crate) fn public_value(&self) -> Value {
        match self {
            Self::Press(_) => json!({
                "type": self.as_str(),
                "key": self.key(),
                "modifiers": self.modifiers(),
            }),
            Self::Move(_) | Self::Click(_) => json!({
                "type": self.as_str(),
                "x": self.x(),
                "y": self.y(),
            }),
        }
    }
}

impl<'de> Deserialize<'de> for UixInputSequenceStep {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

/// 保存严格验证后的应用内键盘与指针混合序列请求。
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixInputSequenceInput {
    coordinate_space: UixPointerCoordinateSpace,
    steps: Vec<UixInputSequenceStep>,
    #[serde(default = "default_interval_ms")]
    interval_ms: u32,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixInputSequenceInput {
    /// 严格解析公开输入，且不回显非法原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX input sequence input violates schema://ui/input-sequence/v1.")?;
        let planned_duration_ms = input.planned_duration_ms();
        let presses = input.presses_requested();
        let pointers = input
            .moves_requested()
            .saturating_add(input.clicks_requested());
        if !(MINIMUM_STEPS..=MAXIMUM_STEPS).contains(&input.steps.len())
            || presses == 0
            || pointers == 0
            || input.interval_ms > MAXIMUM_INTERVAL_MS
            || planned_duration_ms > MAXIMUM_PLANNED_DURATION_MS
            || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
            || input.timeout_ms < planned_duration_ms.saturating_add(MINIMUM_TIMEOUT_MS)
        {
            return Err("UIX input sequence input is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回 provider-neutral 的完整步骤顺序。
    pub(crate) fn steps(&self) -> &[UixInputSequenceStep] {
        &self.steps
    }

    /// 返回请求中的步骤数量。
    pub(crate) fn steps_requested(&self) -> usize {
        self.steps.len()
    }

    /// 返回请求中的 press 步骤数量。
    pub(crate) fn presses_requested(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.kind() == UixInputSequenceStepKind::Press)
            .count()
    }

    /// 返回请求中的 move 步骤数量。
    pub(crate) fn moves_requested(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.kind() == UixInputSequenceStepKind::Move)
            .count()
    }

    /// 返回请求中的 click 步骤数量。
    pub(crate) fn clicks_requested(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.kind() == UixInputSequenceStepKind::Click)
            .count()
    }

    /// 返回固定的 provider-neutral 动作名。
    pub(crate) const fn action(&self) -> &'static str {
        "input-sequence"
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

    fn valid_input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "press", "key": "a", "modifiers": ["control"] },
                { "type": "move", "x": 10.5, "y": 20.25 },
                { "type": "click", "x": 100, "y": 200 }
            ]
        })
    }

    #[test]
    fn mixed_input_reuses_key_and_pointer_components() {
        let Ok(input) = UixInputSequenceInput::parse(&valid_input()) else {
            panic!("有效混合输入必须解析");
        };
        assert_eq!(input.action(), "input-sequence");
        assert_eq!(input.coordinate_space().as_str(), "client-logical-px");
        assert_eq!(input.steps_requested(), 3);
        assert_eq!(input.presses_requested(), 1);
        assert_eq!(input.moves_requested(), 1);
        assert_eq!(input.clicks_requested(), 1);
        assert_eq!(input.interval_ms(), DEFAULT_INTERVAL_MS);
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
        assert_eq!(input.steps()[0].provider_action(), "press_key");
        assert_eq!(input.steps()[0].provider_value()["key"], "a");
        assert_eq!(input.steps()[1].provider_action(), "pointer_move");
        assert_eq!(input.steps()[2].provider_action(), "click_at");
        assert_eq!(input.steps()[0].key(), Some("a"));
        assert_eq!(input.steps()[1].x(), Some(10.5));
    }

    #[test]
    fn mixed_input_requires_both_modalities_and_bounded_timing() {
        let only_press = json!({
            "coordinateSpace": "client-logical-px",
            "steps": [{ "type": "press", "key": "a" }, { "type": "press", "key": "b" }]
        });
        assert!(UixInputSequenceInput::parse(&only_press).is_err());

        let only_pointer = json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "move", "x": 1, "y": 2 },
                { "type": "click", "x": 3, "y": 4 }
            ]
        });
        assert!(UixInputSequenceInput::parse(&only_pointer).is_err());

        let mut too_long = valid_input();
        too_long["steps"] = json!([
            { "type": "press", "key": "a" },
            { "type": "move", "x": 1, "y": 2 },
            { "type": "move", "x": 3, "y": 4 },
            { "type": "move", "x": 5, "y": 6 },
            { "type": "move", "x": 7, "y": 8 },
            { "type": "move", "x": 9, "y": 10 },
            { "type": "move", "x": 11, "y": 12 },
            { "type": "move", "x": 13, "y": 14 },
            { "type": "move", "x": 15, "y": 16 },
            { "type": "move", "x": 17, "y": 18 },
            { "type": "move", "x": 19, "y": 20 },
            { "type": "click", "x": 21, "y": 22 }
        ]);
        too_long["intervalMs"] = json!(500);
        too_long["timeoutMs"] = json!(30_000);
        assert!(UixInputSequenceInput::parse(&too_long).is_err());

        let mut too_short = valid_input();
        too_short["intervalMs"] = json!(100);
        too_short["timeoutMs"] = json!(299);
        assert!(UixInputSequenceInput::parse(&too_short).is_err());
    }

    #[test]
    fn mixed_input_is_closed_and_rejects_cross_step_fields() {
        let mut unknown = valid_input();
        unknown["unexpected"] = json!(true);
        assert!(UixInputSequenceInput::parse(&unknown).is_err());

        let mut press_with_point = valid_input();
        press_with_point["steps"][0]["x"] = json!(1);
        assert!(UixInputSequenceInput::parse(&press_with_point).is_err());

        let mut pointer_with_key = valid_input();
        pointer_with_key["steps"][1]["key"] = json!("a");
        assert!(UixInputSequenceInput::parse(&pointer_with_key).is_err());

        let mut invalid_key = valid_input();
        invalid_key["steps"][0]["key"] = json!("f13");
        assert!(UixInputSequenceInput::parse(&invalid_key).is_err());

        let mut invalid_point = valid_input();
        invalid_point["steps"][1]["x"] = json!(65_535.1);
        assert!(UixInputSequenceInput::parse(&invalid_point).is_err());
    }
}
