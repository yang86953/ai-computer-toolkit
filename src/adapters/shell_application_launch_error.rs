//! 统一 Shell Application Launch Component 私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Shell 启动 Component 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ShellLaunchErrorCode {
    // 表示 Windows Shell 已接收但拒绝启动请求。
    ApplicationStartFailed,
    // 表示精确应用缺少认证的 Shell identity。
    CapabilityUnavailable,
    // 表示 Windows 按当前权限拒绝精确应用启动。
    PermissionDenied,
    // 表示当前线程无法建立可用 Shell COM apartment。
    ShellProviderUnavailable,
    // 表示认证 Shell identity 在使用时已无法解析。
    StaleSession,
}

// 提供 Shell 启动私有错误码与公开协议文本的唯一映射。
impl ShellLaunchErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射 Shell 启动失败。
            Self::ApplicationStartFailed => "APPLICATION_START_FAILED",
            // 映射认证 identity 缺口。
            Self::CapabilityUnavailable => "CAPABILITY_UNAVAILABLE",
            // 映射权限拒绝。
            Self::PermissionDenied => "PERMISSION_DENIED",
            // 映射 Shell provider 初始化失败。
            Self::ShellProviderUnavailable => "SHELL_PROVIDER_UNAVAILABLE",
            // 映射使用时 identity 过期。
            Self::StaleSession => "STALE_SESSION",
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
    // 导入被测 Shell 启动私有错误类型。
    use super::ShellLaunchErrorCode;

    // 验证五种 Shell 启动错误码的完整稳定映射。
    #[test]
    fn all_shell_launch_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持 Shell 启动失败码。
            (
                ShellLaunchErrorCode::ApplicationStartFailed,
                "APPLICATION_START_FAILED",
            ),
            // 保持认证 identity 缺口码。
            (
                ShellLaunchErrorCode::CapabilityUnavailable,
                "CAPABILITY_UNAVAILABLE",
            ),
            // 保持权限拒绝码。
            (ShellLaunchErrorCode::PermissionDenied, "PERMISSION_DENIED"),
            // 保持 Shell provider 初始化失败码。
            (
                ShellLaunchErrorCode::ShellProviderUnavailable,
                "SHELL_PROVIDER_UNAVAILABLE",
            ),
            // 保持使用时过期码。
            (ShellLaunchErrorCode::StaleSession, "STALE_SESSION"),
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
    fn shell_launch_error_constructor_keeps_message_and_empty_details() {
        // 构造稳定权限错误夹具。
        let error = ShellLaunchErrorCode::PermissionDenied.error("permission fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "PERMISSION_DENIED");
        // 构造器必须保持调用方消息。
        assert_eq!(error.message, "permission fixture");
        // 普通错误不得制造 provider 详情。
        assert!(error.details.is_null());
    }
}
