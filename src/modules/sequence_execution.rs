//! 拥有 sequence execution 持久记录、单调 revision 与恢复状态机。

// 导入严格 journal 编解码派生。
use serde::{Deserialize, Serialize};
// 导入 provider-neutral JSON 值与安全错误构造宏。
use serde_json::{Value, json};

// 导入跨状态机与 journal 共用的 canonical 文本验证。
use crate::components::sequence_execution_identity::{is_digest, is_nonce};
// 导入 journal Component 唯一拥有的单记录字节预算。
use crate::components::sequence_execution_journal::MAX_SEQUENCE_EXECUTION_RECORD_BYTES;

// 固定 execution journal 私有版本。
const JOURNAL_CONTRACT_VERSION: &str = "act/sequence-execution-journal/v1";
// 固定状态投影版本。
const EXECUTION_CONTRACT_VERSION: &str = "act/sequence-execution/v1";
// 固定单个 execution 最大步骤数。
pub(crate) const MAX_SEQUENCE_EXECUTION_STEPS: usize = 64;
// 固定原始 sequence input 最大序列化字节数。
const MAX_SEQUENCE_INPUT_BYTES: usize = 16_777_216;
// 固定 Policy revision 最大 UTF-8 字节数。
const MAX_POLICY_REVISION_BYTES: usize = 128;

// 注册同一 Module 内的跨字段恢复验证实现。
#[path = "sequence_execution_validation.rs"]
mod validation;
// 注册同一 Module 内的可信 final、取消与终态迁移。
#[path = "sequence_execution_transitions.rs"]
mod transitions;
// 导入状态迁移需要的单调 revision 助手。
use validation::next_revision;

// 表示 record codec 与状态机封闭失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SequenceExecutionRecordError {
    // JSON 文档无法严格解码或编码。
    InvalidJson,
    // 版本、跨字段形状或预算不合法。
    InvalidRecord,
    // execution、step 或 nonce identity 不 canonical。
    InvalidIdentity,
    // SHA-256 文本不 canonical。
    InvalidDigest,
    // expected revision 与当前事实不一致。
    RevisionConflict,
    // 当前状态不允许请求迁移。
    InvalidTransition,
    // prepared 恢复时语义或 Policy revision 已漂移。
    ResumeConflict,
    // 单调 revision 已达到整数边界。
    RevisionExhausted,
    // 完整记录超过固定字节预算。
    RecordTooLarge,
}

// 表示 execution 的封闭持久状态。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SequenceExecutionState {
    // input 与稳定 step identity 已建立。
    Created,
    // 当前步骤正在准备或 dispatch。
    Running,
    // 在安全检查点等待显式 resume。
    AwaitingResume,
    // 为后续动态确认保留。
    AwaitingConfirmation,
    // 全部步骤确定完成。
    Completed,
    // Workflow 确定失败。
    Failed,
    // 已取得权威取消终态。
    Cancelled,
    // dispatch 后缺少可信 final。
    OutcomeUnknown,
}

// 为 execution 状态提供不可变分类。
impl SequenceExecutionState {
    // 判断状态是否为不可恢复终态。
    pub(super) const fn terminal(self) -> bool {
        // 穷举四个终态。
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::OutcomeUnknown
        )
    }

    // 判断状态是否允许显式 resume。
    pub(super) const fn resumable(self) -> bool {
        // 当前 v1 只有安全等待点可恢复。
        matches!(self, Self::AwaitingResume)
    }
}

// 表示单个 step receipt 的封闭持久状态。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SequenceStepState {
    // 尚未物化候选请求。
    Pending,
    // 候选摘要与 Policy revision 已持久化但未 dispatch。
    Prepared,
    // 可能接受事实已先于 worker 持久化。
    Dispatching,
    // 已取得可信成功 final。
    Completed,
    // 已取得可信失败 final。
    Failed,
    // dispatch 后没有可信 final。
    OutcomeUnknown,
}

