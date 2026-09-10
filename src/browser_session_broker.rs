//! 固定同会话 browser-session broker 的组合根与生命周期宿主。

// 注册唯一拥有 ComputerControlSystem 的 dispatcher。
mod dispatcher;
// 注册只负责响应编码、绝对截止时间选择与有界写入的宿主 Component。
mod response_transport;

// 导入安全错误输出、共享所有权、线程与连接预算。
use std::{
    // 只向 broker 私有 stdout 写启动失败。
    io::Write,
    // 保存固定主程序镜像路径。
    path::{Path, PathBuf},
    // 在线程间共享纯 Rust runtime 与不可变握手文本。
    sync::{
        // 共享不拥有平台 handle 的值。
        Arc,
        // 接收 acceptor 发布完成事实。
        mpsc,
    },
    // 启动固定 secondary acceptor 集合。
    thread::{self, JoinHandle},
    // 约束单连接读取与宿主轮询。
    time::{Duration, Instant},
};

// 导入 JSON frame kind 读取。
use serde_json::Value;

// 导入固定 IPC、协议、运行时、随机 epoch 与统一错误。
use crate::{
    // 导入固定本机 IPC 与 peer 认证。
    adapters::fixed_local_ipc_windows::{
        // 导入固定 sibling、当前 session 与 peer 认证。
        identity::{authenticate_peer_process, current_session_id, sibling_image_path},
        // 导入首实例 guard、secondary factory 与 owner-preserving accept。
        pipe::{
            // 导入 first-instance guard owner。
            BrowserSessionPipeOwner,
            // 导入同名 secondary listener factory。
            BrowserSessionSecondaryFactory,
            // 导入已连接 pipe owner。
            ConnectedPipe,
            // 导入 owner-preserving accept outcome。
            PersistentServerAccept,
        },
    },
    // 导入取消、协议与共享运行时 Component。
    components::{
        // 导入上一 broker 代际会话 profile 恢复组件。
        browser_profile::BrowserProfile,
        // 导入完整 browser-session broker 协议。
        browser_session_broker_protocol::{
            // 导入严格 cancel、request 与错误码。
            // 导入严格 cancel request。
            BrowserSessionBrokerCancellationRequest,
            // 导入冻结 operation。
            BrowserSessionBrokerOperation,
            // 导入稳定协议错误码。
            BrowserSessionBrokerProtocolErrorCode,
            // 导入严格领域 request。
            BrowserSessionBrokerRequest,
            // 导入 cancel 与 domain response。
            response::{
                // 导入 cancel receipt 投影。
                BrowserSessionBrokerCancelReceipt,
                // 导入 cancel rejection 投影。
                BrowserSessionBrokerCancelRejected,
                // 导入 request response 投影。
                BrowserSessionBrokerResponse,
            },
            // 导入连接状态、epoch 与执行决定。
            state::{
                // 导入每连接 response revision 状态机。
                BrowserSessionBrokerConnection,
                // 导入 live broker epoch。
                BrowserSessionBrokerEpoch,
                // 导入 execution ledger 决定。
                BrowserSessionBrokerExecutionDecision,
            },
            // 导入 server-side 严格 codec。
            wire::{encode_cancel_receipt, encode_cancel_rejected, encode_ready, encode_response},
        },
        // 导入共享 epoch runtime 与等待结果。
        browser_session_broker_runtime::{
            BrowserSessionBrokerRuntime, BrowserSessionBrokerRuntimeWait,
        },
        // 观察进程级取消。
        cancellation,
        // 生成 128 位 CNG 随机 epoch。
        secure_nonce_windows::random_nonce,
    },
    // 导入统一错误投影。
    domain::{AppControlError, AppResult, error_json},
};

// 导入唯一 dispatcher owner 与纯 request 端口。
use dispatcher::{BrowserSessionBrokerDispatchPort, BrowserSessionBrokerDispatcher};
// 导入不接触 System 或领域目标的响应 transport Component。
use response_transport::{send_response, write_transport_text};

