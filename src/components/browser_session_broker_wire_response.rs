//! 编解码 request-bound accepted 与 final 响应。

// 导入 JSON 构造与读取类型。
use serde_json::{Map, Value, json};

// 导入父 wire Component 的严格 helper。
use super::{
    // 导入对象形状与 safeError helper。
    exact_keys,
    invalid,
    parse_safe_error,
    require_bool,
    require_null,
    required_string,
    // 导入整数与 safeError 编码 helper。
    required_u64,
    safe_error_value,
};
// 导入协议 request、operation 与错误类型。
use super::super::{
    // 导入 operation 与协议错误码。
    BrowserSessionBrokerOperation,
    BrowserSessionBrokerProtocolErrorCode,
    // 导入失败、request 与协议版本。
    BrowserSessionBrokerProtocolFailure,
    BrowserSessionBrokerRequest,
    CONTRACT_VERSION,
};
// 导入封闭响应类型。
use super::super::response::{
    // 导入 outcome 与 response 投影。
    BrowserSessionBrokerOutcome,
    BrowserSessionBrokerResponse,
};
// 导入 live broker epoch。
use super::super::state::BrowserSessionBrokerEpoch;

// 注册 operation-specific success data 的窄编解码子 Component。
#[path = "browser_session_broker_wire_success.rs"]
mod success_codec;
// 借用 success data 专用编解码入口。
use success_codec::{parse_success, success_value};

// 编码 accepted 的精确字段集合。
pub(super) fn encode_accepted(
    // 借用响应投影。
    response: &BrowserSessionBrokerResponse,
    // 绑定原始严格 request。
    request: &BrowserSessionBrokerRequest,
    // 绑定已认证连接握手得到的当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
) -> Result<Value, BrowserSessionBrokerProtocolFailure> {
    // accepted 必须满足唯一状态组合。
    if !request_binding_valid(response, request)
        // accepted 尚无 final outcome。
        || response.outcome().is_some()
        // accepted 必须 transport accepted。
        || !response.transport_accepted()
        // accepted 必须 business accepted。
        || !response.business_accepted()
        // accepted 尚未 completed。
        || response.completed()
        // accepted 不授权 retry。
        || response.retry_safe()
        // accepted 不是 unknown。
        || response.outcome_unknown()
        // accepted 不携带 success。
        || response.success().is_some()
        // accepted 不携带 error code。
        || response.error_code().is_some()
        // accepted 不携带 error message。
        || response.error_message().is_some()
        // accepted 必须是首个 revision 零。
        || response.request_revision() != 0
    // 任一不匹配都进入拒绝分支。
    {
        // 拒绝非法 accepted 投影。
        return Err(invalid());
    }
    // 构造 schema 精确 accepted 对象。
    let value = json!({
        // 固定 accepted kind。
        "kind": "accepted",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 回显 request nonce。
        "requestNonce": response.request_nonce(),
        // 回显 broker epoch。
        "brokerEpoch": response.broker_epoch(),
        // 回显 request revision。
        "requestRevision": response.request_revision(),
        // 回显 operation 标签。
        "operation": operation_text(response.operation()),
        // 固定 transport accepted。
        "transportAccepted": true,
        // 固定 business accepted。
        "businessAccepted": true,
        // accepted 尚未 completed。
        "completed": false,
        // 保存 request role 派生 mutation flag。
        "targetMayHaveMutated": response.target_may_have_mutated()
    });
    // 复用 request/current-bound decoder 拒绝关联或 flag 漂移。
    decode_accepted(
        value.as_object().ok_or_else(invalid)?,
        request,
        current_epoch,
    )?;
    // 返回经过同源解码验证的值。
    Ok(value)
}

