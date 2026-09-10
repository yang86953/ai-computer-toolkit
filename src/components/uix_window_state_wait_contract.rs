//! 冻结 UIX 协作式窗口框架状态等待的 provider-neutral 输入契约。

use serde::Deserialize;
use serde_json::{Value, json};

const DEFAULT_POLL_INTERVAL_MS: u32 = 50;
const MINIMUM_POLL_INTERVAL_MS: u32 = 20;
const MAXIMUM_POLL_INTERVAL_MS: u32 = 500;
const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MINIMUM_TIMEOUT_MS: u32 = 100;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
const MAXIMUM_CLIENT_DIMENSION: u32 = 65_535;

const fn default_poll_interval_ms() -> u32 {
    DEFAULT_POLL_INTERVAL_MS
}

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 保存一次封闭的框架当前状态等待条件。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum UixWindowStateWaitCondition {
    /// 等待可见性或可呈现性中的至少一个事实满足。
    Visibility {
        visible: Option<bool>,
        presentable: Option<bool>,
    },
    /// 等待当前框架焦点事实满足。
    Focus { focused: bool },
    /// 等待框架报告指定的客户区 logical 尺寸。
    ClientSize { width: u32, height: u32 },
    /// 等待窗口 flags 中至少一个事实满足。
    WindowFlags {
        maximized: Option<bool>,
        minimized: Option<bool>,
        fullscreen: Option<bool>,
    },
}

/// 条件判断所需的完整 provider-neutral 框架当前状态事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UixWindowStateFacts {
    pub(crate) visible: bool,
    pub(crate) presentable: bool,
    pub(crate) focused: bool,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) maximized: bool,
    pub(crate) minimized: bool,
    pub(crate) fullscreen: bool,
}

impl UixWindowStateWaitCondition {
    /// 严格解析可复用的状态条件，并拒绝 null、未知字段和越界值。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        validate_optional_boolean_fields(
            value,
            &[
                "visible",
                "presentable",
                "maximized",
                "minimized",
                "fullscreen",
            ],
        )?;
        let condition = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX window state wait condition violates its closed contract.")?;
        condition.validate()?;
        Ok(condition)
    }

    /// 校验条件自身的封闭字段组合与客户区尺寸边界。
    pub(crate) fn validate(self) -> Result<(), &'static str> {
        if condition_is_well_formed(self) {
            Ok(())
        } else {
            Err("UIX window state wait condition is outside its bounded contract.")
        }
    }

    /// 返回不含 provider 身份的稳定条件值。
    pub(crate) fn public_value(self) -> Value {
        match self {
            Self::Visibility {
                visible,
                presentable,
            } => {
                let mut value = json!({ "type": "visibility" });
                if let Some(visible) = visible {
                    value["visible"] = Value::Bool(visible);
                }
                if let Some(presentable) = presentable {
                    value["presentable"] = Value::Bool(presentable);
                }
                value
            }
            Self::Focus { focused } => json!({
                "type": "focus",
                "focused": focused,
            }),
            Self::ClientSize { width, height } => json!({
                "type": "client-size",
                "width": width,
                "height": height,
            }),
            Self::WindowFlags {
                maximized,
                minimized,
                fullscreen,
            } => {
                let mut value = json!({ "type": "window-flags" });
                if let Some(maximized) = maximized {
                    value["maximized"] = Value::Bool(maximized);
                }
                if let Some(minimized) = minimized {
                    value["minimized"] = Value::Bool(minimized);
                }
                if let Some(fullscreen) = fullscreen {
                    value["fullscreen"] = Value::Bool(fullscreen);
                }
                value
            }
        }
    }

    /// 判断一次完整框架状态观察是否满足条件。
    pub(crate) fn matches(self, facts: UixWindowStateFacts) -> bool {
        match self {
            Self::Visibility {
                visible: expected_visible,
                presentable: expected_presentable,
            } => {
                expected_visible.is_none_or(|expected| expected == facts.visible)
                    && expected_presentable.is_none_or(|expected| expected == facts.presentable)
            }
            Self::Focus { focused: expected } => facts.focused == expected,
            Self::ClientSize {
                width: expected_width,
                height: expected_height,
            } => facts.width == expected_width && facts.height == expected_height,
            Self::WindowFlags {
                maximized: expected_maximized,
                minimized: expected_minimized,
                fullscreen: expected_fullscreen,
            } => {
                expected_maximized.is_none_or(|expected| expected == facts.maximized)
                    && expected_minimized.is_none_or(|expected| expected == facts.minimized)
                    && expected_fullscreen.is_none_or(|expected| expected == facts.fullscreen)
            }
        }
    }
}

/// 保存通过严格验证的窗口状态等待请求。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UixWindowStateWaitInput {
    condition: UixWindowStateWaitCondition,
    #[serde(default = "default_poll_interval_ms")]
    poll_interval_ms: u32,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl UixWindowStateWaitInput {
    /// 严格解析输入且不回显非法原始 JSON。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let Some(condition) = value.get("condition") else {
            return Err("UIX window state wait input violates its bounded contract.");
        };
        validate_optional_boolean_fields(
            condition,
            &[
                "visible",
                "presentable",
                "maximized",
                "minimized",
                "fullscreen",
            ],
        )?;
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "UIX window state wait input violates schema://window/state-wait/v1.")?;
        if !condition_is_well_formed(input.condition)
            || !(MINIMUM_POLL_INTERVAL_MS..=MAXIMUM_POLL_INTERVAL_MS)
                .contains(&input.poll_interval_ms)
            || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
        {
            return Err("UIX window state wait input is outside its bounded contract.");
        }
        Ok(input)
    }

    /// 返回严格解析后的条件。
    pub(crate) const fn condition(&self) -> UixWindowStateWaitCondition {
        self.condition
    }

    /// 返回 provider-neutral 公开条件值。
    pub(crate) fn public_condition(&self) -> Value {
        self.condition.public_value()
    }

    /// 返回有界轮询间隔。
    pub(crate) const fn poll_interval_ms(&self) -> u32 {
        self.poll_interval_ms
    }

    /// 返回覆盖整个等待的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }
}

