//! 验证永久 tombstone 的 revision、重送与语义冲突规则。

// 导入被测私有实现。
use super::*;
// 导入 JSON 构造宏用于损坏 fixture。
use serde_json::json;

// 固定第一个 execution identity。
const FIRST_EXECUTION: &str = "s2:q:00000000000000000000000000000001";
// 固定第二个 execution identity。
const SECOND_EXECUTION: &str = "s2:q:00000000000000000000000000000002";
// 固定第一个 start nonce。
const FIRST_NONCE: &str = "11111111111111111111111111111111";
// 固定第二个 start nonce。
const SECOND_NONCE: &str = "22222222222222222222222222222222";
// 固定第一条 start 语义摘要。
const FIRST_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
// 固定另一条 start 语义摘要。
const SECOND_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

// 验证首次追加、同义重送和 roundtrip 保持唯一事实。
#[test]
fn remember_is_monotonic_and_idempotent() -> Result<(), SequenceExecutionForgottenError> {
    // 建立 revision 零索引。
    let mut index = SequenceExecutionForgottenIndex::new();
    // 首次追加建立 revision 一。
    assert!(index.remember(0, FIRST_EXECUTION, FIRST_NONCE, FIRST_DIGEST)?);
    // revision 严格加一。
    assert_eq!(index.revision(), 1);
    // 完全相同重送不推进 revision。
    assert!(!index.remember(1, FIRST_EXECUTION, FIRST_NONCE, FIRST_DIGEST)?);
    // revision 保持稳定。
    assert_eq!(index.revision(), 1);
    // 紧凑编码完整索引。
    let bytes = index.encode()?;
    // 严格解码同一事实。
    let decoded = SequenceExecutionForgottenIndex::decode(&bytes)?;
    // roundtrip 不得改变任何字段。
    assert_eq!(decoded, index);
    // 同义 start 永久绑定原 execution。
    assert_eq!(
        index.lookup_start(FIRST_NONCE, FIRST_DIGEST)?,
        ForgottenStartLookup::Forgotten {
            execution_id: FIRST_EXECUTION
        }
    );
    // 测试正常完成。
    Ok(())
}

// 验证同 nonce 异义优先冲突且不污染原 tombstone。
#[test]
fn nonce_semantic_conflict_never_rebinds() -> Result<(), SequenceExecutionForgottenError> {
    // 建立一条原始事实。
    let mut index = SequenceExecutionForgottenIndex::new();
    // 追加第一个 tombstone。
    index.remember(0, FIRST_EXECUTION, FIRST_NONCE, FIRST_DIGEST)?;
    // 保存冲突前索引。
    let before = index.clone();
    // 同 nonce 异义必须拒绝。
    assert_eq!(
        index.remember(1, SECOND_EXECUTION, FIRST_NONCE, SECOND_DIGEST),
        Err(SequenceExecutionForgottenError::SemanticConflict)
    );
    // 冲突不得污染原事实。
    assert_eq!(index, before);
    // 查询同样返回语义冲突而不泄漏摘要。
    assert_eq!(
        index.lookup_start(FIRST_NONCE, SECOND_DIGEST)?,
        ForgottenStartLookup::SemanticConflict
    );
    // 新 nonce 仍可追加第二条事实。
    assert!(index.remember(1, SECOND_EXECUTION, SECOND_NONCE, SECOND_DIGEST)?);
    // revision 严格等于记录数量。
    assert_eq!(index.revision(), 2);
    // 测试正常完成。
    Ok(())
}

// 验证 stale revision、identity 重用与损坏文档失败闭合。
#[test]
fn stale_or_corrupt_updates_are_rejected() -> Result<(), SequenceExecutionForgottenError> {
    // 建立一条可信事实。
    let mut index = SequenceExecutionForgottenIndex::new();
    // 追加第一个 tombstone。
    index.remember(0, FIRST_EXECUTION, FIRST_NONCE, FIRST_DIGEST)?;
    // stale expected revision 必须拒绝。
    assert_eq!(
        index.remember(0, SECOND_EXECUTION, SECOND_NONCE, SECOND_DIGEST),
        Err(SequenceExecutionForgottenError::RevisionConflict)
    );
    // 相同 execution 不得换 nonce 重用。
    assert_eq!(
        index.remember(1, FIRST_EXECUTION, SECOND_NONCE, SECOND_DIGEST),
        Err(SequenceExecutionForgottenError::InvalidRecord)
    );

    // 构造 revision 跳跃文档。
    let jumped = json!({
        // 使用固定版本。
        "contractVersion": FORGOTTEN_CONTRACT_VERSION,
        // revision 与一条记录不一致。
        "recordRevision": 9,
        // 保存一条 canonical 记录。
        "records": [{
            // 保存 execution identity。
            "executionId": FIRST_EXECUTION,
            // 保存 start nonce。
            "startRequestNonce": FIRST_NONCE,
            // 保存 start digest。
            "startSemanticDigest": FIRST_DIGEST
        }]
    });
    // 严格 codec 必须拒绝 revision 跳跃。
    assert_eq!(
        SequenceExecutionForgottenIndex::decode(&serde_json::to_vec(&jumped).unwrap_or_default()),
        Err(SequenceExecutionForgottenError::InvalidRecord)
    );
    // 测试正常完成。
    Ok(())
}
