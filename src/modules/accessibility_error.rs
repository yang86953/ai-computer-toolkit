//! 统一 Accessibility Module 私有错误码与公开映射。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Accessibility Module 允许直接产生或公开转换的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AccessibilityErrorCode {
    // 表示精确目标匹配多个窗口。
    AmbiguousTarget,
    // 表示 accessibility provider 不具备后台能力。
    BackgroundOperationUnavailable,
    // 表示 worker 不具备请求的 capability。
    CapabilityGap,
    // 表示调用方边界或 worker 参数不合法。
    InvalidArgument,
    // 表示认证隔离 worker 无法启动。
    IsolatedWorkerUnavailable,
    // 表示公开边界内无法进一步细分的操作失败。
    OperationFailed,
    // 表示 provider 权限不足。
    PermissionDenied,
    // 表示 canonical 窗口目标已过期。
    StaleSession,
    // 表示 worker 无法建立窗口清单。
    WindowEnumerationFailed,
    // 表示 worker envelope、隐私字段或输出形状违反协议。
    WorkerProtocolFailed,
}

// 提供 Module 私有错误类别与公开协议文本的唯一映射。
impl AccessibilityErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射后台能力缺口。
            Self::BackgroundOperationUnavailable => "BACKGROUND_OPERATION_UNAVAILABLE",
            // 映射 worker capability 缺口。
            Self::CapabilityGap => "CAPABILITY_GAP",
            // 映射公共参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射隔离 worker 不可用。
            Self::IsolatedWorkerUnavailable => "ISOLATED_WORKER_UNAVAILABLE",
            // 映射公开通用操作失败。
            Self::OperationFailed => "OPERATION_FAILED",
            // 映射权限拒绝。
            Self::PermissionDenied => "PERMISSION_DENIED",
            // 映射目标过期。
            Self::StaleSession => "STALE_SESSION",
            // 映射窗口枚举失败。
            Self::WindowEnumerationFailed => "WINDOW_ENUMERATION_FAILED",
            // 映射 worker 协议失败。
            Self::WorkerProtocolFailed => "WORKER_PROTOCOL_FAILED",
        }
    }

    // 把允许穿过 observation worker 协议的错误码转换为封闭类型。
    pub(super) fn from_worker_code(code: &str) -> Option<Self> {
        // 穷举固定 worker 失败白名单并拒绝未知值。
        match code {
            // provider 不可用必须收敛为公共后台能力缺口。
            "ACCESSIBILITY_UNAVAILABLE" => Some(Self::BackgroundOperationUnavailable),
            // 保留权限分类。
            "PERMISSION_DENIED" => Some(Self::PermissionDenied),
            // 保留目标过期分类。
            "STALE_SESSION" => Some(Self::StaleSession),
            // 保留目标歧义分类。
            "AMBIGUOUS_TARGET" => Some(Self::AmbiguousTarget),
            // 保留严格参数分类。
            "INVALID_ARGUMENT" => Some(Self::InvalidArgument),
            // 保留 worker capability 缺口分类。
            "CAPABILITY_GAP" => Some(Self::CapabilityGap),
            // 保留内部窗口枚举分类供公开边界继续收敛。
            "WINDOW_ENUMERATION_FAILED" => Some(Self::WindowEnumerationFailed),
            // 未知 worker 错误不得穿透稳定公共边界。
            _ => None,
        }
    }

    // 把 Component 与 worker 私有失败码转换为 Accessibility 公开错误。
    pub(super) fn from_internal_code(code: &str) -> Option<Self> {
        // 只处理当前 Module 明确拥有的公开转换。
        match code {
            // companion 缺失或启动失败都表示隔离 worker 不可用。
            "COMPANION_WORKER_UNAVAILABLE" | "WORKER_START_FAILED" => {
                // 返回认证隔离 worker 不可用分类。
                Some(Self::IsolatedWorkerUnavailable)
            }
            // 管道、输出、等待与内部枚举故障统一为操作失败。
            "WORKER_PROTOCOL_FAILED"
            | "WORKER_OUTPUT_TOO_LARGE"
            | "WORKER_WAIT_FAILED"
            | "WINDOW_ENUMERATION_FAILED" => {
                // 返回公开通用操作失败分类。
                Some(Self::OperationFailed)
            }
            // 其他稳定错误由原所有者继续公开。
            _ => None,
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Module 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型、worker 白名单与公开转换测试。
#[cfg(test)]
mod tests {
    // 导入被测 Accessibility 私有错误类型。
    use super::AccessibilityErrorCode;

    // 验证十种 Module 错误码的完整稳定映射。
    #[test]
    fn all_accessibility_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持目标歧义码。
            (AccessibilityErrorCode::AmbiguousTarget, "AMBIGUOUS_TARGET"),
            // 保持后台能力缺口码。
            (
                AccessibilityErrorCode::BackgroundOperationUnavailable,
                "BACKGROUND_OPERATION_UNAVAILABLE",
            ),
            // 保持 worker capability 缺口码。
            (AccessibilityErrorCode::CapabilityGap, "CAPABILITY_GAP"),
            // 保持参数拒绝码。
            (AccessibilityErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持隔离 worker 不可用码。
            (
                AccessibilityErrorCode::IsolatedWorkerUnavailable,
                "ISOLATED_WORKER_UNAVAILABLE",
            ),
            // 保持通用操作失败码。
            (AccessibilityErrorCode::OperationFailed, "OPERATION_FAILED"),
            // 保持权限拒绝码。
            (
                AccessibilityErrorCode::PermissionDenied,
                "PERMISSION_DENIED",
            ),
            // 保持目标过期码。
            (AccessibilityErrorCode::StaleSession, "STALE_SESSION"),
            // 保持内部窗口枚举失败码。
            (
                AccessibilityErrorCode::WindowEnumerationFailed,
                "WINDOW_ENUMERATION_FAILED",
            ),
            // 保持 worker 协议失败码。
            (
                AccessibilityErrorCode::WorkerProtocolFailed,
                "WORKER_PROTOCOL_FAILED",
            ),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 10);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证 observation worker 的七种失败白名单输入。
    #[test]
    fn worker_error_whitelist_is_closed_and_stable() {
        // 固定 worker 输入与公开分类对照表。
        let mappings = [
            // provider 缺口转换为公共后台能力缺口。
            (
                "ACCESSIBILITY_UNAVAILABLE",
                AccessibilityErrorCode::BackgroundOperationUnavailable,
            ),
            // 保留权限分类。
            (
                "PERMISSION_DENIED",
                AccessibilityErrorCode::PermissionDenied,
            ),
            // 保留目标过期分类。
            ("STALE_SESSION", AccessibilityErrorCode::StaleSession),
            // 保留目标歧义分类。
            ("AMBIGUOUS_TARGET", AccessibilityErrorCode::AmbiguousTarget),
            // 保留参数分类。
            ("INVALID_ARGUMENT", AccessibilityErrorCode::InvalidArgument),
            // 保留 capability 缺口分类。
            ("CAPABILITY_GAP", AccessibilityErrorCode::CapabilityGap),
            // 保留内部窗口枚举分类。
            (
                "WINDOW_ENUMERATION_FAILED",
                AccessibilityErrorCode::WindowEnumerationFailed,
            ),
        ];
        // 核对 worker 白名单规模。
        assert_eq!(mappings.len(), 7);
        // 逐项核对 worker 输入映射。
        for (input, expected) in mappings {
            // 每个白名单输入必须映射到唯一封闭类型。
            assert_eq!(
                AccessibilityErrorCode::from_worker_code(input),
                Some(expected)
            );
        }
        // 未知 worker 错误必须失败闭合。
        assert_eq!(AccessibilityErrorCode::from_worker_code("UNKNOWN"), None);
    }

    // 验证六种内部错误输入的公开收敛。
    #[test]
    fn internal_error_mapping_is_closed_and_stable() {
        // 固定内部输入与公开分类对照表。
        let mappings = [
            // companion 缺失映射为隔离 worker 不可用。
            (
                "COMPANION_WORKER_UNAVAILABLE",
                AccessibilityErrorCode::IsolatedWorkerUnavailable,
            ),
            // worker 启动失败映射为隔离 worker 不可用。
            (
                "WORKER_START_FAILED",
                AccessibilityErrorCode::IsolatedWorkerUnavailable,
            ),
            // worker 协议失败映射为通用操作失败。
            (
                "WORKER_PROTOCOL_FAILED",
                AccessibilityErrorCode::OperationFailed,
            ),
            // worker 输出超限映射为通用操作失败。
            (
                "WORKER_OUTPUT_TOO_LARGE",
                AccessibilityErrorCode::OperationFailed,
            ),
            // worker 等待失败映射为通用操作失败。
            (
                "WORKER_WAIT_FAILED",
                AccessibilityErrorCode::OperationFailed,
            ),
            // 内部窗口枚举失败映射为通用操作失败。
            (
                "WINDOW_ENUMERATION_FAILED",
                AccessibilityErrorCode::OperationFailed,
            ),
        ];
        // 核对内部映射规模。
        assert_eq!(mappings.len(), 6);
        // 逐项核对内部错误公开转换。
        for (input, expected) in mappings {
            // 每个受管输入必须映射到唯一公开分类。
            assert_eq!(
                AccessibilityErrorCode::from_internal_code(input),
                Some(expected)
            );
        }
        // 其他稳定错误必须继续由原所有者传播。
        assert_eq!(
            AccessibilityErrorCode::from_internal_code("STALE_SESSION"),
            None
        );
    }

    // 验证普通构造器保持统一公开 envelope。
    #[test]
    fn accessibility_error_constructor_keeps_message_and_empty_details() {
        // 构造 worker 协议失败夹具。
        let error = AccessibilityErrorCode::WorkerProtocolFailed.error("worker fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "WORKER_PROTOCOL_FAILED");
        // 构造器必须保持公开消息。
        assert_eq!(error.message, "worker fixture");
        // 普通构造器不得制造额外详情。
        assert!(error.details.is_null());
    }
}
