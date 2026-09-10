//! 统一 CLI 适配边界私有错误码。

// 导入 JSON 安全详情类型。
use serde_json::Value;

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 CLI 外层允许直接产生或映射的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 该类型只在 CLI 参数与输入来源适配边界内部传播。
pub(super) enum CliErrorCode {
    // 表示命令、位置参数、选项或 JSON 外壳不符合契约。
    InvalidArgument,
    // 表示请求的兼容 surface 没有发布对应能力。
    CapabilityGap,
    // 表示确认式 CLI 操作缺少逐操作确认。
    ConfirmationRequired,
    // 表示有界 JSON 来源无法被读取。
    InputReadFailed,
}

// 提供 CLI 私有错误类别与公开协议文本的唯一映射。
impl CliErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射命令与参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射兼容 surface 能力缺口。
            Self::CapabilityGap => "CAPABILITY_GAP",
            // 映射缺少逐操作确认。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射 JSON 来源读取失败。
            Self::InputReadFailed => "INPUT_READ_FAILED",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 CLI 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 使用当前封闭错误码构造带安全详情的公开错误。
    pub(super) fn with_details(
        // 接收封闭错误码。
        self,
        // 接收公开安全消息。
        message: impl Into<String>,
        // 接收已经过 CLI 边界筛选的安全详情。
        details: Value,
    ) -> AppControlError {
        // 隐藏 CLI 私有类型并复用统一带详情 envelope。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 声明封闭错误类型的纯映射与构造测试。
#[cfg(test)]
mod tests {
    // 导入 JSON 详情构造宏。
    use serde_json::json;

    // 导入被测 CLI 私有错误类型。
    use super::CliErrorCode;

    // 验证四种 CLI 自有错误码的完整稳定映射。
    #[test]
    fn all_cli_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持参数拒绝码。
            (CliErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持能力缺口码。
            (CliErrorCode::CapabilityGap, "CAPABILITY_GAP"),
            // 保持确认拒绝码。
            (CliErrorCode::ConfirmationRequired, "CONFIRMATION_REQUIRED"),
            // 保持输入读取失败码。
            (CliErrorCode::InputReadFailed, "INPUT_READ_FAILED"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 4);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通构造器保持 CLI 公开错误输入。
    #[test]
    fn cli_error_constructor_keeps_message_and_empty_details() {
        // 构造能力缺口夹具。
        let error = CliErrorCode::CapabilityGap.error("cli fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "CAPABILITY_GAP");
        // 构造器必须保持公开安全消息。
        assert_eq!(error.message, "cli fixture");
        // 普通 CLI 错误不得制造额外详情。
        assert!(error.details.is_null());
    }

    // 验证带详情构造器保持有界输入证据。
    #[test]
    fn cli_details_constructor_keeps_safe_details() {
        // 构造有界字节观察证据。
        let details = json!({ "maximumBytes": 16, "observedBytes": 17 });
        // 构造带详情的参数错误。
        let error = CliErrorCode::InvalidArgument.with_details("cli fixture", details.clone());
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "INVALID_ARGUMENT");
        // 构造器必须保持公开安全消息。
        assert_eq!(error.message, "cli fixture");
        // 构造器必须逐值保持安全详情。
        assert_eq!(error.details, details);
    }
}
