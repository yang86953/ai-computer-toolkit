//! 运行固定同会话长操作 broker 并拥有 registry 连接生命周期。

// 导入结构化启动错误写出、路径与时间工具。
use std::{
    // 写出唯一启动错误 JSON。
    io::Write,
    // 保存固定主程序 sibling 路径。
    path::Path,
    // 取得 Unix 毫秒与单调 handshake deadline。
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// 导入封闭响应序列化派生。
use serde::Serialize;
// 导入 provider-neutral JSON 值与构造宏。
use serde_json::{Value, json};

// 导入 broker 组合的 Adapter、Component 与 Module。
use crate::{
    // 导入固定本机 IPC 与 peer 认证。
    adapters::fixed_local_ipc_windows::{
        // 导入进程、session 与 fixed sibling 认证。
        identity::{
            authenticate_peer_process, current_session_id, process_session_id, sibling_image_path,
        },
        // 导入 local-only 固定 message pipe。
        pipe::{ConnectedPipe, FixedLocalEndpointKind, ServerPipe},
    },
    // 导入协议、存储、取消与时间格式化 Component。
    components::{
        // 响应进程级 Ctrl+C。
        cancellation,
        // 投影业务接受前的封闭错误码与消息。
        long_operation_error_projection,
        // 生成随机 canonical operation handle。
        long_operation_identity,
        // 解析封闭 broker Command/Query frame。
        long_operation_protocol::{
            CONTRACT_VERSION, LongOperationBrokerAction, LongOperationBrokerRequest,
            LongOperationProtocolErrorCode, LongOperationProtocolFailure,
        },
        // 打开固定 Known Folder journal。
        long_operation_storage_windows::{LongOperationStorageError, open_long_operation_journal},
        // 格式化公开终态到期时间。
        utc_timestamp::unix_milliseconds_to_rfc3339,
    },
    // 导入统一启动错误 envelope。
    domain::{AppControlError, AppResult, error_json},
    // 导入 registry 状态与幂等取消语义。
    modules::{
        // 导入取消分类与公开状态。
        long_operation::{CancelRequestEffect, LongOperationStatus},
        // 拥有异步录制任务、取消与线程回收。
        long_operation_recording::LongOperationRecordingTasks,
        // 导入有界 registry 与记录投影。
        long_operation_registry::{
            LongOperationFailure, LongOperationRecord, LongOperationRegistry,
            LongOperationRegistryError,
        },
    },
    // 使用 ComputerControlSystem 执行业务接受前门禁。
    service::AppControlService,
};

// 固定只允许主产品 CLI 连接 broker。
const MAIN_FILE_NAME: &str = "ai-computer-toolkit.exe";
// 限制每条连接的 frame 与最终响应交付等待。
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
// 限制极低概率 operation 指纹碰撞的生成尝试。
const OPERATION_ID_ATTEMPTS: usize = 4;

// 表示 broker response 中的稳定错误对象。
#[derive(Serialize)]
// 固定 camelCase 并保持字段封闭。
#[serde(rename_all = "camelCase")]
struct BrokerResponseError<'message> {
    // 保存稳定错误码。
    code: &'static str,
    // 保存固定安全消息。
    message: &'message str,
}

// 表示 transport 或业务接受前的结构化拒绝。
#[derive(Serialize)]
// 固定 camelCase 并省略 transport 前不可信 nonce。
#[serde(rename_all = "camelCase")]
struct BrokerRejection<'message> {
    // 保存固定 broker 协议版本。
    contract_version: &'static str,
    // 只在 transport 接受后回显 canonical nonce。
    #[serde(skip_serializing_if = "Option::is_none")]
    request_nonce: Option<&'message str>,
    // 标记固定 envelope 是否接受。
    transport_accepted: bool,
    // 拒绝响应永不声称业务接受。
    business_accepted: bool,
    // 保存封闭失败对象。
    error: BrokerResponseError<'message>,
}

