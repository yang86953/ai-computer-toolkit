//! 验证 browser-session broker wire 的严格关联与封闭编解码。

// 导入标准错误 trait。
use std::error::Error;

// 导入 JSON 构造与读取 helper。
use serde_json::{Value, json};

// 导入当前 wire API。
use super::{
    // 导入 request/cancel frame builder。
    BrowserSessionBrokerCancelFrame,
    BrowserSessionBrokerRequestFrame,
    // 导入 control 与 response 解码入口。
    decode_cancel_receipt,
    decode_cancel_rejected,
    decode_ready,
    decode_response,
    // 导入 control 与 response 编码入口。
    encode_cancel_receipt,
    encode_cancel_rejected,
    encode_ready,
    encode_response,
};
// 导入响应类型。
use super::super::response::{
    // 导入 cancel response 投影。
    BrowserSessionBrokerCancelReceipt,
    BrowserSessionBrokerCancelRejected,
    // 导入 request outcome/response/success 投影。
    BrowserSessionBrokerOutcome,
    BrowserSessionBrokerResponse,
    BrowserSessionBrokerSuccess,
};
// 导入状态与 epoch 类型。
use super::super::state::{
    // 导入 cancel status 与连接状态机。
    BrowserSessionBrokerCancelStatus,
    BrowserSessionBrokerConnection,
    // 导入连接 phase 与 broker epoch。
    BrowserSessionBrokerConnectionPhase,
    BrowserSessionBrokerEpoch,
};
// 导入协议错误码与 operation。
use super::super::{
    // 导入 operation 与协议错误码。
    BrowserSessionBrokerOperation,
    BrowserSessionBrokerProtocolErrorCode,
    // 导入可关联协议 failure。
    BrowserSessionBrokerProtocolFailure,
    // 导入两阶段严格 request parser。
    BrowserSessionBrokerRequest,
};

// 固定测试 request nonce。
const REQUEST_NONCE: &str = "0123456789abcdef0123456789abcdef";
// 固定测试 cancel nonce。
const CANCEL_NONCE: &str = "11111111111111111111111111111111";
// 固定另一 cancel nonce。
const FOREIGN_CANCEL_NONCE: &str = "22222222222222222222222222222222";
// 固定测试 epoch。
const EPOCH: &str = "abcdef0123456789abcdef0123456789";
// 固定另一 epoch。
const FOREIGN_EPOCH: &str = "99999999999999999999999999999999";
// 固定第三个 canonical 但未认证的 epoch。
const THIRD_EPOCH: &str = "88888888888888888888888888888888";
// 固定公开 session ID。
const SESSION_ID: &str = "s2:bs:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

