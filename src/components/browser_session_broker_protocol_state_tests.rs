// 导入被测状态组件的私有实现。
use super::*;
// 导入版本化协议常量。
use super::super::CONTRACT_VERSION;
// 导入 request-bound rejection 所需的稳定错误类型。
use super::super::{BrowserSessionBrokerProtocolErrorCode, BrowserSessionBrokerProtocolFailure};
// 导入封闭响应、outcome 与成功投影。
use super::super::response::{
    BrowserSessionBrokerOutcome, BrowserSessionBrokerResponse, BrowserSessionBrokerSuccess,
};
// 导入 JSON 测试构造器。
use serde_json::json;

// 固定合法 request nonce。
const REQUEST_NONCE: &str = "0123456789abcdef0123456789abcdef";
// 固定合法 cancel nonce。
const CANCEL_NONCE: &str = "33333333333333333333333333333333";
// 固定另一条合法 cancel nonce。
const SECOND_CANCEL_NONCE: &str = "44444444444444444444444444444444";
// 固定合法 broker epoch。
const EPOCH: &str = "11111111111111111111111111111111";
// 固定合法 open 成功 session identity。
const SESSION: &str = "s2:bs:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
// 固定同 nonce close response 使用的另一 session identity。
const OTHER_SESSION: &str = "s2:bs:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

