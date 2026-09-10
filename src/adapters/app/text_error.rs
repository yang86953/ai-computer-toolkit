//! 统一 App text document provider Adapter 私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 App text document provider Adapter 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AppTextErrorCode {
    // 表示当前文本创建器未发布请求的 capability。
    CapabilityUnsupported,
    // 表示调用方文本输入违反 provider 契约。
    InvalidArgument,
    // 表示下层 Adapter 成功结果违反公开文档投影契约。
    OperationFailed,
    // 表示当前 canonical 文本创建器目标已不可用。
    StaleSession,
}

// 提供 text document provider 私有错误码与公开协议文本的唯一映射。
impl AppTextErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射 capability 缺口。
            Self::CapabilityUnsupported => "CAPABILITY_UNSUPPORTED",
            // 映射 provider 输入拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射下层成功结果不完整。
            Self::OperationFailed => "OPERATION_FAILED",
            // 映射文本创建器目标过期。
            Self::StaleSession => "STALE_SESSION",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 text document provider 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型的纯映射测试。
#[cfg(test)]
mod tests {
    // 导入被测 text document provider 私有错误类型。
    use super::AppTextErrorCode;

    // 验证四种 text document provider 错误码的完整稳定映射。
    #[test]
    fn all_text_provider_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持 capability 缺口码。
            (
                AppTextErrorCode::CapabilityUnsupported,
                "CAPABILITY_UNSUPPORTED",
            ),
            // 保持参数拒绝码。
            (AppTextErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持执行失败码。
            (AppTextErrorCode::OperationFailed, "OPERATION_FAILED"),
            // 保持文本创建器目标过期码。
            (AppTextErrorCode::StaleSession, "STALE_SESSION"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 4);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }
}
