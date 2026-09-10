//! 定义独立交互会话 session broker 的封闭本机协议。

// 导入协议序列化与反序列化派生。
use serde::{Deserialize, Serialize};
// 导入 provider-neutral JSON 值。
use serde_json::Value;

// 导入 command worker 协议与 opaque 目标原语。
use super::{
    // 复用已经冻结的一次性 command 请求验证。
    interactive_command_protocol::InteractiveCommandRequest,
    // 只允许 canonical 独立交互会话目标。
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
};

// 固定 session broker 协议版本。
pub(crate) const CONTRACT_VERSION: &str = "act/interactive-session-broker/v1";
// 固定同安装构建身份，不包含路径、用户名或平台标识。
pub(crate) const BROKER_BUILD_ID: &str = concat!(
    // 使用包名绑定主产品构建。
    env!("CARGO_PKG_NAME"),
    // 使用稳定分隔符。
    "/",
    // 绑定当前包版本。
    env!("CARGO_PKG_VERSION"),
    // 绑定当前 broker 协议角色。
    "/interactive-session-broker/v1"
);
// 固定请求与 lease nonce 的小写十六进制长度。
const NONCE_LENGTH: usize = 32;
// 固定 endpoint 发布的四条 provider-neutral mutation。
const CERTIFIED_CAPABILITIES: [&str; 4] = [
    // 发布完整通用键盘输入。
    "ui.input.key@1",
    // 发布完整通用指针输入。
    "ui.input.pointer@1",
    // 发布窗口状态与几何生命周期。
    "window.lifecycle@1",
    // 发布独立确认的窗口关闭。
    "window.close@1",
];

// 表示协议边界允许产生的封闭 broker 错误码。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrokerProtocolErrorCode {
    // 表示 JSON、版本、nonce 或字段形状不合法。
    InvalidArgument,
    // 表示 OS peer、固定构建或 endpoint 身份未认证。
    EndpointAuthenticationFailed,
    // 表示调用方使用的独立会话授权代际已经过期。
    StaleSession,
    // 表示固定 worker 返回了不符合契约的输出。
    WorkerProtocolFailed,
}

// 为 broker 错误提供稳定公开文本与安全消息。
impl BrokerProtocolErrorCode {
    // 返回版本化稳定错误码。
    pub(crate) const fn as_str(self) -> &'static str {
        // 穷举封闭 broker 错误集合。
        match self {
            // 映射参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射 endpoint 认证失败。
            Self::EndpointAuthenticationFailed => "ENDPOINT_AUTHENTICATION_FAILED",
            // 映射授权代际过期。
            Self::StaleSession => "STALE_SESSION",
            // 映射 worker 协议失败。
            Self::WorkerProtocolFailed => "WORKER_PROTOCOL_FAILED",
        }
    }

    // 返回不泄漏本机 endpoint、session 或进程事实的固定消息。
    pub(crate) const fn message(self) -> &'static str {
        // 按封闭错误类别选择稳定诊断。
        match self {
            // 描述协议字段错误。
            Self::InvalidArgument => "The interactive session broker request violates protocol v1.",
            // 描述认证失败且不公开失败维度。
            Self::EndpointAuthenticationFailed => {
                "The independent interactive session endpoint could not be authenticated."
            }
            // 描述 opaque 授权代际过期。
            Self::StaleSession => "The independent interactive session authorization is stale.",
            // 描述 worker 输出不可信。
            Self::WorkerProtocolFailed => {
                "The fixed interactive command worker returned an invalid response."
            }
        }
    }
}

// 表示 broker 协议解析阶段的稳定拒绝事实。
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct BrokerProtocolFailure {
    // 保存封闭错误码。
    code: BrokerProtocolErrorCode,
    // 只保存已经通过固定 envelope 验证的请求 nonce。
    request_nonce: Option<String>,
    // 区分完整 transport frame 是否被接受。
    transport_accepted: bool,
}

