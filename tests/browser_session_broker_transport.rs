#![cfg(target_os = "windows")]

//! 验证固定 browser-session broker 的真实 Windows transport 与生命周期。

// 导入临时目录、进程与时间工具。
use std::{
    // 复制固定测试 executable 且读取 JSON 输出。
    fs,
    // 构造临时 sibling 路径。
    path::{Path, PathBuf},
    // 启动独立 broker 与 client 进程。
    process::{Child, Command, Output, Stdio},
    // 等待 endpoint 发布与进程退出。
    thread,
    // 构造唯一目录并约束等待。
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// 导入 provider-neutral JSON 读取。
use serde_json::Value;
// 导入当前 session 与固定 named-pipe 发布等待接口。
use windows::{
    // 只使用测试进程与 pipe 的原生只读事实。
    Win32::System::{
        // 等待固定 browser-session endpoint 可连接。
        Pipes::WaitNamedPipeW,
        // 映射当前测试进程到 native session。
        RemoteDesktop::ProcessIdToSessionId,
        // 查询当前测试进程 ID。
        Threading::GetCurrentProcessId,
    },
    // 导入 UTF-16 指针包装。
    core::PCWSTR,
};

// 固定 Cargo 构建的 broker 测试源镜像。
const BROKER_SOURCE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-broker");
// 固定 Cargo 构建的隐藏 client fixture 源镜像。
const CLIENT_SOURCE: &str =
    env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-transport-client-fixture");
// 固定 broker sibling 文件名。
const BROKER_FILE_NAME: &str = "ai-computer-toolkit-browser-session-broker.exe";
// 固定认证 client sibling 文件名。
const CLIENT_FILE_NAME: &str = "ai-computer-toolkit.exe";
// 固定错误 client 文件名。
const WRONG_CLIENT_FILE_NAME: &str = "untrusted-browser-session-client.exe";

// 为测试断言提供不引入额外依赖的失败消息。
trait Must<T> {
    // 成功时返回值，失败时携带上下文终止测试。
    fn must(self, context: &str) -> T;
}

// 为 Result 实现测试上下文提取。
impl<T, E: std::fmt::Debug> Must<T> for Result<T, E> {
    // 成功时返回值，失败时 panic。
    fn must(self, context: &str) -> T {
        // 保留上下文并避免生产 unwrap。
        self.unwrap_or_else(|error| panic!("{context}: {error:?}"))
    }
}

// 为 Option 实现测试上下文提取。
impl<T> Must<T> for Option<T> {
    // 有值时返回，缺失时 panic。
    fn must(self, context: &str) -> T {
        // 保留上下文并避免生产 unwrap。
        self.unwrap_or_else(|| panic!("{context}"))
    }
}

// 保存隔离测试目录并在退出时回收。
struct TestDirectory {
    // 保存绝对临时路径。
    path: PathBuf,
}

// 为隔离测试目录提供唯一创建。
impl TestDirectory {
    // 在系统临时目录创建不会覆盖正式 target 的 sibling 集合。
    fn create() -> Self {
        // 读取当前时间作为非安全唯一后缀。
        let timestamp = SystemTime::now()
            // 系统时钟应晚于 Unix epoch。
            .duration_since(UNIX_EPOCH)
            // 测试环境时钟异常应直接失败。
            .must("system clock should be after Unix epoch")
            // 使用纳秒降低同进程冲突概率。
            .as_nanos();
        // 组合固定测试前缀、PID 与时间。
        let path = std::env::temp_dir().join(format!(
            // 不复用生产安装目录。
            "ai-computer-toolkit-browser-broker-{}-{timestamp}",
            // PID 只参与测试路径唯一性。
            std::process::id(),
        ));
        // 创建空隔离目录。
        fs::create_dir(&path).must("isolated transport directory should be created");
        // 返回目录 owner。
        Self { path }
    }

    // 返回隔离目录中固定文件路径。
    fn file(&self, name: &str) -> PathBuf {
        // 仅测试常量会调用此 helper。
        self.path.join(name)
    }
}

// 测试退出时尽力回收隔离目录。
impl Drop for TestDirectory {
    // 删除仅由当前测试创建的精确目录。
    fn drop(&mut self) {
        // 回收失败不遮蔽原测试结论。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 保存 broker 子进程并保证测试退出时回收。
struct BrokerProcess {
    // 保存可终止子进程。
    child: Child,
}

// 为 broker 子进程提供启动与显式停止。
impl BrokerProcess {
    // 从隔离目录启动无参数 broker。
    fn start(image: &Path) -> Self {
        // 构造固定 broker 命令。
        let mut command = Command::new(image);
        // broker 不读取测试 stdin。
        command.stdin(Stdio::null());
        // 保留 stdout 供异常审计。
        command.stdout(Stdio::piped());
        // broker 不泄漏继承 stderr。
        command.stderr(Stdio::null());
        // 启动无参数进程。
        let child = command
            // 不提供 endpoint、path 或模式参数。
            .spawn()
            // 固定副本必须可启动。
            .must("browser broker should start from isolated sibling directory");
        // 返回 RAII owner。
        let mut process = Self { child };
        // 手动 owner 必须先发布 endpoint，client 才能运行并避免自启竞争 broker。
        process.wait_until_listening();
        // 返回已经确认拥有 endpoint 的进程。
        process
    }

    // 等待当前 child 发布固定 session endpoint，同时拒绝提前退出。
    fn wait_until_listening(&mut self) {
        // 取得当前测试进程 PID。
        let process_id = unsafe { GetCurrentProcessId() };
        // 初始化不会误报交互 session 的值。
        let mut session_id = 0_u32;
        // 查询当前 native session。
        unsafe { ProcessIdToSessionId(process_id, &mut session_id) }
            // 测试环境必须可解析 session。
            .must("test session should be available");
        // 构造编译期固定前缀与当前 session 后缀。
        let name = format!(
            // 不允许测试覆盖生产固定前缀。
            r"\\.\pipe\ai-computer-toolkit-browser-session-v1-{session_id}",
        );
        // 编码为 NUL 结尾 UTF-16。
        let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        // 使用五秒总预算等待 endpoint 发布。
        let deadline = Instant::now() + Duration::from_secs(5);
        // 短轮询 endpoint 与 child 状态。
        loop {
            // child 提前退出意味着手动 owner 从未发布 endpoint。
            assert!(
                self.child
                    // 非阻塞查询 broker 状态。
                    .try_wait()
                    // Windows 状态查询必须成功。
                    .must("browser broker status should be queryable")
                    // 仍运行时返回 None。
                    .is_none(),
                // 提供稳定竞态诊断。
                "browser broker exited before publishing its endpoint",
            );
            // 让内核等待最多十毫秒。
            if unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), 10) }.as_bool() {
                // endpoint 已可连接且尚未被 fixture 占用。
                return;
            }
            // 总预算耗尽表示 endpoint 未发布。
            assert!(
                Instant::now() < deadline,
                // 提供稳定超时诊断。
                "browser broker endpoint startup timed out",
            );
            // 限制轮询 CPU。
            thread::sleep(Duration::from_millis(10));
        }
    }

    // 终止当前 broker 并等待 handle 释放。
    fn stop(mut self) {
        // 仅测试进程由当前 owner 创建，可安全终止。
        let _ = self.child.kill();
        // 等待进程退出与 first-instance handle 释放。
        let _ = self.child.wait();
    }
}

