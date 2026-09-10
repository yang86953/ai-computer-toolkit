#![cfg(target_os = "windows")]

//! 验证页面命令 accepted 后的取消、期限和断线语义。

// 导入进程、管道、路径与 JSON 测试工具。
use std::{
    // 导入逐行协议读写。
    io::{BufRead, BufReader, Write},
    // 导入测试路径。
    path::{Path, PathBuf},
    // 导入固定子进程与管道类型。
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    // 导入原子测试目录序列。
    sync::atomic::{AtomicU64, Ordering},
};

// 导入中立 JSON 值。
use serde_json::{Value, json};

// 固定生产 browser-session worker 路径。
const WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-worker");
// 固定仓库自有 session runtime fixture 路径。
const RUNTIME: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-runtime-fixture");
// 固定打开协议版本。
const OPEN_VERSION: &str = "act/browser-session-worker/v1";
// 固定页面协议版本。
const PAGE_VERSION: &str = "act/browser-page-command-worker/v1";
// 固定打开请求 nonce。
const OPEN_NONCE: &str = "0123456789abcdef0123456789abcdef";
// 固定页面请求 nonce。
const PAGE_NONCE: &str = "abcdef0123456789abcdef0123456789";
// 保存进程内测试目录序列。
static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 保存一个在途页面中断场景。
#[derive(Clone, Copy)]
enum InterruptScenario {
    // accepted 后显式 cancel-command。
    Cancel,
    // accepted 后总 deadline 到达。
    Deadline,
    // accepted 后浏览器 socket 断开。
    Disconnect,
}

// 保存真实 worker 测试进程与 parent 管道。
struct WorkerFixture {
    // 保存 worker 进程。
    child: Child,
    // 保存 stdin 写端。
    stdin: ChildStdin,
    // 保存 stdout reader。
    stdout: BufReader<ChildStdout>,
    // 保存隔离临时根。
    temporary_root: PathBuf,
}

