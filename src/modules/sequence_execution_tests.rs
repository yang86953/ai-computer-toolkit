//! 验证 sequence execution record codec、revision 与崩溃恢复语义。

// 导入 JSON 构造宏与值类型。
use serde_json::{Value, json};

// 导入受测领域类型。
use super::{
    // 导入记录与错误。
    SequenceExecutionRecord,
    SequenceExecutionRecordError,
    // 导入 execution 和 step 状态。
    SequenceExecutionState,
    SequenceStepState,
};

// 固定第一 broker epoch。
const FIRST_EPOCH: &str = "11111111111111111111111111111111";
// 固定恢复 broker epoch。
const SECOND_EPOCH: &str = "22222222222222222222222222222222";
// 固定 start request nonce。
const START_NONCE: &str = "33333333333333333333333333333333";
// 固定 dispatch nonce。
const DISPATCH_NONCE: &str = "44444444444444444444444444444444";
// 固定 start 语义 SHA-256 形状。
const START_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
// 固定 input SHA-256 形状。
const INPUT_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
// 固定 step 请求 SHA-256 形状。
const STEP_DIGEST: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

// 构造指定步骤数的严格记录。
fn record(step_count: usize) -> Result<SequenceExecutionRecord, SequenceExecutionRecordError> {
    // 使用公开默认的遇错停止策略。
    record_with_continue_on_error(step_count, false)
}

// 构造指定 continueOnError 策略的严格记录。
fn record_with_continue_on_error(
    // 接收固定步骤数量。
    step_count: usize,
    // 接收原始 Workflow 遇错继续策略。
    continue_on_error: bool,
) -> Result<SequenceExecutionRecord, SequenceExecutionRecordError> {
    // 构造最小 provider-neutral steps。
    let steps = (0..step_count)
        // 每个步骤只使用只读状态请求。
        .map(|_| json!({ "verb": "status", "app": "desktop" }))
        // 收集公开输入数组。
        .collect::<Vec<_>>();
    // 构造互不重复的稳定 step identities。
    let step_ids = (0..step_count)
        // 以固定宽度小写十六进制生成测试 identity。
        .map(|index| format!("s2:qs:{:032x}", index + 1))
        // 收集有序 identities。
        .collect::<Vec<_>>();
    // 建立 revision 零记录。
    SequenceExecutionRecord::create(
        // 使用 canonical execution identity。
        "s2:q:00000000000000000000000000000001".to_owned(),
        // 保存固定 start nonce。
        START_NONCE.to_owned(),
        // 保存固定 start digest。
        START_DIGEST.to_owned(),
        // 保存首次 broker epoch。
        FIRST_EPOCH.to_owned(),
        // 保存固定 input digest。
        INPUT_DIGEST.to_owned(),
        // 保存最小公开输入。
        json!({ "steps": steps, "continueOnError": continue_on_error }),
        // 保存稳定 step identities。
        step_ids,
    )
}

// 构造可信 worker final 证据。
fn final_evidence(kind: &str) -> Value {
    // 返回不含 provider 私有数据的固定对象。
    json!({ "workerFinal": kind })
}

// 构造隐私安全的 provider 失败 envelope。
fn provider_error() -> Value {
    // 返回稳定错误码和消息。
    json!({
        // 使用 canonical 大写错误码。
        "code": "PROVIDER_FAILED",
        // 保持消息有界且不含私有值。
        "message": "The provider returned a trusted failure."
    })
}

// 验证新记录严格 roundtrip 且全部 step identity 稳定。
#[test]
fn created_record_roundtrips_with_stable_identities() -> Result<(), SequenceExecutionRecordError> {
    // 建立两步记录。
    let record = record(2)?;
    // 新记录从 revision 零和 created 开始。
    assert_eq!(record.revision(), 0);
    // execution identity 必须保持 canonical。
    assert_eq!(
        record.execution_id(),
        "s2:q:00000000000000000000000000000001"
    );
    // snapshot 必须绑定同一 identity。
    assert_eq!(record.snapshot().execution_id(), record.execution_id());
    // 新 execution 尚无当前步骤。
    assert_eq!(record.snapshot().current_step(), None);
    // 两个 receipt 都从 pending revision 零开始。
    for (index, step) in record.snapshot().steps().iter().enumerate() {
        // 一基索引必须稳定。
        assert_eq!(step.step_index(), index + 1);
        // pending revision 固定为零。
        assert_eq!(step.revision(), 0);
        // 新 receipt 不可能已 dispatch。
        assert!(!step.accepted_may_have_occurred());
        // identity 必须使用独立 step 类别。
        assert!(step.step_id().starts_with("s2:qs:"));
    }
    // 编码完整紧凑 JSON。
    let bytes = record.encode()?;
    // 严格解码同一文档。
    let decoded = SequenceExecutionRecord::decode(&bytes)?;
    // roundtrip 不得改变任何领域事实。
    assert_eq!(decoded, record);
    // 测试正常完成。
    Ok(())
}

