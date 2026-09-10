//! 拥有长操作记录、容量、原子迁移、恢复与到期语义。

// 导入有界内存索引。
use std::collections::HashMap;

// 导入严格 JSON 派生与值类型。
use serde::{Deserialize, Serialize};
// 导入 provider-neutral JSON 结果。
use serde_json::Value;

// 导入 journal Component 与 opaque operation 原语。
use crate::components::long_operation_journal::{
    // 使用原子单记录持久化边界。
    LongOperationJournal,
    // 保留封闭 journal 错误分类。
    LongOperationJournalError,
};

// 导入领域状态机与固定预算。
use super::long_operation::{
    // 导入取消幂等效果。
    CancelRequestEffect,
    // 导入状态机和公开状态。
    LongOperationState,
    LongOperationStatus,
    // 导入资源预算。
    MAX_ACTIVE_OPERATIONS,
    MAX_RESULT_BYTES,
    MAX_TRACKED_OPERATIONS,
    TERMINAL_RETENTION_SECONDS,
};

// 导入 registry 私有记录 schema 与持久化助手。
#[path = "long_operation_registry_record.rs"]
mod record_support;
// 只在当前 Module 内复用严格记录操作。
use record_support::{
    // 导入 journal 解码与时间检查。
    decode_record,
    ensure_monotonic_time,
    // 导入候选收口与原子持久化。
    finalize_mutation,
    // 导入 handle、错误与恢复验证。
    fixed_failure,
    is_valid_error_code,
    is_valid_error_message,
    persist_record,
    recovery_failure,
    validate_operation_id,
    validate_record,
};

// 固定私有 journal 文档版本。
const JOURNAL_CONTRACT_VERSION: &str = "act/long-operation-journal/v1";
// 固定首个长操作 capability。
const WINDOW_RECORD_CAPABILITY: &str = "window.record@1";
// 将终态保留期换算为毫秒。
const TERMINAL_RETENTION_MILLISECONDS: u64 = TERMINAL_RETENTION_SECONDS * 1_000;
// 限制稳定错误码字节数。
const MAX_ERROR_CODE_BYTES: usize = 64;
// 限制稳定错误消息字符数。
const MAX_ERROR_MESSAGE_CHARACTERS: usize = 512;

// 表示 registry 对调用者返回的封闭失败集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LongOperationRegistryError {
    // journal 文件边界或原子提交失败。
    Journal(LongOperationJournalError),
    // operation handle 不是 canonical s2:o。
    InvalidOperation,
    // capability 不属于当前封闭版本。
    InvalidCapability,
    // journal schema、字段组合或版本不合法。
    InvalidRecord,
    // 同一 handle 已经存在。
    DuplicateOperation,
    // 活动或总记录预算已耗尽。
    CapacityExhausted,
    // handle 不存在、已过期或不属于本 broker。
    OperationNotFound,
    // 调用时钟早于既有持久事实。
    ClockRegression,
    // 状态机拒绝当前迁移。
    InvalidTransition,
    // 错误码或消息不符合稳定边界。
    InvalidFailure,
}

// 将 journal 失败提升为 Module 失败。
impl From<LongOperationJournalError> for LongOperationRegistryError {
    // 保留封闭 Component 错误类别。
    fn from(error: LongOperationJournalError) -> Self {
        // 包装而不泄漏路径或平台错误。
        Self::Journal(error)
    }
}

// 保存终态失败的稳定公开事实。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
// 固定 camelCase 并拒绝 journal 扩展字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LongOperationFailure {
    // 保存稳定大写错误码。
    code: String,
    // 保存不含路径或 payload 的有界消息。
    message: String,
}

