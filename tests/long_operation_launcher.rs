#![cfg(target_os = "windows")]

//! 验证生产 launcher 跨进程复用固定长操作 broker 且无前景回退。

// 导入固定子进程、路径、线程与 deadline 工具。
use std::{
    // 创建并清理受控 JSON 输入文件。
    fs,
    // 定位生产 launcher。
    path::PathBuf,
    // 启动测试拥有的 broker 与生产 PowerShell launcher。
    process::{Child, Command, Output, Stdio},
    // 串行化固定同会话 endpoint 的真实 broker 测试。
    sync::Mutex,
    // 轮询固定 endpoint 就绪。
    thread,
    // 约束 fixture 启动等待。
    time::{Duration, Instant},
};

// 导入 provider-neutral JSON 值。
use serde_json::Value;
// 导入当前进程 session 与 named-pipe 等待接口。
use windows::{
    // 使用 Win32 私有测试事实。
    Win32::{
        // 查询当前进程 ID。
        System::{
            // 等待固定 broker pipe 发布。
            Pipes::WaitNamedPipeW,
            // 映射当前进程到 native session。
            RemoteDesktop::ProcessIdToSessionId,
            // 查询当前进程标识。
            Threading::GetCurrentProcessId,
        },
    },
    // 导入宽字符串指针。
    core::PCWSTR,
};

// 固定 cargo 构建的 broker binary。
const BROKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-long-operation-broker");
// 同一测试进程只能有一个用例拥有固定 broker endpoint。
static BROKER_TEST_LOCK: Mutex<()> = Mutex::new(());

// 拥有测试显式启动的 broker 子进程。
struct OwnedBroker {
    // 保存可回收 child handle。
    child: Child,
}

// 测试结束时回收精确拥有的 broker。
impl Drop for OwnedBroker {
    // 终止不含任务记录的测试 broker。
    fn drop(&mut self) {
        // 只终止当前对象直接启动的 child。
        let _ = self.child.kill();
        // 等待进程回收，避免污染后续测试。
        let _ = self.child.wait();
    }
}

// 启动无输入且不继承输出的测试 broker。
fn start_broker() -> OwnedBroker {
    // 只启动 Cargo 提供的固定 broker binary。
    let child = Command::new(BROKER)
        // broker 不读取 stdin。
        .stdin(Stdio::null())
        // broker 启动错误不污染测试输出。
        .stdout(Stdio::null())
        // broker 私有诊断不污染测试输出。
        .stderr(Stdio::null())
        // 启动固定 binary。
        .spawn()
        // 失败时保留测试诊断。
        .unwrap_or_else(|error| panic!("long operation broker fixture failed to start: {error}"));
    // 等待固定 endpoint 对当前 session 发布。
    wait_for_broker_endpoint();
    // 返回 child 唯一所有者。
    OwnedBroker { child }
}

// 等待固定当前 session pipe 出现。
fn wait_for_broker_endpoint() {
    // 读取当前进程 PID。
    let process_id = unsafe { GetCurrentProcessId() };
    // 初始化不会误报真实 session 的值。
    let mut session_id = 0_u32;
    // 查询当前 native session。
    unsafe { ProcessIdToSessionId(process_id, &mut session_id) }
        // 测试环境必须可解析 session。
        .unwrap_or_else(|error| panic!("test session lookup failed: {error}"));
    // 构造编译期固定前缀与 session 后缀。
    let name = format!(r"\\.\pipe\ai-computer-toolkit-long-operation-v1-{session_id}");
    // 编码为 NUL 结尾 UTF-16。
    let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    // 使用五秒总预算等待 endpoint。
    let deadline = Instant::now() + Duration::from_secs(5);
    // 短轮询直到 pipe 出现或 child 启动失败。
    loop {
        // 让内核等待最多十毫秒。
        if unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), 10) }.as_bool() {
            // endpoint 已可连接。
            return;
        }
        // 超时表示 broker 未发布固定 endpoint。
        assert!(
            Instant::now() < deadline,
            "broker endpoint startup timed out"
        );
        // 限制测试轮询 CPU。
        thread::sleep(Duration::from_millis(10));
    }
}

