//! 统一只读 Process Adapter 私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Process Adapter 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AppProcessErrorCode {
    // 表示 process surface 不提供请求的后台写操作。
    BackgroundOperationUnavailable,
    // 表示 opaque 进程目标匹配不唯一。
    AmbiguousTarget,
    // 表示只读观察期间宿主前景发生变化。
    HostInterferenceDetected,
    // 表示调用方目标字段违反 process surface 契约。
    InvalidArgument,
    // 表示 opaque 进程目标已过期或不存在。
    StaleSession,
}

// 提供 Process Adapter 私有错误码与公开协议文本的唯一映射。
impl AppProcessErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射后台写能力缺口。
            Self::BackgroundOperationUnavailable => "BACKGROUND_OPERATION_UNAVAILABLE",
            // 映射 opaque 目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射宿主前景干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射调用参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射过期进程目标。
            Self::StaleSession => "STALE_SESSION",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Process Adapter 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型的纯映射测试。
#[cfg(test)]
mod tests {
    // 导入被测 Process Adapter 私有错误类型。
    use super::AppProcessErrorCode;

    // 验证五种 Process Adapter 错误码的完整稳定映射。
    #[test]
    fn all_process_adapter_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持后台操作缺口码。
            (
                AppProcessErrorCode::BackgroundOperationUnavailable,
                "BACKGROUND_OPERATION_UNAVAILABLE",
            ),
            // 保持目标歧义码。
            (AppProcessErrorCode::AmbiguousTarget, "AMBIGUOUS_TARGET"),
            // 保持宿主干扰码。
            (
                AppProcessErrorCode::HostInterferenceDetected,
                "HOST_INTERFERENCE_DETECTED",
            ),
            // 保持参数拒绝码。
            (AppProcessErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持目标过期码。
            (AppProcessErrorCode::StaleSession, "STALE_SESSION"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 5);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }
}
