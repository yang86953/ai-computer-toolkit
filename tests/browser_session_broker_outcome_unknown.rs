#![cfg(target_os = "windows")]

//! 验证 accepted-only worker 经 production broker 在总 deadline 后保守返回 OutcomeUnknown。

// 导入 Windows 镜像枚举、隔离 sibling、进程与单调采样工具。
use std::{
    // 导入 Windows UTF-16 镜像路径转换。
    ffi::OsString,
    // 导入隔离目录与 sibling 复制 API。
    fs,
    // 导入 Windows 快照结构长度。
    mem::size_of,
    // 导入 Windows UTF-16 路径转换扩展。
    os::windows::ffi::OsStringExt,
    // 导入固定 sibling 路径类型。
    path::{Path, PathBuf},
    // 导入 fixture 与 broker 子进程控制类型。
    process::{Child, Command, Output, Stdio},
    // 导入 endpoint 与进程采样等待工具。
    thread,
    // 导入唯一目录与 fixture 总 deadline。
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// 导入唯一安全 stdout envelope 的 JSON 类型。
use serde_json::{Value, json};
// 导入只读 endpoint、快照、镜像与内核对象 API。
use windows::{
    // 导入 handle 关闭、快照结束与进程等待常量。
    Win32::{
        // 导入基础 handle 类型与等待状态。
        Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        // 导入 pipe、session、进程快照和镜像查询 API。
        System::{
            // 导入完整进程快照 API。
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            // 导入固定 endpoint 就绪等待 API。
            Pipes::WaitNamedPipeW,
            // 导入当前 native session 查询 API。
            RemoteDesktop::ProcessIdToSessionId,
            // 导入只读进程镜像与等待 API。
            Threading::{
                GetCurrentProcessId, GetProcessId, OpenProcess, PROCESS_ACCESS_RIGHTS,
                PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, WaitForSingleObject,
            },
        },
    },
    // 导入 Windows UTF-16 指针视图。
    core::{PCWSTR, PWSTR},
};

// 固定 Cargo 构建出的 production broker 镜像。
const BROKER_SOURCE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-broker");
// 固定无 argv OutcomeUnknown fixture 源镜像。
const OUTCOME_UNKNOWN_FIXTURE_SOURCE: &str =
    env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-outcome-unknown-fixture");
// 固定 accepted-only worker fixture 源镜像。
const ACCEPTED_ONLY_WORKER_SOURCE: &str =
    env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-accepted-only-worker-fixture");
// 固定 production broker sibling 文件名。
const BROKER_FILE_NAME: &str = "ai-computer-toolkit-browser-session-broker.exe";
// 固定认证 client sibling 文件名。
const CLIENT_FILE_NAME: &str = "ai-computer-toolkit.exe";
// 固定 production worker sibling 文件名。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-browser-session-worker.exe";
// 固定完整镜像路径缓冲区上限。
const MAXIMUM_IMAGE_PATH_UNITS: usize = 32_768;
// 固定物理进程采样间隔。
const SAMPLE_INTERVAL: Duration = Duration::from_millis(3);
// 固定 fixture 客户端进程退出的总预算。
const FIXTURE_TIMEOUT: Duration = Duration::from_secs(30);
// 固定 fixture 内部已冻结的请求总 deadline。
const REQUEST_DEADLINE: Duration = Duration::from_secs(5);
// 固定 deadline 附近客户端返回的最晚允许时刻。
const REQUEST_DEADLINE_LATEST: Duration = Duration::from_secs(8);
// 固定 broker 协作取消与 Job 回收的独立清理预算。
const WORKER_REAP_TIMEOUT: Duration = Duration::from_secs(5);
// 固定仅供 worker witness 等待的进程同步权限。
const PROCESS_SYNCHRONIZE: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0010_0000);

// 为本地测试断言提供固定上下文提取。
trait Must<T> {
    // 成功时返回值，失败时停止当前测试。
    fn must(self, context: &str) -> T;
}

