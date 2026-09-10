//! 拥有独立交互会话发现、精确 endpoint 选择与单次 Command 生命周期。

// 导入 JSON 对象和值构造。
use serde_json::{Map, Value, json};

// 导入 host 侧认证 endpoint client 与统一请求。
use crate::{
    // 导入认证 endpoint 发现和执行端口。
    adapters::interactive_session_windows::client::{
        CertifiedInteractiveEndpoint, discover_certified_endpoints, execute_certified_command,
    },
    // 导入四条通用 capability 常量。
    capabilities,
    // 复用 command 协议拥有的封闭 capability 路由。
    components::interactive_command_protocol::capability_route,
    // 导入统一错误、请求与隔离要求。
    domain::{AppControlError, AppResult, CommandRequest, IsolationRequirement},
};

// 固定四条独立会话 mutation 的默认总 deadline。
const DEFAULT_TIMEOUT_MS: u32 = 2_000;
// 固定协议允许的最大总 deadline。
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

// 构造 provider-neutral 隔离 Module 错误。
fn isolation_error(code: &'static str, message: &'static str, details: Value) -> AppControlError {
    // 复用统一错误 envelope。
    AppControlError::with_details(code, message, details)
}

// 将 worker 输出限制到公开错误 envelope 已登记的静态错误码。
fn stable_worker_error_code(code: &str) -> &'static str {
    // 只允许 command worker v1 schema 冻结的错误码穿过边界。
    match code {
        // 保留统一参数错误。
        "INVALID_ARGUMENT" => "INVALID_ARGUMENT",
        // 保留逐操作确认缺失。
        "CONFIRMATION_REQUIRED" => "CONFIRMATION_REQUIRED",
        // 保留独立会话前景许可缺失。
        "FOREGROUND_CONSENT_REQUIRED" => "FOREGROUND_CONSENT_REQUIRED",
        // 保留严格隔离要求缺失。
        "ISOLATION_REQUIRED" => "ISOLATION_REQUIRED",
        // 保留 capability 缺口。
        "CAPABILITY_GAP" => "CAPABILITY_GAP",
        // 保留当前目标状态不支持。
        "CAPABILITY_UNSUPPORTED" => "CAPABILITY_UNSUPPORTED",
        // 保留后台关闭不可用。
        "BACKGROUND_OPERATION_UNAVAILABLE" => "BACKGROUND_OPERATION_UNAVAILABLE",
        // 保留固定 worker 不可用。
        "ISOLATED_WORKER_UNAVAILABLE" => "ISOLATED_WORKER_UNAVAILABLE",
        // 保留 endpoint 认证失败。
        "ENDPOINT_AUTHENTICATION_FAILED" => "ENDPOINT_AUTHENTICATION_FAILED",
        // 保留 stale opaque 目标。
        "STALE_SESSION" => "STALE_SESSION",
        // 保留歧义目标失败。
        "AMBIGUOUS_TARGET" => "AMBIGUOUS_TARGET",
        // 保留静态权限拒绝。
        "PERMISSION_DENIED" => "PERMISSION_DENIED",
        // 保留权限证据缺口。
        "CAPABILITY_ASSESSMENT_UNAVAILABLE" => "CAPABILITY_ASSESSMENT_UNAVAILABLE",
        // 保留写前取消。
        "CANCELLED" => "CANCELLED",
        // 保留前景激活失败。
        "FOREGROUND_ACTIVATION_FAILED" => "FOREGROUND_ACTIVATION_FAILED",
        // 保留宿主前景干扰。
        "HOST_INTERFERENCE_DETECTED" => "HOST_INTERFERENCE_DETECTED",
        // 保留写前或领域 timeout。
        "TIMEOUT" => "TIMEOUT",
        // 保留指针精确命中失败。
        "POINTER_TARGET_NOT_HIT" => "POINTER_TARGET_NOT_HIT",
        // 保留物理坐标上下文缺口。
        "COORDINATE_CONTEXT_UNAVAILABLE" => "COORDINATE_CONTEXT_UNAVAILABLE",
        // 保留通用下层操作失败。
        "OPERATION_FAILED" => "OPERATION_FAILED",
        // 保留业务接受后的未知结果。
        "OUTCOME_UNKNOWN" => "OUTCOME_UNKNOWN",
        // 保留 command worker 自身序列化失败。
        "WORKER_PROTOCOL_ERROR" => "WORKER_PROTOCOL_ERROR",
        // 未登记值一律收敛为 host 侧协议失败。
        _ => "WORKER_PROTOCOL_FAILED",
    }
}