// 表示公开长操作状态中的稳定错误对象。
#[derive(Serialize)]
// 固定 camelCase 字段。
#[serde(rename_all = "camelCase")]
struct OperationResponseError<'record> {
    // 借用稳定错误码。
    code: &'record str,
    // 借用有界错误消息。
    message: &'record str,
}

// 表示公开 long-operation-status schema 的精确投影。
#[derive(Serialize)]
// 固定 camelCase 并省略互斥 result/error。
#[serde(rename_all = "camelCase")]
struct OperationStatusResponse<'record> {
    // 保存公开状态契约版本。
    contract_version: &'static str,
    // 借用 canonical operation handle。
    operation_id: &'record str,
    // 借用封闭 capability ID。
    capability_id: &'record str,
    // 保存六态生命周期状态。
    status: LongOperationStatus,
    // 保存不可逆 dispatch 事实。
    dispatch_started: bool,
    // 标记状态是否不可覆盖。
    terminal: bool,
    // 已建立 handle 固定表示业务接受可能发生。
    accepted_may_have_occurred: bool,
    // 保存保守重试安全事实。
    retry_safe: bool,
    // 保存取消是否曾被持久接受。
    cancel_requested: bool,
    // 非终态为 null，终态为固定 UTC 时间。
    expires_at: Option<String>,
    // completed 才携带完整有界结果。
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<&'record Value>,
    // failed 或 outcome-unknown 才携带错误。
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<OperationResponseError<'record>>,
}

// 表示业务已接受的 broker 成功响应。
#[derive(Serialize)]
// 固定 camelCase 字段。
#[serde(rename_all = "camelCase")]
struct BrokerSuccess<'record> {
    // 保存固定 broker 协议版本。
    contract_version: &'static str,
    // 回显已经验证的请求 nonce。
    request_nonce: &'record str,
    // 成功响应固定已经接受 transport。
    transport_accepted: bool,
    // 成功响应固定已经接受 Query/Command。
    business_accepted: bool,
    // 保存当前 operation 快照。
    operation: OperationStatusResponse<'record>,
}

// 构造不泄漏 pipe、路径、PID、SID 或 RID 的 broker 错误。
fn broker_error(code: &'static str, message: &'static str) -> AppControlError {
    // 复用统一稳定错误 envelope。
    AppControlError::new(code, message)
}

// 构造不会继承旧连接预算的 handshake deadline。
fn handshake_deadline() -> Instant {
    // 固定五秒远小于单调时钟上限。
    Instant::now() + HANDSHAKE_TIMEOUT
}

// 取得当前 Unix 毫秒供 registry 修订与到期使用。
fn current_unix_milliseconds() -> AppResult<u64> {
    // 读取不会受调用方 frame 控制的系统时钟。
    let duration = SystemTime::now()
        // 计算 Unix epoch 后时长。
        .duration_since(UNIX_EPOCH)
        // 时钟早于 epoch 时 broker 失败闭合。
        .map_err(|_| {
            broker_error(
                // 使用稳定 broker 不可用码。
                "BROKER_UNAVAILABLE",
                // 不公开系统时钟细节。
                "The long operation broker clock is unavailable.",
            )
        })?;
    // 将毫秒转换为 u64 并拒绝理论溢出。
    u64::try_from(duration.as_millis()).map_err(|_| {
        // 返回封闭 broker 错误。
        broker_error(
            // 使用稳定 broker 不可用码。
            "BROKER_UNAVAILABLE",
            // 不公开平台时钟数值。
            "The long operation broker clock exceeded its fixed boundary.",
        )
    })
}

// 把存储失败映射为统一 broker 启动错误。
fn storage_error(error: LongOperationStorageError) -> AppControlError {
    // 保留失败类别仅供静态穷举。
    match error {
        // Known Folder 不可用。
        LongOperationStorageError::KnownFolderUnavailable
        // 固定目录无法建立。
        | LongOperationStorageError::DirectoryUnavailable
        // 目录边界不可信。
        | LongOperationStorageError::DirectoryUntrusted
        // journal 无法打开。
        | LongOperationStorageError::Journal(_) => broker_error(
            // 使用 broker 级不可用错误。
            "BROKER_UNAVAILABLE",
            // 不泄漏存储路径或底层 I/O。
            "The long operation broker journal is unavailable.",
        ),
    }
}

