//! 验证 sequence step Job runner 的真实进程回收与部分观察。

// 导入路径、原子状态与时间工具。
use std::{
    // 定位 Cargo 构建的 Rust fixture。
    path::PathBuf,
    // 构造跨轮询取消状态。
    sync::{
        // 串行化真实进程 fixture，避免调试构建启动竞争吞掉短 deadline。
        Mutex,
        // 延迟创建测试互斥量。
        OnceLock,
        // 构造跨轮询取消状态。
        atomic::{AtomicBool, Ordering},
    },
    // 构造 deadline 与取消时钟。
    time::{Duration, Instant},
};

// 导入 JSON 构造器。
use serde_json::json;

// 导入协议类型和被测 runner 私有项。
use crate::components::sequence_step_protocol::{
    // 导入 cancel control。
    SequenceStepWorkerControl,
    // 导入严格请求。
    SequenceStepWorkerRequest,
    // 导入 final outcome。
    frames::SequenceStepWorkerOutcome,
};

// 导入父模块所有私有被测项。
use super::*;

// 固定测试 nonce。
const NONCE: &str = "0123456789abcdef0123456789abcdef";

// 保存进程 fixture 的进程内串行门禁。
static FIXTURE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

// 定位 cargo test --all-targets 构建的固定 Rust fixture。
fn fixture_path() -> PathBuf {
    // 取得当前 lib test executable。
    let current = std::env::current_exe()
        // 测试环境必须能解析自身路径。
        .unwrap_or_else(|error| panic!("test executable path failed: {error}"));
    // 从 target/debug/deps 回到 target/debug。
    let debug = current
        // 取得 deps 目录。
        .parent()
        // 缺失父目录表示 Cargo 布局漂移。
        .and_then(|deps| deps.parent())
        // 返回拥有型路径。
        .map(PathBuf::from)
        // 中止不完整布局。
        .unwrap_or_else(|| panic!("test executable has no target debug directory"));
    // 拼接固定 Rust fixture 文件名。
    let fixture = debug.join("ai-computer-toolkit-sequence-step-fixture.exe");
    // 测试要求 all-targets 先构建 fixture。
    assert!(
        // 核对精确文件存在。
        fixture.is_file(),
        // 提供稳定构建提示。
        "sequence step fixture was not built; run cargo test --all-targets"
    );
    // 返回精确 fixture 路径。
    fixture
}

// 构造指定 timeout 的严格 worker 请求。
fn request(timeout_ms: u32) -> SequenceStepWorkerRequest {
    // 构造 provider-neutral 状态命令。
    let value = json!({
        // 固定协议版本。
        "contractVersion": "act/sequence-step-worker/v1",
        // 固定 canonical nonce。
        "requestNonce": NONCE,
        // 使用测试 deadline。
        "timeoutMs": timeout_ms,
        // 使用最小状态请求。
        "command": {
            // 只读状态动词。
            "verb": "status",
            // 稳定 desktop 路由。
            "app": "desktop",
            // 无 operation。
            "operation": null,
            // 无目标。
            "target": {},
            // 无参数。
            "args": {},
            // 默认数量边界。
            "maxItems": 50,
            // 默认深度边界。
            "maxDepth": 4,
            // 无确认。
            "confirmed": false,
            // 无前台同意。
            "foregroundConsent": false,
            // 标准隔离要求。
            "isolationRequirement": "standard"
        }
    });
    // 严格解析 fixture 请求。
    SequenceStepWorkerRequest::parse(&value.to_string())
        // fixture 漂移中止测试。
        .unwrap_or_else(|failure| panic!("runner request failed: {:?}", failure.code()))
}

