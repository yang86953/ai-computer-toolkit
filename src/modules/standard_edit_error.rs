//! 统一 Standard Edit Module 私有错误码。

// 导入 JSON 详情值。
use serde_json::Value;

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Standard Edit Module 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 该类型只在 Standard Edit Module 内部传播。
pub(super) enum StandardEditErrorCode {
    // 表示 opaque 目标同时命中多个控件。
    AmbiguousTarget,
    // 表示后台路径无法满足认证边界。
    BackgroundOperationUnavailable,
    // 表示调用方没有逐操作确认。
    ConfirmationRequired,
    // 表示操作期间宿主前景发生变化。
    HostInterferenceDetected,
    // 表示输入不满足封闭参数契约。
    InvalidArgument,
    // 表示固定写入或回读验证失败。
    OperationFailed,
    // 表示权限或完整性边界拒绝操作。
    PermissionDenied,
    // 表示精确控件目标已经过期。
    StaleSession,
    // 表示同步窗口消息超过调用方 deadline。
    Timeout,
}

// 提供 Module 私有错误类别与公开协议文本的唯一映射。
impl StandardEditErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射后台路径不可用。
            Self::BackgroundOperationUnavailable => "BACKGROUND_OPERATION_UNAVAILABLE",
            // 映射确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射宿主前景干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射写入或回读失败。
            Self::OperationFailed => "OPERATION_FAILED",
            // 映射权限拒绝。
            Self::PermissionDenied => "PERMISSION_DENIED",
            // 映射精确目标过期。
            Self::StaleSession => "STALE_SESSION",
            // 映射同步消息超时。
            Self::Timeout => "TIMEOUT",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Module 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 使用当前封闭错误码构造带安全详情的公开错误。
    pub(super) fn with_details(
        // 接收稳定公开消息。
        self,
        // 接收公开消息正文。
        message: impl Into<String>,
        // 接收已经过 Module 筛选的安全详情。
        details: Value,
    ) -> AppControlError {
        // 隐藏 Module 私有类型并复用统一 details envelope。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 声明封闭错误类型的纯映射与构造测试。
#[cfg(test)]
mod tests {
    // 导入 JSON 夹具宏。
    use serde_json::json;

    // 导入被测 Standard Edit 私有错误类型。
    use super::StandardEditErrorCode;

    // 验证九种 Module 错误码的完整稳定映射。
    #[test]
    fn all_standard_edit_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持目标歧义码。
            (StandardEditErrorCode::AmbiguousTarget, "AMBIGUOUS_TARGET"),
            // 保持后台路径不可用码。
            (
                StandardEditErrorCode::BackgroundOperationUnavailable,
                "BACKGROUND_OPERATION_UNAVAILABLE",
            ),
            // 保持确认缺失码。
            (
                StandardEditErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持宿主前景干扰码。
            (
                StandardEditErrorCode::HostInterferenceDetected,
                "HOST_INTERFERENCE_DETECTED",
            ),
            // 保持参数拒绝码。
            (StandardEditErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持操作失败码。
            (StandardEditErrorCode::OperationFailed, "OPERATION_FAILED"),
            // 保持权限拒绝码。
            (StandardEditErrorCode::PermissionDenied, "PERMISSION_DENIED"),
            // 保持目标过期码。
            (StandardEditErrorCode::StaleSession, "STALE_SESSION"),
            // 保持同步消息超时码。
            (StandardEditErrorCode::Timeout, "TIMEOUT"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 9);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通构造器保持公开消息与空详情。
    #[test]
    fn standard_edit_error_constructor_keeps_message_and_empty_details() {
        // 构造精确目标过期错误。
        let error = StandardEditErrorCode::StaleSession.error("stale fixture");
        // 普通构造器必须选择封闭映射文本。
        assert_eq!(error.code, "STALE_SESSION");
        // 普通构造器必须保持公开消息。
        assert_eq!(error.message, "stale fixture");
        // 普通构造器不得制造额外详情。
        assert!(error.details.is_null());
    }

    // 验证带详情构造器逐值保持 outcome-unknown 证据。
    #[test]
    fn standard_edit_details_constructor_keeps_safe_details() {
        // 构造不含 native 标识的固定超时详情。
        let details = json!({
            "outcome": "unknown",
            "retrySafe": false,
            "targetMayHaveMutated": true,
            "reason": "synchronous-window-message-timeout",
        });
        // 构造同步消息超时错误。
        let error = StandardEditErrorCode::Timeout.with_details(
            // 使用稳定夹具消息。
            "timeout fixture",
            // 复制详情以便随后逐值核对。
            details.clone(),
        );
        // 带详情构造器必须选择封闭映射文本。
        assert_eq!(error.code, "TIMEOUT");
        // 带详情构造器必须保持公开消息。
        assert_eq!(error.message, "timeout fixture");
        // 带详情构造器必须逐值保持安全详情。
        assert_eq!(error.details, details);
    }
}
