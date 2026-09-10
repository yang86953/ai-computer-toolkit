//! 定义 broker accepted、final 与 cancel receipt 的封闭响应投影。

// 导入 JSON 数据。
use serde_json::Value;
// 导入请求、失败与 operation 类型。
use super::{
    BrowserSessionBrokerOperation, BrowserSessionBrokerProtocolErrorCode,
    BrowserSessionBrokerProtocolFailure,
};
// 导入严格领域 request。
use super::BrowserSessionBrokerRequest;
// 导入公开 opaque ID 常量。
use super::{NONCE_LENGTH, PAGE_PREFIX, SESSION_PREFIX};
// 导入 broker epoch。
use super::state::BrowserSessionBrokerEpoch;
// 注册 cancel receipt/rejected 的窄响应子 Component。
#[path = "browser_session_broker_protocol_control_response.rs"]
mod control_response;
// 注册 request-bound PNG 成功验证子 Component。
#[path = "browser_session_broker_protocol_screenshot.rs"]
mod screenshot;
// 导入 PNG 成功验证器。
use screenshot::screenshot_data;
// 注册页面动作成功投影的 request-bound 回归。
#[cfg(test)]
#[path = "browser_session_broker_protocol_page_action_tests.rs"]
mod page_action_tests;
// 向同协议调用方重导出封闭 cancel 响应类型。
pub(crate) use control_response::{
    BrowserSessionBrokerCancelReceipt, BrowserSessionBrokerCancelRejected,
};

// 表示 final outcome。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerOutcome {
    Rejected,
    ExpiredBeforeAcceptance,
    CancelledBeforeAcceptance,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

// 表示仅由 client 本地证明的未派发观察，绝不进入 broker wire。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionBrokerClientLocalNotDispatched {
    // 保存本地关联 request nonce。
    request_nonce: String,
    // 保存本地已经发起的 operation。
    operation: BrowserSessionBrokerOperation,
}

// 为本地未派发提供最小投影。
impl BrowserSessionBrokerClientLocalNotDispatched {
    // 构造本地观察。
    pub(crate) fn new(request: &BrowserSessionBrokerRequest) -> Self {
        // 保存仅由 client 自己可信掌握的关联值。
        Self {
            // 复制 request nonce。
            request_nonce: request.request_nonce().to_owned(),
            // 复制 operation。
            operation: request.operation(),
        }
    }

    // 返回关联 request nonce。
    pub(crate) fn request_nonce(&self) -> &str {
        // 借用 nonce。
        &self.request_nonce
    }

    // 返回本地发起的 operation。
    pub(crate) const fn operation(&self) -> BrowserSessionBrokerOperation {
        // 复制无状态枚举。
        self.operation
    }

    // 返回 client 本地证明的 transport 接受事实。
    pub(crate) const fn transport_accepted(&self) -> bool {
        // 未收到 broker 关联事实时固定为 false。
        false
    }

    // 返回 client 本地证明的 retry 事实。
    pub(crate) const fn retry_safe(&self) -> bool {
        // 确定未派发才允许自动重试。
        true
    }
}

// 保存不泄漏 native/CDP/private 的成功数据。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BrowserSessionBrokerSuccess {
    // open 只返回公开 session identity。
    Open {
        // 保存公开 session ID。
        session_id: String,
    },
    // close 只返回关闭事实。
    Close,
    // session.inspect 只返回存活的公开 session identity 与布尔事实。
    SessionInspect {
        // 回显已检查的公开 session ID。
        session_id: String,
        // 成功结果固定表示会话存活。
        live: bool,
    },
    // navigate 返回新 page identity 和代际。
    Navigate {
        // 保存公开 page ID。
        page_id: String,
        // 保存从一开始的导航代际。
        generation: u32,
    },
    // wait 返回当前公开页面、正代际与 conditionMet=true 事实。
    Wait {
        page_id: String,
        generation: u32,
    },
    // query 返回封闭元素投影对象。
    Query(Value),
    // click 返回 request-bound 公开身份、代际与 clicked=true 事实。
    Click {
        // 保存当前公开页面。
        page_id: String,
        // 保存当前公开元素。
        element_id: String,
        // 保存正导航代际。
        generation: u32,
    },
    // type 返回 request-bound 公开身份、代际与 UTF-8 字节计数。
    Type {
        // 保存当前公开页面。
        page_id: String,
        // 保存当前公开元素。
        element_id: String,
        // 保存正导航代际。
        generation: u32,
        // 保存已输入的有界字节数。
        utf8_bytes: u16,
    },
    // screenshot 返回封闭 PNG 投影对象。
    Screenshot(Value),
}

