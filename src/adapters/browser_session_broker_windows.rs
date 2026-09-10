//! 启动、连接并认证固定同会话 browser-session broker 的 Windows Adapter。

// 注册只负责无 handle 继承启动固定 broker 的 Windows 子 Adapter。
#[path = "browser_session_broker_process_windows.rs"]
mod broker_process;
// 注册字段封闭的 browser-session request 选择与 builder 子 Adapter。
#[path = "browser_session_broker_windows_request.rs"]
mod request;
// 注册页面导航与只读查询结果投影子 Adapter。
#[path = "browser_session_broker_windows_page.rs"]
mod page;
// 向 Module 暴露不含 transport 事实的页面结果与调用入口。
pub(crate) use page::{
    // 导出确认式点击入口。
    click_confirmed,
    // 导出确认式导航入口。
    navigate_confirmed,
    // 导出页面查询入口。
    query,
    // 导出有界截图入口。
    screenshot,
    // 导出确认式输入入口。
    type_confirmed,
    // 导出有限等待入口。
    wait,
};

// 导入轮询与单调 deadline。
use std::{
    // 为 broker 发布 endpoint 提供短轮询。
    thread,
    // 约束完整握手生命周期。
    time::{Duration, Instant},
};

// 导入固定 IPC、ready parser、取消与统一错误。
use crate::{
    // 导入共享 IPC 与 peer 认证 Adapter。
    adapters::fixed_local_ipc_windows::{
        // 导入当前 session、固定 sibling 与 peer 认证。
        identity::{authenticate_peer_process, current_session_id, sibling_image_path},
        // 导入固定 browser-session endpoint。
        pipe::{BoundedTextWriteAttempt, ConnectedPipe, FixedLocalEndpointKind},
    },
    // 导入 ready parser、冻结预算与进程级取消。
    components::{
        // 解析 server-first ready 且复用协议预算。
        browser_session_broker_protocol::{
            // 导入冻结握手预算。
            MAXIMUM_TIMEOUT_MS,
            // 导入 ready、outcome 与成功投影。
            // 导入 request-bound response 的封闭 outcome 投影。
            response::{
                // 导入终态类别。
                BrowserSessionBrokerOutcome,
                // 导入 ready 投影。
                BrowserSessionBrokerReady,
                // 导入操作成功投影。
                BrowserSessionBrokerSuccess,
            },
            // 导入已认证 epoch 类型。
            state::BrowserSessionBrokerEpoch,
            // 导入严格 request/response wire Component。
            wire::{BrowserSessionBrokerRequestFrame, decode_response},
        },
        // 观察进程级取消。
        cancellation,
        // 导入系统随机 nonce Component。
        secure_nonce_windows::random_nonce,
    },
    // 导入统一错误边界。
    domain::{AppControlError, AppResult},
};

// 导入窄 Windows 进程启动边界。
use broker_process::start_fixed_broker;
// 导入字段封闭的 request 选择与 builder。
use request::{BrowserSessionExchange, build_request_frame};

// 固定唯一可启动的 browser-session broker sibling 文件名。
const BROKER_FILE_NAME: &str = "ai-computer-toolkit-browser-session-broker.exe";
// 固定 endpoint 发布轮询片。
const CONNECT_RETRY_SLICE: Duration = Duration::from_millis(10);

// 构造不泄漏进程、pipe、路径或 session 的 broker 错误。
fn broker_unavailable(message: &'static str) -> AppControlError {
    // 使用稳定 broker 错误码。
    AppControlError::new("BROKER_UNAVAILABLE", message)
}

// 从封闭毫秒预算构造单调 deadline。
pub(super) fn handshake_deadline(timeout_ms: u32) -> AppResult<Instant> {
    // 预算必须落在 broker 协议冻结范围。
    if !(1..=MAXIMUM_TIMEOUT_MS).contains(&timeout_ms) {
        // 返回不回显调用值的普通参数错误。
        return Err(AppControlError::new(
            // 使用稳定参数码。
            "INVALID_ARGUMENT",
            // 不公开内部 deadline。
            "The browser session broker timeout is outside its fixed boundary.",
        ));
    }
    // 使用不会溢出的单调加法。
    Instant::now()
        // 转换固定握手预算。
        .checked_add(Duration::from_millis(u64::from(timeout_ms)))
        // 理论溢出失败闭合。
        .ok_or_else(|| broker_unavailable("The browser session broker deadline is unavailable."))
}

