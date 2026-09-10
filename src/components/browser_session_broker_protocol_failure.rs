// 导入标准错误链契约。
use std::error::Error;
// 导入稳定错误展示类型。
use std::fmt::{self, Display, Formatter};

// 导入父协议定义的已验证 operation 与 request。
use super::{BrowserSessionBrokerOperation, BrowserSessionBrokerRequest};

// 表示封闭输入与状态错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerProtocolErrorCode {
    // 表示字段、类型或版本不符合契约。
    InvalidArgument,
    // 表示 command 未先获得显式确认。
    ConfirmationRequired,
    // 表示 request 使用过期 broker epoch。
    StaleBrokerEpoch,
    // 表示相同 request nonce 代表不同完整语义。
    NonceSemanticConflict,
    // 表示连接已经消费唯一 request 槽位。
    ConnectionRequestLimit,
    // 表示 live request ledger 已达到固定容量。
    BrokerRequestLedgerFull,
    // 表示 live browser session registry 已达到固定容量。
    BrowserSessionRegistryFull,
    // 表示请求引用的公开 session 已关闭或不属于当前 live epoch。
    StaleSession,
    // 表示请求引用的公开 page 已被新导航换代或不存在。
    StalePage,
    // 表示请求引用的公开 element 不属于当前 page 代际。
    StaleElement,
    // 表示 request 在业务接受前耗尽首次固定 deadline。
    RequestExpired,
    // 表示不允许的状态转换。
    ProtocolFailed,
}

// 为错误码提供稳定文本。
impl BrowserSessionBrokerProtocolErrorCode {
    // 返回机器可读错误码。
    pub(crate) const fn as_str(self) -> &'static str {
        // 穷举封闭错误码。
        match self {
            // 映射常规参数错误。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射 stale epoch。
            Self::StaleBrokerEpoch => "STALE_BROKER_EPOCH",
            // 映射 nonce 同义冲突。
            Self::NonceSemanticConflict => "NONCE_SEMANTIC_CONFLICT",
            // 映射连接限额。
            Self::ConnectionRequestLimit => "CONNECTION_REQUEST_LIMIT",
            // 映射固定 ledger 容量拒绝。
            Self::BrokerRequestLedgerFull => "BROKER_REQUEST_LEDGER_FULL",
            // 映射固定 session registry 容量拒绝。
            Self::BrowserSessionRegistryFull => "BROWSER_SESSION_REGISTRY_FULL",
            // 映射失效 session 拒绝。
            Self::StaleSession => "STALE_SESSION",
            // 映射失效 page 拒绝。
            Self::StalePage => "STALE_PAGE",
            // 映射失效 element 拒绝。
            Self::StaleElement => "STALE_ELEMENT",
            // 映射业务接受前 deadline 耗尽。
            Self::RequestExpired => "REQUEST_EXPIRED",
            // 映射状态机错误。
            Self::ProtocolFailed => "BROKER_PROTOCOL_FAILED",
        }
    }

    // 从 wire 文本恢复业务前 request rejection 错误码。
    pub(crate) fn from_rejection_str(code: &str) -> Option<Self> {
        // 仅映射冻结的八种 request rejection。
        match code {
            // 映射字段错误。
            "INVALID_ARGUMENT" => Some(Self::InvalidArgument),
            // 映射确认缺失。
            "CONFIRMATION_REQUIRED" => Some(Self::ConfirmationRequired),
            // 映射 stale epoch。
            "STALE_BROKER_EPOCH" => Some(Self::StaleBrokerEpoch),
            // 映射 nonce 语义冲突。
            "NONCE_SEMANTIC_CONFLICT" => Some(Self::NonceSemanticConflict),
            // 映射 request ledger 容量。
            "BROKER_REQUEST_LEDGER_FULL" => Some(Self::BrokerRequestLedgerFull),
            // 映射 session registry 容量。
            "BROWSER_SESSION_REGISTRY_FULL" => Some(Self::BrowserSessionRegistryFull),
            // 映射 stale session。
            "STALE_SESSION" => Some(Self::StaleSession),
            // 映射 stale page。
            "STALE_PAGE" => Some(Self::StalePage),
            // 映射 stale element。
            "STALE_ELEMENT" => Some(Self::StaleElement),
            // 拒绝任何未冻结拒绝码。
            _ => None,
        }
    }
}

