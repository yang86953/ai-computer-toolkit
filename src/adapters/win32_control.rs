//! `win32-control` 兼容 surface 的 Standard Edit mapper。

// 导入 JSON 对象与构造工具。
use serde_json::{Map, Value, json};

// 引入 Win32 Control Adapter 私有封闭错误码实现。
#[path = "win32_control_error.rs"]
mod error_code;

// 导入 Win32 Control Adapter 私有封闭错误码。
use error_code::Win32ControlErrorCode;

// 导入兼容 Adapter、领域类型与正式 Standard Edit Module。
use crate::{
    // 旧 native 目标仅保留在迁移兼容分支。
    adapters::{
        // 导入统一 Adapter 接口。
        AppAdapter,
        // 导入旧目标兼容所需的固定 Windows helper。
        windows::{foreground_hwnd, set_standard_edit_text, unique_standard_edit_control},
    },
    // 导入稳定错误、请求与结果类型。
    domain::{AppControlError, AppResult, CommandRequest},
    // 正式 s2:c 路线只通过领域 Module。
    modules::standard_edit,
};

// 声明无状态兼容 Adapter。
pub struct Win32ControlAdapter;

// 从请求读取必需 opaque 或 legacy sessionId。
fn required_session_id(request: &CommandRequest) -> AppResult<&str> {
    // 只接受非空 JSON string。
    request
        // 读取目标对象。
        .target
        // 读取固定字段。
        .get("sessionId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 拒绝空值。
        .filter(|value| !value.is_empty())
        // 映射稳定参数错误。
        .ok_or_else(|| {
            // 返回参数错误。
            Win32ControlErrorCode::InvalidArgument.error(
                // 说明必需目标。
                "target.sessionId is required.",
            )
        })
}

// 识别反向迁移前的 Rust native 兼容目标。
fn is_legacy_native_target(session_id: &str) -> bool {
    // 只接受旧固定前缀，不解析或生成新 native ID。
    session_id.starts_with("win32-control:window:")
}

// 从 legacy 参数读取文本和可选 timeout。
fn legacy_input(request: &CommandRequest) -> AppResult<(&str, u32)> {
    // 读取必需文本。
    let text = request
        // 读取参数对象。
        .args
        // 读取固定 text 字段。
        .get("text")
        // 只接受 JSON string。
        .and_then(Value::as_str)
        // 映射参数错误。
        .ok_or_else(|| {
            // 返回稳定参数错误。
            Win32ControlErrorCode::InvalidArgument.error(
                // 说明必需字段。
                "args.text is required.",
            )
        })?;
    // 读取可选 timeout，缺失时使用契约默认。
    let timeout_ms = match request.args.get("timeoutMs") {
        // 缺失时使用 2000ms。
        None => standard_edit::DEFAULT_TIMEOUT_MS,
        // 只接受可转换为 u32 的整数。
        Some(value) => value
            // 读取无符号整数。
            .as_u64()
            // 转换为 u32。
            .and_then(|value| u32::try_from(value).ok())
            // 映射参数错误。
            .ok_or_else(|| {
                // 返回稳定参数错误。
                Win32ControlErrorCode::InvalidArgument.error(
                    // 说明 timeout 类型。
                    "args.timeoutMs must be an integer.",
                )
            })?,
    };
    // 验证 UTF-8 字节上限和 timeout 范围。
    if text.len() > standard_edit::MAXIMUM_UTF8_BYTES
        // 验证 deadline 封闭范围。
        || !(standard_edit::MINIMUM_TIMEOUT_MS..=standard_edit::MAXIMUM_TIMEOUT_MS)
            .contains(&timeout_ms)
    {
        // 返回稳定参数错误。
        return Err(Win32ControlErrorCode::InvalidArgument.error(
            // 说明完整边界。
            "Standard Edit mutation requires at most 65536 UTF-8 bytes and timeoutMs 1..30000.",
        ));
    }
    // 返回借用文本和已验证 deadline。
    Ok((text, timeout_ms))
}

// 将 Module timeout 映射为旧兼容错误码并保留 outcome-unknown。
fn map_legacy_failure(mut error: AppControlError) -> AppControlError {
    // 非 timeout 原样传播。
    if !Win32ControlErrorCode::Timeout.matches(&error) {
        // 返回原错误。
        return error;
    }
    // 提取 Module timeout details。
    let mut details = match error.details {
        // 保留已有对象字段。
        Value::Object(object) => object,
        // 其他形状使用新对象。
        _ => Map::new(),
    };
    // 保留正式 provider 错误码供兼容调用方审计。
    details.insert(
        // 使用稳定字段名。
        "providerErrorCode".to_owned(),
        // 保存正式 TIMEOUT。
        Value::String(Win32ControlErrorCode::Timeout.as_str().to_owned()),
    );
    // 替换为历史兼容错误码。
    error.code = Win32ControlErrorCode::TargetHungOrUnavailable.as_str();
    // 恢复合并后的 details。
    error.details = Value::Object(details);
    // 返回兼容错误。
    error
}

// 构造不含 native 标识的兼容成功结果。
fn legacy_success(session_id: &str, module: &Value) -> AppResult<Value> {
    // 读取正式回读证据。
    let verified_by_readback = module
        // 读取固定字段。
        .get("verifiedByReadback")
        // 只接受布尔值。
        .and_then(Value::as_bool)
        // 缺失表示内部契约失败。
        .ok_or_else(|| {
            // 返回执行失败。
            Win32ControlErrorCode::OperationFailed.error(
                // 不公开内部细节。
                "The standard Edit module returned no readback evidence.",
            )
        })?;
    // 返回旧 app/operation 与新 opaque 目标。
    Ok(json!({
        // 标记成功。
        "ok": true,
        // 保留旧 surface。
        "app": "win32-control",
        // 保留旧 operation。
        "operation": "set-text",
        // 只回显 opaque sessionId。
        "sessionId": session_id,
        // 返回固定回读证据。
        "verifiedByReadback": verified_by_readback,
        // 返回认证执行域。
        "executionDomain": "same-session-no-focus",
        // 只返回前景不变布尔值。
        "foreground": { "unchanged": true },
        // 标记安全兼容形状。
        "compatibilityShape": "secured-standard-edit-legacy-v1",
    }))
}

// 执行旧 native 目标的有限 Rust 兼容分支。
fn set_legacy_native_text(request: &CommandRequest, session_id: &str) -> AppResult<Value> {
    // 确认必须先于 native 目标解析。
    if !request.confirmed {
        // 返回统一确认错误。
        return Err(Win32ControlErrorCode::ConfirmationRequired.error(
            // 明确 mutation 风险。
            "Standard Edit text mutation requires confirmation.",
        ));
    }
    // 读取并验证 legacy 输入。
    let (text, timeout_ms) = legacy_input(request)?;
    // 旧 helper 只认证固定 2000ms，拒绝扩大其兼容范围。
    if timeout_ms != standard_edit::DEFAULT_TIMEOUT_MS {
        // 返回迁移兼容边界错误。
        return Err(Win32ControlErrorCode::BackgroundOperationUnavailable.error(
            // 指示新 timeout 路径必须使用 opaque 目标。
            "Legacy native targets support the fixed 2000ms compatibility timeout only.",
        ));
    }
    // 解析旧目标；该分支不生成或公开 native ID。
    let control = unique_standard_edit_control(&request.target)?;
    // 记录写前前景。
    let foreground_before = foreground_hwnd();
    // 执行历史固定 WM_SETTEXT helper。
    set_standard_edit_text(&control, text)?;
    // 写后读取前景。
    let foreground_after = foreground_hwnd();
    // 前景变化必须 fail closed。
    if foreground_before != foreground_after {
        // 返回宿主干扰错误。
        return Err(
            Win32ControlErrorCode::HostInterferenceDetected.with_details(
                // 明确目标可能已经变更。
                "Foreground changed during legacy standard Edit mutation.",
                // 禁止自动重试。
                json!({
                    // 消息调用已经返回。
                    "outcome": "completed",
                    // 禁止自动重试。
                    "retrySafe": false,
                    // 目标可能已经变更。
                    "targetMayHaveMutated": true,
                }),
            ),
        );
    }
    // 返回无 native 标识的历史兼容结果。
    Ok(json!({
        // 标记成功。
        "ok": true,
        // 保留旧 surface。
        "app": "win32-control",
        // 保留旧 operation。
        "operation": "set-text",
        // 只回显调用方原目标。
        "sessionId": session_id,
        // 历史 helper 未执行回读，不得误报已验证。
        "verifiedByReadback": false,
        // 只返回前景不变布尔值。
        "foreground": { "unchanged": true },
        // 明确该结果属于 native compatibility。
        "compatibilityShape": "legacy-native-standard-edit-v1",
    }))
}

// 执行正式 s2:c 固定写入。
fn set_opaque_text(request: &CommandRequest, session_id: &str) -> AppResult<Value> {
    // confirmation 必须先于参数和目标解析。
    if !request.confirmed {
        // 返回统一确认错误。
        return Err(Win32ControlErrorCode::ConfirmationRequired.error(
            // 明确 mutation 风险。
            "Standard Edit text mutation requires confirmation.",
        ));
    }
    // 读取文本和有界 deadline。
    let (text, timeout_ms) = legacy_input(request)?;
    // 委托正式 Module 并映射旧 timeout 语义。
    let module = standard_edit::set_text(
        // 传入 opaque 目标。
        session_id,
        // 传入 UTF-8 文本。
        text,
        // 传入逐操作确认。
        request.confirmed,
        // 传入有界 deadline。
        timeout_ms,
    )
    // 保留 legacy timeout 兼容码。
    .map_err(map_legacy_failure)?;
    // 构造安全兼容成功结果。
    legacy_success(session_id, &module)
}

// 实现旧 surface 的安全兼容外壳。
impl AppAdapter for Win32ControlAdapter {
    // 返回旧 app ID。
    fn app_id(&self) -> &'static str {
        // 保留兼容 surface。
        "win32-control"
    }

    // 返回正式 Rust Module 状态。
    fn status(&self) -> AppResult<Value> {
        // 获取安全 Module data。
        let data = standard_edit::status()?;
        // 返回旧顶层 envelope 和同一 data。
        Ok(json!({
            // 标记成功。
            "ok": true,
            // 保留旧 app ID。
            "app": self.app_id(),
            // 返回固定 backend 摘要。
            "backend": "Rust fixed SendMessageTimeoutW",
            // 返回后台保证策略。
            "backgroundPolicy": "guaranteed",
            // 返回已认证 operation。
            "certifiedOperations": ["set-text"],
            // 列出明确禁止范围。
            "blocked": ["arbitrary message IDs", "pointer arguments", "focus", "input injection"],
            // 返回安全 Module 状态。
            "data": data,
        }))
    }

    // 返回安全 opaque 控件清单。
    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        // 获取同一 Module 清单。
        let data = standard_edit::sessions(request.max_items)?;
        // 返回旧顶层字段和同一 data。
        Ok(json!({
            // 标记成功。
            "ok": true,
            // 保留旧 app ID。
            "app": self.app_id(),
            // 标记只读。
            "readOnly": true,
            // 复制安全条数。
            "count": data["count"],
            // 复制完整总数。
            "total": data["total"],
            // 复制截断标记。
            "truncated": data["truncated"],
            // 复制安全 sessions。
            "sessions": data["sessions"],
            // 只返回前景不变布尔值。
            "foreground": { "unchanged": true },
            // 返回同一 Module data。
            "data": data,
        }))
    }

    // 精确检查 opaque 控件。
    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        // 读取必需目标。
        let session_id = required_session_id(request)?;
        // 旧 native inspect 不再公开原生记录。
        if is_legacy_native_target(session_id) {
            // 返回迁移要求。
            return Err(Win32ControlErrorCode::TargetIdMigrationRequired.error(
                // 指示调用方重新获取 opaque session。
                "Standard Edit inspection requires a current s2:c opaque target.",
            ));
        }
        // 委托 Module 唯一重新检查。
        let data = standard_edit::inspect(session_id)?;
        // 返回安全兼容 envelope。
        Ok(json!({
            // 标记成功。
            "ok": true,
            // 保留旧 app ID。
            "app": self.app_id(),
            // 标记只读。
            "readOnly": true,
            // 回显 opaque 目标。
            "sessionId": session_id,
            // 返回安全控件观察。
            "control": data["control"],
            // 只返回前景不变布尔值。
            "foreground": { "unchanged": true },
            // 返回同一 Module data。
            "data": data,
        }))
    }

    // 路由唯一认证 operation。
    fn run(&self, request: &CommandRequest) -> AppResult<Value> {
        // 按 operation 分派。
        match request.operation.as_deref() {
            // 执行固定 set-text。
            Some("set-text") => {
                // 读取必需目标。
                let session_id = required_session_id(request)?;
                // 旧 native 目标只进入有限兼容分支。
                if is_legacy_native_target(session_id) {
                    // 执行历史兼容。
                    set_legacy_native_text(request, session_id)
                } else {
                    // 所有新目标必须走正式 opaque Module。
                    set_opaque_text(request, session_id)
                }
            }
            // 未认证 operation fail closed。
            Some(operation) => Err(Win32ControlErrorCode::BackgroundOperationUnavailable.error(
                // 说明未认证范围。
                format!("win32-control.{operation} is not a certified background operation."),
            )),
            // 缺少 operation 返回参数错误。
            None => Err(Win32ControlErrorCode::InvalidArgument.error(
                // 说明必需字段。
                "run requires an operation.",
            )),
        }
    }
}