fn validate_optional_boolean_fields(
    condition: &Value,
    fields: &[&str],
) -> Result<(), &'static str> {
    let Some(object) = condition.as_object() else {
        return Err("UIX window state wait condition must be an object.");
    };
    for field in fields {
        if let Some(value) = object.get(*field)
            && value.as_bool().is_none()
        {
            return Err("UIX window state wait condition flags must be booleans.");
        }
    }
    Ok(())
}

fn condition_is_well_formed(condition: UixWindowStateWaitCondition) -> bool {
    match condition {
        UixWindowStateWaitCondition::Visibility {
            visible,
            presentable,
        } => visible.is_some() || presentable.is_some(),
        UixWindowStateWaitCondition::Focus { .. } => true,
        UixWindowStateWaitCondition::ClientSize { width, height } => {
            (1..=MAXIMUM_CLIENT_DIMENSION).contains(&width)
                && (1..=MAXIMUM_CLIENT_DIMENSION).contains(&height)
        }
        UixWindowStateWaitCondition::WindowFlags {
            maximized,
            minimized,
            fullscreen,
        } => maximized.is_some() || minimized.is_some() || fullscreen.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn state_wait_parses_all_closed_conditions_and_defaults() {
        let values = [
            json!({ "type": "visibility", "visible": true }),
            json!({ "type": "focus", "focused": false }),
            json!({ "type": "client-size", "width": 800, "height": 600 }),
            json!({ "type": "window-flags", "maximized": true }),
        ];
        for value in values {
            let Ok(input) = UixWindowStateWaitInput::parse(&json!({
                "condition": value,
            })) else {
                panic!("有效窗口状态条件必须解析");
            };
            assert_eq!(input.poll_interval_ms(), DEFAULT_POLL_INTERVAL_MS);
            assert_eq!(input.timeout_ms(), DEFAULT_TIMEOUT_MS);
        }
    }

    #[test]
    fn state_wait_enforces_condition_specific_bounds() {
        assert!(
            UixWindowStateWaitInput::parse(&json!({
                "condition": { "type": "visibility" }
            }))
            .is_err()
        );
        assert!(
            UixWindowStateWaitInput::parse(&json!({
                "condition": { "type": "window-flags" }
            }))
            .is_err()
        );
        assert!(
            UixWindowStateWaitInput::parse(&json!({
                "condition": { "type": "client-size", "width": 0, "height": 600 }
            }))
            .is_err()
        );
        assert!(
            UixWindowStateWaitInput::parse(&json!({
                "condition": { "type": "client-size", "width": 65_536, "height": 600 }
            }))
            .is_err()
        );
        assert!(
            UixWindowStateWaitInput::parse(&json!({
                "condition": { "type": "focus", "focused": true },
                "pollIntervalMs": 19,
            }))
            .is_err()
        );
        assert!(
            UixWindowStateWaitInput::parse(&json!({
                "condition": { "type": "focus", "focused": true },
                "timeoutMs": 30_001,
            }))
            .is_err()
        );
    }

    #[test]
    fn state_wait_rejects_null_unknown_and_non_boolean_condition_fields() {
        assert!(
            UixWindowStateWaitInput::parse(&json!({
                "condition": { "type": "visibility", "visible": null }
            }))
            .is_err()
        );
        assert!(
            UixWindowStateWaitInput::parse(&json!({
                "condition": { "type": "visibility", "visible": true, "extra": false }
            }))
            .is_err()
        );
        assert!(
            UixWindowStateWaitInput::parse(&json!({
                "condition": { "type": "focus", "focused": null }
            }))
            .is_err()
        );
        assert!(
            UixWindowStateWaitInput::parse(&json!({
                "condition": { "type": "visibility", "visible": 1 }
            }))
            .is_err()
        );
        assert!(
            UixWindowStateWaitInput::parse(&json!({
                "condition": { "type": "unknown", "visible": true }
            }))
            .is_err()
        );
        assert!(
            UixWindowStateWaitInput::parse(&json!({
                "condition": null,
            }))
            .is_err()
        );
    }

    #[test]
    fn every_condition_kind_matches_only_complete_current_facts() {
        let facts = UixWindowStateFacts {
            visible: true,
            presentable: false,
            focused: true,
            width: 800,
            height: 600,
            maximized: false,
            minimized: false,
            fullscreen: true,
        };
        for condition in [
            UixWindowStateWaitCondition::Visibility {
                visible: Some(true),
                presentable: Some(false),
            },
            UixWindowStateWaitCondition::Focus { focused: true },
            UixWindowStateWaitCondition::ClientSize {
                width: 800,
                height: 600,
            },
            UixWindowStateWaitCondition::WindowFlags {
                maximized: Some(false),
                minimized: None,
                fullscreen: Some(true),
            },
        ] {
            assert!(condition.matches(facts));
        }
        assert!(
            !UixWindowStateWaitCondition::Focus { focused: false }.matches(facts),
            "不满足的条件不得误报成功"
        );
    }
}