// 表示 broker ready 握手的封闭投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionBrokerReady {
    // 保存当前 live epoch。
    epoch: String,
}

// 为 ready 提供严格 parser。
impl BrowserSessionBrokerReady {
    // 解析不泄漏 endpoint 或 native 信息的 ready frame。
    pub(crate) fn parse(text: &str) -> Result<Self, super::BrowserSessionBrokerProtocolFailure> {
        // 在 JSON 解析前实施冻结的 control/response frame 上限。
        let text = bounded_response_input(text)?;
        // 读取唯一 JSON 对象。
        let value = serde_json::from_str::<Value>(text).map_err(|_| super::invalid_argument())?;
        // 取得对象。
        let object = value.as_object().ok_or_else(super::invalid_argument)?;
        // 严格限定 ready 字段。
        if object.len() != 3
            || object.get("kind").and_then(Value::as_str) != Some("broker-ready")
            || object.get("contractVersion").and_then(Value::as_str)
                != Some(super::CONTRACT_VERSION)
        {
            // 拒绝未知或错误 frame。
            return Err(super::invalid_argument());
        }
        // 读取 epoch。
        let epoch = object
            .get("brokerEpoch")
            .and_then(Value::as_str)
            .ok_or_else(super::invalid_argument)?;
        // 校验 canonical epoch。
        if !opaque_epoch(epoch) {
            // 拒绝 epoch。
            return Err(super::invalid_argument());
        }
        // 返回 ready。
        Ok(Self {
            epoch: epoch.to_owned(),
        })
    }

    // 返回当前 epoch。
    pub(crate) fn broker_epoch(&self) -> &str {
        // 借用 epoch。
        &self.epoch
    }
}

// 保存 accepted 或 final 的严格状态组合。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrowserSessionBrokerResponse {
    // 保存关联 request nonce。
    request_nonce: String,
    // 保存构造响应所绑定严格 request 的完整 canonical 语义键。
    request_semantic_key: Option<String>,
    // 保存客户端在 request envelope 中声明的期望 epoch。
    expected_epoch: String,
    // 保存单调 request revision。
    request_revision: u64,
    // 保存 accepted 时为空、final 时存在的 outcome。
    outcome: Option<BrowserSessionBrokerOutcome>,
    // 保存当前 broker epoch。
    epoch: String,
    // 保存固定 operation。
    operation: BrowserSessionBrokerOperation,
    // 保存 transport 接受事实。
    transport_accepted: bool,
    // 保存 business 接受事实。
    business_accepted: bool,
    // 保存可信完成事实。
    completed: bool,
    // 保存安全重试事实。
    retry_safe: bool,
    // 保存 OutcomeUnknown 事实。
    outcome_unknown: bool,
    // 保存 target 可能变化事实。
    target_may_have_mutated: bool,
    // 保存可选成功数据。
    success: Option<BrowserSessionBrokerSuccess>,
    // 保存可选稳定错误码。
    error_code: Option<String>,
    // 保存可选且已经验证的安全错误说明。
    error_message: Option<String>,
}

// 为响应提供封闭构造。
impl BrowserSessionBrokerResponse {
    // 构造业务 accepted 响应。
    pub(crate) fn accepted(
        // 绑定原始严格 request。
        request: &BrowserSessionBrokerRequest,
        // 接收当前 revision。
        request_revision: u64,
        // 借用当前 broker epoch。
        epoch: &BrowserSessionBrokerEpoch,
    ) -> Self {
        // 从同一 request 派生 nonce、operation 与 mutation 事实。
        Self {
            // 复制 request nonce。
            request_nonce: request.request_nonce().to_owned(),
            // 保存完整 request 语义以供 execution ledger 逐字关联。
            request_semantic_key: Some(request.canonical_semantic_key()),
            // 从严格 request 复制期望 epoch。
            expected_epoch: request.expected_broker_epoch().to_owned(),
            // 保存 revision。
            request_revision,
            // accepted 尚无 final outcome。
            outcome: None,
            // 复制 epoch。
            epoch: epoch.as_str().to_owned(),
            // 复制 operation。
            operation: request.operation(),
            // 完整 frame 已收取。
            transport_accepted: true,
            // Module 已越过业务接受点。
            business_accepted: true,
            // accepted 不是 final。
            completed: false,
            // accepted 不授权 retry。
            retry_safe: false,
            // accepted 尚不是 unknown。
            outcome_unknown: false,
            // Command accepted 后必须保守标记可能变化。
            target_may_have_mutated: request.may_mutate_target(),
            // accepted 不携带成功数据。
            success: None,
            // accepted 不携带错误。
            error_code: None,
            // accepted 不携带错误说明。
            error_message: None,
        }
    }

