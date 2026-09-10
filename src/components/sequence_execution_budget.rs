//! 提供与 provider 无关的 sequence 单调 deadline 与取消观察。

// 导入单调时间点与持续时间。
use std::time::{Duration, Instant};

// 固定 sequence 总执行预算的最小毫秒数。
pub(crate) const MINIMUM_SEQUENCE_TOTAL_TIMEOUT_MS: u32 = 1;
// 固定 sequence 总执行预算的兼容默认值为五分钟。
pub(crate) const DEFAULT_SEQUENCE_TOTAL_TIMEOUT_MS: u32 = 300_000;
// 固定总预算上限，使六十四步均可各自使用三十秒硬上限。
pub(crate) const MAXIMUM_SEQUENCE_TOTAL_TIMEOUT_MS: u32 = 1_920_000;
// 固定单步骤 deadline 的最小毫秒数。
pub(crate) const MINIMUM_SEQUENCE_STEP_TIMEOUT_MS: u32 = 1;
// 固定单步骤 deadline 的兼容默认值为三十秒。
pub(crate) const DEFAULT_SEQUENCE_STEP_TIMEOUT_MS: u32 = 30_000;
// 固定单步骤 deadline 的最大毫秒数为三十秒。
pub(crate) const MAXIMUM_SEQUENCE_STEP_TIMEOUT_MS: u32 = 30_000;

// 表示 sequence 观察到的封闭停止原因。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SequenceStopReason {
    // 表示调用方取消优先于同次观察中的 deadline。
    Cancelled,
    // 表示整个 Workflow 的总 deadline 已耗尽。
    WorkflowDeadlineExceeded,
    // 表示当前步骤自己的 deadline 已耗尽。
    StepDeadlineExceeded,
}

// 表示停止事实相对 provider dispatch 的封闭阶段。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SequenceDispatchPhase {
    // 表示请求尚未交给 provider。
    BeforeDispatch,
    // 表示请求已经交给 provider，但尚未建立最终结果。
    AfterDispatch,
}

// 保存一次停止观察的最小可靠事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SequenceStopObservation {
    // 保存取消或两级 deadline 的唯一停止原因。
    pub(crate) reason: SequenceStopReason,
    // 保存停止发生时最后可靠的 dispatch 阶段。
    pub(crate) phase: SequenceDispatchPhase,
}

// 保存整个 sequence Workflow 的单调总 deadline。
#[derive(Clone, Copy, Debug)]
pub(crate) struct SequenceWorkflowBudget {
    // 保存只在进程内使用的单调截止时间。
    deadline: Instant,
}

// 保存从总预算派生的当前步骤 deadline。
#[derive(Clone, Copy, Debug)]
pub(crate) struct SequenceStepBudget {
    // 保留总 deadline 以区分总预算与步骤预算耗尽。
    workflow_deadline: Instant,
    // 保存当前步骤自己的单调截止时间。
    step_deadline: Instant,
}

// 实现总预算的有界创建与步骤派生。
impl SequenceWorkflowBudget {
    // 从当前单调时间创建总预算。
    pub(crate) fn new(total_timeout_ms: u32) -> Option<Self> {
        // 委托可测试的显式起点构造器。
        Self::from_start(Instant::now(), total_timeout_ms)
    }

    // 从调用方提供的单调起点创建总预算。
    pub(crate) fn from_start(started_at: Instant, total_timeout_ms: u32) -> Option<Self> {
        // 拒绝绕过公开契约下界的零预算。
        if !(MINIMUM_SEQUENCE_TOTAL_TIMEOUT_MS..=MAXIMUM_SEQUENCE_TOTAL_TIMEOUT_MS)
            // 核对总预算毫秒数。
            .contains(&total_timeout_ms)
        {
            // 由 Workflow 映射为公开参数错误。
            return None;
        }
        // 使用单调时间和受硬上限约束的持续时间计算截止点。
        let deadline =
            started_at.checked_add(Duration::from_millis(u64::from(total_timeout_ms)))?;
        // 返回只拥有总 deadline 的窄状态。
        Some(Self { deadline })
    }