// 测试提前退出时也回收 broker。
impl Drop for BrokerProcess {
    // 尽力终止并等待子进程。
    fn drop(&mut self) {
        // 仅处理仍存活的当前测试子进程。
        let _ = self.child.kill();
        // 防止留下持有固定 endpoint 的孤儿。
        let _ = self.child.wait();
    }
}

// 把真实 Cargo 测试二进制复制成固定 sibling 集合。
fn install_siblings(directory: &TestDirectory) -> (PathBuf, PathBuf, PathBuf) {
    // 构造隔离 broker 目标。
    let broker = directory.file(BROKER_FILE_NAME);
    // 构造固定认证 client 目标。
    let client = directory.file(CLIENT_FILE_NAME);
    // 构造相同代码但错误镜像路径的 client 目标。
    let wrong_client = directory.file(WRONG_CLIENT_FILE_NAME);
    // 复制 broker 而不触碰正式 target 同名文件。
    fs::copy(BROKER_SOURCE, &broker).must("browser broker test copy should succeed");
    // 复制固定 client 镜像。
    fs::copy(CLIENT_SOURCE, &client).must("fixed browser client test copy should succeed");
    // 复制错误 client 镜像用于 server auth 拒绝。
    fs::copy(CLIENT_SOURCE, &wrong_client)
        .must("untrusted browser client test copy should succeed");
    // 返回三个隔离绝对路径。
    (broker, client, wrong_client)
}

// 运行一次 client 并等待有界退出。
fn run_client(image: &Path) -> Output {
    // 启动固定无参数 client 并捕获 provider-neutral输出。
    Command::new(image)
        // 不接受 stdin。
        .stdin(Stdio::null())
        // 捕获唯一 JSON 结果。
        .stdout(Stdio::piped())
        // 不污染测试日志。
        .stderr(Stdio::null())
        // 执行并等待固定内部五秒预算。
        .output()
        // client 必须可以创建进程。
        .must("browser transport client should execute")
}

