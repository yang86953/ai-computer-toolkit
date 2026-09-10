//! UIX 协作式应用内按键序列版本一输入契约 Component。

use serde::Deserialize;
use serde_json::Value;

use super::uix_key_input_contract::UixKeyPress;

const DEFAULT_INTERVAL_MS: u32 = 0;
const MAXIMUM_INTERVAL_MS: u32 = 500;
const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
const MAXIMUM_PRESSES: usize = 64;
const MAXIMUM_PLANNED_DURATION_MS: u32 = 5_000;

const fn default_interval_ms() -> u32 {
    DEFAULT_INTERVAL_MS
}

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存严格验证后的应用内按键序列请求。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixKeySequenceInput {
    presses: Vec<UixKeyPress>,
    #[serde(default = "default_interval_ms")]
    interval_ms: u32,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixKeySequenceInput {
    /// 严格解析公开输入且不回显非法原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        if value
            .get("presses")
            .and_then(Value::as_array)
            .is_none_or(|presses| {
                presses
                    .iter()
                    .any(|press| UixKeyPress::parse(press).is_err())
            })
        {
            return Err("UIX key sequence input violates schema://ui/key-sequence/v1.");
        }
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX key sequence input violates schema://ui/key-sequence/v1.")?;
        let planned_duration_ms = input.planned_duration_ms();
        if input.presses.is_empty()
            || input.presses.len() > MAXIMUM_PRESSES
            || input.interval_ms > MAXIMUM_INTERVAL_MS
            || planned_duration_ms > MAXIMUM_PLANNED_DURATION_MS
            || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
            || input.timeout_ms < planned_duration_ms.saturating_add(MINIMUM_TIMEOUT_MS)
        {
            return Err("UIX key sequence input is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回 provider-neutral 的完整按键顺序。
    pub(crate) fn presses(&self) -> &[UixKeyPress] {
        &self.presses
    }

    /// 返回请求中的按键数量。
    pub(crate) fn presses_requested(&self) -> usize {
        self.presses.len()
    }

    /// 返回固定的 provider-neutral 动作名。
    pub(crate) const fn action(&self) -> &'static str {
        "key-sequence"
    }

    /// 返回相邻按键之间的有界间隔。
    pub(crate) const fn interval_ms(&self) -> u32 {
        self.interval_ms
    }

    /// 返回 `(presses.len() - 1) * intervalMs` 的计划时长。
    pub(crate) fn planned_duration_ms(&self) -> u32 {
        (self.presses.len().saturating_sub(1) as u32).saturating_mul(self.interval_ms)
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
    fn sequence_input_reuses_key_press_and_applies_defaults() {
        let Ok(input) = UixKeySequenceInput::parse(&json!({
            "presses": [{ "key": "control" }, { "key": "s", "modifiers": ["control"] }]
        })) else {
            panic!("valid key sequence must parse");
        };
        assert_eq!(input.action(), "key-sequence");
        assert_eq!(input.presses_requested(), 2);
        assert_eq!(input.interval_ms(), DEFAULT_INTERVAL_MS);
        assert_eq!(input.planned_duration_ms(), 0);
        assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
        assert_eq!(input.presses()[1].key(), "s");
        assert_eq!(input.presses()[1].modifiers(), ["control"]);
    }

    #[test]
    fn sequence_input_enforces_planned_duration_and_timeout_slack() {
        let Ok(input) = UixKeySequenceInput::parse(&json!({
            "presses": [
                { "key": "a" },
                { "key": "b" },
                { "key": "c" }
            ],
            "intervalMs": 100,
            "timeoutMs": 300
        })) else {
            panic!("bounded key sequence must parse");
        };
        assert_eq!(input.planned_duration_ms(), 200);

        let too_short = json!({
            "presses": [{ "key": "a" }, { "key": "b" }],
            "intervalMs": 100,
            "timeoutMs": 199
        });
        assert!(UixKeySequenceInput::parse(&too_short).is_err());

        let too_long_presses = (0..12).map(|_| json!({ "key": "a" })).collect::<Vec<_>>();
        let too_long = json!({
            "presses": too_long_presses,
            "intervalMs": 500,
            "timeoutMs": 30_000
        });
        assert!(UixKeySequenceInput::parse(&too_long).is_err());
    }

    #[test]
    fn sequence_input_is_closed_and_rejects_invalid_press_values() {
        assert!(UixKeySequenceInput::parse(&json!({ "presses": [] })).is_err());
        assert!(
            UixKeySequenceInput::parse(&json!({
                "presses": [{ "key": "f13" }]
            }))
            .is_err()
        );
        assert!(
            UixKeySequenceInput::parse(&json!({
                "presses": [{ "key": "enter", "modifiers": ["alt", "alt"] }]
            }))
            .is_err()
        );
        assert!(
            UixKeySequenceInput::parse(&json!({
                "presses": [{ "key": "enter" }],
                "unknown": true
            }))
            .is_err()
        );
        assert!(
            UixKeySequenceInput::parse(&json!({
                "presses": [{ "key": "enter" }],
                "intervalMs": 501
            }))
            .is_err()
        );
    }
}
