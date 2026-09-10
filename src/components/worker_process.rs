//! 在 Windows Job 中运行单次 JSON over stdio companion worker。

// 导入路径、线程与时间工具。
use std::{
    // 导入 worker 可执行文件路径类型。
    path::{Path, PathBuf},
    // 导入 reader 线程句柄。
    thread::{self, JoinHandle},
    // 导入 deadline 计算类型。
    time::{Duration, Instant},
};

// 导入 JSON 值。
use serde_json::Value;
// 导入 Windows API。
use windows::{
    // 导入所需 Win32 命名空间。
    Win32::{
        // 导入句柄、继承标志与 wait 结果。
        Foundation::{
            // 导入句柄关闭函数。
            CloseHandle,
            // 导入原生句柄类型。
            HANDLE,
            // 导入继承标志。
            HANDLE_FLAG_INHERIT,
            // 导入句柄继承控制函数。
            SetHandleInformation,
            // 导入 wait 状态。
            WAIT_FAILED,
            WAIT_OBJECT_0,
            WAIT_TIMEOUT,
        },
        // 导入 Windows 安全属性结构。
        Security::SECURITY_ATTRIBUTES,
        // 导入同步管道读写接口。
        Storage::FileSystem::{ReadFile, WriteFile},
        // 导入进程与 Job 子系统。
        System::{
            // 导入 Job 创建、配置、绑定和终止接口。
            JobObjects::{
                // 导入绑定函数。
                AssignProcessToJobObject,
                // 导入 Job 创建函数。
                CreateJobObjectW,
                // 导入关闭即终止限制。
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                // 导入 Job 限制结构。
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                // 导入扩展限制信息类别。
                JobObjectExtendedLimitInformation,
                // 导入 Job 配置函数。
                SetInformationJobObject,
                // 导入 Job 终止函数。
                TerminateJobObject,
            },
            // 导入匿名管道创建接口。
            Pipes::CreatePipe,
            // 导入进程创建与等待接口。
            Threading::{
                // 导入进程创建标志。
                CREATE_NO_WINDOW,
                CREATE_SUSPENDED,
                CREATE_UNICODE_ENVIRONMENT,
                // 导入无窗口与挂起创建标志。
                CreateProcessW,
                // 导入进程退出码接口。
                GetExitCodeProcess,
                // 导入进程创建结果结构。
                PROCESS_INFORMATION,
                // 导入恢复主线程函数。
                ResumeThread,
                // 导入标准句柄启动标志。
                STARTF_USESTDHANDLES,
                // 导入启动结构。
                STARTUPINFOW,
                // 导入 Job 绑定前异常路径的进程终止函数。
                TerminateProcess,
                // 导入等待函数。
                WaitForSingleObject,
            },
        },
    },
    // 导入宽字符串指针。
    core::{PCWSTR, PWSTR},
};

// 导入工具统一错误类型。
use crate::domain::{AppControlError, AppResult};

// 限制 worker stderr，任何非空内容都会使协议失败。
pub(super) const MAXIMUM_STDERR_BYTES: usize = 64 * 1024;
// 轮询间隔同时服务 deadline 与 cancellation。
const WAIT_SLICE_MS: u32 = 10;

// 表示 worker process Component 允许产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkerProcessErrorCode {
    // 表示调用方主动取消且 Job 已完成回收。
    Cancelled,
    // 表示固定 sibling companion 无法安全定位。
    CompanionUnavailable,
    // 表示 Component 调用参数违反硬边界。
    InvalidArgument,
    // 表示版本化请求无法序列化。
    SerializationFailed,
    // 表示 worker 超过单调 deadline 且已完成回收。
    Timeout,
    // 表示 worker 输出超过调用方硬上限。
    OutputTooLarge,
    // 表示 stdio framing、reader 或 JSON 协议失败。
    ProtocolFailed,
    // 表示管道、Job、进程或 reader 无法启动。
    StartFailed,
    // 表示 worker 或 Job 的等待与回收状态失败。
    WaitFailed,
}

