//! 统一 Element Location Module 私有错误码。

// 导入产品级公开错误 envelope。
use crate::domain::AppControlError;

// 表示 Element Location Module 允许直接产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ElementLocationErrorCode {
    // 表示语义 selector 同时匹配多个元素。
    AmbiguousTarget,
    // 表示输入、selector 或搜索边界不合法。
    InvalidArgument,
    // 表示有界搜索无法证明零或唯一匹配。
    SearchIncomplete,
    // 表示 worker 返回值违反位置隐私契约。
    WorkerProtocolFailed,
}

// 提供 Module 私有错误类别与公开协议文本的唯一映射。
impl ElementLocationErrorCode {
    // 返回版本化公开错误码文本。
    pub(super) const fn as_str(self) -> &'static str {
        // 穷举封闭集合。
        match self {
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射公共参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射搜索不完整。
            Self::SearchIncomplete => "SEARCH_INCOMPLETE",
            // 映射 worker 协议失败。
            Self::WorkerProtocolFailed => "WORKER_PROTOCOL_FAILED",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    pub(super) fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Module 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 声明封闭错误类型测试。
#[cfg(test)]
mod tests {
    // 导入被测类型。
    use super::ElementLocationErrorCode;

    // 验证完整错误集合的稳定公开文本。
    #[test]
    fn element_location_errors_are_closed_and_stable() {
        // 固定完整映射。
        let mappings = [
            // 保持目标歧义码。
            (
                ElementLocationErrorCode::AmbiguousTarget,
                "AMBIGUOUS_TARGET",
            ),
            // 保持参数拒绝码。
            (
                ElementLocationErrorCode::InvalidArgument,
                "INVALID_ARGUMENT",
            ),
            // 保持搜索不完整码。
            (
                ElementLocationErrorCode::SearchIncomplete,
                "SEARCH_INCOMPLETE",
            ),
            // 保持 worker 协议失败码。
            (
                ElementLocationErrorCode::WorkerProtocolFailed,
                "WORKER_PROTOCOL_FAILED",
            ),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 4);
        // 逐项核对公开文本。
        for (code, expected) in mappings {
            // 每个私有类别只有一个稳定文本。
            assert_eq!(code.as_str(), expected);
        }
    }
}