// 拒绝在取消或 deadline 之后启动、连接或重试 broker。
fn ensure_handshake_active(deadline: Instant) -> AppResult<()> {
    // 取消只终止当前握手，不关闭已存在 broker。
    if cancellation::is_cancelled() {
        // 返回稳定取消。
        return Err(AppControlError::new(
            // 使用公共取消码。
            "CANCELLED",
            // 不公开连接阶段。
            "The browser session broker handshake was cancelled.",
        ));
    }
    // deadline 到达后不得启动或继续重连。
    if Instant::now() >= deadline {
        // 返回稳定 timeout。
        return Err(AppControlError::new(
            // 使用公共 timeout 码。
            "TIMEOUT",
            // 不公开 broker 进程状态。
            "The browser session broker did not publish its ready frame in time.",
        ));
    }
    // 当前握手仍可继续。
    Ok(())
}

// 在不越过调用总 deadline 的前提下执行一次连接退避。
fn sleep_connect_retry_until(deadline: Instant) -> bool {
    // 计算从当前单调时刻到绝对终点的剩余预算。
    let remaining = deadline.saturating_duration_since(Instant::now());
    // 预算已经耗尽时不得再固定 sleep。
    if remaining.is_zero() {
        // 返回无可用退避事实。
        return false;
    }
    // 选取固定轮询片与剩余预算的较小值。
    let delay = CONNECT_RETRY_SLICE.min(remaining);
    // 只阻塞已被总 deadline 截断的时长。
    thread::sleep(delay);
    // 返回已执行有界退避。
    true
}

// 认证已经连接的固定 broker peer。
fn authenticate_broker(
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
        .map_err(|_| broker_unavailable("The browser session broker peer is unavailable."))?;
    // 核对完整镜像、精确 session、SID 与完整性 RID。
    authenticate_peer_process(peer_process_id, broker_image, session_id)
        // 将身份细节折叠成稳定认证失败。
        .map_err(|_| {
            // 不公开拒绝原因。
            broker_unavailable("The browser session broker peer could not be authenticated.")
        })
}

// 连接并认证当前 session 的固定 browser-session broker。
pub(super) fn connect_certified(deadline: Instant) -> AppResult<ConnectedPipe> {
    // 取得当前 native session 私有事实。
    let session_id = current_session_id()
        // 不公开 session 数值。
        .map_err(|_| broker_unavailable("The browser session broker session is unavailable."))?;
    // 定位固定 broker 镜像供连接后认证。
    let broker_image = sibling_image_path(BROKER_FILE_NAME)
        // 不公开安装路径。
        .map_err(|_| broker_unavailable("The fixed browser session broker is unavailable."))?;
    // 首次尝试复用已经存在的 broker。
    if let Ok(pipe) = ConnectedPipe::connect_for_until(
        // 固定 browser-session endpoint kind。
        FixedLocalEndpointKind::BrowserSession,
        // 固定当前登录 session。
        session_id,
        // 共享握手 deadline。
        deadline,
        // 响应进程级取消。
        cancellation::is_cancelled,
    ) {
        // 连接成功后必须先认证 server。
        authenticate_broker(&pipe, &broker_image, session_id)?;
        // 返回认证连接。
        return Ok(pipe);
    }
    // 首次连接失败可能已经消耗 deadline 或观察到取消。
    ensure_handshake_active(deadline)?;
    // 仅启动固定无参数 sibling；并发 loser 由 first-instance 门禁关闭。
    start_fixed_broker()?;
    // 在总 deadline 内等待 winner 发布 endpoint。
    loop {
        // 每轮连接前拒绝取消或过期预算。
        ensure_handshake_active(deadline)?;
        // 尝试连接固定 endpoint。
        if let Ok(pipe) = ConnectedPipe::connect_for_until(
            // 固定 browser-session endpoint kind。
            FixedLocalEndpointKind::BrowserSession,
            // 固定当前登录 session。
            session_id,
            // 共享总 deadline。
            deadline,
            // 响应进程级取消。
            cancellation::is_cancelled,
        ) {
            // 连接后必须完成 OS peer 认证。
            authenticate_broker(&pipe, &broker_image, session_id)?;
            // 返回认证连接。
            return Ok(pipe);
        }
        // 限制 endpoint 发布轮询 CPU，但绝不越过总 deadline。
        if !sleep_connect_retry_until(deadline) {
            // 复用统一取消或超时投影。
            ensure_handshake_active(deadline)?;
        }
    }
}

