//! 统一共享 Windows backend 私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示共享 Windows backend 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowsErrorCode {
    // 表示窗口或控件目标匹配多个候选。
    AmbiguousTarget,
    // 表示控件类型不属于认证的后台写边界。
    BackgroundOperationUnavailable,
    // 表示 ToolHelp 进程快照结构无法安全建立。
    ProcessSnapshotFailed,
    // 表示标准 Edit 控件未在固定 deadline 内响应。
    TargetHungOrUnavailable,
    // 表示窗口或控件目标没有匹配项。
    TargetNotFound,
    // 表示同步顶层窗口枚举失败。
    WindowEnumerationFailed,
}

// 提供 Windows backend 私有错误类别与公开协议文本的唯一映射。
impl WindowsErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射后台写能力缺口。
            Self::BackgroundOperationUnavailable => "BACKGROUND_OPERATION_UNAVAILABLE",
            // 映射进程快照结构失败。
            Self::ProcessSnapshotFailed => "PROCESS_SNAPSHOT_FAILED",
            // 映射标准 Edit 固定 deadline 失败。
            Self::TargetHungOrUnavailable => "TARGET_HUNG_OR_UNAVAILABLE",
            // 映射目标缺失。
            Self::TargetNotFound => "TARGET_NOT_FOUND",
            // 映射顶层窗口枚举失败。
            Self::WindowEnumerationFailed => "WINDOW_ENUMERATION_FAILED",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 backend 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型的纯映射与构造测试。
#[cfg(test)]
mod tests {
    // 导入被测 Windows backend 私有错误类型。
    use super::WindowsErrorCode;

    // 验证六种 Windows backend 错误码的完整稳定映射。
    #[test]
    fn all_windows_backend_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持目标歧义码。
            (WindowsErrorCode::AmbiguousTarget, "AMBIGUOUS_TARGET"),
            // 保持后台写能力缺口码。
            (
                WindowsErrorCode::BackgroundOperationUnavailable,
                "BACKGROUND_OPERATION_UNAVAILABLE",
            ),
            // 保持进程快照失败码。
            (
                WindowsErrorCode::ProcessSnapshotFailed,
                "PROCESS_SNAPSHOT_FAILED",
            ),
            // 保持标准 Edit timeout 码。
            (
                WindowsErrorCode::TargetHungOrUnavailable,
                "TARGET_HUNG_OR_UNAVAILABLE",
            ),
            // 保持目标缺失码。
            (WindowsErrorCode::TargetNotFound, "TARGET_NOT_FOUND"),
            // 保持窗口枚举失败码。
            (
                WindowsErrorCode::WindowEnumerationFailed,
                "WINDOW_ENUMERATION_FAILED",
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

    // 验证普通构造器保持统一公开 envelope。
    #[test]
    fn windows_backend_error_constructor_keeps_message_and_empty_details() {
        // 构造进程快照失败夹具。
        let error = WindowsErrorCode::ProcessSnapshotFailed.error("snapshot fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "PROCESS_SNAPSHOT_FAILED");
        // 构造器必须保持公开消息。
        assert_eq!(error.message, "snapshot fixture");
        // 普通构造器不得制造额外详情。
        assert!(error.details.is_null());
    }
}
