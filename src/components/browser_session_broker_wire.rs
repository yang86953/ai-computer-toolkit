//! 编解码 browser-session broker v1 的封闭 JSON wire frame。
// 导入 JSON 构造与读取类型。
use serde_json::{Map, Value, json};

// 导入 request canonical helper 与协议常量。
use super::{
    // 导入严格 cancel 输入与错误码。
    BrowserSessionBrokerCancellationRequest,
    BrowserSessionBrokerProtocolErrorCode,
    // 导入请求、失败与版本常量。
    BrowserSessionBrokerProtocolFailure,
    BrowserSessionBrokerRequest,
    CONTRACT_VERSION,
    // 导入双向固定 frame 预算。
    MAXIMUM_INPUT_FRAME_BYTES,
    MAXIMUM_RESPONSE_FRAME_BYTES,
    // 导入同源 canonical key helper。
    append_canonical_piece,
    fingerprint_for_canonical_key,
};
// 导入封闭响应类型。
use super::response::{
    BrowserSessionBrokerCancelReceipt,
    // 导入 cancel 拒绝与 ready。
    BrowserSessionBrokerCancelRejected,
    // 导入 request response 投影。
    BrowserSessionBrokerReady,
    BrowserSessionBrokerResponse,
    // 导入预解析字节门禁与 cancel receipt。
    bounded_response_input,
};
// 导入 broker epoch 与 cancel status。
use super::state::{
    // 导入 cancel status 与 live epoch。
    BrowserSessionBrokerCancelStatus,
    BrowserSessionBrokerEpoch,
};

// 注册 accepted/final 的窄编解码子 Component。
#[path = "browser_session_broker_wire_response.rs"]
mod response_codec;
// 借用 response 专用编解码入口。
use response_codec::{decode_accepted, decode_final, encode_accepted, encode_final};

// 注册 session.inspect query builder 的窄编码子 Component。
#[path = "browser_session_broker_wire_session_inspect.rs"]
mod session_inspect_codec;
// 注册页面导航与只读查询 request builder 的窄编码子 Component。
#[path = "browser_session_broker_wire_page.rs"]
mod page_codec;
// 注册 wire Component 的纯单元测试。
#[cfg(test)]
#[path = "browser_session_broker_wire_tests.rs"]
mod tests;

// 保存已经由同源 parser 再验证的 client request frame。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionBrokerRequestFrame {
    // 保存有界 JSON 文本。
    text: String,
    // 保存严格 request 投影。
    request: BrowserSessionBrokerRequest,
}

// 保存已经由同源 parser 验证的 cancel request frame。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionBrokerCancelFrame {
    // 保存有界 JSON 文本。
    text: String,
    // 保存严格 cancel 投影。
    cancel: BrowserSessionBrokerCancellationRequest,
}

// 为协作取消提供严格 builder。
impl BrowserSessionBrokerCancelFrame {
    // 构造绑定 cancel nonce、目标 nonce 与 expected epoch 的 control frame。
    pub(crate) fn new(
        // 接收本次 cancel 的稳定 nonce。
        cancel_request_nonce: &str,
        // 接收目标 request nonce。
        request_nonce: &str,
        // 接收目标 request 所在 live epoch。
        expected_broker_epoch: &str,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // cancel 不携带 confirmed 或业务 payload。
        let value = json!({
            // 固定 cancel frame kind。
            "kind": "cancel",
            // 固定协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 保存 cancel 自身 nonce。
            "cancelRequestNonce": cancel_request_nonce,
            // 保存目标 request nonce。
            "requestNonce": request_nonce,
            // 绑定目标 live epoch。
            "expectedBrokerEpoch": expected_broker_epoch
        });
        // 编码 request 方向固定预算。
        let text = encode_bounded(&value, MAXIMUM_INPUT_FRAME_BYTES)?;
        // 委托生产 parser 严格验证字段形状。
        let cancel = BrowserSessionBrokerCancellationRequest::parse(&text)?;
        // 返回一一对应的 frame 与投影。
        Ok(Self { text, cancel })
    }

