//! 在唯一线程中串行拥有并调用 browser-session 的 ComputerControlSystem。

// 注册页面动作与截图的窄 dispatcher Component。
mod page_actions;

// 导入共享运行时、同步队列、线程与执行预算。
use std::{
    // 在 dispatcher 与连接线程间共享纯协议运行时。
    sync::{
        // 共享不持有 System 的运行时。
        Arc,
        // 使用固定有界多生产者单消费者队列。
        mpsc::{self, Receiver, SyncSender},
    },
    // 保存唯一 System owner 线程。
    thread::{self, JoinHandle},
    // 将绝对 deadline 投影为 Module 剩余预算。
    time::Duration,
};

// 导入协议、运行时、统一错误与 System 边界。
use crate::{
    // 导入 strict request、response 与状态决定。
    components::{
        // 导入冻结 broker 协议类型。
        browser_session_broker_protocol::{
            // 导入请求、稳定错误码与可关联失败。
            // 导入稳定协议错误码。
            BrowserSessionBrokerProtocolErrorCode,
            // 导入可关联协议失败。
            BrowserSessionBrokerProtocolFailure,
            // 导入严格领域 request。
            BrowserSessionBrokerRequest,
            // 导入完成响应类型。
            response::{
                // 导入封闭 final outcome。
                BrowserSessionBrokerOutcome,
                // 导入 accepted/final 投影。
                BrowserSessionBrokerResponse,
                // 导入 operation-specific 成功数据。
                BrowserSessionBrokerSuccess,
            },
        },
        // 导入不拥有 System 的共享运行时。
        browser_session_broker_runtime::{
            BrowserSessionBrokerDispatchPreparation, BrowserSessionBrokerRuntime,
        },
    },
    // 导入宿主错误边界。
    domain::{AppControlError, AppResult},
    // 导入仅由 dispatcher 调用的 System 投影。
    service::{
        // 导入 ComputerControlSystem 本体。
        AppControlService,
        // 导入 broker 专用 System 结果。
        browser_session_broker::{
            // 导入 close 执行投影。
            BrowserSessionBrokerCloseExecution,
            // 导入 session inspect 的只读执行投影。
            BrowserSessionBrokerInspectExecution,
            // 导入导航执行投影。
            BrowserSessionBrokerNavigateExecution,
            // 导入 open 执行投影。
            BrowserSessionBrokerOpenExecution,
            // 导入业务前预检错误。
            BrowserSessionBrokerPreflightError,
            // 导入查询执行投影。
            BrowserSessionBrokerQueryExecution,
            // 导入等待执行投影。
            BrowserSessionBrokerWaitExecution,
        },
    },
};

// 固定队列容量与 epoch execution ledger 容量一致。
const MAXIMUM_PENDING_REQUESTS: usize = 4_096;

// 表示连接线程只能发送给唯一 dispatcher 的封闭消息。
enum DispatcherMessage {
    // 请求执行一个已经取得唯一 Dispatch 决定的 request。
    Execute(BrowserSessionBrokerRequest),
    // 在此前全部请求之后有序关闭 System。
    Stop,
}

// 保存可克隆但不拥有 System 的 dispatcher 发送端。
#[derive(Clone)]
pub(crate) struct BrowserSessionBrokerDispatchPort {
    // 保存有界队列发送端。
    sender: SyncSender<DispatcherMessage>,
}

// 为连接 handler 提供唯一派发入口。
impl BrowserSessionBrokerDispatchPort {
    // 发送一个首次 request，绝不携带 pipe 或平台 handle。
    pub(crate) fn dispatch(
        // 借用不拥有 System 的发送端口。
        &self,
        // 接收首次 strict request。
        request: BrowserSessionBrokerRequest,
    ) -> AppResult<()> {
        // 有界队列在 dispatcher 存活时提供反压。
        self.sender
            // 只移动纯 Rust 严格 request。
            .send(DispatcherMessage::Execute(request))
            // dispatcher 退出后返回稳定宿主错误。
            .map_err(|_| unavailable("The browser session dispatcher is unavailable."))
    }
}

