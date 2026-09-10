//! 统一 Recording Worker 协议边界私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Recording Worker 协议入口允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 该类型只在 Recording Worker 协议实现内部传播。
pub(super) enum RecordingWorkerErrorCode {
    // 表示调用方请求、协议字段或标准输入不符合封闭契约。
    InvalidArgument,
    // 表示请求缺少逐操作显式确认。
    ConfirmationRequired,
    // 表示父 Module 预留的 staging 路径不符合所有权契约。
    InvalidOutputPath,
    // 表示隔离录制器没有生成合法 MP4 候选。
    VideoOutputMissing,
    // 表示隔离录制器生成的分析候选验证失败。
    VideoAnalysisFailed,
    // 表示 Worker 无法序列化固定协议响应。
    WorkerProtocolError,
}

// 提供 Worker 私有错误类别与公开协议文本的唯一映射。
impl RecordingWorkerErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射请求或协议参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射缺少显式确认。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射 staging 路径所有权拒绝。
            Self::InvalidOutputPath => "INVALID_OUTPUT_PATH",
            // 映射 MP4 候选缺失或无效。
            Self::VideoOutputMissing => "VIDEO_OUTPUT_MISSING",
            // 映射分析候选验证失败。
            Self::VideoAnalysisFailed => "VIDEO_ANALYSIS_FAILED",
            // 映射 Worker 响应序列化失败。
            Self::WorkerProtocolError => "WORKER_PROTOCOL_ERROR",
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
    // 导入被测 Recording Worker 私有错误类型。
    use super::RecordingWorkerErrorCode;

    // 验证六种 Worker 自有错误码的完整稳定映射。
    #[test]
    fn all_recording_worker_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持参数拒绝码。
            (
                RecordingWorkerErrorCode::InvalidArgument,
                "INVALID_ARGUMENT",
            ),
            // 保持显式确认拒绝码。
            (
                RecordingWorkerErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持 staging 路径拒绝码。
            (
                RecordingWorkerErrorCode::InvalidOutputPath,
                "INVALID_OUTPUT_PATH",
            ),
            // 保持视频候选缺失码。
            (
                RecordingWorkerErrorCode::VideoOutputMissing,
                "VIDEO_OUTPUT_MISSING",
            ),
            // 保持分析候选失败码。
            (
                RecordingWorkerErrorCode::VideoAnalysisFailed,
                "VIDEO_ANALYSIS_FAILED",
            ),
            // 保持 Worker 协议错误码。
            (
                RecordingWorkerErrorCode::WorkerProtocolError,
                "WORKER_PROTOCOL_ERROR",
            ),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 6);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通构造器保持统一 worker 失败 envelope 输入。
    #[test]
    fn recording_worker_error_constructor_keeps_message_and_empty_details() {
        // 构造 staging 所有权失败夹具。
        let error = RecordingWorkerErrorCode::InvalidOutputPath.error("worker fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "INVALID_OUTPUT_PATH");
        // 构造器必须保持公开安全消息。
        assert_eq!(error.message, "worker fixture");
        // Worker 自有错误不得制造额外详情。
        assert!(error.details.is_null());
    }
}
