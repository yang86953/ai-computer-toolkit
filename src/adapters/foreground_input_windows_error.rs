//! 定义前景输入窗口 Component 的封闭错误集合。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示前景窗口 Component 可以直接产生的错误类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ForegroundInputWindowsErrorCode {
    // 表示精确窗口在平台调用前已过期。
    StaleSession,
}

// 提供私有类型到公开错误码的唯一映射。
impl ForegroundInputWindowsErrorCode {
    // 返回稳定公开错误码。
    const fn as_str(self) -> &'static str {
        // 穷举全部平台错误类别。
        match self {
            // 映射 canonical 目标过期。
            Self::StaleSession => "STALE_SESSION",
        }
    }

    // 构造不泄漏 Win32 事实的公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 通过唯一映射建立公开 envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明纯错误映射回归。
#[cfg(test)]
mod tests {
    // 导入被测私有错误类型。
    use super::*;

    // 验证公开错误码保持稳定。
    #[test]
    fn foreground_window_error_codes_are_stable() {
        // 核对目标过期映射。
        assert_eq!(
            ForegroundInputWindowsErrorCode::StaleSession.as_str(),
            "STALE_SESSION"
        );
    }
}
