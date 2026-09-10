//! 定义独立交互会话 command worker 的封闭 JSON 协议。

// 导入协议序列化与反序列化派生。
use serde::{Deserialize, Serialize};
// 导入 provider-neutral JSON 值。
use serde_json::Value;

// 导入 canonical opaque 目标解析原语。
use super::opaque_id::{OpaqueTargetId, OpaqueTargetKind};

// 固定独立交互会话 command worker 协议版本。
pub(crate) const CONTRACT_VERSION: &str = "act/interactive-command-worker/v1";
// 固定一次性请求与 endpoint lease nonce 的十六进制长度。
const NONCE_LENGTH: usize = 32;
// 固定 worker 接受的最小 deadline。
const MINIMUM_TIMEOUT_MS: u32 = 1;
// 固定 worker 接受的最大 deadline。
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

// 表示协议边界允许公开的封闭错误码。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InteractiveCommandProtocolErrorCode {
    // 表示 JSON 或字段形状不合法。
    InvalidArgument,
    // 表示请求未携带逐操作确认。
    ConfirmationRequired,
    // 表示前景型 capability 未携带独立会话前景许可。
    ForegroundConsentRequired,
    // 表示请求试图放宽严格隔离计划。
    IsolationRequired,
    // 表示 capability 与 operation 不属于封闭路线。
    CapabilityGap,
    // 表示 worker 不是由固定认证 broker 创建。
    EndpointAuthenticationFailed,
}

// 为协议错误提供稳定 JSON 文本。
impl InteractiveCommandProtocolErrorCode {
    // 返回公开稳定错误码。
    pub(crate) const fn as_str(self) -> &'static str {
        // 穷举全部封闭错误。
        match self {
            // 输出参数错误。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 输出确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 输出独立会话前景许可缺失。
            Self::ForegroundConsentRequired => "FOREGROUND_CONSENT_REQUIRED",
            // 输出严格隔离缺失。
            Self::IsolationRequired => "ISOLATION_REQUIRED",
            // 输出 capability 缺口。
            Self::CapabilityGap => "CAPABILITY_GAP",
            // 输出 broker parent 认证失败。
            Self::EndpointAuthenticationFailed => "ENDPOINT_AUTHENTICATION_FAILED",
        }
    }

    // 返回不包含请求内容或平台事实的固定消息。
    pub(crate) const fn message(self) -> &'static str {
        // 按错误类别选择固定诊断。
        match self {
            // 描述协议字段非法。
            Self::InvalidArgument => "The interactive command worker request violates protocol v1.",
            // 描述逐操作确认缺失。
            Self::ConfirmationRequired => {
                "Interactive session mutation requires explicit confirmation."
            }
            // 描述独立会话前景许可缺失。
            Self::ForegroundConsentRequired => {
                "Interactive session foreground mutation requires explicit consent."
            }
            // 描述严格隔离计划不完整。
            Self::IsolationRequired => {
                "The interactive command worker only accepts a frozen strict isolation plan."
            }
            // 描述封闭 capability 路线。
            Self::CapabilityGap => {
                "The interactive command worker does not support this capability route."
            }
            // 描述固定 broker parent 缺失。
            Self::EndpointAuthenticationFailed => {
                "The fixed command worker parent could not be authenticated."
            }
        }
    }
}

// 表示协议解析阶段的稳定拒绝事实。
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct InteractiveCommandProtocolFailure {
    // 保存封闭错误码。
    code: InteractiveCommandProtocolErrorCode,
    // 只保存已经通过 envelope 验证的请求 nonce。
    request_nonce: Option<String>,
    // 区分 transport envelope 是否已完整接受。
    transport_accepted: bool,
}

// 为协议拒绝提供只读投影。
impl InteractiveCommandProtocolFailure {
    // 构造尚未接受 transport envelope 的失败。
    fn envelope(code: InteractiveCommandProtocolErrorCode) -> Self {
        // 返回不携带不可信关联值的失败。
        Self {
            // 保存封闭错误码。
            code,
            // 不回显未验证 nonce。
            request_nonce: None,
            // 标记 transport envelope 未接受。
            transport_accepted: false,
        }
    }