// 保存唯一 dispatcher 线程的组合根所有权。
pub(crate) struct BrowserSessionBrokerDispatcher {
    // 保存宿主发送 Stop 所需的队列端。
    sender: SyncSender<DispatcherMessage>,
    // 保存唯一创建并销毁 System 的线程。
    worker: Option<JoinHandle<Result<(), BrowserSessionBrokerProtocolFailure>>>,
}

// 为 dispatcher 提供启动、端口复制和有序关闭。
impl BrowserSessionBrokerDispatcher {
    // 启动在线程内部创建 System 的唯一 executor。
    pub(crate) fn start(
        // 接收共享纯 Rust runtime。
        runtime: Arc<BrowserSessionBrokerRuntime>,
    ) -> AppResult<Self> {
        // 建立与 ledger 容量一致的有界队列。
        let (sender, receiver) = mpsc::sync_channel(MAXIMUM_PENDING_REQUESTS);
        // 为错误路径保留共享 runtime。
        let worker_runtime = Arc::clone(&runtime);
        // 启动唯一拥有 ComputerControlSystem 的线程。
        let worker = thread::Builder::new()
            // 使用不含目标、nonce 或平台事实的固定线程名。
            .name("browser-session-dispatcher".to_owned())
            // 在线程内部创建并销毁 System，避免跨线程公开所有权。
            .spawn(move || run_dispatcher(worker_runtime, receiver))
            // 线程创建失败投影为固定不可用。
            .map_err(|_| unavailable("The browser session dispatcher could not start."))?;
        // 返回唯一 owner。
        Ok(Self {
            // 保存 Stop 发送端。
            sender,
            // 保存 join owner。
            worker: Some(worker),
        })
    }

    // 返回只能派发 request 的克隆端口。
    pub(crate) fn port(&self) -> BrowserSessionBrokerDispatchPort {
        // 不复制 System 或 worker owner。
        BrowserSessionBrokerDispatchPort {
            // 仅复制有界队列发送端。
            sender: self.sender.clone(),
        }
    }

    // 在所有连接 handler 退出后排空队列并销毁 System。
    pub(crate) fn shutdown(mut self) -> AppResult<()> {
        // Stop 必须排在已经发送的 request 之后。
        self.sender
            // 发送封闭停止消息。
            .send(DispatcherMessage::Stop)
            // worker 已异常退出时仍继续 join 取得根因。
            .ok();
        // 取出唯一 join owner。
        let Some(worker) = self.worker.take() else {
            // 重复关闭属于宿主生命周期错误。
            return Err(unavailable(
                "The browser session dispatcher was already closed.",
            ));
        };
        // 等待线程内部先销毁 System。
        match worker.join() {
            // 正常排空并关闭。
            Ok(Ok(())) => Ok(()),
            // 协议或 ledger 状态失败映射为固定不可用。
            Ok(Err(_)) => Err(unavailable(
                "The browser session dispatcher stopped after a protocol failure.",
            )),
            // panic 不得泄漏内部载荷。
            Err(_) => Err(unavailable(
                "The browser session dispatcher stopped unexpectedly.",
            )),
        }
    }
}

// 在线程内部创建、使用并最终销毁唯一 System。
fn run_dispatcher(
    // 接收共享纯 Rust runtime。
    runtime: Arc<BrowserSessionBrokerRuntime>,
    // 接收唯一有界队列消费端。
    receiver: Receiver<DispatcherMessage>,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // 只有本线程创建并持有 ComputerControlSystem。
    let mut system = AppControlService::new();
    // 串行处理全部首次 request。
    while let Ok(message) = receiver.recv() {
        // 穷举封闭 dispatcher 消息。
        match message {
            // 处理唯一 Dispatch 请求。
            DispatcherMessage::Execute(request) => {
                // 任一内部协议失败停止新接入并让宿主有序回收。
                if let Err(error) = execute_request(&mut system, &runtime, request) {
                    // 唤醒所有连接并阻止新接入。
                    runtime.stop_accepting();
                    // 返回安全协议失败。
                    return Err(error);
                }
            }
            // Stop 只在所有 handler 已停止发送后到达。
            DispatcherMessage::Stop => break,
        }
    }
    // 函数返回前先在当前线程销毁 System 及其 Module。
    drop(system);
    // 返回正常关闭。
    Ok(())
}

