//! 统一 Video Recording Adapter 私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Video Recording Adapter 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 该类型只在 Video Recording Adapter 内部传播。
pub(super) enum VideoRecordingErrorCode {
    // 表示捕获帧无法转换为合法 RGBA 图像。
    CaptureReadbackFailed,
    // 表示捕获目标尺寸不满足录制边界。
    CaptureTargetFailed,
    // 表示既有视频目标缺少独立覆盖许可。
    OverwriteConfirmationRequired,
    // 表示关键帧或 storyboard 分析失败。
    VideoAnalysisFailed,
    // 表示分析产物无法安全写入。
    VideoAnalysisWriteFailed,
    // 表示编码器无法完成 MP4 收尾。
    VideoEncoderFailed,
    // 表示编码会话配置或启动失败。
    VideoEncoderStartFailed,
    // 表示系统没有可用的自有 H.264 编码链。
    VideoEncoderUnavailable,
    // 表示捕获帧无法写入编码器。
    VideoEncoderWriteFailed,
    // 表示视频候选或最终产物缺失。
    VideoOutputMissing,
    // 表示视频 staging 或原子提交失败。
    VideoWriteFailed,
}

// 提供 Adapter 私有错误类别与公开协议文本的唯一映射。
impl VideoRecordingErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射捕获帧回读失败。
            Self::CaptureReadbackFailed => "CAPTURE_READBACK_FAILED",
            // 映射捕获目标失败。
            Self::CaptureTargetFailed => "CAPTURE_TARGET_FAILED",
            // 映射覆盖确认缺失。
            Self::OverwriteConfirmationRequired => "OVERWRITE_CONFIRMATION_REQUIRED",
            // 映射分析失败。
            Self::VideoAnalysisFailed => "VIDEO_ANALYSIS_FAILED",
            // 映射分析产物写入失败。
            Self::VideoAnalysisWriteFailed => "VIDEO_ANALYSIS_WRITE_FAILED",
            // 映射编码器收尾失败。
            Self::VideoEncoderFailed => "VIDEO_ENCODER_FAILED",
            // 映射编码器启动失败。
            Self::VideoEncoderStartFailed => "VIDEO_ENCODER_START_FAILED",
            // 映射编码器不可用。
            Self::VideoEncoderUnavailable => "VIDEO_ENCODER_UNAVAILABLE",
            // 映射编码帧写入失败。
            Self::VideoEncoderWriteFailed => "VIDEO_ENCODER_WRITE_FAILED",
            // 映射视频产物缺失。
            Self::VideoOutputMissing => "VIDEO_OUTPUT_MISSING",
            // 映射视频提交失败。
            Self::VideoWriteFailed => "VIDEO_WRITE_FAILED",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Adapter 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型的纯映射与构造测试。
#[cfg(test)]
mod tests {
    // 导入被测 Video Recording 私有错误类型。
    use super::VideoRecordingErrorCode;

    // 验证十一种 Adapter 错误码的完整稳定映射。
    #[test]
    fn all_video_recording_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持捕获帧回读失败码。
            (
                VideoRecordingErrorCode::CaptureReadbackFailed,
                "CAPTURE_READBACK_FAILED",
            ),
            // 保持捕获目标失败码。
            (
                VideoRecordingErrorCode::CaptureTargetFailed,
                "CAPTURE_TARGET_FAILED",
            ),
            // 保持覆盖确认缺失码。
            (
                VideoRecordingErrorCode::OverwriteConfirmationRequired,
                "OVERWRITE_CONFIRMATION_REQUIRED",
            ),
            // 保持分析失败码。
            (
                VideoRecordingErrorCode::VideoAnalysisFailed,
                "VIDEO_ANALYSIS_FAILED",
            ),
            // 保持分析写入失败码。
            (
                VideoRecordingErrorCode::VideoAnalysisWriteFailed,
                "VIDEO_ANALYSIS_WRITE_FAILED",
            ),
            // 保持编码器收尾失败码。
            (
                VideoRecordingErrorCode::VideoEncoderFailed,
                "VIDEO_ENCODER_FAILED",
            ),
            // 保持编码器启动失败码。
            (
                VideoRecordingErrorCode::VideoEncoderStartFailed,
                "VIDEO_ENCODER_START_FAILED",
            ),
            // 保持编码器不可用码。
            (
                VideoRecordingErrorCode::VideoEncoderUnavailable,
                "VIDEO_ENCODER_UNAVAILABLE",
            ),
            // 保持编码帧写入失败码。
            (
                VideoRecordingErrorCode::VideoEncoderWriteFailed,
                "VIDEO_ENCODER_WRITE_FAILED",
            ),
            // 保持视频产物缺失码。
            (
                VideoRecordingErrorCode::VideoOutputMissing,
                "VIDEO_OUTPUT_MISSING",
            ),
            // 保持视频提交失败码。
            (
                VideoRecordingErrorCode::VideoWriteFailed,
                "VIDEO_WRITE_FAILED",
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

    // 验证普通构造器保持公开消息与空详情。
    #[test]
    fn video_recording_error_constructor_keeps_message_and_empty_details() {
        // 构造视频候选缺失错误。
        let error = VideoRecordingErrorCode::VideoOutputMissing.error("video fixture");
        // 普通构造器必须选择封闭映射文本。
        assert_eq!(error.code, "VIDEO_OUTPUT_MISSING");
        // 普通构造器必须保持公开消息。
        assert_eq!(error.message, "video fixture");
        // 普通构造器不得制造额外详情。
        assert!(error.details.is_null());
    }
}
