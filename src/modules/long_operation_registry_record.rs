//! 实现长操作 registry 私有 journal schema 与记录不变量。

// 导入严格 JSON 派生与值类型。
use serde::{Deserialize, Serialize};
// 导入 provider-neutral JSON 值。
use serde_json::Value;

// 导入 journal 与 opaque ID Component。
use crate::components::{
    // 导入原子 journal。
    long_operation_journal::LongOperationJournal,
    // 导入 canonical operation 原语。
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
};

// 导入状态与预算。
use super::{
    // 导入父 Module 私有常量。
    JOURNAL_CONTRACT_VERSION,
    // 导入父 Module 私有类型。
    LongOperationFailure,
    LongOperationRecord,
    LongOperationRegistryError,
    MAX_ERROR_CODE_BYTES,
    MAX_ERROR_MESSAGE_CHARACTERS,
    TERMINAL_RETENTION_MILLISECONDS,
    WINDOW_RECORD_CAPABILITY,
};
// 导入相邻长操作状态 Module。
use super::super::long_operation::{
    // 导入领域状态类型。
    LongOperationState,
    LongOperationStatus,
    // 导入结果预算。
    MAX_RESULT_BYTES,
    RecoveryEffect,
};

// 定义严格持久化的单记录 schema。
#[derive(Debug, Deserialize, Serialize)]
// 固定 camelCase 并拒绝未知字段与降级解释。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedLongOperationRecord {
    // 保存私有 journal 版本。
    contract_version: String,
    // 保存 canonical operation handle。
    operation_id: String,
    // 保存封闭 capability ID。
    capability_id: String,
    // 保存公开生命周期状态。
    status: LongOperationStatus,
    // 保存不可逆 dispatch 事实。
    dispatch_started: bool,
    // 保存独立取消事实。
    cancel_requested: bool,
    // 保存单调持久修订号。
    revision: u64,
    // 保存首次接受时间。
    created_at_ms: u64,
    // 保存最后持久迁移时间。
    updated_at_ms: u64,
    // 保存终态固定到期时间。
    expires_at_ms: Option<u64>,
    // 保存 completed 的有界结果。
    result: Option<Value>,
    // 保存失败终态错误。
    error: Option<LongOperationFailure>,
}

// 严格解码 journal 文档并验证文件名绑定。
pub(super) fn decode_record(
    // 接收由文件名恢复的 handle。
    file_operation_id: &str,
    // 接收完整有界 JSON 字节。
    bytes: &[u8],
) -> Result<LongOperationRecord, LongOperationRegistryError> {
    // 严格反序列化单个 JSON 对象。
    let persisted = serde_json::from_slice::<PersistedLongOperationRecord>(bytes)
        // 损坏或字段漂移必须失败闭合。
        .map_err(|_| LongOperationRegistryError::InvalidRecord)?;
    // 文件名与正文 handle 必须完全一致。
    if persisted.operation_id != file_operation_id {
        // 拒绝替换或错配记录。
        return Err(LongOperationRegistryError::InvalidRecord);
    }
    // 私有版本必须精确匹配。
    if persisted.contract_version != JOURNAL_CONTRACT_VERSION {
        // 未知版本不得猜测迁移。
        return Err(LongOperationRegistryError::InvalidRecord);
    }
    // 恢复并验证状态与 dispatch 组合。
    let state = LongOperationState::restore(persisted.status, persisted.dispatch_started)
        // 映射非法组合。
        .map_err(|_| LongOperationRegistryError::InvalidRecord)?;
    // 组装领域记录。
    let record = LongOperationRecord {
        // 转移 canonical handle 候选。
        operation_id: persisted.operation_id,
        // 转移 capability。
        capability_id: persisted.capability_id,
        // 保存已验证状态机。
        state,
        // 保存取消事实。
        cancel_requested: persisted.cancel_requested,
        // 保存修订号。
        revision: persisted.revision,
        // 保存创建时间。
        created_at_ms: persisted.created_at_ms,
        // 保存更新时间。
        updated_at_ms: persisted.updated_at_ms,
        // 保存到期时间。
        expires_at_ms: persisted.expires_at_ms,
        // 转移结果。
        result: persisted.result,
        // 转移错误。
        error: persisted.error,
    };
    // 验证完整不变量。
    validate_record(&record)?;
    // 返回领域记录。
    Ok(record)
}