// 串行执行一个已经取得唯一 Dispatch 决定的 request。
fn execute_request(
    // 可变借用唯一 System。
    system: &mut AppControlService,
    // 借用共享 runtime。
    runtime: &Arc<BrowserSessionBrokerRuntime>,
    // 接收已经取得唯一 Dispatch 的 request。
    request: BrowserSessionBrokerRequest,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // 业务预检前再次核对 cancel、deadline 与 terminal replay。
    match runtime.prepare_dispatch(&request)? {
        // 尚有预算且未被 cancel 才进入 System 预检。
        BrowserSessionBrokerDispatchPreparation::Ready => {}
        // 既有终态意味着此 job 永远不得调用 Module。
        BrowserSessionBrokerDispatchPreparation::Final => return Ok(()),
    }
    // 执行不派发新浏览器业务的领域预检；允许推进此前 accepted close 的自有回收。
    if let Err(error) = preflight(system, &request) {
        // 内部错误不属于 wire rejection 白名单。
        let Some(code) = preflight_error_code(error) else {
            // 失败闭合并触发 broker 生命周期停止。
            return Err(failed());
        };
        // 从完整严格 request 构造可进入 ledger 的拒绝事实。
        let failure = BrowserSessionBrokerProtocolFailure::rejected_for_request(
            // 使用冻结领域拒绝码。
            code, // 绑定原 request 完整语义。
            &request,
        );
        // 构造 revision 零业务前终态。
        let rejected = BrowserSessionBrokerResponse::rejected_for_request(
            // 绑定可关联失败。
            &failure,
            // 绑定原 request。
            &request,
            // 首个 snapshot 固定 revision 零。
            0,
            // 回显当前 live epoch。
            runtime.epoch(),
        )
        // 受控构造必须接受同一 request provenance。
        .ok_or_else(failed)?;
        // 原子覆盖 preflight 与 cancel/deadline 的竞态。
        runtime.commit_dispatch_snapshot(&request, rejected)?;
        // 业务前终态不调用 Module。
        return Ok(());
    }
    // 构造 revision 零 accepted 候选。
    let accepted = BrowserSessionBrokerResponse::accepted(
        // 绑定原 request。
        &request,
        // accepted 固定 revision 零。
        0,
        // 回显当前 live epoch。
        runtime.epoch(),
    );
    // 在 cancel 与 deadline 的同一线性化点提交业务接受。
    let commit = runtime.commit_dispatch_snapshot(&request, accepted)?;
    // 只有本调用刚提交 accepted 才可恰好一次调用 Module。
    if !commit.should_execute() {
        // 既有 accepted 或任何 terminal 都不授予重复执行。
        return Ok(());
    }
    // 只有真正 accepted snapshot 才能进入领域执行。
    if !commit.response().business_accepted() {
        // accepted 已授予执行权后必须先尝试收敛为 unknown。
        return settle_after_acceptance(runtime, &request, Err(failed()));
    }
    // 在不持有 runtime 锁时调用唯一 System。
    let final_response = execute_accepted(system, runtime, &request, commit.deadline_ms());
    // 无论终态构造或落账是否失败都封闭 accepted 后收敛语义。
    settle_after_acceptance(runtime, &request, final_response)
}

// 在 accepted 之后落账权威终态，内部失信时尽力收敛为 unknown。
fn settle_after_acceptance(
    // 借用唯一 epoch runtime。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用已经 accepted 的严格 request。
    request: &BrowserSessionBrokerRequest,
    // 接收 System 执行或终态构造结果。
    candidate: Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure>,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // 正常候选终态先尝试原子落账。
    let failure = match candidate {
        // 落账成功即完成本次执行。
        Ok(response) => match runtime.apply_response(response) {
            // ledger 已保存权威终态。
            Ok(()) => return Ok(()),
            // 落账失败意味着 accepted 后内部状态失信。
            Err(error) => error,
        },
        // 构造失败同样处于 accepted 后不可安全重试区间。
        Err(error) => error,
    };
    // 构造唯一保守 revision 一 unknown。
    let unknown = BrowserSessionBrokerResponse::unknown(
        // 绑定原严格 request。
        request,
        // accepted 后终态固定 revision 一。
        1,
        // 回显当前 broker epoch。
        runtime.epoch(),
    );
    // 尽力落账 unknown，但不覆盖已有 terminal 或伪造成功。
    let _ = runtime.apply_response(unknown);
    // 保留原始内部失败以触发整代 broker 停止。
    Err(failure)
}

