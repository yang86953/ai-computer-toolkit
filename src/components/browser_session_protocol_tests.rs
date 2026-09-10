//! 验证浏览器会话打开协议的封闭输入、身份和双阶段状态机。

// 导入语言中立 JSON 值与构造宏。
use serde_json::{Value, json};

// 导入被测协议类型、函数与资源边界。
use super::{
    // 导入封闭打开结果。
    BrowserSessionOutcome,
    // 导入稳定错误码类别。
    BrowserSessionProtocolErrorCode,
    // 导入封闭会话来源。
    BrowserSessionSource,
    // 导入严格 worker 输入。
    BrowserSessionWorkerInput,
    // 导入固定协议版本。
    CONTRACT_VERSION,
    // 导入输入资源边界。
    MAXIMUM_INPUT_BYTES,
    // 导入输出资源边界。
    MAXIMUM_OUTPUT_BYTES,
    // 导入 stdout 状态机。
    observe_output,
};

// 固定测试请求 nonce。
const REQUEST_NONCE: &str = "0123456789abcdef0123456789abcdef";
// 固定测试授权 nonce。
const AUTHORIZATION_NONCE: &str = "fedcba9876543210fedcba9876543210";
// 固定测试 endpoint opaque ID。
const ENDPOINT_ID: &str = "bse1:11111111111111111111111111111111";
// 固定测试 session opaque ID。
const SESSION_ID: &str = "s2:bs:22222222222222222222222222222222";

// 构造一条严格 accepted 帧。
fn accepted_frame() -> Value {
    // 返回固定关联和事实组合。
    json!({
        // 使用 accepted 帧种类。
        "kind": "open-accepted",
        // 使用固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用测试请求关联值。
        "requestNonce": REQUEST_NONCE,
        // accepted 必须明确为 true。
        "dispatchAccepted": true,
        // accepted 尚未完成。
        "completed": false,
    })
}

// 构造一个安全错误对象。
fn safe_error(code: &str) -> Value {
    // 返回不含 endpoint 或原生事实的错误。
    json!({
        // 使用调用方指定稳定码。
        "code": code,
        // 使用固定安全消息。
        "message": "The browser session request did not complete.",
    })
}

// 构造一条严格 final 帧。
fn final_frame(
    // 接收封闭 outcome 文本。
    outcome: &str,
    // 接收确定完成事实。
    completed: bool,
    // 接收重试安全事实。
    retry_safe: bool,
    // 接收可能接受事实。
    accepted_may_have_occurred: bool,
    // 接收可选 session ID。
    session_id: Option<&str>,
    // 接收可选安全错误。
    error: Option<Value>,
) -> Value {
    // 先构造共同 final 字段。
    let mut frame = json!({
        // 使用 final 帧种类。
        "kind": "open-final",
        // 使用固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用测试请求关联值。
        "requestNonce": REQUEST_NONCE,
        // 使用调用方封闭结果。
        "outcome": outcome,
        // 保存确定完成事实。
        "completed": completed,
        // 保存重试事实。
        "retrySafe": retry_safe,
        // 保存可能接受事实。
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
    });
    // 取得对象以插入可选字段。
    if let Some(object) = frame.as_object_mut() {
        // ready 时插入 opaque 会话 ID。
        if let Some(session_id) = session_id {
            // 保存会话 ID 文本。
            object.insert("sessionId".to_owned(), Value::String(session_id.to_owned()));
        }
        // 非 ready 时插入安全错误。
        if let Some(error) = error {
            // 保存错误对象。
            object.insert("error".to_owned(), error);
        }
    }
    // 返回构造完成的帧。
    frame
}

// 把一组 JSON 值编码为 JSON Lines stdout。
fn lines(frames: &[Value]) -> String {
    // 逐帧使用紧凑 JSON 并以换行连接。
    frames
        // 遍历借用帧。
        .iter()
        // 序列化为单行 JSON。
        .map(Value::to_string)
        // 收集拥有型行。
        .collect::<Vec<_>>()
        // 使用唯一协议分隔符连接。
        .join("\n")
}

