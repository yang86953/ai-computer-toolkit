//! 在 Windows Job 中运行固定 sequence step worker 并保留部分输出观察。

// 导入线程、原子状态与单调时间。
use std::{
    // 导入固定 worker 路径。
    path::Path,
    // 导入 accepted、reader 失败与协议失败共享状态。
    sync::{
        // 保存跨线程共享状态。
        Arc,
        // 保存无锁布尔事实。
        atomic::{AtomicBool, Ordering},
    },
    // 导入 reader 线程句柄与创建器。
    thread::{self, JoinHandle},
    // 导入硬 deadline 类型。
    time::{Duration, Instant},
};

// 导入 Windows 管道、进程、Job 与等待 API。
use windows::{
    // 导入 Win32 命名空间。
    Win32::{
        // 导入 wait 状态常量。
        Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        // 导入同步管道读写函数。
        Storage::FileSystem::{ReadFile, WriteFile},
        // 导入 Job 与进程函数。
        System::{
            // 导入 Job 绑定函数。
            JobObjects::AssignProcessToJobObject,
            // 导入进程创建、退出码与等待函数。
            Threading::{
                // 导入无窗口、挂起和 Unicode 环境标志。
                CREATE_NO_WINDOW,
                CREATE_SUSPENDED,
                CREATE_UNICODE_ENVIRONMENT,
                // 导入固定进程创建函数。
                CreateProcessW,
                // 导入退出码查询函数。
                GetExitCodeProcess,
                // 导入进程创建结果。
                PROCESS_INFORMATION,
                // 导入主线程恢复函数。
                ResumeThread,
                // 导入标准句柄启动标志。
                STARTF_USESTDHANDLES,
                // 导入启动信息结构。
                STARTUPINFOW,
                // 导入 Job 绑定前的进程终止函数。
                TerminateProcess,
                // 导入进程等待函数。
                WaitForSingleObject,
            },
        },
    },
    // 导入宽字符串指针。
    core::{PCWSTR, PWSTR},
};

// 导入父 Component 私有的 Job、pipe 与错误原语。
use super::{
    // 导入 stderr 固定上限。
    MAXIMUM_STDERR_BYTES,
    // 导入私有 handle 所有权。
    OwnedHandle,
    // 导入 wait 轮询间隔。
    WAIT_SLICE_MS,
    // 导入通用 worker 错误类别。
    WorkerProcessErrorCode,
    // 导入固定 Job 创建器。
    create_kill_on_close_job,
    // 导入匿名继承管道创建器。
    inherited_pipe,
    // 导入普通 stderr reader join。
    join_reader,
    // 导入 parent 端继承清理。
    make_parent_only,
    // 导入固定 sibling 定位。
    sibling_companion_path,
    // 导入普通有界 stderr reader。
    spawn_reader,
    // 导入强制回收主 worker。
    terminate_and_reap,
    // 导入正常退出后的 Job 成员清理。
    terminate_remaining_job_members,
    // 导入宽命令行构造。
    wide_command_line,
    // 导入宽路径构造。
    wide_path,
    // 导入 Windows 启动错误映射。
    worker_start_error,
};

// 导入 sequence 协议与统一结果。
use crate::{
    // 导入严格请求、控制、输出上限和帧 parser。
    components::sequence_step_protocol::{
        // 导入 stdout 总上限。
        MAXIMUM_OUTPUT_BYTES,
        // 导入 cancel control 构造器。
        SequenceStepWorkerControl,
        // 导入严格 worker 请求。
        SequenceStepWorkerRequest,
        // 导入完整或部分输出 parser 与观察类型。
        frames::{SequenceStepFrameObservation, parse_frame_log},
    },
    // 导入统一结果类型。
    domain::AppResult,
};

// 固定生产 sequence step worker sibling 文件名。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-sequence-step-worker.exe";
// 在硬 deadline 前预留协作取消窗口。
const COOPERATIVE_STOP_WINDOW: Duration = Duration::from_millis(50);