// 为稳定失败提供验证构造与只读投影。
impl LongOperationFailure {
    // 构造经过边界验证的失败事实。
    pub(crate) fn new(code: &str, message: &str) -> Option<Self> {
        // 拒绝非法错误码或消息。
        if !is_valid_error_code(code) || !is_valid_error_message(message) {
            // 不建立宽松失败对象。
            return None;
        }
        // 复制已经验证的安全文本。
        Some(Self {
            // 保存错误码。
            code: code.to_owned(),
            // 保存错误消息。
            message: message.to_owned(),
        })
    }

    // 返回稳定错误码。
    pub(crate) fn code(&self) -> &str {
        // 借用私有错误码。
        &self.code
    }

    // 返回有界错误消息。
    pub(crate) fn message(&self) -> &str {
        // 借用私有错误消息。
        &self.message
    }

    // 验证反序列化后的字段仍满足稳定边界。
    fn is_valid(&self) -> bool {
        // 复用构造规则。
        is_valid_error_code(&self.code) && is_valid_error_message(&self.message)
    }
}

// 保存一条可查询的长操作领域记录。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LongOperationRecord {
    // 保存 canonical operation handle。
    operation_id: String,
    // 保存封闭 capability ID。
    capability_id: String,
    // 保存状态机最小事实。
    state: LongOperationState,
    // 保存取消是否曾经被持久接受。
    cancel_requested: bool,
    // 保存单调持久修订号。
    revision: u64,
    // 保存首次接受时间戳。
    created_at_ms: u64,
    // 保存最后持久迁移时间戳。
    updated_at_ms: u64,
    // 保存终态固定到期时间。
    expires_at_ms: Option<u64>,
    // 保存 completed 的有界 JSON 结果。
    result: Option<Value>,
    // 保存 failed 或 outcome-unknown 的稳定错误。
    error: Option<LongOperationFailure>,
}

// 为领域记录提供只读投影。
impl LongOperationRecord {
    // 返回 operation handle。
    pub(crate) fn operation_id(&self) -> &str {
        // 借用 canonical handle。
        &self.operation_id
    }

    // 返回 capability ID。
    pub(crate) fn capability_id(&self) -> &str {
        // 借用封闭 capability。
        &self.capability_id
    }

    // 返回公开状态。
    pub(crate) const fn status(&self) -> LongOperationStatus {
        // 投影状态机状态。
        self.state.status()
    }

    // 返回不可逆 dispatch 事实。
    pub(crate) const fn dispatch_started(&self) -> bool {
        // 投影状态机事实。
        self.state.dispatch_started()
    }

    // 返回取消是否曾经被持久接受。
    pub(crate) const fn cancel_requested(&self) -> bool {
        // 返回独立取消事实。
        self.cancel_requested
    }

    // 返回记录是否处于终态。
    pub(crate) const fn terminal(&self) -> bool {
        // 使用状态机终态分类。
        self.state.status().is_terminal()
    }

    // 返回原操作是否允许安全重提。
    pub(crate) const fn retry_safe(&self) -> bool {
        // 投影状态机保守重试语义。
        self.state.retry_safe()
    }

    // 返回单调修订号。
    pub(crate) const fn revision(&self) -> u64 {
        // 复制整数事实。
        self.revision
    }

    // 返回终态到期时间。
    pub(crate) const fn expires_at_ms(&self) -> Option<u64> {
        // 复制可选整数事实。
        self.expires_at_ms
    }

    // 返回有界 JSON 结果。
    pub(crate) const fn result(&self) -> Option<&Value> {
        // 借用可选结果。
        self.result.as_ref()
    }

    // 返回稳定终态错误。
    pub(crate) const fn error(&self) -> Option<&LongOperationFailure> {
        // 借用可选错误。
        self.error.as_ref()
    }
}

// 表示 complete Command 的领域效果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompletionEffect {
    // 有界结果已经持久完成。
    Completed,
    // 超限结果已经持久失败且未被截断。
    FailedResultTooLarge,
}

// 拥有同一 broker 代际内的全部长操作记录。
pub(crate) struct LongOperationRegistry {
    // 独占持久化 Component 生命周期。
    journal: LongOperationJournal,
    // 以内存索引提供无磁盘扫描查询。
    records: HashMap<String, LongOperationRecord>,
}