// 为可调试 Result 实现测试提取。
impl<T, E: std::fmt::Debug> Must<T> for Result<T, E> {
    // 将安全测试上下文与底层错误一起保留在本地失败中。
    fn must(self, context: &str) -> T {
        // 成功透传，失败只中止本地验收。
        self.unwrap_or_else(|error| panic!("{context}: {error:?}"))
    }
}

// 为 Option 实现不伪造平台错误的测试提取。
impl<T> Must<T> for Option<T> {
    // 有值时返回值，缺失时停止当前测试。
    fn must(self, context: &str) -> T {
        // 只保留测试不变量。
        self.unwrap_or_else(|| panic!("{context}"))
    }
}

// 保存当前测试独占的可回收 sibling 目录。
struct TestDirectory {
    // 保存 finish 前唯一拥有的目录路径。
    path: Option<PathBuf>,
}

// 为隔离目录提供固定 sibling 定位与确定回收。
impl TestDirectory {
    // 创建不接触生产安装目录的唯一目录。
    fn create() -> Self {
        // 读取时钟以避免并发测试目录冲突。
        let timestamp = SystemTime::now()
            // 当前测试时钟必须在 Unix epoch 后。
            .duration_since(UNIX_EPOCH)
            // 时钟异常不能安全构造目录。
            .must("system clock should be after Unix epoch")
            // 使用纳秒缩小同一 PID 冲突概率。
            .as_nanos();
        // 构造只含当前 PID 与时间的临时目录。
        let path = std::env::temp_dir().join(format!(
            // 保持目录名前缀不含被测协议私有事实。
            "ai-computer-toolkit-browser-session-outcome-unknown-e2e-{}-{timestamp}",
            // PID 仅用于本地目录唯一性。
            std::process::id(),
        ));
        // 创建精确的新目录。
        fs::create_dir(&path).must("outcome unknown isolated directory should be created");
        // 返回目录唯一 owner。
        Self { path: Some(path) }
    }

    // 返回固定 sibling 的绝对路径。
    fn file(&self, name: &str) -> PathBuf {
        // 目录在显式 finish 前必须存在。
        self.path
            // 禁止 finish 后继续使用测试资源。
            .as_ref()
            // 缺失表示测试生命周期错误。
            .must("outcome unknown isolated directory should remain live")
            // 拼接生产路由冻结的文件名。
            .join(name)
    }

    // 在所有 owned child 回收后确定删除目录。
    fn finish(mut self) {
        // 取走唯一目录 owner 以避免 Drop 重复删除。
        let path = self
            // 转移目录路径。
            .path
            // 只允许完成一次。
            .take()
            // 缺失表示重复完成。
            .must("outcome unknown isolated directory should finish once");
        // 目录必须无泄漏句柄才能完整回收。
        fs::remove_dir_all(path).must("outcome unknown isolated directory should be removable");
    }
}

// 在异常展开时尽力回收当前测试目录。
impl Drop for TestDirectory {
    // 不遮蔽先前的失败原因。
    fn drop(&mut self) {
        // 仅在 finish 未转移路径时回收。
        if let Some(path) = self.path.take() {
            // 异常路径只尽力删除。
            let _ = fs::remove_dir_all(path);
        }
    }
}

// 让快照和进程 witness handle 自动关闭。
struct TestHandle(HANDLE);

// 为有效 Windows handle 提供窄所有权包装。
impl TestHandle {
    // 接管有效 handle 并拒绝无效 sentinel。
    fn new(handle: HANDLE) -> Option<Self> {
        // 只有有效 handle 可成为 owner。
        (!handle.is_invalid()).then_some(Self(handle))
    }

    // 返回仅供本测试 Windows API 使用的裸 handle。
    const fn raw(&self) -> HANDLE {
        // handle 不越过本测试边界。
        self.0
    }
}

// 确保每条退出路径关闭唯一拥有的 handle。
impl Drop for TestHandle {
    // 关闭 Windows 内核 handle。
    fn drop(&mut self) {
        // 关闭失败不能改变此前的观察结论。
        let _ = unsafe { CloseHandle(self.0) };
    }
}

