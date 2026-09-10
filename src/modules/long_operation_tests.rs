//! 验证长操作状态机的合法迁移、取消竞争与恢复语义。

// 导入当前 Module 的私有测试接口。
use super::*;

// 验证正常执行只能从 accepted 经 running 到 completed。
#[test]
// 覆盖 dispatch 事实点与成功终态。
fn completes_only_after_dispatch() -> Result<(), LongOperationTransitionError> {
    // 构造 broker 已接受的新任务。
    let mut state = LongOperationState::accepted();
    // 初始状态不得声称已 dispatch。
    assert!(!state.dispatch_started());
    // dispatch 前完成必须被拒绝。
    let error = match state.complete() {
        // 保留预期的非法迁移。
        Err(error) => error,
        // 成功意味着 dispatch 门禁回归。
        Ok(()) => panic!("accepted task unexpectedly completed"),
    };
    // 核对拒绝前状态。
    assert_eq!(error.from(), LongOperationStatus::Accepted);
    // 核对稳定动作名称。
    assert_eq!(error.action(), "complete");
    // 开始 worker dispatch。
    state.start_dispatch()?;
    // 核对 running 状态。
    assert_eq!(state.status(), LongOperationStatus::Running);
    // 核对 dispatch 事实。
    assert!(state.dispatch_started());
    // 使用 worker 成功证据终结任务。
    state.complete()?;
    // 核对成功终态。
    assert_eq!(state.status(), LongOperationStatus::Completed);
    // 终态不得被失败覆盖。
    assert!(state.fail().is_err());
    // 返回测试成功。
    Ok(())
}

// 验证取消 Command 幂等且不会伪造完成。
#[test]
// 覆盖首次取消、重复取消和 worker 竞争完成。
fn cancellation_is_idempotent_and_nonterminal_until_proven()
-> Result<(), LongOperationTransitionError> {
    // 构造并 dispatch 新任务。
    let mut state = LongOperationState::accepted();
    // 越过 dispatch 事实点。
    state.start_dispatch()?;
    // 首次取消要求传播到 worker。
    assert_eq!(state.request_cancel(), CancelRequestEffect::Requested);
    // 取消请求仍不是终态。
    assert!(!state.status().is_terminal());
    // 核对独立取消请求状态。
    assert_eq!(state.status(), LongOperationStatus::CancelRequested);
    // 重复取消不得重复触发资源动作。
    assert_eq!(
        state.request_cancel(),
        CancelRequestEffect::AlreadyRequested
    );
    // worker 可能在取消竞争中先完成。
    state.complete()?;
    // 最终事实必须是 completed 而非伪造 cancelled。
    assert_eq!(state.status(), LongOperationStatus::Completed);
    // 终态取消只返回既有事实。
    assert_eq!(state.request_cancel(), CancelRequestEffect::AlreadyTerminal);
    // 返回测试成功。
    Ok(())
}

// 验证 dispatch 前取消可以证明失败且允许安全重提。
#[test]
// 覆盖 accepted 到 cancel-requested 再到 failed。
fn pre_dispatch_cancellation_can_fail_retry_safely() -> Result<(), LongOperationTransitionError> {
    // 构造尚未 dispatch 的任务。
    let mut state = LongOperationState::accepted();
    // 在队列中请求取消。
    assert_eq!(state.request_cancel(), CancelRequestEffect::Requested);
    // 取消请求不得允许后续 dispatch。
    assert!(state.start_dispatch().is_err());
    // broker 证明未 dispatch 后记录取消失败终态。
    state.fail()?;
    // 核对失败终态。
    assert_eq!(state.status(), LongOperationStatus::Failed);
    // 未 dispatch 的原提交允许安全重提。
    assert!(state.retry_safe());
    // 已建立 handle 表明业务接受可能发生。
    assert!(state.accepted_may_have_occurred());
    // 返回测试成功。
    Ok(())
}