    // 返回可发送 JSON 文本。
    pub(crate) fn text(&self) -> &str {
        // 借用 wire 文本。
        &self.text
    }

    // 返回严格 cancel 投影。
    pub(crate) fn cancel(&self) -> &BrowserSessionBrokerCancellationRequest {
        // 借用 cancel。
        &self.cancel
    }
}

// 为冻结的 open/close command 提供 confirmed-only builder。
impl BrowserSessionBrokerRequestFrame {
    // 构造只能表达 confirmed=true 的 open command。
    pub(crate) fn open_confirmed(
        // 接收 canonical request nonce。
        request_nonce: &str,
        // 接收当前 broker epoch。
        expected_broker_epoch: &str,
        // 接收剩余 deadline 预算。
        remaining_timeout_ms: u32,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // open canonical 语义只包含版本、epoch 与 operation。
        let fingerprint = request_fingerprint(expected_broker_epoch, "open", None);
        // 构造精确字段集合并固化显式确认。
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
            // 固定 open operation。
            "operation": "open",
            // command builder 只能表达 confirmed=true。
            "confirmed": true
        });
        // 使用当前 epoch 的同源 parser 再验证 builder 输出。
        Self::from_value(value, expected_broker_epoch)
    }

    // 构造只能表达 confirmed=true 且 target 为 opaque s2:bs 的 close command。
    pub(crate) fn close_confirmed(
        // 接收 canonical request nonce。
        request_nonce: &str,
        // 接收当前 broker epoch。
        expected_broker_epoch: &str,
        // 接收剩余 deadline 预算。
        remaining_timeout_ms: u32,
        // 接收唯一公开 session target。
        session_id: &str,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // close canonical 语义包含 epoch、operation 与 opaque session。
        let fingerprint = request_fingerprint(expected_broker_epoch, "close", Some(session_id));
        // 构造精确字段集合并固化显式确认。
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
            // 固定 close operation。
            "operation": "close",
            // 保存唯一公开 session target。
            "sessionId": session_id,
            // command builder 只能表达 confirmed=true。
            "confirmed": true
        });
        // 使用当前 epoch 的同源 parser 严格拒绝非 s2:bs target。
        Self::from_value(value, expected_broker_epoch)
    }

    // 从 builder 值生成有界 wire 并再次解析。
    fn from_value(
        // 接收尚未信任的 builder JSON。
        value: Value,
        // 接收 parser 当前 epoch。
        current_epoch: &str,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // 序列化为稳定 JSON 文本。
        let text = encode_bounded(&value, MAXIMUM_INPUT_FRAME_BYTES)?;
        // 通过生产 parser 验证字段、确认、target 与 fingerprint。
        let request = BrowserSessionBrokerRequest::parse_for_epoch(&text, current_epoch)?;
        // 返回只包含严格 frame 的投影。
        Ok(Self { text, request })
    }

    // 返回可发送的 JSON 文本。
    pub(crate) fn text(&self) -> &str {
        // 借用 wire 文本。
        &self.text
    }

    // 返回与文本一一对应的严格 request。
    pub(crate) fn request(&self) -> &BrowserSessionBrokerRequest {
        // 借用 request 投影。
        &self.request
    }
}
// 编码 server-first broker-ready frame。
pub(crate) fn encode_ready(
    // 借用当前 live epoch。
    epoch: &BrowserSessionBrokerEpoch,
) -> Result<String, BrowserSessionBrokerProtocolFailure> {
    // ready 只回显版本与随机 epoch。
    let value = json!({
        // 固定 server-first ready kind。
        "kind": "broker-ready",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 回显当前 live epoch。
        "brokerEpoch": epoch.as_str()
    });
    // 实施 control/response frame 上限。
    encode_bounded(&value, MAXIMUM_RESPONSE_FRAME_BYTES)
}

// 严格解码 broker-ready 且拒绝额外字段。
pub(crate) fn decode_ready(
    // 借用 server-first frame。
    text: &str,
) -> Result<BrowserSessionBrokerReady, BrowserSessionBrokerProtocolFailure> {
    // 委托既有 ready parser 执行有界解析。
    BrowserSessionBrokerReady::parse(text)
}

