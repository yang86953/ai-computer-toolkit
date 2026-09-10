//! 定义同会话浏览器 broker 的封闭输入 parser 与协议值类型。

// 导入 JSON 对象和值。
use serde_json::{Map, Value};

// 注册同协议的失败投影 Component。
#[path = "browser_session_broker_protocol_failure.rs"]
// 声明失败 Component 模块。
mod failure;
// 重新导出协议内部使用的稳定失败契约。
pub(crate) use failure::{
    // 重新导出封闭协议错误码。
    BrowserSessionBrokerProtocolErrorCode,
    // 重新导出可关联失败事实。
    BrowserSessionBrokerProtocolFailure,
};
// 注册同协议的状态 Component。
#[path = "browser_session_broker_protocol_state.rs"]
pub(crate) mod state;
// 注册同协议的响应 Component。
#[path = "browser_session_broker_protocol_response.rs"]
pub(crate) mod response;
// 注册同协议的严格 wire 编解码 Component。
#[path = "browser_session_broker_wire.rs"]
pub(crate) mod wire;
// 注册 URL 等窄输入验证 Component。
#[path = "browser_session_broker_protocol_validation.rs"]
mod validation;

// 固定版本化协议名。
pub(crate) const CONTRACT_VERSION: &str = "act/browser-session-broker/v1";
// 固定 nonce 与 epoch 长度。
pub(crate) const NONCE_LENGTH: usize = 32;
// 固定语义摘要长度。
pub(crate) const FINGERPRINT_LENGTH: usize = 16;
// 固定请求预算上界。
pub(crate) const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
// 固定每条 request/cancel 输入 frame 的 UTF-8 上界。
pub(crate) const MAXIMUM_INPUT_FRAME_BYTES: usize = 64 * 1024;
// 固定每条 broker control/response frame 的 UTF-8 上界。
pub(crate) const MAXIMUM_RESPONSE_FRAME_BYTES: usize = 16 * 1024 * 1024 + 128 * 1024;
// 固定输入文本上界。
pub(crate) const MAXIMUM_TYPE_TEXT_BYTES: usize = 16 * 1024;
// 固定 session opaque 前缀。
pub(crate) const SESSION_PREFIX: &str = "s2:bs:";
// 固定 page opaque 前缀。
pub(crate) const PAGE_PREFIX: &str = "s2:bp:";
// 固定 element opaque 前缀。
pub(crate) const ELEMENT_PREFIX: &str = "s2:be:";

// 验证公开页面 identity 是否属于 canonical `s2:bp` 外壳。
pub(crate) fn canonical_page_id(value: &str) -> bool {
    // 复用 broker 协议的固定前缀、长度与小写十六进制边界。
    value
        // 去掉唯一公开页面前缀。
        .strip_prefix(PAGE_PREFIX)
        // 只接受三十二位随机 identity 后缀。
        .is_some_and(|suffix| hex(suffix, NONCE_LENGTH))
}

// 验证公开元素 identity 是否属于 canonical `s2:be` 外壳。
pub(crate) fn canonical_element_id(value: &str) -> bool {
    // 复用 broker 协议的固定前缀、长度与小写十六进制边界。
    value
        // 去掉唯一公开元素前缀。
        .strip_prefix(ELEMENT_PREFIX)
        // 只接受三十二位随机 identity 后缀。
        .is_some_and(|suffix| hex(suffix, NONCE_LENGTH))
}

// 验证公开页面导航 URL 是否满足 broker 同源完整边界。
pub(crate) fn public_http_url(value: &str) -> bool {
    // 委托无状态 URL 验证 Component，避免 public client 与 wire parser 漂移。
    validation::http_url(value)
}

// 表示 provider-neutral selector。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSemanticSelector {
    // 保存可选 role。
    role: Option<String>,
    // 保存可选名称。
    name: Option<String>,
    // 保存可选文本。
    text: Option<String>,
    // 保存精确匹配标记。
    exact: bool,
}

// 为 selector 提供只读投影。
impl BrowserSemanticSelector {
    // 从 provider-neutral 字段构造 selector，完整边界仍由 wire parser 复核。
    pub(crate) fn new(
        // 接收可选 role。
        role: Option<String>,
        // 接收可选名称。
        name: Option<String>,
        // 接收可选文本。
        text: Option<String>,
        // 接收精确匹配标记。
        exact: bool,
    ) -> Self {
        // 保存不含 native 查询语言的字段。
        Self {
            // 保存 role。
            role,
            // 保存名称。
            name,
            // 保存文本。
            text,
            // 保存精确标记。
            exact,
        }
    }

    // 返回 role。
    pub(crate) fn role(&self) -> Option<&str> {
        // 借用 role。
        self.role.as_deref()
    }