// 判断 capability 是否属于独立会话 v1 封闭路线。
#[cfg(test)]
pub(crate) fn supports_capability(capability: &str) -> bool {
    // 只接受 command 协议已经冻结的 provider-neutral mutation。
    capability_route(capability).is_some()
}

// 返回 capability 对应的唯一 generic operation。
fn expected_operation(capability: &str) -> Option<&'static str> {
    // 从同一 command 协议路由读取唯一 operation。
    capability_route(capability).map(|(operation, _)| operation)
}

// 从 capability 输入中提取并移除唯一总 deadline。
fn command_input_and_timeout(request: &CommandRequest) -> AppResult<(Value, u32)> {
    // 取得 capability 自有 input 对象。
    let mut input = request
        // 读取统一 wrapper 的 input 字段。
        .args
        // 取得固定字段。
        .get("input")
        // 要求对象。
        .and_then(Value::as_object)
        // 克隆为 worker command 独立所有权。
        .cloned()
        // 缺失或类型错误失败闭合。
        .ok_or_else(|| {
            // 返回稳定参数错误。
            isolation_error(
                // 使用固定参数错误码。
                "INVALID_ARGUMENT",
                // 不回显 input。
                "The isolated capability input must be an object.",
                // 证明没有本地 provider 或 fallback。
                json!({
                    // 本地 provider 未调用。
                    "localProviderInvoked": false,
                    // 当前桌面未接管。
                    "foregroundFallbackUsed": false
                }),
            )
        })?;
    // 从领域 input 移除 timeout，完整 worker 生命周期只保留顶层一份。
    let timeout = input
        // 移除可选 timeoutMs。
        .remove("timeoutMs")
        // 映射 JSON 值为 u64。
        .map(|value| {
            // 只接受整数。
            value.as_u64().ok_or_else(|| {
                // 返回稳定参数错误。
                isolation_error(
                    // 使用固定参数错误码。
                    "INVALID_ARGUMENT",
                    // 不回显异常值。
                    "The isolated capability timeout must be an integer.",
                    // 固定零副作用证据。
                    json!({
                        // 本地 provider 未调用。
                        "localProviderInvoked": false,
                        // 当前桌面未接管。
                        "foregroundFallbackUsed": false
                    }),
                )
            })
        })
        // 展平可选解析结果。
        .transpose()?
        // 缺失时使用四条路线共享的既有默认值。
        .unwrap_or(u64::from(DEFAULT_TIMEOUT_MS));
    // 收窄并核对协议允许范围。
    let timeout = u32::try_from(timeout)
        // 转换溢出失败闭合。
        .ok()
        // 只接受 1..30000。
        .filter(|value| (1..=MAXIMUM_TIMEOUT_MS).contains(value))
        // 映射稳定参数错误。
        .ok_or_else(|| {
            // 返回不泄漏原始值的错误。
            isolation_error(
                // 使用固定参数错误码。
                "INVALID_ARGUMENT",
                // 说明稳定范围。
                "The isolated capability timeout must be within 1..30000 milliseconds.",
                // 固定零副作用证据。
                json!({
                    // 本地 provider 未调用。
                    "localProviderInvoked": false,
                    // 当前桌面未接管。
                    "foregroundFallbackUsed": false
                }),
            )
        })?;
    // 返回移除第二 deadline 的领域输入与总预算。
    Ok((Value::Object(input), timeout))
}

