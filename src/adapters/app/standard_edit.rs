//! `ui.text.input@1` 的标准 Edit provider。

// 把错误码实现保留为当前 Standard Edit provider Adapter 的普通私有类型。
#[path = "standard_edit_error.rs"]
mod error_code;

// 导入 JSON 值与构造工具。
use serde_json::{Value, json};

// 导入当前 Standard Edit provider Adapter 私有封闭错误码。
use error_code::AppStandardEditErrorCode;

// 导入版本化 capability、领域请求和 Standard Edit Module。
use crate::{
    // 读取唯一 capability ID。
    capabilities,
    // 使用稳定领域错误与请求类型。
    domain::{AppResult, CommandRequest},
    // 仅通过 Module 执行领域行为。
    modules::standard_edit,
};

// 导入 app facade provider 接口与精确目标读取工具。
use super::{CapabilityProvider, session::required_session_id};

// 声明无状态 Standard Edit provider。
pub(super) struct StandardEditProvider;

// 固定该 provider 独占的 capability 集合。
const STANDARD_EDIT_CAPABILITIES: &[&str] = &[capabilities::UI_TEXT_INPUT];

// 将 Module 成功事实收窄为 provider-neutral app data。
fn public_apply_result(result: &Value, session_id: &str) -> AppResult<Value> {
    // 读取固定回读验证事实。
    let verified_by_readback = result
        // 读取 Module 字段。
        .get("verifiedByReadback")
        // 只接受布尔值。
        .and_then(Value::as_bool)
        // 缺失证明内部契约失效。
        .ok_or_else(|| {
            // 返回内部执行失败。
            AppStandardEditErrorCode::OperationFailed.error(
                // 不公开内部实现细节。
                "The standard Edit module returned no readback evidence.",
            )
        })?;
    // 读取安全 UTF-8 字节计数。
    let text_bytes = result
        // 读取 Module 字段。
        .get("textBytes")
        // 只接受无符号整数。
        .and_then(Value::as_u64)
        // 缺失证明内部契约失效。
        .ok_or_else(|| {
            // 返回内部执行失败。
            AppStandardEditErrorCode::OperationFailed.error(
                // 不公开文本或 native 事实。
                "The standard Edit module returned no bounded input evidence.",
            )
        })?;
    // 返回 provider-neutral data 与 mapper 元数据。
    Ok(json!({
        // 标记领域状态。
        "state": "applied",
        // 标记认证执行域。
        "executionDomain": "same-session-no-focus",
        // 返回回读验证。
        "verifiedByReadback": verified_by_readback,
        // 返回 UTF-8 字节数而不回显文本。
        "textBytes": text_bytes,
        // 返回前景不变事实。
        "foreground": { "unchanged": true },
        // 供 facade 提升为顶层兼容形状。
        "compatibilityShape": "provider-neutral-standard-edit-v1",
        // 回显调用方已知 opaque 目标供 mapper 核对。
        "targetId": session_id,
    }))
}

// 实现统一 app provider 接口。
impl CapabilityProvider for StandardEditProvider {
    // 返回内部 provider 标识；facade 不公开该值。
    fn provider_id(&self) -> &'static str {
        // 返回稳定内部名称。
        "standard-edit"
    }

    // 返回独占 capability 集合。
    fn capabilities(&self) -> &'static [&'static str] {
        // 返回固定切片。
        STANDARD_EDIT_CAPABILITIES
    }

    // 每次调用重新发现并唯一匹配 opaque 控件。
    fn accepts_session(&self, session_id: &str) -> AppResult<bool> {
        // 委托 Module 执行 fail-closed 匹配。
        standard_edit::accepts_session(session_id)
    }

    // 对精确控件直接返回固定 capability，避免扩大公开清单边界。
    fn session_capabilities(&self, session_id: &str) -> AppResult<Option<Vec<String>>> {
        // 每次调用仍先重新发现并唯一匹配目标。
        if !self.accepts_session(session_id)? {
            // 零命中允许 facade 查询其他 provider。
            return Ok(None);
        }
        // 返回该 provider 独占的固定 capability。
        Ok(Some(vec![capabilities::UI_TEXT_INPUT.to_owned()]))
    }

    // 返回安全只读状态。
    fn status(&self) -> AppResult<Value> {
        // 委托 Module 并保持 provider 私有。
        standard_edit::status()
    }

    // 返回安全控件 session 清单。
    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        // 使用调用方有界条数。
        standard_edit::sessions(request.max_items)
    }

    // 精确检查当前 opaque 控件。
    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        // 读取必需 opaque session。
        let session_id = required_session_id(request)?;
        // 委托 Module 重新发现和检查。
        standard_edit::inspect(session_id)
    }

    // 执行固定 capability mutation。
    fn execute(&self, capability: &str, request: &CommandRequest) -> AppResult<Value> {
        // 拒绝该 provider 未登记的 capability。
        if capability != capabilities::UI_TEXT_INPUT {
            // 返回稳定 capability 错误。
            return Err(AppStandardEditErrorCode::CapabilityUnsupported.error(
                // 明确 provider 边界。
                "The standard Edit session supports ui.text.input@1 only.",
            ));
        }
        // confirmation 必须先于 input、目标和权限解析。
        if !request.confirmed {
            // 返回统一确认错误。
            return Err(AppStandardEditErrorCode::ConfirmationRequired.error(
                // 明确 mutation 风险。
                "Standard Edit text mutation requires confirmation.",
            ));
        }
        // 读取 provider-neutral input 对象。
        let input = request.args.get("input").ok_or_else(|| {
            // 返回参数错误。
            AppStandardEditErrorCode::InvalidArgument.error(
                // 说明必需字段。
                "args.input is required for ui.text.input@1.",
            )
        })?;
        // 验证文本和 timeout。
        let (text, timeout_ms) = standard_edit::provider_input(input)?;
        // 读取精确 opaque 目标。
        let session_id = required_session_id(request)?;
        // 执行唯一目标固定 mutation。
        let result = standard_edit::set_text(
            // 传入 opaque 目标。
            session_id,
            // 传入已验证文本。
            text,
            // 传入逐操作确认。
            request.confirmed,
            // 传入有界 deadline。
            timeout_ms,
        )?;
        // 收窄成功数据并保留原 opaque 目标。
        public_apply_result(&result, session_id)
    }
}

