//! 构造 browser-session broker v1 的 session.inspect query frame。

// 导入 JSON 构造宏。
use serde_json::json;

// 导入父 wire Component 的 request frame 与指纹 helper。
use super::{BrowserSessionBrokerRequestFrame, request_fingerprint};
// 导入协议失败与固定版本。
use super::super::{BrowserSessionBrokerProtocolFailure, CONTRACT_VERSION};

// 为严格 request frame 扩展只读会话查询。
impl BrowserSessionBrokerRequestFrame {
    // 构造只读 session.inspect query。
    pub(crate) fn session_inspect(
        // 接收 canonical request nonce。
        request_nonce: &str,
        // 接收当前 broker epoch。
        expected_broker_epoch: &str,
        // 接收剩余 deadline 预算。
        remaining_timeout_ms: u32,
        // 接收唯一公开 session target。
        session_id: &str,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // session.inspect canonical 语义包含 epoch、operation 与 opaque session。
        let fingerprint = request_fingerprint(
            // 绑定 live broker epoch。
            expected_broker_epoch,
            // 固定 query operation。
            "session.inspect",
            // 绑定公开 session target。
            Some(session_id),
        );
        // 构造不含 confirmed 的精确 query 字段集合。
        let value = json!({
            // 固定 request frame kind。
            "kind": "request",
            // 固定协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 保存 request nonce。
            "requestNonce": request_nonce,
            // 保存同源 semantic fingerprint。
            "semanticFingerprint": fingerprint,
            // 绑定 expected broker epoch。
            "expectedBrokerEpoch": expected_broker_epoch,
            // 保存剩余 deadline 预算。
            "remainingTimeoutMs": remaining_timeout_ms,
            // 固定 session.inspect operation。
            "operation": "session.inspect",
            // 保存唯一公开 session target。
            "sessionId": session_id
        });
        // 使用同源 parser 拒绝非 s2:bs target 或额外确认字段。
        Self::from_value(value, expected_broker_epoch)
    }
}