    // 返回封闭错误码。
    pub(crate) const fn code(&self) -> InteractiveCommandProtocolErrorCode {
        // 复制无状态枚举值。
        self.code
    }

    // 返回可安全回显的请求 nonce。
    pub(crate) fn request_nonce(&self) -> Option<&str> {
        // 将拥有型关联值投影为借用文本。
        self.request_nonce.as_deref()
    }

    // 返回 transport envelope 接受状态。
    pub(crate) const fn transport_accepted(&self) -> bool {
        // 只公开封闭布尔事实。
        self.transport_accepted
    }
}

// 表示 host 与 command worker 之间的严格一次性请求。
#[derive(Debug, Deserialize)]
// 使用 camelCase 并拒绝任意扩展字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InteractiveCommandRequest {
    // 保存固定协议版本。
    contract_version: String,
    // 保存不含目标事实的一次性请求 nonce。
    request_nonce: String,
    // 保存 broker 本次 endpoint lease 的一次性 nonce。
    endpoint_lease_nonce: String,
    // 保存版本化 provider-neutral capability。
    capability: String,
    // 保存统一 facade operation。
    operation: String,
    // 保存 worker 会话内需要重新解析的 canonical 窗口目标。
    session_id: String,
    // 保存 capability 自有的 provider-neutral 输入对象。
    input: Value,
    // 保存逐操作确认事实。
    confirmed: bool,
    // 保存只作用于独立会话的前景许可。
    foreground_consent: bool,
    // 保存 host 冻结的严格隔离要求。
    isolation_requirement: String,
    // 保存 host 冻结的外部执行域。
    required_execution_realm: String,
    // 保存 host 冻结的零干扰策略。
    host_impact_policy: String,
    // 保存覆盖完整请求生命周期的有界 deadline。
    timeout_ms: u32,
}