// 提供 Component 私有错误码到公开协议文本的唯一映射。
impl WorkerProcessErrorCode {
    // 返回版本化公开错误码文本。
    const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有字符串逐字不变。
        match self {
            // 映射取消结果。
            Self::Cancelled => "CANCELLED",
            // 映射固定 companion 缺失。
            Self::CompanionUnavailable => "COMPANION_WORKER_UNAVAILABLE",
            // 映射参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射 JSON 序列化失败。
            Self::SerializationFailed => "SERIALIZATION_FAILED",
            // 映射单调 deadline 超时。
            Self::Timeout => "TIMEOUT",
            // 映射输出资源边界失败。
            Self::OutputTooLarge => "WORKER_OUTPUT_TOO_LARGE",
            // 映射 stdio 与 JSON 协议失败。
            Self::ProtocolFailed => "WORKER_PROTOCOL_FAILED",
            // 映射 worker 启动失败。
            Self::StartFailed => "WORKER_START_FAILED",
            // 映射等待与回收失败。
            Self::WaitFailed => "WORKER_WAIT_FAILED",
        }
    }

    // 使用当前封闭错误码构造统一公开错误。
    fn error(self, message: impl Into<String>) -> AppControlError {
        // 复用产品级 envelope 错误类型并隐藏 Component 私有枚举。
        AppControlError::new(self.as_str(), message)
    }
}

// 定位与当前主程序同目录的固定 companion worker。
pub(crate) fn sibling_companion_path(
    // 接收不含目录的固定文件名。
    file_name: &str,
    // 接收不含本机路径的安全描述。
    description: &str,
) -> AppResult<PathBuf> {
    // 读取当前主程序绝对路径。
    let executable = std::env::current_exe().map_err(|_| {
        // 不公开本机路径。
        WorkerProcessErrorCode::CompanionUnavailable.error(
            // 提供安全诊断。
            format!("The {description} executable location could not be resolved."),
        )
    })?;
    // 取得固定 sibling 目录。
    let directory = executable.parent().ok_or_else(|| {
        // 返回不含本机路径的结构化错误。
        WorkerProcessErrorCode::CompanionUnavailable.error(
            // 提供安全诊断。
            format!("The {description} executable has no companion directory."),
        )
    })?;
    // 仅拼接编译期固定 worker 文件名。
    let worker = directory.join(file_name);
    // 正式 sibling 存在时直接返回。
    if worker.is_file() {
        // 返回同目录固定 worker。
        return Ok(worker);
    }
    // Cargo integration test 位于 target/debug/deps，允许只读解析上一级固定构建产物。
    #[cfg(debug_assertions)]
    if directory.file_name().is_some_and(|name| name == "deps")
        // 只拼接同一个编译期固定文件名。
        && directory
            // 取得 target/debug。
            .parent()
            // 构造固定 worker 路径。
            .is_some_and(|parent| parent.join(file_name).is_file())
    {
        // 安全取得刚才已经证明存在的父目录。
        if let Some(parent) = directory.parent() {
            // 返回 Cargo 测试构建的固定 sibling 等价物。
            return Ok(parent.join(file_name));
        }
    }
    // 缺失 companion 必须显式失败。
    Err(WorkerProcessErrorCode::CompanionUnavailable.error(
        // 标明固定 worker 缺失。
        format!("The {description} companion is not installed beside the main executable."),
    ))
}

// 只读判断固定 sibling companion 是否当前可达。
pub(crate) fn sibling_companion_available(file_name: &str) -> bool {
    // 无法解析当前程序位置时按不可用 fail closed。
    let Ok(executable) = std::env::current_exe() else {
        // 返回不可用。
        return false;
    };
    // 缺少父目录时按不可用处理。
    let Some(directory) = executable.parent() else {
        // 返回不可用。
        return false;
    };
    // 只检查固定 sibling 文件是否存在。
    directory.join(file_name).is_file()
}

// 表示成功回收的 companion worker 结果。
pub(crate) struct WorkerOutput {
    // 保存 worker 退出码。
    pub(crate) exit_code: u32,
    // 保存已解析的单行 JSON 结果。
    pub(crate) envelope: Value,
}

// 让私有 Win32 handle 在所有返回路径上确定性关闭。
pub(super) struct OwnedHandle(HANDLE);

// 原生 handle 所有权可以安全移交给单个 reader 线程。
unsafe impl Send for OwnedHandle {}

