#![cfg(target_os = "windows")]

//! 验证 production broker 经 launcher 双向 peer 认证、真实 wire/runtime/dispatcher 服务 open/close。

// 导入临时目录、JSON stdin、进程与等待工具。
use std::{
    // 导入 Windows 镜像路径缓冲区转换。
    ffi::OsString,
    // 导入仅测试目录的创建、复制与回收。
    fs,
    // 导入向独立 launcher stdin 写入 JSON。
    io::Write,
    // 导入 Windows 进程快照结构长度。
    mem::size_of,
    // 导入 Windows UTF-16 路径转换。
    os::windows::ffi::OsStringExt,
    // 导入固定 sibling 路径。
    path::{Path, PathBuf},
    // 导入 broker 和四次独立 launcher 进程。
    process::{Child, Command, Output, Stdio},
    // 导入 endpoint 发布轮询。
    thread,
    // 导入唯一目录与等待预算。
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// 导入 provider-neutral JSON 值。
use serde_json::{Value, json};
// 导入固定 endpoint 发布所需的最小 Windows 事实。
use windows::{
    // 导入 test-only 进程见证与 endpoint 等待 API。
    Win32::{
        // 导入测试句柄关闭与等待结果。
        Foundation::{
            CloseHandle, ERROR_NO_MORE_FILES, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        // 导入当前 session、进程快照与 named-pipe 等待 API。
        System::{
            // 导入只读进程快照 API。
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            // 导入等待固定 browser-session endpoint。
            Pipes::WaitNamedPipeW,
            // 导入当前测试进程 native session 查询。
            RemoteDesktop::ProcessIdToSessionId,
            // 导入完整镜像查询与有界进程等待。
            Threading::{
                GetCurrentProcessId, OpenProcess, PROCESS_ACCESS_RIGHTS,
                PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, WaitForSingleObject,
            },
        },
    },
    // 导入 NUL 结尾与可写 UTF-16 视图。
    core::{PCWSTR, PWSTR},
};

// 固定 Cargo 构建的生产 broker 源镜像。
const BROKER_SOURCE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-broker");
// 固定 Cargo 构建的 command fixture 源镜像。
const COMMAND_SOURCE: &str =
    env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-command-fixture");
// 固定 Cargo 构建的 fixture worker 源镜像。
const WORKER_SOURCE: &str =
    env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-production-worker-fixture");
// 固定 broker sibling 名称。
const BROKER_FILE_NAME: &str = "ai-computer-toolkit-browser-session-broker.exe";
// 固定 command launcher 镜像名称。
const COMMAND_FILE_NAME: &str = "ai-computer-toolkit.exe";
// 固定生产 worker sibling 名称。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-browser-session-worker.exe";
// 固定进程等待所需的同步权限位。
const PROCESS_SYNCHRONIZE: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0010_0000);
// 固定 test-only 完整镜像路径缓冲区。
const MAXIMUM_IMAGE_PATH_UNITS: usize = 32_768;
// 为物理进程复制、认证与 worker 启动验收使用协议允许的完整预算。
const COMMAND_TIMEOUT_MS: u32 = 30_000;

// 为测试断言提供不引入额外依赖的失败上下文。
trait Must<T> {
    // 成功时返回值，失败时以上下文终止当前测试。
    fn must(self, context: &str) -> T;
}

// 为 Result 提供上下文提取。
impl<T, E: std::fmt::Debug> Must<T> for Result<T, E> {
    // 成功时返回值，失败时携带原始诊断。
    fn must(self, context: &str) -> T {
        // 测试失败保留局部上下文。
        self.unwrap_or_else(|error| panic!("{context}: {error:?}"))
    }
}

// 为 Option 提供上下文提取。
impl<T> Must<T> for Option<T> {
    // 有值时返回值，缺失时终止当前测试。
    fn must(self, context: &str) -> T {
        // 缺失值没有可展示的底层错误。
        self.unwrap_or_else(|| panic!("{context}"))
    }
}

// 保存仅由本测试创建的隔离 sibling 目录。
struct TestDirectory {
    // 保存用于复制 fixture 的绝对路径。
    path: Option<PathBuf>,
}

// 为隔离目录提供创建和固定 sibling 定位。
impl TestDirectory {
    // 创建一个不接触生产安装目录的空测试目录。
    fn create() -> Self {
        // 取得系统时钟形成测试级唯一后缀。
        let timestamp = SystemTime::now()
            // 测试时钟必须晚于 Unix epoch。
            .duration_since(UNIX_EPOCH)
            // 时钟异常不能安全创建可追溯路径。
            .must("system clock should be after Unix epoch")
            // 使用纳秒减少同一 session 并发碰撞。
            .as_nanos();
        // 构造只用于当前 PID 的精确测试目录。
        let path = std::env::temp_dir().join(format!(
            // 不复用生产目录或 broker 名称。
            "ai-computer-toolkit-browser-session-command-e2e-{}-{timestamp}",
            // PID 只参与本地目录唯一性。
            std::process::id(),
        ));
        // 创建精确新目录。
        fs::create_dir(&path).must("isolated command e2e directory should be created");
        // 返回目录唯一 owner。
        Self { path: Some(path) }
    }

    // 返回此隔离目录下固定 sibling 的绝对路径。
    fn file(&self, name: &str) -> PathBuf {
        // sibling 名称由被测生产路由冻结。
        self.path
            // 测试显式回收前目录必须仍由 owner 持有。
            .as_ref()
            // 缺失目录表示测试错误地在完成后继续使用。
            .must("isolated command e2e directory should remain live")
            // 拼接固定 sibling 名称。
            .join(name)
    }

    // 显式删除测试目录并把删除失败纳入验收结论。
    fn finish(mut self) {
        // 取得仍由测试独占的目录路径。
        let path = self
            // 转移路径以阻止 Drop 重复删除。
            .path
            // 只允许显式完成一次。
            .take()
            // 缺失路径表示生命周期测试自身漂移。
            .must("isolated command e2e directory should finish once");
        // 所有 broker 与 worker 退出后目录必须可完整回收。
        fs::remove_dir_all(path).must("isolated command e2e directory should be removable");
    }
}

// 测试完成后尽力回收仅由当前测试创建的目录。
impl Drop for TestDirectory {
    // 回收目录及已停止的 fixture copies。
    fn drop(&mut self) {
        // 仅异常展开路径保留尽力回收语义。
        if let Some(path) = self.path.take() {
            // 异常路径不覆盖原始断言失败。
            let _ = fs::remove_dir_all(path);
        }
    }
}

// 让 test-only Windows handle 在所有退出路径上确定关闭。
struct TestHandle(HANDLE);

// 为有效测试 handle 提供最小 RAII 所有权。
impl TestHandle {
    // 接管 Windows API 返回的有效 handle。
    fn new(handle: HANDLE) -> Option<Self> {
        // 空值和 INVALID_HANDLE_VALUE 都不建立见证。
        (!handle.is_invalid()).then_some(Self(handle))
    }

    // 返回只在本测试文件内使用的复制 handle 值。
    const fn raw(&self) -> HANDLE {
        // HANDLE 不进入任何产品或 JSON 边界。
        self.0
    }
}

// 确保 test-only handle 不泄漏到测试进程生命周期之外。
impl Drop for TestHandle {
    // 关闭当前唯一拥有的 Windows handle。
    fn drop(&mut self) {
        // 关闭失败不改变此前基于同一 handle 的生命周期结论。
        let _ = unsafe { CloseHandle(self.0) };
    }
}

// 保存与精确 worker 镜像绑定且防 PID 复用的内核进程见证。
struct WorkerProcessWitness {
    // 持有目标进程对象直到退出事实完成。
    process: TestHandle,
}

// 为 worker 进程见证提供精确镜像发现和有界退出等待。
impl WorkerProcessWitness {
    // 在总预算内查找唯一精确镜像并持有其内核进程对象。
    fn find_exact_image(expected: &Path, timeout: Duration) -> Self {
        // 建立不会被轮询重置的总 deadline。
        let deadline = Instant::now() + timeout;
        // 持续读取独立进程快照直到 worker 发布。
        loop {
            // 只接受当前隔离目录中的完整镜像匹配。
            let mut matches = matching_worker_processes(expected);
            // 同一路径同时出现多个 worker 表示 registry 或回收边界失信。
            assert!(
                matches.len() <= 1,
                "more than one fixed browser worker was live"
            );
            // 唯一匹配建立后返回其持有型见证。
            if let Some(process) = matches.pop() {
                // 返回不公开 PID 的内核对象所有权。
                return process;
            }
            // 查找总预算耗尽即失败。
            assert!(
                Instant::now() < deadline,
                "fixed browser worker did not become live"
            );
            // 限制进程快照轮询 CPU。
            thread::sleep(Duration::from_millis(10));
        }
    }

    // 等待同一内核进程对象退出而不重新按 PID 查询。
    fn wait_exited(&self, timeout: Duration) -> bool {
        // 把测试有界时长转换为 Windows 毫秒。
        let timeout_ms = u32::try_from(timeout.as_millis())
            // 测试预算固定远小于 u32 上限。
            .must("worker exit timeout should fit Windows milliseconds");
        // 只等待已经绑定的进程对象。
        (unsafe { WaitForSingleObject(self.process.raw(), timeout_ms) }) == WAIT_OBJECT_0
    }

    // 只读判断同一内核进程对象当前仍未退出。
    fn is_live(&self) -> bool {
        // 零预算等待只观察当前生命周期，不改变 worker。
        (unsafe { WaitForSingleObject(self.process.raw(), 0) }) == WAIT_TIMEOUT
    }
}

// 查询精确镜像的所有当前进程并返回持有型见证。
fn matching_worker_processes(expected: &Path) -> Vec<WorkerProcessWitness> {
    // 规范化测试自行复制且仍存在的目标镜像路径。
    let expected = fs::canonicalize(expected).must("fixed browser worker image should exist");
    // 创建只读进程快照。
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        // 当前 Windows 测试环境必须允许读取同用户进程清单。
        .must("browser worker process snapshot should be available");
    // 接管快照 handle。
    let snapshot =
        TestHandle::new(snapshot).must("browser worker process snapshot should be valid");
    // 初始化 Windows 要求的结构大小。
    let mut entry = PROCESSENTRY32W {
        // 写入精确结构长度。
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>())
            // 平台结构长度必须能投影到 Win32 u32。
            .must("process snapshot entry size should fit u32"),
        // 其余字段保持平台零值。
        ..Default::default()
    };
    // 进程快照至少应包含当前测试进程。
    unsafe { Process32FirstW(snapshot.raw(), &mut entry) }
        // 无法读取首项不能证明 worker 生命周期。
        .must("browser worker process snapshot should contain entries");
    // 保存完整镜像精确匹配。
    let mut matches = Vec::new();
    // 遍历快照直到自然结束。
    loop {
        // 尝试以查询和同步所需的最小权限绑定当前候选。
        if let Some(process) = worker_process_for_entry(&entry, &expected) {
            // 保存持有型进程见证。
            matches.push(WorkerProcessWitness { process });
        }
        // 快照自然结束时停止遍历。
        if let Err(error) = unsafe { Process32NextW(snapshot.raw(), &mut entry) } {
            // 只有 Windows 明确的无更多文件才能证明快照完整结束。
            assert_eq!(
                error.code(),
                ERROR_NO_MORE_FILES.to_hresult(),
                "browser worker process snapshot ended unexpectedly"
            );
            // 完整快照已经遍历完毕。
            break;
        }
    }
    // 返回不包含 PID 或路径文本的私有见证集合。
    matches
}