// 编码严格 request-bound 响应或业务前早期拒绝。
pub(crate) fn encode_response(
    // 借用封闭响应投影。
    response: &BrowserSessionBrokerResponse,
    // 严格路径绑定原 request；早期 rejected 路径必须为空。
    request: Option<&BrowserSessionBrokerRequest>,
    // 绑定已认证连接握手得到的当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
) -> Result<String, BrowserSessionBrokerProtocolFailure> {
    // accepted 与 final 使用不同精确字段集合。
    let value = if response.outcome().is_none() {
        // accepted 只能从严格 request-bound 路径编码。
        encode_accepted(response, request.ok_or_else(invalid)?, current_epoch)?
    } else {
        // final 由子 Component 封闭区分 strict 与 early rejection。
        encode_final(response, request, current_epoch)?
    };
    // 在发送前实施冻结响应上限。
    encode_bounded(&value, MAXIMUM_RESPONSE_FRAME_BYTES)
}

// 严格解码 accepted/final，并绑定原始 request 防止跨请求重放。
pub(crate) fn decode_response(
    // 借用不可信 wire frame。
    text: &str,
    // 绑定调用方原始严格 request。
    request: &BrowserSessionBrokerRequest,
    // 绑定已认证连接握手得到的当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 在 JSON 解析前实施冻结字节门禁。
    let text = bounded_response_input(text)?;
    // 解析唯一 JSON 值。
    let value = serde_json::from_str::<Value>(text).map_err(|_| invalid())?;
    // 根必须是对象。
    let object = value.as_object().ok_or_else(invalid)?;
    // response decoder 明确拒绝 ready、request、cancel 与 cancel control。
    match required_string(object, "kind")? {
        // accepted 使用封闭解码器。
        "accepted" => decode_accepted(object, request, current_epoch),
        // final 使用 outcome 封闭解码器。
        "final" => decode_final(object, request, current_epoch),
        // 其余 frame kind 均不属于 response 通道。
        _ => Err(invalid()),
    }
}

// 编码 cancel receipt control frame。
pub(crate) fn encode_cancel_receipt(
    // 借用封闭 receipt。
    receipt: &BrowserSessionBrokerCancelReceipt,
    // 绑定已认证连接握手得到的当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
) -> Result<String, BrowserSessionBrokerProtocolFailure> {
    // receipt 必须属于冻结 revision/status 矩阵且回显认证 current epoch。
    if !receipt.valid_wire_revision() || receipt.broker_epoch() != current_epoch.as_str() {
        // 不发送 schema-invalid receipt。
        return Err(invalid());
    }
    // 构造精确 control 字段集合。
    let value = json!({
        // 固定 cancel receipt kind。
        "kind": "cancel-receipt",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 回显 cancel nonce。
        "cancelRequestNonce": receipt.cancel_request_nonce(),
        // 回显目标 request nonce。
        "requestNonce": receipt.request_nonce(),
        // 回显当前 broker epoch。
        "brokerEpoch": receipt.broker_epoch(),
        // 回显单调 cancel revision。
        "cancelRevision": receipt.cancel_revision(),
        // 回显封闭 cancel status。
        "status": cancel_status_text(receipt.status())
    });
    // 使用冻结 response 上限编码。
    encode_bounded(&value, MAXIMUM_RESPONSE_FRAME_BYTES)
}