// 保存不回显调用方内容的失败。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionBrokerProtocolFailure {
    // 保存稳定错误码。
    code: BrowserSessionBrokerProtocolErrorCode,
    // 保存完整 envelope 是否已 transport 接受。
    transport_accepted: bool,
    // 保存已 canonical 的关联 nonce。
    request_nonce: Option<String>,
    // 保存已确定 operation。
    operation: Option<BrowserSessionBrokerOperation>,
    // 保存已 canonical 的期望 broker epoch。
    expected_epoch: Option<String>,
    // 仅严格 request-bound failure 保存完整 canonical 语义键。
    request_semantic_key: Option<String>,
}

// 为失败提供受控构造和只读访问。
impl BrowserSessionBrokerProtocolFailure {
    // 构造安全失败。
    pub(crate) const fn new(code: BrowserSessionBrokerProtocolErrorCode) -> Self {
        // 保存无 transport 接受事实的失败。
        Self {
            // 保存唯一错误类别。
            code,
            // 标记 envelope 尚未被接受。
            transport_accepted: false,
            // 不保留无法安全关联的 nonce。
            request_nonce: None,
            // 不保留无法安全确定的 operation。
            operation: None,
            // 不保留无法安全确定的 epoch。
            expected_epoch: None,
            // envelope 失败没有完整 strict request 语义。
            request_semantic_key: None,
        }
    }

    // 返回错误码。
    pub(crate) const fn code(&self) -> BrowserSessionBrokerProtocolErrorCode {
        // 复制枚举。
        self.code
    }

    // 构造 envelope 已 canonical 的业务前拒绝。
    pub(crate) fn rejected(
        // 接收稳定错误码。
        code: BrowserSessionBrokerProtocolErrorCode,
        // 接收 parser 已验证的 request nonce。
        request_nonce: String,
        // 接收 parser 已确定的 operation。
        operation: BrowserSessionBrokerOperation,
        // 接收 parser 已验证的 expected broker epoch。
        expected_epoch: String,
    ) -> Self {
        // 保存可安全回显的公共关联值。
        Self {
            // 保存稳定错误码。
            code,
            // 标记完整 envelope 已被 transport 接受。
            transport_accepted: true,
            // 保留 canonical request nonce。
            request_nonce: Some(request_nonce),
            // 保留已确定 operation。
            operation: Some(operation),
            // 保留 canonical expected broker epoch。
            expected_epoch: Some(expected_epoch),
            // parser early rejection 不得伪造完整 strict request 语义。
            request_semantic_key: None,
        }
    }

    // 从已严格解析的 request 安全构造可关联业务前拒绝。
    pub(crate) fn rejected_for_request(
        // 接收业务前稳定错误码。
        code: BrowserSessionBrokerProtocolErrorCode,
        // 绑定 parser 已验证的公共关联字段。
        request: &BrowserSessionBrokerRequest,
    ) -> Self {
        // 先复制完整 canonical request identity。
        let mut failure = Self::rejected(
            // 传递稳定错误码。
            code,
            // 复制 canonical nonce。
            request.request_nonce().to_owned(),
            // 复制已解析 operation。
            request.operation(),
            // 复制 canonical expected epoch。
            request.expected_broker_epoch().to_owned(),
        );
        // 仅此严格构造路径保存完整 canonical 语义键。
        failure.request_semantic_key = Some(request.canonical_semantic_key());
        // 返回可进入 ledger 的 strict failure。
        failure
    }

    // 返回 transport 接受事实。
    pub(crate) const fn transport_accepted(&self) -> bool {
        // 复制接受事实。
        self.transport_accepted
    }

    // 返回安全 request nonce。
    pub(crate) fn request_nonce(&self) -> Option<&str> {
        // 借用 canonical nonce。
        self.request_nonce.as_deref()
    }

    // 返回已安全确定的 operation。
    pub(crate) const fn operation(&self) -> Option<BrowserSessionBrokerOperation> {
        // 复制 operation。
        self.operation
    }

    // 返回已安全确定的期望 broker epoch。
    pub(crate) fn expected_broker_epoch(&self) -> Option<&str> {
        // 借用 canonical expected epoch。
        self.expected_epoch.as_deref()
    }

    // 返回严格 request-bound failure 的完整 canonical 语义键。
    pub(crate) fn request_semantic_key(&self) -> Option<&str> {
        // 借用可选完整语义键。
        self.request_semantic_key.as_deref()
    }
}

// 只输出稳定错误码。
impl Display for BrowserSessionBrokerProtocolFailure {
    // 格式化安全文本。
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // 不回显不可信输入。
        formatter.write_str(self.code.as_str())
    }
}

// 让调用方接入标准错误链。
impl Error for BrowserSessionBrokerProtocolFailure {}