// 为快照候选建立 handle-bound PID 与完整镜像匹配。
fn worker_process_for_entry(entry: &PROCESSENTRY32W, expected: &Path) -> Option<TestHandle> {
    // 使用镜像查询和有界等待所需的最小权限打开候选。
    let handle = unsafe {
        OpenProcess(
            // 组合只读镜像查询与同步等待权限。
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            // 测试 handle 不得被后续进程继承。
            false,
            // PID 仅用于取得稳定内核对象。
            entry.th32ProcessID,
        )
    }
    // 进程可能在快照后退出；此时跳过候选。
    .ok()?;
    // 接管有效进程 handle。
    let handle = TestHandle::new(handle)?;
    // 从同一对象回读 PID 以拒绝快照后的 PID 复用。
    if unsafe { windows::Win32::System::Threading::GetProcessId(handle.raw()) }
        != entry.th32ProcessID
    {
        // 漂移候选不建立见证。
        return None;
    }
    // 查询同一内核对象的完整镜像路径。
    let image = process_image(handle.raw())?;
    // 只允许当前仍处于 live 状态的候选成为进程见证。
    match unsafe { WaitForSingleObject(handle.raw(), 0) } {
        // 零超时仍等待表示进程当前 live。
        WAIT_TIMEOUT => {}
        // 已退出候选属于快照竞态，应由外层重新观察。
        WAIT_OBJECT_0 => return None,
        // 其他状态包含 WAIT_FAILED，不能冒充可靠见证。
        status => {
            // 明确锁住 Windows 失败状态而不公开原生数值。
            assert_ne!(
                status, WAIT_FAILED,
                "fixed browser worker liveness query failed"
            );
            // 未知等待状态同样失败闭合。
            panic!("fixed browser worker returned an invalid liveness state");
        }
    }
    // 规范化仍存活候选的真实文件路径。
    let image = fs::canonicalize(image).ok()?;
    // Windows 路径比较必须忽略盘符和文件名大小写。
    image
        // 转换为不会泄漏出测试失败信息的文本视图。
        .to_string_lossy()
        // 与测试自行复制的完整镜像逐字比较。
        .eq_ignore_ascii_case(&expected.to_string_lossy())
        // 只有精确匹配才转移 handle 所有权。
        .then_some(handle)
}