    // 从已经安全关联的 parser 或 ledger 失败构造拒绝终态。
    pub(crate) fn rejected(
        // 借用包含安全公共关联字段的失败。
        failure: &BrowserSessionBrokerProtocolFailure,
        // 接收当前 revision。
        request_revision: u64,
        // 借用当前 epoch。
        epoch: &BrowserSessionBrokerEpoch,
    ) -> Option<Self> {
        // 未完整接收 envelope 的失败不能伪造 broker wire final。
        if !failure.transport_accepted() {
            // 保持未派发事实仅由 client 本地投影。
            return None;
        }
        // 读取已经由 parser 证明安全的 nonce。
        let request_nonce = failure.request_nonce()?.to_owned();
        // 读取已经由 parser 证明安全的 operation。
        let operation = failure.operation()?;
        // 读取 envelope 阶段已经 canonical 的期望 epoch。
        let expected_epoch = failure.expected_broker_epoch()?;
        // 委托封闭业务前构造。
        Self::before_acceptance(
            request_nonce,
            // parser 早期拒绝没有可证明的完整 request 语义。
            None,
            // 保留 envelope 期望 epoch 供 stale 输出关联。
            expected_epoch,
            request_revision,
            epoch,
            operation,
            BrowserSessionBrokerOutcome::Rejected,
            failure.code(),
        )
    }

    // 从严格 request 与逐项匹配的失败构造可进入 execution ledger 的拒绝终态。
    pub(crate) fn rejected_for_request(
        // 借用包含安全公共关联字段的失败。
        failure: &BrowserSessionBrokerProtocolFailure,
        // 绑定已经完整严格解析的 request。
        request: &BrowserSessionBrokerRequest,
        // 接收当前 revision。
        request_revision: u64,
        // 借用当前 broker epoch。
        epoch: &BrowserSessionBrokerEpoch,
    ) -> Option<Self> {
        // failure 必须证明完整 frame 已 transport 接受。
        if !failure.transport_accepted()
            // failure nonce 必须逐字匹配严格 request。
            || failure.request_nonce() != Some(request.request_nonce())
            // failure operation 也必须匹配严格 request。
            || failure.operation() != Some(request.operation())
            // failure 期望 epoch 必须匹配严格 request envelope。
            || failure.expected_broker_epoch() != Some(request.expected_broker_epoch())
            // 仅允许 failure 自身已绑定同一完整 canonical 语义键。
            || !failure.request_semantic_key().is_some_and(|key| {
                // 逐字比较原 request 完整语义。
                key == request.canonical_semantic_key()
            })
        {
            // 拒绝任何无法绑定完整 request 的失败。
            return None;
        }
        // 委托业务前终态构造并保存完整 canonical 语义。
        Self::before_acceptance(
            // 从严格 request 复制 canonical nonce。
            request.request_nonce().to_owned(),
            // 保存 ledger 后续逐字核对所需的完整语义键。
            Some(request.canonical_semantic_key()),
            // 从严格 request 复制期望 epoch。
            request.expected_broker_epoch(),
            // 保存首个业务前终态 revision。
            request_revision,
            // 回显当前 live broker epoch。
            epoch,
            // 从严格 request 复制 operation。
            request.operation(),
            // request-bound parser 或领域预检失败固定为 rejected。
            BrowserSessionBrokerOutcome::Rejected,
            // 保留冻结的稳定错误码。
            failure.code(),
        )
    }

    // 从严格 request 构造业务接受前 deadline 终态。
    pub(crate) fn expired_before_acceptance(
        // 绑定原始严格 request。
        request: &BrowserSessionBrokerRequest,
        // 接收当前 revision。
        request_revision: u64,
        // 借用当前 epoch。
        epoch: &BrowserSessionBrokerEpoch,
    ) -> Option<Self> {
        // 委托封闭业务前构造并固定 deadline 错误码。
        Self::before_acceptance(
            request.request_nonce().to_owned(),
            // deadline 终态仍绑定已经严格解析的完整 request 语义。
            Some(request.canonical_semantic_key()),
            // 从严格 request 复制期望 epoch。
            request.expected_broker_epoch(),
            request_revision,
            epoch,
            request.operation(),
            BrowserSessionBrokerOutcome::ExpiredBeforeAcceptance,
            BrowserSessionBrokerProtocolErrorCode::RequestExpired,
        )
    }