// 从认证清单中精确选择公开 s2:i。
fn select_endpoint(
    endpoints: &[CertifiedInteractiveEndpoint],
    interactive_session_id: &str,
) -> AppResult<CertifiedInteractiveEndpoint> {
    // 收集公开身份完全匹配的 endpoint。
    let matches = endpoints
        // 遍历认证清单。
        .iter()
        // 只保留精确 opaque ID。
        .filter(|endpoint| endpoint.interactive_session_id() == interactive_session_id)
        // 克隆窄 endpoint 句柄。
        .cloned()
        // 收集以区分歧义。
        .collect::<Vec<_>>();
    // 按命中数量分类。
    match matches.as_slice() {
        // 唯一命中返回当前认证 endpoint。
        [endpoint] => Ok(endpoint.clone()),
        // 零命中按清单是否为空区分 runtime 缺失与 stale。
        [] if endpoints.is_empty() => Err(isolation_error(
            // 无任何授权 endpoint 使用结构化不可用。
            "ISOLATED_WORKER_UNAVAILABLE",
            // 不暗示当前桌面 fallback。
            "No certified independent interactive session endpoint is currently available.",
            // 固定零副作用证据。
            json!({
                // 本地 provider 未调用。
                "localProviderInvoked": false,
                // 当前桌面未接管。
                "foregroundFallbackUsed": false,
                // 请求可在重新发现后安全重试。
                "retrySafe": true,
                // 目标未改变。
                "targetMayHaveMutated": false
            }),
        )),
        // 非空清单中的零命中表示授权代际已过期。
        [] => Err(isolation_error(
            // 使用精确 stale 语义。
            "STALE_SESSION",
            // 不公开其他 endpoint。
            "The independent interactive session authorization is stale.",
            // 只回显调用方已知 opaque ID。
            json!({
                // 回显公开目标。
                "interactiveSessionId": interactive_session_id,
                // 本地 provider 未调用。
                "localProviderInvoked": false,
                // 当前桌面未接管。
                "foregroundFallbackUsed": false,
                // 重新发现后可以安全重试。
                "retrySafe": true,
                // 目标未改变。
                "targetMayHaveMutated": false
            }),
        )),
        // 指纹碰撞或重复 endpoint 必须失败闭合。
        _ => Err(isolation_error(
            // 使用精确歧义错误码。
            "AMBIGUOUS_TARGET",
            // 不任取一个 endpoint。
            "More than one certified endpoint matched the opaque interactive session.",
            // 只回显调用方已知 opaque ID。
            json!({
                // 回显公开目标。
                "interactiveSessionId": interactive_session_id,
                // 本地 provider 未调用。
                "localProviderInvoked": false,
                // 当前桌面未接管。
                "foregroundFallbackUsed": false,
                // 歧义消失后可安全重试。
                "retrySafe": true,
                // 目标未改变。
                "targetMayHaveMutated": false
            }),
        )),
    }
}

// 发现当前全部双向认证 endpoint。
pub(crate) fn discover() -> AppResult<Value> {
    // 取得只含认证 endpoint 的集合。
    let endpoints = discover_certified_endpoints()?;
    // 投影符合公共 observation schema 的对象。
    let sessions = endpoints
        // 遍历认证 endpoint。
        .iter()
        // 删除 private native route。
        .map(CertifiedInteractiveEndpoint::observation)
        // 收集稳定公开集合。
        .collect::<Vec<_>>();
    // 返回 provider-neutral 发现结果。
    Ok(json!({
        // 标记查询成功，即使当前集合为空。
        "ok": true,
        // 输出稳定 capability ID。
        "capability": capabilities::INTERACTIVE_SESSION_DISCOVER,
        // 明确查询无副作用。
        "readOnly": true,
        // 输出主机无头发现域。
        "executionRealm": "host-headless",
        // 当前桌面没有 fallback。
        "foregroundFallbackUsed": false,
        // 输出认证 endpoint 数量。
        "count": sessions.len(),
        // 输出公共 session 观察集合。
        "sessions": sessions
    }))
}