// 从已打开进程对象读取有界完整镜像路径。
fn process_image(process: HANDLE) -> Option<PathBuf> {
    // 分配 Windows 文档允许的固定最大路径缓冲区。
    let mut buffer = vec![0_u16; MAXIMUM_IMAGE_PATH_UNITS];
    // 传入容量并接收实际 UTF-16 单元数。
    let mut length = u32::try_from(buffer.len()).ok()?;
    // 从同一进程对象查询默认 DOS 路径。
    unsafe {
        QueryFullProcessImageNameW(
            // 使用 handle-bound 进程身份。
            process,
            // 保持默认 Win32 路径格式。
            Default::default(),
            // 提供可写 UTF-16 缓冲区。
            PWSTR(buffer.as_mut_ptr()),
            // 提供并接收有界长度。
            &mut length,
        )
    }
    // 无权限或进程已退出时跳过候选。
    .ok()?;
    // 转换平台返回的实际长度。
    let length = usize::try_from(length).ok()?;
    // 空路径与越界长度都不能建立镜像见证。
    if length == 0 || length > buffer.len() {
        // 失败闭合且不公开候选路径。
        return None;
    }
    // 把精确 UTF-16 前缀转换为拥有型 Windows 路径。
    Some(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

// 断言当前不存在测试目录中的精确 worker 镜像。
fn assert_no_worker_now(expected: &Path) {
    // 初始与 stale 路径都不得存在残留 worker。
    assert!(
        matching_worker_processes(expected).is_empty(),
        "fixed browser worker should not be live"
    );
}

// 在总预算内等待测试目录中的精确 worker 镜像全部退出。
fn wait_for_no_worker(expected: &Path, timeout: Duration) {
    // 建立不会因进程快照轮询而重置的总 deadline。
    let deadline = Instant::now() + timeout;
    // 等待所有精确匹配离开系统进程清单。
    loop {
        // 零匹配证明没有同镜像 worker 残留。
        if matching_worker_processes(expected).is_empty() {
            // 生命周期验收完成。
            return;
        }
        // 总预算耗尽即报告资源残留。
        assert!(
            Instant::now() < deadline,
            "fixed browser worker remained live"
        );
        // 限制进程快照轮询 CPU。
        thread::sleep(Duration::from_millis(10));
    }
}

// 保存由当前测试创建的 fixture broker 子进程。
struct BrokerProcess {
    // 保存可显式停止的 child owner。
    child: Child,
}

// 为 fixture broker 提供启动、endpoint 发布等待与停止。
impl BrokerProcess {
    // 从固定 sibling 路径启动无参数 fixture broker。
    fn start(image: &Path) -> Self {
        // 创建不继承测试 stdin 或 stderr 的 broker 命令。
        let mut command = Command::new(image);
        // broker 不接受任何 argv、环境或路径覆盖。
        command.stdin(Stdio::null());
        // 捕获仅在失败时产生的安全 stdout。
        command.stdout(Stdio::piped());
        // 生产 broker 不应污染测试 stderr。
        command.stderr(Stdio::null());
        // 启动固定 fixture broker。
        let child = command
            // 不传入任何选项。
            .spawn()
            // 固定 fixture broker 必须可启动。
            .must("fixture broker should start from fixed sibling directory");
        // 保存 child owner。
        let mut process = Self { child };
        // 在 command launcher 连接前确认 endpoint 已发布。
        process.wait_until_listening();
        // 返回已发布 endpoint 的 broker。
        process
    }

    // 等待当前 session 的固定 endpoint 发布并拒绝提前退出。
    fn wait_until_listening(&mut self) {
        // 读取当前测试进程 PID。
        let process_id = unsafe { GetCurrentProcessId() };
        // 初始化 native session 输出。
        let mut session_id = 0_u32;
        // 查询当前测试进程对应的 native session。
        unsafe { ProcessIdToSessionId(process_id, &mut session_id) }
            // 当前测试环境必须存在可认证 session。
            .must("test session should be available");
        // 构造冻结 endpoint 名称而不接受测试覆盖。
        let name = format!(r"\\.\pipe\ai-computer-toolkit-browser-session-v1-{session_id}");
        // 编码为 Windows API 所需的 NUL 结尾 UTF-16。
        let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        // 设置 broker 发布总预算。
        let deadline = Instant::now() + Duration::from_secs(5);
        // 短轮询 endpoint 与 child 生命周期。
        loop {
            // broker 先退出表示没有成功拥有 first-instance guard。
            assert!(
                self.child
                    // 非阻塞读取 child 状态。
                    .try_wait()
                    // 进程状态查询必须成功。
                    .must("fixture broker status should be queryable")
                    // None 表示 broker 仍在运行。
                    .is_none(),
                // 保持失败原因不含路径、PID 或 pipe 值。
                "fixture broker exited before publishing endpoint",
            );
            // 让内核在短片内等待 endpoint 可连接。
            if unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), 10) }.as_bool() {
                // endpoint 已由 fixture broker 发布。
                return;
            }
            // 发布预算耗尽即拒绝继续测试。
            assert!(
                Instant::now() < deadline,
                // 不泄漏具体 endpoint。
                "fixture broker endpoint startup timed out",
            );
            // 限制测试轮询 CPU。
            thread::sleep(Duration::from_millis(10));
        }
    }

    // 查询测试拥有的 broker 是否仍在运行而不公开进程身份。
    fn is_running(&mut self) -> bool {
        // None 表示 child 尚未退出。
        self.child
            // 非阻塞读取同一 child owner 的状态。
            .try_wait()
            // 测试必须能查询自有进程。
            .must("fixture broker status should remain queryable")
            // 只返回生命周期布尔值。
            .is_none()
    }

    // 终止当前测试创建的 broker 并等待其释放 first-instance guard。
    fn stop(mut self) {
        // 测试拥有该子进程，因此允许终止。
        let _ = self.child.kill();
        // 等待内核资源与 endpoint handle 释放。
        let _ = self.child.wait();
    }
}