// 计算与 request parser 同源的 FNV-1a 语义摘要。
fn fingerprint(key: &str) -> String {
    // 初始化固定 FNV offset basis。
    let mut hash = 0xcbf29ce484222325_u64;
    // 逐字节混入 canonical semantic key。
    for byte in key.bytes() {
        // 执行 FNV-1a 单步更新。
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    // 编码固定十六进制摘要。
    format!("{hash:016x}")
}

// 构造供状态和连接测试共享的严格 open request。
fn request() -> BrowserSessionBrokerRequest {
    // 构造 open request 的 canonical semantic key。
    let key = format!("{}|32:{}|4:open", CONTRACT_VERSION, EPOCH);
    // 构造已确认 command 的严格 JSON frame。
    let text = json!({
        // 标记业务 request。
        "kind": "request",
        // 绑定版本化协议。
        "contractVersion": CONTRACT_VERSION,
        // 写入 canonical request nonce。
        "requestNonce": REQUEST_NONCE,
        // 写入 parser 可验证的语义摘要。
        "semanticFingerprint": fingerprint(&key),
        // 绑定当前 live epoch。
        "expectedBrokerEpoch": EPOCH,
        // 提供有效执行预算。
        "remainingTimeoutMs": 100,
        // 选择 open command。
        "operation": "open",
        // 显式确认 mutation command。
        "confirmed": true
    })
    // 序列化为 parser 输入。
    .to_string();
    // 解析并返回严格 request。
    BrowserSessionBrokerRequest::parse_for_epoch(&text, EPOCH)
        // 测试输入必须被接受。
        .expect("request must parse")
}

// 构造复用固定 nonce 与 epoch、但可选择 session 的严格 close request。
fn close_request(
    // 借用本次 close 的 canonical session identity。
    session_id: &str,
) -> BrowserSessionBrokerRequest {
    // 构造包含 session target 的完整 canonical semantic key。
    let key = format!(
        // 使用与生产 parser 相同的长度前缀字段顺序。
        "{}|32:{}|5:close|{}:{}",
        // 写入协议版本。
        CONTRACT_VERSION,
        // 写入当前 broker epoch。
        EPOCH,
        // 写入 session 的 UTF-8 字节长度。
        session_id.len(),
        // 写入未经归一化的 session identity。
        session_id,
    );
    // 构造已确认 close command 的严格 JSON frame。
    let text = json!({
        // 标记业务 request。
        "kind": "request",
        // 绑定版本化协议。
        "contractVersion": CONTRACT_VERSION,
        // 故意让两份 close 复用同一个 request nonce。
        "requestNonce": REQUEST_NONCE,
        // 写入各自完整语义对应的摘要。
        "semanticFingerprint": fingerprint(&key),
        // 绑定同一个 live broker epoch。
        "expectedBrokerEpoch": EPOCH,
        // 提供有效执行预算。
        "remainingTimeoutMs": 100,
        // 两份 request 都选择 close operation。
        "operation": "close",
        // 显式确认 mutation command。
        "confirmed": true,
        // 仅 session target 在两份 request 间不同。
        "sessionId": session_id
    })
    // 序列化为 parser 输入。
    .to_string();
    // 解析并返回严格 close request。
    BrowserSessionBrokerRequest::parse_for_epoch(&text, EPOCH)
        // 测试输入必须被接受。
        .expect("close request must parse")
}

// 构造指定 revision 的合法 open completed final。
fn completed(
    // 借用已解析的严格 request。
    request: &BrowserSessionBrokerRequest,
    // 借用当前 live epoch。
    epoch: &BrowserSessionBrokerEpoch,
    // 接收目标 response revision。
    revision: u64,
) -> BrowserSessionBrokerResponse {
    // 从 request 构造封闭的业务完成终态。
    BrowserSessionBrokerResponse::finished(
        // 绑定唯一 request。
        request,
        // 写入调用方指定的 revision。
        revision,
        // 绑定当前 live epoch。
        epoch,
        // 建立 completed outcome。
        BrowserSessionBrokerOutcome::Completed,
        // 提供 open 专属的公开 session identity。
        Some(BrowserSessionBrokerSuccess::Open {
            // 写入 canonical session identity。
            session_id: SESSION.to_owned(),
        }),
    )
    // response shape 必须合法。
    .expect("completed response must be valid")
}

// 构造关联指定 nonce 的严格 cancel request。
fn cancel_request(cancel_nonce: &str) -> BrowserSessionBrokerCancellationRequest {
    // 构造严格控制 cancel frame。
    let text = json!({
        // 标记控制 cancel frame。
        "kind": "cancel",
        // 绑定版本化协议。
        "contractVersion": CONTRACT_VERSION,
        // 提供调用方指定的 cancel nonce。
        "cancelRequestNonce": cancel_nonce,
        // 关联固定的已接受 request。
        "requestNonce": REQUEST_NONCE,
        // 绑定当前 broker epoch。
        "expectedBrokerEpoch": EPOCH
    })
    // 序列化为 parser 输入。
    .to_string();
    // 解析并返回严格 cancel request。
    BrowserSessionBrokerCancellationRequest::parse(&text)
        // 测试输入必须被接受。
        .expect("cancel request must parse")
}

// 建立已绑定唯一 request 的 transport-accepted 连接。
fn transport_connection(request: &BrowserSessionBrokerRequest) -> BrowserSessionBrokerConnection {
    // 构造空连接状态。
    let mut connection = BrowserSessionBrokerConnection::default();
    // 绑定唯一 transport request。
    connection
        // 推进到 transport-accepted。
        .accept_transport(request)
        // 严格 request 必须被接受。
        .expect("transport must accept request");
    // 返回已绑定的连接。
    connection
}

// 验证连接只接受与阶段严格连续的 response revision。
#[test]
fn connection_rejects_non_contiguous_response_revisions() {
    // 构造当前 live epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned()).expect("epoch must be valid");
    // 构造严格 request。
    let request = request();
    // accepted 首次 revision 非零必须拒绝。
    assert!(
        transport_connection(&request)
            // 应用伪造的 accepted revision 九九九。
            .apply_response(&BrowserSessionBrokerResponse::accepted(
                &request, 999, &epoch
            ))
            // 阶段 gate 必须失败闭合。
            .is_err()
    );
    // 构造业务前到期终态。
    let preaccept_7 = BrowserSessionBrokerResponse::expired_before_acceptance(&request, 7, &epoch)
        // 业务前终态 shape 必须合法。
        .expect("preaccept response must be valid");
    // 业务前终态首次 revision 非零必须拒绝。
    assert!(
        transport_connection(&request)
            // 应用伪造的业务前 revision 七。
            .apply_response(&preaccept_7)
            // 阶段 gate 必须失败闭合。
            .is_err()
    );
    // 直接业务终态 revision 二不得作为跳过 accepted 的 replay。
    assert!(
        transport_connection(&request)
            // 应用不允许的 completed revision 二。
            .apply_response(&completed(&request, &epoch, 2))
            // 阶段 gate 必须失败闭合。
            .is_err()
    );
    // 建立已通过 accepted 的连接。
    let mut accepted_connection = transport_connection(&request);
    // accepted 的首个 revision 零必须通过。
    accepted_connection
        // 应用唯一允许的首个 accepted。
        .apply_response(&BrowserSessionBrokerResponse::accepted(&request, 0, &epoch))
        // accepted revision 零必须通过。
        .expect("accepted revision zero must pass");
    // accepted 后跳到 revision 九必须拒绝。
    assert!(
        accepted_connection
            // 应用不连续的 completed final。
            .apply_response(&completed(&request, &epoch, 9))
            // 阶段 gate 必须失败闭合。
            .is_err()
    );
    // 建立另一条已通过 accepted 的连接。
    let mut valid_connection = transport_connection(&request);
    // accepted 的首个 revision 零必须通过。
    valid_connection
        // 应用唯一允许的首个 accepted。
        .apply_response(&BrowserSessionBrokerResponse::accepted(&request, 0, &epoch))
        // accepted revision 零必须通过。
        .expect("accepted revision zero must pass");
    // accepted 后的连续 revision 一终态必须通过。
    valid_connection
        // 应用唯一允许的 completed final。
        .apply_response(&completed(&request, &epoch, 1))
        // completed revision 一必须通过。
        .expect("completed revision one must pass");
    // 合法终态必须建立 final 阶段。
    assert_eq!(
        // 读取最终连接阶段。
        valid_connection.phase(),
        // 核对 final-wins 阶段。
        BrowserSessionBrokerConnectionPhase::Final
    );
}

// 验证 cancel revision 溢出不会留下已保存的业务终态。
#[test]
fn terminal_cancel_revision_overflow_keeps_execution_unwritten() {
    // 构造当前 live epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned()).expect("epoch must be valid");
    // 建立空的统一状态边界。
    let mut state = BrowserSessionBrokerEpochState::default();
    // 构造严格 request。
    let request = request();
    // 首次 request 必须建立 execution record。
    state
        // 在线性化点接受首次 request。
        .observe_request(&epoch, 1_000, &request)
        // request 必须可 dispatch。
        .expect("request must dispatch");
    // 建立业务接受，令取消可进入协作状态。
    state
        // 保存封闭 accepted 响应。
        .apply_response(
            &epoch,
            BrowserSessionBrokerResponse::accepted(&request, 0, &epoch),
        )
        // accepted 必须可保存。
        .expect("accepted response must save");
    // 构造关联该 request 的 cancel frame。
    let cancel = cancel_request(CANCEL_NONCE);
    // 线性化协作取消并保留首个 receipt。
    let receipt = state
        // 登记 accepted target 的取消请求。
        .observe_cancel(&epoch, &cancel)
        // cancel 必须进入 cancellation-requested。
        .expect("cancel must be requested");
    // 确认被溢出污染的是仍待结算的协作取消。
    assert_eq!(
        // 核对首个 cancel receipt。
        receipt,
        // 首次协作取消必须从零 revision 建立。
        (BrowserSessionBrokerCancelStatus::CancellationRequested, 0)
    );
    // 制造唯一可能阻止批量结算的 revision 溢出。
    state
        // 使用仅测试编译的私有注入 helper。
        .cancellations
        // 将仍待结算 cancel 的 revision 置为最大值。
        .set_pending_revision_for_test(CANCEL_NONCE, u64::MAX)
        // 测试前置条件必须允许注入。
        .expect("pending cancel revision must be injectable");
    // 构造紧随 accepted 的合法业务 completed final。
    let terminal = completed(&request, &epoch, 1);
    // revision 预检必须在 execution 写入前失败。
    assert!(state.apply_response(&epoch, terminal).is_err());
    // 原 accepted snapshot 必须仍是唯一已保存响应。
    let snapshot = state
        // 读取测试专用 snapshot 投影。
        .execution_response_for_test(REQUEST_NONCE)
        // accepted snapshot 必须仍存在。
        .expect("accepted snapshot must remain");
    // 保存的 revision 不得从 accepted 的零推进。
    assert_eq!(snapshot.request_revision(), 0);
    // 保存的 snapshot 不得被业务终态覆盖。
    assert_eq!(snapshot.outcome(), None);
}

// 验证同一 target 的全部协作取消会由业务终态同时自动结算。
#[test]
fn terminal_automatically_finalizes_every_pending_cancel_for_target() {
    // 构造当前 live epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned()).expect("epoch must be valid");
    // 建立空的统一状态边界。
    let mut state = BrowserSessionBrokerEpochState::default();
    // 构造严格 request。
    let request = request();
    // 首次 request 必须建立 execution record。
    state
        // 在线性化点接受首次 request。
        .observe_request(&epoch, 1_000, &request)
        // request 必须可 dispatch。
        .expect("request must dispatch");
    // 保存业务 accepted revision 零。
    state
        // 应用首个 accepted response。
        .apply_response(
            &epoch,
            BrowserSessionBrokerResponse::accepted(&request, 0, &epoch),
        )
        // accepted 必须可保存。
        .expect("accepted response must save");
    // 构造第一条协作 cancel。
    let first_cancel = cancel_request(CANCEL_NONCE);
    // 构造第二条指向相同 target 的协作 cancel。
    let second_cancel = cancel_request(SECOND_CANCEL_NONCE);
    // 第一条 cancel 必须进入 pending 状态。
    assert_eq!(
        // 在线性化点登记第一条 cancel。
        state
            .observe_cancel(&epoch, &first_cancel)
            .expect("first cancel must register"),
        // 首次 receipt 从 revision 零建立。
        (BrowserSessionBrokerCancelStatus::CancellationRequested, 0)
    );
    // 第二条不同 nonce 的 cancel 也必须独立进入 pending 状态。
    assert_eq!(
        // 在线性化点登记第二条 cancel。
        state
            .observe_cancel(&epoch, &second_cancel)
            .expect("second cancel must register"),
        // 第二条 receipt 同样从 revision 零建立。
        (BrowserSessionBrokerCancelStatus::CancellationRequested, 0)
    );
    // 保存同一 target 的非取消业务终态。
    state
        // 业务 completed 必须自动结算所有 pending cancel。
        .apply_response(&epoch, completed(&request, &epoch, 1))
        // 终态必须可保存。
        .expect("completed response must save");
    // 第一条 duplicate cancel 必须重放自动结算的 too-late revision 一。
    assert_eq!(
        // 重送第一条 cancel 不得再次触碰 target。
        state
            .observe_cancel(&epoch, &first_cancel)
            .expect("first receipt must replay"),
        // 业务终态已建立，因此取消太晚。
        (BrowserSessionBrokerCancelStatus::TooLate, 1)
    );
    // 第二条 duplicate cancel 也必须重放自动结算的 too-late revision 一。
    assert_eq!(
        // 重送第二条 cancel 不得再次触碰 target。
        state
            .observe_cancel(&epoch, &second_cancel)
            .expect("second receipt must replay"),
        // 每个同 target cancel 都必须被独立推进。
        (BrowserSessionBrokerCancelStatus::TooLate, 1)
    );
}

// 验证同 nonce、operation 与 epoch 的 response 仍须匹配首个 close session 语义。
#[test]
fn execution_ledger_rejects_response_from_different_close_session() {
    // 构造当前 live epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned()).expect("epoch must be valid");
    // 构造 ledger 首次保存的 close request。
    let recorded_request = close_request(SESSION);
    // 构造仅 session 不同的同 nonce close request。
    let different_request = close_request(OTHER_SESSION);
    // 建立空的 epoch 状态边界。
    let mut state = BrowserSessionBrokerEpochState::default();
    // 首次 close request 必须建立唯一 execution record。
    state
        // 在线性化点保存首个完整 request 语义。
        .observe_request(&epoch, 1_000, &recorded_request)
        // 合法首个 request 必须可以 dispatch。
        .expect("recorded close request must dispatch");
    // 使用另一 session 的严格 request 构造形状合法的 accepted response。
    let different_response = BrowserSessionBrokerResponse::accepted(
        // 绑定同 nonce、operation、epoch 但不同完整语义的 request。
        &different_request,
        // accepted 仍使用首个 revision 零。
        0,
        // 回显相同 broker epoch。
        &epoch,
    );
    // execution ledger 必须拒绝未绑定首个完整 canonical 语义的响应。
    assert!(state.apply_response(&epoch, different_response).is_err());
    // 拒绝后不得给首条 record 安装任何 snapshot。
    assert!(state.execution_response_for_test(REQUEST_NONCE).is_none());
}

// 验证严格 request-bound 业务前拒绝可保存重放且不会串入异义 request。
#[test]
fn execution_ledger_applies_only_request_bound_rejection() {
    // 构造当前 live epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned()).expect("epoch must be valid");
    // 构造 ledger 首次保存的 close request。
    let recorded_request = close_request(SESSION);
    // 构造同 nonce、operation、epoch 但 session 不同的 request。
    let different_request = close_request(OTHER_SESSION);
    // 建立空的 epoch 状态边界。
    let mut state = BrowserSessionBrokerEpochState::default();
    // 首次 request 必须建立唯一 execution record。
    state
        // 在线性化点保存完整 canonical request 语义。
        .observe_request(&epoch, 1_000, &recorded_request)
        // 合法首个 request 必须可以 dispatch。
        .expect("recorded close request must dispatch");
    // 为另一 session 构造语义自洽的 stale-session 失败。
    let different_failure = BrowserSessionBrokerProtocolFailure::rejected_for_request(
        // 使用 close 领域预检允许的稳定错误码。
        BrowserSessionBrokerProtocolErrorCode::StaleSession,
        // 绑定不同 session 的严格 request。
        &different_request,
    );
    // 将该失败投影为完整 request-bound rejection。
    let different_response = BrowserSessionBrokerResponse::rejected_for_request(
        // failure 与不同 request 的 nonce/operation 相符。
        &different_failure,
        // 保存不同 session 对应的完整 canonical key。
        &different_request,
        // 业务前终态固定从 revision 零建立。
        0,
        // 回显当前 epoch。
        &epoch,
    )
    // 构造器应接受内部自洽的 response。
    .expect("different request rejection must be representable");
    // ledger 必须按首个 record 的完整语义拒绝另一 session response。
    assert!(state.apply_response(&epoch, different_response).is_err());
    // 语义串线失败后不得安装 snapshot。
    assert!(state.execution_response_for_test(REQUEST_NONCE).is_none());
    // 为首个 record 构造 request-bound stale-session 失败。
    let recorded_failure = BrowserSessionBrokerProtocolFailure::rejected_for_request(
        // 使用相同稳定领域预检错误码。
        BrowserSessionBrokerProtocolErrorCode::StaleSession,
        // 绑定首个完整严格 request。
        &recorded_request,
    );
    // failure 的 operation 与另一个 open request 不匹配时构造器必须拒绝。
    assert!(
        BrowserSessionBrokerResponse::rejected_for_request(
            // 传入 close failure。
            &recorded_failure,
            // 故意绑定同 nonce 但 operation 不同的 open request。
            &request(),
            // 使用首个 revision。
            0,
            // 回显当前 epoch。
            &epoch,
        )
        // nonce/operation 关联门禁必须失败。
        .is_none()
    );
    // 构造与首个 record 完整语义一致的 rejection。
    let recorded_response = BrowserSessionBrokerResponse::rejected_for_request(
        // 使用与首个 request 关联的 failure。
        &recorded_failure,
        // 保存首个 request 的完整 canonical key。
        &recorded_request,
        // 业务前终态固定 revision 零。
        0,
        // 回显当前 epoch。
        &epoch,
    )
    // 完整关联的 rejection 必须可构造。
    .expect("recorded request rejection must be representable");
    // request-bound rejection 必须成为可信业务前 snapshot。
    state
        // 在线性化点应用终态。
        .apply_response(&epoch, recorded_response)
        // 完整语义一致时必须接受。
        .expect("request-bound rejection must apply");
    // 同义 request 重送必须 replay 已保存 rejection。
    let replay = state
        // 再次观察完全相同的严格 request。
        .observe_request(&epoch, 1_001, &recorded_request)
        // terminal record 必须可读取。
        .expect("terminal request must replay");
    // 决定必须是携带原 rejection 的 replay。
    let BrowserSessionBrokerExecutionDecision::Replay { response } = replay else {
        // attach 或 dispatch 都会破坏 terminal-wins。
        panic!("request-bound rejection must replay");
    };
    // replay 必须保持 rejected outcome 与稳定错误码。
    assert_eq!(
        // 读取封闭 outcome 和错误码。
        (response.outcome(), response.error_code()),
        // 核对首次保存的业务前拒绝事实。
        (
            Some(BrowserSessionBrokerOutcome::Rejected),
            Some("STALE_SESSION")
        )
    );
}
