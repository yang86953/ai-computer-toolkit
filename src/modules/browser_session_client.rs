//! 通过固定 Rust broker 投影公开浏览器会话生命周期结果。

// 把页面导航与只读 Query 保留在同一 client Module 的窄子文件。
#[path = "browser_session_page_client.rs"]
mod page;
// 向 App provider 公开同一 Module 的三个页面入口。
pub(crate) use page::{navigate, query, wait};
// 把元素动作与页面截图保留在同一 client Module 的独立窄子文件。
#[path = "browser_session_page_action_client.rs"]
mod page_action;
// 向 App provider 公开同一 Module 的三项动作入口。
pub(crate) use page_action::{click, screenshot, type_text};

// 导入 JSON 构造器与值类型。
use serde_json::{Value, json};

// 导入固定本机 broker Adapter。
use crate::adapters::browser_session_broker_windows;
// 导入严格公开输入与浏览器会话身份分类 Component。
use crate::components::{
    browser_session_identity::{BrowserSessionIdentityShape, classify_browser_session_id},
    browser_session_lifecycle_error::canonical_target_for_capability,
    browser_session_lifecycle_input::{
        DEFAULT_TIMEOUT_MS as INPUT_DEFAULT_TIMEOUT_MS, parse_browser_session_lifecycle_input,
    },
};
// 导入稳定 capability ID 与统一错误边界。
use crate::{
    capabilities,
    domain::{AppControlError, AppResult},
};

// 向 assessment 与 App provider 公开同源的默认总预算。
pub(crate) const DEFAULT_TIMEOUT_MS: u32 = INPUT_DEFAULT_TIMEOUT_MS;

// 构造确认缺失时的统一公开错误。
pub(super) fn confirmation_required(
    // 接收稳定公开 capability ID。
    capability: &'static str,
    // 接收调用方已经提供的公开 target。
    target_id: &str,
) -> AppControlError {
    // 保持 Policy 同源的公开确认错误码。
    lifecycle_error(
        // 未确认不得连接 broker。
        "CONFIRMATION_REQUIRED",
        // 不回显目标或输入。
        "Browser session lifecycle operations require explicit confirmation.",
        // 回显公开 capability。
        capability,
        // 回显调用方已知 target。
        target_id,
        // 确认缺失可证明尚未派发。
        "not-dispatched",
        // 未越过业务接受点。
        false,
        // 确认拒绝是可信终态。
        true,
        // 补齐确认后可人工重试。
        true,
        // 未派发不会改变生命周期。
        false,
    )
}

// 为公开 lifecycle 错误附加不泄漏私有 broker 字段的最小事实。
pub(super) fn lifecycle_error(
    // 接收已登记的公开错误码。
    code: &'static str,
    // 接收不泄漏内部实现的安全错误说明。
    message: impl Into<String>,
    // 接收公开 capability。
    capability: &'static str,
    // 接收调用方已知 target。
    target_id: &str,
    // 接收最后可证明的 outcome。
    outcome: &'static str,
    // 接收业务是否已可证明接受。
    accepted: bool,
    // 接收是否已有可信终态。
    final_state_reached: bool,
    // 接收调用方能否安全人工重试。
    retry_safe: bool,
    // 接收目标是否可能已改变。
    target_may_have_mutated: bool,
) -> AppControlError {
    // 基础详情始终只回显公开 capability 与业务事实。
    let mut details = json!({
        // 回显固定 capability。
        "capability": capability,
        // 输出封闭 outcome。
        "outcome": outcome,
        // 输出已知接受事实。
        "accepted": accepted,
        // 输出可信终态事实。
        "finalStateReached": final_state_reached,
        // 输出人工重试安全性。
        "retrySafe": retry_safe,
        // 输出保守 mutation 事实。
        "targetMayHaveMutated": target_may_have_mutated,
    });
    // 仅在 capability 与 target 均 canonical 且同 kind 时回显身份。
    if let Some(target_id) = canonical_target_for_capability(capability, target_id) {
        // JSON 宏保证 details 为对象。
        if let Some(object) = details.as_object_mut() {
            // 回显 caller 已知且经同源认证的不透明 target。
            object.insert("targetId".to_owned(), Value::String(target_id.to_owned()));
        }
    }
    // 任何不可安全重试的生命周期错误都必须禁止自动重派。
    if !retry_safe {
        // JSON 宏保证 details 为对象。
        if let Some(object) = details.as_object_mut() {
            // 固定标记自动重试禁止。
            object.insert("automaticRetryProhibited".to_owned(), Value::Bool(true));
        }
    }
    // 返回不包含 nonce、epoch、pipe、worker 或 native 事实的错误。
    AppControlError::with_details(code, message, details)
}