// 为协议拒绝提供安全构造与只读投影。
impl BrokerProtocolFailure {
    // 构造尚未接受固定 frame 的失败。
    fn envelope(code: BrokerProtocolErrorCode) -> Self {
        // 返回不回显不可信关联值的失败。
        Self {
            // 保存封闭错误码。
            code,
            // 不保存未认证 nonce。
            request_nonce: None,
            // 标记 transport frame 未接受。
            transport_accepted: false,
        }
    }

    // 构造固定 frame 已接受后的语义拒绝。
    pub(crate) fn accepted(code: BrokerProtocolErrorCode, request_nonce: &str) -> Self {
        // 返回只携带可信 nonce 的失败。
        Self {
            // 保存封闭错误码。
            code,
            // 复制 canonical 请求 nonce。
            request_nonce: Some(request_nonce.to_owned()),
            // 标记 transport frame 已接受。
            transport_accepted: true,
        }
    }

    // 返回封闭错误码。
    pub(crate) const fn code(&self) -> BrokerProtocolErrorCode {
        // 复制无状态枚举值。
        self.code
    }

    // 返回可安全回显的请求 nonce。
    pub(crate) fn request_nonce(&self) -> Option<&str> {
        // 将拥有型文本投影为借用。
        self.request_nonce.as_deref()
    }

    // 返回 transport frame 接受状态。
    pub(crate) const fn transport_accepted(&self) -> bool {
        // 只公开封闭布尔事实。
        self.transport_accepted
    }
}

// 表示连接建立后的第一条封闭操作。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
// 使用 kebab-case 对齐 JSON 协议。
#[serde(rename_all = "kebab-case")]
pub(crate) enum BrokerInitialOperation {
    // 表示只读 endpoint 观察后立即断开。
    Observe,
    // 表示签发同一连接内唯一 command lease。
    OpenCommandLease,
}

// 表示 broker 每条连接接受的第一条请求。
#[derive(Debug, Deserialize, Serialize)]
// 使用 camelCase 并拒绝任意扩展字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrokerInitialRequest {
    // 保存固定协议版本。
    contract_version: String,
    // 保存一次性请求关联值。
    request_nonce: String,
    // 保存只读观察或 lease 操作。
    operation: BrokerInitialOperation,
}

// 为首帧请求提供严格解析与只读事实。
impl BrokerInitialRequest {
    // 严格解析一条完整 JSON frame。
    pub(crate) fn parse(text: &str) -> Result<Self, BrokerProtocolFailure> {
        // 兼容唯一 UTF-8 BOM 并去除外围空白。
        let normalized = text.trim_start_matches('\u{feff}').trim();
        // 只接受字段封闭的单个 JSON 对象。
        let request = serde_json::from_str::<Self>(normalized)
            // 不回显不可信输入。
            .map_err(|_| {
                BrokerProtocolFailure::envelope(BrokerProtocolErrorCode::InvalidArgument)
            })?;
        // 固定版本不得协商或猜测。
        if request.contract_version != CONTRACT_VERSION
            // 请求 nonce 必须 canonical。
            || !is_canonical_nonce(&request.request_nonce)
        {
            // envelope 未完成时不回显 nonce。
            return Err(BrokerProtocolFailure::envelope(
                // 使用封闭参数错误。
                BrokerProtocolErrorCode::InvalidArgument,
            ));
        }
        // 返回尚未触碰任何 endpoint 或 worker 的请求。
        Ok(request)
    }

    // 构造 host 使用的严格首帧请求。
    pub(crate) fn new(request_nonce: String, operation: BrokerInitialOperation) -> Option<Self> {
        // host 也必须先产生 canonical 随机 nonce。
        is_canonical_nonce(&request_nonce).then_some(Self {
            // 固定当前协议版本。
            contract_version: CONTRACT_VERSION.to_owned(),
            // 保存已验证 nonce。
            request_nonce,
            // 保存封闭操作。
            operation,
        })
    }

    // 返回已验证请求 nonce。
    pub(crate) fn request_nonce(&self) -> &str {
        // 关联值不携带目标事实。
        &self.request_nonce
    }

    // 返回封闭首帧操作。
    pub(crate) const fn operation(&self) -> BrokerInitialOperation {
        // 复制无状态枚举值。
        self.operation
    }
}

