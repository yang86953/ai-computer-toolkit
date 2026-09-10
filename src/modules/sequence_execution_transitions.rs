//! 实现 sequence execution 的可信 step final、取消请求与终态迁移。

// 导入 provider-neutral JSON 值与固定结果构造宏。
use serde_json::{Value, json};

// 导入父 Module 状态、记录与验证助手。
use super::{
    // 导入 execution 记录与封闭错误。
    SequenceExecutionRecord,
    SequenceExecutionRecordError,
    // 导入 execution 和 step 状态。
    SequenceExecutionState,
    SequenceStepState,
    // 导入安全错误与单调 revision 验证。
    validation::{is_safe_error, next_revision},
};

// 为领域记录补充可信 final 与取消迁移。
impl SequenceExecutionRecord {
    // 原子建立当前 dispatching step 的可信成功 final。
    pub(crate) fn finish_step_success(
        // 可变借用领域记录。
        &mut self,
        // 接收调用方观察的 record revision。
        expected_revision: u64,
        // 接收一基当前步骤。
        step_index: usize,
        // 接收完整有界 provider 结果。
        result: Value,
        // 接收可信 worker 生命周期证据。
        execution_evidence: Value,
        // 最后一步必须接收完整 Workflow 结果。
        workflow_result: Option<Value>,
        // 接收当前 broker epoch。
        broker_epoch: &str,
    ) -> Result<(), SequenceExecutionRecordError> {
        // 在克隆候选上建立全部可信 final 事实。
        self.mutate(expected_revision, broker_epoch, |candidate| {
            // 只允许当前 running dispatching 步骤结束。
            candidate.require_dispatching_step(step_index)?;
            // worker 证据必须是对象。
            if !execution_evidence.is_object() {
                // 拒绝缺少可信 final 证据。
                return Err(SequenceExecutionRecordError::InvalidRecord);
            }
            // 判断当前是否为最后一步。
            let is_last = step_index == candidate.snapshot.total_steps;
            // Workflow 结果只能且必须在整体完成时出现。
            let workflow_result = terminal_workflow_result(is_last, workflow_result)?;
            // 取得精确 receipt。
            let step = candidate.step_mut(step_index)?;
            // 推进 step revision。
            step.step_revision = next_revision(step.step_revision)?;
            // 保存可信成功状态。
            step.state = SequenceStepState::Completed;
            // final 已经确定完成。
            step.completed = true;
            // 保存完整 provider 结果，包括 JSON null。
            step.result = result;
            // 成功 final 不携带错误。
            step.error = Value::Null;
            // 保存可信执行证据。
            step.execution_evidence = execution_evidence;
            // 最后一步建立整体完成，否则进入下一安全检查点。
            if is_last {
                // 整个 Workflow 已运行到末尾。
                candidate.snapshot.state = SequenceExecutionState::Completed;
                // 保留最后步骤定位。
                candidate.snapshot.current_step = json!(step_index);
                // 保存完整 Workflow 结果。
                candidate.snapshot.workflow_result = workflow_result;
            } else {
                // 下一步尚未物化，可以安全继续。
                candidate.snapshot.state = SequenceExecutionState::AwaitingResume;
                // currentStep 指向下一 pending 步骤。
                candidate.snapshot.current_step = json!(step_index + 1);
                // 非终态不得提前发布 Workflow 结果。
                candidate.snapshot.workflow_result = Value::Null;
            }
            // 返回候选成功。
            Ok(())
        })
    }