    // 返回名称。
    pub(crate) fn name(&self) -> Option<&str> {
        // 借用名称。
        self.name.as_deref()
    }

    // 返回文本。
    pub(crate) fn text(&self) -> Option<&str> {
        // 借用文本。
        self.text.as_deref()
    }

    // 返回精确标记。
    pub(crate) const fn exact(&self) -> bool {
        // 复制布尔值。
        self.exact
    }
}

// 表示有限 wait 条件。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BrowserWaitCondition {
    // 等待文档 ready。
    DocumentReady,
    // 等待语义元素出现。
    ElementPresent(BrowserSemanticSelector),
    // 等待文本出现。
    TextPresent {
        // 保存目标文本。
        text: String,
        // 保存匹配方式。
        exact: bool,
    },
}

// 表示冻结的九种领域操作。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerOperation {
    // 打开会话。
    Open,
    // 关闭会话。
    Close,
    // 查询会话是否仍存活。
    SessionInspect,
    // 导航会话。
    Navigate,
    // 等待状态。
    Wait,
    // 查询元素。
    Query,
    // 点击元素。
    Click,
    // 输入文本。
    Type,
    // 截图。
    Screenshot,
}

// 表示请求的交互角色。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerRequestRole {
    // 会改变目标的 command。
    Command,
    // 无隐藏副作用的 query。
    Query,
}

// 保存所有 request 共有的私有字段。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RequestIdentity {
    // 保存 request nonce。
    nonce: String,
    // 保存调用方计算的 wire 摘要。
    fingerprint: String,
    // 保存所期待的 broker epoch。
    expected_epoch: String,
    // 保存当前调用剩余预算。
    remaining_timeout_ms: u32,
}

// 表示严格 parser 输出的 request。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerRequest {
    // open 没有 target 或 payload。
    Open(RequestIdentity),
    // close 只有 session。
    Close(RequestIdentity, String),
    // session.inspect 只读取 session 存活事实。
    SessionInspect(RequestIdentity, String),
    // navigate 有 session 与 URL。
    Navigate(RequestIdentity, String, String),
    // wait 有 session、page 与条件。
    Wait(RequestIdentity, String, String, BrowserWaitCondition),
    // query 有 session、page、selector 与 limit。
    Query(
        RequestIdentity,
        String,
        String,
        BrowserSemanticSelector,
        u16,
    ),
    // click 有三级 opaque target。
    Click(RequestIdentity, String, String, String),
    // type 有三级 target、文本与 replace。
    Type(RequestIdentity, String, String, String, String, bool),
    // screenshot 有 session 与 page。
    Screenshot(RequestIdentity, String, String),
}

// 为 request 提供统一严格解析。
impl BrowserSessionBrokerRequest {
    // 在解析 target/payload 前校验当前 broker epoch。
    pub(crate) fn parse_for_epoch(
        text: &str,
        current_epoch: &str,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // 读取 root 以验证安全 envelope。
        let value =
            serde_json::from_str::<Value>(bounded_input(text)?).map_err(|_| invalid_argument())?;
        // 取得对象。
        let object = value.as_object().ok_or_else(invalid_argument)?;
        // 校验 request kind 与公共 identity。
        if string(object, "kind")? != "request" {
            // 拒绝错误 frame。
            return Err(invalid_argument());
        }
        // 只读取公共关联字段。
        let identity = identity(object)?;
        // 安全解析 operation 标签。
        let operation = operation(string(object, "operation")?)?;
        // stale epoch 必须先于任何 target、payload 和确认判断。
        if identity.expected_epoch != current_epoch {
            // 返回可关联的业务前 stale 拒绝。
            return Err(BrowserSessionBrokerProtocolFailure::rejected(
                BrowserSessionBrokerProtocolErrorCode::StaleBrokerEpoch,
                identity.nonce,
                operation,
                identity.expected_epoch,
            ));
        }
        // 当前 epoch 才进入完整字段 parser。
        Self::parse(text).map_err(|failure| {
            // 公共 envelope 已 canonical，后续拒绝可安全关联。
            if failure.transport_accepted() {
                // 保留已有关联失败。
                failure
            } else {
                // 只回显 nonce 和 operation，不回显 target 或 payload。
                BrowserSessionBrokerProtocolFailure::rejected(
                    failure.code(),
                    identity.nonce,
                    operation,
                    identity.expected_epoch,
                )
            }
        })
    }

