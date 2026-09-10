//! 验证 sequence execution journal 的版本、身份与跨字段不变量。

// 导入集合以拒绝重复 step identity。
use std::collections::HashSet;
// 导入 provider-neutral JSON 值。
use serde_json::Value;

// 导入跨状态机与 journal 共用的 canonical 文本验证。
use crate::components::sequence_execution_identity::{
    execution_fingerprint, is_digest, is_nonce, step_fingerprint,
};

// 导入父 Module 的记录、状态、错误与硬边界。
use super::{
    EXECUTION_CONTRACT_VERSION, JOURNAL_CONTRACT_VERSION, MAX_POLICY_REVISION_BYTES,
    MAX_SEQUENCE_EXECUTION_STEPS, MAX_SEQUENCE_INPUT_BYTES, SequenceExecutionRecord,
    SequenceExecutionRecordError, SequenceExecutionState, SequenceStepReceipt, SequenceStepState,
};

// 为记录类型补充同一 Module 所有权内的完整验证。
impl SequenceExecutionRecord {
    // 验证版本、identity、预算与完整跨字段不变量。
    pub(super) fn validate(&self) -> Result<(), SequenceExecutionRecordError> {
        // 两个契约版本必须精确匹配。
        if self.contract_version != JOURNAL_CONTRACT_VERSION
            // snapshot 使用独立状态版本。
            || self.snapshot.contract_version != EXECUTION_CONTRACT_VERSION
        {
            // 未知版本不得猜测迁移。
            return Err(SequenceExecutionRecordError::InvalidRecord);
        }
        // execution identity 两处必须 canonical 且一致。
        if execution_fingerprint(&self.execution_id).is_none()
            // snapshot 绑定相同 identity。
            || self.snapshot.execution_id != self.execution_id
        {
            // 拒绝 identity 漂移。
            return Err(SequenceExecutionRecordError::InvalidIdentity);
        }
        // 全部 nonce 与 epoch 必须 canonical。
        if !is_nonce(&self.start_request_nonce)
            // 验证创建 epoch。
            || !is_nonce(&self.created_broker_epoch)
            // 验证最近 epoch。
            || !is_nonce(&self.last_broker_epoch)
        {
            // 拒绝不可关联身份。
            return Err(SequenceExecutionRecordError::InvalidIdentity);
        }
        // start 与 input digest 必须 canonical。
        if !is_digest(&self.start_semantic_digest) || !is_digest(&self.input_digest) {
            // 拒绝摘要漂移。
            return Err(SequenceExecutionRecordError::InvalidDigest);
        }
        // record 与 snapshot revision 必须相等。
        if self.record_revision != self.snapshot.execution_revision {
            // 拒绝双 revision 漂移。
            return Err(SequenceExecutionRecordError::InvalidRecord);
        }
        // input 必须满足共享字节预算。
        let input_bytes = serde_json::to_vec(&self.input)
            // Value 编码失败视为损坏。
            .map_err(|_| SequenceExecutionRecordError::InvalidJson)?;
        // 拒绝超限 input。
        if input_bytes.len() > MAX_SEQUENCE_INPUT_BYTES {
            // 不恢复超限文档。
            return Err(SequenceExecutionRecordError::InvalidRecord);
        }
        // 读取 input 步骤数。
        let input_steps = self
            // 读取 steps 字段。
            .input
            // 只接受数组。
            .get("steps")
            // 转换为数组。
            .and_then(Value::as_array)
            // 取得数量。
            .map(Vec::len)
            // 缺失时记录损坏。
            .ok_or(SequenceExecutionRecordError::InvalidRecord)?;
        // 三处步骤数量必须一致且有界。
        if input_steps == 0
            // 拒绝超上限。
            || input_steps > MAX_SEQUENCE_EXECUTION_STEPS
            // 总数必须一致。
            || self.snapshot.total_steps != input_steps
            // receipt 数量必须一致。
            || self.snapshot.steps.len() != input_steps
        {
            // 拒绝步骤集合漂移。
            return Err(SequenceExecutionRecordError::InvalidRecord);
        }
        // 派生布尔必须与状态一致。
        if self.snapshot.resumable != self.snapshot.state.resumable()
            // 验证终态分类。
            || self.snapshot.terminal != self.snapshot.state.terminal()
            // 验证未知分类。
            || self.snapshot.outcome_unknown
                != (self.snapshot.state == SequenceExecutionState::OutcomeUnknown)
        {
            // 拒绝伪造状态投影。
            return Err(SequenceExecutionRecordError::InvalidRecord);
        }
        // 解析可选一基当前步骤。
        let current_step = nullable_index(&self.snapshot.current_step)?;
        // 当前步骤必须落在范围内。
        if current_step.is_some_and(|index| index == 0 || index > input_steps) {
            // 拒绝越界恢复位置。
            return Err(SequenceExecutionRecordError::InvalidRecord);
        }
        // created 唯一允许没有当前步骤。
        if (self.snapshot.state == SequenceExecutionState::Created) != current_step.is_none() {
            // 拒绝状态与位置漂移。
            return Err(SequenceExecutionRecordError::InvalidRecord);
        }
        // 收集 identity 以拒绝重复。
        let mut identities = HashSet::with_capacity(input_steps);
        // 逐项验证 receipt 顺序与 payload。
        for (index, step) in self.snapshot.steps.iter().enumerate() {
            // identity 必须 canonical 且唯一。
            if step_fingerprint(&step.step_id).is_none()
                // 重复 identity 使恢复歧义。
                || !identities.insert(step.step_id.as_str())
                // 一基索引必须与数组顺序一致。
                || step.step_index != index + 1
            {
                // 拒绝 identity 或顺序漂移。
                return Err(SequenceExecutionRecordError::InvalidIdentity);
            }
            // 验证 receipt 状态组合。
            validate_step(step)?;
        }
        // workflowResult 必须是 required nullable 对象。
        if !self.snapshot.workflow_result.is_null() && !self.snapshot.workflow_result.is_object() {
            // 拒绝 schema 外结果形状。
            return Err(SequenceExecutionRecordError::InvalidRecord);
        }
        // 按 execution 状态验证唯一合法步骤前缀、当前项与后缀。
        match self.snapshot.state {
            // created 只允许全部 pending 且没有取消或结果事实。
            SequenceExecutionState::Created => {
                // 任一步骤非 pending 都是漂移。
                if self
                    // 借用全部 receipts。
                    .snapshot
                    // 读取 steps。
                    .steps
                    // 创建迭代器。
                    .iter()
                    // 查找非 pending。
                    .any(|step| step.state != SequenceStepState::Pending)
                    // created 不得提前持久取消。
                    || self.snapshot.cancel_requested
                    // created 不得提前发布 Workflow 结果。
                    || !self.snapshot.workflow_result.is_null()
                {
                    // 拒绝伪造 created。
                    return Err(SequenceExecutionRecordError::InvalidRecord);
                }
            }
            // running 指向唯一 prepared 或 dispatching 当前步骤。
            SequenceExecutionState::Running => {
                // 当前索引已经由通用边界验证。
                let current = current_step.ok_or(SequenceExecutionRecordError::InvalidRecord)?;
                // 前缀必须确定、当前必须活动、后缀必须 pending。
                if !ordered_steps_match(&self.snapshot.steps, current, |step| {
                    // 只允许两个运行中状态。
                    matches!(step.state, SequenceStepState::Prepared | SequenceStepState::Dispatching)
                })
                    // 运行中不得提前发布 Workflow 结果。
                    || !self.snapshot.workflow_result.is_null()
                    // 取消请求只有 dispatching 时能保持 running。
                    || (self.snapshot.cancel_requested
                        && self.snapshot.steps[current - 1].state
                            != SequenceStepState::Dispatching)
                {
                    // 拒绝假 running 或失序 receipt。
                    return Err(SequenceExecutionRecordError::InvalidRecord);
                }
            }
            // 安全恢复点指向 pending 或 prepared 当前步骤。
            SequenceExecutionState::AwaitingResume => {
                // 当前索引已经由通用边界验证。
                let current = current_step.ok_or(SequenceExecutionRecordError::InvalidRecord)?;
                // 核对确定前缀、安全当前项与 pending 后缀。
                if !ordered_steps_match(&self.snapshot.steps, current, |step| {
                    // 尚未 dispatch 的两个状态可恢复。
                    matches!(step.state, SequenceStepState::Pending | SequenceStepState::Prepared)
                })
                    // 已请求取消应直接收敛而不是等待 resume。
                    || self.snapshot.cancel_requested
                    // 等待点不得提前发布 Workflow 结果。
                    || !self.snapshot.workflow_result.is_null()
                {
                    // 拒绝不可安全恢复的组合。
                    return Err(SequenceExecutionRecordError::InvalidRecord);
                }
            }
            // 确认等待点为 #2389 保留 prepared 当前步骤。
            SequenceExecutionState::AwaitingConfirmation => {
                // 当前索引已经由通用边界验证。
                let current = current_step.ok_or(SequenceExecutionRecordError::InvalidRecord)?;
                // 只能停在未 dispatch prepared receipt。
                if !ordered_steps_match(&self.snapshot.steps, current, |step| {
                    // 动态确认必须绑定已经物化的 prepared 候选。
                    step.state == SequenceStepState::Prepared
                })
                    // 当前版本不允许确认与取消事实混合。
                    || self.snapshot.cancel_requested
                    // 等待确认不是 Workflow 终态。
                    || !self.snapshot.workflow_result.is_null()
                {
                    // 拒绝伪造确认等待点。
                    return Err(SequenceExecutionRecordError::InvalidRecord);
                }
            }
            // completed 表示全部声明步骤已处理到末尾。
            SequenceExecutionState::Completed => {
                // 最后定位必须是固定总步骤数。
                if current_step != Some(self.snapshot.total_steps)
                    // 每个步骤必须有可信成功或失败 final。
                    || self.snapshot.steps.iter().any(|step| {
                        // 普通失败在 continueOnError 下仍可运行到末尾。
                        !matches!(step.state, SequenceStepState::Completed | SequenceStepState::Failed)
                    })
                    // 完整执行必须发布 Workflow 结果对象。
                    || !self.snapshot.workflow_result.is_object()
                {
                    // 拒绝伪造整体完成。
                    return Err(SequenceExecutionRecordError::InvalidRecord);
                }
            }
            // failed 指向首个停止后续步骤的可信失败。
            SequenceExecutionState::Failed => {
                // 当前索引已经由通用边界验证。
                let current = current_step.ok_or(SequenceExecutionRecordError::InvalidRecord)?;
                // 前缀确定、当前失败、后缀仍 pending。
                if !ordered_steps_match(&self.snapshot.steps, current, |step| {
                    // 终止来源必须是可信失败 final。
                    step.state == SequenceStepState::Failed
                })
                    // 失败终态必须发布 Workflow 结果对象。
                    || !self.snapshot.workflow_result.is_object()
                {
                    // 拒绝失序失败终态。
                    return Err(SequenceExecutionRecordError::InvalidRecord);
                }
            }
            // cancelled 必须携带取消意图和安全或权威停止位置。
            SequenceExecutionState::Cancelled => {
                // 当前索引已经由通用边界验证。
                let current = current_step.ok_or(SequenceExecutionRecordError::InvalidRecord)?;
                // 前缀确定、当前未 dispatch 或已权威取消、后缀 pending。
                if !ordered_steps_match(&self.snapshot.steps, current, |step| {
                    // 未 dispatch 可直接取消；dispatch 后用 CANCELLED 失败 final。
                    matches!(step.state, SequenceStepState::Pending | SequenceStepState::Prepared)
                        || (step.state == SequenceStepState::Failed
                            && step.error_code() == Some("CANCELLED"))
                })
                    // 取消终态必须保留取消请求事实。
                    || !self.snapshot.cancel_requested
                    // 取消终态必须发布最小 Workflow 结果。
                    || !self.snapshot.workflow_result.is_object()
                {
                    // 拒绝伪造取消事实。
                    return Err(SequenceExecutionRecordError::InvalidRecord);
                }
            }
            // outcome-unknown 指向唯一不可重派 receipt。
            SequenceExecutionState::OutcomeUnknown => {
                // 当前索引已经由通用边界验证。
                let current = current_step.ok_or(SequenceExecutionRecordError::InvalidRecord)?;
                // 前缀确定、当前 unknown、后缀 pending。
                if !ordered_steps_match(&self.snapshot.steps, current, |step| {
                    // 当前步骤必须保存未知终态。
                    step.state == SequenceStepState::OutcomeUnknown
                })
                    // 未知终态不伪造完整 Workflow 结果。
                    || !self.snapshot.workflow_result.is_null()
                {
                    // 拒绝丢失未知来源。
                    return Err(SequenceExecutionRecordError::InvalidRecord);
                }
            }
        }
        // 全部不变量成立。
        Ok(())
    }
}