// 连接固定 broker 并读取唯一 server-first ready frame。
pub(crate) fn probe_ready(timeout_ms: u32) -> AppResult<String> {
    // 构造覆盖连接、认证与读取的单一 deadline。
    let deadline = handshake_deadline(timeout_ms)?;
    // 建立并认证固定同会话连接。
    let pipe = connect_certified(deadline)?;
    // 在认证前绝不读取 JSON；此处只读取 server-first ready。
    let text = pipe
        // 读取冻结有界响应 frame。
        .read_text_until(deadline, cancellation::is_cancelled)
        // 不泄漏 endpoint 读取细节。
        .map_err(|_| {
            // 返回稳定握手失败。
            broker_unavailable("The browser session broker ready frame is unavailable.")
        })?;
    // 使用同源 parser 严格校验 kind、版本、字段和 epoch。
    let ready = BrowserSessionBrokerReady::parse(&text)
        // parser 细节不穿过 transport boundary。
        .map_err(|_| {
            // 返回稳定协议失败。
            broker_unavailable("The browser session broker ready frame is invalid.")
        })?;
    // 复制 provider-neutral epoch 后立即关闭探测连接。
    let epoch = ready.broker_epoch().to_owned();
    // 显式关闭 client，使 server 确知 ready 已被读取后再 relisten。
    drop(pipe);
    // 返回不含 native transport 事实的 epoch。
    Ok(epoch)
}

// 区分客户端仍能证明未派发与必须保守恢复的 exchange 失败。
enum BrowserSessionExchangeFailure {
    // 完整 message 从未写入 broker。
    NotDispatched(AppControlError),
    // 已写入或平台无法排除已投递。
    DeliveryMayHaveOccurred,
    // 零字节回压仍证明当前 frame 尚未派发。
    BackpressureZero,
}

// 构造写入前仍可确定未派发的剩余 request 预算。
fn remaining_request_timeout_ms(deadline: Instant) -> AppResult<u32> {
    // 取消或到期后绝不构造新的 request frame。
    ensure_handshake_active(deadline)?;
    // 计算不扩张总 deadline 的剩余时长。
    let remaining = deadline
        // 不允许越过单调终点。
        .checked_duration_since(Instant::now())
        // 终点竞态按确定性未派发超时闭合。
        .ok_or_else(|| {
            // 返回公开 timeout 而不泄漏时间源。
            AppControlError::new(
                // 使用稳定 timeout 代码。
                "TIMEOUT",
                // 明确 request 尚未写入。
                "The browser session broker handshake exhausted the request deadline.",
            )
        })?;
    // 向下取整以确保 wire 预算绝不延长原调用 deadline。
    let millis = remaining.as_millis();
    // 不足一完整毫秒时不得开始 request 写入。
    if millis == 0 {
        // 返回确定性的写前超时。
        return Err(AppControlError::new(
            // 使用稳定 timeout 代码。
            "TIMEOUT",
            // 明确 request 尚未写入。
            "The browser session broker handshake exhausted the request deadline.",
        ));
    }
    // 将协议已限制的毫秒数窄化为 u32。
    u32::try_from(millis).map_err(|_| {
        // 理论溢出保持写前参数失败。
        AppControlError::new(
            // 使用稳定参数代码。
            "INVALID_ARGUMENT",
            // 不公开单调时钟细节。
            "The browser session broker remaining deadline is invalid.",
        )
    })
}

// 读取并严格验证当前已认证连接的唯一 server-first ready frame。
pub(super) fn read_certified_ready(pipe: &ConnectedPipe, deadline: Instant) -> AppResult<String> {
    // 在 write 前保留底层取消、deadline 与认证错误语义。
    let text = pipe.read_text_until(deadline, cancellation::is_cancelled)?;
    // 只允许冻结的 ready schema 进入后续 request 构造。
    let ready = BrowserSessionBrokerReady::parse(&text).map_err(|_| {
        // ready 解析失败不公开 pipe 内容。
        broker_unavailable("The browser session broker ready frame is invalid.")
    })?;
    // 返回不携带 native transport 事实的 epoch。
    Ok(ready.broker_epoch().to_owned())
}