// 聚合固定 runner 启动所需的拥有型协议输入。
struct SequenceStepRunPlan {
    // 保存严格请求行。
    request_line: Vec<u8>,
    // 保存唯一 cancel 行。
    cancel_line: Vec<u8>,
    // 保存可信请求 nonce。
    request_nonce: String,
    // 保存总生命周期起点。
    started: Instant,
    // 保存硬 deadline。
    timeout: Duration,
}

// 表示 runner 最终停止 worker 的封闭原因。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SequenceStepRunnerStop {
    // worker 在任何外部停止前自行完成。
    Completed,
    // 调用方取消触发协作或强制停止。
    Cancelled,
    // 当前步骤硬 deadline 触发协作或强制停止。
    Deadline,
}

// 保存 runner 回收后建立的最后可靠观察。
pub(crate) struct SequenceStepRunnerOutput {
    // 保存完整或部分帧状态机。
    observation: SequenceStepFrameObservation,
    // 保存停止原因。
    stop: SequenceStepRunnerStop,
    // 保存正常退出时的 worker 退出码。
    exit_code: Option<u32>,
    // 保存是否由 parent 强制终止完整 Job。
    forced_reap: bool,
}

// 为 runner 输出提供只读投影。
impl SequenceStepRunnerOutput {
    // 返回最后可靠帧观察。
    pub(crate) const fn observation(&self) -> &SequenceStepFrameObservation {
        // 借用拥有型观察。
        &self.observation
    }

    // 返回停止原因。
    pub(crate) const fn stop(&self) -> SequenceStepRunnerStop {
        // 复制封闭枚举。
        self.stop
    }

    // 返回仅正常退出时存在的退出码。
    pub(crate) const fn exit_code(&self) -> Option<u32> {
        // 复制可选退出码。
        self.exit_code
    }

    // 返回 parent 是否强制回收 Job。
    pub(crate) const fn forced_reap(&self) -> bool {
        // 复制回收事实。
        self.forced_reap
    }
}

// 表示当前轮询是否应开始外部停止。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StopTrigger {
    // 尚未达到取消或 deadline 预留窗口。
    None,
    // 调用方取消优先触发。
    Cancelled,
    // deadline 预留窗口触发。
    Deadline,
}

// 计算取消优先且不越过硬 deadline 的停止触发。
fn stop_trigger(
    // 接收当前是否已取消。
    cancelled: bool,
    // 接收已耗用总生命周期。
    elapsed: Duration,
    // 接收请求携带的唯一剩余 deadline。
    timeout: Duration,
) -> StopTrigger {
    // 同一观察中取消优先于 deadline。
    if cancelled {
        // 返回调用方取消。
        return StopTrigger::Cancelled;
    }
    // 在硬 deadline 前预留固定协作窗口。
    if elapsed >= timeout.saturating_sub(COOPERATIVE_STOP_WINDOW) {
        // 返回 deadline 停止。
        return StopTrigger::Deadline;
    }
    // 尚可继续等待 worker。
    StopTrigger::None
}

// 把 parent 请求或 control 完整写入保持打开的 stdin。
fn write_bytes(
    // 借用 parent stdin 写端。
    handle: &OwnedHandle,
    // 接收已经有界的协议帧。
    bytes: &[u8],
) -> AppResult<()> {
    // 保存已写入偏移。
    let mut offset = 0_usize;
    // 处理理论短写。
    while offset < bytes.len() {
        // 保存本次写入长度。
        let mut written = 0_u32;
        // 写入剩余字节。
        unsafe {
            WriteFile(
                // 使用 parent 独占写端。
                handle.raw(),
                // 写入剩余协议字节。
                Some(&bytes[offset..]),
                // 接收实际写入长度。
                Some(&mut written),
                // 使用同步 I/O。
                None,
            )
        }
        // 映射管道写入失败。
        .map_err(|error| worker_start_error("WriteFile(sequence worker stdin)", error))?;
        // 零长度无法继续。
        if written == 0 {
            // 返回稳定启动失败。
            return Err(WorkerProcessErrorCode::StartFailed.error(
                // 不公开句柄或输入。
                "The sequence worker stdin closed before the frame was written.",
            ));
        }
        // 转换并推进偏移。
        offset = offset.saturating_add(usize::try_from(written).map_err(|_| {
            // 映射理论长度溢出。
            WorkerProcessErrorCode::StartFailed.error("Sequence worker write length overflowed.")
        })?);
    }
    // 完整帧已写入。
    Ok(())
}