    // 从当前时间派生一个独立步骤预算。
    pub(crate) fn begin_step(&self, step_timeout_ms: u32) -> Option<SequenceStepBudget> {
        // 委托可测试的显式时间构造器。
        self.begin_step_at(Instant::now(), step_timeout_ms)
    }

    // 从指定单调时间派生独立步骤预算。
    pub(crate) fn begin_step_at(
        // 借用不变的 Workflow 总预算。
        &self,
        // 接收步骤开始的单调时间。
        started_at: Instant,
        // 接收当前步骤独立 deadline 毫秒数。
        step_timeout_ms: u32,
    ) -> Option<SequenceStepBudget> {
        // 拒绝绕过公开步骤边界的预算。
        if !(MINIMUM_SEQUENCE_STEP_TIMEOUT_MS..=MAXIMUM_SEQUENCE_STEP_TIMEOUT_MS)
            // 核对步骤预算毫秒数。
            .contains(&step_timeout_ms)
        {
            // 由 Workflow 映射为公开参数错误。
            return None;
        }
        // 计算不重置总预算的独立步骤截止点。
        let step_deadline =
            started_at.checked_add(Duration::from_millis(u64::from(step_timeout_ms)))?;
        // 同时保留两级 deadline 供停止原因判定。
        Some(SequenceStepBudget {
            // 复制进程内单调总截止点。
            workflow_deadline: self.deadline,
            // 保存本步骤截止点。
            step_deadline,
        })
    }
}

// 实现当前步骤的停止观察与剩余预算投影。
impl SequenceStepBudget {
    // 按固定优先级观察取消、总 deadline 与步骤 deadline。
    pub(crate) fn observe(
        // 借用当前步骤预算。
        &self,
        // 接收单调观察时间。
        observed_at: Instant,
        // 接收上层已经读取的取消事实。
        cancellation_requested: bool,
        // 接收相对 dispatch 的最后可靠阶段。
        phase: SequenceDispatchPhase,
    ) -> Option<SequenceStopObservation> {
        // 同一次观察中取消拥有最高优先级。
        if cancellation_requested {
            // 返回不推测 provider 结果的停止事实。
            return Some(SequenceStopObservation {
                // 固定取消原因。
                reason: SequenceStopReason::Cancelled,
                // 保留调用方提供的可靠阶段。
                phase,
            });
        }
        // 总预算耗尽优先于同一时刻的步骤预算耗尽。
        if observed_at >= self.workflow_deadline {
            // 返回总预算停止事实。
            return Some(SequenceStopObservation {
                // 区分 Workflow 总 deadline。
                reason: SequenceStopReason::WorkflowDeadlineExceeded,
                // 保留调用方提供的可靠阶段。
                phase,
            });
        }
        // 只有总预算仍有效时才检查步骤 deadline。
        if observed_at >= self.step_deadline {
            // 返回步骤预算停止事实。
            return Some(SequenceStopObservation {
                // 区分当前步骤 deadline。
                reason: SequenceStopReason::StepDeadlineExceeded,
                // 保留调用方提供的可靠阶段。
                phase,
            });
        }
        // 没有观察到停止条件。
        None
    }

    // 返回传给 worker 或 Module 的唯一剩余毫秒预算。
    pub(crate) fn remaining_timeout_ms(&self, observed_at: Instant) -> Option<u32> {
        // 取总 deadline 与步骤 deadline 的较早者，禁止重置任一预算。
        let effective_deadline = self.workflow_deadline.min(self.step_deadline);
        // 已到截止时间时不伪造一毫秒可用预算。
        if observed_at >= effective_deadline {
            // 报告预算已经耗尽。
            return None;
        }
        // 只计算尚未耗尽的正持续时间。
        let remaining = effective_deadline.checked_duration_since(observed_at)?;
        // 将纳秒余量向上取整到协议允许的毫秒。
        let rounded_millis = remaining.as_nanos().saturating_add(999_999) / 1_000_000;
        // 非零 Duration 在极窄平台分辨率下仍至少投影为一毫秒。
        let bounded_millis = rounded_millis.max(1);
        // 两级公开上限保证结果可安全转换为 u32。
        u32::try_from(bounded_millis).ok()
    }
}