// 编码 final 的精确字段集合。
pub(super) fn encode_final(
    // 借用响应投影。
    response: &BrowserSessionBrokerResponse,
    // 严格路径绑定 request；early rejected 路径必须为空。
    request: Option<&BrowserSessionBrokerRequest>,
    // 绑定已认证连接握手得到的当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
) -> Result<Value, BrowserSessionBrokerProtocolFailure> {
    // final 必须携带封闭 outcome。
    let outcome = response.outcome().ok_or_else(invalid)?;
    // 严格路径核对完整语义，早期路径核对唯一无语义键终态。
    match request {
        // 严格响应必须逐字绑定原 request。
        Some(request) if request_binding_valid(response, request) => {}
        // early response 必须是与当前连接一致的 rejected。
        None if response.is_early_rejection_for(current_epoch) => {}
        // 拒绝 early response 被 strict request 认领或任何关联漂移。
        _ => return Err(invalid()),
    }
    // 业务前 final 为 rev0，业务接受后 final 为 rev1。
    let expected_revision = if response.business_accepted() {
        // accepted 后业务 final 固定为一。
        1
    } else {
        // 业务前 final 固定为零。
        0
    };
    // 拒绝跳号或任意 revision。
    if response.request_revision() != expected_revision {
        // 不编码 revision 漂移。
        return Err(invalid());
    }
    // 将 operation-specific success 映射为封闭 data。
    let data = success_value(response.success())?;
    // 保留验证后的 safeError code/message。
    let error = match (response.error_code(), response.error_message()) {
        // 无错误必须同时无说明。
        (None, None) => Value::Null,
        // 有错误必须同时有安全说明。
        (Some(code), Some(message)) => safe_error_value(code, message)?,
        // 拒绝半个 safeError。
        _ => return Err(invalid()),
    };
    // 构造 schema 精确 final 对象。
    let value = json!({
        // 固定 final kind。
        "kind": "final",
        // 固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 回显 request nonce。
        "requestNonce": response.request_nonce(),
        // 回显 broker epoch。
        "brokerEpoch": response.broker_epoch(),
        // 回显 request revision。
        "requestRevision": response.request_revision(),
        // 回显 operation 标签。
        "operation": operation_text(response.operation()),
        // 回显 outcome 标签。
        "outcome": outcome_text(outcome),
        // 回显 transport 事实。
        "transportAccepted": response.transport_accepted(),
        // 回显 business 事实。
        "businessAccepted": response.business_accepted(),
        // 回显 completed 事实。
        "completed": response.completed(),
        // 回显 retry 事实。
        "retrySafe": response.retry_safe(),
        // 回显 unknown 事实。
        "outcomeUnknown": response.outcome_unknown(),
        // 回显 mutation 事实。
        "targetMayHaveMutated": response.target_may_have_mutated(),
        // 保存 operation-specific data。
        "data": data,
        // 保存 safeError 或 null。
        "error": error
    });
    // strict 路径复用 request/current-bound decoder 拒绝所有字段漂移。
    if let Some(request) = request {
        // 解码验证并丢弃临时投影。
        decode_final(
            value.as_object().ok_or_else(invalid)?,
            request,
            current_epoch,
        )?;
    }
    // 返回经过同源解码验证的值。
    Ok(value)
}

// 校验 response 与调用方严格 request 的完整语义绑定。
fn request_binding_valid(
    // 借用 response 投影。
    response: &BrowserSessionBrokerResponse,
    // 绑定原始严格 request。
    request: &BrowserSessionBrokerRequest,
) -> bool {
    // nonce 与 operation 必须逐字匹配。
    response.request_nonce() == request.request_nonce()
        // operation 必须匹配。
        && response.operation() == request.operation()
        // strict 路径必须存在完整语义并逐字匹配。
        && response.request_semantic_key().is_some_and(|key| {
            // 比较 canonical semantic key。
            key == request.canonical_semantic_key()
        })
        // response 保存的 envelope expected epoch 必须匹配 strict request。
        && response.expected_broker_epoch() == request.expected_broker_epoch()
}