// 把协议错误码映射为 schema 已冻结文本。
fn protocol_code(code: LongOperationProtocolErrorCode) -> &'static str {
    // 复用封闭协议枚举投影。
    code.as_str()
}

// 构造 transport 或语义解析拒绝。
fn protocol_rejection(failure: &LongOperationProtocolFailure) -> BrokerRejection<'_> {
    // 选择固定安全消息。
    let message = match failure.code() {
        // 参数拒绝不回显字段或值。
        LongOperationProtocolErrorCode::InvalidArgument => {
            // 返回固定参数消息。
            "The long operation broker request is invalid."
        }
        // 确认拒绝保持独立语义。
        LongOperationProtocolErrorCode::ConfirmationRequired => {
            // 返回固定确认消息。
            "The long operation submission requires explicit confirmation."
        }
    };
    // 构造 schema 对齐拒绝。
    BrokerRejection {
        // 使用固定版本。
        contract_version: CONTRACT_VERSION,
        // transport 前为 None，接受后为 canonical nonce。
        request_nonce: failure.request_nonce(),
        // 复制 transport 接受事实。
        transport_accepted: failure.transport_accepted(),
        // parser 拒绝永不建立业务事实。
        business_accepted: false,
        // 保存封闭错误。
        error: BrokerResponseError {
            // 投影稳定错误码。
            code: protocol_code(failure.code()),
            // 使用固定安全消息。
            message,
        },
    }
}

// 构造 transport 已接受但业务未接受的拒绝。
fn business_rejection<'nonce>(
    // 借用已经验证的请求 nonce。
    request_nonce: &'nonce str,
    // 接收 schema 已冻结错误码。
    code: &'static str,
    // 接收固定安全消息。
    message: &'static str,
) -> BrokerRejection<'nonce> {
    // 构造 pre-business response。
    BrokerRejection {
        // 使用固定版本。
        contract_version: CONTRACT_VERSION,
        // 回显 canonical nonce。
        request_nonce: Some(request_nonce),
        // envelope 已接受。
        transport_accepted: true,
        // 业务未接受。
        business_accepted: false,
        // 保存固定错误。
        error: BrokerResponseError { code, message },
    }
}

// 将 registry 记录投影为公开状态。
fn operation_response(
    // 借用有界记录快照。
    record: &LongOperationRecord,
) -> AppResult<OperationStatusResponse<'_>> {
    // 终态必须具有固定到期时间，非终态固定为 null。
    let expires_at = match record.expires_at_ms() {
        // 转换终态 Unix 毫秒。
        Some(milliseconds) => Some(
            // 生成 UTC RFC 3339。
            unix_milliseconds_to_rfc3339(milliseconds)
                // 超出四位年份时失败闭合。
                .ok_or_else(|| {
                    broker_error(
                        // 使用 broker 不可用错误。
                        "BROKER_UNAVAILABLE",
                        // 不公开原始时间值。
                        "The long operation expiry could not be represented safely.",
                    )
                })?,
        ),
        // 非终态没有固定保留截止时间。
        None => None,
    };
    // 投影可选稳定错误。
    let error = record
        // 读取可选失败。
        .error()
        // 借用固定字段。
        .map(operation_error_response);
    // 构造公开状态对象。
    Ok(OperationStatusResponse {
        // 使用公开状态契约版本。
        contract_version: "act/long-operation/v1",
        // 借用 canonical handle。
        operation_id: record.operation_id(),
        // 借用封闭 capability。
        capability_id: record.capability_id(),
        // 投影状态。
        status: record.status(),
        // 投影 dispatch 事实。
        dispatch_started: record.dispatch_started(),
        // 投影终态分类。
        terminal: record.terminal(),
        // 已有记录固定可能被业务接受。
        accepted_may_have_occurred: true,
        // 投影重试安全事实。
        retry_safe: record.retry_safe(),
        // 投影独立取消事实。
        cancel_requested: record.cancel_requested(),
        // 保存可选固定到期文本。
        expires_at,
        // 借用可选完整结果。
        result: record.result(),
        // 保存可选错误投影。
        error,
    })
}

