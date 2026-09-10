#![cfg(target_os = "windows")]

//! 验证 production broker 的 raw 协议恢复场景与精确 worker 生命周期。

// 导入 Windows 完整镜像枚举、隔离文件、进程与单调等待工具。
use std::{
    // 导入 Windows UTF-16 完整路径转换。
    ffi::OsString,
    // 导入测试副本创建、复制与回收。
    fs,
    // 导入 Windows 进程快照结构长度。
    mem::size_of,
    // 导入 Windows UTF-16 路径转换扩展。
    os::windows::ffi::OsStringExt,
    // 导入固定 sibling 与镜像路径。
    path::{Path, PathBuf},
    // 导入无 argv fixture 与 broker 子进程控制。
    process::{Child, Command, Output, Stdio},
    // 导入 endpoint 发布与 worker 采样等待。
    thread,
    // 导入测试唯一目录与总时限。
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// 导入唯一 stdout envelope 的 JSON 解析类型。
use serde_json::Value;
// 导入只读 endpoint 等待、进程快照与完整镜像查询 API。
use windows::{
    // 导入测试 handle 关闭与快照终止错误。
    Win32::{
        // 导入 Windows handle 基础类型。
        Foundation::{
            // 导入 handle 关闭、快照终止与进程等待状态。
            CloseHandle,
            ERROR_NO_MORE_FILES,
            HANDLE,
            WAIT_OBJECT_0,
            WAIT_TIMEOUT,
        },
        // 导入 named pipe、当前 session 与进程枚举 API。
        System::{
            // 导入精确完整镜像的进程快照 API。
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            // 导入 broker 监听等待 API。
            Pipes::WaitNamedPipeW,
            // 导入当前测试 Windows session 查询。
            RemoteDesktop::ProcessIdToSessionId,
            // 导入最小权限进程镜像查询 API。
            Threading::{
                GetCurrentProcessId, GetProcessId, OpenProcess, PROCESS_ACCESS_RIGHTS,
                PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, WaitForSingleObject,
            },
        },
    },
    // 导入 Windows UTF-16 指针视图。
    core::{PCWSTR, PWSTR},
};

// 固定 Cargo 构建出的真实 broker 镜像。
const BROKER_SOURCE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-broker");
// 固定 Cargo 构建出的无 argv raw protocol fixture 镜像。
const RAW_FIXTURE_SOURCE: &str =
    env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-raw-protocol-fixture");
// 固定 Cargo 构建出的 ready production worker fixture 镜像。
const WORKER_SOURCE: &str =
    env!("CARGO_BIN_EXE_ai-computer-toolkit-browser-session-production-worker-fixture");
// 固定 broker sibling 文件名。
const BROKER_FILE_NAME: &str = "ai-computer-toolkit-browser-session-broker.exe";
// 固定认证 client sibling 文件名。
const CLIENT_FILE_NAME: &str = "ai-computer-toolkit.exe";
// 固定生产 worker sibling 文件名。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-browser-session-worker.exe";
// 固定完整镜像缓冲区上限。
const MAXIMUM_IMAGE_PATH_UNITS: usize = 32_768;
// 固定 sampler 间隔以覆盖短暂断连恢复窗口。
const SAMPLE_INTERVAL: Duration = Duration::from_millis(3);
// 固定 fixture 全部协议场景的总预算。
const FIXTURE_TIMEOUT: Duration = Duration::from_secs(30);
// 固定仅供精确 worker witness 等待的同步权限位。
const PROCESS_SYNCHRONIZE: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0010_0000);

// 为 Result 提供不泄漏平台错误的测试上下文。
trait Must<T> {
    // 成功时返回值，失败时固定终止测试。
    fn must(self, context: &str) -> T;
}

