//! 在显式授权的独立交互会话内运行固定 session broker。

// 导入标准输出、路径与 deadline 类型。
use std::{
    // 导入结构化启动错误写出接口。
    io::Write,
    // 导入固定 worker 路径。
    path::Path,
    // 导入 command worker deadline 与 broker handshake 单调时钟。
    time::{Duration, Instant},
};

// 导入最终响应使用的 JSON 序列化 trait。
use serde::Serialize;
// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 导入 broker 需要组合的窄 Adapter、Component 与 Module 事实。
use crate::{
    // 导入 Windows endpoint 与 peer 认证。
    adapters::{
        // 导入独立会话 Windows 私有边界。
        fixed_local_ipc_windows::{
            // 导入进程、session 与固定 sibling 认证。
            identity::{
                active_session_generation, authenticate_peer_process, current_session_id,
                process_session_id, sibling_image_path,
            },
            // 导入 local-only message pipe。
            pipe::{ConnectedPipe, MAXIMUM_FRAME_BYTES, ServerPipe},
        },
        // 导入活动桌面本地证明。
        security_context_windows::WindowsSecurityContextProbe,
    },
    // 导入 broker 协议、结果包装、随机源与 Job runner。
    components::{
        // 导入进程内取消状态。
        cancellation,
        // 导入首帧、lease 与 command frame。
        interactive_session_broker_protocol::{
            BrokerCommandRequest, BrokerEndpointAttestation, BrokerEndpointResponse,
            BrokerInitialOperation, BrokerInitialRequest, BrokerProtocolFailure,
        },
        // 导入 command result 与 dispatch 前拒绝。
        interactive_session_broker_response::{BrokerCommandResult, rejection},
        // 导入授权代际的 opaque 指纹原语。
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
        // 导入系统随机 nonce。
        secure_nonce_windows::random_nonce,
        // 导入固定 sibling Job runner。
        worker_process::run_companion,
    },
    // 导入统一错误 envelope。
    domain::{AppControlError, AppResult, error_json},
    // 导入活动会话与输入桌面封闭事实。
    modules::permission_boundary::{
        DesktopSecurityState, SecurityContextProbe, SessionSecurityState,
    },
};

// 固定只允许主产品 CLI 连接 broker。
const MAIN_FILE_NAME: &str = "ai-computer-toolkit.exe";
// 固定 broker 只创建同安装目录的 command worker。
const COMMAND_WORKER_FILE_NAME: &str = "ai-computer-toolkit-interactive-command-worker.exe";
// 固定 command worker stdout 上限与 broker frame 上限一致。
const MAXIMUM_WORKER_OUTPUT_BYTES: usize = MAXIMUM_FRAME_BYTES;
// 限制每条 broker 首帧或 lease 后 command 的空闲等待。
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

// 构造一次不会继承旧连接预算的 broker handshake deadline。
fn handshake_deadline() -> Instant {
    // 五秒固定边界不会溢出实际 Windows 单调时钟。
    Instant::now() + HANDSHAKE_TIMEOUT
}

// 构造不泄漏 session、PID、pipe 或路径的 broker 错误。
fn broker_error(code: &'static str, message: &'static str) -> AppControlError {
    // 复用统一公开错误 envelope。
    AppControlError::new(code, message)
}

// 把 broker 私有协议失败收敛为统一进程错误。
fn protocol_error(failure: BrokerProtocolFailure) -> AppControlError {
    // 只传播封闭错误码与固定安全消息。
    AppControlError::new(failure.code().as_str(), failure.code().message())
}

