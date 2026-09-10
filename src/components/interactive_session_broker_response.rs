//! 定义独立交互会话 broker 的结果包装与 dispatch 前拒绝。

// 导入协议序列化与反序列化派生。
use serde::{Deserialize, Serialize};
// 导入 provider-neutral JSON 值。
use serde_json::Value;

// 导入 broker 协议失败事实与固定版本。
use super::interactive_session_broker_protocol::{BrokerProtocolFailure, CONTRACT_VERSION};

// 表示 broker 成功完成 fixed worker transport 后的封闭包装。
#[derive(Debug, Deserialize, Serialize)]
// 使用 camelCase 并拒绝任意扩展字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrokerCommandResult {
    // 固定标记 broker transport 成功返回。
    ok: bool,
    // 输出固定 broker 协议版本。
    contract_version: String,
    // 回显请求 nonce。
    request_nonce: String,
    // 固定 command result 操作。
    operation: String,
    // 回显同一连接 lease 以阻止跨连接结果替换。
    endpoint_lease_nonce: String,
    // 保存 fixed worker 的完整版本化 envelope。
    result: Value,
}

// 为 command result 提供唯一构造与严格解析。
impl BrokerCommandResult {
    // 包装已经由 Job runner 完整回收的 worker envelope。
    pub(crate) fn new(request_nonce: &str, endpoint_lease_nonce: &str, result: Value) -> Self {
        // 返回仅证明 broker transport 完成的包装。
        Self {
            // broker 自身成功完成 transport。
            ok: true,
            // 固定当前协议版本。
            contract_version: CONTRACT_VERSION.to_owned(),
            // 回显已验证请求 nonce。
            request_nonce: request_nonce.to_owned(),
            // 固定响应操作。
            operation: "command-result".to_owned(),
            // 回显 broker 签发 lease。
            endpoint_lease_nonce: endpoint_lease_nonce.to_owned(),
            // 保存 worker 业务结果或结构化拒绝。
            result,
        }
    }

    // 严格解析 host 收到的 command result 包装。
    pub(crate) fn parse(
        text: &str,
        expected_request_nonce: &str,
        expected_endpoint_lease_nonce: &str,
    ) -> Result<Self, BrokerProtocolFailure> {
        // 只接受字段封闭的单个 JSON 对象。
        let response = serde_json::from_str::<Self>(text.trim()).map_err(|_| {
            // accepted 后异常响应不能信任任何关联值。
            BrokerProtocolFailure::accepted(
                // 使用固定 worker 协议失败码。
                super::interactive_session_broker_protocol::BrokerProtocolErrorCode::WorkerProtocolFailed,
                // 回显 host 已知请求 nonce。
                expected_request_nonce,
            )
        })?;
        // 核对固定 envelope 和本连接两个关联值。
        let valid = response.ok
            // 固定 broker 协议版本。
            && response.contract_version == CONTRACT_VERSION
            // 请求 nonce 必须与首帧相同。
            && response.request_nonce == expected_request_nonce
            // 响应操作固定为 command result。
            && response.operation == "command-result"
            // lease 必须来自本连接。
            && response.endpoint_lease_nonce == expected_endpoint_lease_nonce
            // worker envelope 必须是 JSON object。
            && response.result.is_object();
        // 任一错配都拒绝整个结果。
        if !valid {
            // 返回 accepted 后 worker 协议失败。
            return Err(BrokerProtocolFailure::accepted(
                // 使用固定 worker 协议失败码。
                super::interactive_session_broker_protocol::BrokerProtocolErrorCode::WorkerProtocolFailed,
                // 只回显 host 已知关联值。
                expected_request_nonce,
            ));
        }
        // 返回已经绑定本连接的结果。
        Ok(response)
    }

    // 消费包装并返回 fixed worker envelope。
    pub(crate) fn into_result(self) -> Value {
        // broker 不改写 worker 业务字段。
        self.result
    }
}