// 验证一条完整记录的跨字段不变量。
pub(super) fn validate_record(
    // 借用领域记录。
    record: &LongOperationRecord,
) -> Result<(), LongOperationRegistryError> {
    // handle 必须是 canonical operation。
    validate_operation_id(&record.operation_id)
        // journal 失配统一视为记录损坏。
        .map_err(|_| LongOperationRegistryError::InvalidRecord)?;
    // capability 与修订、时间顺序必须合法。
    if record.capability_id != WINDOW_RECORD_CAPABILITY
        // 修订从一开始。
        || record.revision == 0
        // 更新时间不得早于创建时间。
        || record.updated_at_ms < record.created_at_ms
    {
        // 拒绝基础字段漂移。
        return Err(LongOperationRegistryError::InvalidRecord);
    }
    // cancel-requested 状态必须保留独立取消事实。
    if record.status() == LongOperationStatus::CancelRequested && !record.cancel_requested {
        // 拒绝丢失取消意图的记录。
        return Err(LongOperationRegistryError::InvalidRecord);
    }
    // 所有错误必须满足稳定文本边界。
    if record.error.as_ref().is_some_and(|error| !error.is_valid()) {
        // 拒绝不安全错误文本。
        return Err(LongOperationRegistryError::InvalidRecord);
    }
    // 终态到期必须固定从最后迁移计算。
    let expected_expiry = if record.terminal() {
        // 使用 checked_add 防止时间溢出。
        Some(
            record
                // 从最后可靠终态时间开始。
                .updated_at_ms
                // 加固定保留期。
                .checked_add(TERMINAL_RETENTION_MILLISECONDS)
                // 溢出视为记录损坏。
                .ok_or(LongOperationRegistryError::InvalidRecord)?,
        )
    } else {
        // 非终态不得预设到期。
        None
    };
    // 到期字段必须精确匹配。
    if record.expires_at_ms != expected_expiry {
        // 拒绝轮询延长或任意生命周期。
        return Err(LongOperationRegistryError::InvalidRecord);
    }
    // 按封闭状态验证 result/error 互斥形状。
    match record.status() {
        // accepted 不得携带终态 payload。
        LongOperationStatus::Accepted
        // running 不得携带终态 payload。
        | LongOperationStatus::Running
        // cancel-requested 仍不是终态。
        | LongOperationStatus::CancelRequested => {
            // 非终态不得携带 result 或 error。
            if record.result.is_some() || record.error.is_some() {
                // 拒绝伪造终态 payload。
                return Err(LongOperationRegistryError::InvalidRecord);
            }
        }
        // completed 必须只有有界 result。
        LongOperationStatus::Completed => {
            // 要求结果存在且错误缺失。
            let Some(result) = record.result.as_ref().filter(|_| record.error.is_none()) else {
                // 拒绝不完整完成记录。
                return Err(LongOperationRegistryError::InvalidRecord);
            };
            // 精确计算结果 JSON 预算。
            let bytes = serde_json::to_vec(result)
                // Value 序列化失败视为损坏。
                .map_err(|_| LongOperationRegistryError::InvalidRecord)?;
            // 结果不得超过固定一 MiB。
            if bytes.len() > MAX_RESULT_BYTES {
                // 拒绝从 journal 恢复超限成功。
                return Err(LongOperationRegistryError::InvalidRecord);
            }
        }
        // failed 必须只有稳定 error。
        LongOperationStatus::Failed => {
            // 失败不得携带 result 且必须携带 error。
            if record.result.is_some() || record.error.is_none() {
                // 拒绝不完整失败记录。
                return Err(LongOperationRegistryError::InvalidRecord);
            }
        }
        // outcome-unknown 必须使用固定错误码。
        LongOperationStatus::OutcomeUnknown => {
            // 未知不得携带结果。
            if record.result.is_some()
                // 必须携带固定错误码。
                || record.error.as_ref().map(LongOperationFailure::code) != Some("OUTCOME_UNKNOWN")
            {
                // 拒绝伪造未知终态。
                return Err(LongOperationRegistryError::InvalidRecord);
            }
        }
    }
    // 返回全部不变量成立。
    Ok(())
}

// 完成一次候选修订、更新时间与到期计算。
pub(super) fn finalize_mutation(
    // 可变借用候选记录。
    record: &mut LongOperationRecord,
    // 接收迁移时刻。
    now_ms: u64,
) -> Result<(), LongOperationRegistryError> {
    // 再次防御时钟回退。
    ensure_monotonic_time(record, now_ms)?;
    // 修订号必须单调且不得溢出。
    record.revision = record
        // 读取既有修订。
        .revision
        // 增加一次持久迁移。
        .checked_add(1)
        // 溢出时拒绝覆盖事实。
        .ok_or(LongOperationRegistryError::InvalidRecord)?;
    // 冻结最后迁移时间。
    record.updated_at_ms = now_ms;
    // 终态从当前迁移时刻固定计算到期。
    record.expires_at_ms = if record.terminal() {
        // checked_add 防止溢出产生短生命周期。
        Some(
            now_ms
                // 加固定二十四小时。
                .checked_add(TERMINAL_RETENTION_MILLISECONDS)
                // 溢出时拒绝提交。
                .ok_or(LongOperationRegistryError::InvalidRecord)?,
        )
    } else {
        // 非终态保持无到期时间。
        None
    };
    // 返回修订完成。
    Ok(())
}