// 检查 stdout 中第一个完整帧并发布 accepted 事实。
fn observe_first_frame(
    // 借用累计 stdout。
    output: &[u8],
    // 接收当前请求 nonce。
    request_nonce: &str,
    // 借用 accepted 原子标记。
    accepted: &AtomicBool,
) -> AppResult<()> {
    // accepted 已建立后无需重复解析。
    if accepted.load(Ordering::Acquire) {
        // 保持既有事实。
        return Ok(());
    }
    // 只有完整换行帧才能解析。
    let Some(end) = output.iter().position(|byte| *byte == b'\n') else {
        // 等待后续字节。
        return Ok(());
    };
    // 首帧必须是 UTF-8。
    let text = std::str::from_utf8(&output[..=end]).map_err(|_| {
        // 不回显 worker 输出。
        WorkerProcessErrorCode::ProtocolFailed.error("Sequence worker stdout is not UTF-8.")
    })?;
    // 以允许部分日志的状态解析首帧。
    let observation = parse_frame_log(text, request_nonce, true).map_err(|failure| {
        // 映射为通用 worker 协议失败。
        WorkerProcessErrorCode::ProtocolFailed.error(format!(
            // 只公开封闭类别。
            "Sequence worker first frame failed with {}.",
            // 使用稳定协议错误文本。
            failure.code().as_str(),
        ))
    })?;
    // 只有合法 accepted 首帧发布事实。
    if observation.dispatch_accepted() {
        // 以 Release 顺序发布给 wait 线程。
        accepted.store(true, Ordering::Release);
    }
    // 首帧是合法 accepted 或 not-dispatched final。
    Ok(())
}

// 启动实时有界 stdout reader。
fn spawn_sequence_reader(
    // 取得 worker stdout 读取端。
    handle: OwnedHandle,
    // 接收请求 nonce。
    request_nonce: String,
    // 接收 accepted 共享状态。
    accepted: Arc<AtomicBool>,
    // 接收 reader 失败共享状态。
    reader_failed: Arc<AtomicBool>,
) -> AppResult<JoinHandle<AppResult<Vec<u8>>>> {
    // 创建固定 reader 线程。
    thread::Builder::new()
        // 使用稳定诊断名称。
        .name("act-sequence-step-output".to_owned())
        // 独占读取 handle 与累计缓冲区。
        .spawn(move || {
            // 保存有界累计输出。
            let mut output = Vec::new();
            // 使用固定小块读取。
            let mut buffer = [0_u8; 8192];
            // 持续读取到 EOF。
            loop {
                // 保存本次读取长度。
                let mut read = 0_u32;
                // 同步读取 stdout。
                let result =
                    unsafe { ReadFile(handle.raw(), Some(&mut buffer), Some(&mut read), None) };
                // EOF 或断管结束读取。
                if result.is_err() || read == 0 {
                    // 返回当前完整累计输出。
                    break;
                }
                // 转换读取长度。
                let read = usize::try_from(read).map_err(|_| {
                    // 发布 reader 失败。
                    reader_failed.store(true, Ordering::Release);
                    // 返回稳定协议错误。
                    WorkerProcessErrorCode::ProtocolFailed
                        .error("Sequence worker read length overflowed.")
                })?;
                // 输出不得超过双阶段协议总上限。
                if output.len().saturating_add(read) > MAXIMUM_OUTPUT_BYTES {
                    // 通知 wait 线程立即回收 Job。
                    reader_failed.store(true, Ordering::Release);
                    // 返回稳定输出上限错误。
                    return Err(WorkerProcessErrorCode::OutputTooLarge.error(
                        // 不回显输出。
                        "The sequence worker exceeded its output boundary.",
                    ));
                }
                // 追加本次有效字节。
                output.extend_from_slice(&buffer[..read]);
                // 只扫描本次新块是否出现首个换行，避免无界半行形成平方复杂度。
                let first_line_completed = !accepted.load(Ordering::Acquire)
                    // 新块包含换行才需要解析累计首帧。
                    && buffer[..read].contains(&b'\n');
                // 第一条完整帧出现后立即发布 accepted。
                if first_line_completed
                    // 仅在完成首帧时解析一次累计输出。
                    && let Err(error) = observe_first_frame(&output, &request_nonce, &accepted)
                {
                    // 通知 wait 线程协议已失败。
                    reader_failed.store(true, Ordering::Release);
                    // 返回原始结构化错误。
                    return Err(error);
                }
            }
            // 返回完整或被 Job 截断的有界输出。
            Ok(output)
        })
        // 映射线程创建失败。
        .map_err(|error| WorkerProcessErrorCode::StartFailed.error(error.to_string()))
}

