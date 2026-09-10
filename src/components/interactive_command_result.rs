//! 拥有独立交互会话 command worker 的业务结果状态机与安全投影。

// 导入 provider-neutral JSON 值与构造宏。
use serde_json::{Value, json};

// 导入统一领域错误与固定 command 协议版本。
use crate::{
    // 导入 command worker 固定协议版本。
    components::interactive_command_protocol::CONTRACT_VERSION,
    // 导入 System 公开错误。
    domain::AppControlError,
};

// 删除只属于目标会话内层 System 的执行计划证明。
fn remove_inner_execution_proof(result: &mut serde_json::Map<String, Value>) {
    // 外层 host System 将附加真实跨会话执行域。
    for field in [
        // 删除内层本地执行域。
        "executionRealm",
        // 删除内层 capability 要求域。
        "requiredExecutionRealm",
        // 删除内层标准隔离要求。
        "isolationRequirement",
        // 删除内层本地影响策略。
        "hostImpactPolicy",
        // 删除内层本地执行域认证结论。
        "executionRealmCertified",
    ] {
        // 丢弃可能与外层严格计划冲突的字段。
        result.remove(field);
    }
}

// 构造固定 command 完成响应。
pub(crate) fn completed(request_nonce: &str, mut data: Value) -> Value {
    // 统一 System 成功结果必须保持对象形状。
    let Some(object) = data.as_object_mut() else {
        // 理论漂移在 mutation 后必须保守变为 OutcomeUnknown。
        return outcome_unknown(request_nonce);
    };
    // 删除只在 worker 会话内成立的执行计划证明。
    remove_inner_execution_proof(object);
    // 返回 transport、业务接受与确定完成三层事实。
    json!({
        // 标记命令确定成功。
        "ok": true,
        // 固定 command worker 协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 回显 broker 已验证请求 nonce。
        "requestNonce": request_nonce,
        // 完整协议请求已经接受。
        "transportAccepted": true,
        // mutation 已进入目标会话业务状态机。
        "businessAccepted": true,
        // 最终结果已经确定完成。
        "completed": true,
        // 使用封闭完成结果。
        "outcome": "completed",
        // 已完成 mutation 不允许自动重试。
        "retrySafe": false,
        // 目标已经按命令改变。
        "targetMayHaveMutated": true,
        // 返回不含内层执行计划的领域结果。
        "data": data,
        // 输出目标 worker 内可认证的生命周期证据。
        "evidence": {
            // 已进入目标 session 本地 provider。
            "localProviderInvoked": true,
            // 成功结果证明目标已经唯一解析。
            "targetResolved": true,
            // 成功结果证明 mutation 已 dispatch。
            "mutationDispatched": true,
            // 永远没有回退 host 当前桌面。
            "foregroundFallbackUsed": false
        }
    })
}

// 判断下层错误是否明确发生在业务接受之后。
fn accepted_outcome_unknown(error: &AppControlError) -> bool {
    // 公开码或 details 任一明确 unknown 即采用保守状态。
    error.code == "OUTCOME_UNKNOWN"
        // Window Close 等能力保留原错误码但在 details 声明 unknown。
        || error.details.get("outcome").and_then(Value::as_str) == Some("unknown")
}

// 把四条目标 Module 的前置错误收敛到内部协议白名单。
fn pre_dispatch_code(code: &str) -> &'static str {
    // 只保留跨进程契约登记的 provider-neutral 类别。
    match code {
        // 保留公共参数拒绝。
        "INVALID_ARGUMENT" => "INVALID_ARGUMENT",
        // 保留逐操作确认缺口。
        "CONFIRMATION_REQUIRED" => "CONFIRMATION_REQUIRED",
        // 保留目标会话前景许可缺口。
        "FOREGROUND_CONSENT_REQUIRED" => "FOREGROUND_CONSENT_REQUIRED",
        // 保留严格隔离要求缺口。
        "ISOLATION_REQUIRED" => "ISOLATION_REQUIRED",
        // 保留 capability 目录缺口。
        "CAPABILITY_GAP" => "CAPABILITY_GAP",
        // 保留当前目标状态不支持。
        "CAPABILITY_UNSUPPORTED" => "CAPABILITY_UNSUPPORTED",
        // 保留 endpoint worker 不可用。
        "ISOLATED_WORKER_UNAVAILABLE" => "ISOLATED_WORKER_UNAVAILABLE",
        // 保留 endpoint 认证失败。
        "ENDPOINT_AUTHENTICATION_FAILED" => "ENDPOINT_AUTHENTICATION_FAILED",
        // 保留 stale opaque 目标。
        "STALE_SESSION" => "STALE_SESSION",
        // 保留歧义目标。
        "AMBIGUOUS_TARGET" => "AMBIGUOUS_TARGET",
        // 保留静态权限拒绝。
        "PERMISSION_DENIED" => "PERMISSION_DENIED",
        // 保留静态权限证据缺口。
        "CAPABILITY_ASSESSMENT_UNAVAILABLE" => "CAPABILITY_ASSESSMENT_UNAVAILABLE",
        // 保留写前取消。
        "CANCELLED" => "CANCELLED",
        // 保留写前前景激活失败。
        "FOREGROUND_ACTIVATION_FAILED" => "FOREGROUND_ACTIVATION_FAILED",
        // 保留写前宿主干扰。
        "HOST_INTERFERENCE_DETECTED" => "HOST_INTERFERENCE_DETECTED",
        // 保留写前 deadline。
        "TIMEOUT" => "TIMEOUT",
        // 保留指针精确目标命中失败。
        "POINTER_TARGET_NOT_HIT" => "POINTER_TARGET_NOT_HIT",
        // 保留物理坐标上下文缺口。
        "COORDINATE_CONTEXT_UNAVAILABLE" => "COORDINATE_CONTEXT_UNAVAILABLE",
        // 保留后台关闭不可用。
        "BACKGROUND_OPERATION_UNAVAILABLE" => "BACKGROUND_OPERATION_UNAVAILABLE",
        // 其他目标 Module 错误收敛为不泄漏 provider 的通用失败。
        _ => "OPERATION_FAILED",
    }
}

