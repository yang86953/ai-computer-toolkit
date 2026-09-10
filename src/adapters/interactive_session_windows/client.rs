//! 从 host session 发现并调用已认证独立交互会话 endpoint。

// 导入单调 deadline 类型。
use std::time::{Duration, Instant};

// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 导入 host client 使用的固定协议与 Windows 私有边界。
use crate::{
    // 导入 broker 协议与随机 nonce。
    components::{
        // 复用进程级取消信号。
        cancellation,
        // 导入首帧、证明、lease 与 command frame。
        interactive_session_broker_protocol::{
            BrokerCommandRequest, BrokerEndpointAttestation, BrokerEndpointOperation,
            BrokerEndpointResponse, BrokerInitialOperation, BrokerInitialRequest,
            BrokerProtocolFailure,
        },
        // 导入 command result 严格包装。
        interactive_session_broker_response::BrokerCommandResult,
        // 导入系统随机关联值。
        secure_nonce_windows::random_nonce,
    },
    // 导入统一错误 envelope。
    domain::{AppControlError, AppResult},
};

// 导入共享 fixed local IPC Windows Adapter。
use crate::adapters::fixed_local_ipc_windows::{
    // 导入 WTS 候选、fixed image 与 peer 认证。
    identity::{
        active_other_session_ids, authenticate_peer_process, current_session_id, sibling_image_path,
    },
    // 导入固定 local-only message pipe。
    pipe::ConnectedPipe,
};

// 固定 host 只连接同安装目录的 broker 二进制。
const BROKER_FILE_NAME: &str = "ai-computer-toolkit-interactive-session-broker.exe";
// 限制只读发现 handshake 的完整生命周期。
const DISCOVERY_TIMEOUT_MS: u32 = 5_000;

// 把私有 broker 协议失败映射为统一安全错误。
fn protocol_error(failure: BrokerProtocolFailure) -> AppControlError {
    // 只传播封闭错误码与固定消息。
    AppControlError::new(failure.code().as_str(), failure.code().message())
}

// 构造 command 可能被接受后的不可自动重试错误。
fn outcome_unknown() -> AppControlError {
    // 返回 provider-neutral 结果语义。
    AppControlError::with_details(
        // 使用固定 OutcomeUnknown 错误码。
        "OUTCOME_UNKNOWN",
        // 不公开 pipe、PID 或 session number。
        "The independent interactive session command may have been accepted before the endpoint disconnected.",
        // 固定不可重试与可能已变更事实。
        json!({
            // broker 已签发 lease 且 host 已发送完整 command。
            "businessAccepted": true,
            // 无法认证确定完成。
            "completed": false,
            // 使用稳定 outcome 文本。
            "outcome": "unknown",
            // 禁止自动重试 mutation。
            "retrySafe": false,
            // 保守声明目标可能已经改变。
            "targetMayHaveMutated": true,
            // 当前 host provider 从未接管。
            "localProviderInvoked": false,
            // 当前桌面 fallback 始终关闭。
            "foregroundFallbackUsed": false
        }),
    )
}

// 为尚未发送 command 的 endpoint 错误附加零副作用事实。
fn pre_dispatch_error(error: AppControlError) -> AppControlError {
    // 保留稳定错误码与安全消息并统一生命周期证据。
    AppControlError::with_details(
        // 传播封闭公开错误码。
        error.code,
        // 传播已经净化的安全消息。
        error.message,
        // 固定证明 host 本地 provider 和目标均未触碰。
        json!({
            // mutation 尚未进入目标业务状态机。
            "businessAccepted": false,
            // 请求没有确定完成。
            "completed": false,
            // 失败发生在 dispatch 前。
            "outcome": "not-dispatched",
            // 修复 endpoint 或重新发现后可安全重试。
            "retrySafe": true,
            // 目标不可能因本请求改变。
            "targetMayHaveMutated": false,
            // host 本地 provider 从未调用。
            "localProviderInvoked": false,
            // 当前桌面 fallback 始终关闭。
            "foregroundFallbackUsed": false
        }),
    )
}

