//! 通过固定 Chromium 参数建立工具自有的隔离浏览器协议会话。

// 导入有界输入、回环协议探测、进程和并发原语。
use std::{
    // 导入文件读取。
    fs,
    // 导入有界行读取与协议输出。
    io::{BufRead, BufReader, Read, Write},
    // 导入回环 TCP 协议探测。
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    // 导入路径借用。
    path::Path,
    // 导入固定子进程模板。
    process::Child,
    // 导入输入线程通道。
    sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError},
    // 导入输入线程与单调期限。
    thread,
    // 导入有界等待时长。
    time::{Duration, Instant},
};

// 加载私有会话握手输出组件。
#[path = "browser_session_worker_output.rs"]
mod output;
// 加载私有 Chromium 命令模板组件。
#[path = "browser_session_worker_command.rs"]
mod command;

// 导入固定 Chromium 命令模板。
use command::browser_command;
// 导入封闭会话握手输出函数。
use output::{write_accepted, write_failed, write_not_dispatched, write_ready, write_unknown};

// 导入私有协议、runtime、profile 与安全 nonce Component。
use crate::components::{
    // 导入固定 CDP target 会话所有者。
    browser_cdp_session::BrowserCdpSession,
    // 导入页面取消协议。
    browser_page_protocol::BrowserPageWorkerInput,
    // 导入固定页面命令帧处理器。
    browser_page_worker::{self, PageInputDisposition},
    // 导入工具自有空 profile 所有者。
    browser_profile::BrowserProfile,
    // 导入认证 Chromium runtime 发现器。
    browser_runtime,
    // 导入冻结的打开握手协议。
    browser_session_protocol::{
        BrowserSessionSource, BrowserSessionWorkerInput, MAXIMUM_INPUT_BYTES,
    },
    // 导入系统随机 nonce。
    secure_nonce_windows::random_nonce,
};

// 固定 Chromium 调试端口事实文件名。
const DEVTOOLS_ACTIVE_PORT_FILE: &str = "DevToolsActivePort";
// 固定协议探测响应上限。
const MAXIMUM_PROBE_BYTES: usize = 64 * 1024;
// 固定 worker 轮询粒度。
const POLL_INTERVAL: Duration = Duration::from_millis(10);
// 固定浏览器优雅回收等待上限。
const BROWSER_REAP_TIMEOUT: Duration = Duration::from_secs(5);

// 表示 stdin reader 向会话生命周期报告的封闭事件。
enum InputEvent {
    // 携带一个未解析的有界 JSON 行。
    Line(String),
    // 表示 parent 已关闭 stdin。
    Eof,
    // 表示输入资源或编码不合法。
    Invalid,
}

// 表示启动等待阶段的封闭停止原因。
enum StartupStop {
    // 表示关联取消已经到达。
    Cancelled,
    // 表示 parent 连接在 final 前断开。
    Disconnected,
    // 表示总 deadline 到达。
    Deadline,
    // 表示浏览器进程提前退出。
    BrowserExited,
    // 表示调试协议事实已经就绪。
    Ready {
        // 保存仅供 worker 建连的回环端口。
        port: u16,
        // 保存仅供 worker 建连的 browser websocket 路径。
        path: String,
    },
}