// 把固定 broker 的封闭错误收敛到公开 lifecycle 语义。
pub(super) fn project_broker_error(
    // 接收 broker Adapter 已过滤的安全错误。
    error: AppControlError,
    // 接收公开 capability。
    capability: &'static str,
    // 接收调用方已知 target。
    target_id: &str,
) -> AppControlError {
    // 读取 Adapter 从 strict final 保留的业务接受事实。
    let accepted = error
        // 访问封闭安全详情。
        .details
        // 读取业务接受布尔值。
        .get("businessAccepted")
        // 只接受 JSON boolean。
        .and_then(Value::as_bool)
        // 写后无法恢复时必须按可能已接受处理。
        .unwrap_or(error.code == "OUTCOME_UNKNOWN");
    // 读取是否已经取得可信终态。
    let final_state_reached = error
        // 访问封闭安全详情。
        .details
        // 读取 completed 事实。
        .get("completed")
        // 只接受 JSON boolean。
        .and_then(Value::as_bool)
        // 写前错误本身就是可信未派发终态。
        .unwrap_or(error.code != "OUTCOME_UNKNOWN");
    // 读取协议冻结的重试事实。
    let retry_safe = error
        // 访问封闭安全详情。
        .details
        // 读取 retrySafe。
        .get("retrySafe")
        // 只接受 JSON boolean。
        .and_then(Value::as_bool)
        // 只有明确未接受的本地失败默认可人工重试。
        .unwrap_or(!accepted);
    // 读取保守目标 mutation 事实。
    let target_may_have_mutated = error
        // 访问封闭安全详情。
        .details
        // 读取 targetMayHaveMutated。
        .get("targetMayHaveMutated")
        // 只接受 JSON boolean。
        .and_then(Value::as_bool)
        // 缺失事实时只对已接受 mutation 保守认为可能变化。
        .unwrap_or_else(|| {
            // 从单一 registry 读取 capability 副作用事实。
            accepted
                // 未登记 ID 不得被默认放宽为 mutation=false。
                && capabilities::definition(capability)
                    // 使用强类型 action 判断副作用。
                    .is_none_or(|definition| definition.action.mutates())
        });
    // 读取 strict final 或本地恢复投影的未知结果事实。
    let outcome_unknown = error
        // 访问封闭安全详情。
        .details
        // 读取 outcomeUnknown。
        .get("outcomeUnknown")
        // 只接受 JSON boolean。
        .and_then(Value::as_bool)
        // 保留显式未知错误码的防御性含义。
        .unwrap_or(error.code == "OUTCOME_UNKNOWN");
    // 把私有错误码收敛到已登记公共集合。
    let public_code = match error.code {
        // 私有过期码只能投影为公共 timeout。
        "REQUEST_EXPIRED" => "TIMEOUT",
        // 私有通用业务失败只有在 accepted 后才能投影为公共操作失败。
        "BROKER_OPERATION_FAILED" if accepted => "OPERATION_FAILED",
        // 业务接受前的私有 broker 失败只能收敛为固定路线不可用。
        "BROKER_OPERATION_FAILED" => "BROKER_UNAVAILABLE",
        // 认证和协议内部错误不得形成新公开码。
        "ENDPOINT_AUTHENTICATION_FAILED" | "BROKER_PROTOCOL_FAILED" => "BROKER_UNAVAILABLE",
        // 这些代码已由公共 error envelope 登记，可原样保留。
        "STALE_SESSION"
        | "STALE_PAGE"
        | "STALE_ELEMENT"
        | "BROWSER_SESSION_REGISTRY_FULL"
        | "BROKER_UNAVAILABLE"
        | "CAPABILITY_UNAVAILABLE"
        | "ISOLATED_WORKER_UNAVAILABLE"
        | "CANCELLED"
        | "TIMEOUT"
        | "INVALID_ARGUMENT"
        | "OUTCOME_UNKNOWN" => error.code,
        // 其他动态或私有码在 accepted 后统一收敛为可信操作失败。
        _ if accepted => "OPERATION_FAILED",
        // 接受前无法解释的私有码不得伪造业务失败。
        _ => "BROKER_UNAVAILABLE",
    };
    // 从最后可信事实选择公开 outcome。
    let outcome = if outcome_unknown {
        // 缺少可信 final 时只能公开 unknown。
        "unknown"
    } else if accepted {
        // 已接受的可信失败公开 failed。
        "failed"
    } else {
        // 业务前错误公开 not-dispatched。
        "not-dispatched"
    };
    // 为收敛错误码选择不泄漏实现的固定说明。
    let message = match public_code {
        // stale 只说明公开身份不可用。
        "STALE_SESSION" => "The browser session is stale or no longer available.",
        // stale page 只说明公开页面 identity 已过期。
        "STALE_PAGE" => "The browser page is stale or no longer current.",
        // stale element 只说明公开元素 identity 已过期。
        "STALE_ELEMENT" => "The browser element is stale or no longer current.",
        // timeout 不区分内部阶段。
        "TIMEOUT" => "The browser session request timed out.",
        // unknown 明确禁止猜测 final。
        "OUTCOME_UNKNOWN" => "The browser session operation outcome is unknown after acceptance.",
        // 容量错误只说明固定公开上限。
        "BROWSER_SESSION_REGISTRY_FULL" => "The browser session registry is full.",
        // 取消保持中性请求语义。
        "CANCELLED" => "The browser session request was cancelled before dispatch.",
        // 固定路线缺口不公开认证或文件细节。
        "BROKER_UNAVAILABLE" | "CAPABILITY_UNAVAILABLE" | "ISOLATED_WORKER_UNAVAILABLE" => {
            "The fixed browser session route is unavailable."
        }
        // 输入错误不回显原 JSON。
        "INVALID_ARGUMENT" => "The browser session request is invalid.",
        // 其余可信失败保持公共操作失败。
        _ => "The browser session operation could not be completed.",
    };
    // 输出统一且不含私有 broker 字段的生命周期错误。
    lifecycle_error(
        // 使用已登记公共错误码。
        public_code,
        // 使用固定安全说明。
        message,
        // 回显公开 capability。
        capability,
        // 回显调用方已知 target。
        target_id,
        // 输出最后可信 outcome。
        outcome,
        // 输出业务接受事实。
        accepted,
        // 输出可信终态事实。
        final_state_reached,
        // 输出安全重试事实。
        retry_safe,
        // 输出保守 mutation 事实。
        target_may_have_mutated,
    )
}