// 从有界毫秒预算构造不会溢出的单调 deadline。
fn request_deadline(timeout_ms: u32) -> AppResult<Instant> {
    // 零值不能成为有效协议预算。
    if timeout_ms == 0 {
        // 返回普通参数错误。
        return Err(AppControlError::new(
            // 使用稳定参数码。
            "INVALID_ARGUMENT",
            // 不回显任意调用值。
            "The interactive endpoint timeout must be positive.",
        ));
    }
    // 由当前单调时刻加上有界预算。
    Instant::now()
        // 使用不会 panic 的加法。
        .checked_add(Duration::from_millis(u64::from(timeout_ms)))
        // 理论溢出失败闭合。
        .ok_or_else(|| {
            // 返回稳定参数错误。
            AppControlError::new(
                // 使用普通参数码。
                "INVALID_ARGUMENT",
                // 不公开单调时钟内部值。
                "The interactive endpoint deadline overflowed.",
            )
        })
}

// 在 command 构造前取得扣除 handshake 后的剩余毫秒预算。
fn remaining_timeout_ms(deadline: Instant) -> AppResult<u32> {
    // dispatch 前取消可以确定为零副作用。
    if cancellation::is_cancelled() {
        // 返回统一取消错误。
        return Err(AppControlError::new(
            // 使用稳定取消码。
            "CANCELLED",
            // 不公开当前 endpoint 状态。
            "The interactive endpoint request was cancelled before command dispatch.",
        ));
    }
    // 计算扣除发现、连接、认证与 lease 的剩余预算。
    let remaining = deadline
        // deadline 到达后不得发送 command。
        .checked_duration_since(Instant::now())
        // 映射为确定性 dispatch 前 timeout。
        .ok_or_else(|| {
            // 返回统一 timeout。
            AppControlError::new(
                // 使用稳定 deadline 码。
                "TIMEOUT",
                // 明确 command 尚未发送。
                "The interactive endpoint handshake exhausted the command deadline.",
            )
        })?;
    // 向下取整为协议毫秒，确保 worker 预算绝不扩大原始 deadline。
    let millis = remaining.as_millis();
    // 不足一个完整协议毫秒时不得再派发 mutation。
    if millis == 0 {
        // 返回确定性的 dispatch 前 timeout。
        return Err(AppControlError::new(
            // 使用稳定 deadline 码。
            "TIMEOUT",
            // 明确 command 尚未发送。
            "The interactive endpoint handshake exhausted the command deadline.",
        ));
    }
    // 协议上限保证正常路径可收窄为 u32。
    u32::try_from(millis).map_err(|_| {
        // 理论溢出仍失败闭合。
        AppControlError::new(
            // 使用稳定参数码。
            "INVALID_ARGUMENT",
            // 不公开内部时钟值。
            "The interactive endpoint remaining deadline overflowed.",
        )
    })
}

// 表示已经双向认证但不公开 native session 的 endpoint。
#[derive(Clone)]
pub(crate) struct CertifiedInteractiveEndpoint {
    // 保存只供后续连接使用的 native session ID。
    native_session_id: u32,
    // 保存 broker 发布并由 host 验证的公开证明。
    attestation: BrokerEndpointAttestation,
}

// 为认证 endpoint 提供 provider-neutral 只读投影。
impl CertifiedInteractiveEndpoint {
    // 构造只用于纯选择测试且不连接真实 endpoint 的认证 fixture。
    #[cfg(test)]
    pub(crate) fn fixture(native_session_id: u32, interactive_session_id: &str) -> Self {
        // 用生产证明构造器保证 fixture 仍满足完整冻结契约。
        let attestation = BrokerEndpointAttestation::new(interactive_session_id.to_owned())
            // 测试只传入 canonical s2:i，失败必须立即暴露。
            .unwrap_or_else(|_| panic!("interactive endpoint fixture ID must be canonical"));
        // 返回不触碰 WTS、pipe 或 provider 的纯数据对象。
        Self {
            // 保存仅供选择结果区分的测试 native session。
            native_session_id,
            // 保存完整认证证明。
            attestation,
        }
    }

