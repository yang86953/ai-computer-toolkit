//! 统一 Capture Worker 协议边界私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Capture Worker 协议入口允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 该类型只在 Capture Worker 协议实现内部传播。
pub(super) enum CaptureWorkerErrorCode {
    // 表示调用方请求、协议字段或标准输入不符合封闭契约。
    InvalidArgument,
    // 表示请求缺少逐操作显式确认。
    ConfirmationRequired,
    // 表示协议版本或 operation 不在认证集合内。
    CapabilityGap,
    // 表示父 Module 预留的 staging 路径不符合所有权契约。
    InvalidOutputPath,
    // 表示 PNG 候选大小、读取或容器签名验证失败。
    ScreenshotWriteFailed,
}

// 提供 Worker 私有错误类别与公开协议文本的唯一映射。
impl CaptureWorkerErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射请求或协议参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射缺少显式确认。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射未认证协议或 operation。
            Self::CapabilityGap => "CAPABILITY_GAP",
            // 映射 staging 路径所有权拒绝。
            Self::InvalidOutputPath => "INVALID_OUTPUT_PATH",
            // 映射 PNG 候选写入或验证失败。
            Self::ScreenshotWriteFailed => "SCREENSHOT_WRITE_FAILED",
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
    // 导入被测 Capture Worker 私有错误类型。
    use super::CaptureWorkerErrorCode;

    // 验证五种 Worker 自有错误码的完整稳定映射。
    #[test]
    fn all_capture_worker_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持参数拒绝码。
            (CaptureWorkerErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持显式确认拒绝码。
            (
                CaptureWorkerErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持认证能力缺口码。
            (CaptureWorkerErrorCode::CapabilityGap, "CAPABILITY_GAP"),
            // 保持 staging 路径拒绝码。
            (
                CaptureWorkerErrorCode::InvalidOutputPath,
                "INVALID_OUTPUT_PATH",
            ),
            // 保持 PNG 写入失败码。
            (
                CaptureWorkerErrorCode::ScreenshotWriteFailed,
                "SCREENSHOT_WRITE_FAILED",
            ),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 5);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通构造器保持统一 worker 失败 envelope 输入。
    #[test]
    fn capture_worker_error_constructor_keeps_message_and_empty_details() {
        // 构造 staging 所有权失败夹具。
        let error = CaptureWorkerErrorCode::InvalidOutputPath.error("worker fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "INVALID_OUTPUT_PATH");
        // 构造器必须保持公开安全消息。
        assert_eq!(error.message, "worker fixture");
        // Worker 自有错误不得制造额外详情。
        assert!(error.details.is_null());
    }
}
