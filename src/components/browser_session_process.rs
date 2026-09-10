//! 在 Windows Job 中运行固定 browser-session worker 并保留 ready 会话所有权。

// 导入路径、通道、线程与单调时间工具。
use std::{
    // 导入固定 worker 路径。
    path::Path,
    // 导入 reader 事件通道。
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    // 导入 reader 线程句柄。
    thread::{self, JoinHandle},
    // 导入 deadline 工具。
    time::{Duration, Instant},
};

// 导入通用序列化 trait。
use serde::Serialize;

// 导入 JSON 值。
// 导入 Windows 进程、管道、Job 与等待 API。
use windows::{
    // 导入 Win32 命名空间。
    Win32::{
        // 导入等待状态。
        Foundation::WAIT_OBJECT_0,
        // 导入同步管道读写。
        Storage::FileSystem::{ReadFile, WriteFile},
        // 导入 Job 绑定。
        System::JobObjects::AssignProcessToJobObject,
        // 导入进程创建与控制。
        System::Threading::{
            // 导入固定创建标志。
            CREATE_NO_WINDOW,
            CREATE_SUSPENDED,
            CREATE_UNICODE_ENVIRONMENT,
            // 导入进程创建函数。
            CreateProcessW,
            // 导入进程信息结构。
            PROCESS_INFORMATION,
            // 导入主线程恢复函数。
            ResumeThread,
            // 导入 stdio 启动标志。
            STARTF_USESTDHANDLES,
            // 导入启动信息结构。
            STARTUPINFOW,
            // 导入 Job 绑定前终止函数。
            TerminateProcess,
            // 导入有界等待。
            WaitForSingleObject,
        },
    },
    // 导入宽字符指针。
    core::{PCWSTR, PWSTR},
};

// 导入统一错误与结果。
use crate::domain::{AppControlError, AppResult};

// 导入冻结协议和共用 Job 原语。
use super::{
    // 导入 browser-session 协议状态机。
    browser_session_protocol::{
        self, BrowserSessionFrameObservation, BrowserSessionOutcome, BrowserSessionSource,
        BrowserSessionWorkerInput, CONTRACT_VERSION, MAXIMUM_OUTPUT_BYTES,
    },
    // 导入安全 request nonce。
    secure_nonce_windows::random_nonce,
    // 导入共用 Windows worker 原语。
    worker_process::{
        self, MAXIMUM_STDERR_BYTES, OwnedHandle, create_kill_on_close_job, inherited_pipe,
        join_reader, make_parent_only, spawn_reader, terminate_and_reap, wide_command_line,
        wide_path, worker_start_error,
    },
};

// 把 live 会话与打开结果所有权拆到独立窄文件。
#[path = "browser_session_process_result.rs"]
mod result;
// 把 ready 后页面命令聚合拆到独立窄文件。
#[path = "browser_session_process_page.rs"]
mod page;
// 导入同一 Component 的结果与资源所有者。
pub(crate) use page::BrowserPageCommandResult;
// 导出打开聚合与 live 会话所有权。
pub(crate) use result::{BrowserSessionOpenResult, BrowserSessionProcess};
// 导入部分输出聚合器。
use result::incomplete_result;

// 固定生产 worker 文件名。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-browser-session-worker.exe";
// 固定测试 worker 文件名。
const FIXTURE_FILE_NAME: &str = "ai-computer-toolkit-browser-session-worker-fixture.exe";
// 固定 parent 等待轮询粒度。
const WAIT_SLICE: Duration = Duration::from_millis(10);
// 固定 cancel 后协作终态宽限。
const CANCEL_GRACE: Duration = Duration::from_millis(250);
// 固定 close 后整树回收等待。
const CLOSE_TIMEOUT_MS: u32 = 5_000;

