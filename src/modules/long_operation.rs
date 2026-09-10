//! 拥有长操作任务状态机、预算与恢复语义。

// 导入协议序列化派生。
use serde::{Deserialize, Serialize};

// 固定同一 broker 最多并发执行的任务数。
pub(crate) const MAX_ACTIVE_OPERATIONS: usize = 4;
// 固定 registry 最多保留的任务记录数。
pub(crate) const MAX_TRACKED_OPERATIONS: usize = 128;
// 固定单个终态结果的 UTF-8 JSON 字节预算。
pub(crate) const MAX_RESULT_BYTES: usize = 1_048_576;
// 固定终态记录保留秒数。
pub(crate) const TERMINAL_RETENTION_SECONDS: u64 = 86_400;

// 表示长操作对调用方公开的封闭生命周期状态。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
// 使用 kebab-case 固定 JSON 文本。
#[serde(rename_all = "kebab-case")]
pub(crate) enum LongOperationStatus {
    // broker 已持久接受但尚未开始 dispatch。
    Accepted,
    // broker 已经开始 dispatch，实际动作可能发生。
    Running,
    // broker 已记录取消请求但尚未证明终态。
    CancelRequested,
    // worker 已证明操作成功完成。
    Completed,
    // broker 已证明操作失败或在 dispatch 前终止。
    Failed,
    // dispatch 后无法证明最终结果。
    OutcomeUnknown,
}

// 为公开状态提供终态分类。
impl LongOperationStatus {
    // 返回状态是否不可再迁移。
    pub(crate) const fn is_terminal(self) -> bool {
        // 只把有证据的最终状态视为终态。
        matches!(
            // 检查当前状态。
            self,
            // 成功完成是终态。
            Self::Completed
                // 已证明失败是终态。
                | Self::Failed
                // 结果未知也是保守终态。
                | Self::OutcomeUnknown
        )
    }
}

// 表示取消 Command 对当前状态产生的幂等效果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CancelRequestEffect {
    // 首次记录取消请求。
    Requested,
    // 取消请求此前已经记录。
    AlreadyRequested,
    // 任务已经处于终态，取消不改变事实。
    AlreadyTerminal,
}

// 表示 broker 恢复时对中断记录产生的效果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryEffect {
    // 终态记录无需恢复。
    Unchanged,
    // dispatch 前中断可证明没有动作并转为失败。
    FailedBeforeDispatch,
    // dispatch 后中断无法证明结果并转为未知。
    OutcomeBecameUnknown,
}

// 表示状态机拒绝的一次非法迁移。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LongOperationTransitionError {
    // 保存迁移前状态。
    from: LongOperationStatus,
    // 保存不含 payload 的稳定动作名称。
    action: &'static str,
}

// 为非法迁移提供只读诊断。
impl LongOperationTransitionError {
    // 构造不泄漏任务结果的迁移错误。
    const fn new(from: LongOperationStatus, action: &'static str) -> Self {
        // 保存封闭状态和静态动作名称。
        Self { from, action }
    }

    // 返回迁移前状态。
    pub(crate) const fn from(&self) -> LongOperationStatus {
        // 复制无状态枚举值。
        self.from
    }

    // 返回稳定动作名称。
    pub(crate) const fn action(&self) -> &'static str {
        // 返回不含任务输入的静态文本。
        self.action
    }
}

// 保存一个任务记录的最小领域状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LongOperationState {
    // 保存公开生命周期状态。
    status: LongOperationStatus,
    // 记录动作是否已经越过 dispatch 事实点。
    dispatch_started: bool,
}

// 为长操作记录提供唯一状态迁移入口。
impl LongOperationState {
    // 构造 broker 已持久接受的新任务。
    pub(crate) const fn accepted() -> Self {
        // 新任务尚未越过 dispatch 事实点。
        Self {
            // 初始公开状态固定为 accepted。
            status: LongOperationStatus::Accepted,
            // 初始不允许声称动作已经发生。
            dispatch_started: false,
        }
    }

    // 从严格 journal 事实恢复领域状态。
    pub(crate) const fn restore(
        // 接收已经反序列化的公开状态。
        status: LongOperationStatus,
        // 接收已经持久化的 dispatch 事实。
        dispatch_started: bool,
    ) -> Result<Self, LongOperationTransitionError> {
        // 只允许状态与不可逆事实形成契约内组合。
        let valid = match status {
            // accepted 尚未越过 dispatch 点。
            LongOperationStatus::Accepted => !dispatch_started,
            // running 必须已经越过 dispatch 点。
            LongOperationStatus::Running => dispatch_started,
            // cancel-requested 可发生在 dispatch 前后。
            LongOperationStatus::CancelRequested => true,
            // completed 必须有 dispatch 事实。
            LongOperationStatus::Completed => dispatch_started,
            // failed 可证明 dispatch 前或后的失败。
            LongOperationStatus::Failed => true,
            // outcome-unknown 只能发生在 dispatch 后。
            LongOperationStatus::OutcomeUnknown => dispatch_started,
        };
        // 拒绝 journal 伪造的状态组合。
        if !valid {
            // 返回不泄漏持久化内容的稳定迁移错误。
            return Err(LongOperationTransitionError::new(status, "restore"));
        }
        // 恢复经过验证的最小领域事实。
        Ok(Self {
            // 保留公开状态。
            status,
            // 保留不可逆 dispatch 事实。
            dispatch_started,
        })
    }

