//! MCP 平台后端共用的错误与未知结果语义。

use serde_json::{Value, json};

/// 客户端侧失败；`code` 直接进入 MCP 工具错误负载。
#[derive(Debug)]
pub struct BrokerFailure {
    pub code: String,
    pub message: String,
    pub outcome_unknown: bool,
    pub accepted_may_have_occurred: bool,
}

impl BrokerFailure {
    pub(crate) fn failed(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            outcome_unknown: false,
            accepted_may_have_occurred: false,
        }
    }

    /// 传输层不确定结果：不得自动重放。
    pub(crate) fn unknown(message: impl Into<String>) -> Self {
        Self {
            code: "OUTCOME_UNKNOWN".to_owned(),
            message: message.into(),
            outcome_unknown: true,
            accepted_may_have_occurred: true,
        }
    }

    /// 转换为 MCP 工具错误的文本负载。
    pub fn payload(&self) -> Value {
        json!({
            "code": self.code,
            "message": self.message,
            "outcome": if self.code == "CANCELLED" { "cancelled" } else if self.outcome_unknown { "unknown" } else { "failed" },
            "acceptedMayHaveOccurred": self.accepted_may_have_occurred,
            "automaticRetryProhibited": true,
        })
    }
}
