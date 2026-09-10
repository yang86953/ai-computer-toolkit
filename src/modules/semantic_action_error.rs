//! 统一 Semantic Action Module 私有错误码。

// 导入统一错误 envelope 与结构化证据。
use crate::domain::AppControlError;
// 导入 JSON 值。
use serde_json::Value;

// 表示 Semantic Action Module 允许产生的封闭错误集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SemanticActionErrorCode {
    // 表示元素不支持请求动作。
    ActionUnsupported,
    // 表示 UIA provider 当前不可用。
    AccessibilityUnavailable,
    // 表示目标解析歧义。
    AmbiguousTarget,
    // 表示 capability 未发布或不适用。
    CapabilityUnsupported,
    // 表示缺少逐操作确认。
    ConfirmationRequired,
    // 表示唯一元素未启用。
    ElementNotEnabled,
    // 表示完整搜索没有元素命中。
    ElementNotFound,
    // 表示请求输入不合法。
    InvalidArgument,
    // 表示执行期间发生前景干扰。
    HostInterferenceDetected,
    // 表示 dispatch 后结果无法确定。
    OutcomeUnknown,
    // 表示静态或 provider 权限拒绝。
    PermissionDenied,
    // 表示有界搜索无法证明唯一。
    SearchIncomplete,
    // 表示 opaque 目标已过期。
    StaleSession,
    // 表示 worker 或 assessment 违反内部协议。
    WorkerProtocolFailed,
}

// 提供私有错误类别到公开文本的唯一映射。
impl SemanticActionErrorCode {
    // 返回稳定错误码。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合。
        match self {
            // 映射动作缺口。
            Self::ActionUnsupported => "ACTION_UNSUPPORTED",
            // 映射可访问性不可用。
            Self::AccessibilityUnavailable => "ACCESSIBILITY_UNAVAILABLE",
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射 capability 缺口。
            Self::CapabilityUnsupported => "CAPABILITY_UNSUPPORTED",
            // 映射确认门禁。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射元素未启用。
            Self::ElementNotEnabled => "ELEMENT_NOT_ENABLED",
            // 映射元素缺失。
            Self::ElementNotFound => "ELEMENT_NOT_FOUND",
            // 映射参数错误。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射前景干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射不确定结果。
            Self::OutcomeUnknown => "OUTCOME_UNKNOWN",
            // 映射权限拒绝。
            Self::PermissionDenied => "PERMISSION_DENIED",
            // 映射搜索不完整。
            Self::SearchIncomplete => "SEARCH_INCOMPLETE",
            // 映射 stale 目标。
            Self::StaleSession => "STALE_SESSION",
            // 映射内部协议失败。
            Self::WorkerProtocolFailed => "WORKER_PROTOCOL_FAILED",
        }
    }

    // 从 worker 白名单文本恢复 Module 私有类别。
    pub(super) fn from_worker_code(code: &str) -> Option<Self> {
        // 只接受 worker 契约冻结的十项错误。
        match code {
            // 接受动作缺口。
            "ACTION_UNSUPPORTED" => Some(Self::ActionUnsupported),
            // 接受 provider 不可用。
            "ACCESSIBILITY_UNAVAILABLE" => Some(Self::AccessibilityUnavailable),
            // 接受目标歧义。
            "AMBIGUOUS_TARGET" => Some(Self::AmbiguousTarget),
            // 接受元素未启用。
            "ELEMENT_NOT_ENABLED" => Some(Self::ElementNotEnabled),
            // 接受元素缺失。
            "ELEMENT_NOT_FOUND" => Some(Self::ElementNotFound),
            // 接受协议输入拒绝。
            "INVALID_ARGUMENT" => Some(Self::InvalidArgument),
            // 接受 worker 内确认的结果未知。
            "OUTCOME_UNKNOWN" => Some(Self::OutcomeUnknown),
            // 接受权限拒绝。
            "PERMISSION_DENIED" => Some(Self::PermissionDenied),
            // 接受搜索不完整。
            "SEARCH_INCOMPLETE" => Some(Self::SearchIncomplete),
            // 接受目标过期。
            "STALE_SESSION" => Some(Self::StaleSession),
            // 拒绝开放错误空间。
            _ => None,
        }
    }

    // 构造普通结构化错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 复用产品级 envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 构造带 provider-neutral 证据的错误。
    pub(super) fn with_details(
        // 接收安全消息。
        self,
        // 接收可转换消息。
        message: impl Into<String>,
        // 接收结构化证据。
        details: Value,
    ) -> AppControlError {
        // 复用产品级 envelope。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 验证错误集合与 worker 白名单。
#[cfg(test)]
mod tests {
    // 导入被测错误类别。
    use super::SemanticActionErrorCode;

    // 验证 worker 只能穿过冻结白名单。
    #[test]
    fn worker_error_whitelist_is_closed() {
        // OutcomeUnknown 必须保持唯一映射。
        assert_eq!(
            SemanticActionErrorCode::from_worker_code("OUTCOME_UNKNOWN"),
            Some(SemanticActionErrorCode::OutcomeUnknown)
        );
        // 未知 provider 文本必须拒绝。
        assert_eq!(
            SemanticActionErrorCode::from_worker_code("PROVIDER_PANIC"),
            None
        );
    }
}
