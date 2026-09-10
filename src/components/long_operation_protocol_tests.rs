//! 验证长操作 broker 协议的封闭字段与确认顺序。

// 导入当前 Component 的私有测试接口。
use super::*;
// 导入 JSON fixture 构造器。
use serde_json::json;

// 返回 canonical 请求 nonce。
fn request_nonce() -> &'static str {
    // 使用固定三十二位小写十六进制 fixture。
    "0123456789abcdef0123456789abcdef"
}

// 返回 canonical 窗口目标。
fn window_target() -> String {
    // 从私有测试身份生成不可逆公开目标。
    OpaqueTargetId::new(OpaqueTargetKind::Window, "long-operation-window").to_string()
}

// 返回 canonical operation handle。
fn operation_id() -> String {
    // 从会话与随机值的合成 fixture 生成任务句柄。
    OpaqueTargetId::new(OpaqueTargetKind::Operation, "session-7|nonce-a").to_string()
}

// 构造经过确认的固定窗口录制提交。
fn submit_json(confirmed: bool, target: Value) -> String {
    // 序列化协议 fixture。
    json!({
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 固定请求关联值。
        "requestNonce": request_nonce(),
        // 选择提交 Command。
        "action": "submit",
        // 固定首个长操作 capability。
        "capabilityId": WINDOW_RECORD_CAPABILITY,
        // 传入精确窗口目标。
        "target": target,
        // 使用领域 Module 后续验证的对象。
        "input": {"path": "recording.mp4"},
        // 显式传入确认事实。
        "confirmed": confirmed
    })
    // 转换为单个 JSON frame。
    .to_string()
}

// 验证确认后的窗口录制提交可以严格解析。
#[test]
// 覆盖 action、版本、nonce、capability、目标和输入投影。
fn parses_confirmed_window_record_submission() -> Result<(), LongOperationProtocolFailure> {
    // 构造单字段 canonical 窗口目标。
    let target = json!({"sessionId": window_target()});
    // 解析固定提交 frame。
    let request = LongOperationBrokerRequest::parse(&submit_json(true, target))?;
    // 核对固定版本。
    assert_eq!(request.contract_version(), CONTRACT_VERSION);
    // 核对 canonical 关联值。
    assert_eq!(request.request_nonce(), request_nonce());
    // 核对提交 action。
    assert_eq!(request.action(), LongOperationBrokerAction::Submit);
    // 提交不得提前拥有 operation handle。
    assert_eq!(request.operation_id(), None);
    // 核对冻结 capability。
    assert_eq!(request.capability_id(), Some(WINDOW_RECORD_CAPABILITY));
    // 目标保持对象形状。
    assert!(request.target().is_some_and(Value::is_object));
    // 输入保持对象形状。
    assert!(request.input().is_some_and(Value::is_object));
    // 返回测试成功。
    Ok(())
}

// 验证生产 client 构造器与 broker parser 使用同一字段门禁。
#[test]
fn client_submit_constructor_is_confirmation_first_and_closed() {
    // 构造确认后的合法 submit。
    let request = LongOperationBrokerRequest::submit_window_record(
        // 使用 canonical 请求 nonce。
        request_nonce().to_owned(),
        // 使用 canonical 窗口目标。
        window_target(),
        // 使用对象领域输入。
        json!({"outputPath": "C:\\recordings\\capture.mp4"}),
        // 显式确认。
        true,
    )
    // 合法生产请求必须可构造。
    .unwrap_or_else(|| panic!("confirmed client submit should be constructed"));
    // 固定选择 submit action。
    assert_eq!(request.action(), LongOperationBrokerAction::Submit);
    // 构造器不得建立 operation handle。
    assert_eq!(request.operation_id(), None);
    // 固定 capability 不得由调用方替换。
    assert_eq!(request.capability_id(), Some(WINDOW_RECORD_CAPABILITY));
    // 未确认请求必须在目标和输入语义前拒绝。
    assert!(
        // 使用相同 target 与 input，只改变确认事实。
        LongOperationBrokerRequest::submit_window_record(
            // 使用 canonical 请求 nonce。
            request_nonce().to_owned(),
            // 使用 canonical 窗口目标。
            window_target(),
            // 注入仍为对象的领域输入。
            json!({"nativePath": 42}),
            // 明确不确认。
            false,
        )
        // 构造器必须失败闭合。
        .is_none()
    );
}