// 使用固定 Rust fixture 执行私有 runner。
fn run_fixture(
    // 接收封闭 fixture 模式。
    mode: &str,
    // 接收硬 deadline。
    timeout_ms: u32,
    // 接收取消观察器。
    cancelled: impl Fn(bool) -> bool,
) -> crate::domain::AppResult<SequenceStepRunnerOutput> {
    // 串行化真实子进程，避免测试线程竞争影响硬 deadline 语义。
    let _fixture_guard = FIXTURE_LOCK
        // 延迟创建无状态互斥量。
        .get_or_init(|| Mutex::new(()))
        // 中毒表示更早 fixture 已 panic，当前测试必须停止。
        .lock()
        // 不使用 unwrap 以保持项目门禁。
        .unwrap_or_else(|error| panic!("sequence fixture lock was poisoned: {error}"));
    // 构造严格请求。
    let request = request(timeout_ms);
    // 序列化请求行。
    let request_line = request
        // 使用协议 Component。
        .to_line()
        // fixture 漂移中止测试。
        .unwrap_or_else(|failure| panic!("request line failed: {:?}", failure.code()));
    // 构造唯一 cancel 行。
    let cancel_line = SequenceStepWorkerControl::cancel(NONCE)
        // nonce 固定合法。
        .and_then(|control| control.to_line())
        // fixture 漂移中止测试。
        .unwrap_or_else(|failure| panic!("cancel line failed: {:?}", failure.code()));
    // 从当前时刻开始 fixture deadline。
    let started = Instant::now();
    // 调用仅本模块可见的固定路径实现。
    run_fixed(
        // 使用 Cargo 构建的 Rust fixture。
        &fixture_path(),
        // 只传递封闭 fixture 模式。
        &[mode.to_owned()],
        // 聚合 fixture 协议输入和时间预算。
        SequenceStepRunPlan {
            // 转移严格请求行。
            request_line,
            // 转移 cancel 行。
            cancel_line,
            // 保存固定 nonce。
            request_nonce: NONCE.to_owned(),
            // 保存总生命周期起点。
            started,
            // 保存硬 deadline。
            timeout: Duration::from_millis(u64::from(timeout_ms)),
        },
        // 传入取消观察器。
        cancelled,
    )
}

// 验证取消在 deadline 预留窗口中仍保持优先。
#[test]
fn cancellation_precedes_deadline_trigger() {
    // 使用一秒硬 deadline。
    let timeout = Duration::from_secs(1);
    // 未进入预留窗口时继续执行。
    assert_eq!(
        // 观察普通中段。
        stop_trigger(false, Duration::from_millis(500), timeout),
        // 不停止。
        StopTrigger::None,
    );
    // 进入最后五十毫秒触发 deadline。
    assert_eq!(
        // 观察预留窗口边界。
        stop_trigger(false, Duration::from_millis(950), timeout),
        // 触发 deadline。
        StopTrigger::Deadline,
    );
    // 同时取消时必须选择取消。
    assert_eq!(
        // 同一时刻观察取消和 deadline。
        stop_trigger(true, Duration::from_millis(950), timeout),
        // 取消优先。
        StopTrigger::Cancelled,
    );
}

// 验证完整 accepted 首帧能实时建立观察。
#[test]
fn complete_first_frame_publishes_dispatch_accepted() {
    // 构造 accepted 帧。
    let frame = crate::components::sequence_step_protocol::frames::dispatch_accepted_frame(NONCE)
        // fixture 漂移中止测试。
        .unwrap_or_else(|failure| panic!("accepted fixture failed: {:?}", failure.code()));
    // 初始未 accepted。
    let accepted = AtomicBool::new(false);
    // 观察完整首帧。
    observe_first_frame(&frame, NONCE, &accepted)
        // 合法帧不得失败。
        .unwrap_or_else(|error| panic!("accepted observation failed: {error}"));
    // accepted 必须对 wait 线程可见。
    assert!(accepted.load(Ordering::Acquire));
}

// 验证不完整首帧不会提前建立 accepted。
#[test]
fn partial_first_frame_does_not_publish_dispatch_accepted() {
    // 构造完整 accepted 帧。
    let frame = crate::components::sequence_step_protocol::frames::dispatch_accepted_frame(NONCE)
        // fixture 漂移中止测试。
        .unwrap_or_else(|failure| panic!("accepted fixture failed: {:?}", failure.code()));
    // 初始未 accepted。
    let accepted = AtomicBool::new(false);
    // 去除换行形成不完整帧。
    observe_first_frame(&frame[..frame.len().saturating_sub(1)], NONCE, &accepted)
        // 不完整帧应继续等待而非失败。
        .unwrap_or_else(|error| panic!("partial observation failed: {error}"));
    // 不得提前发布 accepted。
    assert!(!accepted.load(Ordering::Acquire));
}