// 解码 accepted 并绑定原始 request。
pub(super) fn decode_accepted(
    // 借用 JSON 对象。
    object: &Map<String, Value>,
    // 绑定原始严格 request。
    request: &BrowserSessionBrokerRequest,
    // 绑定已认证连接握手得到的当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 固定 accepted 字段集合。
    exact_keys(
        // 借用 accepted 对象。
        object,
        // 固定 accepted 键集合。
        &[
            // 要求 kind。
            "kind",
            // 要求 contractVersion。
            "contractVersion",
            // 要求 requestNonce。
            "requestNonce",
            // 要求 brokerEpoch。
            "brokerEpoch",
            // 要求 requestRevision。
            "requestRevision",
            // 要求 operation。
            "operation",
            // 要求 transportAccepted。
            "transportAccepted",
            // 要求 businessAccepted。
            "businessAccepted",
            // 要求 completed。
            "completed",
            // 要求 targetMayHaveMutated。
            "targetMayHaveMutated",
        ],
    )?;
    // 校验版本、nonce、operation 与 expected epoch 关联。
    decode_response_identity(object, "accepted", request, current_epoch)?;
    // 非 stale accepted 必须位于 request expected epoch。
    require_final_epoch(request, current_epoch, false)?;
    // 固定 accepted 状态组合。
    require_bool(object, "transportAccepted", true)?;
    // accepted 必须越过业务接受点。
    require_bool(object, "businessAccepted", true)?;
    // accepted 尚不是 final。
    require_bool(object, "completed", false)?;
    // mutation 事实必须从 request role 派生。
    require_bool(object, "targetMayHaveMutated", request.may_mutate_target())?;
    // accepted 必须是首个 response revision 零。
    if required_u64(object, "requestRevision")? != 0 {
        // 拒绝 accepted revision 漂移。
        return Err(invalid());
    }
    // 构造封闭 accepted 响应。
    Ok(BrowserSessionBrokerResponse::accepted(
        // 绑定原 request。
        request,
        // accepted revision 固定为零。
        0,
        // 绑定认证连接当前 epoch。
        current_epoch,
    ))
}

// 解码 final 并绑定原始 request。
pub(super) fn decode_final(
    // 借用 JSON 对象。
    object: &Map<String, Value>,
    // 绑定原始严格 request。
    request: &BrowserSessionBrokerRequest,
    // 绑定已认证连接握手得到的当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 固定 final 字段集合。
    exact_keys(
        // 借用 final 对象。
        object,
        // 固定 final 键集合。
        &[
            // 要求 kind。
            "kind",
            // 要求 contractVersion。
            "contractVersion",
            // 要求 requestNonce。
            "requestNonce",
            // 要求 brokerEpoch。
            "brokerEpoch",
            // 要求 requestRevision。
            "requestRevision",
            // 要求 operation。
            "operation",
            // 要求 outcome。
            "outcome",
            // 要求 transportAccepted。
            "transportAccepted",
            // 要求 businessAccepted。
            "businessAccepted",
            // 要求 completed。
            "completed",
            // 要求 retrySafe。
            "retrySafe",
            // 要求 outcomeUnknown。
            "outcomeUnknown",
            // 要求 targetMayHaveMutated。
            "targetMayHaveMutated",
            // 要求 data。
            "data",
            // 要求 error。
            "error",
        ],
    )?;
    // 校验版本、nonce 与 operation 关联；epoch 由 outcome 专用规则校验。
    decode_response_identity(object, "final", request, current_epoch)?;
    // 所有 broker wire final 都必须 transport accepted。
    require_bool(object, "transportAccepted", true)?;
    // 读取单调 revision。
    let revision = required_u64(object, "requestRevision")?;
    // 按 outcome 执行封闭状态与 payload 校验。
    match required_string(object, "outcome")? {
        // parser/ledger/domain 的业务前拒绝。
        "rejected" => decode_rejected(object, request, revision, current_epoch),
        // 业务接受前 deadline 终态。
        "expired-before-acceptance" => decode_expired(object, request, revision, current_epoch),
        // cancel tombstone 的业务前终态。
        "cancelled-before-acceptance" => {
            // 解码不可重试的取消先到终态。
            decode_cancelled_before(object, request, revision, current_epoch)
        }
        // operation-specific 成功终态。
        "completed" => decode_completed(object, request, revision, current_epoch),
        // 业务接受后的失败终态。
        "failed" => decode_business_error(
            // 借用 final 对象。
            object,
            // 绑定 request。
            request,
            // 保存 revision。
            revision,
            // 绑定 epoch。
            current_epoch,
            // 固定 failed outcome。
            BrowserSessionBrokerOutcome::Failed,
        ),
        // 业务接受后的取消终态。
        "cancelled" => decode_business_error(
            // 借用 final 对象。
            object,
            // 绑定 request。
            request,
            // 保存 revision。
            revision,
            // 绑定 epoch。
            current_epoch,
            // 固定 cancelled outcome。
            BrowserSessionBrokerOutcome::Cancelled,
        ),
        // 业务接受后失去可信最终结果。
        "unknown" => decode_unknown(object, request, revision, current_epoch),
        // 拒绝未知 outcome。
        _ => Err(invalid()),
    }
}