// 验证公开内部 schema 与冻结资源及身份边界一致。
#[test]
fn schema_keeps_frozen_protocol_boundaries() -> Result<(), Box<dyn std::error::Error>> {
    // 解析仓库内版本化 schema。
    let schema: Value = serde_json::from_str(include_str!(
        // 使用编译期相对路径定位契约。
        "../../contracts/internal/browser-session-worker-v1.schema.json"
    ))?;
    // schema ID 必须固定。
    assert_eq!(
        // 读取 schema ID。
        schema["$id"],
        // 对比冻结地址。
        "https://ai-computer-toolkit.local/contracts/internal/browser-session-worker-v1.schema.json"
    );
    // 协议版本必须与 Rust 单一来源一致。
    assert_eq!(schema["$defs"]["version"]["const"], CONTRACT_VERSION);
    // deadline 下界固定为一毫秒。
    assert_eq!(
        schema["$defs"]["openBase"]["properties"]["timeoutMs"]["minimum"],
        1
    );
    // deadline 上界固定为三十秒。
    assert_eq!(
        // 读取 schema 上界。
        schema["$defs"]["openBase"]["properties"]["timeoutMs"]["maximum"],
        // 对比冻结数值。
        30_000
    );
    // endpoint 身份不能是 URL 或端口。
    assert_eq!(
        // 读取 endpoint pattern。
        schema["$defs"]["endpointId"]["pattern"],
        // 对比 opaque ID 格式。
        "^bse1:[0-9a-f]{32}$"
    );
    // session 身份不能是浏览器原生 target ID。
    assert_eq!(
        // 读取 session pattern。
        schema["$defs"]["sessionId"]["pattern"],
        // 对比 opaque ID 格式。
        "^s2:bs:[0-9a-f]{32}$"
    );
    // Rust 输入边界保持八 KiB。
    assert_eq!(MAXIMUM_INPUT_BYTES, 8 * 1024);
    // Rust 输出边界保持六十四 KiB。
    assert_eq!(MAXIMUM_OUTPUT_BYTES, 64 * 1024);
    // 测试正常完成。
    Ok(())
}

// 验证两种来源与取消输入均严格解析。
#[test]
fn input_accepts_only_canonical_sources_and_correlation() -> Result<(), Box<dyn std::error::Error>>
{
    // 构造显式授权 endpoint 打开请求。
    let authorized = json!({
        // 使用唯一打开 kind。
        "kind": "open",
        // 使用固定版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用 canonical request nonce。
        "requestNonce": REQUEST_NONCE,
        // 使用有效总 deadline。
        "timeoutMs": 30_000,
        // 使用私有 registry 身份与授权 nonce。
        "source": { "kind": "authorized-endpoint", "endpointId": ENDPOINT_ID, "authorizationNonce": AUTHORIZATION_NONCE },
    });
    // 解析显式授权来源。
    let parsed = BrowserSessionWorkerInput::parse_line(&authorized.to_string())?;
    // 保持请求关联值。
    assert_eq!(parsed.request_nonce(), REQUEST_NONCE);
    // 来源必须投影为授权 endpoint 变体。
    assert!(matches!(
        parsed.source(),
        Some(BrowserSessionSource::AuthorizedEndpoint { .. })
    ));

    // 构造不接受路径或参数的隔离 profile 请求。
    let isolated = json!({
        // 使用唯一打开 kind。
        "kind": "open",
        // 使用固定版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用 canonical request nonce。
        "requestNonce": REQUEST_NONCE,
        // 使用最小合法 deadline。
        "timeoutMs": 1,
        // 隔离来源只有 kind。
        "source": { "kind": "isolated-profile" },
    });
    // 解析工具自有隔离来源。
    let parsed = BrowserSessionWorkerInput::parse_line(&isolated.to_string())?;
    // 来源必须投影为无字段隔离变体。
    assert!(matches!(
        parsed.source(),
        Some(BrowserSessionSource::IsolatedProfile {})
    ));

    // 构造关联同一请求的幂等取消。
    let cancel = json!({
        // 使用取消 kind。
        "kind": "cancel",
        // 使用固定版本。
        "contractVersion": CONTRACT_VERSION,
        // 复用打开请求 nonce。
        "requestNonce": REQUEST_NONCE,
    });
    // 解析取消请求。
    let parsed = BrowserSessionWorkerInput::parse_line(&cancel.to_string())?;
    // 取消没有会话来源。
    assert!(parsed.source().is_none());
    // 测试正常完成。
    Ok(())
}

