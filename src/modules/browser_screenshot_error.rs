//! 定义 Browser Screenshot Module 私有的封闭错误码映射。

// 导入统一公开错误 envelope。
use crate::domain::AppControlError;

// 枚举当前 Module 自己产生或筛选的稳定错误类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// 该类型只在 Browser Screenshot Module 内部传播。
pub(super) enum BrowserScreenshotErrorCode {
    // 表示 Chromium 执行失败。
    BrowserFailed,
    // 表示 Chromium 无法启动。
    BrowserStartFailed,
    // 表示隔离浏览器超过领域 deadline。
    BrowserTimeout,
    // 表示没有可用的认证 Chromium runtime。
    BrowserUnavailable,
    // 表示调用方没有逐操作确认。
    ConfirmationRequired,
    // 表示隔离浏览器意外改变前景窗口。
    ForegroundChanged,
    // 表示请求参数不满足封闭输入契约。
    InvalidArgument,
    // 表示输出目标不满足安全路径契约。
    InvalidOutputPath,
    // 表示既有输出缺少独立覆盖许可。
    OverwriteConfirmationRequired,
    // 表示 PNG 候选缺失、无效或无法提交。
    ScreenshotMissing,
    // 表示独占临时 profile 无法建立或跨越协议边界。
    TempProfileFailed,
    // 表示通用 worker process watchdog 超时。
    WorkerProcessTimeout,
    // 表示 worker envelope 违反封闭协议。
    WorkerProtocolViolation,
}

