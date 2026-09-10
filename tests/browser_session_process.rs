#![cfg(target_os = "windows")]

//! 验证 browser-session parent Job 客户端的真实固定进程聚合。

// 导入进程与 JSON 测试工具。
use std::{
    // 导入固定命令。
    process::Command,
    // 导入测试目录序列。
    sync::{
        // 导入生产 fixture 串行锁。
        Mutex,
        // 导入测试目录序列。
        atomic::{AtomicU64, Ordering},
    },
    // 导入清理完成轮询。
    thread,
    // 导入有界等待时间。
    time::{Duration, Instant},
};

// 导入 JSON 值。
use serde_json::Value;

// 固定客户端 fixture 路径。
const CLIENT: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-client-fixture");
// 强制 Cargo 构建固定 worker fixture。
const _WORKER_FIXTURE: &str =
    env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-worker-fixture");
// 强制 Cargo 构建生产 browser-session worker。
const _PRODUCTION_WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-worker");
// 固定 session runtime fixture 路径。
const RUNTIME: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-runtime-fixture");
// 保存生产链测试目录序列。
static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);
// 串行化真实生产链，避免 Windows 进程与 profile 清理互相争用。
static PRODUCTION_LOCK: Mutex<()> = Mutex::new(());

// 运行一个固定客户端 fixture 模式。
fn run(mode: &str) -> Value {
    // 启动固定客户端测试驱动。
    let output = Command::new(CLIENT)
        // 只传入封闭模式名称。
        .arg(mode)
        // 等待进程完成并收集输出。
        .output()
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("client fixture failed: {error}"));
    // 客户端驱动必须成功执行。
    assert!(
        output.status.success(),
        "client fixture stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // 严格解析单行 JSON。
    serde_json::from_slice(&output.stdout)
        // 协议污染时给出明确诊断。
        .unwrap_or_else(|error| panic!("client fixture JSON failed: {error}"))
}