// 为中断 fixture 提供确定性生命周期。
impl WorkerFixture {
    // 启动指定 runtime 模式的生产 worker。
    fn spawn(mode: &str) -> Self {
        // 生成唯一测试临时根。
        let temporary_root = std::env::temp_dir().join(format!(
            // 使用固定前缀、进程和序列。
            "act-browser-page-interrupt-{}-{}",
            // 注入测试进程 ID。
            std::process::id(),
            // 注入唯一序列。
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        // 清理理论陈旧目录。
        let _ = std::fs::remove_dir_all(&temporary_root);
        // 创建可写临时根。
        std::fs::create_dir(&temporary_root)
            // 测试环境失败时给出诊断。
            .unwrap_or_else(|error| panic!("temporary root failed: {error}"));
        // 构造固定 worker 命令。
        let mut command = Command::new(WORKER);
        // 只发现仓库 runtime fixture。
        command.env("AI_COMPUTER_TOOLKIT_BROWSER_PATH", RUNTIME);
        // 注入固定故障模式。
        command.env("ACT_BROWSER_SESSION_RUNTIME_FIXTURE_MODE", mode);
        // 隔离 TEMP。
        command.env("TEMP", &temporary_root);
        // 隔离 TMP。
        command.env("TMP", &temporary_root);
        // 建立 parent stdin。
        command.stdin(Stdio::piped());
        // 建立 parent stdout。
        command.stdout(Stdio::piped());
        // 隔离诊断输出。
        command.stderr(Stdio::null());
        // 启动真实 worker。
        let mut child = command
            // 执行无 shell spawn。
            .spawn()
            // 启动失败时终止测试。
            .unwrap_or_else(|error| panic!("worker spawn failed: {error}"));
        // 取得 stdin 写端。
        let stdin = child
            // 移交 stdin。
            .stdin
            // 取得所有权。
            .take()
            // 缺失表示模板漂移。
            .unwrap_or_else(|| panic!("worker stdin missing"));
        // 取得 stdout 读端。
        let stdout = child
            // 移交 stdout。
            .stdout
            // 取得所有权。
            .take()
            // 缺失表示模板漂移。
            .unwrap_or_else(|| panic!("worker stdout missing"));
        // 返回完整 fixture。
        Self {
            // 保存进程。
            child,
            // 保存 stdin。
            stdin,
            // 缓冲 stdout。
            stdout: BufReader::new(stdout),
            // 保存临时根。
            temporary_root,
        }
    }

    // 写入一条 JSON Lines 输入。
    fn write(&mut self, value: &Value) {
        // 序列化固定请求。
        let text = serde_json::to_string(value)
            // 固定值必须可序列化。
            .unwrap_or_else(|error| panic!("request serialization failed: {error}"));
        // 写入、换行并 flush。
        writeln!(self.stdin, "{text}")
            // 强制暴露帧。
            .and_then(|()| self.stdin.flush())
            // 管道失败时终止测试。
            .unwrap_or_else(|error| panic!("worker stdin failed: {error}"));
    }

    // 读取一条必需 worker 帧。
    fn read_frame(&mut self) -> Value {
        // 保存一行输出。
        let mut line = String::new();
        // 读取到换行。
        let read = self
            // 借用 stdout reader。
            .stdout
            // 读取一行。
            .read_line(&mut line)
            // 读取失败时终止测试。
            .unwrap_or_else(|error| panic!("worker stdout failed: {error}"));
        // 必需帧不得为 EOF。
        assert_ne!(read, 0, "worker stdout ended before required frame");
        // 严格解析 JSON。
        serde_json::from_str(line.trim_end())
            // 协议污染时终止测试。
            .unwrap_or_else(|error| panic!("worker frame failed: {error}"))
    }

    // 关闭 stdin 并等待 worker 退出。
    fn close_and_wait(mut self) -> u32 {
        // 关闭 parent 输入端。
        drop(self.stdin);
        // 等待 worker 退出。
        self.child
            // 回收进程。
            .wait()
            // 等待失败时终止测试。
            .unwrap_or_else(|error| panic!("worker wait failed: {error}"))
            // 读取退出码。
            .code()
            // 转换为无符号值。
            .and_then(|code| u32::try_from(code).ok())
            // 外部终止使用哨兵。
            .unwrap_or(u32::MAX)
    }

    // 返回工具 profile 固定根。
    fn profile_root(&self) -> PathBuf {
        // 与生产 BrowserProfile 规则一致。
        self.temporary_root.join("ai-computer-toolkit-browser")
    }
}

// 构造固定打开请求。
fn open_request() -> Value {
    // 返回隔离 profile 打开。
    json!({
        // 使用 open 变体。
        "kind": "open",
        // 使用冻结版本。
        "contractVersion": OPEN_VERSION,
        // 关联打开请求。
        "requestNonce": OPEN_NONCE,
        // 使用充足打开预算。
        "timeoutMs": 5_000,
        // 使用工具自有隔离来源。
        "source": { "kind": "isolated-profile" },
    })
}

// 构造初始页面导航。
fn navigate_request(session_id: &str, timeout_ms: u32) -> Value {
    // 返回强类型导航命令。
    json!({
        // 使用 command 变体。
        "kind": "command",
        // 使用冻结页面版本。
        "contractVersion": PAGE_VERSION,
        // 绑定当前会话。
        "sessionId": session_id,
        // 关联页面请求。
        "requestNonce": PAGE_NONCE,
        // 注入场景期限。
        "timeoutMs": timeout_ms,
        // 初始导航没有页面引用。
        "pageRef": Value::Null,
        // 初始代际为零。
        "navigationGeneration": 0,
        // 使用固定测试 URL。
        "operation": { "kind": "navigate", "url": "https://example.test/page" },
    })
}

// 构造关联页面取消。
fn cancel_request(session_id: &str) -> Value {
    // 返回 cancel-command。
    json!({
        // 使用取消变体。
        "kind": "cancel-command",
        // 使用冻结页面版本。
        "contractVersion": PAGE_VERSION,
        // 绑定当前会话。
        "sessionId": session_id,
        // 关联页面请求。
        "requestNonce": PAGE_NONCE,
    })
}

// 验证三种 accepted 后中断都保守投影未知并回收。
#[test]
// 逐项执行取消、deadline 与断线场景。
fn accepted_page_interrupts_report_unknown_and_reap_session() {
    // 遍历封闭中断集合。
    for scenario in [
        // 覆盖显式取消。
        InterruptScenario::Cancel,
        // 覆盖总期限。
        InterruptScenario::Deadline,
        // 覆盖浏览器断线。
        InterruptScenario::Disconnect,
    ] {
        // 映射固定 runtime 模式与页面期限。
        let (mode, timeout_ms) = match scenario {
            // 取消使用延迟响应和正常预算。
            InterruptScenario::Cancel => ("page-delay", 5_000),
            // deadline 使用延迟响应和短预算。
            InterruptScenario::Deadline => ("page-delay", 50),
            // 断线使用固定断开和正常预算。
            InterruptScenario::Disconnect => ("page-disconnect", 5_000),
        };
        // 启动当前故障模式。
        let mut fixture = WorkerFixture::spawn(mode);
        // 保存待验证 profile 根。
        let profile_root = fixture.profile_root();
        // 打开隔离会话。
        fixture.write(&open_request());
        // 丢弃打开 accepted。
        let _ = fixture.read_frame();
        // 读取 ready final。
        let ready = fixture.read_frame();
        // 取得 opaque 会话身份。
        let session_id = ready
            // 读取会话字段。
            .get("sessionId")
            // 转换为字符串。
            .and_then(Value::as_str)
            // ready 必须携带身份。
            .unwrap_or_else(|| panic!("ready session id missing"));
        // 发送当前页面导航。
        fixture.write(&navigate_request(session_id, timeout_ms));
        // 读取页面 accepted。
        let accepted = fixture.read_frame();
        // 核对 accepted 边界。
        assert_eq!(
            // 读取帧种类。
            accepted.get("kind").and_then(Value::as_str),
            // 必须已 accepted。
            Some("command-accepted")
        );
        // 取消场景发送关联 cancel-command。
        if matches!(scenario, InterruptScenario::Cancel) {
            // 写入页面取消。
            fixture.write(&cancel_request(session_id));
        }
        // 读取保守 final。
        let final_frame = fixture.read_frame();
        // 三种 accepted 后中断都必须 unknown。
        assert_eq!(
            // 读取 outcome。
            final_frame.get("outcome").and_then(Value::as_str),
            // 必须保守未知。
            Some("unknown")
        );
        // 核对唯一未知错误码。
        assert_eq!(
            // 读取错误码。
            final_frame.pointer("/error/code").and_then(Value::as_str),
            // 必须为 OUTCOME_UNKNOWN。
            Some("OUTCOME_UNKNOWN")
        );
        // 不可信会话必须失败退出。
        assert_eq!(fixture.close_and_wait(), 2);
        // profile 必须清理。
        assert!(!Path::new(&profile_root).exists());
    }
}
