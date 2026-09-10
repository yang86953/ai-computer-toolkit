//! 统一 Observation Worker 协议边界私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Observation Worker 协议入口允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 该类型只在 Observation Worker 协议实现内部传播。
pub(super) enum ObservationWorkerErrorCode {
    // 表示调用方请求、协议字段或标准输入不符合封闭契约。
    InvalidArgument,
    // 表示 operation 不在认证只读集合内。
    CapabilityGap,
    // 表示 canonical 窗口目标或 UIA element 已经过期。
    StaleSession,
    // 表示 canonical 窗口目标重新解析为多个候选。
    AmbiguousTarget,
    // 表示 UIA provider 明确拒绝当前权限。
    PermissionDenied,
    // 表示 COM 或 UIA provider 当前不可用。
    AccessibilityUnavailable,
}

// 提供 Worker 私有错误类别与公开协议文本的唯一映射。
impl ObservationWorkerErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射请求或协议参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射未认证 operation。
            Self::CapabilityGap => "CAPABILITY_GAP",
            // 映射目标或 element 过期。
            Self::StaleSession => "STALE_SESSION",
            // 映射多个目标候选。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射 provider 权限拒绝。
            Self::PermissionDenied => "PERMISSION_DENIED",
            // 映射 COM 或 UIA provider 不可用。
            Self::AccessibilityUnavailable => "ACCESSIBILITY_UNAVAILABLE",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Worker 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型的纯映射与构造测试。
#[cfg(test)]
mod tests {
    // 导入被测 Observation Worker 私有错误类型。
    use super::ObservationWorkerErrorCode;

    // 验证六种 Worker 自有错误码的完整稳定映射。
    #[test]
    fn all_observation_worker_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持参数拒绝码。
            (
                ObservationWorkerErrorCode::InvalidArgument,
                "INVALID_ARGUMENT",
            ),
            // 保持认证能力缺口码。
            (ObservationWorkerErrorCode::CapabilityGap, "CAPABILITY_GAP"),
            // 保持目标过期码。
            (ObservationWorkerErrorCode::StaleSession, "STALE_SESSION"),
            // 保持目标歧义码。
            (
                ObservationWorkerErrorCode::AmbiguousTarget,
                "AMBIGUOUS_TARGET",
            ),
            // 保持权限拒绝码。
            (
                ObservationWorkerErrorCode::PermissionDenied,
                "PERMISSION_DENIED",
            ),
            // 保持可访问性不可用码。
            (
                ObservationWorkerErrorCode::AccessibilityUnavailable,
                "ACCESSIBILITY_UNAVAILABLE",
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

    // 验证普通构造器保持统一 worker 失败 envelope 输入。
    #[test]
    fn observation_worker_error_constructor_keeps_message_and_empty_details() {
        // 构造 provider 不可用失败夹具。
        let error = ObservationWorkerErrorCode::AccessibilityUnavailable.error("worker fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "ACCESSIBILITY_UNAVAILABLE");
        // 构造器必须保持公开安全消息。
        assert_eq!(error.message, "worker fixture");
        // Worker 自有错误不得制造额外详情。
        assert!(error.details.is_null());
    }
}