// 固定唯一允许连接 broker 的主程序 sibling 文件名。
const MAIN_FILE_NAME: &str = "ai-computer-toolkit.exe";
// 单连接必须在冻结协议最大预算内给出唯一输入 frame。
const TRANSPORT_CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);
// first guard 占用一个实例，其余十五个 secondary 全部由宿主持有。
const SECONDARY_ACCEPTOR_COUNT: usize = 15;
// 使用短轮询观察进程取消或 dispatcher 停止。
const HOST_POLL_INTERVAL: Duration = Duration::from_millis(10);

// 保存一个 acceptor 线程及其唯一 join owner。
struct AcceptorWorker {
    // 保存连接线程 join handle。
    join: JoinHandle<AppResult<()>>,
}

// 服务一条已经由 secondary listener 接受的连接。
fn serve_connection(
    // 借用当前线程拥有的 connected secondary。
    pipe: &ConnectedPipe,
    // 借用固定主程序镜像。
    main_image: &Path,
    // 接收当前 native session。
    broker_session_id: u32,
    // 借用当前 epoch ready 文本。
    ready_text: &str,
    // 借用共享纯 Rust runtime。
    runtime: &Arc<BrowserSessionBrokerRuntime>,
    // 借用不拥有 System 的派发端口。
    dispatch: &BrowserSessionBrokerDispatchPort,
) {
    // 取得内核记录的 client PID。
    let peer_process_id = match pipe.peer_process_id() {
        // 保存可认证 PID。
        Ok(process_id) => process_id,
        // 无法认证的 peer 不得收到任何 JSON。
        Err(_) => return,
    };
    // 在 JSON 前核对固定镜像、session、SID 与完整性。
    if authenticate_peer_process(peer_process_id, main_image, broker_session_id).is_err() {
        // 认证失败只关闭当前 secondary 连接。
        return;
    }
    // 计算唯一输入 frame 的 transport 绝对 deadline。
    let Some(deadline) = Instant::now().checked_add(TRANSPORT_CONNECTION_TIMEOUT) else {
        // 理论时钟溢出时关闭当前连接。
        return;
    };
    // 认证成功后先发送严格 codec 生成的 server-first ready。
    if pipe
        // ready 写入与后续唯一输入共享同一 transport deadline。
        .write_text_until(ready_text, deadline, || should_stop(runtime))
        // 任何写入或取消失败都只关闭当前连接。
        .is_err()
    {
        // 不解析未完成 server-first 握手的 peer 输入。
        return;
    }
    // 每连接最多读取一条 request 或 cancel frame。
    let text = match pipe.read_text_until(deadline, || should_stop(runtime)) {
        // 保存完整有界 UTF-8 frame。
        Ok(text) => text,
        // 超时、取消、断线或 framing 失败只关闭当前连接。
        Err(_) => return,
    };
    // 完整 frame 到达时即冻结 request 预算的服务器单调起点。
    let frame_received_at = Instant::now();
    // 只读取 frame kind 用于路由到各自严格 parser。
    let kind = serde_json::from_str::<Value>(&text)
        // 根必须是对象。
        .ok()
        // 读取 kind 字段。
        .and_then(|value| value.get("kind").and_then(Value::as_str).map(str::to_owned));
    // 每连接只执行一个封闭路由。
    match kind.as_deref() {
        // domain request 进入 request parser 与 execution ledger。
        Some("request") => serve_request(pipe, &text, frame_received_at, runtime, dispatch),
        // cancel 使用独立连接进入 cancel ledger。
        Some("cancel") => serve_cancel(pipe, &text, runtime),
        // ready/response/control 或未知 frame 均失败闭合。
        _ => {}
    }
}