// 执行 command/query 的业务前门禁，不创建新 browser worker/Job 或派发新浏览器业务。
fn preflight(
    // 可变借用唯一 System，允许 Module 非阻塞收割已完成生命周期。
    system: &mut AppControlService,
    // 借用严格 request。
    request: &BrowserSessionBrokerRequest,
) -> Result<(), BrowserSessionBrokerPreflightError> {
    // 封闭匹配当前已接入的三个 command 与三个 Query。
    match request {
        // open 只检查固定 registry 容量。
        BrowserSessionBrokerRequest::Open(_) => system.prepare_browser_session_broker_open(),
        // close 只检查 opaque session 仍属于 live registry。
        BrowserSessionBrokerRequest::Close(_, session_id) => {
            // 委托 System 预检，不访问 Module。
            system.prepare_browser_session_broker_close(session_id)
        }
        // session.inspect 只读取 live registry，不改变 worker 或 Job。
        BrowserSessionBrokerRequest::SessionInspect(_, session_id) => {
            // 委托 System 的只读查询预检，不穿透 Module。
            system.prepare_browser_session_broker_inspect(session_id)
        }
        // navigate 只验证 session 仍属于 live registry。
        BrowserSessionBrokerRequest::Navigate(_, session_id, _) => {
            // URL 已由 parser 验证，预检不得派发 worker 命令。
            system.prepare_browser_session_broker_navigate(session_id)
        }
        // wait 必须绑定当前 session/page 导航代际。
        BrowserSessionBrokerRequest::Wait(_, session_id, page_id, _) => {
            // 只读取 Module 当前页面映射。
            system.prepare_browser_session_broker_page(session_id, page_id)
        }
        // query 必须绑定当前 session/page 导航代际。
        BrowserSessionBrokerRequest::Query(_, session_id, page_id, _, _) => {
            // 只读取 Module 当前页面映射。
            system.prepare_browser_session_broker_page(session_id, page_id)
        }
        // 页面动作与截图交给不拥有 System 的窄协调 Component。
        _ => page_actions::preflight(system, request)
            // 未知 operation 仍必须失败闭合。
            .unwrap_or(Err(BrowserSessionBrokerPreflightError::Internal)),
    }
}

// 将 System 预检错误映射为冻结业务前 rejection。
fn preflight_error_code(
    // 接收 System 封闭预检错误。
    error: BrowserSessionBrokerPreflightError,
) -> Option<BrowserSessionBrokerProtocolErrorCode> {
    // 只允许 schema 冻结的三个领域错误。
    match error {
        // registry full 可在业务接受前安全重试新 nonce。
        BrowserSessionBrokerPreflightError::RegistryFull => {
            // 映射固定错误码。
            Some(BrowserSessionBrokerProtocolErrorCode::BrowserSessionRegistryFull)
        }
        // stale session 是确定业务前拒绝。
        BrowserSessionBrokerPreflightError::StaleSession => {
            // 映射固定错误码。
            Some(BrowserSessionBrokerProtocolErrorCode::StaleSession)
        }
        // stale page 是确定业务前拒绝。
        BrowserSessionBrokerPreflightError::StalePage => {
            // 映射固定错误码。
            Some(BrowserSessionBrokerProtocolErrorCode::StalePage)
        }
        // stale element 只在当前 session/page 内成立。
        BrowserSessionBrokerPreflightError::StaleElement => {
            // 映射固定错误码。
            Some(BrowserSessionBrokerProtocolErrorCode::StaleElement)
        }
        // 内部错误不得伪装成 schema rejection。
        BrowserSessionBrokerPreflightError::Internal => None,
    }
}