// 验证 prepare 与 dispatch 严格单调且 stale revision 无部分修改。
#[test]
fn prepare_and_dispatch_require_order_and_expected_revision()
-> Result<(), SequenceExecutionRecordError> {
    // 建立两步记录以验证禁止跳步。
    let mut record = record(2)?;
    // 第二步不能先于第一步准备。
    assert_eq!(
        record.prepare_step(0, 2, STEP_DIGEST, "policy-v1", FIRST_EPOCH),
        Err(SequenceExecutionRecordError::InvalidTransition)
    );
    // 失败不能推进 revision。
    assert_eq!(record.revision(), 0);
    // 正确准备第一步。
    record.prepare_step(0, 1, STEP_DIGEST, "policy-v1", FIRST_EPOCH)?;
    // record 与 step 都严格加一。
    assert_eq!(record.revision(), 1);
    // 当前步骤固定为一。
    assert_eq!(record.snapshot().current_step(), Some(1));
    // receipt 进入 prepared。
    assert_eq!(
        record.snapshot().steps()[0].state(),
        SequenceStepState::Prepared
    );
    // 保存合法状态以比较失败原子性。
    let prepared = record.clone();
    // stale expected revision 必须拒绝。
    assert_eq!(
        record.begin_dispatch(0, 1, DISPATCH_NONCE, FIRST_EPOCH),
        Err(SequenceExecutionRecordError::RevisionConflict)
    );
    // stale 请求不得污染任何字段。
    assert_eq!(record, prepared);
    // 当前 revision 可进入 dispatching。
    record.begin_dispatch(1, 1, DISPATCH_NONCE, FIRST_EPOCH)?;
    // record revision 严格推进到二。
    assert_eq!(record.revision(), 2);
    // receipt revision 与状态同步推进。
    assert_eq!(record.snapshot().steps()[0].revision(), 2);
    // dispatch receipt 先保守记录可能接受。
    assert!(record.snapshot().steps()[0].accepted_may_have_occurred());
    // 重复 prepare established step 必须拒绝。
    assert_eq!(
        record.prepare_step(2, 1, STEP_DIGEST, "policy-v1", FIRST_EPOCH),
        Err(SequenceExecutionRecordError::InvalidTransition)
    );
    // 测试正常完成。
    Ok(())
}

