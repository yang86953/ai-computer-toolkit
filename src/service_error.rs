//! 统一 ComputerControlSystem 私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 ComputerControlSystem 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ComputerControlSystemErrorCode {
    // 表示调用方请求未知 app、operation 或 method。
    InvalidArgument,
    // 表示冻结执行计划等 System 不变量失效。
    OperationFailed,
    // 表示公开 descriptor 无法投影为稳定 JSON。
    SerializationFailed,
}

// 提供 System 私有错误类别与公开协议文本的唯一映射。
impl ComputerControlSystemErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射公共查询参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射冻结执行计划完整性失败。
            Self::OperationFailed => "OPERATION_FAILED",
            // 映射 descriptor 序列化失败。
            Self::SerializationFailed => "SERIALIZATION_FAILED",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 System 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型的纯映射与构造测试。
#[cfg(test)]
mod tests {
    // 导入被测 ComputerControlSystem 私有错误类型。
    use super::ComputerControlSystemErrorCode;

    // 验证三种 System 错误码的完整稳定映射。
    #[test]
    fn all_computer_control_system_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持公共参数拒绝码。
            (
                ComputerControlSystemErrorCode::InvalidArgument,
                "INVALID_ARGUMENT",
            ),
            // 保持冻结执行计划完整性失败码。
            (
                ComputerControlSystemErrorCode::OperationFailed,
                "OPERATION_FAILED",
            ),
            // 保持 descriptor 序列化失败码。
            (
                ComputerControlSystemErrorCode::SerializationFailed,
                "SERIALIZATION_FAILED",
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

    // 验证普通构造器保持统一公开 envelope。
    #[test]
    fn computer_control_system_error_constructor_keeps_message_and_empty_details() {
        // 构造冻结执行计划完整性失败夹具。
        let error = ComputerControlSystemErrorCode::OperationFailed.error("system fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "OPERATION_FAILED");
        // 构造器必须保持公开消息。
        assert_eq!(error.message, "system fixture");
        // 普通构造器不得制造额外详情。
        assert!(error.details.is_null());
    }
}