// 严格解码 cancel receipt control frame。
pub(crate) fn decode_cancel_receipt(
    // 借用不可信 control frame。
    text: &str,
    // 绑定原始严格 cancel request。
    cancel: &BrowserSessionBrokerCancellationRequest,
    // 绑定已认证连接握手得到的当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
    // 借用该 cancel 上一次已接受的 receipt。
    previous: Option<&BrowserSessionBrokerCancelReceipt>,
) -> Result<BrowserSessionBrokerCancelReceipt, BrowserSessionBrokerProtocolFailure> {
    // 先执行有界 JSON 对象解析。
    let object = bounded_object(text)?;
    // 固定唯一字段集合。
    exact_keys(
        // 借用 control 对象。
        &object,
        // 固定 receipt 键集合。
        &[
            // 要求 kind。
            "kind",
            // 要求 contractVersion。
            "contractVersion",
            // 要求 cancelRequestNonce。
            "cancelRequestNonce",
            // 要求 requestNonce。
            "requestNonce",
            // 要求 brokerEpoch。
            "brokerEpoch",
            // 要求 cancelRevision。
            "cancelRevision",
            // 要求 status。
            "status",
        ],
    )?;
    // 固定 frame kind 与版本。
    fixed_envelope(&object, "cancel-receipt")?;
    // 读取封闭四态。
    let status = parse_cancel_status(required_string(&object, "status")?)?;
    // 委托响应类型重新校验关联 identity。
    let receipt = BrowserSessionBrokerCancelReceipt::from_wire(
        // 读取 cancel nonce。
        required_string(&object, "cancelRequestNonce")?.to_owned(),
        // 读取 target nonce。
        required_string(&object, "requestNonce")?.to_owned(),
        // 读取 broker epoch。
        required_string(&object, "brokerEpoch")?.to_owned(),
        // 读取 cancel revision。
        required_u64(&object, "cancelRevision")?,
        // 保存封闭 status。
        status,
    )
    // 非 canonical identity 闭合为 INVALID_ARGUMENT。
    .ok_or_else(invalid)?;
    // 双 nonce 与 epoch 必须逐字匹配原 cancel。
    require_cancel_identity(
        // 回显 cancel nonce。
        receipt.cancel_request_nonce(),
        // 回显 target nonce。
        receipt.request_nonce(),
        // 回显 broker epoch。
        receipt.broker_epoch(),
        // 绑定原 cancel。
        cancel,
        // 绑定认证连接当前 epoch。
        current_epoch,
        // receipt 不是 stale rejection。
        false,
    )?;
    // revision 与四态必须相对上次 receipt 单调。
    require_cancel_receipt_transition(previous, &receipt)?;
    // 返回不会串线的 receipt。
    Ok(receipt)
}

// 编码 cancel-rejected control frame。
pub(crate) fn encode_cancel_rejected(
    // 借用封闭拒绝投影。
    rejected: &BrowserSessionBrokerCancelRejected,
    // 绑定原始严格 cancel request。
    cancel: &BrowserSessionBrokerCancellationRequest,
    // 绑定已认证连接握手得到的当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
) -> Result<String, BrowserSessionBrokerProtocolFailure> {
    // 编码前复核双 nonce、current epoch 与 stale iff 关系。
    require_cancel_identity(
        // 回显 cancel nonce。
        rejected.cancel_request_nonce(),
        // 回显目标 request nonce。
        rejected.request_nonce(),
        // 回显 broker epoch。
        rejected.broker_epoch(),
        // 绑定原 cancel。
        cancel,
        // 绑定认证连接当前 epoch。
        current_epoch,
        // 仅 stale code 使用 current!=expected。
        rejected.error_code() == "STALE_BROKER_EPOCH",
    )?;
    // 保留安全错误说明以支持 replay 无损编码。
    let error = safe_error_value(rejected.error_code(), rejected.error_message())?;
    // 构造精确 control 字段集合。
    let value = json!({
        // 固定 cancel rejection kind。
        "kind": "cancel-rejected",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 回显 cancel nonce。
        "cancelRequestNonce": rejected.cancel_request_nonce(),
        // 回显 target nonce。
        "requestNonce": rejected.request_nonce(),
        // 回显 broker epoch。
        "brokerEpoch": rejected.broker_epoch(),
        // 固定 transport 已接受。
        "transportAccepted": true,
        // 固定 business 未接受。
        "businessAccepted": false,
        // 固定可信完成。
        "completed": true,
        // 保存安全错误对象。
        "error": error
    });
    // 使用冻结 response 上限编码。
    encode_bounded(&value, MAXIMUM_RESPONSE_FRAME_BYTES)
}