    // 返回 canonical s2:i 授权代际。
    pub(crate) fn interactive_session_id(&self) -> &str {
        // 不公开 native session number。
        self.attestation.interactive_session_id()
    }

    // 返回符合公共 observation schema 的 JSON 对象。
    pub(crate) fn observation(&self) -> Value {
        // 只投影冻结的 provider-neutral 字段。
        json!({
            // 公共 schema 使用通用 sessionId 字段。
            "sessionId": self.attestation.interactive_session_id(),
            // 固定目标类别。
            "targetKind": "independent-interactive-session",
            // 只有认证 endpoint 才进入集合。
            "state": "available",
            // 固定隔离执行域。
            "executionRealm": "isolated-worker",
            // 固定独立交互会话类别。
            "isolationKind": "independent-interactive-session",
            // 发布冻结的四条通用 mutation。
            "capabilities": [
                // 通用键盘输入。
                "ui.input.key@1",
                // 通用指针输入。
                "ui.input.pointer@1",
                // 窗口状态与几何生命周期。
                "window.lifecycle@1",
                // 独立确认窗口关闭。
                "window.close@1"
            ],
            // 身份同时绑定登录与授权生命周期。
            "identityFreshness": "authorization-and-session-lifetime"
        })
    }
}

// 对一个 WTS 候选执行完整 host 侧观察握手。
fn probe_session(native_session_id: u32) -> AppResult<CertifiedInteractiveEndpoint> {
    // 不允许 host 当前 session 进入独立 route。
    if native_session_id == current_session_id()? {
        // 返回稳定 endpoint 认证失败。
        return Err(AppControlError::new(
            // 使用固定认证错误码。
            "ENDPOINT_AUTHENTICATION_FAILED",
            // 不公开 native session number。
            "The interactive endpoint is not independent from the host session.",
        ));
    }
    // 为发现连接和响应冻结一个总 handshake deadline。
    let deadline = request_deadline(DISCOVERY_TIMEOUT_MS)?;
    // 连接候选 session 的固定 local-only pipe。
    let pipe = ConnectedPipe::connect_until(
        // 绑定 WTS 候选。
        native_session_id,
        // 连接和读取共享同一 deadline。
        deadline,
        // 响应进程级取消。
        cancellation::is_cancelled,
    )?;
    // 取得内核记录的 server PID。
    let server_process_id = pipe.peer_process_id()?;
    // 定位同安装目录的固定 broker 镜像。
    let broker_image = sibling_image_path(BROKER_FILE_NAME)?;
    // 核对 fixed image、候选 session 与同一用户主体。
    authenticate_peer_process(server_process_id, &broker_image, native_session_id)?;
    // 生成一次性观察请求 nonce。
    let request_nonce = random_nonce()?;
    // 构造字段封闭的观察首帧。
    let request = BrokerInitialRequest::new(
        // 保存随机关联值。
        request_nonce.clone(),
        // 只执行观察，不签发 lease。
        BrokerInitialOperation::Observe,
    )
    // 系统随机输出理论上始终 canonical。
    .ok_or_else(|| {
        // 形状漂移保持认证失败。
        AppControlError::new(
            // 使用固定认证错误码。
            "ENDPOINT_AUTHENTICATION_FAILED",
            // 不公开随机值。
            "The interactive endpoint request nonce was invalid.",
        )
    })?;
    // 交付单条观察请求。
    pipe.write_json(&request)?;
    // 读取单条 endpoint response。
    let text = pipe.read_text_until(
        // 复用发现 handshake deadline。
        deadline,
        // 响应进程级取消。
        cancellation::is_cancelled,
    )?;
    // 严格核对版本、nonce、操作、构建与 endpoint 证明。
    let response = BrokerEndpointResponse::parse(
        // 传入完整响应文本。
        &text,
        // 绑定本次随机 nonce。
        &request_nonce,
        // 只接受观察响应。
        BrokerEndpointOperation::Observation,
    )
    // 映射为统一安全错误。
    .map_err(protocol_error)?;
    // 返回保留 private native route 的认证 endpoint。
    Ok(CertifiedInteractiveEndpoint {
        // 保存后续重新握手使用的 WTS session。
        native_session_id,
        // 复制已经完整验证的公开证明。
        attestation: response.endpoint().clone(),
    })
}