// 在 accepted 后调用 System 并构造 revision 一终态。
fn execute_accepted(
    // 可变借用唯一 System。
    system: &mut AppControlService,
    // 借用共享 runtime。
    runtime: &Arc<BrowserSessionBrokerRuntime>,
    // 借用已经 accepted 的 request。
    request: &BrowserSessionBrokerRequest,
    // 接收不可延长服务器绝对 deadline。
    deadline_ms: u64,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 按 operation 调用唯一 System 端口。
    match request {
        // 执行隔离浏览器会话打开。
        BrowserSessionBrokerRequest::Open(_) => {
            // 复制 nonce 供不持锁取消 closure 查询。
            let request_nonce = request.request_nonce().to_owned();
            // 计算 accepted 后当前剩余预算。
            let timeout = remaining_duration(runtime.now_ms(), deadline_ms);
            // 调用 System，closure 每次只短锁读取取消事实。
            let execution = system.execute_browser_session_broker_open(timeout, || {
                // 每次轮询都短锁读取可被合法重送缩短的 deadline 与 cancel 事实。
                runtime
                    .execution_should_stop(&request_nonce)
                    .unwrap_or(true)
            });
            // 映射 System 的 provider-neutral 执行投影。
            open_final(runtime, request, execution)
        }
        // 执行会话关闭与整树回收。
        BrowserSessionBrokerRequest::Close(_, session_id) => {
            // 复制 nonce 供不持锁取消 closure 查询。
            let request_nonce = request.request_nonce().to_owned();
            // 计算 accepted 后当前不可延长的剩余预算。
            let timeout = remaining_duration(runtime.now_ms(), deadline_ms);
            // 由 System 唯一调用 Module 的有界 close，取消只能请求协作停止。
            let execution = system.execute_browser_session_broker_close(
                // 传入已预检的 opaque session identity。
                session_id,
                // 传入 ledger 当前剩余总预算。
                timeout,
                // 每次轮询都短锁读取可缩短 deadline 与 cancel 事实。
                || {
                    runtime
                        .execution_should_stop(&request_nonce)
                        .unwrap_or(true)
                },
            );
            // 映射关闭结果。
            close_final(runtime, request, execution)
        }
        // 执行无隐藏副作用的 session.inspect Query。
        BrowserSessionBrokerRequest::SessionInspect(_, session_id) => {
            // 由唯一 System 线程只读调用 Module registry。
            let execution = system.execute_browser_session_broker_inspect(session_id);
            // 映射 Query 的最小 completed/failed 结果。
            inspect_final(runtime, request, execution)
        }
        // 执行确认式页面导航并换发公开 page identity。
        BrowserSessionBrokerRequest::Navigate(_, session_id, url) => {
            // 复制 nonce 供不持锁取消 closure 查询。
            let request_nonce = request.request_nonce().to_owned();
            // 计算 accepted 后当前剩余预算。
            let timeout = remaining_duration(runtime.now_ms(), deadline_ms);
            // 由 System 唯一调用 Module 的导航入口。
            let execution = system.execute_browser_session_broker_navigate(
                // 传入已预检的公开 session。
                session_id,
                // 传入 parser 已验证且不会回显的 URL。
                url,
                // 传入 ledger 当前剩余总预算。
                timeout,
                // 每次轮询都短锁读取 deadline 与 cancel 事实。
                || {
                    runtime
                        .execution_should_stop(&request_nonce)
                        .unwrap_or(true)
                },
            );
            // 映射导航终态。
            navigate_final(runtime, request, execution)
        }
        // 执行无隐藏副作用的有限等待 Query。
        BrowserSessionBrokerRequest::Wait(_, session_id, page_id, condition) => {
            // 复制 nonce 供不持锁取消 closure 查询。
            let request_nonce = request.request_nonce().to_owned();
            // 计算 accepted 后当前剩余预算。
            let timeout = remaining_duration(runtime.now_ms(), deadline_ms);
            // 由 System 唯一调用 Module 的 wait 入口。
            let execution = system.execute_browser_session_broker_wait(
                // 传入已预检的公开 session。
                session_id,
                // 传入已预检的当前 page。
                page_id,
                // 传入 parser 已验证的有限条件。
                condition,
                // 传入 ledger 当前剩余总预算。
                timeout,
                // 每次轮询都短锁读取 deadline 与 cancel 事实。
                || {
                    runtime
                        .execution_should_stop(&request_nonce)
                        .unwrap_or(true)
                },
            );
            // 映射 wait Query 终态。
            wait_final(runtime, request, execution)
        }
        // 执行无隐藏副作用的 provider-neutral 元素查询。
        BrowserSessionBrokerRequest::Query(_, session_id, page_id, selector, max_results) => {
            // 复制 nonce 供不持锁取消 closure 查询。
            let request_nonce = request.request_nonce().to_owned();
            // 计算 accepted 后当前剩余预算。
            let timeout = remaining_duration(runtime.now_ms(), deadline_ms);
            // 由 System 唯一调用 Module 的 query 入口并签发元素 identity。
            let execution = system.execute_browser_session_broker_query(
                // 传入已预检的公开 session。
                session_id,
                // 传入已预检的当前 page。
                page_id,
                // 传入 parser 已验证的 selector。
                selector,
                // 保留 parser 已验证的结果上限。
                *max_results,
                // 传入 ledger 当前剩余总预算。
                timeout,
                // 每次轮询都短锁读取 deadline 与 cancel 事实。
                || {
                    runtime
                        .execution_should_stop(&request_nonce)
                        .unwrap_or(true)
                },
            );
            // 映射 query 终态。
            query_final(runtime, request, execution)
        }
        // 页面动作与截图交给不拥有 System 的窄协调 Component。
        _ => page_actions::execute(system, runtime, request, deadline_ms)
            // 未知 operation 不得越过 accepted。
            .unwrap_or_else(|| Err(failed())),
    }
}

