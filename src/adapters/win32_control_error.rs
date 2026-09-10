//! 统一 Win32 Control Adapter 私有错误码及 timeout 分类。

// 导入 JSON 详情值。
use serde_json::Value;

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Win32 Control Adapter 直接产生或显式映射的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Win32ControlErrorCode {
    // 表示兼容 surface 不支持请求的后台操作或参数范围。
    BackgroundOperationUnavailable,
    // 表示 Standard Edit mutation 缺少逐操作确认。
    ConfirmationRequired,
    // 表示 legacy mutation 期间宿主前景发生变化。
    HostInterferenceDetected,
    // 表示调用方目标或参数违反兼容契约。
    InvalidArgument,
    // 表示下层成功结果缺少兼容投影所需证据。
    OperationFailed,
    // 表示兼容调用方可见的目标超时或不可用。
    TargetHungOrUnavailable,
    // 表示 legacy native 目标必须迁移到 opaque 身份。
    TargetIdMigrationRequired,
    // 表示正式 Standard Edit Module 的 timeout 分类。
    Timeout,
}

// 提供 Adapter 私有错误类别与公开协议文本的唯一映射。
impl Win32ControlErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射后台操作能力缺口。
            Self::BackgroundOperationUnavailable => "BACKGROUND_OPERATION_UNAVAILABLE",
            // 映射逐操作确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射宿主前景干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射兼容结果投影失败。
            Self::OperationFailed => "OPERATION_FAILED",
            // 映射旧调用方可见的目标超时。
            Self::TargetHungOrUnavailable => "TARGET_HUNG_OR_UNAVAILABLE",
            // 映射 native 目标迁移要求。
            Self::TargetIdMigrationRequired => "TARGET_ID_MIGRATION_REQUIRED",
            // 映射正式 Module timeout。
            Self::Timeout => "TIMEOUT",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Adapter 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 使用当前封闭错误码构造带安全详情的公开错误。
    pub(super) fn with_details(
        // 接收封闭错误类别。
        self,
        // 接收调用方可见的安全消息。
        message: impl Into<String>,
        // 接收已经净化的 provider-neutral 详情。
        details: Value,
    ) -> AppControlError {
        // 隐藏 Adapter 私有类型并保持既有 details 形状。
        AppControlError::with_details(self.as_str(), message, details)
    }

    // 判断公开错误是否属于当前 Adapter 分类。
    pub(super) fn matches(self, error: &AppControlError) -> bool {
        // 只比较稳定公开错误码，不读取 provider 私有事实。
        error.code == self.as_str()
    }
}

// 声明封闭错误类型的纯映射、构造与分类测试。
#[cfg(test)]
mod tests {
    // 导入 JSON 详情夹具构造宏。
    use serde_json::json;

    // 导入被测 Win32 Control Adapter 私有错误类型。
    use super::Win32ControlErrorCode;

    // 验证八种兼容错误码的完整稳定映射。
    #[test]
    fn all_win32_control_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持后台操作能力缺口码。
            (
                Win32ControlErrorCode::BackgroundOperationUnavailable,
                "BACKGROUND_OPERATION_UNAVAILABLE",
            ),
            // 保持确认缺失码。
            (
                Win32ControlErrorCode::ConfirmationRequired,
                "CONFIRMATION_REQUIRED",
            ),
            // 保持宿主前景干扰码。
            (
                Win32ControlErrorCode::HostInterferenceDetected,
                "HOST_INTERFERENCE_DETECTED",
            ),
            // 保持参数拒绝码。
            (Win32ControlErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持兼容结果投影失败码。
            (Win32ControlErrorCode::OperationFailed, "OPERATION_FAILED"),
            // 保持旧调用方目标超时码。
            (
                Win32ControlErrorCode::TargetHungOrUnavailable,
                "TARGET_HUNG_OR_UNAVAILABLE",
            ),
            // 保持 native 目标迁移要求码。
            (
                Win32ControlErrorCode::TargetIdMigrationRequired,
                "TARGET_ID_MIGRATION_REQUIRED",
            ),
            // 保持正式 Module timeout 码。
            (Win32ControlErrorCode::Timeout, "TIMEOUT"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 8);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 验证普通与带详情构造器保持统一公开 envelope。
    #[test]
    fn win32_control_error_constructors_keep_message_and_details() {
        // 构造普通确认缺失错误。
        let plain = Win32ControlErrorCode::ConfirmationRequired.error("confirm fixture");
        // 普通构造器必须选择封闭映射文本。
        assert_eq!(plain.code, "CONFIRMATION_REQUIRED");
        // 普通构造器必须保持公开消息。
        assert_eq!(plain.message, "confirm fixture");
        // 普通构造器不得制造额外详情。
        assert!(plain.details.is_null());

        // 构造 provider-neutral outcome 详情。
        let details = json!({ "outcome": "completed", "retrySafe": false });
        // 构造带详情的宿主干扰错误。
        let detailed = Win32ControlErrorCode::HostInterferenceDetected.with_details(
            // 提供稳定公开消息。
            "foreground fixture",
            // 复制安全详情以便随后对照。
            details.clone(),
        );
        // 带详情构造器必须选择封闭映射文本。
        assert_eq!(detailed.code, "HOST_INTERFERENCE_DETECTED");
        // 带详情构造器必须保持公开消息。
        assert_eq!(detailed.message, "foreground fixture");
        // 带详情构造器必须逐字保持安全 JSON。
        assert_eq!(detailed.details, details);
    }

    // 验证 timeout 分类只接受精确公开错误码。
    #[test]
    fn timeout_classification_matches_exact_public_code() {
        // 构造正式 Module timeout 错误。
        let timeout = Win32ControlErrorCode::Timeout.error("timeout fixture");
        // 精确 timeout 必须命中。
        assert!(Win32ControlErrorCode::Timeout.matches(&timeout));
        // 其他 Adapter 分类不得误命中。
        assert!(!Win32ControlErrorCode::InvalidArgument.matches(&timeout));
    }
}