// 仅在测试构建中验证 mapper 语义。
#[cfg(test)]
mod tests {
    // 导入被测 helper。
    use super::*;

    // 验证 timeout 保留 outcome unknown 并映射旧错误码。
    #[test]
    fn timeout_mapper_preserves_unknown_outcome() {
        // 构造正式 provider timeout。
        let error = Win32ControlErrorCode::Timeout.with_details(
            // 提供稳定消息。
            "timeout",
            // 提供 outcome-unknown 事实。
            json!({
                // 结果未知。
                "outcome": "unknown",
                // 禁止重试。
                "retrySafe": false,
                // 目标可能变更。
                "targetMayHaveMutated": true,
            }),
        );
        // 执行兼容映射。
        let mapped = map_legacy_failure(error);
        // 核对旧错误码。
        assert_eq!(mapped.code, "TARGET_HUNG_OR_UNAVAILABLE");
        // 核对正式 provider 错误码。
        assert_eq!(mapped.details["providerErrorCode"], "TIMEOUT");
        // 核对结果未知。
        assert_eq!(mapped.details["outcome"], "unknown");
        // 核对禁止重试。
        assert_eq!(mapped.details["retrySafe"], false);
    }

    // 验证成功 mapper 不公开 native 标识。
    #[test]
    fn success_mapper_uses_only_opaque_target() -> AppResult<()> {
        // 构造含 native 噪声的 Module 成功结果。
        let module = json!({
            // 提供回读证据。
            "verifiedByReadback": true,
            // 模拟内部 native 字段。
            "hwnd": 42,
            // 模拟内部进程字段。
            "processId": 7,
        });
        // 执行成功映射。
        let mapped = legacy_success("s2:c:1111111111111111", &module)?;
        // 序列化检查递归字段。
        let serialized = mapped.to_string();
        // 核对 opaque 目标。
        assert_eq!(mapped["sessionId"], "s2:c:1111111111111111");
        // 不得公开 HWND。
        assert!(!serialized.contains("hwnd"));
        // 不得公开 PID。
        assert!(!serialized.contains("processId"));
        // 返回成功。
        Ok(())
    }
}
