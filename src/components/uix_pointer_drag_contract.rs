//! UIX 协作式应用内指针拖拽版本一输入契约 Component。

use serde::Deserialize;
use serde_json::Value;

const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
const DEFAULT_TIMEOUT_MS: u32 = MAXIMUM_TIMEOUT_MS;
const MINIMUM_SAMPLES: u16 = 1;
const MAXIMUM_SAMPLES: u16 = 64;
const DEFAULT_SAMPLES: u16 = 12;
const MAXIMUM_DURATION_MS: u32 = 5_000;
const DEFAULT_DURATION_MS: u32 = 250;
const MAXIMUM_CLIENT_COORDINATE: f64 = 65_535.0;

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

const fn default_samples() -> u16 {
    DEFAULT_SAMPLES
}

const fn default_duration_ms() -> u32 {
    DEFAULT_DURATION_MS
}

/// 表示 UIX 拖拽唯一认证的客户区坐标空间。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub(crate) enum UixPointerDragCoordinateSpace {
    /// 使用 UIX 跨平台客户区 logical px。
    #[serde(rename = "client-logical-px")]
    ClientLogicalPx,
}

impl UixPointerDragCoordinateSpace {
    /// 返回稳定公开坐标空间名称。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ClientLogicalPx => "client-logical-px",
        }
    }
}

/// 保存一次拖拽端点，不携带原生窗口或桌面坐标身份。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct UixPointerDragPoint {
    x: f64,
    y: f64,
}

impl UixPointerDragPoint {
    fn valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && (0.0..=MAXIMUM_CLIENT_COORDINATE).contains(&self.x)
            && (0.0..=MAXIMUM_CLIENT_COORDINATE).contains(&self.y)
    }

    /// 返回端点的 logical 客户区横坐标。
    pub(crate) const fn x(self) -> f64 {
        self.x
    }

    /// 返回端点的 logical 客户区纵坐标。
    pub(crate) const fn y(self) -> f64 {
        self.y
    }
}

/// 保存严格验证后的固定左键原子拖拽请求。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixPointerDragInput {
    coordinate_space: UixPointerDragCoordinateSpace,
    start: UixPointerDragPoint,
    end: UixPointerDragPoint,
    #[serde(default = "default_samples")]
    samples: u16,
    #[serde(default = "default_duration_ms")]
    duration_ms: u32,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixPointerDragInput {
    /// 严格解析公开输入且不回显原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX pointer drag input violates schema://ui/pointer-drag/v1.")?;
        let minimum_timeout_for_duration = input.duration_ms.saturating_add(MINIMUM_TIMEOUT_MS);
        if !input.start.valid()
            || !input.end.valid()
            || !(MINIMUM_SAMPLES..=MAXIMUM_SAMPLES).contains(&input.samples)
            || input.duration_ms > MAXIMUM_DURATION_MS
            || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
            || input.timeout_ms < minimum_timeout_for_duration
        {
            return Err("UIX pointer drag input is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回固定的客户区 logical px 坐标空间。
    pub(crate) const fn coordinate_space(&self) -> UixPointerDragCoordinateSpace {
        self.coordinate_space
    }

    /// 返回拖拽起点。
    pub(crate) const fn start(&self) -> UixPointerDragPoint {
        self.start
    }

    /// 返回拖拽终点。
    pub(crate) const fn end(&self) -> UixPointerDragPoint {
        self.end
    }

    /// 返回请求的有界移动采样数。
    pub(crate) const fn samples(&self) -> u16 {
        self.samples
    }

    /// 返回请求的有界移动采样数，名称与结果字段保持一致。
    pub(crate) const fn samples_requested(&self) -> u16 {
        self.samples
    }

    /// 返回拖拽持续时间。
    pub(crate) const fn duration_ms(&self) -> u32 {
        self.duration_ms
    }

    /// 返回覆盖发现、认证、调度与响应的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }

    /// 返回固定左键语义，不开放独立 down/up 输入。
    pub(crate) const fn button(&self) -> &'static str {
        "left"
    }

    /// 返回稳定公开动作名。
    pub(crate) const fn action(&self) -> &'static str {
        "drag"
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn valid_input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 10.5, "y": 20.25 },
            "end": { "x": 100, "y": 200 }
        })
    }

    #[test]
    fn drag_input_is_bounded_and_uses_safe_defaults() {
        let Ok(input) = UixPointerDragInput::parse(&valid_input()) else {
            panic!("valid drag input must parse");
        };
        assert_eq!(input.coordinate_space().as_str(), "client-logical-px");
        assert_eq!(input.button(), "left");
        assert_eq!(input.action(), "drag");
        assert_eq!(input.start().x(), 10.5);
        assert_eq!(input.end().y(), 200.0);
        assert_eq!(input.samples(), DEFAULT_SAMPLES);
        assert_eq!(input.samples_requested(), DEFAULT_SAMPLES);
        assert_eq!(input.duration_ms(), DEFAULT_DURATION_MS);
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn drag_input_requires_finite_coordinates_and_timeout_slack() {
        let mut input = valid_input();
        input["start"]["x"] = json!(-0.1);
        assert!(UixPointerDragInput::parse(&input).is_err());

        let mut input = valid_input();
        input["end"]["y"] = json!(65_535.1);
        assert!(UixPointerDragInput::parse(&input).is_err());

        let mut input = valid_input();
        input["durationMs"] = json!(5_000);
        input["timeoutMs"] = json!(5_099);
        assert!(UixPointerDragInput::parse(&input).is_err());

        let mut input = valid_input();
        input["durationMs"] = json!(5_000);
        input["timeoutMs"] = json!(5_100);
        let Ok(parsed) = UixPointerDragInput::parse(&input) else {
            panic!("duration plus timeout slack must parse");
        };
        assert_eq!(parsed.duration_ms(), 5_000);
        assert_eq!(parsed.timeout_ms(), 5_100);
    }

    #[test]
    fn drag_input_is_closed_and_rejects_independent_button_actions() {
        let mut input = valid_input();
        input["samples"] = json!(0);
        assert!(UixPointerDragInput::parse(&input).is_err());

        let mut input = valid_input();
        input["samples"] = json!(65);
        assert!(UixPointerDragInput::parse(&input).is_err());

        let mut input = valid_input();
        input["coordinateSpace"] = json!("screen-physical-px");
        assert!(UixPointerDragInput::parse(&input).is_err());

        let mut input = valid_input();
        input["button"] = json!("right");
        assert!(UixPointerDragInput::parse(&input).is_err());

        let mut input = valid_input();
        input["start"]["pointerDown"] = json!(true);
        assert!(UixPointerDragInput::parse(&input).is_err());
    }
}
