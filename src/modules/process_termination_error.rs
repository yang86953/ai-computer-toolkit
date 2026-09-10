//! 统一 Process Lifecycle Module 私有错误码。

// 导入公开详情与错误 envelope。
use serde_json::Value;

// 导入产品级结构化错误。
use crate::domain::AppControlError;

// 表示 Process Lifecycle Module 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProcessTerminationErrorCode {
    // 精确 opaque 进程目标命中多个当前记录。
    AmbiguousTarget,
    // 当前前景或 inventory 无法满足后台不干扰承诺。
    BackgroundOperationUnavailable,
    // dispatch 前观察到取消。
    Cancelled,
    // 当前目标缺少所选固定终止协议。
    CapabilityUnsupported,
    // mutation 缺少逐操作确认。
    ConfirmationRequired,
    // dispatch 前主机前景发生竞争变化。
    HostInterferenceDetected,
    // 目标或输入违反公开契约。
    InvalidArgument,
    // 平台在事实建立前拒绝非权限操作。
    OperationFailed,
    // 平台接受后无法认证最终结果。
    OutcomeUnknown,
    // 权限、完整性或保护目标门禁拒绝执行。
    PermissionDenied,
    // 进程代际已退出、变更或无法重新解析。
    StaleSession,
    // dispatch 前 deadline 已耗尽。
    Timeout,
}

// 提供私有错误类型到公开协议文本的唯一映射。
impl ProcessTerminationErrorCode {
    // 返回稳定公开错误码。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举全部错误类别。
        match self {
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射后台能力不可用。
            Self::BackgroundOperationUnavailable => "BACKGROUND_OPERATION_UNAVAILABLE",
            // 映射写前取消。
            Self::Cancelled => "CANCELLED",
            // 映射固定协议不支持。
            Self::CapabilityUnsupported => "CAPABILITY_UNSUPPORTED",
            // 映射确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射写前主机干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射公共参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射未接受的平台失败。
            Self::OperationFailed => "OPERATION_FAILED",
            // 映射已接受后的未知结果。
            Self::OutcomeUnknown => "OUTCOME_UNKNOWN",
            // 映射权限与保护门禁。
            Self::PermissionDenied => "PERMISSION_DENIED",
            // 映射过期进程代际。
            Self::StaleSession => "STALE_SESSION",
            // 映射写前 deadline。
            Self::Timeout => "TIMEOUT",
        }
    }

    // 构造不含额外事实的公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 使用统一 error envelope 隐藏私有枚举。
        AppControlError::new(self.as_str(), message)
    }

    // 构造只含 provider-neutral 安全详情的公开错误。
    pub(super) fn with_details(
        // 接收当前封闭错误类别。
        self,
        // 接收稳定公开消息。
        message: impl Into<String>,
        // 接收已经过滤原生事实的详情。
        details: Value,
    ) -> AppControlError {
        // 使用统一带详情 error envelope。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 验证封闭错误集合与构造器。
#[cfg(test)]
mod tests {
    // 导入 JSON 夹具构造器。
    use serde_json::json;

    // 导入被测私有错误类型。
    use super::ProcessTerminationErrorCode;

    // 锁定十二种公开错误码文本。
    #[test]
    fn all_process_termination_error_codes_are_stable() {
        // 构造完整映射表。
        let mappings = [
            // 目标歧义。
            (
                ProcessTerminationErrorCode::AmbiguousTarget,
                "AMBIGUOUS_TARGET",
            ),
            // 后台不可用。
            (
                ProcessTerminationErrorCode::BackgroundOperationUnavailable,
                "BACKGROUND_OPERATION_UNAVAILABLE",
            ),
            // 写前取消。
            (ProcessTerminationErrorCode::Cancelled, "CANCELLED"),
            // capability 不支持。
            (
                ProcessTerminationErrorCode::CapabilityUnsupported,
                "CAPABILITY_UNSUPPORTED",
            ),
            // 确认缺失。
            (
                ProcessTerminationErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 主机干扰。
            (
                ProcessTerminationErrorCode::HostInterferenceDetected,
                "HOST_INTERFERENCE_DETECTED",
            ),
            // 参数拒绝。
            (
                ProcessTerminationErrorCode::InvalidArgument,
                "INVALID_ARGUMENT",
            ),
            // 未接受失败。
            (
                ProcessTerminationErrorCode::OperationFailed,
                "OPERATION_FAILED",
            ),
            // 已接受未知结果。
            (
                ProcessTerminationErrorCode::OutcomeUnknown,
                "OUTCOME_UNKNOWN",
            ),
            // 权限拒绝。
            (
                ProcessTerminationErrorCode::PermissionDenied,
                "PERMISSION_DENIED",
            ),
            // 目标过期。
            (ProcessTerminationErrorCode::StaleSession, "STALE_SESSION"),
            // 写前超时。
            (ProcessTerminationErrorCode::Timeout, "TIMEOUT"),
        ];
        // 完整集合规模必须固定。
        assert_eq!(mappings.len(), 12);
        // 逐项核对稳定文本。
        for (code, expected) in mappings {
            // 私有枚举必须映射到唯一公开值。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通与详情构造器不改变错误事实。
    #[test]
    fn constructors_keep_message_and_safe_details() {
        // 构造普通 stale 错误。
        let plain = ProcessTerminationErrorCode::StaleSession.error("stale fixture");
        // 核对稳定错误码。
        assert_eq!(plain.code, "STALE_SESSION");
        // 核对消息正文。
        assert_eq!(plain.message, "stale fixture");
        // 普通错误不附加详情。
        assert!(plain.details.is_null());
        // 构造已接受后的未知结果。
        let detailed = ProcessTerminationErrorCode::OutcomeUnknown.with_details(
            // 使用稳定夹具消息。
            "unknown fixture",
            // 只提供公开结果状态。
            json!({ "accepted": true, "retrySafe": false }),
        );
        // 核对未知结果错误码。
        assert_eq!(detailed.code, "OUTCOME_UNKNOWN");
        // 核对安全详情未漂移。
        assert_eq!(
            detailed.details,
            // 对比完整夹具对象。
            json!({ "accepted": true, "retrySafe": false })
        );
    }
}