// 表示固定测试 worker 允许的封闭行为。
#[derive(Clone, Copy, Debug)]
pub(crate) enum BrowserSessionFixtureMode {
    // 输出 accepted 后异常退出。
    AcceptedOnly,
    // 不输出任何帧直接退出。
    ZeroFrame,
    // 输出 ready 后等待 cancel。
    Ready,
    // 输出 accepted 后忽略 cancel 直至 Job 回收。
    HangAfterAccepted,
}

// 把固定测试行为映射到唯一 argv。
impl BrowserSessionFixtureMode {
    // 返回仓库 fixture 识别的固定参数。
    const fn argument(self) -> &'static str {
        // 穷举封闭测试行为。
        match self {
            // 映射 accepted-only。
            Self::AcceptedOnly => "--mode=accepted-only",
            // 映射零帧。
            Self::ZeroFrame => "--mode=zero-frame",
            // 映射 ready。
            Self::Ready => "--mode=ready",
            // 映射 accepted 后挂起。
            Self::HangAfterAccepted => "--mode=hang-after-accepted",
        }
    }
}

// 保存 parent 对 worker stdout 的封闭观察事件。
enum ReaderEvent {
    // 携带一条完整 UTF-8 JSON 行。
    Line(String),
    // 表示 worker 已关闭 stdout。
    Eof,
    // 表示输出非法或超过硬上限。
    Failed,
}

// 把协议错误映射到产品级稳定错误。
fn protocol_error(message: &'static str) -> AppControlError {
    // 使用冻结协议错误码。
    AppControlError::new("WORKER_PROTOCOL_FAILED", message)
}

// 向持久 stdin 写入一条有界输入。
fn write_input<T: Serialize>(handle: &OwnedHandle, input: &T) -> AppResult<()> {
    // 序列化严格输入。
    let mut bytes = serde_json::to_vec(input)
        // 隐藏序列化内部细节。
        .map_err(|_| protocol_error("The browser session request could not be serialized."))?;
    // 添加 JSON Lines 终止符。
    bytes.push(b'\n');
    // 保存已写偏移。
    let mut offset = 0_usize;
    // 处理理论短写。
    while offset < bytes.len() {
        // 保存当前写入长度。
        let mut written = 0_u32;
        // 写入剩余字节。
        unsafe {
            WriteFile(
                // 使用 parent 写端。
                handle.raw(),
                // 传入剩余切片。
                Some(&bytes[offset..]),
                // 接收实际长度。
                Some(&mut written),
                // 使用同步 I/O。
                None,
            )
        }
        // 映射管道错误。
        .map_err(|error| worker_start_error("WriteFile(browser session stdin)", error))?;
        // 零写无法继续。
        if written == 0 {
            // 返回协议断开。
            return Err(protocol_error("The browser session worker closed stdin."));
        }
        // 推进偏移。
        offset = offset.saturating_add(usize::try_from(written).map_err(|_| {
            // 映射不可能的长度溢出。
            protocol_error("The browser session write length overflowed.")
        })?);
    }
    // 完整输入已经进入管道。
    Ok(())
}