// 从已打开的进程对象读取有界完整镜像路径。
fn process_image(process: HANDLE) -> Option<PathBuf> {
    // 分配 Windows 文档允许的最大 UTF-16 容量。
    let mut buffer = vec![0_u16; MAXIMUM_IMAGE_PATH_UNITS];
    // 传入容量并由 Windows 回写实际长度。
    let mut length = u32::try_from(buffer.len()).ok()?;
    // 从同一内核对象查询默认 DOS 完整路径。
    unsafe {
        QueryFullProcessImageNameW(
            // 使用已验证的进程 handle。
            process,
            // 保持默认 Win32 路径形式。
            Default::default(),
            // 提供可写 UTF-16 缓冲区。
            PWSTR(buffer.as_mut_ptr()),
            // 提供并接收缓冲区长度。
            &mut length,
        )
    }
    // 进程退出或权限不足时拒绝候选。
    .ok()?;
    // 投影平台返回的 UTF-16 单元数量。
    let length = usize::try_from(length).ok()?;
    // 拒绝空路径和越界长度。
    if length == 0 || length > buffer.len() {
        // 不把不可信路径传给后续匹配。
        return None;
    }
    // 构造拥有型 Windows 路径。
    Some(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

// 为快照候选建立完整镜像匹配的 live 内核 witness。
fn entry_is_exact_live_worker(entry: &PROCESSENTRY32W, expected: &Path) -> Option<TestHandle> {
    // 以最小权限打开快照候选。
    let handle = unsafe {
        OpenProcess(
            // 只查询完整镜像并等待退出。
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            // 禁止子进程继承测试 handle。
            false,
            // PID 只用于取得同一内核对象。
            entry.th32ProcessID,
        )
    }
    // 快照后的已退出候选不进入采样。
    .ok()?;
    // 接管有效查询 handle。
    let handle = TestHandle::new(handle)?;
    // 拒绝快照后 PID 已复用的候选。
    if unsafe { GetProcessId(handle.raw()) } != entry.th32ProcessID {
        // PID 漂移不能成为 worker witness。
        return None;
    }
    // 查询同一内核对象的完整镜像路径。
    let image = process_image(handle.raw())?;
    // 规范化刚刚读取的镜像以消除短路径别名。
    let image = fs::canonicalize(image).ok()?;
    // Windows 路径比较必须忽略大小写。
    if !image
        // 转为无损失文本。
        .to_string_lossy()
        // 与本测试复制的完整镜像精确比较。
        .eq_ignore_ascii_case(&expected.to_string_lossy())
    {
        // 非本测试 worker 不能进入 witness。
        return None;
    }
    // 只有仍 live 的同一对象才计入当前样本。
    if unsafe { WaitForSingleObject(handle.raw(), 0) } != WAIT_TIMEOUT {
        // 已退出快照竞态不形成虚假 live 段。
        return None;
    }
    // 返回持有内核对象的精确 worker witness。
    Some(handle)
}

// 查找最多一个仍 live 的精确复制 worker。
fn exact_live_worker(expected: &Path) -> Option<TestHandle> {
    // 规范化测试刚复制的预期镜像。
    let expected = fs::canonicalize(expected).must("accepted-only worker image should exist");
    // 创建当前 Windows 的只读进程快照。
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        // 当前环境必须允许读取同用户进程清单。
        .must("accepted-only worker process snapshot should be available");
    // 接管快照 handle。
    let snapshot = TestHandle::new(snapshot).must("accepted-only worker snapshot should be valid");
    // 初始化 Windows 规定的快照条目长度。
    let mut entry = PROCESSENTRY32W {
        // 把 Rust 结构大小投影为 Windows 字段。
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>())
            // 平台结构大小必须适合 u32。
            .must("accepted-only worker snapshot entry size should fit u32"),
        // 其余字段保持 Windows 零初始化。
        ..Default::default()
    };
    // 快照至少必须包含当前测试进程。
    unsafe { Process32FirstW(snapshot.raw(), &mut entry) }
        // 无首项无法证明 worker 生命周期。
        .must("accepted-only worker snapshot should contain entries");
    // 保存本轮最多一个精确 worker witness。
    let mut found = None;
    // 遍历快照至 Windows 明确结束。
    loop {
        // 只接管完整镜像严格匹配的 live worker。
        if let Some(worker) = entry_is_exact_live_worker(&entry, &expected) {
            // 同一固定镜像并发两个 worker 违反唯一派发契约。
            assert!(
                found.is_none(),
                "accepted-only fixture should run no more than one fixed worker"
            );
            // 保存本轮唯一 worker。
            found = Some(worker);
        }
        // 推进到下一条快照记录。
        if let Err(error) = unsafe { Process32NextW(snapshot.raw(), &mut entry) } {
            // 只接受 Windows 自然无更多条目的结束。
            assert_eq!(
                error.code(),
                ERROR_NO_MORE_FILES.to_hresult(),
                "worker snapshot ended unexpectedly"
            );
            // 当前快照已完整遍历。
            break;
        }
    }
    // 返回本轮精确 worker witness 或零样本。
    found
}

// 保存连续 live 段的内核对象与物理峰值。
struct WorkerSampleTracker {
    // 保存当前 live 段的唯一 kernel witness。
    current: Option<TestHandle>,
    // 保存零到一转换次数。
    segments: usize,
    // 保存每轮精确 worker 数的最大值。
    maximum_live: usize,
}

// 为每三毫秒快照验证唯一 worker 生命周期。
impl WorkerSampleTracker {
    // 创建初始零 worker 采样器。
    const fn new() -> Self {
        // 初始状态必须是零段、零峰值和零 witness。
        Self {
            current: None,
            segments: 0,
            maximum_live: 0,
        }
    }

    // 吸收一次最多一个 worker 的快照。
    fn observe(&mut self, next: Option<TestHandle>) {
        // 记录本轮物理 worker 数。
        self.maximum_live = self.maximum_live.max(if next.is_some() { 1 } else { 0 });
        // 验证当前连续段的同一内核对象语义。
        match (self.current.take(), next) {
            // 连续 live 样本必须仍是同一个尚未退出对象。
            (Some(previous), Some(current)) => {
                // 上轮 witness 必须仍 live，否则段内发生替换。
                assert_eq!(
                    unsafe { WaitForSingleObject(previous.raw(), 0) },
                    WAIT_TIMEOUT,
                    "worker exited inside a live sample segment"
                );
                // 本轮 snapshot 必须为同一 kernel process。
                assert_eq!(
                    unsafe { GetProcessId(previous.raw()) },
                    unsafe { GetProcessId(current.raw()) },
                    "worker identity changed inside one live segment"
                );
                // 保留最早 witness 以抵抗 PID 重用。
                self.current = Some(previous);
            }
            // 一到零转换只能在 held witness 已退出后成立。
            (Some(previous), None) => {
                // worker 必须由 broker Job 回收而非只从快照消失。
                assert_eq!(
                    unsafe { WaitForSingleObject(previous.raw(), 0) },
                    WAIT_OBJECT_0,
                    "worker disappeared before its kernel process exited"
                );
                // 当前采样回到零 worker。
                self.current = None;
            }
            // 零到一转换开始唯一 lifecycle 段。
            (None, Some(current)) => {
                // 记录 accepted-only dispatch 的唯一物理段。
                self.segments += 1;
                // 持有对象以见证后续同一性和退出。
                self.current = Some(current);
            }
            // 连续零保持初始或回收后的无 worker 状态。
            (None, None) => {
                // 保持零 witness。
                self.current = None;
            }
        }
    }

    // 验证最终零状态并返回观察结论。
    fn finish(mut self) -> (usize, usize) {
        // 最后一段若仍持有 witness 则必须已经退出。
        if let Some(previous) = self.current.take() {
            // fixture 返回前 broker Job 必须已回收 worker。
            assert_eq!(
                unsafe { WaitForSingleObject(previous.raw(), 0) },
                WAIT_OBJECT_0,
                "fixture ended before broker Job reaped the worker"
            );
        }
        // 返回精确段数与采样峰值。
        (self.segments, self.maximum_live)
    }
}

// 保存当前测试创建的 production broker child。
struct BrokerProcess {
    // 保存可显式终止的唯一 child owner。
    child: Child,
}

// 为 broker 提供启动、endpoint 发布等待、存活和停止。
impl BrokerProcess {
    // 从固定 sibling 启动无参数 production broker。
    fn start(image: &Path) -> Self {
        // 启动不继承测试标准流的 broker。
        let child = Command::new(image)
            // broker 不读取测试 stdin。
            .stdin(Stdio::null())
            // broker 不得污染测试 stdout。
            .stdout(Stdio::null())
            // broker 不得污染测试 stderr。
            .stderr(Stdio::null())
            // 启动固定绝对 sibling。
            .spawn()
            // 启动失败不能继续 E2E。
            .must("outcome unknown broker should start");
        // 保存当前测试拥有的 child。
        let mut process = Self { child };
        // 等待固定 endpoint 真正发布。
        process.wait_until_listening();
        // 返回已监听 broker。
        process
    }

    // 等待当前 session 的固定 endpoint。
    fn wait_until_listening(&mut self) {
        // 查询当前测试的 native session。
        let mut session_id = 0_u32;
        // 当前环境必须提供可认证 session。
        unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session_id) }
            .must("outcome unknown test session should be available");
        // 构造 production 路由冻结的 endpoint 名称。
        let endpoint = format!(r"\\.\pipe\ai-computer-toolkit-browser-session-v1-{session_id}");
        // 编码为 Windows API 所需 NUL 结尾 UTF-16。
        let endpoint = endpoint.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        // 设置不会因轮询重置的启动 deadline。
        let deadline = Instant::now() + Duration::from_secs(5);
        // 轮询 endpoint 与 broker 生命周期。
        loop {
            // broker 发布前不得退出。
            assert!(
                self.child
                    .try_wait()
                    .must("outcome unknown broker status should be queryable")
                    .is_none(),
                "outcome unknown broker exited before endpoint publication"
            );
            // endpoint 已发布即可继续。
            if unsafe { WaitNamedPipeW(PCWSTR(endpoint.as_ptr()), 10) }.as_bool() {
                // 固定 endpoint 已就绪。
                return;
            }
            // 超过启动预算不能继续。
            assert!(
                Instant::now() < deadline,
                "outcome unknown broker endpoint startup timed out"
            );
            // 限制轮询 CPU。
            thread::sleep(Duration::from_millis(10));
        }
    }

    // 断言 broker 仍在运行但不推断关闭语义。
    fn assert_running(&mut self) {
        // fixture 返回后 broker 必须保持 live。
        assert!(
            self.child
                .try_wait()
                .must("outcome unknown broker status should be queryable")
                .is_none(),
            "broker should remain live after enforced worker reap"
        );
    }

    // 终止本测试唯一拥有的 broker。
    fn stop(mut self) {
        // 测试拥有 child，允许强制停止。
        let _ = self.child.kill();
        // 等待 broker handle 释放。
        let _ = self.child.wait();
    }
}