// 解码业务前 rejected。
fn decode_rejected(
    // 借用 final 对象。
    object: &Map<String, Value>,
    // 绑定原始 request。
    request: &BrowserSessionBrokerRequest,
    // 接收 revision。
    revision: u64,
    // 借用当前 epoch。
    epoch: &BrowserSessionBrokerEpoch,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 业务前 final 必须是首个 revision 零。
    require_revision(revision, 0)?;
    // rejected 是可重试业务前终态。
    require_pre_acceptance(object, true)?;
    // rejected 不携带 data。
    require_null(object, "data")?;
    // 读取安全错误对象。
    let (code, message) = parse_safe_error(object.get("error"))?;
    // stale rejection 必须回显另一个 canonical 当前 epoch。
    require_final_epoch(request, epoch, code == "STALE_BROKER_EPOCH")?;
    // 错误码必须属于 request rejection 白名单。
    let error_code = BrowserSessionBrokerProtocolErrorCode::from_rejection_str(&code)
        // 拒绝任何未冻结的 request rejection 错误码。
        .ok_or_else(invalid)?;
    // 从严格 request 构造可关联 failure。
    let failure = BrowserSessionBrokerProtocolFailure::rejected_for_request(error_code, request);
    // 从 failure 构造封闭 rejected 响应。
    let mut response = BrowserSessionBrokerResponse::rejected_for_request(
        // 绑定已验证的 failure。
        &failure, // 保存完整 strict request semantic key。
        request,  // 保存首个 revision。
        revision, // 回显认证连接当前 epoch。
        epoch,
    )
    // failure 必须包含安全关联字段。
    .ok_or_else(invalid)?;
    // 保留 wire safeError message。
    response.replace_safe_error_from_wire(code, message)?;
    // 返回无损投影。
    Ok(response)
}

// 解码 deadline 业务前终态。
fn decode_expired(
    // 借用 final 对象。
    object: &Map<String, Value>,
    // 绑定原始 request。
    request: &BrowserSessionBrokerRequest,
    // 接收 revision。
    revision: u64,
    // 借用当前 epoch。
    epoch: &BrowserSessionBrokerEpoch,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 非 stale final 必须回显 expected epoch。
    require_final_epoch(request, epoch, false)?;
    // 业务前 final 必须是首个 revision 零。
    require_revision(revision, 0)?;
    // expired 是可重试业务前终态。
    require_pre_acceptance(object, true)?;
    // expired 不携带 data。
    require_null(object, "data")?;
    // 读取安全错误对象。
    let (code, message) = parse_safe_error(object.get("error"))?;
    // deadline outcome 只允许唯一错误码。
    if code != "REQUEST_EXPIRED" {
        // 拒绝跨 outcome 错误码。
        return Err(invalid());
    }
    // 从严格 request 构造唯一响应。
    let mut response = BrowserSessionBrokerResponse::expired_before_acceptance(
        // 绑定原 request。
        request,  // 保存 revision。
        revision, // 绑定当前 epoch。
        epoch,
    )
    // 构造不应失败。
    .ok_or_else(invalid)?;
    // 保留 wire safeError message。
    response.replace_safe_error_from_wire(code, message)?;
    // 返回无损投影。
    Ok(response)
}

