//! 统一 RecordingConfig 普通领域边界私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 RecordingConfig 允许直接产生或映射的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 该类型只在录制配置验证边界内部传播。
pub(super) enum RecordingConfigErrorCode {
    // 表示参数、字段、范围或输出路径不符合封闭契约。
    InvalidArgument,
    // 表示既有输出缺少独立覆盖许可。
    OverwriteConfirmationRequired,
    // 表示输出目标状态无法被可靠检查。
    OperationFailed,
}

// 提供录制配置私有错误类别与公开协议文本的唯一映射。
impl RecordingConfigErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射参数与路径拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射缺少覆盖许可。
            Self::OverwriteConfirmationRequired => "OVERWRITE_CONFIRMATION_REQUIRED",
            // 映射输出状态检查失败。
            Self::OperationFailed => "OPERATION_FAILED",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏录制配置私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型的纯映射与构造测试。
#[cfg(test)]
mod tests {
    // 导入被测 RecordingConfig 私有错误类型。
    use super::RecordingConfigErrorCode;

    // 验证三种配置自有错误码的完整稳定映射。
    #[test]
    fn all_recording_config_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持参数拒绝码。
            (
                RecordingConfigErrorCode::InvalidArgument,
                "INVALID_ARGUMENT",
            ),
            // 保持覆盖确认拒绝码。
            (
                RecordingConfigErrorCode::OverwriteConfirmationRequired,
                "OVERWRITE_CONFIRMATION_REQUIRED",
            ),
            // 保持输出状态检查失败码。
            (
                RecordingConfigErrorCode::OperationFailed,
                "OPERATION_FAILED",
            ),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 3);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通构造器保持统一配置失败 envelope 输入。
    #[test]
    fn recording_config_error_constructor_keeps_message_and_empty_details() {
        // 构造覆盖许可缺失夹具。
        let error = RecordingConfigErrorCode::OverwriteConfirmationRequired.error("config fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "OVERWRITE_CONFIRMATION_REQUIRED");
        // 构造器必须保持公开安全消息。
        assert_eq!(error.message, "config fixture");
        // 配置自有错误不得制造额外详情。
        assert!(error.details.is_null());
    }
}
