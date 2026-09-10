//! 验证长操作 registry 的容量、原子状态、恢复、取消、预算与到期。

// 导入被测私有 Module。
use super::*;
// 导入 fixture 文件与唯一序列工具。
use std::{
    // 导入测试文件操作。
    fs,
    // 导入 fixture 路径类型。
    path::PathBuf,
    // 导入并行测试唯一序列。
    sync::atomic::{AtomicU64, Ordering},
};

// 导入 JSON fixture 宏。
use serde_json::json;

// 为测试目录提供唯一序列。
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 独占一个 registry journal fixture。
struct FixtureRegistry {
    // 保存精确父目录。
    path: PathBuf,
}

// 构造 registry fixture。
impl FixtureRegistry {
    // 创建唯一父目录。
    fn new(label: &str) -> Self {
        // 取得进程内唯一编号。
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 在系统临时目录下使用固定安全前缀。
        let path = std::env::temp_dir().join(format!(
            // 固定前缀、进程、标签与序列。
            "act-long-operation-registry-{}-{label}-{sequence}",
            // 注入当前测试进程 ID。
            std::process::id(),
        ));
        // 清理相同精确旧 fixture。
        let _ = fs::remove_dir_all(&path);
        // 创建真实父目录。
        fs::create_dir(&path)
            // 失败时提供测试诊断。
            .unwrap_or_else(|error| panic!("fixture creation failed: {error}"));
        // 返回 fixture 所有者。
        Self { path }
    }

    // 打开同一 journal 的新 registry 实例。
    fn open(&self, now_ms: u64) -> Result<LongOperationRegistry, LongOperationRegistryError> {
        // 打开固定 journal 叶目录。
        let journal = LongOperationJournal::open(&self.path.join("journal"))?;
        // 加载并恢复 registry。
        LongOperationRegistry::open(journal, now_ms)
    }
}