// 解析并服务一条 open/close Command 或 session.inspect Query。
fn serve_request(
    // 借用当前连接。
    pipe: &ConnectedPipe,
    // 借用唯一 request frame。
    text: &str,
    // 接收完整 frame 已到达的单调时刻。
    frame_received_at: Instant,
    // 借用共享 runtime。
    runtime: &Arc<BrowserSessionBrokerRuntime>,
    // 借用 dispatcher 发送端。
    dispatch: &BrowserSessionBrokerDispatchPort,
) {
    // epoch-first parser 在 target/payload 前拒绝 stale envelope。
    let request = match BrowserSessionBrokerRequest::parse_for_epoch(text, runtime.epoch().as_str())
    {
        // 保存完整 strict request。
        Ok(request) => request,
        // 可关联 early rejection 通过无 strict request codec 路径返回。
        Err(failure) => {
            // 只有 transport-accepted failure 才能构造 wire final。
            if let Some(response) = BrowserSessionBrokerResponse::rejected(
                // 绑定 parser failure。
                &failure,
                // early final 固定 revision 零。
                0,
                // 回显认证连接 current epoch。
                runtime.epoch(),
            ) {
                // early response 明确不绑定任意 strict request。
                if let Ok(encoded) = encode_response(
                    // 借用 early rejection。
                    &response,
                    // 禁止 strict request 洗白。
                    None,
                    // 绑定当前认证 epoch。
                    runtime.epoch(),
                ) {
                    // 单次写入结构化 rejection。
                    // 以固定 transport 预算写入早期拒绝。
                    let _ = write_transport_text(pipe, &encoded, runtime);
                }
            }
            // parser failure 永不进入 execution ledger。
            return;
        }
    };
    // 建立即使 ledger 拒绝也不能重置的 request fallback deadline。
    let Some(request_deadline) = frame_received_at.checked_add(Duration::from_millis(u64::from(
        // 使用 strict parser 已验证的剩余毫秒数。
        request.remaining_timeout_ms(),
    ))) else {
        // 时钟理论溢出时失败闭合。
        return;
    };
    // 只允许已接入 dispatcher 的五个 Command 与四个 Query 越过 transport parser。
    if !matches!(
        // 读取冻结 operation。
        request.operation(),
        // 允许 open。
        BrowserSessionBrokerOperation::Open
            // 允许 close。
            | BrowserSessionBrokerOperation::Close
            // 允许无确认且无副作用的 session.inspect Query。
            | BrowserSessionBrokerOperation::SessionInspect
            // 允许确认式页面导航 Command。
            | BrowserSessionBrokerOperation::Navigate
            // 允许有限条件等待 Query。
            | BrowserSessionBrokerOperation::Wait
            // 允许 provider-neutral 元素 Query。
            | BrowserSessionBrokerOperation::Query
            // 允许 confirmation-first 元素点击 Command。
            | BrowserSessionBrokerOperation::Click
            // 允许 confirmation-first 文本输入 Command。
            | BrowserSessionBrokerOperation::Type
            // 允许不改变目标的有界页面截图 Query。
            | BrowserSessionBrokerOperation::Screenshot
    ) {
        // 其余冻结 operation 在对应纵切接通前继续失败闭合。
        return;
    }
    // 每条物理连接建立独立 request revision 状态机。
    let mut connection = BrowserSessionBrokerConnection::default();
    // 绑定本连接唯一 nonce、operation 与 mutation 事实。
    if connection.accept_transport(&request).is_err() {
        // 理论状态漂移失败闭合。
        return;
    }
    // 首次观察决定唯一 Dispatch、Attach 或 Replay。
    let mut observation = match runtime.observe_received_request(frame_received_at, &request) {
        // 保存决定与无丢失等待代际。
        Ok(observation) => observation,
        // ledger 拒绝使用 strict request provenance 编码。
        Err(failure) => {
            // 只有与完整 request 逐字关联的 failure 可构造终态。
            if let Some(response) = BrowserSessionBrokerResponse::rejected_for_request(
                // 绑定 ledger failure。
                &failure,
                // 绑定 strict request。
                &request,
                // 业务前 rejection 固定 revision 零。
                0,
                // 回显 current epoch。
                runtime.epoch(),
            ) {
                // 发送严格 request-bound rejection。
                let _ = send_response(
                    pipe,
                    &request,
                    &mut connection,
                    &response,
                    request_deadline,
                    runtime,
                );
            }
            // ledger rejection 不派发。
            return;
        }
    };
    // 记录本连接是否已经提交唯一 Dispatch 消息。
    let mut dispatched = false;
    // 保存本连接已经写出的最高 response revision。
    let mut last_sent_revision = None;
    // 持续观察 accepted/final，断线不改变 execution 状态。
    loop {
        // 先复制决定，避免借用跨等待。
        let decision = observation.decision().clone();
        // 保存本轮等待使用的绝对 deadline。
        let deadline_ms = match decision {
            // 首次 request 唯一派发一次。
            BrowserSessionBrokerExecutionDecision::Dispatch { deadline_ms, .. } => {
                // 第二次 Dispatch 是状态污染。
                if dispatched {
                    // 停止本连接但不伪造 final。
                    return;
                }
                // 标记已经派发。
                dispatched = true;
                // channel 只移动纯 Rust request，不移动 pipe。
                if dispatch.dispatch(request.clone()).is_err() {
                    // 唯一 dispatcher 不可用时停止整个宿主接入。
                    runtime.stop_accepting();
                    // 关闭当前连接。
                    return;
                }
                // 使用 ledger 首次绝对 deadline 等待 snapshot。
                deadline_ms
            }
            // 同义在途 request 只观察已有或未来 snapshot。
            BrowserSessionBrokerExecutionDecision::Attach {
                deadline_ms,
                response,
                ..
            } => {
                // accepted snapshot 可立即发送一次。
                if let Some(response) = response {
                    // 全局 generation 可能由无关 request/cancel 推进，相同 snapshot 不得重发。
                    if response_is_newer(last_sent_revision, &response) {
                        // 发送失败表示 client 断线，但 execution 继续。
                        if !send_response(
                            pipe,
                            &request,
                            &mut connection,
                            &response,
                            request_deadline,
                            runtime,
                        ) {
                            // 不隐式取消 accepted execution。
                            return;
                        }
                        // 保存已经成功写出的最高 revision。
                        last_sent_revision = Some(response.request_revision());
                    }
                    // 任何 final 都结束本连接。
                    if response.outcome().is_some() {
                        // 返回后关闭 secondary handle。
                        return;
                    }
                }
                // 等待更高 revision 或 terminal replay。
                deadline_ms
            }
            // terminal-wins 只重放一次 final。
            BrowserSessionBrokerExecutionDecision::Replay { response } => {
                // 连接状态允许直接收到 revision 一业务终态 replay。
                let _ = send_response(
                    pipe,
                    &request,
                    &mut connection,
                    &response,
                    request_deadline,
                    runtime,
                );
                // 每连接完成唯一 request 后关闭。
                return;
            }
        };
        // 等待状态变化、deadline 或宿主停止。
        match runtime.wait_for_change(observation.generation(), deadline_ms) {
            // 新 snapshot 到达后重观察同义 request。
            Ok(BrowserSessionBrokerRuntimeWait::Changed) => {
                // 重观察只会 Attach/Replay，绝不延长 deadline。
                observation = match runtime.observe_existing_request(&request) {
                    // 保存最新决定与代际。
                    Ok(observation) => observation,
                    // 状态失败关闭连接并保留 ledger 事实。
                    Err(_) => return,
                };
            }
            // 单连接预算到达时关闭；dispatcher 仍负责权威终态。
            Ok(BrowserSessionBrokerRuntimeWait::DeadlineReached)
            // 宿主停止只关闭连接，不伪造 request cancellation。
            | Ok(BrowserSessionBrokerRuntimeWait::Stopping)
            // 同步状态失败同样关闭连接。
            | Err(_) => return,
        }
    }
}

