//! 验证 broker-owned 录制任务的异步所有权与终态收敛。

// 导入文件、同步与唯一目录工具。
use std::{
    // 创建并清理精确测试目录。
    fs,
    // 保存测试目录。
    path::PathBuf,
    // 协调测试与 worker。
    sync::{
        // 分配唯一 fixture 序列。
        atomic::{AtomicU64, Ordering},
        // 使用有界等待的测试 channel。
        mpsc,
    },
    // 限制测试等待时间。
    time::{Duration, Instant},
};

// 导入受控 JSON 结果。
use serde_json::json;

// 导入被测 Module 私有入口。
use super::*;
// 导入真实原子 journal Component。
use crate::components::long_operation_journal::LongOperationJournal;
// 导入公开生命周期状态。
use crate::modules::long_operation::LongOperationStatus;

// 为并行测试分配唯一目录序列。
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 拥有精确测试目录与任务 Module。
struct RecordingTasksFixture {
    // 保存唯一可清理目录。
    path: PathBuf,
    // 保存被测任务 Module。
    tasks: LongOperationRecordingTasks,
}

// 构造真实 journal 支持的测试 Module。
impl RecordingTasksFixture {
    // 创建空 broker 代际。
    fn new(label: &str) -> Self {
        // 取得进程内唯一序列。
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 构造不会与生产 Known Folder 重叠的目录。
        let path = std::env::temp_dir().join(format!(
            // 固定前缀、进程、标签与序列。
            "act-long-operation-recording-{}-{label}-{sequence}",
            // 只用 PID 隔离测试目录。
            std::process::id(),
        ));
        // 清理同名测试残留。
        let _ = fs::remove_dir_all(&path);
        // 创建精确测试根。
        fs::create_dir(&path)
            // 失败时保留完整测试诊断。
            .unwrap_or_else(|error| panic!("recording task fixture creation failed: {error}"));
        // 打开真实原子 journal。
        let journal = LongOperationJournal::open(&path.join("journal"))
            // 失败时保留完整测试诊断。
            .unwrap_or_else(|error| panic!("recording task journal open failed: {error:?}"));
        // 取得当前单调可表达时刻。
        let now_ms = current_unix_milliseconds()
            // 当前 Windows 测试环境必须有有效系统时钟。
            .unwrap_or_else(|error| panic!("recording task clock failed: {error:?}"));
        // 完成空 registry 恢复。
        let registry = LongOperationRegistry::open(journal, now_ms)
            // 空 journal 必须可恢复。
            .unwrap_or_else(|error| panic!("recording task registry open failed: {error:?}"));
        // 返回完整 fixture。
        Self {
            // 保存精确测试目录。
            path,
            // 由被测 Module 接管 registry。
            tasks: LongOperationRecordingTasks::new(registry),
        }
    }

    // 返回当前测试时刻。
    fn now_ms(&self) -> u64 {
        // 读取与生产迁移相同的时钟。
        current_unix_milliseconds()
            // 当前 Windows 测试环境必须有有效系统时钟。
            .unwrap_or_else(|error| panic!("recording task fixture clock failed: {error:?}"))
    }

    // 有界等待一个 operation 进入持久终态。
    fn wait_for_terminal(&mut self, operation_id: &str) -> LongOperationRecord {
        // 固定测试等待截止点。
        let deadline = Instant::now() + Duration::from_secs(2);
        // 在截止前重复查询持久事实。
        loop {
            // 取得当前查询时刻。
            let now_ms = self.now_ms();
            // 查询并顺带回收已结束线程。
            let record = self
                // 使用被测 status 入口。
                .tasks
                // 读取同一 operation。
                .status(operation_id, now_ms)
                // 已接受任务必须持续可查询。
                .unwrap_or_else(|error| panic!("recording task polling failed: {error:?}"));
            // 首个终态立即返回。
            if record.terminal() {
                // 返回持久终态快照。
                return record;
            }
            // 超时表示 worker 生命周期未收敛。
            assert!(
                // 使用单调时钟判断截止。
                Instant::now() < deadline,
                // 保留固定测试诊断。
                "recording task did not reach a terminal state"
            );
            // 让出线程供 worker 推进。
            std::thread::yield_now();
        }
    }
}