    // 构造 parser、ledger 或 deadline 产生的业务前确定结果。
    fn before_acceptance(
        // 接收已经 canonical 的 request nonce。
        request_nonce: String,
        // 接收可选的完整严格 request 语义键。
        request_semantic_key: Option<String>,
        // 接收已经 canonical 的 request 期望 epoch。
        expected_epoch: &str,
        // 接收当前 revision。
        request_revision: u64,
        // 借用当前 epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 接收已经安全确定的 operation。
        operation: BrowserSessionBrokerOperation,
        // 接收 rejected 或 expired-before-acceptance。
        outcome: BrowserSessionBrokerOutcome,
        // 接收冻结 schema 允许的稳定错误码。
        error_code: BrowserSessionBrokerProtocolErrorCode,
    ) -> Option<Self> {
        // 只允许 schema 中可重试的两个业务前 wire outcome。
        if !matches!(
            outcome,
            BrowserSessionBrokerOutcome::Rejected
                | BrowserSessionBrokerOutcome::ExpiredBeforeAcceptance
        ) || !canonical_nonce(&request_nonce)
            // 期望 epoch 必须是 canonical 十六进制身份。
            || !opaque_epoch(expected_epoch)
            // 业务前 response 只允许安全错误集合。
            || !pre_acceptance_error_allowed(outcome, error_code)
        {
            // 拒绝伪造其他状态。
            return None;
        }
        // 返回确定未越过 business acceptance 的 final。
        Some(Self {
            // 保存 canonical request nonce。
            request_nonce,
            // 仅严格 request 构造路径保存完整语义绑定。
            request_semantic_key,
            // 保存 request envelope 的期望 epoch。
            expected_epoch: expected_epoch.to_owned(),
            // 保存 revision。
            request_revision,
            // 保存业务前 outcome。
            outcome: Some(outcome),
            // 保存当前 epoch。
            epoch: epoch.as_str().to_owned(),
            // 保存 operation。
            operation,
            // broker 已完整收取 frame。
            transport_accepted: true,
            // Module 尚未接受。
            business_accepted: false,
            // 这是可信 final。
            completed: true,
            // 确定未执行，可安全重试。
            retry_safe: true,
            // 结果不是 unknown。
            outcome_unknown: false,
            // 业务未接受，target 未变化。
            target_may_have_mutated: false,
            // 失败不携带 data。
            success: None,
            // 使用稳定业务前拒绝码。
            error_code: Some(error_code.as_str().to_owned()),
            // 使用不泄漏 provider 信息的固定说明。
            error_message: Some(default_error_message(error_code.as_str())),
        })
    }

