//! 定义 Keyboard Input Module 的封闭错误集合。

// 导入 JSON details 与统一错误 envelope。
use serde_json::Value;

// 导入产品级错误类型。
use crate::domain::AppControlError;

// 表示 Keyboard Input Module 可以直接产生的错误类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum KeyboardInputErrorCode {
    // 表示静态权限事实无法完整取得。
    CapabilityAssessmentUnavailable,
    // 表示调用在前景执行期间被取消。
    Cancelled,
    // 表示缺少逐操作确认。
    ConfirmationRequired,
    // 表示窗口无法稳定成为前景目标。
    ForegroundActivationFailed,
    // 表示执行期间目标不再保持前景。
    HostInterferenceDetected,
    // 表示 dispatch 后结果无法确定。
    OutcomeUnknown,
    // 表示静态权限关系明确阻塞。
    PermissionDenied,
    // 表示调用超过同步 deadline。
    Timeout,
}

// 提供私有类型到公开错误码的唯一映射。
impl KeyboardInputErrorCode {
    // 返回稳定公开错误码。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举全部 Module 错误。
        match self {
            // 映射 assessment 缺口。
            Self::CapabilityAssessmentUnavailable => "CAPABILITY_ASSESSMENT_UNAVAILABLE",
            // 映射安全取消。
            Self::Cancelled => "CANCELLED",
            // 映射确认缺口。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射前景激活失败。
            Self::ForegroundActivationFailed => "FOREGROUND_ACTIVATION_FAILED",
            // 映射前景干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射不确定结果。
            Self::OutcomeUnknown => "OUTCOME_UNKNOWN",
            // 映射权限拒绝。
            Self::PermissionDenied => "PERMISSION_DENIED",
            // 映射 deadline 到期。
            Self::Timeout => "TIMEOUT",
        }
    }

    // 构造普通统一错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Module 私有类型。
        AppControlError::new(self.as_str(), message)
    }

    // 构造带固定安全事实的统一错误。
    pub(super) fn with_details(
        self,
        // 接收公开安全消息。
        message: impl Into<String>,
        // 接收已经净化的 details。
        details: Value,
    ) -> AppControlError {
        // 隐藏 Module 私有类型。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 声明错误映射纯测试。
#[cfg(test)]
mod tests {
    // 导入被测错误集合。
    use super::KeyboardInputErrorCode;

    // 验证全部公开错误文本保持稳定。
    #[test]
    fn keyboard_input_error_codes_are_stable() {
        // 固定全部映射。
        let mappings = [
            // assessment 缺口。
            (
                KeyboardInputErrorCode::CapabilityAssessmentUnavailable,
                "CAPABILITY_ASSESSMENT_UNAVAILABLE",
            ),
            // 安全取消。
            (KeyboardInputErrorCode::Cancelled, "CANCELLED"),
            // 确认缺口。
            (
                KeyboardInputErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 前景激活失败。
            (
                KeyboardInputErrorCode::ForegroundActivationFailed,
                "FOREGROUND_ACTIVATION_FAILED",
            ),
            // 前景干扰。
            (
                KeyboardInputErrorCode::HostInterferenceDetected,
                "HOST_INTERFERENCE_DETECTED",
            ),
            // 不确定结果。
            (KeyboardInputErrorCode::OutcomeUnknown, "OUTCOME_UNKNOWN"),
            // 权限拒绝。
            (
                KeyboardInputErrorCode::PermissionDenied,
                "PERMISSION_DENIED",
            ),
            // deadline 到期。
            (KeyboardInputErrorCode::Timeout, "TIMEOUT"),
        ];
        // 核对集合规模。
        assert_eq!(mappings.len(), 8);
        // 逐项核对稳定文本。
        for (code, expected) in mappings {
            // 文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }
}