// 作用域结束时清理精确 fixture。
impl Drop for FixtureRegistry {
    // 删除当前实例拥有的目录。
    fn drop(&mut self) {
        // 只删除唯一固定前缀路径。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 从递增整数构造 canonical operation handle。
fn operation_id(index: u64) -> String {
    // 固定十六进制宽度。
    format!("s2:o:{index:016x}")
}

// 构造验证通过的稳定失败。
fn failure(code: &str) -> LongOperationFailure {
    // 使用固定安全消息。
    LongOperationFailure::new(code, "The operation failed in the owned test fixture.")
        // 测试调用点必须传入合法错误码。
        .unwrap_or_else(|| panic!("invalid failure fixture: {code}"))
}

// 验证活动容量、重复 handle 与持久查询。
#[test]
fn acceptance_is_bounded_unique_and_durable() {
    // 创建独占 fixture。
    let fixture = FixtureRegistry::new("accept");
    // 打开空 registry。
    let mut registry = fixture
        // 使用固定时刻。
        .open(1_000)
        // 打开必须成功。
        .unwrap_or_else(|error| panic!("registry open failed: {error:?}"));
    // 接受固定活动预算数量。
    for index in 1..=MAX_ACTIVE_OPERATIONS as u64 {
        // 构造 canonical handle。
        let handle = operation_id(index);
        // 原子接受记录。
        let record = registry
            // 提交封闭 capability。
            .accept(&handle, WINDOW_RECORD_CAPABILITY, 1_000 + index)
            // 接受必须成功。
            .unwrap_or_else(|error| panic!("accept failed: {error:?}"));
        // 初始状态固定 accepted。
        assert_eq!(record.status(), LongOperationStatus::Accepted);
    }
    // 活动数量达到固定预算。
    assert_eq!(registry.active_count(), MAX_ACTIVE_OPERATIONS);
    // 第五个活动任务在业务接受前失败。
    assert_eq!(
        // 尝试超额接受。
        registry.accept(&operation_id(5), WINDOW_RECORD_CAPABILITY, 2_000),
        // 返回容量耗尽。
        Err(LongOperationRegistryError::CapacityExhausted)
    );
    // 重复 handle 不得覆盖既有记录。
    assert_eq!(
        // 尝试重复第一个 handle。
        registry.accept(&operation_id(1), WINDOW_RECORD_CAPABILITY, 2_000),
        // 返回重复错误。
        Err(LongOperationRegistryError::DuplicateOperation)
    );
    // 查询不修改修订号。
    let before = registry
        // 第一次查询。
        .status(&operation_id(1), 2_000)
        // 查询必须成功。
        .unwrap_or_else(|error| panic!("status failed: {error:?}"));
    // 再次查询同一记录。
    let after = registry
        // 第二次查询。
        .status(&operation_id(1), 3_000)
        // 查询必须成功。
        .unwrap_or_else(|error| panic!("status failed: {error:?}"));
    // Query 不得修改 revision。
    assert_eq!(before.revision(), after.revision());
}

// 验证未到期终态同样受 128 条总记录预算约束。
#[test]
fn tracked_terminal_records_cannot_exceed_fixed_capacity() {
    // 创建独占 fixture。
    let fixture = FixtureRegistry::new("tracked-capacity");
    // 打开空 registry。
    let mut registry = fixture
        // 使用固定时刻。
        .open(10_000)
        // 打开必须成功。
        .unwrap_or_else(|error| panic!("registry open failed: {error:?}"));
    // 依次接受并终结全部记录预算。
    for index in 1..=MAX_TRACKED_OPERATIONS as u64 {
        // 构造唯一 canonical handle。
        let handle = operation_id(1_000 + index);
        // 计算不会触发到期的接受时刻。
        let accepted_at = 10_000 + index * 2;
        // 接受单个任务。
        registry
            // 建立 accepted 记录。
            .accept(&handle, WINDOW_RECORD_CAPABILITY, accepted_at)
            // 接受必须成功。
            .unwrap_or_else(|error| panic!("accept {index} failed: {error:?}"));
        // 立即以 dispatch 前失败释放活动预算。
        registry
            // 使用唯一终态时刻。
            .fail(&handle, failure("FIXTURE_FAILED"), accepted_at + 1)
            // 终结必须成功。
            .unwrap_or_else(|error| panic!("fail {index} failed: {error:?}"));
    }
    // 总记录达到固定预算。
    assert_eq!(registry.tracked_count(), MAX_TRACKED_OPERATIONS);
    // 当前没有活动任务。
    assert_eq!(registry.active_count(), 0);
    // 第 129 条在业务接受前失败。
    assert_eq!(
        // 尝试接受额外任务。
        registry.accept(
            &operation_id(9_999),
            WINDOW_RECORD_CAPABILITY,
            10_000 + MAX_TRACKED_OPERATIONS as u64 * 2 + 2,
        ),
        // 返回总容量耗尽。
        Err(LongOperationRegistryError::CapacityExhausted)
    );
}

// 验证取消幂等且完成证据赢得取消竞争。
#[test]
fn cancellation_is_idempotent_and_completion_wins_the_race() {
    // 创建独占 fixture。
    let fixture = FixtureRegistry::new("cancel");
    // 打开空 registry。
    let mut registry = fixture
        // 使用固定时刻。
        .open(10)
        // 打开必须成功。
        .unwrap_or_else(|error| panic!("registry open failed: {error:?}"));
    // 固定 handle。
    let handle = operation_id(10);
    // 接受任务。
    registry
        // 建立持久 handle。
        .accept(&handle, WINDOW_RECORD_CAPABILITY, 10)
        // 接受必须成功。
        .unwrap_or_else(|error| panic!("accept failed: {error:?}"));
    // 持久 dispatch 事实。
    registry
        // 进入 running。
        .start_dispatch(&handle, 20)
        // dispatch 必须成功。
        .unwrap_or_else(|error| panic!("dispatch failed: {error:?}"));
    // 首次取消需要传播。
    let (first_effect, first) = registry
        // 请求取消。
        .request_cancel(&handle, 30)
        // 取消必须成功。
        .unwrap_or_else(|error| panic!("cancel failed: {error:?}"));
    // 验证首次效果。
    assert_eq!(first_effect, CancelRequestEffect::Requested);
    // 验证持久取消事实。
    assert!(first.cancel_requested());
    // 重复取消不增加修订。
    let (second_effect, second) = registry
        // 重复取消。
        .request_cancel(&handle, 40)
        // 重复取消必须成功。
        .unwrap_or_else(|error| panic!("repeat cancel failed: {error:?}"));
    // 验证幂等效果。
    assert_eq!(second_effect, CancelRequestEffect::AlreadyRequested);
    // revision 不得变化。
    assert_eq!(first.revision(), second.revision());
    // worker 成功证据可从 cancel-requested 完成。
    let (effect, completed) = registry
        // 提交有界结果。
        .complete(&handle, json!({"frames": 7}), 50)
        // 完成必须成功。
        .unwrap_or_else(|error| panic!("complete failed: {error:?}"));
    // 返回成功分类。
    assert_eq!(effect, CompletionEffect::Completed);
    // 终态为 completed。
    assert_eq!(completed.status(), LongOperationStatus::Completed);
    // 原取消事实仍被保留。
    assert!(completed.cancel_requested());
    // 终态取消不得覆盖或增加修订。
    let (terminal_effect, terminal) = registry
        // 终态重复取消。
        .request_cancel(&handle, 60)
        // 调用必须成功。
        .unwrap_or_else(|error| panic!("terminal cancel failed: {error:?}"));
    // 返回已终态分类。
    assert_eq!(terminal_effect, CancelRequestEffect::AlreadyTerminal);
    // revision 保持不变。
    assert_eq!(terminal.revision(), completed.revision());
}

// 验证超限结果失败且不会保存截断 payload。
#[test]
fn oversized_result_fails_without_persisting_partial_result() {
    // 创建独占 fixture。
    let fixture = FixtureRegistry::new("result-budget");
    // 打开空 registry。
    let mut registry = fixture
        // 使用固定时刻。
        .open(100)
        // 打开必须成功。
        .unwrap_or_else(|error| panic!("registry open failed: {error:?}"));
    // 固定 handle。
    let handle = operation_id(20);
    // 接受并 dispatch。
    registry
        // 建立 accepted。
        .accept(&handle, WINDOW_RECORD_CAPABILITY, 100)
        // 接受必须成功。
        .unwrap_or_else(|error| panic!("accept failed: {error:?}"));
    // 开始 dispatch。
    registry
        // 建立 running。
        .start_dispatch(&handle, 110)
        // dispatch 必须成功。
        .unwrap_or_else(|error| panic!("dispatch failed: {error:?}"));
    // 构造必然超过 JSON 一 MiB 的字符串结果。
    let oversized = Value::String("x".repeat(MAX_RESULT_BYTES));
    // 尝试完成超限结果。
    let (effect, record) = registry
        // 提交完整超限值。
        .complete(&handle, oversized, 120)
        // 应以可靠失败完成调用。
        .unwrap_or_else(|error| panic!("oversized completion failed: {error:?}"));
    // 返回超限失败分类。
    assert_eq!(effect, CompletionEffect::FailedResultTooLarge);
    // 任务收敛为 failed。
    assert_eq!(record.status(), LongOperationStatus::Failed);
    // 不保存任何部分结果。
    assert!(record.result().is_none());
    // 保存固定错误码。
    assert_eq!(
        record.error().map(LongOperationFailure::code),
        Some("OPERATION_RESULT_TOO_LARGE")
    );
}

// 验证 broker 重启按 dispatch 事实持久恢复。
#[test]
fn restart_recovers_pre_and_post_dispatch_distinctly() {
    // 创建独占 fixture。
    let fixture = FixtureRegistry::new("recovery");
    // 固定两个 handle。
    let queued = operation_id(30);
    // 固定 dispatch 后 handle。
    let running = operation_id(31);
    // 在局部作用域创建首个 broker registry。
    {
        // 打开空 registry。
        let mut registry = fixture
            // 使用固定时刻。
            .open(1_000)
            // 打开必须成功。
            .unwrap_or_else(|error| panic!("registry open failed: {error:?}"));
        // 接受 dispatch 前任务。
        registry
            // 建立 queued 记录。
            .accept(&queued, WINDOW_RECORD_CAPABILITY, 1_000)
            // 接受必须成功。
            .unwrap_or_else(|error| panic!("queued accept failed: {error:?}"));
        // 接受第二个任务。
        registry
            // 建立 running 候选。
            .accept(&running, WINDOW_RECORD_CAPABILITY, 1_001)
            // 接受必须成功。
            .unwrap_or_else(|error| panic!("running accept failed: {error:?}"));
        // 持久第二个 dispatch 事实。
        registry
            // 进入 running。
            .start_dispatch(&running, 1_002)
            // dispatch 必须成功。
            .unwrap_or_else(|error| panic!("dispatch failed: {error:?}"));
        // 作用域结束模拟 broker 中断。
    }
    // 重新打开同一 journal。
    let mut recovered = fixture
        // 使用启动时刻。
        .open(2_000)
        // 恢复必须成功。
        .unwrap_or_else(|error| panic!("recovery open failed: {error:?}"));
    // 查询 dispatch 前恢复结果。
    let queued_record = recovered
        // 查询 queued handle。
        .status(&queued, 2_000)
        // 查询必须成功。
        .unwrap_or_else(|error| panic!("queued status failed: {error:?}"));
    // dispatch 前收敛为 failed。
    assert_eq!(queued_record.status(), LongOperationStatus::Failed);
    // dispatch 前失败允许安全重试。
    assert!(queued_record.retry_safe());
    // 查询 dispatch 后恢复结果。
    let running_record = recovered
        // 查询 running handle。
        .status(&running, 2_000)
        // 查询必须成功。
        .unwrap_or_else(|error| panic!("running status failed: {error:?}"));
    // dispatch 后收敛为 outcome-unknown。
    assert_eq!(running_record.status(), LongOperationStatus::OutcomeUnknown);
    // dispatch 后不得自动重试。
    assert!(!running_record.retry_safe());
    // 固定未知错误码。
    assert_eq!(
        running_record.error().map(LongOperationFailure::code),
        Some("OUTCOME_UNKNOWN")
    );
}

// 验证终态到期不被查询延长并从持久索引清理。
#[test]
fn terminal_expiry_is_fixed_and_persists_across_restart() {
    // 创建独占 fixture。
    let fixture = FixtureRegistry::new("expiry");
    // 打开空 registry。
    let mut registry = fixture
        // 使用固定时刻。
        .open(5_000)
        // 打开必须成功。
        .unwrap_or_else(|error| panic!("registry open failed: {error:?}"));
    // 固定 handle。
    let handle = operation_id(40);
    // 接受任务。
    registry
        // 建立 accepted。
        .accept(&handle, WINDOW_RECORD_CAPABILITY, 5_000)
        // 接受必须成功。
        .unwrap_or_else(|error| panic!("accept failed: {error:?}"));
    // 以 dispatch 前可靠失败终结。
    let terminal = registry
        // 写入稳定失败。
        .fail(&handle, failure("FIXTURE_FAILED"), 6_000)
        // 失败终结必须成功。
        .unwrap_or_else(|error| panic!("fail transition failed: {error:?}"));
    // 取得固定到期时间。
    let expires_at = terminal
        // 终态必须有到期时间。
        .expires_at_ms()
        // 缺失时给出测试诊断。
        .unwrap_or_else(|| panic!("terminal expiry missing"));
    // 到期前查询成功。
    let queried = registry
        // 在边界前一毫秒查询。
        .status(&handle, expires_at - 1)
        // 查询必须成功。
        .unwrap_or_else(|error| panic!("pre-expiry status failed: {error:?}"));
    // 查询不得延长到期时间。
    assert_eq!(queried.expires_at_ms(), Some(expires_at));
    // 到期边界先撤销索引。
    assert_eq!(
        // 在精确边界查询。
        registry.status(&handle, expires_at),
        // 返回 not found。
        Err(LongOperationRegistryError::OperationNotFound)
    );
    // 内存索引已经清理。
    assert_eq!(registry.tracked_count(), 0);
    // 重新打开同一 journal 不得恢复已过期记录。
    let mut reopened = fixture
        // 使用到期后时刻。
        .open(expires_at + 1)
        // 打开必须成功。
        .unwrap_or_else(|error| panic!("reopen failed: {error:?}"));
    // 持久索引仍不包含该 handle。
    assert_eq!(
        // 查询旧 handle。
        reopened.status(&handle, expires_at + 1),
        // 返回 not found。
        Err(LongOperationRegistryError::OperationNotFound)
    );
}

// 验证损坏和未知版本 journal 在启动时失败闭合。
#[test]
fn corrupt_or_unknown_version_journal_is_rejected() {
    // 创建独占 fixture。
    let fixture = FixtureRegistry::new("corrupt");
    // 打开一次以创建 journal 目录。
    let journal = LongOperationJournal::open(&fixture.path.join("journal"))
        // 打开必须成功。
        .unwrap_or_else(|error| panic!("journal open failed: {error:?}"));
    // 写入语法损坏的 canonical 文件。
    journal
        // 使用固定 handle 与损坏 JSON。
        .persist(&operation_id(50), b"{")
        // 原子写入必须成功。
        .unwrap_or_else(|error| panic!("corrupt fixture persist failed: {error:?}"));
    // registry 必须拒绝损坏记录。
    assert!(matches!(
        // 尝试启动 registry。
        fixture.open(10_000),
        // 返回严格记录错误。
        Err(LongOperationRegistryError::InvalidRecord)
    ));
    // 删除损坏记录。
    journal
        // 清理精确 handle。
        .remove(&operation_id(50))
        // 删除必须成功。
        .unwrap_or_else(|error| panic!("corrupt fixture removal failed: {error:?}"));
    // 写入字段完整但版本未知的记录。
    let unknown_version = json!({
        // 使用未知 journal 版本。
        "contractVersion": "act/long-operation-journal/v2",
        // 文件名与正文 handle 一致。
        "operationId": operation_id(51),
        // 使用封闭 capability。
        "capabilityId": WINDOW_RECORD_CAPABILITY,
        // 使用合法非终态状态。
        "status": "accepted",
        // 尚未 dispatch。
        "dispatchStarted": false,
        // 尚未取消。
        "cancelRequested": false,
        // 首次修订。
        "revision": 1,
        // 固定创建时间。
        "createdAtMs": 10_000,
        // 固定更新时间。
        "updatedAtMs": 10_000,
        // 非终态没有到期。
        "expiresAtMs": null,
        // 非终态没有结果。
        "result": null,
        // 非终态没有错误。
        "error": null
    });
    // 序列化未知版本 fixture。
    let bytes = serde_json::to_vec(&unknown_version)
        // Value 序列化必须成功。
        .unwrap_or_else(|error| panic!("unknown version serialization failed: {error}"));
    // 原子写入未知版本记录。
    journal
        // 使用匹配 handle。
        .persist(&operation_id(51), &bytes)
        // 写入必须成功。
        .unwrap_or_else(|error| panic!("unknown version persist failed: {error:?}"));
    // registry 必须拒绝未知版本。
    assert!(matches!(
        // 尝试启动 registry。
        fixture.open(10_000),
        // 返回严格记录错误。
        Err(LongOperationRegistryError::InvalidRecord)
    ));
}
