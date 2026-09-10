//! 严格解析固定长操作 broker 对已认证客户端返回的响应。

// 导入封闭 JSON 派生与值类型。
use serde::{Deserialize, Serialize};
// 导入 provider-neutral JSON 值。
use serde_json::Value;

// 导入 opaque operation 解析与公开状态枚举。
use crate::{
    // 只接受 operation 类别 opaque handle。
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    // 复用唯一生命周期状态文本。
    modules::long_operation::LongOperationStatus,
};

// 导入固定 broker 协议版本。
use super::long_operation_protocol::CONTRACT_VERSION;

// 表示客户端可安全消费的封闭响应。
pub(crate) enum LongOperationClientResponse {
    // 保存完整公开 operation 状态对象。
    Success(Value),
    // 保存业务接受前的稳定错误。
    Rejected {
        // 保存 schema 登记错误码。
        code: String,
        // 保存有界安全消息。
        message: String,
    },
}

// 表示响应无法通过版本、关联或字段门禁。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LongOperationResponseFailure;

// 表示 broker 顶层 wire response。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BrokerWireResponse {
    // 保存固定 broker 版本。
    contract_version: String,
    // 保存已接受请求的 canonical nonce。
    request_nonce: Option<String>,
    // 保存 transport 接受事实。
    transport_accepted: bool,
    // 保存业务接受事实。
    business_accepted: bool,
    // 成功时保存严格 operation 对象。
    operation: Option<LongOperationStatusWire>,
    // 拒绝时保存稳定错误。
    error: Option<BrokerWireError>,
}

// 表示 broker 业务拒绝错误。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrokerWireError {
    // 保存稳定错误码。
    code: String,
    // 保存安全错误消息。
    message: String,
}

// 表示公开 operation 错误对象。
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OperationWireError {
    // 保存稳定错误码。
    code: String,
    // 保存安全错误消息。
    message: String,
}

// 表示公开长操作状态的字段封闭 wire shape。
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LongOperationStatusWire {
    // 保存公开状态契约版本。
    contract_version: String,
    // 保存 canonical operation handle。
    operation_id: String,
    // 保存首个封闭 capability。
    capability_id: String,
    // 保存六态生命周期。
    status: LongOperationStatus,
    // 保存不可逆 dispatch 事实。
    dispatch_started: bool,
    // 保存终态分类。
    terminal: bool,
    // 保存 handle 已建立后的保守接受事实。
    accepted_may_have_occurred: bool,
    // 保存是否允许安全重提。
    retry_safe: bool,
    // 保存取消是否曾被接受。
    cancel_requested: bool,
    // 非终态为 null，终态为 UTC 时间。
    expires_at: Option<String>,
    // completed 才携带完整结果。
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    // failed 或 outcome-unknown 才携带错误。
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<OperationWireError>,
}

