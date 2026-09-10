//! 连接、认证并按需启动固定同会话长操作 broker。

// 导入固定进程启动、空 stdio、轮询与单调 deadline。
use std::{
    // 安装 Windows 无窗口进程创建标志。
    os::windows::process::CommandExt,
    // 只允许启动固定 sibling executable。
    process::{Command, Stdio},
    // 为 broker 发布 endpoint 提供短轮询。
    thread,
    // 约束完整请求生命周期。
    time::{Duration, Instant},
};

// 导入固定协议请求与进程级取消。
use crate::{
    // 导入共享 IPC 与 peer 认证 Adapter。
    adapters::fixed_local_ipc_windows::{
        // 导入当前 session、固定 sibling 与 peer 认证。
        identity::{authenticate_peer_process, current_session_id, sibling_image_path},
        // 导入固定长操作 endpoint。
        pipe::{ConnectedPipe, FixedLocalEndpointKind},
    },
    // 导入协议请求与取消 Component。
    components::{cancellation, long_operation_protocol::LongOperationBrokerRequest},
    // 导入统一错误边界。
    domain::{AppControlError, AppResult},
};

// 导入 provider-neutral acceptance-aware 错误细节。
use serde_json::json;

// 固定唯一可启动的 broker sibling 文件名。
const BROKER_FILE_NAME: &str = "ai-computer-toolkit-long-operation-broker.exe";
// 让 broker 不继承 launcher 控制台并拥有独立 Ctrl+C 组。
const BROKER_CREATION_FLAGS: u32 = 0x0800_0000 | 0x0000_0200;
// 固定 endpoint 发布轮询片。
const CONNECT_RETRY_SLICE: Duration = Duration::from_millis(10);

// 构造不泄漏进程、pipe、路径或 session 的 broker 错误。
fn broker_unavailable(message: &'static str) -> AppControlError {
    // 使用公共登记错误码。
    AppControlError::new("BROKER_UNAVAILABLE", message)
}

// 构造 submit 写入开始后不可自动重试的未知结果。
fn submit_outcome_unknown(message: &'static str) -> AppControlError {
    // 返回 acceptance-aware 公共错误。
    AppControlError::with_details(
        // 使用稳定未知结果码。
        "OUTCOME_UNKNOWN",
        // 使用不泄漏 pipe 或 broker 状态的消息。
        message,
        // 明确业务接受可能发生且禁止自动重提。
        json!({
            // 写入已经开始，transport 接受可能发生。
            "transportAcceptedMayHaveOccurred": true,
            // broker 可能已经原子建立 handle。
            "businessAcceptedMayHaveOccurred": true,
            // 无 handle 时仍不得猜测动作未执行。
            "retrySafe": false
        }),
    )
}

// 从公开毫秒预算构造单调 deadline。
fn request_deadline(timeout_ms: u32) -> AppResult<Instant> {
    // 零预算不得启动或连接 broker。
    if timeout_ms == 0 {
        // 返回普通参数错误。
        return Err(AppControlError::new(
            // 使用稳定参数码。
            "INVALID_ARGUMENT",
            // 不回显原始输入。
            "The long operation broker timeout must be positive.",
        ));
    }
    // 使用不会溢出的单调加法。
    Instant::now()
        // 转换调用方固定预算。
        .checked_add(Duration::from_millis(u64::from(timeout_ms)))
        // 理论溢出失败闭合。
        .ok_or_else(|| broker_unavailable("The long operation broker deadline is unavailable."))
}

