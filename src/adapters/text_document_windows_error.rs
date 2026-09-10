//! 统一 Text Document Windows 启动 Component 私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示固定 Notepad 启动边界允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TextDocumentLaunchErrorCode {
    // 表示启动影响改变了宿主前景且已触发回滚。
    HostInterferenceDetected,
    // 表示固定 Notepad 启动或回滚边界无法建立。
    NotepadStartFailed,
}

// 提供 Text Document Windows 启动错误与公开协议文本的唯一映射。
impl TextDocumentLaunchErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射宿主前景干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射固定 Notepad 启动失败。
            Self::NotepadStartFailed => "NOTEPAD_START_FAILED",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Component 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型的纯映射测试。
#[cfg(test)]
mod tests {
    // 导入被测 Text Document Windows 启动错误类型。
    use super::TextDocumentLaunchErrorCode;

    // 验证两种启动边界错误码的完整稳定映射。
    #[test]
    fn all_text_document_launch_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持宿主前景干扰码。
            (
                TextDocumentLaunchErrorCode::HostInterferenceDetected,
                "HOST_INTERFERENCE_DETECTED",
            ),
            // 保持固定 Notepad 启动失败码。
            (
                TextDocumentLaunchErrorCode::NotepadStartFailed,
                "NOTEPAD_START_FAILED",
            ),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 2);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通构造器保持统一公开 envelope。
    #[test]
    fn text_document_launch_error_constructor_keeps_message_and_empty_details() {
        // 构造固定启动失败夹具。
        let error = TextDocumentLaunchErrorCode::NotepadStartFailed.error("launch fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "NOTEPAD_START_FAILED");
        // 构造器必须保持调用方消息。
        assert_eq!(error.message, "launch fixture");
        // 普通错误不得制造 provider 详情。
        assert!(error.details.is_null());
    }
}