    // 解析 kind=request frame。
    fn parse(text: &str) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // 读取唯一 JSON 根对象。
        let value =
            serde_json::from_str::<Value>(bounded_input(text)?).map_err(|_| invalid_argument())?;
        // 取得对象。
        let object = value.as_object().ok_or_else(invalid_argument)?;
        // 校验 frame kind。
        if string(object, "kind")? != "request" {
            // 拒绝错误 frame。
            return Err(invalid_argument());
        }
        // 先校验版本与公共关联字段。
        let identity = identity(object)?;
        // 读取 operation。
        let operation = string(object, "operation")?;
        // 按封闭 operation 解析。
        let request = match operation {
            // open 确认优先。
            "open" => parse_open(object, identity)?,
            // close 确认优先。
            "close" => parse_close(object, identity)?,
            // session.inspect 是无确认的 query。
            "session.inspect" => parse_session_inspect(object, identity)?,
            // navigate 确认优先。
            "navigate" => parse_navigate(object, identity)?,
            // wait 是 query。
            "wait" => parse_wait(object, identity)?,
            // query 是 query。
            "query" => parse_query(object, identity)?,
            // click 确认优先。
            "click" => parse_click(object, identity)?,
            // type 确认优先。
            "type" => parse_type(object, identity)?,
            // screenshot 是 query。
            "screenshot" => parse_screenshot(object, identity)?,
            // 未知操作拒绝。
            _ => return Err(invalid_argument()),
        };
        // 指纹必须等于规范语义计算结果。
        if request.semantic_fingerprint() != request.computed_semantic_fingerprint() {
            // 拒绝 wire 摘要漂移。
            return Err(invalid_argument());
        }
        // 返回严格请求。
        Ok(request)
    }

    // 返回 request nonce。
    pub(crate) fn request_nonce(&self) -> &str {
        // 投影共有 identity。
        &self.identity().nonce
    }

    // 返回期望 broker epoch。
    pub(crate) fn expected_broker_epoch(&self) -> &str {
        // 投影共有 identity。
        &self.identity().expected_epoch
    }

    // 返回剩余预算。
    pub(crate) fn remaining_timeout_ms(&self) -> u32 {
        // 投影共有 identity。
        self.identity().remaining_timeout_ms
    }

    // 返回 wire 指纹。
    pub(crate) fn semantic_fingerprint(&self) -> &str {
        // 投影共有 identity。
        &self.identity().fingerprint
    }

    // 返回 operation。
    pub(crate) const fn operation(&self) -> BrowserSessionBrokerOperation {
        // 映射变体。
        match self {
            // 映射 open。
            Self::Open(_) => BrowserSessionBrokerOperation::Open,
            // 映射 close。
            Self::Close(..) => BrowserSessionBrokerOperation::Close,
            // 映射 session.inspect。
            Self::SessionInspect(..) => BrowserSessionBrokerOperation::SessionInspect,
            // 映射 navigate。
            Self::Navigate(..) => BrowserSessionBrokerOperation::Navigate,
            // 映射 wait。
            Self::Wait(..) => BrowserSessionBrokerOperation::Wait,
            // 映射 query。
            Self::Query(..) => BrowserSessionBrokerOperation::Query,
            // 映射 click。
            Self::Click(..) => BrowserSessionBrokerOperation::Click,
            // 映射 type。
            Self::Type(..) => BrowserSessionBrokerOperation::Type,
            // 映射 screenshot。
            Self::Screenshot(..) => BrowserSessionBrokerOperation::Screenshot,
        }
    }

    // 返回交互角色。
    pub(crate) const fn role(&self) -> BrowserSessionBrokerRequestRole {
        // 只读操作是 query。
        match self.operation() {
            // session.inspect 是 query。
            BrowserSessionBrokerOperation::SessionInspect
            // wait 是 query。
            | BrowserSessionBrokerOperation::Wait
            // query 是 query。
            | BrowserSessionBrokerOperation::Query
            // screenshot 是 query。
            | BrowserSessionBrokerOperation::Screenshot => BrowserSessionBrokerRequestRole::Query,
            // 其余是 command。
            _ => BrowserSessionBrokerRequestRole::Command,
        }
    }

    // 返回是否可能突变目标。
    pub(crate) const fn may_mutate_target(&self) -> bool {
        // command 才可能突变。
        matches!(self.role(), BrowserSessionBrokerRequestRole::Command)
    }

    // 构造完整且无碰撞假设的 canonical 语义键。
    pub(crate) fn canonical_semantic_key(&self) -> String {
        // 初始化版本化语义键。
        let mut key = String::from(CONTRACT_VERSION);
        // epoch 属于不可变 request 语义。
        piece(&mut key, &self.identity().expected_epoch);
        // 追加不可变业务字段。
        match self {
            // open 只有操作。
            Self::Open(_) => piece(&mut key, "open"),
            // close 绑定 session。
            Self::Close(_, session) => {
                // 追加操作。
                piece(&mut key, "close");
                // 追加目标。
                piece(&mut key, session);
            }
            // session.inspect 绑定唯一会话目标。
            Self::SessionInspect(_, session) => {
                // 追加操作。
                piece(&mut key, "session.inspect");
                // 追加会话目标。
                piece(&mut key, session);
            }
            // navigate 绑定 session 与 URL。
            Self::Navigate(_, session, url) => {
                // 追加操作。
                piece(&mut key, "navigate");
                // 追加会话。
                piece(&mut key, session);
                // 追加 URL。
                piece(&mut key, url);
            }
            // wait 绑定完整条件。
            Self::Wait(_, session, page, condition) => {
                // 追加操作。
                piece(&mut key, "wait");
                // 追加会话。
                piece(&mut key, session);
                // 追加页面。
                piece(&mut key, page);
                // 追加条件。
                condition_key(&mut key, condition);
            }
            // query 绑定 selector 和 limit。
            Self::Query(_, session, page, selector, limit) => {
                // 追加操作。
                piece(&mut key, "query");
                // 追加会话。
                piece(&mut key, session);
                // 追加页面。
                piece(&mut key, page);
                // 追加 selector。
                selector_key(&mut key, selector);
                // 追加 limit。
                piece(&mut key, &limit.to_string());
            }
            // click 绑定三级目标。
            Self::Click(_, session, page, element) => {
                // 追加操作。
                piece(&mut key, "click");
                // 追加会话。
                piece(&mut key, session);
                // 追加页面。
                piece(&mut key, page);
                // 追加元素。
                piece(&mut key, element);
            }
            // type 绑定三级目标、文本和 replace。
            Self::Type(_, session, page, element, text, replace) => {
                // 追加操作。
                piece(&mut key, "type");
                // 追加会话。
                piece(&mut key, session);
                // 追加页面。
                piece(&mut key, page);
                // 追加元素。
                piece(&mut key, element);
                // 追加文本。
                piece(&mut key, text);
                // 追加 replace。
                piece(&mut key, if *replace { "1" } else { "0" });
            }
            // screenshot 绑定 session/page。
            Self::Screenshot(_, session, page) => {
                // 追加操作。
                piece(&mut key, "screenshot");
                // 追加会话。
                piece(&mut key, session);
                // 追加页面。
                piece(&mut key, page);
            }
        }
        // 返回完整键。
        key
    }

    // 计算 FNV-1a 64-bit wire 指纹。
    pub(crate) fn computed_semantic_fingerprint(&self) -> String {
        // 委托同源 wire 摘要 helper。
        fingerprint_for_canonical_key(&self.canonical_semantic_key())
    }

    // 借用变体共有 identity。
    fn identity(&self) -> &RequestIdentity {
        // 统一投影第一个字段。
        match self {
            // 绑定 open identity。
            Self::Open(identity)
            // 绑定 close identity。
            | Self::Close(identity, ..)
            // 绑定 session.inspect identity。
            | Self::SessionInspect(identity, ..)
            // 绑定 navigate identity。
            | Self::Navigate(identity, ..)
            // 绑定 wait identity。
            | Self::Wait(identity, ..)
            // 绑定 query identity。
            | Self::Query(identity, ..)
            // 绑定 click identity。
            | Self::Click(identity, ..)
            // 绑定 type identity。
            | Self::Type(identity, ..)
            // 绑定 screenshot identity。
            | Self::Screenshot(identity, ..) => identity,
        }
    }
}