    // 构造取消 tombstone 对后到 request 的 final。
    pub(crate) fn cancelled_before_acceptance(
        // 绑定后到的严格 request。
        request: &BrowserSessionBrokerRequest,
        // 接收 tombstone revision。
        request_revision: u64,
        // 借用当前 epoch。
        epoch: &BrowserSessionBrokerEpoch,
    ) -> Self {
        // 返回不可重试且未改变 target 的业务前取消终态。
        Self {
            // 复制 request nonce。
            request_nonce: request.request_nonce().to_owned(),
            // 保存后到 request 的完整语义绑定。
            request_semantic_key: Some(request.canonical_semantic_key()),
            // 从后到的严格 request 复制期望 epoch。
            expected_epoch: request.expected_broker_epoch().to_owned(),
            // 保存 tombstone revision。
            request_revision,
            // 固定取消先到 outcome。
            outcome: Some(BrowserSessionBrokerOutcome::CancelledBeforeAcceptance),
            // 保存当前 epoch。
            epoch: epoch.as_str().to_owned(),
            // 复制 request operation。
            operation: request.operation(),
            // 后到 frame 已完整收取。
            transport_accepted: true,
            // 不允许越过业务接受点。
            business_accepted: false,
            // tombstone 是可信 final。
            completed: true,
            // 同 nonce 被 tombstone 永久阻止。
            retry_safe: false,
            // 结果不是 unknown。
            outcome_unknown: false,
            // target 从未被业务接受。
            target_may_have_mutated: false,
            // final 不携带成功数据。
            success: None,
            // 固定取消先到错误码。
            error_code: Some("CANCELLED_BEFORE_ACCEPTANCE".to_owned()),
            // 固定取消先到安全说明。
            error_message: Some(default_error_message("CANCELLED_BEFORE_ACCEPTANCE")),
        }
    }
    // 构造已接受后的确定终态。
    pub(crate) fn finished(
        // 绑定原始严格 request。
        request: &BrowserSessionBrokerRequest,
        // 接收当前 revision。
        request_revision: u64,
        // 借用当前 epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 接收完成、失败或取消终态。
        outcome: BrowserSessionBrokerOutcome,
        // 接收与 operation 一一匹配的成功数据。
        success: Option<BrowserSessionBrokerSuccess>,
    ) -> Option<Self> {
        // 只允许 business accepted 后的三个确定终态。
        if !matches!(
            outcome,
            BrowserSessionBrokerOutcome::Completed
                | BrowserSessionBrokerOutcome::Failed
                | BrowserSessionBrokerOutcome::Cancelled
        )
            // completed 必须且只能携带成功数据。
            || (outcome == BrowserSessionBrokerOutcome::Completed) != success.is_some()
            // 成功数据必须与原 request operation 严格匹配。
            || !success
                // 借用可选成功数据。
                .as_ref()
                // 无数据只允许失败/取消，有数据必须通过 operation validator。
                .is_none_or(|data| valid_success(request, data))
        {
            // 拒绝非法状态组合或跨 operation 数据。
            return None;
        }
        // 返回可信 final。
        Some(Self {
            // 从原 request 复制 nonce。
            request_nonce: request.request_nonce().to_owned(),
            // 保存构造终态所绑定的完整 request 语义。
            request_semantic_key: Some(request.canonical_semantic_key()),
            // 从严格 request 复制期望 epoch。
            expected_epoch: request.expected_broker_epoch().to_owned(),
            // 保存 revision。
            request_revision,
            // 保存 final outcome。
            outcome: Some(outcome),
            // 保存当前 epoch。
            epoch: epoch.as_str().to_owned(),
            // 从原 request 复制 operation。
            operation: request.operation(),
            // wire 已接受。
            transport_accepted: true,
            // 业务已接受。
            business_accepted: true,
            // 这是可信 final。
            completed: true,
            // 已接受后禁止自动 retry。
            retry_safe: false,
            // 确定终态不是 unknown。
            outcome_unknown: false,
            // Command 必须保守标记可能变化。
            target_may_have_mutated: request.may_mutate_target(),
            // 保存可选成功数据。
            success,
            // 非 completed 使用稳定执行失败码。
            error_code: (outcome != BrowserSessionBrokerOutcome::Completed)
                // 只在失败或取消时保存错误。
                .then(|| "BROKER_OPERATION_FAILED".to_owned()),
            // 非 completed 使用不泄漏 provider 信息的固定说明。
            error_message: (outcome != BrowserSessionBrokerOutcome::Completed)
                // 只在失败或取消时保存安全说明。
                .then(|| default_error_message("BROKER_OPERATION_FAILED")),
        })
    }

    // 从已经严格校验的 wire safeError 构造失败或取消终态。
    pub(super) fn finished_from_wire(
        // 绑定原始严格 request。
        request: &BrowserSessionBrokerRequest,
        // 接收当前 revision。
        request_revision: u64,
        // 借用当前 broker epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 接收 failed 或 cancelled。
        outcome: BrowserSessionBrokerOutcome,
        // 保存 wire Component 已校验的稳定错误码。
        error_code: String,
        // 保存 wire Component 已校验的安全说明。
        error_message: String,
    ) -> Option<Self> {
        // 先复用业务接受后终态不变量。
        let mut response = Self::finished(request, request_revision, epoch, outcome, None)?;
        // 覆盖为 wire 携带的稳定错误码。
        response.error_code = Some(error_code);
        // 保留 wire 说明以便 replay 无损重编码。
        response.error_message = Some(error_message);
        // 返回已验证终态。
        Some(response)
    }

    // 用 wire decoder 已校验的 safeError 替换固定默认说明。
    pub(super) fn replace_safe_error_from_wire(
        // 可变借用已经封闭的错误响应。
        &mut self,
        // 接收必须与当前响应一致的稳定错误码。
        error_code: String,
        // 接收已经过 wire 字符与长度验证的安全说明。
        error_message: String,
    ) -> Result<(), BrowserSessionBrokerProtocolFailure> {
        // 仅允许逐字保持现有稳定错误码。
        if self.error_code.as_deref() != Some(error_code.as_str()) {
            // 拒绝跨 outcome 改写错误码。
            return Err(super::invalid_argument());
        }
        // 保留安全说明用于无损 replay。
        self.error_message = Some(error_message);
        // 返回替换成功。
        Ok(())
    }