// 严格解析并绑定本次 client nonce。
pub(crate) fn parse_response(
    // 接收单条有界 JSON frame。
    text: &str,
    // 接收本次请求的 canonical nonce。
    expected_nonce: &str,
) -> Result<LongOperationClientResponse, LongOperationResponseFailure> {
    // 只接受字段封闭响应对象。
    let response: BrokerWireResponse =
        serde_json::from_str(text.trim_start_matches('\u{feff}').trim())
            // 任意 shape 漂移失败闭合。
            .map_err(|_| LongOperationResponseFailure)?;
    // 已认证 broker 仍必须匹配版本、transport 与关联值。
    if response.contract_version != CONTRACT_VERSION
        // 客户端合法请求必须完成 transport 接受。
        || !response.transport_accepted
        // 回显 nonce 必须精确匹配本次请求。
        || response.request_nonce.as_deref() != Some(expected_nonce)
    {
        // 拒绝跨请求或版本漂移响应。
        return Err(LongOperationResponseFailure);
    }
    // 按业务接受事实验证互斥 payload。
    if response.business_accepted {
        // 成功不得同时携带错误。
        if response.error.is_some() {
            // 拒绝矛盾响应。
            return Err(LongOperationResponseFailure);
        }
        // 成功必须携带严格 operation 状态。
        let operation = response.operation.ok_or(LongOperationResponseFailure)?;
        // 验证公开 operation 自身不变量。
        validate_operation(&operation)?;
        // 转回 provider-neutral JSON 供 System 投影。
        let operation =
            serde_json::to_value(operation).map_err(|_| LongOperationResponseFailure)?;
        // 返回业务成功。
        Ok(LongOperationClientResponse::Success(operation))
    } else {
        // 拒绝不得伪造 operation。
        if response.operation.is_some() {
            // 拒绝矛盾响应。
            return Err(LongOperationResponseFailure);
        }
        // 拒绝必须携带稳定错误。
        let error = response.error.ok_or(LongOperationResponseFailure)?;
        // 错误字段必须满足公共安全边界。
        if !matches!(
            // 只接受 broker schema 冻结的错误码。
            error.code.as_str(),
            // 参数错误。
            "INVALID_ARGUMENT"
                // 确认错误。
                | "CONFIRMATION_REQUIRED"
                // 容量错误。
                | "OPERATION_CAPACITY_EXHAUSTED"
                // 零命中错误。
                | "OPERATION_NOT_FOUND"
                // 结果预算错误。
                | "OPERATION_RESULT_TOO_LARGE"
                // 权限门禁错误。
                | "PERMISSION_DENIED"
                // capability 或固定 worker 缺口。
                | "CAPABILITY_GAP"
                // broker 生命周期错误。
                | "BROKER_UNAVAILABLE"
        ) || error.message.is_empty()
            || error.message.chars().count() > 512
        {
            // 拒绝未受约束的错误文本。
            return Err(LongOperationResponseFailure);
        }
        // 返回业务接受前拒绝。
        Ok(LongOperationClientResponse::Rejected {
            // 转移稳定错误码。
            code: error.code,
            // 转移安全消息。
            message: error.message,
        })
    }
}

// 验证公开 operation shape 的关键跨字段不变量。
fn validate_operation(
    operation: &LongOperationStatusWire,
) -> Result<(), LongOperationResponseFailure> {
    // 解析 opaque handle 类别。
    let operation_kind = OpaqueTargetId::parse(&operation.operation_id).map(OpaqueTargetId::kind);
    // 计算状态是否终态。
    let terminal = operation.status.is_terminal();
    // 验证版本、handle、capability 与终态一致性。
    if operation.contract_version != "act/long-operation/v1"
        // 只接受 operation 类别。
        || operation_kind != Some(OpaqueTargetKind::Operation)
        // 当前版本只接受窗口录制。
        || operation.capability_id != "window.record@1"
        // handle 已建立必须保守声明可能接受。
        || !operation.accepted_may_have_occurred
        // 状态与 terminal 字段必须一致。
        || operation.terminal != terminal
        // 非终态不得携带 expiresAt。
        || (!terminal && operation.expires_at.is_some())
        // 终态必须携带 expiresAt。
        || (terminal && operation.expires_at.is_none())
    {
        // 拒绝不一致 operation。
        return Err(LongOperationResponseFailure);
    }
    // completed 必须独占 result，失败终态必须独占 error，非终态两者皆无。
    let payload_valid = match operation.status {
        // completed 只携带结果。
        LongOperationStatus::Completed => operation.result.is_some() && operation.error.is_none(),
        // failed 与 outcome-unknown 只携带错误。
        LongOperationStatus::Failed | LongOperationStatus::OutcomeUnknown => {
            operation.result.is_none() && operation.error.is_some()
        }
        // 非终态不得携带终态 payload。
        LongOperationStatus::Accepted
        | LongOperationStatus::Running
        | LongOperationStatus::CancelRequested => {
            operation.result.is_none() && operation.error.is_none()
        }
    };
    // 返回封闭验证结果。
    payload_valid
        .then_some(())
        .ok_or(LongOperationResponseFailure)
}

// 声明严格 broker response 回归测试。
#[cfg(test)]
// 将 fixture 放入独立文件控制 Component 规模。
#[path = "long_operation_broker_response_tests.rs"]
mod tests;