// 验证 dispatch 前后中断使用不同恢复终态。
#[test]
// 覆盖可证明失败与 OutcomeUnknown 分界。
fn recovery_distinguishes_pre_and_post_dispatch() -> Result<(), LongOperationTransitionError> {
    // 构造 dispatch 前记录。
    let mut queued = LongOperationState::accepted();
    // 恢复 dispatch 前中断。
    assert_eq!(
        queued.recover_after_interruption(),
        RecoveryEffect::FailedBeforeDispatch
    );
    // 核对可证明失败。
    assert_eq!(queued.status(), LongOperationStatus::Failed);
    // dispatch 前失败允许重提。
    assert!(queued.retry_safe());

    // 构造 dispatch 后记录。
    let mut running = LongOperationState::accepted();
    // 越过实际动作事实点。
    running.start_dispatch()?;
    // 恢复 dispatch 后中断。
    assert_eq!(
        running.recover_after_interruption(),
        RecoveryEffect::OutcomeBecameUnknown
    );
    // 核对保守未知终态。
    assert_eq!(running.status(), LongOperationStatus::OutcomeUnknown);
    // dispatch 后未知不得自动重提。
    assert!(!running.retry_safe());
    // 重复恢复不得覆盖终态。
    assert_eq!(
        running.recover_after_interruption(),
        RecoveryEffect::Unchanged
    );
    // 返回测试成功。
    Ok(())
}

// 验证显式未知迁移只允许发生在 dispatch 后。
#[test]
// 覆盖 accepted、running 与终态拒绝。
fn outcome_unknown_requires_dispatch_and_is_terminal() -> Result<(), LongOperationTransitionError> {
    // 构造 dispatch 前任务。
    let mut state = LongOperationState::accepted();
    // dispatch 前不得声称结果未知。
    assert!(state.mark_outcome_unknown().is_err());
    // 开始实际动作。
    state.start_dispatch()?;
    // dispatch 后允许保守未知。
    state.mark_outcome_unknown()?;
    // 核对未知终态。
    assert_eq!(state.status(), LongOperationStatus::OutcomeUnknown);
    // 未知不得被后续完成覆盖。
    assert!(state.complete().is_err());
    // 返回测试成功。
    Ok(())
}

// 验证固定容量、结果与保留预算保持有界。
#[test]
// 防止后续实现把限制放宽成无界 registry。
fn lifecycle_budgets_are_fixed_and_bounded() {
    // 并发限制必须为正且小于总记录容量。
    const { assert!(MAX_ACTIVE_OPERATIONS > 0) };
    // 活跃任务不得填满全部 registry。
    const { assert!(MAX_ACTIVE_OPERATIONS < MAX_TRACKED_OPERATIONS) };
    // 结果预算固定为一 MiB。
    assert_eq!(MAX_RESULT_BYTES, 1_048_576);
    // 终态固定保留二十四小时。
    assert_eq!(TERMINAL_RETENTION_SECONDS, 86_400);
}

// 验证全部公开状态文本与 schema 枚举逐字一致。
#[test]
// 覆盖 kebab-case 序列化契约。
fn statuses_serialize_to_frozen_text() -> serde_json::Result<()> {
    // 列出状态与期望文本。
    let cases = [
        // accepted 文本。
        (LongOperationStatus::Accepted, "accepted"),
        // running 文本。
        (LongOperationStatus::Running, "running"),
        // cancel-requested 文本。
        (LongOperationStatus::CancelRequested, "cancel-requested"),
        // completed 文本。
        (LongOperationStatus::Completed, "completed"),
        // failed 文本。
        (LongOperationStatus::Failed, "failed"),
        // outcome-unknown 文本。
        (LongOperationStatus::OutcomeUnknown, "outcome-unknown"),
    ];
    // 逐一验证稳定 JSON 文本。
    for (status, expected) in cases {
        // 序列化当前状态。
        let encoded = serde_json::to_value(status)?;
        // 核对逐字文本。
        assert_eq!(encoded, serde_json::Value::String(expected.to_owned()));
    }
    // 返回测试成功。
    Ok(())
}
