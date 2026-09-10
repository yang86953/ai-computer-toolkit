//! 统一 Accessibility Wait Module 私有错误码。

// 导入 JSON 详情值。
use serde_json::Value;

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Accessibility Wait Module 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AccessibilityWaitErrorCode {
    // 表示语义 selector 同时匹配多个元素。
    AmbiguousTarget,
    // 表示调用方取消有界等待。
    Cancelled,
    // 表示输入、selector 或采样边界不合法。
    InvalidArgument,
    // 表示内部采样缺少必须存在的成功证据。
    OperationFailed,
    // 表示有界树无法证明 selector 唯一性或属性完整性。
    SearchIncomplete,
    // 表示稳定条件未在总 deadline 内成立。
    Timeout,
}

// 提供 Module 私有错误类别与公开协议文本的唯一映射。
impl AccessibilityWaitErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射取消。
            Self::Cancelled => "CANCELLED",
            // 映射公共参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射内部成功证据缺失。
            Self::OperationFailed => "OPERATION_FAILED",
            // 映射搜索不完整。
            Self::SearchIncomplete => "SEARCH_INCOMPLETE",
            // 映射总等待超时。
            Self::Timeout => "TIMEOUT",
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

    // 导入被测 Accessibility Wait 私有错误类型。
    use super::AccessibilityWaitErrorCode;

    // 验证六种 Module 错误码的完整稳定映射。
    #[test]
    fn all_accessibility_wait_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持目标歧义码。
            (
                AccessibilityWaitErrorCode::AmbiguousTarget,
                "AMBIGUOUS_TARGET",
            ),
            // 保持取消码。
            (AccessibilityWaitErrorCode::Cancelled, "CANCELLED"),
            // 保持参数拒绝码。
            (
                AccessibilityWaitErrorCode::InvalidArgument,
                "INVALID_ARGUMENT",
            ),
            // 保持操作失败码。
            (
                AccessibilityWaitErrorCode::OperationFailed,
                "OPERATION_FAILED",
            ),
            // 保持搜索不完整码。
            (
                AccessibilityWaitErrorCode::SearchIncomplete,
                "SEARCH_INCOMPLETE",
            ),
            // 保持超时码。
            (AccessibilityWaitErrorCode::Timeout, "TIMEOUT"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 6);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通与带详情构造器保持统一公开 envelope。
    #[test]
    fn accessibility_wait_error_constructors_keep_message_and_details() {
        // 构造内部成功证据缺失错误。
        let plain = AccessibilityWaitErrorCode::OperationFailed.error("wait fixture");
        // 普通构造器必须选择封闭映射文本。
        assert_eq!(plain.code, "OPERATION_FAILED");
        // 普通构造器必须保持公开消息。
        assert_eq!(plain.message, "wait fixture");
        // 普通构造器不得制造额外详情。
        assert!(plain.details.is_null());

        // 构造只含安全采样计数的超时错误。
        let detailed = AccessibilityWaitErrorCode::Timeout.with_details(
            // 使用稳定夹具消息。
            "timeout fixture",
            // 只提供安全计数详情。
            json!({ "samples": 2 }),
        );
        // 带详情构造器必须选择封闭映射文本。
        assert_eq!(detailed.code, "TIMEOUT");
        // 带详情构造器必须保持公开消息。
        assert_eq!(detailed.message, "timeout fixture");
        // 带详情构造器必须逐值保持安全详情。
        assert_eq!(detailed.details, json!({ "samples": 2 }));
    }
}