    // 返回当前公开状态。
    pub(crate) const fn status(self) -> LongOperationStatus {
        // 复制无状态枚举值。
        self.status
    }

    // 返回是否已经越过 dispatch 事实点。
    pub(crate) const fn dispatch_started(self) -> bool {
        // 只公开保守布尔事实。
        self.dispatch_started
    }

    // 返回原提交是否可能已被业务接受。
    pub(crate) const fn accepted_may_have_occurred(self) -> bool {
        // 只有已建立 state 的请求才会调用此投影。
        true
    }

    // 返回重新提交原写操作是否安全。
    pub(crate) const fn retry_safe(self) -> bool {
        // 仅 dispatch 前已证明失败允许安全重提。
        matches!(self.status, LongOperationStatus::Failed) && !self.dispatch_started
    }

    // 记录 worker dispatch 已开始。
    pub(crate) fn start_dispatch(&mut self) -> Result<(), LongOperationTransitionError> {
        // 只有 accepted 可以越过 dispatch 事实点。
        if self.status != LongOperationStatus::Accepted || self.dispatch_started {
            // 拒绝重复 dispatch、取消后 dispatch 与终态 dispatch。
            return Err(LongOperationTransitionError::new(
                // 记录拒绝时状态。
                self.status,
                // 使用稳定动作名称。
                "start-dispatch",
            ));
        }
        // 先记录不可逆 dispatch 事实。
        self.dispatch_started = true;
        // 再公开 running 状态。
        self.status = LongOperationStatus::Running;
        // 返回迁移成功。
        Ok(())
    }

    // 幂等记录取消请求而不伪造终态。
    pub(crate) fn request_cancel(&mut self) -> CancelRequestEffect {
        // 终态取消只返回既有事实。
        if self.status.is_terminal() {
            // 不改变任何终态字段。
            return CancelRequestEffect::AlreadyTerminal;
        }
        // 已记录取消请求时保持幂等。
        if self.status == LongOperationStatus::CancelRequested {
            // 不重复触发资源动作。
            return CancelRequestEffect::AlreadyRequested;
        }
        // accepted 或 running 均进入非终态取消请求。
        self.status = LongOperationStatus::CancelRequested;
        // 告知调用方需要首次传播取消。
        CancelRequestEffect::Requested
    }

    // 记录 worker 已证明成功完成。
    pub(crate) fn complete(&mut self) -> Result<(), LongOperationTransitionError> {
        // 成功必须发生在 dispatch 之后且尚未终态。
        if !self.dispatch_started || self.status.is_terminal() {
            // accepted、dispatch 前取消或终态均不得伪造成功。
            return Err(LongOperationTransitionError::new(
                // 记录拒绝时状态。
                self.status,
                // 使用稳定动作名称。
                "complete",
            ));
        }
        // 取消与完成竞争时以 worker 的完成证据为准。
        self.status = LongOperationStatus::Completed;
        // 返回迁移成功。
        Ok(())
    }

    // 记录已经证明的失败。
    pub(crate) fn fail(&mut self) -> Result<(), LongOperationTransitionError> {
        // 终态不得被后续错误覆盖。
        if self.status.is_terminal() {
            // 保留首个可靠终态。
            return Err(LongOperationTransitionError::new(
                // 记录拒绝时状态。
                self.status,
                // 使用稳定动作名称。
                "fail",
            ));
        }
        // accepted、running 与 cancel-requested 均可被证据终结为失败。
        self.status = LongOperationStatus::Failed;
        // 返回迁移成功。
        Ok(())
    }

    // 在 dispatch 后记录无法证明最终结果。
    pub(crate) fn mark_outcome_unknown(&mut self) -> Result<(), LongOperationTransitionError> {
        // 只有 dispatch 后的非终态允许变为未知。
        if !self.dispatch_started || self.status.is_terminal() {
            // dispatch 前必须使用可证明失败而不是未知。
            return Err(LongOperationTransitionError::new(
                // 记录拒绝时状态。
                self.status,
                // 使用稳定动作名称。
                "mark-outcome-unknown",
            ));
        }
        // 保守终结为 outcome-unknown。
        self.status = LongOperationStatus::OutcomeUnknown;
        // 返回迁移成功。
        Ok(())
    }

    // 按 dispatch 事实恢复一次 broker 中断。
    pub(crate) fn recover_after_interruption(&mut self) -> RecoveryEffect {
        // 已有终态不得被恢复流程覆盖。
        if self.status.is_terminal() {
            // 保持终态不变。
            return RecoveryEffect::Unchanged;
        }
        // dispatch 后无法证明 worker 最终结果。
        if self.dispatch_started {
            // 使用保守未知终态。
            self.status = LongOperationStatus::OutcomeUnknown;
            // 返回未知恢复效果。
            return RecoveryEffect::OutcomeBecameUnknown;
        }
        // dispatch 前中断可证明动作尚未发生。
        self.status = LongOperationStatus::Failed;
        // 返回可安全重试的恢复效果。
        RecoveryEffect::FailedBeforeDispatch
    }
}

// 声明长操作领域状态机回归测试。
#[cfg(test)]
// 将迁移矩阵放入独立文件以保持 Module 精简。
#[path = "long_operation_tests.rs"]
mod tests;