// 为任意可调试错误的 Result 实现测试提取。
impl<T, E: std::fmt::Debug> Must<T> for Result<T, E> {
    // 把调用点的安全上下文与底层调试信息一并保留在本地测试失败中。
    fn must(self, context: &str) -> T {
        // 成功透传，失败停止当前验收。
        self.unwrap_or_else(|error| panic!("{context}: {error:?}"))
    }
}

// 为 Option 提供不需要伪造错误的测试提取。
impl<T> Must<T> for Option<T> {
    // 有值时返回，缺失时固定失败。
    fn must(self, context: &str) -> T {
        // 只保留测试约束，不投影未知 native 事实。
        self.unwrap_or_else(|| panic!("{context}"))
    }
}

// 保存当前测试独占的隔离 sibling 目录。
struct TestDirectory {
    // 保存可显式完成回收的唯一目录路径。
    path: Option<PathBuf>,
}

// 为隔离目录提供固定 sibling 定位与确定回收。
impl TestDirectory {
    // 创建不接触生产安装目录的空目录。
    fn create() -> Self {
        // 读取时钟以避免并发测试目录冲突。
        let timestamp = SystemTime::now()
            // 测试时钟必须在 Unix epoch 之后。
            .duration_since(UNIX_EPOCH)
            // 时钟异常不能安全构造可回收目录。
            .must("system clock should be after Unix epoch")
            // 使用纳秒缩小同一进程并发冲突概率。
            .as_nanos();
        // 构造仅含当前测试 PID 与时间的临时目录。
        let path = std::env::temp_dir().join(format!(
            // 保持目录名前缀不含被测协议私有事实。
            "ai-computer-toolkit-browser-session-raw-e2e-{}-{timestamp}",
            // PID 仅用于本地目录唯一性。
            std::process::id(),
        ));
        // 创建精确的新目录。
        fs::create_dir(&path).must("raw protocol isolated directory should be created");
        // 返回唯一目录 owner。
        Self { path: Some(path) }
    }

    // 返回固定 sibling 的绝对路径。
    fn file(&self, name: &str) -> PathBuf {
        // 目录在显式 finish 前必须仍存在。
        self.path
            // 拒绝 finish 后继续使用测试资源。
            .as_ref()
            // 路径丢失表示测试生命周期错误。
            .must("raw protocol isolated directory should remain live")
            // 拼接生产路由冻结的固定 sibling 名称。
            .join(name)
    }

    // 在所有 owned child 回收后确定删除目录。
    fn finish(mut self) {
        // 取得并清空唯一目录 owner。
        let path = self
            // 转移路径避免 Drop 再次删除。
            .path
            // 只允许完成一次。
            .take()
            // 缺失路径表示重复完成。
            .must("raw protocol isolated directory should finish once");
        // 目录必须无泄漏句柄才能被完整回收。
        fs::remove_dir_all(path).must("raw protocol isolated directory should be removable");
    }
}

// 在异常展开时尽力回收本测试创建的目录。
impl Drop for TestDirectory {
    // 不遮蔽原始失败的尽力回收。
    fn drop(&mut self) {
        // 仅在 finish 尚未转移路径时执行回收。
        if let Some(path) = self.path.take() {
            // 异常路径只尽力删除。
            let _ = fs::remove_dir_all(path);
        }
    }
}

// 让测试进程快照与候选进程 handle 自动关闭。
struct TestHandle(HANDLE);

// 为有效 Windows handle 提供窄所有权包装。
impl TestHandle {
    // 接管有效 handle，拒绝无效 sentinel。
    fn new(handle: HANDLE) -> Option<Self> {
        // 只有有效 handle 才能进入 RAII owner。
        (!handle.is_invalid()).then_some(Self(handle))
    }

    // 返回仅供本测试 Windows API 使用的裸 handle。
    const fn raw(&self) -> HANDLE {
        // handle 不跨出测试进程。
        self.0
    }
}