// 运行真实生产 worker 与 runtime fixture 链。
fn run_production(mode: &str) -> Value {
    // 独占真实生产链 fixture 生命周期。
    let _guard = PRODUCTION_LOCK
        // 获取串行锁。
        .lock()
        // 前序 panic 不应永久阻塞后续清理验证。
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // 生成唯一临时根。
    let temporary_root = std::env::temp_dir().join(format!(
        // 使用固定测试前缀。
        "act-browser-session-process-{}-{}",
        // 注入测试进程 ID。
        std::process::id(),
        // 注入唯一序列。
        TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    // 清理理论陈旧目录。
    let _ = std::fs::remove_dir_all(&temporary_root);
    // 创建可写临时根。
    std::fs::create_dir(&temporary_root)
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("temporary root failed: {error}"));
    // 启动固定客户端驱动。
    let output = Command::new(CLIENT)
        // 选择生产链模式。
        .arg(mode)
        // 让生产 worker 只发现固定 runtime fixture。
        .env("AI_COMPUTER_TOOLKIT_BROWSER_PATH", RUNTIME)
        // 隔离工具 profile 根。
        .env("TEMP", &temporary_root)
        // 同步 Windows 另一临时变量。
        .env("TMP", &temporary_root)
        // 等待完整链关闭。
        .output()
        // 测试环境失败时给出明确诊断。
        .unwrap_or_else(|error| panic!("production client fixture failed: {error}"));
    // 生产链必须成功执行。
    assert!(
        output.status.success(),
        "production client stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // 保存 profile 根。
    let profile_root = temporary_root.join("ai-computer-toolkit-browser");
    // 为 Windows 目录句柄释放提供有界观察窗口。
    let cleanup_started = Instant::now();
    // 等待精确 profile 根消失。
    while profile_root.exists() && cleanup_started.elapsed() < Duration::from_secs(10) {
        // 使用短休眠避免忙轮询。
        thread::sleep(Duration::from_millis(10));
    }
    // profile 根必须已经清理。
    assert!(!profile_root.exists());
    // 清理精确测试临时根。
    std::fs::remove_dir_all(&temporary_root)
        // 清理失败暴露测试污染。
        .unwrap_or_else(|error| panic!("temporary cleanup failed: {error}"));
    // 严格解析测试投影。
    serde_json::from_slice(&output.stdout)
        // 协议污染时给出明确诊断。
        .unwrap_or_else(|error| panic!("production client JSON failed: {error}"))
}

// 读取结果节点。
fn result(value: &Value) -> &Value {
    // 测试驱动必须报告成功。
    assert_eq!(value.get("ok").and_then(Value::as_bool), Some(true));
    // 返回必需 result。
    value
        // 读取结果字段。
        .get("result")
        // 缺失结果时终止测试。
        .unwrap_or_else(|| panic!("client result missing"))
}

// 验证完整生产 Job 链可以 ready 并优雅关闭。
#[test]
// 覆盖生产 worker、runtime fixture、回环探测和 profile 清理。
fn production_worker_opens_and_closes_under_parent_job() {
    // 运行生产链。
    let output = run_production("production-ready");
    // 取得结果。
    let result = result(&output);
    // 必须 ready。
    assert_eq!(result.get("outcome").and_then(Value::as_str), Some("ready"));
    // opaque ID 形状必须有效。
    assert_eq!(
        result.get("sessionIdValid").and_then(Value::as_bool),
        Some(true)
    );
    // worker 必须响应显式 close。
    assert_eq!(
        result.get("gracefulClose").and_then(Value::as_bool),
        Some(true)
    );
}

// 验证 ready 后 parent 保留页面帧通道并完成导航聚合。
#[test]
// 覆盖生产 worker、runtime fixture 与 live 页面命令生命周期。
fn production_parent_executes_page_navigation_and_keeps_session_live() {
    // 运行生产页面导航链。
    let output = run_production("production-page-navigate");
    // 取得结果。
    let result = result(&output);
    // 页面导航必须确定完成。
    assert_eq!(
        result.get("outcome").and_then(Value::as_str),
        Some("completed")
    );
    // 导航必须推进到第一代。
    assert_eq!(
        result.get("navigationGeneration").and_then(Value::as_u64),
        Some(1)
    );
    // 只接受 canonical 私有页面引用。
    assert_eq!(
        result.get("pageRefValid").and_then(Value::as_bool),
        Some(true)
    );
    // 确定完成不得强制回收。
    assert_eq!(
        result.get("forcedReap").and_then(Value::as_bool),
        Some(false)
    );
    // 页面命令完成后会话仍必须优雅关闭。
    assert_eq!(
        result.get("gracefulClose").and_then(Value::as_bool),
        Some(true)
    );
}

// 验证 Browser Session Module 隔离 worker 私有引用并单调换代页面身份。
#[test]
// 覆盖 Module registry、两次真实导航、stale page 与显式 close。
fn production_module_owns_public_page_identity_and_navigation_generation() {
    // 运行真实 Module 导航链。
    let output = run_production("production-module-navigation");
    // 序列化结果不得包含 worker 私有引用前缀。
    assert!(!output.to_string().contains("w1:"));
    // 取得结果。
    let result = result(&output);
    // 取得独立动作证据对象。
    let actions = result
        // 读取 actions 字段。
        .get("actions")
        // 缺失时提供明确测试诊断。
        .unwrap_or_else(|| panic!("Module action evidence is missing"));
    // 打开必须 ready 且可信完成。
    assert_eq!(
        result.get("openOutcome").and_then(Value::as_str),
        Some("ready")
    );
    // 打开完成事实必须成立。
    assert_eq!(
        result.get("openCompleted").and_then(Value::as_bool),
        Some(true)
    );
    // 公开 session ID 形状必须 canonical。
    assert_eq!(
        result.get("sessionIdValid").and_then(Value::as_bool),
        Some(true)
    );
    // 两次导航都必须确定完成。
    assert_eq!(
        result.get("firstOutcome").and_then(Value::as_str),
        Some("completed")
    );
    // 第二次导航同样确定完成。
    assert_eq!(
        result.get("secondOutcome").and_then(Value::as_str),
        Some("completed")
    );
    // 两次公开页面身份都必须 canonical。
    assert_eq!(
        result.get("firstPageIdValid").and_then(Value::as_bool),
        Some(true)
    );
    // 第二代公开页面身份必须 canonical。
    assert_eq!(
        result.get("secondPageIdValid").and_then(Value::as_bool),
        Some(true)
    );
    // 每次导航必须换发新身份。
    assert_eq!(
        result.get("pageIdsDistinct").and_then(Value::as_bool),
        Some(true)
    );
    // 第一代必须为一。
    assert_eq!(
        result.get("firstGeneration").and_then(Value::as_u64),
        Some(1)
    );
    // 第一代报告必须与 registry 一致。
    assert_eq!(
        result
            .get("firstReportedGeneration")
            .and_then(Value::as_u64),
        Some(1)
    );
    // 第二代必须为二。
    assert_eq!(
        result.get("secondGeneration").and_then(Value::as_u64),
        Some(2)
    );
    // 第二代报告必须与 registry 一致。
    assert_eq!(
        result
            .get("secondReportedGeneration")
            .and_then(Value::as_u64),
        Some(2)
    );
    // 旧页面必须结构化 stale。
    assert_eq!(
        result.get("stalePageCode").and_then(Value::as_str),
        Some("STALE_PAGE")
    );
    // worker 必须响应显式 close。
    assert_eq!(
        result.get("gracefulClose").and_then(Value::as_bool),
        Some(true)
    );
    // 关闭后 session 必须立即失效。
    assert_eq!(
        result.get("closedSessionCode").and_then(Value::as_str),
        Some("STALE_SESSION")
    );
    // 三类 wait 都必须满足。
    assert_eq!(
        actions.get("waitConditionMet").and_then(Value::as_bool),
        Some(true)
    );
    // selector wait 必须满足。
    assert_eq!(
        actions.get("elementWaitMet").and_then(Value::as_bool),
        Some(true)
    );
    // text wait 必须满足。
    assert_eq!(
        actions.get("textWaitMet").and_then(Value::as_bool),
        Some(true)
    );
    // query 必须确定完成且签发公开元素身份。
    assert_eq!(
        actions.get("queryOutcome").and_then(Value::as_str),
        Some("completed")
    );
    // 公开元素身份必须 canonical。
    assert_eq!(
        actions.get("elementIdValid").and_then(Value::as_bool),
        Some(true)
    );
    // 元素摘要不得含 native facts。
    assert_eq!(
        actions.get("elementRole").and_then(Value::as_str),
        Some("button")
    );
    // 固定元素必须可用。
    assert_eq!(
        actions.get("elementEnabled").and_then(Value::as_bool),
        Some(true)
    );
    // 固定 fixture 必须形成多命中且不截断。
    assert_eq!(
        actions.get("queryMatchCount").and_then(Value::as_u64),
        Some(2)
    );
    // 固定多命中未超过结果上限。
    assert_eq!(
        actions.get("queryTruncated").and_then(Value::as_bool),
        Some(false)
    );
    // 相同私有元素必须复用同一公开 identity。
    assert_eq!(
        actions
            .get("elementIdentityStable")
            .and_then(Value::as_bool),
        Some(true)
    );
    // 缺失 role 必须确定返回零命中。
    assert_eq!(
        actions.get("emptyQueryIsEmpty").and_then(Value::as_bool),
        Some(true)
    );
    // 零命中总数必须为零。
    assert_eq!(
        actions.get("emptyQueryCount").and_then(Value::as_u64),
        Some(0)
    );
    // 未确认必须先于 stale 身份解析。
    assert_eq!(
        actions.get("confirmationCode").and_then(Value::as_str),
        Some("CONFIRMATION_REQUIRED")
    );
    // 当前页面未知元素必须结构化 stale。
    assert_eq!(
        actions.get("staleElementCode").and_then(Value::as_str),
        Some("STALE_ELEMENT")
    );
    // 确认式点击必须完成。
    assert_eq!(actions.get("clicked").and_then(Value::as_bool), Some(true));
    // 确认式输入必须完成并报告 UTF-8 字节数。
    assert_eq!(actions.get("typed").and_then(Value::as_bool), Some(true));
    // 固定文本为十二字节。
    assert_eq!(actions.get("typedBytes").and_then(Value::as_u64), Some(12));
    // 截图必须只投影固定 PNG MIME。
    assert_eq!(
        actions.get("screenshotMime").and_then(Value::as_str),
        Some("image/png")
    );
    // 截图 Base64 与摘要形状必须有效。
    assert_eq!(
        actions
            .get("screenshotBase64Present")
            .and_then(Value::as_bool),
        Some(true)
    );
    // 固定一像素截图尺寸必须准确。
    assert_eq!(
        actions.get("screenshotWidth").and_then(Value::as_u64),
        Some(1)
    );
    // 高度同样固定一像素。
    assert_eq!(
        actions.get("screenshotHeight").and_then(Value::as_u64),
        Some(1)
    );
    // 截图摘要必须保持固定十六进制形状。
    assert_eq!(
        actions
            .get("screenshotDigestValid")
            .and_then(Value::as_bool),
        Some(true)
    );
}

// 验证固定 ready fixture 保持 live 所有权直到 close。
#[test]
// 覆盖 parent 端显式 close。
fn ready_fixture_is_closed_gracefully() {
    // 运行 ready fixture。
    let output = run("ready");
    // 取得结果。
    let result = result(&output);
    // 核对 ready。
    assert_eq!(result.get("outcome").and_then(Value::as_str), Some("ready"));
    // 打开阶段不得强制回收。
    assert_eq!(
        result.get("forcedReap").and_then(Value::as_bool),
        Some(false)
    );
    // close 必须优雅完成。
    assert_eq!(
        result.get("gracefulClose").and_then(Value::as_bool),
        Some(true)
    );
}

// 验证 accepted-only 异常退出保守聚合未知。
#[test]
// 覆盖异常退出和 OutcomeUnknown。
fn accepted_only_exit_becomes_outcome_unknown() {
    // 运行 accepted-only fixture。
    let output = run("accepted-only");
    // 取得结果。
    let result = result(&output);
    // 必须 unknown。
    assert_eq!(
        result.get("outcome").and_then(Value::as_str),
        Some("unknown")
    );
    // 必须未完成。
    assert_eq!(
        result.get("completed").and_then(Value::as_bool),
        Some(false)
    );
    // 必须使用固定错误码。
    assert_eq!(
        result.get("errorCode").and_then(Value::as_str),
        Some("OUTCOME_UNKNOWN")
    );
    // parent 必须回收 Job。
    assert_eq!(
        result.get("forcedReap").and_then(Value::as_bool),
        Some(true)
    );
}

// 验证零帧退出确定未派发且可重试。
#[test]
// 覆盖 dispatch 前异常退出。
fn zero_frame_exit_is_not_dispatched() {
    // 运行零帧 fixture。
    let output = run("zero-frame");
    // 取得结果。
    let result = result(&output);
    // 必须未派发。
    assert_eq!(
        result.get("outcome").and_then(Value::as_str),
        Some("not-dispatched")
    );
    // 可安全重试。
    assert_eq!(result.get("retrySafe").and_then(Value::as_bool), Some(true));
    // 明确没有接受可能。
    assert_eq!(
        result
            // 读取接受事实。
            .get("acceptedMayHaveOccurred")
            // 转换为布尔。
            .and_then(Value::as_bool),
        // 必须为假。
        Some(false)
    );
}

// 验证 deadline 与取消都在 accepted 后保守未知并整树回收。
#[test]
// 覆盖两种停止竞争来源的统一聚合。
fn cancellation_and_deadline_force_reap_accepted_worker() {
    // 逐一运行 deadline 与取消。
    for mode in ["deadline", "cancel", "race"] {
        // 运行当前固定模式。
        let output = run(mode);
        // 取得结果。
        let result = result(&output);
        // accepted 后无 final 必须 unknown。
        assert_eq!(
            result.get("outcome").and_then(Value::as_str),
            Some("unknown")
        );
        // parent 必须强制回收。
        assert_eq!(
            result.get("forcedReap").and_then(Value::as_bool),
            Some(true)
        );
    }
}