// 将 open 执行投影为封闭业务终态。
fn open_final(
    // 借用 current epoch runtime。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用原始 open request。
    request: &BrowserSessionBrokerRequest,
    // 接收 System open 执行投影。
    execution: BrowserSessionBrokerOpenExecution,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 穷举 System 投影。
    match execution {
        // ready 返回公开 opaque session identity。
        BrowserSessionBrokerOpenExecution::Completed { session_id } => finished(
            // 绑定 runtime。
            runtime,
            // 绑定 request。
            request,
            // 使用完成 outcome。
            BrowserSessionBrokerOutcome::Completed,
            // open 只携带 session identity。
            Some(BrowserSessionBrokerSuccess::Open { session_id }),
        ),
        // System 只证明确定失败，cancel flag 本身不能升级为权威 cancelled。
        BrowserSessionBrokerOpenExecution::Failed => {
            // 保持 System 的确定 failed 投影，不从并发 cancel 推断因果。
            finished(runtime, request, BrowserSessionBrokerOutcome::Failed, None)
        }
        // 无可信结果必须保守 OutcomeUnknown。
        BrowserSessionBrokerOpenExecution::Unknown => Ok(BrowserSessionBrokerResponse::unknown(
            // 绑定原 request。
            request,
            // accepted 后固定 revision 一。
            1,
            // 回显当前 epoch。
            runtime.epoch(),
        )),
    }
}

// 将 close 执行投影为封闭业务终态。
fn close_final(
    // 借用 current epoch runtime。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用原始 close request。
    request: &BrowserSessionBrokerRequest,
    // 接收 System close 执行投影。
    execution: BrowserSessionBrokerCloseExecution,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 穷举 System 投影。
    match execution {
        // Module 已完成 registry 移除与资源回收。
        BrowserSessionBrokerCloseExecution::Completed => finished(
            // 绑定 runtime。
            runtime,
            // 绑定 request。
            request,
            // 使用 completed outcome。
            BrowserSessionBrokerOutcome::Completed,
            // close 成功不携带结果字段。
            Some(BrowserSessionBrokerSuccess::Close),
        ),
        // 确定执行失败不携带 data。
        BrowserSessionBrokerCloseExecution::Failed => {
            // 构造 failed final。
            finished(runtime, request, BrowserSessionBrokerOutcome::Failed, None)
        }
        // deadline、取消或后台回收未完成时不得伪造确定结果。
        BrowserSessionBrokerCloseExecution::Unknown => Ok(BrowserSessionBrokerResponse::unknown(
            // 绑定原 close request。
            request,
            // accepted 后终态固定 revision 一。
            1,
            // 回显当前 epoch。
            runtime.epoch(),
        )),
    }
}