// 验证两级 deadline、取消优先级和 dispatch 阶段保持语义。
#[cfg(test)]
mod tests {
    // 导入持续时间供合成单调观察使用。
    use std::time::{Duration, Instant};

    // 导入全部被测封闭类型与边界常量。
    use super::{
        // 导入默认步骤预算。
        DEFAULT_SEQUENCE_STEP_TIMEOUT_MS,
        // 导入默认总预算。
        DEFAULT_SEQUENCE_TOTAL_TIMEOUT_MS,
        // 导入 dispatch 阶段。
        SequenceDispatchPhase,
        // 导入停止观察。
        SequenceStopObservation,
        // 导入停止原因。
        SequenceStopReason,
        // 导入总预算。
        SequenceWorkflowBudget,
    };

    // 验证较短步骤 deadline 不会重置总预算。
    #[test]
    fn step_budget_uses_the_earlier_deadline() -> Result<(), Box<dyn std::error::Error>> {
        // 冻结合成单调起点。
        let started_at = Instant::now();
        // 建立十秒总预算。
        let workflow = SequenceWorkflowBudget::from_start(started_at, 10_000)
            // 把意外的构造失败转换为可传播测试错误。
            .ok_or_else(|| std::io::Error::other("valid workflow budget was rejected"))?;
        // 在总流程开始两秒后建立三秒步骤预算。
        let step = workflow
            // 显式派生当前步骤截止点。
            .begin_step_at(started_at + Duration::from_millis(2_000), 3_000)
            // 把意外的构造失败转换为可传播测试错误。
            .ok_or_else(|| std::io::Error::other("valid step budget was rejected"))?;
        // 步骤开始时应暴露完整三秒余量。
        assert_eq!(
            // 查询刚开始步骤的剩余时间。
            step.remaining_timeout_ms(started_at + Duration::from_millis(2_000)),
            // 剩余时间必须完整。
            Some(3_000)
        );
        // 步骤截止点必须先于仍剩五秒的总预算触发。
        let stopped = step
            // 在步骤截止时观察停止事实。
            .observe(
                // 使用精确步骤截止点。
                started_at + Duration::from_millis(5_000),
                // 本次没有取消。
                false,
                // provider 尚未 dispatch。
                SequenceDispatchPhase::BeforeDispatch,
            )
            // 把意外缺失转换为可传播测试错误。
            .ok_or_else(|| std::io::Error::other("step deadline was not observed"))?;
        // 核对封闭步骤 deadline 原因。
        assert_eq!(
            // 读取停止观察原因。
            stopped.reason,
            // 必须区分步骤 deadline。
            SequenceStopReason::StepDeadlineExceeded
        );
        // 测试正常完成。
        Ok(())
    }

