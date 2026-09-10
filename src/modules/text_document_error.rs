//! 统一 Text Document Module 私有错误码。

// 导入 JSON 详情值。
use serde_json::Value;

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Text Document Module 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 该类型只在 Text Document Module 内部传播。
pub(super) enum TextDocumentErrorCode {
    // 表示安全前置条件无法证明后台隔离启动。
    BackgroundOperationUnavailable,
    // 表示调用方没有逐操作确认。
    ConfirmationRequired,
    // 表示工具自有 UTF-8 artifact 无法可靠创建。
    DocumentCreateFailed,
    // 表示输入不满足封闭文本契约。
    InvalidArgument,
    // 表示固定系统 Notepad runtime 不可用。
    NotepadUnavailable,
    // 表示新 artifact 无法通过逐字节回读。
    TextVerificationFailed,
}

// 提供 Module 私有错误类别与公开协议文本的唯一映射。
impl TextDocumentErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射后台隔离启动不可证明。
            Self::BackgroundOperationUnavailable => "BACKGROUND_OPERATION_UNAVAILABLE",
            // 映射确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射文档创建失败。
            Self::DocumentCreateFailed => "DOCUMENT_CREATE_FAILED",
            // 映射参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射固定 runtime 缺失。
            Self::NotepadUnavailable => "NOTEPAD_UNAVAILABLE",
            // 映射文本回读验证失败。
            Self::TextVerificationFailed => "TEXT_VERIFICATION_FAILED",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Module 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 使用当前封闭错误码构造带安全详情的公开错误。
    pub(super) fn with_details(
        // 接收稳定公开消息。
        self,
        // 接收公开消息正文。
        message: impl Into<String>,
        // 接收已经过 Module 筛选的安全详情。
        details: Value,
    ) -> AppControlError {
        // 隐藏 Module 私有类型并复用统一 details envelope。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 声明封闭错误类型的纯映射与构造测试。
#[cfg(test)]
mod tests {
    // 导入 JSON 夹具宏。
    use serde_json::json;

    // 导入被测 Text Document 私有错误类型。
    use super::TextDocumentErrorCode;

    // 验证六种 Module 错误码的完整稳定映射。
    #[test]
    fn all_text_document_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持后台操作不可用码。
            (
                TextDocumentErrorCode::BackgroundOperationUnavailable,
                "BACKGROUND_OPERATION_UNAVAILABLE",
            ),
            // 保持确认缺失码。
            (
                TextDocumentErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持文档创建失败码。
            (
                TextDocumentErrorCode::DocumentCreateFailed,
                "DOCUMENT_CREATE_FAILED",
            ),
            // 保持参数拒绝码。
            (TextDocumentErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持固定 runtime 缺失码。
            (
                TextDocumentErrorCode::NotepadUnavailable,
                "NOTEPAD_UNAVAILABLE",
            ),
            // 保持文本验证失败码。
            (
                TextDocumentErrorCode::TextVerificationFailed,
                "TEXT_VERIFICATION_FAILED",
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

    // 验证普通构造器保持公开消息与空详情。
    #[test]
    fn text_document_error_constructor_keeps_message_and_empty_details() {
        // 构造确认缺失错误。
        let error = TextDocumentErrorCode::ConfirmationRequired.error("confirmation fixture");
        // 普通构造器必须选择封闭映射文本。
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
        // 普通构造器必须保持公开消息。
        assert_eq!(error.message, "confirmation fixture");
        // 普通构造器不得制造额外详情。
        assert!(error.details.is_null());
    }

    // 验证带详情构造器逐值保持 Module 筛选后的安全详情。
    #[test]
    fn text_document_details_constructor_keeps_safe_details() {
        // 构造无 PID、无路径的固定安全详情。
        let details = json!({
            "reason": "process-snapshot-incomplete",
            "artifactCreated": false,
            "safeToRetryAutomatically": false,
        });
        // 构造后台隔离不可证明错误。
        let error = TextDocumentErrorCode::BackgroundOperationUnavailable.with_details(
            // 使用稳定夹具消息。
            "snapshot fixture",
            // 复制详情以便随后逐值核对。
            details.clone(),
        );
        // 带详情构造器必须选择封闭映射文本。
        assert_eq!(error.code, "BACKGROUND_OPERATION_UNAVAILABLE");
        // 带详情构造器必须保持公开消息。
        assert_eq!(error.message, "snapshot fixture");
        // 带详情构造器必须逐值保持安全详情。
        assert_eq!(error.details, details);
    }
}
