//! 把业务接受前的统一错误投影为 broker schema 登记值。

// 导入统一 System 错误。
use crate::domain::AppControlError;

// 投影不泄漏输入、路径或平台事实的稳定错误码与消息。
pub(crate) fn pre_acceptance(error: &AppControlError) -> (&'static str, &'static str) {
    // 只保留响应 schema 已登记的安全错误类别。
    match error.code {
        // 输入或 target 语义失败。
        "INVALID_ARGUMENT" => (
            // 保留稳定参数错误码。
            "INVALID_ARGUMENT",
            // 不回显输入或路径。
            "The long operation submission is invalid.",
        ),
        // 逐操作确认失败。
        "CONFIRMATION_REQUIRED" => (
            // 保留独立确认错误码。
            "CONFIRMATION_REQUIRED",
            // 使用固定确认消息。
            "The long operation submission requires explicit confirmation.",
        ),
        // 当前安全上下文禁止 mutation。
        "PERMISSION_DENIED" => (
            // 保留权限错误码。
            "PERMISSION_DENIED",
            // 不公开 session、桌面或完整性事实。
            "The current security context does not permit this long operation.",
        ),
        // catalog 或固定 worker 缺口。
        "CAPABILITY_GAP" | "BACKGROUND_OPERATION_UNAVAILABLE" => (
            // 统一投影 capability 缺口。
            "CAPABILITY_GAP",
            // 不公开安装或 worker 路径。
            "The requested long operation capability is unavailable.",
        ),
        // 其他 System 失败保守视为 broker 不可用。
        _ => (
            // 使用 broker 生命周期错误码。
            "BROKER_UNAVAILABLE",
            // 不公开内部错误消息。
            "The long operation broker could not validate the submission safely.",
        ),
    }
}

// 声明错误投影的封闭回归矩阵。
#[cfg(test)]
// 保持生产 Component 文件简短。
#[path = "long_operation_error_projection_tests.rs"]
mod tests;