// 提供 handle 所有权操作。
impl OwnedHandle {
    // 从有效 Win32 handle 建立所有权。
    pub(super) fn new(handle: HANDLE, operation: &str) -> AppResult<Self> {
        // 拒绝空句柄和 INVALID_HANDLE_VALUE。
        if handle.is_invalid() {
            // 返回不泄漏原生值的稳定错误。
            return Err(WorkerProcessErrorCode::StartFailed.error(
                // 标明失败步骤。
                format!("{operation} returned an invalid handle."),
            ));
        }
        // 接管有效句柄。
        Ok(Self(handle))
    }

    // 返回仅供 Component 内部 Win32 调用使用的复制句柄值。
    pub(super) const fn raw(&self) -> HANDLE {
        // HANDLE 是不可序列化的进程内值。
        self.0
    }
}

// 在作用域结束时关闭 Win32 handle。
impl Drop for OwnedHandle {
    // 回收内核资源。
    fn drop(&mut self) {
        // 句柄只由当前 OwnedHandle 关闭一次。
        let _ = unsafe { CloseHandle(self.0) };
    }
}

// 保存匿名管道的 parent 与 child 端点。
pub(super) struct PipePair {
    // 保存读取端。
    pub(super) read: OwnedHandle,
    // 保存写入端。
    pub(super) write: OwnedHandle,
}

// 建立允许 worker 端继承的匿名管道。
pub(super) fn inherited_pipe() -> AppResult<PipePair> {
    // 初始化读取端。
    let mut read = HANDLE::default();
    // 初始化写入端。
    let mut write = HANDLE::default();
    // 允许由调用方随后精确清除 parent 端继承。
    let attributes = SECURITY_ATTRIBUTES {
        // 设置结构长度。
        nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>()).map_err(|_| {
            // 返回结构长度溢出。
            WorkerProcessErrorCode::StartFailed.error("SECURITY_ATTRIBUTES size overflowed.")
        })?,
        // 不使用自定义安全描述符。
        lpSecurityDescriptor: std::ptr::null_mut(),
        // 初始允许句柄继承。
        bInheritHandle: true.into(),
    };
    // 创建同步匿名管道。
    unsafe { CreatePipe(&mut read, &mut write, Some(&raw const attributes), 0) }
        // 映射为稳定启动错误。
        .map_err(|error| worker_start_error("CreatePipe", error))?;
    // 接管读取端。
    let read = OwnedHandle::new(read, "CreatePipe(read)")?;
    // 接管写入端。
    let write = OwnedHandle::new(write, "CreatePipe(write)")?;
    // 返回完整端点对。
    Ok(PipePair { read, write })
}

// 把 Windows 错误限制在 Component 内并转换为公共错误。
pub(super) fn worker_start_error(
    // 接收失败步骤。
    operation: &str,
    // 接收原生错误。
    error: windows::core::Error,
) -> AppControlError {
    // 不公开句柄或命令行，只保留系统错误文本。
    WorkerProcessErrorCode::StartFailed.error(format!("{operation} failed: {error}"))
}

// 清除 parent 管道端继承标志。
pub(super) fn make_parent_only(handle: &OwnedHandle) -> AppResult<()> {
    // 只修改 HANDLE_FLAG_INHERIT 位。
    unsafe { SetHandleInformation(handle.raw(), HANDLE_FLAG_INHERIT.0, Default::default()) }
        // 映射为稳定启动错误。
        .map_err(|error| worker_start_error("SetHandleInformation", error))
}

// 建立关闭即终止的 Windows Job。
pub(super) fn create_kill_on_close_job() -> AppResult<OwnedHandle> {
    // 创建未命名 Job，避免跨进程名称碰撞。
    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        // 映射创建错误。
        .map_err(|error| worker_start_error("CreateJobObjectW", error))?;
    // 接管 Job handle。
    let job = OwnedHandle::new(job, "CreateJobObjectW")?;
    // 初始化扩展限制结构。
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    // 确保主进程退出或异常返回时 worker 一并终止。
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // 转换结构长度。
    let limits_size = u32::try_from(std::mem::size_of_val(&limits)).map_err(|_| {
        // 返回结构长度溢出。
        WorkerProcessErrorCode::StartFailed.error("Job limit size overflowed.")
    })?;
    // 安装 Job 限制。
    unsafe {
        SetInformationJobObject(
            // 传入 Job handle。
            job.raw(),
            // 使用扩展限制类别。
            JobObjectExtendedLimitInformation,
            // 传入只读结构指针。
            (&raw const limits).cast(),
            // 传入结构长度。
            limits_size,
        )
    }
    // 映射配置错误。
    .map_err(|error| worker_start_error("SetInformationJobObject", error))?;
    // 返回已配置 Job。
    Ok(job)
}