// 验证当前步骤前后唯一允许的顺序形状。
fn ordered_steps_match(
    // 借用全部有序 receipts。
    steps: &[SequenceStepReceipt],
    // 接收一基当前步骤。
    current: usize,
    // 接收当前 receipt 的封闭状态判断。
    current_matches: impl Fn(&SequenceStepReceipt) -> bool,
) -> bool {
    // 逐项核对确定前缀、当前项与 pending 后缀。
    steps.iter().all(|step| {
        // 按一基索引分类。
        if step.step_index < current {
            // 只有可信 completed/failed 可以位于当前步骤之前。
            matches!(
                step.state,
                SequenceStepState::Completed | SequenceStepState::Failed
            )
        } else if step.step_index == current {
            // 当前项使用调用方状态判断。
            current_matches(step)
        } else {
            // 后续步骤必须保持从未物化的 pending。
            step.state == SequenceStepState::Pending
        }
    })
}

// 验证单个 receipt 的状态与字段组合。
fn validate_step(step: &SequenceStepReceipt) -> Result<(), SequenceExecutionRecordError> {
    // 读取三个 required nullable 字符串。
    let digest = nullable_string(&step.semantic_digest)?;
    // 读取 Policy revision。
    let policy = nullable_string(&step.policy_revision)?;
    // 读取 dispatch nonce。
    let nonce = nullable_string(&step.dispatch_nonce)?;
    // 已建立 digest 时必须 canonical。
    if digest.is_some_and(|value| !is_digest(value)) {
        // 拒绝摘要漂移。
        return Err(SequenceExecutionRecordError::InvalidDigest);
    }
    // Policy revision 必须非空且有界。
    if policy.is_some_and(|value| value.is_empty() || value.len() > MAX_POLICY_REVISION_BYTES) {
        // 拒绝无效 revision。
        return Err(SequenceExecutionRecordError::InvalidRecord);
    }
    // dispatch nonce 必须 canonical。
    if nonce.is_some_and(|value| !is_nonce(value)) {
        // 拒绝不可关联 nonce。
        return Err(SequenceExecutionRecordError::InvalidIdentity);
    }
    // 按状态选择固定字段形状。
    let valid = match step.state {
        // pending 只允许 revision 零空事实。
        SequenceStepState::Pending => {
            // 核对全部空字段。
            step.step_revision == 0
                && digest.is_none()
                && policy.is_none()
                && nonce.is_none()
                && !step.accepted_may_have_occurred
                && !step.completed
                && empty_payload(step)
        }
        // prepared 只增加 digest 与 Policy revision。
        SequenceStepState::Prepared => {
            // 核对首次迁移事实。
            step.step_revision == 1
                && digest.is_some()
                && policy.is_some()
                && nonce.is_none()
                && !step.accepted_may_have_occurred
                && !step.completed
                && empty_payload(step)
        }
        // dispatching 必须先建立可能接受事实。
        SequenceStepState::Dispatching => {
            // 核对第二次迁移事实。
            step.step_revision == 2
                && digest.is_some()
                && policy.is_some()
                && nonce.is_some()
                && step.accepted_may_have_occurred
                && !step.completed
                && empty_payload(step)
        }
        // completed 必须携带结果与证据且无错误。
        SequenceStepState::Completed => {
            // 核对成功 final 事实。
            terminal_base(step, digest, policy, nonce)
                && step.completed
                && step.error.is_null()
                && step.execution_evidence.is_object()
        }
        // failed 必须携带错误与证据且无结果。
        SequenceStepState::Failed => {
            // 核对失败 final 事实。
            terminal_base(step, digest, policy, nonce)
                && step.completed
                && step.result.is_null()
                && is_safe_error(&step.error)
                && step.execution_evidence.is_object()
        }
        // unknown 必须携带固定错误且不伪造完成。
        SequenceStepState::OutcomeUnknown => {
            // 核对未知终态事实。
            terminal_base(step, digest, policy, nonce)
                && !step.completed
                && step.result.is_null()
                && step.error.get("code").and_then(Value::as_str) == Some("OUTCOME_UNKNOWN")
                && step.execution_evidence.is_object()
        }
    };
    // 无效组合统一视为记录损坏。
    if !valid {
        // 返回封闭错误。
        return Err(SequenceExecutionRecordError::InvalidRecord);
    }
    // 当前 receipt 合法。
    Ok(())
}