// 保存一个严格且可恢复的 step receipt。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SequenceStepReceipt {
    // 保存稳定随机 step identity。
    pub(super) step_id: String,
    // 保存一基步骤索引。
    pub(super) step_index: usize,
    // 保存 step 自有单调 revision。
    pub(super) step_revision: u64,
    // 保存 null 或最终请求 SHA-256 digest。
    pub(super) semantic_digest: Value,
    // 保存 null 或有界 Policy revision。
    pub(super) policy_revision: Value,
    // 保存封闭 step 状态。
    pub(super) state: SequenceStepState,
    // 保存 null 或稳定 dispatch nonce。
    pub(super) dispatch_nonce: Value,
    // 保存 provider 是否可能已经接受。
    pub(super) accepted_may_have_occurred: bool,
    // 保存可信 final 是否已经完成。
    pub(super) completed: bool,
    // 保存可信成功结果或 null。
    pub(super) result: Value,
    // 保存可信失败错误或 null。
    pub(super) error: Value,
    // 保存 worker 生命周期证据或 null。
    pub(super) execution_evidence: Value,
}

// 为 step receipt 提供只读领域投影。
impl SequenceStepReceipt {
    // 返回稳定 step identity。
    pub(crate) fn step_id(&self) -> &str {
        // 借用 canonical 文本。
        &self.step_id
    }

    // 返回一基步骤索引。
    pub(crate) const fn step_index(&self) -> usize {
        // 复制稳定索引。
        self.step_index
    }

    // 返回单调 step revision。
    pub(crate) const fn revision(&self) -> u64 {
        // 复制 revision。
        self.step_revision
    }

    // 返回封闭 step 状态。
    pub(crate) const fn state(&self) -> SequenceStepState {
        // 复制状态。
        self.state
    }

    // 返回 dispatch 是否可能已经发生。
    pub(crate) const fn accepted_may_have_occurred(&self) -> bool {
        // 复制保守接受事实。
        self.accepted_may_have_occurred
    }

    // 返回安全错误码而不暴露错误 payload。
    pub(crate) fn error_code(&self) -> Option<&str> {
        // 只读取对象中的稳定 code 字段。
        self.error.get("code").and_then(Value::as_str)
    }
}

// 保存可由 broker 返回的 execution 状态投影。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SequenceExecutionSnapshot {
    // 保存状态投影契约版本。
    pub(super) contract_version: String,
    // 保存稳定 execution identity。
    pub(super) execution_id: String,
    // 保存 execution 单调 revision。
    pub(super) execution_revision: u64,
    // 保存封闭 execution 状态。
    pub(super) state: SequenceExecutionState,
    // 保存 null 或一基当前步骤。
    pub(super) current_step: Value,
    // 保存固定总步骤数。
    pub(super) total_steps: usize,
    // 保存是否可显式 resume。
    pub(super) resumable: bool,
    // 保存是否为不可变终态。
    pub(super) terminal: bool,
    // 保存是否已经失去可信终态。
    pub(super) outcome_unknown: bool,
    // 保存取消意图是否已经持久化。
    pub(super) cancel_requested: bool,
    // 保存按一基索引排列的全部 receipt。
    pub(super) steps: Vec<SequenceStepReceipt>,
    // 保存有界 Workflow 结果或 null。
    pub(super) workflow_result: Value,
}

// 为 execution snapshot 提供只读领域投影。
impl SequenceExecutionSnapshot {
    // 返回稳定 execution identity。
    pub(crate) fn execution_id(&self) -> &str {
        // 借用 canonical 文本。
        &self.execution_id
    }

    // 返回 execution 单调 revision。
    pub(crate) const fn revision(&self) -> u64 {
        // 复制 revision。
        self.execution_revision
    }

    // 返回封闭 execution 状态。
    pub(crate) const fn state(&self) -> SequenceExecutionState {
        // 复制状态。
        self.state
    }

    // 返回全部有序 step receipt。
    pub(crate) fn steps(&self) -> &[SequenceStepReceipt] {
        // 借用受固定上限保护的切片。
        &self.steps
    }

    // 返回可选一基当前步骤。
    pub(crate) fn current_step(&self) -> Option<usize> {
        // 读取并安全收窄无符号整数。
        self.current_step
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
    }
}