// 验证 prepared 中断可恢复而 dispatching 中断永久未知。
#[test]
fn interruption_recovery_never_redispatches_established_step()
-> Result<(), SequenceExecutionRecordError> {
    // 建立并准备第一步。
    let mut prepared = record(1)?;
    // 保存可证明未 dispatch 的 prepared receipt。
    prepared.prepare_step(0, 1, STEP_DIGEST, "policy-v1", FIRST_EPOCH)?;
    // 新 broker 把它收敛到安全等待点。
    assert!(prepared.recover_after_interruption(SECOND_EPOCH)?);
    // execution 允许显式 resume。
    assert_eq!(
        prepared.snapshot().state(),
        SequenceExecutionState::AwaitingResume
    );
    // receipt 保持 prepared 且未报告接受。
    assert_eq!(
        prepared.snapshot().steps()[0].state(),
        SequenceStepState::Prepared
    );
    // 未 dispatch 事实保持为假。
    assert!(!prepared.snapshot().steps()[0].accepted_may_have_occurred());
    // 第二次启动读取稳定等待点不得推进 revision。
    let stable_revision = prepared.revision();
    // 稳定等待点无需再次恢复。
    assert!(!prepared.recover_after_interruption(FIRST_EPOCH)?);
    // revision 必须保持不变。
    assert_eq!(prepared.revision(), stable_revision);
    // 保存恢复冲突前的完整记录。
    let before_conflict = prepared.clone();
    // Policy revision 漂移必须拒绝恢复。
    assert_eq!(
        prepared.resume_prepared(stable_revision, 1, STEP_DIGEST, "policy-v2", SECOND_EPOCH),
        Err(SequenceExecutionRecordError::ResumeConflict)
    );
    // 恢复冲突不得污染任何持久事实。
    assert_eq!(prepared, before_conflict);
    // 逐值相同的摘要和 Policy revision 可以重新进入 running。
    prepared.resume_prepared(stable_revision, 1, STEP_DIGEST, "policy-v1", SECOND_EPOCH)?;
    // 只推进 execution revision，不重做 prepared step。
    assert_eq!(prepared.snapshot().steps()[0].revision(), 1);
    // execution 已重新进入运行中。
    assert_eq!(prepared.snapshot().state(), SequenceExecutionState::Running);
    // 随后才能建立 dispatching receipt。
    prepared.begin_dispatch(prepared.revision(), 1, DISPATCH_NONCE, SECOND_EPOCH)?;
    // receipt 单调推进为 dispatching。
    assert_eq!(
        prepared.snapshot().steps()[0].state(),
        SequenceStepState::Dispatching
    );

    // 建立已经持久 dispatching 的独立记录。
    let mut dispatching = record(1)?;
    // 准备最终请求摘要。
    dispatching.prepare_step(0, 1, STEP_DIGEST, "policy-v1", FIRST_EPOCH)?;
    // 在 worker 前建立 dispatch receipt。
    dispatching.begin_dispatch(1, 1, DISPATCH_NONCE, FIRST_EPOCH)?;
    // 模拟 broker crash 后恢复。
    assert!(dispatching.recover_after_interruption(SECOND_EPOCH)?);
    // 整个 execution 必须进入未知终态。
    assert_eq!(
        dispatching.snapshot().state(),
        SequenceExecutionState::OutcomeUnknown
    );
    // step receipt 同样不可重派。
    assert_eq!(
        dispatching.snapshot().steps()[0].state(),
        SequenceStepState::OutcomeUnknown
    );
    // unknown 使用固定错误码。
    assert_eq!(
        dispatching.snapshot().steps()[0].error_code(),
        Some("OUTCOME_UNKNOWN")
    );
    // 终态再次恢复不得推进 revision。
    let terminal_revision = dispatching.revision();
    // 终态读取不是迁移。
    assert!(!dispatching.recover_after_interruption(FIRST_EPOCH)?);
    // revision 保持不变。
    assert_eq!(dispatching.revision(), terminal_revision);
    // 测试正常完成。
    Ok(())
}

// 验证严格 codec 拒绝缺失字段、重复 identity 和跨字段漂移。
#[test]
fn strict_decode_rejects_corrupt_or_ambiguous_records() -> Result<(), SequenceExecutionRecordError>
{
    // 取得一条可信两步记录的 JSON 对象。
    let base = serde_json::to_value(record(2)?)
        // Value 序列化理论上必须成功。
        .map_err(|_| SequenceExecutionRecordError::InvalidJson)?;
    // 构造缺失 required recordRevision 的文档。
    let mut missing = base.clone();
    // 移除必需字段。
    missing
        // 取得顶层对象。
        .as_object_mut()
        // 测试输入必须是对象。
        .ok_or(SequenceExecutionRecordError::InvalidJson)?
        // 删除 revision。
        .remove("recordRevision");
    // Serde 必须在跨字段验证前拒绝缺失非可选字段。
    assert_eq!(
        SequenceExecutionRecord::decode(&serde_json::to_vec(&missing).unwrap_or_default()),
        Err(SequenceExecutionRecordError::InvalidJson)
    );

    // 构造 snapshot revision 与 record revision 不一致的文档。
    let mut revision = base.clone();
    // 覆盖 snapshot revision。
    revision["snapshot"]["executionRevision"] = json!(9);
    // 双 revision 漂移必须拒绝。
    assert_eq!(
        SequenceExecutionRecord::decode(&serde_json::to_vec(&revision).unwrap_or_default()),
        Err(SequenceExecutionRecordError::InvalidRecord)
    );

    // 构造两个 step 使用同一 identity 的文档。
    let mut duplicate = base;
    // 复制第一步 identity 到第二步。
    duplicate["snapshot"]["steps"][1]["stepId"] =
        duplicate["snapshot"]["steps"][0]["stepId"].clone();
    // 歧义 identity 必须失败闭合。
    assert_eq!(
        SequenceExecutionRecord::decode(&serde_json::to_vec(&duplicate).unwrap_or_default()),
        Err(SequenceExecutionRecordError::InvalidIdentity)
    );
    // 空文档使用独立 parser 错误。
    assert_eq!(
        SequenceExecutionRecord::decode(&[]),
        Err(SequenceExecutionRecordError::InvalidJson)
    );
    // 测试正常完成。
    Ok(())
}