// 验证 confirmed-only open/close builder 与 ready 往返。
#[test]
fn builders_and_ready_are_strict() -> Result<(), Box<dyn Error>> {
    // 构造 confirmed open。
    let open = BrowserSessionBrokerRequestFrame::open_confirmed(REQUEST_NONCE, EPOCH, 500)?;
    // 读取 open JSON。
    let open_value = serde_json::from_str::<Value>(open.text())?;
    // builder 必须显式发送 confirmed=true。
    assert_eq!(open_value.get("confirmed"), Some(&Value::Bool(true)));
    // parser 投影必须为 open。
    assert_eq!(
        open.request().operation(),
        BrowserSessionBrokerOperation::Open
    );
    // 指纹必须等于 parser 同源计算。
    assert_eq!(
        // 读取 wire 指纹。
        open.request().semantic_fingerprint(),
        // 重新计算 canonical 指纹。
        open.request().computed_semantic_fingerprint()
    );
    // 构造 confirmed close。
    let close = BrowserSessionBrokerRequestFrame::close_confirmed(
        // 使用独立 request nonce。
        FOREIGN_CANCEL_NONCE,
        // 绑定同一 live epoch。
        EPOCH,
        // 设置有界预算。
        500,
        // 只允许公开 session ID。
        SESSION_ID,
    )?;
    // parser 投影必须为 close。
    assert_eq!(
        close.request().operation(),
        BrowserSessionBrokerOperation::Close
    );
    // 非 s2:bs target 必须被 builder 的同源 parser 拒绝。
    assert!(
        BrowserSessionBrokerRequestFrame::close_confirmed(
            // 使用 canonical nonce。
            FOREIGN_CANCEL_NONCE,
            // 使用 canonical epoch。
            EPOCH,
            // 使用合法预算。
            500,
            // 伪造 page ID 作为 session target。
            "s2:bp:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        // 必须返回错误。
        .is_err()
    );
    // 构造当前 epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())?;
    // 编码 ready。
    let ready_text = encode_ready(&epoch)?;
    // 解码 ready。
    let ready = decode_ready(&ready_text)?;
    // ready 必须逐字回显 epoch。
    assert_eq!(ready.broker_epoch(), EPOCH);
    // ready 不允许额外字段。
    assert!(decode_ready(
        // 构造带泄漏字段的 ready。
        r#"{"kind":"broker-ready","contractVersion":"act/browser-session-broker/v1","brokerEpoch":"abcdef0123456789abcdef0123456789","endpoint":"x"}"#,
    )
    // 必须拒绝。
    .is_err());
    // 结束测试。
    Ok(())
}

// 验证 response 解码绑定 request/epoch 并拒绝 control frame。
#[test]
fn response_codec_is_request_bound() -> Result<(), Box<dyn Error>> {
    // 构造 open request。
    let frame = BrowserSessionBrokerRequestFrame::open_confirmed(REQUEST_NONCE, EPOCH, 500)?;
    // 建立当前 epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())?;
    // 构造 accepted revision 零。
    let accepted = BrowserSessionBrokerResponse::accepted(frame.request(), 0, &epoch);
    // 编码 accepted。
    let text = encode_response(&accepted, Some(frame.request()), &epoch)?;
    // 解码 request-bound accepted。
    let decoded = decode_response(&text, frame.request(), &epoch)?;
    // 核对 revision。
    assert_eq!(decoded.request_revision(), 0);
    // 核对业务接受事实。
    assert!(decoded.business_accepted());
    // 构造 schema-invalid accepted revision。
    let accepted_late = BrowserSessionBrokerResponse::accepted(frame.request(), 999, &epoch);
    // encoder 必须拒绝非零 accepted revision。
    assert!(encode_response(&accepted_late, Some(frame.request()), &epoch).is_err());
    // 另一 current epoch 不得编码非 stale accepted。
    let foreign_epoch = BrowserSessionBrokerEpoch::new(FOREIGN_EPOCH.to_owned())?;
    // output epoch 必须绑定认证 current。
    assert!(encode_response(&accepted, Some(frame.request()), &foreign_epoch).is_err());
    // broker-ready 不能进入 response decoder。
    assert!(decode_response(&encode_ready(&epoch)?, frame.request(), &epoch).is_err());
    // 改写 response epoch。
    let mut foreign = serde_json::from_str::<Value>(&text)?;
    // 设置另一 canonical epoch。
    foreign["brokerEpoch"] = Value::String(FOREIGN_EPOCH.to_owned());
    // 跨 epoch response 必须拒绝。
    assert!(decode_response(&serde_json::to_string(&foreign)?, frame.request(), &epoch).is_err());
    // 改写 accepted revision。
    let mut skipped = serde_json::from_str::<Value>(&text)?;
    // 首个 accepted 不允许跳到任意 revision。
    skipped["requestRevision"] = json!(999);
    // decoder 必须拒绝 accepted revision 漂移。
    assert!(decode_response(&serde_json::to_string(&skipped)?, frame.request(), &epoch).is_err());
    // 构造 completed open。
    let completed = BrowserSessionBrokerResponse::finished(
        // 绑定原 request。
        frame.request(),
        // accepted 后 revision 一。
        1,
        // 绑定当前 epoch。
        &epoch,
        // 使用 completed outcome。
        BrowserSessionBrokerOutcome::Completed,
        // 携带严格 open data。
        Some(BrowserSessionBrokerSuccess::Open {
            // 返回公开 session ID。
            session_id: SESSION_ID.to_owned(),
        }),
    )
    // 合法 completed 必须可构造。
    .expect("open completed must be valid");
    // 构造错误使用 revision 零的业务 final。
    let early_completed = BrowserSessionBrokerResponse::finished(
        // 绑定原 request。
        frame.request(),
        // 伪造业务 final revision 零。
        0,
        // 绑定当前 epoch。
        &epoch,
        // 使用 completed outcome。
        BrowserSessionBrokerOutcome::Completed,
        // 携带合法 open data。
        Some(BrowserSessionBrokerSuccess::Open {
            // 返回公开 session ID。
            session_id: SESSION_ID.to_owned(),
        }),
    )
    // response 构造器允许交由 wire revision gate 拒绝。
    .expect("response shape remains valid before wire revision validation");
    // encoder 必须拒绝业务 final revision 零。
    assert!(encode_response(&early_completed, Some(frame.request()), &epoch).is_err());
    // 编码 completed。
    let completed_text = encode_response(&completed, Some(frame.request()), &epoch)?;
    // 解码 completed。
    let completed_decoded = decode_response(&completed_text, frame.request(), &epoch)?;
    // 核对 completed outcome。
    assert_eq!(
        completed_decoded.outcome(),
        Some(BrowserSessionBrokerOutcome::Completed)
    );
    // 改写 business final revision。
    let mut late_final = serde_json::from_str::<Value>(&completed_text)?;
    // v1 final 必须紧随 accepted 为 revision 一。
    late_final["requestRevision"] = json!(9);
    // decoder 必须拒绝跳号。
    assert!(
        decode_response(
            &serde_json::to_string(&late_final)?,
            frame.request(),
            &epoch
        )
        .is_err()
    );
    // 改写 command mutation flag。
    let mut wrong_mutation = serde_json::from_str::<Value>(&completed_text)?;
    // command final 不允许 false。
    wrong_mutation["targetMayHaveMutated"] = Value::Bool(false);
    // flag 漂移必须拒绝。
    assert!(
        decode_response(
            // 序列化伪造 final。
            &serde_json::to_string(&wrong_mutation)?,
            // 仍绑定原 request。
            frame.request(),
            // 绑定认证连接当前 epoch。
            &epoch,
        )
        // 必须返回错误。
        .is_err()
    );
    // 结束测试。
    Ok(())
}

// 验证 rejected safeError message 无损往返及新增业务前错误码。
#[test]
fn rejected_round_trip_preserves_safe_message() -> Result<(), Box<dyn Error>> {
    // 构造 open request。
    let frame = BrowserSessionBrokerRequestFrame::open_confirmed(REQUEST_NONCE, EPOCH, 500)?;
    // 建立当前 epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())?;
    // 构造 registry full 可关联 failure。
    let failure = BrowserSessionBrokerProtocolFailure::rejected_for_request(
        // 使用冻结新错误码。
        BrowserSessionBrokerProtocolErrorCode::BrowserSessionRegistryFull,
        // 绑定严格 request。
        frame.request(),
    );
    // 构造业务前 rejected。
    let rejected = BrowserSessionBrokerResponse::rejected_for_request(
        // 绑定可关联 failure。
        &failure,
        // 保存 strict request semantic key。
        frame.request(),
        // 业务前 final 固定 revision 零。
        0,
        // 绑定当前 epoch。
        &epoch,
    )
    // 白名单必须允许该错误码。
    .expect("registry full must be a wire rejection");
    // 编码默认安全说明。
    let text = encode_response(&rejected, Some(frame.request()), &epoch)?;
    // 改为另一条仍安全的说明以验证无损恢复。
    let mut value = serde_json::from_str::<Value>(&text)?;
    // 设置合法用户安全说明。
    value["error"]["message"] = Value::String("Registry capacity reached.".to_owned());
    // 解码自定义说明。
    let decoded = decode_response(&serde_json::to_string(&value)?, frame.request(), &epoch)?;
    // 重编码必须保留逐字说明。
    let replay = serde_json::from_str::<Value>(&encode_response(
        // 编码已解码 response。
        &decoded,
        // 绑定原 request。
        Some(frame.request()),
        // 绑定认证 current epoch。
        &epoch,
    )?)?;
    // 核对说明没有被默认值替换。
    assert_eq!(replay["error"]["message"], "Registry capacity reached.");
    // stale session 也必须为业务前白名单错误。
    let stale = BrowserSessionBrokerProtocolFailure::rejected_for_request(
        // 使用冻结 stale session 错误码。
        BrowserSessionBrokerProtocolErrorCode::StaleSession,
        // 绑定严格 request。
        frame.request(),
    );
    // 投影必须成功。
    assert!(
        BrowserSessionBrokerResponse::rejected_for_request(
            // 绑定 stale failure。
            &stale,
            // 绑定严格 request。
            frame.request(),
            // 固定业务前 revision。
            0,
            // 回显当前 epoch。
            &epoch,
        )
        // 必须可构造。
        .is_some()
    );
    // 结束测试。
    Ok(())
}

// 验证 cancel builder 与 request-bound control response codec。
#[test]
fn cancel_codec_is_bound_to_original_control_request() -> Result<(), Box<dyn Error>> {
    // 构造严格 cancel frame。
    let frame = BrowserSessionBrokerCancelFrame::new(CANCEL_NONCE, REQUEST_NONCE, EPOCH)?;
    // parser 必须逐字保存 cancel nonce。
    assert_eq!(frame.cancel().cancel_request_nonce(), CANCEL_NONCE);
    // frame 不得携带确认字段。
    assert!(!frame.text().contains("confirmed"));
    // 建立当前 epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())?;
    // 构造 cancellation-requested receipt revision 零。
    let receipt = BrowserSessionBrokerCancelReceipt::new(
        // 绑定原 cancel。
        frame.cancel(),
        // 绑定当前 epoch。
        &epoch,
        // 首次 receipt revision 为零。
        0,
        // 取消协作尚未终结。
        BrowserSessionBrokerCancelStatus::CancellationRequested,
    );
    // 编码 receipt。
    let receipt_text = encode_cancel_receipt(&receipt, &epoch)?;
    // 构造非法 rev1 非终态 receipt。
    let revision_one_pending = BrowserSessionBrokerCancelReceipt::new(
        // 绑定原 cancel。
        frame.cancel(),
        // 绑定当前 epoch。
        &epoch,
        // 使用 revision 一。
        1,
        // 错误保留中间态。
        BrowserSessionBrokerCancelStatus::CancellationRequested,
    );
    // encoder 必须拒绝 rev1 中间态。
    assert!(encode_cancel_receipt(&revision_one_pending, &epoch).is_err());
    // 构造任意跳号 receipt。
    let revision_nine = BrowserSessionBrokerCancelReceipt::new(
        // 绑定原 cancel。
        frame.cancel(),
        // 绑定当前 epoch。
        &epoch,
        // 使用非法 revision 九。
        9,
        // 即使是 terminal 也不得跳号。
        BrowserSessionBrokerCancelStatus::Cancelled,
    );
    // encoder 必须拒绝大于一的 revision。
    assert!(encode_cancel_receipt(&revision_nine, &epoch).is_err());
    // request-bound 解码 receipt。
    let decoded = decode_cancel_receipt(
        // 借用 receipt 文本。
        &receipt_text,
        // 绑定原 cancel。
        frame.cancel(),
        // 绑定认证连接当前 epoch。
        &epoch,
        // 首次 receipt 没有 cursor。
        None,
    )?;
    // 核对 revision。
    assert_eq!(decoded.cancel_revision(), 0);
    // 构造同 cancel 的 terminal receipt revision 一。
    let terminal = BrowserSessionBrokerCancelReceipt::new(
        // 绑定原 cancel。
        frame.cancel(),
        // 绑定当前 epoch。
        &epoch,
        // 只允许连续加一。
        1,
        // 协作取消收敛为 cancelled。
        BrowserSessionBrokerCancelStatus::Cancelled,
    );
    // 编码 terminal receipt。
    let terminal_text = encode_cancel_receipt(&terminal, &epoch)?;
    // 使用上一次 receipt cursor 解码。
    let terminal_decoded = decode_cancel_receipt(
        // 借用 terminal 文本。
        &terminal_text,
        // 绑定原 cancel。
        frame.cancel(),
        // 绑定认证连接当前 epoch。
        &epoch,
        // 绑定上一次 revision 零 receipt。
        Some(&decoded),
    )?;
    // 核对单调 revision。
    assert_eq!(terminal_decoded.cancel_revision(), 1);
    // 首次观察可能是丢失 rev0 后重放的 terminal rev1。
    let recovered = decode_cancel_receipt(
        // 借用 terminal 文本。
        &terminal_text,
        // 绑定原 cancel。
        frame.cancel(),
        // 绑定认证连接当前 epoch。
        &epoch,
        // 故意省略 cursor。
        None,
    )?;
    // 恢复结果必须是 terminal rev1。
    assert_eq!(recovered.cancel_revision(), 1);
    // 另一 cancel nonce 不得接收该 receipt。
    let foreign = BrowserSessionBrokerCancelFrame::new(
        // 使用另一 cancel nonce。
        FOREIGN_CANCEL_NONCE,
        // 指向相同 target。
        REQUEST_NONCE,
        // 使用同一 epoch。
        EPOCH,
    )?;
    // 双 nonce 关联门禁必须拒绝串线。
    assert!(
        decode_cancel_receipt(
            // 借用原 receipt 文本。
            &receipt_text,
            // 绑定另一 cancel。
            foreign.cancel(),
            // 绑定认证连接当前 epoch。
            &epoch,
            // 首次 receipt 没有 cursor。
            None,
        )
        // 必须拒绝。
        .is_err()
    );
    // 构造 cancel-rejected。
    let rejected = BrowserSessionBrokerCancelRejected::new(
        // 绑定原 cancel。
        frame.cancel(),
        // 绑定当前 epoch。
        &epoch,
        // 使用 cancel 白名单错误码。
        "NONCE_SEMANTIC_CONFLICT",
    )
    // 合法拒绝必须可构造。
    .expect("cancel rejection must be valid");
    // 编码 cancel-rejected。
    let rejected_text = encode_cancel_rejected(&rejected, frame.cancel(), &epoch)?;
    // 建立另一认证 current epoch。
    let foreign_epoch = BrowserSessionBrokerEpoch::new(FOREIGN_EPOCH.to_owned())?;
    // 非 stale rejection 不得编码到 foreign current epoch。
    assert!(encode_cancel_rejected(&rejected, frame.cancel(), &foreign_epoch).is_err());
    // request-bound 解码必须成功。
    let decoded_rejected = decode_cancel_rejected(&rejected_text, frame.cancel(), &epoch)?;
    // 核对稳定错误码。
    assert_eq!(decoded_rejected.error_code(), "NONCE_SEMANTIC_CONFLICT");
    // 跨 cancel 解码必须拒绝。
    assert!(decode_cancel_rejected(&rejected_text, foreign.cancel(), &epoch).is_err());
    // 结束测试。
    Ok(())
}

// 验证连接仅按 request-bound response 不变量推进业务前与重放终态。
#[test]
fn connection_transitions_are_response_driven() -> Result<(), Box<dyn Error>> {
    // 构造严格 open request。
    let frame = BrowserSessionBrokerRequestFrame::open_confirmed(REQUEST_NONCE, EPOCH, 500)?;
    // 建立当前 epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())?;
    // 建立空连接。
    let mut connection = BrowserSessionBrokerConnection::default();
    // 接受唯一 transport request。
    connection.accept_transport(frame.request())?;
    // 构造业务前 deadline final revision 零。
    let expired = BrowserSessionBrokerResponse::expired_before_acceptance(
        // 绑定当前 request。
        frame.request(),
        // 首个 response revision 为零。
        0,
        // 绑定当前 epoch。
        &epoch,
    )
    // 合法 expired 必须可构造。
    .expect("expired response must be valid");
    // response 不变量允许 transport accepted 直接 final。
    connection.apply_response(&expired)?;
    // 阶段必须终结。
    assert_eq!(
        connection.phase(),
        BrowserSessionBrokerConnectionPhase::Final
    );
    // 建立另一连接验证 replay business terminal。
    let mut replay_connection = BrowserSessionBrokerConnection::default();
    // 接受同一 request 的新连接。
    replay_connection.accept_transport(frame.request())?;
    // 构造 completed revision 一。
    let completed = BrowserSessionBrokerResponse::finished(
        // 绑定 request。
        frame.request(),
        // accepted 后 final revision 一。
        1,
        // 绑定 epoch。
        &epoch,
        // 使用 completed outcome。
        BrowserSessionBrokerOutcome::Completed,
        // 返回公开 session ID。
        Some(BrowserSessionBrokerSuccess::Open {
            // 复制 canonical session ID。
            session_id: SESSION_ID.to_owned(),
        }),
    )
    // 合法 completed 必须可构造。
    .expect("completed response must be valid");
    // replay 可以直接以已缓存业务终态收敛。
    replay_connection.apply_response(&completed)?;
    // replay 连接必须终结。
    assert_eq!(
        replay_connection.phase(),
        BrowserSessionBrokerConnectionPhase::Final
    );
    // 结束测试。
    Ok(())
}

// 验证 stale request rejection 回显另一个 canonical 当前 epoch。
#[test]
fn stale_request_rejection_uses_current_epoch() -> Result<(), Box<dyn Error>> {
    // 构造绑定旧 epoch 的 request。
    let frame = BrowserSessionBrokerRequestFrame::open_confirmed(REQUEST_NONCE, EPOCH, 500)?;
    // 建立另一个当前 epoch。
    let current = BrowserSessionBrokerEpoch::new(FOREIGN_EPOCH.to_owned())?;
    // 构造可关联 stale failure。
    let failure = BrowserSessionBrokerProtocolFailure::rejected_for_request(
        // 使用 stale epoch 错误码。
        BrowserSessionBrokerProtocolErrorCode::StaleBrokerEpoch,
        // 绑定旧 epoch request。
        frame.request(),
    );
    // rejected 必须回显当前 epoch。
    let rejected = BrowserSessionBrokerResponse::rejected_for_request(
        // 绑定 stale failure。
        &failure,
        // 保存 strict request semantic key。
        frame.request(),
        // 业务前 final 固定 revision 零。
        0,
        // 回显认证连接当前 epoch。
        &current,
    )
    // stale failure 必须可投影。
    .expect("stale request rejection must be valid");
    // 编码当前 epoch rejection。
    let text = encode_response(&rejected, Some(frame.request()), &current)?;
    // 解码必须允许且要求另一个当前 epoch。
    let decoded = decode_response(&text, frame.request(), &current)?;
    // 核对 current epoch。
    assert_eq!(decoded.broker_epoch(), FOREIGN_EPOCH);
    // 建立第三个未认证 epoch。
    let third = BrowserSessionBrokerEpoch::new(THIRD_EPOCH.to_owned())?;
    // 合法 canonical 但不等于 frame/current 的第三 epoch 必须拒绝。
    assert!(decode_response(&text, frame.request(), &third).is_err());
    // 结束测试。
    Ok(())
}

// 验证 parser early rejection 不需 strict request 也能安全编码且不能被误认领。
#[test]
fn early_rejection_encoding_is_epoch_bound_and_unclaimable() -> Result<(), Box<dyn Error>> {
    // 构造绑定旧 epoch 的严格 request frame。
    let frame = BrowserSessionBrokerRequestFrame::open_confirmed(REQUEST_NONCE, EPOCH, 500)?;
    // 直接借用 builder 已经严格生成的 wire 文本。
    let request_text = frame.text();
    // 建立已认证的新 current epoch。
    let current = BrowserSessionBrokerEpoch::new(FOREIGN_EPOCH.to_owned())?;
    // epoch-first parser 产生没有 strict request 的 stale failure。
    let failure = BrowserSessionBrokerRequest::parse_for_epoch(request_text, current.as_str())
        // stale envelope 必须在深层解析前拒绝。
        .expect_err("stale request must be rejected before strict parsing");
    // 从 early failure 构造无 semantic key 响应。
    let early = BrowserSessionBrokerResponse::rejected(&failure, 0, &current)
        // canonical failure 必须可投影。
        .expect("early stale rejection must be representable");
    // parser early failure 不得被原 frame 的 strict request 洗白。
    assert!(
        BrowserSessionBrokerResponse::rejected_for_request(
            // 传入 parse_for_epoch 产生的 early failure。
            &failure,
            // 即使使用原 frame request 也不得补写 semantic key。
            frame.request(),
            // 业务前 final 固定 revision 零。
            0,
            // 回显认证连接 current epoch。
            &current,
        )
        // 缺少 failure provenance 必须拒绝。
        .is_none()
    );
    // 不提供 strict request 编码 early rejection。
    let text = encode_response(&early, None, &current)?;
    // 读取 wire 对象。
    let value = serde_json::from_str::<Value>(&text)?;
    // frame 必须回显已认证 current epoch。
    assert_eq!(value["brokerEpoch"], FOREIGN_EPOCH);
    // early response 不得被任意 strict request 认领。
    assert!(encode_response(&early, Some(frame.request()), &current).is_err());
    // 建立第三个 canonical 但未认证 epoch。
    let third = BrowserSessionBrokerEpoch::new(THIRD_EPOCH.to_owned())?;
    // 编码必须拒绝非 current 的第三 epoch。
    assert!(encode_response(&early, None, &third).is_err());
    // strict decoder 仍可在原 request 关联下恢复 stale 终态。
    let decoded = decode_response(&text, frame.request(), &current)?;
    // decoder 产物必须补全完整 request semantic key。
    assert!(decoded.request_semantic_key().is_some());
    // 构造 expected 仍为旧 epoch 的非 stale early failure。
    let nonstale = BrowserSessionBrokerProtocolFailure::rejected(
        // 使用非 stale 确认错误码。
        BrowserSessionBrokerProtocolErrorCode::ConfirmationRequired,
        // 复制 canonical request nonce。
        frame.request().request_nonce().to_owned(),
        // 复制已解析 operation。
        frame.request().operation(),
        // 保留旧 expected epoch。
        EPOCH.to_owned(),
    );
    // 故意让非 stale 响应回显新 current epoch。
    let foreign_nonstale = BrowserSessionBrokerResponse::rejected(&nonstale, 0, &current)
        // 响应类型先保留事实供 encoder 闭合。
        .expect("canonical early rejection must be representable");
    // 非 stale 错误不得跨 expected/current epoch 编码。
    assert!(encode_response(&foreign_nonstale, None, &current).is_err());
    // 结束测试。
    Ok(())
}

// 验证 stale cancel rejection 回显另一个 canonical 当前 epoch。
#[test]
fn stale_cancel_rejection_uses_current_epoch() -> Result<(), Box<dyn Error>> {
    // 构造绑定旧 epoch 的 cancel。
    let frame = BrowserSessionBrokerCancelFrame::new(CANCEL_NONCE, REQUEST_NONCE, EPOCH)?;
    // 建立另一个当前 epoch。
    let current = BrowserSessionBrokerEpoch::new(FOREIGN_EPOCH.to_owned())?;
    // 构造 stale cancel rejection。
    let rejected = BrowserSessionBrokerCancelRejected::new(
        // 绑定旧 cancel。
        frame.cancel(),
        // 回显当前 epoch。
        &current,
        // 使用 stale 专用错误码。
        "STALE_BROKER_EPOCH",
    )
    // 合法 stale 拒绝必须可构造。
    .expect("stale cancel rejection must be valid");
    // 编码 current epoch rejection。
    let text = encode_cancel_rejected(&rejected, frame.cancel(), &current)?;
    // stale rejection 不得编码到原 expected epoch。
    let expected = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())?;
    // stale iff 关系必须在 output 侧闭合。
    assert!(encode_cancel_rejected(&rejected, frame.cancel(), &expected).is_err());
    // request-bound 解码必须允许 current epoch。
    let decoded = decode_cancel_rejected(&text, frame.cancel(), &current)?;
    // 核对当前 epoch。
    assert_eq!(decoded.broker_epoch(), FOREIGN_EPOCH);
    // 建立第三个未认证 epoch。
    let third = BrowserSessionBrokerEpoch::new(THIRD_EPOCH.to_owned())?;
    // 合法 canonical 但不等于 frame/current 的第三 epoch 必须拒绝。
    assert!(decode_cancel_rejected(&text, frame.cancel(), &third).is_err());
    // 结束测试。
    Ok(())
}
