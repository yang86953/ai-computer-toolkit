//! 保存 sequence execution forget 后不可重建的紧凑去重事实。

// 导入严格 JSON 编解码派生。
use serde::{Deserialize, Serialize};

// 导入 sequence identity 唯一格式权威。
use super::sequence_execution_identity::{execution_fingerprint, is_digest, is_nonce};

// 固定 forgotten index 私有版本。
const FORGOTTEN_CONTRACT_VERSION: &str = "act/sequence-execution-forgotten/v1";
// 固定最多保留的永久去重 tombstone 数量。
pub(crate) const MAX_SEQUENCE_FORGOTTEN_RECORDS: usize = 4_096;
// 固定完整 forgotten index 最大字节数。
pub(crate) const MAX_SEQUENCE_FORGOTTEN_INDEX_BYTES: usize = 1_048_576;

// 表示 forgotten index 编解码与单调更新失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SequenceExecutionForgottenError {
    // JSON 无法严格编解码。
    InvalidJson,
    // 版本、revision、顺序或容量事实损坏。
    InvalidRecord,
    // execution 或 start nonce 不是 canonical。
    InvalidIdentity,
    // start 语义摘要不是 canonical SHA-256 文本。
    InvalidDigest,
    // expected revision 与当前事实不一致。
    RevisionConflict,
    // 相同 start nonce 携带不同完整语义。
    SemanticConflict,
    // tombstone 固定容量已耗尽。
    CapacityExceeded,
    // 单调 revision 达到整数边界。
    RevisionExhausted,
    // 紧凑索引超过固定字节预算。
    RecordTooLarge,
}

// 保存一条不可删除的 execution 去重事实。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ForgottenExecutionRecord {
    // 保存已经 forget 的 canonical execution identity。
    execution_id: String,
    // 保存原 start request nonce。
    start_request_nonce: String,
    // 保存原完整 start 语义 digest。
    start_semantic_digest: String,
}

// 保存全部永久 tombstone 与单调 revision。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SequenceExecutionForgottenIndex {
    // 保存固定私有版本。
    contract_version: String,
    // 保存每次首次追加严格加一的 revision。
    record_revision: u64,
    // 保存按首次 forget 顺序排列的紧凑事实。
    records: Vec<ForgottenExecutionRecord>,
}

// 表示 start 请求相对永久 tombstone 的只读结论。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ForgottenStartLookup<'a> {
    // start nonce 从未被 forget。
    NotFound,
    // 同 nonce、同语义已经永久 stale。
    Forgotten {
        // 借用原 execution identity 供安全错误关联。
        execution_id: &'a str,
    },
    // 同 nonce 携带不同语义，必须优先返回冲突。
    SemanticConflict,
}

// 为 forgotten index 提供严格 codec、查询与单调追加。
impl SequenceExecutionForgottenIndex {
    // 建立 revision 零的空索引。
    pub(crate) fn new() -> Self {
        // 返回固定版本空记录。
        Self {
            // 保存固定契约版本。
            contract_version: FORGOTTEN_CONTRACT_VERSION.to_owned(),
            // 空索引从 revision 零开始。
            record_revision: 0,
            // 尚无永久 tombstone。
            records: Vec::new(),
        }
    }

    // 严格解码一个完整有界 forgotten index。
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, SequenceExecutionForgottenError> {
        // 空文档与超限文档都不得解析。
        if bytes.is_empty() {
            // 空字节不是 JSON 索引。
            return Err(SequenceExecutionForgottenError::InvalidJson);
        }
        // 读取前拒绝超出固定预算。
        if bytes.len() > MAX_SEQUENCE_FORGOTTEN_INDEX_BYTES {
            // 返回独立资源错误。
            return Err(SequenceExecutionForgottenError::RecordTooLarge);
        }
        // 严格反序列化并拒绝未知字段。
        let index = serde_json::from_slice::<Self>(bytes)
            // 不泄漏 parser 细节。
            .map_err(|_| SequenceExecutionForgottenError::InvalidJson)?;
        // 验证版本、revision 与全部唯一性。
        index.validate()?;
        // 返回可信索引。
        Ok(index)
    }

