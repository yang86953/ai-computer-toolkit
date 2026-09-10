//! Permission Boundary Module 私有封闭错误码。

// 导入公开错误 envelope 与安全详情值。
use crate::domain::AppControlError;
// 导入 JSON 详情类型。
use serde_json::Value;

// 表示受保护上下文门禁允许产生的完整错误集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PermissionBoundaryErrorCode {
    // 表示已确认的系统安全上下文拒绝目标访问。
    PermissionDenied,
    // 表示平台事实不足以认证目标访问。
    CapabilityAssessmentUnavailable,
}

// 提供私有类型到稳定公开协议文本的唯一映射。
impl PermissionBoundaryErrorCode {
    // 返回稳定公开错误码。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举全部封闭错误类别。
        match self {
            // 映射明确权限阻塞。
            Self::PermissionDenied => "PERMISSION_DENIED",
            // 映射不可确定的权限评估。
            Self::CapabilityAssessmentUnavailable => "CAPABILITY_ASSESSMENT_UNAVAILABLE",
        }
    }

    // 构造带 provider-neutral 证据的公开错误。
    pub(super) fn with_details(
        // 复制轻量错误类别。
        self,
        // 接收稳定公开消息。
        message: impl Into<String>,
        // 接收不含原生安全事实的详情。
        details: Value,
    ) -> AppControlError {
        // 隐藏私有类型并复用统一 envelope。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 声明错误映射的完整纯测试。
#[cfg(test)]
mod tests {
    // 导入被测封闭错误类型。
    use super::PermissionBoundaryErrorCode;

    // 验证两种错误码不会发生文本漂移。
    #[test]
    fn all_permission_boundary_errors_keep_stable_public_text() {
        // 固定完整映射表。
        let mappings = [
            // 固定明确阻塞错误。
            (
                PermissionBoundaryErrorCode::PermissionDenied,
                "PERMISSION_DENIED",
            ),
            // 固定不可确定评估错误。
            (
                PermissionBoundaryErrorCode::CapabilityAssessmentUnavailable,
                "CAPABILITY_ASSESSMENT_UNAVAILABLE",
            ),
        ];
        // 核对封闭集合规模。
        assert_eq!(mappings.len(), 2);
        // 逐项核对稳定文本。
        for (code, expected) in mappings {
            // 要求唯一映射逐字一致。
            assert_eq!(code.as_str(), expected);
        }
    }
}