// 提供 registry 启动恢复与全部状态迁移入口。
impl LongOperationRegistry {
    // 严格加载 journal 并把中断记录持久收敛为终态。
    pub(crate) fn open(
        // 转移 journal 所有权。
        journal: LongOperationJournal,
        // 接收 broker 启动时刻。
        now_ms: u64,
    ) -> Result<Self, LongOperationRegistryError> {
        // 严格加载文件边界内的全部文档。
        let documents = journal.load()?;
        // final 记录数不得超过 registry 固定预算。
        if documents.len() > MAX_TRACKED_OPERATIONS {
            // 拒绝从磁盘静默扩大预算。
            return Err(LongOperationRegistryError::CapacityExhausted);
        }
        // 预留受预算限制的内存索引。
        let mut records = HashMap::with_capacity(documents.len());
        // 逐条验证 schema、文件名绑定与恢复语义。
        for document in documents {
            // 严格解码 schema 并验证文件名绑定。
            let mut record = decode_record(document.operation_id(), document.bytes())?;
            // 已到期终态先撤销持久索引。
            if record
                // 读取可选终态时间。
                .expires_at_ms
                // 到期边界使用大于等于。
                .is_some_and(|expires_at| expires_at <= now_ms)
            {
                // 删除精确 final 记录。
                journal.remove(&record.operation_id)?;
                // 不向内存索引暴露到期记录。
                continue;
            }
            // 非终态记录必须在 broker 启动时收敛。
            if !record.terminal() {
                // 拒绝时钟回退覆盖更新事实。
                ensure_monotonic_time(&record, now_ms)?;
                // 按 dispatch 事实执行保守恢复。
                let effect = record.state.recover_after_interruption();
                // 恢复必定为两个终态之一。
                record.error = Some(recovery_failure(effect)?);
                // 恢复不产生结果。
                record.result = None;
                // 完成修订、时间与固定到期计算。
                finalize_mutation(&mut record, now_ms)?;
                // 启动成功前先持久化恢复事实。
                persist_record(&journal, &record)?;
            }
            // 重复 handle 表示 journal 不一致。
            if records
                // 插入 canonical handle 索引。
                .insert(record.operation_id.clone(), record)
                // 发现既有值即为重复。
                .is_some()
            {
                // 拒绝任取一个记录。
                return Err(LongOperationRegistryError::InvalidRecord);
            }
        }
        // 返回已经恢复且有界的 registry。
        Ok(Self {
            // 保留 journal 唯一所有权。
            journal,
            // 保留无扫描索引。
            records,
        })
    }

    // 原子接受一个尚未 dispatch 的新任务。
    pub(crate) fn accept(
        // 可变借用 registry。
        &mut self,
        // 接收 broker 已生成的 canonical handle。
        operation_id: &str,
        // 接收封闭 capability ID。
        capability_id: &str,
        // 接收业务接受时刻。
        now_ms: u64,
    ) -> Result<LongOperationRecord, LongOperationRegistryError> {
        // 每次接受前撤销已到期索引。
        self.expire(now_ms)?;
        // 严格验证 operation handle。
        validate_operation_id(operation_id)?;
        // 当前版本只允许窗口录制 capability。
        if capability_id != WINDOW_RECORD_CAPABILITY {
            // 在业务接受前拒绝未知 capability。
            return Err(LongOperationRegistryError::InvalidCapability);
        }
        // 同一 handle 不得覆盖既有记录。
        if self.records.contains_key(operation_id) {
            // 返回独立重复错误。
            return Err(LongOperationRegistryError::DuplicateOperation);
        }
        // 总记录预算包括尚未到期终态。
        if self.records.len() >= MAX_TRACKED_OPERATIONS
            // 活动预算只统计非终态。
            || self.active_count() >= MAX_ACTIVE_OPERATIONS
        {
            // 不驱逐、不扩大且不建立 handle。
            return Err(LongOperationRegistryError::CapacityExhausted);
        }
        // 构造首次持久接受事实。
        let record = LongOperationRecord {
            // 保存 canonical handle。
            operation_id: operation_id.to_owned(),
            // 保存封闭 capability。
            capability_id: capability_id.to_owned(),
            // 初始状态固定为 accepted。
            state: LongOperationState::accepted(),
            // 初始尚未请求取消。
            cancel_requested: false,
            // 首次持久版本固定为一。
            revision: 1,
            // 冻结创建时间。
            created_at_ms: now_ms,
            // 首次更新时间等于创建时间。
            updated_at_ms: now_ms,
            // 非终态没有到期时间。
            expires_at_ms: None,
            // 非终态没有结果。
            result: None,
            // 非终态没有错误。
            error: None,
        };
        // 先原子持久化再公开 handle。
        persist_record(&self.journal, &record)?;
        // 插入无扫描内存索引。
        self.records
            // 复制键并转移记录。
            .insert(operation_id.to_owned(), record.clone());
        // 返回业务接受证据。
        Ok(record)
    }

