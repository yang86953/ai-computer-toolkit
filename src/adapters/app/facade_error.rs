//! 统一 App facade Adapter 私有错误码及分类。

// 导入 provider-neutral JSON 证据。
use serde_json::Value;

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 App facade Adapter 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AppFacadeErrorCode {
    // 表示多个 provider 同时解析同一 opaque session。
    AmbiguousTarget,
    // 表示精确 session 未发布请求的 capability。
    CapabilityUnsupported,
    // 表示非前台执行域观察到主机前景变化。
    HostInterferenceDetected,
    // 表示 provider 成功结果违反 facade 一致性契约。
    OperationFailed,
    // 表示当前 provider inventory 无法解析 session。
    TargetNotFound,
}

// 提供 facade 私有错误码与公开协议文本的唯一映射。
impl AppFacadeErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射跨 provider 目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射 session capability 缺口。
            Self::CapabilityUnsupported => "CAPABILITY_UNSUPPORTED",
            // 映射主机前景干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射 provider 结果不一致。
            Self::OperationFailed => "OPERATION_FAILED",
            // 映射 session 未找到。
            Self::TargetNotFound => "TARGET_NOT_FOUND",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 facade 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 使用当前封闭错误码构造带公开证据的错误。
    pub(super) fn with_details(
        // 接收稳定公开消息。
        self,
        // 接收可转换为拥有型字符串的消息。
        message: impl Into<String>,
        // 接收 provider-neutral 公开证据。
        details: Value,
    ) -> AppControlError {
        // 隐藏 facade 私有类型并保持既有 details 形状。
        AppControlError::with_details(self.as_str(), message, details)
    }

    // 判断公开错误是否属于当前 facade 分类。
    pub(super) fn matches(self, error: &AppControlError) -> bool {
        // 只比较稳定公开错误码，不取得 provider 私有事实。
        error.code == self.as_str()
    }
}

// 声明封闭错误类型的纯映射与分类测试。
#[cfg(test)]
mod tests {
    // 导入被测 facade 私有错误类型。
    use super::AppFacadeErrorCode;

    // 验证五种 facade 错误码的完整稳定映射。
    #[test]
    fn all_facade_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持目标歧义码。
            (AppFacadeErrorCode::AmbiguousTarget, "AMBIGUOUS_TARGET"),
            // 保持 capability 缺口码。
            (
                AppFacadeErrorCode::CapabilityUnsupported,
                "CAPABILITY_UNSUPPORTED",
            ),
            // 保持主机干扰码。
            (
                AppFacadeErrorCode::HostInterferenceDetected,
                "HOST_INTERFERENCE_DETECTED",
            ),
            // 保持执行失败码。
            (AppFacadeErrorCode::OperationFailed, "OPERATION_FAILED"),
            // 保持目标未找到码。
            (AppFacadeErrorCode::TargetNotFound, "TARGET_NOT_FOUND"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 5);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证类型化分类只接受相同公开错误码。
    #[test]
    fn error_classification_matches_exact_public_code() {
        // 构造 facade 自有目标未找到错误。
        let target_not_found =
            AppFacadeErrorCode::TargetNotFound.error("The app session is unavailable.");
        // 相同类别必须命中。
        assert!(AppFacadeErrorCode::TargetNotFound.matches(&target_not_found));
        // 不同 facade 类别不得误命中。
        assert!(!AppFacadeErrorCode::OperationFailed.matches(&target_not_found));
    }
}