// 启动逐行有界 stdout reader。
fn spawn_frame_reader(handle: OwnedHandle) -> AppResult<(Receiver<ReaderEvent>, JoinHandle<()>)> {
    // 建立 reader 事件通道。
    let (sender, receiver) = mpsc::channel();
    // 启动唯一 reader 线程。
    let reader = thread::Builder::new()
        // 使用固定诊断名称。
        .name("act-browser-session-frame-reader".to_owned())
        // 在线程内独占管道读端。
        .spawn(move || {
            // 保存尚未形成完整行的字节。
            let mut pending = Vec::new();
            // 保存总输出字节数。
            let mut total = 0_usize;
            // 使用固定小块读取。
            let mut buffer = [0_u8; 4096];
            // 持续读取直到 EOF。
            loop {
                // 保存本次读取长度。
                let mut read = 0_u32;
                // 执行同步读取。
                let result =
                    unsafe { ReadFile(handle.raw(), Some(&mut buffer), Some(&mut read), None) };
                // 断管或 EOF 结束读取。
                if result.is_err() || read == 0 {
                    // 非空尾部仍作为最后一行交给严格 parser。
                    if !pending.is_empty() {
                        // 严格转换 UTF-8。
                        match String::from_utf8(std::mem::take(&mut pending)) {
                            // 发送最后一行。
                            Ok(line) => {
                                // receiver 消失时直接结束。
                                if sender.send(ReaderEvent::Line(line)).is_err() {
                                    // parent 已停止观察。
                                    return;
                                }
                            }
                            // 非 UTF-8 输出失败闭合。
                            Err(_) => {
                                // 尝试报告失败。
                                let _ = sender.send(ReaderEvent::Failed);
                                // 结束 reader。
                                return;
                            }
                        }
                    }
                    // 报告 EOF。
                    let _ = sender.send(ReaderEvent::Eof);
                    // 结束 reader。
                    return;
                }
                // 转换读取长度。
                let Ok(read) = usize::try_from(read) else {
                    // 报告异常长度。
                    let _ = sender.send(ReaderEvent::Failed);
                    // 结束 reader。
                    return;
                };
                // 更新总输出边界。
                total = total.saturating_add(read);
                // 超限时失败闭合。
                if total > MAXIMUM_OUTPUT_BYTES {
                    // 报告资源失败。
                    let _ = sender.send(ReaderEvent::Failed);
                    // 结束 reader。
                    return;
                }
                // 追加本次字节。
                pending.extend_from_slice(&buffer[..read]);
                // 逐条提取 LF 结尾帧。
                while let Some(index) = pending.iter().position(|byte| *byte == b'\n') {
                    // 取得含 LF 的前缀。
                    let mut line = pending.drain(..=index).collect::<Vec<_>>();
                    // 移除 LF。
                    line.pop();
                    // 兼容 CRLF。
                    if line.last() == Some(&b'\r') {
                        // 移除 CR。
                        line.pop();
                    }
                    // 严格转换 UTF-8。
                    let Ok(line) = String::from_utf8(line) else {
                        // 报告编码失败。
                        let _ = sender.send(ReaderEvent::Failed);
                        // 结束 reader。
                        return;
                    };
                    // 发送完整行。
                    if sender.send(ReaderEvent::Line(line)).is_err() {
                        // parent 已停止观察。
                        return;
                    }
                }
            }
        })
        // 映射线程创建失败。
        .map_err(|_| protocol_error("The browser session reader could not be started."))?;
    // 返回 receiver 与线程所有权。
    Ok((receiver, reader))
}

// 保存已经完成 Job 绑定和 stdio 建立的 worker。
struct SpawnedWorker {
    // 保存 parent stdin。
    stdin: OwnedHandle,
    // 保存 worker process。
    process: OwnedHandle,
    // 保存 Job。
    job: OwnedHandle,
    // 保存 frame receiver。
    frames: Receiver<ReaderEvent>,
    // 保存 stdout reader。
    stdout_reader: JoinHandle<()>,
    // 保存 stderr reader。
    stderr_reader: JoinHandle<AppResult<Vec<u8>>>,
}