    // 原子建立当前 dispatching step 的可信失败 final。
    pub(crate) fn finish_step_failure(
        // 可变借用领域记录。
        &mut self,
        // 接收调用方观察的 record revision。
        expected_revision: u64,
        // 接收一基当前步骤。
        step_index: usize,
        // 接收隐私安全的 provider 错误对象。
        error: Value,
        // 接收可信 worker 生命周期证据。
        execution_evidence: Value,
        // 终止或最后一步必须接收完整 Workflow 结果。
        workflow_result: Option<Value>,
        // 接收当前 broker epoch。
        broker_epoch: &str,
    ) -> Result<(), SequenceExecutionRecordError> {
        // 在克隆候选上建立全部可信 final 事实。
        self.mutate(expected_revision, broker_epoch, |candidate| {
            // 只允许当前 running dispatching 步骤结束。
            candidate.require_dispatching_step(step_index)?;
            // 错误 envelope 与 worker 证据都必须可信。
            if !is_safe_error(&error) || !execution_evidence.is_object() {
                // 拒绝不安全错误或缺失证据。
                return Err(SequenceExecutionRecordError::InvalidRecord);
            }
            // 从持久 input 读取原始 continueOnError 策略。
            let continue_on_error = candidate.continue_on_error()?;
            // 判断当前是否为最后一步。
            let is_last = step_index == candidate.snapshot.total_steps;
            // 只有继续且仍有下一步时保持非终态。
            let will_continue = continue_on_error && !is_last;
            // 其他路径必须建立完整 Workflow 结果。
            let workflow_result = terminal_workflow_result(!will_continue, workflow_result)?;
            // 取得精确 receipt。
            let step = candidate.step_mut(step_index)?;
            // 推进 step revision。
            step.step_revision = next_revision(step.step_revision)?;
            // 保存可信失败状态。
            step.state = SequenceStepState::Failed;
            // 失败 final 同样已经确定完成。
            step.completed = true;
            // 失败不得伪造成功结果。
            step.result = Value::Null;
            // 保存隐私安全错误。
            step.error = error;
            // 保存可信执行证据。
            step.execution_evidence = execution_evidence;
            // 按原 input 策略决定继续或终止。
            if will_continue {
                // 下一步尚未物化，可以安全继续。
                candidate.snapshot.state = SequenceExecutionState::AwaitingResume;
                // currentStep 指向下一 pending 步骤。
                candidate.snapshot.current_step = json!(step_index + 1);
                // 非终态不得提前发布 Workflow 结果。
                candidate.snapshot.workflow_result = Value::Null;
            } else {
                // continueOnError 运行到末尾算完整结束，否则算失败终止。
                candidate.snapshot.state = if continue_on_error && is_last {
                    // 所有声明步骤都已处理。
                    SequenceExecutionState::Completed
                } else {
                    // 普通失败按策略停止后续步骤。
                    SequenceExecutionState::Failed
                };
                // 保留产生终态的步骤定位。
                candidate.snapshot.current_step = json!(step_index);
                // 保存完整 Workflow 结果。
                candidate.snapshot.workflow_result = workflow_result;
            }
            // 返回候选成功。
            Ok(())
        })
    }

    // 持久化取消请求并在未 dispatch 时直接建立取消终态。
    pub(crate) fn request_cancel(
        // 可变借用领域记录。
        &mut self,
        // 接收调用方观察的 record revision。
        expected_revision: u64,
        // 接收当前 broker epoch。
        broker_epoch: &str,
    ) -> Result<bool, SequenceExecutionRecordError> {
        // expected revision 即使重复请求也必须精确匹配。
        if self.record_revision != expected_revision {
            // 拒绝 stale cancel。
            return Err(SequenceExecutionRecordError::RevisionConflict);
        }
        // 终态不再接受取消请求。
        if self.snapshot.state.terminal() {
            // 禁止改写不可变事实。
            return Err(SequenceExecutionRecordError::InvalidTransition);
        }
        // 已持久取消意图的重送不推进 revision。
        if self.snapshot.cancel_requested {
            // 返回没有新迁移。
            return Ok(false);
        }
        // 在克隆候选上建立取消事实。
        self.mutate(expected_revision, broker_epoch, |candidate| {
            // 先持久化取消意图。
            candidate.snapshot.cancel_requested = true;
            // 检查当前步骤是否已经 dispatch。
            let dispatching = candidate
                // 读取可选当前步骤。
                .snapshot
                // 转换为一基索引。
                .current_step()
                // 读取 receipt 状态。
                .is_some_and(|index| {
                    // 只有明确 dispatching 表示可能接受。
                    candidate.step_state(index) == Ok(SequenceStepState::Dispatching)
                });
            // 已 dispatch 只能等待协作停止或可信 final。
            if dispatching {
                // execution 保持 running。
                candidate.snapshot.state = SequenceExecutionState::Running;
                // 不伪造 Workflow 结果。
                candidate.snapshot.workflow_result = Value::Null;
                // 返回候选成功。
                return Ok(());
            }
            // 未 dispatch 时选择现有或首个 pending 步骤定位。
            let current_step = candidate
                // 优先保留 existing currentStep。
                .snapshot
                // 读取可选索引。
                .current_step()
                // created 从首个 pending 开始定位。
                .or_else(|| candidate.first_pending_step())
                // 非终态必须仍有步骤。
                .ok_or(SequenceExecutionRecordError::InvalidRecord)?;
            // 建立权威取消终态。
            candidate.snapshot.state = SequenceExecutionState::Cancelled;
            // 保存取消位置。
            candidate.snapshot.current_step = json!(current_step);
            // 保存不包含 provider 数据的最小 Workflow 结果。
            candidate.snapshot.workflow_result = cancellation_workflow_result();
            // 返回候选成功。
            Ok(())
        })?;
        // 报告已经建立新 revision。
        Ok(true)
    }