// 判断 snapshot 是否严格推进本连接已发送 revision。
fn response_is_newer(
    // 接收本连接已发送的最高 revision。
    last_sent_revision: Option<u64>,
    // 借用当前 ledger snapshot。
    response: &BrowserSessionBrokerResponse,
) -> bool {
    // 首个 snapshot 可发送，随后只允许严格更高 revision。
    last_sent_revision.is_none_or(|revision| response.request_revision() > revision)
}

// 解析并服务一条独立 cancel control frame。
fn serve_cancel(
    // 借用当前 connected secondary。
    pipe: &ConnectedPipe,
    // 借用唯一 cancel frame。
    text: &str,
    // 借用 current epoch runtime。
    runtime: &Arc<BrowserSessionBrokerRuntime>,
) {
    // cancel parser 严格验证双 nonce、epoch 与精确字段。
    let cancel = match BrowserSessionBrokerCancellationRequest::parse(text) {
        // 保存严格 cancel。
        Ok(cancel) => cancel,
        // 无 canonical 双 nonce 的失败不得回显 control frame。
        Err(_) => return,
    };
    // 在线性化点去重 cancel 并推进 target。
    match runtime.observe_cancel(&cancel) {
        // 成功返回当前单调 receipt。
        Ok((status, revision)) => {
            // 从严格 cancel 构造关联 receipt。
            let receipt = BrowserSessionBrokerCancelReceipt::new(
                // 绑定原 cancel。
                &cancel,
                // 回显认证 current epoch。
                runtime.epoch(),
                // 保存 cancel revision。
                revision,
                // 保存四态 status。
                status,
            );
            // codec 复核 revision/status 与 current epoch。
            if let Ok(encoded) = encode_cancel_receipt(&receipt, runtime.epoch()) {
                // 单次写入 control response。
                // 以固定 transport 预算写入 cancel receipt。
                let _ = write_transport_text(pipe, &encoded, runtime);
            }
        }
        // stale、nonce conflict 或 ledger full 使用 cancel-rejected。
        Err(failure) => {
            // 只允许 cancel schema 白名单错误。
            if matches!(
                // 读取稳定错误码。
                failure.code(),
                // 允许 stale epoch。
                BrowserSessionBrokerProtocolErrorCode::StaleBrokerEpoch
                    // 允许相同 cancel nonce 异义。
                    | BrowserSessionBrokerProtocolErrorCode::NonceSemanticConflict
                    // 允许固定 ledger 容量失败。
                    | BrowserSessionBrokerProtocolErrorCode::BrokerRequestLedgerFull
            ) {
                // 构造双 nonce 与 current epoch 绑定的拒绝。
                if let Some(rejected) = BrowserSessionBrokerCancelRejected::new(
                    // 绑定严格 cancel。
                    &cancel,
                    // 回显 current epoch。
                    runtime.epoch(),
                    // 传递冻结错误码。
                    failure.code().as_str(),
                ) {
                    // codec 再次复核 expected/current stale iff 关系。
                    if let Ok(encoded) = encode_cancel_rejected(
                        // 借用拒绝投影。
                        &rejected,
                        // 绑定原 cancel。
                        &cancel,
                        // 绑定认证 current epoch。
                        runtime.epoch(),
                    ) {
                        // 单次写入 cancel-rejected。
                        // 以固定 transport 预算写入 cancel rejection。
                        let _ = write_transport_text(pipe, &encoded, runtime);
                    }
                }
            }
        }
    }
}