// 写出最终响应并有界等待 client 读取后关闭连接。
fn write_final_response(pipe: &ConnectedPipe, response: &impl Serialize) -> AppResult<()> {
    // 单帧写入只占用创建时已经预留的有界 pipe 输出缓冲区。
    pipe.write_json(response)?;
    // 最终响应后协议不接受额外 client frame，只等待读取方关闭连接。
    match pipe.read_text_until(
        // 防止已认证但挂起的 client 无限占用唯一 broker instance。
        handshake_deadline(),
        // 响应 broker 进程取消。
        cancellation::is_cancelled,
    ) {
        // client 读完响应后关闭 handle 是正常的 delivery completion。
        Err(error) if error.code == "ISOLATED_WORKER_UNAVAILABLE" => Ok(()),
        // timeout、取消或其他平台失败关闭本连接并让长期 broker 恢复服务。
        Err(error) => Err(error),
        // 最终响应后的额外 frame 违反每连接封闭状态机。
        Ok(_) => Err(broker_error(
            // 使用普通协议参数码。
            "INVALID_ARGUMENT",
            // 不回显额外 frame 内容。
            "The interactive endpoint received an unexpected frame after its final response.",
        )),
    }
}

// 认证 broker 自身仍位于活动 Default 输入桌面。
fn certify_local_interactive_context() -> AppResult<()> {
    // 捕获不含 native 值的封闭安全姿态。
    let facts = WindowsSecurityContextProbe.probe();
    // 同时要求活动交互会话和 Default 输入桌面。
    if facts.session != SessionSecurityState::ActiveInteractive
        // 锁屏、UAC 与非输入桌面均不得发布 endpoint。
        || facts.desktop != DesktopSecurityState::ActiveDefaultInput
    {
        // 返回稳定权限阻塞而不尝试绕过。
        return Err(broker_error(
            // 使用公开权限拒绝码。
            "PERMISSION_DENIED",
            // 不公开桌面或 session 名称。
            "The interactive session broker requires an active Default input desktop.",
        ));
    }
    // 返回本地会话状态通过。
    Ok(())
}

// 从系统 session 登录代际与随机授权代际构造公开 s2:i。
fn authorized_interactive_session_id(session_id: u32) -> AppResult<String> {
    // 取得 WTS 登录代际并同时认证活动状态。
    let session_generation = active_session_generation(session_id)?;
    // 生成每次 broker 启动唯一的授权代际。
    let authorization_generation = random_nonce()?;
    // 组合只供散列的私有身份。
    let identity = format!("{session_generation}|{authorization_generation}");
    // 只返回 canonical opaque 指纹。
    Ok(OpaqueTargetId::new(OpaqueTargetKind::InteractiveSession, &identity).to_string())
}

// 把协议拒绝写回已认证连接。
fn write_rejection(
    pipe: &ConnectedPipe,
    failure: &BrokerProtocolFailure,
    lease_issued: bool,
) -> AppResult<()> {
    // peer 和本地 session 已在读取协议帧前认证。
    let response = rejection(
        // 传入封闭失败事实。
        failure,
        // peer 已认证。
        true,
        // broker 本地 session 已认证。
        true,
        // 记录是否已签发本连接 lease。
        lease_issued,
    );
    // 写入单条有界 JSON 响应并等待 client 读取后释放连接。
    write_final_response(pipe, &response)
}

