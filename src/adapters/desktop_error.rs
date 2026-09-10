//! 统一 legacy Desktop Adapter 私有错误码。

// 导入公开错误 envelope 与 JSON 详情值。
use crate::domain::AppControlError;
// 导入公开错误详情载体。
use serde_json::Value;

// 表示 Desktop Adapter 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DesktopAdapterErrorCode {
    // 表示请求的后台控制 operation 未获认证。
    BackgroundOperationUnavailable,
    // 表示敏感读取或 mutation 缺少逐操作确认。
    ConfirmationRequired,
    // 表示 Windows 未能移动鼠标光标。
    CursorMoveFailed,
    // 表示已授权目标窗口无法成为前景窗口。
    ForegroundActivationFailed,
    // 表示受控操作期间前景窗口发生变化。
    ForegroundChanged,
    // 表示 Windows 输入结构或发送准备失败。
    InputFailed,
    // 表示 Windows 未完整接收输入事件。
    InputBlockedOrPartial,
    // 表示调用方参数违反 Desktop Adapter 契约。
    InvalidArgument,
    // 表示下层成功结果无法投影为公开兼容形状。
    OperationFailed,
    // 表示固定可执行文件无法直接启动。
    ProcessStartFailed,
    // 表示精确诊断目标当前不可公开观察。
    TargetNotFound,
    // 表示旧兼容窗口关闭消息发送失败。
    WindowCloseFailed,
}

// 提供 Desktop Adapter 私有错误码与公开协议文本的唯一映射。
impl DesktopAdapterErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射后台 operation 缺口。
            Self::BackgroundOperationUnavailable => "BACKGROUND_OPERATION_UNAVAILABLE",
            // 映射逐操作确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射光标移动失败。
            Self::CursorMoveFailed => "CURSOR_MOVE_FAILED",
            // 映射目标激活失败。
            Self::ForegroundActivationFailed => "FOREGROUND_ACTIVATION_FAILED",
            // 映射宿主前景变化。
            Self::ForegroundChanged => "FOREGROUND_CHANGED",
            // 映射输入准备失败。
            Self::InputFailed => "INPUT_FAILED",
            // 映射输入阻断或部分成功。
            Self::InputBlockedOrPartial => "INPUT_BLOCKED_OR_PARTIAL",
            // 映射参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射内部结果投影失败。
            Self::OperationFailed => "OPERATION_FAILED",
            // 映射直接进程启动失败。
            Self::ProcessStartFailed => "PROCESS_START_FAILED",
            // 映射诊断目标不可见。
            Self::TargetNotFound => "TARGET_NOT_FOUND",
            // 映射旧兼容窗口关闭失败。
            Self::WindowCloseFailed => "WINDOW_CLOSE_FAILED",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Desktop Adapter 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 使用当前封闭错误码构造带安全详情的公开错误。
    pub(super) fn with_details(
        // 接收稳定公开消息。
        self,
        // 接收调用方可见的错误说明。
        message: impl Into<String>,
        // 接收已经净化的 provider-neutral 详情。
        details: Value,
        // 返回统一公开错误 envelope。
    ) -> AppControlError {
        // 隐藏私有错误类型并保留既有详情形状。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 声明封闭错误类型的纯映射测试。
#[cfg(test)]
mod tests {
    // 导入被测 Desktop Adapter 私有错误类型。
    use super::DesktopAdapterErrorCode;
    // 导入安全详情夹具构造器与空值。
    use serde_json::{Value, json};

    // 验证十二种 Desktop Adapter 错误码的完整稳定映射。
    #[test]
    fn all_desktop_adapter_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持后台 operation 缺口码。
            (
                DesktopAdapterErrorCode::BackgroundOperationUnavailable,
                "BACKGROUND_OPERATION_UNAVAILABLE",
            ),
            // 保持确认缺失码。
            (
                DesktopAdapterErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持光标移动失败码。
            (
                DesktopAdapterErrorCode::CursorMoveFailed,
                "CURSOR_MOVE_FAILED",
            ),
            // 保持前景激活失败码。
            (
                DesktopAdapterErrorCode::ForegroundActivationFailed,
                "FOREGROUND_ACTIVATION_FAILED",
            ),
            // 保持前景变化码。
            (
                DesktopAdapterErrorCode::ForegroundChanged,
                "FOREGROUND_CHANGED",
            ),
            // 保持输入准备失败码。
            (DesktopAdapterErrorCode::InputFailed, "INPUT_FAILED"),
            // 保持输入阻断或部分成功码。
            (
                DesktopAdapterErrorCode::InputBlockedOrPartial,
                "INPUT_BLOCKED_OR_PARTIAL",
            ),
            // 保持参数拒绝码。
            (DesktopAdapterErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持内部投影失败码。
            (DesktopAdapterErrorCode::OperationFailed, "OPERATION_FAILED"),
            // 保持进程启动失败码。
            (
                DesktopAdapterErrorCode::ProcessStartFailed,
                "PROCESS_START_FAILED",
            ),
            // 保持目标缺失码。
            (DesktopAdapterErrorCode::TargetNotFound, "TARGET_NOT_FOUND"),
            // 保持旧窗口关闭失败码。
            (
                DesktopAdapterErrorCode::WindowCloseFailed,
                "WINDOW_CLOSE_FAILED",
            ),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 12);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通与带详情构造器保持统一公开 envelope。
    #[test]
    fn desktop_adapter_error_constructors_keep_message_and_details() {
        // 构造普通参数错误。
        let plain = DesktopAdapterErrorCode::InvalidArgument.error("invalid fixture");
        // 普通错误使用映射后的稳定码。
        assert_eq!(plain.code, "INVALID_ARGUMENT");
        // 普通错误保持调用方消息。
        assert_eq!(plain.message, "invalid fixture");
        // 普通错误不制造额外详情。
        assert_eq!(plain.details, Value::Null);

        // 构造已经净化的详情夹具。
        let details = json!({ "foregroundUnchanged": false });
        // 构造带详情的前景变化错误。
        let detailed = DesktopAdapterErrorCode::ForegroundChanged.with_details(
            // 提供稳定公开消息。
            "foreground changed",
            // 复制详情以便随后对照。
            details.clone(),
        );
        // 带详情错误使用同一映射后的稳定码。
        assert_eq!(detailed.code, "FOREGROUND_CHANGED");
        // 带详情错误保持调用方消息。
        assert_eq!(detailed.message, "foreground changed");
        // 带详情错误逐字保留安全 JSON。
        assert_eq!(detailed.details, details);
    }
}
