//! UIX 协作式应用内多点悬停序列版本一输入契约 Component。

use serde::Deserialize;
use serde_json::{Value, json};

use super::uix_pointer_input_contract::{UixPointerCoordinateSpace, UixPointerPoint};

const DEFAULT_INTERVAL_MS: u32 = 0;
const MAXIMUM_INTERVAL_MS: u32 = 500;
const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_MOVES: usize = 2;
const MAXIMUM_MOVES: usize = 64;
const MAXIMUM_PLANNED_DURATION_MS: u32 = 5_000;

const fn default_interval_ms() -> u32 {
    DEFAULT_INTERVAL_MS
}

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存严格验证后的应用内 pointer_move 序列请求。
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixPointerMoveSequenceInput {
    coordinate_space: UixPointerCoordinateSpace,
    moves: Vec<UixPointerPoint>,
    #[serde(default = "default_interval_ms")]
    interval_ms: u32,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixPointerMoveSequenceInput {
    /// 严格解析公开输入且不回显非法原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let input = serde_json::from_value::<Self>(value.clone()).map_err(
            |_| "UIX pointer move sequence input violates schema://ui/pointer-move-sequence/v1.",
        )?;
        let planned_duration_ms = input.planned_duration_ms();
        if !(MINIMUM_MOVES..=MAXIMUM_MOVES).contains(&input.moves.len())
            || input.moves.iter().any(|point| point.validate().is_err())
            || input.interval_ms > MAXIMUM_INTERVAL_MS
            || planned_duration_ms > MAXIMUM_PLANNED_DURATION_MS
            || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
            || input.timeout_ms < planned_duration_ms.saturating_add(MINIMUM_TIMEOUT_MS)
        {
            return Err("UIX pointer move sequence input is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回 provider-neutral 的完整移动点顺序。
    pub(crate) fn moves(&self) -> &[UixPointerPoint] {
        &self.moves
    }

    /// 返回请求中的移动点数量。
    pub(crate) fn moves_requested(&self) -> usize {
        self.moves.len()
    }

    /// 返回固定的 provider-neutral 动作名。
    pub(crate) const fn action(&self) -> &'static str {
        "pointer-move-sequence"
    }

    /// 返回固定的客户区 logical px 坐标空间。
    pub(crate) const fn coordinate_space(&self) -> UixPointerCoordinateSpace {
        self.coordinate_space
    }

    /// 返回相邻移动点之间的有界间隔。
    pub(crate) const fn interval_ms(&self) -> u32 {
        self.interval_ms
    }

    /// 返回 `(moves.len() - 1) * intervalMs` 的计划时长。
    pub(crate) fn planned_duration_ms(&self) -> u32 {
        (self.moves.len().saturating_sub(1) as u32).saturating_mul(self.interval_ms)
    }

    /// 返回覆盖发现、认证、调度与响应的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }

    /// 生成不含 provider 身份的公开点序列。
    pub(crate) fn public_moves(&self) -> Vec<Value> {
        self.moves
            .iter()
            .map(|point| json!({ "x": point.x(), "y": point.y() }))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn valid_input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "moves": [
                { "x": 10.5, "y": 20.25 },
                { "x": 100, "y": 200 },
                { "x": 300, "y": 400 }
            ]
        })
    }

    #[test]
    fn move_sequence_reuses_logical_point_validation_and_defaults() {
        let Ok(input) = UixPointerMoveSequenceInput::parse(&valid_input()) else {
            panic!("有效移动序列必须解析");
        };
        assert_eq!(input.action(), "pointer-move-sequence");
        assert_eq!(input.coordinate_space().as_str(), "client-logical-px");
        assert_eq!(input.moves_requested(), 3);
        assert_eq!(input.interval_ms(), DEFAULT_INTERVAL_MS);
        assert_eq!(input.planned_duration_ms(), 0);
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
        assert_eq!(input.moves()[0].x(), 10.5);
        assert_eq!(input.moves()[1].y(), 200.0);
        assert_eq!(input.public_moves()[2]["x"], 300.0);
    }

    #[test]
    fn move_sequence_enforces_duration_timeout_and_bounds() {
        let mut too_short = valid_input();
        too_short["intervalMs"] = json!(100);
        too_short["timeoutMs"] = json!(299);
        assert!(UixPointerMoveSequenceInput::parse(&too_short).is_err());

        let mut too_long = valid_input();
        too_long["intervalMs"] = json!(500);
        too_long["timeoutMs"] = json!(30_000);
        too_long["moves"] = json!(
            (0..12)
                .map(|index| json!({ "x": index, "y": index + 1 }))
                .collect::<Vec<_>>()
        );
        assert!(UixPointerMoveSequenceInput::parse(&too_long).is_err());

        let mut too_few = valid_input();
        too_few["moves"] = json!([{ "x": 1, "y": 2 }]);
        assert!(UixPointerMoveSequenceInput::parse(&too_few).is_err());
    }

    #[test]
    fn move_sequence_is_closed_and_rejects_invalid_points() {
        let mut unknown = valid_input();
        unknown["unexpected"] = json!(true);
        assert!(UixPointerMoveSequenceInput::parse(&unknown).is_err());

        let mut invalid_space = valid_input();
        invalid_space["coordinateSpace"] = json!("screen-physical-px");
        assert!(UixPointerMoveSequenceInput::parse(&invalid_space).is_err());

        let mut invalid_point = valid_input();
        invalid_point["moves"][0]["x"] = json!(65_535.1);
        assert!(UixPointerMoveSequenceInput::parse(&invalid_point).is_err());

        let mut invalid_field = valid_input();
        invalid_field["moves"][0]["kind"] = json!("pointer_move");
        assert!(UixPointerMoveSequenceInput::parse(&invalid_field).is_err());
    }
}