// 把 Windows 路径转换为 NUL 结尾 UTF-16。
pub(super) fn wide_path(path: &Path) -> Vec<u16> {
    // 使用 Windows 原生 OsStr 编码。
    use std::os::windows::ffi::OsStrExt;
    // 编码并附加终止符。
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

// 按 Windows CRT 规则引用一个命令行参数。
fn quote_argument(value: &str) -> String {
    // 无空白、引号且非空的参数无需引用。
    if !value.is_empty()
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte == b'"')
    {
        // 原样返回简单参数。
        return value.to_owned();
    }
    // 以双引号开始引用参数。
    let mut quoted = String::from("\"");
    // 累积连续反斜杠数量。
    let mut backslashes = 0_usize;
    // 遍历 Unicode 字符；Windows 路径分隔符与引号均为 ASCII。
    for character in value.chars() {
        // 反斜杠延迟输出以判断是否位于引号前。
        if character == '\\' {
            // 累积反斜杠。
            backslashes = backslashes.saturating_add(1);
            // 继续下一个字符。
            continue;
        }
        // 引号前的反斜杠必须翻倍并转义引号。
        if character == '"' {
            // 输出两倍反斜杠再加一个转义反斜杠。
            quoted.push_str(&"\\".repeat(backslashes.saturating_mul(2).saturating_add(1)));
            // 输出原始引号。
            quoted.push('"');
        } else {
            // 普通字符前原样输出累计反斜杠。
            quoted.push_str(&"\\".repeat(backslashes));
            // 输出普通字符。
            quoted.push(character);
        }
        // 清除累计反斜杠。
        backslashes = 0;
    }
    // 结束引号前的尾随反斜杠必须翻倍。
    quoted.push_str(&"\\".repeat(backslashes.saturating_mul(2)));
    // 关闭参数引号。
    quoted.push('"');
    // 返回安全命令行片段。
    quoted
}

// 构造 CreateProcessW 可修改的 NUL 结尾命令行。
pub(super) fn wide_command_line(executable: &Path, arguments: &[String]) -> Vec<u16> {
    // 先把 argv[0] 设为精确可执行文件路径。
    let mut command_line = quote_argument(&executable.to_string_lossy());
    // 依次追加调用方参数。
    for argument in arguments {
        // 插入参数分隔空格。
        command_line.push(' ');
        // 追加安全引用的参数。
        command_line.push_str(&quote_argument(argument));
    }
    // 使用 Windows 原生 UTF-16 编码并附加 NUL。
    command_line.encode_utf16().chain(Some(0)).collect()
}

// 启动 reader 线程并强制输出上限。
pub(super) fn spawn_reader(
    // 接收独占读取端。
    handle: OwnedHandle,
    // 接收硬字节上限。
    maximum_bytes: usize,
) -> AppResult<JoinHandle<AppResult<Vec<u8>>>> {
    // 为单次 worker 建立有界 reader。
    thread::Builder::new()
        // 使用固定诊断线程名。
        .name("act-worker-pipe-reader".to_owned())
        // 在线程内独占读取端。
        .spawn(move || read_bounded(handle, maximum_bytes))
        // 映射线程创建失败。
        .map_err(|error| WorkerProcessErrorCode::StartFailed.error(error.to_string()))
}