// 保存一条严格原子 journal 记录。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SequenceExecutionRecord {
    // 保存私有 journal 版本。
    pub(super) contract_version: String,
    // 保存 record 单调 revision。
    pub(super) record_revision: u64,
    // 保存稳定 execution identity。
    pub(super) execution_id: String,
    // 保存跨 broker 重启 start 去重 nonce。
    pub(super) start_request_nonce: String,
    // 保存完整 start 语义 SHA-256 digest。
    pub(super) start_semantic_digest: String,
    // 保存首次创建时 broker epoch。
    pub(super) created_broker_epoch: String,
    // 保存最近一次状态推进的 broker epoch。
    pub(super) last_broker_epoch: String,
    // 保存规范 input SHA-256 digest。
    pub(super) input_digest: String,
    // 保存完整严格 SequenceInput JSON。
    pub(super) input: Value,
    // 保存 execution 当前状态投影。
    pub(super) snapshot: SequenceExecutionSnapshot,
}

// 为 execution record 提供构造、编解码和状态迁移。
impl SequenceExecutionRecord {
    // 建立 revision 零且全部 step pending 的新 execution。
    pub(crate) fn create(
        // 接收 canonical execution identity。
        execution_id: String,
        // 接收稳定 start request nonce。
        start_request_nonce: String,
        // 接收完整 start 语义 digest。
        start_semantic_digest: String,
        // 接收当前 broker epoch。
        broker_epoch: String,
        // 接收规范 input digest。
        input_digest: String,
        // 接收完整严格 input 值。
        input: Value,
        // 接收按步骤顺序排列的稳定 identity。
        step_ids: Vec<String>,
    ) -> Result<Self, SequenceExecutionRecordError> {
        // 从 input 读取公开步骤数量。
        let total_steps = input
            // 读取 steps 数组。
            .get("steps")
            // 只接受数组。
            .and_then(Value::as_array)
            // 取得数量。
            .map(Vec::len)
            // 错误形状拒绝记录。
            .ok_or(SequenceExecutionRecordError::InvalidRecord)?;
        // 步骤数必须有界并与 identity 数量一致。
        if total_steps == 0
            // 拒绝超过公开上限。
            || total_steps > MAX_SEQUENCE_EXECUTION_STEPS
            // 拒绝 identity 数量漂移。
            || step_ids.len() != total_steps
        {
            // 不建立部分 record。
            return Err(SequenceExecutionRecordError::InvalidRecord);
        }
        // 为每个稳定 identity 建立 pending receipt。
        let steps = step_ids
            // 消耗有序 identity。
            .into_iter()
            // 取得零基位置。
            .enumerate()
            // 构造严格 receipt。
            .map(|(index, step_id)| SequenceStepReceipt {
                // 保存随机 step identity。
                step_id,
                // 转换为一基索引。
                step_index: index + 1,
                // pending 从 revision 零开始。
                step_revision: 0,
                // 尚无最终请求摘要。
                semantic_digest: Value::Null,
                // 尚无 Policy revision。
                policy_revision: Value::Null,
                // 保存 pending 状态。
                state: SequenceStepState::Pending,
                // 尚无 dispatch nonce。
                dispatch_nonce: Value::Null,
                // provider 不可能接受。
                accepted_may_have_occurred: false,
                // 尚无可信 final。
                completed: false,
                // 尚无结果。
                result: Value::Null,
                // 尚无错误。
                error: Value::Null,
                // 尚无执行证据。
                execution_evidence: Value::Null,
            })
            // 收集有序 receipt。
            .collect();
        // 组装 revision 零候选记录。
        let record = Self {
            // 保存固定 journal 版本。
            contract_version: JOURNAL_CONTRACT_VERSION.to_owned(),
            // 新 record 从 revision 零开始。
            record_revision: 0,
            // 保存 execution identity。
            execution_id: execution_id.clone(),
            // 保存跨重启 start nonce。
            start_request_nonce,
            // 保存 start 语义摘要。
            start_semantic_digest,
            // 保存首次 broker epoch。
            created_broker_epoch: broker_epoch.clone(),
            // 最近 epoch 初始等于创建 epoch。
            last_broker_epoch: broker_epoch,
            // 保存 input digest。
            input_digest,
            // 保存完整 input。
            input,
            // 建立状态投影。
            snapshot: SequenceExecutionSnapshot {
                // 保存公开状态版本。
                contract_version: EXECUTION_CONTRACT_VERSION.to_owned(),
                // 绑定同一 execution identity。
                execution_id,
                // snapshot revision 与 record 一致。
                execution_revision: 0,
                // 新 execution 尚未准备步骤。
                state: SequenceExecutionState::Created,
                // created 尚无当前步骤。
                current_step: Value::Null,
                // 保存固定总数。
                total_steps,
                // 正常 start 不等待 resume。
                resumable: false,
                // created 不是终态。
                terminal: false,
                // 尚无未知终态。
                outcome_unknown: false,
                // 尚未请求取消。
                cancel_requested: false,
                // 保存全部稳定 receipt。
                steps,
                // 尚无 Workflow 聚合结果。
                workflow_result: Value::Null,
            },
        };
        // 构造结果必须满足恢复同源验证。
        record.validate()?;
        // 返回严格新记录。
        Ok(record)
    }