// 以挂起状态启动 worker、绑定 Job 后再恢复。
fn spawn_worker(executable: &Path, arguments: &[String]) -> AppResult<SpawnedWorker> {
    // 建立 stdin 管道。
    let stdin_pipe = inherited_pipe()?;
    // parent 只保留写端。
    make_parent_only(&stdin_pipe.write)?;
    // 建立 stdout 管道。
    let stdout_pipe = inherited_pipe()?;
    // parent 只保留读端。
    make_parent_only(&stdout_pipe.read)?;
    // 建立 stderr 管道。
    let stderr_pipe = inherited_pipe()?;
    // parent 只保留读端。
    make_parent_only(&stderr_pipe.read)?;
    // 建立 kill-on-close Job。
    let job = create_kill_on_close_job()?;
    // 构造 stdio 启动信息。
    let startup = windows::Win32::System::Threading::STARTUPINFOW {
        // 设置结构长度。
        cb: u32::try_from(std::mem::size_of::<STARTUPINFOW>())
            // 映射长度溢出。
            .map_err(|_| protocol_error("The browser session startup size overflowed."))?,
        // 安装显式 stdio。
        dwFlags: STARTF_USESTDHANDLES,
        // worker 读取 child stdin 端。
        hStdInput: stdin_pipe.read.raw(),
        // worker 写入 child stdout 端。
        hStdOutput: stdout_pipe.write.raw(),
        // worker 写入独立 stderr 端。
        hStdError: stderr_pipe.write.raw(),
        // 其余字段使用系统默认。
        ..Default::default()
    };
    // 初始化进程信息。
    let mut process = PROCESS_INFORMATION::default();
    // 编码绝对 worker 路径。
    let executable_wide = wide_path(executable);
    // 构造固定参数命令行。
    let mut command_line = wide_command_line(executable, arguments);
    // 挂起创建 worker。
    unsafe {
        CreateProcessW(
            // 使用精确可执行文件。
            PCWSTR(executable_wide.as_ptr()),
            // 传入可修改命令行。
            Some(PWSTR(command_line.as_mut_ptr())),
            // 使用默认进程安全。
            None,
            // 使用默认线程安全。
            None,
            // 只继承 child 管道端。
            true,
            // 禁止窗口并在 Job 绑定前挂起。
            CREATE_NO_WINDOW | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            // 继承当前环境。
            None,
            // 继承当前目录。
            PCWSTR::null(),
            // 传入 stdio 模板。
            &startup,
            // 接收句柄。
            &mut process,
        )
    }
    // 映射进程创建失败。
    .map_err(|error| worker_start_error("CreateProcessW(browser session worker)", error))?;
    // 接管进程句柄。
    let process_handle = OwnedHandle::new(process.hProcess, "browser session process")?;
    // 接管主线程句柄。
    let thread_handle = match OwnedHandle::new(process.hThread, "browser session thread") {
        // 保存有效句柄。
        Ok(handle) => handle,
        // 无效句柄时直接终止尚未绑定的进程。
        Err(error) => {
            // 终止挂起进程。
            let _ = unsafe { TerminateProcess(process_handle.raw(), 2) };
            // 等待退出。
            let _ = unsafe { WaitForSingleObject(process_handle.raw(), CLOSE_TIMEOUT_MS) };
            // 返回启动失败。
            return Err(error);
        }
    };
    // 在任何 worker 指令执行前绑定 Job。
    if let Err(error) = unsafe { AssignProcessToJobObject(job.raw(), process_handle.raw()) } {
        // Job 未取得所有权时直接终止挂起进程。
        let _ = unsafe { TerminateProcess(process_handle.raw(), 2) };
        // 等待进程退出。
        let _ = unsafe { WaitForSingleObject(process_handle.raw(), CLOSE_TIMEOUT_MS) };
        // 返回绑定失败。
        return Err(worker_start_error(
            "AssignProcessToJobObject(browser session)",
            error,
        ));
    }
    // 启动 stdout frame reader。
    let (frames, stdout_reader) = spawn_frame_reader(stdout_pipe.read)?;
    // 启动有界 stderr reader。
    let stderr_reader = spawn_reader(stderr_pipe.read, MAXIMUM_STDERR_BYTES)?;
    // 关闭 parent 的 child stdin 副本。
    drop(stdin_pipe.read);
    // 关闭 parent 的 child stdout 副本。
    drop(stdout_pipe.write);
    // 关闭 parent 的 child stderr 副本。
    drop(stderr_pipe.write);
    // 恢复已经进入 Job 的 worker。
    if unsafe { ResumeThread(thread_handle.raw()) } == u32::MAX {
        // 回收完整 Job。
        terminate_and_reap(&job, &process_handle);
        // 返回启动失败。
        return Err(protocol_error(
            "The browser session worker could not be resumed.",
        ));
    }
    // 主线程句柄不再需要。
    drop(thread_handle);
    // 返回完整 worker 所有权。
    Ok(SpawnedWorker {
        // 保存 parent stdin。
        stdin: stdin_pipe.write,
        // 保存 process。
        process: process_handle,
        // 保存 Job。
        job,
        // 保存帧通道。
        frames,
        // 保存 stdout reader。
        stdout_reader,
        // 保存 stderr reader。
        stderr_reader,
    })
}