// 原子序列化并持久一条完整记录。
pub(super) fn persist_record(
    // 借用 journal Component。
    journal: &LongOperationJournal,
    // 借用经过验证的领域记录。
    record: &LongOperationRecord,
) -> Result<(), LongOperationRegistryError> {
    // 转换为封闭持久 DTO。
    let persisted = PersistedLongOperationRecord {
        // 固定私有版本。
        contract_version: JOURNAL_CONTRACT_VERSION.to_owned(),
        // 复制 canonical handle。
        operation_id: record.operation_id.clone(),
        // 复制 capability。
        capability_id: record.capability_id.clone(),
        // 复制公开状态。
        status: record.status(),
        // 复制 dispatch 事实。
        dispatch_started: record.dispatch_started(),
        // 复制取消事实。
        cancel_requested: record.cancel_requested,
        // 复制修订号。
        revision: record.revision,
        // 复制创建时间。
        created_at_ms: record.created_at_ms,
        // 复制更新时间。
        updated_at_ms: record.updated_at_ms,
        // 复制到期时间。
        expires_at_ms: record.expires_at_ms,
        // 克隆有界结果。
        result: record.result.clone(),
        // 克隆稳定错误。
        error: record.error.clone(),
    };
    // 生成单个完整 JSON 文档。
    let bytes = serde_json::to_vec(&persisted)
        // DTO 序列化失败视为内部记录错误。
        .map_err(|_| LongOperationRegistryError::InvalidRecord)?;
    // 以 journal-first 顺序原子提交。
    journal.persist(&record.operation_id, &bytes)?;
    // 返回持久事实已建立。
    Ok(())
}

// 验证 canonical operation handle。
pub(super) fn validate_operation_id(
    // 接收 handle 文本。
    operation_id: &str,
) -> Result<(), LongOperationRegistryError> {
    // 使用共享 opaque parser 严格解析。
    let parsed = OpaqueTargetId::parse(operation_id)
        // 拒绝宽松外壳。
        .ok_or(LongOperationRegistryError::InvalidOperation)?;
    // 只接受 operation 类别。
    if parsed.kind() != OpaqueTargetKind::Operation {
        // 拒绝窗口、进程或其他目标。
        return Err(LongOperationRegistryError::InvalidOperation);
    }
    // 返回 handle 合法。
    Ok(())
}

// 验证错误码是固定大写 ASCII 形状。
pub(super) fn is_valid_error_code(code: &str) -> bool {
    // 拒绝空值和超限字节。
    if code.is_empty() || code.len() > MAX_ERROR_CODE_BYTES {
        // 返回非法。
        return false;
    }
    // 首字符必须是 ASCII 大写字母。
    if !code.as_bytes()[0].is_ascii_uppercase() {
        // 返回非法。
        return false;
    }
    // 其余字符只允许大写字母、数字和下划线。
    code.bytes()
        // 遍历全部字节。
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

// 验证错误消息非空且不超过 schema 字符预算。
pub(super) fn is_valid_error_message(message: &str) -> bool {
    // 统计 Unicode 标量并拒绝空文本。
    !message.is_empty()
        // 限制公开字符数。
        && message.chars().count() <= MAX_ERROR_MESSAGE_CHARACTERS
}

// 构造编译期固定且已知合法的失败事实。
pub(super) fn fixed_failure(code: &str, message: &str) -> LongOperationFailure {
    // 使用结构字面量避免生产路径 panic。
    LongOperationFailure {
        // 固定调用点只传入审查过的错误码。
        code: code.to_owned(),
        // 固定调用点只传入审查过的消息。
        message: message.to_owned(),
    }
}

// 将恢复效果映射为固定失败事实。
pub(super) fn recovery_failure(
    // 接收状态机恢复分类。
    effect: RecoveryEffect,
) -> Result<LongOperationFailure, LongOperationRegistryError> {
    // 按 dispatch 事实选择稳定错误。
    match effect {
        // dispatch 前可证明没有业务动作。
        RecoveryEffect::FailedBeforeDispatch => Ok(fixed_failure(
            // 使用可安全重试的中断码。
            "BROKER_INTERRUPTED_BEFORE_DISPATCH",
            // 明确动作未开始。
            "The broker restarted before dispatch began; retrying the original operation is safe.",
        )),
        // dispatch 后无法证明最终结果。
        RecoveryEffect::OutcomeBecameUnknown => Ok(fixed_failure(
            // 使用 schema 固定未知码。
            "OUTCOME_UNKNOWN",
            // 不声称动作未执行。
            "The broker restarted after dispatch began and the final outcome could not be proven.",
        )),
        // open 只对非终态调用恢复。
        RecoveryEffect::Unchanged => Err(LongOperationRegistryError::InvalidRecord),
    }
}

// 验证迁移时钟不早于既有可靠事实。
pub(super) fn ensure_monotonic_time(
    // 借用既有记录。
    record: &LongOperationRecord,
    // 接收新时刻。
    now_ms: u64,
) -> Result<(), LongOperationRegistryError> {
    // 时钟回退不得覆盖更新时间或缩短生命周期。
    if now_ms < record.updated_at_ms {
        // 返回独立时钟错误。
        return Err(LongOperationRegistryError::ClockRegression);
    }
    // 返回时间顺序合法。
    Ok(())
}
