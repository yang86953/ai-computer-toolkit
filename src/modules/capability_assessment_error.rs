//! 统一 Capability Assessment Module 私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Capability Assessment Module 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CapabilityAssessmentErrorCode {
    // 表示精确目标匹配多个当前记录。
    AmbiguousTarget,
    // 表示只读评估期间宿主前景发生变化。
    HostInterferenceDetected,
    // 表示 canonical 目标在使用时无法重新发现。
    StaleSession,
}

// 提供 Module 私有错误类别与公开协议文本的唯一映射。
impl CapabilityAssessmentErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射宿主前景干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射使用时目标过期。
            Self::StaleSession => "STALE_SESSION",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Module 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型的纯映射与构造测试。
#[cfg(test)]
mod tests {
    // 导入被测 Capability Assessment 私有错误类型。
    use super::CapabilityAssessmentErrorCode;

    // 验证三种 Module 错误码的完整稳定映射。
    #[test]
    fn all_capability_assessment_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持目标歧义码。
            (
                CapabilityAssessmentErrorCode::AmbiguousTarget,
                "AMBIGUOUS_TARGET",
            ),
            // 保持宿主前景干扰码。
            (
                CapabilityAssessmentErrorCode::HostInterferenceDetected,
                "HOST_INTERFERENCE_DETECTED",
            ),
            // 保持目标过期码。
            (CapabilityAssessmentErrorCode::StaleSession, "STALE_SESSION"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 3);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通构造器保持统一公开 envelope。
    #[test]
    fn capability_assessment_error_constructor_keeps_message_and_empty_details() {
        // 构造使用时目标过期夹具。
        let error = CapabilityAssessmentErrorCode::StaleSession.error("stale fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "STALE_SESSION");
        // 构造器必须保持公开消息。
        assert_eq!(error.message, "stale fixture");
        // 普通构造器不得制造额外详情。
        assert!(error.details.is_null());
    }
}
