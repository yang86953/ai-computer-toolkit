//! 统一 Window Screenshot Module 私有错误码及 worker 失败白名单。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Window Screenshot Module 允许产生或转发的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowScreenshotErrorCode {
    // 表示 opaque 目标无法唯一解析。
    AmbiguousTarget,
    // 表示捕获设备创建或使用失败。
    CaptureDeviceFailed,
    // 表示截图像素读取或编码失败。
    CaptureReadbackFailed,
    // 表示捕获会话或线程启动失败。
    CaptureStartFailed,
    // 表示 WGC 无法绑定精确目标。
    CaptureTargetFailed,
    // 表示精确目标在捕获时已经隐藏。
    CaptureTargetHidden,
    // 表示零帧预检拒绝目标。
    CaptureTargetIneligible,
    // 表示精确目标在捕获时已经最小化。
    CaptureTargetMinimized,
    // 表示截图等待超过 deadline。
    CaptureTimeout,
    // 表示系统捕获 runtime 不可用。
    CaptureUnavailable,
    // 表示调用方没有逐操作确认。
    ConfirmationRequired,
    // 表示主进程或 worker 观察到前景变化。
    HostInterferenceDetected,
    // 表示请求参数不满足公开契约。
    InvalidArgument,
    // 表示输出目标不满足安全路径契约。
    InvalidOutputPath,
    // 表示既有输出缺少独立覆盖许可。
    OverwriteConfirmationRequired,
    // 表示截图候选无法安全写入或提交。
    ScreenshotWriteFailed,
    // 表示 opaque 目标已失效或身份改变。
    StaleSession,
    // 表示 worker envelope 违反封闭协议。
    WorkerProtocolViolation,
}

// 提供 Module 私有错误码与公开协议文本的唯一映射。
impl WindowScreenshotErrorCode {
    // 固定 capture worker 失败 envelope 允许转发的十五种类别。
    const WORKER_ALLOWED: [Self; 15] = [
        // 允许失效目标。
        Self::StaleSession,
        // 允许歧义目标。
        Self::AmbiguousTarget,
        // 允许确认缺失。
        Self::ConfirmationRequired,
        // 允许不可捕获目标。
        Self::CaptureTargetIneligible,
        // 允许隐藏目标分类。
        Self::CaptureTargetHidden,
        // 允许最小化目标分类。
        Self::CaptureTargetMinimized,
        // 允许捕获 runtime 缺失。
        Self::CaptureUnavailable,
        // 允许捕获目标失败。
        Self::CaptureTargetFailed,
        // 允许捕获设备失败。
        Self::CaptureDeviceFailed,
        // 允许捕获启动失败。
        Self::CaptureStartFailed,
        // 允许截图超时。
        Self::CaptureTimeout,
        // 允许像素读取或编码失败。
        Self::CaptureReadbackFailed,
        // 允许截图写入失败。
        Self::ScreenshotWriteFailed,
        // 允许 staging 输出路径失效。
        Self::InvalidOutputPath,
        // 允许前景干扰。
        Self::HostInterferenceDetected,
    ];

    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射歧义目标。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射捕获设备失败。
            Self::CaptureDeviceFailed => "CAPTURE_DEVICE_FAILED",
            // 映射截图像素读取失败。
            Self::CaptureReadbackFailed => "CAPTURE_READBACK_FAILED",
            // 映射捕获启动失败。
            Self::CaptureStartFailed => "CAPTURE_START_FAILED",
            // 映射捕获目标失败。
            Self::CaptureTargetFailed => "CAPTURE_TARGET_FAILED",
            // 映射隐藏捕获目标。
            Self::CaptureTargetHidden => "CAPTURE_TARGET_HIDDEN",
            // 映射不可捕获目标。
            Self::CaptureTargetIneligible => "CAPTURE_TARGET_INELIGIBLE",
            // 映射最小化捕获目标。
            Self::CaptureTargetMinimized => "CAPTURE_TARGET_MINIMIZED",
            // 映射捕获超时。
            Self::CaptureTimeout => "CAPTURE_TIMEOUT",
            // 映射捕获 runtime 不可用。
            Self::CaptureUnavailable => "CAPTURE_UNAVAILABLE",
            // 映射确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射前景干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射无效输出路径。
            Self::InvalidOutputPath => "INVALID_OUTPUT_PATH",
            // 映射覆盖确认缺失。
            Self::OverwriteConfirmationRequired => "OVERWRITE_CONFIRMATION_REQUIRED",
            // 映射截图写入失败。
            Self::ScreenshotWriteFailed => "SCREENSHOT_WRITE_FAILED",
            // 映射失效目标。
            Self::StaleSession => "STALE_SESSION",
            // 映射 worker 协议违规。
            Self::WorkerProtocolViolation => "WORKER_PROTOCOL_VIOLATION",
        }
    }

    // 从 capture worker 失败 envelope 解析允许转发的封闭类别。
    pub(super) fn from_worker(code: &str) -> Option<Self> {
        // 只从封闭类型白名单中查找逐字匹配项。
        Self::WORKER_ALLOWED
            // 按值遍历复制型私有枚举。
            .into_iter()
            // 公共字符串只由 as_str 唯一映射。
            .find(|candidate| candidate.as_str() == code)
    }

    // 使用当前封闭错误码构造产品级公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Module 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型的纯映射测试。
