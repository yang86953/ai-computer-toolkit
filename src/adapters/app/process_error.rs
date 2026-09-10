//! 统一进程 capability provider 私有错误码。

// 导入公开错误 envelope。
use crate::domain::AppControlError;

// 表示 provider 自身只允许产生的封闭错误类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AppProcessProviderErrorCode {
    // 调用 capability 不属于本 provider。
    CapabilityUnsupported,
}

// 提供私有类别到公开错误码的唯一映射。
impl AppProcessProviderErrorCode {
    // 返回稳定公开文本。
    const fn as_str(self) -> &'static str {
        // 当前封闭集合只有 capability 缺口。
        match self {
            // 映射 provider 能力缺口。
            Self::CapabilityUnsupported => "CAPABILITY_UNSUPPORTED",
        }
    }

    // 构造公开错误并隐藏 provider 私有类型。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 验证 provider 私有错误文本。
#[cfg(test)]
mod tests {
    // 导入被测类型。
    use super::AppProcessProviderErrorCode;

    // 锁定唯一公开映射。
    #[test]
    fn capability_error_text_is_stable() {
        // 构造 provider 能力缺口。
        let error = AppProcessProviderErrorCode::CapabilityUnsupported.error("fixture");
        // 核对稳定错误码。
        assert_eq!(error.code, "CAPABILITY_UNSUPPORTED");
        // 核对稳定消息。
        assert_eq!(error.message, "fixture");
    }
}