// 投影稳定 operation 失败对象。
fn operation_error_response(failure: &LongOperationFailure) -> OperationResponseError<'_> {
    // 只借用已经验证的安全字段。
    OperationResponseError {
        // 借用错误码。
        code: failure.code(),
        // 借用错误消息。
        message: failure.message(),
    }
}

// 构造业务已接受的成功响应。
fn success_response<'record>(
    // 借用 canonical request nonce。
    request_nonce: &'record str,
    // 借用 operation 记录。
    record: &'record LongOperationRecord,
) -> AppResult<BrokerSuccess<'record>> {
    // 构造字段封闭成功对象。
    Ok(BrokerSuccess {
        // 使用固定 broker 版本。
        contract_version: CONTRACT_VERSION,
        // 回显 canonical nonce。
        request_nonce,
        // 成功固定接受 transport。
        transport_accepted: true,
        // 成功固定接受 Query/Command。
        business_accepted: true,
        // 投影 operation 状态。
        operation: operation_response(record)?,
    })
}

// 映射 registry 业务失败为 broker schema 错误。
fn registry_rejection<'nonce>(
    // 借用 canonical nonce。
    request_nonce: &'nonce str,
    // 接收封闭 registry 失败。
    error: LongOperationRegistryError,
) -> BrokerRejection<'nonce> {
    // 按是否可安全公开分类。
    match error {
        // canonical handle 已由协议验证，零命中统一 not found。
        LongOperationRegistryError::OperationNotFound
        // 防御内部 handle 分类漂移。
        | LongOperationRegistryError::InvalidOperation => business_rejection(
            // 回显 canonical nonce。
            request_nonce,
            // 使用 broker schema 错误码。
            "OPERATION_NOT_FOUND",
            // 不区分过期、代际或零命中。
            "The long operation is unavailable in this broker generation.",
        ),
        // 容量失败发生在业务接受前。
        LongOperationRegistryError::CapacityExhausted => business_rejection(
            // 回显 canonical nonce。
            request_nonce,
            // 使用固定容量错误码。
            "OPERATION_CAPACITY_EXHAUSTED",
            // 不公开实际计数。
            "The long operation broker capacity is exhausted.",
        ),
        // 其他错误表示 broker 内部事实无法安全继续。
        LongOperationRegistryError::Journal(_)
        // 未知 capability 不应到达 status/cancel。
        | LongOperationRegistryError::InvalidCapability
        // 损坏记录已在启动期拒绝。
        | LongOperationRegistryError::InvalidRecord
        // 重复 operation 只属于未来 submit。
        | LongOperationRegistryError::DuplicateOperation
        // 时钟回退不能覆盖事实。
        | LongOperationRegistryError::ClockRegression
        // 非法迁移不能伪造结果。
        | LongOperationRegistryError::InvalidTransition
        // 非法失败对象不能公开。
        | LongOperationRegistryError::InvalidFailure => business_rejection(
            // 回显 canonical nonce。
            request_nonce,
            // 使用 broker 级不可用错误。
            "BROKER_UNAVAILABLE",
            // 不泄漏内部状态。
            "The long operation broker could not establish a reliable result.",
        ),
    }
}

// 把业务接受前的 System 失败投影为 broker 封闭拒绝。
fn system_rejection<'nonce>(
    // 借用 canonical request nonce。
    request_nonce: &'nonce str,
    // 借用统一 System 错误。
    error: &AppControlError,
) -> BrokerRejection<'nonce> {
    // 由窄 Component 拥有 schema 映射。
    let (code, message) = long_operation_error_projection::pre_acceptance(error);
    // 构造 transport 已接受但业务未接受的响应。
    business_rejection(request_nonce, code, message)
}

