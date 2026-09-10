// 导入标准错误 trait 供测试返回类型使用。
use std::error::Error;

// 导入 session.inspect request 与 operation 契约。
use super::*;
// 导入 response 的 outcome 与 success 投影。
use super::response::*;
// 导入 live broker epoch 投影。
use super::state::BrowserSessionBrokerEpoch;
// 导入 JSON 构造与不可信值类型。
use serde_json::{Value, json};

// 固定测试 broker epoch。
const EPOCH: &str = "11111111111111111111111111111111";
// 固定测试 session identity。
const SESSION: &str = "s2:bs:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
// 固定独立测试 request nonce。
const NONCE: &str = "44444444444444444444444444444444";

// 解析合法 session.inspect Query。
fn session_inspect_request() -> Result<BrowserSessionBrokerRequest, Box<dyn Error>> {
    // 构造包含 operation 与 session target 的完整 canonical key。
    let key = format!(
        // 长度前缀与生产 parser 同源。
        "{}|32:{}|15:session.inspect|38:{}",
        // 写入协议版本。
        CONTRACT_VERSION,
        // 写入 live epoch。
        EPOCH,
        // 写入 session target。
        SESSION
    );
    // 构造不携带 confirmed 的严格 Query frame。
    let value = json!({
        // 固定 request kind。
        "kind": "request",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用独立 canonical nonce。
        "requestNonce": NONCE,
        // 写入正确 semantic fingerprint。
        "semanticFingerprint": fingerprint_for_canonical_key(&key),
        // 绑定当前 broker epoch。
        "expectedBrokerEpoch": EPOCH,
        // 使用有效剩余预算。
        "remainingTimeoutMs": 100,
        // 选择 session.inspect Query。
        "operation": "session.inspect",
        // 绑定公开 session target。
        "sessionId": SESSION
    });
    // 返回严格 session.inspect request。
    Ok(BrowserSessionBrokerRequest::parse_for_epoch(
        // 序列化测试 frame。
        &value.to_string(),
        // 绑定当前 epoch。
        EPOCH,
    )?)
}

// 验证 session.inspect 的 Query 形状、指纹与两阶段响应绑定。
#[test]
fn session_inspect_is_query_and_requires_accepted_before_completed() -> Result<(), Box<dyn Error>> {
    // 解析不含 confirmed 的合法 Query。
    let request = session_inspect_request()?;
    // operation 必须为新冻结标签。
    assert_eq!(
        request.operation(),
        BrowserSessionBrokerOperation::SessionInspect
    );
    // Query 绝不声明 target 可能变化。
    assert!(!request.may_mutate_target());
    // wire 指纹必须与 parser 同源计算。
    assert_eq!(
        request.semantic_fingerprint(),
        request.computed_semantic_fingerprint()
    );
    // 构造与合法 Query 逐字段相同的不可信 JSON。
    let mut confirmed = serde_json::from_value::<Value>(json!({
        // 固定 request kind。
        "kind": "request",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 保持相同 request nonce。
        "requestNonce": NONCE,
        // 保持已验证的 fingerprint。
        "semanticFingerprint": request.semantic_fingerprint(),
        // 保持同一 broker epoch。
        "expectedBrokerEpoch": EPOCH,
        // 保持有效剩余预算。
        "remainingTimeoutMs": 100,
        // 固定 session.inspect Query。
        "operation": "session.inspect",
        // 保持原 session target。
        "sessionId": SESSION
    }))?;
    // 故意加入 Query 禁止的确认字段。
    confirmed["confirmed"] = json!(true);
    // 严格 parser 必须拒绝。
    assert!(BrowserSessionBrokerRequest::parse_for_epoch(&confirmed.to_string(), EPOCH).is_err());
    // 建立 live epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())?;
    // 首个业务响应必须是 accepted revision 零。
    let accepted = BrowserSessionBrokerResponse::accepted(&request, 0, &epoch);
    // Query accepted 不得标记 mutation。
    assert!(!accepted.target_may_have_mutated());
    // 构造与原 session 逐字绑定的完成结果。
    let completed = BrowserSessionBrokerResponse::finished(
        // 绑定原 Query。
        &request,
        // accepted 后严格推进到 revision 一。
        1,
        // 绑定当前 epoch。
        &epoch,
        // 固定可信完成。
        BrowserSessionBrokerOutcome::Completed,
        // 只返回原 session 与 live=true。
        Some(BrowserSessionBrokerSuccess::SessionInspect {
            // 回显原公开 session identity。
            session_id: SESSION.to_owned(),
            // 成功事实固定为 live。
            live: true,
        }),
    );
    // 合法绑定必须可构造。
    assert!(completed.is_some());
    // 绑定其他 session 必须被 response validator 拒绝。
    assert!(
        BrowserSessionBrokerResponse::finished(
            // 绑定原 Query。
            &request,
            // 使用业务终态 revision。
            1,
            // 绑定当前 epoch。
            &epoch,
            // 固定可信完成。
            BrowserSessionBrokerOutcome::Completed,
            // 故意回显另一 session identity。
            Some(BrowserSessionBrokerSuccess::SessionInspect {
                // 使用不同但 canonical 的 target。
                session_id: "s2:bs:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
                // 仍声明 live 以隔离 identity 错配。
                live: true,
            }),
        )
        // 错配必须拒绝。
        .is_none()
    );
    // 结束测试。
    Ok(())
}