// 作用域结束时回收 worker 并清理精确目录。
impl Drop for RecordingTasksFixture {
    // 执行可恢复清理。
    fn drop(&mut self) {
        // 确保没有线程继续持有 journal。
        let _ = self.tasks.shutdown();
        // 只删除当前 fixture 自建目录。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 验证 submit 立即返回且 client 生命周期不拥有 worker。
#[test]
fn submit_returns_before_worker_and_disconnect_does_not_cancel() {
    // 创建空任务代际。
    let mut fixture = RecordingTasksFixture::new("disconnect");
    // 固定 canonical operation handle。
    let operation_id = "s2:o:0000000000000101";
    // 创建由测试显式释放的 worker 门闩。
    let (release_tx, release_rx) = mpsc::channel::<()>();
    // 提交一个等待门闩的受控 worker。
    let accepted = fixture
        // 使用被测 Module。
        .tasks
        // 注入不触碰窗口或文件的执行函数。
        .submit_with(operation_id, fixture.now_ms(), move |_| {
            // 等待模拟 client 已断开后的独立释放。
            release_rx
                // 使用有界等待防止测试永久阻塞。
                .recv_timeout(Duration::from_secs(2))
                // 超时保留测试诊断。
                .unwrap_or_else(|error| panic!("recording task release timed out: {error}"));
            // 返回完整受控结果。
            Ok(json!({"artifact": "fixture"}))
        })
        // 业务接受必须成功。
        .unwrap_or_else(|error| panic!("recording task submit failed: {error:?}"));
    // submit 返回持久 accepted 快照而不等待 worker。
    assert_eq!(accepted.status(), LongOperationStatus::Accepted);
    // 模拟 client 断开后释放 worker。
    release_tx
        // 发送唯一完成信号。
        .send(())
        // channel 必须仍由 worker 持有。
        .unwrap_or_else(|error| panic!("recording task release failed: {error}"));
    // 在不发送 broker 取消的情况下等待持久终态。
    let completed = fixture.wait_for_terminal(operation_id);
    // worker 在 client 生命周期外完成。
    assert_eq!(completed.status(), LongOperationStatus::Completed);
    // dispatch 事实必须已经持久。
    assert!(completed.dispatch_started());
    // 完整结果必须被保存。
    assert_eq!(completed.result(), Some(&json!({"artifact": "fixture"})));
}

// 验证运行中取消传播到逐任务令牌并可靠终结。
#[test]
fn cancel_is_persisted_and_propagated_to_running_worker() {
    // 创建空任务代际。
    let mut fixture = RecordingTasksFixture::new("cancel");
    // 固定 canonical operation handle。
    let operation_id = "s2:o:0000000000000102";
    // 创建 worker 已开始信号。
    let (started_tx, started_rx) = mpsc::channel::<()>();
    // 提交观察逐任务取消令牌的 worker。
    fixture
        // 使用被测 Module。
        .tasks
        // 注入受控执行函数。
        .submit_with(operation_id, fixture.now_ms(), move |cancelled| {
            // 告知测试已越过 dispatch 并进入执行函数。
            started_tx
                // 发送唯一开始信号。
                .send(())
                // 接收方必须仍存活。
                .unwrap_or_else(|error| panic!("recording task start signal failed: {error}"));
            // 有界轮询逐任务取消令牌。
            for _ in 0..2_000 {
                // 观察 acquire 取消事实。
                if cancelled.load(Ordering::Acquire) {
                    // 返回统一取消失败。
                    return Err(AppControlError::new("CANCELLED", "fixture cancellation"));
                }
                // 让出线程避免忙等独占。
                std::thread::yield_now();
            }
            // 未观察到取消表示传播失败。
            Err(AppControlError::new(
                // 使用测试专用失败码。
                "OPERATION_FAILED",
                // 保留简短测试诊断。
                "fixture did not observe cancellation",
            ))
        })
        // 业务接受必须成功。
        .unwrap_or_else(|error| panic!("cancellable recording task submit failed: {error:?}"));
    // 等待 worker 已经进入 running。
    started_rx
        // 使用有界等待防止测试永久阻塞。
        .recv_timeout(Duration::from_secs(2))
        // 超时保留测试诊断。
        .unwrap_or_else(|error| panic!("recording task start timed out: {error}"));
    // 持久接受首次取消并传播令牌。
    let (effect, cancelled) = fixture
        // 使用被测 cancel 入口。
        .tasks
        // 取消同一 operation。
        .cancel(operation_id, fixture.now_ms())
        // 取消必须可靠持久化。
        .unwrap_or_else(|error| panic!("recording task cancel failed: {error:?}"));
    // 首次取消使用 Requested 分类。
    assert_eq!(effect, CancelRequestEffect::Requested);
    // 返回快照公开独立取消事实。
    assert!(cancelled.cancel_requested());
    // 等待 worker 终态并回收线程。
    fixture
        // 使用被测关闭入口。
        .tasks
        // 持久取消并回收 worker。
        .shutdown()
        // 受控 worker 必须可靠关闭。
        .unwrap_or_else(|error| panic!("recording task shutdown failed: {error:?}"));
    // 查询最终失败记录。
    let failed = fixture
        // 使用被测 status 入口。
        .tasks
        // 读取同一 operation。
        .status(operation_id, fixture.now_ms())
        // 终态必须可查询。
        .unwrap_or_else(|error| panic!("cancelled recording task status failed: {error:?}"));
    // 取消后的 worker 证据终结为 failed。
    assert_eq!(failed.status(), LongOperationStatus::Failed);
    // dispatch 后失败不得声称安全重提。
    assert!(!failed.retry_safe());
    // 取消事实保持独立可见。
    assert!(failed.cancel_requested());
}

// 验证持久取消与 dispatch 竞争在同一 registry 锁内收敛。
#[test]
fn persisted_cancel_wins_before_dispatch() {
    // 创建空任务代际。
    let mut fixture = RecordingTasksFixture::new("predispatch-cancel");
    // 固定 canonical operation handle。
    let operation_id = "s2:o:0000000000000104";
    // 取得业务接受时刻。
    let accepted_at = fixture.now_ms();
    // 直接构造尚未创建 worker 的 accepted 夹具。
    fixture
        // 取得测试 registry 锁。
        .tasks
        // 使用只限测试的夹具入口。
        .registry_for_test()
        // 原子接受固定 capability。
        .accept(operation_id, WINDOW_RECORD_CAPABILITY, accepted_at)
        // 夹具接受必须成功。
        .unwrap_or_else(|error| panic!("predispatch fixture accept failed: {error:?}"));
    // 取得取消时刻。
    let cancelled_at = fixture.now_ms();
    // 在 worker dispatch 前持久接受取消。
    fixture
        // 取得测试 registry 锁。
        .tasks
        // 使用只限测试的夹具入口。
        .registry_for_test()
        // 持久取消同一 operation。
        .request_cancel(operation_id, cancelled_at)
        // 夹具取消必须成功。
        .unwrap_or_else(|error| panic!("predispatch fixture cancel failed: {error:?}"));
    // 模拟 worker 在取消后取得调度。
    let effect = transition_dispatch(
        // 借用被测共享 registry。
        &fixture.tasks.registry,
        // 绑定同一 operation。
        operation_id,
        // 线程令牌尚未观察到取消，验证持久事实仍优先。
        false,
    )
    // 竞争必须可靠收敛。
    .unwrap_or_else(|error| panic!("predispatch transition failed: {error:?}"));
    // 禁止越过 dispatch 点。
    assert!(matches!(effect, DispatchEffect::CancelledBeforeDispatch));
    // 查询持久终态。
    let failed = fixture
        // 使用被测 status 入口。
        .tasks
        // 读取同一 operation。
        .status(operation_id, fixture.now_ms())
        // 终态必须可查询。
        .unwrap_or_else(|error| panic!("predispatch status failed: {error:?}"));
    // dispatch 前取消收敛为 failed。
    assert_eq!(failed.status(), LongOperationStatus::Failed);
    // 未越过 dispatch 点。
    assert!(!failed.dispatch_started());
    // 允许调用方安全重提原操作。
    assert!(failed.retry_safe());
}

// 验证 worker panic 被隔离并持久收敛为稳定失败。
#[test]
fn worker_panic_becomes_terminal_failure() {
    // 创建空任务代际。
    let mut fixture = RecordingTasksFixture::new("panic");
    // 固定 canonical operation handle。
    let operation_id = "s2:o:0000000000000103";
    // 提交一个受控 panic worker。
    fixture
        // 使用被测 Module。
        .tasks
        // 注入不触碰外部系统的 panic。
        .submit_with(
            operation_id,
            fixture.now_ms(),
            move |_| -> AppResult<Value> {
                // 模拟隔离 worker wrapper 意外 panic。
                panic!("fixture worker panic")
            },
        )
        // 业务接受必须成功。
        .unwrap_or_else(|error| panic!("panic recording task submit failed: {error:?}"));
    // 在不发送 broker 取消的情况下等待 panic 收敛。
    let failed = fixture.wait_for_terminal(operation_id);
    // panic 不得穿透 broker 或留下 running。
    assert_eq!(failed.status(), LongOperationStatus::Failed);
    // panic 发生在 dispatch 后，禁止安全重提。
    assert!(!failed.retry_safe());
    // 只公开固定 worker 不可用错误码。
    assert_eq!(
        // 读取稳定失败码。
        failed.error().map(LongOperationFailure::code),
        // 核对固定契约值。
        Some(WORKER_UNAVAILABLE_CODE)
    );
}