// 将 session.inspect 执行投影为封闭 Query 终态。
fn inspect_final(
    // 借用 current epoch runtime。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用原始 session.inspect request。
    request: &BrowserSessionBrokerRequest,
    // 接收 System 的最小只读执行投影。
    execution: BrowserSessionBrokerInspectExecution,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 穷举 System 投影，不从 Query 制造 unknown 或 mutation 事实。
    match execution {
        // Module 已在线性化点确认目标 live。
        BrowserSessionBrokerInspectExecution::Completed { session_id, live } => finished(
            // 绑定 runtime。
            runtime,
            // 绑定原 request。
            request,
            // Query 成功使用 completed outcome。
            BrowserSessionBrokerOutcome::Completed,
            // 只携带 opaque identity 与 live 布尔事实。
            Some(BrowserSessionBrokerSuccess::SessionInspect { session_id, live }),
        ),
        // accepted 后内部查询失败只能形成确定 failed，不伪造 stale 或 unknown。
        BrowserSessionBrokerInspectExecution::Failed => {
            // 构造不携带成功数据的 Query failed final。
            finished(runtime, request, BrowserSessionBrokerOutcome::Failed, None)
        }
    }
}

// 将 navigate 执行投影为封闭 Command 终态。
fn navigate_final(
    // 借用 current epoch runtime。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用原始 navigate request。
    request: &BrowserSessionBrokerRequest,
    // 接收 System 导航执行投影。
    execution: BrowserSessionBrokerNavigateExecution,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 穷举 System 投影并保留 accepted 后 mutation 真值。
    match execution {
        // 成功只返回新 page identity 与正代际。
        BrowserSessionBrokerNavigateExecution::Completed {
            // 取得公开 page identity。
            page_id,
            // 取得正导航代际。
            generation,
        } => finished(
            // 绑定 runtime。
            runtime,
            // 绑定原 request。
            request,
            // 使用 completed outcome。
            BrowserSessionBrokerOutcome::Completed,
            // 构造 navigate 专属成功数据。
            Some(BrowserSessionBrokerSuccess::Navigate {
                // 保存公开 page identity。
                page_id,
                // 保存正导航代际。
                generation,
            }),
        ),
        // 确定失败不携带成功数据。
        BrowserSessionBrokerNavigateExecution::Failed => {
            // 构造 accepted 后 failed final。
            finished(runtime, request, BrowserSessionBrokerOutcome::Failed, None)
        }
        // 丢失可信结果必须保守 unknown。
        BrowserSessionBrokerNavigateExecution::Unknown => {
            Ok(BrowserSessionBrokerResponse::unknown(
                // 绑定原 navigate request。
                request,
                // accepted 后终态固定 revision 一。
                1,
                // 回显当前 epoch。
                runtime.epoch(),
            ))
        }
    }
}

// 将 wait 执行投影为封闭 Query 终态。
fn wait_final(
    // 借用 current epoch runtime。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用原始 wait request。
    request: &BrowserSessionBrokerRequest,
    // 接收 System wait 执行投影。
    execution: BrowserSessionBrokerWaitExecution,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 穷举 Query 执行结果。
    match execution {
        // 条件满足只返回固定 success 变体。
        BrowserSessionBrokerWaitExecution::Completed {
            // 取得当前公开页面 identity。
            page_id,
            // 取得 Module 报告的正导航代际。
            generation,
        } => finished(
            // 绑定 runtime。
            runtime,
            // 绑定原 request。
            request,
            // 使用 completed outcome。
            BrowserSessionBrokerOutcome::Completed,
            // wait success 固定编码 conditionMet=true。
            Some(BrowserSessionBrokerSuccess::Wait {
                // 回显当前公开 page identity。
                page_id,
                // 回显正导航代际。
                generation,
            }),
        ),
        // 确定失败不携带成功数据。
        BrowserSessionBrokerWaitExecution::Failed => {
            // 构造 accepted 后 failed final。
            finished(runtime, request, BrowserSessionBrokerOutcome::Failed, None)
        }
        // 丢失可信结果保留 Query unknown 且 mutation=false。
        BrowserSessionBrokerWaitExecution::Unknown => Ok(BrowserSessionBrokerResponse::unknown(
            // 绑定原 wait request。
            request,
            // accepted 后终态固定 revision 一。
            1,
            // 回显当前 epoch。
            runtime.epoch(),
        )),
    }
}