// 表示独立 cancel 控制请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionBrokerCancellationRequest {
    // 保存 cancel 请求自己的稳定 nonce。
    cancel_nonce: String,
    // 保存目标 request nonce。
    request_nonce: String,
    // 保存期望 broker epoch。
    expected_epoch: String,
}

// 为 cancel 提供严格解析和访问。
impl BrowserSessionBrokerCancellationRequest {
    // 解析 kind=cancel frame。
    pub(crate) fn parse(text: &str) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // 读取唯一 JSON 对象。
        let value =
            serde_json::from_str::<Value>(bounded_input(text)?).map_err(|_| invalid_argument())?;
        // 取得对象。
        let object = value.as_object().ok_or_else(invalid_argument)?;
        // 校验精确字段集合。
        exact(
            object,
            vec![
                "kind",
                "contractVersion",
                "cancelRequestNonce",
                "requestNonce",
                "expectedBrokerEpoch",
            ],
        )?;
        // 校验 kind 和版本。
        if string(object, "kind")? != "cancel"
            || string(object, "contractVersion")? != CONTRACT_VERSION
        {
            // 拒绝错误 frame。
            return Err(invalid_argument());
        }
        // 读取 cancel nonce。
        let cancel_nonce = string(object, "cancelRequestNonce")?.to_owned();
        // 读取目标 request nonce。
        let request_nonce = string(object, "requestNonce")?.to_owned();
        // 读取期望 epoch。
        let expected_epoch = string(object, "expectedBrokerEpoch")?.to_owned();
        // 校验三项 canonical nonce。
        if !hex(&cancel_nonce, NONCE_LENGTH)
            || !hex(&request_nonce, NONCE_LENGTH)
            || !hex(&expected_epoch, NONCE_LENGTH)
        {
            // 拒绝非 canonical identity。
            return Err(invalid_argument());
        }
        // 返回严格 cancel。
        Ok(Self {
            cancel_nonce,
            request_nonce,
            expected_epoch,
        })
    }

    // 返回 cancel nonce。
    pub(crate) fn cancel_request_nonce(&self) -> &str {
        // 借用 cancel nonce。
        &self.cancel_nonce
    }

    // 返回目标 request nonce。
    pub(crate) fn request_nonce(&self) -> &str {
        // 借用目标 nonce。
        &self.request_nonce
    }

    // 返回期望 epoch。
    pub(crate) fn expected_broker_epoch(&self) -> &str {
        // 借用 epoch。
        &self.expected_epoch
    }

    // 返回完全可比较的 cancel 语义键。
    pub(crate) fn canonical_semantic_key(&self) -> String {
        // 使用长度前缀避免字段歧义。
        let mut key = String::from(CONTRACT_VERSION);
        // 写入 cancel 操作。
        piece(&mut key, "cancel");
        // 写入目标 request。
        piece(&mut key, &self.request_nonce);
        // 写入 epoch。
        piece(&mut key, &self.expected_epoch);
        // 返回键。
        key
    }
}

