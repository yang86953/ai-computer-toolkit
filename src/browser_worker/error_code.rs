//! 统一 Browser Worker 协议边界私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Browser Worker 协议入口允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 该类型只在 Browser Worker 协议实现内部传播。
pub(super) enum BrowserWorkerErrorCode {
    // 表示请求缺少逐操作显式确认。
    ConfirmationRequired,
    // 表示协议版本不在认证集合内。
    CapabilityGap,
    // 表示没有可用的认证 Chromium runtime。
    BrowserUnavailable,
    // 表示认证 Chromium runtime 无法启动。
    BrowserStartFailed,
    // 表示 Chromium 执行或进程监控失败。
    BrowserFailed,
    // 表示 Chromium 超过 Worker 内部 deadline。
    BrowserTimeout,
    // 表示请求、URL、viewport、deadline 或 stdin 不符合封闭契约。
    InvalidArgument,
    // 表示父 Module 预留的 staging 不符合所有权契约。
    InvalidOutputPath,
    // 表示 profile 不在工具自有临时边界内。
    TempProfileFailed,
    // 表示 PNG 候选缺失或证明不完整。
    ScreenshotMissing,
    // 表示隔离 Chromium 意外改变宿主前景。
    ForegroundChanged,
}

// 提供 Worker 私有错误类别与公开协议文本的唯一映射。
impl BrowserWorkerErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射缺少显式确认。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射未认证协议版本。
            Self::CapabilityGap => "CAPABILITY_GAP",
            // 映射认证 Chromium runtime 缺失。
            Self::BrowserUnavailable => "BROWSER_UNAVAILABLE",
            // 映射 Chromium 启动失败。
            Self::BrowserStartFailed => "BROWSER_START_FAILED",
            // 映射 Chromium 执行或监控失败。
            Self::BrowserFailed => "BROWSER_FAILED",
            // 映射 Chromium deadline。
            Self::BrowserTimeout => "BROWSER_TIMEOUT",
            // 映射请求或协议参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射 staging 路径所有权拒绝。
            Self::InvalidOutputPath => "INVALID_OUTPUT_PATH",
            // 映射 profile 所有权失败。
            Self::TempProfileFailed => "TEMP_PROFILE_FAILED",
            // 映射 PNG 候选缺失或无效。
            Self::ScreenshotMissing => "SCREENSHOT_MISSING",
            // 映射宿主前景变化。
            Self::ForegroundChanged => "FOREGROUND_CHANGED",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Worker 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型的纯映射与构造测试。
#[cfg(test)]
mod tests {
    // 导入被测 Browser Worker 私有错误类型。
    use super::BrowserWorkerErrorCode;

    // 验证十一种 Worker 自有错误码的完整稳定映射。
    #[test]
    fn all_browser_worker_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持显式确认拒绝码。
            (
                BrowserWorkerErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持认证能力缺口码。
            (BrowserWorkerErrorCode::CapabilityGap, "CAPABILITY_GAP"),
            // 保持 Chromium runtime 缺失码。
            (
                BrowserWorkerErrorCode::BrowserUnavailable,
                "BROWSER_UNAVAILABLE",
            ),
            // 保持 Chromium 启动失败码。
            (
                BrowserWorkerErrorCode::BrowserStartFailed,
                "BROWSER_START_FAILED",
            ),
            // 保持 Chromium 执行失败码。
            (BrowserWorkerErrorCode::BrowserFailed, "BROWSER_FAILED"),
            // 保持 Chromium deadline 码。
            (BrowserWorkerErrorCode::BrowserTimeout, "BROWSER_TIMEOUT"),
            // 保持参数拒绝码。
            (BrowserWorkerErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持 staging 路径拒绝码。
            (
                BrowserWorkerErrorCode::InvalidOutputPath,
                "INVALID_OUTPUT_PATH",
            ),
            // 保持临时 profile 失败码。
            (
                BrowserWorkerErrorCode::TempProfileFailed,
                "TEMP_PROFILE_FAILED",
            ),
            // 保持截图候选缺失码。
            (
                BrowserWorkerErrorCode::ScreenshotMissing,
                "SCREENSHOT_MISSING",
            ),
            // 保持前景变化码。
            (
                BrowserWorkerErrorCode::ForegroundChanged,
                "FOREGROUND_CHANGED",
            ),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 11);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通构造器保持统一 worker 失败 envelope 输入。
    #[test]
    fn browser_worker_error_constructor_keeps_message_and_empty_details() {
        // 构造 Chromium 执行失败夹具。
        let error = BrowserWorkerErrorCode::BrowserFailed.error("worker fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "BROWSER_FAILED");
        // 构造器必须保持公开安全消息。
        assert_eq!(error.message, "worker fixture");
        // Worker 自有错误不得制造额外详情。
        assert!(error.details.is_null());
    }
}