    // 严格解码一个完整有界 journal 文档。
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, SequenceExecutionRecordError> {
        // 读取前拒绝超出固定记录预算。
        if bytes.len() > MAX_SEQUENCE_EXECUTION_RECORD_BYTES {
            // 区分资源边界错误。
            return Err(SequenceExecutionRecordError::RecordTooLarge);
        }
        // 空文档不是 JSON record。
        if bytes.is_empty() {
            // 返回严格 parser 错误。
            return Err(SequenceExecutionRecordError::InvalidJson);
        }
        // 严格反序列化并拒绝未知字段。
        let record = serde_json::from_slice::<Self>(bytes)
            // 不泄漏 parser 细节。
            .map_err(|_| SequenceExecutionRecordError::InvalidJson)?;
        // 验证全部恢复不变量。
        record.validate()?;
        // 返回可信记录。
        Ok(record)
    }

    // 编码完整 record 并再次验证字节边界。
    pub(crate) fn encode(&self) -> Result<Vec<u8>, SequenceExecutionRecordError> {
        // 禁止写入无效内存候选。
        self.validate()?;
        // 使用紧凑 JSON 编码完整文档。
        let bytes = serde_json::to_vec(self)
            // 理论序列化失败保持封闭。
            .map_err(|_| SequenceExecutionRecordError::InvalidJson)?;
        // 编码结果必须满足固定预算。
        if bytes.len() > MAX_SEQUENCE_EXECUTION_RECORD_BYTES {
            // 不返回可部分写入的字节。
            return Err(SequenceExecutionRecordError::RecordTooLarge);
        }
        // 返回完整拥有型文档。
        Ok(bytes)
    }

    // 把 pending step 推进为 prepared 候选。
    pub(crate) fn prepare_step(
        // 可变借用领域记录。
        &mut self,
        // 接收调用方观察的 record revision。
        expected_revision: u64,
        // 接收一基步骤索引。
        step_index: usize,
        // 接收最终请求 SHA-256 digest。
        semantic_digest: &str,
        // 接收有界 Policy revision。
        policy_revision: &str,
        // 接收当前 broker epoch。
        broker_epoch: &str,
    ) -> Result<(), SequenceExecutionRecordError> {
        // 在克隆候选上完成单向迁移。
        self.mutate(expected_revision, broker_epoch, |candidate| {
            // 只允许新 execution 或安全恢复点准备下一步。
            if !matches!(
                candidate.snapshot.state,
                // 新执行直接准备第一步。
                SequenceExecutionState::Created | SequenceExecutionState::AwaitingResume
            )
                // 必须严格选择首个尚未建立事实的步骤。
                || candidate.first_pending_step() != Some(step_index)
                // 恢复点必须与持久 currentStep 一致。
                || (candidate.snapshot.state == SequenceExecutionState::AwaitingResume
                    && candidate.snapshot.current_step() != Some(step_index))
            {
                // 禁止跳过或重排步骤。
                return Err(SequenceExecutionRecordError::InvalidTransition);
            }
            // 摘要必须 canonical。
            if !is_digest(semantic_digest) {
                // 拒绝不稳定摘要。
                return Err(SequenceExecutionRecordError::InvalidDigest);
            }
            // Policy revision 必须非空且有界。
            if policy_revision.is_empty() || policy_revision.len() > MAX_POLICY_REVISION_BYTES {
                // 拒绝无法安全持久化的 revision。
                return Err(SequenceExecutionRecordError::InvalidRecord);
            }
            // 取得精确 receipt。
            let step = candidate.step_mut(step_index)?;
            // 只允许 pending 建立 prepared。
            if step.state != SequenceStepState::Pending {
                // 禁止重跑 established step。
                return Err(SequenceExecutionRecordError::InvalidTransition);
            }
            // 推进 step revision。
            step.step_revision = next_revision(step.step_revision)?;
            // 保存最终请求 digest。
            step.semantic_digest = Value::String(semantic_digest.to_owned());
            // 保存 Policy revision。
            step.policy_revision = Value::String(policy_revision.to_owned());
            // 进入可证明未 dispatch 的状态。
            step.state = SequenceStepState::Prepared;
            // execution 开始处理当前步骤。
            candidate.snapshot.state = SequenceExecutionState::Running;
            // 保存一基当前步骤。
            candidate.snapshot.current_step = json!(step_index);
            // 返回候选成功。
            Ok(())
        })
    }