// 验证正常 worker 退出要求完整 final。
#[test]
fn completed_fixture_returns_full_observation() {
    // 运行确定成功 fixture。
    let output = run_fixture("completed", 5_000, |_| false)
        // runner 不应失败。
        .unwrap_or_else(|error| panic!("completed runner failed: {error}"));
    // 无外部停止。
    assert_eq!(output.stop(), SequenceStepRunnerStop::Completed);
    // 不需要强制回收。
    assert!(!output.forced_reap());
    // fixture 使用零退出码。
    assert_eq!(output.exit_code(), Some(0));
    // accepted 必须存在。
    assert!(output.observation().dispatch_accepted());
    // final 必须存在。
    let final_observation = output
        // 借用帧观察。
        .observation()
        // 借用 final。
        .final_observation()
        // 缺失 final 中止测试。
        .unwrap_or_else(|| panic!("completed fixture omitted final"));
    // final 必须确定成功。
    assert_eq!(
        final_observation.outcome(),
        SequenceStepWorkerOutcome::Completed
    );
}

// 验证 dispatch 前挂起在 deadline 后只保留零帧。
#[test]
fn deadline_before_dispatch_returns_zero_frame_observation() {
    // 运行不输出帧的挂起 fixture。
    let output = run_fixture("before-dispatch-hang", 1_000, |_| false)
        // runner 不应丢失部分观察。
        .unwrap_or_else(|error| panic!("before-dispatch runner failed: {error}"));
    // 停止原因必须是 deadline。
    assert_eq!(output.stop(), SequenceStepRunnerStop::Deadline);
    // Job 必须被强制回收。
    assert!(output.forced_reap());
    // 没有 accepted。
    assert!(!output.observation().dispatch_accepted());
    // 零帧没有 final。
    assert!(output.observation().final_observation().is_none());
}

// 验证 accepted 后 deadline 保留 accepted-only 而不伪造 final。
#[test]
fn deadline_after_dispatch_returns_accepted_only_observation() {
    // 运行 accepted 后挂起 fixture。
    let output = run_fixture("after-accepted-hang", 1_500, |_| false)
        // runner 不应丢失部分观察。
        .unwrap_or_else(|error| panic!("after-accepted runner failed: {error}"));
    // 停止原因必须是 deadline。
    assert_eq!(output.stop(), SequenceStepRunnerStop::Deadline);
    // Job 必须被强制回收。
    assert!(output.forced_reap());
    // accepted 必须实时保存。
    assert!(output.observation().dispatch_accepted());
    // 不得伪造 final。
    assert!(output.observation().final_observation().is_none());
}

// 验证调用方取消优先并在固定协作窗口后回收 Job。
#[test]
fn cancellation_after_dispatch_precedes_deadline() {
    // 只在 reader 已实时观察 accepted 后触发取消。
    let output = run_fixture("after-accepted-hang", 5_000, |accepted| accepted)
        // runner 不应丢失部分观察。
        .unwrap_or_else(|error| panic!("cancelled runner failed: {error}"));
    // 取消必须优先于远端 deadline。
    assert_eq!(output.stop(), SequenceStepRunnerStop::Cancelled);
    // 协作窗口后必须强制回收。
    assert!(output.forced_reap());
    // accepted 事实必须保留。
    assert!(output.observation().dispatch_accepted());
}

// 验证无界 stdout 触发结构化资源错误并回收 Job。
#[test]
fn oversized_output_fails_closed() {
    // 运行无界输出 fixture。
    let error = match run_fixture("oversized", 30_000, |_| false) {
        // 捕获预期资源错误。
        Err(error) => error,
        // 成功表示输出上限失效。
        Ok(_) => panic!("oversized fixture unexpectedly succeeded"),
    };
    // 必须返回固定输出上限错误。
    assert_eq!(error.code, "WORKER_OUTPUT_TOO_LARGE");
}
