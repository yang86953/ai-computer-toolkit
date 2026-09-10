//! UIX 协作式应用内左键点击序列版本一输入契约 Component。

use serde::Deserialize;
use serde_json::Value;

use super::uix_pointer_input_contract::{UixPointerCoordinateSpace, UixPointerPoint};

const DEFAULT_INTERVAL_MS: u32 = 0;
const MAXIMUM_INTERVAL_MS: u32 = 500;
const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
const MAXIMUM_CLICKS: usize = 64;
const MAXIMUM_PLANNED_DURATION_MS: u32 = 5_000;

const fn default_interval_ms() -> u32 {
    DEFAULT_INTERVAL_MS
}

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存严格验证后的应用内普通左键点击序列请求。
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixPointerClickSequenceInput {
    coordinate_space: UixPointerCoordinateSpace,
    clicks: Vec<UixPointerPoint>,
    #[serde(default = "default_interval_ms")]
    interval_ms: u32,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixPointerClickSequenceInput {
    /// 严格解析公开输入且不回显非法原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        if value
            .get("clicks")
            .and_then(Value::as_array)
            .is_none_or(|clicks| {
                clicks
                    .iter()
                    .any(|point| UixPointerPoint::parse(point).is_err())
            })
        {
            return Err(
                "UIX pointer click sequence input violates schema://ui/pointer-click-sequence/v1.",
            );
        }
        let input = serde_json::from_value::<Self>(value.clone()).map_err(
            |_| "UIX pointer click sequence input violates schema://ui/pointer-click-sequence/v1.",
        )?;
        let planned_duration_ms = input.planned_duration_ms();
        if input.clicks.is_empty()
            || input.clicks.len() > MAXIMUM_CLICKS
            || input.interval_ms > MAXIMUM_INTERVAL_MS
            || planned_duration_ms > MAXIMUM_PLANNED_DURATION_MS
            || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
            || input.timeout_ms < planned_duration_ms.saturating_add(MINIMUM_TIMEOUT_MS)
        {
            return Err("UIX pointer click sequence input is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回 provider-neutral 的完整点击点顺序。
    pub(crate) fn clicks(&self) -> &[UixPointerPoint] {
        &self.clicks
    }

    /// 返回请求中的点击数量。
    pub(crate) fn clicks_requested(&self) -> usize {
        self.clicks.len()
    }

    /// 返回固定的 provider-neutral 动作名。
    pub(crate) const fn action(&self) -> &'static str {
        "click-sequence"
    }

    /// 返回固定的客户区 logical px 坐标空间。
    pub(crate) const fn coordinate_space(&self) -> UixPointerCoordinateSpace {
        self.coordinate_space
    }

    /// 返回相邻点击之间的有界间隔。
    pub(crate) const fn interval_ms(&self) -> u32 {
        self.interval_ms
    }

    /// 返回 `(clicks.len() - 1) * intervalMs` 的计划时长。
    pub(crate) fn planned_duration_ms(&self) -> u32 {
        (self.clicks.len().saturating_sub(1) as u32).saturating_mul(self.interval_ms)
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

    #[test]
    fn click_sequence_reuses_bounded_points_and_defaults() {
        let Ok(input) = UixPointerClickSequenceInput::parse(&json!({
            "coordinateSpace": "client-logical-px",
            "clicks": [
                { "x": 10.5, "y": 20.25 },
                { "x": 100, "y": 200 }
            ]
        })) else {
            panic!("valid pointer click sequence must parse");
        };
        assert_eq!(input.action(), "click-sequence");
        assert_eq!(input.coordinate_space().as_str(), "client-logical-px");
        assert_eq!(input.clicks_requested(), 2);
        assert_eq!(input.clicks()[0].x(), 10.5);
        assert_eq!(input.clicks()[1].y(), 200.0);
        assert_eq!(input.interval_ms(), DEFAULT_INTERVAL_MS);
        assert_eq!(input.planned_duration_ms(), 0);
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn click_sequence_enforces_planned_duration_and_timeout_slack() {
        let Ok(input) = UixPointerClickSequenceInput::parse(&json!({
            "coordinateSpace": "client-logical-px",
            "clicks": [
                { "x": 1, "y": 2 },
                { "x": 3, "y": 4 },
                { "x": 5, "y": 6 }
            ],
            "intervalMs": 100,
            "timeoutMs": 300
        })) else {
            panic!("bounded pointer click sequence must parse");
        };
        assert_eq!(input.planned_duration_ms(), 200);

        let too_short = json!({
            "coordinateSpace": "client-logical-px",
            "clicks": [{ "x": 1, "y": 2 }, { "x": 3, "y": 4 }],
            "intervalMs": 100,
            "timeoutMs": 199
        });
        assert!(UixPointerClickSequenceInput::parse(&too_short).is_err());

        let too_long_clicks = (0..12)
            .map(|_| json!({ "x": 1, "y": 2 }))
            .collect::<Vec<_>>();
        let too_long = json!({
            "coordinateSpace": "client-logical-px",
            "clicks": too_long_clicks,
            "intervalMs": 500,
            "timeoutMs": 30_000
        });
        assert!(UixPointerClickSequenceInput::parse(&too_long).is_err());
    }

    #[test]
    fn click_sequence_is_closed_and_rejects_invalid_clicks() {
        assert!(
            UixPointerClickSequenceInput::parse(&json!({
                "coordinateSpace": "client-logical-px",
                "clicks": []
            }))
            .is_err()
        );
        assert!(
            UixPointerClickSequenceInput::parse(&json!({
                "coordinateSpace": "screen-physical-px",
                "clicks": [{ "x": 1, "y": 2 }]
            }))
            .is_err()
        );
        assert!(
            UixPointerClickSequenceInput::parse(&json!({
                "coordinateSpace": "client-logical-px",
                "clicks": [{ "x": 65_535.1, "y": 2 }]
            }))
            .is_err()
        );
        assert!(
            UixPointerClickSequenceInput::parse(&json!({
                "coordinateSpace": "client-logical-px",
                "clicks": [{ "x": 1, "y": 2, "button": "right" }]
            }))
            .is_err()
        );
        assert!(
            UixPointerClickSequenceInput::parse(&json!({
                "coordinateSpace": "client-logical-px",
                "clicks": [{ "x": 1, "y": 2 }],
                "unknown": true
            }))
            .is_err()
        );
    }
}
