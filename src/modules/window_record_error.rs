//! 统一 Window Record Module 私有错误码与 worker 白名单。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Window Record Module 使用的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowRecordErrorCode {
    // 表示 opaque 窗口目标当前无法唯一解析。
    AmbiguousTarget,
    // 表示捕获资源清理失败。
    CaptureCleanupFailed,
    // 表示捕获设备失败。
    CaptureDeviceFailed,
    // 表示捕获 readback 失败。
    CaptureReadbackFailed,
    // 表示捕获期间尺寸变化。
    CaptureSizeChanged,
    // 表示捕获启动失败。
    CaptureStartFailed,
    // 表示捕获目标失败。
    CaptureTargetFailed,
    // 表示捕获目标变为隐藏。
    CaptureTargetHidden,
    // 表示目标不符合认证捕获资格。
    CaptureTargetIneligible,
    // 表示捕获目标变为最小化。
    CaptureTargetMinimized,
    // 表示捕获内部 deadline 超时。
    CaptureTimeout,
    // 表示认证捕获 runtime 不可用。
    CaptureUnavailable,
    // 表示捕获 worker 生命周期失败。
    CaptureWorkerFailed,
    // 表示 mutation 缺少逐操作确认。
    ConfirmationRequired,
    // 表示录制期间前景身份发生变化。
    ForegroundChanged,
    // 表示目标或录制输入不合法。
    InvalidArgument,
    // 表示最终或 staging 输出路径不可用。
    InvalidOutputPath,
    // 表示现有输出缺少覆盖确认。
    OverwriteConfirmationRequired,
    // 表示精确窗口身份已经过期。
    StaleSession,
    // 表示 worker process Component 的通用 timeout 分类。
    WorkerProcessTimeout,
    // 表示录制分析候选不合法。
    VideoAnalysisFailed,
    // 表示 worker 无法写入分析候选。
    VideoAnalysisWriteFailed,
    // 表示多产物事务提交失败。
    VideoArtifactCommitFailed,
    // 表示多产物事务回滚后结果未知。
    VideoArtifactResultUnknown,
    // 表示编码器运行失败。
    VideoEncoderFailed,
    // 表示编码器启动失败。
    VideoEncoderStartFailed,
    // 表示认证 H.264 编码器不可用。
    VideoEncoderUnavailable,
    // 表示编码器写入失败。
    VideoEncoderWriteFailed,
    // 表示 worker 未产生合法 MP4 候选。
    VideoOutputMissing,
    // 表示外层录制 watchdog 到达 deadline。
    VideoRecordingTimeout,
    // 表示 worker 无法写入视频候选。
    VideoWriteFailed,
    // 表示 worker envelope 违反固定协议。
    WorkerProtocolError,
}