// 从 stdin 持续读取严格有界的 JSON Lines。
fn spawn_input_reader() -> Result<Receiver<InputEvent>, ()> {
    // 建立 worker 内部单生产者通道。
    let (sender, receiver) = mpsc::channel();
    // 启动唯一 stdin reader。
    thread::Builder::new()
        // 使用固定诊断线程名。
        .name("act-browser-session-input".to_owned())
        // 在线程内独占 stdin 锁。
        .spawn(move || {
            // 取得当前进程标准输入。
            let stdin = std::io::stdin();
            // 缓冲同步匿名管道读取。
            let mut reader = BufReader::new(stdin.lock());
            // 持续读取 open 后的可选 cancel。
            loop {
                // 保存一条原始 UTF-8 字节行。
                let mut bytes = Vec::new();
                // 读取到换行或 EOF。
                let read = reader.read_until(b'\n', &mut bytes);
                // 按读取结果投影封闭事件。
                let event = match read {
                    // 零字节表示 parent 已关闭管道。
                    Ok(0) => InputEvent::Eof,
                    // 超过硬上限时不继续解析输入。
                    Ok(_) if bytes.len() > MAXIMUM_INPUT_BYTES.saturating_add(1) => {
                        InputEvent::Invalid
                    }
                    // 正常字节转换为严格 UTF-8 单行。
                    Ok(_) => match normalize_line(bytes) {
                        // 返回有界协议行。
                        Some(line) => InputEvent::Line(line),
                        // 非法 framing 或 UTF-8 失败闭合。
                        None => InputEvent::Invalid,
                    },
                    // 管道读取失败视为断开。
                    Err(_) => InputEvent::Eof,
                };
                // 保存当前事件是否终止 reader。
                let terminal = !matches!(event, InputEvent::Line(_));
                // parent 生命周期结束后允许 reader 静默退出。
                if sender.send(event).is_err() || terminal {
                    // 结束唯一 reader。
                    break;
                }
            }
        })
        // 隐藏线程创建细节。
        .map_err(|_| ())?;
    // 返回事件接收端。
    Ok(receiver)
}

// 把 read_until 结果规范化为无换行 JSON 文本。
fn normalize_line(mut bytes: Vec<u8>) -> Option<String> {
    // 移除唯一允许的 LF 终止符。
    if bytes.last() == Some(&b'\n') {
        // 删除 LF。
        bytes.pop();
        // 兼容 Windows CRLF。
        if bytes.last() == Some(&b'\r') {
            // 删除 CR。
            bytes.pop();
        }
    }
    // 空行不是协议帧。
    if bytes.is_empty() || bytes.len() > MAXIMUM_INPUT_BYTES {
        // 拒绝空输入和超限输入。
        return None;
    }
    // 严格转换 UTF-8。
    String::from_utf8(bytes).ok()
}

// 读取并验证工具 profile 内的 DevToolsActivePort。
fn read_devtools_endpoint(profile: &Path) -> Option<(u16, String)> {
    // 读取 Chromium 写入的私有事实文件。
    let text = fs::read_to_string(profile.join(DEVTOOLS_ACTIVE_PORT_FILE)).ok()?;
    // 按固定两行形状解析。
    let mut lines = text.lines();
    // 第一行必须是非零端口。
    let port = lines
        // 读取端口文本。
        .next()?
        // 解析无符号端口。
        .parse::<u16>()
        // 丢弃解析失败。
        .ok()
        // 拒绝保留端口零。
        .filter(|port| *port != 0)?;
    // 第二行必须是 browser websocket 路径。
    let path = lines
        // 读取唯一 websocket 路径。
        .next()?
        // 只接受固定 DevTools browser 前缀。
        .strip_prefix("/devtools/browser/")?;
    // 路径尾部必须是有界安全 token。
    if path.is_empty()
        // 限制 token 长度。
        || path.len() > 256
        // 只接受 URL path 安全集合。
        || !path
            // 遍历 ASCII 字节。
            .bytes()
            // 拒绝分隔符和控制字符。
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        // 不允许额外第三行。
        || lines.next().is_some()
    {
        // 非规范事实失败闭合。
        return None;
    }
    // 返回仅供 worker 内部探测的私有 endpoint。
    Some((port, format!("/devtools/browser/{path}")))
}