// 运行一个只持有本线程 secondary pipe 的 acceptor。
fn run_acceptor(
    // 接收不延长 first owner 生命周期的 factory。
    factory: BrowserSessionSecondaryFactory,
    // 共享固定主程序镜像。
    main_image: Arc<PathBuf>,
    // 接收当前 native session。
    broker_session_id: u32,
    // 共享当前 epoch ready 文本。
    ready_text: Arc<String>,
    // 共享纯 Rust runtime。
    runtime: Arc<BrowserSessionBrokerRuntime>,
    // 接收不拥有 System 的派发端口。
    dispatch: BrowserSessionBrokerDispatchPort,
    // 接收一次 listener 发布通知端。
    published: mpsc::SyncSender<bool>,
) -> AppResult<()> {
    // 首次创建固定同名 secondary listener。
    let mut server = match factory.create_listener() {
        // 保存尚未连接的完整 owner。
        Ok(server) => server,
        // 发布失败并返回原错误。
        Err(error) => {
            // 告知组合根此 acceptor 未发布。
            let _ = published.send(false);
            // 返回 listener 创建失败。
            return Err(error);
        }
    };
    // 只有 listener owner 已建立后才发布 ready 事实。
    let _ = published.send(true);
    // 当前线程串行复用自己的 secondary slot。
    loop {
        // owner-preserving accept 在停止时返还 listener。
        match server.accept_persistent(|| should_stop(&runtime)) {
            // pipe 始终留在当前 acceptor 线程。
            PersistentServerAccept::Connected(pipe) => {
                // 认证、ready、单帧协议与等待均不跨线程移动 pipe。
                serve_connection(
                    // 借用当前 connected secondary。
                    &pipe,
                    // 借用固定 sibling image。
                    &main_image,
                    // 固定当前 native session。
                    broker_session_id,
                    // 复用当前 epoch ready 文本。
                    &ready_text,
                    // 共享纯 Rust runtime。
                    &runtime,
                    // 借用纯 request dispatcher 端口。
                    &dispatch,
                );
                // 当前连接结束后先释放 secondary handle。
                drop(pipe);
                // 宿主停止时不再创建新 listener。
                if should_stop(&runtime) {
                    // 正常退出 acceptor。
                    return Ok(());
                }
                // 从仍 live 的 first owner factory 创建替代 secondary。
                server = match factory.create_listener() {
                    // 保存新 listener owner。
                    Ok(server) => server,
                    // listener 基础设施失败会停止整个 epoch。
                    Err(error) => {
                        // 唤醒其他 acceptor 与连接 handler。
                        runtime.stop_accepting();
                        // 返回结构化基础设施错误。
                        return Err(error);
                    }
                };
            }
            // 停止 accept 时先释放返还的 secondary owner。
            PersistentServerAccept::Cancelled(listener) => {
                // 显式释放 listener。
                drop(listener);
                // 正常结束线程。
                return Ok(());
            }
            // accept 失败仍由 outcome 返还完整 owner。
            PersistentServerAccept::Failed(listener, error) => {
                // 先停止全部新接入。
                runtime.stop_accepting();
                // 再释放当前 secondary owner。
                drop(listener);
                // 返回基础设施错误。
                return Err(error);
            }
        }
    }
}