// 表示经过固定 peer 校验后可以向 host 发布的 endpoint 证明。
#[derive(Clone, Debug, Deserialize, Serialize)]
// 使用 camelCase 并拒绝任意扩展字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrokerEndpointAttestation {
    // 保存授权代际绑定的公开独立会话目标。
    interactive_session_id: String,
    // 固定 provider-neutral 目标类别。
    target_kind: String,
    // 固定当前可用状态。
    state: String,
    // 固定隔离执行域。
    execution_realm: String,
    // 固定独立交互会话类别。
    isolation_kind: String,
    // 保存封闭通用 capability 集合。
    capabilities: Vec<String>,
    // 固定身份新鲜度语义。
    identity_freshness: String,
    // 固定 broker 自身活动会话事实。
    session_state: String,
    // 固定 broker 自身 Default 输入桌面事实。
    desktop_state: String,
    // 保存不含路径的固定构建身份。
    broker_build_id: String,
    // 证明固定 sibling command worker 已安装。
    command_worker_installed: bool,
    // 固定 OS peer 认证级别描述。
    peer_authentication: String,
}

// 为 endpoint 证明提供唯一构造与完整验证。
impl BrokerEndpointAttestation {
    // 从 broker 私有授权代际产生的公开目标构造证明。
    pub(crate) fn new(interactive_session_id: String) -> Result<Self, BrokerProtocolFailure> {
        // 只允许 canonical s2:i 进入认证证明。
        if OpaqueTargetId::parse(&interactive_session_id).map(OpaqueTargetId::kind)
            != Some(OpaqueTargetKind::InteractiveSession)
        {
            // 不接受其他 opaque 类别或宽松别名。
            return Err(BrokerProtocolFailure::envelope(
                // 内部构造错误仍使用封闭参数码。
                BrokerProtocolErrorCode::InvalidArgument,
            ));
        }
        // 返回全部事实固定的 endpoint 证明。
        Ok(Self {
            // 保存唯一可公开身份。
            interactive_session_id,
            // 不公开 Windows session number。
            target_kind: "independent-interactive-session".to_owned(),
            // 该类型只表示认证可用 endpoint。
            state: "available".to_owned(),
            // 命令固定在隔离 worker 域执行。
            execution_realm: "isolated-worker".to_owned(),
            // 区分普通 Job companion。
            isolation_kind: "independent-interactive-session".to_owned(),
            // 从封闭常量建立拥有型能力集合。
            capabilities: CERTIFIED_CAPABILITIES
                .iter()
                .map(ToString::to_string)
                .collect(),
            // 身份同时绑定会话与授权生命周期。
            identity_freshness: "authorization-and-session-lifetime".to_owned(),
            // broker 只在活动交互会话发布。
            session_state: "active-interactive".to_owned(),
            // broker 只在 Default 输入桌面发布。
            desktop_state: "active-default-input".to_owned(),
            // 使用同安装固定构建身份。
            broker_build_id: BROKER_BUILD_ID.to_owned(),
            // 构造前由 broker 确认 fixed sibling 存在。
            command_worker_installed: true,
            // 记录 host 必须独立完成的 OS peer 认证等级。
            peer_authentication: "os-process-session-principal-integrity-and-fixed-image"
                .to_owned(),
        })
    }

