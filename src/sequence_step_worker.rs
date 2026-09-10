//! 实现固定 sequence step worker 的双阶段 JSON Lines stdio 生命周期。

// 导入有界行读取、输出和线程间状态工具。
use std::{
    // 导入标准输入缓冲读取与标准输出写入接口。
    io::{BufRead, BufReader, Read, Write},
    // 导入跨控制线程共享的原子失败事实。
    sync::{
        // 保存共享协议失败标记。
        Arc,
        // 保存无锁布尔状态。
        atomic::{AtomicBool, Ordering},
    },
    // 导入唯一 stdin control reader 线程。
    thread,
};

// 导入 provider-neutral JSON 值。
use serde_json::Value;

// 导入双阶段协议、取消 Component、统一错误和 System。
use crate::{
    // 导入 worker 使用的窄 Component。
    components::{
        // 导入进程内取消发布与读取。
        cancellation,
        // 导入严格请求、控制与输出帧。
        sequence_step_protocol::{
            // 导入请求行硬上限。
            MAXIMUM_REQUEST_BYTES,
            // 导入单次控制状态机。
            SequenceStepControlState,
            // 导入协议错误类别。
            SequenceStepProtocolErrorCode,
            // 导入严格 worker 请求。
            SequenceStepWorkerRequest,
            // 导入 accepted/final 帧构造器。
            frames::{
                completed_frame, dispatch_accepted_frame, failed_frame, not_dispatched_frame,
            },
        },
    },
    // 导入统一错误、结果、请求与公开错误投影。
    domain::{AppControlError, AppResult, CommandRequest, error_json},
    // 导入唯一 ComputerControlSystem。
    service::AppControlService,
};

// 固定 transport 或结构化业务拒绝退出码。
const REJECTED_EXIT_CODE: i32 = 2;
// 固定 stdout 无法交付时的独立退出码。
const OUTPUT_FAILED_EXIT_CODE: i32 = 3;

// 从任意 BufRead 读取一个有硬上限的 JSON Lines 帧。
fn read_bounded_line<R: BufRead>(
    // 借用保持后续 control 可读的缓冲输入。
    reader: &mut R,
    // 接收不含行尾的最大 UTF-8 字节数。
    maximum_bytes: usize,
) -> Result<Option<String>, SequenceStepProtocolErrorCode> {
    // 为上限和一个溢出探针预留空间。
    let mut bytes = Vec::with_capacity(maximum_bytes.saturating_add(1));
    // 只允许本次读取消费上限、换行和一个溢出探针。
    let limit = u64::try_from(maximum_bytes)
        // 转换失败时使用最保守零边界。
        .unwrap_or(0)
        // 允许 CRLF 和一个探针字节。
        .saturating_add(2);
    // 使用临时 Take 保留底层 reader 所有权。
    let read = reader
        // 限制本次读取量。
        .take(limit)
        // 读取到单个换行或限制耗尽。
        .read_until(b'\n', &mut bytes)
        // I/O 失败不得回显输入。
        .map_err(|_| SequenceStepProtocolErrorCode::ProtocolFailed)?;
    // EOF 且无字节表示没有后续帧。
    if read == 0 {
        // 返回可判定 EOF。
        return Ok(None);
    }
    // 去除唯一 LF 行尾。
    if bytes.last() == Some(&b'\n') {
        // 删除 LF。
        bytes.pop();
    }
    // 去除可选 CR 行尾。
    if bytes.last() == Some(&b'\r') {
        // 删除 CR。
        bytes.pop();
    }
    // 无换行的超限读取或实际负载过大都拒绝。
    if bytes.len() > maximum_bytes {
        // 返回固定资源错误。
        return Err(SequenceStepProtocolErrorCode::RequestTooLarge);
    }
    // 请求和 control 必须是 UTF-8。
    let text = String::from_utf8(bytes)
        // 编码漂移属于封闭协议失败。
        .map_err(|_| SequenceStepProtocolErrorCode::InvalidArgument)?;
    // 返回单个不含行尾的帧。
    Ok(Some(text))
}

// 把协议错误类别转换为不携带输入的统一错误。
fn protocol_error(code: SequenceStepProtocolErrorCode) -> AppControlError {
    // 使用稳定错误码和固定安全消息。
    AppControlError::new(
        // 转发协议唯一公开文本。
        code.as_str(),
        // 不回显请求、control 或平台事实。
        "The sequence step worker rejected its JSON Lines protocol state.",
    )
}