// 从成功 client 输出读取 canonical broker epoch。
fn ready_epoch(output: Output) -> String {
    // ready 探针必须成功退出。
    assert!(output.status.success(), "ready probe should succeed");
    // 输出必须是 UTF-8 JSON。
    let value = serde_json::from_slice::<Value>(&output.stdout)
        // 固定 fixture 输出必须可解析。
        .must("ready probe should emit JSON");
    // 成功 envelope 必须明确 ok。
    assert_eq!(value.get("ok").and_then(Value::as_bool), Some(true));
    // 读取 provider-neutral epoch。
    let epoch = value
        // 定位 brokerEpoch。
        .get("brokerEpoch")
        // 要求字符串类型。
        .and_then(Value::as_str)
        // 缺失 epoch 表示握手漂移。
        .must("ready probe should return brokerEpoch")
        // 复制用于跨进程比较。
        .to_owned();
    // epoch 必须是 128 位小写十六进制。
    assert_eq!(epoch.len(), 32);
    // 每个字符必须 canonical。
    assert!(
        epoch
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    // 返回 epoch。
    epoch
}

// 在短预算内等待 second broker first-instance loser 退出。
fn wait_for_exit(child: &mut Child) -> Option<std::process::ExitStatus> {
    // 固定两秒应足够完成首实例门禁。
    let deadline = Instant::now() + Duration::from_secs(2);
    // 轮询有界退出。
    while Instant::now() < deadline {
        // 查询非阻塞退出状态。
        if let Some(status) = child
            // Windows 进程查询必须成功。
            .try_wait()
            // 查询失败应直接失败。
            .must("duplicate broker status should be queryable")
        {
            // 返回已退出状态。
            return Some(status);
        }
        // 限制轮询 CPU。
        thread::sleep(Duration::from_millis(10));
    }
    // 超时表示 loser 错误存活。
    None
}

// 验证真实固定镜像双向认证、server-first ready、重听和代际生命周期。
#[test]
fn fixed_transport_reuses_epoch_rejects_loser_and_rotates_on_restart() {
    // 创建不会覆盖 target 正式 executable 的隔离目录。
    let directory = TestDirectory::create();
    // 安装固定 sibling 集合。
    let (broker_image, client_image, wrong_client_image) = install_siblings(&directory);
    // 启动首实例 broker owner。
    let broker = BrokerProcess::start(&broker_image);
    // 手动 owner 已发布 endpoint，client 不会触发自启竞争 broker。
    let first_epoch = ready_epoch(run_client(&client_image));
    // 第二连接必须由同一 handle 与同一进程代际服务。
    let second_epoch = ready_epoch(run_client(&client_image));
    // 同一 broker 进程跨连接保持 epoch。
    assert_eq!(first_epoch, second_epoch);
    // 错误镜像虽然位于同目录且运行同代码，仍不得收到 ready。
    let wrong_output = run_client(&wrong_client_image);
    // server auth 必须在 JSON 前关闭错误 peer。
    assert!(!wrong_output.status.success());
    // 启动同 endpoint 第二 broker 以验证 first-instance loser。
    let mut loser = Command::new(&broker_image)
        // loser 不读取 stdin。
        .stdin(Stdio::null())
        // 捕获安全启动错误。
        .stdout(Stdio::piped())
        // 不继承 stderr。
        .stderr(Stdio::null())
        // 不传递任何参数。
        .spawn()
        // 测试必须能创建 loser 进程。
        .must("duplicate browser broker should start and lose first-instance race");
    // loser 必须快速非零退出。
    let loser_status = wait_for_exit(&mut loser)
        // 存活表示 first-instance 门禁失败。
        .must("duplicate browser broker should exit promptly");
    // 重复 broker 不得报告成功。
    assert!(!loser_status.success());
    // loser 不能释放 winner 的 listener；第三连接仍使用同 epoch。
    let third_epoch = ready_epoch(run_client(&client_image));
    // winner 仍稳定持有同一内核实例。
    assert_eq!(first_epoch, third_epoch);
    // 终止 winner 并等待 first-instance handle 释放。
    broker.stop();
    // 启动新 broker 代际。
    let replacement = BrokerProcess::start(&broker_image);
    // 新代际必须服务固定 client。
    let replacement_epoch = ready_epoch(run_client(&client_image));
    // restart 后必须生成新随机 epoch。
    assert_ne!(first_epoch, replacement_epoch);
    // 显式停止 replacement，确保临时目录可回收。
    replacement.stop();
}