// 启动固定十五个 secondary acceptor 并等待全部 listener 发布。
fn start_acceptors(
    // 接收可克隆 secondary factory。
    factory: BrowserSessionSecondaryFactory,
    // 共享固定主程序镜像。
    main_image: Arc<PathBuf>,
    // 接收当前 native session。
    broker_session_id: u32,
    // 共享 current epoch ready 文本。
    ready_text: Arc<String>,
    // 共享纯 Rust runtime。
    runtime: Arc<BrowserSessionBrokerRuntime>,
    // 接收不拥有 System 的派发端口。
    dispatch: BrowserSessionBrokerDispatchPort,
) -> AppResult<Vec<AcceptorWorker>> {
    // 建立每线程一次的发布事实 channel。
    let (published_tx, published_rx) = mpsc::sync_channel(SECONDARY_ACCEPTOR_COUNT);
    // 预留固定 join owner 集合。
    let mut workers = Vec::with_capacity(SECONDARY_ACCEPTOR_COUNT);
    // 启动固定数量线程以保持 cancel/Attach 独立连接可用。
    for index in 0..SECONDARY_ACCEPTOR_COUNT {
        // 复制不延长 first owner 生命周期的 factory。
        let worker_factory = factory.clone();
        // 共享固定 sibling path。
        let worker_image = Arc::clone(&main_image);
        // 共享不可变 ready 文本。
        let worker_ready = Arc::clone(&ready_text);
        // 共享纯 Rust runtime。
        let worker_runtime = Arc::clone(&runtime);
        // 复制不拥有 System 的 dispatcher 端口。
        let worker_dispatch = dispatch.clone();
        // 复制一次发布通知端。
        let worker_published = published_tx.clone();
        // 启动不携带 nonce 或目标事实的固定 acceptor 线程。
        let join = match thread::Builder::new()
            // index 只用于进程内诊断线程名。
            .name(format!("browser-session-acceptor-{index}"))
            // pipe 在 closure 内创建、连接、使用并释放。
            .spawn(move || {
                // 委托单线程 accept 循环。
                run_acceptor(
                    // 移入弱 factory。
                    worker_factory,
                    // 移入固定 image。
                    worker_image,
                    // 复制 native session。
                    broker_session_id,
                    // 移入 ready 文本。
                    worker_ready,
                    // 移入 runtime clone。
                    worker_runtime,
                    // 移入 dispatcher port。
                    worker_dispatch,
                    // 移入一次发布端。
                    worker_published,
                )
            }) {
            // 保存成功启动的 join owner。
            Ok(join) => join,
            // 线程启动失败时先停止已启动 acceptor。
            Err(_) => {
                // 广播停止事实。
                runtime.stop_accepting();
                // join 已启动线程，确保 secondary 全部释放。
                let _ = join_acceptors(workers);
                // 返回固定宿主错误。
                return Err(unavailable(
                    "The browser session broker could not start its connection acceptors.",
                ));
            }
        };
        // 保存唯一 join owner。
        workers.push(AcceptorWorker { join });
    }
    // 释放组合根额外 sender，使异常线程退出可被观察。
    drop(published_tx);
    // 等待每个线程已经拥有一个 secondary listener。
    for _ in 0..SECONDARY_ACCEPTOR_COUNT {
        // 任一 false 或 channel 提前关闭均停止启动。
        if published_rx.recv().ok() != Some(true) {
            // 阻止其余 acceptor 继续接入。
            runtime.stop_accepting();
            // 回收全部已创建 secondary。
            let _ = join_acceptors(workers);
            // 返回固定发布失败。
            return Err(unavailable(
                "The browser session broker could not publish its connection listeners.",
            ));
        }
    }
    // 返回全部 acceptor join owner。
    Ok(workers)
}