// 表示 broker 在启动 command worker 前的稳定拒绝响应。
#[derive(Debug, Serialize)]
// 使用 camelCase 对齐内部协议。
#[serde(rename_all = "camelCase")]
pub(crate) struct BrokerRejection {
    // 固定标记请求失败。
    ok: bool,
    // 输出固定 broker 协议版本。
    contract_version: &'static str,
    // 只对可信 frame 回显请求 nonce。
    #[serde(skip_serializing_if = "Option::is_none")]
    request_nonce: Option<String>,
    // 固定响应操作。
    operation: &'static str,
    // 区分完整 transport frame 是否已接受。
    transport_accepted: bool,
    // 明确 mutation 尚未被接受。
    business_accepted: bool,
    // 明确 capability 尚未完成。
    completed: bool,
    // 明确失败发生在 dispatch 前。
    outcome: &'static str,
    // dispatch 前失败允许使用新请求重试。
    retry_safe: bool,
    // 明确目标没有因本请求改变。
    target_may_have_mutated: bool,
    // 输出封闭错误对象。
    error: BrokerError,
    // 输出不含 native 值的认证阶段证据。
    evidence: BrokerRejectionEvidence,
}

// 表示 broker 的稳定错误对象。
#[derive(Debug, Serialize)]
// 使用 camelCase 保持协议一致。
#[serde(rename_all = "camelCase")]
struct BrokerError {
    // 保存稳定错误码。
    code: &'static str,
    // 保存不泄漏 endpoint 的固定消息。
    message: &'static str,
}

// 表示 dispatch 前拒绝的最小生命周期证据。
#[derive(Debug, Serialize)]
// 使用 camelCase 保持协议一致。
#[serde(rename_all = "camelCase")]
struct BrokerRejectionEvidence {
    // 记录 OS peer 是否已经认证。
    peer_authenticated: bool,
    // 记录 endpoint session 与桌面是否已经认证。
    session_certified: bool,
    // 记录本连接是否已经签发 lease。
    lease_issued: bool,
    // 固定证明 command worker 尚未启动。
    command_worker_started: bool,
    // 固定证明没有当前桌面 fallback。
    foreground_fallback_used: bool,
}

// 从协议失败构造零 dispatch 拒绝响应。
pub(crate) fn rejection(
    failure: &BrokerProtocolFailure,
    peer_authenticated: bool,
    session_certified: bool,
    lease_issued: bool,
) -> BrokerRejection {
    // 返回字段组合固定的失败 envelope。
    BrokerRejection {
        // 标记业务失败。
        ok: false,
        // 固定输出 broker v1。
        contract_version: CONTRACT_VERSION,
        // 只回显已经验证的关联值。
        request_nonce: failure.request_nonce().map(str::to_owned),
        // 固定响应操作。
        operation: "rejected",
        // 输出 transport 接受状态。
        transport_accepted: failure.transport_accepted(),
        // mutation 尚未被接受。
        business_accepted: false,
        // capability 尚未完成。
        completed: false,
        // 失败发生在 dispatch 前。
        outcome: "not-dispatched",
        // 使用新 nonce 修正前置条件后可以重试。
        retry_safe: true,
        // 尚未启动 worker，目标不可能已改变。
        target_may_have_mutated: false,
        // 构造封闭错误对象。
        error: BrokerError {
            // 输出稳定错误码。
            code: failure.code().as_str(),
            // 输出固定安全消息。
            message: failure.code().message(),
        },
        // 输出逐阶段零副作用证据。
        evidence: BrokerRejectionEvidence {
            // 复制调用方已知 peer 认证事实。
            peer_authenticated,
            // 复制 endpoint 认证事实。
            session_certified,
            // 复制 lease 是否已经签发。
            lease_issued,
            // 本构造器只允许 worker 启动前调用。
            command_worker_started: false,
            // broker 永远不回退 host 当前桌面。
            foreground_fallback_used: false,
        },
    }
}