// 验证缺少确认时不会解析伪目标或路径语义。
#[test]
// 同时放入 native 目标和未知领域字段以证明确认优先。
fn rejects_before_target_semantics_when_unconfirmed() {
    // 构造未确认且目标非法的 frame。
    let result = LongOperationBrokerRequest::parse(&submit_json(
        // 明确不确认。
        false,
        // 使用必须在确认后才会被拒绝的 native 形状。
        json!({"hwnd": 42}),
    ));
    // 提取预期的确认拒绝。
    let error = match result {
        // 保留封闭失败事实。
        Err(error) => error,
        // 成功意味着确认门禁回归。
        Ok(_) => panic!("unconfirmed submission unexpectedly parsed"),
    };
    // 确认拒绝优先于目标参数拒绝。
    assert_eq!(
        error.code(),
        LongOperationProtocolErrorCode::ConfirmationRequired
    );
    // 固定 envelope 已经接受，但业务提交尚未接受。
    assert!(error.transport_accepted());
}

// 验证 status 与 cancel 只接受 canonical operation handle。
#[test]
// 覆盖两个 handle-only action 的互斥字段。
fn parses_handle_only_status_and_cancel() -> Result<(), LongOperationProtocolFailure> {
    // 逐一验证只读查询与幂等取消 Command。
    for action in ["status", "cancel"] {
        // 构造只带 operation handle 的请求。
        let text = json!({
            // 固定协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 固定请求关联值。
            "requestNonce": request_nonce(),
            // 使用当前迭代 action。
            "action": action,
            // 使用 canonical operation handle。
            "operationId": operation_id()
        })
        // 转换为 JSON frame。
        .to_string();
        // 严格解析请求。
        let request = LongOperationBrokerRequest::parse(&text)?;
        // handle 必须保持 canonical operation 类别。
        assert!(
            request
                // 读取 operation handle。
                .operation_id()
                // 验证固定类别前缀。
                .is_some_and(|value| value.starts_with("s2:o:"))
        );
        // Query/Command 不得携带 submit capability。
        assert_eq!(request.capability_id(), None);
    }
    // 返回测试成功。
    Ok(())
}

// 验证 envelope 对版本、nonce 与扩展字段失败闭合。
#[test]
// 覆盖 transport 尚未接受的三类协议错误。
fn rejects_noncanonical_envelopes_without_acceptance() {
    // 构造必须在 envelope 阶段拒绝的请求集合。
    let invalid = [
        // 拒绝未知协议版本。
        json!({"contractVersion":"act/long-operation-broker/v2","requestNonce":request_nonce(),"action":"status","operationId":operation_id()}),
        // 拒绝大写 nonce。
        json!({"contractVersion":CONTRACT_VERSION,"requestNonce":"0123456789ABCDEF0123456789ABCDEF","action":"status","operationId":operation_id()}),
        // 拒绝任意扩展字段。
        json!({"contractVersion":CONTRACT_VERSION,"requestNonce":request_nonce(),"action":"status","operationId":operation_id(),"nativeHandle":42}),
    ];
    // 逐一验证封闭拒绝。
    for value in invalid {
        // 解析当前非法 frame。
        let result = LongOperationBrokerRequest::parse(&value.to_string());
        // 提取预期的 envelope 拒绝。
        let error = match result {
            // 保留封闭失败事实。
            Err(error) => error,
            // 成功意味着 envelope 门禁回归。
            Ok(_) => panic!("invalid envelope unexpectedly parsed"),
        };
        // 全部收敛为稳定参数错误。
        assert_eq!(
            error.code().as_str(),
            LongOperationProtocolErrorCode::InvalidArgument.as_str()
        );
        // 不得标记 transport 已接受。
        assert!(!error.transport_accepted());
    }
}

// 验证 handle Query 拒绝其他 opaque 类别与 submit 字段。
#[test]
// 防止 window target 或 payload 被误当作 operation handle。
fn rejects_non_operation_or_mixed_handle_queries() {
    // 构造窗口类别冒充 operation 的请求。
    let wrong_kind = json!({
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 固定请求关联值。
        "requestNonce": request_nonce(),
        // 选择 status Query。
        "action": "status",
        // 注入错误 opaque 类别。
        "operationId": window_target()
    });
    // 构造混入 submit 字段的请求。
    let mixed = json!({
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 固定请求关联值。
        "requestNonce": request_nonce(),
        // 选择 cancel Command。
        "action": "cancel",
        // 使用正确 operation handle。
        "operationId": operation_id(),
        // 混入禁止的 capability。
        "capabilityId": WINDOW_RECORD_CAPABILITY
    });
    // 逐一验证 action 专属字段拒绝。
    for value in [wrong_kind, mixed] {
        // 解析当前非法请求。
        let result = LongOperationBrokerRequest::parse(&value.to_string());
        // 提取预期的字段组合拒绝。
        let error = match result {
            // 保留封闭失败事实。
            Err(error) => error,
            // 成功意味着 action 字段隔离回归。
            Ok(_) => panic!("mixed handle request unexpectedly parsed"),
        };
        // 使用稳定参数错误。
        assert_eq!(
            error.code(),
            LongOperationProtocolErrorCode::InvalidArgument
        );
        // 固定 envelope 已经接受。
        assert!(error.transport_accepted());
    }
}