// join 全部 acceptor 并保留首个基础设施错误。
fn join_acceptors(
    // 接收全部 secondary acceptor join owner。
    workers: Vec<AcceptorWorker>,
) -> AppResult<()> {
    // 初始没有错误。
    let mut first_error = None;
    // 每个 secondary owner 必须在线程退出前释放。
    for worker in workers {
        // join 不得遗漏任何线程。
        match worker.join.join() {
            // 正常线程无错误。
            Ok(Ok(())) => {}
            // 保存首个结构化 listener 错误。
            Ok(Err(error)) if first_error.is_none() => first_error = Some(error),
            // 后续错误不覆盖首因。
            Ok(Err(_)) => {}
            // panic 使用固定不可用错误。
            Err(_) if first_error.is_none() => {
                // 不泄漏 panic payload。
                first_error = Some(unavailable(
                    "A browser session connection acceptor stopped unexpectedly.",
                ));
            }
            // 已有首因时忽略后续 panic 文本。
            Err(_) => {}
        }
    }
    // 有首因则返回，否则证明全部 secondary 已释放。
    first_error.map_or(Ok(()), Err)
}

// 判断 accept/read 循环是否应停止。
fn should_stop(
    // 借用 runtime 接入与停止事实。
    runtime: &BrowserSessionBrokerRuntime,
) -> bool {
    // 进程控制台取消优先。
    cancellation::is_cancelled()
        // runtime 停止或锁污染也必须失败闭合。
        || !runtime.accepting_connections().unwrap_or(false)
}