    // 验证从认证连接读取的证明没有字段漂移。
    fn validate(&self) -> Result<(), BrokerProtocolFailure> {
        // 验证公开目标类别与全部固定事实。
        let valid = OpaqueTargetId::parse(&self.interactive_session_id)
            // 只接受独立交互会话类别。
            .is_some_and(|target| target.kind() == OpaqueTargetKind::InteractiveSession)
            // 固定目标类别文本。
            && self.target_kind == "independent-interactive-session"
            // 固定可用状态。
            && self.state == "available"
            // 固定执行域。
            && self.execution_realm == "isolated-worker"
            // 固定隔离类别。
            && self.isolation_kind == "independent-interactive-session"
            // 能力集合和值顺序必须完全匹配。
            && self.capabilities == CERTIFIED_CAPABILITIES
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
            // 固定身份新鲜度。
            && self.identity_freshness == "authorization-and-session-lifetime"
            // 固定活动会话证明。
            && self.session_state == "active-interactive"
            // 固定输入桌面证明。
            && self.desktop_state == "active-default-input"
            // 构建身份必须同源。
            && self.broker_build_id == BROKER_BUILD_ID
            // fixed sibling 必须可用。
            && self.command_worker_installed
            // OS peer 认证等级不得放宽。
            && self.peer_authentication
                == "os-process-session-principal-integrity-and-fixed-image";
        // 任一字段漂移都使 endpoint 不可认证。
        if !valid {
            // 返回不公开具体失败维度的认证错误。
            return Err(BrokerProtocolFailure::envelope(
                // 使用 endpoint 认证失败码。
                BrokerProtocolErrorCode::EndpointAuthenticationFailed,
            ));
        }
        // 返回完整证明通过。
        Ok(())
    }

    // 返回公开独立交互会话目标。
    pub(crate) fn interactive_session_id(&self) -> &str {
        // 不公开任何 native session 事实。
        &self.interactive_session_id
    }
}

// 表示 broker 对首帧观察或 lease 的成功响应操作。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
// 使用 kebab-case 对齐 JSON 协议。
#[serde(rename_all = "kebab-case")]
pub(crate) enum BrokerEndpointOperation {
    // 表示只读观察结果。
    Observation,
    // 表示同一连接内的 command lease。
    CommandLease,
}

// 表示 broker 对首帧的认证成功响应。
#[derive(Debug, Deserialize, Serialize)]
// 使用 camelCase 并拒绝任意扩展字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrokerEndpointResponse {
    // 固定标记 broker transport 成功。
    ok: bool,
    // 输出固定协议版本。
    contract_version: String,
    // 回显已认证请求 nonce。
    request_nonce: String,
    // 区分观察与 lease。
    operation: BrokerEndpointOperation,
    // 只在 command lease 响应中输出 broker 生成的 nonce。
    #[serde(skip_serializing_if = "Option::is_none")]
    endpoint_lease_nonce: Option<String>,
    // 输出经过 broker 本地证明的 endpoint 事实。
    endpoint: BrokerEndpointAttestation,
}

// 为 endpoint 响应提供服务端构造和 host 严格解析。
impl BrokerEndpointResponse {
    // 构造只读观察响应。
    pub(crate) fn observation(request_nonce: &str, endpoint: BrokerEndpointAttestation) -> Self {
        // 返回不签发 lease 的响应。
        Self {
            // broker transport 已成功。
            ok: true,
            // 固定当前协议版本。
            contract_version: CONTRACT_VERSION.to_owned(),
            // 回显已验证 nonce。
            request_nonce: request_nonce.to_owned(),
            // 标记只读观察。
            operation: BrokerEndpointOperation::Observation,
            // 观察连接不产生 lease。
            endpoint_lease_nonce: None,
            // 保存认证 endpoint 证明。
            endpoint,
        }
    }

    // 构造同一连接内唯一 command lease 响应。
    pub(crate) fn command_lease(
        request_nonce: &str,
        endpoint_lease_nonce: String,
        endpoint: BrokerEndpointAttestation,
    ) -> Result<Self, BrokerProtocolFailure> {
        // lease 必须 canonical 且不得与请求 nonce 重用。
        if !is_canonical_nonce(&endpoint_lease_nonce) || endpoint_lease_nonce == request_nonce {
            // broker 自身随机源失败时拒绝签发。
            return Err(BrokerProtocolFailure::accepted(
                // 使用普通协议参数错误。
                BrokerProtocolErrorCode::InvalidArgument,
                // 回显已经验证的请求 nonce。
                request_nonce,
            ));
        }
        // 返回一次性 lease。
        Ok(Self {
            // broker transport 已成功。
            ok: true,
            // 固定当前协议版本。
            contract_version: CONTRACT_VERSION.to_owned(),
            // 回显已验证 nonce。
            request_nonce: request_nonce.to_owned(),
            // 标记 command lease。
            operation: BrokerEndpointOperation::CommandLease,
            // 保存 broker 生成的唯一 lease。
            endpoint_lease_nonce: Some(endpoint_lease_nonce),
            // 保存同一连接的 endpoint 证明。
            endpoint,
        })
    }