// 解码取消先到业务前终态。
fn decode_cancelled_before(
    // 借用 final 对象。
    object: &Map<String, Value>,
    // 绑定原始 request。
    request: &BrowserSessionBrokerRequest,
    // 接收 revision。
    revision: u64,
    // 借用当前 epoch。
    epoch: &BrowserSessionBrokerEpoch,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 非 stale final 必须回显 expected epoch。
    require_final_epoch(request, epoch, false)?;
    // 业务前 final 必须是首个 revision 零。
    require_revision(revision, 0)?;
    // 取消先到不可自动重试。
    require_pre_acceptance(object, false)?;
    // 取消先到不携带 data。
    require_null(object, "data")?;
    // 读取安全错误对象。
    let (code, message) = parse_safe_error(object.get("error"))?;
    // 只接受唯一错误码。
    if code != "CANCELLED_BEFORE_ACCEPTANCE" {
        // 拒绝错误码漂移。
        return Err(invalid());
    }
    // 构造封闭 tombstone 响应。
    let mut response = BrowserSessionBrokerResponse::cancelled_before_acceptance(
        // 绑定原 request。
        request,  // 保存 revision。
        revision, // 绑定当前 epoch。
        epoch,
    );
    // 保留 wire safeError message。
    response.replace_safe_error_from_wire(code, message)?;
    // 返回无损投影。
    Ok(response)
}

// 解码 operation-specific completed。
fn decode_completed(
    // 借用 final 对象。
    object: &Map<String, Value>,
    // 绑定原始 request。
    request: &BrowserSessionBrokerRequest,
    // 接收 revision。
    revision: u64,
    // 借用当前 epoch。
    epoch: &BrowserSessionBrokerEpoch,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // completed 必须回显 expected epoch。
    require_final_epoch(request, epoch, false)?;
    // 业务终态必须紧随 accepted 成为 revision 一。
    require_revision(revision, 1)?;
    // completed 必须为业务接受后的可信终态。
    require_business_final(object, request, true, false)?;
    // completed 不携带 error。
    require_null(object, "error")?;
    // 按 request operation 严格解析唯一 data 形状。
    let success = parse_success(request, object.get("data").ok_or_else(invalid)?)?;
    // request-bound 构造器再次验证 data 与 request payload 的关系。
    BrowserSessionBrokerResponse::finished(
        // 绑定原 request。
        request,
        // 保存 revision。
        revision,
        // 绑定当前 epoch。
        epoch,
        // 固定 completed outcome。
        BrowserSessionBrokerOutcome::Completed,
        // 保存 operation-specific success。
        Some(success),
    )
    // 跨 operation 或非法 data 闭合失败。
    .ok_or_else(invalid)
}

// 解码业务接受后的 failed/cancelled。
fn decode_business_error(
    // 借用 final 对象。
    object: &Map<String, Value>,
    // 绑定原始 request。
    request: &BrowserSessionBrokerRequest,
    // 接收 revision。
    revision: u64,
    // 借用当前 epoch。
    epoch: &BrowserSessionBrokerEpoch,
    // 接收 failed 或 cancelled。
    outcome: BrowserSessionBrokerOutcome,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 业务终态必须回显 expected epoch。
    require_final_epoch(request, epoch, false)?;
    // 业务终态必须紧随 accepted 成为 revision 一。
    require_revision(revision, 1)?;
    // failed/cancelled 均是可信业务终态。
    require_business_final(object, request, true, false)?;
    // 失败不携带 data。
    require_null(object, "data")?;
    // 读取任意符合 safeError 边界的稳定错误。
    let (code, message) = parse_safe_error(object.get("error"))?;
    // wire-only 构造器保留安全 code/message。
    BrowserSessionBrokerResponse::finished_from_wire(
        // 绑定原 request。
        request,  // 保存 revision。
        revision, // 绑定当前 epoch。
        epoch,    // 保存 failed 或 cancelled outcome。
        outcome,  // 保存稳定错误码。
        code,     // 保存安全错误说明。
        message,
    )
    // 拒绝任何非 failed/cancelled 组合。
    .ok_or_else(invalid)
}

