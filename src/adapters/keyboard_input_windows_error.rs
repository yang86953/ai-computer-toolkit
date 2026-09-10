//! 定义 Windows 键盘输入 Adapter 的封闭错误集合。

// 导入公开 JSON 值。
use serde_json::Value;

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示平台键盘 Component 可以直接产生的错误类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum KeyboardInputWindowsErrorCode {
    // 表示 Rust allowlist 与私有 Windows 映射发生漂移。
    MappingUnavailable,
    // 表示 Windows 没有完整接收单个输入事件。
    KeyboardDispatchFailed,
}

// 提供私有类型到公开错误码的唯一映射。
impl KeyboardInputWindowsErrorCode {
    // 返回稳定公开错误码。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举全部平台错误类别。
        match self {
            // 映射私有键表漂移。
            Self::MappingUnavailable => "KEYBOARD_MAPPING_UNAVAILABLE",
            // 映射平台调度拒绝或部分结果。
            Self::KeyboardDispatchFailed => "KEYBOARD_DISPATCH_FAILED",
        }
    }

    // 构造不泄漏 Win32 事实的公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 通过唯一映射建立公开 envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 构造携带 provider-neutral 补偿事实的公开错误。
    pub(super) fn with_details(
        self,
        // 接收稳定公开消息。
        message: impl Into<String>,
        // 接收不含平台键码的公开细节。
        details: Value,
    ) -> AppControlError {
        // 通过唯一映射建立带细节 envelope。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 声明纯错误映射回归。
#[cfg(test)]
mod tests {
    // 导入被测私有错误类型。
    use super::*;

    // 验证公开错误码保持稳定。
    #[test]
    fn keyboard_windows_error_codes_are_stable() {
        // 核对映射漂移错误。
        assert_eq!(
            KeyboardInputWindowsErrorCode::MappingUnavailable.as_str(),
            "KEYBOARD_MAPPING_UNAVAILABLE"
        );
        // 核对调度失败错误。
        assert_eq!(
            KeyboardInputWindowsErrorCode::KeyboardDispatchFailed.as_str(),
            "KEYBOARD_DISPATCH_FAILED"
        );
    }
}