    // 查询当前记录且不延长终态保留期。
    pub(crate) fn status(
        // 可变借用以允许到期撤销。
        &mut self,
        // 接收 handle-only Query。
        operation_id: &str,
        // 接收查询时刻。
        now_ms: u64,
    ) -> Result<LongOperationRecord, LongOperationRegistryError> {
        // 查询前撤销到期记录。
        self.expire(now_ms)?;
        // 严格验证 handle 外壳。
        validate_operation_id(operation_id)?;
        // 返回记录快照而不修改 revision 或 expires。
        self.records
            // 按 canonical handle 查询。
            .get(operation_id)
            // 克隆有界记录快照。
            .cloned()
            // 零命中统一映射为 not found。
            .ok_or(LongOperationRegistryError::OperationNotFound)
    }

    // 持久记录 worker dispatch 已开始。
    pub(crate) fn start_dispatch(
        // 可变借用 registry。
        &mut self,
        // 接收 canonical handle。
        operation_id: &str,
        // 接收 dispatch 事实时刻。
        now_ms: u64,
    ) -> Result<LongOperationRecord, LongOperationRegistryError> {
        // 克隆既有记录作为原子候选。
        let mut candidate = self.candidate(operation_id, now_ms)?;
        // 由领域状态机验证不可逆迁移。
        candidate
            // 记录 dispatch 事实。
            .state
            // 执行状态迁移。
            .start_dispatch()
            // 不泄漏内部状态细节。
            .map_err(|_| LongOperationRegistryError::InvalidTransition)?;
        // 持久化并替换内存事实。
        self.commit_candidate(candidate, now_ms)
    }

    // 幂等持久记录取消请求。
    pub(crate) fn request_cancel(
        // 可变借用 registry。
        &mut self,
        // 接收 handle-only Command。
        operation_id: &str,
        // 接收取消接受时刻。
        now_ms: u64,
    ) -> Result<(CancelRequestEffect, LongOperationRecord), LongOperationRegistryError> {
        // 克隆既有记录作为原子候选。
        let mut candidate = self.candidate(operation_id, now_ms)?;
        // 由状态机分类首次、重复与终态取消。
        let effect = candidate.state.request_cancel();
        // 重复或终态取消不得延长生命周期或增加修订。
        if effect != CancelRequestEffect::Requested {
            // 返回未修改快照。
            return Ok((effect, candidate));
        }
        // 保留取消曾经被接受的独立事实。
        candidate.cancel_requested = true;
        // 原子提交首次取消。
        let record = self.commit_candidate(candidate, now_ms)?;
        // 返回传播提示与新记录。
        Ok((effect, record))
    }

