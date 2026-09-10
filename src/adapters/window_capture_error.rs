//! 统一 Window Capture Adapter 私有错误码。

// 导入 JSON 详情值。
use serde_json::Value;

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Window Capture Adapter 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 该类型只在 Window Capture Adapter 内部传播。
pub(super) enum WindowCaptureErrorCode {
    // 表示 WGC 帧或捕获资源无法按契约关闭。
    CaptureCleanupFailed,
    // 表示硬件和 WARP 均无法提供 D3D11 捕获设备。
    CaptureDeviceFailed,
    // 表示首帧元数据无法读取或验证。
    CaptureFrameMetadataFailed,
    // 表示 WGC 帧无法按 BGRA8 契约读回。
    CaptureReadbackFailed,
    // 表示录制期间目标内容尺寸发生变化。
    CaptureSizeChanged,
    // 表示 MTA 线程、帧池或捕获会话无法启动。
    CaptureStartFailed,
    // 表示目标无法建立有效 WGC item 或尺寸边界。
    CaptureTargetFailed,
    // 表示目标窗口当前不可见。
    CaptureTargetHidden,
    // 表示零帧预检拒绝当前捕获目标。
    CaptureTargetIneligible,
    // 表示目标窗口当前已最小化。
    CaptureTargetMinimized,
    // 表示目标在有界 deadline 内没有提供合成帧。
    CaptureTimeout,
    // 表示当前 Windows 环境没有可用 WGC runtime。
    CaptureUnavailable,
    // 表示独立 MTA 捕获线程异常终止。
    CaptureWorkerFailed,
    // 表示截图候选缺失或为空。
    ScreenshotMissing,
    // 表示 RGBA 候选无法编码为 PNG。
    ScreenshotWriteFailed,
    // 表示私有原生窗口目标已经失效。
    TargetNotFound,
}