    // 把 prepared step 先持久候选推进为 dispatching。
    pub(crate) fn begin_dispatch(
        // 可变借用领域记录。
        &mut self,
        // 接收调用方观察的 record revision。
        expected_revision: u64,
        // 接收一基步骤索引。
        step_index: usize,
        // 接收稳定 worker dispatch nonce。
        dispatch_nonce: &str,
        // 接收当前 broker epoch。
        broker_epoch: &str,
    ) -> Result<(), SequenceExecutionRecordError> {
        // 在克隆候选上完成单向迁移。
        self.mutate(expected_revision, broker_epoch, |candidate| {
            // dispatch 必须对应当前 running 步骤。
            if candidate.snapshot.state != SequenceExecutionState::Running
                // 当前步骤必须逐值匹配。
                || candidate.snapshot.current_step() != Some(step_index)
            {
                // 禁止跨步骤或非运行状态 dispatch。
                return Err(SequenceExecutionRecordError::InvalidTransition);
            }
            // dispatch nonce 必须 canonical。
            if !is_nonce(dispatch_nonce) {
                // 拒绝不可关联 nonce。
                return Err(SequenceExecutionRecordError::InvalidIdentity);
            }
            // 取得精确 receipt。
            let step = candidate.step_mut(step_index)?;
            // 只有 prepared 可以 dispatch。
            if step.state != SequenceStepState::Prepared {
                // 禁止重复或跳跃 dispatch。
                return Err(SequenceExecutionRecordError::InvalidTransition);
            }
            // 推进 step revision。
            step.step_revision = next_revision(step.step_revision)?;
            // 保存固定 dispatch nonce。
            step.dispatch_nonce = Value::String(dispatch_nonce.to_owned());
            // 先保守标记 provider 可能接受。
            step.accepted_may_have_occurred = true;
            // 进入不可安全重派状态。
            step.state = SequenceStepState::Dispatching;
            // execution 保持运行中。
            candidate.snapshot.state = SequenceExecutionState::Running;
            // 保存精确当前步骤。
            candidate.snapshot.current_step = json!(step_index);
            // 返回候选成功。
            Ok(())
        })
    }

    // 验证 prepared 恢复事实并重新进入 running。
    pub(crate) fn resume_prepared(
        // 可变借用领域记录。
        &mut self,
        // 接收调用方观察的 record revision。
        expected_revision: u64,
        // 接收一基步骤索引。
        step_index: usize,
        // 接收重新计算的最终请求 digest。
        semantic_digest: &str,
        // 接收当前 Policy revision。
        policy_revision: &str,
        // 接收当前 broker epoch。
        broker_epoch: &str,
    ) -> Result<(), SequenceExecutionRecordError> {
        // 在克隆候选上验证全部恢复事实。
        self.mutate(expected_revision, broker_epoch, |candidate| {
            // 只允许精确 awaiting-resume prepared 步骤恢复。
            if candidate.snapshot.state != SequenceExecutionState::AwaitingResume
                // currentStep 必须逐值一致。
                || candidate.snapshot.current_step() != Some(step_index)
                // receipt 状态必须仍为 prepared。
                || candidate.step_state(step_index)? != SequenceStepState::Prepared
            {
                // 禁止从其他状态伪造恢复。
                return Err(SequenceExecutionRecordError::InvalidTransition);
            }
            // 取得只读 receipt 事实。
            let step = candidate
                // 安全转换为零基索引。
                .snapshot
                // 借用全部 receipts。
                .steps
                // 读取精确步骤。
                .get(step_index - 1)
                // 理论越界失败闭合。
                .ok_or(SequenceExecutionRecordError::InvalidTransition)?;
            // digest 与 Policy revision 都必须逐值一致。
            if step.semantic_digest.as_str() != Some(semantic_digest)
                // Policy revision 漂移同样阻止 dispatch。
                || step.policy_revision.as_str() != Some(policy_revision)
            {
                // 不重物化成另一语义。
                return Err(SequenceExecutionRecordError::ResumeConflict);
            }
            // execution 重新进入运行中但不改写 step receipt。
            candidate.snapshot.state = SequenceExecutionState::Running;
            // 返回候选成功。
            Ok(())
        })
    }