// 执行一条已经由 System 与 Policy 冻结的独立会话 mutation。
pub(crate) fn execute(request: &CommandRequest) -> AppResult<Value> {
    // 独立会话路线永远要求 strict。
    if request.isolation_requirement != IsolationRequirement::Strict {
        // 禁止标准模式借用 endpoint 字段。
        return Err(isolation_error(
            // 使用固定隔离要求错误码。
            "ISOLATION_REQUIRED",
            // 不公开请求内容。
            "Independent interactive session execution requires strict isolation.",
            // 固定零副作用证据。
            json!({
                // 本地 provider 未调用。
                "localProviderInvoked": false,
                // 当前桌面未接管。
                "foregroundFallbackUsed": false
            }),
        ));
    }
    // 读取版本化 capability。
    let capability = request
        // 读取统一 wrapper 字段。
        .args
        // 取得 capability。
        .get("capability")
        // 要求字符串。
        .and_then(Value::as_str)
        // 缺失时失败闭合。
        .ok_or_else(|| {
            // 返回稳定参数错误。
            isolation_error(
                // 使用固定参数错误码。
                "INVALID_ARGUMENT",
                // 说明必需字段。
                "The isolated request requires a versioned capability.",
                // 固定零副作用证据。
                json!({
                    // 本地 provider 未调用。
                    "localProviderInvoked": false,
                    // 当前桌面未接管。
                    "foregroundFallbackUsed": false
                }),
            )
        })?;
    // 只接受四条冻结路线。
    let operation = expected_operation(capability).ok_or_else(|| {
        // 返回稳定 capability 缺口。
        isolation_error(
            // 使用固定 capability 缺口码。
            "CAPABILITY_GAP",
            // 不允许任意通用请求转发。
            "The capability is not available through independent interactive sessions.",
            // 固定零副作用证据。
            json!({
                // 回显公开 capability。
                "capability": capability,
                // 本地 provider 未调用。
                "localProviderInvoked": false,
                // 当前桌面未接管。
                "foregroundFallbackUsed": false
            }),
        )
    })?;
    // generic verb 必须与 capability 唯一路线一致。
    if request.operation.as_deref() != Some(operation) {
        // 返回稳定参数错误。
        return Err(isolation_error(
            // 使用固定参数错误码。
            "INVALID_ARGUMENT",
            // 不允许 operation 借用。
            "The isolated capability does not match the generic operation.",
            // 输出安全公开事实。
            json!({
                // 回显 capability。
                "capability": capability,
                // 输出期望 operation。
                "expectedOperation": operation,
                // 本地 provider 未调用。
                "localProviderInvoked": false,
                // 当前桌面未接管。
                "foregroundFallbackUsed": false
            }),
        ));
    }
    // 读取 worker 会话内需要重新解析的原窗口目标。
    let session_id = request
        // 访问 target wrapper。
        .target
        // 读取原 sessionId。
        .get("sessionId")
        // 要求字符串。
        .and_then(Value::as_str)
        // 缺失时失败闭合。
        .ok_or_else(|| {
            // 返回稳定参数错误。
            isolation_error(
                // 使用固定参数错误码。
                "INVALID_ARGUMENT",
                // 不回显其他 target 字段。
                "The isolated request requires an exact window sessionId.",
                // 固定零副作用证据。
                json!({
                    // 本地 provider 未调用。
                    "localProviderInvoked": false,
                    // 当前桌面未接管。
                    "foregroundFallbackUsed": false
                }),
            )
        })?;
    // 读取独立会话 endpoint 选择。
    let interactive_session_id = request
        // 访问 target wrapper。
        .target
        // 读取固定字段。
        .get("interactiveSessionId")
        // 要求字符串。
        .and_then(Value::as_str)
        // 缺失时失败闭合。
        .ok_or_else(|| {
            // 返回稳定参数错误。
            isolation_error(
                // 使用固定参数错误码。
                "INVALID_ARGUMENT",
                // 说明发现依赖。
                "The isolated request requires a discovered interactiveSessionId.",
                // 固定零副作用证据。
                json!({
                    // 本地 provider 未调用。
                    "localProviderInvoked": false,
                    // 当前桌面未接管。
                    "foregroundFallbackUsed": false
                }),
            )
        })?;
    // 提取领域 input 与唯一总 deadline。
    let (input, timeout_ms) = command_input_and_timeout(request)?;
    // 每次执行重新发现并认证 endpoint。
    let endpoints = discover_certified_endpoints()?;
    // 精确选择同一授权代际。
    let endpoint = select_endpoint(&endpoints, interactive_session_id)?;
    // 在新握手和新 lease 上执行唯一 command。
    let result = execute_certified_command(
        // 传入本次重新认证的 endpoint。
        &endpoint,
        // host handshake 与 worker 共享这一唯一总预算。
        timeout_ms,
        // 用 broker nonce 与扣减后的 deadline 构造固定 worker command。
        |request_nonce, endpoint_lease_nonce, remaining_timeout_ms| {
            // 构造字段封闭的 command worker 请求。
            json!({
                // 固定 command worker 协议版本。
                "contractVersion": "act/interactive-command-worker/v1",
                // 绑定本次 broker 首帧 nonce。
                "requestNonce": request_nonce,
                // 绑定同一连接 lease。
                "endpointLeaseNonce": endpoint_lease_nonce,
                // 转发已验证 capability。
                "capability": capability,
                // 转发唯一 generic operation。
                "operation": operation,
                // 只转发原窗口 opaque 目标。
                "sessionId": session_id,
                // 转发已移除第二 deadline 的领域输入。
                "input": input,
                // 保留逐操作确认。
                "confirmed": request.confirmed,
                // 许可只作用于独立会话前景。
                "foregroundConsent": request.foreground_consent,
                // 冻结严格隔离要求。
                "isolationRequirement": "strict",
                // 冻结隔离 worker 执行域。
                "requiredExecutionRealm": "isolated-worker",
                // 冻结零当前桌面干扰策略。
                "hostImpactPolicy": "strict-no-interference",
                // 传递完整生命周期剩余预算。
                "timeoutMs": remaining_timeout_ms
            })
        },
    )?;
    // fixed worker 成功才投影 data；结构化拒绝转换为统一错误。
    if result.get("ok") == Some(&Value::Bool(true)) {
        // 取得 worker 业务 data 对象。
        let mut data = result
            // 读取固定 data 字段。
            .get("data")
            // 要求对象。
            .and_then(Value::as_object)
            // 克隆为 System 结果。
            .cloned()
            // 缺失成功数据视为 worker 协议失败。
            .ok_or_else(|| {
                // 返回稳定协议错误。
                isolation_error(
                    // 使用 worker 协议失败码。
                    "WORKER_PROTOCOL_FAILED",
                    // 不回显 worker 输出。
                    "The isolated command worker omitted its completed data.",
                    // 禁止自动重试已完成 mutation。
                    json!({
                        // 业务可能已完成。
                        "businessAccepted": true,
                        // 无法认证完成结果。
                        "completed": false,
                        // 保守使用未知 outcome。
                        "outcome": "unknown",
                        // 禁止自动重试。
                        "retrySafe": false,
                        // 目标可能已经改变。
                        "targetMayHaveMutated": true,
                        // 本地 provider 未调用。
                        "localProviderInvoked": false,
                        // 当前桌面未接管。
                        "foregroundFallbackUsed": false
                    }),
                )
            })?;
        // 由 Module 附加独立会话公开身份。
        data.insert(
            // 使用稳定字段名。
            "interactiveSessionId".to_owned(),
            // 回显调用方已知目标。
            Value::String(interactive_session_id.to_owned()),
        );
        // 固定独立会话类别。
        data.insert(
            // 使用稳定字段名。
            "isolationKind".to_owned(),
            // 输出 provider-neutral 类别。
            Value::String("independent-interactive-session".to_owned()),
        );
        // 返回对象供 System 附加执行计划证明。
        return Ok(Value::Object(data));
    }
    // 读取 worker 封闭错误对象。
    let error = result.get("error").and_then(Value::as_object);
    // 只接受非空稳定错误码。
    let code = error
        // 展开可选错误对象。
        .and_then(|error| error.get("code"))
        // 要求字符串。
        .and_then(Value::as_str)
        // 拒绝空值。
        .filter(|value| !value.is_empty())
        // 缺失时使用 worker 协议失败。
        .unwrap_or("WORKER_PROTOCOL_FAILED");
    // 不允许动态或未登记错误码进入公开 envelope。
    let code = stable_worker_error_code(code);
    // 只接受非空安全消息。
    let message = error
        // 展开可选错误对象。
        .and_then(|error| error.get("message"))
        // 要求字符串。
        .and_then(Value::as_str)
        // 拒绝空值。
        .filter(|value| !value.is_empty())
        // 立即取得所有权，避免任何 worker JSON 借用跨过错误构造。
        .map(str::to_owned)
        // 缺失时使用固定安全消息。
        .unwrap_or_else(|| "The isolated command worker rejected the request.".to_owned());
    // 从 worker 状态构造 provider-neutral details。
    let mut details = Map::new();
    // 复制调用方已知 endpoint ID。
    details.insert(
        // 使用稳定字段名。
        "interactiveSessionId".to_owned(),
        // 输出公开身份。
        Value::String(interactive_session_id.to_owned()),
    );
    // 复制封闭结果字段和证据，不传播未知 worker 字段。
    for field in [
        // 复制 transport 接受事实。
        "transportAccepted",
        // 复制业务接受事实。
        "businessAccepted",
        // 复制确定完成事实。
        "completed",
        // 复制 outcome。
        "outcome",
        // 复制重试安全事实。
        "retrySafe",
        // 复制目标变化事实。
        "targetMayHaveMutated",
        // 复制 worker 证据对象。
        "evidence",
    ] {
        // 只在 worker 提供字段时复制。
        if let Some(value) = result.get(field) {
            // 复制 provider-neutral JSON 值。
            details.insert(field.to_owned(), value.clone());
        }
    }
    // host 本地 provider 永远没有接管。
    details.insert("localProviderInvoked".to_owned(), Value::Bool(false));
    // host 当前桌面 fallback 永远关闭。
    details.insert("foregroundFallbackUsed".to_owned(), Value::Bool(false));
    // 返回 worker 的封闭业务错误。
    Err(AppControlError::with_details(
        code,
        message,
        Value::Object(details),
    ))
}

