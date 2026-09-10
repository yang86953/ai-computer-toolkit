//! 统一 Window Close Module 私有错误码。

// 导入 JSON 详情值。
use serde_json::Value;

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Window Close Module 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowCloseErrorCode {
    // 表示 opaque 窗口目标当前无法唯一解析。
    AmbiguousTarget,
    // 表示完整清单或前景门禁无法认证后台关闭。
    BackgroundOperationUnavailable,
    // 表示静态目标权限事实不足以认证关闭。
    CapabilityAssessmentUnavailable,
    // 表示 mutation 缺少逐操作确认。
    ConfirmationRequired,
    // 表示关闭请求后宿主前景身份发生变化。
    HostInterferenceDetected,
    // 表示目标、deadline 或 provider input 不合法。
    InvalidArgument,
    // 表示平台拒绝排队固定关闭请求。
    OperationFailed,
    // 表示系统权限边界拒绝固定关闭请求。
    PermissionDenied,
    // 表示精确窗口身份已经过期。
    StaleSession,
    // 表示消息已排队但目标未在 deadline 内失效。
    Timeout,
}

// 提供 Module 私有错误类别与公开协议文本的唯一映射。
impl WindowCloseErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射后台执行不可用。
            Self::BackgroundOperationUnavailable => "BACKGROUND_OPERATION_UNAVAILABLE",
            // 映射权限评估不可用。
            Self::CapabilityAssessmentUnavailable => "CAPABILITY_ASSESSMENT_UNAVAILABLE",
            // 映射逐操作确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射消息后宿主干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射公共参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射平台操作失败。
            Self::OperationFailed => "OPERATION_FAILED",
            // 映射系统权限拒绝。
            Self::PermissionDenied => "PERMISSION_DENIED",
            // 映射目标过期。
            Self::StaleSession => "STALE_SESSION",
            // 映射结果未知的关闭超时。
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
        // 接收已经由 Module 筛选的安全详情。
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

    // 导入被测 Window Close 私有错误类型。
    use super::WindowCloseErrorCode;

    // 验证十种 Module 错误码的完整稳定映射。
    #[test]
    fn all_window_close_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持目标歧义码。
            (WindowCloseErrorCode::AmbiguousTarget, "AMBIGUOUS_TARGET"),
            // 保持后台不可用码。
            (
                WindowCloseErrorCode::BackgroundOperationUnavailable,
                "BACKGROUND_OPERATION_UNAVAILABLE",
            ),
            // 保持权限评估不可用码。
            (
                WindowCloseErrorCode::CapabilityAssessmentUnavailable,
                "CAPABILITY_ASSESSMENT_UNAVAILABLE",
            ),
            // 保持确认缺失码。
            (
                WindowCloseErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持宿主干扰码。
            (
                WindowCloseErrorCode::HostInterferenceDetected,
                "HOST_INTERFERENCE_DETECTED",
            ),
            // 保持参数拒绝码。
            (WindowCloseErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持操作失败码。
            (WindowCloseErrorCode::OperationFailed, "OPERATION_FAILED"),
            // 保持权限拒绝码。
            (WindowCloseErrorCode::PermissionDenied, "PERMISSION_DENIED"),
            // 保持目标过期码。
            (WindowCloseErrorCode::StaleSession, "STALE_SESSION"),
            // 保持超时码。
            (WindowCloseErrorCode::Timeout, "TIMEOUT"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 10);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通与带详情构造器保持统一公开 envelope。
    #[test]
    fn window_close_error_constructors_keep_message_and_details() {
        // 构造权限拒绝错误。
        let plain = WindowCloseErrorCode::PermissionDenied.error("permission fixture");
        // 普通构造器必须选择封闭映射文本。
        assert_eq!(plain.code, "PERMISSION_DENIED");
        // 普通构造器必须保持公开消息。
        assert_eq!(plain.message, "permission fixture");
        // 普通构造器不得制造额外详情。
        assert!(plain.details.is_null());

        // 构造只含安全结果状态的超时错误。
        let detailed = WindowCloseErrorCode::Timeout.with_details(
            // 使用稳定夹具消息。
            "timeout fixture",
            // 只提供 outcome-unknown 详情。
            json!({ "outcome": "unknown", "retrySafe": false }),
        );
        // 带详情构造器必须选择封闭映射文本。
        assert_eq!(detailed.code, "TIMEOUT");
        // 带详情构造器必须保持公开消息。
        assert_eq!(detailed.message, "timeout fixture");
        // 带详情构造器必须逐值保持安全详情。
        assert_eq!(
            detailed.details,
            // 核对完整夹具详情。
            json!({ "outcome": "unknown", "retrySafe": false })
        );
    }
}