// 为严格请求提供解析、验证和最小读取接口。
impl InteractiveCommandRequest {
    // 严格解析单个 JSON 请求并保持 envelope 后 confirmation-first 顺序。
    pub(crate) fn parse(text: &str) -> Result<Self, InteractiveCommandProtocolFailure> {
        // 兼容标准输入前唯一 UTF-8 BOM 并去除外围空白。
        let normalized = text.trim_start_matches('\u{feff}').trim();
        // 只接受一个完整且字段封闭的 JSON 对象。
        let request = serde_json::from_str::<Self>(normalized)
            // 不回显潜在敏感输入。
            .map_err(|_| {
                // 字段未封闭时 transport envelope 不成立。
                InteractiveCommandProtocolFailure::envelope(
                    // 返回普通协议参数错误。
                    InteractiveCommandProtocolErrorCode::InvalidArgument,
                )
            })?;
        // 固定协议版本不得协商或向后猜测。
        if request.contract_version != CONTRACT_VERSION {
            // 返回协议形状错误。
            return Err(InteractiveCommandProtocolFailure::envelope(
                // 使用封闭参数错误。
                InteractiveCommandProtocolErrorCode::InvalidArgument,
            ));
        }
        // 两个 nonce 都必须是 canonical 128 位小写十六进制值。
        if !is_canonical_nonce(&request.request_nonce)
            // 同时验证 endpoint lease nonce。
            || !is_canonical_nonce(&request.endpoint_lease_nonce)
            // 请求与 lease nonce 不得复用同一值。
            || request.request_nonce == request.endpoint_lease_nonce
        {
            // 拒绝宽松、可变长度或可回显身份的关联值。
            return Err(InteractiveCommandProtocolFailure::envelope(
                // 使用封闭参数错误。
                InteractiveCommandProtocolErrorCode::InvalidArgument,
            ));
        }
        // 确认必须先于 capability、target 和 input 语义验证。
        if !request.confirmed {
            // 返回 transport 已接受的逐操作确认缺失。
            return Err(request.failure(
                // 使用固定确认错误。
                InteractiveCommandProtocolErrorCode::ConfirmationRequired,
            ));
        }
        // host 与 worker 必须冻结同一个严格执行计划。
        if request.isolation_requirement != "strict"
            // 外部执行域必须固定为隔离 worker。
            || request.required_execution_realm != "isolated-worker"
            // 主机影响策略必须固定为严格零干扰。
            || request.host_impact_policy != "strict-no-interference"
        {
            // 禁止 worker 接受降级后的计划。
            return Err(request.failure(
                // 使用固定隔离错误。
                InteractiveCommandProtocolErrorCode::IsolationRequired,
            ));
        }
        // 取得 capability 对应的唯一 operation 与前景许可要求。
        let Some((expected_operation, foreground_required)) = capability_route(&request.capability)
        else {
            // 未列入协议的 capability 保持结构化缺口。
            return Err(request.failure(
                // 使用固定 capability 缺口。
                InteractiveCommandProtocolErrorCode::CapabilityGap,
            ));
        };
        // capability 不得借用另一条统一 operation。
        if request.operation != expected_operation {
            // 将错配视为封闭路由缺口。
            return Err(request.failure(
                // 使用固定 capability 缺口。
                InteractiveCommandProtocolErrorCode::CapabilityGap,
            ));
        }
        // 前景型 capability 必须独立授权 worker 会话前景变化。
        if foreground_required && !request.foreground_consent {
            // 不把 host 当前桌面的许可隐式带入 worker。
            return Err(request.failure(
                // 使用固定前景许可错误。
                InteractiveCommandProtocolErrorCode::ForegroundConsentRequired,
            ));
        }
        // worker 只接受需要在自己 inventory 中重新解析的窗口目标。
        if OpaqueTargetId::parse(&request.session_id).map(OpaqueTargetId::kind)
            != Some(OpaqueTargetKind::Window)
        {
            // 拒绝 native、旧版本、元素快照和递归会话目标。
            return Err(request.failure(
                // 使用封闭参数错误。
                InteractiveCommandProtocolErrorCode::InvalidArgument,
            ));
        }
        // capability 输入必须保持对象形状且不得携带路由或第二 deadline。
        if !request.input.is_object()
            // 阻止 worker 成为第二层隔离路由入口。
            || contains_interactive_session_id(&request.input)
            // 完整生命周期只允许顶层 timeoutMs 一个 deadline 来源。
            || request.input.get("timeoutMs").is_some()
        {
            // 返回协议形状错误且不回显输入。
            return Err(request.failure(
                // 使用封闭参数错误。
                InteractiveCommandProtocolErrorCode::InvalidArgument,
            ));
        }
        // 完整 worker 生命周期必须使用明确且有界的 deadline。
        if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&request.timeout_ms) {
            // 拒绝零值和无界等待。
            return Err(request.failure(
                // 使用封闭参数错误。
                InteractiveCommandProtocolErrorCode::InvalidArgument,
            ));
        }
        // 返回已冻结且尚未触碰任何目标的请求。
        Ok(request)
    }

    // 返回已验证的一次性请求关联值。
    pub(crate) fn request_nonce(&self) -> &str {
        // 只暴露随机关联值，不暴露 endpoint 或目标事实。
        &self.request_nonce
    }

    // 返回已经通过协议验证的一次性 endpoint lease 关联值。
    pub(crate) fn endpoint_lease_nonce(&self) -> &str {
        // lease 只用于同一 broker 连接内的生命周期绑定。
        &self.endpoint_lease_nonce
    }

    // 返回覆盖 command worker 完整生命周期的剩余 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        // 数值已经通过 1..30000 范围验证。
        self.timeout_ms
    }

    // 返回已经通过封闭路由验证的 capability。
    pub(crate) fn capability(&self) -> &str {
        // 只借用版本化公开 ID。
        &self.capability
    }

    // 返回 capability 唯一匹配的 generic operation。
    pub(crate) fn operation(&self) -> &str {
        // 只借用已验证 operation。
        &self.operation
    }

    // 返回 worker 会话内必须重新解析的 canonical 窗口目标。
    pub(crate) fn session_id(&self) -> &str {
        // 只借用已验证 s2:w。
        &self.session_id
    }

    // 返回已移除递归 route 与第二 deadline 的领域输入。
    pub(crate) const fn input(&self) -> &Value {
        // 只借用字段封闭的 JSON object。
        &self.input
    }

    // 返回协议确认事实。
    pub(crate) const fn confirmed(&self) -> bool {
        // 解析成功时该值固定为 true。
        self.confirmed
    }

    // 返回目标会话前景许可事实。
    pub(crate) const fn foreground_consent(&self) -> bool {
        // 该许可不作用于 host 当前桌面。
        self.foreground_consent
    }

    // 构造 transport envelope 已接受后的语义拒绝。
    fn failure(
        // 借用当前已验证 envelope。
        &self,
        // 接收封闭错误码。
        code: InteractiveCommandProtocolErrorCode,
    ) -> InteractiveCommandProtocolFailure {
        // 返回携带可信请求关联值的失败。
        InteractiveCommandProtocolFailure {
            // 保存封闭错误码。
            code,
            // 只复制已通过格式验证的请求 nonce。
            request_nonce: Some(self.request_nonce.clone()),
            // 标记完整 transport envelope 已接受。
            transport_accepted: true,
        }
    }
}