// 处理一条已经通过 OS peer 认证的 Command 或 Query。
fn handle_request(
    // 独占 broker 代际任务与 registry 状态。
    tasks: &mut LongOperationRecordingTasks,
    // 借用 ComputerControlSystem 预检边界。
    service: &AppControlService,
    // 借用严格解析请求。
    request: &LongOperationBrokerRequest,
    // 接收当前 broker 时刻。
    now_ms: u64,
) -> AppResult<Value> {
    // 任一异步持久迁移失败后拒绝继续提供不可靠状态。
    if !tasks.healthy() {
        // 要求 broker 进程失败闭合并由下一代恢复 journal。
        return Err(broker_error(
            // 使用稳定 broker 生命周期错误码。
            "BROKER_UNAVAILABLE",
            // 不公开失败任务或 journal 事实。
            "The long operation broker state is unavailable.",
        ));
    }
    // 按封闭 action 选择领域入口。
    match request.action() {
        // submit 在完整预检后原子接受并异步 dispatch。
        LongOperationBrokerAction::Submit => {
            // 协议已保证 target 是单字段对象。
            let session_id = request
                // 借用 submit target。
                .target()
                // 读取 target 对象。
                .and_then(Value::as_object)
                // 读取唯一 sessionId。
                .and_then(|target| target.get("sessionId"))
                // 只接受 canonical 字符串。
                .and_then(Value::as_str)
                // 防御协议与 broker 漂移。
                .ok_or_else(|| {
                    broker_error(
                        // 使用 broker 内部失败。
                        "BROKER_UNAVAILABLE",
                        // 不回显 target。
                        "The broker submission target is unavailable.",
                    )
                })?;
            // 协议已保证 input 是对象。
            let input = request.input().ok_or_else(|| {
                // 防御协议与 broker 漂移。
                broker_error(
                    // 使用 broker 内部失败。
                    "BROKER_UNAVAILABLE",
                    // 不回显 input。
                    "The broker submission input is unavailable.",
                )
            })?;
            // 在业务接受前执行 Policy、权限与纯领域验证。
            if let Err(error) = service.validate_long_operation_window_record(session_id, input) {
                // 序列化封闭 System 拒绝。
                return serde_json::to_value(system_rejection(request.request_nonce(), &error))
                    // 序列化理论失败映射 broker 错误。
                    .map_err(|_| {
                        broker_error(
                            // 使用 broker 生命周期错误码。
                            "BROKER_UNAVAILABLE",
                            // 不回显请求内容。
                            "The broker rejection could not be serialized.",
                        )
                    });
            }
            // 尝试生成并原子接受唯一 operation handle。
            for _ in 0..OPERATION_ID_ATTEMPTS {
                // 每次碰撞都使用全新随机身份。
                let operation_id = long_operation_identity::new_operation_id()?;
                // 接受后立即启动 broker-owned Rust worker。
                match tasks.submit(
                    // 传入 canonical operation handle。
                    &operation_id,
                    // worker 拥有目标文本。
                    session_id.to_owned(),
                    // worker 拥有完整有界输入。
                    input.clone(),
                    // 冻结业务接受时刻。
                    now_ms,
                ) {
                    // 成功包括 accepted 或线程创建失败后的可靠 failed。
                    Ok(record) => {
                        // 返回业务已接受 operation 快照。
                        return serde_json::to_value(success_response(
                            // 绑定请求 nonce。
                            request.request_nonce(),
                            // 投影持久记录。
                            &record,
                        )?)
                        // 序列化理论失败映射 broker 错误。
                        .map_err(|_| {
                            broker_error(
                                // 使用 broker 生命周期错误码。
                                "BROKER_UNAVAILABLE",
                                // 不回显 operation 或输入。
                                "The broker submission result could not be serialized.",
                            )
                        });
                    }
                    // 极低概率指纹碰撞只重生 handle，不重放业务行为。
                    Err(LongOperationRegistryError::DuplicateOperation) => continue,
                    // 其他失败发生在业务接受前。
                    Err(error) => {
                        // 返回 registry 封闭拒绝。
                        return serde_json::to_value(registry_rejection(
                            // 绑定请求 nonce。
                            request.request_nonce(),
                            // 投影失败类别。
                            error,
                        ))
                        // 序列化理论失败映射 broker 错误。
                        .map_err(|_| {
                            broker_error(
                                // 使用 broker 生命周期错误码。
                                "BROKER_UNAVAILABLE",
                                // 不回显 registry 事实。
                                "The broker rejection could not be serialized.",
                            )
                        });
                    }
                }
            }
            // 多次随机指纹碰撞表示身份生成无法可靠继续。
            Err(broker_error(
                // 使用 broker 生命周期错误码。
                "BROKER_UNAVAILABLE",
                // 不公开碰撞 handle。
                "The long operation broker could not allocate a unique operation handle.",
            ))
        }
        // status 是无副作用 Query。
        LongOperationBrokerAction::Status => {
            // 协议已经保证 operation ID 存在。
            let operation_id = request.operation_id().ok_or_else(|| {
                // 防御协议与 broker 漂移。
                broker_error(
                    "BROKER_UNAVAILABLE",
                    "The broker request identity is unavailable.",
                )
            })?;
            // 查询并撤销已到期记录。
            match tasks.status(operation_id, now_ms) {
                // 返回业务已接受状态。
                Ok(record) => {
                    serde_json::to_value(success_response(request.request_nonce(), &record)?)
                        // 序列化理论失败映射 broker 错误。
                        .map_err(|_| {
                            broker_error(
                                "BROKER_UNAVAILABLE",
                                "The broker status could not be serialized.",
                            )
                        })
                }
                // 返回业务接受前拒绝。
                Err(error) => {
                    serde_json::to_value(registry_rejection(request.request_nonce(), error))
                        // 序列化理论失败映射 broker 错误。
                        .map_err(|_| {
                            broker_error(
                                "BROKER_UNAVAILABLE",
                                "The broker rejection could not be serialized.",
                            )
                        })
                }
            }
        }
        // cancel 是幂等 Command。
        LongOperationBrokerAction::Cancel => {
            // 协议已经保证 operation ID 存在。
            let operation_id = request.operation_id().ok_or_else(|| {
                // 防御协议与 broker 漂移。
                broker_error(
                    "BROKER_UNAVAILABLE",
                    "The broker request identity is unavailable.",
                )
            })?;
            // 原子持久取消事实或返回既有终态。
            match tasks.cancel(operation_id, now_ms) {
                // 首次、重复与终态取消都返回当前真实记录。
                Ok((
                    // 显式穷举三种幂等效果。
                    CancelRequestEffect::Requested
                    | CancelRequestEffect::AlreadyRequested
                    | CancelRequestEffect::AlreadyTerminal,
                    // 保存当前记录。
                    record,
                )) => serde_json::to_value(success_response(request.request_nonce(), &record)?)
                    // 序列化理论失败映射 broker 错误。
                    .map_err(|_| {
                        broker_error(
                            "BROKER_UNAVAILABLE",
                            "The broker cancel result could not be serialized.",
                        )
                    }),
                // 返回业务接受前拒绝。
                Err(error) => {
                    serde_json::to_value(registry_rejection(request.request_nonce(), error))
                        // 序列化理论失败映射 broker 错误。
                        .map_err(|_| {
                            broker_error(
                                "BROKER_UNAVAILABLE",
                                "The broker rejection could not be serialized.",
                            )
                        })
                }
            }
        }
    }
}