// 在异常路径上尽力停止本测试 broker。
impl Drop for BrokerProcess {
    // 不触碰本测试之外的任何进程。
    fn drop(&mut self) {
        // 尽力终止仍 live broker。
        let _ = self.child.kill();
        // 等待 child 避免 handle 泄漏。
        let _ = self.child.wait();
    }
}

// 复制 production broker、无 argv fixture 与 accepted-only worker 到固定 sibling。
fn install_siblings(directory: &TestDirectory) -> (PathBuf, PathBuf, PathBuf) {
    // 定位 fixed broker sibling。
    let broker = directory.file(BROKER_FILE_NAME);
    // 定位认证 client sibling。
    let client = directory.file(CLIENT_FILE_NAME);
    // 定位 production worker sibling。
    let worker = directory.file(WORKER_FILE_NAME);
    // 安装 production broker 原始镜像。
    fs::copy(BROKER_SOURCE, &broker).must("outcome unknown broker sibling copy should succeed");
    // 安装隐藏 fixture 但保持 production client 文件名。
    fs::copy(OUTCOME_UNKNOWN_FIXTURE_SOURCE, &client)
        .must("outcome unknown fixture sibling copy should succeed");
    // 安装 accepted-only fixture 但保持 production worker 文件名。
    fs::copy(ACCEPTED_ONLY_WORKER_SOURCE, &worker)
        .must("accepted-only worker sibling copy should succeed");
    // 返回所有固定绝对 sibling 路径。
    (broker, client, worker)
}