// 将已认证 epoch 文本转换为 response decoder 所需的私有协议投影。
fn decode_epoch(epoch: &str) -> AppResult<BrowserSessionBrokerEpoch> {
    // 只接受 ready parser 已验证的 canonical epoch。
    BrowserSessionBrokerEpoch::new(epoch.to_owned()).map_err(|_| {
        // 理论形状漂移按不可用关闭。
        broker_unavailable("The browser session broker epoch is invalid.")
    })
}

// 在每个实际 WriteFile 尝试前按同一总 deadline 重建严格请求帧。
fn build_request_frame_for_deadline(
    // 借用字段封闭的命令选择。
    command: &BrowserSessionExchange<'_>,
    // 借用 ready 后唯一生成的 secure nonce。
    nonce: &str,
    // 借用唯一允许恢复的 ready epoch。
    epoch: &str,
    // 接收覆盖所有尝试的唯一单调 deadline。
    deadline: Instant,
) -> AppResult<BrowserSessionBrokerRequestFrame> {
    // 先在 WriteFile 前取得不能扩张的当前剩余预算。
    let remaining_timeout_ms = remaining_request_timeout_ms(deadline)?;
    // 用该预算构造会被实际写入和 decoder 绑定的 frame。
    build_request_frame(command, nonce, epoch, remaining_timeout_ms)
        // builder 失败发生在 WriteFile 前，保持本地可证明未派发错误。
        .map_err(|failure| AppControlError::new(failure.code().as_str(), failure.code().as_str()))
}

// 在一条已认证连接上构造当前 frame、单次写入并读取 accepted/final 或直接 final。
fn exchange_on_connection(
    // 借用本次已认证 pipe。
    pipe: &ConnectedPipe,
    // 借用字段封闭的命令选择。
    command: &BrowserSessionExchange<'_>,
    // 借用 ready 后唯一生成的 secure nonce。
    nonce: &str,
    // 借用当前已认证连接声明的唯一 epoch 文本。
    epoch_text: &str,
    // 借用本连接已认证的 live epoch。
    epoch: &BrowserSessionBrokerEpoch,
    // 接收覆盖写入和全部响应的总 deadline。
    deadline: Instant,
) -> Result<
    crate::components::browser_session_broker_protocol::response::BrowserSessionBrokerResponse,
    BrowserSessionExchangeFailure,
> {
    // 每次 WriteFile 前重建剩余预算；remaining 不进入 semantic key。
    let frame = build_request_frame_for_deadline(command, nonce, epoch_text, deadline)
        // 本次 frame 尚未交给 pipe，因此保留原 AppError。
        .map_err(BrowserSessionExchangeFailure::NotDispatched)?;
    // 单次非阻塞尝试将回压与投递不确定性封闭分类。
    match pipe.try_write_text_once(
        // 写入刚按当前预算生成并将绑定 decoder 的 frame。
        frame.text(),
        // 写前取消保持原始取消错误。
        cancellation::is_cancelled,
    ) {
        // 完整写入后才允许将该实际 frame 交给 response decoder。
        Ok(BoundedTextWriteAttempt::Written) => {}
        // 零字节回压由外层短暂停顿后重新生成 frame。
        Ok(BoundedTextWriteAttempt::BackpressureZero) => {
            // 当前 frame 未派发，绝不进入 recovery。
            return Err(BrowserSessionExchangeFailure::BackpressureZero);
        }
        // 平台错误或短写不能证明未派发，只能同 nonce 恢复。
        Ok(BoundedTextWriteAttempt::DeliveryUncertain) => {
            // 不公开 pipe 或 native 错误。
            return Err(BrowserSessionExchangeFailure::DeliveryMayHaveOccurred);
        }
        // Component 保留的写前错误仍由首次调用者原样接收。
        Err(failure) => {
            // 理论投递不确定分类同样不能被伪造为写前失败。
            if failure.delivery_may_have_occurred() {
                // 保守进入同 nonce 恢复。
                return Err(BrowserSessionExchangeFailure::DeliveryMayHaveOccurred);
            }
            // 零字节未派发时转移原安全 AppError。
            return Err(BrowserSessionExchangeFailure::NotDispatched(
                failure.into_error(),
            ));
        }
    }
    // 读取 accepted 或允许直接返回的 final。
    let first_text = pipe
        // 写入成功后任何读失败都不能证明未派发。
        .read_text_until(deadline, cancellation::is_cancelled)
        // 屏蔽 transport 诊断并保留投递语义。
        .map_err(|_| BrowserSessionExchangeFailure::DeliveryMayHaveOccurred)?;
    // 严格绑定原 request、nonce、operation 与认证 epoch。
    let first = decode_response(&first_text, frame.request(), epoch)
        // codec 失败不能被当作业务终态。
        .map_err(|_| BrowserSessionExchangeFailure::DeliveryMayHaveOccurred)?;
    // 直接 final 不再等待第二帧。
    if first.outcome().is_some() {
        // 返回已严格绑定的 final。
        return Ok(first);
    }
    // accepted 必须是冻结的 revision 零。
    if first.request_revision() != 0 {
        // 拒绝 decoder 以外的状态漂移。
        return Err(BrowserSessionExchangeFailure::DeliveryMayHaveOccurred);
    }
    // 读取 accepted 后唯一允许的 final。
    let final_text = pipe
        // accepted 后读失败只能进入原 nonce 恢复。
        .read_text_until(deadline, cancellation::is_cancelled)
        // 不泄漏 pipe 阶段细节。
        .map_err(|_| BrowserSessionExchangeFailure::DeliveryMayHaveOccurred)?;
    // 再次严格绑定原 request 与同一 epoch。
    let final_response = decode_response(&final_text, frame.request(), epoch)
        // codec 失败必须保守进入恢复。
        .map_err(|_| BrowserSessionExchangeFailure::DeliveryMayHaveOccurred)?;
    // accepted 后第二帧必须是 revision 一的 final。
    if final_response.outcome().is_none() || final_response.request_revision() != 1 {
        // 拒绝重复 accepted 或不单调终态。
        return Err(BrowserSessionExchangeFailure::DeliveryMayHaveOccurred);
    }
    // 返回可信 strict final。
    Ok(final_response)
}