// 写出最终响应并有界等待 client 读取后关闭连接。
fn write_final_response(pipe: &ConnectedPipe, response: &impl Serialize) -> AppResult<()> {
    // 单帧写入已经受 256 KiB pipe 边界约束。
    pipe.write_json(response)?;
    // 最终响应后只等待 client 关闭确认读取。
    match pipe.read_text_until(
        // 防止 peer 无限占用唯一 broker instance。
        handshake_deadline(),
        // 响应 broker 进程取消。
        cancellation::is_cancelled,
    ) {
        // client 读完并关闭是正常交付完成。
        Err(error) if error.code == "ISOLATED_WORKER_UNAVAILABLE" => Ok(()),
        // 其他 timeout/cancel 只关闭本连接。
        Err(error) => Err(error),
        // 最终响应后额外 frame 违反单帧协议。
        Ok(_) => Err(broker_error(
            // 使用稳定参数错误。
            "INVALID_ARGUMENT",
            // 不回显额外 frame。
            "The long operation broker received an unexpected extra frame.",
        )),
    }
}

// 处理一个经过内核连接的固定同会话 client。
fn serve_connection(
    // 转移连接所有权。
    pipe: ConnectedPipe,
    // 传递 broker native session 私有事实。
    broker_session_id: u32,
    // 借用固定主程序镜像。
    main_image: &Path,
    // 独占 broker 代际任务所有权。
    tasks: &mut LongOperationRecordingTasks,
    // 借用业务接受前的 ComputerControlSystem 门禁。
    service: &AppControlService,
) -> AppResult<()> {
    // 取得内核记录的 client PID。
    let peer_process_id = pipe.peer_process_id()?;
    // 取得 client 所属 native session。
    let peer_session_id = process_session_id(peer_process_id)?;
    // 长操作 broker 只接受同一登录会话 peer。
    if peer_session_id != broker_session_id {
        // 不公开任一 session 数值。
        return Err(broker_error(
            // 使用统一认证错误码。
            "ENDPOINT_AUTHENTICATION_FAILED",
            // 明确同会话边界。
            "The long operation broker peer is not in the broker session.",
        ));
    }
    // 核对固定主程序镜像、同 session、SID 与完整性 RID。
    authenticate_peer_process(peer_process_id, main_image, broker_session_id)?;
    // 读取唯一有界 JSON frame。
    let text = pipe.read_text_until(
        // 每连接使用独立固定 deadline。
        handshake_deadline(),
        // 响应 broker 取消。
        cancellation::is_cancelled,
    )?;
    // 严格解析完整 frame。
    let request = match LongOperationBrokerRequest::parse(&text) {
        // 保存已经验证的请求。
        Ok(request) => request,
        // 协议拒绝不触碰 registry。
        Err(failure) => {
            // 构造 acceptance-aware 拒绝。
            let response = protocol_rejection(&failure);
            // 写出唯一响应。
            return write_final_response(&pipe, &response);
        }
    };
    // 取得本次状态迁移时刻。
    let now_ms = current_unix_milliseconds()?;
    // 协调任务 Module 与 System 门禁处理 Command/Query。
    let response = handle_request(tasks, service, &request, now_ms)?;
    // 写出唯一响应并等待 client 关闭。
    write_final_response(&pipe, &response)
}