// 严格解码 cancel-rejected control frame。
pub(crate) fn decode_cancel_rejected(
    // 借用不可信 control frame。
    text: &str,
    // 绑定原始严格 cancel request。
    cancel: &BrowserSessionBrokerCancellationRequest,
    // 绑定已认证连接握手得到的当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
) -> Result<BrowserSessionBrokerCancelRejected, BrowserSessionBrokerProtocolFailure> {
    // 先执行有界 JSON 对象解析。
    let object = bounded_object(text)?;
    // 固定唯一字段集合。
    exact_keys(
        // 借用 control 对象。
        &object,
        // 固定 rejection 键集合。
        &[
            // 要求 kind。
            "kind",
            // 要求 contractVersion。
            "contractVersion",
            // 要求 cancelRequestNonce。
            "cancelRequestNonce",
            // 要求 requestNonce。
            "requestNonce",
            // 要求 brokerEpoch。
            "brokerEpoch",
            // 要求 transportAccepted。
            "transportAccepted",
            // 要求 businessAccepted。
            "businessAccepted",
            // 要求 completed。
            "completed",
            // 要求 error。
            "error",
        ],
    )?;
    // 固定 frame kind 与版本。
    fixed_envelope(&object, "cancel-rejected")?;
    // 固定业务前拒绝状态组合。
    require_bool(&object, "transportAccepted", true)?;
    // cancel 尚未进入业务接受点。
    require_bool(&object, "businessAccepted", false)?;
    // cancel-rejected 是可信终态。
    require_bool(&object, "completed", true)?;
    // 读取并校验安全错误对象。
    let (error_code, error_message) = parse_safe_error(object.get("error"))?;
    // 委托响应类型执行 cancel 专用错误白名单。
    let rejected = BrowserSessionBrokerCancelRejected::from_wire(
        // 读取 cancel nonce。
        required_string(&object, "cancelRequestNonce")?.to_owned(),
        // 读取 target nonce。
        required_string(&object, "requestNonce")?.to_owned(),
        // 读取 broker epoch。
        required_string(&object, "brokerEpoch")?.to_owned(),
        // 保存封闭 error code。
        error_code,
        // 保存安全 error message。
        error_message,
    )
    // 非 canonical 字段闭合为 INVALID_ARGUMENT。
    .ok_or_else(invalid)?;
    // 双 nonce 与 epoch 必须逐字匹配原 cancel。
    require_cancel_identity(
        // 回显 cancel nonce。
        rejected.cancel_request_nonce(),
        // 回显 target nonce。
        rejected.request_nonce(),
        // 回显 broker epoch。
        rejected.broker_epoch(),
        // 绑定原 cancel。
        cancel,
        // 绑定认证连接当前 epoch。
        current_epoch,
        // 仅 stale code 使用 current epoch 规则。
        rejected.error_code() == "STALE_BROKER_EPOCH",
    )?;
    // 返回不会串线的拒绝。
    Ok(rejected)
}

// 计算与 request parser 完全同源的 open/close 指纹。
fn request_fingerprint(epoch: &str, operation: &str, target: Option<&str>) -> String {
    // 从协议版本开始构造 canonical key。
    let mut key = String::from(CONTRACT_VERSION);
    // expected epoch 属于不可变语义。
    append_canonical_piece(&mut key, epoch);
    // 追加 operation。
    append_canonical_piece(&mut key, operation);
    // close 才追加 session target。
    if let Some(target) = target {
        // 追加 opaque target 原文。
        append_canonical_piece(&mut key, target);
    }
    // 使用 parser 同源 FNV-1a helper。
    fingerprint_for_canonical_key(&key)
}