// 仅在测试构建中验证 mapper 不泄漏内部事实。
#[cfg(test)]
mod tests {
    // 导入被测 provider 私有 helper。
    use super::*;

    // 导入不触发真实控件访问的请求 verb。
    use crate::domain::Verb;

    // 验证 app data 只保留领域证据。
    #[test]
    fn public_apply_mapper_hides_provider_and_native_details() -> AppResult<()> {
        // 构造含多余内部事实的 Module 结果。
        let result = json!({
            // 保留回读证据。
            "verifiedByReadback": true,
            // 保留字节数。
            "textBytes": 5,
            // 模拟必须删除的私有字段。
            "sessionId": "s2:c:1111111111111111",
            // 模拟 native 字段。
            "hwnd": 42,
            // 模拟 provider 字段。
            "provider": "rust-win32",
        });
        // 执行 mapper。
        let public = public_apply_result(&result, "s2:c:1111111111111111")?;
        // 序列化便于递归检查。
        let serialized = public.to_string();
        // 核对回读证据。
        assert_eq!(public["verifiedByReadback"], true);
        // 核对原 opaque 目标。
        assert_eq!(public["targetId"], "s2:c:1111111111111111");
        // 不得公开 HWND。
        assert!(!serialized.contains("hwnd"));
        // 不得公开 provider 名。
        assert!(!serialized.contains("rust-win32"));
        // 返回成功。
        Ok(())
    }

    // 验证 capability、确认与 input 门禁在真实控件访问前保持固定顺序。
    #[test]
    fn execution_gates_fail_before_real_control_access() {
        // 构造不含目标或 input 的基础 mutation 请求。
        let request = CommandRequest::read(Verb::Run, "app");
        // 未登记 capability 必须最先失败。
        let unsupported = StandardEditProvider
            // 使用合成未知 capability，禁止进入 Module。
            .execute("ui.text.replace@1", &request)
            // 未登记 capability 不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("unknown capability must fail"));
        // 保持稳定 capability 缺口码。
        assert_eq!(unsupported.code, "CAPABILITY_UNSUPPORTED");
        // 保持既有 capability 缺口消息。
        assert_eq!(
            unsupported.message,
            "The standard Edit session supports ui.text.input@1 only."
        );

        // 已登记但未确认的请求必须在解析 input 与目标前失败。
        let confirmation = StandardEditProvider
            // 使用正式 capability 且保持 confirmed=false。
            .execute(capabilities::UI_TEXT_INPUT, &request)
            // 缺少确认不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("unconfirmed mutation must fail"));
        // 保持稳定确认码。
        assert_eq!(confirmation.code, "CONFIRMATION_REQUIRED");
        // 保持既有确认消息。
        assert_eq!(
            confirmation.message,
            "Standard Edit text mutation requires confirmation."
        );

        // 克隆请求以验证确认后的 input 外壳门禁。
        let mut confirmed = request;
        // 只提供逐操作确认，不提供 input 或目标。
        confirmed.confirmed = true;
        // input 缺失必须在目标发现前失败。
        let invalid_input = StandardEditProvider
            // 使用正式 capability 与已确认请求。
            .execute(capabilities::UI_TEXT_INPUT, &confirmed)
            // 缺少 input 不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("missing input must fail"));
        // 保持稳定参数错误码。
        assert_eq!(invalid_input.code, "INVALID_ARGUMENT");
        // 保持既有 input 缺失消息。
        assert_eq!(
            invalid_input.message,
            "args.input is required for ui.text.input@1."
        );
    }

    // 验证 Module 的不完整成功结果保持 provider 自有失败语义。
    #[test]
    fn public_apply_mapper_rejects_missing_success_evidence() {
        // 缺少 verifiedByReadback 必须失败闭合。
        let missing_readback = public_apply_result(
            // 仅提供有界字节数。
            &json!({ "textBytes": 5 }),
            // 使用调用方已知 opaque 目标。
            "s2:c:1111111111111111",
        )
        // 不完整结果必须返回错误。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("missing readback evidence must fail"));
        // 保持稳定执行失败码。
        assert_eq!(missing_readback.code, "OPERATION_FAILED");
        // 保持既有回读证据消息。
        assert_eq!(
            missing_readback.message,
            "The standard Edit module returned no readback evidence."
        );

        // 缺少 textBytes 同样必须失败闭合。
        let missing_text_bytes = public_apply_result(
            // 仅提供回读验证事实。
            &json!({ "verifiedByReadback": true }),
            // 使用调用方已知 opaque 目标。
            "s2:c:1111111111111111",
        )
        // 不完整结果必须返回错误。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("missing bounded input evidence must fail"));
        // 保持稳定执行失败码。
        assert_eq!(missing_text_bytes.code, "OPERATION_FAILED");
        // 保持既有字节证据消息。
        assert_eq!(
            missing_text_bytes.message,
            "The standard Edit module returned no bounded input evidence."
        );
    }
}