#[cfg(test)]
mod tests {
    // 导入被测私有错误类型。
    use super::WindowScreenshotErrorCode;

    // 验证十八种 Module 错误码的完整稳定映射。
    #[test]
    fn all_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持目标歧义码。
            (
                WindowScreenshotErrorCode::AmbiguousTarget,
                "AMBIGUOUS_TARGET",
            ),
            // 保持捕获设备失败码。
            (
                WindowScreenshotErrorCode::CaptureDeviceFailed,
                "CAPTURE_DEVICE_FAILED",
            ),
            // 保持像素读取失败码。
            (
                WindowScreenshotErrorCode::CaptureReadbackFailed,
                "CAPTURE_READBACK_FAILED",
            ),
            // 保持捕获启动失败码。
            (
                WindowScreenshotErrorCode::CaptureStartFailed,
                "CAPTURE_START_FAILED",
            ),
            // 保持捕获目标失败码。
            (
                WindowScreenshotErrorCode::CaptureTargetFailed,
                "CAPTURE_TARGET_FAILED",
            ),
            // 保持隐藏捕获目标码。
            (
                WindowScreenshotErrorCode::CaptureTargetHidden,
                "CAPTURE_TARGET_HIDDEN",
            ),
            // 保持不可捕获目标码。
            (
                WindowScreenshotErrorCode::CaptureTargetIneligible,
                "CAPTURE_TARGET_INELIGIBLE",
            ),
            // 保持最小化捕获目标码。
            (
                WindowScreenshotErrorCode::CaptureTargetMinimized,
                "CAPTURE_TARGET_MINIMIZED",
            ),
            // 保持捕获超时码。
            (WindowScreenshotErrorCode::CaptureTimeout, "CAPTURE_TIMEOUT"),
            // 保持捕获 runtime 不可用码。
            (
                WindowScreenshotErrorCode::CaptureUnavailable,
                "CAPTURE_UNAVAILABLE",
            ),
            // 保持确认缺失码。
            (
                WindowScreenshotErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持前景干扰码。
            (
                WindowScreenshotErrorCode::HostInterferenceDetected,
                "HOST_INTERFERENCE_DETECTED",
            ),
            // 保持参数拒绝码。
            (
                WindowScreenshotErrorCode::InvalidArgument,
                "INVALID_ARGUMENT",
            ),
            // 保持无效输出路径码。
            (
                WindowScreenshotErrorCode::InvalidOutputPath,
                "INVALID_OUTPUT_PATH",
            ),
            // 保持覆盖确认缺失码。
            (
                WindowScreenshotErrorCode::OverwriteConfirmationRequired,
                "OVERWRITE_CONFIRMATION_REQUIRED",
            ),
            // 保持截图写入失败码。
            (
                WindowScreenshotErrorCode::ScreenshotWriteFailed,
                "SCREENSHOT_WRITE_FAILED",
            ),
            // 保持目标失效码。
            (WindowScreenshotErrorCode::StaleSession, "STALE_SESSION"),
            // 保持 worker 协议违规码。
            (
                WindowScreenshotErrorCode::WorkerProtocolViolation,
                "WORKER_PROTOCOL_VIOLATION",
            ),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 18);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证 worker 白名单只接受十五种稳定类别。
    #[test]
    fn worker_error_whitelist_is_closed() {
        // 固定 worker 可转发文本集合。
        let allowed = [
            // 允许失效目标。
            "STALE_SESSION",
            // 允许歧义目标。
            "AMBIGUOUS_TARGET",
            // 允许确认缺失。
            "CONFIRMATION_REQUIRED",
            // 允许不可捕获目标。
            "CAPTURE_TARGET_INELIGIBLE",
            // 允许隐藏目标分类。
            "CAPTURE_TARGET_HIDDEN",
            // 允许最小化目标分类。
            "CAPTURE_TARGET_MINIMIZED",
            // 允许捕获 runtime 缺失。
            "CAPTURE_UNAVAILABLE",
            // 允许捕获目标失败。
            "CAPTURE_TARGET_FAILED",
            // 允许捕获设备失败。
            "CAPTURE_DEVICE_FAILED",
            // 允许捕获启动失败。
            "CAPTURE_START_FAILED",
            // 允许截图超时。
            "CAPTURE_TIMEOUT",
            // 允许像素读取失败。
            "CAPTURE_READBACK_FAILED",
            // 允许截图写入失败。
            "SCREENSHOT_WRITE_FAILED",
            // 允许无效 staging 路径。
            "INVALID_OUTPUT_PATH",
            // 允许前景干扰。
            "HOST_INTERFERENCE_DETECTED",
        ];
        // 核对白名单规模不漂移。
        assert_eq!(allowed.len(), 15);
        // 逐项核对类型化解析。
        for code in allowed {
            // 允许项必须返回同文封闭类型。
            assert_eq!(
                WindowScreenshotErrorCode::from_worker(code)
                    // 白名单项必须可解析。
                    .map(WindowScreenshotErrorCode::as_str),
                // 公开文本必须保持不变。
                Some(code)
            );
        }
        // 未知 provider 错误不得越过 Module 边界。
        assert_eq!(
            WindowScreenshotErrorCode::from_worker("UNKNOWN_PROVIDER_ERROR"),
            None
        );
    }
}