// 构造写入开始后无法取得同 epoch 可信 final 的保守结果。
fn post_dispatch_outcome_unknown(
    // 借用字段封闭的原始业务选择。
    command: &BrowserSessionExchange<'_>,
) -> AppControlError {
    // 返回禁止自动重提的稳定未知结果。
    AppControlError::with_details(
        // 使用协议冻结的未知结果代码。
        "OUTCOME_UNKNOWN",
        // 不公开 pipe、broker 或 peer 状态。
        "The browser session broker request outcome is unknown after delivery began.",
        // 只保留公开 Module 可消费的保守业务事实。
        serde_json::json!({
            // 写入已开始，transport 可能已经接受。
            "transportAccepted": true,
            // 公共边界必须按可能越过业务接受点处理。
            "businessAccepted": true,
            // 没有取得可信 final。
            "completed": false,
            // 投递风险存在时禁止安全重试。
            "retrySafe": false,
            // 明确标记结果未知。
            "outcomeUnknown": true,
            // 只允许 Command 保守声明目标可能已经变化。
            "targetMayHaveMutated": command.may_mutate_target(),
        }),
    )
}

// 用 strict response 的安全布尔事实构造私有 client 失败。
fn response_failure(
    // 接收收敛后的静态错误码。
    code: &'static str,
    // 接收 wire codec 已验证的安全说明。
    message: &str,
    // 借用已经 request-bound 的 final 响应。
    response: &crate::components::browser_session_broker_protocol::response::BrowserSessionBrokerResponse,
) -> AppControlError {
    // 不输出 operation、nonce、epoch、revision 或 transport 标识。
    AppControlError::with_details(
        // 保存静态 client 错误码。
        code,
        // 复制已验证安全说明。
        message,
        // 只投影公共生命周期 Module 所需的封闭事实。
        serde_json::json!({
            // 保存 transport 接受事实。
            "transportAccepted": response.transport_accepted(),
            // 保存业务接受事实。
            "businessAccepted": response.business_accepted(),
            // 保存可信终态事实。
            "completed": response.completed(),
            // 保存协议冻结的重试事实。
            "retrySafe": response.retry_safe(),
            // 保存未知结果事实。
            "outcomeUnknown": response.outcome_unknown(),
            // 保存保守 mutation 事实。
            "targetMayHaveMutated": response.target_may_have_mutated(),
        }),
    )
}