// 返回仓库唯一生产 launcher 路径。
fn launcher_path() -> PathBuf {
    // 从 Cargo manifest 根定位固定 PowerShell launcher。
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/windows/Invoke-ComputerControl.ps1")
}

// 经生产 launcher 执行固定参数。
fn launcher(arguments: &[&str]) -> Output {
    // 启动系统 PowerShell 执行唯一 launcher。
    Command::new("powershell.exe")
        // 禁止 profile 改写测试环境。
        .arg("-NoProfile")
        // 执行仓库固定脚本。
        .arg("-File")
        // 传入 launcher 路径。
        .arg(launcher_path())
        // 传入固定公开参数。
        .args(arguments)
        // 收集唯一 stdout/stderr。
        .output()
        // 启动失败时保留测试诊断。
        .unwrap_or_else(|error| panic!("production launcher failed to start: {error}"))
}

// 解析 launcher 唯一 JSON stdout。
fn launcher_json(output: &Output) -> Value {
    // 只从 stdout 解析结构化响应。
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        // 测试失败时保留 launcher 文本。
        panic!(
            "launcher JSON failed: {error}; stdout={}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

// 验证多个 launcher 进程复用同一 broker 并保持 handle-only 零命中语义。
#[test]
fn production_launchers_share_status_await_and_idempotent_cancel_route() {
    // 独占固定同会话 endpoint 的测试生命周期。
    let _guard = BROKER_TEST_LOCK
        // 取得串行测试锁。
        .lock()
        // poison 表示另一真实 broker 用例已异常。
        .unwrap_or_else(|error| panic!("long operation broker test lock poisoned: {error}"));
    // 测试拥有固定 broker 生命周期。
    let broker = start_broker();
    // 第一 launcher 执行无副作用 status Query。
    let status = launcher(&[
        // 使用长操作 surface。
        "operation",
        // 查询状态。
        "status",
        // 使用不存在但 canonical 的 handle。
        "s2:o:ffffffffffffffff",
        // 使用有界 transport deadline。
        "--timeout-ms",
        // 固定五秒预算。
        "5000",
    ]);
    // 零命中必须返回非零退出状态。
    assert!(!status.status.success());
    // 解析 status 响应。
    let status = launcher_json(&status);
    // 保持统一零命中错误码。
    assert_eq!(status["error"]["code"], "OPERATION_NOT_FOUND");
    // transport 已接受但业务未接受。
    assert_eq!(status["error"]["details"]["transportAccepted"], true);
    // 第二 launcher 执行有界 await Query。
    let await_result = launcher(&[
        // 使用长操作 surface。
        "operation",
        // 等待权威终态。
        "await",
        // 使用同一不存在但 canonical 的 handle。
        "s2:o:ffffffffffffffff",
        // 使用有界总等待 deadline。
        "--timeout-ms",
        // 固定五秒预算。
        "5000",
    ]);
    // 零命中 await 必须返回非零状态。
    assert!(!await_result.status.success());
    // 解析 await 响应。
    let await_result = launcher_json(&await_result);
    // await 必须忠实传播首次 status 的权威零命中。
    assert_eq!(await_result["error"]["code"], "OPERATION_NOT_FOUND");
    // 零命中不得伪造 timeout、取消或任务终态。
    assert!(await_result.get("operation").is_none());
    // 回收第一 broker 以模拟跨 launcher 之间的 broker 重启。
    drop(broker);
    // 启动新 broker 代际并恢复同一固定 journal。
    let _replacement = start_broker();
    // 第三 launcher 执行幂等 cancel Command。
    let cancel = launcher(&[
        // 使用长操作 surface。
        "operation",
        // 请求取消。
        "cancel",
        // 使用同一 canonical handle。
        "s2:o:ffffffffffffffff",
        // 使用有界 transport deadline。
        "--timeout-ms",
        // 固定五秒预算。
        "5000",
    ]);
    // 零命中 cancel 同样必须返回非零。
    assert!(!cancel.status.success());
    // 解析 cancel 响应。
    let cancel = launcher_json(&cancel);
    // 两个 launcher 观察同一 registry 代际零命中事实。
    assert_eq!(cancel["error"]["code"], "OPERATION_NOT_FOUND");
    // 禁止任何前景回退证据字段。
    assert!(cancel.to_string().find("foreground").is_none());
}

// 验证生产 launcher 把 start 投递给 broker 且在业务接受前拒绝非法输入。
#[test]
fn production_launcher_routes_confirmed_start_without_creating_invalid_task() {
    // 独占固定同会话 endpoint 的测试生命周期。
    let _guard = BROKER_TEST_LOCK
        // 取得串行测试锁。
        .lock()
        // poison 表示另一真实 broker 用例已异常。
        .unwrap_or_else(|error| panic!("long operation broker test lock poisoned: {error}"));
    // 测试拥有固定 broker 生命周期。
    let _broker = start_broker();
    // 构造唯一受控输入文件。
    let input_path = std::env::temp_dir().join(format!(
        // 固定测试前缀与进程隔离。
        "act-long-operation-invalid-start-{}.json",
        // 使用当前测试进程 ID。
        std::process::id(),
    ));
    // 写入领域 Module 必须拒绝的相对输出路径。
    fs::write(&input_path, br#"{"outputPath":"relative.mp4"}"#)
        // 夹具写入失败时保留完整测试诊断。
        .unwrap_or_else(|error| panic!("invalid start fixture write failed: {error}"));
    // 转换为 launcher 接受的路径文本。
    let input_path_text = input_path.to_string_lossy().into_owned();
    // 经生产 PowerShell launcher 提交确认后的 start。
    let start = launcher(&[
        // 使用长操作 surface。
        "operation",
        // 使用非幂等 start Command。
        "start",
        // 使用首个冻结 capability。
        "window.record@1",
        // 传入唯一公开精确目标。
        "--target",
        // 使用 canonical 但不存在的窗口目标。
        "sessionId=s2:w:0000000000000001",
        // 传入受控 JSON 文件。
        "--input",
        // 借用测试文件路径。
        &input_path_text,
        // 显式确认副作用。
        "--confirm",
        // 使用有界 transport deadline。
        "--timeout-ms",
        // 固定五秒预算。
        "5000",
    ]);
    // 删除当前测试自建的输入文件。
    let _ = fs::remove_file(&input_path);
    // 无效领域输入必须返回非零退出状态。
    assert!(!start.status.success());
    // 解析 start 拒绝响应。
    let start = launcher_json(&start);
    // broker 保留稳定参数错误码。
    assert_eq!(start["error"]["code"], "INVALID_ARGUMENT");
    // transport 已接受完整协议 frame。
    assert_eq!(start["error"]["details"]["transportAccepted"], true);
    // 业务未建立 operation handle。
    assert_eq!(start["error"]["details"]["businessAccepted"], false);
    // 拒绝不得伪造 operation 状态。
    assert!(start.get("operation").is_none());
}

// 验证 launcher 与 client 源码没有 caller 可控进程或前景入口。
#[test]
fn client_route_is_fixed_and_foreground_free() {
    // 嵌入固定 broker Adapter 源码。
    let adapter = include_str!("../src/adapters/long_operation_broker_windows.rs");
    // 只允许固定 sibling 文件名。
    assert!(adapter.contains("ai-computer-toolkit-long-operation-broker.exe"));
    // 禁止 shell 解释器。
    assert!(!adapter.contains("cmd.exe"));
    // 禁止 caller argv 解析。
    assert!(!adapter.contains("std::env::args"));
    // 禁止前景激活。
    assert!(!adapter.contains("SetForegroundWindow"));
    // submit 使用独立非幂等投递入口。
    assert!(adapter.contains("pub(crate) fn exchange_submit"));
    // 写后失败必须公开未知结果语义。
    assert!(adapter.contains("businessAcceptedMayHaveOccurred"));
    // 生产 launcher 继续只进入 Rust runtime。
    let launcher = include_str!("../tools/windows/Invoke-ComputerControl.ps1");
    // 不得出现 C++ fallback。
    assert!(!launcher.contains("ai-computer-toolkit-cpp.exe"));
}