// 写出并刷新一个已经由协议 Component 构造的完整帧。
fn write_frame<W: Write>(
    // 借用 worker 唯一 stdout writer。
    writer: &mut W,
    // 接收包含固定换行的帧字节。
    frame: &[u8],
) -> AppResult<()> {
    // 一次写出完整有界帧。
    writer.write_all(frame).map_err(|_| {
        // 管道关闭必须结构化失败且不回显内容。
        AppControlError::new(
            // 使用 transport 输出失败码。
            "WORKER_OUTPUT_FAILED",
            // 使用固定诊断。
            "The sequence step worker could not write its protocol frame.",
        )
    })?;
    // accepted 必须在 provider 调用前真正对父进程可见。
    writer.flush().map_err(|_| {
        // 刷新失败与写入失败共享稳定类别。
        AppControlError::new(
            // 使用 transport 输出失败码。
            "WORKER_OUTPUT_FAILED",
            // 使用固定诊断。
            "The sequence step worker could not flush its protocol frame.",
        )
    })
}

// 在独立线程读取至多一个有效 cancel 并拒绝额外帧。
fn consume_controls<R: BufRead>(
    // 取得请求行之后的 stdin reader 所有权。
    mut reader: R,
    // 接收已经验证的请求 nonce。
    request_nonce: String,
    // 接收跨线程协议失败标记。
    protocol_failed: Arc<AtomicBool>,
) {
    // 初始没有接受任何 control。
    let mut state = SequenceStepControlState::new();
    // 读取可选唯一 control。
    let first = read_bounded_line(&mut reader, MAXIMUM_REQUEST_BYTES);
    // 按 EOF、有效帧或失败更新进程内状态。
    match first {
        // 父进程关闭 stdin 且未取消。
        Ok(None) => return,
        // 非空首帧必须是严格 cancel。
        Ok(Some(line)) => {
            // 关联、版本或类别漂移必须取消当前执行。
            if state.accept_cancel(&line, &request_nonce).is_err() {
                // 发布协议失败事实。
                protocol_failed.store(true, Ordering::Release);
                // 让轮询 cancellation 的 Module 尽快停止。
                cancellation::request_cancellation();
                // 不再读取不可信输入。
                return;
            }
            // 有效 cancel 立即传播到进程内 Module。
            cancellation::request_cancellation();
        }
        // 读取、大小或编码失败必须失败闭合。
        Err(_) => {
            // 发布协议失败事实。
            protocol_failed.store(true, Ordering::Release);
            // 同时请求领域执行停止。
            cancellation::request_cancellation();
            // 结束控制线程。
            return;
        }
    }
    // cancel 之后只允许 EOF，任何第三行都是协议失败。
    match read_bounded_line(&mut reader, MAXIMUM_REQUEST_BYTES) {
        // EOF 是唯一合法尾部。
        Ok(None) => {}
        // 第二个 control、空行或读取失败均拒绝。
        Ok(Some(_)) | Err(_) => {
            // 发布重复或额外输入失败。
            protocol_failed.store(true, Ordering::Release);
            // 保持 cancellation 已请求。
            cancellation::request_cancellation();
        }
    }
}

// 执行一个已经严格解析的请求并输出 accepted/final 状态机。
fn execute_parsed<W, C, F>(
    // 取得严格 worker 请求所有权。
    request: SequenceStepWorkerRequest,
    // 借用唯一 stdout writer。
    writer: &mut W,
    // 借用控制线程发布的协议失败事实。
    protocol_failed: &AtomicBool,
    // 接收当前进程内取消观察器。
    cancelled: C,
    // 接收可替换但保持同一 hook 契约的 System executor。
    execute: F,
) -> i32
where
    // writer 必须支持完整写入和刷新。
    W: Write,
    // 取消观察器只读取进程内原子状态。
    C: Fn() -> bool,
    // executor 只消费一次请求和一次可失败 dispatch hook。
    F: FnOnce(CommandRequest, &mut dyn FnMut() -> AppResult<()>) -> AppResult<Value>,
{
    // 复制很小的可信关联值供两帧共享。
    let request_nonce = request.request_nonce().to_owned();
    // 初始尚未发布 accepted。
    let mut accepted = false;
    // 把严格协议命令恢复为统一 System 请求。
    let command = request.into_command().into_request();
    // 使用词法作用域确保 final 写入前释放 hook 的可变借用。
    let execution = {
        // 构造只能由 System 在门禁后调用的观察器。
        let mut dispatch_hook = || {
            // control 协议失败必须先于 accepted 失败闭合。
            if protocol_failed.load(Ordering::Acquire) {
                // 返回稳定协议错误。
                return Err(protocol_error(
                    // 使用关联或顺序失败类别。
                    SequenceStepProtocolErrorCode::ProtocolFailed,
                ));
            }
            // dispatch 前取消不能发布 accepted。
            if cancelled() {
                // 返回可判定取消错误。
                return Err(AppControlError::new(
                    // 使用既有取消文本。
                    "CANCELLED",
                    // 明确尚未 dispatch。
                    "The sequence step was cancelled before dispatch.",
                ));
            }
            // 构造关联到当前请求的唯一 accepted 帧。
            let frame = dispatch_accepted_frame(&request_nonce)
                // 映射理论协议构造失败。
                .map_err(|failure| protocol_error(failure.code()))?;
            // 写出并刷新 accepted 后才提交内存事实。
            write_frame(writer, &frame)?;
            // 保存父进程已经可观察的 accepted。
            accepted = true;
            // 允许 System 进入领域调用。
            Ok(())
        };
        // 通过同一个 ComputerControlSystem 执行一次请求。
        execute(command, &mut dispatch_hook)
    };
    // 已观察到的 control 漂移优先于领域返回，避免把非法 transport 报成成功。
    let execution = if protocol_failed.load(Ordering::Acquire) {
        // 使用稳定协议失败覆盖本次 worker 终态。
        Err(protocol_error(
            // 关联、重复或额外帧都属于状态机失败。
            SequenceStepProtocolErrorCode::ProtocolFailed,
        ))
    } else {
        // 没有 control 漂移时保留真实 System 结果。
        execution
    };
    // 在消费结果前保存确定成功事实。
    let execution_succeeded = execution.is_ok();
    // 把 System 结果投影为唯一 final 帧。
    let frame = match execution {
        // 成功必须已经发布 accepted。
        Ok(result) if accepted => completed_frame(&request_nonce, result),
        // System 在 hook 前成功无法构造可信 final。
        Ok(_) => return OUTPUT_FAILED_EXIT_CODE,
        // hook 后错误属于确定返回的 dispatch 后失败或未知。
        Err(error) if accepted => failed_frame(
            // 保留请求关联。
            &request_nonce,
            // 投影统一公开错误对象。
            error_json(&error)["error"].clone(),
        ),
        // hook 前错误是确定未 dispatch。
        Err(error) => not_dispatched_frame(
            // 保留请求关联。
            &request_nonce,
            // 投影统一公开错误对象。
            error_json(&error)["error"].clone(),
        ),
    };
    // 协议帧构造失败无法安全输出替代帧。
    let Ok(frame) = frame else {
        // 返回 transport 输出失败。
        return OUTPUT_FAILED_EXIT_CODE;
    };
    // final 写入和刷新失败使用独立退出码。
    if write_frame(writer, &frame).is_err() {
        // 父进程不得把缺失 final 当成功。
        return OUTPUT_FAILED_EXIT_CODE;
    }
    // 根据 System 结果返回成功或结构化失败退出码。
    if execution_succeeded && accepted {
        // 确定成功。
        0
    } else {
        // 确定拒绝、失败或未知。
        REJECTED_EXIT_CODE
    }
}