// 解码 OutcomeUnknown。
fn decode_unknown(
    // 借用 final 对象。
    object: &Map<String, Value>,
    // 绑定原始 request。
    request: &BrowserSessionBrokerRequest,
    // 接收 revision。
    revision: u64,
    // 借用当前 epoch。
    epoch: &BrowserSessionBrokerEpoch,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // unknown 必须回显 expected epoch。
    require_final_epoch(request, epoch, false)?;
    // unknown 必须紧随 accepted 成为 revision 一。
    require_revision(revision, 1)?;
    // unknown 已业务接受但不是可信完成。
    require_business_final(object, request, false, true)?;
    // unknown 不携带 data。
    require_null(object, "data")?;
    // 读取安全错误对象。
    let (code, message) = parse_safe_error(object.get("error"))?;
    // unknown 只允许唯一稳定错误码。
    if code != "OUTCOME_UNKNOWN" {
        // 拒绝错误码漂移。
        return Err(invalid());
    }
    // 构造封闭 unknown 响应。
    let mut response = BrowserSessionBrokerResponse::unknown(request, revision, epoch);
    // 保留 wire safeError message。
    response.replace_safe_error_from_wire(code, message)?;
    // 返回无损投影。
    Ok(response)
}

// 校验 response 与原 request 的不可变关联字段。
fn decode_response_identity(
    // 借用 response 对象。
    object: &Map<String, Value>,
    // 接收固定 kind。
    kind: &str,
    // 绑定原始 request。
    request: &BrowserSessionBrokerRequest,
    // 绑定已认证连接握手得到的当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // 固定 kind 与 contract version。
    if required_string(object, "kind")? != kind
        // 版本必须逐字匹配 v1。
        || required_string(object, "contractVersion")? != CONTRACT_VERSION
        // nonce 必须逐字匹配原 request。
        || required_string(object, "requestNonce")? != request.request_nonce()
        // operation 必须逐字匹配原 request。
        || required_string(object, "operation")? != operation_text(request.operation())
        // frame epoch 必须逐字匹配认证连接当前 epoch。
        || required_string(object, "brokerEpoch")? != current_epoch.as_str()
    // 任一不匹配都进入拒绝分支。
    {
        // 拒绝串线、跨 epoch 或跨 operation frame。
        return Err(invalid());
    }
    // 返回完整关联事实。
    Ok(())
}

// 校验 final 的 expected 或 stale-current epoch 规则。
fn require_final_epoch(
    // 绑定原始 request。
    request: &BrowserSessionBrokerRequest,
    // 绑定已认证连接当前 epoch。
    current_epoch: &BrowserSessionBrokerEpoch,
    // 指示该 final 是否为 stale epoch rejection。
    stale_rejection: bool,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // stale 必须回显与旧 expected 不同的当前 epoch。
    let valid = if stale_rejection {
        // 比较当前与旧 expected epoch。
        current_epoch.as_str() != request.expected_broker_epoch()
    } else {
        // 其余 final 必须留在 request expected epoch。
        current_epoch.as_str() == request.expected_broker_epoch()
    };
    // 拒绝 epoch 关联漂移。
    if !valid {
        // 返回统一协议错误。
        return Err(invalid());
    }
    // 返回 epoch 规则合法事实。
    Ok(())
}