// 验证 fixed command worker 的完整 envelope 与结果状态机。
fn validate_worker_envelope(value: &Value, expected_request_nonce: &str) -> AppResult<()> {
    // worker 必须返回字段可读的 JSON object。
    let object = value.as_object().ok_or_else(|| {
        // 不回显 worker 输出。
        broker_error(
            "WORKER_PROTOCOL_FAILED",
            "The fixed interactive command worker returned a non-object response.",
        )
    })?;
    // 核对固定版本与关联值。
    let identity_valid = object
        // 读取固定协议版本。
        .get("contractVersion")
        // 要求逐字匹配。
        .and_then(Value::as_str)
        == Some("act/interactive-command-worker/v1")
        // request nonce 必须与 broker frame 相同。
        && object
            // 读取 worker 回显 nonce。
            .get("requestNonce")
            // 要求字符串。
            .and_then(Value::as_str)
            == Some(expected_request_nonce);
    // 身份不一致不能作为本请求结果。
    if !identity_valid {
        // 返回稳定 worker 协议失败。
        return Err(broker_error(
            "WORKER_PROTOCOL_FAILED",
            "The fixed interactive command worker response identity is invalid.",
        ));
    }
    // 读取状态机必需布尔字段。
    let transport_accepted = object.get("transportAccepted").and_then(Value::as_bool);
    // 读取业务接受事实。
    let business_accepted = object.get("businessAccepted").and_then(Value::as_bool);
    // 读取确定完成事实。
    let completed = object.get("completed").and_then(Value::as_bool);
    // 读取重试安全事实。
    let retry_safe = object.get("retrySafe").and_then(Value::as_bool);
    // 读取目标变更可能性。
    let target_may_have_mutated = object
        // 访问固定字段。
        .get("targetMayHaveMutated")
        // 要求布尔值。
        .and_then(Value::as_bool);
    // 读取封闭 outcome。
    let outcome = object.get("outcome").and_then(Value::as_str);
    // 按三种协议状态核对不可变关系。
    let state_valid = match (
        // 传入 transport 事实。
        transport_accepted,
        // 传入业务事实。
        business_accepted,
        // 传入完成事实。
        completed,
        // 传入 outcome。
        outcome,
        // 传入重试事实。
        retry_safe,
        // 传入目标事实。
        target_may_have_mutated,
    ) {
        // dispatch 前拒绝允许 transport true 或 false，但业务必须未接受。
        (Some(_), Some(false), Some(false), Some("not-dispatched"), Some(true), Some(false)) => {
            // 状态关系完整。
            true
        }
        // 确定完成必须已经接受业务且禁止自动重试。
        (Some(true), Some(true), Some(true), Some("completed"), Some(false), Some(true)) => true,
        // OutcomeUnknown 必须保守声明可能已变更。
        (Some(true), Some(true), Some(false), Some("unknown"), Some(false), Some(true)) => true,
        // 其他组合违反冻结协议。
        _ => false,
    };
    // 状态关系不成立时拒绝整个结果。
    if !state_valid {
        // 返回稳定 worker 协议失败。
        return Err(broker_error(
            "WORKER_PROTOCOL_FAILED",
            "The fixed interactive command worker response state is invalid.",
        ));
    }
    // evidence 必须是对象且明确禁止当前桌面 fallback。
    let evidence_valid = object
        // 读取固定证据字段。
        .get("evidence")
        // 要求对象。
        .and_then(Value::as_object)
        // 核对所有必需布尔字段存在。
        .is_some_and(|evidence| {
            // 本地 provider 调用必须是布尔值。
            evidence.get("localProviderInvoked").and_then(Value::as_bool).is_some()
                // 目标解析必须是布尔值。
                && evidence.get("targetResolved").and_then(Value::as_bool).is_some()
                // mutation dispatch 必须是布尔值。
                && evidence.get("mutationDispatched").and_then(Value::as_bool).is_some()
                // broker 永不接受当前桌面 fallback。
                && evidence.get("foregroundFallbackUsed") == Some(&Value::Bool(false))
        });
    // 缺失证据不能认证结果。
    if !evidence_valid {
        // 返回稳定 worker 协议失败。
        return Err(broker_error(
            "WORKER_PROTOCOL_FAILED",
            "The fixed interactive command worker response evidence is invalid.",
        ));
    }
    // 返回完整 worker envelope 通过。
    Ok(())
}

