//! 独立交互会话 host client 的结果语义回归测试。

// 导入统一错误类型。
use crate::domain::AppControlError;

// 只测试父模块的私有结果映射函数。
use super::{outcome_unknown, pre_dispatch_error, request_deadline};

// 验证 dispatch 前失败明确声明零副作用且可重试。
#[test]
fn pre_dispatch_failure_is_retry_safe_and_side_effect_free() {
    // 构造一个已净化的 endpoint 缺失错误。
    let error = AppControlError::new(
        // 使用生产端点缺失码。
        "ISOLATED_WORKER_UNAVAILABLE",
        // 使用不含内部状态的固定消息。
        "The endpoint is unavailable.",
    );
    // 附加 dispatch 前生命周期证据。
    let error = pre_dispatch_error(error);
    // 原始稳定错误码必须保留。
    assert_eq!(error.code, "ISOLATED_WORKER_UNAVAILABLE");
    // mutation 尚未进入业务状态机。
    assert_eq!(error.details["businessAccepted"], false);
    // 请求没有确定完成。
    assert_eq!(error.details["completed"], false);
    // 结果明确处于未派发状态。
    assert_eq!(error.details["outcome"], "not-dispatched");
    // 修复 endpoint 后可以安全重试。
    assert_eq!(error.details["retrySafe"], true);
    // 目标不可能因本请求改变。
    assert_eq!(error.details["targetMayHaveMutated"], false);
    // host 本地 provider 从未调用。
    assert_eq!(error.details["localProviderInvoked"], false);
    // 当前桌面 fallback 从未启用。
    assert_eq!(error.details["foregroundFallbackUsed"], false);
}

// 验证 command 发送后的断连统一收敛为不可自动重试的未知结果。
#[test]
fn post_dispatch_failure_is_outcome_unknown() {
    // 构造业务可能已接受后的保守结果。
    let error = outcome_unknown();
    // 使用固定 OutcomeUnknown 错误码。
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    // command 已越过业务接受边界。
    assert_eq!(error.details["businessAccepted"], true);
    // host 无法认证确定完成。
    assert_eq!(error.details["completed"], false);
    // 结果明确标记未知。
    assert_eq!(error.details["outcome"], "unknown");
    // mutation 不得自动重试。
    assert_eq!(error.details["retrySafe"], false);
    // 保守声明目标可能改变。
    assert_eq!(error.details["targetMayHaveMutated"], true);
    // host 本地 provider 从未调用。
    assert_eq!(error.details["localProviderInvoked"], false);
    // 当前桌面 fallback 从未启用。
    assert_eq!(error.details["foregroundFallbackUsed"], false);
}

// 验证总 deadline 拒绝零预算并接受协议上限内预算。
#[test]
fn request_deadline_requires_positive_budget() {
    // 零预算必须在连接前失败。
    let error = match request_deadline(0) {
        // 成功将违反总预算不变量。
        Ok(_) => panic!("zero timeout unexpectedly produced a deadline"),
        // 保存预期参数错误。
        Err(error) => error,
    };
    // 使用固定参数错误码。
    assert_eq!(error.code, "INVALID_ARGUMENT");
    // 正预算必须构造出未来 deadline。
    assert!(request_deadline(1_000).is_ok());
}