    // 在 broker 启动时收敛中断 record。
    pub(crate) fn recover_after_interruption(
        // 可变借用领域记录。
        &mut self,
        // 接收恢复 broker epoch。
        broker_epoch: &str,
    ) -> Result<bool, SequenceExecutionRecordError> {
        // 已有终态或稳定等待点不因读取推进 revision。
        if self.snapshot.state.terminal()
            // awaiting-resume 已经是持久恢复结论。
            || matches!(
                self.snapshot.state,
                // 安全恢复等待点。
                SequenceExecutionState::AwaitingResume
                    // 动态确认等待点同样稳定。
                    | SequenceExecutionState::AwaitingConfirmation
            )
        {
            // 报告没有迁移。
            return Ok(false);
        }
        // 冻结当前 expected revision。
        let expected_revision = self.record_revision;
        // 在克隆候选上执行保守恢复。
        self.mutate(expected_revision, broker_epoch, |candidate| {
            // 取得可选当前步骤。
            let current_step = candidate.snapshot.current_step();
            // dispatching 中断必须永久变成 unknown。
            if let Some(step_index) = current_step
                // 只读取已验证 receipt。
                && candidate.step_state(step_index)? == SequenceStepState::Dispatching
            {
                // 取得当前 receipt。
                let step = candidate.step_mut(step_index)?;
                // 推进 step revision。
                step.step_revision = next_revision(step.step_revision)?;
                // 保存不可重派状态。
                step.state = SequenceStepState::OutcomeUnknown;
                // unknown 不伪造可信完成。
                step.completed = false;
                // 保存固定安全错误。
                step.error = json!({
                    // 使用稳定错误码。
                    "code": "OUTCOME_UNKNOWN",
                    // 不推测 provider 事实。
                    "message": "The sequence step was interrupted after dispatch may have occurred."
                });
                // 保存最小恢复证据。
                step.execution_evidence = json!({
                    // 标记恢复路径。
                    "recoveredAfterInterruption": true,
                    // 保留最后可靠事实。
                    "lastReliableObservation": "dispatching-receipt"
                });
                // 整个 execution 进入未知终态。
                candidate.snapshot.state = SequenceExecutionState::OutcomeUnknown;
                // 返回候选成功。
                return Ok(());
            }
            // 已持久取消且从未 dispatch 的记录直接建立取消终态。
            if candidate.snapshot.cancel_requested {
                // 选择既有或首个 pending 步骤定位。
                let cancelled_step = current_step
                    // created 尚无 currentStep 时使用第一 pending。
                    .or_else(|| candidate.first_pending_step())
                    // 非终态必须仍有步骤。
                    .ok_or(SequenceExecutionRecordError::InvalidRecord)?;
                // 建立不可倒退取消终态。
                candidate.snapshot.state = SequenceExecutionState::Cancelled;
                // 保存精确取消位置。
                candidate.snapshot.current_step = json!(cancelled_step);
                // 保存固定最小 Workflow 结果。
                candidate.snapshot.workflow_result = transitions::cancellation_workflow_result();
                // 返回候选成功。
                return Ok(());
            }
            // created、pending 或 prepared 进入安全等待点。
            let resume_step = current_step
                // 无当前步骤时选择首个 pending。
                .or_else(|| candidate.first_pending_step())
                // 非终态必须仍有步骤。
                .ok_or(SequenceExecutionRecordError::InvalidRecord)?;
            // 等待显式 expected-revision resume。
            candidate.snapshot.state = SequenceExecutionState::AwaitingResume;
            // 保存恢复起点。
            candidate.snapshot.current_step = json!(resume_step);
            // 返回候选成功。
            Ok(())
        })?;
        // 报告已经建立新候选。
        Ok(true)
    }