    // 在 dispatching worker 返回权威取消 final 后建立取消终态。
    pub(crate) fn finish_cancelled(
        // 可变借用领域记录。
        &mut self,
        // 接收调用方观察的 record revision。
        expected_revision: u64,
        // 接收一基当前步骤。
        step_index: usize,
        // 接收可信 worker 生命周期证据。
        execution_evidence: Value,
        // 接收当前 broker epoch。
        broker_epoch: &str,
    ) -> Result<(), SequenceExecutionRecordError> {
        // 在克隆候选上建立权威取消 final。
        self.mutate(expected_revision, broker_epoch, |candidate| {
            // 必须已经持久化取消请求。
            if !candidate.snapshot.cancel_requested {
                // 禁止无请求伪造取消。
                return Err(SequenceExecutionRecordError::InvalidTransition);
            }
            // 只允许当前 dispatching 步骤结束。
            candidate.require_dispatching_step(step_index)?;
            // worker 证据必须是对象。
            if !execution_evidence.is_object() {
                // 拒绝缺少可信停止事实。
                return Err(SequenceExecutionRecordError::InvalidRecord);
            }
            // 取得精确 receipt。
            let step = candidate.step_mut(step_index)?;
            // 推进 step revision。
            step.step_revision = next_revision(step.step_revision)?;
            // 以可信失败 receipt 表示 provider 未完成请求。
            step.state = SequenceStepState::Failed;
            // worker 已经返回可信 final。
            step.completed = true;
            // 取消不携带成功结果。
            step.result = Value::Null;
            // 保存固定隐私安全取消错误。
            step.error = json!({
                // 使用稳定取消码。
                "code": "CANCELLED",
                // 不公开 worker 或 provider 私有信息。
                "message": "The sequence step stopped after an authoritative cancellation."
            });
            // 保存可信停止证据。
            step.execution_evidence = execution_evidence;
            // 整个 execution 建立取消终态。
            candidate.snapshot.state = SequenceExecutionState::Cancelled;
            // 保留当前步骤定位。
            candidate.snapshot.current_step = json!(step_index);
            // 保存最小 Workflow 结果。
            candidate.snapshot.workflow_result = cancellation_workflow_result();
            // 返回候选成功。
            Ok(())
        })
    }

    // 验证当前步骤是 running dispatching receipt。
    fn require_dispatching_step(
        // 借用领域记录。
        &self,
        // 接收一基步骤索引。
        step_index: usize,
    ) -> Result<(), SequenceExecutionRecordError> {
        // execution、currentStep 与 receipt 必须逐值一致。
        if self.snapshot.state != SequenceExecutionState::Running
            // 当前步骤必须匹配。
            || self.snapshot.current_step() != Some(step_index)
            // receipt 必须已经持久 dispatching。
            || self.step_state(step_index)? != SequenceStepState::Dispatching
        {
            // 禁止从未 dispatch 或其他步骤建立 final。
            return Err(SequenceExecutionRecordError::InvalidTransition);
        }
        // 当前步骤可以建立可信 final。
        Ok(())
    }

    // 从持久 input 读取 continueOnError 默认语义。
    fn continue_on_error(&self) -> Result<bool, SequenceExecutionRecordError> {
        // 省略字段使用公开默认 false。
        match self.input.get("continueOnError") {
            // 缺失表示 false。
            None => Ok(false),
            // 显式布尔保持原值。
            Some(Value::Bool(value)) => Ok(*value),
            // 其他类型表示持久 input 损坏。
            Some(_) => Err(SequenceExecutionRecordError::InvalidRecord),
        }
    }
}

// 验证 terminal 与非 terminal Workflow 结果出现规则。
fn terminal_workflow_result(
    // 标记当前迁移是否建立 execution 终态。
    terminal: bool,
    // 接收可选 Workflow 结果。
    workflow_result: Option<Value>,
) -> Result<Value, SequenceExecutionRecordError> {
    // 终态要求对象，非终态要求缺失。
    match (terminal, workflow_result) {
        // 终态保存严格对象。
        (true, Some(value)) if value.is_object() => Ok(value),
        // 非终态保持 required nullable 字段为 null。
        (false, None) => Ok(Value::Null),
        // 其他组合拒绝提前、缺失或错误形状。
        _ => Err(SequenceExecutionRecordError::InvalidRecord),
    }
}

// 构造不含 provider 数据的固定取消 Workflow 结果。
pub(super) fn cancellation_workflow_result() -> Value {
    // 返回稳定最小对象。
    json!({
        // execution 没有完整成功。
        "ok": false,
        // 明确终止类别。
        "cancelled": true
    })
}