// 声明不触碰真实 endpoint 的纯边界测试。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 导入被测纯函数与统一请求。
    use super::*;
    // 导入顶层动词。
    use crate::domain::Verb;

    // 固定第一条 canonical 独立会话 fixture ID。
    const SESSION_A: &str = "s2:i:1111111111111111";
    // 固定第二条 canonical 独立会话 fixture ID。
    const SESSION_B: &str = "s2:i:2222222222222222";

    // 从选择结果中提取结构化错误且拒绝意外成功。
    fn selection_error(
        // 接收纯选择函数结果。
        result: AppResult<CertifiedInteractiveEndpoint>,
    ) -> AppControlError {
        // 按确定成功或失败分类。
        match result {
            // 当前帮助函数只用于失败用例。
            Ok(_) => panic!("endpoint selection unexpectedly succeeded"),
            // 返回结构化错误供字段断言。
            Err(error) => error,
        }
    }

    // 验证四条 capability 集合保持封闭。
    #[test]
    fn supported_capabilities_are_exactly_the_frozen_mutations() {
        // 四条冻结路线全部接受。
        for capability in [
            // 键盘输入。
            capabilities::UI_INPUT_KEY,
            // 指针输入。
            capabilities::UI_INPUT_POINTER,
            // 窗口生命周期。
            capabilities::WINDOW_LIFECYCLE,
            // 窗口关闭。
            capabilities::WINDOW_CLOSE,
        ] {
            // 每条路线必须受支持。
            assert!(supports_capability(capability));
        }
        // 只读 UIA 与任意未知 capability 不得转发。
        assert!(!supports_capability(capabilities::UI_ELEMENT_LOCATE));
        // 未注册任意命令不得转发。
        assert!(!supports_capability("arbitrary.command@1"));
    }

    // 验证 timeout 从领域 input 提升为唯一总预算。
    #[test]
    fn command_timeout_is_removed_from_nested_input() {
        // 构造统一 app.apply 请求。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 提供带 timeout 的领域输入。
        request.args.insert(
            // 使用固定 input wrapper。
            "input".to_owned(),
            // 使用键盘最小输入。
            json!({ "key": "ENTER", "timeoutMs": 3000 }),
        );
        // 提取领域输入和总预算。
        let (input, timeout) = command_input_and_timeout(&request)
            // 合法输入必须成功。
            .unwrap_or_else(|error| panic!("timeout extraction failed: {error}"));
        // 总预算保留原值。
        assert_eq!(timeout, 3000);
        // worker 嵌套 input 不再含第二 deadline。
        assert!(input.get("timeoutMs").is_none());
        // 其他领域字段保持不变。
        assert_eq!(input["key"], "ENTER");
    }

    // 验证空认证清单按运行时缺失失败且不触碰 provider。
    #[test]
    fn empty_endpoint_set_is_structured_unavailable() {
        // 对空清单执行精确选择。
        let error = selection_error(select_endpoint(&[], SESSION_A));
        // 使用固定隔离 worker 不可用码。
        assert_eq!(error.code, "ISOLATED_WORKER_UNAVAILABLE");
        // host 本地 provider 从未调用。
        assert_eq!(error.details["localProviderInvoked"], false);
        // 当前桌面 fallback 从未启用。
        assert_eq!(error.details["foregroundFallbackUsed"], false);
        // 重新发现后可以安全重试。
        assert_eq!(error.details["retrySafe"], true);
        // 目标没有因选择失败而改变。
        assert_eq!(error.details["targetMayHaveMutated"], false);
    }

    // 验证非空清单中的缺失 ID 被识别为 stale 授权代际。
    #[test]
    fn missing_endpoint_in_nonempty_set_is_stale() {
        // 构造一条不连接真实 WTS 的认证 fixture。
        let endpoints = [CertifiedInteractiveEndpoint::fixture(1, SESSION_A)];
        // 选择另一条公开 ID。
        let error = selection_error(select_endpoint(&endpoints, SESSION_B));
        // 使用固定 stale 错误码。
        assert_eq!(error.code, "STALE_SESSION");
        // 只回显调用方已知公开 ID。
        assert_eq!(error.details["interactiveSessionId"], SESSION_B);
        // 当前桌面 fallback 从未启用。
        assert_eq!(error.details["foregroundFallbackUsed"], false);
        // stale 选择没有触碰目标。
        assert_eq!(error.details["targetMayHaveMutated"], false);
    }

    // 验证重复公开 ID 永远失败闭合而不任取 endpoint。
    #[test]
    fn duplicate_endpoint_identity_is_ambiguous() {
        // 构造两条 native route 不同但公开授权代际相同的 fixture。
        let endpoints = [
            // 第一条候选。
            CertifiedInteractiveEndpoint::fixture(1, SESSION_A),
            // 第二条候选。
            CertifiedInteractiveEndpoint::fixture(2, SESSION_A),
        ];
        // 对重复身份执行选择。
        let error = selection_error(select_endpoint(&endpoints, SESSION_A));
        // 使用固定歧义错误码。
        assert_eq!(error.code, "AMBIGUOUS_TARGET");
        // 只回显调用方已知公开 ID。
        assert_eq!(error.details["interactiveSessionId"], SESSION_A);
        // host 本地 provider 从未调用。
        assert_eq!(error.details["localProviderInvoked"], false);
        // 歧义选择没有触碰目标。
        assert_eq!(error.details["targetMayHaveMutated"], false);
    }
}