// 判断非终态 receipt 的三个 payload 都为空。
fn empty_payload(step: &SequenceStepReceipt) -> bool {
    // result、error 与 evidence 必须都是 null。
    step.result.is_null() && step.error.is_null() && step.execution_evidence.is_null()
}

// 验证三个 dispatch 基础事实与 revision 下界。
fn terminal_base(
    // 借用 receipt。
    step: &SequenceStepReceipt,
    // 接收可选 digest。
    digest: Option<&str>,
    // 接收可选 Policy revision。
    policy: Option<&str>,
    // 接收可选 dispatch nonce。
    nonce: Option<&str>,
) -> bool {
    // 终态至少经历三次迁移并保留 dispatch 事实。
    step.step_revision >= 3
        && digest.is_some()
        && policy.is_some()
        && nonce.is_some()
        && step.accepted_may_have_occurred
}

// 解析 required nullable 一基索引。
fn nullable_index(value: &Value) -> Result<Option<usize>, SequenceExecutionRecordError> {
    // 只接受 null 或可收窄整数。
    match value {
        // null 表示没有当前步骤。
        Value::Null => Ok(None),
        // 数字必须为无符号平台整数。
        Value::Number(number) => number
            // 读取并收窄。
            .as_u64()
            // 转换为 usize。
            .and_then(|value| usize::try_from(value).ok())
            // 包装存在值。
            .map(Some)
            // 非法数字拒绝。
            .ok_or(SequenceExecutionRecordError::InvalidRecord),
        // 其他类型拒绝。
        _ => Err(SequenceExecutionRecordError::InvalidRecord),
    }
}