// 运行无 argv fixture 并在返回后继续采样至 broker Job 确认回收。
fn run_fixture_and_sample(image: &Path, worker: &Path) -> (Output, usize, usize, Duration) {
    // 启动封闭 fixture，不注入 argv、stdin、环境或路径覆盖。
    let mut child = Command::new(image)
        // fixture 协议禁止 stdin。
        .stdin(Stdio::null())
        // 捕获唯一安全 stdout envelope。
        .stdout(Stdio::piped())
        // 捕获并验证 stderr 为空。
        .stderr(Stdio::piped())
        // 启动认证要求的 production-name sibling。
        .spawn()
        // fixture 必须能够启动。
        .must("outcome unknown fixture should start");
    // 记录客户端自身返回耗时而不混入后续 Job 回收等待。
    let started = Instant::now();
    // 保存 fixture 客户端进程退出的独立预算。
    let deadline = Instant::now() + FIXTURE_TIMEOUT;
    // 创建初始零 worker 采样器。
    let mut tracker = WorkerSampleTracker::new();
    // 持续采样至 fixture 完成。
    loop {
        // 记录本轮完整镜像与 held kernel witness。
        tracker.observe(exact_live_worker(worker));
        // fixture 退出后读取唯一标准流输出。
        if child
            .try_wait()
            .must("outcome unknown fixture status should be queryable")
            .is_some()
        {
            // 提取退出码、stdout 与 stderr。
            let output = child
                .wait_with_output()
                .must("outcome unknown fixture should be waitable");
            // 记录客户端在原请求预算附近返回的时长。
            let elapsed = started.elapsed();
            // 设置不延长客户端请求语义的独立 Job 回收 deadline。
            let reap_deadline = Instant::now() + WORKER_REAP_TIMEOUT;
            // 客户端返回后仍持续物理采样，直至 held witness 已确认退出。
            loop {
                // 取得当前完整镜像的 live worker 样本。
                let worker = exact_live_worker(worker);
                // 记录零样本是否已经形成最终零状态。
                let reaped = worker.is_none();
                // 将本轮样本交给同一 kernel identity 跟踪器。
                tracker.observe(worker);
                // 只有真实零样本才完成 broker Job 回收见证。
                if reaped {
                    // worker 已由 broker Job 回收。
                    break;
                }
                // 清理超时不能被解释为客户端 wire 终态。
                assert!(
                    Instant::now() < reap_deadline,
                    "broker Job did not reap the accepted-only worker in the cleanup budget"
                );
                // 维持与执行期相同的物理采样频率。
                thread::sleep(SAMPLE_INTERVAL);
            }
            // 读取已经观察最终零样本后的 lifecycle 结论。
            let (segments, maximum_live) = tracker.finish();
            // 返回客户端输出、进程见证结论和仅客户端耗时。
            return (output, segments, maximum_live, elapsed);
        }
        // 不能超过 fixture 唯一总预算。
        assert!(
            Instant::now() < deadline,
            "outcome unknown fixture timed out"
        );
        // 严格维持约三毫秒物理采样间隔。
        thread::sleep(SAMPLE_INTERVAL);
    }
}