// 查询正常退出码。
fn process_exit_code(process: &OwnedHandle) -> AppResult<u32> {
    // 初始化退出码。
    let mut exit_code = 0_u32;
    // 查询已退出 worker。
    unsafe { GetExitCodeProcess(process.raw(), &mut exit_code) }
        // 映射查询失败。
        .map_err(|error| worker_start_error("GetExitCodeProcess(sequence worker)", error))?;
    // 返回稳定退出码。
    Ok(exit_code)
}

// 在确定回收后解析完整或部分 stdout。
fn parse_output(
    // 取得累计 stdout。
    output: Vec<u8>,
    // 接收请求 nonce。
    request_nonce: &str,
    // 标记是否允许 Job 截断的部分帧序列。
    allow_partial: bool,
) -> AppResult<SequenceStepFrameObservation> {
    // 整体输出必须是 UTF-8。
    let text = std::str::from_utf8(&output).map_err(|_| {
        // 返回不含输出的稳定错误。
        WorkerProcessErrorCode::ProtocolFailed.error("Sequence worker stdout is not UTF-8.")
    })?;
    // 委托协议 Component 验证帧顺序、关联与形状。
    parse_frame_log(text, request_nonce, allow_partial).map_err(|failure| {
        // 映射为统一 worker 协议失败。
        WorkerProcessErrorCode::ProtocolFailed.error(format!(
            // 只公开封闭协议类别。
            "Sequence worker output failed with {}.",
            // 使用稳定错误文本。
            failure.code().as_str(),
        ))
    })
}

// 运行固定 sibling sequence step worker。
pub(crate) fn run(
    // 借用已经严格验证的 worker 请求。
    request: &SequenceStepWorkerRequest,
    // 接收调用方取消观察器。
    cancelled: impl Fn() -> bool,
) -> AppResult<SequenceStepRunnerOutput> {
    // 从任何资源创建前启动总生命周期计时。
    let started = Instant::now();
    // 取得可信关联值。
    let request_nonce = request.request_nonce().to_owned();
    // 使用请求唯一剩余 deadline。
    let timeout = Duration::from_millis(u64::from(request.timeout_ms()));
    // 构造严格请求行。
    let request_line = request
        // 序列化固定协议。
        .to_line()
        // 映射协议构造失败。
        .map_err(|failure| WorkerProcessErrorCode::ProtocolFailed.error(failure.code().as_str()))?;
    // 预先构造唯一 cancel control，避免停止路径分配失败。
    let cancel_line = SequenceStepWorkerControl::cancel(&request_nonce)
        // 映射理论 nonce 漂移。
        .and_then(|control| control.to_line())
        // 映射为通用协议失败。
        .map_err(|failure| WorkerProcessErrorCode::ProtocolFailed.error(failure.code().as_str()))?;
    // 定位固定 sibling，不接受调用方路径。
    let executable = sibling_companion_path(WORKER_FILE_NAME, "sequence step worker")?;
    // 委托封闭路径运行器。
    run_fixed(
        // 传入固定 sibling 路径。
        &executable,
        // 生产 worker 不接受任何 argv。
        &[],
        // 聚合拥有型协议输入与时间预算。
        SequenceStepRunPlan {
            // 转移严格请求行。
            request_line,
            // 转移 cancel 行。
            cancel_line,
            // 转移可信 nonce。
            request_nonce,
            // 保存原始起点。
            started,
            // 保存硬 deadline。
            timeout,
        },
        // 传入取消观察器。
        |_| cancelled(),
    )
}

