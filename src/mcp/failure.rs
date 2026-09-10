//! MCP 平台后端共用的错误与未知结果语义。

use serde_json::{Value, json};

/// 客户端侧失败；`code` 直接进入 MCP 工具错误负载。
#[derive(Debug)]
pub struct BrokerFailure {
    pub code: String,
    pub message: String,
    pub outcome_unknown: bool,
    pub accepted_may_have_occurred: bool,
    /// broker 业务拒绝里的 `error.details`，原样保留。
    ///
    /// 输入失败最需要的诊断信息（`stage`、`completedSteps`、`inputEventsSent`、
    /// `releasesConfirmed`、`sessionCleanupConfirmed`）都在这里；丢掉它们就只剩
    /// 一句「provider did not complete the requested sequence」，无法判断失败发生在哪一步。
    pub details: Value,
}

impl BrokerFailure {
    pub(crate) fn failed(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            outcome_unknown: false,
            accepted_may_have_occurred: false,
            details: Value::Null,
        }
    }

    /// 传输层不确定结果：不得自动重放。
    pub(crate) fn unknown(message: impl Into<String>) -> Self {
        Self {
            code: "OUTCOME_UNKNOWN".to_owned(),
            message: message.into(),
            outcome_unknown: true,
            accepted_may_have_occurred: true,
            details: Value::Null,
        }
    }

    /// 转换为 MCP 工具错误的文本负载。
    pub fn payload(&self) -> Value {
        let mut payload = json!({
            "code": self.code,
            "message": self.message,
            "outcome": if self.code == "CANCELLED" { "cancelled" } else if self.outcome_unknown { "unknown" } else { "failed" },
            "acceptedMayHaveOccurred": self.accepted_may_have_occurred,
            "automaticRetryProhibited": true,
        });
        if !self.details.is_null() {
            if let Some(target) = payload.as_object_mut() {
                target.insert("details".to_owned(), self.details.clone());
            }
        }
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 业务细节必须能到达调用方：诊断输入失败靠 `stage` 与已完成步数，而不是一句通用文案。
    #[test]
    fn business_details_survive_into_the_tool_error() {
        let mut failure = BrokerFailure::failed("OUTCOME_UNKNOWN", "provider failed");
        failure.outcome_unknown = true;
        failure.accepted_may_have_occurred = true;
        // 没有细节时不凭空造字段，保持既有负载形状。
        assert!(failure.payload().get("details").is_none());

        failure.details = json!({"stage": "absolute-motion", "completedSteps": 12});
        let payload = failure.payload();
        assert_eq!(payload["details"]["stage"], "absolute-motion");
        assert_eq!(payload["details"]["completedSteps"], 12);
        assert_eq!(payload["acceptedMayHaveOccurred"], true);
    }
}
