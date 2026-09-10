//! 封装 App facade 中 Browser Session provider 前后的错误阶段投影。

// 导入统一错误与请求类型。
use crate::domain::{AppControlError, CommandRequest};

// 投影 provider 前失败并保持其他 capability 原错误。
pub(super) fn before_provider(
    // 接收结构化错误。
    error: AppControlError,
    // 借用完整公开请求。
    request: &CommandRequest,
) -> AppControlError {
    // 委托独立 Component 生成未派发真值。
    crate::components::browser_session_lifecycle_error::project_before_dispatch(error, request)
}

// 投影 provider 成功后的 facade 拒绝。
pub(super) fn after_provider(
    // 接收 facade 结构化错误。
    error: AppControlError,
    // 借用原公开请求。
    request: &CommandRequest,
) -> AppControlError {
    // 委托独立 Component 生成不可重试真值。
    crate::components::browser_session_lifecycle_error::project_after_provider(error, request)
}