// 解析公共 identity。
fn identity(
    object: &Map<String, Value>,
) -> Result<RequestIdentity, BrowserSessionBrokerProtocolFailure> {
    // 校验固定版本。
    if string(object, "contractVersion")? != CONTRACT_VERSION {
        // 拒绝错误版本。
        return Err(invalid_argument());
    }
    // 读取 request nonce。
    let nonce = string(object, "requestNonce")?.to_owned();
    // 读取 fingerprint。
    let fingerprint = string(object, "semanticFingerprint")?.to_owned();
    // 读取 expected epoch。
    let expected_epoch = string(object, "expectedBrokerEpoch")?.to_owned();
    // 读取 remaining 预算。
    let remaining_timeout_ms = integer32(object, "remainingTimeoutMs")?;
    // 校验 canonical 字段和范围。
    if !hex(&nonce, NONCE_LENGTH)
        || !hex(&fingerprint, FINGERPRINT_LENGTH)
        || !hex(&expected_epoch, NONCE_LENGTH)
        || !(1..=MAXIMUM_TIMEOUT_MS).contains(&remaining_timeout_ms)
    {
        // 拒绝公共字段漂移。
        return Err(invalid_argument());
    }
    // 返回 identity。
    Ok(RequestIdentity {
        nonce,
        fingerprint,
        expected_epoch,
        remaining_timeout_ms,
    })
}

// 解析 open。
fn parse_open(
    object: &Map<String, Value>,
    identity: RequestIdentity,
) -> Result<BrowserSessionBrokerRequest, BrowserSessionBrokerProtocolFailure> {
    // 确认优先。
    confirmed(object)?;
    // 拒绝 target 和 payload。
    exact(object, request_keys(&["confirmed"]))?;
    // 返回 open。
    Ok(BrowserSessionBrokerRequest::Open(identity))
}

// 解析 close。
fn parse_close(
    object: &Map<String, Value>,
    identity: RequestIdentity,
) -> Result<BrowserSessionBrokerRequest, BrowserSessionBrokerProtocolFailure> {
    // 确认优先。
    confirmed(object)?;
    // 校验封闭字段。
    exact(object, request_keys(&["confirmed", "sessionId"]))?;
    // 读取 opaque session。
    let session = opaque(object, "sessionId", SESSION_PREFIX)?;
    // 返回 close。
    Ok(BrowserSessionBrokerRequest::Close(identity, session))
}

// 解析 session.inspect。
fn parse_session_inspect(
    object: &Map<String, Value>,
    identity: RequestIdentity,
) -> Result<BrowserSessionBrokerRequest, BrowserSessionBrokerProtocolFailure> {
    // 只允许公共 envelope 与 session target，不接受 confirmed。
    exact(object, request_keys(&["sessionId"]))?;
    // 读取 canonical session identity。
    let session = opaque(object, "sessionId", SESSION_PREFIX)?;
    // 返回无副作用查询。
    Ok(BrowserSessionBrokerRequest::SessionInspect(
        identity, session,
    ))
}