    // 从 host 收到的文本严格解析响应。
    pub(crate) fn parse(
        text: &str,
        expected_request_nonce: &str,
        expected_operation: BrokerEndpointOperation,
    ) -> Result<Self, BrokerProtocolFailure> {
        // 只接受字段封闭的单个 JSON 对象。
        let response = serde_json::from_str::<Self>(text.trim())
            // 不信任远端错误细节。
            .map_err(|_| {
                BrokerProtocolFailure::envelope(
                    BrokerProtocolErrorCode::EndpointAuthenticationFailed,
                )
            })?;
        // 核对固定 envelope、关联值和预期操作。
        let envelope_valid = response.ok
            // 版本必须完全一致。
            && response.contract_version == CONTRACT_VERSION
            // nonce 必须 canonical 且与本次请求一致。
            && is_canonical_nonce(&response.request_nonce)
            // 禁止接受另一请求的响应。
            && response.request_nonce == expected_request_nonce
            // 禁止观察与 lease 类型混淆。
            && response.operation == expected_operation;
        // envelope 漂移时 endpoint 不可认证。
        if !envelope_valid {
            // 返回统一认证失败。
            return Err(BrokerProtocolFailure::envelope(
                // 不区分具体失败字段。
                BrokerProtocolErrorCode::EndpointAuthenticationFailed,
            ));
        }
        // 观察响应不得携带 lease；lease 响应必须携带独立 canonical nonce。
        let lease_valid = match expected_operation {
            // 观察不能隐式签发 lease。
            BrokerEndpointOperation::Observation => response.endpoint_lease_nonce.is_none(),
            // command lease 必须 canonical 且区别于请求 nonce。
            BrokerEndpointOperation::CommandLease => response
                // 读取可选 lease。
                .endpoint_lease_nonce
                // 验证固定形状和唯一性。
                .as_deref()
                .is_some_and(|nonce| {
                    // 只接受 canonical nonce。
                    is_canonical_nonce(nonce)
                        // 不允许复用请求关联值。
                        && nonce != expected_request_nonce
                }),
        };
        // lease 形状不符合操作时拒绝整个 endpoint。
        if !lease_valid {
            // 返回统一认证失败。
            return Err(BrokerProtocolFailure::envelope(
                // 不回显异常 lease。
                BrokerProtocolErrorCode::EndpointAuthenticationFailed,
            ));
        }
        // 验证 endpoint 全部固定事实。
        response.endpoint.validate()?;
        // 返回已通过完整认证的响应。
        Ok(response)
    }

    // 返回认证 endpoint 证明。
    pub(crate) const fn endpoint(&self) -> &BrokerEndpointAttestation {
        // 只借用不可变证明。
        &self.endpoint
    }

    // 返回 command lease nonce。
    pub(crate) fn endpoint_lease_nonce(&self) -> Option<&str> {
        // 观察响应保持 None。
        self.endpoint_lease_nonce.as_deref()
    }
}

// 表示 lease 签发后同一连接必须接收的唯一 command frame。
#[derive(Debug, Deserialize, Serialize)]
// 使用 camelCase 并拒绝任意扩展字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BrokerCommandRequest {
    // 保存固定 broker 协议版本。
    contract_version: String,
    // 保存与首帧相同的请求 nonce。
    request_nonce: String,
    // 固定 command 操作文本。
    operation: String,
    // 保存 broker 在同一连接签发的 lease。
    endpoint_lease_nonce: String,
    // 保存调用方选择的公开独立会话授权代际。
    interactive_session_id: String,
    // 保存将原样交给固定 stdio worker 的封闭请求。
    command: Value,
}

