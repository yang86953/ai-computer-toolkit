// 导入 JSON 构造宏。
use serde_json::json;

// 导入当前 Component 的私有接口。
use super::*;
// 导入独立响应 Component 的 dispatch 前拒绝构造器。
use crate::components::interactive_session_broker_response::rejection;

// 构造固定测试 endpoint 证明。
fn endpoint() -> BrokerEndpointAttestation {
    // 使用 canonical s2:i 测试目标。
    BrokerEndpointAttestation::new("s2:i:0123456789abcdef".to_owned())
        // 测试夹具必须可构造。
        .unwrap_or_else(|_| panic!("canonical interactive target must be accepted"))
}

// 构造最小合法 command worker 请求。
fn command(request_nonce: &str, lease_nonce: &str) -> Value {
    // 返回字段完全封闭的严格窗口关闭请求。
    json!({
        // 使用固定 command worker 协议版本。
        "contractVersion": "act/interactive-command-worker/v1",
        // 使用 broker 首帧关联值。
        "requestNonce": request_nonce,
        // 使用 broker 签发 lease。
        "endpointLeaseNonce": lease_nonce,
        // 使用通用窗口关闭 capability。
        "capability": "window.close@1",
        // 使用统一 close operation。
        "operation": "close",
        // 使用 worker 会话内重新解析的窗口目标。
        "sessionId": "s2:w:fedcba9876543210",
        // 窗口关闭输入为空对象。
        "input": {},
        // 满足逐操作确认。
        "confirmed": true,
        // 窗口关闭不需要激活许可。
        "foregroundConsent": false,
        // 冻结严格隔离要求。
        "isolationRequirement": "strict",
        // 冻结隔离 worker 执行域。
        "requiredExecutionRealm": "isolated-worker",
        // 冻结零干扰策略。
        "hostImpactPolicy": "strict-no-interference",
        // 使用最小充分 deadline。
        "timeoutMs": 1000
    })
}

// 验证首帧只接受观察和 lease 两条固定操作。
#[test]
fn initial_frame_is_closed_and_nonce_bound() {
    // 构造合法只读观察请求。
    let value = json!({
        // 使用固定 broker 协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用 canonical nonce。
        "requestNonce": "0123456789abcdef0123456789abcdef",
        // 使用只读观察操作。
        "operation": "observe"
    });
    // 解析合法首帧。
    let request = BrokerInitialRequest::parse(&value.to_string())
        // 合法夹具必须成功。
        .unwrap_or_else(|_| panic!("valid observation frame must parse"));
    // 核对封闭操作。
    assert_eq!(request.operation(), BrokerInitialOperation::Observe);
    // 核对可信关联值。
    assert_eq!(request.request_nonce(), "0123456789abcdef0123456789abcdef");

    // 添加任意扩展字段。
    let mut extended = value;
    // JSON 宏保证对象形状。
    if let Some(object) = extended.as_object_mut() {
        // 插入调用方指定 endpoint 字段。
        object.insert("pipe".to_owned(), json!("arbitrary"));
    }
    // 未知字段必须使 transport envelope 失败。
    let failure = BrokerInitialRequest::parse(&extended.to_string())
        // 保存预期失败。
        .err()
        // 意外成功时明确测试失败。
        .unwrap_or_else(|| panic!("unknown initial field must fail"));
    // 未接受 envelope 不回显 nonce。
    assert!(!failure.transport_accepted());
    // 不可信 nonce 不进入错误响应。
    assert_eq!(failure.request_nonce(), None);
}

// 验证观察与 lease 响应不能互相替换。
#[test]
fn endpoint_response_binds_operation_build_and_lease() {
    // 使用固定请求 nonce。
    let request_nonce = "0123456789abcdef0123456789abcdef";
    // 构造观察响应。
    let observation = BrokerEndpointResponse::observation(request_nonce, endpoint());
    // 序列化版本化响应。
    let text = serde_json::to_string(&observation)
        // 静态响应必须可序列化。
        .unwrap_or_else(|error| panic!("observation serialization failed: {error}"));
    // host 以观察操作解析成功。
    let parsed = BrokerEndpointResponse::parse(
        // 传入完整响应。
        &text,
        // 绑定请求 nonce。
        request_nonce,
        // 绑定观察操作。
        BrokerEndpointOperation::Observation,
    )
    // 合法响应必须通过。
    .unwrap_or_else(|_| panic!("valid observation must authenticate"));
    // 观察不产生 lease。
    assert_eq!(parsed.endpoint_lease_nonce(), None);
    // endpoint 只公开 s2:i。
    assert_eq!(
        parsed.endpoint().interactive_session_id(),
        "s2:i:0123456789abcdef"
    );

    // 观察响应不能作为 lease 响应使用。
    let mismatch = BrokerEndpointResponse::parse(
        // 复用相同响应文本。
        &text,
        // 复用相同请求 nonce。
        request_nonce,
        // 错误期待 lease。
        BrokerEndpointOperation::CommandLease,
    );
    // 类型替换必须失败闭合。
    assert!(mismatch.is_err());

    // 构造合法 command lease。
    let lease = BrokerEndpointResponse::command_lease(
        // 绑定首帧 nonce。
        request_nonce,
        // 使用不同 canonical lease nonce。
        "fedcba9876543210fedcba9876543210".to_owned(),
        // 使用相同 endpoint 证明。
        endpoint(),
    )
    // 合法 lease 必须构造成功。
    .unwrap_or_else(|_| panic!("valid lease must be issued"));
    // 核对 lease 不为空。
    assert_eq!(
        lease.endpoint_lease_nonce(),
        Some("fedcba9876543210fedcba9876543210")
    );
}