    // 编码完整索引并再次验证字节边界。
    pub(crate) fn encode(&self) -> Result<Vec<u8>, SequenceExecutionForgottenError> {
        // 禁止写入无效内存候选。
        self.validate()?;
        // 使用紧凑 JSON 编码完整文档。
        let bytes = serde_json::to_vec(self)
            // 理论编码失败保持封闭。
            .map_err(|_| SequenceExecutionForgottenError::InvalidJson)?;
        // 编码结果必须满足固定预算。
        if bytes.len() > MAX_SEQUENCE_FORGOTTEN_INDEX_BYTES {
            // 不返回可部分提交的字节。
            return Err(SequenceExecutionForgottenError::RecordTooLarge);
        }
        // 返回完整拥有型文档。
        Ok(bytes)
    }

    // 在 expected revision 上首次追加永久 tombstone。
    pub(crate) fn remember(
        // 可变借用索引。
        &mut self,
        // 接收调用方观察的 revision。
        expected_revision: u64,
        // 接收 canonical execution identity。
        execution_id: &str,
        // 接收原 start request nonce。
        start_request_nonce: &str,
        // 接收原完整 start 语义 digest。
        start_semantic_digest: &str,
    ) -> Result<bool, SequenceExecutionForgottenError> {
        // expected revision 必须精确相等。
        if self.record_revision != expected_revision {
            // 拒绝 stale forget 更新。
            return Err(SequenceExecutionForgottenError::RevisionConflict);
        }
        // 输入身份与摘要必须 canonical。
        validate_identity(execution_id, start_request_nonce, start_semantic_digest)?;
        // 同 nonce 的既有事实优先决定 attach 或冲突。
        if let Some(existing) = self
            // 借用全部 tombstones。
            .records
            // 创建迭代器。
            .iter()
            // 查找同一 start nonce。
            .find(|record| record.start_request_nonce == start_request_nonce)
        {
            // 同 nonce 异义必须返回稳定冲突。
            if existing.start_semantic_digest != start_semantic_digest {
                // 不允许用新的 execution 覆盖原语义。
                return Err(SequenceExecutionForgottenError::SemanticConflict);
            }
            // 同 nonce 同语义必须绑定原 execution。
            if existing.execution_id != execution_id {
                // 拒绝不可能的一对多映射。
                return Err(SequenceExecutionForgottenError::InvalidRecord);
            }
            // 完全相同重送不推进 revision。
            return Ok(false);
        }
        // execution identity 不得由另一 nonce 重用。
        if self
            // 借用全部 tombstones。
            .records
            // 创建迭代器。
            .iter()
            // 查找相同 execution。
            .any(|record| record.execution_id == execution_id)
        {
            // 拒绝 identity 漂移。
            return Err(SequenceExecutionForgottenError::InvalidRecord);
        }
        // 固定容量耗尽时失败闭合。
        if self.records.len() >= MAX_SEQUENCE_FORGOTTEN_RECORDS {
            // 禁止无界增长。
            return Err(SequenceExecutionForgottenError::CapacityExceeded);
        }
        // 在克隆候选上建立追加事实。
        let mut candidate = self.clone();
        // 计算下一个单调 revision。
        candidate.record_revision = candidate
            // 读取当前 revision。
            .record_revision
            // 严格加一。
            .checked_add(1)
            // 达到整数边界失败闭合。
            .ok_or(SequenceExecutionForgottenError::RevisionExhausted)?;
        // 按首次 forget 顺序追加紧凑事实。
        candidate.records.push(ForgottenExecutionRecord {
            // 保存 execution identity。
            execution_id: execution_id.to_owned(),
            // 保存 start nonce。
            start_request_nonce: start_request_nonce.to_owned(),
            // 保存完整语义摘要。
            start_semantic_digest: start_semantic_digest.to_owned(),
        });
        // 候选必须满足全部不变量与字节预算。
        candidate.encode()?;
        // 全部成功后替换原索引。
        *self = candidate;
        // 报告建立了新 revision。
        Ok(true)
    }