// 把合法 final 与 live worker 所有权组合为打开结果。
fn final_result(
    // 接收完整协议观察。
    observation: &BrowserSessionFrameObservation,
    // 接收 worker 所有权。
    worker: SpawnedWorker,
    // 保存请求 nonce。
    request_nonce: String,
) -> AppResult<BrowserSessionOpenResult> {
    // final 必须存在。
    let final_observation = observation
        // 借用唯一 final。
        .final_observation()
        // 缺失 final 表示调用方错误。
        .ok_or_else(|| protocol_error("The browser session final observation is missing."))?;
    // ready 时把 Job 所有权转交 live 会话。
    if final_observation.outcome() == BrowserSessionOutcome::Ready {
        // 取得 canonical session ID。
        let session_id = final_observation
            // 借用 session ID。
            .session_id()
            // parser 已验证存在。
            .ok_or_else(|| protocol_error("The ready browser session identity is missing."))?
            // 保存独立身份。
            .to_owned();
        // 返回 live ready 结果。
        return Ok(BrowserSessionOpenResult {
            // 保存 ready。
            outcome: BrowserSessionOutcome::Ready,
            // 保存完成事实。
            completed: final_observation.completed(),
            // 保存重试事实。
            retry_safe: final_observation.retry_safe(),
            // 保存接受事实。
            accepted_may_have_occurred: final_observation.accepted_may_have_occurred(),
            // ready 不携带错误。
            error: None,
            // 转交完整 live 会话。
            session: Some(BrowserSessionProcess {
                // 保存 opaque ID。
                session_id,
                // 保存 stdin。
                stdin: Some(worker.stdin),
                // 保存 process。
                process: Some(worker.process),
                // 保存 Job。
                job: Some(worker.job),
                // 保存 stdout reader。
                stdout_reader: Some(worker.stdout_reader),
                // 保存 stderr reader。
                stderr_reader: Some(worker.stderr_reader),
                // 保留页面命令帧接收端。
                frames: Some(worker.frames),
            }),
            // ready 时尚未强制回收。
            forced_reap: false,
            // 保存关联 nonce。
            request_nonce,
        });
    }
    // 非 ready final 必须回收 worker。
    let mut session = BrowserSessionProcess {
        // 使用空内部身份，永不公开。
        session_id: String::new(),
        // 保存 stdin。
        stdin: Some(worker.stdin),
        // 保存 process。
        process: Some(worker.process),
        // 保存 Job。
        job: Some(worker.job),
        // 保存 stdout reader。
        stdout_reader: Some(worker.stdout_reader),
        // 保存 stderr reader。
        stderr_reader: Some(worker.stderr_reader),
        // 保留失败回收前的帧接收端。
        frames: Some(worker.frames),
    };
    // 强制关闭剩余资源。
    session.force_reap();
    // 返回封闭失败 final。
    Ok(BrowserSessionOpenResult {
        // 保存 outcome。
        outcome: final_observation.outcome(),
        // 保存完成事实。
        completed: final_observation.completed(),
        // 保存重试事实。
        retry_safe: final_observation.retry_safe(),
        // 保存接受事实。
        accepted_may_have_occurred: final_observation.accepted_may_have_occurred(),
        // 复制安全错误。
        error: final_observation.error().cloned(),
        // 没有 live 会话。
        session: None,
        // parent 已完成回收。
        forced_reap: true,
        // 保存关联 nonce。
        request_nonce,
    })
}