// 在同一已认证连接内执行唯一 command lease。
fn execute_leased_command(
    pipe: &ConnectedPipe,
    request_nonce: &str,
    endpoint_lease_nonce: &str,
    interactive_session_id: &str,
    command_worker: &Path,
) -> AppResult<()> {
    // 读取 lease 后唯一 command frame。
    let text = pipe.read_text_until(
        // lease 后 command 必须在固定 handshake 窗口内到达。
        handshake_deadline(),
        // 响应 broker 进程取消。
        cancellation::is_cancelled,
    )?;
    // 交叉验证连接、lease、endpoint 与 command worker 请求。
    let command = match BrokerCommandRequest::parse(
        // 传入完整 JSON 文本。
        &text,
        // 绑定首帧请求 nonce。
        request_nonce,
        // 绑定本连接唯一 lease。
        endpoint_lease_nonce,
        // 绑定当前 broker 授权代际。
        interactive_session_id,
    ) {
        // 保存已验证 command。
        Ok(command) => command,
        // 协议拒绝发生在启动 worker 前。
        Err(failure) => {
            // 写出零 dispatch 拒绝。
            write_rejection(pipe, &failure, true)?;
            // 本连接已得到确定响应。
            return Ok(());
        }
    };
    // 从强类型 command 取得剩余 deadline。
    let timeout_ms = command
        // 读取已经验证的强类型剩余预算。
        .command_timeout_ms()
        // 理论上的协议漂移收敛为统一错误。
        .map_err(protocol_error)?;
    // 在恢复 worker 主线程前由统一 Component 绑定关闭即回收 Job。
    let output = run_companion(
        // 使用固定同安装 sibling 路径。
        command_worker,
        // command worker 不接受任何 argv。
        &[],
        // 只传递已经完整交叉验证的 JSON request。
        command.command(),
        // 使用 host 冻结且 broker 未扩大的剩余预算。
        Duration::from_millis(u64::from(timeout_ms)),
        // 限制完整 worker envelope。
        MAXIMUM_WORKER_OUTPUT_BYTES,
        // 复用 broker 进程取消信号。
        cancellation::is_cancelled,
    )?;
    // worker 只允许成功或结构化业务拒绝退出码。
    if !matches!(output.exit_code, 0 | 2) {
        // 连接将关闭，host 必须按 accepted 后断线处理 OutcomeUnknown。
        return Err(broker_error(
            "WORKER_PROTOCOL_FAILED",
            "The fixed interactive command worker exited without a certified result.",
        ));
    }
    // 验证完整 worker 状态机和关联值。
    validate_worker_envelope(&output.envelope, command.request_nonce())?;
    // 用同一 lease 包装已经完成回收的 worker 结果。
    let result = BrokerCommandResult::new(
        // 绑定本次请求 nonce。
        command.request_nonce(),
        // 绑定本连接 endpoint lease。
        command.endpoint_lease_nonce(),
        // 转移 worker envelope。
        output.envelope,
    );
    // 写回唯一 command result，并有界等待 host 读取后关闭连接。
    write_final_response(pipe, &result)
}