// 验证 JSON null 结果仍能作为完整成功值表达。
#[test]
fn result_slot_uses_required_json_value_shape() -> Result<(), SequenceExecutionRecordError> {
    // 创建一条记录并读取序列化字段。
    let encoded = record(1)?.encode()?;
    // 解析为通用 JSON 以核对 required null 字段存在。
    let value: Value = serde_json::from_slice(&encoded)
        // 理论解析失败转为测试错误。
        .map_err(|_| SequenceExecutionRecordError::InvalidJson)?;
    // result 字段必须存在且显式为 null。
    assert!(value["snapshot"]["steps"][0].get("result").is_some());
    // semanticDigest 同样必须存在而不是依赖 Option 缺省。
    assert!(
        value["snapshot"]["steps"][0]
            .get("semanticDigest")
            .is_some()
    );
    // 测试正常完成。
    Ok(())
}

// 验证可信 success final 逐步推进并只在末尾发布 Workflow 结果。
#[test]
fn trusted_success_finals_advance_to_completed() -> Result<(), SequenceExecutionRecordError> {
    // 建立两步 execution。
    let mut record = record(2)?;
    // 准备第一步。
    record.prepare_step(0, 1, STEP_DIGEST, "policy-v1", FIRST_EPOCH)?;
    // 先持久 dispatching receipt。
    record.begin_dispatch(1, 1, DISPATCH_NONCE, FIRST_EPOCH)?;
    // 非最后步骤不得接收 Workflow 结果。
    let before_invalid = record.clone();
    // 提前发布 Workflow 结果必须失败。
    assert_eq!(
        record.finish_step_success(
            2,
            1,
            json!({ "value": 1 }),
            final_evidence("completed"),
            Some(json!({ "ok": true })),
            FIRST_EPOCH,
        ),
        Err(SequenceExecutionRecordError::InvalidRecord)
    );
    // 失败不得污染 dispatching 事实。
    assert_eq!(record, before_invalid);
    // 建立第一步可信 success final。
    record.finish_step_success(
        2,
        1,
        json!({ "value": 1 }),
        final_evidence("completed"),
        None,
        FIRST_EPOCH,
    )?;
    // execution 进入第二步安全检查点。
    assert_eq!(
        record.snapshot().state(),
        SequenceExecutionState::AwaitingResume
    );
    // currentStep 指向第二步。
    assert_eq!(record.snapshot().current_step(), Some(2));
    // 第一步保存可信完成。
    assert_eq!(
        record.snapshot().steps()[0].state(),
        SequenceStepState::Completed
    );
    // Workflow 结果仍为 null。
    assert!(record.snapshot().workflow_result.is_null());

    // 准备第二步。
    record.prepare_step(record.revision(), 2, STEP_DIGEST, "policy-v1", FIRST_EPOCH)?;
    // 建立独立 dispatch nonce。
    record.begin_dispatch(
        record.revision(),
        2,
        "55555555555555555555555555555555",
        FIRST_EPOCH,
    )?;
    // 最后一步必须同时发布完整 Workflow 结果。
    record.finish_step_success(
        record.revision(),
        2,
        Value::Null,
        final_evidence("completed"),
        Some(json!({ "ok": true, "completed": 2 })),
        FIRST_EPOCH,
    )?;
    // execution 建立 completed 终态。
    assert_eq!(record.snapshot().state(), SequenceExecutionState::Completed);
    // completed 不可 resume。
    assert!(!record.snapshot().resumable);
    // Workflow 结果保存为对象。
    assert!(record.snapshot().workflow_result.is_object());
    // 测试正常完成。
    Ok(())
}