// 将经严格 codec 验证的业务 final 投影为公开安全错误。
fn final_error(
    // 借用已经 request-bound 的 final 响应。
    response: &crate::components::browser_session_broker_protocol::response::BrowserSessionBrokerResponse,
) -> AppControlError {
    // broker 自己声明 unknown 时保持保守且禁止猜测执行结果。
    if response.outcome() == Some(BrowserSessionBrokerOutcome::Unknown) {
        // 返回绑定该 final 的统一未知结果。
        return response_failure(
            // 使用公开边界已登记的未知结果码。
            "OUTCOME_UNKNOWN",
            // 不公开 transport 阶段。
            "The browser session broker request outcome is unknown after delivery began.",
            // 保留 strict response 的安全布尔事实。
            response,
        );
    }
    // 只使用 wire codec 已验证的安全错误码和说明。
    match (response.error_code(), response.error_message()) {
        // 映射当前冻结且可作为 AppControlError 静态代码的业务前容量拒绝。
        (Some("BROWSER_SESSION_REGISTRY_FULL"), Some(message)) => {
            // 保留安全说明。
            response_failure("BROWSER_SESSION_REGISTRY_FULL", message, response)
        }
        // 映射当前冻结且可作为 AppControlError 静态代码的 stale session 拒绝。
        (Some("STALE_SESSION"), Some(message)) => {
            // 保留安全说明。
            response_failure("STALE_SESSION", message, response)
        }
        // 映射当前冻结的业务前 stale page 拒绝。
        (Some("STALE_PAGE"), Some(message)) => {
            // 保留安全说明。
            response_failure("STALE_PAGE", message, response)
        }
        // 映射当前冻结的业务前 stale element 拒绝。
        (Some("STALE_ELEMENT"), Some(message)) => {
            // 保留安全说明。
            response_failure("STALE_ELEMENT", message, response)
        }
        // 映射当前冻结的业务前 deadline 拒绝。
        (Some("REQUEST_EXPIRED"), Some(message)) => {
            // 保留安全说明。
            response_failure("REQUEST_EXPIRED", message, response)
        }
        // 其他经验证 safeError 统一投影为公开 broker 失败。
        (_, Some(message)) => {
            // 不将动态 wire code 写入静态 AppControlError code。
            response_failure("BROKER_OPERATION_FAILED", message, response)
        }
        // 缺少 safeError 的不应发生终态保持失败闭合。
        _ => response_failure(
            // 使用稳定公共失败代码。
            "BROKER_OPERATION_FAILED",
            // 不公开内部状态组合。
            "The browser session broker request did not complete successfully.",
            // 保留已验证 final 的安全布尔事实。
            response,
        ),
    }
}

// 在首帧写入开始后，只用同 nonce、同 epoch 与当前剩余预算恢复可信终态。
fn recover_same_epoch(
    // 借用字段封闭的原始命令选择。
    command: &BrowserSessionExchange<'_>,
    // 借用首次 ready 后唯一生成的 secure nonce。
    nonce: &str,
    // 借用首次 ready 得到的 epoch。
    expected_epoch: &str,
    // 接收首次 ready 后构造的总 deadline。
    deadline: Instant,
) -> AppResult<
    crate::components::browser_session_broker_protocol::response::BrowserSessionBrokerResponse,