// 验证 URL、路径、未知字段和越界资源不能穿透协议。
#[test]
fn input_rejects_native_endpoint_and_profile_fields() {
    // 构造携带调试 URL 的伪授权来源。
    let endpoint_url = json!({
        // 使用打开 kind。
        "kind": "open",
        // 使用固定版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用 canonical nonce。
        "requestNonce": REQUEST_NONCE,
        // 使用有效 deadline。
        "timeoutMs": 1000,
        // URL 字段不属于协议。
        "source": { "kind": "authorized-endpoint", "endpointId": ENDPOINT_ID, "authorizationNonce": AUTHORIZATION_NONCE, "url": "http://127.0.0.1:9222" },
    });
    // URL 必须由 deny_unknown_fields 拒绝。
    assert_eq!(
        // 取得失败类别。
        BrowserSessionWorkerInput::parse_line(&endpoint_url.to_string())
            .err()
            .map(|error| error.code()),
        // 对比普通参数拒绝。
        Some(BrowserSessionProtocolErrorCode::InvalidArgument)
    );

    // 构造携带用户 profile 路径的伪隔离来源。
    let profile_path = json!({
        // 使用打开 kind。
        "kind": "open",
        // 使用固定版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用 canonical nonce。
        "requestNonce": REQUEST_NONCE,
        // 使用有效 deadline。
        "timeoutMs": 1000,
        // profilePath 不属于封闭隔离来源。
        "source": { "kind": "isolated-profile", "profilePath": "C:/Users/example" },
    });
    // 调用方路径必须拒绝。
    assert!(BrowserSessionWorkerInput::parse_line(&profile_path.to_string()).is_err());

    // 构造错误大小写 endpoint ID。
    let uppercase_endpoint = json!({
        // 使用打开 kind。
        "kind": "open",
        // 使用固定版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用 canonical nonce。
        "requestNonce": REQUEST_NONCE,
        // 使用有效 deadline。
        "timeoutMs": 1000,
        // 大写十六进制不是 canonical。
        "source": { "kind": "authorized-endpoint", "endpointId": "bse1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "authorizationNonce": AUTHORIZATION_NONCE },
    });
    // 非 canonical endpoint 必须拒绝。
    assert!(BrowserSessionWorkerInput::parse_line(&uppercase_endpoint.to_string()).is_err());

    // 构造超过输入上限的非敏感文本。
    let oversized = "x".repeat(MAXIMUM_INPUT_BYTES + 1);
    // 超限必须在 JSON 解析前使用资源错误。
    assert_eq!(
        // 取得失败类别。
        BrowserSessionWorkerInput::parse_line(&oversized)
            .err()
            .map(|error| error.code()),
        // 对比请求过大类别。
        Some(BrowserSessionProtocolErrorCode::RequestTooLarge)
    );
}

// 验证 accepted 后 ready final 建立唯一确定会话事实。
#[test]
fn accepted_then_ready_final_is_valid() -> Result<(), Box<dyn std::error::Error>> {
    // 构造 accepted 与 ready final。
    let output = lines(&[
        // 首帧建立 accepted。
        accepted_frame(),
        // 终帧只返回 opaque session ID。
        final_frame("ready", true, false, true, Some(SESSION_ID), None),
    ]);
    // 运行严格 stdout 状态机。
    let observation = observe_output(&output, REQUEST_NONCE)?;
    // accepted 事实必须保留。
    assert!(observation.accepted());
    // final 必须存在。
    let final_observation = observation
        // 借用可选 final。
        .final_observation()
        // 将理论缺失转为测试错误。
        .ok_or_else(|| std::io::Error::other("ready final was not observed"))?;
    // outcome 必须为 ready。
    assert_eq!(final_observation.outcome(), BrowserSessionOutcome::Ready);
    // ready 是确定终态。
    assert!(final_observation.completed());
    // accepted 会话不可由框架自动重试。
    assert!(!final_observation.retry_safe());
    // endpoint 或 runtime 确定可能已接受。
    assert!(final_observation.accepted_may_have_occurred());
    // 只公开 canonical opaque session ID。
    assert_eq!(final_observation.session_id(), Some(SESSION_ID));
    // ready 不携带错误。
    assert!(final_observation.error().is_none());
    // 测试正常完成。
    Ok(())
}

// 验证零帧、accepted-only 和未 dispatch final 保持不同事实。
#[test]
fn partial_and_not_dispatched_observations_remain_distinct()
-> Result<(), Box<dyn std::error::Error>> {
    // 空 stdout 只证明尚未观察到 dispatch。
    let empty = observe_output("", REQUEST_NONCE)?;
    // 不伪造 accepted。
    assert!(!empty.accepted());
    // 不伪造 final。
    assert!(empty.final_observation().is_none());

    // accepted-only 表示外部资源可能已经接受。
    let accepted_only = observe_output(&accepted_frame().to_string(), REQUEST_NONCE)?;
    // accepted 事实必须保留。
    assert!(accepted_only.accepted());
    // 缺失 final 必须交由 runner 聚合为未知。
    assert!(accepted_only.final_observation().is_none());

    // 未 dispatch final 不允许前序 accepted。
    let rejected = final_frame(
        // 使用确定未 dispatch 结果。
        "not-dispatched",
        // 拒绝是确定终态。
        true,
        // 修正请求后可重试。
        true,
        // 外部没有接受。
        false,
        // 不返回 session ID。
        None,
        // 返回安全授权错误。
        Some(safe_error("ENDPOINT_AUTHENTICATION_FAILED")),
    );
    // 解析单 final 输出。
    let observation = observe_output(&rejected.to_string(), REQUEST_NONCE)?;
    // 明确没有 accepted。
    assert!(!observation.accepted());
    // final 必须存在。
    let final_observation = observation
        // 借用 final。
        .final_observation()
        // 将理论缺失转为测试错误。
        .ok_or_else(|| std::io::Error::other("not-dispatched final was not observed"))?;
    // outcome 必须为确定未 dispatch。
    assert_eq!(
        final_observation.outcome(),
        BrowserSessionOutcome::NotDispatched
    );
    // 测试正常完成。
    Ok(())
}

// 验证未知终态必须在 accepted 后使用固定错误码。
#[test]
fn unknown_outcome_requires_accepted_and_fixed_error() {
    // 构造合法 accepted 后未知终态。
    let valid = lines(&[
        // 建立 accepted 事实。
        accepted_frame(),
        // 使用唯一未知错误码。
        final_frame(
            "unknown",
            false,
            false,
            true,
            None,
            Some(safe_error("OUTCOME_UNKNOWN")),
        ),
    ]);
    // 合法未知终态必须接受。
    assert!(observe_output(&valid, REQUEST_NONCE).is_ok());

    // 构造未 accepted 的伪未知终态。
    let before_accepted = final_frame(
        // 伪造 unknown。
        "unknown",
        // 保持未完成。
        false,
        // 禁止重试。
        false,
        // 声称可能接受。
        true,
        // 不返回会话 ID。
        None,
        // 使用固定未知错误。
        Some(safe_error("OUTCOME_UNKNOWN")),
    );
    // 缺失 accepted 时必须拒绝。
    assert!(observe_output(&before_accepted.to_string(), REQUEST_NONCE).is_err());

    // 构造 accepted 后使用普通失败码的伪 unknown。
    let wrong_code = lines(&[
        // 建立 accepted。
        accepted_frame(),
        // 错误码与 unknown 不一致。
        final_frame(
            "unknown",
            false,
            false,
            true,
            None,
            Some(safe_error("TIMEOUT")),
        ),
    ]);
    // unknown 错误码漂移必须拒绝。
    assert!(observe_output(&wrong_code, REQUEST_NONCE).is_err());
}

// 验证关联漂移、重复帧和原生 session 身份失败闭合。
#[test]
fn output_rejects_correlation_order_and_identity_drift() {
    // 构造错误 nonce 的 accepted 帧。
    let mut wrong_nonce = accepted_frame();
    // 修改关联值而不改变其他字段。
    wrong_nonce["requestNonce"] = Value::String(AUTHORIZATION_NONCE.to_owned());
    // 关联漂移必须拒绝。
    assert!(observe_output(&wrong_nonce.to_string(), REQUEST_NONCE).is_err());

    // 构造重复 accepted。
    let duplicated = lines(&[
        // 首个 accepted 合法。
        accepted_frame(),
        // 第二个 accepted 必须拒绝。
        accepted_frame(),
    ]);
    // 重复 accepted 必须拒绝。
    assert!(observe_output(&duplicated, REQUEST_NONCE).is_err());

    // 构造携带原生浏览器 target ID 的 ready final。
    let native_session = lines(&[
        // 建立 accepted。
        accepted_frame(),
        // 使用不带 opaque 前缀的伪 session。
        final_frame("ready", true, false, true, Some("CDP-target-123"), None),
    ]);
    // 原生身份不得穿透。
    assert!(observe_output(&native_session, REQUEST_NONCE).is_err());

    // 构造超过 stdout 上限的噪声。
    let oversized = "x".repeat(MAXIMUM_OUTPUT_BYTES + 1);
    // 超限必须使用稳定输出资源类别。
    assert_eq!(
        // 取得失败类别。
        observe_output(&oversized, REQUEST_NONCE)
            .err()
            .map(|error| error.code()),
        // 对比输出过大类别。
        Some(BrowserSessionProtocolErrorCode::OutputTooLarge)
    );
}