// 检查固定长度的小写十六进制 nonce。
fn is_canonical_nonce(value: &str) -> bool {
    // 同时固定长度和字符集。
    value.len() == NONCE_LENGTH
        // 逐字节拒绝大写、分隔符和非 ASCII。
        && value
            // 遍历全部 nonce 字节。
            .bytes()
            // 只接受小写十六进制。
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 返回封闭 capability 路由与其独立会话前景许可要求。
pub(crate) fn capability_route(capability: &str) -> Option<(&'static str, bool)> {
    // 只列出独立交互会话契约的最小通用集合。
    match capability {
        // 键盘输入使用统一 apply 且需要独立会话前景许可。
        "ui.input.key@1" => Some(("apply", true)),
        // 指针输入使用统一 apply 且需要独立会话前景许可。
        "ui.input.pointer@1" => Some(("apply", true)),
        // 窗口状态和几何使用统一 apply 且需要独立会话前景许可。
        "window.lifecycle@1" => Some(("apply", true)),
        // 窗口关闭保留独立 close operation 且不要求激活目标。
        "window.close@1" => Some(("close", false)),
        // 其他 capability 不得经该 worker 任意转发。
        _ => None,
    }
}

// 递归检测禁止传给 command worker 的 endpoint 选择字段。
fn contains_interactive_session_id(value: &Value) -> bool {
    // 按 JSON 值类别递归检查。
    match value {
        // 对对象同时检查键名和子值。
        Value::Object(object) => object.iter().any(|(key, child)| {
            // 任意层级的递归 endpoint 字段都必须拒绝。
            key == "interactiveSessionId" || contains_interactive_session_id(child)
        }),
        // 对数组检查全部元素。
        Value::Array(items) => items.iter().any(contains_interactive_session_id),
        // 标量不可能携带 JSON 字段名。
        _ => false,
    }
}

// 表示 command worker 的稳定失败响应。
#[derive(Debug, Serialize)]
// 使用 camelCase 对齐内部 JSON 契约。
#[serde(rename_all = "camelCase")]
pub(crate) struct InteractiveCommandRejection {
    // 固定标记请求未成功。
    ok: bool,
    // 输出固定协议版本。
    contract_version: &'static str,
    // 仅对已完整验证的请求回显随机关联值。
    #[serde(skip_serializing_if = "Option::is_none")]
    request_nonce: Option<String>,
    // 区分协议 transport 是否已接受完整请求。
    transport_accepted: bool,
    // 明确业务 mutation 尚未被接受。
    business_accepted: bool,
    // 明确 capability 尚未完成。
    completed: bool,
    // 明确该失败发生在 dispatch 前。
    outcome: &'static str,
    // dispatch 前失败允许调用方修正请求后重试。
    retry_safe: bool,
    // 明确目标不可能因本请求改变。
    target_may_have_mutated: bool,
    // 输出封闭错误对象。
    error: InteractiveCommandError,
    // 输出零目标访问与零 fallback 证据。
    evidence: InteractiveCommandRejectionEvidence,
}

// 表示 command worker 的稳定错误对象。
#[derive(Debug, Serialize)]
// 使用 camelCase 保持协议一致。
#[serde(rename_all = "camelCase")]
struct InteractiveCommandError {
    // 保存稳定错误码。
    code: &'static str,
    // 保存不泄漏平台事实的固定消息。
    message: &'static str,
}

// 表示 dispatch 前拒绝的最小安全证据。
#[derive(Debug, Serialize)]
// 使用 camelCase 保持协议一致。
#[serde(rename_all = "camelCase")]
struct InteractiveCommandRejectionEvidence {
    // 证明未调用 host 或 worker 本地 provider。
    local_provider_invoked: bool,
    // 证明尚未解析 opaque 窗口目标。
    target_resolved: bool,
    // 证明尚未派发 mutation。
    mutation_dispatched: bool,
    // 证明没有回退当前桌面。
    foreground_fallback_used: bool,
}

// 构造不携带平台事实的 dispatch 前拒绝响应。
pub(crate) fn rejection(
    // 仅在完整请求通过协议验证后携带关联值。
    request_nonce: Option<&str>,
    // 区分协议 transport 是否接受完整请求。
    transport_accepted: bool,
    // 接收封闭协议错误码。
    code: InteractiveCommandProtocolErrorCode,
) -> InteractiveCommandRejection {
    // 返回字段组合固定的失败 envelope。
    InteractiveCommandRejection {
        // 标记业务失败。
        ok: false,
        // 固定输出 v1。
        contract_version: CONTRACT_VERSION,
        // 只回显已验证关联值。
        request_nonce: request_nonce.map(str::to_owned),
        // 输出 transport 接受状态。
        transport_accepted,
        // mutation 尚未被接受。
        business_accepted: false,
        // capability 尚未完成。
        completed: false,
        // 失败发生在 dispatch 前。
        outcome: "not-dispatched",
        // 修正前置条件后可以安全重新发起新请求。
        retry_safe: true,
        // 目标不可能已改变。
        target_may_have_mutated: false,
        // 构造封闭错误。
        error: InteractiveCommandError {
            // 输出稳定错误码。
            code: code.as_str(),
            // 输出固定安全消息。
            message: code.message(),
        },
        // 固定输出零副作用证据。
        evidence: InteractiveCommandRejectionEvidence {
            // 未调用任何本地 provider。
            local_provider_invoked: false,
            // 未解析窗口目标。
            target_resolved: false,
            // 未派发 mutation。
            mutation_dispatched: false,
            // 未回退当前桌面前景输入。
            foreground_fallback_used: false,
        },
    }
}

// 声明纯协议 Component 的回归测试。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 导入当前 Component 的私有接口。
    use super::*;

    // 构造最小合法的严格键盘请求。
    fn valid_request() -> Value {
        // 返回字段完全封闭的请求对象。
        json!({
            // 使用固定协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 使用 canonical 请求 nonce。
            "requestNonce": "0123456789abcdef0123456789abcdef",
            // 使用 canonical endpoint lease nonce。
            "endpointLeaseNonce": "fedcba9876543210fedcba9876543210",
            // 使用通用键盘 capability。
            "capability": "ui.input.key@1",
            // 使用统一 apply operation。
            "operation": "apply",
            // 使用 canonical 窗口目标。
            "sessionId": "s2:w:0000000000000001",
            // 使用 provider-neutral 输入。
            "input": { "key": "ENTER" },
            // 提供逐操作确认。
            "confirmed": true,
            // 提供独立会话前景许可。
            "foregroundConsent": true,
            // 固定严格隔离。
            "isolationRequirement": "strict",
            // 固定隔离 worker 外部执行域。
            "requiredExecutionRealm": "isolated-worker",
            // 固定严格零干扰策略。
            "hostImpactPolicy": "strict-no-interference",
            // 使用有界 deadline。
            "timeoutMs": 5000
        })
    }

    // 解析必须失败的 fixture 并返回结构化拒绝。
    fn failure_for(request: &Value) -> InteractiveCommandProtocolFailure {
        // 按严格协议解析当前 fixture。
        match InteractiveCommandRequest::parse(&request.to_string()) {
            // 返回预期的结构化拒绝。
            Err(failure) => failure,
            // 意外成功时终止测试。
            Ok(_) => panic!("invalid interactive command request was accepted"),
        }
    }

    // 验证合法严格请求可以解析且只暴露随机关联值。
    #[test]
    fn strict_request_parses_without_target_access() {
        // 序列化合法请求。
        let text = valid_request().to_string();
        // 严格解析请求。
        let request = InteractiveCommandRequest::parse(&text)
            // 合法 fixture 不应失败。
            .unwrap_or_else(|error| panic!("valid request rejected: {error:?}"));
        // 只核对安全随机关联值。
        assert_eq!(request.request_nonce(), "0123456789abcdef0123456789abcdef");
    }

    // 验证 confirmation-first 优先于 target 与 input 语义。
    #[test]
    fn confirmation_precedes_target_validation() {
        // 构造基础请求。
        let mut request = valid_request();
        // 移除逐操作确认。
        request["confirmed"] = json!(false);
        // 同时放入非法 native 目标。
        request["sessionId"] = json!("window:42");
        // 取得结构化拒绝。
        let failure = failure_for(&request);
        // 必须先返回确认错误。
        assert_eq!(
            failure.code(),
            InteractiveCommandProtocolErrorCode::ConfirmationRequired
        );
        // 完整 envelope 已经被 transport 接受。
        assert!(failure.transport_accepted());
        // 已验证的请求 nonce 可以安全关联响应。
        assert_eq!(
            failure.request_nonce(),
            Some("0123456789abcdef0123456789abcdef")
        );
    }

    // 验证 worker 不能接受递归独立会话路由。
    #[test]
    fn recursive_interactive_session_route_is_rejected() {
        // 构造基础请求。
        let mut request = valid_request();
        // 在嵌套输入中尝试注入第二层 endpoint 选择。
        request["input"] = json!({
            // 保留普通键盘字段。
            "key": "ENTER",
            // 嵌套递归路由字段。
            "routing": { "interactiveSessionId": "s2:i:0000000000000001" }
        });
        // 必须在任何目标解析前拒绝。
        assert_eq!(
            failure_for(&request).code(),
            InteractiveCommandProtocolErrorCode::InvalidArgument
        );
    }

    // 验证严格计划的三个冻结字段不可独立放宽。
    #[test]
    fn strict_plan_fields_cannot_be_downgraded() {
        // 逐个覆盖全部冻结字段。
        for (field, value) in [
            // 尝试放宽隔离要求。
            ("isolationRequirement", "standard"),
            // 尝试改回 host 前景域。
            ("requiredExecutionRealm", "host-foreground"),
            // 尝试改回后台优先策略。
            ("hostImpactPolicy", "background-preferred"),
        ] {
            // 构造独立请求。
            let mut request = valid_request();
            // 覆盖当前冻结字段。
            request[field] = json!(value);
            // 每一种放宽都必须返回隔离错误。
            assert_eq!(
                failure_for(&request).code(),
                InteractiveCommandProtocolErrorCode::IsolationRequired
            );
        }
    }

    // 验证 capability 与 operation 使用封闭一一映射。
    #[test]
    fn capability_routes_are_closed() {
        // 列出协议最小通用集合。
        for (capability, operation, foreground_required) in [
            // 覆盖键盘输入。
            ("ui.input.key@1", "apply", true),
            // 覆盖指针输入。
            ("ui.input.pointer@1", "apply", true),
            // 覆盖窗口生命周期。
            ("window.lifecycle@1", "apply", true),
            // 覆盖独立窗口关闭。
            ("window.close@1", "close", false),
        ] {
            // 构造独立请求。
            let mut request = valid_request();
            // 写入当前 capability。
            request["capability"] = json!(capability);
            // 写入唯一 operation。
            request["operation"] = json!(operation);
            // 写入当前前景要求。
            request["foregroundConsent"] = json!(foreground_required);
            // 封闭路线必须通过纯协议验证。
            assert!(InteractiveCommandRequest::parse(&request.to_string()).is_ok());
        }
        // 构造错误 operation 请求。
        let mut mismatch = valid_request();
        // 键盘 capability 不得借用 close。
        mismatch["operation"] = json!("close");
        // 错配必须保持 capability 缺口。
        assert_eq!(
            failure_for(&mismatch).code(),
            InteractiveCommandProtocolErrorCode::CapabilityGap
        );
    }

    // 验证 capability input 不能创建第二个 deadline 来源。
    #[test]
    fn input_timeout_cannot_override_request_deadline() {
        // 构造基础请求。
        let mut request = valid_request();
        // 尝试在 capability input 中重复 deadline。
        request["input"] = json!({ "key": "ENTER", "timeoutMs": 5000 });
        // 重复 deadline 必须在任何目标访问前拒绝。
        assert_eq!(
            failure_for(&request).code(),
            InteractiveCommandProtocolErrorCode::InvalidArgument
        );
    }

    // 验证请求 nonce 与 endpoint lease nonce 不得复用。
    #[test]
    fn request_and_endpoint_nonce_must_be_distinct() {
        // 构造基础请求。
        let mut request = valid_request();
        // 复用请求 nonce 作为 lease nonce。
        request["endpointLeaseNonce"] = request["requestNonce"].clone();
        // 取得 envelope 级拒绝。
        let failure = failure_for(&request);
        // 复用必须返回普通参数错误。
        assert_eq!(
            failure.code(),
            InteractiveCommandProtocolErrorCode::InvalidArgument
        );
        // 不可信 envelope 不得标记 transport 接受。
        assert!(!failure.transport_accepted());
        // 不得回显复用的 nonce。
        assert_eq!(failure.request_nonce(), None);
    }

    // 验证 dispatch 前拒绝响应固定零副作用证据。
    #[test]
    fn rejection_envelope_is_fail_closed() {
        // 构造已通过 transport 的 endpoint 认证拒绝响应。
        let response = rejection(
            // 使用已验证请求 nonce。
            Some("0123456789abcdef0123456789abcdef"),
            // 标记协议 transport 已接受。
            true,
            // 使用固定 broker parent 认证错误。
            InteractiveCommandProtocolErrorCode::EndpointAuthenticationFailed,
        );
        // 转换为 JSON 供逐字段断言。
        let value = serde_json::to_value(response)
            // 固定普通结构不应序列化失败。
            .unwrap_or_else(|error| panic!("response serialization failed: {error}"));
        // mutation 不得被接受。
        assert_eq!(value["businessAccepted"], false);
        // 结果必须分类为 dispatch 前。
        assert_eq!(value["outcome"], "not-dispatched");
        // 本地 provider 必须保持零调用。
        assert_eq!(value["evidence"]["localProviderInvoked"], false);
        // 当前桌面回退必须保持关闭。
        assert_eq!(value["evidence"]["foregroundFallbackUsed"], false);
    }
}