> {
    // 直到总预算耗尽前，重连仅用于同 nonce attach/replay。
    loop {
        // 连接、认证或 ready 任何失败都不能证明动作未执行。
        let pipe = match connect_certified(deadline) {
            // 保留认证连接供恢复 exchange 使用。
            Ok(pipe) => pipe,
            // 取消、deadline 或 transport 失败都保守结束。
            Err(_) => return Err(post_dispatch_outcome_unknown(command)),
        };
        // ready 读取失败同样不能安全重新派发。
        let epoch = match read_certified_ready(&pipe, deadline) {
            // 保留恢复连接声明的 epoch。
            Ok(epoch) => epoch,
            // 不能信任当前命令的执行状态。
            Err(_) => return Err(post_dispatch_outcome_unknown(command)),
        };
        // epoch 改变表示原 ledger 已不可认证恢复。
        if epoch != expected_epoch {
            // 不新建 nonce 或 frame，直接返回未知。
            return Err(post_dispatch_outcome_unknown(command));
        }
        // 重新构造相同 epoch 的 decoder 私有投影。
        let decoded_epoch = match decode_epoch(&epoch) {
            // 保留经 canonical 验证的 epoch。
            Ok(decoded_epoch) => decoded_epoch,
            // 理论协议漂移按未知闭合。
            Err(_) => return Err(post_dispatch_outcome_unknown(command)),
        };
        // 在同一已认证连接内，零字节回压只会触发当前预算的 frame 重建。
        let exchange = loop {
            // 单次尝试只会使用同 nonce、同 epoch 和当前剩余预算。
            let exchange = exchange_on_connection(
                // 复用恢复连接。
                &pipe,
                // 保持原字段封闭命令。
                command,
                // 绝不生成新 nonce。
                nonce,
                // epoch 已与首次 ready 严格相等。
                &epoch,
                // decoder 仍绑定同一 canonical epoch。
                &decoded_epoch,
                // 复用唯一总 deadline。
                deadline,
            );
            // 只有可证明零字节回压允许留在同一连接重试。
            if matches!(
                &exchange,
                Err(BrowserSessionExchangeFailure::BackpressureZero)
            ) {
                // 退避绝不越过总 deadline；下次尝试会重新核验取消和预算。
                if !sleep_connect_retry_until(deadline) {
                    // 先前已存在投递不确定性，预算耗尽只能保守未知。
                    return Err(post_dispatch_outcome_unknown(command));
                }
                // 在新 remainingTimeoutMs 下重建同 nonce frame。
                continue;
            }
            // 完整写入、写后不确定或写前错误都交给恢复总状态处理。
            break exchange;
        };
        // 在下一次重连前主动释放本次失效连接。
        drop(pipe);
        // 只接受当前连接返回的可信 final。
        // 恢复连接上的零写或写后失败都不能改变原操作未知风险。
        if let Ok(response) = exchange {
            // 返回当前可信 final。
            return Ok(response);
        }
        // 避免连续失效连接导致 busy loop，但绝不越过原总 deadline。
        if !sleep_connect_retry_until(deadline) {
            // 原操作投递风险已存在，预算耗尽只能返回未知。
            return Err(post_dispatch_outcome_unknown(command));
        }
    }
}

// 执行一条 private mutation/Query，并把写后异常限制为同 epoch 恢复。
fn exchange_request(
    // 接收字段封闭的命令选择。
    command: BrowserSessionExchange<'_>,
    // 接收调用方总预算。
    timeout_ms: u32,
) -> AppResult<
    crate::components::browser_session_broker_protocol::response::BrowserSessionBrokerResponse,
> {
    // 冻结覆盖启动、连接、认证、ready、写入与恢复的总 deadline。
    let deadline = handshake_deadline(timeout_ms)?;
    // 写前连接失败仍保留其原始 AppError。
    let pipe = connect_certified(deadline)?;
    // 写前 ready 读取失败仍保留其原始 AppError。
    let epoch = read_certified_ready(&pipe, deadline)?;
    // 在 ready 后才生成一次且仅一次 request nonce。
    let nonce = random_nonce()?;
    // 将 ready epoch 变为 response decoder 的私有绑定。
    let decoded_epoch = decode_epoch(&epoch)?;
    // 在首连接上只让零字节回压重建当前剩余预算的 frame。
    let first_exchange = loop {
        // 单次尝试在 WriteFile 前取得当前剩余预算。
        let exchange = exchange_on_connection(
            // 复用首条认证连接。
            &pipe,
            // 保持字段封闭命令不变。
            &command,
            // 复用 ready 后唯一 secure nonce。
            &nonce,
            // 绑定首次 ready epoch。
            &epoch,
            // decoder 绑定当前 epoch。
            &decoded_epoch,
            // 复用唯一总 deadline。
            deadline,
        );
        // 仅在明确零字节回压后于同连接短暂退避。
        if matches!(
            &exchange,
            Err(BrowserSessionExchangeFailure::BackpressureZero)
        ) {
            // 退避至多消耗总 deadline 的剩余部分。
            if !sleep_connect_retry_until(deadline) {
                // 仍处于首次未派发路径，保留取消或 timeout 的原始错误。
                return Err(match remaining_request_timeout_ms(deadline) {
                    // 单调 deadline 已耗尽时应返回调用前错误投影。
                    Err(error) => error,
                    // 理论时钟异常不得误启动或恢复 broker。
                    Ok(_) => AppControlError::new(
                        // 保持稳定的写前 timeout 代码。
                        "TIMEOUT",
                        // 不公开单调时间源细节。
                        "The browser session broker handshake exhausted the request deadline.",
                    ),
                });
            }
            // 下次 WriteFile 前会重新生成更短 remainingTimeoutMs。
            continue;
        }
        // 其他情况都不得继续生成新 frame。
        break exchange;
    };
    // 避免失效首连接阻塞 broker 接受同 nonce attach/replay。
    drop(pipe);
    // 首次 exchange 一旦开始写入，任何失败都只能走同 nonce 恢复。
    match first_exchange {
        // 返回直接 final 或 accepted 后 final。
        Ok(response) => Ok(response),
        // 零字节全帧投递仍可证明未派发。
        Err(BrowserSessionExchangeFailure::NotDispatched(error)) => Err(error),
        // 写后或无法证明零投递的异常只能用原 nonce 恢复。
        Err(BrowserSessionExchangeFailure::DeliveryMayHaveOccurred) => {
            // 绝不生成新 nonce；恢复帧只会更新未入 semantic key 的剩余预算。
            recover_same_epoch(&command, &nonce, &epoch, deadline)
        }
        // 零字节回压已在首连接循环内消费，保留失败闭合防御。
        Err(BrowserSessionExchangeFailure::BackpressureZero) => Err(AppControlError::new(
            // 不公开内部循环状态。
            "BROKER_UNAVAILABLE",
            // 理论不可达状态保持安全失败。
            "The browser session broker request could not be dispatched.",
        )),
    }
}

