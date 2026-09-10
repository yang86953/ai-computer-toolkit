//! Window Close Module 成功事实的 provider-neutral 投影 Component。

// 导入 JSON 值与构造器。
use serde_json::{Value, json};

// 导入公开结果类型。
use crate::domain::AppResult;

// 导入 parent Adapter 私有封闭错误码。
use super::error_code::AppDesktopErrorCode;

// 把 Window Close Module 成功事实收窄为 provider-neutral app data。
pub(super) fn public_window_close_result(
    // 接收 Module 私有成功事实。
    result: &Value,
    // 接收调用方已知 opaque 目标。
    session_id: &str,
) -> AppResult<Value> {
    // 读取目标已关闭证据。
    let closed = result
        // 访问固定字段。
        .get("closed")
        // 只接受布尔值。
        .and_then(Value::as_bool)
        // 缺失表示内部契约失效。
        .filter(|value| *value)
        // 映射为稳定内部错误。
        .ok_or_else(|| {
            // 返回 provider 结果不完整。
            AppDesktopErrorCode::OperationFailed.error(
                // 不公开平台事实。
                "The window close module returned no closure evidence.",
            )
        })?;
    // 读取前景未变证据。
    let foreground_unchanged = result
        // 访问固定字段。
        .get("foregroundUnchanged")
        // 只接受布尔值。
        .and_then(Value::as_bool)
        // 只接受认证成功值。
        .filter(|value| *value)
        // 缺失表示内部契约失效。
        .ok_or_else(|| {
            // 返回 provider 结果不完整。
            AppDesktopErrorCode::OperationFailed.error(
                // 不公开前景句柄。
                "The window close module returned no foreground evidence.",
            )
        })?;
    // 读取静态权限预检通过证据。
    let permission_preflight = result
        // 访问固定字段。
        .get("permissionPreflight")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 只接受同级或较低完整性成功关系。
        .filter(|value| *value == "no-static-integrity-block-observed")
        // 缺失表示内部契约失效。
        .ok_or_else(|| {
            // 返回 provider 结果不完整。
            AppDesktopErrorCode::OperationFailed.error(
                // 不公开平台权限事实。
                "The window close module returned no certified permission evidence.",
            )
        })?;
    // 读取没有主动写探针的证据。
    let active_write_probe_performed = result
        // 访问固定字段。
        .get("activeWriteProbePerformed")
        // 只接受布尔值。
        .and_then(Value::as_bool)
        // 只接受安全的 false。
        .filter(|value| !*value)
        // 缺失或 true 都表示内部契约失效。
        .ok_or_else(|| {
            // 返回 provider 结果不完整。
            AppDesktopErrorCode::OperationFailed.error(
                // 不公开平台权限事实。
                "The window close module returned invalid write-probe evidence.",
            )
        })?;
    // 返回固定 compatibility mapper 数据。
    Ok(json!({
        // 回显调用方已知 opaque 目标供 facade 核对。
        "targetId": session_id,
        // 返回 provider-neutral 目标类别。
        "kind": "application-window",
        // 返回稳定关闭状态。
        "state": "closed",
        // 返回关闭证据。
        "closed": closed,
        // 返回嵌套前景证据。
        "foreground": { "unchanged": foreground_unchanged },
        // 返回无主动试写的静态权限关系。
        "permissionPreflight": permission_preflight,
        // 明确权限评估没有主动写探针。
        "activeWriteProbePerformed": active_write_probe_performed,
        // 请求 facade 提升固定兼容形状。
        "compatibilityShape": "provider-neutral-window-close-v1",
    }))
}