// 测试异常退出时也回收当前测试创建的 broker。
impl Drop for BrokerProcess {
    // 尽力终止仍存活的 broker child。
    fn drop(&mut self) {
        // 不触碰任何非本测试创建的进程。
        let _ = self.child.kill();
        // 避免留下固定 endpoint owner。
        let _ = self.child.wait();
    }
}

// 把 production broker、认证 launcher 与无参数 worker 替身复制成固定 sibling 集合。
fn install_siblings(directory: &TestDirectory) -> (PathBuf, PathBuf, PathBuf) {
    // 定位 fixture broker 的生产固定名称。
    let broker = directory.file(BROKER_FILE_NAME);
    // 定位 command fixture 的认证主程序名称。
    let command = directory.file(COMMAND_FILE_NAME);
    // 定位 production executor 的固定 worker 名称。
    let worker = directory.file(WORKER_FILE_NAME);
    // 复制生产 broker 到固定 broker sibling 名称。
    fs::copy(BROKER_SOURCE, &broker).must("production broker sibling copy should succeed");
    // 复制 command fixture 到认证要求的主程序名称。
    fs::copy(COMMAND_SOURCE, &command).must("command fixture sibling copy should succeed");
    // 复制无参数生产 worker 替身到生产固定 worker sibling 名称。
    fs::copy(WORKER_SOURCE, &worker).must("production worker fixture sibling copy should succeed");
    // 返回 broker、launcher 与 test-only worker 见证所需的精确镜像路径。
    (broker, command, worker)
}