    // 返回稳定 execution identity。
    pub(crate) fn execution_id(&self) -> &str {
        // 借用 canonical 文本。
        &self.execution_id
    }

    // 返回 record 单调 revision。
    pub(crate) const fn revision(&self) -> u64 {
        // 复制 revision。
        self.record_revision
    }

    // 返回当前状态投影。
    pub(crate) const fn snapshot(&self) -> &SequenceExecutionSnapshot {
        // 借用不可变 snapshot。
        &self.snapshot
    }

    // 在克隆候选上执行单次 revision 迁移。
    fn mutate(
        // 可变借用原记录。
        &mut self,
        // 接收 expected revision。
        expected_revision: u64,
        // 接收当前 broker epoch。
        broker_epoch: &str,
        // 接收只修改候选的领域迁移。
        transition: impl FnOnce(&mut Self) -> Result<(), SequenceExecutionRecordError>,
    ) -> Result<(), SequenceExecutionRecordError> {
        // expected revision 必须相等。
        if self.record_revision != expected_revision {
            // 拒绝 stale Command。
            return Err(SequenceExecutionRecordError::RevisionConflict);
        }
        // broker epoch 必须 canonical。
        if !is_nonce(broker_epoch) {
            // 拒绝身份漂移。
            return Err(SequenceExecutionRecordError::InvalidIdentity);
        }
        // 克隆候选保证失败无部分修改。
        let mut candidate = self.clone();
        // 执行领域迁移。
        transition(&mut candidate)?;
        // 推进 record revision。
        candidate.record_revision = next_revision(candidate.record_revision)?;
        // 同步 snapshot revision。
        candidate.snapshot.execution_revision = candidate.record_revision;
        // 保存最近 broker epoch。
        candidate.last_broker_epoch = broker_epoch.to_owned();
        // 刷新派生布尔投影。
        candidate.refresh_derived_state();
        // 完整候选必须通过恢复同源验证。
        candidate.validate()?;
        // 只在全部成功后替换原记录。
        *self = candidate;
        // 报告迁移完成。
        Ok(())
    }

    // 按一基索引读取 step 状态。
    fn step_state(
        // 借用 record。
        &self,
        // 接收一基步骤索引。
        step_index: usize,
    ) -> Result<SequenceStepState, SequenceExecutionRecordError> {
        // 安全转换并读取状态。
        step_index
            // 拒绝零值下溢。
            .checked_sub(1)
            // 读取 receipt。
            .and_then(|index| self.snapshot.steps.get(index))
            // 复制封闭状态。
            .map(|step| step.state)
            // 越界拒绝迁移。
            .ok_or(SequenceExecutionRecordError::InvalidTransition)
    }

    // 按一基索引可变读取 receipt。
    fn step_mut(
        // 可变借用 record。
        &mut self,
        // 接收一基步骤索引。
        step_index: usize,
    ) -> Result<&mut SequenceStepReceipt, SequenceExecutionRecordError> {
        // 安全转换并读取 receipt。
        step_index
            // 拒绝零值下溢。
            .checked_sub(1)
            // 可变读取 receipt。
            .and_then(|index| self.snapshot.steps.get_mut(index))
            // 越界拒绝迁移。
            .ok_or(SequenceExecutionRecordError::InvalidTransition)
    }

    // 查找首个 pending receipt 的一基索引。
    fn first_pending_step(&self) -> Option<usize> {
        // 按固定顺序查找 pending。
        self.snapshot
            // 借用全部 steps。
            .steps
            // 创建迭代器。
            .iter()
            // 查找首个 pending。
            .find(|step| step.state == SequenceStepState::Pending)
            // 返回一基索引。
            .map(|step| step.step_index)
    }

    // 刷新 execution 状态唯一决定的布尔投影。
    fn refresh_derived_state(&mut self) {
        // resumable 只在安全等待点为真。
        self.snapshot.resumable = self.snapshot.state.resumable();
        // terminal 映射四个终态。
        self.snapshot.terminal = self.snapshot.state.terminal();
        // unknown 布尔必须与状态一致。
        self.snapshot.outcome_unknown =
            self.snapshot.state == SequenceExecutionState::OutcomeUnknown;
    }
}

// 把 Module 单元测试拆到独立文件以保持生产文件低于 900 行。
#[cfg(test)]
#[path = "sequence_execution_tests.rs"]
mod tests;
