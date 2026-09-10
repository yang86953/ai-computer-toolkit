//! 面向桌面交互的紧凑输入计划；先完整验证，再交给同一会话执行。

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    components::{desktop_session_keyboard_input, keyboard_input_contract::KeyboardInput},
    domain::{AppControlError, AppResult},
};

/// 固定一次交互请求允许的最大公开步骤数。
pub(crate) const MAXIMUM_INTERACTION_STEPS: usize = 648;
/// 固定展开后的最大平台工作单元数。
pub(crate) const MAXIMUM_INTERACTION_UNITS: usize = 16_384;
/// 固定交互请求最长 deadline；同时是 wait 合计的唯一上界。
pub(crate) const MAXIMUM_INTERACTION_TIMEOUT_MS: u32 = 30_000;

/// 截图对应的输入区域代际；不携带平台设备或映射身份。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FrameMapping {
    pub generation: u64,
    pub width: u32,
    pub height: u32,
}

/// 坐标只在某次已返回截图的像素范围内解释。
#[derive(Clone, Copy, Debug)]
pub(crate) struct FramePoint {
    pub x: u32,
    pub y: u32,
    pub image_width: u32,
    pub image_height: u32,
    pub mapping: FrameMapping,
    pub button: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WirePlan {
    #[serde(default)]
    frame_id: Option<String>,
    steps: Vec<WireStep>,
    #[serde(default = "default_timeout")]
    timeout_ms: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum WireStep {
    Key {
        keys: Vec<String>,
    },
    Text {
        text: String,
    },
    Click {
        x: u32,
        y: u32,
        #[serde(default)]
        button: Button,
    },
    Move {
        x: u32,
        y: u32,
    },
    Wait {
        ms: u32,
    },
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Button {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug)]
pub(crate) enum InteractionStep {
    Keyboard(Vec<KeyboardInput>),
    Point { x: u32, y: u32, button: Option<u32> },
    Wait(u32),
}

pub(crate) struct InteractionPlan {
    pub frame_id: Option<String>,
    pub steps: Vec<InteractionStep>,
    pub timeout_ms: u32,
}

const fn default_timeout() -> u32 {
    30_000
}

fn invalid(message: &str) -> AppControlError {
    AppControlError::new("INVALID_ARGUMENT", message)
}

/// 文本明确采用美式 ASCII 按键位置；不冒充任意输入法下的 Unicode 文本输入。
pub(crate) fn parse(value: &Value) -> AppResult<InteractionPlan> {
    let wire: WirePlan = serde_json::from_value(value.clone())
        .map_err(|_| invalid("Interaction input has unknown or invalid fields."))?;
    if wire.steps.is_empty()
        || wire.steps.len() > MAXIMUM_INTERACTION_STEPS
        || !(1..=MAXIMUM_INTERACTION_TIMEOUT_MS).contains(&wire.timeout_ms)
    {
        return Err(invalid(&format!(
            "Interaction requires 1..{MAXIMUM_INTERACTION_STEPS} steps and a 1..{MAXIMUM_INTERACTION_TIMEOUT_MS}ms timeout.",
        )));
    }
    if wire.frame_id.as_ref().is_some_and(|id| {
        id.len() != 32
            || !id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    }) {
        return Err(invalid("frameId must be a returned observation identity."));
    }
    let mut steps = Vec::with_capacity(wire.steps.len());
    let mut units = 0usize;
    let mut waits = 0u32;
    for step in wire.steps {
        steps.push(match step {
            WireStep::Text { text } => {
                if text.is_empty() || text.len() > 4096 || !text.bytes().all(|b| (32..=126).contains(&b)) {
                    return Err(invalid("Text must contain 1..4096 printable ASCII characters; use key steps for Enter or Tab."));
                }
                units = units.saturating_add(text.len() * 4);
                let keys = text.bytes().map(ascii_key).collect::<Vec<_>>();
                let mut chunks = Vec::new();
                for chunk in keys.chunks(128) {
                    chunks.push(desktop_session_keyboard_input::parse(&json!({"steps":chunk,"timeoutMs":wire.timeout_ms}))?);
                }
                InteractionStep::Keyboard(chunks)
            }
            WireStep::Key { keys } => {
                units = units.saturating_add(keys.len() * 2);
                let step = if keys.len() == 1 { json!({"type":"key","key":keys[0]}) }
                    else { json!({"type":"chord","keys":keys}) };
                InteractionStep::Keyboard(vec![desktop_session_keyboard_input::parse(&json!({"steps":[step],"timeoutMs":wire.timeout_ms}))?])
            }
            WireStep::Click { x, y, button } => {
                if wire.frame_id.is_none() { return Err(invalid("Screenshot clicks require frameId.")); }
                units += 3;
                InteractionStep::Point { x, y, button: Some(match button { Button::Left=>1, Button::Right=>2, Button::Middle=>3 }) }
            }
            WireStep::Move { x, y } => {
                if wire.frame_id.is_none() { return Err(invalid("Screenshot movement requires frameId.")); }
                units += 1;
                InteractionStep::Point { x, y, button: None }
            }
            WireStep::Wait { ms } => {
                if ms == 0 || ms > 1000 { return Err(invalid("Each wait must be 1..1000ms.")); }
                waits = waits.saturating_add(ms);
                InteractionStep::Wait(ms)
            }
        });
        if units > MAXIMUM_INTERACTION_UNITS {
            return Err(invalid(&format!(
                "Interaction spends {units} input units, above the {MAXIMUM_INTERACTION_UNITS} limit; split it into smaller batches."
            )));
        }
        // wait 合计只受本批 timeoutMs 约束：timeoutMs 已封顶，再叠一层独立上限会让
        // 大批次无法附带等待，等于把步数上限变成空头承诺。
        if waits >= wire.timeout_ms {
            return Err(invalid(&format!(
                "Waits total {waits}ms and must stay below timeoutMs ({}ms): raise timeoutMs or shorten the waits.",
                wire.timeout_ms
            )));
        }
    }
    Ok(InteractionPlan {
        frame_id: wire.frame_id,
        steps,
        timeout_ms: wire.timeout_ms,
    })
}

fn ascii_key(byte: u8) -> Value {
    let (key, shift) = match byte {
        b'a'..=b'z' | b'0'..=b'9' => ((byte as char).to_string(), false),
        b'A'..=b'Z' => ((byte.to_ascii_lowercase() as char).to_string(), true),
        _ => {
            let plain = b" `-=[]\\;',./";
            let shifted = b" ~_+{}|:\"<>?";
            let names = [
                "space",
                "grave",
                "minus",
                "equals",
                "left-bracket",
                "right-bracket",
                "backslash",
                "semicolon",
                "apostrophe",
                "comma",
                "period",
                "slash",
            ];
            if let Some(i) = plain.iter().position(|b| *b == byte) {
                (names[i].to_owned(), false)
            } else if let Some(i) = shifted.iter().position(|b| *b == byte) {
                (names[i].to_owned(), true)
            } else {
                let digits = b")!@#$%^&*(";
                let i = digits.iter().position(|b| *b == byte).unwrap_or(0);
                (i.to_string(), true)
            }
        }
    };
    if shift {
        json!({"type":"chord","keys":["left-shift",key]})
    } else {
        json!({"type":"key","key":key})
    }
}

/// 使用像素中心映射，并拒绝越界、退化区域或截图与输入区域比例不符。
pub(crate) fn normalized_point(point: &FramePoint) -> AppResult<(f64, f64)> {
    let FramePoint {
        x,
        y,
        image_width: w,
        image_height: h,
        mapping,
        ..
    } = *point;
    if w == 0 || h == 0 || x >= w || y >= h || mapping.width == 0 || mapping.height == 0 {
        return Err(invalid("The point is outside the observed image."));
    }
    let error =
        (f64::from(w) * f64::from(mapping.height) - f64::from(h) * f64::from(mapping.width)).abs();
    if error > f64::from(mapping.width.max(mapping.height)) * 2.0 {
        return Err(AppControlError::new(
            "INPUT_MAPPING_UNAVAILABLE",
            "The observed image does not match the input region.",
        ));
    }
    Ok((
        (f64::from(x) + 0.5) / f64::from(w),
        (f64::from(y) + 0.5) / f64::from(h),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 步数上限、等待预算与工作单元边界都按公开契约取值，不接受各写一份。
    #[test]
    fn step_wait_and_unit_budgets_follow_the_published_contract() {
        let keys = |count: usize| {
            let steps = (0..count)
                .map(|_| json!({"type": "key", "keys": ["a"]}))
                .collect::<Vec<_>>();
            json!({"steps": steps, "timeoutMs": MAXIMUM_INTERACTION_TIMEOUT_MS})
        };
        // 上限内接受，越界拒绝：边界本身是契约的一部分。
        assert!(parse(&keys(MAXIMUM_INTERACTION_STEPS)).is_ok());
        let Err(over) = parse(&keys(MAXIMUM_INTERACTION_STEPS + 1)) else {
            panic!("over-limit batch must be rejected before dispatch")
        };
        assert!(over.message.contains(&MAXIMUM_INTERACTION_STEPS.to_string()));

        // wait 合计只需低于本批 timeoutMs：不再叠一层独立的等待总额上限。
        // 单步仍封顶 1000ms，所以长停顿由多个 wait 步累加。
        let waits = |count: u32, timeout: u32| {
            let steps = (0..count)
                .map(|_| json!({"type": "wait", "ms": 1_000}))
                .collect::<Vec<_>>();
            json!({"steps": steps, "timeoutMs": timeout})
        };
        assert!(parse(&waits(1, MAXIMUM_INTERACTION_TIMEOUT_MS)).is_ok());
        assert!(parse(&waits(25, MAXIMUM_INTERACTION_TIMEOUT_MS)).is_ok());
        assert!(parse(&waits(30, 30_000)).is_err());

        // 文本按每字符 4 单元计费：648 步对文本密集批次不成立。
        let text_steps = (0..MAXIMUM_INTERACTION_STEPS)
            .map(|_| json!({"type": "text", "text": "abcdefgh"}))
            .collect::<Vec<_>>();
        let Err(dense) = parse(&json!({"steps": text_steps, "timeoutMs": 30_000})) else {
            panic!("text-heavy batch must bind on the work-unit budget")
        };
        assert!(dense.message.contains("input units"));
    }

    #[test]
    fn printable_ascii_is_bounded_and_balanced_including_every_symbol() {
        let text = (32u8..=126).map(char::from).collect::<String>();
        let p = parse(&json!({"steps":[{"type":"text","text":text}]})).unwrap();
        let InteractionStep::Keyboard(chunks) = &p.steps[0] else {
            panic!()
        };
        assert_eq!(chunks[0].steps.len(), 95);
        assert_eq!(
            ascii_key(b'('),
            json!({"type":"chord","keys":["left-shift","9"]})
        );
        assert_eq!(
            ascii_key(b'"'),
            json!({"type":"chord","keys":["left-shift","apostrophe"]})
        );
        assert!(parse(&json!({"steps":[{"type":"text","text":"中文"}]})).is_err());
        assert!(parse(&json!({"steps":[{"type":"text","text":"abc\n"}]})).is_err());
    }

    #[test]
    fn whole_plan_rejects_invalid_tail_and_excessive_work_before_dispatch() {
        assert!(
            parse(
                &json!({"steps":[{"type":"text","text":"ok"},{"type":"key","keys":["bad-key"]}]})
            )
            .is_err()
        );
        assert!(parse(&json!({"steps":[{"type":"click","x":1,"y":2}]})).is_err());
        assert!(parse(&json!({"steps":[{"type":"text","text":"x".repeat(4096)},{"type":"key","keys":["enter"]}]})).is_err());
        assert!(parse(&json!({"steps":[{"type":"wait","ms":1000}],"timeoutMs":1000})).is_err());
    }

    #[test]
    fn waiting_budget_error_reports_the_actual_limits() {
        // 等待合计必须严格小于 timeoutMs；超限要报出实际数值、上限和修法，便于一次改对。
        let error = match parse(&json!({
            "steps":[{"type":"wait","ms":900},{"type":"wait","ms":900}],
            "timeoutMs":1500
        })) {
            Ok(_) => panic!("expected the waiting budget to be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code, "INVALID_ARGUMENT");
        assert!(error.message.contains("1800ms"), "{}", error.message);
        assert!(error.message.contains("1500ms"), "{}", error.message);
        assert!(error.message.contains("raise timeoutMs"), "{}", error.message);
        // 预算之内的同一批步骤照常通过，错误信息改动不影响可用性。
        assert!(
            parse(&json!({"steps":[{"type":"wait","ms":900}],"timeoutMs":1500})).is_ok()
        );
    }

    #[test]
    fn preview_points_map_to_pixel_centers_and_reject_invalid_geometry() {
        let mut p = FramePoint {
            x: 640,
            y: 360,
            image_width: 1280,
            image_height: 720,
            mapping: FrameMapping {
                generation: 1,
                width: 1920,
                height: 1080,
            },
            button: Some(1),
        };
        let (x, y) = normalized_point(&p).unwrap();
        assert!((x - 640.5 / 1280.0).abs() < 1e-12 && (y - 360.5 / 720.0).abs() < 1e-12);
        p.x = 1280;
        assert!(normalized_point(&p).is_err());
        p.x = 1;
        p.mapping.height = 1920;
        assert!(normalized_point(&p).is_err());
    }
}
