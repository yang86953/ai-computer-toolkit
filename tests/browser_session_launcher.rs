#![cfg(target_os = "windows")]

//! 验证生产 PowerShell launcher 的公开浏览器会话生命周期与纯 Rust 发布边界。

// 导入文件、进程、路径与有界等待工具。
use std::{
    // 导入隔离安装与请求文件操作。
    fs,
    // 导入隔离布局路径类型。
    path::{Path, PathBuf},
    // 导入生产 launcher 子进程输出。
    process::{Command, Output, Stdio},
    // 导入同一固定 broker endpoint 的进程内串行锁。
    sync::Mutex,
    // 导入进程与文件回收轮询。
    thread,
    // 导入唯一目录时间戳和总预算。
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// 导入 provider-neutral JSON 值与构造器。
use serde_json::{Value, json};
// 从窄测试辅助模块导入精确进程见证。
#[path = "support/browser_session_launcher_process.rs"]
mod process_support;
// 导入不公开 PID 或路径的测试能力。
use process_support::{matching_processes, wait_for_no_process, wait_for_process};
// 将页面公开 launcher 成功纵切拆到窄测试模块，保持本文件规模上限。
#[path = "support/browser_session_launcher_page.rs"]
mod page;
// 将页面公开 launcher 的业务前错误矩阵拆到窄测试模块。
#[path = "support/browser_session_launcher_page_errors.rs"]
mod page_errors;
// 将页面 accepted 后故障与恢复矩阵拆到窄测试模块。
#[path = "support/browser_session_launcher_page_faults.rs"]
mod page_faults;
// 将页面动作公开 launcher 成功纵切拆到窄测试模块。
#[path = "support/browser_session_launcher_page_actions.rs"]
mod page_actions;
// 将页面动作公开 launcher 的业务前错误矩阵拆到窄测试模块。
#[path = "support/browser_session_launcher_page_action_errors.rs"]
mod page_action_errors;
// 将页面动作 accepted 后故障与恢复矩阵拆到窄测试模块。
#[path = "support/browser_session_launcher_page_action_faults.rs"]
mod page_action_faults;

// 固定 Cargo 构建出的生产主程序。
const MAIN_SOURCE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit");
// 固定 Cargo 构建出的生产 Browser Session Broker。
const BROKER_SOURCE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-broker");
// 固定 Cargo 构建出的生产 Browser Session Worker。
const WORKER_SOURCE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-worker");
// 固定工具自有 Chromium 协议 runtime fixture。
const RUNTIME_SOURCE: &str =
    env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-runtime-fixture");
// 固定 accepted-only worker fixture 供公开未知结果投影验证。
const ACCEPTED_ONLY_WORKER_SOURCE: &str =
    env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-accepted-only-worker-fixture");
// 固定生产 launcher 的仓库相对路径。
const LAUNCHER_RELATIVE_PATH: &str = "tools/windows/Invoke-ComputerControl.ps1";
// 固定生产主程序 sibling 名称。
const MAIN_FILE_NAME: &str = "ai-computer-toolkit.exe";
// 固定生产 broker sibling 名称。
const BROKER_FILE_NAME: &str = "ai-computer-toolkit-browser-session-broker.exe";
// 固定生产 worker sibling 名称。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-browser-session-worker.exe";
// 固定测试 runtime sibling 名称。
const RUNTIME_FILE_NAME: &str = "ai-computer-toolkit-browser-session-runtime-fixture.exe";
// 固定工具 profile 根名称。
const PROFILE_ROOT_NAME: &str = "ai-computer-toolkit-browser";
// 固定单次生产 launcher 测试进程总预算。
const LAUNCHER_PROCESS_TIMEOUT: Duration = Duration::from_secs(45);
// 串行化会共享当前登录会话固定 endpoint 的两个生产 launcher 场景。
static TEST_LOCK: Mutex<()> = Mutex::new(());

// 为测试提取提供不使用 unwrap 的上下文。
trait Must<T> {
    // 成功时返回值，失败时终止当前测试。
    fn must(self, context: &str) -> T;
}

// 为 Result 提供测试上下文提取。
impl<T, E: std::fmt::Debug> Must<T> for Result<T, E> {
    // 保留底层调试信息但不输出用户数据。
    fn must(self, context: &str) -> T {
        // 失败时使用固定上下文。
        self.unwrap_or_else(|error| panic!("{context}: {error:?}"))
    }
}

// 为 Option 提供测试上下文提取。
impl<T> Must<T> for Option<T> {
    // 缺失值时使用固定上下文。
    fn must(self, context: &str) -> T {
        // 不猜测缺失值。
        self.unwrap_or_else(|| panic!("{context}"))
    }
}

// 保存当前测试独占的原样 launcher 安装布局。
struct LauncherLayout {
    // 保存隔离根目录。
    root: Option<PathBuf>,
    // 保存原样生产 launcher 路径。
    launcher: PathBuf,
    // 保存生产 broker 精确镜像路径。
    broker: PathBuf,
    // 保存生产 worker 精确镜像路径。
    worker: PathBuf,
    // 保存 runtime fixture 精确镜像路径。
    runtime: PathBuf,
    // 保存隔离临时目录。
    temporary: PathBuf,
}

// 为隔离 launcher 布局提供安装与清理。
impl LauncherLayout {
    // 复制原样 launcher 与固定 Rust siblings。
    fn install(worker_source: &Path) -> Self {
        // 生成测试唯一时间戳。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时间。
            .duration_since(UNIX_EPOCH)
            // 时钟异常不能安全创建唯一目录。
            .must("launcher fixture clock should be available")
            // 使用纳秒降低碰撞概率。
            .as_nanos();
        // 构造只属于当前测试进程的根目录。
        let root = std::env::temp_dir().join(format!(
            // 固定前缀便于异常诊断。
            "act-browser-session-launcher-{}-{stamp}",
            // 注入当前测试进程 ID。
            std::process::id(),
        ));
        // 创建原样 launcher 所需的 tools 目录。
        fs::create_dir_all(root.join("tools/windows"))
            // 创建失败时停止测试。
            .must("launcher tools directory should be created");
        // 创建原样 launcher 所需的 debug sibling 目录。
        let binary_directory = root.join("target").join("debug");
        // 创建固定二进制目录。
        fs::create_dir_all(&binary_directory)
            // 创建失败时停止测试。
            .must("launcher binary directory should be created");
        // 创建隔离 TEMP/TMP 根。
        let temporary = root.join("temporary");
        // 创建精确临时目录。
        fs::create_dir(&temporary).must("launcher temporary directory should be created");
        // 定位仓库中的原样生产 launcher。
        let launcher_source = Path::new(env!("CARGO_MANIFEST_DIR")).join(LAUNCHER_RELATIVE_PATH);
        // 定位隔离布局中的生产 launcher。
        let launcher = root.join(LAUNCHER_RELATIVE_PATH);
        // 原样复制生产 launcher。
        fs::copy(&launcher_source, &launcher).must("production launcher should be copied verbatim");
        // 定位固定生产主程序 sibling。
        let main = binary_directory.join(MAIN_FILE_NAME);
        // 复制当前 Cargo 构建的生产主程序。
        fs::copy(MAIN_SOURCE, &main).must("production main sibling should be copied");
        // 定位固定 broker sibling。
        let broker = binary_directory.join(BROKER_FILE_NAME);
        // 复制当前 Cargo 构建的生产 broker。
        fs::copy(BROKER_SOURCE, &broker).must("production broker sibling should be copied");
        // 定位固定 worker sibling。
        let worker = binary_directory.join(WORKER_FILE_NAME);
        // 复制当前场景选择的固定 worker。
        fs::copy(worker_source, &worker).must("browser session worker sibling should be copied");
        // 定位工具自有 runtime fixture sibling。
        let runtime = binary_directory.join(RUNTIME_FILE_NAME);
        // 复制当前 Cargo 构建的 runtime fixture。
        fs::copy(RUNTIME_SOURCE, &runtime).must("browser runtime fixture should be copied");
        // 返回唯一布局所有者。
        Self {
            // 保存可转移的根目录所有权。
            root: Some(root),
            // 保存 launcher 路径。
            launcher,
            // 保存 broker 路径。
            broker,
            // 保存 worker 路径。
            worker,
            // 保存 runtime 路径。
            runtime,
            // 保存临时根。
            temporary,
        }
    }

    // 返回工具 profile 根。
    fn profile_root(&self) -> PathBuf {
        // profile 只允许位于隔离 TEMP 中。
        self.temporary.join(PROFILE_ROOT_NAME)
    }

    // 在所有 owned 进程退出后删除完整布局。
    fn finish(mut self) {
        // 取得尚未清理的唯一根。
        let root = self.root.take().must("launcher layout should finish once");
        // 删除仅由本测试创建的精确目录。
        fs::remove_dir_all(root).must("launcher layout should be fully removable");
    }
}

// 在断言展开路径尽力回收测试目录。
impl Drop for LauncherLayout {
    // 只删除本实例创建的精确根。
    fn drop(&mut self) {
        // 根仍存在时执行尽力清理。
        if let Some(root) = self.root.take() {
            // 仅终止精确隔离镜像路径对应的 broker。
            for broker in matching_processes(&self.broker, true) {
                // 失败路径也必须收敛本测试拥有的固定进程。
                broker.terminate_owned();
            }
            // 不覆盖原始断言失败。
            let _ = fs::remove_dir_all(root);
        }
    }
}

// 经原样生产 PowerShell launcher 执行固定参数。
fn run_launcher(layout: &LauncherLayout, arguments: &[&str]) -> Output {
    // 启动 Windows 原生 PowerShell 7。
    let mut command = Command::new("pwsh.exe");
    // 固定不加载用户 profile。
    command.args(["-NoProfile", "-File"]);
    // 传递原样 launcher 路径。
    command.arg(&layout.launcher);
    // 传递公开 CLI 参数。
    command.args(arguments);
    // 让生产 worker 只发现工具自有 runtime fixture。
    command.env("AI_COMPUTER_TOOLKIT_BROWSER_PATH", &layout.runtime);
    // 隔离当前场景的 TEMP。
    command.env("TEMP", &layout.temporary);
    // 同步隔离 TMP。
    command.env("TMP", &layout.temporary);
    // 捕获唯一公开 stdout。
    command.stdout(Stdio::piped());
    // 捕获失败诊断但不投影到公开断言。
    command.stderr(Stdio::piped());
    // 启动可由测试有界回收的 launcher。
    let mut child = command
        // 不接受 shell 或调用方进程参数扩张。
        .spawn()
        // 启动失败时提供固定上下文。
        .must("production PowerShell launcher should start");
    // 建立不会因轮询重置的 launcher 总 deadline。
    let deadline = Instant::now() + LAUNCHER_PROCESS_TIMEOUT;
    // 在固定预算内等待唯一 launcher 退出。
    loop {
        // 只观察当前 owned 子进程。
        match child
            // 非阻塞读取退出事实。
            .try_wait()
            // 等待失败时提供固定上下文。
            .must("production PowerShell launcher should remain waitable")
        {
            // 已退出时收集完整有界输出。
            Some(_) => {
                // 返回公开进程结果。
                return child
                    // 收集 stdout 与 stderr 管道。
                    .wait_with_output()
                    // 输出收集失败不能伪造 launcher 结果。
                    .must("production PowerShell launcher output should be available");
            }
            // 预算内继续短轮询。
            None if Instant::now() < deadline => {
                // 限制进程等待轮询 CPU。
                thread::sleep(Duration::from_millis(10));
            }
            // 超出冻结预算时回收 owned launcher。
            None => {
                // 终止仅由当前测试启动的精确子进程。
                child
                    // 请求 Windows 终止当前 owned 进程。
                    .kill()
                    // 终止失败表示测试无法证明收敛。
                    .must("timed out production PowerShell launcher should terminate");
                // 等待终止完成并关闭捕获管道。
                let _ = child.wait_with_output();
                // 明确报告生产 launcher 超出冻结预算。
                panic!("production PowerShell launcher exceeded the fixed test deadline");
            }
        }
    }
}

// 解析 launcher 的唯一 JSON stdout。
fn output_json(output: &Output) -> Value {
    // stdout 必须只包含一个 JSON 文档。
    serde_json::from_slice(&output.stdout).must("launcher stdout should be JSON")
}

// 写入本次公开生命周期调用的 structured input。
fn lifecycle_input(
    // 接收隔离布局。
    layout: &LauncherLayout,
    // 接收公开 verb。
    verb: &str,
    // 接收公开 capability。
    capability: &str,
    // 接收公开 opaque target。
    target: &str,
    // 接收公开总预算。
    timeout_ms: u32,
) -> PathBuf {
    // 构造不会被生产代码解释为 native 输入的文件名。
    let path = layout.temporary.join(format!("{verb}-{timeout_ms}.json"));
    // 构造统一 App facade structured wrapper。
    let value = json!({
        // 只传唯一公开目标字段。
        "target": { "sessionId": target },
        // 只传 capability 与冻结 lifecycle input。
        "args": { "capability": capability, "input": { "timeoutMs": timeout_ms } }
    });
    // 序列化为 UTF-8 JSON。
    let bytes = serde_json::to_vec(&value).must("lifecycle input should serialize");
    // 写入测试独占文件。
    fs::write(&path, bytes).must("lifecycle input file should be written");
    // 返回只由 launcher 读取的精确路径。
    path
}

// 经生产 launcher 执行公开 open 或 close。
fn run_lifecycle(
    // 接收隔离布局。
    layout: &LauncherLayout,
    // 接收 generic verb。
    verb: &str,
    // 接收公开 capability。
    capability: &str,
    // 接收公开 target。
    target: &str,
    // 接收确认事实。
    confirmed: bool,
    // 接收总预算。
    timeout_ms: u32,
) -> Output {
    // 写入严格 structured input。
    let input = lifecycle_input(layout, verb, capability, target, timeout_ms);
    // 转换为 PowerShell argv 文本。
    let input = input.to_string_lossy().into_owned();
    // 按确认状态选择封闭参数集合。
    if confirmed {
        // 已确认命令显式携带 confirm。
        run_launcher(
            layout,
            &["run", "app", verb, "--input", &input, "--confirm"],
        )
    } else {
        // 未确认命令不携带任何确认旁路。
        run_launcher(layout, &["run", "app", verb, "--input", &input])
    }
}

// 从生产 sessions 结果取得唯一 host target。
fn discover_host(layout: &LauncherLayout) -> String {
    // 仅请求首个 host session，避免枚举无关应用数据。
    let output = run_launcher(layout, &["sessions", "app", "--max-items", "1"]);
    // 只读发现必须成功。
    assert!(output.status.success(), "host discovery should succeed");
    // 解析公开结果。
    let value = output_json(&output);
    // 读取封闭 sessions 数组。
    let sessions = value
        // 进入公开 data。
        .pointer("/data/sessions")
        // 只接受数组。
        .and_then(Value::as_array)
        // 缺失数组表示公开契约漂移。
        .must("host discovery should return sessions");
    // max-items=1 必须只返回 host。
    assert_eq!(sessions.len(), 1);
    // 读取唯一 host 对象。
    let host = &sessions[0];
    // session kind 必须为 host。
    assert_eq!(host.get("kind").and_then(Value::as_str), Some("host"));
    // host 必须发布 open capability。
    assert_eq!(
        host.pointer("/capabilities/0/id").and_then(Value::as_str),
        Some("browser.session.open@1")
    );
    // 提取唯一 canonical host identity。
    host.get("sessionId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 缺失 identity 表示发现契约漂移。
        .must("host discovery should return sessionId")
        // 返回给独立 launcher 使用。
        .to_owned()
}

// 递归拒绝公开输出中的私有实现文本。
fn assert_no_private_text(value: &Value) {
    // 按 JSON 类型递归检查。
    match value {
        // 字符串值不能泄漏实现事实。
        Value::String(text) => {
            // 统一大小写覆盖消息变体。
            let lower = text.to_ascii_lowercase();
            // 允许公开契约冻结的执行域枚举完整值。
            if lower == "isolated-worker" {
                // 精确枚举不代表私有 worker 身份或实现文本。
                return;
            }
            // 检查冻结禁止词集合。
            for forbidden in [
                // 禁止 correlation 与代际事实。
                "nonce",
                "epoch",
                "fingerprint",
                "revision",
                // 禁止 transport 与本机身份。
                "pipe",
                "endpoint",
                "pid",
                "sid",
                "integrity",
                // 禁止资源和浏览器私有事实。
                "worker",
                "job",
                "stdio",
                "profile",
                "websocket",
                "cdp",
                "w1:",
                // 禁止调用方页面输入与凭据类别。
                "http://",
                "https://",
                "cookie",
                "credential",
                "selector",
                // 禁止 native 页面实现类别。
                "native",
                "devtools",
                "backendnode",
                // 禁止 C++ 或前台回退声明。
                "c++",
                "foreground fallback",
                "user browser",
            ] {
                // 任一私有词出现均失败。
                assert!(
                    !lower.contains(forbidden),
                    "public output leaked forbidden private category: {forbidden}"
                );
            }
        }
        // 数组逐项检查。
        Value::Array(items) => {
            // 不放宽嵌套成员。
            for item in items {
                // 递归检查当前成员。
                assert_no_private_text(item);
            }
        }
        // 对象同时检查键和值。
        Value::Object(object) => {
            // 遍历全部公开字段。
            for (key, item) in object {
                // 字段名同样属于公开契约。
                assert_no_private_text(&Value::String(key.clone()));
                // 递归检查字段值。
                assert_no_private_text(item);
            }
        }
        // 非文本标量不携带禁止词。
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

// 验证可信 open success 并返回公开 session identity。
fn assert_open_success(output: &Output, host_id: &str) -> String {
    // open 必须取得可信完成。
    assert!(output.status.success(), "public open should succeed");
    // 解析公开 success envelope。
    let value = output_json(output);
    // 核对稳定 facade 与 capability。
    assert_eq!(value.get("app").and_then(Value::as_str), Some("app"));
    // 核对 generic create verb。
    assert_eq!(value.get("verb").and_then(Value::as_str), Some("create"));
    // 核对固定 open capability。
    assert_eq!(
        value.get("capability").and_then(Value::as_str),
        Some("browser.session.open@1")
    );
    // 顶层 target 必须保留原 host。
    assert_eq!(value.get("targetId").and_then(Value::as_str), Some(host_id));
    // 核对固定隔离执行域。
    assert_eq!(
        value.get("executionRealm").and_then(Value::as_str),
        Some("isolated-worker")
    );
    // 核对可信完成与不可重试事实。
    assert_eq!(
        value.pointer("/data/finalStateReached"),
        Some(&Value::Bool(true))
    );
    // 核对固定完成 outcome。
    assert_eq!(
        value.pointer("/data/outcome").and_then(Value::as_str),
        Some("completed")
    );
    // 核对前景不变。
    assert_eq!(
        value.pointer("/meta/foreground/unchanged"),
        Some(&Value::Bool(true))
    );
    // 公开输出不得泄漏私有字段。
    assert_no_private_text(&value);
    // 提取公开 session identity。
    let session_id = value
        // 进入 open data。
        .pointer("/data/sessionId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 成功必须签发 identity。
        .must("public open should return sessionId")
        // 保留给后续独立 launcher。
        .to_owned();
    // identity 必须匹配 canonical 形状。
    assert!(session_id.starts_with("s2:bs:") && session_id.len() == 38);
    // 返回公开 identity。
    session_id
}

// 验证可信 close success。
fn assert_close_success(output: &Output, session_id: &str) {
    // close 必须取得可信完成。
    assert!(output.status.success(), "public close should succeed");
    // 解析公开 success envelope。
    let value = output_json(output);
    // 核对固定 close capability。
    assert_eq!(
        value.get("capability").and_then(Value::as_str),
        Some("browser.session.close@1")
    );
    // 顶层 target 必须保留已关闭 identity。
    assert_eq!(
        value.get("targetId").and_then(Value::as_str),
        Some(session_id)
    );
    // 核对关闭事实。
    assert_eq!(value.pointer("/data/closed"), Some(&Value::Bool(true)));
    // 核对可信终态。
    assert_eq!(
        value.pointer("/data/finalStateReached"),
        Some(&Value::Bool(true))
    );
    // 核对前景不变。
    assert_eq!(
        value.pointer("/meta/foreground/unchanged"),
        Some(&Value::Bool(true))
    );
    // close data 不得重复 identity。
    assert!(value.pointer("/data/sessionId").is_none());
    // 公开输出不得泄漏私有字段。
    assert_no_private_text(&value);
}

// 等待工具 profile 根缺失或为空。
fn wait_for_profiles_clean(layout: &LauncherLayout) {
    // 建立固定回收预算。
    let deadline = Instant::now() + Duration::from_secs(5);
    // 轮询工具自有根。
    loop {
        // 根缺失或已经没有入口时视为完整清理。
        let profiles_clean = fs::read_dir(layout.profile_root())
            // 成功枚举时确认没有剩余入口。
            .map(|mut entries| entries.next().is_none())
            // 根被回收时同样满足资源收敛。
            .unwrap_or(true);
        // 清理完成时返回。
        if profiles_clean {
            // profile 生命周期已经收敛。
            return;
        }
        // 超时不能伪造清理。
        assert!(
            Instant::now() < deadline,
            "browser profile should be cleaned"
        );
        // 限制文件轮询 CPU。
        thread::sleep(Duration::from_millis(10));
    }
}

// 验证唯一生产 launcher 的真实公开生命周期与发布边界。
#[test]
fn production_launcher_closes_public_browser_sessions_without_private_leaks() {
    // 独占当前登录会话的固定 broker endpoint。
    let _test_guard = TEST_LOCK
        // 中毒表示前一场景已失败，继续运行会隐藏根因。
        .lock()
        // 保留明确测试上下文。
        .must("browser launcher tests should serialize");
    // 安装原样 launcher、生产 Rust siblings 与真实 worker。
    let layout = LauncherLayout::install(Path::new(WORKER_SOURCE));
    // 原样 launcher 必须不包含任何 C++ 路由。
    let launcher_text = fs::read_to_string(&layout.launcher)
        // launcher 必须保持 UTF-8 可读。
        .must("production launcher should be UTF-8");
    // 禁止历史 C++ binary 或 runtime 错误。
    assert!(!launcher_text.contains("ai-computer-toolkit-cpp.exe"));
    // 禁止历史 C++ runtime 选择器。
    assert!(!launcher_text.contains("CPP_RUNTIME_UNAVAILABLE"));
    // 经生产 launcher 发现当前 host。
    let host_id = discover_host(&layout);
    // 未确认 open 必须先于 broker 启动失败。
    let unconfirmed = run_lifecycle(
        // 使用隔离生产布局。
        &layout,
        // 使用 generic create。
        "create",
        // 使用固定 open capability。
        "browser.session.open@1",
        // 使用刚发现的 host target。
        &host_id,
        // 不提供确认。
        false,
        // 使用完整预算但不得进入 broker。
        30_000,
    );
    // 未确认请求必须非零退出。
    assert!(!unconfirmed.status.success());
    // 核对 confirmation-first 公开错误。
    assert_eq!(
        output_json(&unconfirmed)
            .pointer("/error/code")
            .and_then(Value::as_str),
        Some("CONFIRMATION_REQUIRED")
    );
    // 未确认请求不得启动 broker。
    assert!(matching_processes(&layout.broker, false).is_empty());
    // 最小预算必须在业务接受前结构化超时。
    let timeout = run_lifecycle(
        // 使用同一隔离布局。
        &layout,
        // 使用 generic create。
        "create",
        // 使用固定 open capability。
        "browser.session.open@1",
        // 使用当前 host。
        &host_id,
        // 已确认以进入总预算边界。
        true,
        // 一毫秒不足以完成 broker 启动与认证。
        1,
    );
    // 总预算耗尽必须非零退出。
    assert!(!timeout.status.success());
    // 核对公开 TIMEOUT 且未派发。
    let timeout_value = output_json(&timeout);
    // 错误码不得泄漏私有阶段。
    assert_eq!(
        timeout_value.pointer("/error/code").and_then(Value::as_str),
        Some("TIMEOUT")
    );
    // 业务接受事实必须为 false。
    assert_eq!(
        timeout_value.pointer("/error/details/accepted"),
        Some(&Value::Bool(false))
    );
    // 业务接受前耗尽的 open 不得遗留临时 profile。
    wait_for_profiles_clean(&layout);
    // timeout 输出不得泄漏私有字段。
    assert_no_private_text(&timeout_value);
    // 已确认 open 必须启动或连接固定 broker。
    let open = run_lifecycle(
        // 使用同一生产布局。
        &layout,
        // 使用 generic create。
        "create",
        // 使用固定 open capability。
        "browser.session.open@1",
        // 使用当前 host。
        &host_id,
        // 显式确认。
        true,
        // 使用完整物理预算。
        30_000,
    );
    // 提取可信公开 session identity。
    let session_id = assert_open_success(&open, &host_id);
    // 成功 open 后绑定当前测试拥有的固定 broker。
    let broker = wait_for_process(&layout.broker, true);
    // 精确绑定生产 worker。
    let worker = wait_for_process(&layout.worker, false);
    // 精确绑定 runtime fixture descendant。
    let runtime = wait_for_process(&layout.runtime, false);
    // profile 根必须只位于隔离 TEMP。
    assert!(layout.profile_root().is_dir());
    // 全新 launcher 关闭公开 session。
    let close = run_lifecycle(
        // 使用同一固定 broker 代际。
        &layout,
        // 使用 generic close。
        "close",
        // 使用固定 close capability。
        "browser.session.close@1",
        // 传递公开 session identity。
        &session_id,
        // 显式确认。
        true,
        // 使用完整回收预算。
        30_000,
    );
    // 核对可信 close success。
    assert_close_success(&close, &session_id);
    // 同一 worker 必须在 close 后退出。
    assert!(worker.wait_exited(Duration::from_secs(5)));
    // 同一 runtime descendant 必须在 close 后退出。
    assert!(runtime.wait_exited(Duration::from_secs(5)));
    // 进程清单必须收敛为无 worker。
    wait_for_no_process(&layout.worker);
    // 进程清单必须收敛为无 runtime。
    wait_for_no_process(&layout.runtime);
    // 工具 profile 必须完整清理。
    wait_for_profiles_clean(&layout);
    // close 后再次 close 必须在业务接受前 stale。
    let closed_stale = run_lifecycle(
        // 使用同一 broker 代际。
        &layout,
        // 使用 generic close。
        "close",
        // 使用固定 close capability。
        "browser.session.close@1",
        // 重用已关闭公开 identity。
        &session_id,
        // 显式确认。
        true,
        // 使用标准预算。
        5_000,
    );
    // stale close 必须非零退出。
    assert!(!closed_stale.status.success());
    // 核对公开 stale 语义。
    let closed_stale = output_json(&closed_stale);
    // 错误码必须固定。
    assert_eq!(
        closed_stale.pointer("/error/code").and_then(Value::as_str),
        Some("STALE_SESSION")
    );
    // stale 必须证明业务未接受。
    assert_eq!(
        closed_stale.pointer("/error/details/accepted"),
        Some(&Value::Bool(false))
    );
    // stale 输出不得泄漏私有字段。
    assert_no_private_text(&closed_stale);
    // 创建第二个 live session 供 broker restart stale 验证。
    let second_open = run_lifecycle(
        // 使用同一生产布局。
        &layout,
        // 使用 generic create。
        "create",
        // 使用固定 open capability。
        "browser.session.open@1",
        // 使用当前 host。
        &host_id,
        // 显式确认。
        true,
        // 使用完整预算。
        30_000,
    );
    // 提取第二个 live identity。
    let second_session_id = assert_open_success(&second_open, &host_id);
    // 第二个 identity 必须是新意图。
    assert_ne!(second_session_id, session_id);
    // 绑定第二个 worker 供 broker 崩溃回收见证。
    let second_worker = wait_for_process(&layout.worker, false);
    // 终止测试隔离布局中的唯一 broker。
    broker.terminate_owned();
    // broker Job 必须回收第二个 worker。
    assert!(second_worker.wait_exited(Duration::from_secs(5)));
    // worker 精确镜像必须收敛为零。
    wait_for_no_process(&layout.worker);
    // 全新 launcher 会启动新 broker 代际并拒绝旧 identity。
    let restart_stale = run_lifecycle(
        // 使用同一隔离安装镜像。
        &layout,
        // 使用 generic close。
        "close",
        // 使用固定 close capability。
        "browser.session.close@1",
        // 使用旧 broker 代际 identity。
        &second_session_id,
        // 显式确认。
        true,
        // 使用标准预算。
        5_000,
    );
    // 旧 identity 不得重绑。
    assert!(!restart_stale.status.success());
    // 核对新代际 stale 结果。
    let restart_stale = output_json(&restart_stale);
    // replacement broker 取得唯一所有权后必须回收上一代会话 profile。
    wait_for_profiles_clean(&layout);
    // 只允许公开 stale 错误。
    assert_eq!(
        restart_stale.pointer("/error/code").and_then(Value::as_str),
        Some("STALE_SESSION")
    );
    // restart stale 输出不得泄漏代际事实。
    assert_no_private_text(&restart_stale);
    // 绑定 replacement broker 供确定清理。
    let replacement = wait_for_process(&layout.broker, true);
    // 终止当前测试拥有的 replacement broker。
    replacement.terminate_owned();
    // endpoint owner 退出后才能删除安装布局。
    wait_for_no_process(&layout.broker);
    // 删除全部测试产物。
    layout.finish();
}

// 验证 accepted 后丢失可信 final 只投影公开 OutcomeUnknown。
#[test]
fn production_launcher_projects_accepted_timeout_as_outcome_unknown() {
    // 独占当前登录会话的固定 broker endpoint。
    let _test_guard = TEST_LOCK
        // 中毒表示前一场景已失败，继续运行会隐藏根因。
        .lock()
        // 保留明确测试上下文。
        .must("browser launcher tests should serialize");
    // 安装原样 launcher、生产 broker 与 accepted-only worker。
    let layout = LauncherLayout::install(Path::new(ACCEPTED_ONLY_WORKER_SOURCE));
    // 经生产 launcher 发现当前 host。
    let host_id = discover_host(&layout);
    // 执行会越过 worker accepted 但无法取得 final 的 open。
    let output = run_lifecycle(
        // 使用隔离生产布局。
        &layout,
        // 使用 generic create。
        "create",
        // 使用固定 open capability。
        "browser.session.open@1",
        // 使用当前 host。
        &host_id,
        // 显式确认。
        true,
        // 使用短但足以越过本地认证的预算。
        2_000,
    );
    // 未取得可信 final 必须非零退出。
    assert!(!output.status.success());
    // 解析统一公开错误。
    let value = output_json(&output);
    // 错误码必须保守为 OutcomeUnknown。
    assert_eq!(
        value.pointer("/error/code").and_then(Value::as_str),
        Some("OUTCOME_UNKNOWN")
    );
    // 必须保留已接受事实。
    assert_eq!(
        value.pointer("/error/details/accepted"),
        Some(&Value::Bool(true))
    );
    // 未取得可信 final。
    assert_eq!(
        value.pointer("/error/details/finalStateReached"),
        Some(&Value::Bool(false))
    );
    // accepted mutation 不可安全重试。
    assert_eq!(
        value.pointer("/error/details/retrySafe"),
        Some(&Value::Bool(false))
    );
    // 自动重试必须禁止。
    assert_eq!(
        value.pointer("/error/details/automaticRetryProhibited"),
        Some(&Value::Bool(true))
    );
    // open unknown 不得猜测 session identity。
    assert!(value.pointer("/error/details/sessionId").is_none());
    // 公开错误不得泄漏私有 broker 事实。
    assert_no_private_text(&value);
    // broker 必须回收 accepted-only worker。
    wait_for_no_process(&layout.worker);
    // 绑定测试隔离布局中的 broker。
    let broker = wait_for_process(&layout.broker, true);
    // 终止测试拥有的 broker。
    broker.terminate_owned();
    // endpoint owner 必须退出。
    wait_for_no_process(&layout.broker);
    // 删除全部测试产物。
    layout.finish();
}