    // 查询 start nonce 与语义相对永久 tombstone 的结论。
    pub(crate) fn lookup_start(
        // 借用索引。
        &self,
        // 接收 canonical start nonce。
        start_request_nonce: &str,
        // 接收 canonical 完整语义 digest。
        start_semantic_digest: &str,
    ) -> Result<ForgottenStartLookup<'_>, SequenceExecutionForgottenError> {
        // 查询输入必须 canonical。
        if !is_nonce(start_request_nonce) {
            // 拒绝 nonce 别名。
            return Err(SequenceExecutionForgottenError::InvalidIdentity);
        }
        // digest 必须 canonical。
        if !is_digest(start_semantic_digest) {
            // 拒绝摘要别名。
            return Err(SequenceExecutionForgottenError::InvalidDigest);
        }
        // 查找同 nonce 事实。
        let Some(record) = self
            // 借用全部记录。
            .records
            // 创建迭代器。
            .iter()
            // 精确匹配 start nonce。
            .find(|record| record.start_request_nonce == start_request_nonce)
        else {
            // 从未 forget 的 nonce。
            return Ok(ForgottenStartLookup::NotFound);
        };
        // 同 nonce 异义优先返回冲突。
        if record.start_semantic_digest != start_semantic_digest {
            // 不公开原摘要。
            return Ok(ForgottenStartLookup::SemanticConflict);
        }
        // 同义 start 永久关联原 execution。
        Ok(ForgottenStartLookup::Forgotten {
            // 只借用 canonical identity。
            execution_id: &record.execution_id,
        })
    }

    // 返回当前单调 revision。
    pub(crate) const fn revision(&self) -> u64 {
        // 复制 revision。
        self.record_revision
    }

    // 验证版本、容量、revision 与唯一映射。
    fn validate(&self) -> Result<(), SequenceExecutionForgottenError> {
        // 契约版本必须精确匹配。
        if self.contract_version != FORGOTTEN_CONTRACT_VERSION {
            // 未知版本不得猜测迁移。
            return Err(SequenceExecutionForgottenError::InvalidRecord);
        }
        // 记录数量必须满足固定容量。
        if self.records.len() > MAX_SEQUENCE_FORGOTTEN_RECORDS {
            // 拒绝超限恢复。
            return Err(SequenceExecutionForgottenError::CapacityExceeded);
        }
        // 只有追加操作，因此 revision 必须等于记录数。
        if self.record_revision != self.records.len() as u64 {
            // 拒绝回退、跳跃或隐藏删除。
            return Err(SequenceExecutionForgottenError::InvalidRecord);
        }
        // 使用有界集合拒绝 identity 或 nonce 重复。
        let mut executions = std::collections::HashSet::with_capacity(self.records.len());
        // 收集 start nonce 唯一性。
        let mut nonces = std::collections::HashSet::with_capacity(self.records.len());
        // 逐项验证全部紧凑事实。
        for record in &self.records {
            // 验证 canonical identity、nonce 与 digest。
            validate_identity(
                // 借用 execution identity。
                &record.execution_id,
                // 借用 start nonce。
                &record.start_request_nonce,
                // 借用完整语义摘要。
                &record.start_semantic_digest,
            )?;
            // execution 与 nonce 都必须唯一。
            if !executions.insert(record.execution_id.as_str())
                // 同 nonce 不能重复出现。
                || !nonces.insert(record.start_request_nonce.as_str())
            {
                // 拒绝歧义映射。
                return Err(SequenceExecutionForgottenError::InvalidRecord);
            }
        }
        // 全部不变量成立。
        Ok(())
    }
}

// 默认建立空 forgotten index。
impl Default for SequenceExecutionForgottenIndex {
    // 返回固定版本空索引。
    fn default() -> Self {
        // 委托唯一构造函数。
        Self::new()
    }
}

// 验证单条 tombstone 的 canonical 字段。
fn validate_identity(
    // 借用 execution identity。
    execution_id: &str,
    // 借用 start nonce。
    start_request_nonce: &str,
    // 借用完整 start 语义摘要。
    start_semantic_digest: &str,
) -> Result<(), SequenceExecutionForgottenError> {
    // execution 和 nonce 必须 canonical。
    if execution_fingerprint(execution_id).is_none() || !is_nonce(start_request_nonce) {
        // 返回统一身份错误。
        return Err(SequenceExecutionForgottenError::InvalidIdentity);
    }
    // digest 必须 canonical。
    if !is_digest(start_semantic_digest) {
        // 返回独立摘要错误。
        return Err(SequenceExecutionForgottenError::InvalidDigest);
    }
    // 字段合法。
    Ok(())
}

// 声明 forgotten index 单调语义测试。
#[cfg(test)]
#[path = "sequence_execution_forgotten_tests.rs"]
mod tests;