// 为 command frame 提供严格交叉验证。
impl BrokerCommandRequest {
    // 严格解析 lease 后的唯一 command frame。
    pub(crate) fn parse(
        text: &str,
        expected_request_nonce: &str,
        expected_endpoint_lease_nonce: &str,
        expected_interactive_session_id: &str,
    ) -> Result<Self, BrokerProtocolFailure> {
        // 只接受字段封闭的单个 JSON 对象。
        let request = serde_json::from_str::<Self>(text.trim())
            // 不回显潜在目标或输入。
            .map_err(|_| {
                BrokerProtocolFailure::envelope(BrokerProtocolErrorCode::InvalidArgument)
            })?;
        // 固定 envelope 与两个 nonce 必须先完整成立。
        let envelope_valid = request.contract_version == CONTRACT_VERSION
            // 请求 nonce 必须 canonical。
            && is_canonical_nonce(&request.request_nonce)
            // lease nonce 必须 canonical。
            && is_canonical_nonce(&request.endpoint_lease_nonce)
            // 两种 nonce 不得复用。
            && request.request_nonce != request.endpoint_lease_nonce
            // 第二帧操作固定为 execute。
            && request.operation == "execute";
        // 无效 envelope 不回显 nonce。
        if !envelope_valid {
            // 返回 transport 未接受的参数错误。
            return Err(BrokerProtocolFailure::envelope(
                // 使用封闭参数错误。
                BrokerProtocolErrorCode::InvalidArgument,
            ));
        }
        // 同一连接必须复用首帧请求 nonce 与 broker 签发 lease。
        if request.request_nonce != expected_request_nonce
            // 拒绝其他连接或历史 lease。
            || request.endpoint_lease_nonce != expected_endpoint_lease_nonce
        {
            // 返回不公开具体错配维度的 endpoint 认证失败。
            return Err(BrokerProtocolFailure::accepted(
                // 使用统一认证错误。
                BrokerProtocolErrorCode::EndpointAuthenticationFailed,
                // 只回显本帧已验证 nonce。
                &request.request_nonce,
            ));
        }
        // 公开 endpoint 必须是当前 broker 授权代际。
        if request.interactive_session_id != expected_interactive_session_id
            // 同时拒绝非 canonical 或其他目标类别。
            || OpaqueTargetId::parse(&request.interactive_session_id).map(OpaqueTargetId::kind)
                != Some(OpaqueTargetKind::InteractiveSession)
        {
            // 返回明确 stale 且尚未启动 worker。
            return Err(BrokerProtocolFailure::accepted(
                // 使用授权代际过期码。
                BrokerProtocolErrorCode::StaleSession,
                // 回显已验证请求 nonce。
                &request.request_nonce,
            ));
        }
        // command 必须保持对象形状。
        if !request.command.is_object() {
            // 标准输入 worker 请求不会接受标量或数组。
            return Err(BrokerProtocolFailure::accepted(
                // 使用封闭参数错误。
                BrokerProtocolErrorCode::InvalidArgument,
                // 回显已验证请求 nonce。
                &request.request_nonce,
            ));
        }
        // 将同一 JSON 对象交给已经冻结的 command worker 协议验证。
        let command_text = serde_json::to_string(&request.command).map_err(|_| {
            // 序列化失败不应回显输入。
            BrokerProtocolFailure::accepted(
                // 使用封闭参数错误。
                BrokerProtocolErrorCode::InvalidArgument,
                // 回显已验证请求 nonce。
                &request.request_nonce,
            )
        })?;
        // 完整验证 confirmation-first、strict 计划、capability、目标与 deadline。
        let command = InteractiveCommandRequest::parse(&command_text).map_err(|failure| {
            // broker 只保留 command 协议是否接受，不传播私有类型。
            BrokerProtocolFailure {
                // command 语义拒绝保持参数错误，实际 worker 未启动。
                code: BrokerProtocolErrorCode::InvalidArgument,
                // 只在 command 协议已认证 nonce 时回显 broker nonce。
                request_nonce: failure
                    // 读取 command 可信关联值。
                    .request_nonce()
                    // 要求与 broker frame 相同。
                    .filter(|nonce| *nonce == request.request_nonce)
                    // 复制为 broker 拥有值。
                    .map(str::to_owned),
                // 复用 command 协议 transport 接受事实。
                transport_accepted: failure.transport_accepted(),
            }
        })?;
        // command 内外两个关联值必须逐字相同。
        if command.request_nonce() != request.request_nonce
            // command 必须绑定本连接 lease。
            || command.endpoint_lease_nonce() != request.endpoint_lease_nonce
        {
            // 拒绝替换或嵌套另一条 command。
            return Err(BrokerProtocolFailure::accepted(
                // 使用统一 endpoint 认证错误。
                BrokerProtocolErrorCode::EndpointAuthenticationFailed,
                // 回显已验证 broker nonce。
                &request.request_nonce,
            ));
        }
        // 返回尚未启动 worker 的完整认证 command frame。
        Ok(request)
    }