// 提供 Module 私有错误码与公开协议文本的唯一映射。
impl BrowserScreenshotErrorCode {
    // 固定 browser worker 失败 envelope 允许转发的九种类别。
    const WORKER_ALLOWED: [Self; 9] = [
        // 允许确认缺失。
        Self::ConfirmationRequired,
        // 允许参数拒绝。
        Self::InvalidArgument,
        // 允许 runtime 缺失。
        Self::BrowserUnavailable,
        // 允许浏览器启动失败。
        Self::BrowserStartFailed,
        // 允许浏览器执行失败。
        Self::BrowserFailed,
        // 允许浏览器 deadline。
        Self::BrowserTimeout,
        // 允许截图候选缺失。
        Self::ScreenshotMissing,
        // 允许 staging 输出路径失效。
        Self::InvalidOutputPath,
        // 允许前景干扰。
        Self::ForegroundChanged,
    ];

    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射浏览器执行失败。
            Self::BrowserFailed => "BROWSER_FAILED",
            // 映射浏览器启动失败。
            Self::BrowserStartFailed => "BROWSER_START_FAILED",
            // 映射浏览器 deadline。
            Self::BrowserTimeout => "BROWSER_TIMEOUT",
            // 映射浏览器 runtime 缺失。
            Self::BrowserUnavailable => "BROWSER_UNAVAILABLE",
            // 映射确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射前景变化。
            Self::ForegroundChanged => "FOREGROUND_CHANGED",
            // 映射参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射无效输出路径。
            Self::InvalidOutputPath => "INVALID_OUTPUT_PATH",
            // 映射覆盖确认缺失。
            Self::OverwriteConfirmationRequired => "OVERWRITE_CONFIRMATION_REQUIRED",
            // 映射截图候选缺失。
            Self::ScreenshotMissing => "SCREENSHOT_MISSING",
            // 映射临时 profile 失败。
            Self::TempProfileFailed => "TEMP_PROFILE_FAILED",
            // 映射通用 worker process timeout。
            Self::WorkerProcessTimeout => "TIMEOUT",
            // 映射 worker 协议违规。
            Self::WorkerProtocolViolation => "WORKER_PROTOCOL_VIOLATION",
        }
    }

    // 从 browser worker 失败 envelope 解析允许转发的封闭类别。
    pub(super) fn from_worker(code: &str) -> Option<Self> {
        // 只从封闭类型白名单中查找逐字匹配项。
        Self::WORKER_ALLOWED
            // 按值遍历复制型私有枚举。
            .into_iter()
            // 公共字符串只由 as_str 唯一映射。
            .find(|candidate| candidate.as_str() == code)
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Module 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误映射、worker 白名单与构造测试。
#[cfg(test)]
mod tests {
    // 导入被测 Browser Screenshot 私有错误类型。
    use super::BrowserScreenshotErrorCode;

    // 验证十三项 Module 错误码的完整稳定映射。
    #[test]
    fn all_browser_screenshot_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持浏览器执行失败码。
            (BrowserScreenshotErrorCode::BrowserFailed, "BROWSER_FAILED"),
            // 保持浏览器启动失败码。
            (
                BrowserScreenshotErrorCode::BrowserStartFailed,
                "BROWSER_START_FAILED",
            ),
            // 保持浏览器 deadline 码。
            (
                BrowserScreenshotErrorCode::BrowserTimeout,
                "BROWSER_TIMEOUT",
            ),
            // 保持浏览器 runtime 缺失码。
            (
                BrowserScreenshotErrorCode::BrowserUnavailable,
                "BROWSER_UNAVAILABLE",
            ),
            // 保持确认缺失码。
            (
                BrowserScreenshotErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持前景变化码。
            (
                BrowserScreenshotErrorCode::ForegroundChanged,
                "FOREGROUND_CHANGED",
            ),
            // 保持参数拒绝码。
            (
                BrowserScreenshotErrorCode::InvalidArgument,
                "INVALID_ARGUMENT",
            ),
            // 保持输出路径错误码。
            (
                BrowserScreenshotErrorCode::InvalidOutputPath,
                "INVALID_OUTPUT_PATH",
            ),
            // 保持覆盖确认缺失码。
            (
                BrowserScreenshotErrorCode::OverwriteConfirmationRequired,
                "OVERWRITE_CONFIRMATION_REQUIRED",
            ),
            // 保持截图候选缺失码。
            (
                BrowserScreenshotErrorCode::ScreenshotMissing,
                "SCREENSHOT_MISSING",
            ),
            // 保持临时 profile 失败码。
            (
                BrowserScreenshotErrorCode::TempProfileFailed,
                "TEMP_PROFILE_FAILED",
            ),
            // 保持通用 worker process timeout 码。
            (BrowserScreenshotErrorCode::WorkerProcessTimeout, "TIMEOUT"),
            // 保持 worker 协议违规码。
            (
                BrowserScreenshotErrorCode::WorkerProtocolViolation,
                "WORKER_PROTOCOL_VIOLATION",
            ),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 13);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证 worker 白名单只接受原有九种稳定类别。
    #[test]
    fn browser_worker_error_whitelist_is_closed() {
        // 固定 worker 可转发文本集合。
        let allowed = [
            // 允许确认缺失。
            "CONFIRMATION_REQUIRED",
            // 允许参数拒绝。
            "INVALID_ARGUMENT",
            // 允许 runtime 缺失。
            "BROWSER_UNAVAILABLE",
            // 允许浏览器启动失败。
            "BROWSER_START_FAILED",
            // 允许浏览器执行失败。
            "BROWSER_FAILED",
            // 允许浏览器 deadline。
            "BROWSER_TIMEOUT",
            // 允许截图候选缺失。
            "SCREENSHOT_MISSING",
            // 允许 staging 输出路径失效。
            "INVALID_OUTPUT_PATH",
            // 允许前景变化。
            "FOREGROUND_CHANGED",
        ];
        // 核对原有白名单规模。
        assert_eq!(allowed.len(), 9);
        // 验证每项白名单输入只映射回自身。
        for expected in allowed {
            // 解析 worker 错误类别。
            let parsed = BrowserScreenshotErrorCode::from_worker(expected);
            // 白名单项必须可解析且逐字保持。
            assert_eq!(
                parsed.map(BrowserScreenshotErrorCode::as_str),
                Some(expected)
            );
        }
        // 通用 process timeout 不是 worker envelope 白名单成员。
        assert_eq!(BrowserScreenshotErrorCode::from_worker("TIMEOUT"), None);
        // 覆盖确认只由父 Module 产生，不接受 worker 注入。
        assert_eq!(
            BrowserScreenshotErrorCode::from_worker("OVERWRITE_CONFIRMATION_REQUIRED"),
            None,
        );
        // 任意未知 provider 码必须失败闭合。
        assert_eq!(
            BrowserScreenshotErrorCode::from_worker("PROVIDER_PRIVATE_FAILURE"),
            None,
        );
    }

    // 验证普通构造器保持统一公开 envelope。
    #[test]
    fn browser_screenshot_error_constructor_keeps_message_and_empty_details() {
        // 构造截图候选缺失错误。
        let error = BrowserScreenshotErrorCode::ScreenshotMissing.error("candidate fixture");
        // 构造器必须选择封闭映射文本。
        assert_eq!(error.code, "SCREENSHOT_MISSING");
        // 构造器必须保持公开消息。
        assert_eq!(error.message, "candidate fixture");
        // 普通构造器不得制造额外详情。
        assert!(error.details.is_null());
    }
}