// 启动固定无参数 sibling broker，所有 stdio 都不继承 launcher。
fn start_fixed_broker() -> AppResult<()> {
    // 从当前已认证 executable 目录定位固定 sibling。
    let image = sibling_image_path(BROKER_FILE_NAME)
        // 路径细节不得进入公共响应。
        .map_err(|_| broker_unavailable("The fixed long operation broker is unavailable."))?;
    // 只启动固定绝对 sibling，不接受 caller path、argv 或 shell。
    let mut command = Command::new(image);
    // broker 不接受 stdin。
    command.stdin(Stdio::null());
    // broker 启动错误不会污染 CLI JSON stdout。
    command.stdout(Stdio::null());
    // broker 私有诊断不会污染 CLI stderr 契约。
    command.stderr(Stdio::null());
    // broker 不继承当前控制台或 Ctrl+C 生命周期。
    command.creation_flags(BROKER_CREATION_FLAGS);
    // 启动后立即释放 launcher 侧进程 handle；broker 自己拥有长期生命周期。
    command
        // 不传递任何参数。
        .spawn()
        // 启动失败返回结构化 broker 不可用。
        .map(|_| ())
        // 不公开 Windows 或路径错误。
        .map_err(|_| broker_unavailable("The fixed long operation broker could not be started."))
}

// 连接并认证当前 session 的固定 broker。
fn connect_certified(deadline: Instant, allow_start: bool) -> AppResult<ConnectedPipe> {
    // 取得当前 native session 私有事实。
    let session_id = current_session_id()
        // 不公开 session 数值。
        .map_err(|_| broker_unavailable("The long operation broker session is unavailable."))?;
    // 定位固定 broker 镜像供连接后认证。
    let broker_image = sibling_image_path(BROKER_FILE_NAME)
        // 不公开安装路径。
        .map_err(|_| broker_unavailable("The fixed long operation broker is unavailable."))?;
    // 首次尝试复用已经存在的 broker。
    if let Ok(pipe) = ConnectedPipe::connect_for_until(
        // 固定长操作 endpoint kind。
        FixedLocalEndpointKind::LongOperation,
        // 固定当前登录 session。
        session_id,
        // 共享请求 deadline。
        deadline,
        // 响应进程级取消。
        cancellation::is_cancelled,
    ) {
        // 认证固定镜像、session、SID 与完整性。
        authenticate_pipe(&pipe, &broker_image, session_id)?;
        // 返回已经认证的连接。
        return Ok(pipe);
    }
    // 只有首次 transport 尝试可以启动 broker。
    if allow_start {
        // 启动固定无参数 sibling；并发 launcher 的 loser 会被首实例门禁关闭。
        start_fixed_broker()?;
    }
    // 在总 deadline 内等待 winner 发布固定 endpoint。
    loop {
        // Ctrl+C 只终止当前 launcher 请求，不关闭已存在 broker。
        if cancellation::is_cancelled() {
            // 返回稳定取消。
            return Err(AppControlError::new(
                // 使用公共取消码。
                "CANCELLED",
                // 不公开连接阶段。
                "The long operation broker request was cancelled.",
            ));
        }
        // deadline 到达后停止启动或重连。
        if Instant::now() >= deadline {
            // 返回稳定 timeout。
            return Err(AppControlError::new(
                // 使用公共 timeout 码。
                "TIMEOUT",
                // 不公开 broker 进程状态。
                "The long operation broker did not accept the request deadline.",
            ));
        }
        // 尝试连接固定 endpoint。
        if let Ok(pipe) = ConnectedPipe::connect_for_until(
            // 固定长操作 endpoint kind。
            FixedLocalEndpointKind::LongOperation,
            // 固定当前登录 session。
            session_id,
            // 共享总 deadline。
            deadline,
            // 响应进程级取消。
            cancellation::is_cancelled,
        ) {
            // 连接后必须完成 OS peer 认证。
            authenticate_pipe(&pipe, &broker_image, session_id)?;
            // 返回认证连接。
            return Ok(pipe);
        }
        // 限制 endpoint 发布轮询 CPU。
        thread::sleep(CONNECT_RETRY_SLICE);
    }
}

// 认证 pipe 内核 peer 与固定 broker sibling。
fn authenticate_pipe(
    // 借用已连接 pipe。
    pipe: &ConnectedPipe,
    // 借用固定 broker 镜像。
    broker_image: &std::path::Path,
    // 接收当前 native session。
    session_id: u32,
) -> AppResult<()> {
    // 取得内核记录的 server PID。
    let peer_process_id = pipe
        // 查询 client 角色的 server PID。
        .peer_process_id()
        // 不公开 PID。
        .map_err(|_| broker_unavailable("The long operation broker peer is unavailable."))?;
    // 核对完整镜像、精确 session、SID 与完整性 RID。
    authenticate_peer_process(peer_process_id, broker_image, session_id)
        // 统一为长操作 broker 不可用且不泄漏身份事实。
        .map_err(|_| {
            broker_unavailable("The long operation broker peer could not be authenticated.")
        })
}