// 构造确定未 dispatch 的目标 worker 业务拒绝。
fn pre_dispatch_rejection(request_nonce: &str, error: &AppControlError) -> Value {
    // 选择跨进程白名单错误码。
    let code = pre_dispatch_code(error.code);
    // 返回修正请求或重新发现后可安全重试的状态。
    json!({
        // 标记命令失败。
        "ok": false,
        // 固定 command worker 协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 回显已验证请求 nonce。
        "requestNonce": request_nonce,
        // 完整协议请求已经接受。
        "transportAccepted": true,
        // 下层明确没有建立 mutation 接受事实。
        "businessAccepted": false,
        // mutation 未完成。
        "completed": false,
        // 固定未 dispatch 结果。
        "outcome": "not-dispatched",
        // 新请求可在修正前置条件后重试。
        "retrySafe": true,
        // 本请求没有改变目标。
        "targetMayHaveMutated": false,
        // 输出白名单错误对象。
        "error": {
            // 输出稳定错误码。
            "code": code,
            // 不跨进程传播 provider 私有消息。
            "message": "The independent session command was rejected before mutation dispatch."
        },
        // 输出保守的 worker 内阶段证据。
        "evidence": {
            // 请求已交给目标 session 的本地 System/provider pipeline。
            "localProviderInvoked": true,
            // 拒绝结果不声称目标已经成功解析。
            "targetResolved": false,
            // 下层契约明确 mutation 未 dispatch。
            "mutationDispatched": false,
            // 永远没有回退 host 当前桌面。
            "foregroundFallbackUsed": false
        }
    })
}

// 构造业务已接受但最终结果不可认证的固定响应。
fn outcome_unknown(request_nonce: &str) -> Value {
    // 返回不可自动重试的保守状态。
    json!({
        // 标记命令未取得确定成功。
        "ok": false,
        // 固定 command worker 协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 回显已验证请求 nonce。
        "requestNonce": request_nonce,
        // 完整协议请求已经接受。
        "transportAccepted": true,
        // 目标会话 mutation 已建立接受事实。
        "businessAccepted": true,
        // 最终完成无法认证。
        "completed": false,
        // 固定未知结果。
        "outcome": "unknown",
        // 禁止可能重复 mutation 的自动重试。
        "retrySafe": false,
        // 目标可能已经改变。
        "targetMayHaveMutated": true,
        // 输出固定安全错误对象。
        "error": {
            // 使用唯一未知结果错误码。
            "code": "OUTCOME_UNKNOWN",
            // 不跨进程传播平台或 provider 细节。
            "message": "The independent session command outcome could not be certified after acceptance."
        },
        // 输出保守的 accepted 后证据。
        "evidence": {
            // mutation 在目标 session 的本地 provider 内执行。
            "localProviderInvoked": true,
            // accepted 事实建立前已经解析目标。
            "targetResolved": true,
            // 保守视为 mutation 可能已 dispatch。
            "mutationDispatched": true,
            // 永远没有回退 host 当前桌面。
            "foregroundFallbackUsed": false
        }
    })
}

// 把目标会话 System 错误转换为 command worker 生命周期状态。
pub(crate) fn failed(request_nonce: &str, error: &AppControlError) -> Value {
    // accepted 后异常必须优先转换为 OutcomeUnknown。
    if accepted_outcome_unknown(error) {
        // 禁止按原错误码推断未发生。
        return outcome_unknown(request_nonce);
    }
    // 其他四条 Module 错误都由其契约证明发生在 dispatch 前。
    pre_dispatch_rejection(request_nonce, error)
}

// 声明纯结果状态机测试。
#[cfg(test)]
mod tests {
    // 导入被测构造器和统一错误。
    use super::*;

    // 验证 details 中的 unknown 优先于原错误码。
    #[test]
    fn details_unknown_becomes_non_retryable() {
        // 构造 Window Close 风格 TIMEOUT。
        let error = AppControlError::with_details(
            // 保留领域原始码。
            "TIMEOUT",
            // 消息不会跨进程传播。
            "fixture",
            // 显式声明 accepted 后 unknown。
            json!({ "outcome": "unknown", "retrySafe": false }),
        );
        // 构造 worker 响应。
        let value = failed("0123456789abcdef0123456789abcdef", &error);
        // 必须标记业务已接受。
        assert_eq!(value["businessAccepted"], true);
        // 必须禁止自动重试。
        assert_eq!(value["retrySafe"], false);
        // 必须统一为 OutcomeUnknown。
        assert_eq!(value["error"]["code"], "OUTCOME_UNKNOWN");
    }

    // 验证成功投影删除内层执行计划。
    #[test]
    fn completed_result_removes_inner_plan() {
        // 构造带内层执行域的 System 结果。
        let value = completed(
            // 使用 canonical 请求 nonce。
            "0123456789abcdef0123456789abcdef",
            // 提供最小领域结果与内层证明。
            json!({ "capability": "window.close@1", "executionRealm": "same-session-no-focus" }),
        );
        // 领域 capability 必须保留。
        assert_eq!(value["data"]["capability"], "window.close@1");
        // 外层 host System 才能附加真实执行域。
        assert!(value["data"].get("executionRealm").is_none());
        // 完成结果不可自动重试。
        assert_eq!(value["outcome"], "completed");
    }
}