// 运行固定同会话 broker 循环。
fn run_server() -> AppResult<()> {
    // 取得 broker 当前 native session。
    let broker_session_id = current_session_id()?;
    // 先取得首实例 pipe，防止第二 broker 并发恢复 journal。
    let mut server = ServerPipe::create_for(
        // 固定选择长操作 endpoint。
        FixedLocalEndpointKind::LongOperation,
        // 使用当前 broker session。
        broker_session_id,
    )?;
    // 定位固定主产品 CLI 镜像供 peer 认证。
    let main_image = sibling_image_path(MAIN_FILE_NAME)?;
    // 首实例所有权建立后再打开 Known Folder journal。
    let journal = open_long_operation_journal().map_err(storage_error)?;
    // 取得恢复时刻。
    let now_ms = current_unix_milliseconds()?;
    // broker 唯一打开 registry 并在接收前完成恢复。
    let registry = LongOperationRegistry::open(journal, now_ms).map_err(|_| {
        // 不公开 journal 内容或路径。
        broker_error(
            // 使用 broker 不可用错误。
            "BROKER_UNAVAILABLE",
            // 明确恢复无法可靠完成。
            "The long operation broker registry could not be recovered safely.",
        )
    })?;
    // 由异步录制 Module 接管 registry 与 worker 生命周期。
    let mut tasks = LongOperationRecordingTasks::new(registry);
    // 构造业务接受前使用的 ComputerControlSystem 门禁。
    let service = AppControlService::new();
    // 持续服务串行单帧连接。
    loop {
        // 进程取消在新连接前停止接收。
        if cancellation::is_cancelled() {
            // 先传播取消并回收全部 broker-owned worker。
            tasks.shutdown().map_err(|_| {
                // 关闭持久化失败要求下代恢复。
                broker_error(
                    // 使用稳定 broker 生命周期错误码。
                    "BROKER_UNAVAILABLE",
                    // 不公开失败任务或 journal 事实。
                    "The long operation broker could not persist shutdown safely.",
                )
            })?;
            // registry 已逐迁移 durable 后正常退出。
            return Ok(());
        }
        // 任一 worker 持久迁移失败后停止接受新连接。
        if !tasks.healthy() {
            // 传播取消并回收本代际 worker。
            let _ = tasks.shutdown();
            // 由下一个 broker 代际执行保守 journal 恢复。
            return Err(broker_error(
                // 使用稳定 broker 生命周期错误码。
                "BROKER_UNAVAILABLE",
                // 不公开失败任务或 journal 事实。
                "The long operation broker state became unavailable.",
            ));
        }
        // 等待一个本机 client，同时保持空闲 broker 可取消。
        let pipe = match server.accept(|| {
            // 进程取消或持久状态失败都中断空闲 accept。
            cancellation::is_cancelled() || !tasks.healthy()
        }) {
            // 保存已连接 pipe。
            Ok(pipe) => pipe,
            // Ctrl+C 在无 client 时属于正常关闭。
            Err(error) if error.code == "CANCELLED" => {
                // 在回收前保存失败闭合分类。
                let healthy = tasks.healthy();
                // 空闲取消仍必须回收活动 worker。
                let shutdown = tasks.shutdown();
                // 正常进程取消与持久迁移失败必须保持不同退出语义。
                return if healthy {
                    // 正常关闭仍要求取消意图可靠持久化。
                    shutdown.map_err(|_| {
                        // 映射为稳定 broker 生命周期错误。
                        broker_error(
                            // 使用稳定 broker 生命周期错误码。
                            "BROKER_UNAVAILABLE",
                            // 不公开失败任务或 journal 事实。
                            "The long operation broker could not persist shutdown safely.",
                        )
                    })
                } else {
                    // 持久状态失败要求下代恢复。
                    Err(broker_error(
                        // 使用稳定 broker 生命周期错误码。
                        "BROKER_UNAVAILABLE",
                        // 不公开失败任务或 journal 事实。
                        "The long operation broker state became unavailable.",
                    ))
                };
            }
            // 其他 endpoint 错误终止 broker 启动。
            Err(error) => return Err(error),
        };
        // 单个未认证、断连或超时 client 只关闭自己的连接。
        let _ = serve_connection(
            // 转移连接。
            pipe,
            // 传递同 session 认证事实。
            broker_session_id,
            // 传递固定主程序镜像。
            &main_image,
            // broker 保持任务 Module 唯一所有权。
            &mut tasks,
            // 复用同一个 System 预检边界。
            &service,
        );
        // 当前连接释放后重新取得同名首实例。
        server = ServerPipe::create_for(
            // 固定长操作 endpoint。
            FixedLocalEndpointKind::LongOperation,
            // 使用同一 native session。
            broker_session_id,
        )?;
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
                    // 使用稳定 broker 错误对象。
                    "error": {
                        // 固定 broker 错误码。
                        "code": "BROKER_UNAVAILABLE",
                        // 固定安全消息。
                        "message": "The long operation broker could not serialize its startup error."
                    }
                })
                // JSON 宏生成对象可稳定转文本。
                .to_string()
            });
            // 只向 stdout 写一条 JSON 诊断。
            let _ = writeln!(std::io::stdout(), "{text}");
            // 返回结构化失败退出码。
            2
        }
    }
}

// 声明 broker 响应与 registry 路由回归测试。
#[cfg(test)]
// 将 fixture 放入独立文件控制 broker 规模。
#[path = "long_operation_broker_tests.rs"]
mod tests;