// 解析 navigate。
fn parse_navigate(
    object: &Map<String, Value>,
    identity: RequestIdentity,
) -> Result<BrowserSessionBrokerRequest, BrowserSessionBrokerProtocolFailure> {
    // 确认优先。
    confirmed(object)?;
    // 校验封闭字段。
    exact(object, request_keys(&["confirmed", "sessionId", "url"]))?;
    // 读取 URL。
    let url = string(object, "url")?.to_owned();
    // 拒绝非 http/https URL。
    if !validation::http_url(&url) {
        // 拒绝 URL。
        return Err(invalid_argument());
    }
    // 读取 session。
    let session = opaque(object, "sessionId", SESSION_PREFIX)?;
    // 返回 navigate。
    Ok(BrowserSessionBrokerRequest::Navigate(
        identity, session, url,
    ))
}

// 解析 wait。
fn parse_wait(
    object: &Map<String, Value>,
    identity: RequestIdentity,
) -> Result<BrowserSessionBrokerRequest, BrowserSessionBrokerProtocolFailure> {
    // 校验封闭 query 字段。
    exact(object, request_keys(&["sessionId", "pageId", "condition"]))?;
    // 读取 session。
    let session = opaque(object, "sessionId", SESSION_PREFIX)?;
    // 读取 page。
    let page = opaque(object, "pageId", PAGE_PREFIX)?;
    // 解析 condition。
    let condition = condition(object.get("condition"))?;
    // 返回 wait。
    Ok(BrowserSessionBrokerRequest::Wait(
        identity, session, page, condition,
    ))
}

// 解析 query。
fn parse_query(
    object: &Map<String, Value>,
    identity: RequestIdentity,
) -> Result<BrowserSessionBrokerRequest, BrowserSessionBrokerProtocolFailure> {
    // 校验封闭 query 字段。
    exact(
        object,
        request_keys(&["sessionId", "pageId", "selector", "maxResults"]),
    )?;
    // 读取结果上限。
    let limit = integer16(object, "maxResults")?;
    // 拒绝非法上限。
    if !(1..=100).contains(&limit) {
        // 拒绝越界。
        return Err(invalid_argument());
    }
    // 返回 query。
    Ok(BrowserSessionBrokerRequest::Query(
        identity,
        opaque(object, "sessionId", SESSION_PREFIX)?,
        opaque(object, "pageId", PAGE_PREFIX)?,
        selector(object.get("selector"))?,
        limit,
    ))
}

// 解析 click。
fn parse_click(
    object: &Map<String, Value>,
    identity: RequestIdentity,
) -> Result<BrowserSessionBrokerRequest, BrowserSessionBrokerProtocolFailure> {
    // 确认优先。
    confirmed(object)?;
    // 校验封闭字段。
    exact(
        object,
        request_keys(&["confirmed", "sessionId", "pageId", "elementId"]),
    )?;
    // 返回 click。
    Ok(BrowserSessionBrokerRequest::Click(
        identity,
        opaque(object, "sessionId", SESSION_PREFIX)?,
        opaque(object, "pageId", PAGE_PREFIX)?,
        opaque(object, "elementId", ELEMENT_PREFIX)?,
    ))
}

// 解析 type。
fn parse_type(
    object: &Map<String, Value>,
    identity: RequestIdentity,
) -> Result<BrowserSessionBrokerRequest, BrowserSessionBrokerProtocolFailure> {
    // 确认优先。
    confirmed(object)?;
    // 校验封闭字段。
    exact(
        object,
        request_keys(&[
            "confirmed",
            "sessionId",
            "pageId",
            "elementId",
            "text",
            "replace",
        ]),
    )?;
    // 读取输入文本。
    let text = string(object, "text")?.to_owned();
    // 拒绝空或超限文本。
    if text.is_empty() || text.len() > MAXIMUM_TYPE_TEXT_BYTES {
        // 拒绝文本。
        return Err(invalid_argument());
    }
    // 返回 type。
    Ok(BrowserSessionBrokerRequest::Type(
        identity,
        opaque(object, "sessionId", SESSION_PREFIX)?,
        opaque(object, "pageId", PAGE_PREFIX)?,
        opaque(object, "elementId", ELEMENT_PREFIX)?,
        text,
        boolean(object, "replace")?,
    ))
}