// 为确认、身份和输入等本地预检错误附加未派发真值。
pub(super) fn project_pre_dispatch_error(
    // 接收本地已证明尚未接触 broker 的错误。
    error: AppControlError,
    // 接收公开 capability。
    capability: &'static str,
    // 接收调用方已知 target。
    target_id: &str,
) -> AppControlError {
    // 保留已登记公开码与安全消息，并补全未派发事实。
    lifecycle_error(
        // 错误来自静态公开集合。
        error.code,
        // 将拥有型消息作为临时只读借用。
        &error.message,
        // 回显 capability。
        capability,
        // 回显 target。
        target_id,
        // 本地预检从未派发。
        "not-dispatched",
        // 未越过业务接受点。
        false,
        // 本地拒绝是可信终态。
        true,
        // 调用方修正输入后可人工重试。
        true,
        // 本地预检不改变目标。
        false,
    )
}

// 验证 browser session 身份可安全进入固定 broker。
pub(super) fn require_canonical_browser_session(session_id: &str) -> AppResult<()> {
    // 按独立 Component 的封闭身份形状选择公开错误。
    match classify_browser_session_id(session_id) {
        // canonical 身份后续仍由 broker 验证 live 代际。
        BrowserSessionIdentityShape::Canonical => Ok(()),
        // 同前缀畸形输入属于调用外壳错误，且不得触碰 broker。
        BrowserSessionIdentityShape::Malformed => Err(AppControlError::new(
            // 使用公开输入错误码。
            "INVALID_ARGUMENT",
            // 不回显原始身份文本。
            "The browser session target is invalid.",
        )),
        // 其他 opaque kind 不是 Browser Session，属于调用外壳错误。
        BrowserSessionIdentityShape::NotBrowserSession => Err(AppControlError::new(
            // 使用同一公开输入错误码。
            "INVALID_ARGUMENT",
            // 不泄漏其他 provider 的目标分类。
            "The browser session target is invalid.",
        )),
    }
}