// 处理一个经过内核连接的固定 broker session。
fn serve_connection(
    pipe: ConnectedPipe,
    broker_session_id: u32,
    interactive_session_id: &str,
    main_image: &Path,
    command_worker: &Path,
) -> AppResult<()> {
    // 每条连接重新认证 broker 本地活动桌面。
    certify_local_interactive_context()?;
    // 取得内核记录的 client PID。
    let peer_process_id = pipe.peer_process_id()?;
    // 取得 client 所属 session。
    let peer_session_id = process_session_id(peer_process_id)?;
    // 独立会话要求 client 与 broker 系统 session 不同。
    if peer_session_id == broker_session_id {
        // 同会话调用不能冒充零打扰 endpoint。
        return Err(broker_error(
            "ENDPOINT_AUTHENTICATION_FAILED",
            "The interactive endpoint peer is not in an independent session.",
        ));
    }
    // 核对固定主程序镜像、精确 peer session 与同一用户主体。
    authenticate_peer_process(peer_process_id, main_image, peer_session_id)?;
    // 读取首条观察或 lease frame。
    let text = pipe.read_text_until(
        // 未认证 peer 不能无限占用唯一 broker instance。
        handshake_deadline(),
        // 响应 broker 进程取消。
        cancellation::is_cancelled,
    )?;
    // 严格解析首帧。
    let request = match BrokerInitialRequest::parse(&text) {
        // 保存已验证请求。
        Ok(request) => request,
        // 协议错误在任何 lease 或 worker 前返回。
        Err(failure) => {
            // 写出零副作用拒绝。
            write_rejection(&pipe, &failure, false)?;
            // 本连接得到确定结果。
            return Ok(());
        }
    };
    // 构造不含 native 事实的 endpoint 证明。
    let endpoint = BrokerEndpointAttestation::new(interactive_session_id.to_owned())
        // 内部 opaque 构造漂移收敛为统一错误。
        .map_err(protocol_error)?;
    // 按首帧操作决定连接生命周期。
    match request.operation() {
        // 只读观察返回证明后立即断开。
        BrokerInitialOperation::Observe => {
            // 构造不签发 lease 的响应。
            let response = BrokerEndpointResponse::observation(request.request_nonce(), endpoint);
            // 写出单条观察结果，并有界等待 host 读取后关闭连接。
            write_final_response(&pipe, &response)
        }
        // command lease 在同一连接继续接收唯一命令。
        BrokerInitialOperation::OpenCommandLease => {
            // 生成 broker 独有的一次性 lease nonce。
            let endpoint_lease_nonce = random_nonce()?;
            // 构造严格 lease 响应。
            let response = BrokerEndpointResponse::command_lease(
                // 绑定首帧请求 nonce。
                request.request_nonce(),
                // 复制一份给响应，原值继续绑定本连接。
                endpoint_lease_nonce.clone(),
                // 附加 endpoint 证明。
                endpoint,
            )
            // 随机 lease 形状漂移收敛为统一错误。
            .map_err(protocol_error)?;
            // 先交付 lease，再读取同一连接唯一 command。
            pipe.write_json(&response)?;
            // 启动固定 Job worker 或返回 dispatch 前拒绝。
            execute_leased_command(
                // 复用同一 pipe。
                &pipe,
                // 绑定首帧 nonce。
                request.request_nonce(),
                // 绑定 broker 签发 lease。
                &endpoint_lease_nonce,
                // 绑定当前授权代际。
                interactive_session_id,
                // 使用固定 command worker 路径。
                command_worker,
            )
        }
    }
}

// 运行显式授权会话中的单实例 broker 循环。
fn run_server() -> AppResult<()> {
    // 启动前认证活动 Default 输入桌面。
    certify_local_interactive_context()?;
    // 取得 broker 当前 native session。
    let broker_session_id = current_session_id()?;
    // 构造本次启动唯一授权代际。
    let interactive_session_id = authorized_interactive_session_id(broker_session_id)?;
    // 定位固定主产品 CLI 镜像供 peer 认证。
    let main_image = sibling_image_path(MAIN_FILE_NAME)?;
    // 定位固定 command worker 供 Job runner 使用。
    let command_worker = sibling_image_path(COMMAND_WORKER_FILE_NAME)?;
    // 持续服务串行、单次 lease 连接。
    loop {
        // 进程取消在新连接创建前停止服务。
        if cancellation::is_cancelled() {
            // 返回正常关闭。
            return Ok(());
        }
        // 创建带显式 DACL 和首实例门禁的固定 pipe。
        let server = ServerPipe::create(broker_session_id)?;
        // 等待一个本机 client，同时保持空闲 broker 可取消。
        let pipe = match server.accept(cancellation::is_cancelled) {
            // 保存已经连接的固定 pipe。
            Ok(pipe) => pipe,
            // Ctrl+C 在没有 client 时属于正常关闭。
            Err(error) if error.code == "CANCELLED" => return Ok(()),
            // 其他 endpoint 错误终止本次 broker 启动。
            Err(error) => return Err(error),
        };
        // 单个未认证或中断 client 只关闭自己的 pipe，不终止授权代际。
        let _ = serve_connection(
            // 转移连接所有权。
            pipe,
            // 传递 broker native session 私有事实。
            broker_session_id,
            // 传递公开授权代际。
            &interactive_session_id,
            // 传递固定主程序镜像。
            &main_image,
            // 传递固定 command worker 镜像。
            &command_worker,
        );
    }
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
                json!({
                    // 标记启动失败。
                    "ok": false,
                    // 使用稳定 broker 错误码。
                    "error": {
                        // 固定序列化失败码。
                        "code": "WORKER_PROTOCOL_FAILED",
                        // 固定安全消息。
                        "message": "The interactive session broker could not serialize its startup error."
                    }
                })
                // JSON 宏生成对象可以稳定序列化为文本。
                .to_string()
            });
            // 只向 stdout 写一条 JSON 诊断。
            let _ = writeln!(std::io::stdout(), "{text}");
            // 返回结构化失败退出码。
            2
        }
    }
}