// 解析 required nullable 字符串。
fn nullable_string(value: &Value) -> Result<Option<&str>, SequenceExecutionRecordError> {
    // 只接受 null 或字符串。
    match value {
        // null 表示事实尚未建立。
        Value::Null => Ok(None),
        // 字符串返回借用。
        Value::String(text) => Ok(Some(text)),
        // 其他类型拒绝。
        _ => Err(SequenceExecutionRecordError::InvalidRecord),
    }
}

// 验证安全错误对象的最小封闭字段。
pub(super) fn is_safe_error(value: &Value) -> bool {
    // 错误必须是对象。
    let Some(object) = value.as_object() else {
        // 拒绝非对象。
        return false;
    };
    // 只允许三个稳定字段。
    if object
        // 遍历字段名。
        .keys()
        // 查找未知字段。
        .any(|key| !matches!(key.as_str(), "code" | "message" | "details"))
    {
        // 保持 envelope 封闭。
        return false;
    }
    // 验证稳定错误码。
    let code = object
        // 读取 code。
        .get("code")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 验证字符集与字节上限。
        .is_some_and(|code| {
            // code 非空且有界。
            !code.is_empty()
                && code.len() <= 128
                // 只接受大写、数字和下划线。
                && code.bytes().all(|byte| {
                    // 返回单字节分类。
                    byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                })
        });
    // 验证有界错误消息。
    let message = object
        // 读取 message。
        .get("message")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 验证非空和字符上限。
        .is_some_and(|message| !message.is_empty() && message.chars().count() <= 512);
    // details 缺失或对象均合法。
    let details = object
        // 读取可选 details。
        .get("details")
        // 缺失保持合法。
        .is_none_or(Value::is_object);
    // 返回完整判断。
    code && message && details
}

// 计算下一个单调 revision 并拒绝溢出。
pub(super) fn next_revision(value: u64) -> Result<u64, SequenceExecutionRecordError> {
    // checked add 防止 revision 回绕。
    value
        // 单次迁移严格加一。
        .checked_add(1)
        // 达到边界时失败闭合。
        .ok_or(SequenceExecutionRecordError::RevisionExhausted)
}