// 运行固定 worker 并聚合打开握手。
fn open_with_executable(
    // 借用固定 worker 路径。
    executable: &Path,
    // 借用固定测试参数或生产空参数。
    arguments: &[String],
    // 接收封闭来源。
    source: BrowserSessionSource,
    // 接收总 deadline。
    timeout: Duration,
    // 接收取消观察。
    cancelled: impl Fn() -> bool,
) -> AppResult<BrowserSessionOpenResult> {
    // 核对冻结 deadline 范围。
    let timeout_ms = u32::try_from(timeout.as_millis())
        .ok()
        .filter(|value| (1..=30_000).contains(value))
        .ok_or_else(|| {
            // 返回稳定参数失败。
            AppControlError::new(
                "INVALID_ARGUMENT",
                "Browser session timeout must be 1..=30000ms.",
            )
        })?;
    // 生成一次性 request nonce。
    let request_nonce = random_nonce()?;
    // 构造冻结 open 输入。
    let open = BrowserSessionWorkerInput::Open {
        // 使用固定版本。
        contract_version: CONTRACT_VERSION.to_owned(),
        // 保存随机关联值。
        request_nonce: request_nonce.clone(),
        // 使用不重置总 deadline。
        timeout_ms,
        // 保存封闭来源。
        source,
    };
    // 从任何资源创建前记录单调起点。
    let started = Instant::now();
    // 启动并绑定固定 worker。
    let worker = spawn_worker(executable, arguments)?;
    // 写入 open，保持 stdin 供 cancel 使用。
    if let Err(error) = write_input(&worker.stdin, &open) {
        // 启动后写入失败必须回收 Job。
        terminate_and_reap(&worker.job, &worker.process);
        // 返回写入错误。
        return Err(error);
    }
    // 保存按行累积的可信 stdout。
    let mut output = String::new();
    // 保存最近一次合法观察。
    let mut accepted = false;
    // 保存是否已发送 cancel。
    let mut cancel_sent = false;
    // 保存 cancel 后终止宽限起点。
    let mut cancel_started = None;
    // 持续等待 final 或停止条件。
    loop {
        // 用户取消或总 deadline 只触发一次协作 cancel。
        if !cancel_sent && (cancelled() || started.elapsed() >= timeout) {
            // 构造关联 cancel。
            let cancel = BrowserSessionWorkerInput::Cancel {
                // 使用固定版本。
                contract_version: CONTRACT_VERSION.to_owned(),
                // 关联 open。
                request_nonce: request_nonce.clone(),
            };
            // 尽力发送取消。
            let _ = write_input(&worker.stdin, &cancel);
            // 标记已经发送。
            cancel_sent = true;
            // 启动固定协作宽限。
            cancel_started = Some(Instant::now());
        }
        // cancel 宽限耗尽后强制回收并聚合部分事实。
        if cancel_started.is_some_and(|instant| instant.elapsed() >= CANCEL_GRACE) {
            // 回收完整 Job。
            terminate_and_reap(&worker.job, &worker.process);
            // 返回零帧或 accepted-only 聚合。
            return Ok(incomplete_result(accepted, true, request_nonce));
        }
        // worker 已退出且当前没有待处理帧时按部分事实聚合。
        let process_exited =
            unsafe { WaitForSingleObject(worker.process.raw(), 0) } == WAIT_OBJECT_0;
        // 等待下一 stdout 事件。
        match worker.frames.recv_timeout(WAIT_SLICE) {
            // 追加一条严格帧。
            Ok(ReaderEvent::Line(line)) => {
                // 在帧之间插入唯一 LF。
                if !output.is_empty() {
                    // 保持 JSON Lines 形状。
                    output.push('\n');
                }
                // 追加当前行。
                output.push_str(&line);
                // 解析全部前缀并更新 accepted 事实。
                let observation =
                    match browser_session_protocol::observe_output(&output, &request_nonce) {
                        // 保存合法观察。
                        Ok(observation) => observation,
                        // 协议污染前若已 accepted 则保守 unknown。
                        Err(_) if accepted => {
                            // 回收完整 Job。
                            terminate_and_reap(&worker.job, &worker.process);
                            // 返回保守未知。
                            return Ok(incomplete_result(true, true, request_nonce));
                        }
                        // accepted 前协议污染直接失败。
                        Err(_) => {
                            // 回收完整 Job。
                            terminate_and_reap(&worker.job, &worker.process);
                            // 返回结构化协议错误。
                            return Err(protocol_error(
                                "The browser session worker output violated protocol v1.",
                            ));
                        }
                    };
                // 保存 accepted 前缀事实。
                accepted = observation.accepted();
                // final 到达时完成聚合。
                if observation.final_observation().is_some() {
                    // 转交 live 或回收失败 worker。
                    return final_result(&observation, worker, request_nonce);
                }
            }
            // EOF 使用当前部分事实聚合。
            Ok(ReaderEvent::Eof) => {
                // 回收完整 Job。
                terminate_and_reap(&worker.job, &worker.process);
                // 返回零帧或 accepted-only。
                return Ok(incomplete_result(accepted, true, request_nonce));
            }
            // 非法或超限输出按 accepted 事实聚合。
            Ok(ReaderEvent::Failed) => {
                // 回收完整 Job。
                terminate_and_reap(&worker.job, &worker.process);
                // accepted 后保守 unknown。
                if accepted {
                    // 返回未知结果。
                    return Ok(incomplete_result(true, true, request_nonce));
                }
                // accepted 前返回协议失败。
                return Err(protocol_error(
                    "The browser session worker output was invalid or too large.",
                ));
            }
            // 周期超时继续检查过程状态。
            Err(RecvTimeoutError::Timeout) if !process_exited => {}
            // worker 退出或 reader 断开时聚合部分事实。
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                // 回收完整 Job。
                terminate_and_reap(&worker.job, &worker.process);
                // 返回保守结果。
                return Ok(incomplete_result(accepted, true, request_nonce));
            }
        }
    }
}