// 将 JSON 值编码为冻结大小内的 UTF-8 文本。
fn encode_bounded(
    // 借用已封闭 JSON 值。
    value: &Value,
    // 接收方向特定上限。
    maximum_bytes: usize,
) -> Result<String, BrowserSessionBrokerProtocolFailure> {
    // serde_json 序列化内存值不应失败，失败仍闭合。
    let text = serde_json::to_string(value).map_err(|_| invalid())?;
    // 拒绝任何超出方向预算的 frame。
    if text.len() > maximum_bytes {
        // 不发送超限内容。
        return Err(invalid());
    }
    // 返回有界文本。
    Ok(text)
}
// 有界解析一个独立 JSON 对象。
fn bounded_object(
    // 借用不可信 control frame。
    text: &str,
) -> Result<Map<String, Value>, BrowserSessionBrokerProtocolFailure> {
    // 先实施 response/control 方向字节门禁。
    let text = bounded_response_input(text)?;
    // 解析唯一 JSON 值。
    let value = serde_json::from_str::<Value>(text).map_err(|_| invalid())?;
    // 根必须是对象并转为自有 Map。
    value.as_object().cloned().ok_or_else(invalid)
}

// 校验精确键集合并拒绝未知字段。
pub(super) fn exact_keys(
    // 借用 JSON 对象。
    object: &Map<String, Value>,
    // 接收冻结键集合。
    keys: &[&str],
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // 长度和每个键必须完全一致。
    if object.len() != keys.len()
        // 每个实际键都必须属于冻结集合。
        || !object.keys().all(|key| keys.contains(&key.as_str()))
    // 任一不匹配都进入拒绝分支。
    {
        // 拒绝缺失或额外字段。
        return Err(invalid());
    }
    // 返回精确集合事实。
    Ok(())
}

// 校验 control frame 的固定 kind 与版本。
fn fixed_envelope(
    // 借用 control 对象。
    object: &Map<String, Value>,
    // 接收期望 kind。
    kind: &str,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // kind 与版本必须逐字匹配。
    if required_string(object, "kind")? != kind
        // 版本必须逐字匹配 v1。
        || required_string(object, "contractVersion")? != CONTRACT_VERSION
    // 任一不匹配都进入拒绝分支。
    {
        // 拒绝错误 envelope。
        return Err(invalid());
    }
    // 返回 envelope 合法事实。
    Ok(())
}

// 读取必需字符串字段。
pub(super) fn required_string<'a>(
    // 借用 JSON 对象。
    object: &'a Map<String, Value>,
    // 接收字段名。
    name: &str,
) -> Result<&'a str, BrowserSessionBrokerProtocolFailure> {
    // 只接受 JSON string。
    object
        // 查找字段。
        .get(name)
        // 投影字符串。
        .and_then(Value::as_str)
        // 缺失或错类型闭合失败。
        .ok_or_else(invalid)
}

// 读取必需无符号整数字段。
pub(super) fn required_u64(
    // 借用 JSON 对象。
    object: &Map<String, Value>,
    // 接收字段名。
    name: &str,
) -> Result<u64, BrowserSessionBrokerProtocolFailure> {
    // 只接受可无损表达的 JSON 非负整数。
    object
        // 查找字段。
        .get(name)
        // 投影 u64。
        .and_then(Value::as_u64)
        // 缺失、负数或浮点闭合失败。
        .ok_or_else(invalid)
}

// 要求布尔字段等于冻结值。
pub(super) fn require_bool(
    // 借用 JSON 对象。
    object: &Map<String, Value>,
    // 接收字段名。
    name: &str,
    // 接收期望值。
    expected: bool,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // 类型和值必须同时匹配。
    if object.get(name).and_then(Value::as_bool) != Some(expected) {
        // 拒绝错类型或错状态。
        return Err(invalid());
    }
    // 返回匹配事实。
    Ok(())
}

// 要求字段显式为 null。
pub(super) fn require_null(
    // 借用 JSON 对象。
    object: &Map<String, Value>,
    // 接收字段名。
    name: &str,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // 字段必须存在且为 JSON null。
    if !object.get(name).is_some_and(Value::is_null) {
        // 拒绝缺失或非 null。
        return Err(invalid());
    }
    // 返回 null 事实。
    Ok(())
}