// 从匿名管道读取到 EOF，并拒绝超过上限的结果。
fn read_bounded(handle: OwnedHandle, maximum_bytes: usize) -> AppResult<Vec<u8>> {
    // 保存累计输出。
    let mut output = Vec::new();
    // 使用固定小块避免一次分配过大。
    let mut buffer = [0_u8; 8192];
    // 持续读取到 worker 关闭管道。
    loop {
        // 保存本次读取长度。
        let mut read = 0_u32;
        // 执行同步读取。
        let result = unsafe { ReadFile(handle.raw(), Some(&mut buffer), Some(&mut read), None) };
        // 断管或 EOF 结束读取；worker 结果随后按协议验证。
        if result.is_err() || read == 0 {
            // 退出读取循环。
            break;
        }
        // 转换读取长度。
        let read = usize::try_from(read).map_err(|_| {
            // 返回不可能长度的结构化错误。
            WorkerProcessErrorCode::ProtocolFailed.error("Worker read length overflowed.")
        })?;
        // 检查累计长度不会超过调用方边界。
        if output.len().saturating_add(read) > maximum_bytes {
            // 返回确定的输出过大错误。
            return Err(WorkerProcessErrorCode::OutputTooLarge.error(
                // 不回显潜在敏感输出。
                "The isolated worker exceeded its output boundary.",
            ));
        }
        // 仅追加本次有效字节。
        output.extend_from_slice(&buffer[..read]);
    }
    // 返回完整有界字节流。
    Ok(output)
}

// 向 worker 写入单行 JSON 后关闭 stdin。
fn write_request(handle: OwnedHandle, request: &Value) -> AppResult<()> {
    // 序列化版本化请求。
    let mut bytes = serde_json::to_vec(request)
        // 映射序列化错误。
        .map_err(|error| WorkerProcessErrorCode::SerializationFailed.error(error.to_string()))?;
    // 固定使用单行 framing。
    bytes.push(b'\n');
    // 保存已写偏移。
    let mut offset = 0_usize;
    // 处理理论上的短写。
    while offset < bytes.len() {
        // 保存本次写入长度。
        let mut written = 0_u32;
        // 写入剩余请求字节。
        unsafe {
            WriteFile(
                handle.raw(),
                Some(&bytes[offset..]),
                Some(&mut written),
                None,
            )
        }
        // 映射管道写入错误。
        .map_err(|error| worker_start_error("WriteFile(worker stdin)", error))?;
        // 零长度写入无法继续。
        if written == 0 {
            // 返回稳定协议错误。
            return Err(WorkerProcessErrorCode::StartFailed.error(
                // 提供无敏感值诊断。
                "Worker stdin closed before the request was written.",
            ));
        }
        // 转换并推进已写偏移。
        offset = offset.saturating_add(usize::try_from(written).map_err(|_| {
            // 映射不可能的长度溢出。
            WorkerProcessErrorCode::StartFailed.error("Worker write length overflowed.")
        })?);
    }
    // OwnedHandle 离开作用域时关闭 stdin，形成 EOF。
    Ok(())
}

// 等待 reader 线程并展平线程与协议错误。
pub(super) fn join_reader(reader: JoinHandle<AppResult<Vec<u8>>>) -> AppResult<Vec<u8>> {
    // 捕获线程 panic 并返回结构化错误。
    reader
        // 等待 reader 结束。
        .join()
        // 映射 panic。
        .map_err(|_| WorkerProcessErrorCode::ProtocolFailed.error("Worker reader panicked."))?
}

// 强制终止 Job 并等待主 worker 退出。
pub(super) fn terminate_and_reap(job: &OwnedHandle, process: &OwnedHandle) {
    // 终止 Job 中全部进程。
    let _ = unsafe { TerminateJobObject(job.raw(), 2) };
    // 有界等待已发出的内核终止完成。
    let _ = unsafe { WaitForSingleObject(process.raw(), 5_000) };
}

// 在主 worker 已退出后关闭仍留在同一 Job 的私有子孙进程。
fn terminate_remaining_job_members(job: &OwnedHandle, process: &OwnedHandle) -> AppResult<()> {
    // 终止仍属于 Job 的全部进程，避免继承管道让 reader 无界等待。
    unsafe { TerminateJobObject(job.raw(), 2) }
        // 把 Job 清理失败转换为稳定 Component 错误。
        .map_err(|error| worker_start_error("TerminateJobObject", error))?;
    // 主 worker 理论上已经退出，但仍以有界等待确认内核对象状态。
    let wait = unsafe { WaitForSingleObject(process.raw(), 5_000) };
    // 只有已退出状态满足回收契约。
    if wait != WAIT_OBJECT_0 {
        // 拒绝把未确认回收的结果认证为成功。
        return Err(WorkerProcessErrorCode::WaitFailed.error(
            // 不公开原生 wait 状态或进程身份。
            "The isolated worker Job could not be fully reaped.",
        ));
    }
    // 返回已完成整树终止的事实。
    Ok(())
}