// 通过全新 command launcher stdin 执行一个严格 JSON 请求。
fn run_launcher(image: &Path, request: Value) -> Output {
    // 启动固定 command fixture。
    let mut child = Command::new(image)
        // 为本次 launcher 保留唯一 stdin。
        .stdin(Stdio::piped())
        // 捕获唯一 JSON 输出。
        .stdout(Stdio::piped())
        // 捕获 stderr 并在输出投影前证明其为空。
        .stderr(Stdio::piped())
        // 启动独立 launcher 进程。
        .spawn()
        // launcher 必须可创建。
        .must("command launcher should start");
    // 严格序列化唯一 stdin 文档。
    let input = serde_json::to_vec(&request).must("command request should serialize");
    // 取得 child 独占 stdin。
    let mut stdin = child
        .stdin
        .take()
        .must("command launcher should expose stdin");
    // 写入完整 JSON 文档。
    stdin
        .write_all(&input)
        // stdin 写入必须成功。
        .must("command launcher should receive request");
    // EOF 是 fixture 单文档输入边界的一部分。
    drop(stdin);
    // 等待 launcher 返回唯一 stdout envelope。
    let output = child
        .wait_with_output()
        // launcher 必须可被等待。
        .must("command launcher should exit");
    // 每个独立 launcher 都不得产生 stderr 旁路诊断。
    assert!(
        output.stderr.is_empty(),
        "launcher stderr should remain empty"
    );
    // 返回已经验证零 stderr 的原始进程输出。
    output
}

// 把 launcher 输出解析成唯一 JSON envelope。
fn output_json(output: &Output) -> Value {
    // stdout 必须是单个 UTF-8 JSON 文档。
    serde_json::from_slice::<Value>(&output.stdout).must("launcher stdout should be JSON")
}

// 断言对象字段集合精确匹配冻结测试投影。
fn assert_exact_keys(value: &Value, expected: &[&str]) {
    // 顶层必须为 JSON 对象。
    let object = value
        .as_object()
        .must("fixture envelope should be an object");
    // 键数不得接受隐式输出扩展。
    assert_eq!(object.len(), expected.len());
    // 每个输出键都必须在冻结集合中。
    assert!(object.keys().all(|key| expected.contains(&key.as_str())));
}