// 确保测试 handle 在每条退出路径关闭。
impl Drop for TestHandle {
    // 关闭唯一拥有的 Windows handle。
    fn drop(&mut self) {
        // 关闭失败不改变此前快照结论。
        let _ = unsafe { CloseHandle(self.0) };
    }
}

// 查找唯一仍 live 的精确复制 worker，并把内核对象作为本轮采样见证。
fn exact_live_worker(expected: &Path) -> Option<TestHandle> {
    // 规范化本测试刚复制的镜像路径。
    let expected = fs::canonicalize(expected).must("fixed raw worker image should exist");
    // 创建当前 Windows 进程的只读快照。
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        // 本地测试环境必须允许读取同用户进程清单。
        .must("raw worker process snapshot should be available");
    // 接管快照 handle。
    let snapshot = TestHandle::new(snapshot).must("raw worker process snapshot should be valid");
    // 初始化 Windows 规定的快照条目长度。
    let mut entry = PROCESSENTRY32W {
        // 把 Rust 结构长度安全投影为 Win32 字段。
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>())
            // 平台结构大小必须适合 u32。
            .must("raw worker snapshot entry size should fit u32"),
        // 其余字段保持 Windows 零初始化。
        ..Default::default()
    };
    // 快照至少必须包含当前测试进程。
    unsafe { Process32FirstW(snapshot.raw(), &mut entry) }
        // 无首项不能证明生命周期。
        .must("raw worker process snapshot should contain entries");
    // 保存唯一精确 worker 的 handle-bound 见证。
    let mut found = None;
    // 遍历快照至 Windows 明确结束。
    loop {
        // 单个候选只在镜像完全匹配、未退出且 PID 未复用时接管。
        if let Some(worker) = entry_is_exact_live_worker(&entry, &expected) {
            // 同一固定镜像同时出现两个 worker 即违反唯一派发契约。
            assert!(
                found.is_none(),
                "raw fixture should not run more than one fixed worker"
            );
            // 保留唯一候选，直到本次采样结束或进入连续段 witness。
            found = Some(worker);
        }
        // 推进到下一条快照记录。
        if let Err(error) = unsafe { Process32NextW(snapshot.raw(), &mut entry) } {
            // 只有自然无更多条目才可接受。
            assert_eq!(
                error.code(),
                ERROR_NO_MORE_FILES.to_hresult(),
                "raw worker snapshot ended unexpectedly"
            );
            // 快照已完整遍历。
            break;
        }
    }
    // 返回不暴露 PID 或路径的可选内核对象。
    found
}

// 为精确镜像快照候选建立仍 live 的 handle-bound witness。
fn entry_is_exact_live_worker(entry: &PROCESSENTRY32W, expected: &Path) -> Option<TestHandle> {
    // 用最小查询权限打开快照候选。
    let handle = match unsafe {
        OpenProcess(
            // 只请求完整镜像查询权限。
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            // 禁止子进程继承测试 handle。
            false,
            // PID 只用于取得同一内核对象。
            entry.th32ProcessID,
        )
    } {
        // 成功时继续绑定候选。
        Ok(handle) => handle,
        // 候选可能在快照后退出。
        Err(_) => return None,
    };
    // 接管有效的查询 handle。
    let Some(handle) = TestHandle::new(handle) else {
        // 无效 handle 不能证明匹配。
        return None;
    };
    // 拒绝快照后 PID 已复用的候选。
    if unsafe { GetProcessId(handle.raw()) } != entry.th32ProcessID {
        // PID 漂移不计入 worker。
        return None;
    }
    // 查询同一内核对象的完整镜像路径。
    let Some(image) = process_image(handle.raw()) else {
        // 退出或无权限候选不计入 worker。
        return None;
    };
    // 规范化当前镜像以消除短路径别名。
    let Ok(image) = fs::canonicalize(image) else {
        // 已退出候选不能建立匹配。
        return None;
    };
    // Windows 路径比较必须忽略大小写。
    let matches = image
        // 转为无损失时的文本视图。
        .to_string_lossy()
        // 与测试复制的完整镜像精确比较。
        .eq_ignore_ascii_case(&expected.to_string_lossy());
    // 路径不匹配不能成为本测试的 worker witness。
    if !matches {
        // 释放无关候选 handle。
        return None;
    }
    // 仅把仍未退出的内核对象计为 live worker。
    if unsafe { WaitForSingleObject(handle.raw(), 0) } != WAIT_TIMEOUT {
        // 快照竞态中的已退出候选不应形成虚假 live 段。
        return None;
    }
    // 转移同一内核对象以抵抗后续 PID 重用。
    Some(handle)
}