// 使用生产固定 sibling 打开隔离浏览器会话。
pub(crate) fn open_isolated(
    // 接收总 deadline。
    timeout: Duration,
    // 接收取消观察。
    cancelled: impl Fn() -> bool,
) -> AppResult<BrowserSessionOpenResult> {
    // 定位固定生产 worker。
    let executable = worker_process::sibling_companion_path(
        // 只使用编译期固定文件名。
        WORKER_FILE_NAME,
        // 使用安全描述。
        "browser session worker",
    )?;
    // 生产 worker 不接收 argv。
    open_with_executable(
        // 借用固定 sibling。
        &executable,
        // 使用空参数。
        &[],
        // 选择工具自有空 profile。
        BrowserSessionSource::IsolatedProfile {},
        // 传入总 deadline。
        timeout,
        // 传入取消观察。
        cancelled,
    )
}

// 使用仓库固定 fixture 验证 parent Job 聚合。
pub(crate) fn open_fixture(
    // 接收封闭 fixture 行为。
    mode: BrowserSessionFixtureMode,
    // 接收总 deadline。
    timeout: Duration,
    // 接收取消观察。
    cancelled: impl Fn() -> bool,
) -> AppResult<BrowserSessionOpenResult> {
    // 定位固定测试 worker。
    let executable = worker_process::sibling_companion_path(
        // 使用编译期固定 fixture 名。
        FIXTURE_FILE_NAME,
        // 使用安全描述。
        "browser session worker fixture",
    )?;
    // 只使用封闭枚举映射的固定参数。
    let arguments = [mode.argument().to_owned()];
    // 运行同一生产 Job 客户端。
    open_with_executable(
        // 借用固定 fixture。
        &executable,
        // 借用固定参数。
        &arguments,
        // fixture 只模拟空隔离来源。
        BrowserSessionSource::IsolatedProfile {},
        // 传入 deadline。
        timeout,
        // 传入取消观察。
        cancelled,
    )
}