// 提供 Module 私有错误类别、worker 白名单与公开文本的唯一映射。
impl WindowRecordErrorCode {
    // 返回稳定公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射捕获清理失败。
            Self::CaptureCleanupFailed => "CAPTURE_CLEANUP_FAILED",
            // 映射捕获设备失败。
            Self::CaptureDeviceFailed => "CAPTURE_DEVICE_FAILED",
            // 映射捕获 readback 失败。
            Self::CaptureReadbackFailed => "CAPTURE_READBACK_FAILED",
            // 映射捕获尺寸变化。
            Self::CaptureSizeChanged => "CAPTURE_SIZE_CHANGED",
            // 映射捕获启动失败。
            Self::CaptureStartFailed => "CAPTURE_START_FAILED",
            // 映射捕获目标失败。
            Self::CaptureTargetFailed => "CAPTURE_TARGET_FAILED",
            // 映射捕获目标隐藏。
            Self::CaptureTargetHidden => "CAPTURE_TARGET_HIDDEN",
            // 映射目标资格拒绝。
            Self::CaptureTargetIneligible => "CAPTURE_TARGET_INELIGIBLE",
            // 映射捕获目标最小化。
            Self::CaptureTargetMinimized => "CAPTURE_TARGET_MINIMIZED",
            // 映射捕获内部超时。
            Self::CaptureTimeout => "CAPTURE_TIMEOUT",
            // 映射捕获 runtime 不可用。
            Self::CaptureUnavailable => "CAPTURE_UNAVAILABLE",
            // 映射捕获 worker 失败。
            Self::CaptureWorkerFailed => "CAPTURE_WORKER_FAILED",
            // 映射逐操作确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射录制期间前景变化。
            Self::ForegroundChanged => "FOREGROUND_CHANGED",
            // 映射公共参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射输出路径拒绝。
            Self::InvalidOutputPath => "INVALID_OUTPUT_PATH",
            // 映射覆盖确认缺失。
            Self::OverwriteConfirmationRequired => "OVERWRITE_CONFIRMATION_REQUIRED",
            // 映射目标过期。
            Self::StaleSession => "STALE_SESSION",
            // 映射 worker process 通用超时。
            Self::WorkerProcessTimeout => "TIMEOUT",
            // 映射分析候选失败。
            Self::VideoAnalysisFailed => "VIDEO_ANALYSIS_FAILED",
            // 映射分析候选写入失败。
            Self::VideoAnalysisWriteFailed => "VIDEO_ANALYSIS_WRITE_FAILED",
            // 映射多产物提交失败。
            Self::VideoArtifactCommitFailed => "VIDEO_ARTIFACT_COMMIT_FAILED",
            // 映射多产物结果未知。
            Self::VideoArtifactResultUnknown => "VIDEO_ARTIFACT_RESULT_UNKNOWN",
            // 映射编码器运行失败。
            Self::VideoEncoderFailed => "VIDEO_ENCODER_FAILED",
            // 映射编码器启动失败。
            Self::VideoEncoderStartFailed => "VIDEO_ENCODER_START_FAILED",
            // 映射编码器不可用。
            Self::VideoEncoderUnavailable => "VIDEO_ENCODER_UNAVAILABLE",
            // 映射编码器写入失败。
            Self::VideoEncoderWriteFailed => "VIDEO_ENCODER_WRITE_FAILED",
            // 映射视频候选缺失。
            Self::VideoOutputMissing => "VIDEO_OUTPUT_MISSING",
            // 映射外层录制超时。
            Self::VideoRecordingTimeout => "VIDEO_RECORDING_TIMEOUT",
            // 映射视频候选写入失败。
            Self::VideoWriteFailed => "VIDEO_WRITE_FAILED",
            // 映射 worker 协议违规。
            Self::WorkerProtocolError => "WORKER_PROTOCOL_ERROR",
        }
    }

    // 从不可信 worker 文本收窄为允许公开的封闭错误类别。
    pub(super) fn from_worker(value: &str) -> Option<Self> {
        // 只接受现有二十七项固定 worker 白名单。
        match value {
            // 接受目标过期。
            "STALE_SESSION" => Some(Self::StaleSession),
            // 接受目标歧义。
            "AMBIGUOUS_TARGET" => Some(Self::AmbiguousTarget),
            // 接受确认缺失。
            "CONFIRMATION_REQUIRED" => Some(Self::ConfirmationRequired),
            // 接受参数错误。
            "INVALID_ARGUMENT" => Some(Self::InvalidArgument),
            // 接受输出路径错误。
            "INVALID_OUTPUT_PATH" => Some(Self::InvalidOutputPath),
            // 接受目标资格拒绝。
            "CAPTURE_TARGET_INELIGIBLE" => Some(Self::CaptureTargetIneligible),
            // 接受捕获 runtime 不可用。
            "CAPTURE_UNAVAILABLE" => Some(Self::CaptureUnavailable),
            // 接受捕获目标失败。
            "CAPTURE_TARGET_FAILED" => Some(Self::CaptureTargetFailed),
            // 接受捕获设备失败。
            "CAPTURE_DEVICE_FAILED" => Some(Self::CaptureDeviceFailed),
            // 接受捕获启动失败。
            "CAPTURE_START_FAILED" => Some(Self::CaptureStartFailed),
            // 接受捕获 worker 失败。
            "CAPTURE_WORKER_FAILED" => Some(Self::CaptureWorkerFailed),
            // 接受捕获 readback 失败。
            "CAPTURE_READBACK_FAILED" => Some(Self::CaptureReadbackFailed),
            // 接受捕获清理失败。
            "CAPTURE_CLEANUP_FAILED" => Some(Self::CaptureCleanupFailed),
            // 接受目标隐藏竞态。
            "CAPTURE_TARGET_HIDDEN" => Some(Self::CaptureTargetHidden),
            // 接受目标最小化竞态。
            "CAPTURE_TARGET_MINIMIZED" => Some(Self::CaptureTargetMinimized),
            // 接受捕获尺寸变化。
            "CAPTURE_SIZE_CHANGED" => Some(Self::CaptureSizeChanged),
            // 接受捕获内部超时。
            "CAPTURE_TIMEOUT" => Some(Self::CaptureTimeout),
            // 接受编码器不可用。
            "VIDEO_ENCODER_UNAVAILABLE" => Some(Self::VideoEncoderUnavailable),
            // 接受编码器启动失败。
            "VIDEO_ENCODER_START_FAILED" => Some(Self::VideoEncoderStartFailed),
            // 接受编码器写入失败。
            "VIDEO_ENCODER_WRITE_FAILED" => Some(Self::VideoEncoderWriteFailed),
            // 接受编码器运行失败。
            "VIDEO_ENCODER_FAILED" => Some(Self::VideoEncoderFailed),
            // 接受视频候选缺失。
            "VIDEO_OUTPUT_MISSING" => Some(Self::VideoOutputMissing),
            // 接受视频候选写入失败。
            "VIDEO_WRITE_FAILED" => Some(Self::VideoWriteFailed),
            // 接受覆盖确认缺失。
            "OVERWRITE_CONFIRMATION_REQUIRED" => Some(Self::OverwriteConfirmationRequired),
            // 接受分析候选写入失败。
            "VIDEO_ANALYSIS_WRITE_FAILED" => Some(Self::VideoAnalysisWriteFailed),
            // 接受分析失败。
            "VIDEO_ANALYSIS_FAILED" => Some(Self::VideoAnalysisFailed),
            // 接受前景变化。
            "FOREGROUND_CHANGED" => Some(Self::ForegroundChanged),
            // 未知、外层或 Module 私有码一律拒绝。
            _ => None,
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Module 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误映射、worker 白名单与构造测试。
#[cfg(test)]
mod tests {
    // 导入被测 Window Record 私有错误类型。
    use super::WindowRecordErrorCode;

    // 验证三十二项 Module 错误码的完整稳定映射。
    #[test]
    fn all_window_record_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持目标歧义码。
            (WindowRecordErrorCode::AmbiguousTarget, "AMBIGUOUS_TARGET"),
            // 保持捕获清理失败码。
            (
                WindowRecordErrorCode::CaptureCleanupFailed,
                "CAPTURE_CLEANUP_FAILED",
            ),
            // 保持捕获设备失败码。
            (
                WindowRecordErrorCode::CaptureDeviceFailed,
                "CAPTURE_DEVICE_FAILED",
            ),
            // 保持捕获 readback 失败码。
            (
                WindowRecordErrorCode::CaptureReadbackFailed,
                "CAPTURE_READBACK_FAILED",
            ),
            // 保持捕获尺寸变化码。
            (
                WindowRecordErrorCode::CaptureSizeChanged,
                "CAPTURE_SIZE_CHANGED",
            ),
            // 保持捕获启动失败码。
            (
                WindowRecordErrorCode::CaptureStartFailed,
                "CAPTURE_START_FAILED",
            ),
            // 保持捕获目标失败码。
            (
                WindowRecordErrorCode::CaptureTargetFailed,
                "CAPTURE_TARGET_FAILED",
            ),
            // 保持捕获目标隐藏码。
            (
                WindowRecordErrorCode::CaptureTargetHidden,
                "CAPTURE_TARGET_HIDDEN",
            ),
            // 保持捕获资格拒绝码。
            (
                WindowRecordErrorCode::CaptureTargetIneligible,
                "CAPTURE_TARGET_INELIGIBLE",
            ),
            // 保持捕获目标最小化码。
            (
                WindowRecordErrorCode::CaptureTargetMinimized,
                "CAPTURE_TARGET_MINIMIZED",
            ),
            // 保持捕获超时码。
            (WindowRecordErrorCode::CaptureTimeout, "CAPTURE_TIMEOUT"),
            // 保持捕获不可用码。
            (
                WindowRecordErrorCode::CaptureUnavailable,
                "CAPTURE_UNAVAILABLE",
            ),
            // 保持捕获 worker 失败码。
            (
                WindowRecordErrorCode::CaptureWorkerFailed,
                "CAPTURE_WORKER_FAILED",
            ),
            // 保持确认缺失码。
            (
                WindowRecordErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持前景变化码。
            (
                WindowRecordErrorCode::ForegroundChanged,
                "FOREGROUND_CHANGED",
            ),
            // 保持参数拒绝码。
            (WindowRecordErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持输出路径错误码。
            (
                WindowRecordErrorCode::InvalidOutputPath,
                "INVALID_OUTPUT_PATH",
            ),
            // 保持覆盖确认缺失码。
            (
                WindowRecordErrorCode::OverwriteConfirmationRequired,
                "OVERWRITE_CONFIRMATION_REQUIRED",
            ),
            // 保持目标过期码。
            (WindowRecordErrorCode::StaleSession, "STALE_SESSION"),
            // 保持 worker process 通用超时码。
            (WindowRecordErrorCode::WorkerProcessTimeout, "TIMEOUT"),
            // 保持分析失败码。
            (
                WindowRecordErrorCode::VideoAnalysisFailed,
                "VIDEO_ANALYSIS_FAILED",
            ),
            // 保持分析写入失败码。
            (
                WindowRecordErrorCode::VideoAnalysisWriteFailed,
                "VIDEO_ANALYSIS_WRITE_FAILED",
            ),
            // 保持多产物提交失败码。
            (
                WindowRecordErrorCode::VideoArtifactCommitFailed,
                "VIDEO_ARTIFACT_COMMIT_FAILED",
            ),
            // 保持多产物结果未知码。
            (
                WindowRecordErrorCode::VideoArtifactResultUnknown,
                "VIDEO_ARTIFACT_RESULT_UNKNOWN",
            ),
            // 保持编码器运行失败码。
            (
                WindowRecordErrorCode::VideoEncoderFailed,
                "VIDEO_ENCODER_FAILED",
            ),
            // 保持编码器启动失败码。
            (
                WindowRecordErrorCode::VideoEncoderStartFailed,
                "VIDEO_ENCODER_START_FAILED",
            ),
            // 保持编码器不可用码。
            (
                WindowRecordErrorCode::VideoEncoderUnavailable,
                "VIDEO_ENCODER_UNAVAILABLE",
            ),
            // 保持编码器写入失败码。
            (
                WindowRecordErrorCode::VideoEncoderWriteFailed,
                "VIDEO_ENCODER_WRITE_FAILED",
            ),
            // 保持视频候选缺失码。
            (
                WindowRecordErrorCode::VideoOutputMissing,
                "VIDEO_OUTPUT_MISSING",
            ),
            // 保持外层录制超时码。
            (
                WindowRecordErrorCode::VideoRecordingTimeout,
                "VIDEO_RECORDING_TIMEOUT",
            ),
            // 保持视频候选写入失败码。
            (
                WindowRecordErrorCode::VideoWriteFailed,
                "VIDEO_WRITE_FAILED",
            ),
            // 保持 worker 协议错误码。
            (
                WindowRecordErrorCode::WorkerProtocolError,
                "WORKER_PROTOCOL_ERROR",
            ),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 32);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证 worker 只接受原有二十七项白名单输入。
    #[test]
    fn worker_error_whitelist_is_closed_and_stable() {
        // 固定完整 worker 文本与类型对照表。
        let mappings = [
            // 接受目标过期。
            ("STALE_SESSION", WindowRecordErrorCode::StaleSession),
            // 接受目标歧义。
            ("AMBIGUOUS_TARGET", WindowRecordErrorCode::AmbiguousTarget),
            // 接受确认缺失。
            (
                "CONFIRMATION_REQUIRED",
                WindowRecordErrorCode::ConfirmationRequired,
            ),
            // 接受参数错误。
            ("INVALID_ARGUMENT", WindowRecordErrorCode::InvalidArgument),
            // 接受输出路径错误。
            (
                "INVALID_OUTPUT_PATH",
                WindowRecordErrorCode::InvalidOutputPath,
            ),
            // 接受目标资格拒绝。
            (
                "CAPTURE_TARGET_INELIGIBLE",
                WindowRecordErrorCode::CaptureTargetIneligible,
            ),
            // 接受捕获 runtime 不可用。
            (
                "CAPTURE_UNAVAILABLE",
                WindowRecordErrorCode::CaptureUnavailable,
            ),
            // 接受捕获目标失败。
            (
                "CAPTURE_TARGET_FAILED",
                WindowRecordErrorCode::CaptureTargetFailed,
            ),
            // 接受捕获设备失败。
            (
                "CAPTURE_DEVICE_FAILED",
                WindowRecordErrorCode::CaptureDeviceFailed,
            ),
            // 接受捕获启动失败。
            (
                "CAPTURE_START_FAILED",
                WindowRecordErrorCode::CaptureStartFailed,
            ),
            // 接受捕获 worker 失败。
            (
                "CAPTURE_WORKER_FAILED",
                WindowRecordErrorCode::CaptureWorkerFailed,
            ),
            // 接受捕获 readback 失败。
            (
                "CAPTURE_READBACK_FAILED",
                WindowRecordErrorCode::CaptureReadbackFailed,
            ),
            // 接受捕获清理失败。
            (
                "CAPTURE_CLEANUP_FAILED",
                WindowRecordErrorCode::CaptureCleanupFailed,
            ),
            // 接受目标隐藏竞态。
            (
                "CAPTURE_TARGET_HIDDEN",
                WindowRecordErrorCode::CaptureTargetHidden,
            ),
            // 接受目标最小化竞态。
            (
                "CAPTURE_TARGET_MINIMIZED",
                WindowRecordErrorCode::CaptureTargetMinimized,
            ),
            // 接受捕获尺寸变化。
            (
                "CAPTURE_SIZE_CHANGED",
                WindowRecordErrorCode::CaptureSizeChanged,
            ),
            // 接受捕获超时。
            ("CAPTURE_TIMEOUT", WindowRecordErrorCode::CaptureTimeout),
            // 接受编码器不可用。
            (
                "VIDEO_ENCODER_UNAVAILABLE",
                WindowRecordErrorCode::VideoEncoderUnavailable,
            ),
            // 接受编码器启动失败。
            (
                "VIDEO_ENCODER_START_FAILED",
                WindowRecordErrorCode::VideoEncoderStartFailed,
            ),
            // 接受编码器写入失败。
            (
                "VIDEO_ENCODER_WRITE_FAILED",
                WindowRecordErrorCode::VideoEncoderWriteFailed,
            ),
            // 接受编码器运行失败。
            (
                "VIDEO_ENCODER_FAILED",
                WindowRecordErrorCode::VideoEncoderFailed,
            ),
            // 接受视频候选缺失。
            (
                "VIDEO_OUTPUT_MISSING",
                WindowRecordErrorCode::VideoOutputMissing,
            ),
            // 接受视频候选写入失败。
            (
                "VIDEO_WRITE_FAILED",
                WindowRecordErrorCode::VideoWriteFailed,
            ),
            // 接受覆盖确认缺失。
            (
                "OVERWRITE_CONFIRMATION_REQUIRED",
                WindowRecordErrorCode::OverwriteConfirmationRequired,
            ),
            // 接受分析写入失败。
            (
                "VIDEO_ANALYSIS_WRITE_FAILED",
                WindowRecordErrorCode::VideoAnalysisWriteFailed,
            ),
            // 接受分析失败。
            (
                "VIDEO_ANALYSIS_FAILED",
                WindowRecordErrorCode::VideoAnalysisFailed,
            ),
            // 接受前景变化。
            (
                "FOREGROUND_CHANGED",
                WindowRecordErrorCode::ForegroundChanged,
            ),
        ];
        // 核对 worker 白名单规模。
        assert_eq!(mappings.len(), 27);
        // 逐项核对 worker 收窄结果。
        for (text, expected) in mappings {
            // 每项稳定文本必须收窄为唯一类型。
            assert_eq!(WindowRecordErrorCode::from_worker(text), Some(expected));
        }
        // 通用 worker process timeout 不得伪装成 recording worker 错误。
        assert_eq!(WindowRecordErrorCode::from_worker("TIMEOUT"), None);
        // 未知 provider 文本必须失败闭合。
        assert_eq!(
            WindowRecordErrorCode::from_worker("UNKNOWN_WORKER_CODE"),
            None
        );
    }

    // 验证普通构造器保持统一公开 envelope。
    #[test]
    fn window_record_error_constructor_keeps_message_and_empty_details() {
        // 构造多产物提交失败错误。
        let error = WindowRecordErrorCode::VideoArtifactCommitFailed.error("commit fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "VIDEO_ARTIFACT_COMMIT_FAILED");
        // 构造器必须保持公开消息。
        assert_eq!(error.message, "commit fixture");
        // 普通构造器不得制造额外详情。
        assert!(error.details.is_null());
    }
}
