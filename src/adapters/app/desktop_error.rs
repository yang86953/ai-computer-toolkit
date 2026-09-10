//! 统一 App desktop provider Adapter 私有错误码及分类。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 App desktop provider Adapter 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AppDesktopErrorCode {
    // 表示当前 opaque 窗口同时命中多个实时目标。
    AmbiguousTarget,
    // 表示当前窗口 session 未发布请求的 capability。
    CapabilityUnsupported,
    // 表示敏感窗口操作缺少逐操作确认。
    ConfirmationRequired,
    // 表示调用方请求字段违反 provider 输入契约。
    InvalidArgument,
    // 表示下层 Module 成功结果违反 provider 投影契约。
    OperationFailed,
    // 表示实时窗口 inventory 已无法解析 opaque session。
    StaleSession,
}

// 提供 desktop provider 私有错误码与公开协议文本的唯一映射。
impl AppDesktopErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射实时目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射 capability 缺口。
            Self::CapabilityUnsupported => "CAPABILITY_UNSUPPORTED",
            // 映射敏感操作确认缺口。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射 provider 输入拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射下层结果不一致。
            Self::OperationFailed => "OPERATION_FAILED",
            // 映射实时窗口目标过期。
            Self::StaleSession => "STALE_SESSION",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 desktop provider 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 判断公开错误是否属于当前 desktop provider 分类。
    pub(super) fn matches(self, error: &AppControlError) -> bool {
        // 只比较稳定公开错误码，不读取下层 Module 私有事实。
        error.code == self.as_str()
    }
}

// 声明封闭错误类型的纯映射与分类测试。
#[cfg(test)]
mod tests {
    // 导入被测 desktop provider 私有错误类型。
    use super::AppDesktopErrorCode;

    // 验证六种 desktop provider 错误码的完整稳定映射。
    #[test]
    fn all_desktop_provider_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持实时目标歧义码。
            (AppDesktopErrorCode::AmbiguousTarget, "AMBIGUOUS_TARGET"),
            // 保持 capability 缺口码。
            (
                AppDesktopErrorCode::CapabilityUnsupported,
                "CAPABILITY_UNSUPPORTED",
            ),
            // 保持敏感操作确认码。
            (
                AppDesktopErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持参数拒绝码。
            (AppDesktopErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持执行失败码。
            (AppDesktopErrorCode::OperationFailed, "OPERATION_FAILED"),
            // 保持窗口目标过期码。
            (AppDesktopErrorCode::StaleSession, "STALE_SESSION"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 6);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证类型化分类只接受相同公开错误码。
    #[test]
    fn stale_session_classification_matches_exact_public_code() {
        // 构造 desktop provider 自有窗口过期错误。
        let stale = AppDesktopErrorCode::StaleSession.error("The window session no longer exists.");
        // 相同类别必须命中。
        assert!(AppDesktopErrorCode::StaleSession.matches(&stale));
        // 不同 provider 类别不得误命中。
        assert!(!AppDesktopErrorCode::AmbiguousTarget.matches(&stale));
    }
}