// 使用已由生产入口冻结的精确 worker 路径执行生命周期。
fn run_fixed(
    // 接收固定 sibling 绝对路径。
    executable: &Path,
    // 接收仅由私有测试 fixture 使用的冻结参数。
    arguments: &[String],
    // 取得拥有型协议输入与时间预算。
    plan: SequenceStepRunPlan,
    // 接收调用方取消观察器。
    cancelled: impl Fn(bool) -> bool,
) -> AppResult<SequenceStepRunnerOutput> {
    // 借用 plan 内可信 nonce。
    let request_nonce = plan.request_nonce.as_str();
    // 复制单调时间与硬 deadline。
    let started = plan.started;
    // 复制硬 deadline。
    let timeout = plan.timeout;
    // 建立 worker stdin 管道。
    let stdin_pipe = inherited_pipe()?;
    // parent 只保留写端。
    make_parent_only(&stdin_pipe.write)?;
    // 建立 worker stdout 管道。
    let stdout_pipe = inherited_pipe()?;
    // parent 只保留读端。
    make_parent_only(&stdout_pipe.read)?;
    // 建立 worker stderr 管道。
    let stderr_pipe = inherited_pipe()?;
    // parent 只保留读端。
    make_parent_only(&stderr_pipe.read)?;
    // 创建关闭即终止 Job。
    let job = create_kill_on_close_job()?;
    // 初始化标准句柄启动信息。
    let startup = STARTUPINFOW {
        // 设置结构长度。
        cb: u32::try_from(std::mem::size_of::<STARTUPINFOW>()).map_err(|_| {
            // 返回结构长度溢出。
            WorkerProcessErrorCode::StartFailed.error("STARTUPINFOW size overflowed.")
        })?,
        // 告知系统使用指定 stdio。
        dwFlags: STARTF_USESTDHANDLES,
        // child 读取 stdin。
        hStdInput: stdin_pipe.read.raw(),
        // child 写入 stdout。
        hStdOutput: stdout_pipe.write.raw(),
        // child 写入独立 stderr。
        hStdError: stderr_pipe.write.raw(),
        // 其他字段使用默认值。
        ..Default::default()
    };
    // 初始化进程创建结果。
    let mut process = PROCESS_INFORMATION::default();
    // 编码精确 executable 路径。
    let executable_wide = wide_path(executable);
    // 构造由私有调用方冻结的命令行。
    let mut command_line = wide_command_line(executable, arguments);
    // 挂起创建 worker，先绑定 Job 再恢复。
    unsafe {
        CreateProcessW(
            // 使用精确应用路径。
            PCWSTR(executable_wide.as_ptr()),
            // 传入可修改命令行。
            Some(PWSTR(command_line.as_mut_ptr())),
            // 不继承进程安全描述符。
            None,
            // 不继承线程安全描述符。
            None,
            // 只继承显式 child 管道端。
            true,
            // 隐藏窗口、挂起并保留 Unicode 环境。
            CREATE_NO_WINDOW | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            // 继承当前环境。
            None,
            // 继承当前目录。
            PCWSTR::null(),
            // 传入 stdio 启动信息。
            &startup,
            // 接收进程和主线程 handle。
            &mut process,
        )
    }
    // 映射进程创建失败。
    .map_err(|error| worker_start_error("CreateProcessW(sequence worker)", error))?;
    // 接管进程 handle。
    let process_handle = OwnedHandle::new(process.hProcess, "CreateProcessW(sequence process)")?;
    // 接管主线程 handle。
    let thread_handle = match OwnedHandle::new(process.hThread, "CreateProcessW(sequence thread)") {
        // 保存有效线程 handle。
        Ok(handle) => handle,
        // 无效线程 handle 时直接回收未绑定进程。
        Err(error) => {
            // 终止仍挂起的进程。
            let _ = unsafe { TerminateProcess(process_handle.raw(), 2) };
            // 有界等待退出。
            let _ = unsafe { WaitForSingleObject(process_handle.raw(), 5_000) };
            // 返回原错误。
            return Err(error);
        }
    };
    // 在恢复和写请求前绑定 Job。
    if let Err(error) = unsafe { AssignProcessToJobObject(job.raw(), process_handle.raw()) } {
        // Job 未取得所有权时直接终止挂起进程。
        let _ = unsafe { TerminateProcess(process_handle.raw(), 2) };
        // 有界等待退出。
        let _ = unsafe { WaitForSingleObject(process_handle.raw(), 5_000) };
        // 返回稳定绑定失败。
        return Err(worker_start_error(
            "AssignProcessToJobObject(sequence worker)",
            error,
        ));
    }
    // 创建 accepted 共享状态。
    let accepted = Arc::new(AtomicBool::new(false));
    // 创建 reader 失败共享状态。
    let reader_failed = Arc::new(AtomicBool::new(false));
    // 启动实时 stdout reader。
    let stdout_reader = spawn_sequence_reader(
        // 转移 parent stdout 读端。
        stdout_pipe.read,
        // 复制小 nonce。
        request_nonce.to_owned(),
        // 共享 accepted 状态。
        Arc::clone(&accepted),
        // 共享 reader 失败状态。
        Arc::clone(&reader_failed),
    )?;
    // 启动有界 stderr reader。
    let stderr_reader = spawn_reader(stderr_pipe.read, MAXIMUM_STDERR_BYTES)?;
    // parent 关闭 child stdin 读端副本。
    drop(stdin_pipe.read);
    // parent 关闭 child stdout 写端副本。
    drop(stdout_pipe.write);
    // parent 关闭 child stderr 写端副本。
    drop(stderr_pipe.write);
    // 在恢复 worker 前观察已经耗尽的 deadline 或既有取消。
    let initial_stop = if cancelled(false) {
        // 同一观察中取消优先。
        Some(SequenceStepRunnerStop::Cancelled)
    } else if started.elapsed() >= timeout {
        // 启动阶段已耗尽硬 deadline。
        Some(SequenceStepRunnerStop::Deadline)
    } else {
        // 仍可安全恢复 worker。
        None
    };
    // pre-dispatch 停止不得让挂起 worker 执行首条指令。
    if let Some(stop) = initial_stop {
        // 回收已绑定但尚未恢复的完整 Job。
        terminate_and_reap(&job, &process_handle);
        // 关闭未写入请求的 stdin。
        drop(stdin_pipe.write);
        // 取得理论上为空的 stdout。
        let stdout = join_reader(stdout_reader)?;
        // 取得理论上为空的 stderr。
        let stderr = join_reader(stderr_reader)?;
        // 挂起 worker 不得产生 stderr。
        if !stderr.is_empty() {
            // 返回稳定协议失败。
            return Err(WorkerProcessErrorCode::ProtocolFailed.error(
                // 不回显诊断内容。
                "The suspended sequence worker wrote diagnostics before dispatch.",
            ));
        }
        // 零帧是唯一可信 pre-dispatch 观察。
        let observation = parse_output(stdout, request_nonce, true)?;
        // 返回已强制回收且无退出码的停止事实。
        return Ok(SequenceStepRunnerOutput {
            // 保存零帧观察。
            observation,
            // 保存取消或 deadline 原因。
            stop,
            // 强制回收不公开终止码。
            exit_code: None,
            // Job 已强制回收。
            forced_reap: true,
        });
    }
    // 恢复已在 Job 内的主线程。
    if unsafe { ResumeThread(thread_handle.raw()) } == u32::MAX {
        // 回收完整 Job。
        terminate_and_reap(&job, &process_handle);
        // 回收 reader。
        let _ = join_reader(stdout_reader);
        // 回收 stderr reader。
        let _ = join_reader(stderr_reader);
        // 返回稳定恢复失败。
        return Err(WorkerProcessErrorCode::StartFailed.error(
            // 不公开线程 handle。
            "ResumeThread failed for the sequence worker.",
        ));
    }
    // 主线程 handle 不再需要。
    drop(thread_handle);
    // 写入请求但保持 stdin 打开。
    if let Err(error) = write_bytes(&stdin_pipe.write, &plan.request_line) {
        // 请求未完整交付时回收 Job。
        terminate_and_reap(&job, &process_handle);
        // 回收 stdout reader。
        let _ = join_reader(stdout_reader);
        // 回收 stderr reader。
        let _ = join_reader(stderr_reader);
        // 返回写入失败。
        return Err(error);
    }
    // 初始没有外部停止原因。
    let mut stop = SequenceStepRunnerStop::Completed;
    // 初始没有发送 control。
    let mut cancel_sent = false;
    // 初始没有记录协作停止起点。
    let mut stop_requested_at = None;
    // 初始没有强制回收。
    let mut forced_reap = false;
    // 轮询进程、取消、deadline 与 reader 失败。
    loop {
        // 检查 worker 是否退出。
        let wait = unsafe { WaitForSingleObject(process_handle.raw(), WAIT_SLICE_MS) };
        // 正常退出结束轮询。
        if wait == WAIT_OBJECT_0 {
            // 保留当前停止原因。
            break;
        }
        // wait 失败必须回收 Job。
        if wait == WAIT_FAILED {
            // 强制回收完整 Job。
            terminate_and_reap(&job, &process_handle);
            // 关闭 stdin。
            drop(stdin_pipe.write);
            // 回收 stdout reader。
            let _ = join_reader(stdout_reader);
            // 回收 stderr reader。
            let _ = join_reader(stderr_reader);
            // 返回稳定等待失败。
            return Err(WorkerProcessErrorCode::WaitFailed.error(
                // 不公开 wait 状态。
                "Waiting for the sequence worker failed.",
            ));
        }
        // 未知非 timeout 状态也必须回收 Job。
        if wait != WAIT_TIMEOUT {
            // 强制回收完整 Job。
            terminate_and_reap(&job, &process_handle);
            // 关闭 stdin。
            drop(stdin_pipe.write);
            // 回收 stdout reader。
            let _ = join_reader(stdout_reader);
            // 回收 stderr reader。
            let _ = join_reader(stderr_reader);
            // 返回稳定等待失败。
            return Err(WorkerProcessErrorCode::WaitFailed.error(
                // 不公开 wait 状态。
                "The sequence worker returned an unknown wait state.",
            ));
        }
        // reader 失败时立即回收以避免管道背压死锁。
        if reader_failed.load(Ordering::Acquire) {
            // 强制回收完整 Job。
            terminate_and_reap(&job, &process_handle);
            // 标记强制回收。
            forced_reap = true;
            // 结束轮询并在 join 时返回 reader 错误。
            break;
        }
        // 尚未发送 control 时计算取消优先触发。
        if !cancel_sent {
            // 读取当前单调耗时。
            let elapsed = started.elapsed();
            // 计算唯一停止触发。
            match stop_trigger(
                // 把当前 accepted 事实只提供给私有测试观察器。
                cancelled(accepted.load(Ordering::Acquire)),
                // 传入当前耗时。
                elapsed,
                // 传入硬 deadline。
                timeout,
            ) {
                // 未触发时继续等待。
                StopTrigger::None => {}
                // 调用方取消触发。
                StopTrigger::Cancelled => {
                    // 保存停止原因。
                    stop = SequenceStepRunnerStop::Cancelled;
                    // 尝试发送唯一 cancel；失败仍由 Job 回收。
                    let _ = write_bytes(&stdin_pipe.write, &plan.cancel_line);
                    // 防止重复发送。
                    cancel_sent = true;
                    // 记录协作停止起点。
                    stop_requested_at = Some(Instant::now());
                }
                // deadline 预留窗口触发。
                StopTrigger::Deadline => {
                    // 保存 deadline 原因。
                    stop = SequenceStepRunnerStop::Deadline;
                    // 尝试发送唯一 cancel；失败仍由 Job 回收。
                    let _ = write_bytes(&stdin_pipe.write, &plan.cancel_line);
                    // 防止重复发送。
                    cancel_sent = true;
                    // 记录协作停止起点。
                    stop_requested_at = Some(Instant::now());
                }
            }
        }
        // 调用方取消只等待一个固定协作窗口。
        if stop == SequenceStepRunnerStop::Cancelled
            // 必须已经发送过 cancel。
            && stop_requested_at
                // 读取协作窗口耗时。
                .is_some_and(|requested_at| requested_at.elapsed() >= COOPERATIVE_STOP_WINDOW)
        {
            // 协作窗口耗尽后回收完整 Job。
            terminate_and_reap(&job, &process_handle);
            // 标记强制回收。
            forced_reap = true;
            // 结束轮询。
            break;
        }
        // 硬 deadline 到达时必须终止完整 Job。
        if started.elapsed() >= timeout {
            // 强制回收所有 worker 后代。
            terminate_and_reap(&job, &process_handle);
            // 标记强制回收。
            forced_reap = true;
            // deadline 是默认停止原因；同轮取消仍保持优先。
            if stop == SequenceStepRunnerStop::Completed {
                // 保存 deadline 原因。
                stop = SequenceStepRunnerStop::Deadline;
            }
            // 结束轮询。
            break;
        }
    }
    // parent 完成 control 生命周期后关闭 stdin。
    drop(stdin_pipe.write);
    // 正常退出时读取确定退出码。
    let exit_code = if forced_reap {
        // 强制回收不公开内核终止码。
        None
    } else {
        // 查询正常或协作退出码。
        Some(process_exit_code(&process_handle)?)
    };
    // 清理主 worker 退出后仍留在 Job 的后代。
    terminate_remaining_job_members(&job, &process_handle)?;
    // 取得完整或部分 stdout。
    let stdout = join_reader(stdout_reader)?;
    // 取得 stderr。
    let stderr = join_reader(stderr_reader)?;
    // stderr 必须为空。
    if !stderr.is_empty() {
        // 返回固定协议失败。
        return Err(WorkerProcessErrorCode::ProtocolFailed.error(
            // 不回显诊断内容。
            "The sequence worker wrote diagnostics to stderr.",
        ));
    }
    // 强制回收只允许零帧或 accepted-only 部分日志。
    let observation = parse_output(stdout, request_nonce, forced_reap)?;
    // 返回完整生命周期事实。
    Ok(SequenceStepRunnerOutput {
        // 保存最后可靠观察。
        observation,
        // 保存停止原因。
        stop,
        // 保存可选退出码。
        exit_code,
        // 保存回收事实。
        forced_reap,
    })
}

// 编译停止优先级与 accepted 实时观察回归。
#[cfg(test)]
#[path = "worker_process_sequence_tests.rs"]
mod tests;