    // 以 worker 成功证据终结任务或以超限失败终结。
    pub(crate) fn complete(
        // 可变借用 registry。
        &mut self,
        // 接收 canonical handle。
        operation_id: &str,
        // 接收完整 JSON 结果。
        result: Value,
        // 接收终态时刻。
        now_ms: u64,
    ) -> Result<(CompletionEffect, LongOperationRecord), LongOperationRegistryError> {
        // 克隆既有记录作为原子候选。
        let mut candidate = self.candidate(operation_id, now_ms)?;
        // 计算结果本身的完整 UTF-8 JSON 字节数。
        let result_bytes = serde_json::to_vec(&result)
            // Value 序列化失败视为非法记录。
            .map_err(|_| LongOperationRegistryError::InvalidRecord)?;
        // 超限结果不得截断或伪装成功。
        let effect = if result_bytes.len() > MAX_RESULT_BYTES {
            // 以可靠失败终结当前非终态。
            candidate
                // 使用状态机终结。
                .state
                // 记录失败。
                .fail()
                // 拒绝覆盖既有终态。
                .map_err(|_| LongOperationRegistryError::InvalidTransition)?;
            // 写入固定安全错误。
            candidate.error = Some(fixed_failure(
                // 使用契约错误码。
                "OPERATION_RESULT_TOO_LARGE",
                // 不回显结果内容。
                "The long operation result exceeded the fixed JSON byte budget.",
            ));
            // 返回超限失败分类。
            CompletionEffect::FailedResultTooLarge
        } else {
            // 以 worker 成功证据终结当前非终态。
            candidate
                // 使用状态机终结。
                .state
                // 记录成功完成。
                .complete()
                // 拒绝 dispatch 前或终态完成。
                .map_err(|_| LongOperationRegistryError::InvalidTransition)?;
            // 只保存完整有界结果。
            candidate.result = Some(result);
            // 返回成功完成分类。
            CompletionEffect::Completed
        };
        // 原子提交不可覆盖终态。
        let record = self.commit_candidate(candidate, now_ms)?;
        // 返回完成效果与记录。
        Ok((effect, record))
    }

    // 以领域可靠失败证据终结任务。
    pub(crate) fn fail(
        // 可变借用 registry。
        &mut self,
        // 接收 canonical handle。
        operation_id: &str,
        // 接收已构造的稳定错误。
        failure: LongOperationFailure,
        // 接收终态时刻。
        now_ms: u64,
    ) -> Result<LongOperationRecord, LongOperationRegistryError> {
        // 防御反序列化外的内部错误构造漂移。
        if !failure.is_valid() {
            // 拒绝不稳定错误文本。
            return Err(LongOperationRegistryError::InvalidFailure);
        }
        // 克隆既有记录作为原子候选。
        let mut candidate = self.candidate(operation_id, now_ms)?;
        // 由状态机拒绝覆盖终态。
        candidate
            // 使用领域状态机。
            .state
            // 记录可靠失败。
            .fail()
            // 映射非法迁移。
            .map_err(|_| LongOperationRegistryError::InvalidTransition)?;
        // 保存有界失败事实。
        candidate.error = Some(failure);
        // 原子提交终态。
        self.commit_candidate(candidate, now_ms)
    }

    // 在 dispatch 后保守终结为 outcome-unknown。
    pub(crate) fn mark_outcome_unknown(
        // 可变借用 registry。
        &mut self,
        // 接收 canonical handle。
        operation_id: &str,
        // 接收终态时刻。
        now_ms: u64,
    ) -> Result<LongOperationRecord, LongOperationRegistryError> {
        // 克隆既有记录作为原子候选。
        let mut candidate = self.candidate(operation_id, now_ms)?;
        // 只允许 dispatch 后非终态进入未知。
        candidate
            // 使用领域状态机。
            .state
            // 记录保守未知。
            .mark_outcome_unknown()
            // 映射非法迁移。
            .map_err(|_| LongOperationRegistryError::InvalidTransition)?;
        // 写入固定未知结果错误。
        candidate.error = Some(fixed_failure(
            // schema 要求固定错误码。
            "OUTCOME_UNKNOWN",
            // 不声称动作未执行。
            "The final outcome could not be proven after dispatch started.",
        ));
        // 原子提交终态。
        self.commit_candidate(candidate, now_ms)
    }