// 打开会话并只在可信 completed open 成功时返回公开 session ID。
pub(crate) fn open_confirmed(timeout_ms: u32) -> AppResult<String> {
    // 执行字段封闭的 confirmed open exchange。
    let response = exchange_request(BrowserSessionExchange::OpenConfirmed, timeout_ms)?;
    // 只接受 completed open 成功投影。
    match (response.outcome(), response.success()) {
        // 返回严格 decoder 已验证的 opaque session identity。
        (
            Some(BrowserSessionBrokerOutcome::Completed),
            Some(BrowserSessionBrokerSuccess::Open { session_id }),
        ) => Ok(session_id.to_owned()),
        // 其余 final 都投影为冻结安全错误。
        _ => Err(final_error(&response)),
    }
}

// 关闭会话并只在可信 completed close 成功时返回完成事实。
pub(crate) fn close_confirmed(session_id: &str, timeout_ms: u32) -> AppResult<()> {
    // 执行字段封闭的 confirmed close exchange。
    let response = exchange_request(
        // 只借用公开 opaque session identity。
        BrowserSessionExchange::CloseConfirmed(session_id),
        // 传递完整调用预算。
        timeout_ms,
    )?;
    // 只接受 completed close 成功投影。
    match (response.outcome(), response.success()) {
        // close 不泄漏内部回收细节。
        (
            Some(BrowserSessionBrokerOutcome::Completed),
            Some(BrowserSessionBrokerSuccess::Close),
        ) => Ok(()),
        // 其余 final 都投影为冻结安全错误。
        _ => Err(final_error(&response)),
    }
}

// 查询公开 opaque session 是否仍属于当前 broker 代际的 live registry。
pub(crate) fn inspect_session(session_id: &str, timeout_ms: u32) -> AppResult<()> {
    // Query 与 mutation 共用认证、nonce、epoch、deadline、Attach 和 Replay 语义。
    let response = exchange_request(
        // 只构造字段封闭且不带 confirmed 的 session.inspect。
        BrowserSessionExchange::SessionInspect(session_id),
        // 传递覆盖完整物理交换的总预算。
        timeout_ms,
    )?;
    // 只接受 revision 一 completed 且回显相同 session 的 live 事实。
    match (response.outcome(), response.success()) {
        // strict decoder 已验证 sessionId 与原查询逐字相等且 live=true。
        (
            Some(BrowserSessionBrokerOutcome::Completed),
            Some(BrowserSessionBrokerSuccess::SessionInspect {
                session_id: returned_session_id,
                live,
            }),
        ) if returned_session_id == session_id && *live => Ok(()),
        // stale、failed 或 unknown 均复用冻结安全错误投影。
        _ => Err(final_error(&response)),
    }
}

// 仅加载 client frame 与中性错误语义的纯单元测试。
#[cfg(test)]
#[path = "browser_session_broker_windows_tests.rs"]
mod tests;