// 提供 Adapter 私有错误类别与公开协议文本的唯一映射。
impl WindowCaptureErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射捕获资源清理失败。
            Self::CaptureCleanupFailed => "CAPTURE_CLEANUP_FAILED",
            // 映射 D3D11 捕获设备失败。
            Self::CaptureDeviceFailed => "CAPTURE_DEVICE_FAILED",
            // 映射首帧元数据失败。
            Self::CaptureFrameMetadataFailed => "CAPTURE_FRAME_METADATA_FAILED",
            // 映射帧读回失败。
            Self::CaptureReadbackFailed => "CAPTURE_READBACK_FAILED",
            // 映射捕获期间尺寸变化。
            Self::CaptureSizeChanged => "CAPTURE_SIZE_CHANGED",
            // 映射捕获启动失败。
            Self::CaptureStartFailed => "CAPTURE_START_FAILED",
            // 映射捕获目标失败。
            Self::CaptureTargetFailed => "CAPTURE_TARGET_FAILED",
            // 映射隐藏目标。
            Self::CaptureTargetHidden => "CAPTURE_TARGET_HIDDEN",
            // 映射预检拒绝目标。
            Self::CaptureTargetIneligible => "CAPTURE_TARGET_INELIGIBLE",
            // 映射最小化目标。
            Self::CaptureTargetMinimized => "CAPTURE_TARGET_MINIMIZED",
            // 映射首帧等待超时。
            Self::CaptureTimeout => "CAPTURE_TIMEOUT",
            // 映射 WGC runtime 不可用。
            Self::CaptureUnavailable => "CAPTURE_UNAVAILABLE",
            // 映射捕获线程异常终止。
            Self::CaptureWorkerFailed => "CAPTURE_WORKER_FAILED",
            // 映射截图候选缺失。
            Self::ScreenshotMissing => "SCREENSHOT_MISSING",
            // 映射 PNG 写入失败。
            Self::ScreenshotWriteFailed => "SCREENSHOT_WRITE_FAILED",
            // 映射原生目标失效。
            Self::TargetNotFound => "TARGET_NOT_FOUND",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Adapter 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 使用当前封闭错误码构造带安全详情的公开错误。
    pub(super) fn with_details(
        // 接收封闭错误码。
        self,
        // 接收公开安全消息。
        message: impl Into<String>,
        // 接收已经过 Adapter 边界筛选的安全详情。
        details: Value,
    ) -> AppControlError {
        // 隐藏 Adapter 私有类型并复用统一带详情 envelope。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 声明封闭错误类型的纯映射与构造测试。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 导入被测 Window Capture 私有错误类型。
    use super::WindowCaptureErrorCode;

    // 验证十六种 Adapter 错误码的完整稳定映射。
    #[test]
    fn all_window_capture_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持捕获清理失败码。
            (
                WindowCaptureErrorCode::CaptureCleanupFailed,
                "CAPTURE_CLEANUP_FAILED",
            ),
            // 保持捕获设备失败码。
            (
                WindowCaptureErrorCode::CaptureDeviceFailed,
                "CAPTURE_DEVICE_FAILED",
            ),
            // 保持首帧元数据失败码。
            (
                WindowCaptureErrorCode::CaptureFrameMetadataFailed,
                "CAPTURE_FRAME_METADATA_FAILED",
            ),
            // 保持帧读回失败码。
            (
                WindowCaptureErrorCode::CaptureReadbackFailed,
                "CAPTURE_READBACK_FAILED",
            ),
            // 保持捕获期间尺寸变化码。
            (
                WindowCaptureErrorCode::CaptureSizeChanged,
                "CAPTURE_SIZE_CHANGED",
            ),
            // 保持捕获启动失败码。
            (
                WindowCaptureErrorCode::CaptureStartFailed,
                "CAPTURE_START_FAILED",
            ),
            // 保持捕获目标失败码。
            (
                WindowCaptureErrorCode::CaptureTargetFailed,
                "CAPTURE_TARGET_FAILED",
            ),
            // 保持隐藏目标码。
            (
                WindowCaptureErrorCode::CaptureTargetHidden,
                "CAPTURE_TARGET_HIDDEN",
            ),
            // 保持预检拒绝目标码。
            (
                WindowCaptureErrorCode::CaptureTargetIneligible,
                "CAPTURE_TARGET_INELIGIBLE",
            ),
            // 保持最小化目标码。
            (
                WindowCaptureErrorCode::CaptureTargetMinimized,
                "CAPTURE_TARGET_MINIMIZED",
            ),
            // 保持首帧超时码。
            (WindowCaptureErrorCode::CaptureTimeout, "CAPTURE_TIMEOUT"),
            // 保持 WGC 不可用码。
            (
                WindowCaptureErrorCode::CaptureUnavailable,
                "CAPTURE_UNAVAILABLE",
            ),
            // 保持捕获线程失败码。
            (
                WindowCaptureErrorCode::CaptureWorkerFailed,
                "CAPTURE_WORKER_FAILED",
            ),
            // 保持截图候选缺失码。
            (
                WindowCaptureErrorCode::ScreenshotMissing,
                "SCREENSHOT_MISSING",
            ),
            // 保持截图写入失败码。
            (
                WindowCaptureErrorCode::ScreenshotWriteFailed,
                "SCREENSHOT_WRITE_FAILED",
            ),
            // 保持原生目标失效码。
            (WindowCaptureErrorCode::TargetNotFound, "TARGET_NOT_FOUND"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 16);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通构造器保持统一公开 envelope。
    #[test]
    fn window_capture_error_constructor_keeps_message_and_empty_details() {
        // 构造 WGC runtime 不可用夹具。
        let error = WindowCaptureErrorCode::CaptureUnavailable.error("capture fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "CAPTURE_UNAVAILABLE");
        // 构造器必须保持调用方消息。
        assert_eq!(error.message, "capture fixture");
        // 普通错误不得制造 provider 详情。
        assert!(error.details.is_null());
    }

    // 验证带详情构造器只保留调用方提供的安全详情。
    #[test]
    fn window_capture_details_constructor_keeps_safe_details() {
        // 构造只含捕获尺寸的安全详情夹具。
        let error = WindowCaptureErrorCode::CaptureSizeChanged.with_details(
            // 提供稳定公开消息。
            "size fixture",
            // 提供不含原生目标或路径的尺寸事实。
            json!({ "initialWidth": 640, "currentWidth": 800 }),
        );
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "CAPTURE_SIZE_CHANGED");
        // 构造器必须保持公开消息。
        assert_eq!(error.message, "size fixture");
        // 构造器必须逐字保持安全详情。
        assert_eq!(
            // 核对实际详情。
            error.details,
            // 核对预期安全详情。
            json!({ "initialWidth": 640, "currentWidth": 800 })
        );
    }
}
