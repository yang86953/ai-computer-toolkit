//! 定义 Windows 指针输入 Adapter 的封闭错误集合。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示平台指针 Component 可以直接产生的错误类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PointerInputWindowsErrorCode {
    // 表示线程 DPI 上下文无法建立。
    CoordinateContextUnavailable,
    // 表示公开点不在认证坐标范围内。
    InvalidArgument,
    // 表示 Windows 没有完整接收单个输入事件。
    PointerDispatchFailed,
    // 表示精确窗口在平台调用前已过期。
    StaleSession,
}

// 提供私有类型到公开错误码的唯一映射。
impl PointerInputWindowsErrorCode {
    // 返回稳定公开错误码。
    const fn as_str(self) -> &'static str {
        // 穷举全部平台错误类别。
        match self {
            // 映射坐标上下文缺口。
            Self::CoordinateContextUnavailable => "COORDINATE_CONTEXT_UNAVAILABLE",
            // 映射参数范围错误。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射平台输入拒绝。
            Self::PointerDispatchFailed => "POINTER_DISPATCH_FAILED",
            // 映射过期目标。
            Self::StaleSession => "STALE_SESSION",
        }
    }

    // 构造不公开 Win32 错误文本的统一错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Adapter 私有类型。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明错误映射纯回归测试。
#[cfg(test)]
mod tests {
    // 导入被测错误集合。
    use super::PointerInputWindowsErrorCode;

    // 验证封闭错误码文本保持稳定。
    #[test]
    fn pointer_input_windows_error_codes_are_stable() {
        // 固定全部映射。
        let mappings = [
            // 坐标上下文缺口。
            (
                PointerInputWindowsErrorCode::CoordinateContextUnavailable,
                "COORDINATE_CONTEXT_UNAVAILABLE",
            ),
            // 参数范围错误。
            (
                PointerInputWindowsErrorCode::InvalidArgument,
                "INVALID_ARGUMENT",
            ),
            // 输入调度失败。
            (
                PointerInputWindowsErrorCode::PointerDispatchFailed,
                "POINTER_DISPATCH_FAILED",
            ),
            // 目标过期。
            (PointerInputWindowsErrorCode::StaleSession, "STALE_SESSION"),
        ];
        // 核对集合规模。
        assert_eq!(mappings.len(), 4);
        // 逐项核对稳定文本。
        for (code, expected) in mappings {
            // 错误码必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }
}