// 声明 worker envelope 状态机的纯回归。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 导入被测验证函数。
    use super::validate_worker_envelope;

    // 构造固定 dispatch 前拒绝。
    fn rejection() -> serde_json::Value {
        // 返回字段完整的候选 worker 响应。
        json!({
            // 标记业务失败。
            "ok": false,
            // 固定协议版本。
            "contractVersion": "act/interactive-command-worker/v1",
            // 回显固定请求 nonce。
            "requestNonce": "0123456789abcdef0123456789abcdef",
            // transport 已接受。
            "transportAccepted": true,
            // 业务未接受。
            "businessAccepted": false,
            // 未确定完成。
            "completed": false,
            // dispatch 前失败。
            "outcome": "not-dispatched",
            // 新请求可重试。
            "retrySafe": true,
            // 目标未改变。
            "targetMayHaveMutated": false,
            // 保存固定错误对象。
            "error": {
                // 使用候选路线不可用。
                "code": "ISOLATED_WORKER_UNAVAILABLE",
                // 提供非空安全消息。
                "message": "The route is not connected."
            },
            // 保存零副作用证据。
            "evidence": {
                // 未调用 provider。
                "localProviderInvoked": false,
                // 未解析目标。
                "targetResolved": false,
                // 未派发 mutation。
                "mutationDispatched": false,
                // 未回退当前桌面。
                "foregroundFallbackUsed": false
            }
        })
    }

    // 验证固定 dispatch 前拒绝通过完整状态机。
    #[test]
    fn worker_rejection_state_is_accepted() {
        // 合法响应必须通过。
        assert!(
            validate_worker_envelope(
                // 传入完整候选响应。
                &rejection(),
                // 绑定请求 nonce。
                "0123456789abcdef0123456789abcdef"
            )
            .is_ok()
        );
    }

    // 验证 nonce 替换和不安全状态组合失败闭合。
    #[test]
    fn worker_response_identity_and_state_cannot_drift() {
        // 构造基础响应。
        let mut value = rejection();
        // 替换为另一请求 nonce。
        value["requestNonce"] = json!("fedcba9876543210fedcba9876543210");
        // 关联值错配必须失败。
        assert!(
            validate_worker_envelope(
                // 传入被替换响应。
                &value,
                // 保持原期望 nonce。
                "0123456789abcdef0123456789abcdef"
            )
            .is_err()
        );

        // 恢复基础响应。
        let mut value = rejection();
        // 伪造业务已接受但仍可安全重试。
        value["businessAccepted"] = json!(true);
        // 不安全状态组合必须失败。
        assert!(
            validate_worker_envelope(
                // 传入漂移状态。
                &value,
                // 绑定正确 nonce。
                "0123456789abcdef0123456789abcdef"
            )
            .is_err()
        );
    }
}