// 将 query 执行投影为封闭 Query 终态。
fn query_final(
    // 借用 current epoch runtime。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用原始 query request。
    request: &BrowserSessionBrokerRequest,
    // 接收 System query 执行投影。
    execution: BrowserSessionBrokerQueryExecution,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 穷举 Query 执行结果。
    match execution {
        // 成功携带 response Component 将再次验证的有界数据。
        BrowserSessionBrokerQueryExecution::Completed { data } => finished(
            // 绑定 runtime。
            runtime,
            // 绑定原 request。
            request,
            // 使用 completed outcome。
            BrowserSessionBrokerOutcome::Completed,
            // 保存 provider-neutral 查询对象。
            Some(BrowserSessionBrokerSuccess::Query(data)),
        ),
        // 确定失败不携带成功数据。
        BrowserSessionBrokerQueryExecution::Failed => {
            // 构造 accepted 后 failed final。
            finished(runtime, request, BrowserSessionBrokerOutcome::Failed, None)
        }
        // 丢失可信结果保留 Query unknown 且 mutation=false。
        BrowserSessionBrokerQueryExecution::Unknown => Ok(BrowserSessionBrokerResponse::unknown(
            // 绑定原 query request。
            request,
            // accepted 后终态固定 revision 一。
            1,
            // 回显当前 epoch。
            runtime.epoch(),
        )),
    }
}

// 构造 completed、failed 或 cancelled revision 一终态。
fn finished(
    // 借用 current epoch runtime。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用原始严格 request。
    request: &BrowserSessionBrokerRequest,
    // 接收封闭业务终态。
    outcome: BrowserSessionBrokerOutcome,
    // 接收 operation-specific 成功数据。
    success: Option<BrowserSessionBrokerSuccess>,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 委托响应 Component 校验 operation-specific data。
    BrowserSessionBrokerResponse::finished(
        // 绑定原 request。
        request,
        // accepted 后固定 revision 一。
        1,
        // 回显当前 epoch。
        runtime.epoch(),
        // 保存封闭 outcome。
        outcome,
        // 保存可选成功数据。
        success,
    )
    // 构造失败表示内部 operation/result 漂移。
    .ok_or_else(failed)
}

// 将服务器绝对 deadline 转为不会溢出的剩余时长。
fn remaining_duration(
    // 接收当前服务器单调时刻。
    now_ms: u64,
    // 接收不可延长绝对 deadline。
    deadline_ms: u64,
) -> Duration {
    // accepted 后零预算会让取消 closure 立即停止。
    Duration::from_millis(deadline_ms.saturating_sub(now_ms))
}

// 构造不泄漏内部状态的协议失败。
fn failed() -> BrowserSessionBrokerProtocolFailure {
    // 使用状态机失败码。
    BrowserSessionBrokerProtocolFailure::new(
        // 不伪装为可重试业务拒绝。
        BrowserSessionBrokerProtocolErrorCode::ProtocolFailed,
    )
}

// 构造 broker 宿主稳定不可用错误。
fn unavailable(
    // 接收不含内部事实的固定说明。
    message: &'static str,
) -> AppControlError {
    // 不公开线程、channel 或 System 内部事实。
    AppControlError::new("BROKER_UNAVAILABLE", message)
}

// 注册不启动真实浏览器的 dispatcher 回归。
#[cfg(test)]
mod tests;