// 从标准输入读取请求、启动 control reader 并执行唯一 System command。
pub fn run_stdio() -> i32 {
    // 建立拥有 stdin 的缓冲 reader，后续可完整移交控制线程。
    let mut reader = BufReader::new(std::io::stdin());
    // 读取第一条严格请求帧。
    let request_line = match read_bounded_line(&mut reader, MAXIMUM_REQUEST_BYTES) {
        // 非空帧进入严格解析。
        Ok(Some(line)) => line,
        // EOF 或 transport 失败都没有可信 nonce 可输出。
        Ok(None) | Err(_) => return REJECTED_EXIT_CODE,
    };
    // 严格解析版本、nonce、deadline 与统一命令。
    let request = match SequenceStepWorkerRequest::parse(&request_line) {
        // 保存已验证请求。
        Ok(request) => request,
        // 无可信关联值时不得伪造 final。
        Err(_) => return REJECTED_EXIT_CODE,
    };
    // 复制关联值供控制线程核验。
    let request_nonce = request.request_nonce().to_owned();
    // 创建跨线程协议失败标记。
    let protocol_failed = Arc::new(AtomicBool::new(false));
    // 复制标记所有权给控制线程。
    let control_failure = Arc::clone(&protocol_failed);
    // 启动唯一 stdin 控制线程且不持有 provider 状态。
    let control_thread = thread::Builder::new()
        // 使用固定诊断线程名。
        .name("act-sequence-step-control".to_owned())
        // reader 线程只发布取消或协议失败事实。
        .spawn(move || consume_controls(reader, request_nonce, control_failure));
    // 控制线程无法创建时必须在 dispatch 前拒绝。
    if control_thread.is_err() {
        // 不存在控制传播能力时不得调用 System。
        return REJECTED_EXIT_CODE;
    }
    // 锁定 stdout 以保持两帧严格顺序。
    let stdout = std::io::stdout();
    // 获取唯一写锁。
    let mut writer = stdout.lock();
    // 创建唯一生产 System。
    let service = AppControlService::new();
    // 执行请求并让 System 拥有 hook 调用时机。
    execute_parsed(
        // 转移严格请求。
        request,
        // 写入进程 stdout。
        &mut writer,
        // 读取控制线程协议状态。
        protocol_failed.as_ref(),
        // 读取进程内 Module 共用取消事实。
        cancellation::is_cancelled,
        // 复用同一个 ComputerControlSystem 路由。
        |command, hook| service.execute_with_dispatch_hook(command, hook),
    )
}

// 编译 worker 投影与有界输入的独立回归。
#[cfg(test)]
#[path = "sequence_step_worker_tests.rs"]
mod tests;