// 验证第二帧同时绑定连接、lease、endpoint 与 command worker 请求。
#[test]
fn command_frame_cross_checks_all_lifecycle_identifiers() {
    // 使用首帧请求 nonce。
    let request_nonce = "0123456789abcdef0123456789abcdef";
    // 使用 broker 签发的 lease nonce。
    let lease_nonce = "fedcba9876543210fedcba9876543210";
    // 使用当前授权代际公开目标。
    let interactive_session_id = "s2:i:0123456789abcdef";
    // 构造完整 broker command frame。
    let request = BrokerCommandRequest::new(
        // 复制请求 nonce。
        request_nonce.to_owned(),
        // 复制 lease nonce。
        lease_nonce.to_owned(),
        // 复制 endpoint 目标。
        interactive_session_id.to_owned(),
        // 嵌入完整 command worker 请求。
        command(request_nonce, lease_nonce),
    );
    // 序列化 frame。
    let text = serde_json::to_string(&request)
        // 静态 frame 必须可序列化。
        .unwrap_or_else(|error| panic!("command serialization failed: {error}"));
    // 完整交叉验证成功。
    let parsed = BrokerCommandRequest::parse(
        // 传入 command frame。
        &text,
        // 绑定首帧 nonce。
        request_nonce,
        // 绑定本连接 lease。
        lease_nonce,
        // 绑定当前 endpoint 授权代际。
        interactive_session_id,
    )
    // 合法 command 必须通过。
    .unwrap_or_else(|_| panic!("valid command frame must parse"));
    // deadline 从强类型 command 协议读取。
    assert_eq!(
        parsed
            // 读取有界剩余预算。
            .command_timeout_ms()
            // 合法 command 必须提供预算。
            .unwrap_or_else(|_| panic!("validated command must expose timeout")),
        1000
    );

    // 使用历史 endpoint 授权代际解析相同 frame。
    let stale = BrokerCommandRequest::parse(
        // 复用完整 frame。
        &text,
        // 保持连接 nonce。
        request_nonce,
        // 保持本连接 lease。
        lease_nonce,
        // 改变期望授权代际。
        "s2:i:1111111111111111",
    )
    // 保存预期失败。
    .err()
    // 意外成功时明确测试失败。
    .unwrap_or_else(|| panic!("stale endpoint must fail"));
    // 必须使用明确 stale 语义。
    assert_eq!(stale.code(), BrokerProtocolErrorCode::StaleSession);

    // 使用不同连接 lease 解析相同 frame。
    let replaced = BrokerCommandRequest::parse(
        // 复用完整 frame。
        &text,
        // 保持请求 nonce。
        request_nonce,
        // 提供另一个 canonical lease。
        "11111111111111111111111111111111",
        // 保持 endpoint。
        interactive_session_id,
    )
    // 保存预期失败。
    .err()
    // 意外成功时明确测试失败。
    .unwrap_or_else(|| panic!("cross-connection lease replacement must fail"));
    // lease 替换属于 endpoint 认证失败。
    assert_eq!(
        replaced.code(),
        BrokerProtocolErrorCode::EndpointAuthenticationFailed
    );
}

// 验证 broker 拒绝响应固定为 dispatch 前零副作用。
#[test]
fn rejection_never_claims_worker_dispatch() {
    // 构造可信 frame 后的 endpoint 认证失败。
    let failure = BrokerProtocolFailure::accepted(
        // 使用固定错误码。
        BrokerProtocolErrorCode::EndpointAuthenticationFailed,
        // 使用 canonical 请求 nonce。
        "0123456789abcdef0123456789abcdef",
    );
    // 构造已经认证 peer 和 session、尚未启动 worker 的拒绝。
    let response = rejection(
        // 传入协议失败。
        &failure, // peer 已认证。
        true,     // session 已认证。
        true,     // lease 尚未签发。
        false,
    );
    // 转换为 JSON 以核对稳定 envelope。
    let value = serde_json::to_value(response)
        // 静态响应必须可序列化。
        .unwrap_or_else(|error| panic!("rejection serialization failed: {error}"));
    // transport frame 已接受。
    assert_eq!(value["transportAccepted"], true);
    // mutation 尚未被业务接受。
    assert_eq!(value["businessAccepted"], false);
    // 失败发生在 dispatch 前。
    assert_eq!(value["outcome"], "not-dispatched");
    // worker 未启动。
    assert_eq!(value["evidence"]["commandWorkerStarted"], false);
    // 当前桌面 fallback 永远为 false。
    assert_eq!(value["evidence"]["foregroundFallbackUsed"], false);
}