// 递归验证输出不泄漏 worker、pipe、进程、epoch、nonce、profile 或路径事实。
fn assert_no_private_text(value: &Value) {
    // 递归检查所有 JSON 类型。
    match value {
        // 字符串值不得包含禁止的内部事实。
        Value::String(text) => {
            // 统一大小写以覆盖消息文本变化。
            let lower = text.to_ascii_lowercase();
            // session ID 是公开身份，以下片段仍不得出现。
            for forbidden in [
                // 禁止 worker 私有引用或实现名称。
                "w1:",
                "worker",
                "job",
                "stdio",
                // 禁止本地 transport 与 endpoint 事实。
                "pipe",
                "endpoint",
                "bse1:",
                "handle",
                // 禁止原生进程与路径事实。
                "pid",
                "path",
                // 禁止 broker correlation 与代际事实。
                "epoch",
                "nonce",
                "fingerprint",
                "revision",
                // 禁止浏览器调试与凭据事实。
                "profile",
                "websocket",
                "cdp",
                "credential",
            ] {
                // 禁止任何内部路由或身份泄漏。
                assert!(
                    !lower.contains(forbidden),
                    "fixture output leaked private text"
                );
            }
        }
        // 数组逐项检查。
        Value::Array(values) => {
            // 递归检查每个元素。
            for item in values {
                // 不放宽嵌套输出。
                assert_no_private_text(item);
            }
        }
        // 对象同时检查键和值。
        Value::Object(object) => {
            // 遍历每个输出字段。
            for (key, item) in object {
                // 字段名也属于输出契约。
                assert_no_private_text(&Value::String(key.clone()));
                // 检查字段值。
                assert_no_private_text(item);
            }
        }
        // 数字、布尔和 null 不携带文本路由事实。
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

// 从成功 open envelope 提取唯一公开 session identity。
fn open_session_id(output: &Output) -> String {
    // 解析 open 的唯一输出。
    let open = output_json(output);
    // 只提取冻结错误码用于失败定位，不展示消息或私有值。
    let failure_code = open
        // 失败 envelope 的 error 字段必须是对象。
        .get("error")
        // 只读取稳定 code。
        .and_then(|error| error.get("code"))
        // code 必须是公开字符串。
        .and_then(Value::as_str)
        // 缺失时使用不含输入的固定占位符。
        .unwrap_or("UNAVAILABLE");
    // open 必须完成而非只被 transport 接管。
    assert!(
        output.status.success(),
        "open launcher failed with {failure_code}"
    );
    // open 只允许 ok 与 result 字段。
    assert_exact_keys(&open, &["ok", "result"]);
    // open 必须明确成功。
    assert_eq!(open.get("ok").and_then(Value::as_bool), Some(true));
    // 读取唯一 result 对象。
    let result = open.get("result").must("open should return result");
    // result 只公开 session identity。
    assert_exact_keys(result, &["sessionId"]);
    // 读取 worker CNG 生成的公开 session ID。
    let session_id = result
        // 只接受字符串 identity。
        .get("sessionId")
        // 拒绝空值或其他 JSON 类型。
        .and_then(Value::as_str)
        // 缺失 identity 表示 fixture 违反公开结果契约。
        .must("open should return sessionId")
        // 保留用于后续独立 launcher。
        .to_owned();
    // session ID 必须保留唯一公开前缀与 canonical CNG hex 长度。
    assert!(session_id.starts_with("s2:bs:") && session_id.len() == 38);
    // open 不得泄漏私有 transport 实施细节。
    assert_no_private_text(&open);
    // 返回唯一公开 identity。
    session_id
}

// 验证独立 launcher 完成 open/inspect/close、stale inspect 与 broker restart 后 stale。
#[test]
fn production_broker_runs_open_inspect_close_and_restart_stale_through_real_transport() {
    // 创建不接触生产安装目录的测试 sibling 目录。
    let directory = TestDirectory::create();
    // 安装 production broker、认证 launcher 与 production worker 固定 sibling 替身。
    let (broker_image, command_image, worker_image) = install_siblings(&directory);
    // broker 启动前隔离目录不得已有 worker 残留。
    assert_no_worker_now(&worker_image);
    // 启动固定生产 broker host。
    let mut broker = BrokerProcess::start(&broker_image);
    // 记录首个 open 的单调起点，仅在失败时报告阶段耗时。
    let first_open_started = Instant::now();
    // 第一个独立 launcher 只发送 open。
    let open_output = run_launcher(
        // 使用固定认证 launcher。
        &command_image,
        // 本测试验证完成生命周期而非短 deadline 分支。
        json!({ "operation": "open", "timeoutMs": COMMAND_TIMEOUT_MS }),
    );
    // 失败时只补充 broker/worker 生命周期布尔事实定位执行阶段。
    if !open_output.status.success() {
        // 统计仅当前隔离目录的精确 worker 镜像。
        let worker_live = !matching_worker_processes(&worker_image).is_empty();
        // 保持失败诊断不含 PID、路径、pipe、epoch 或 nonce。
        assert!(
            open_output.status.success(),
            "open failed while broker_live={} worker_live={worker_live}",
            broker.is_running(),
        );
    }
    // 读取第一个 live 会话的公开 identity。
    let session_id = open_session_id(&open_output);
    // 保存首个 open 已完成时长，避免失败诊断依赖墙钟。
    let first_open_elapsed = first_open_started.elapsed();
    // 精确持有第一个 live worker 的内核进程对象。
    let first_worker =
        WorkerProcessWitness::find_exact_image(&worker_image, Duration::from_secs(5));
    // 第二个独立 launcher 只查询当前 live session。
    let inspect_output = run_launcher(
        // 使用固定认证 launcher。
        &command_image,
        // Query 只携带公开 target 与总预算。
        json!({ "operation": "inspect", "sessionId": session_id, "timeoutMs": COMMAND_TIMEOUT_MS }),
    );
    // live inspect 必须完成且不只返回 transport accepted。
    assert!(
        inspect_output.status.success(),
        "live session inspect should succeed"
    );
    // 解析只读 Query 的唯一输出。
    let inspect = output_json(&inspect_output);
    // inspect 成功只允许 ok 与 result。
    assert_exact_keys(&inspect, &["ok", "result"]);
    // 读取最小只读结果。
    let inspect_result = inspect.get("result").must("inspect should return result");
    // 结果只回显公开 session identity 与 live 事实。
    assert_exact_keys(inspect_result, &["sessionId", "live"]);
    // Query 必须逐字返回原公开 target。
    assert_eq!(
        inspect_result.get("sessionId").and_then(Value::as_str),
        Some(session_id.as_str())
    );
    // 当前 registry 目标必须报告 live。
    assert_eq!(
        inspect_result.get("live").and_then(Value::as_bool),
        Some(true)
    );
    // Query 结果不得泄漏 broker、worker、pipe 或协议内部事实。
    assert_no_private_text(&inspect);
    // Query 前后必须仍是同一个内核 worker 对象。
    assert!(
        first_worker.is_live(),
        "inspect must not terminate the live worker"
    );
    // 精确镜像清单必须保持一个且仅一个 worker。
    assert_eq!(
        matching_worker_processes(&worker_image).len(),
        1,
        "inspect must not create another worker"
    );
    // 记录 close 的单调起点，仅在失败时报告阶段耗时。
    let first_close_started = Instant::now();
    // 第三个全新 launcher 只发送对应 close。
    let close_output = run_launcher(
        &command_image,
        json!({ "operation": "close", "sessionId": session_id, "timeoutMs": COMMAND_TIMEOUT_MS }),
    );
    // 解析 close 唯一输出。
    let close = output_json(&close_output);
    // 只提取冻结错误码用于失败定位。
    let close_failure_code = close
        // 读取可选 error 对象。
        .get("error")
        // 读取公开稳定 code。
        .and_then(|error| error.get("code"))
        // code 必须为字符串。
        .and_then(Value::as_str)
        // 成功路径使用固定占位符。
        .unwrap_or("UNAVAILABLE");
    // close 失败时只补充 broker、worker 与单调耗时事实定位生命周期阶段。
    if !close_output.status.success() {
        // 统计仅当前隔离目录的精确 worker 镜像。
        let worker_live = !matching_worker_processes(&worker_image).is_empty();
        // 报告不含 PID、路径、pipe、epoch 或 nonce 的固定诊断。
        panic!(
            "close failed with {close_failure_code}; broker_live={} worker_live={worker_live} open_ms={} close_ms={}",
            // 仅观察测试拥有的 broker 子进程。
            broker.is_running(),
            // 单调时长只用于区分业务与 transport deadline 阶段。
            first_open_elapsed.as_millis(),
            // close 时长同样不携带任何私有 identity。
            first_close_started.elapsed().as_millis(),
        );
    }
    // close 只允许 ok 与 result 字段。
    assert_exact_keys(&close, &["ok", "result"]);
    // close 必须明确成功。
    assert_eq!(close.get("ok").and_then(Value::as_bool), Some(true));
    // 读取 close result。
    let close_result = close.get("result").must("close should return result");
    // result 只公开关闭事实。
    assert_exact_keys(close_result, &["closed"]);
    // close 必须确认已经关闭。
    assert_eq!(
        close_result.get("closed").and_then(Value::as_bool),
        Some(true)
    );
    // close 不得泄漏私有 transport 实施细节。
    assert_no_private_text(&close);
    // close 必须使同一内核 worker 对象在有界时间内退出。
    assert!(
        first_worker.wait_exited(Duration::from_secs(5)),
        "close should reap its fixed browser worker"
    );
    // 退出事实完成后释放 test-only 进程 handle。
    drop(first_worker);
    // close 后隔离目录不得存在第二个或泄漏 worker。
    wait_for_no_worker(&worker_image, Duration::from_secs(5));
    // close 后全新 launcher 对同一 target 执行只读 inspect。
    let closed_stale_output = run_launcher(
        // 使用固定认证 launcher。
        &command_image,
        // 只传已关闭的公开 identity。
        json!({ "operation": "inspect", "sessionId": session_id, "timeoutMs": COMMAND_TIMEOUT_MS }),
    );
    // 已关闭目标必须在 accepted 前确定拒绝。
    assert!(
        !closed_stale_output.status.success(),
        "closed session inspect should be stale"
    );
    // 解析 stale Query 的安全错误。
    let closed_stale = output_json(&closed_stale_output);
    // 只核对唯一公开错误码。
    assert_eq!(
        closed_stale.pointer("/error/code").and_then(Value::as_str),
        Some("STALE_SESSION")
    );
    // stale Query 不得泄漏私有传输或生命周期事实。
    assert_no_private_text(&closed_stale);
    // stale Query 不得启动任何 worker。
    assert_no_worker_now(&worker_image);
    // 后续全新 launcher 打开仍 live 的第二个 session。
    let second_open_output = run_launcher(
        &command_image,
        json!({ "operation": "open", "timeoutMs": COMMAND_TIMEOUT_MS }),
    );
    // 保存只由旧 broker epoch 签发的 live session identity。
    let second_session_id = open_session_id(&second_open_output);
    // 两次独立 CNG session identity 不得意外重用。
    assert_ne!(
        second_session_id, session_id,
        "independent opens should issue different session identities"
    );
    // 精确持有第二个 live worker 的内核进程对象。
    let second_worker =
        WorkerProcessWitness::find_exact_image(&worker_image, Duration::from_secs(5));
    // 强制终止旧 broker 仅模拟崩溃与代际重启，不冒充有序 shutdown。
    broker.stop();
    // broker 崩溃后固定 worker 不得成为残留进程。
    assert!(
        second_worker.wait_exited(Duration::from_secs(5)),
        "broker crash should leave no live fixed browser worker"
    );
    // 退出事实完成后释放 test-only 进程 handle。
    drop(second_worker);
    // 进程清单必须同样收敛为零匹配。
    wait_for_no_worker(&worker_image, Duration::from_secs(5));
    // 启动新的 broker epoch 并确认旧 endpoint owner 已释放。
    let replacement = BrokerProcess::start(&broker_image);
    // 新代际全新 launcher 用旧 epoch 的 live ID 执行 inspect。
    let stale_output = run_launcher(
        &command_image,
        json!({ "operation": "inspect", "sessionId": second_session_id, "timeoutMs": COMMAND_TIMEOUT_MS }),
    );
    // restart stale inspect 必须返回结构化失败。
    assert!(!stale_output.status.success(), "stale inspect should fail");
    // 解析 stale 唯一输出。
    let stale = output_json(&stale_output);
    // stale 输出只允许 ok 与 error 字段。
    assert_exact_keys(&stale, &["ok", "error"]);
    // stale 必须明确失败。
    assert_eq!(stale.get("ok").and_then(Value::as_bool), Some(false));
    // 读取冻结 error envelope。
    let error = stale.get("error").must("stale inspect should return error");
    // 统一 error envelope 只允许 code 与 message。
    assert_exact_keys(error, &["code", "message"]);
    // 错误必须为唯一公开 stale 语义。
    assert_eq!(
        error.get("code").and_then(Value::as_str),
        Some("STALE_SESSION")
    );
    // stale 输出同样不得泄漏私有 transport 实施细节。
    assert_no_private_text(&stale);
    // stale inspect 不得为旧 identity 启动新的 worker。
    assert_no_worker_now(&worker_image);
    // 再用独立 launcher 保留旧验收中的 restart stale close 证据。
    let stale_close_output = run_launcher(
        // 使用同一固定认证 launcher。
        &command_image,
        // close 只绑定旧代际公开 identity。
        json!({ "operation": "close", "sessionId": second_session_id, "timeoutMs": COMMAND_TIMEOUT_MS }),
    );
    // restart stale close 必须保持确定失败。
    assert!(
        !stale_close_output.status.success(),
        "stale close should fail"
    );
    // 解析 stale close 的唯一安全输出。
    let stale_close = output_json(&stale_close_output);
    // close 必须保留公开 STALE_SESSION 语义。
    assert_eq!(
        stale_close.pointer("/error/code").and_then(Value::as_str),
        Some("STALE_SESSION")
    );
    // stale close 同样不得泄漏私有 transport 或生命周期事实。
    assert_no_private_text(&stale_close);
    // stale close 不得为旧 target 启动 worker。
    assert_no_worker_now(&worker_image);
    // 显式停止测试拥有的 replacement broker。
    replacement.stop();
    // 所有进程退出后隔离 sibling 目录必须可确定删除。
    directory.finish();
}

// 加载真实 worker 页面读取纵切的独立集成回归。
#[path = "../support/browser_session_broker_page_reads.rs"]
mod page_reads;
// 加载真实 worker 页面动作与截图纵切的独立集成回归。
#[path = "../support/browser_session_broker_page_actions.rs"]
mod page_actions;