    // 返回当前活动任务数。
    pub(crate) fn active_count(&self) -> usize {
        // 只统计非终态记录。
        self.records
            // 遍历记录值。
            .values()
            // 保留活动记录。
            .filter(|record| !record.terminal())
            // 返回有限计数。
            .count()
    }

    // 返回当前全部可查询记录数。
    pub(crate) fn tracked_count(&self) -> usize {
        // 返回内存索引大小。
        self.records.len()
    }

    // 克隆一个存在且未过期的原子迁移候选。
    fn candidate(
        // 可变借用 registry。
        &mut self,
        // 接收 canonical handle。
        operation_id: &str,
        // 接收迁移时刻。
        now_ms: u64,
    ) -> Result<LongOperationRecord, LongOperationRegistryError> {
        // 迁移前撤销到期记录。
        self.expire(now_ms)?;
        // 严格验证 handle。
        validate_operation_id(operation_id)?;
        // 克隆既有记录以保证持久失败时内存不变。
        let record = self
            // 查询内存索引。
            .records
            // 按 canonical handle 定位。
            .get(operation_id)
            // 克隆有界记录。
            .cloned()
            // 零命中统一返回 not found。
            .ok_or(LongOperationRegistryError::OperationNotFound)?;
        // 拒绝时钟倒退覆盖修订。
        ensure_monotonic_time(&record, now_ms)?;
        // 返回尚未提交的候选。
        Ok(record)
    }

    // 完成候选修订并以 journal-first 顺序替换事实。
    fn commit_candidate(
        // 可变借用 registry。
        &mut self,
        // 接收完成领域迁移的候选。
        mut candidate: LongOperationRecord,
        // 接收迁移时刻。
        now_ms: u64,
    ) -> Result<LongOperationRecord, LongOperationRegistryError> {
        // 更新修订、时间与终态到期。
        finalize_mutation(&mut candidate, now_ms)?;
        // 验证候选仍满足完整 journal schema。
        validate_record(&candidate)?;
        // 先原子持久化候选。
        persist_record(&self.journal, &candidate)?;
        // 持久成功后替换内存事实。
        self.records
            // 使用候选自有 canonical handle。
            .insert(candidate.operation_id.clone(), candidate.clone());
        // 返回新记录快照。
        Ok(candidate)
    }

    // 到期先撤销内存索引再清理精确 final 文件。
    fn expire(&mut self, now_ms: u64) -> Result<(), LongOperationRegistryError> {
        // 收集已经到期的终态 handle。
        let expired = self
            // 遍历索引。
            .records
            // 保留键和值。
            .iter()
            // 只选择到期终态。
            .filter_map(|(operation_id, record)| {
                // 到期边界使用大于等于。
                record
                    // 读取终态到期时间。
                    .expires_at_ms
                    // 只保留已经到期的记录。
                    .filter(|expires_at| *expires_at <= now_ms)
                    // 克隆待撤销 handle。
                    .map(|_| operation_id.clone())
            })
            // 固定到独立列表避免借用冲突。
            .collect::<Vec<_>>();
        // 逐条先撤销查询索引。
        for operation_id in expired {
            // 从内存索引删除。
            self.records.remove(&operation_id);
            // 再清理 journal final 文件。
            self.journal.remove(&operation_id)?;
        }
        // 返回全部到期清理完成。
        Ok(())
    }
}

// 声明 registry 容量、恢复与终态回归测试。
#[cfg(test)]
// 将 fixture 放入独立文件控制生产 Module 规模。
#[path = "long_operation_registry_tests.rs"]
mod tests;
