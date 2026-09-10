//! 定义 Window Lifecycle Module 的封闭错误集合。

// 导入 JSON 详情值。
use serde_json::Value;

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Window Lifecycle Module 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowLifecycleErrorCode {
    // 表示 opaque 窗口目标当前无法唯一解析。
    AmbiguousTarget,
    // 表示静态权限事实无法给出认证结论。
    CapabilityAssessmentUnavailable,
    // 表示目标窗口样式或状态不支持请求动作。
    CapabilityUnsupported,
    // 表示写入前用户取消请求。
    Cancelled,
    // 表示 mutation 缺少逐操作确认。
    ConfirmationRequired,
    // 表示物理像素坐标上下文无法认证。
    CoordinateContextUnavailable,
    // 表示缺少预先前景影响同意。
    ForegroundConsentRequired,
    // 表示已接受调用后宿主前景发生未授权变化。
    HostInterferenceDetected,
    // 表示目标、输入或几何范围不合法。
    InvalidArgument,
    // 表示平台调用在接受前失败。
    OperationFailed,
    // 表示调用已接受但最终状态无法认证。
    OutcomeUnknown,
    // 表示系统权限边界拒绝固定窗口调用。
    PermissionDenied,
    // 表示精确窗口身份已经过期。
    StaleSession,
    // 表示写入前已经到达 deadline。
    Timeout,
}

// 提供 Module 私有错误类别与公开协议文本的唯一映射。
impl WindowLifecycleErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合。
        match self {
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射 assessment 缺口。
            Self::CapabilityAssessmentUnavailable => "CAPABILITY_ASSESSMENT_UNAVAILABLE",
            // 映射动作不支持。
            Self::CapabilityUnsupported => "CAPABILITY_UNSUPPORTED",
            // 映射安全取消。
            Self::Cancelled => "CANCELLED",
            // 映射逐操作确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射物理坐标缺口。
            Self::CoordinateContextUnavailable => "COORDINATE_CONTEXT_UNAVAILABLE",
            // 映射前景同意缺失。
            Self::ForegroundConsentRequired => "FOREGROUND_CONSENT_REQUIRED",
            // 映射宿主干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射公共参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射接受前平台失败。
            Self::OperationFailed => "OPERATION_FAILED",
            // 映射接受后结果未知。
            Self::OutcomeUnknown => "OUTCOME_UNKNOWN",
            // 映射权限拒绝。
            Self::PermissionDenied => "PERMISSION_DENIED",
            // 映射目标过期。
            Self::StaleSession => "STALE_SESSION",
            // 映射接受前超时。
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

// 声明封闭错误类型的纯映射测试。
#[cfg(test)]
mod tests {
    // 导入被测错误类型。
    use super::WindowLifecycleErrorCode;

    // 验证完整 Module 错误码集合保持稳定公开文本。
    #[test]
    fn all_window_lifecycle_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持目标歧义码。
            (
                WindowLifecycleErrorCode::AmbiguousTarget,
                "AMBIGUOUS_TARGET",
            ),
            // 保持 assessment 缺口码。
            (
                WindowLifecycleErrorCode::CapabilityAssessmentUnavailable,
                "CAPABILITY_ASSESSMENT_UNAVAILABLE",
            ),
            // 保持动作不支持码。
            (
                WindowLifecycleErrorCode::CapabilityUnsupported,
                "CAPABILITY_UNSUPPORTED",
            ),
            // 保持取消码。
            (WindowLifecycleErrorCode::Cancelled, "CANCELLED"),
            // 保持确认缺失码。
            (
                WindowLifecycleErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持坐标上下文缺口码。
            (
                WindowLifecycleErrorCode::CoordinateContextUnavailable,
                "COORDINATE_CONTEXT_UNAVAILABLE",
            ),
            // 保持前景同意缺失码。
            (
                WindowLifecycleErrorCode::ForegroundConsentRequired,
                "FOREGROUND_CONSENT_REQUIRED",
            ),
            // 保持宿主干扰码。
            (
                WindowLifecycleErrorCode::HostInterferenceDetected,
                "HOST_INTERFERENCE_DETECTED",
            ),
            // 保持参数拒绝码。
            (
                WindowLifecycleErrorCode::InvalidArgument,
                "INVALID_ARGUMENT",
            ),
            // 保持操作失败码。
            (
                WindowLifecycleErrorCode::OperationFailed,
                "OPERATION_FAILED",
            ),
            // 保持结果未知码。
            (WindowLifecycleErrorCode::OutcomeUnknown, "OUTCOME_UNKNOWN"),
            // 保持权限拒绝码。
            (
                WindowLifecycleErrorCode::PermissionDenied,
                "PERMISSION_DENIED",
            ),
            // 保持 stale 码。
            (WindowLifecycleErrorCode::StaleSession, "STALE_SESSION"),
            // 保持超时码。
            (WindowLifecycleErrorCode::Timeout, "TIMEOUT"),
        ];
        // 核对封闭集合规模。
        assert_eq!(mappings.len(), 14);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }
}
