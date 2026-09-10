//! 冻结 Linux Portal 桌面会话的相对指针输入契约。

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::Value;

use crate::{
    components::pointer_input_contract::{
        DEFAULT_POINTER_TIMEOUT_MS, MAXIMUM_CLICK_INTERVAL_MS, MAXIMUM_DRAG_DURATION_MS,
        MAXIMUM_DRAG_SAMPLES, MAXIMUM_POINTER_STEPS, MAXIMUM_POINTER_TIMEOUT_MS,
        MAXIMUM_POINTER_WORK_UNITS, MAXIMUM_SCROLL_TICKS, MINIMUM_POINTER_TIMEOUT_MS,
        PointerButton, PointerButtonPhase, PointerScrollAxis,
    },
    domain::{AppControlError, AppResult},
};

const MAXIMUM_RELATIVE_DELTA: i32 = 65_535;

const fn default_timeout_ms() -> u32 {
    DEFAULT_POINTER_TIMEOUT_MS
}

const fn default_click_count() -> u8 {
    1
}

const fn default_click_interval_ms() -> u32 {
    100
}

const fn default_drag_duration_ms() -> u32 {
    250
}

const fn default_drag_samples() -> u16 {
    12
}

/// 表示 EIS relative motion 使用的 logical px 增量空间。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub(crate) enum DesktopPointerCoordinateSpace {
    #[serde(rename = "relative-logical-px")]
    RelativeLogicalPx,
}

impl DesktopPointerCoordinateSpace {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RelativeLogicalPx => "relative-logical-px",
        }
    }
}

/// 保存一次带符号相对移动，不声称最终绝对坐标。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesktopPointerDelta {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

impl DesktopPointerDelta {
    fn valid(self) -> bool {
        (self.x != 0 || self.y != 0)
            && self.x.unsigned_abs() <= MAXIMUM_RELATIVE_DELTA as u32
            && self.y.unsigned_abs() <= MAXIMUM_RELATIVE_DELTA as u32
    }
}

/// 表示会话级相对指针的封闭动作集合。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum DesktopPointerStep {
    Move {
        delta: DesktopPointerDelta,
    },
    Button {
        button: PointerButton,
        phase: PointerButtonPhase,
    },
    Click {
        button: PointerButton,
        #[serde(default = "default_click_count")]
        count: u8,
        #[serde(default = "default_click_interval_ms", rename = "intervalMs")]
        interval_ms: u32,
    },
    Scroll {
        axis: PointerScrollAxis,
        ticks: i32,
    },
    Drag {
        button: PointerButton,
        delta: DesktopPointerDelta,
        #[serde(default = "default_drag_duration_ms", rename = "durationMs")]
        duration_ms: u32,
        #[serde(default = "default_drag_samples")]
        samples: u16,
    },
}

impl DesktopPointerStep {
    const fn work_units(&self) -> usize {
        match self {
            Self::Move { .. } => 1,
            Self::Button { .. } => 1,
            Self::Click { count, .. } => *count as usize * 2,
            Self::Scroll { .. } => 1,
            Self::Drag { samples, .. } => 2 + *samples as usize,
        }
    }
}