// 打开新 browser session 并投影冻结公开成功 data。
pub(crate) fn open(
    // 接收 facade 已传递的逐操作确认事实。
    confirmed: bool,
    // 接收可选公开 input。
    input: Option<&Value>,
    // 接收当前 host 公开 target 供错误回显。
    target_id: &str,
) -> AppResult<Value> {
    // confirmation 必须先于 input 解析和 broker 启动。
    if !confirmed {
        // 未确认不得读取 input 或连接 broker。
        return Err(confirmation_required(
            // 传递固定 open capability。
            capabilities::BROWSER_SESSION_OPEN,
            // 回显原 host target。
            target_id,
        ));
    }
    // 严格解析唯一公开 input 并固定总预算。
    let input = parse_browser_session_lifecycle_input(input).map_err(|error| {
        // input 在 broker 前解析失败，必须保持未派发真值。
        project_pre_dispatch_error(error, capabilities::BROWSER_SESSION_OPEN, target_id)
    })?;
    // 调用固定 Rust broker，不允许 caller 选择任何 transport。
    let session_id = browser_session_broker_windows::open_confirmed(input.timeout_ms())
        // 将私有错误收敛到公开 lifecycle 结果。
        .map_err(|error| {
            project_broker_error(error, capabilities::BROWSER_SESSION_OPEN, target_id)
        })?;
    // 只在可信 completed final 后输出冻结成功 data。
    Ok(json!({
        // 回显固定 capability。
        "capability": capabilities::BROWSER_SESSION_OPEN,
        // 输出固定领域动作。
        "action": "open",
        // 输出可信完成。
        "outcome": "completed",
        // 输出固定 dispatch 终态。
        "dispatchState": "completed",
        // 成功必定已越过业务接受点。
        "accepted": true,
        // 成功必定已获得可信 final。
        "finalStateReached": true,
        // 生命周期动作保守视为可能变更目标。
        "targetMayHaveMutated": true,
        // 新身份只有在 Module 持有完整资源后发布。
        "state": "live",
        // 只输出 broker 签发的 opaque identity。
        "sessionId": session_id,
        // 固定路线不改变 host 前景。
        "foregroundUnchanged": true,
        // confirmation-first 已在 dispatch 前完成。
        "confirmationEvaluatedBeforeDispatch": true,
        // 无需前景同意的策略仍已完成评估。
        "foregroundConsentEvaluatedBeforeDispatch": true,
        // 成功 mutation 不能安全重试。
        "retrySafe": false,
        // 禁止公共 Adapter 自动重派。
        "automaticRetryProhibited": true,
    }))
}