    // 构造断线后的保守 unknown。
    pub(crate) fn unknown(
        // 绑定原始严格 request。
        request: &BrowserSessionBrokerRequest,
        // 接收当前 revision。
        request_revision: u64,
        // 借用当前 epoch。
        epoch: &BrowserSessionBrokerEpoch,
    ) -> Self {
        // 返回 accepted 后丢失可信 final 的保守结果。
        Self {
            // 从原 request 复制 nonce。
            request_nonce: request.request_nonce().to_owned(),
            // 保存 OutcomeUnknown 所绑定的完整 request 语义。
            request_semantic_key: Some(request.canonical_semantic_key()),
            // 从严格 request 复制期望 epoch。
            expected_epoch: request.expected_broker_epoch().to_owned(),
            // 保存 revision。
            request_revision,
            // 固定 unknown outcome。
            outcome: Some(BrowserSessionBrokerOutcome::Unknown),
            // 保存当前 epoch。
            epoch: epoch.as_str().to_owned(),
            // 从原 request 复制 operation。
            operation: request.operation(),
            // frame 已被 transport 接受。
            transport_accepted: true,
            // 业务已越过接受点。
            business_accepted: true,
            // unknown 不是可信完成。
            completed: false,
            // unknown 禁止自动 retry。
            retry_safe: false,
            // 固定 OutcomeUnknown 事实。
            outcome_unknown: true,
            // Command 必须保守标记可能变化。
            target_may_have_mutated: request.may_mutate_target(),
            // unknown 不携带成功数据。
            success: None,
            // 固定安全错误码。
            error_code: Some("OUTCOME_UNKNOWN".to_owned()),
            // 固定安全错误说明。
            error_message: Some(default_error_message("OUTCOME_UNKNOWN")),
        }
    }
    // 返回 broker epoch。
    pub(crate) fn broker_epoch(&self) -> &str {
        // 借用 epoch。
        &self.epoch
    }
    // 返回关联 request nonce。
    pub(crate) fn request_nonce(&self) -> &str {
        // 借用 nonce。
        &self.request_nonce
    }
    // 返回构造响应所绑定的完整严格 request 语义。
    pub(super) fn request_semantic_key(&self) -> Option<&str> {
        // 借用可选 canonical 语义键。
        self.request_semantic_key.as_deref()
    }
    // 返回 request envelope 声明的期望 epoch。
    pub(crate) fn expected_broker_epoch(&self) -> &str {
        // 借用 canonical 期望 epoch。
        &self.expected_epoch
    }
    // 核对无 strict request 时唯一允许的早期 rejected 身份。
    pub(crate) fn is_early_rejection_for(&self, current_epoch: &BrowserSessionBrokerEpoch) -> bool {
        // 从稳定错误码判定 stale 语义。
        let stale = self.error_code.as_deref() == Some("STALE_BROKER_EPOCH");
        // semantic key 缺席只能来自封闭 early rejection 构造器。
        self.request_semantic_key.is_none()
            // early 路径只允许 rejected 终态。
            && self.outcome == Some(BrowserSessionBrokerOutcome::Rejected)
            // 业务前终态固定为 revision 零。
            && self.request_revision == 0
            // 输出 epoch 必须绑定已认证连接。
            && self.epoch == current_epoch.as_str()
            // stale 当且仅当 expected 与 current 不同。
            && ((self.expected_epoch != current_epoch.as_str()) == stale)
    }
    // 返回单调 request revision。
    pub(crate) const fn request_revision(&self) -> u64 {
        // 复制 revision。
        self.request_revision
    }
    // 返回 operation。
    pub(crate) const fn operation(&self) -> BrowserSessionBrokerOperation {
        // 复制 operation。
        self.operation
    }
    // 返回可选 outcome。
    pub(crate) const fn outcome(&self) -> Option<BrowserSessionBrokerOutcome> {
        // 复制 outcome。
        self.outcome
    }
    // 返回 transport 接受事实。
    pub(crate) const fn transport_accepted(&self) -> bool {
        // 复制事实。
        self.transport_accepted
    }
    // 返回 business 接受事实。
    pub(crate) const fn business_accepted(&self) -> bool {
        // 复制事实。
        self.business_accepted
    }
    // 返回 completed 事实。
    pub(crate) const fn completed(&self) -> bool {
        // 复制事实。
        self.completed
    }
    // 返回 retry 事实。
    pub(crate) const fn retry_safe(&self) -> bool {
        // 复制事实。
        self.retry_safe
    }
    // 返回 unknown 事实。
    pub(crate) const fn outcome_unknown(&self) -> bool {
        // 复制事实。
        self.outcome_unknown
    }
    // 返回 mutation 事实。
    pub(crate) const fn target_may_have_mutated(&self) -> bool {
        // 复制事实。
        self.target_may_have_mutated
    }
    // 返回成功数据。
    pub(crate) fn success(&self) -> Option<&BrowserSessionBrokerSuccess> {
        // 借用可选成功投影。
        self.success.as_ref()
    }
    // 返回安全错误码。
    pub(crate) fn error_code(&self) -> Option<&str> {
        // 借用稳定错误码。
        self.error_code.as_deref()
    }
    // 返回安全错误说明。
    pub(crate) fn error_message(&self) -> Option<&str> {
        // 借用可选安全说明。
        self.error_message.as_deref()
    }
    // 计算缓存当前已验证响应所需的保守 replay 字节数。
    pub(crate) fn replay_payload_bytes(&self) -> usize {
        // 为固定 wire envelope、键名和关联字段预留空间。
        const RESPONSE_ENVELOPE_BYTES: usize = 1024;
        // 只对可能较大的结构化 data 计算真实 JSON 字节数。
        let data_bytes = match self.success.as_ref() {
            // query 的 Value 已由 request-bound validator 验证。
            Some(BrowserSessionBrokerSuccess::Query(value))
            // screenshot 的 Value 已由 PNG validator 验证。
            | Some(BrowserSessionBrokerSuccess::Screenshot(value)) => {
                // Value 序列化不会失败；失败时闭合为最大值以拒绝缓存。
                serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
            }
            // 其余响应只有固定有界字段。
            _ => 0,
        };
        // 防止整数溢出导致低估 replay 预算。
        data_bytes.saturating_add(RESPONSE_ENVELOPE_BYTES)
    }
}

