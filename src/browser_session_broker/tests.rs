//! 验证 browser-session broker 宿主的连接级 snapshot 门禁。

// 导入 strict request builder、accepted response 与 epoch。
use crate::components::browser_session_broker_protocol::{
    // 导入 accepted response。
    response::BrowserSessionBrokerResponse,
    // 导入 canonical epoch。
    state::BrowserSessionBrokerEpoch,
    // 导入 open builder。
    wire::BrowserSessionBrokerRequestFrame,
};

// 导入被测 revision helper。
use super::response_is_newer;

// 验证全局无关唤醒不会使同一 accepted revision 重发。
#[test]
fn unchanged_accepted_snapshot_is_not_sent_twice() {
    // 构造固定 canonical epoch。
    let epoch = BrowserSessionBrokerEpoch::new(
        // 使用 32 位小写十六进制。
        "77777777777777777777777777777777".to_owned(),
    )
    // 固定测试 epoch 必须合法。
    .unwrap_or_else(|error| panic!("epoch must parse: {error}"));
    // 构造 strict open request。
    let frame = BrowserSessionBrokerRequestFrame::open_confirmed(
        // 使用固定 request nonce。
        "88888888888888888888888888888888",
        // 绑定同一 live epoch。
        epoch.as_str(),
        // 使用合法剩余预算。
        10_000,
    )
    // builder 必须通过生产 parser。
    .unwrap_or_else(|error| panic!("request must parse: {error}"));
    // 构造 revision 零 accepted snapshot。
    let accepted = BrowserSessionBrokerResponse::accepted(
        // 绑定原 request。
        frame.request(),
        // accepted 固定 revision 零。
        0,
        // 回显 live epoch。
        &epoch,
    );
    // 首次观察必须发送 accepted。
    assert!(response_is_newer(None, &accepted));
    // 任意无关全局 generation 唤醒后同 revision 不得再次发送。
    assert!(!response_is_newer(Some(0), &accepted));
}