// 关闭当前 live browser session 并投影冻结公开成功 data。
pub(crate) fn close(
    // 接收 facade 已传递的逐操作确认事实。
    confirmed: bool,
    // 接收可选公开 input。
    input: Option<&Value>,
    // 接收调用方提供的公开 browser session identity。
    session_id: &str,
) -> AppResult<Value> {
    // confirmation 必须先于 target 形状、input 与 broker 访问。
    if !confirmed {
        // 未确认不得读取 target 或连接 broker。
        return Err(confirmation_required(
            // 传递固定 close capability。
            capabilities::BROWSER_SESSION_CLOSE,
            // 回显原 session target。
            session_id,
        ));
    }
    // 非 generic 身份在 broker 前失败闭合。
    require_canonical_browser_session(session_id).map_err(|error| {
        // identity 在 broker 前拒绝，补齐未派发真值。
        project_pre_dispatch_error(error, capabilities::BROWSER_SESSION_CLOSE, session_id)
    })?;
    // 严格解析唯一公开 input 并固定总预算。
    let input = parse_browser_session_lifecycle_input(input).map_err(|error| {
        // input 在 broker 前解析失败，必须保持未派发真值。
        project_pre_dispatch_error(error, capabilities::BROWSER_SESSION_CLOSE, session_id)
    })?;
    // 只经固定 Rust broker 调用 confirmed close。
    browser_session_broker_windows::close_confirmed(session_id, input.timeout_ms())
        // 将私有错误收敛到公开 lifecycle 结果。
        .map_err(|error| {
            project_broker_error(error, capabilities::BROWSER_SESSION_CLOSE, session_id)
        })?;
    // 只在 Module 已完成逆序回收后输出冻结 close data。
    Ok(json!({
        // 回显固定 capability。
        "capability": capabilities::BROWSER_SESSION_CLOSE,
        // 输出固定领域动作。
        "action": "close",
        // 输出可信完成。
        "outcome": "completed",
        // 输出固定 dispatch 终态。
        "dispatchState": "completed",
        // 成功必定已越过业务接受点。
        "accepted": true,
        // 成功必定已获得可信 final。
        "finalStateReached": true,
        // 生命周期动作保守视为可能变更目标。
        "targetMayHaveMutated": true,
        // 关闭后不泄漏或重复 session identity。
        "state": "closed",
        // 只表达 Module 已证明完成回收。
        "closed": true,
        // 固定路线不改变 host 前景。
        "foregroundUnchanged": true,
        // confirmation-first 已在 dispatch 前完成。
        "confirmationEvaluatedBeforeDispatch": true,
        // 无需前景同意的策略仍已完成评估。
        "foregroundConsentEvaluatedBeforeDispatch": true,
        // 成功 mutation 不能安全重试。
        "retrySafe": false,
        // 禁止公共 Adapter 自动重派。
        "automaticRetryProhibited": true,
    }))
}

// 查询浏览器 session 的 live 新鲜度，不改变 worker 或 Module 所有权。
pub(crate) fn inspect_session(session_id: &str, timeout_ms: u32) -> AppResult<()> {
    // 严格拒绝 generic opaque identity。
    require_canonical_browser_session(session_id)?;
    // Query 不带 confirmation，直接复用固定 broker 的只读 inspect。
    browser_session_broker_windows::inspect_session(session_id, timeout_ms)
}