// 解析并验证 safeError 精确对象。
pub(super) fn parse_safe_error(
    // 借用可选 error 值。
    value: Option<&Value>,
) -> Result<(String, String), BrowserSessionBrokerProtocolFailure> {
    // error 必须是对象。
    let object = value.and_then(Value::as_object).ok_or_else(invalid)?;
    // 字段集合固定为 code/message。
    exact_keys(object, &["code", "message"])?;
    // 读取字符串字段。
    let code = required_string(object, "code")?;
    // 读取安全说明。
    let message = required_string(object, "message")?;
    // 校验冻结边界。
    if !safe_error(code, message) {
        // 拒绝非法 error 内容。
        return Err(invalid());
    }
    // 返回自有值供响应保存与 replay。
    Ok((code.to_owned(), message.to_owned()))
}

// 构造已经验证的 safeError JSON 值。
pub(super) fn safe_error_value(
    // 借用稳定错误码。
    code: &str,
    // 借用安全说明。
    message: &str,
) -> Result<Value, BrowserSessionBrokerProtocolFailure> {
    // 编码前再次执行同一验证。
    if !safe_error(code, message) {
        // 拒绝不安全响应。
        return Err(invalid());
    }
    // 构造精确 safeError 对象。
    Ok(json!({ "code": code, "message": message }))
}

// 校验 safeError 字符与长度边界。
fn safe_error(code: &str, message: &str) -> bool {
    // code 长度为一到六十四字节。
    let code_length = (1..=64).contains(&code.len());
    // 首字符必须为大写字母。
    let first = code.as_bytes().first().is_some_and(u8::is_ascii_uppercase);
    // 后续只允许大写、数字或下划线。
    let tail = code
        // 转为 ASCII 字节迭代。
        .bytes()
        // 跳过首字符。
        .skip(1)
        // 检查全部剩余字符。
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
    // message 按 Unicode scalar 数量执行一到五百一十二边界。
    let message_length = (1..=512).contains(&message.chars().count());
    // 所有边界必须同时满足。
    code_length && first && tail && message_length
}

// 将 cancel status 映射为冻结标签。
fn cancel_status_text(status: BrowserSessionBrokerCancelStatus) -> &'static str {
    // 穷举四态。
    match status {
        // 映射未知 target 终态。
        BrowserSessionBrokerCancelStatus::UnknownRequest => "unknown-request",
        // 映射协作取消请求中间态。
        BrowserSessionBrokerCancelStatus::CancellationRequested => "cancellation-requested",
        // 映射太晚终态。
        BrowserSessionBrokerCancelStatus::TooLate => "too-late",
        // 映射已取消终态。
        BrowserSessionBrokerCancelStatus::Cancelled => "cancelled",
    }
}

// 解析冻结 cancel status 标签。
fn parse_cancel_status(
    // 借用 status 文本。
    value: &str,
) -> Result<BrowserSessionBrokerCancelStatus, BrowserSessionBrokerProtocolFailure> {
    // 只允许四个冻结标签。
    match value {
        // 恢复未知 target 终态。
        "unknown-request" => Ok(BrowserSessionBrokerCancelStatus::UnknownRequest),
        // 恢复协作取消中间态。
        "cancellation-requested" => Ok(BrowserSessionBrokerCancelStatus::CancellationRequested),
        // 恢复太晚终态。
        "too-late" => Ok(BrowserSessionBrokerCancelStatus::TooLate),
        // 恢复已取消终态。
        "cancelled" => Ok(BrowserSessionBrokerCancelStatus::Cancelled),
        // 拒绝未知状态。
        _ => Err(invalid()),
    }
}

// 校验 control response 与原 cancel 的三项关联 identity。
fn require_cancel_identity(
    // 借用 response cancel nonce。
    cancel_nonce: &str,
    // 借用 response target nonce。
    request_nonce: &str,
    // 借用 response broker epoch。
    epoch: &str,
    // 绑定原始严格 cancel。
    cancel: &BrowserSessionBrokerCancellationRequest,
    // 绑定已认证连接握手得到的当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
    // 指示是否为 stale epoch 专用拒绝。
    stale_rejection: bool,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // 三项 identity 必须逐字匹配原 control request。
    if cancel_nonce != cancel.cancel_request_nonce()
        // target nonce 必须匹配原 cancel。
        || request_nonce != cancel.request_nonce()
        // frame epoch 必须逐字匹配认证连接当前 epoch。
        || epoch != current_epoch.as_str()
        // epoch 规则由 stale rejection 与否决定。
        || if stale_rejection {
            // stale rejection 必须回显另一个当前 epoch。
            current_epoch.as_str() == cancel.expected_broker_epoch()
        } else {
            // 其余 control response 必须留在 expected epoch。
            current_epoch.as_str() != cancel.expected_broker_epoch()
        }
    // 任一不匹配都进入拒绝分支。
    {
        // 拒绝并发 cancel 串线或跨 epoch response。
        return Err(invalid());
    }
    // 返回完整关联事实。
    Ok(())
}