    // 验证总 deadline 在较长步骤中保持最终边界。
    #[test]
    fn workflow_deadline_wins_before_later_step_deadline() -> Result<(), Box<dyn std::error::Error>>
    {
        // 冻结合成单调起点。
        let started_at = Instant::now();
        // 建立五秒总预算。
        let workflow = SequenceWorkflowBudget::from_start(started_at, 5_000)
            // 把意外的构造失败转换为可传播测试错误。
            .ok_or_else(|| std::io::Error::other("valid workflow budget was rejected"))?;
        // 在第四秒建立最长三十秒步骤预算。
        let step = workflow
            // 步骤预算不能延长总预算。
            .begin_step_at(started_at + Duration::from_millis(4_000), 30_000)
            // 把意外的构造失败转换为可传播测试错误。
            .ok_or_else(|| std::io::Error::other("valid step budget was rejected"))?;
        // worker 只能取得总预算剩余的一秒。
        assert_eq!(
            // 查询总预算耗尽前的剩余时间。
            step.remaining_timeout_ms(started_at + Duration::from_millis(4_000)),
            // 只允许剩余一秒。
            Some(1_000)
        );
        // 总 deadline 到达时必须报告总预算停止。
        assert_eq!(
            // 在总截止点观察。
            step.observe(
                // 使用精确总截止点。
                started_at + Duration::from_millis(5_000),
                // 本次没有取消。
                false,
                // provider 已经 dispatch。
                SequenceDispatchPhase::AfterDispatch,
            ),
            // 保留总预算原因与 dispatch 后阶段。
            Some(SequenceStopObservation {
                // 必须区分 Workflow 总 deadline。
                reason: SequenceStopReason::WorkflowDeadlineExceeded,
                // 必须保留最后可靠阶段。
                phase: SequenceDispatchPhase::AfterDispatch,
            })
        );
        // 精确到达总截止点后不得伪造一毫秒执行窗口。
        assert_eq!(
            // 查询已经耗尽的有效预算。
            step.remaining_timeout_ms(started_at + Duration::from_millis(5_000)),
            // 耗尽必须明确无剩余预算。
            None
        );
        // 测试正常完成。
        Ok(())
    }

    // 验证同次观察中的取消优先级和 dispatch 阶段不被改写。
    #[test]
    fn cancellation_wins_deadline_race_and_keeps_dispatch_phase()
    -> Result<(), Box<dyn std::error::Error>> {
        // 冻结合成单调起点。
        let started_at = Instant::now();
        // 建立最短总预算。
        let workflow = SequenceWorkflowBudget::from_start(started_at, 1)
            // 把意外的构造失败转换为可传播测试错误。
            .ok_or_else(|| std::io::Error::other("minimum workflow budget was rejected"))?;
        // 同时建立最短步骤预算。
        let step = workflow
            // 使用相同起点制造三者竞态。
            .begin_step_at(started_at, 1)
            // 把意外的构造失败转换为可传播测试错误。
            .ok_or_else(|| std::io::Error::other("minimum step budget was rejected"))?;
        // 在两个 deadline 均耗尽时同时报告取消。
        let stopped = step
            // 执行唯一停止观察。
            .observe(
                // 使用两个 deadline 的共同截止点。
                started_at + Duration::from_millis(1),
                // 同次观察发现取消。
                true,
                // provider 已经 dispatch。
                SequenceDispatchPhase::AfterDispatch,
            )
            // 把意外缺失转换为可传播测试错误。
            .ok_or_else(|| std::io::Error::other("cancelled deadline race was not observed"))?;
        // 取消必须拥有固定最高优先级。
        assert_eq!(stopped.reason, SequenceStopReason::Cancelled);
        // dispatch 后事实不得被伪造成未执行。
        assert_eq!(stopped.phase, SequenceDispatchPhase::AfterDispatch);
        // 测试正常完成。
        Ok(())
    }

    // 验证边界拒绝与兼容默认值均保持有限。
    #[test]
    fn timeout_bounds_reject_zero_and_accept_defaults() -> Result<(), Box<dyn std::error::Error>> {
        // 零总预算违反公开最小边界。
        assert!(SequenceWorkflowBudget::from_start(Instant::now(), 0).is_none());
        // 默认总预算必须有效。
        let workflow = SequenceWorkflowBudget::from_start(
            // 使用合成起点。
            Instant::now(),
            // 使用冻结的五分钟默认值。
            DEFAULT_SEQUENCE_TOTAL_TIMEOUT_MS,
        )
        // 把意外的构造失败转换为可传播测试错误。
        .ok_or_else(|| std::io::Error::other("default workflow budget was rejected"))?;
        // 零步骤预算违反公开最小边界。
        assert!(workflow.begin_step_at(Instant::now(), 0).is_none());
        // 默认步骤预算必须有效且有限。
        assert!(
            workflow
                .begin_step(DEFAULT_SEQUENCE_STEP_TIMEOUT_MS)
                .is_some()
        );
        // 测试正常完成。
        Ok(())
    }
}