    // 构造 host 在同一 lease 连接发送的 command frame。
    pub(crate) fn new(
        request_nonce: String,
        endpoint_lease_nonce: String,
        interactive_session_id: String,
        command: Value,
    ) -> Self {
        // host 仍需通过 broker parse 执行最终交叉验证。
        Self {
            // 固定当前 broker 协议版本。
            contract_version: CONTRACT_VERSION.to_owned(),
            // 保存首帧关联值。
            request_nonce,
            // 固定第二帧操作。
            operation: "execute".to_owned(),
            // 保存 broker 签发 lease。
            endpoint_lease_nonce,
            // 保存调用方发现的公开 endpoint。
            interactive_session_id,
            // 保存已冻结 command worker 请求。
            command,
        }
    }

    // 返回将原样交给 fixed sibling worker 的 command JSON。
    pub(crate) const fn command(&self) -> &Value {
        // 只借用已经完整交叉验证的对象。
        &self.command
    }

    // 返回已经绑定首帧的请求 nonce。
    pub(crate) fn request_nonce(&self) -> &str {
        // 只公开不携带目标事实的关联值。
        &self.request_nonce
    }

    // 返回本连接唯一 endpoint lease nonce。
    pub(crate) fn endpoint_lease_nonce(&self) -> &str {
        // lease 不跨连接复用。
        &self.endpoint_lease_nonce
    }

    // 返回 command worker 生命周期剩余 deadline。
    pub(crate) fn command_timeout_ms(&self) -> Result<u32, BrokerProtocolFailure> {
        // 序列化已经验证的 command 对象。
        let text = serde_json::to_string(&self.command).map_err(|_| {
            // 理论上的内部漂移使用 worker 协议失败。
            BrokerProtocolFailure::accepted(
                // 标记内部 worker 协议不可认证。
                BrokerProtocolErrorCode::WorkerProtocolFailed,
                // 回显已验证请求 nonce。
                &self.request_nonce,
            )
        })?;
        // 重新解析只为取得强类型有界 deadline。
        InteractiveCommandRequest::parse(&text)
            // 投影已经验证的数值。
            .map(|command| command.timeout_ms())
            // 任何漂移都视为内部 worker 协议失败。
            .map_err(|_| {
                // 返回不可自动放宽的失败。
                BrokerProtocolFailure::accepted(
                    // 使用 worker 协议失败码。
                    BrokerProtocolErrorCode::WorkerProtocolFailed,
                    // 回显已验证请求 nonce。
                    &self.request_nonce,
                )
            })
    }
}

// 检查固定长度的小写十六进制 nonce。
pub(crate) fn is_canonical_nonce(value: &str) -> bool {
    // 同时固定长度和字符集。
    value.len() == NONCE_LENGTH
        // 逐字节拒绝大写、分隔符和非 ASCII。
        && value
            // 遍历全部 nonce 字节。
            .bytes()
            // 只接受小写十六进制。
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 声明纯 broker 协议 Component 的回归测试。
#[cfg(test)]
// 将大规模协议矩阵保存在独立测试文件，保持生产 Component 小于行数上限。
#[path = "interactive_session_broker_protocol_tests.rs"]
mod tests;
