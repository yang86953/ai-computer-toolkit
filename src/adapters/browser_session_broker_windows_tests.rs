//! 验证 Browser Session Broker Adapter 的 request 构造与安全错误语义。

// 导入当前 Adapter 的私有 request builder、错误投影与封闭交换类型。
use super::{BrowserSessionExchange, build_request_frame, post_dispatch_outcome_unknown};
// 导入封闭 Broker operation 供 Query 形状断言。
use crate::components::browser_session_broker_protocol::BrowserSessionBrokerOperation;

// 使用符合冻结 nonce 形状的固定 request nonce。
const REQUEST_NONCE: &str = "0123456789abcdef0123456789abcdef";
// 使用符合冻结 nonce 形状的固定 ready epoch。
const BROKER_EPOCH: &str = "fedcba9876543210fedcba9876543210";
// 使用 canonical browser-session identity。
const SESSION_ID: &str = "s2:bs:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

// 验证零字节回压后的重建缩短 wire 预算而保持相同 ledger identity。
#[test]
fn backpressure_rebuilds_shorter_remaining_without_changing_nonce_or_fingerprint() {
    // 构造首次尚未尝试 WriteFile 的 confirmed open frame。
    let first = build_request_frame(
        // open 没有公开 target。
        &BrowserSessionExchange::OpenConfirmed,
        // 保持一次交换唯一的 secure nonce。
        REQUEST_NONCE,
        // 保持首次认证的 epoch。
        BROKER_EPOCH,
        // 使用首次较长的剩余预算。
        500,
    )
    // 固定输入必须满足生产 strict builder。
    .expect("first browser frame should be strict");
    // 构造同一次零字节回压后的下一次 WriteFile frame。
    let retried = build_request_frame(
        // 保持相同字段封闭 operation。
        &BrowserSessionExchange::OpenConfirmed,
        // 绝不为回压生成新 nonce。
        REQUEST_NONCE,
        // epoch 不变时才允许重建。
        BROKER_EPOCH,
        // 使用严格更短的当前总 deadline 剩余预算。
        499,
    )
    // 固定输入必须满足生产 strict builder。
    .expect("retried browser frame should be strict");
    // 重建后的 wire 预算必须真实缩短。
    assert_eq!(retried.request().remaining_timeout_ms(), 499);
    // 首帧预算作为缩短比较基线。
    assert_eq!(first.request().remaining_timeout_ms(), 500);
    // 重建不得改变 broker ledger 的 request nonce。
    assert_eq!(
        retried.request().request_nonce(),
        first.request().request_nonce()
    );
    // remainingTimeoutMs 不得进入 canonical semantic fingerprint。
    assert_eq!(
        retried.request().semantic_fingerprint(),
        first.request().semantic_fingerprint()
    );
    // wire 文本必须改变，以保证 server 能观察到缩短后的预算。
    assert_ne!(retried.text(), first.text());
}

// 验证 Query frame 与投递后安全错误都使用中性的 request 语义。
#[test]
fn session_inspect_uses_query_shape_and_neutral_request_error() {
    // 构造不含 confirmed 的只读 Query frame。
    let frame = build_request_frame(
        // 选择 session.inspect Query。
        &BrowserSessionExchange::SessionInspect(SESSION_ID),
        // 绑定唯一 request nonce。
        REQUEST_NONCE,
        // 绑定当前 broker epoch。
        BROKER_EPOCH,
        // 使用有效剩余预算。
        500,
    )
    // 固定输入必须通过严格 builder。
    .expect("session inspect frame should be strict");
    // Query operation 必须准确投影为 session.inspect。
    assert_eq!(
        frame.request().operation(),
        BrowserSessionBrokerOperation::SessionInspect
    );
    // Query 不得携带 Command confirmation 字段。
    assert!(!frame.text().contains("confirmed"));
    // 取得共享投递后未知结果投影。
    let error = post_dispatch_outcome_unknown(
        // 绑定原 session.inspect Query 角色。
        &BrowserSessionExchange::SessionInspect(SESSION_ID),
    );
    // 错误消息必须适用于 Command 与 Query。
    assert!(error.message.contains("request"));
    // 不得再把 Query 误称为 Command。
    assert!(!error.message.contains("command"));
    // 写后恢复失败必须保留业务可能已接受的安全事实。
    assert_eq!(error.details["businessAccepted"], true);
    // 未取得可信 final 时不得声称完成。
    assert_eq!(error.details["completed"], false);
    // 投递风险存在时不得允许安全重试。
    assert_eq!(error.details["retrySafe"], false);
    // Query 即使写后未知也不得声称目标可能变化。
    assert_eq!(error.details["targetMayHaveMutated"], false);
    // 私有 transport 身份不得进入安全详情。
    assert!(error.details.get("brokerEpoch").is_none());
}