// 验证 client Module 的 confirmation、identity 与错误收敛边界。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造器。
    use serde_json::json;

    // 导入稳定 capability。
    use crate::capabilities;

    // 导入内部错误投影与公开入口。
    use super::{close, open, project_broker_error};

    // 验证 browser_session_public_route 确认优先于 input 与 broker。
    #[test]
    fn browser_session_public_route_confirmation_precedes_input_validation() {
        // 未确认且非法 input 必须首先返回 confirmation。
        let error = open(
            false,
            Some(&json!({ "pipe": "forbidden" })),
            "s2:h:0123456789abcdef",
        )
        // 未确认请求不得成功。
        .expect_err("unconfirmed open must fail");
        // 错误必须保留确认优先语义。
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    // 验证 browser_session_public_route 非 generic target 在 broker 前拒绝。
    #[test]
    fn browser_session_public_route_close_rejects_generic_opaque_target() {
        // 已确认 close 使用普通窗口身份。
        let error = close(true, None, "s2:w:0123456789abcdef")
            // 不得将窗口交给 browser broker。
            .expect_err("non-browser target must fail");
        // wrong-kind target 属于公开输入错误。
        assert_eq!(error.code, "INVALID_ARGUMENT");
        // 同前缀畸形身份必须作为输入错误在 broker 前拒绝。
        let malformed = close(true, None, "s2:bs:ABCDEF")
            // 畸形目标不得进入 broker。
            .expect_err("malformed browser target must fail");
        // 使用冻结的公开输入错误码。
        assert_eq!(malformed.code, "INVALID_ARGUMENT");
        // 本地拒绝必须保留未派发真值。
        assert_eq!(malformed.details["accepted"], false);
        // 畸形身份不得被错误详情重新公开为 canonical target。
        assert!(malformed.details.get("targetId").is_none());
    }

    // 验证 browser_session_public_route broker 私有 deadline 只投影公开 timeout。
    #[test]
    fn browser_session_public_route_projects_private_deadline_to_timeout() {
        // 构造 broker Adapter 私有错误。
        let error = project_broker_error(
            // 模拟 broker final 投影。
            crate::domain::AppControlError::new("REQUEST_EXPIRED", "private"),
            // 使用 close capability。
            capabilities::BROWSER_SESSION_CLOSE,
            // 使用公开 target。
            "s2:bs:0123456789abcdef0123456789abcdef",
        );
        // 私有错误码不得离开 Module。
        assert_eq!(error.code, "TIMEOUT");
        // 已证明未派发才允许重试。
        assert_eq!(error.details["accepted"], false);
        // 不得标记 target 可能变化。
        assert_eq!(error.details["targetMayHaveMutated"], false);
    }

    // 验证 browser_session_public_route 已接受失败保留不可重试事实。
    #[test]
    fn browser_session_public_route_preserves_accepted_failure_truth() {
        // 构造来自 strict final 的安全私有布尔事实。
        let broker_error = crate::domain::AppControlError::with_details(
            // 使用私有通用 broker 失败码。
            "BROKER_OPERATION_FAILED",
            // 私有说明不得进入公共输出。
            "private broker failure",
            // 只模拟 Adapter 保留的封闭事实。
            json!({
                // 已经越过业务接受点。
                "businessAccepted": true,
                // 已取得可信失败终态。
                "completed": true,
                // 已接受 mutation 不可重试。
                "retrySafe": false,
                // 终态不是未知结果。
                "outcomeUnknown": false,
                // 目标可能已经改变。
                "targetMayHaveMutated": true,
            }),
        );
        // 投影到 public close 错误。
        let error = project_broker_error(
            // 传递私有 final。
            broker_error,
            // 绑定公开 close capability。
            capabilities::BROWSER_SESSION_CLOSE,
            // 绑定调用方已知 target。
            "s2:bs:0123456789abcdef0123456789abcdef",
        );
        // 动态私有码必须收敛为公共操作失败。
        assert_eq!(error.code, "OPERATION_FAILED");
        // 已接受事实必须保留。
        assert_eq!(error.details["accepted"], true);
        // 可信失败终态必须保留。
        assert_eq!(error.details["finalStateReached"], true);
        // 已接受失败不得安全重试。
        assert_eq!(error.details["retrySafe"], false);
        // 自动重派必须禁止。
        assert_eq!(error.details["automaticRetryProhibited"], true);
        // 私有说明不得回显。
        assert!(!error.message.contains("private"));
    }

    // 验证 browser_session_public_route 接受前私有失败不会冒充 accepted 操作失败。
    #[test]
    fn browser_session_public_route_preaccept_private_failure_is_route_unavailable() {
        // 构造不带 accepted 事实的私有 broker 错误。
        let error = project_broker_error(
            // 使用只允许留在 Adapter 与 Module 间的错误码。
            crate::domain::AppControlError::new("BROKER_OPERATION_FAILED", "private"),
            // 绑定公开 close capability。
            capabilities::BROWSER_SESSION_CLOSE,
            // 绑定 canonical caller target。
            "s2:bs:0123456789abcdef0123456789abcdef",
        );
        // 接受前私有失败必须收敛为固定路线不可用。
        assert_eq!(error.code, "BROKER_UNAVAILABLE");
        // 路线失败明确未越过业务接受点。
        assert_eq!(error.details["accepted"], false);
        // 未派发失败不得声称目标可能改变。
        assert_eq!(error.details["targetMayHaveMutated"], false);
        // 私有消息不得进入公开输出。
        assert!(!error.message.contains("private"));
    }
}