// 枚举并只返回通过双向认证的独立交互 endpoint。
pub(crate) fn discover_certified_endpoints() -> AppResult<Vec<CertifiedInteractiveEndpoint>> {
    // 保存认证成功端点。
    let mut endpoints = Vec::new();
    // 逐个探测活动且不同于 host 的 WTS session。
    for session_id in active_other_session_ids()? {
        // 未部署、未认证或竞态消失的候选不进入公开集合。
        if let Ok(endpoint) = probe_session(session_id) {
            // 保存认证 endpoint。
            endpoints.push(endpoint);
        }
    }
    // 使用公开 opaque ID 排序，避免泄漏 native session 顺序。
    endpoints.sort_by(|left, right| {
        // 比较唯一公开身份。
        left.interactive_session_id()
            // 与右侧公开身份比较。
            .cmp(right.interactive_session_id())
    });
    // 返回只含认证 endpoint 的集合。
    Ok(endpoints)
}

// 在重新认证的 endpoint lease 上执行唯一封闭 command。
pub(crate) fn execute_certified_command(
    // 接收发现阶段保留的认证 endpoint。
    endpoint: &CertifiedInteractiveEndpoint,
    // 接收覆盖 host handshake 与 worker 的唯一总预算。
    timeout_ms: u32,
    // 只允许上层用两个 nonce 和扣减后的 deadline 构造固定 command。
    build_command: impl FnOnce(&str, &str, u32) -> Value,
) -> AppResult<Value> {
    // 在任何 endpoint 连接前冻结完整请求 deadline。
    let deadline = request_deadline(timeout_ms).map_err(pre_dispatch_error)?;
    // 重新连接固定 broker，旧观察不能充当当前认证。
    let pipe = ConnectedPipe::connect_until(
        // 使用发现保留的 private session route。
        endpoint.native_session_id,
        // 连接耗时计入完整请求预算。
        deadline,
        // 响应进程级取消。
        cancellation::is_cancelled,
    )
    // command 尚未发送，连接失败保持可安全重试。
    .map_err(pre_dispatch_error)?;
    // 取得本连接 server PID。
    let server_process_id = pipe
        // 内核 peer 查询发生在 command 前。
        .peer_process_id()
        // 查询失败保持零副作用证据。
        .map_err(pre_dispatch_error)?;
    // 定位固定 broker image。
    let broker_image = sibling_image_path(BROKER_FILE_NAME)
        // 固定 sibling 缺失发生在 command 前。
        .map_err(pre_dispatch_error)?;
    // 再次核对 fixed image、session 与主体。
    authenticate_peer_process(
        // 传入内核 server PID。
        server_process_id,
        // 传入固定 sibling 路径。
        &broker_image,
        // 绑定发现时的 private WTS session。
        endpoint.native_session_id,
    )
    // 对等认证发生在 command 前。
    .map_err(pre_dispatch_error)?;
    // 为 command lease 生成新的请求 nonce。
    let request_nonce = random_nonce()
        // 随机源失败发生在 command 前。
        .map_err(pre_dispatch_error)?;
    // 构造 lease 首帧。
    let request = BrokerInitialRequest::new(
        // 保存一次性关联值。
        request_nonce.clone(),
        // 请求同一连接唯一 command lease。
        BrokerInitialOperation::OpenCommandLease,
    )
    // 系统随机输出理论上始终 canonical。
    .ok_or_else(|| {
        // 返回封闭认证失败。
        pre_dispatch_error(AppControlError::new(
            // 使用固定错误码。
            "ENDPOINT_AUTHENTICATION_FAILED",
            // 不公开随机值。
            "The interactive endpoint lease request nonce was invalid.",
        ))
    })?;
    // 交付 lease 请求。
    pipe.write_json(&request)
        // lease 请求不携带 mutation。
        .map_err(pre_dispatch_error)?;
    // 读取 lease response。
    let text = pipe
        // handshake 与 worker 共享原始 deadline。
        .read_text_until(deadline, cancellation::is_cancelled)
        // command 尚未发送，timeout 或断线可安全重试。
        .map_err(pre_dispatch_error)?;
    // 严格核对当前 lease 与 endpoint 证明。
    let response = BrokerEndpointResponse::parse(
        // 传入完整响应文本。
        &text,
        // 绑定请求 nonce。
        &request_nonce,
        // 只接受 command lease。
        BrokerEndpointOperation::CommandLease,
    )
    // 映射为统一安全错误。
    .map_err(protocol_error)
    // lease 响应认证失败发生在 command 前。
    .map_err(pre_dispatch_error)?;
    // endpoint 授权代际变化必须返回 stale，不能自动重绑。
    if response.endpoint().interactive_session_id() != endpoint.interactive_session_id() {
        // 返回明确 stale 语义。
        return Err(pre_dispatch_error(AppControlError::new(
            // 使用固定 stale 错误码。
            "STALE_SESSION",
            // 不公开新旧 native session。
            "The independent interactive session authorization changed after discovery.",
        )));
    }
    // 取得 broker 签发的一次性 lease。
    let endpoint_lease_nonce = response.endpoint_lease_nonce().ok_or_else(|| {
        // 缺失 lease 已违反 parse 不变量。
        pre_dispatch_error(AppControlError::new(
            // 使用固定认证错误码。
            "ENDPOINT_AUTHENTICATION_FAILED",
            // 不公开响应内容。
            "The interactive endpoint omitted its command lease.",
        ))
    })?;
    // 扣除连接、peer 认证与 lease handshake 后取得 worker 剩余预算。
    let remaining_timeout_ms = remaining_timeout_ms(deadline).map_err(pre_dispatch_error)?;
    // 由上层 Module 用两个关联值和剩余预算构造冻结 command worker 请求。
    let command = build_command(
        // 绑定本连接请求 nonce。
        &request_nonce,
        // 绑定 broker 签发 lease。
        endpoint_lease_nonce,
        // 只传递扣减后的 worker 预算。
        remaining_timeout_ms,
    );
    // 包装并在 broker 再次完整交叉验证。
    let request = BrokerCommandRequest::new(
        // 绑定首帧 nonce。
        request_nonce.clone(),
        // 绑定本连接 lease。
        endpoint_lease_nonce.to_owned(),
        // 绑定发现的公开授权代际。
        endpoint.interactive_session_id().to_owned(),
        // 保存上层冻结 command。
        command,
    );
    // command 写入一旦开始就可能被 broker 接受，任何异常都必须保守视为 OutcomeUnknown。
    pipe.write_json(&request).map_err(|_| outcome_unknown())?;
    // 读取 command result；此处开始保守映射 accepted 后失败。
    let text = pipe
        // accepted 后仍受原始单调 deadline 与取消约束。
        .read_text_until(deadline, cancellation::is_cancelled)
        // 任何 timeout、取消或断线都保守映射为未知结果。
        .map_err(|_| outcome_unknown())?;
    // 严格核对 broker 包装的两个关联值。
    let response = BrokerCommandResult::parse(
        // 传入完整响应。
        &text,
        // 绑定请求 nonce。
        &request_nonce,
        // 绑定本连接 lease。
        endpoint_lease_nonce,
    )
    // 异常结果视为 accepted 后未知。
    .map_err(|_| outcome_unknown())?;
    // 返回 fixed worker 的完整业务 envelope。
    Ok(response.into_result())
}

// 将 host 生命周期结果语义测试放在独立文件。
#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