// 回收两个 reader，避免取消与超时后留下线程或管道。
fn reap_readers(
    // 接收 stdout reader。
    stdout_reader: JoinHandle<AppResult<Vec<u8>>>,
    // 接收 stderr reader。
    stderr_reader: JoinHandle<AppResult<Vec<u8>>>,
) {
    // 等待 stdout reader 完成。
    let _ = join_reader(stdout_reader);
    // 等待 stderr reader 完成。
    let _ = join_reader(stderr_reader);
}

// 运行无参数 companion worker 并解析单行 JSON envelope。
pub(crate) fn run_companion(
    // 接收精确 companion 路径。
    executable: &Path,
    // 接收 companion 参数；生产观察 worker 使用空数组。
    arguments: &[String],
    // 接收版本化 JSON 请求。
    request: &Value,
    // 接收 1..335000ms deadline，覆盖最长录制、首帧等待与回收宽限。
    timeout: Duration,
    // 接收 stdout 硬上限。
    maximum_output_bytes: usize,
    // 接收调用方取消轮询函数。
    cancelled: impl Fn() -> bool,
) -> AppResult<WorkerOutput> {
    // 拒绝无效 deadline 或输出上限。
    if timeout.is_zero() || timeout > Duration::from_millis(335_000) || maximum_output_bytes == 0 {
        // 返回通用参数错误。
        return Err(WorkerProcessErrorCode::InvalidArgument.error(
            // 说明允许范围。
            "Worker timeout must be 1..335000ms and output boundary must be positive.",
        ));
    }
    // 从任何 Job、pipe 或进程资源创建前启动完整生命周期 deadline。
    let started = Instant::now();
    // 建立 worker stdin 管道。
    let stdin_pipe = inherited_pipe()?;
    // parent 只保留写入端。
    make_parent_only(&stdin_pipe.write)?;
    // 建立 worker stdout 管道。
    let stdout_pipe = inherited_pipe()?;
    // parent 只保留读取端。
    make_parent_only(&stdout_pipe.read)?;
    // 建立 worker stderr 管道。
    let stderr_pipe = inherited_pipe()?;
    // parent 只保留读取端。
    make_parent_only(&stderr_pipe.read)?;
    // 建立关闭即终止的 Job。
    let job = create_kill_on_close_job()?;
    // 初始化 worker 启动信息。
    let startup = STARTUPINFOW {
        // 设置结构长度。
        cb: u32::try_from(std::mem::size_of::<STARTUPINFOW>()).map_err(|_| {
            // 映射结构长度溢出。
            WorkerProcessErrorCode::StartFailed.error("STARTUPINFOW size overflowed.")
        })?,
        // 告知 CreateProcess 使用指定 stdio handles。
        dwFlags: STARTF_USESTDHANDLES,
        // worker 从 child 读取端接收请求。
        hStdInput: stdin_pipe.read.raw(),
        // worker 向 child 写入端输出结果。
        hStdOutput: stdout_pipe.write.raw(),
        // worker 诊断也进入独立 child 写入端。
        hStdError: stderr_pipe.write.raw(),
        // 其余字段使用系统默认值。
        ..Default::default()
    };
    // 初始化进程创建结果。
    let mut process = PROCESS_INFORMATION::default();
    // 编码精确 worker 路径。
    let executable_wide = wide_path(executable);
    // 构造 CreateProcessW 可修改的完整 argv。
    let mut command_line = wide_command_line(executable, arguments);
    // 以挂起状态创建 worker，杜绝 Job 绑定前执行竞态。
    unsafe {
        CreateProcessW(
            // 使用精确绝对可执行文件路径。
            PCWSTR(executable_wide.as_ptr()),
            // 传入含 argv[0] 的可修改命令行。
            Some(PWSTR(command_line.as_mut_ptr())),
            // 不继承进程安全描述符。
            None,
            // 不继承线程安全描述符。
            None,
            // 只继承显式保留继承位的 child 管道端。
            true,
            // 隐藏窗口、挂起主线程并保留 Unicode 环境。
            CREATE_NO_WINDOW | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            // 继承当前环境。
            None,
            // 继承当前目录。
            PCWSTR::null(),
            // 传入 stdio 启动信息。
            &startup,
            // 接收进程与主线程 handle。
            &mut process,
        )
    }
    // 映射进程创建错误。
    .map_err(|error| worker_start_error("CreateProcessW", error))?;
    // 接管 worker 进程 handle。
    let process_handle = OwnedHandle::new(process.hProcess, "CreateProcessW(process)")?;
    // 接管 worker 主线程 handle。
    let thread_handle = match OwnedHandle::new(process.hThread, "CreateProcessW(thread)") {
        // 保存有效线程 handle。
        Ok(handle) => handle,
        // 异常无效线程 handle 时先关闭进程 Job。
        Err(error) => {
            // 此时尚未绑定 Job，必须直接终止仍挂起的进程。
            let _ = unsafe { TerminateProcess(process_handle.raw(), 2) };
            // 等待进程退出。
            let _ = unsafe { WaitForSingleObject(process_handle.raw(), 5_000) };
            // 返回结构化启动错误。
            return Err(error);
        }
    };
    // 在 worker 首条指令执行前绑定 Job。
    if let Err(error) = unsafe { AssignProcessToJobObject(job.raw(), process_handle.raw()) } {
        // 绑定失败时 Job 尚不拥有 worker，直接终止仍挂起的进程。
        let _ = unsafe { TerminateProcess(process_handle.raw(), 2) };
        // 等待进程内核对象进入已退出状态。
        let _ = unsafe { WaitForSingleObject(process_handle.raw(), 5_000) };
        // 返回稳定启动错误。
        return Err(worker_start_error("AssignProcessToJobObject", error));
    }
    // 启动 stdout reader前移交 parent 读取端。
    let stdout_reader = spawn_reader(stdout_pipe.read, maximum_output_bytes)?;
    // 启动 stderr reader 前移交 parent 读取端。
    let stderr_reader = spawn_reader(stderr_pipe.read, MAXIMUM_STDERR_BYTES)?;
    // parent 关闭三个 child 端，保证 EOF 能随 worker 退出到达 reader。
    drop(stdin_pipe.read);
    // 关闭 stdout child 写入端副本。
    drop(stdout_pipe.write);
    // 关闭 stderr child 写入端副本。
    drop(stderr_pipe.write);
    // 恢复 worker 主线程。
    let resume_result = unsafe { ResumeThread(thread_handle.raw()) };
    // u32::MAX 表示恢复失败。
    if resume_result == u32::MAX {
        // 终止并回收已绑定 Job 的 worker。
        terminate_and_reap(&job, &process_handle);
        // 等待 reader 释放管道。
        reap_readers(stdout_reader, stderr_reader);
        // 返回稳定启动错误。
        return Err(WorkerProcessErrorCode::StartFailed.error(
            // 标明失败步骤。
            "ResumeThread failed for the isolated worker.",
        ));
    }
    // 主线程 handle 不再需要。
    drop(thread_handle);
    // 把请求完整写入 worker 后关闭 stdin。
    if let Err(error) = write_request(stdin_pipe.write, request) {
        // 写入失败时终止 worker，避免 provider 残留。
        terminate_and_reap(&job, &process_handle);
        // 回收 reader。
        reap_readers(stdout_reader, stderr_reader);
        // 返回原始结构化错误。
        return Err(error);
    }
    // 轮询到退出、取消或 deadline。
    loop {
        // 检查 worker 进程状态。
        let wait = unsafe { WaitForSingleObject(process_handle.raw(), WAIT_SLICE_MS) };
        // 正常退出时结束轮询。
        if wait == WAIT_OBJECT_0 {
            // 离开轮询。
            break;
        }
        // wait API 失败必须终止 worker。
        if wait == WAIT_FAILED {
            // 回收 Job。
            terminate_and_reap(&job, &process_handle);
            // 回收 reader。
            reap_readers(stdout_reader, stderr_reader);
            // 返回稳定等待错误。
            return Err(WorkerProcessErrorCode::WaitFailed.error(
                // 不公开进程 handle。
                "Waiting for the isolated worker failed.",
            ));
        }
        // 仅允许 timeout 状态继续轮询。
        if wait != WAIT_TIMEOUT {
            // 未知 wait 状态也 fail closed。
            terminate_and_reap(&job, &process_handle);
            // 回收 reader。
            reap_readers(stdout_reader, stderr_reader);
            // 返回稳定等待错误。
            return Err(WorkerProcessErrorCode::WaitFailed.error(
                // 不公开原生状态值。
                "The isolated worker returned an unknown wait state.",
            ));
        }
        // cancellation 优先于下一轮等待。
        if cancelled() {
            // 终止并等待 worker 退出。
            terminate_and_reap(&job, &process_handle);
            // 回收 reader。
            reap_readers(stdout_reader, stderr_reader);
            // 返回稳定取消错误。
            return Err(WorkerProcessErrorCode::Cancelled.error(
                // 明确 worker 已回收。
                "The isolated worker was cancelled and reaped.",
            ));
        }
        // deadline 到达时终止整个 Job。
        if started.elapsed() >= timeout {
            // 终止并等待 worker 退出。
            terminate_and_reap(&job, &process_handle);
            // 回收 reader。
            reap_readers(stdout_reader, stderr_reader);
            // 返回稳定 provider timeout。
            return Err(WorkerProcessErrorCode::Timeout.error(
                // 明确 worker 已回收。
                "The isolated provider worker exceeded its deadline and was reaped.",
            ));
        }
    }
    // 读取 worker 退出码。
    let mut exit_code = 0_u32;
    // 查询已退出进程的稳定退出码。
    unsafe { GetExitCodeProcess(process_handle.raw(), &mut exit_code) }
        // 映射查询失败。
        .map_err(|error| worker_start_error("GetExitCodeProcess", error))?;
    // 在等待管道 EOF 前终止主 worker 遗留的全部 Job 成员。
    terminate_remaining_job_members(&job, &process_handle)?;
    // 等待并取得 stdout。
    let stdout = join_reader(stdout_reader)?;
    // 等待并取得 stderr。
    let stderr = join_reader(stderr_reader)?;
    // 任何 stderr 都违反纯 JSON stdout 协议。
    if !stderr.is_empty() {
        // 不回显潜在 provider 敏感诊断。
        return Err(WorkerProcessErrorCode::ProtocolFailed.error(
            // 说明拒绝原因。
            "The isolated worker wrote diagnostics to stderr.",
        ));
    }
    // 按 UTF-8 解析 stdout。
    let text = std::str::from_utf8(&stdout)
        // 拒绝非 UTF-8 输出。
        .map_err(|_| WorkerProcessErrorCode::ProtocolFailed.error("Worker stdout is not UTF-8."))?;
    // 要求单个非空 JSON 行。
    let trimmed = text.trim_end_matches(['\r', '\n']);
    // 空输出或内嵌换行都违反 framing。
    if trimmed.is_empty() || trimmed.contains(['\r', '\n']) {
        // 返回稳定协议错误。
        return Err(WorkerProcessErrorCode::ProtocolFailed.error(
            // 不回显原始输出。
            format!(
                // 只报告长度和换行数量，不回显 worker 内容。
                "Worker stdout framing is invalid (exitCode={}, bytes={}, embeddedLineBreaks={}).",
                // 输出 worker 退出码。
                exit_code,
                // 输出总字节数。
                stdout.len(),
                // 输出 trim 后仍存在的换行数量。
                trimmed.matches(['\r', '\n']).count(),
            ),
        ));
    }
    // 解析 JSON envelope。
    let envelope = serde_json::from_str::<Value>(trimmed)
        // 拒绝无效 JSON。
        .map_err(|_| {
            WorkerProcessErrorCode::ProtocolFailed.error("Worker stdout is not valid JSON.")
        })?;
    // 返回退出码与结构化 envelope。
    Ok(WorkerOutput {
        // 保存进程退出码。
        exit_code,
        // 保存 JSON envelope。
        envelope,
    })
}

// 导出只启动固定 sequence step sibling 的流式 Job runner 子 Component。
#[allow(dead_code)]
#[path = "worker_process_sequence.rs"]
pub(crate) mod sequence;

// 编译 worker process Component 的独立测试。
#[cfg(test)]
#[path = "worker_process_tests.rs"]
mod tests;