// 解析 screenshot。
fn parse_screenshot(
    object: &Map<String, Value>,
    identity: RequestIdentity,
) -> Result<BrowserSessionBrokerRequest, BrowserSessionBrokerProtocolFailure> {
    // 校验封闭 query 字段。
    exact(object, request_keys(&["sessionId", "pageId"]))?;
    // 返回 screenshot。
    Ok(BrowserSessionBrokerRequest::Screenshot(
        identity,
        opaque(object, "sessionId", SESSION_PREFIX)?,
        opaque(object, "pageId", PAGE_PREFIX)?,
    ))
}

// 返回 request 公共键与操作键。
fn request_keys<'a>(extra: &'a [&'a str]) -> Vec<&'a str> {
    // 创建公共字段集合。
    let mut keys = vec![
        "kind",
        "contractVersion",
        "requestNonce",
        "semanticFingerprint",
        "expectedBrokerEpoch",
        "remainingTimeoutMs",
        "operation",
    ];
    // 追加操作特有字段。
    keys.extend_from_slice(extra);
    // 返回集合。
    keys
}

// 解析固定 operation 标签。
fn operation(
    value: &str,
) -> Result<BrowserSessionBrokerOperation, BrowserSessionBrokerProtocolFailure> {
    // 映射封闭集合。
    match value {
        // 映射 open。
        "open" => Ok(BrowserSessionBrokerOperation::Open),
        // 映射 close。
        "close" => Ok(BrowserSessionBrokerOperation::Close),
        // 映射 session.inspect。
        "session.inspect" => Ok(BrowserSessionBrokerOperation::SessionInspect),
        // 映射 navigate。
        "navigate" => Ok(BrowserSessionBrokerOperation::Navigate),
        // 映射 wait。
        "wait" => Ok(BrowserSessionBrokerOperation::Wait),
        // 映射 query。
        "query" => Ok(BrowserSessionBrokerOperation::Query),
        // 映射 click。
        "click" => Ok(BrowserSessionBrokerOperation::Click),
        // 映射 type。
        "type" => Ok(BrowserSessionBrokerOperation::Type),
        // 映射 screenshot。
        "screenshot" => Ok(BrowserSessionBrokerOperation::Screenshot),
        // 拒绝未知 operation。
        _ => Err(invalid_argument()),
    }
}

// 读取字符串。
fn string<'a>(
    object: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, BrowserSessionBrokerProtocolFailure> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(invalid_argument)
}
// 读取布尔值。
fn boolean(
    object: &Map<String, Value>,
    name: &str,
) -> Result<bool, BrowserSessionBrokerProtocolFailure> {
    object
        .get(name)
        .and_then(Value::as_bool)
        .ok_or_else(invalid_argument)
}
// 读取 u16。
fn integer16(
    object: &Map<String, Value>,
    name: &str,
) -> Result<u16, BrowserSessionBrokerProtocolFailure> {
    object
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(invalid_argument)
}
// 读取 u32。
fn integer32(
    object: &Map<String, Value>,
    name: &str,
) -> Result<u32, BrowserSessionBrokerProtocolFailure> {
    object
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(invalid_argument)
}
// 验证精确字段集合。
fn exact(
    object: &Map<String, Value>,
    keys: Vec<&str>,
) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    if object.len() != keys.len() || object.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err(invalid_argument());
    }
    Ok(())
}
// 在 command 解析的第一时间验证确认。
fn confirmed(object: &Map<String, Value>) -> Result<(), BrowserSessionBrokerProtocolFailure> {
    if object.get("confirmed").and_then(Value::as_bool) != Some(true) {
        return Err(BrowserSessionBrokerProtocolFailure::new(
            BrowserSessionBrokerProtocolErrorCode::ConfirmationRequired,
        ));
    }
    Ok(())
}
// 读取 canonical opaque ID。
fn opaque(
    object: &Map<String, Value>,
    name: &str,
    prefix: &str,
) -> Result<String, BrowserSessionBrokerProtocolFailure> {
    let value = string(object, name)?;
    if !value
        .strip_prefix(prefix)
        .is_some_and(|suffix| hex(suffix, NONCE_LENGTH))
    {
        return Err(invalid_argument());
    }
    Ok(value.to_owned())
}
// 解析 selector。
fn selector(
    value: Option<&Value>,
) -> Result<BrowserSemanticSelector, BrowserSessionBrokerProtocolFailure> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(invalid_argument)?;
    exact(object, vec!["role", "name", "text", "exact"])?;
    let role = optional_string(object, "role")?;
    let name = optional_string(object, "name")?;
    let text = optional_string(object, "text")?;
    if [role.as_deref(), name.as_deref(), text.as_deref()]
        .into_iter()
        .all(|item| item.is_none())
        || [role.as_deref(), name.as_deref(), text.as_deref()]
            .into_iter()
            .flatten()
            .any(|item| item.is_empty() || item.chars().count() > 1024)
    {
        return Err(invalid_argument());
    }
    Ok(BrowserSemanticSelector {
        role,
        name,
        text,
        exact: boolean(object, "exact")?,
    })
}
// 读取 nullable 字符串。
fn optional_string(
    object: &Map<String, Value>,
    name: &str,
) -> Result<Option<String>, BrowserSessionBrokerProtocolFailure> {
    match object.get(name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.to_owned())),
        _ => Err(invalid_argument()),
    }
}
// 解析 wait condition。
fn condition(
    value: Option<&Value>,
) -> Result<BrowserWaitCondition, BrowserSessionBrokerProtocolFailure> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(invalid_argument)?;
    match string(object, "kind")? {
        "document-ready" => {
            exact(object, vec!["kind"])?;
            Ok(BrowserWaitCondition::DocumentReady)
        }
        "element-present" => {
            exact(object, vec!["kind", "selector"])?;
            Ok(BrowserWaitCondition::ElementPresent(selector(
                object.get("selector"),
            )?))
        }
        "text-present" => {
            exact(object, vec!["kind", "text", "exact"])?;
            let text = string(object, "text")?.to_owned();
            if text.is_empty() || text.chars().count() > 1024 {
                return Err(invalid_argument());
            }
            Ok(BrowserWaitCondition::TextPresent {
                text,
                exact: boolean(object, "exact")?,
            })
        }
        _ => Err(invalid_argument()),
    }
}
// 追加长度前缀字段。
fn piece(key: &mut String, value: &str) {
    key.push('|');
    key.push_str(&value.len().to_string());
    key.push(':');
    key.push_str(value);
}
// 向 wire builder 开放同源长度前缀编码。
pub(super) fn append_canonical_piece(key: &mut String, value: &str) {
    // 委托唯一 canonical 字段实现。
    piece(key, value);
}
// 为 parser 与 wire builder 计算同源 FNV-1a 64-bit 摘要。
pub(super) fn fingerprint_for_canonical_key(key: &str) -> String {
    // 初始化 FNV offset basis。
    let mut hash = 0xcbf29ce484222325_u64;
    // 混入完整 canonical key。
    for byte in key.bytes() {
        // 执行 FNV-1a 更新。
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    // 格式化固定小写十六进制。
    format!("{hash:016x}")
}
// 追加 selector。
fn selector_key(key: &mut String, selector: &BrowserSemanticSelector) {
    // 写入 selector 域标签。
    piece(key, "selector");
    // 写入 role 的存在位和内容。
    option_key(key, selector.role());
    // 写入 name 的存在位和内容。
    option_key(key, selector.name());
    // 写入 text 的存在位和内容。
    option_key(key, selector.text());
    // 写入 exact 标记。
    piece(key, if selector.exact() { "1" } else { "0" });
}

// 以显式存在位编码可选字符串，避免 sentinel 冲突。
fn option_key(key: &mut String, value: Option<&str>) {
    // 按存在性编码。
    match value {
        // 标记缺失。
        None => piece(key, "0"),
        // 标记存在并追加内容。
        Some(value) => {
            // 写入存在标记。
            piece(key, "1");
            // 写入具体内容。
            piece(key, value);
        }
    }
}
// 追加 condition。
fn condition_key(key: &mut String, condition: &BrowserWaitCondition) {
    match condition {
        BrowserWaitCondition::DocumentReady => piece(key, "document-ready"),
        BrowserWaitCondition::ElementPresent(selector) => {
            piece(key, "element-present");
            selector_key(key, selector);
        }
        BrowserWaitCondition::TextPresent { text, exact } => {
            piece(key, "text-present");
            piece(key, text);
            piece(key, if *exact { "1" } else { "0" });
        }
    }
}
// 判断 canonical 小写 hex。
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
// 构造常规输入错误。
pub(crate) const fn invalid_argument() -> BrowserSessionBrokerProtocolFailure {
    BrowserSessionBrokerProtocolFailure::new(BrowserSessionBrokerProtocolErrorCode::InvalidArgument)
}
// 在 JSON 解析前实施固定输入字节门禁并统一 BOM/外围空白。
fn bounded_input(text: &str) -> Result<&str, BrowserSessionBrokerProtocolFailure> {
    if text.len() > MAXIMUM_INPUT_FRAME_BYTES {
        return Err(invalid_argument());
    }
    Ok(text.trim_start_matches('\u{feff}').trim())
}

// 加载定向回归测试。
#[cfg(test)]
#[path = "browser_session_broker_protocol_tests.rs"]
mod tests;

// 加载 session.inspect 专属协议回归，避免通用测试文件越过行数门禁。
#[cfg(test)]
#[path = "browser_session_broker_session_inspect_tests.rs"]
mod session_inspect_tests;