// 校验业务前 final 的固定状态组合。
fn require_pre_acceptance(
    // 借用 final 对象。
    object: &Map<String, Value>,
    // 接收 outcome 固定 retrySafe。
    retry_safe: bool,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // 业务尚未接受。
    require_bool(object, "businessAccepted", false)?;
    // 业务前 final 是可信 completed。
    require_bool(object, "completed", true)?;
    // retrySafe 由具体 outcome 冻结。
    require_bool(object, "retrySafe", retry_safe)?;
    // 业务前 final 永不 unknown。
    require_bool(object, "outcomeUnknown", false)?;
    // target 尚未交给业务。
    require_bool(object, "targetMayHaveMutated", false)
}

// 校验业务接受后 final 的固定状态组合。
fn require_business_final(
    // 借用 final 对象。
    object: &Map<String, Value>,
    // 绑定原始 request 的 command/query 角色。
    request: &BrowserSessionBrokerRequest,
    // 接收 completed 事实。
    completed: bool,
    // 接收 outcomeUnknown 事实。
    outcome_unknown: bool,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // 业务必须已接受。
    require_bool(object, "businessAccepted", true)?;
    // completed 必须匹配 outcome。
    require_bool(object, "completed", completed)?;
    // 业务接受后永不允许自动 retry。
    require_bool(object, "retrySafe", false)?;
    // unknown 事实必须匹配 outcome。
    require_bool(object, "outcomeUnknown", outcome_unknown)?;
    // mutation flag 必须逐字匹配 request role。
    require_bool(object, "targetMayHaveMutated", request.may_mutate_target())?;
    // 返回其余共享组合合法。
    Ok(())
}

// 校验 v1 request response 的冻结 revision。
fn require_revision(
    // 接收 wire revision。
    actual: u64,
    // 接收 outcome 对应的期望 revision。
    expected: u64,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    // revision 必须逐值相等。
    if actual != expected {
        // 拒绝跳号或倒退。
        return Err(invalid());
    }
    // 返回连续 revision 事实。
    Ok(())
}

// 将 operation 枚举编码为冻结标签。
fn operation_text(operation: BrowserSessionBrokerOperation) -> &'static str {
    // 穷举九种固定操作。
    match operation {
        // 映射 open。
        BrowserSessionBrokerOperation::Open => "open",
        // 映射 close。
        BrowserSessionBrokerOperation::Close => "close",
        // 映射 session.inspect。
        BrowserSessionBrokerOperation::SessionInspect => "session.inspect",
        // 映射 navigate。
        BrowserSessionBrokerOperation::Navigate => "navigate",
        // 映射 wait。
        BrowserSessionBrokerOperation::Wait => "wait",
        // 映射 query。
        BrowserSessionBrokerOperation::Query => "query",
        // 映射 click。
        BrowserSessionBrokerOperation::Click => "click",
        // 映射 type。
        BrowserSessionBrokerOperation::Type => "type",
        // 映射 screenshot。
        BrowserSessionBrokerOperation::Screenshot => "screenshot",
    }
}

// 将 final outcome 编码为冻结标签。
fn outcome_text(outcome: BrowserSessionBrokerOutcome) -> &'static str {
    // 穷举所有 broker wire outcome。
    match outcome {
        // 映射 rejected。
        BrowserSessionBrokerOutcome::Rejected => "rejected",
        // 映射 deadline 终态。
        BrowserSessionBrokerOutcome::ExpiredBeforeAcceptance => "expired-before-acceptance",
        // 映射取消先到终态。
        BrowserSessionBrokerOutcome::CancelledBeforeAcceptance => "cancelled-before-acceptance",
        // 映射 completed。
        BrowserSessionBrokerOutcome::Completed => "completed",
        // 映射 failed。
        BrowserSessionBrokerOutcome::Failed => "failed",
        // 映射 cancelled。
        BrowserSessionBrokerOutcome::Cancelled => "cancelled",
        // 映射 unknown。
        BrowserSessionBrokerOutcome::Unknown => "unknown",
    }
}
