//! 统一只读 Window Adapter 私有错误码。

// 导入 JSON 详情值。
use serde_json::Value;

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示只读 Window Adapter 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowAdapterErrorCode {
    // 表示 opaque 窗口身份匹配多个实时窗口。
    AmbiguousTarget,
    // 表示只读窗口 surface 不允许后台写操作。
    BackgroundOperationUnavailable,
    // 表示只读观察期间宿主前景发生变化。
    HostInterferenceDetected,
    // 表示调用方提供了非法或缺失的目标字段。
    InvalidArgument,
    // 表示 opaque 窗口身份在使用时不再唯一存在。
    StaleSession,
}

// 提供 Window Adapter 私有错误码与公开协议文本的唯一映射。
impl WindowAdapterErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射窗口目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射只读 surface 的后台写能力缺口。
            Self::BackgroundOperationUnavailable => "BACKGROUND_OPERATION_UNAVAILABLE",
            // 映射宿主前景干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射目标参数错误。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射使用时窗口目标过期。
            Self::StaleSession => "STALE_SESSION",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Adapter 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 使用当前封闭错误码构造带安全详情的公开错误。
    pub(super) fn with_details(
        // 接收封闭错误码。
        self,
        // 接收公开安全消息。
        message: impl Into<String>,
        // 接收已经过调用方边界筛选的安全详情。
        details: Value,
    ) -> AppControlError {
        // 隐藏 Adapter 私有类型并复用统一带详情 envelope。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 声明封闭错误类型的纯映射与构造测试。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 导入被测 Window Adapter 私有错误类型。
    use super::WindowAdapterErrorCode;

    // 验证五种 Window Adapter 错误码的完整稳定映射。
    #[test]
    fn all_window_adapter_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持窗口目标歧义码。
            (WindowAdapterErrorCode::AmbiguousTarget, "AMBIGUOUS_TARGET"),
            // 保持后台写能力缺口码。
            (
                WindowAdapterErrorCode::BackgroundOperationUnavailable,
                "BACKGROUND_OPERATION_UNAVAILABLE",
            ),
            // 保持宿主前景干扰码。
            (
                WindowAdapterErrorCode::HostInterferenceDetected,
                "HOST_INTERFERENCE_DETECTED",
            ),
            // 保持目标参数错误码。
            (WindowAdapterErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持使用时窗口目标过期码。
            (WindowAdapterErrorCode::StaleSession, "STALE_SESSION"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 5);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通构造器保持统一公开 envelope。
    #[test]
    fn window_adapter_error_constructor_keeps_message_and_empty_details() {
        // 构造窗口目标过期夹具。
        let error = WindowAdapterErrorCode::StaleSession.error("stale fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "STALE_SESSION");
        // 构造器必须保持调用方消息。
        assert_eq!(error.message, "stale fixture");
        // 普通错误不得制造 provider 详情。
        assert!(error.details.is_null());
    }

    // 验证带详情构造器只保留调用方提供的安全详情。
    #[test]
    fn window_adapter_details_constructor_keeps_safe_details() {
        // 构造只含安全字段名的参数错误夹具。
        let error = WindowAdapterErrorCode::InvalidArgument.with_details(
            // 提供稳定公开消息。
            "invalid fixture",
            // 只提供安全字段名，不提供原生值。
            json!({ "field": "hwnd" }),
        );
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "INVALID_ARGUMENT");
        // 构造器必须保持公开消息。
        assert_eq!(error.message, "invalid fixture");
        // 构造器必须逐字保持安全详情。
        assert_eq!(error.details, json!({ "field": "hwnd" }));
    }
}
