//! 统一 Semantic Action Worker 协议边界私有错误码。

// 导入 JSON 证据与产品级错误 envelope。
use crate::domain::AppControlError;
// 导入结构化 JSON 值。
use serde_json::Value;

// 表示 Semantic Action Worker 允许直接产生的封闭错误集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SemanticActionWorkerErrorCode {
    // 表示请求或协议字段无效。
    InvalidArgument,
    // 表示窗口或元素目标已过期。
    StaleSession,
    // 表示窗口或 selector 解析为多个候选。
    AmbiguousTarget,
    // 表示 provider 明确拒绝权限。
    PermissionDenied,
    // 表示 UIA 或 provider 当前不可用。
    AccessibilityUnavailable,
    // 表示完整搜索证明没有匹配元素。
    ElementNotFound,
    // 表示唯一元素当前不可操作。
    ElementNotEnabled,
    // 表示有界搜索不能证明零或唯一。
    SearchIncomplete,
    // 表示唯一元素不支持请求动作模式。
    ActionUnsupported,
    // 表示动作调用后无法证明最终结果。
    OutcomeUnknown,
}

// 提供私有错误类别到稳定协议文本的唯一映射。
impl SemanticActionWorkerErrorCode {
    // 返回公开错误码。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合。
        match self {
            // 映射协议拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射目标过期。
            Self::StaleSession => "STALE_SESSION",
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射权限拒绝。
            Self::PermissionDenied => "PERMISSION_DENIED",
            // 映射 provider 不可用。
            Self::AccessibilityUnavailable => "ACCESSIBILITY_UNAVAILABLE",
            // 映射元素缺失。
            Self::ElementNotFound => "ELEMENT_NOT_FOUND",
            // 映射元素未启用。
            Self::ElementNotEnabled => "ELEMENT_NOT_ENABLED",
            // 映射搜索不完整。
            Self::SearchIncomplete => "SEARCH_INCOMPLETE",
            // 映射动作模式缺口。
            Self::ActionUnsupported => "ACTION_UNSUPPORTED",
            // 映射不确定结果。
            Self::OutcomeUnknown => "OUTCOME_UNKNOWN",
        }
    }

    // 构造不带额外证据的结构化错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 复用统一公开 envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 构造带 provider-neutral 证据的结构化错误。
    pub(super) fn with_details(
        // 接收稳定安全消息。
        self,
        // 接收可转换消息。
        message: impl Into<String>,
        // 接收不含原生身份的证据。
        details: Value,
    ) -> AppControlError {
        // 复用统一公开 envelope。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 验证封闭错误集合的稳定映射。
#[cfg(test)]
mod tests {
    // 导入被测错误集合。
    use super::SemanticActionWorkerErrorCode;

    // 固定全部公开错误文本。
    #[test]
    fn semantic_action_worker_errors_are_closed_and_stable() {
        // 枚举全部十种错误。
        let mappings = [
            // 协议参数错误。
            (
                SemanticActionWorkerErrorCode::InvalidArgument,
                "INVALID_ARGUMENT",
            ),
            // 目标过期。
            (SemanticActionWorkerErrorCode::StaleSession, "STALE_SESSION"),
            // 目标歧义。
            (
                SemanticActionWorkerErrorCode::AmbiguousTarget,
                "AMBIGUOUS_TARGET",
            ),
            // 权限拒绝。
            (
                SemanticActionWorkerErrorCode::PermissionDenied,
                "PERMISSION_DENIED",
            ),
            // provider 不可用。
            (
                SemanticActionWorkerErrorCode::AccessibilityUnavailable,
                "ACCESSIBILITY_UNAVAILABLE",
            ),
            // 元素缺失。
            (
                SemanticActionWorkerErrorCode::ElementNotFound,
                "ELEMENT_NOT_FOUND",
            ),
            // 元素未启用。
            (
                SemanticActionWorkerErrorCode::ElementNotEnabled,
                "ELEMENT_NOT_ENABLED",
            ),
            // 搜索不完整。
            (
                SemanticActionWorkerErrorCode::SearchIncomplete,
                "SEARCH_INCOMPLETE",
            ),
            // 动作不支持。
            (
                SemanticActionWorkerErrorCode::ActionUnsupported,
                "ACTION_UNSUPPORTED",
            ),
            // 结果不确定。
            (
                SemanticActionWorkerErrorCode::OutcomeUnknown,
                "OUTCOME_UNKNOWN",
            ),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 10);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }
}
