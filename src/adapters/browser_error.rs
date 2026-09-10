//! 统一 Browser Adapter 私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Browser Adapter 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AppBrowserErrorCode {
    // 表示 Browser Adapter 未认证请求的后台操作。
    BackgroundOperationUnavailable,
    // 表示调用方缺少必需 operation。
    InvalidArgument,
}

// 提供 Browser Adapter 私有错误码与公开协议文本的唯一映射。
impl AppBrowserErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射后台操作缺口。
            Self::BackgroundOperationUnavailable => "BACKGROUND_OPERATION_UNAVAILABLE",
            // 映射调用参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Browser Adapter 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型的纯映射测试。
#[cfg(test)]
mod tests {
    // 导入被测 Browser Adapter 私有错误类型。
    use super::AppBrowserErrorCode;

    // 验证两种 Browser Adapter 错误码的完整稳定映射。
    #[test]
    fn all_browser_adapter_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持后台操作缺口码。
            (
                AppBrowserErrorCode::BackgroundOperationUnavailable,
                "BACKGROUND_OPERATION_UNAVAILABLE",
            ),
            // 保持参数拒绝码。
            (AppBrowserErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 2);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }
}