// 验证成功数据与原 request 字段逐项一致且不泄漏私有标识。
fn valid_success(
    request: &BrowserSessionBrokerRequest,
    success: &BrowserSessionBrokerSuccess,
) -> bool {
    // 按 request 变体封闭匹配唯一成功数据。
    match (request, success) {
        // open 只允许 canonical session identity。
        (
            BrowserSessionBrokerRequest::Open(_),
            BrowserSessionBrokerSuccess::Open { session_id },
        ) => opaque(session_id, SESSION_PREFIX),
        // close 只允许关闭事实。
        (BrowserSessionBrokerRequest::Close(..), BrowserSessionBrokerSuccess::Close) => true,
        // session.inspect 只允许回显原 session 且 live=true。
        (
            BrowserSessionBrokerRequest::SessionInspect(_, session_id),
            BrowserSessionBrokerSuccess::SessionInspect {
                session_id: returned_session_id,
                live,
            },
        ) => session_id == returned_session_id && *live,
        // navigate 只允许 canonical page identity 与正代际。
        (
            BrowserSessionBrokerRequest::Navigate(..),
            BrowserSessionBrokerSuccess::Navigate {
                page_id,
                generation,
            },
        ) => opaque(page_id, PAGE_PREFIX) && *generation >= 1,
        // wait 只允许满足事实。
        (
            BrowserSessionBrokerRequest::Wait(_, _, page_id, _),
            BrowserSessionBrokerSuccess::Wait {
                page_id: returned_page_id,
                generation,
            },
        ) => page_id == returned_page_id && *generation >= 1,
        // query 结果还必须遵守该 request 的 maxResults。
        (
            BrowserSessionBrokerRequest::Query(_, _, page_id, _, max_results),
            BrowserSessionBrokerSuccess::Query(value),
        ) => query_data(value, usize::from(*max_results), page_id),
        // click 必须逐字绑定当前 page/element 与正代际。
        (
            BrowserSessionBrokerRequest::Click(_, _, page_id, element_id),
            BrowserSessionBrokerSuccess::Click {
                page_id: returned_page_id,
                element_id: returned_element_id,
                generation,
            },
        ) => page_id == returned_page_id && element_id == returned_element_id && *generation >= 1,
        // type 必须绑定 page/element，且字节数等于原始 UTF-8 文本。
        (
            BrowserSessionBrokerRequest::Type(_, _, page_id, element_id, text, _),
            BrowserSessionBrokerSuccess::Type {
                page_id: returned_page_id,
                element_id: returned_element_id,
                generation,
                utf8_bytes,
            },
        ) => {
            page_id == returned_page_id
                && element_id == returned_element_id
                && *generation >= 1
                && usize::from(*utf8_bytes) == text.len()
        }
        // screenshot 只允许绑定原 page 的封闭 PNG 数据。
        (
            BrowserSessionBrokerRequest::Screenshot(_, _, page_id),
            BrowserSessionBrokerSuccess::Screenshot(value),
        ) => screenshot_data(value, page_id),
        // 拒绝任何跨 operation 组合。
        _ => false,
    }
}
// 验证 opaque ID。
fn opaque(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|id| {
        id.len() == NONCE_LENGTH
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
// 验证 canonical request nonce。
fn canonical_nonce(value: &str) -> bool {
    value.len() == NONCE_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
// 验证 elementProjection 的精确 schema 形状。
fn element_projection(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    exact_keys(object, &["elementId", "role", "name", "text", "enabled"])
        && object
            .get("elementId")
            .and_then(Value::as_str)
            .is_some_and(|value| opaque(value, "s2:be:"))
        && nullable_text(object.get("role"))
        && nullable_text(object.get("name"))
        && nullable_text(object.get("text"))
        && object.get("enabled").and_then(Value::as_bool).is_some()
}
// 验证 nullable 且有界的可见文本。
fn nullable_text(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Null))
        || value
            .and_then(Value::as_str)
            .is_some_and(|value| value.chars().count() <= 1024)
}
// 验证精确键集合。
fn exact_keys(object: &serde_json::Map<String, Value>, keys: &[&str]) -> bool {
    object.len() == keys.len() && object.keys().all(|key| keys.contains(&key.as_str()))
}
// 验证 schema 与原 request 限定的 query 成功数据。
fn query_data(value: &Value, max_results: usize, expected_page_id: &str) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(page_id) = object.get("pageId").and_then(Value::as_str) else {
        return false;
    };
    let Some(generation) = object.get("navigationGeneration").and_then(Value::as_u64) else {
        return false;
    };
    let Some(matches) = object.get("matches").and_then(Value::as_array) else {
        return false;
    };
    let Some(count) = object.get("matchCount").and_then(Value::as_u64) else {
        return false;
    };
    let Some(truncated) = object.get("truncated").and_then(Value::as_bool) else {
        return false;
    };
    exact_keys(
        object,
        &[
            "pageId",
            "navigationGeneration",
            "matches",
            "matchCount",
            "truncated",
        ],
    ) && page_id == expected_page_id
        && opaque(page_id, PAGE_PREFIX)
        && (1..=u64::from(u32::MAX)).contains(&generation)
        && matches.len() <= max_results
        && matches.len() <= 100
        && matches.iter().all(element_projection)
        && count <= u64::from(u32::MAX)
        && count >= matches.len() as u64
        && truncated == (count > matches.len() as u64)
}
// 判断业务前 outcome 与错误码组合是否在冻结契约内。
const fn pre_acceptance_error_allowed(
    outcome: BrowserSessionBrokerOutcome,
    error: BrowserSessionBrokerProtocolErrorCode,
) -> bool {
    // 按业务前 outcome 封闭允许的稳定错误码。
    match outcome {
        // rejected 只能来自 parser、epoch、去重或容量门禁。
        BrowserSessionBrokerOutcome::Rejected => matches!(
            error,
            BrowserSessionBrokerProtocolErrorCode::InvalidArgument
                | BrowserSessionBrokerProtocolErrorCode::ConfirmationRequired
                | BrowserSessionBrokerProtocolErrorCode::StaleBrokerEpoch
                | BrowserSessionBrokerProtocolErrorCode::NonceSemanticConflict
                | BrowserSessionBrokerProtocolErrorCode::BrokerRequestLedgerFull
                | BrowserSessionBrokerProtocolErrorCode::BrowserSessionRegistryFull
                | BrowserSessionBrokerProtocolErrorCode::StaleSession
                | BrowserSessionBrokerProtocolErrorCode::StalePage
                | BrowserSessionBrokerProtocolErrorCode::StaleElement
        ),
        // expired-before-acceptance 只对应首次固定 deadline。
        BrowserSessionBrokerOutcome::ExpiredBeforeAcceptance => {
            // 使用唯一稳定 deadline 错误码。
            matches!(error, BrowserSessionBrokerProtocolErrorCode::RequestExpired)
        }
        // 拒绝其余 outcome 进入业务前构造。
        _ => false,
    }
}
// 验证 broker epoch。
fn opaque_epoch(value: &str) -> bool {
    // 使用 nonce 形状。
    value.len() == NONCE_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 在 JSON 解析前实施 broker control/response 的固定字节门禁。
pub(super) fn bounded_response_input(
    text: &str,
) -> Result<&str, super::BrowserSessionBrokerProtocolFailure> {
    // 超出冻结 response frame 上限时立即失败闭合。
    if text.len() > super::MAXIMUM_RESPONSE_FRAME_BYTES {
        // 不解析或分配攻击者控制的 JSON 树。
        return Err(super::invalid_argument());
    }
    // 与 request parser 一致地只移除 BOM 和外围空白。
    Ok(text.trim_start_matches('\u{feff}').trim())
}

// 生成不泄漏 target/provider 信息的固定说明。
fn default_error_message(error_code: &str) -> String {
    // 仅回显已经封闭的稳定错误码。
    format!("Browser session broker reported {error_code}.")
}