// 递归拒绝 stdout 中的 transport、进程、路径和 worker 私有事实。
fn assert_no_private_text(value: &Value) {
    // 按 JSON 形状递归检查。
    match value {
        // 检查所有字符串值。
        Value::String(text) => {
            // 统一大小写覆盖错误文本变体。
            let lower = text.to_ascii_lowercase();
            // 禁止所有非公开 IPC、进程与实现词。
            for forbidden in [
                "nonce",
                "epoch",
                "fingerprint",
                "revision",
                "pipe",
                "endpoint",
                "pid",
                "path",
                "worker",
                "profile",
                "cdp",
                "websocket",
                "stdio",
                "job",
            ] {
                // 任何私有词进入 stdout 都违反公共边界。
                assert!(
                    !lower.contains(forbidden),
                    "outcome unknown stdout leaked private text"
                );
            }
        }
        // 递归检查数组成员。
        Value::Array(items) => {
            // 不放宽嵌套输出。
            for item in items {
                // 检查当前成员。
                assert_no_private_text(item);
            }
        }
        // 递归检查对象键和值。
        Value::Object(object) => {
            // 字段名和字段值都属于 stdout 契约。
            for (key, item) in object {
                // 把对象键纳入同一文本检查。
                assert_no_private_text(&Value::String(key.clone()));
                // 检查对象值。
                assert_no_private_text(item);
            }
        }
        // 非文本 JSON 标量不承载禁用词。
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

// 验证 accepted-only deadline 只报告保守 OutcomeUnknown 并强制回收 Job。
#[test]
fn production_broker_reaps_accepted_only_worker_and_returns_safe_outcome_unknown() {
    // 创建当前测试独占的 sibling 目录。
    let directory = TestDirectory::create();
    // 安装 production broker、no-argv fixture 和 accepted-only worker。
    let (broker_image, fixture_image, worker_image) = install_siblings(&directory);
    // broker 启动前本测试 worker 镜像必须为零。
    assert!(
        exact_live_worker(&worker_image).is_none(),
        "worker should be absent before broker startup"
    );
    // 启动真实 production broker。
    let mut broker = BrokerProcess::start(&broker_image);
    // 运行 fixture 并观察完整物理 worker 生命周期。
    let (output, segments, maximum_live, elapsed) =
        run_fixture_and_sample(&fixture_image, &worker_image);
    // fixture 必须返回非成功状态。
    assert!(
        !output.status.success(),
        "accepted-only deadline fixture should fail safely"
    );
    // 客户端必须在自身五秒请求 deadline 附近返回，不等待 Job 回收无限延迟。
    assert!(
        elapsed >= REQUEST_DEADLINE.saturating_sub(Duration::from_secs(1))
            && elapsed <= REQUEST_DEADLINE_LATEST,
        "outcome unknown fixture should return near the request deadline"
    );
    // fixture 不得以 stderr 旁路输出诊断。
    assert!(
        output.stderr.is_empty(),
        "outcome unknown fixture stderr should remain empty"
    );
    // stdout 必须是唯一 JSON public envelope。
    let value = serde_json::from_slice::<Value>(&output.stdout)
        .must("outcome unknown fixture stdout should be JSON");
    // stdout 必须精确匹配冻结的安全未知结果。
    assert_eq!(
        value,
        json!({
            "ok": false,
            "error": {
                "code": "OUTCOME_UNKNOWN",
                "message": "The browser session broker command outcome is unknown after delivery began."
            }
        })
    );
    // stdout 不得泄漏 broker、worker 或 local transport 私有事实。
    assert_no_private_text(&value);
    // 初始零后必须恰好观察到一个零到一到零 worker 生命周期段。
    assert_eq!(
        segments, 1,
        "accepted-only fixture should expose exactly one worker lifecycle segment"
    );
    // 每轮完整镜像快照中最多只能存在一个 worker。
    assert_eq!(
        maximum_live, 1,
        "accepted-only fixture should expose a maximum of one live worker"
    );
    // 独立回收窗口结束后 broker Job 已完成 worker 回收。
    assert!(
        exact_live_worker(&worker_image).is_none(),
        "broker Job should reap the accepted-only worker"
    );
    // 强制回收后 broker 必须继续存活。
    broker.assert_running();
    // 显式停止当前测试拥有的 broker，不将其解释为有序关闭。
    broker.stop();
    // broker 停止后仍不得遗留 worker。
    assert!(
        exact_live_worker(&worker_image).is_none(),
        "worker should remain absent after broker stop"
    );
    // 所有 child 停止后隔离目录必须可删除。
    directory.finish();
}
