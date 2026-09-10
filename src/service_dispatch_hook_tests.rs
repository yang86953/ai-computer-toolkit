//! 验证 ComputerControlSystem 私有 dispatch hook 的顺序与单次性。

// 导入原子计数器。
use std::sync::atomic::{AtomicUsize, Ordering};

// 导入统一错误、请求与动词。
use crate::domain::{AppControlError, CommandRequest, Verb};
// 导入公开 Browser Session close capability。
use crate::capabilities;

// 导入被测 System。
use super::AppControlService;

// 验证未知 provider 在 hook 之前失败。
#[test]
fn unresolved_provider_never_triggers_dispatch_hook() {
    // 创建完整生产 registry 的 System。
    let service = AppControlService::new();
    // 保存 hook 调用次数。
    let calls = AtomicUsize::new(0);
    // 执行无法解析的只读 provider。
    let result = service.execute_with_dispatch_hook(
        // 使用不会触碰精确目标的状态请求。
        CommandRequest::read(Verb::Status, "missing-sequence-step-provider"),
        // 记录理论上的 hook 调用。
        || {
            // 增加观察次数。
            calls.fetch_add(1, Ordering::Relaxed);
            // 允许后续调用继续。
            Ok(())
        },
    );
    // provider 解析必须失败。
    assert!(result.is_err());
    // 未解析 provider 不得产生 accepted 事实。
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

// 验证已解析 provider 只触发一次 hook 且 hook 可以阻止领域调用。
#[test]
fn resolved_provider_triggers_one_fail_closed_dispatch_hook() {
    // 创建完整生产 registry 的 System。
    let service = AppControlService::new();
    // 保存 hook 调用次数。
    let calls = AtomicUsize::new(0);
    // 对稳定 desktop provider 执行状态请求。
    let result = service.execute_with_dispatch_hook(
        // 状态请求不需要目标或确认。
        CommandRequest::read(Verb::Status, "desktop"),
        // 用结构化失败证明 hook 位于 adapter 调用前。
        || {
            // 记录唯一调用。
            calls.fetch_add(1, Ordering::Relaxed);
            // 阻止任何后续领域调用。
            Err(AppControlError::new(
                // 使用测试专用稳定错误码。
                "DISPATCH_HOOK_STOPPED",
                // 使用不含平台事实的消息。
                "The dispatch hook stopped the request before provider invocation.",
            ))
        },
    );
    // hook 失败必须原样成为 System 结果。
    let error = match result {
        // 保留观察器错误。
        Err(error) => error,
        // 成功表示 provider 绕过了失败闭合观察器。
        Ok(_) => panic!("dispatch hook unexpectedly allowed the provider"),
    };
    // 错误码证明失败来自观察器。
    assert_eq!(error.code, "DISPATCH_HOOK_STOPPED");
    // 每个请求只允许一次 accepted 观察。
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

// 验证 browser_session_public_route 的 System 前置错误始终携带未派发真值。
#[test]
fn browser_session_public_route_system_errors_include_pre_dispatch_truth() {
    // 创建完整生产 registry 的 System。
    let service = AppControlService::new();
    // 从统一 App run 请求开始。
    let mut request = CommandRequest::read(Verb::Run, "app");
    // 使用冻结 generic close operation。
    request.operation = Some("close".to_owned());
    // 写入 Browser Session close capability。
    request.args.insert(
        // 使用统一 capability 字段。
        "capability".to_owned(),
        // 写入稳定公开 ID。
        serde_json::json!(capabilities::BROWSER_SESSION_CLOSE),
    );
    // 写入 caller 已知 target，确认错误不得提前验证其形状。
    request.target.insert(
        // 使用统一目标字段。
        "sessionId".to_owned(),
        // 使用 canonical 测试目标。
        serde_json::json!("s2:bs:0123456789abcdef0123456789abcdef"),
    );
    // 未确认请求必须在 provider 之前失败。
    let error = service
        // 执行唯一生产 System 路径。
        .execute(request)
        // 取得预期确认错误。
        .expect_err("unconfirmed browser session close must fail");
    // 确认门禁保持公开错误码。
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    // 错误必须绑定稳定 capability。
    assert_eq!(
        error.details["capability"],
        capabilities::BROWSER_SESSION_CLOSE
    );
    // provider 前失败明确未接受。
    assert_eq!(error.details["accepted"], false);
    // 未派发拒绝是可信终态。
    assert_eq!(error.details["finalStateReached"], true);
    // 下层 Policy 诊断不得进入公共详情。
    assert!(error.details.get("availableOperations").is_none());
}

// 验证 lifecycle capability 在错配 generic verb 时仍保持 confirmation-first 详情。
#[test]
fn browser_session_public_route_wrong_operation_keeps_confirmation_details() {
    // 创建完整生产 registry 的 System。
    let service = AppControlService::new();
    // 从统一 App run 请求开始。
    let mut request = CommandRequest::read(Verb::Run, "app");
    // 故意使用与 open capability 不匹配的 close operation。
    request.operation = Some("close".to_owned());
    // 写入明确的 Browser Session open capability。
    request.args.insert(
        // 使用统一 capability 字段。
        "capability".to_owned(),
        // 写入稳定公开 ID。
        serde_json::json!(capabilities::BROWSER_SESSION_OPEN),
    );
    // 未确认请求必须在 operation 配对和 target 解析前失败。
    let error = service
        // 执行唯一生产 System 编排。
        .execute(request)
        // 取得 confirmation-first 错误。
        .expect_err("unconfirmed mismatched lifecycle request must fail");
    // Policy 必须先返回逐操作确认要求。
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    // 错配 verb 不得丢失 caller 明确提供的 capability。
    assert_eq!(
        error.details["capability"],
        capabilities::BROWSER_SESSION_OPEN
    );
    // provider 前错误必须保持未派发状态。
    assert_eq!(error.details["outcome"], "not-dispatched");
    // 缺失 target 时不得捏造 target identity。
    assert!(error.details.get("targetId").is_none());
    // Policy 私有目录详情不得离开 lifecycle envelope。
    assert!(error.details.get("availableOperations").is_none());
}
