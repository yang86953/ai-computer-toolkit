// 导入标准错误 trait 供测试返回类型使用。
use std::error::Error;

// 导入 parser 契约。
use super::*;
// 导入 response 契约。
use super::response::*;
// 导入 epoch state 契约。
use super::state::*;
// 导入 JSON 构造和值类型。
use serde_json::{Value, json};

// 固定测试 request nonce。
const NONCE: &str = "0123456789abcdef0123456789abcdef";
// 固定测试 cancel nonce。
const CANCEL_NONCE: &str = "33333333333333333333333333333333";
// 固定测试 broker epoch。
const EPOCH: &str = "11111111111111111111111111111111";
// 固定测试 session identity。
const SESSION: &str = "s2:bs:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
// 固定测试 page identity。
const PAGE: &str = "s2:bp:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
// 固定另一条测试 request nonce。
const OTHER_NONCE: &str = "44444444444444444444444444444444";

// 计算与 parser 同源的 FNV-1a wire 摘要。
fn fingerprint(key: &str) -> String {
    // 初始化固定 offset basis。
    let mut hash = 0xcbf29ce484222325_u64;
    // 逐字节混入完整 canonical key。
    for byte in key.bytes() {
        // 执行 FNV-1a 更新。
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    // 返回固定十六进制文本。
    format!("{hash:016x}")
}

// 构造合法 open request JSON。
fn open_json(remaining_timeout_ms: u32) -> Value {
    // 委托可指定 nonce 的构造器。
    open_json_for(NONCE, remaining_timeout_ms)
}

// 构造可指定 nonce 的合法 open request JSON。
fn open_json_for(request_nonce: &str, remaining_timeout_ms: u32) -> Value {
    // 构造 version、epoch 与 operation 的 canonical key。
    let key = format!("{}|32:{}|4:open", CONTRACT_VERSION, EPOCH);
    // 返回严格 request frame。
    json!({
        // 固定 frame kind。
        "kind": "request",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 固定 request nonce。
        "requestNonce": request_nonce,
        // 写入规范语义摘要。
        "semanticFingerprint": fingerprint(&key),
        // 绑定当前 broker epoch。
        "expectedBrokerEpoch": EPOCH,
        // 写入剩余预算。
        "remainingTimeoutMs": remaining_timeout_ms,
        // 选择 open command。
        "operation": "open",
        // 显式确认 command。
        "confirmed": true
    })
}

// 解析合法 open request。
fn open_request(remaining_timeout_ms: u32) -> Result<BrowserSessionBrokerRequest, Box<dyn Error>> {
    // 使用 epoch-first parser。
    Ok(BrowserSessionBrokerRequest::parse_for_epoch(
        // 序列化测试 frame。
        &open_json(remaining_timeout_ms).to_string(),
        // 绑定当前 epoch。
        EPOCH,
    )?)
}

// 解析使用指定 nonce 的合法 open request。
fn open_request_for(
    // 借用 canonical request nonce。
    request_nonce: &str,
    // 接收剩余执行预算。
    remaining_timeout_ms: u32,
) -> Result<BrowserSessionBrokerRequest, Box<dyn Error>> {
    // 使用 epoch-first parser。
    Ok(BrowserSessionBrokerRequest::parse_for_epoch(
        // 序列化测试 frame。
        &open_json_for(request_nonce, remaining_timeout_ms).to_string(),
        // 绑定当前 epoch。
        EPOCH,
    )?)
}

// 解析与 open 使用相同 nonce 但内部自洽的 close request。
fn conflicting_close_request() -> Result<BrowserSessionBrokerRequest, Box<dyn Error>> {
    // 构造包含 operation 与 target 的完整 canonical key。
    let key = format!("{}|32:{}|5:close|38:{}", CONTRACT_VERSION, EPOCH, SESSION);
    // 构造严格 close frame。
    let value = json!({
        // 固定 request kind。
        "kind": "request",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 故意复用 open nonce。
        "requestNonce": NONCE,
        // 使用 close 自身语义的正确 fingerprint。
        "semanticFingerprint": fingerprint(&key),
        // 绑定当前 broker epoch。
        "expectedBrokerEpoch": EPOCH,
        // 使用有效剩余预算。
        "remainingTimeoutMs": 100,
        // 选择 close command。
        "operation": "close",
        // 显式确认 command。
        "confirmed": true,
        // 绑定公开 session identity。
        "sessionId": SESSION
    });
    // 返回内部自洽的另一项语义。
    Ok(BrowserSessionBrokerRequest::parse_for_epoch(
        // 序列化测试 frame。
        &value.to_string(),
        // 绑定当前 epoch。
        EPOCH,
    )?)
}

// 解析合法 screenshot query。
fn screenshot_request() -> Result<BrowserSessionBrokerRequest, Box<dyn Error>> {
    // 构造完整 screenshot canonical key。
    let key = format!(
        // 长度前缀与生产 parser 同源。
        "{}|32:{}|10:screenshot|38:{}|38:{}",
        // 写入协议版本。
        CONTRACT_VERSION,
        // 写入 epoch。
        EPOCH,
        // 写入 session target。
        SESSION,
        // 写入 page target。
        PAGE
    );
    // 构造严格 screenshot frame。
    let value = json!({
        // 固定 request kind。
        "kind": "request",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用另一 canonical nonce。
        "requestNonce": OTHER_NONCE,
        // 写入正确 semantic fingerprint。
        "semanticFingerprint": fingerprint(&key),
        // 绑定当前 broker epoch。
        "expectedBrokerEpoch": EPOCH,
        // 使用有效剩余预算。
        "remainingTimeoutMs": 100,
        // 选择 screenshot query。
        "operation": "screenshot",
        // 绑定 session target。
        "sessionId": SESSION,
        // 绑定 page target。
        "pageId": PAGE
    });
    // 返回严格 screenshot request。
    Ok(BrowserSessionBrokerRequest::parse_for_epoch(
        // 序列化测试 frame。
        &value.to_string(),
        // 绑定当前 epoch。
        EPOCH,
    )?)
}

// 构造绑定当前 epoch 的 cancel frame。
fn cancel_request() -> Result<BrowserSessionBrokerCancellationRequest, Box<dyn Error>> {
    // 委托可指定 cancel nonce 的构造器。
    cancel_request_for(CANCEL_NONCE)
}

// 构造可指定 cancel nonce 且绑定同一 target 的 frame。
fn cancel_request_for(
    // 借用 canonical cancel nonce。
    cancel_nonce: &str,
) -> Result<BrowserSessionBrokerCancellationRequest, Box<dyn Error>> {
    // 构造独立 cancel command。
    let value = json!({
        // 固定 control kind。
        "kind": "cancel",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用 cancel 自身 nonce。
        "cancelRequestNonce": cancel_nonce,
        // 关联原 request nonce。
        "requestNonce": NONCE,
        // 绑定当前 broker epoch。
        "expectedBrokerEpoch": EPOCH
    });
    // 解析严格 cancel frame。
    Ok(BrowserSessionBrokerCancellationRequest::parse(
        // 序列化测试对象。
        &value.to_string(),
    )?)
}

// 验证最终 schema 的握手、revision、取消 tombstone 与 wire root。
#[test]
fn schema_freezes_ready_revision_tombstone_and_wire_outcomes() -> Result<(), Box<dyn Error>> {
    // 解析版本化 JSON Schema。
    let schema: Value = serde_json::from_str(include_str!(
        // 读取与本 Component 同源的契约文件。
        "../../contracts/internal/browser-session-broker-v1.schema.json"
    ))?;
    // broker-ready 必须是认证后的首个控制帧。
    assert_eq!(
        // 读取 ready kind 常量。
        schema["$defs"]["brokerReady"]["properties"]["kind"]["const"],
        // 核对冻结文本。
        "broker-ready"
    );
    // 构造形状合法的 ready frame。
    let ready = json!({
        // 固定控制帧类型。
        "kind": "broker-ready",
        // 固定契约版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用 canonical epoch。
        "brokerEpoch": EPOCH
    })
    // 序列化为 wire JSON。
    .to_string();
    // 上限内 ready 必须可解析。
    assert_eq!(
        // 解析严格 ready frame。
        BrowserSessionBrokerReady::parse(&ready)?.broker_epoch(),
        // 回显原 epoch。
        EPOCH
    );
    // 在合法 JSON 前添加足以超过 response frame 上限的外围空白。
    let oversized_ready = format!("{}{}", " ".repeat(MAXIMUM_RESPONSE_FRAME_BYTES), ready);
    // 超限必须在 trim/JSON 解析前失败闭合。
    assert!(BrowserSessionBrokerReady::parse(&oversized_ready).is_err());
    // rejected error 必须引用封闭错误集合。
    assert_eq!(
        // 读取 rejected error schema 引用。
        schema["$defs"]["finalRejected"]["allOf"][1]["properties"]["error"]["$ref"],
        // 固定专用 rejection error。
        "#/$defs/rejectionError"
    );
    // 读取业务前拒绝的封闭错误变体。
    let rejection_errors = schema["$defs"]["rejectionError"]["oneOf"]
        // rejectionError 必须保持数组形状。
        .as_array()
        // schema 漂移时立即失败。
        .ok_or("rejectionError.oneOf must be an array")?;
    // Module registry 满必须由 schema 接受为业务前拒绝。
    assert!(rejection_errors.iter().any(|entry| {
        // 读取组合 schema 中冻结的错误码。
        entry["allOf"][1]["properties"]["code"]["const"]
            // 核对容量错误码。
            == "BROWSER_SESSION_REGISTRY_FULL"
    }));
    // stale close 也必须由同一 schema 接受为业务前拒绝。
    assert!(rejection_errors.iter().any(|entry| {
        // 读取组合 schema 中冻结的错误码。
        entry["allOf"][1]["properties"]["code"]["const"]
            // 核对 stale session 错误码。
            == "STALE_SESSION"
    }));
    // expired error 必须固定 REQUEST_EXPIRED。
    assert_eq!(
        // 读取 expired 专用错误码。
        schema["$defs"]["expiredBeforeAcceptanceError"]["allOf"][1]["properties"]["code"]["const"],
        // 与 Rust response 枚举一致。
        "REQUEST_EXPIRED"
    );
    // accepted 必须携带 requestRevision。
    assert!(
        schema["$defs"]["accepted"]["required"]
            // 取得字段数组。
            .as_array()
            // 查找 revision 字段。
            .is_some_and(|fields| fields.iter().any(|field| field == "requestRevision"))
    );
    // accepted 必须固定为首个 request revision 零。
    assert_eq!(
        // 读取 accepted revision 常量。
        schema["$defs"]["accepted"]["properties"]["requestRevision"]["const"],
        // 与 Rust decoder 的首态一致。
        0
    );
    // 固定各类 final 的 v1 revision 矩阵。
    let revision_cases = [
        // 业务前拒绝保持 revision 零。
        ("finalRejected", 0),
        // deadline 在接受前耗尽保持 revision 零。
        ("finalExpiredBeforeAcceptance", 0),
        // cancel tombstone 保持 revision 零。
        ("finalCancelledBeforeAcceptance", 0),
        // 业务接受后失败推进到 revision 一。
        ("finalFailed", 1),
        // 业务接受后取消推进到 revision 一。
        ("finalCancelled", 1),
        // 无法确定结果仍是业务终态 revision 一。
        ("finalUnknown", 1),
        // operation-specific 成功共享 revision 一。
        ("completedCommon", 1),
    ];
    // 逐项防止 schema 放宽已经冻结的 revision。
    for (definition, expected) in revision_cases {
        // 每个 final 的专用 allOf 分支必须写出 const。
        assert_eq!(
            // 读取专用 final revision 常量。
            schema["$defs"][definition]["allOf"][1]["properties"]["requestRevision"]["const"],
            // 核对冻结 revision。
            expected
        );
    }
    // cancel receipt revision 只允许零或一。
    assert_eq!(
        // 读取 cancel revision 最大值。
        schema["$defs"]["cancelReceipt"]["properties"]["cancelRevision"]["maximum"],
        // 与 Rust 单调游标一致。
        1
    );
    // 非终态 cancel status 只能出现在 revision 零。
    assert_eq!(
        // 读取非终态分支 revision 常量。
        schema["$defs"]["cancelReceipt"]["oneOf"][0]["properties"]["cancelRevision"]["const"],
        // 固定初始 revision。
        0
    );
    // terminal cancel status 允许首包即终态或从中间态推进。
    assert_eq!(
        // 读取 terminal 分支允许的 revision 集合。
        schema["$defs"]["cancelReceipt"]["oneOf"][1]["properties"]["cancelRevision"]["enum"],
        // 与丢失 revision 零后的恢复规则一致。
        json!([0, 1])
    );
    // wire root 不得包含 client-local finalNotDispatched。
    assert!(
        !schema["oneOf"]
            // 取得 root union。
            .as_array()
            // 扫描所有引用。
            .is_some_and(|entries| entries.iter().any(|entry| {
                // 比较 ref 文本。
                entry["$ref"] == "#/$defs/finalNotDispatched"
            }))
    );
    // wire 必须包含 cancelled-before-acceptance。
    assert!(
        schema["oneOf"]
            // 取得 root union。
            .as_array()
            // 扫描冻结引用。
            .is_some_and(|entries| entries.iter().any(|entry| {
                // 核对取消 tombstone final。
                entry["$ref"] == "#/$defs/finalCancelledBeforeAcceptance"
            }))
    );
    // query 总命中数必须使用 u32 上限。
    assert_eq!(
        // 读取 matchCount maximum。
        schema["$defs"]["finalQueryCompleted"]["allOf"][1]["properties"]["data"]["properties"]["matchCount"]
            ["maximum"],
        // 核对 u32 最大值。
        u64::from(u32::MAX)
    );
    // request oneOf 必须冻结九种领域 operation。
    assert_eq!(
        schema["$defs"]["request"]["oneOf"].as_array().map(Vec::len),
        Some(9)
    );
    // session.inspect 必须归类为 Query。
    assert!(
        schema["$defs"]["queryOperation"]["enum"]
            // 读取 Query 标签集合。
            .as_array()
            // 查找新冻结标签。
            .is_some_and(|operations| operations
                .iter()
                .any(|operation| operation == "session.inspect"))
    );
    // session.inspect request 只要求 sessionId，不允许 confirmed。
    assert_eq!(
        // 读取该 Query 的唯一业务必需字段。
        schema["$defs"]["sessionInspectRequest"]["allOf"][1]["required"],
        // 固定只有 session target。
        json!(["sessionId"])
    );
    // completed oneOf 也必须与九种 operation 同源。
    assert_eq!(
        schema["$defs"]["finalCompleted"]["oneOf"]
            .as_array()
            .map(Vec::len),
        Some(9)
    );
    // session.inspect 成功必须固定 Query mutation flag 为 false。
    assert_eq!(
        schema["$defs"]["finalSessionInspectCompleted"]["allOf"][1]["properties"]["targetMayHaveMutated"]
            ["const"],
        false
    );
    // 结束测试。
    Ok(())
}

// 验证 stale epoch 与 confirmation-first 都先于 target/payload。
#[test]
fn parser_rejects_epoch_and_confirmation_before_deep_validation() {
    // 合法公开 URL 必须解析出真实 host。
    assert!(validation::http_url("https://example.test/path?x=1"));
    // 空 authority 必须拒绝。
    assert!(!validation::http_url("http:///path"));
    // 只有端口而没有 host 必须拒绝。
    assert!(!validation::http_url("http://:8080"));
    // 标准括号 IPv6 与端口必须允许。
    assert!(validation::http_url("http://[::1]:8080/"));
    // 构造 stale epoch 且 target 错误的 close command。
    let stale = json!({
        // 固定 request kind。
        "kind":"request",
        // 固定版本。
        "contractVersion":CONTRACT_VERSION,
        // 使用 canonical nonce。
        "requestNonce":NONCE,
        // 指纹故意错误但形状合法。
        "semanticFingerprint":"0000000000000000",
        // 使用旧 epoch。
        "expectedBrokerEpoch":"22222222222222222222222222222222",
        // 使用有效剩余预算。
        "remainingTimeoutMs":10,
        // 选择 close command。
        "operation":"close",
        // 显式确认 command。
        "confirmed":true,
        // target 故意包含私有文本。
        "sessionId":"CDP-private"
    });
    // stale epoch 必须胜过 target 与指纹深层校验。
    let stale_failure = BrowserSessionBrokerRequest::parse_for_epoch(&stale.to_string(), EPOCH)
        // 该输入必须失败。
        .expect_err("stale epoch must fail before target validation");
    // 核对 stale epoch 错误码。
    assert_eq!(
        // 读取稳定错误码。
        stale_failure.code(),
        // 核对 stale epoch。
        BrowserSessionBrokerProtocolErrorCode::StaleBrokerEpoch
    );
    // 建立当前 epoch 响应投影。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned()).expect("test epoch must be valid");
    // 已 canonical 的 stale 失败必须可构造 rejected final。
    let rejected = BrowserSessionBrokerResponse::rejected(&stale_failure, 0, &epoch)
        // parser 已提供完整公共关联。
        .expect("stale failure must project to rejected");
    // rejected 必须固定 stale 错误码。
    assert_eq!(rejected.error_code(), Some("STALE_BROKER_EPOCH"));
    // 构造当前 epoch、未确认且 target 错误的 close command。
    let mut unconfirmed = stale;
    // 改回当前 epoch。
    unconfirmed["expectedBrokerEpoch"] = json!(EPOCH);
    // 去掉确认事实。
    unconfirmed["confirmed"] = json!(false);
    // confirmation-first 必须胜过 target 与指纹。
    let failure = BrowserSessionBrokerRequest::parse_for_epoch(&unconfirmed.to_string(), EPOCH)
        // 该输入必须失败。
        .expect_err("unconfirmed command must fail");
    // 核对 confirmation 错误码。
    assert_eq!(
        // 读取稳定错误码。
        failure.code(),
        // 固定 confirmation required。
        BrowserSessionBrokerProtocolErrorCode::ConfirmationRequired
    );
    // envelope 已 canonical，因此可安全关联。
    assert!(failure.transport_accepted());
    // 只回显 request nonce，不回显 target。
    assert_eq!(failure.request_nonce(), Some(NONCE));
    // parser early failure 不得携带完整 strict request 语义键。
    assert!(failure.request_semantic_key().is_none());
    // 构造公共关联字段与 early failure 相同的严格 close request。
    let strict_request = conflicting_close_request()
        // 测试 strict request 必须可解析。
        .expect("strict close request must parse");
    // early failure 不得经 strict 构造器洗白为 request-bound response。
    assert!(
        BrowserSessionBrokerResponse::rejected_for_request(
            // 传入 parser early failure。
            &failure,
            // 即使 nonce、operation 与 expected epoch 相同也不允许。
            &strict_request,
            // 业务前终态固定 revision 零。
            0,
            // 回显当前 epoch。
            &epoch,
        )
        // 必须拒绝缺少完整 semantic key 的 failure。
        .is_none()
    );
    // 超限 frame 必须在 JSON 解析前失败且不可关联。
    let oversized = " ".repeat(MAXIMUM_INPUT_FRAME_BYTES + 1);
    // 核对固定输入上界。
    assert!(
        !BrowserSessionBrokerRequest::parse_for_epoch(&oversized, EPOCH)
            // 取得超限失败。
            .expect_err("oversized frame must fail")
            // 超限前没有可信 transport 关联。
            .transport_accepted()
    );
}

// 验证同 nonce 只 dispatch 一次，且快照只能来自 request-bound response。
#[test]
fn execution_ledger_dispatches_once_and_replays_terminal() -> Result<(), Box<dyn Error>> {
    // 建立当前 epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())?;
    // 建立完整 epoch state。
    let mut state = BrowserSessionBrokerEpochState::default();
    // 解析首次 open request。
    let request = open_request(100)?;
    // 首次观察只能 dispatch。
    assert!(matches!(
        // 在服务器单调时钟 1000ms 观察。
        state.observe_request(&epoch, 1_000, &request)?,
        // 核对 deadline 与首个 revision。
        BrowserSessionBrokerExecutionDecision::Dispatch {
            // 首次绝对期限应为 1100ms。
            deadline_ms: 1_100,
            // 首个 snapshot revision 为零。
            request_revision: 0
        }
    ));
    // replay reservation 必须在 business acceptance 前建立。
    assert!(state.execution_reserved_replay_bytes_for_test() > 0);
    // 新 record 已经占用固定容量。
    assert_eq!(state.execution_record_count_for_test(), 1);
    // 同 nonce 更大 remaining 不得延长原 deadline。
    let duplicate = open_request(500)?;
    // 同义重送只能 attach。
    assert!(matches!(
        // 下一毫秒重送。
        state.observe_request(&epoch, 1_001, &duplicate)?,
        // deadline 仍保持首次值。
        BrowserSessionBrokerExecutionDecision::Attach {
            // 不允许延长到 1501ms。
            deadline_ms: 1_100,
            // 尚无 snapshot，revision 仍为零。
            request_revision: 0,
            // 尚无 accepted response。
            response: None
        }
    ));
    // 构造另一 nonce 的严格 request。
    let foreign_request = open_request_for(OTHER_NONCE, 100)?;
    // 构造与另一 request 绑定的合法 accepted。
    let foreign_response = BrowserSessionBrokerResponse::accepted(&foreign_request, 0, &epoch);
    // EpochState 不接受没有对应 ledger request 的响应。
    assert!(state.apply_response(&epoch, foreign_response).is_err());
    // 构造试图跳过 accepted 的业务 completed final。
    let premature_completed = BrowserSessionBrokerResponse::finished(
        // 绑定已有 ledger request。
        &request,
        // 错误地使用首个 revision。
        0,
        // 回显当前 epoch。
        &epoch,
        // 声称业务已经完成。
        BrowserSessionBrokerOutcome::Completed,
        // open 成功数据本身合法。
        Some(BrowserSessionBrokerSuccess::Open {
            // 使用 canonical session identity。
            session_id: SESSION.to_owned(),
        }),
    )
    // response 构造合法，但状态机必须拒绝跳过 accepted。
    .expect("completed response shape must be valid");
    // business final 不得成为 revision 零的首个 snapshot。
    assert!(
        state
            // 应用提前到达的 final。
            .apply_response(&epoch, premature_completed)
            // 阶段门禁必须失败闭合。
            .is_err()
    );
    // 构造与原 request 绑定的 accepted revision 零。
    let accepted = BrowserSessionBrokerResponse::accepted(&request, 0, &epoch);
    // 只有封闭 response 能进入 ledger，调用方没有 snapshot 构造入口。
    state.apply_response(&epoch, accepted)?;
    // 同 revision accepted 不得覆盖当前状态。
    assert!(
        state
            // 再次提交 revision 零。
            .apply_response(
                // 绑定当前 epoch。
                &epoch,
                // 重新构造相同 accepted。
                BrowserSessionBrokerResponse::accepted(&request, 0, &epoch),
            )
            // 低 revision 必须失败。
            .is_err()
    );
    // 构造 revision 一的 completed final。
    let completed = BrowserSessionBrokerResponse::finished(
        // 绑定原 request。
        &request,
        // 状态推进严格加一。
        1,
        // 回显当前 epoch。
        &epoch,
        // 建立 completed outcome。
        BrowserSessionBrokerOutcome::Completed,
        // open 只携带 canonical session identity。
        Some(BrowserSessionBrokerSuccess::Open {
            // 使用公开 session identity。
            session_id: SESSION.to_owned(),
        }),
    )
    // 合法 operation-specific data 必须构造成功。
    .expect("open completed response must be valid");
    // 保存 terminal response。
    state.apply_response(&epoch, completed)?;
    // 终态重送只能 replay，绝不能 dispatch。
    assert!(matches!(
        // 再次观察同 request。
        state.observe_request(&epoch, 1_002, &request)?,
        // 核对 terminal replay。
        BrowserSessionBrokerExecutionDecision::Replay { response }
            // revision 与 outcome 必须保持 final。
            if response.request_revision() == 1
                && response.outcome() == Some(BrowserSessionBrokerOutcome::Completed)
    ));
    // final 后即使更高 revision accepted 也必须被 terminal-wins 拒绝。
    assert!(
        state
            // 尝试覆盖 terminal。
            .apply_response(
                // 绑定当前 epoch。
                &epoch,
                // 使用更高 revision 的 accepted。
                BrowserSessionBrokerResponse::accepted(&request, 2, &epoch),
            )
            // 终态后不得覆盖。
            .is_err()
    );
    // 结束测试。
    Ok(())
}

// 验证 cancel 先到会安装 tombstone 并阻止后到 request。
#[test]
fn cancel_before_request_installs_terminal_tombstone() -> Result<(), Box<dyn Error>> {
    // 建立当前 epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())?;
    // 建立空 epoch state。
    let mut state = BrowserSessionBrokerEpochState::default();
    // 解析 cancel frame。
    let cancel = cancel_request()?;
    // 未知 target 必须先安装 tombstone，再返回 unknown-request receipt。
    assert_eq!(
        // 在线性化点观察 cancel。
        state.observe_cancel(&epoch, &cancel)?,
        // 首次 receipt revision 为零。
        (BrowserSessionBrokerCancelStatus::UnknownRequest, 0)
    );
    // duplicate cancel 只 replay receipt，不重复改变 target。
    assert_eq!(
        // 重送完全相同的 cancel。
        state.observe_cancel(&epoch, &cancel)?,
        // 保持相同状态与 revision。
        (BrowserSessionBrokerCancelStatus::UnknownRequest, 0)
    );
    // 后到原 request 必须命中 tombstone并完成首次语义绑定。
    let request = open_request(100)?;
    // request 不得 dispatch。
    let decision = state.observe_request(&epoch, 1_000, &request)?;
    // 首个迟到 request 必须直接 replay 缓存 final。
    let BrowserSessionBrokerExecutionDecision::Replay { response } = decision else {
        // 任何 dispatch 或 attach 都违反取消先到语义。
        panic!("late request must replay tombstone final");
    };
    // tombstone final 不可重试且不改变 target。
    assert!(!response.retry_safe() && !response.target_may_have_mutated());
    // 核对固定 outcome。
    assert_eq!(
        // 读取 final outcome。
        response.outcome(),
        // 固定 cancelled-before-acceptance。
        Some(BrowserSessionBrokerOutcome::CancelledBeforeAcceptance)
    );
    // ledger 必须缓存同一 final response。
    assert_eq!(
        // 读取专用只读测试投影。
        state.execution_response_for_test(NONCE),
        // 核对首次 replay 值。
        Some(&response)
    );
    // 完全同义 request 必须 replay 同一 final。
    assert!(matches!(
        // 再次观察 open request。
        state.observe_request(&epoch, 1_001, &request)?,
        // 核对缓存 final 未变化。
        BrowserSessionBrokerExecutionDecision::Replay { response: replayed }
            // nonce、revision 与 outcome 必须稳定。
            if replayed == response
    ));
    // 构造相同 nonce 的另一份内部自洽语义。
    let conflicting = conflicting_close_request()?;
    // 墓碑绑定后必须拒绝异义 request。
    let conflict = state
        // 在线性化点观察 close。
        .observe_request(&epoch, 1_002, &conflicting)
        // 同 nonce 异义必须失败。
        .expect_err("bound tombstone must reject different semantics");
    // 核对稳定冲突错误码。
    assert_eq!(
        // 读取错误码。
        conflict.code(),
        // 使用 nonce semantic conflict。
        BrowserSessionBrokerProtocolErrorCode::NonceSemanticConflict
    );
    // ledger 拒绝必须带安全公共关联。
    assert!(conflict.transport_accepted());
    // 冲突必须可投影为 wire rejected final。
    let rejected = BrowserSessionBrokerResponse::rejected_for_request(
        // 绑定 ledger 返回的可关联 failure。
        &conflict,
        // 绑定导致冲突的完整严格 request。
        &conflicting,
        // 业务前拒绝固定 revision 零。
        0,
        // 回显当前 live epoch。
        &epoch,
    )
    // 关联信息完整时必须成功。
    .expect("ledger conflict must project to rejected");
    // 核对稳定 wire 错误码。
    assert_eq!(rejected.error_code(), Some("NONCE_SEMANTIC_CONFLICT"));
    // 结束测试。
    Ok(())
}

// 验证 accepted target 的 cancel 只能先请求协作停止并单调终结。
#[test]
fn accepted_cancel_is_requested_then_terminal() -> Result<(), Box<dyn Error>> {
    // 建立当前 epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())?;
    // 建立空 epoch state。
    let mut state = BrowserSessionBrokerEpochState::default();
    // 解析 open request。
    let request = open_request(100)?;
    // 首次 request 进入 execution ledger。
    state.observe_request(&epoch, 1_000, &request)?;
    // 保存 business accepted revision 零。
    state.apply_response(
        // 绑定当前 epoch。
        &epoch,
        // 只接受响应 Component 构造的封闭值。
        BrowserSessionBrokerResponse::accepted(&request, 0, &epoch),
    )?;
    // 解析 cancel。
    let cancel = cancel_request()?;
    // accepted 后 cancel 只能返回 cancellation-requested。
    assert_eq!(
        // 在线性化点观察 cancel。
        state.observe_cancel(&epoch, &cancel)?,
        // 首次 receipt revision 为零。
        (BrowserSessionBrokerCancelStatus::CancellationRequested, 0)
    );
    // 构造同 target 的第二个 cancel nonce。
    let second_cancel = cancel_request_for(OTHER_NONCE)?;
    // 第二个 cancel 也必须独立建立 revision 零中间态。
    assert_eq!(
        // 在线性化点观察第二个 cancel。
        state.observe_cancel(&epoch, &second_cancel)?,
        // 每个 cancel nonce 都有独立 revision。
        (BrowserSessionBrokerCancelStatus::CancellationRequested, 0)
    );
    // target 必须可观察到持久协作取消事实。
    assert!(state.cancellation_requested(NONCE));
    // 构造权威 cancelled 业务终态。
    let cancelled = BrowserSessionBrokerResponse::finished(
        // 绑定原始 request。
        &request,
        // accepted 后 revision 严格加一。
        1,
        // 绑定当前 epoch。
        &epoch,
        // 由业务执行事实决定取消终态。
        BrowserSessionBrokerOutcome::Cancelled,
        // cancelled 不携带成功 data。
        None,
    )
    // 合法 cancelled 必须可构造。
    .expect("cancelled response must be valid");
    // 保存业务终态会原子推进同 target 的全部 cancel receipt。
    state.apply_response(&epoch, cancelled)?;
    // 重送同一 cancel 只能 replay 已自动结算的终态。
    assert_eq!(
        // 再次观察相同 cancel nonce。
        state.observe_cancel(&epoch, &cancel)?,
        // status 为 cancelled 且 revision 加一。
        (BrowserSessionBrokerCancelStatus::Cancelled, 1)
    );
    // 同 target 的第二个 cancel receipt 必须一起收敛。
    assert_eq!(
        // 重送第二个 cancel。
        state.observe_cancel(&epoch, &second_cancel)?,
        // 它也独立推进到 revision 一。
        (BrowserSessionBrokerCancelStatus::Cancelled, 1)
    );
    // 结束测试。
    Ok(())
}

// 验证 response 从原 request 派生关联值并拒绝跨 operation 数据。
#[test]
fn response_combinations_are_request_bound_and_closed() -> Result<(), Box<dyn Error>> {
    // 建立当前 epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())?;
    // 解析 open request。
    let request = open_request(100)?;
    // accepted 必须从 request 派生 nonce 与 mutation 事实。
    let accepted = BrowserSessionBrokerResponse::accepted(&request, 0, &epoch);
    // 核对 request 关联。
    assert_eq!(accepted.request_nonce(), NONCE);
    // open accepted 必须保守标记 target 可能变化。
    assert!(accepted.target_may_have_mutated());
    // command unknown 必须固定 OutcomeUnknown。
    let unknown = BrowserSessionBrokerResponse::unknown(&request, 1, &epoch);
    // 核对 unknown 状态组合。
    assert!(unknown.outcome_unknown() && !unknown.completed());
    // deadline 到期必须构造业务前终态。
    let expired = BrowserSessionBrokerResponse::expired_before_acceptance(&request, 0, &epoch)
        // 严格 request 必须可关联。
        .expect("request expiration must be representable");
    // expired 必须固定 REQUEST_EXPIRED 错误码。
    assert_eq!(expired.error_code(), Some("REQUEST_EXPIRED"));
    // open 不得携带 screenshot 成功数据。
    assert!(
        BrowserSessionBrokerResponse::finished(
            // 绑定 open request。
            &request,
            // 使用下一 revision。
            1,
            // 回显当前 epoch。
            &epoch,
            // 声称 completed。
            BrowserSessionBrokerOutcome::Completed,
            // 故意提供错误 operation 的数据。
            Some(BrowserSessionBrokerSuccess::Screenshot(json!({}))),
        )
        // 必须拒绝跨 operation 数据。
        .is_none()
    );
    // 解析 screenshot request 以验证 operation-specific payload。
    let screenshot = screenshot_request()?;
    // 即使字段形状完整，伪 PNG 也不得成为 completed final。
    assert!(
        BrowserSessionBrokerResponse::finished(
            // 绑定 screenshot request。
            &screenshot,
            // 使用首个 final revision。
            0,
            // 回显当前 epoch。
            &epoch,
            // 声称 completed。
            BrowserSessionBrokerOutcome::Completed,
            // 提供签名错误的伪 PNG。
            Some(BrowserSessionBrokerSuccess::Screenshot(json!({
                // 回显原 request page。
                "pageId": PAGE,
                // 使用正导航代际。
                "navigationGeneration": 1,
                // 使用固定 MIME。
                "mimeType": "image/png",
                // Base64 可解码但内容不是 PNG。
                "pngBase64": "bm90LXBuZw==",
                // 保存解码后伪字节数。
                "pngBytes": 7,
                // 提供伪造宽度。
                "width": 1,
                // 提供伪造高度。
                "height": 1,
                // 提供形状合法但错误的摘要。
                "digest": "0000000000000000"
            }))),
        )
        // PNG signature 门禁必须拒绝。
        .is_none()
    );
    // client-local not-dispatched 不得伪装 broker 接受。
    let local = BrowserSessionBrokerClientLocalNotDispatched::new(&request);
    // 核对 client-local 事实。
    assert!(!local.transport_accepted() && local.retry_safe());
    // 结束测试。
    Ok(())
}