// 运行固定同会话 browser-session broker 代际。
fn run_server() -> AppResult<()> {
    // 取得 broker 当前 native session。
    let broker_session_id = current_session_id()?;
    // 创建并内部占用真正 first-instance guard，永不交给业务 worker。
    let owner = BrowserSessionPipeOwner::create(broker_session_id)?;
    // 取得唯一 broker 所有权后只回收上一代专用会话 profile。
    BrowserProfile::cleanup_stale_sessions()?;
    // 取得不延长 first owner 生命周期的同名 secondary factory。
    let factory = owner.secondary_factory();
    // 定位固定主程序镜像供每连接认证。
    let main_image = Arc::new(sibling_image_path(MAIN_FILE_NAME)?);
    // 进程代际只生成一次 CNG 随机 epoch。
    let epoch = BrowserSessionBrokerEpoch::new(random_nonce()?)
        // 理论 nonce 漂移映射为固定宿主不可用。
        .map_err(|_| unavailable("The browser session broker epoch was unavailable."))?;
    // 建立不持有 System 或 pipe 的共享 runtime。
    let runtime = Arc::new(BrowserSessionBrokerRuntime::new(epoch));
    // 用同源 codec 生成唯一 server-first ready 文本。
    let ready_text = Arc::new(
        // 编码当前 live epoch。
        encode_ready(runtime.epoch())
            // 编码不变量失败映射为固定宿主错误。
            .map_err(|_| unavailable("The browser session broker ready frame was unavailable."))?,
    );
    // 所有 fallible protocol 初始化完成后再启动唯一 System dispatcher。
    let dispatcher = BrowserSessionBrokerDispatcher::start(Arc::clone(&runtime))?;
    // 取得只传纯 Rust request 的派发端口。
    let dispatch = dispatcher.port();
    // 允许随后发布的 secondary 进入连接等待。
    if runtime.start_accepting().is_err() {
        // 先关闭 dispatcher 并在线程内销毁 System。
        let result = dispatcher.shutdown();
        // System 关闭后最后释放 first guard。
        drop(owner);
        // 优先返回 dispatcher 关闭错误或 runtime 错误。
        return result.and(Err(unavailable(
            "The browser session broker runtime could not start.",
        )));
    }
    // 启动并等待全部十五个 secondary listener 发布。
    let workers = match start_acceptors(
        // 传入弱 factory。
        factory,
        // 共享固定 sibling image。
        main_image,
        // 固定当前 native session。
        broker_session_id,
        // 共享 ready 文本。
        ready_text,
        // 共享纯 Rust runtime。
        Arc::clone(&runtime),
        // 复制 dispatcher 端口。
        dispatch.clone(),
    ) {
        // 保存全部 acceptor owner。
        Ok(workers) => workers,
        // 发布失败执行严格关闭顺序。
        Err(error) => {
            // 阻止新接入并唤醒连接。
            runtime.stop_accepting();
            // 释放最后一个外部 dispatch port。
            drop(dispatch);
            // 在线程内销毁 System。
            let _ = dispatcher.shutdown();
            // System 后最后释放 first guard。
            drop(owner);
            // 返回 listener 发布错误。
            return Err(error);
        }
    };
    // 保持进程代际直到控制台取消或 dispatcher 失败关闭 runtime。
    while !should_stop(&runtime) {
        // 使用短片轮询，不持有任何 protocol 或 System 锁。
        thread::sleep(HOST_POLL_INTERVAL);
    }
    // 第一步停止全部新 accept/read 与 attach 等待。
    runtime.stop_accepting();
    // 第二步 join 连接线程，确保全部 secondary handle 已释放。
    let acceptor_result = join_acceptors(workers);
    // handler 全部退出后释放最后一个可克隆 dispatch port。
    drop(dispatch);
    // 第三步排空此前已派发请求并在线程内销毁 System/Module。
    let dispatcher_result = dispatcher.shutdown();
    // 第四步也是最后一步释放 first-instance guard。
    drop(owner);
    // dispatcher/System 故障优先于连接回收故障。
    dispatcher_result.and(acceptor_result)
}

// 构造不泄漏 endpoint、线程或平台事实的宿主错误。
fn unavailable(
    // 接收不含平台或目标事实的固定说明。
    message: &'static str,
) -> AppControlError {
    // 使用稳定 broker 不可用错误码。
    AppControlError::new("BROKER_UNAVAILABLE", message)
}

// 运行 broker 并只在启动或不可恢复失败时写出结构化 JSON。
pub fn run() -> i32 {
    // 执行长期 broker 生命周期。
    match run_server() {
        // 正常取消返回成功退出码。
        Ok(()) => 0,
        // 不可恢复失败写入单条安全 JSON。
        Err(error) => {
            // 将统一错误转换为公共 JSON envelope。
            let value = error_json(&error);
            // 序列化理论失败时使用固定文本。
            let text = serde_json::to_string(&value).unwrap_or_else(|_| {
                // 不包含任何平台事实。
                String::from(
                    "{\"ok\":false,\"error\":{\"code\":\"BROKER_UNAVAILABLE\",\"message\":\"The browser session broker could not serialize its startup error.\"}}",
                )
            });
            // 只向 stdout 写一条 JSON 诊断。
            let _ = writeln!(std::io::stdout(), "{text}");
            // 返回结构化失败退出码。
            2
        }
    }
}

// 注册不打开 pipe 或真实浏览器的宿主辅助回归。
#[cfg(test)]
mod tests;