// 验证普通失败只按原始 continueOnError 策略决定后续步骤。
#[test]
fn trusted_failure_respects_persisted_continue_policy() -> Result<(), SequenceExecutionRecordError>
{
    // 建立遇错停止的两步 execution。
    let mut stopping = record_with_continue_on_error(2, false)?;
    // 准备并 dispatch 第一步。
    stopping.prepare_step(0, 1, STEP_DIGEST, "policy-v1", FIRST_EPOCH)?;
    // 建立 dispatching receipt。
    stopping.begin_dispatch(1, 1, DISPATCH_NONCE, FIRST_EPOCH)?;
    // 可信失败必须同时发布终止 Workflow 结果。
    stopping.finish_step_failure(
        2,
        1,
        provider_error(),
        final_evidence("failed"),
        Some(json!({ "ok": false, "failed": 1 })),
        FIRST_EPOCH,
    )?;
    // execution 停止为 failed。
    assert_eq!(stopping.snapshot().state(), SequenceExecutionState::Failed);
    // 后续第二步仍保持 pending。
    assert_eq!(
        stopping.snapshot().steps()[1].state(),
        SequenceStepState::Pending
    );

    // 建立允许普通错误后继续的两步 execution。
    let mut continuing = record_with_continue_on_error(2, true)?;
    // 准备并 dispatch 第一步。
    continuing.prepare_step(0, 1, STEP_DIGEST, "policy-v1", FIRST_EPOCH)?;
    // 建立 dispatching receipt。
    continuing.begin_dispatch(1, 1, DISPATCH_NONCE, FIRST_EPOCH)?;
    // 非末尾失败不能提前发布 Workflow 结果。
    continuing.finish_step_failure(
        2,
        1,
        provider_error(),
        final_evidence("failed"),
        None,
        FIRST_EPOCH,
    )?;
    // execution 进入下一步安全检查点。
    assert_eq!(
        continuing.snapshot().state(),
        SequenceExecutionState::AwaitingResume
    );
    // 前缀保留可信失败。
    assert_eq!(
        continuing.snapshot().steps()[0].state(),
        SequenceStepState::Failed
    );
    // 下一步仍可按原顺序准备。
    continuing.prepare_step(
        continuing.revision(),
        2,
        STEP_DIGEST,
        "policy-v1",
        FIRST_EPOCH,
    )?;
    // 建立第二步 dispatching receipt。
    continuing.begin_dispatch(
        continuing.revision(),
        2,
        "55555555555555555555555555555555",
        FIRST_EPOCH,
    )?;
    // 第二步成功使全部声明步骤处理完成。
    continuing.finish_step_success(
        continuing.revision(),
        2,
        json!({ "value": 2 }),
        final_evidence("completed"),
        Some(json!({ "ok": false, "completed": 1, "failed": 1 })),
        FIRST_EPOCH,
    )?;
    // 运行到末尾使用 completed 状态并由结果报告普通失败。
    assert_eq!(
        continuing.snapshot().state(),
        SequenceExecutionState::Completed
    );
    // 测试正常完成。
    Ok(())
}

// 验证取消请求不会把已 dispatch 步骤静默冒充为 Cancelled。
#[test]
fn cancellation_requires_authoritative_stop_after_dispatch()
-> Result<(), SequenceExecutionRecordError> {
    // created 尚未 dispatch，可直接取消。
    let mut created = record(1)?;
    // 持久化取消请求。
    assert!(created.request_cancel(0, FIRST_EPOCH)?);
    // execution 建立取消终态。
    assert_eq!(
        created.snapshot().state(),
        SequenceExecutionState::Cancelled
    );
    // 取消意图保持可查询。
    assert!(created.snapshot().cancel_requested);
    // pending receipt 不被伪造成 worker final。
    assert_eq!(
        created.snapshot().steps()[0].state(),
        SequenceStepState::Pending
    );

    // 建立已经 dispatching 的独立 execution。
    let mut dispatched = record(1)?;
    // 准备当前步骤。
    dispatched.prepare_step(0, 1, STEP_DIGEST, "policy-v1", FIRST_EPOCH)?;
    // 建立可能接受事实。
    dispatched.begin_dispatch(1, 1, DISPATCH_NONCE, FIRST_EPOCH)?;
    // cancel 只先持久取消意图。
    assert!(dispatched.request_cancel(2, FIRST_EPOCH)?);
    // 不能仅因请求取消就宣称 Cancelled。
    assert_eq!(
        dispatched.snapshot().state(),
        SequenceExecutionState::Running
    );
    // 同 revision 的重复取消不推进状态。
    let cancel_revision = dispatched.revision();
    // 重复请求返回没有迁移。
    assert!(!dispatched.request_cancel(cancel_revision, SECOND_EPOCH)?);
    // revision 保持不变。
    assert_eq!(dispatched.revision(), cancel_revision);
    // 只有 worker 可信停止 final 可以建立 Cancelled。
    dispatched.finish_cancelled(
        cancel_revision,
        1,
        final_evidence("cancelled"),
        SECOND_EPOCH,
    )?;
    // execution 进入取消终态。
    assert_eq!(
        dispatched.snapshot().state(),
        SequenceExecutionState::Cancelled
    );
    // step 使用可信失败 final 表达权威取消。
    assert_eq!(
        dispatched.snapshot().steps()[0].state(),
        SequenceStepState::Failed
    );
    // 固定错误码可安全查询。
    assert_eq!(
        dispatched.snapshot().steps()[0].error_code(),
        Some("CANCELLED")
    );
    // 测试正常完成。
    Ok(())
}
