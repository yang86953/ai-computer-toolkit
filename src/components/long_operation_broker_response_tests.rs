//! 验证长操作 client response 的字段、关联与业务接受边界。

// 导入 JSON fixture 宏。
use serde_json::json;

// 导入被测私有 parser。
use super::*;

// 固定 canonical 请求 nonce。
const REQUEST_NONCE: &str = "0123456789abcdef0123456789abcdef";

// 验证 accepted 状态可以完整投影且保留 null expiresAt。
#[test]
fn parses_strict_success_response() {
    // 构造字段封闭成功 frame。
    let frame = json!({
        // 固定 broker 版本。
        "contractVersion": CONTRACT_VERSION,
        // 绑定本次请求。
        "requestNonce": REQUEST_NONCE,
        // transport 已接受。
        "transportAccepted": true,
        // status Query 已接受。
        "businessAccepted": true,
        // 返回完整公开状态。
        "operation": {
            // 固定状态版本。
            "contractVersion": "act/long-operation/v1",
            // canonical operation handle。
            "operationId": "s2:o:0000000000000001",
            // 固定首个 capability。
            "capabilityId": "window.record@1",
            // 初始状态。
            "status": "accepted",
            // 尚未 dispatch。
            "dispatchStarted": false,
            // 非终态。
            "terminal": false,
            // handle 已建立。
            "acceptedMayHaveOccurred": true,
            // 非终态不声称可重提。
            "retrySafe": false,
            // 尚未取消。
            "cancelRequested": false,
            // 非终态无固定到期时间。
            "expiresAt": null
        }
    })
    // 转换为受控 JSON 文本。
    .to_string();
    // 严格解析成功响应。
    let response = parse_response(&frame, REQUEST_NONCE)
        // fixture 必须通过。
        .unwrap_or_else(|_| panic!("strict success response should parse"));
    // 验证返回成功 operation。
    match response {
        // 保留完整状态对象。
        LongOperationClientResponse::Success(operation) => {
            // 状态必须保持 accepted。
            assert_eq!(operation["status"], "accepted");
            // expiresAt 必须保持 null。
            assert_eq!(operation["expiresAt"], serde_json::Value::Null);
        }
        // 不能误判为业务拒绝。
        LongOperationClientResponse::Rejected { .. } => panic!("success became rejection"),
    }
}

// 验证业务拒绝保留稳定错误但不伪造 operation。
#[test]
fn parses_strict_business_rejection() {
    // 构造零命中拒绝 frame。
    let frame = json!({
        // 固定 broker 版本。
        "contractVersion": CONTRACT_VERSION,
        // 绑定本次请求。
        "requestNonce": REQUEST_NONCE,
        // transport 已接受。
        "transportAccepted": true,
        // 业务未接受。
        "businessAccepted": false,
        // 保存稳定错误。
        "error": {
            // 使用 schema 登记码。
            "code": "OPERATION_NOT_FOUND",
            // 使用安全消息。
            "message": "The long operation is unavailable in this broker generation."
        }
    })
    // 转换为受控 JSON 文本。
    .to_string();
    // 严格解析拒绝响应。
    let response = parse_response(&frame, REQUEST_NONCE)
        // fixture 必须通过。
        .unwrap_or_else(|_| panic!("strict rejection should parse"));
    // 验证错误码未漂移。
    match response {
        // 保存业务拒绝。
        LongOperationClientResponse::Rejected { code, .. } => {
            // 错误码必须精确。
            assert_eq!(code, "OPERATION_NOT_FOUND");
        }
        // 不能误判为成功。
        LongOperationClientResponse::Success(_) => panic!("rejection became success"),
    }
}

// 验证错误 nonce、未知字段与矛盾 payload 全部失败闭合。
#[test]
fn rejects_unbound_or_conflicting_response() {
    // 构造携带错误 nonce 和额外 operation 的拒绝。
    let frame = json!({
        // 固定 broker 版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用另一请求的 nonce。
        "requestNonce": "11111111111111111111111111111111",
        // transport 已接受。
        "transportAccepted": true,
        // 声称业务拒绝。
        "businessAccepted": false,
        // 保存稳定错误。
        "error": {"code": "OPERATION_NOT_FOUND", "message": "Unavailable."},
        // 注入未知顶层字段。
        "nativePid": 42
    })
    // 转换为受控 JSON 文本。
    .to_string();
    // 任一漂移都必须失败。
    assert_eq!(
        parse_response(&frame, REQUEST_NONCE).err(),
        Some(LongOperationResponseFailure)
    );
}