// 从已打开的进程对象读取有界完整镜像路径。
fn process_image(process: HANDLE) -> Option<PathBuf> {
    // 分配 Windows 文档允许的最大 UTF-16 路径容量。
    let mut buffer = vec![0_u16; MAXIMUM_IMAGE_PATH_UNITS];
    // 传入容量并由 Windows 回写实际长度。
    let mut length = u32::try_from(buffer.len()).ok()?;
    // 从同一内核对象查询默认 DOS 完整路径。
    unsafe {
        QueryFullProcessImageNameW(
            // 使用已验证的候选 handle。
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
    // 拒绝空路径或越界长度。
    if length == 0 || length > buffer.len() {
        // 不把不可信路径交给后续匹配。
        return None;
    }
    // 构造拥有型 Windows 路径。
    Some(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

// 保存当前测试创建的 production broker 子进程。
struct BrokerProcess {
    // 保存可显式终止的唯一 child owner。
    child: Child,
}

// 为 broker fixture 提供启动、endpoint 发布等待与停止。
impl BrokerProcess {
    // 从固定 sibling 启动无参数 production broker。
    fn start(image: &Path) -> Self {
        // 构造不继承 stdin/stdout/stderr 的 broker 命令。
        let child = Command::new(image)
            // broker 不读取测试 stdin。
            .stdin(Stdio::null())
            // broker 不得向测试 stdout 写入。
            .stdout(Stdio::null())
            // broker 不得向测试 stderr 写入。
            .stderr(Stdio::null())
            // 启动固定 sibling 镜像。
            .spawn()
            // 启动失败不能继续协议验收。
            .must("raw protocol broker should start");
        // 保存唯一 child owner。
        let mut process = Self { child };
        // 等待固定 endpoint 真正发布。
        process.wait_until_listening();
        // 返回已监听 broker。
        process
    }

    // 等待当前 session 的固定 endpoint 发布。
    fn wait_until_listening(&mut self) {
        // 查询当前测试所在 Windows session。
        let mut session_id = 0_u32;
        // 当前环境必须提供可认证 native session。
        unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session_id) }
            .must("raw protocol test session should be available");
        // 构造生产路由冻结的 endpoint 名称。
        let endpoint = format!(r"\\.\pipe\ai-computer-toolkit-browser-session-v1-{session_id}");
        // 编码为 Windows API 所需 NUL 结尾 UTF-16。
        let endpoint = endpoint.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        // 设置不会因轮询重置的发布 deadline。
        let deadline = Instant::now() + Duration::from_secs(5);
        // 短轮询 endpoint 与 broker 生命周期。
        loop {
            // broker 不得在发布前自行退出。
            assert!(
                self.child
                    .try_wait()
                    .must("raw protocol broker status should be queryable")
                    .is_none(),
                "raw protocol broker exited before publishing endpoint",
            );
            // 内核已有 endpoint 时继续测试。
            if unsafe { WaitNamedPipeW(PCWSTR(endpoint.as_ptr()), 10) }.as_bool() {
                // 固定 endpoint 已发布。
                return;
            }
            // 发布超时不能继续启动 raw fixture。
            assert!(
                Instant::now() < deadline,
                "raw protocol broker endpoint startup timed out"
            );
            // 限制轮询 CPU。
            thread::sleep(Duration::from_millis(10));
        }
    }

    // 终止本测试唯一拥有的 broker。
    fn stop(mut self) {
        // 当前测试拥有该 child，允许显式停止。
        let _ = self.child.kill();
        // 等待 endpoint handle 释放。
        let _ = self.child.wait();
    }
}

// 在异常路径上也回收本测试创建的 broker。
impl Drop for BrokerProcess {
    // 尽力终止仍 live 的 broker。
    fn drop(&mut self) {
        // 不触碰本测试之外的任何进程。
        let _ = self.child.kill();
        // 等待 child 避免句柄泄漏。
        let _ = self.child.wait();
    }
}

// 把 production broker、无 argv raw fixture 和 ready worker 复制成固定 sibling 集合。
fn install_siblings(directory: &TestDirectory) -> (PathBuf, PathBuf, PathBuf) {
    // 定位固定 broker sibling。
    let broker = directory.file(BROKER_FILE_NAME);
    // 定位认证路由要求的固定 client sibling。
    let client = directory.file(CLIENT_FILE_NAME);
    // 定位生产 worker sibling。
    let worker = directory.file(WORKER_FILE_NAME);
    // 安装生产 broker 镜像。
    fs::copy(BROKER_SOURCE, &broker).must("raw protocol broker sibling copy should succeed");
    // 安装隐藏 raw fixture 但保留 production client 文件名。
    fs::copy(RAW_FIXTURE_SOURCE, &client).must("raw protocol fixture sibling copy should succeed");
    // 安装 ready worker fixture 作为 production fixed worker。
    fs::copy(WORKER_SOURCE, &worker).must("raw protocol worker sibling copy should succeed");
    // 返回所有固定 sibling 绝对路径。
    (broker, client, worker)
}

// 保存连续 live 段当前 worker 的内核对象与已完成段数。
struct WorkerSampleTracker {
    // 保存当前连续段唯一 worker 的 handle-bound witness。
    current: Option<TestHandle>,
    // 保存已经开始的精确 worker 生命周期段数。
    segments: usize,
}

// 为采样器验证 worker 不会在同一 live 段被替换。
impl WorkerSampleTracker {
    // 创建尚未观察到任何 worker 的采样器。
    const fn new() -> Self {
        // 初始状态必须是无 worker、零生命周期段。
        Self {
            current: None,
            segments: 0,
        }
    }

    // 吸收一次精确 worker 快照并验证连续段的内核对象不变。
    fn observe(&mut self, next: Option<TestHandle>) {
        // 取出上一轮持有的唯一内核对象。
        match (self.current.take(), next) {
            // 连续 live 样本必须仍指向尚未退出的同一进程对象。
            (Some(previous), Some(current)) => {
                // 上一见证若已退出，说明同一段内发生 worker 替换。
                assert_eq!(
                    unsafe { WaitForSingleObject(previous.raw(), 0) },
                    WAIT_TIMEOUT,
                    "raw fixture replaced a fixed worker inside one live segment"
                );
                // 当前快照也必须仍是同一个 handle-bound PID。
                assert_eq!(
                    unsafe { GetProcessId(previous.raw()) },
                    unsafe { GetProcessId(current.raw()) },
                    "raw fixture changed fixed worker identity inside one live segment"
                );
                // 保留旧见证以持续抵抗 PID 重用，并释放本轮重复 handle。
                self.current = Some(previous);
            }
            // 一到零转换必须先观察到上一 worker 内核对象已经退出。
            (Some(previous), None) => {
                // 仅已退出对象允许结束当前 lifecycle 段。
                assert_eq!(
                    unsafe { WaitForSingleObject(previous.raw(), 0) },
                    WAIT_OBJECT_0,
                    "raw fixture removed a fixed worker before it exited"
                );
                // 丢弃已退出 witness，使下一个一成为新段。
                self.current = None;
            }
            // 零到一转换开始一个新的独立 worker 生命周期段。
            (None, Some(current)) => {
                // 记录此 worker 对应的独立 open 业务接受。
                self.segments += 1;
                // 持有内核对象直到观察到其退出。
                self.current = Some(current);
            }
            // 连续零保持无 worker，覆盖 cancel tombstone 的无派发窗口。
            (None, None) => {
                // 保持无 worker witness。
                self.current = None;
            }
        }
    }

    // 验证最后一个 live 段已经完整退出并返回精确段数。
    fn finish(mut self) -> usize {
        // 测试结束前不得仍持有 live worker。
        if let Some(previous) = self.current.take() {
            // fixture 返回前必须已使最后 worker 退出。
            assert_eq!(
                unsafe { WaitForSingleObject(previous.raw(), 0) },
                WAIT_OBJECT_0,
                "raw fixture ended before its fixed worker exited"
            );
        }
        // 返回已经由 0->1 转换验证的精确段数。
        self.segments
    }
}

// 启动不带 argv 和 stdin 的 raw fixture 并在运行期采样 worker witness。
fn run_raw_fixture_and_sample(image: &Path, worker: &Path) -> (Output, usize) {
    // 启动封闭 raw fixture，不注入任何输入、环境或路径覆盖。
    let mut child = Command::new(image)
        // fixture 协议禁止 stdin。
        .stdin(Stdio::null())
        // 捕获唯一安全 JSON stdout。
        .stdout(Stdio::piped())
        // 捕获 stderr 并在调用者验证其为空。
        .stderr(Stdio::piped())
        // 启动认证要求的固定 sibling 镜像。
        .spawn()
        // fixture 必须可以启动。
        .must("raw protocol fixture should start");
    // 保存覆盖所有四个真实场景的总 deadline。
    let deadline = Instant::now() + FIXTURE_TIMEOUT;
    // 创建持有内核对象的连续段验证器。
    let mut tracker = WorkerSampleTracker::new();
    // 持续采样至 fixture 完成。
    loop {
        // 记录不泄漏身份的当前精确 worker 内核对象。
        tracker.observe(exact_live_worker(worker));
        // 已退出时停止采样并读取唯一输出。
        if child
            .try_wait()
            .must("raw protocol fixture status should be queryable")
            .is_some()
        {
            // 取得 stdout、stderr 与退出码。
            let output = child
                .wait_with_output()
                .must("raw protocol fixture should be waitable");
            // 返回已经验证连续身份的精确 lifecycle 段数。
            return (output, tracker.finish());
        }
        // fixture 不能超出独立测试预算。
        assert!(Instant::now() < deadline, "raw protocol fixture timed out");
        // 每 2-5ms 采样一次 exact worker 镜像。
        thread::sleep(SAMPLE_INTERVAL);
    }
}

// 递归断言 fixture 输出不泄漏任何协议或本机私有事实。
fn assert_no_private_text(value: &Value) {
    // 按 JSON 形状逐层检查。
    match value {
        // 检查所有字符串值与字段名。
        Value::String(text) => {
            // 统一大小写覆盖错误文本变体。
            let lower = text.to_ascii_lowercase();
            // 禁止协议身份、进程、endpoint、浏览器与路径事实。
            for forbidden in [
                // 禁止 nonce 与 epoch 相关内部状态。
                "nonce",
                "epoch",
                "fingerprint",
                "revision",
                // 禁止固定 IPC 与进程身份。
                "pipe",
                "endpoint",
                "pid",
                "path",
                "worker",
                "w1:",
                // 禁止浏览器调试或个人配置事实。
                "profile",
                "cdp",
                "websocket",
                "stdio",
                "job",
            ] {
                // 任意内部词出现均违反公共 stdout 边界。
                assert!(
                    !lower.contains(forbidden),
                    "raw fixture output leaked private text"
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
            // 字段名同样属于公开契约。
            for (key, item) in object {
                // 把键按字符串纳入同一泄漏检查。
                assert_no_private_text(&Value::String(key.clone()));
                // 检查字段值。
                assert_no_private_text(item);
            }
        }
        // 非文本标量不携带上述私有事实。
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

// 验证无 argv raw fixture 覆盖真实 nonce、取消和 accepted 后恢复边界。
#[test]
fn production_broker_runs_raw_protocol_recovery_without_private_leaks() {
    // 创建不接触生产安装目录的隔离 sibling 目录。
    let directory = TestDirectory::create();
    // 安装生产 broker、raw fixture 与 ready worker。
    let (broker_image, fixture_image, worker_image) = install_siblings(&directory);
    // 启动前不得已有本测试 exact worker 残留。
    assert!(exact_live_worker(&worker_image).is_none());
    // 启动真实生产 broker。
    let broker = BrokerProcess::start(&broker_image);
    // 启动无 argv/无 stdin fixture 并同步采样精确 worker 生命周期。
    let (output, worker_segments) = run_raw_fixture_and_sample(&fixture_image, &worker_image);
    // fixture 必须不经 stderr 输出旁路诊断。
    assert!(
        output.stderr.is_empty(),
        "raw protocol fixture stderr should remain empty"
    );
    // 四组真实协议观察必须整体成功。
    assert!(
        output.status.success(),
        "raw protocol fixture should succeed"
    );
    // stdout 必须是唯一 JSON envelope。
    let value = serde_json::from_slice::<Value>(&output.stdout)
        .must("raw protocol fixture stdout should be JSON");
    // 顶层只允许冻结成功投影。
    let object = value
        .as_object()
        .must("raw protocol fixture envelope should be an object");
    // 不允许 stdout 合同添加隐式调试字段。
    assert_eq!(object.len(), 2);
    // 只允许 ok 与 result 两个顶层字段。
    assert!(object.contains_key("ok") && object.contains_key("result"));
    // fixture 必须明确报告完整成功。
    assert_eq!(value.get("ok").and_then(Value::as_bool), Some(true));
    // 读取闭合的公开结果对象。
    let result = value
        .get("result")
        .must("raw protocol fixture should return result");
    // 结果字段必须精确覆盖四项场景结论。
    let result_object = result
        .as_object()
        .must("raw protocol result should be an object");
    // 不允许协议 fixture 暴露额外实现细节。
    assert_eq!(result_object.len(), 5);
    // 五项物理场景均必须显式为真。
    for key in [
        // 同 request 同义重送只回放原终态。
        "sameRequestReplay",
        // 同 identity 异义语义被业务前拒绝。
        "semanticConflict",
        // 取消先到会阻止后到 target 执行。
        "cancelBeforeTarget",
        // accepted 后主动断连可在原预算内恢复。
        "acceptedDisconnectRecovery",
        // accepted 后短读超时可在原预算内恢复。
        "acceptedReadTimeoutRecovery",
    ] {
        // 每项必须是唯一公开布尔事实。
        assert_eq!(result.get(key).and_then(Value::as_bool), Some(true));
    }
    // stdout 递归禁止所有私有 transport、进程、路径和浏览器事实。
    assert_no_private_text(&value);
    // 取消 tombstone 不派发且三个真实 open 必须恰好形成三个完整 lifecycle 段。
    assert_eq!(
        worker_segments, 3,
        "raw fixture should expose exactly three separated worker lifecycles"
    );
    // fixture 结束时 exact worker 必须已回收。
    assert!(exact_live_worker(&worker_image).is_none());
    // 显式停止当前测试拥有的 broker。
    broker.stop();
    // broker 停止后仍不得遗留 worker。
    assert!(exact_live_worker(&worker_image).is_none());
    // 所有 child 退出后目录必须完整可删除。
    directory.finish();
}