/// 保存严格验证后的会话级相对指针请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopPointerInput {
    pub(crate) coordinate_space: DesktopPointerCoordinateSpace,
    pub(crate) steps: Vec<DesktopPointerStep>,
    pub(crate) timeout_ms: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DesktopPointerWire {
    coordinate_space: DesktopPointerCoordinateSpace,
    steps: Vec<DesktopPointerStep>,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

fn invalid_argument(message: impl Into<String>) -> AppControlError {
    AppControlError::new("INVALID_ARGUMENT", message)
}

/// 在任何 session 查询或 EIS 访问前冻结全部相对指针语义。
pub(crate) fn parse(value: &Value) -> AppResult<DesktopPointerInput> {
    let wire = serde_json::from_value::<DesktopPointerWire>(value.clone()).map_err(|_| {
        invalid_argument("Pointer input does not match the pointer-input-v3 schema.")
    })?;
    let input = DesktopPointerInput {
        coordinate_space: wire.coordinate_space,
        steps: wire.steps,
        timeout_ms: wire.timeout_ms,
    };
    validate(&input)?;
    Ok(input)
}

fn validate(input: &DesktopPointerInput) -> AppResult<()> {
    if input.steps.is_empty() || input.steps.len() > MAXIMUM_POINTER_STEPS {
        return Err(invalid_argument(format!(
            "Pointer input requires 1..={MAXIMUM_POINTER_STEPS} steps."
        )));
    }
    if !(MINIMUM_POINTER_TIMEOUT_MS..=MAXIMUM_POINTER_TIMEOUT_MS).contains(&input.timeout_ms) {
        return Err(invalid_argument(format!(
            "Pointer input timeoutMs must be within {MINIMUM_POINTER_TIMEOUT_MS}..={MAXIMUM_POINTER_TIMEOUT_MS}."
        )));
    }
    let mut held = BTreeSet::new();
    let mut work_units = 0usize;
    for step in &input.steps {
        work_units = work_units.saturating_add(step.work_units());
        if work_units > MAXIMUM_POINTER_WORK_UNITS {
            return Err(invalid_argument(format!(
                "Pointer input expands beyond {MAXIMUM_POINTER_WORK_UNITS} work units."
            )));
        }
        match step {
            DesktopPointerStep::Move { delta } => validate_delta(*delta)?,
            DesktopPointerStep::Button { button, phase } => match phase {
                PointerButtonPhase::Down if !held.insert(*button) => {
                    return Err(invalid_argument(
                        "A pointer button cannot be pressed twice without an intervening release.",
                    ));
                }
                PointerButtonPhase::Up if !held.remove(button) => {
                    return Err(invalid_argument(
                        "A pointer button release must match an earlier press in the same request.",
                    ));
                }
                _ => {}
            },
            DesktopPointerStep::Click {
                count, interval_ms, ..
            } => {
                if !matches!(*count, 1 | 2) {
                    return Err(invalid_argument("Pointer click count must be 1 or 2."));
                }
                if *interval_ms > MAXIMUM_CLICK_INTERVAL_MS {
                    return Err(invalid_argument(format!(
                        "Pointer click intervalMs must be within 0..={MAXIMUM_CLICK_INTERVAL_MS}."
                    )));
                }
                if !held.is_empty() {
                    return Err(invalid_argument(
                        "Pointer click cannot execute while this request holds a button.",
                    ));
                }
            }
            DesktopPointerStep::Scroll { ticks, .. } => {
                if *ticks == 0 || ticks.unsigned_abs() > MAXIMUM_SCROLL_TICKS as u32 {
                    return Err(invalid_argument(format!(
                        "Pointer scroll ticks must be non-zero and within -{MAXIMUM_SCROLL_TICKS}..={MAXIMUM_SCROLL_TICKS}."
                    )));
                }
            }
            DesktopPointerStep::Drag {
                delta,
                duration_ms,
                samples,
                ..
            } => {
                validate_delta(*delta)?;
                if *duration_ms > MAXIMUM_DRAG_DURATION_MS {
                    return Err(invalid_argument(format!(
                        "Pointer drag durationMs must be within 0..={MAXIMUM_DRAG_DURATION_MS}."
                    )));
                }
                if *samples == 0 || *samples > MAXIMUM_DRAG_SAMPLES {
                    return Err(invalid_argument(format!(
                        "Pointer drag samples must be within 1..={MAXIMUM_DRAG_SAMPLES}."
                    )));
                }
                if !held.is_empty() {
                    return Err(invalid_argument(
                        "Pointer drag cannot execute while this request holds a button.",
                    ));
                }
            }
        }
    }
    if !held.is_empty() {
        return Err(invalid_argument(
            "Every pointer button press must be released in the same request.",
        ));
    }
    Ok(())
}

fn validate_delta(delta: DesktopPointerDelta) -> AppResult<()> {
    if !delta.valid() {
        return Err(invalid_argument(format!(
            "Pointer delta must be non-zero and each axis within -{MAXIMUM_RELATIVE_DELTA}..={MAXIMUM_RELATIVE_DELTA}."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn complete_relative_pointer_sequence_is_accepted() {
        let input = parse(&json!({
            "coordinateSpace": "relative-logical-px",
            "timeoutMs": 10_000,
            "steps": [
                {"type": "move", "delta": {"x": 10, "y": -5}},
                {"type": "button", "button": "left", "phase": "down"},
                {"type": "move", "delta": {"x": 3, "y": 4}},
                {"type": "scroll", "axis": "horizontal", "ticks": -2},
                {"type": "button", "button": "left", "phase": "up"},
                {"type": "click", "button": "right", "count": 2, "intervalMs": 25},
                {"type": "drag", "button": "middle", "delta": {"x": 40, "y": 20}, "samples": 8}
            ]
        }))
        .unwrap_or_else(|error| panic!("complete relative sequence must parse: {error}"));
        assert_eq!(input.coordinate_space.as_str(), "relative-logical-px");
        assert_eq!(input.steps.len(), 7);
    }

    #[test]
    fn zero_delta_native_fields_and_unbalanced_buttons_are_rejected() {
        for value in [
            json!({
                "coordinateSpace": "relative-logical-px",
                "steps": [{"type": "move", "delta": {"x": 0, "y": 0}}]
            }),
            json!({
                "coordinateSpace": "relative-logical-px",
                "steps": [{"type": "button", "button": "left", "phase": "down"}]
            }),
            json!({
                "coordinateSpace": "relative-logical-px",
                "steps": [{"type": "click", "button": "left", "linuxButtonCode": 272}]
            }),
        ] {
            let error = parse(&value).expect_err("unsafe relative pointer input must fail");
            assert_eq!(error.code, "INVALID_ARGUMENT");
        }
    }
}