// 发送一条 status/cancel 请求，并为幂等操作提供一次重连。
pub(crate) fn exchange(
    // 借用字段封闭协议请求。
    request: &LongOperationBrokerRequest,
    // 接收完整调用预算。
    timeout_ms: u32,
) -> AppResult<String> {
    // 冻结覆盖启动、连接、认证、写入与读取的总 deadline。
    let deadline = request_deadline(timeout_ms)?;
    // status Query 与 cancel Command 都允许在 transport 中断后幂等重发一次。
    for attempt in 0..2 {
        // 首次允许按需启动，第二次允许 broker 崩溃后重新启动。
        let pipe = connect_certified(deadline, true)?;
        // 写入完整单帧请求。
        if pipe.write_json(request).is_err() {
            // 第一次写入失败可安全重连。
            if attempt == 0 {
                // 进入第二次 transport 尝试。
                continue;
            }
            // 第二次失败结束请求。
            return Err(broker_unavailable(
                "The long operation broker request could not be delivered.",
            ));
        }
        // 读取唯一响应 frame。
        match pipe.read_text_until(deadline, cancellation::is_cancelled) {
            // 返回完整响应供协议 Component 验证。
            Ok(text) => return Ok(text),
            // 第一次断连后重发同一幂等逻辑请求。
            Err(_) if attempt == 0 => continue,
            // 第二次失败返回结构化不可用。
            Err(_) => {
                // 不猜测 broker 或 OS 根因。
                return Err(broker_unavailable(
                    "The long operation broker response is unavailable.",
                ));
            }
        }
    }
    // 固定两次循环理论上不可到达。
    Err(broker_unavailable(
        "The long operation broker request could not be completed.",
    ))
}

// 发送一次非幂等 submit，写入开始后绝不自动重试。
pub(crate) fn exchange_submit(
    // 借用字段封闭 submit 请求。
    request: &LongOperationBrokerRequest,
    // 接收完整调用预算。
    timeout_ms: u32,
) -> AppResult<String> {
    // 冻结覆盖启动、连接、认证、写入与读取的总 deadline。
    let deadline = request_deadline(timeout_ms)?;
    // 连接前失败尚未投递请求，可以安全返回普通 transport 错误。
    let pipe = connect_certified(deadline, true)?;
    // 单次写入开始后不再连接或重发同一 submit。
    pipe.write_json(request).map_err(|_| {
        // 无法证明 broker 是否已经接收完整 message frame。
        submit_outcome_unknown(
            "The long operation submission outcome is unknown after delivery began.",
        )
    })?;
    // 只读取当前连接的唯一响应 frame。
    pipe.read_text_until(deadline, cancellation::is_cancelled)
        // 断连、超时或取消都可能发生在业务接受之后。
        .map_err(|_| {
            submit_outcome_unknown(
                "The long operation submission outcome is unknown because its response is unavailable.",
            )
        })
}

// 验证 submit 未知结果保持禁止重试的 acceptance 事实。
#[cfg(test)]
mod submit_tests {
    // 导入被测错误构造器。
    use super::submit_outcome_unknown;

    // 验证未知结果不被降级为普通 broker 不可用。
    #[test]
    fn post_write_failure_is_non_retryable_outcome_unknown() {
        // 构造受控写后失败。
        let error = submit_outcome_unknown("fixture response unavailable");
        // 保留独立未知结果错误码。
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        // 明确业务接受可能发生。
        assert_eq!(error.details["businessAcceptedMayHaveOccurred"], true);
        // 禁止调用方自动重提。
        assert_eq!(error.details["retrySafe"], false);
    }
}
