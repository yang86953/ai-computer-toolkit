//! 冻结精确窗口修订等待的 provider-neutral 输入。

use serde::Deserialize;
use serde_json::Value;

/// 为端点解析和等待响应预留最小可用时间。
const MINIMUM_TIMEOUT_MS: u32 = 100;
/// 与 UIX Agent v1 的单次等待上限保持一致。
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
/// 缺省使用 Agent 允许的最长有界等待。
const DEFAULT_TIMEOUT_MS: u32 = MAXIMUM_TIMEOUT_MS;

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 表示一次窗口修订等待的唯一条件。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum WindowRevisionWaitCondition {
    /// 等待语义修订号严格越过阈值。
    RevisionAfter {
        /// 保存调用方已经观察到的修订号。
        revision: u64,
    },
    /// 等待至少呈现指定修订号。
    PresentedAtLeast {
        /// 保存调用方要求已经呈现的修订号。
        revision: u64,
    },
}

impl WindowRevisionWaitCondition {
    /// 返回稳定公开条件名。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RevisionAfter { .. } => "revision-after",
            Self::PresentedAtLeast { .. } => "presented-at-least",
        }
    }

    /// 返回条件携带的单调阈值。
    pub(crate) const fn revision(self) -> u64 {
        match self {
            Self::RevisionAfter { revision } | Self::PresentedAtLeast { revision } => revision,
        }
    }
}

/// 保存通过严格验证的窗口修订等待输入。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WindowRevisionWaitInput {
    /// 保存二选一等待条件。
    pub(crate) condition: WindowRevisionWaitCondition,
    /// 保存覆盖重新解析、认证和等待的总 deadline。
    #[serde(default = "default_timeout_ms")]
    pub(crate) timeout_ms: u32,
}

impl WindowRevisionWaitInput {
    /// 严格解析输入且不把原始内容写入错误消息。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        let input = serde_json::from_value::<Self>(value.clone())
            .map_err(|_| "Window revision wait input violates schema://window/revision-wait/v1.")?;
        if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms) {
            return Err("Window revision wait timeout is outside its bounded contract.");
        }
        Ok(input)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn revision_wait_input_is_closed_and_bounded() {
        let input = WindowRevisionWaitInput::parse(&json!({
            "condition": { "type": "revision-after", "revision": 7 }
        }))
        .expect("valid revision condition must parse");
        assert_eq!(input.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(input.condition.revision(), 7);
        assert!(
            WindowRevisionWaitInput::parse(&json!({
                "condition": { "type": "presented-at-least", "revision": 8 },
                "timeoutMs": 99
            }))
            .is_err()
        );
        assert!(
            WindowRevisionWaitInput::parse(&json!({
                "condition": { "type": "revision-after", "revision": 7, "extra": true }
            }))
            .is_err()
        );
    }
}