// 通过回环 HTTP 验证 Chromium 调试协议已响应。
fn probe_devtools(port: u16, expected_path: &str, remaining: Duration) -> bool {
    // 单次连接预算不得超过总期限。
    let timeout = remaining.min(Duration::from_millis(250));
    // 零预算不执行网络调用。
    if timeout.is_zero() {
        // 报告尚未就绪。
        return false;
    }
    // 只连接 IPv4 回环。
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    // 建立有界回环连接。
    let Ok(mut stream) = TcpStream::connect_timeout(&address, timeout) else {
        // endpoint 尚未监听。
        return false;
    };
    // 限制读写等待。
    let _ = stream.set_read_timeout(Some(timeout));
    // 限制请求写入等待。
    let _ = stream.set_write_timeout(Some(timeout));
    // 构造不含授权或调用方数据的固定探测。
    let request = format!(
        // 使用 HTTP/1.0 保证服务端响应后关闭连接。
        "GET /json/version HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
    // 写入完整固定请求。
    if stream.write_all(request.as_bytes()).is_err() {
        // 写入失败表示尚未就绪。
        return false;
    }
    // 读取有界响应。
    let mut response = Vec::new();
    // 强制响应上限并保留一个溢出探针字节。
    if stream
        // 借用连接并限制读取。
        .take(u64::try_from(MAXIMUM_PROBE_BYTES).unwrap_or(u64::MAX).saturating_add(1))
        // 读取到连接关闭。
        .read_to_end(&mut response)
        // 网络错误失败闭合。
        .is_err()
        // 拒绝超限响应。
        || response.len() > MAXIMUM_PROBE_BYTES
    {
        // 响应不可信。
        return false;
    }
    // 严格转换 UTF-8 供封闭检查。
    let Ok(response) = String::from_utf8(response) else {
        // 非 UTF-8 响应失败闭合。
        return false;
    };
    // 响应必须成功且回显同一 browser websocket 路径。
    (response.starts_with("HTTP/1.1 200 ") || response.starts_with("HTTP/1.0 200 "))
        // 同时要求响应体包含准确路径。
        && response.contains(expected_path)
}

// 检查是否收到与当前 open 关联的取消或断开。
fn poll_stop(receiver: &Receiver<InputEvent>, request_nonce: &str) -> Option<StartupStop> {
    // 非阻塞读取当前唯一待处理项。
    match receiver.try_recv() {
        // 解析并核对 cancel。
        Ok(InputEvent::Line(line)) => {
            // 只接受同版本同 nonce 的 cancel。
            match BrowserSessionWorkerInput::parse_line(&line) {
                // 关联 cancel 请求停止。
                Ok(BrowserSessionWorkerInput::Cancel {
                    // 取得关联 nonce。
                    request_nonce: nonce,
                    // 忽略已验证固定版本。
                    ..
                }) if nonce == request_nonce => {
                    // 返回确定取消。
                    Some(StartupStop::Cancelled)
                }
                // 任何第二个 open、关联漂移或非法行都视为断开协议。
                _ => Some(StartupStop::Disconnected),
            }
        }
        // EOF 表示 parent 失联。
        Ok(InputEvent::Eof) | Ok(InputEvent::Invalid) => {
            // 返回保守断开。
            Some(StartupStop::Disconnected)
        }
        // 当前没有更多输入。
        Err(TryRecvError::Empty) => None,
        // sender 异常消失等价断开。
        Err(TryRecvError::Disconnected) => Some(StartupStop::Disconnected),
    }
}

// 等待浏览器调试协议、取消、断开或 deadline。
fn wait_until_ready(
    // 借用启动后的浏览器进程。
    child: &mut Child,
    // 借用工具自有 profile。
    profile: &Path,
    // 借用 stdin 事件。
    receiver: &Receiver<InputEvent>,
    // 借用当前请求 nonce。
    request_nonce: &str,
    // 接收绝对单调期限。
    deadline: Instant,
) -> StartupStop {
    // 持续轮询有限状态。
    loop {
        // 取消和断开优先于新 dispatch 探测。
        if let Some(stop) = poll_stop(receiver, request_nonce) {
            // 返回输入停止原因。
            return stop;
        }
        // 浏览器提前退出是确定失败。
        match child.try_wait() {
            // 任何退出状态都不能认证 ready。
            Ok(Some(_)) | Err(_) => return StartupStop::BrowserExited,
            // 继续检查私有协议。
            Ok(None) => {}
        }
        // 总期限不因轮询重置。
        let now = Instant::now();
        // 到达总期限后停止。
        if now >= deadline {
            // 返回确定 deadline。
            return StartupStop::Deadline;
        }
        // 如果事实文件合法则执行真实回环协议探测。
        if let Some((port, path)) = read_devtools_endpoint(profile)
            // 使用剩余总预算探测。
            && probe_devtools(port, &path, deadline.saturating_duration_since(now))
        {
            // 协议端点已可信响应。
            return StartupStop::Ready { port, path };
        }
        // 短暂等待避免忙轮询。
        thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}

// 终止并有界回收 worker 自己启动的浏览器主进程。
fn terminate_browser(child: &mut Child) -> bool {
    // 请求终止直接子进程；父进程 Job 负责完整进程树兜底。
    let _ = child.kill();
    // 记录回收起点。
    let started = Instant::now();
    // 有界轮询主进程退出。
    loop {
        // 主进程退出即完成回收确认。
        match child.try_wait() {
            // 确认退出。
            Ok(Some(_)) => return true,
            // 等待状态失败。
            Err(_) => return false,
            // 仍在运行时检查边界。
            Ok(None) if started.elapsed() >= BROWSER_REAP_TIMEOUT => return false,
            // 仍在预算内则继续。
            Ok(None) => thread::sleep(POLL_INTERVAL),
        }
    }
}

// 在 ready 后持有会话直至 cancel、断开或浏览器退出。
fn hold_ready_session(
    // 独占浏览器主进程。
    child: &mut Child,
    // 借用 stdin 事件。
    receiver: &Receiver<InputEvent>,
    // 借用关联 nonce。
    request_nonce: &str,
    // 独占已附着的私有 CDP 会话。
    mut cdp_session: BrowserCdpSession,
    // 借用公开 opaque 会话身份。
    session_id: &str,
) -> i32 {
    // 持续等待生命周期终止事件。
    loop {
        // 以短周期等待输入，兼顾浏览器退出探测。
        match receiver.recv_timeout(POLL_INTERVAL) {
            // 关联 cancel 触发正常关闭。
            Ok(InputEvent::Line(line)) => {
                // 严格解析唯一允许的后续消息。
                if matches!(
                    BrowserSessionWorkerInput::parse_line(&line),
                    Ok(BrowserSessionWorkerInput::Cancel { request_nonce: nonce, .. }) if nonce == request_nonce
                ) {
                    // 回收自有浏览器并报告关闭结果。
                    return if terminate_browser(child) { 0 } else { 2 };
                }
                // 保存当前或已排队的页面命令行。
                let mut page_line = line;
                // 顺序处理主线程在 final 返回竞态中预读的下一命令。
                loop {
                    // 执行可被 stdin 或浏览器退出中断的页面命令。
                    let (disposition, pending_line, session_close) = run_page_line(
                        // 借用浏览器进程供存活检查。
                        child,
                        // 借用 stdin 事件通道。
                        receiver,
                        // 借用当前页面命令。
                        &page_line,
                        // 借用当前会话身份。
                        session_id,
                        // 借用打开请求 nonce 供会话 cancel 关联。
                        request_nonce,
                        // 可变借用 CDP 会话。
                        &mut cdp_session,
                    );
                    // 会话级 cancel 在页面 final 竞态中仍保持正常关闭语义。
                    if session_close {
                        // 回收自有浏览器并按结果退出。
                        return if terminate_browser(child) { 0 } else { 2 };
                    }
                    // 不可信页面状态要求结束整个会话。
                    if matches!(disposition, PageInputDisposition::EndSession) {
                        // 回收浏览器进程树。
                        let _ = terminate_browser(child);
                        // 返回失败。
                        return 2;
                    }
                    // final 边界竞态预读到下一 command 时继续顺序执行。
                    let Some(next_line) = pending_line else {
                        // 没有预读输入则回到外层生命周期等待。
                        break;
                    };
                    // 移交下一页面命令。
                    page_line = next_line;
                }
            }
            // parent 断开时必须回收自有资源。
            Ok(InputEvent::Eof) | Ok(InputEvent::Invalid) | Err(RecvTimeoutError::Disconnected) => {
                // 回收浏览器并按结果退出。
                return if terminate_browser(child) { 0 } else { 2 };
            }
            // 周期超时用于检查浏览器存活。
            Err(RecvTimeoutError::Timeout) => {}
        }
        // 浏览器异常退出使会话失效。
        match child.try_wait() {
            // 已退出时 worker 同步结束。
            Ok(Some(_)) => return 2,
            // 查询失败时尝试回收。
            Err(_) => {
                // 回收自有资源。
                let _ = terminate_browser(child);
                // 返回失败。
                return 2;
            }
            // 浏览器仍存活。
            Ok(None) => {}
        }
    }
}

// 执行一条页面命令并在主线程持续消费取消、断开与浏览器退出。
fn run_page_line(
    // 借用浏览器进程供存活检查。
    child: &mut Child,
    // 借用 stdin reader 通道。
    receiver: &Receiver<InputEvent>,
    // 借用原始页面协议行。
    line: &str,
    // 借用当前公开会话身份。
    session_id: &str,
    // 借用打开请求 nonce。
    session_request_nonce: &str,
    // 可变借用私有 CDP 会话。
    cdp_session: &mut BrowserCdpSession,
) -> (PageInputDisposition, Option<String>, bool) {
    // 非当前会话 command 可直接同步处理。
    let Some(command_nonce) = browser_page_worker::command_nonce(line, session_id) else {
        // 处理 idle cancel、stale 会话或非法输入。
        return (
            // 同步处理非 command 输入。
            browser_page_worker::handle_line(line, session_id, cdp_session),
            // 未预读下一行。
            None,
            // 没有会话级关闭。
            false,
        );
    };
    // 在启动命令线程前取得窄 socket shutdown 能力。
    let Ok(cancellation_handle) = cdp_session.cancellation_handle() else {
        // 无法保持取消语义时拒绝继续使用会话。
        return (PageInputDisposition::EndSession, None, false);
    };
    // scoped 线程保证借用不会逃逸会话生命周期。
    thread::scope(|scope| {
        // 在独立线程执行可能阻塞的 CDP 命令。
        let command = scope.spawn(|| {
            // 执行严格页面帧处理。
            browser_page_worker::handle_line(line, session_id, cdp_session)
        });
        // 保存尚未消费的窄取消句柄。
        let mut cancellation_handle = Some(cancellation_handle);
        // 保存是否观察到会话级中断。
        let mut interrupted = false;
        // 保存 final 返回竞态中预读的下一页面命令。
        let mut pending_line = None;
        // 保存会话级 cancel 事实。
        let mut session_close = false;
        // 主线程持续等待 command 完成或外部停止。
        while !command.is_finished() {
            // 以短周期读取 stdin，保证取消可达。
            match receiver.recv_timeout(POLL_INTERVAL) {
                // 处理一条在途输入。
                Ok(InputEvent::Line(pending)) => {
                    // 判断是否为关联页面取消。
                    let page_cancel = matches!(
                        // 严格解析页面输入。
                        BrowserPageWorkerInput::parse_line(&pending),
                        // 只接受同会话同 nonce 的取消。
                        Ok(BrowserPageWorkerInput::CancelCommand { session_id: pending_session, request_nonce, .. })
                            if pending_session == session_id && request_nonce == command_nonce
                    );
                    // 当前会话的下一 command 可以顺序排队。
                    let next_command = matches!(
                        // 严格解析页面输入。
                        BrowserPageWorkerInput::parse_line(&pending),
                        // 只接受当前会话的 command。
                        Ok(input @ BrowserPageWorkerInput::Command { .. })
                            if input.session_id() == session_id
                    );
                    // 判断是否为关联打开会话取消。
                    let session_cancel = matches!(
                        // 严格解析打开协议。
                        BrowserSessionWorkerInput::parse_line(&pending),
                        // 只接受关联打开请求的 cancel。
                        Ok(BrowserSessionWorkerInput::Cancel { request_nonce: nonce, .. })
                            if nonce == session_request_nonce
                    );
                    // 保存预读命令并等待当前 handler 完成。
                    if next_command {
                        // 保留原始有界行。
                        pending_line = Some(pending);
                        // 停止从 receiver 继续读取。
                        break;
                    }
                    // 页面取消在命令恰已完成时是幂等无操作。
                    if page_cancel && command.is_finished() {
                        // 继续取得命令结果。
                        break;
                    }
                    // 会话级取消无论命令是否刚完成都请求正常关闭。
                    if session_cancel {
                        // 保存正常关闭事实。
                        session_close = true;
                        // 在途命令必须中断为未知。
                        if !command.is_finished()
                            // 取得仍可用的取消句柄。
                            && let Some(handle) = cancellation_handle.take()
                        {
                            // 中断阻塞 I/O。
                            handle.cancel();
                        }
                        // 停止消费输入并等待命令线程结束。
                        break;
                    }
                    // 任何在途第二帧都终止当前顺序连接。
                    interrupted = true;
                    // 消费窄句柄并 shutdown socket。
                    if let Some(handle) = cancellation_handle.take() {
                        // 中断阻塞 I/O。
                        handle.cancel();
                    }
                    // 不再消费后续输入。
                    break;
                }
                // parent EOF、无效输入或 reader 消失都中断在途结果。
                Ok(InputEvent::Eof)
                | Ok(InputEvent::Invalid)
                | Err(RecvTimeoutError::Disconnected) => {
                    // 标记会话中断。
                    interrupted = true;
                    // 消费窄句柄并 shutdown socket。
                    if let Some(handle) = cancellation_handle.take() {
                        // 中断阻塞 I/O。
                        handle.cancel();
                    }
                    // 不再消费后续输入。
                    break;
                }
                // 周期超时用于检查浏览器进程。
                Err(RecvTimeoutError::Timeout) => {
                    // 浏览器退出或查询失败都中断在途结果。
                    if !matches!(child.try_wait(), Ok(None)) {
                        // 标记浏览器中断。
                        interrupted = true;
                        // 消费窄句柄并 shutdown socket。
                        if let Some(handle) = cancellation_handle.take() {
                            // 中断阻塞 I/O。
                            handle.cancel();
                        }
                        // 停止等待。
                        break;
                    }
                }
            }
        }
        // scoped join 应只在 panic 时失败。
        let disposition = command.join().unwrap_or(PageInputDisposition::EndSession);
        // 中断后的连接不可复用。
        if interrupted {
            // 结束整个会话。
            (PageInputDisposition::EndSession, None, session_close)
        } else {
            // 返回命令去向与可选预读行。
            (disposition, pending_line, session_close)
        }
    })
}

// 执行已经验证的 isolated-profile 打开请求。
fn execute_isolated(
    // 借用输入通道。
    receiver: &Receiver<InputEvent>,
    // 借用请求 nonce。
    request_nonce: &str,
    // 接收冻结总期限。
    timeout_ms: u32,
) -> i32 {
    // 从 worker 接受 open 时开始唯一总期限。
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    // 在 accepted 前发现固定 runtime。
    let Some(runtime) = browser_runtime::find() else {
        // runtime 缺失确定未派发。
        let _ = write_not_dispatched(
            request_nonce,
            "BROWSER_UNAVAILABLE",
            "No certified Chromium runtime is available for an isolated browser session.",
        );
        // 返回稳定失败退出码。
        return 2;
    };
    // 在 accepted 前创建工具自有空 profile。
    let profile = match BrowserProfile::create_session() {
        // 保存唯一 profile 所有者。
        Ok(profile) => profile,
        // profile 创建失败确定未派发。
        Err(_) => {
            // 输出不含路径的失败。
            let _ = write_not_dispatched(
                request_nonce,
                "TEMP_PROFILE_FAILED",
                "The isolated browser profile could not be created.",
            );
            // 返回稳定失败退出码。
            return 2;
        }
    };
    // profile 创建期间到达的取消或断开必须在业务接受前停止。
    if poll_stop(receiver, request_nonce).is_some() {
        // 空 profile 随当前作用域同步清理。
        return 2;
    }
    // profile 创建已经耗尽总预算时不得再声明接受或启动浏览器。
    if Instant::now() >= deadline {
        // 保持业务未接受并同步清理空 profile。
        return 2;
    }
    // accepted 必须在浏览器 spawn 前 flush。
    if write_accepted(request_nonce).is_err() {
        // parent 已不可达时由 profile Drop 清理空目录。
        return 2;
    }
    // 启动固定 Chromium，不接受 executable、path 或 argv 输入。
    let mut child = match browser_command(&runtime, profile.path()).spawn() {
        // 保存浏览器进程所有权。
        Ok(child) => child,
        // spawn 失败是 accepted 后确定失败。
        Err(_) => {
            // 输出安全失败。
            let _ = write_failed(
                request_nonce,
                "BROWSER_START_FAILED",
                "The isolated Chromium process could not be started.",
            );
            // profile 随作用域清理。
            return 2;
        }
    };
    // 等待协议就绪或生命周期停止。
    match wait_until_ready(
        // 借用浏览器进程。
        &mut child,
        // 借用自有 profile。
        profile.path(),
        // 借用输入通道。
        receiver,
        // 关联请求 nonce。
        request_nonce,
        // 传入不重置总期限。
        deadline,
    ) {
        // 协议就绪时签发 opaque 会话身份。
        StartupStop::Ready { port, path } => {
            // 在签发公开会话前完成真实 CDP 握手和 target 初始化。
            let cdp_session = match BrowserCdpSession::bootstrap(port, &path, deadline) {
                // 保存唯一私有连接所有者。
                Ok(session) => session,
                // 初始化失败必须回收已派发浏览器。
                Err(error) => {
                    // 先终止工具自有浏览器。
                    let reaped = terminate_browser(&mut child);
                    // 只有确认回收后才能报告确定失败。
                    if reaped {
                        // 透传稳定错误码与安全消息。
                        let _ = write_failed(request_nonce, error.code, &error.message);
                    } else {
                        // 回收未知时保守投影 OutcomeUnknown。
                        let _ = write_unknown(request_nonce);
                    }
                    // 返回失败。
                    return 2;
                }
            };
            // 使用系统随机源建立会话代际。
            let session_nonce = match random_nonce() {
                // 保存 canonical nonce。
                Ok(nonce) => nonce,
                // 随机源失败时确定回收。
                Err(_) => {
                    // 终止自有浏览器。
                    let _ = terminate_browser(&mut child);
                    // 输出 accepted 后失败。
                    let _ = write_failed(
                        request_nonce,
                        "SESSION_ID_FAILED",
                        "The browser session identity could not be generated.",
                    );
                    // 返回失败。
                    return 2;
                }
            };
            // 组合不含端口、PID 或 profile 的公开 opaque 会话 ID。
            let session_id = format!("s2:bs:{session_nonce}");
            // ready final 必须在进入持有循环前 flush。
            if write_ready(request_nonce, &session_id).is_err() {
                // 输出断开时回收全部自有资源。
                let _ = terminate_browser(&mut child);
                // 返回失败。
                return 2;
            }
            // 持有 worker、浏览器与 profile 直至关闭。
            hold_ready_session(
                // 传入浏览器进程所有权。
                &mut child,
                // 传入 stdin 事件。
                receiver,
                // 关联打开请求。
                request_nonce,
                // 移交私有 CDP 会话。
                cdp_session,
                // 借用公开会话身份。
                &session_id,
            )
        }
        // 关联取消可以确定浏览器已回收。
        StartupStop::Cancelled => {
            // 先回收自有浏览器。
            let reaped = terminate_browser(&mut child);
            // 只有回收确认后输出确定取消。
            if reaped {
                // 输出 accepted 后确定失败。
                let _ = write_failed(
                    request_nonce,
                    "CANCELLED",
                    "The isolated browser session was cancelled and reaped.",
                );
            } else {
                // 无法确认回收时保守报告未知。
                let _ = write_unknown(request_nonce);
            }
            // 返回失败退出码。
            2
        }
        // parent 断开后输出通道也可能已关闭。
        StartupStop::Disconnected => {
            // 尽力回收自有浏览器。
            let _ = terminate_browser(&mut child);
            // 保守投影 accepted-only 语义。
            let _ = write_unknown(request_nonce);
            // 返回失败。
            2
        }
        // deadline 后确定终止自有浏览器。
        StartupStop::Deadline => {
            // 回收浏览器主进程。
            let reaped = terminate_browser(&mut child);
            // 按回收证据选择确定或未知结果。
            if reaped {
                // 输出确定 deadline。
                let _ = write_failed(
                    request_nonce,
                    "DEADLINE_EXCEEDED",
                    "The isolated browser session exceeded its opening deadline and was reaped.",
                );
            } else {
                // 无法确认回收时使用未知。
                let _ = write_unknown(request_nonce);
            }
            // 返回失败。
            2
        }
        // 浏览器在 ready 前退出是确定失败。
        StartupStop::BrowserExited => {
            // 等待主进程句柄完成回收。
            let _ = child.wait();
            // 输出 accepted 后确定失败。
            let _ = write_failed(
                request_nonce,
                "BROWSER_EXITED",
                "The isolated Chromium process exited before its protocol endpoint became ready.",
            );
            // 返回失败。
            2
        }
    }
}

// 执行一条严格 open 并持续管理会话。
fn execute_open(input: BrowserSessionWorkerInput, receiver: &Receiver<InputEvent>) -> i32 {
    // 只接受第一帧 open。
    let BrowserSessionWorkerInput::Open {
        // 取得请求 nonce。
        request_nonce,
        // 取得总 deadline。
        timeout_ms,
        // 取得封闭来源。
        source,
        // 版本已经由 parser 验证。
        ..
    } = input
    else {
        // 首帧 cancel 不可建立打开语义。
        return 2;
    };
    // 按冻结来源执行且绝不自动切换。
    match source {
        // 当前批次实现工具自启空 profile。
        BrowserSessionSource::IsolatedProfile {} => {
            // 委托隔离生命周期。
            execute_isolated(receiver, &request_nonce, timeout_ms)
        }
        // 显式 endpoint 必须等待固定认证 broker，不扫描本机端口。
        BrowserSessionSource::AuthorizedEndpoint { .. } => {
            // 当前没有已认证 endpoint broker，因此确定未派发。
            let _ = write_not_dispatched(
                &request_nonce,
                "AUTHORIZED_ENDPOINT_UNAVAILABLE",
                "No authenticated local browser endpoint broker is available.",
            );
            // 返回稳定失败。
            2
        }
    }
}

// 从标准输入执行固定 browser-session worker。
pub fn run_stdio() -> i32 {
    // 启动唯一有界输入 reader。
    let receiver = match spawn_input_reader() {
        // 保存事件接收端。
        Ok(receiver) => receiver,
        // reader 无法启动时没有可信请求关联值。
        Err(()) => return 2,
    };
    // 等待第一条协议输入或断开。
    let first = match receiver.recv() {
        // 保存第一条有界行。
        Ok(InputEvent::Line(line)) => line,
        // 零帧、非法输入或内部断开不伪造关联 final。
        Ok(InputEvent::Eof) | Ok(InputEvent::Invalid) | Err(_) => return 2,
    };
    // 严格解析冻结输入。
    let input = match BrowserSessionWorkerInput::parse_line(&first) {
        // 保存已验证输入。
        Ok(input) => input,
        // 非法请求不回显不可信 nonce。
        Err(_) => return 2,
    };
    // 执行唯一打开生命周期。
    execute_open(input, &receiver)
}

// 保持纯函数回归靠近 worker 私有实现。
#[cfg(test)]
// 加载独立测试文件避免生产文件超过行数边界。
#[path = "browser_session_worker_tests.rs"]
mod tests;