// 校验同一 cancel receipt 的首次与后续单调转换。
fn require_cancel_receipt_transition(
    // 借用上一次已接受 receipt。
    previous: Option<&BrowserSessionBrokerCancelReceipt>,
    // 借用本次新 receipt。
    current: &BrowserSessionBrokerCancelReceipt,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // 首次观察可能已经丢失 revision 零并收到 terminal replay。
    let Some(previous) = previous else {
        // 允许 rev0 任意初态，或 rev1 的两个业务终态。
        let valid = current.cancel_revision() == 0
            // 丢失初次回执后允许直接恢复 terminal rev1。
            || (current.cancel_revision() == 1
                // rev1 只能是权威取消或太晚。
                && matches!(
                    // 读取当前 terminal status。
                    current.status(),
                    // 允许 cancelled。
                    BrowserSessionBrokerCancelStatus::Cancelled
                        // 允许 too-late。
                        | BrowserSessionBrokerCancelStatus::TooLate
                ));
        // 拒绝更大 revision 或 rev1 非终态。
        if !valid {
            // 保持恢复集合封闭。
            return Err(invalid());
        }
        // 返回首次或丢包恢复事实。
        return Ok(());
    };
    // 上一次 receipt 也必须关联同一 control request。
    if previous.cancel_request_nonce() != current.cancel_request_nonce()
        // target nonce 必须匹配 cursor。
        || previous.request_nonce() != current.request_nonce()
        // broker epoch 必须匹配 cursor。
        || previous.broker_epoch() != current.broker_epoch()
    // 任一不匹配都进入拒绝分支。
    {
        // 拒绝跨 cancel cursor。
        return Err(invalid());
    }
    // 同 revision 只能逐字 replay 同一状态。
    if current.cancel_revision() == previous.cancel_revision() {
        // 状态不一致表示 revision 漂移。
        if current.status() != previous.status() {
            // 拒绝同 revision 改写。
            return Err(invalid());
        }
        // 接受幂等 replay。
        return Ok(());
    }
    // 唯一推进是 cancellation-requested revision 加一后终结。
    let expected = previous
        // 读取上次 revision。
        .cancel_revision()
        // 防止整数溢出。
        .checked_add(1)
        // 溢出闭合失败。
        .ok_or_else(invalid)?;
    // 校验连续 revision 与封闭状态转换。
    if current.cancel_revision() != expected
        // 唯一可推进源状态是 cancellation-requested。
        || previous.status() != BrowserSessionBrokerCancelStatus::CancellationRequested
        // 目标状态只能是两个业务终态。
        || !matches!(
            // 读取当前 status。
            current.status(),
            // 允许 cancelled。
            BrowserSessionBrokerCancelStatus::Cancelled
                // 允许 too-late。
                | BrowserSessionBrokerCancelStatus::TooLate
        )
    // 任一不匹配都进入拒绝分支。
    {
        // 拒绝跳号、倒退或终态再推进。
        return Err(invalid());
    }
    // 返回单调推进事实。
    Ok(())
}

// 构造 wire Component 的统一 INVALID_ARGUMENT。
pub(super) const fn invalid() -> BrowserSessionBrokerProtocolFailure {
    // 只返回稳定错误码且不回显不可信内容。
    BrowserSessionBrokerProtocolFailure::new(
        // 固定 INVALID_ARGUMENT。
        BrowserSessionBrokerProtocolErrorCode::InvalidArgument,
    )
}
