//! 投影 Windows browser-session Broker 的页面导航与只读查询结果。

// 导入 provider-neutral JSON 值。
use serde_json::Value;

// 导入 broker selector、等待条件与严格成功投影。
use crate::components::browser_session_broker_protocol::{
    // 导入 provider-neutral selector。
    BrowserSemanticSelector,
    // 导入有限等待条件。
    BrowserWaitCondition,
    // 导入 final outcome 与成功数据。
    response::{BrowserSessionBrokerOutcome, BrowserSessionBrokerSuccess},
};
// 导入统一错误与结果类型。
use crate::domain::{AppControlError, AppResult};

// 导入父 Adapter 的认证 exchange 与安全错误投影。
use super::{BrowserSessionExchange, exchange_request, final_error};

// 保存可信 completed 导航的 provider-neutral 结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionNavigation {
    // 保存 Module 签发的公开 page identity。
    page_id: String,
    // 保存从一开始的导航代际。
    generation: u32,
}

// 保存可信 completed 页面 Query 的当前页面事实。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionPageObservation {
    // 保存 Module 已验证的当前公开 page identity。
    page_id: String,
    // 保存 Module 报告的正导航代际。
    generation: u32,
}

// 保存可信 completed 元素动作的 request-bound 结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionElementAction {
    // 保存当前公开 page identity。
    page_id: String,
    // 保存当前公开 element identity。
    element_id: String,
    // 保存正导航代际。
    generation: u32,
}

// 保存可信 completed 文本输入结果，不保留原始文本。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionTypeAction {
    // 保存 request-bound 元素动作事实。
    action: BrowserSessionElementAction,
    // 保存已输入的 UTF-8 字节数。
    utf8_bytes: u16,
}

// 保存可信 completed 页面的有界 PNG 投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionPageScreenshot {
    // 保存当前公开 page identity。
    page_id: String,
    // 保存正导航代际。
    generation: u32,
    // 保存固定 PNG MIME。
    mime_type: String,
    // 保存有界 PNG Base64。
    png_base64: String,
    // 保存原始 PNG 字节数。
    png_bytes: u64,
    // 保存 IHDR 宽度。
    width: u32,
    // 保存 IHDR 高度。
    height: u32,
    // 保存稳定摘要。
    digest: String,
}

// 为元素动作提供只读投影。
impl BrowserSessionElementAction {
    // 返回当前公开 page identity。
    pub(crate) fn page_id(&self) -> &str {
        // 借用公开 page。
        &self.page_id
    }

    // 返回当前公开 element identity。
    pub(crate) fn element_id(&self) -> &str {
        // 借用公开 element。
        &self.element_id
    }

    // 返回正导航代际。
    pub(crate) const fn generation(&self) -> u32 {
        // 复制代际。
        self.generation
    }
}

// 为文本输入结果提供只读投影。
impl BrowserSessionTypeAction {
    // 返回 request-bound 元素动作事实。
    pub(crate) const fn action(&self) -> &BrowserSessionElementAction {
        // 借用嵌套动作。
        &self.action
    }

    // 返回已输入的 UTF-8 字节数。
    pub(crate) const fn utf8_bytes(&self) -> u16 {
        // 复制字节数。
        self.utf8_bytes
    }
}

// 为页面截图提供只读投影。
impl BrowserSessionPageScreenshot {
    // 返回当前公开 page identity。
    pub(crate) fn page_id(&self) -> &str {
        // 借用公开 page。
        &self.page_id
    }

    // 返回正导航代际。
    pub(crate) const fn generation(&self) -> u32 {
        // 复制代际。
        self.generation
    }

    // 返回固定 PNG MIME。
    pub(crate) fn mime_type(&self) -> &str {
        // 借用 MIME。
        &self.mime_type
    }

    // 返回有界 PNG Base64。
    pub(crate) fn png_base64(&self) -> &str {
        // 借用 Base64。
        &self.png_base64
    }

    // 返回原始 PNG 字节数。
    pub(crate) const fn png_bytes(&self) -> u64 {
        // 复制字节数。
        self.png_bytes
    }

    // 返回 IHDR 宽度。
    pub(crate) const fn width(&self) -> u32 {
        // 复制宽度。
        self.width
    }

    // 返回 IHDR 高度。
    pub(crate) const fn height(&self) -> u32 {
        // 复制高度。
        self.height
    }

    // 返回稳定摘要。
    pub(crate) fn digest(&self) -> &str {
        // 借用摘要。
        &self.digest
    }
}

// 为页面观察提供只读投影。
impl BrowserSessionPageObservation {
    // 返回当前公开 page identity。
    pub(crate) fn page_id(&self) -> &str {
        // 借用公开 identity。
        &self.page_id
    }

    // 返回正导航代际。
    pub(crate) const fn generation(&self) -> u32 {
        // 复制代际。
        self.generation
    }
}

// 为导航结果提供只读投影。
impl BrowserSessionNavigation {
    // 返回公开 page identity。
    pub(crate) fn page_id(&self) -> &str {
        // 借用公开 identity。
        &self.page_id
    }

    // 返回正导航代际。
    pub(crate) const fn generation(&self) -> u32 {
        // 复制代际。
        self.generation
    }
}

// 导航 live session 并只在可信 completed final 后返回新 page。
pub(crate) fn navigate_confirmed(
    // 借用公开 session identity。
    session_id: &str,
    // 借用 parser 将再次验证的 HTTP(S) URL。
    url: &str,
    // 接收完整调用预算。
    timeout_ms: u32,
) -> AppResult<BrowserSessionNavigation> {
    // 执行字段封闭且 confirmed 的 navigate exchange。
    let response = exchange_request(
        // 只借用公开 session 与 URL。
        BrowserSessionExchange::NavigateConfirmed { session_id, url },
        // 传递覆盖完整物理交换的总预算。
        timeout_ms,
    )?;
    // 只接受 completed navigate 成功投影。
    match (response.outcome(), response.success()) {
        // strict decoder 已验证 page identity 与正代际。
        (
            // final 必须可信完成。
            Some(BrowserSessionBrokerOutcome::Completed),
            // data 必须是 navigate 专属投影。
            Some(BrowserSessionBrokerSuccess::Navigate {
                // 借用公开 page identity。
                page_id,
                // 借用正导航代际。
                generation,
            }),
        ) => Ok(BrowserSessionNavigation {
            // 复制公开 page identity。
            page_id: page_id.to_owned(),
            // 复制正导航代际。
            generation: *generation,
        }),
        // 其余 final 都投影为冻结安全错误。
        _ => Err(final_error(&response)),
    }
}

// 等待当前 page 满足有限条件并只接受 conditionMet=true。
pub(crate) fn wait(
    // 借用公开 session identity。
    session_id: &str,
    // 借用当前公开 page identity。
    page_id: &str,
    // 借用有限等待条件。
    condition: &BrowserWaitCondition,
    // 接收完整调用预算。
    timeout_ms: u32,
) -> AppResult<BrowserSessionPageObservation> {
    // 执行不带 confirmed 的 wait Query exchange。
    let response = exchange_request(
        // 只借用公开 target 与 provider-neutral 条件。
        BrowserSessionExchange::Wait {
            // 绑定公开 session。
            session_id,
            // 绑定当前 page。
            page_id,
            // 绑定有限条件。
            condition,
        },
        // 传递覆盖完整物理交换的总预算。
        timeout_ms,
    )?;
    // 只接受 completed wait 成功投影。
    match (response.outcome(), response.success()) {
        // strict decoder 已验证 conditionMet=true。
        (
            // final 必须可信完成。
            Some(BrowserSessionBrokerOutcome::Completed),
            // data 必须是 wait 专属投影。
            Some(BrowserSessionBrokerSuccess::Wait {
                // 借用当前公开页面 identity。
                page_id,
                // 借用正导航代际。
                generation,
            }),
        ) => Ok(BrowserSessionPageObservation {
            // 复制公开 page identity。
            page_id: page_id.to_owned(),
            // 复制正代际。
            generation: *generation,
        }),
        // 其余 final 都投影为冻结安全错误。
        _ => Err(final_error(&response)),
    }
}

// 查询当前 page 并返回已经 request-bound 验证的元素投影对象。
pub(crate) fn query(
    // 借用公开 session identity。
    session_id: &str,
    // 借用当前公开 page identity。
    page_id: &str,
    // 借用 provider-neutral selector。
    selector: &BrowserSemanticSelector,
    // 接收有界结果上限。
    max_results: u16,
    // 接收完整调用预算。
    timeout_ms: u32,
) -> AppResult<Value> {
    // 执行不带 confirmed 的 query Query exchange。
    let response = exchange_request(
        // 只借用公开 target 与 provider-neutral selector。
        BrowserSessionExchange::Query {
            // 绑定公开 session。
            session_id,
            // 绑定当前 page。
            page_id,
            // 绑定 selector。
            selector,
            // 绑定结果上限。
            max_results,
        },
        // 传递覆盖完整物理交换的总预算。
        timeout_ms,
    )?;
    // 只接受 completed query 成功投影。
    match (response.outcome(), response.success()) {
        // strict decoder 已按原 maxResults 验证完整查询对象。
        (
            // final 必须可信完成。
            Some(BrowserSessionBrokerOutcome::Completed),
            // data 必须是 query 专属投影。
            Some(BrowserSessionBrokerSuccess::Query(data)),
        ) => Ok(data.clone()),
        // 其余 final 都投影为冻结安全错误。
        _ => Err(final_error(&response)),
    }
}

// 点击当前元素并只接受 request-bound completed 成功。
pub(crate) fn click_confirmed(
    // 借用公开 session identity。
    session_id: &str,
    // 借用当前公开 page identity。
    page_id: &str,
    // 借用当前公开 element identity。
    element_id: &str,
    // 接收完整调用预算。
    timeout_ms: u32,
) -> AppResult<BrowserSessionElementAction> {
    // 执行 confirmation-first click exchange。
    let response = exchange_request(
        // 只借用公开三级 target。
        BrowserSessionExchange::ClickConfirmed {
            // 绑定公开 session。
            session_id,
            // 绑定当前 page。
            page_id,
            // 绑定当前 element。
            element_id,
        },
        // 传递覆盖完整物理交换的总预算。
        timeout_ms,
    )?;
    // 只接受 completed click 专属投影。
    match (response.outcome(), response.success()) {
        // strict decoder 已验证 request-bound identity、clicked 与正代际。
        (
            // final 必须可信完成。
            Some(BrowserSessionBrokerOutcome::Completed),
            // data 必须是 click 专属投影。
            Some(BrowserSessionBrokerSuccess::Click {
                // 借用当前 page。
                page_id,
                // 借用当前 element。
                element_id,
                // 借用正导航代际。
                generation,
            }),
        ) => Ok(BrowserSessionElementAction {
            // 复制公开 page。
            page_id: page_id.to_owned(),
            // 复制公开 element。
            element_id: element_id.to_owned(),
            // 复制正代际。
            generation: *generation,
        }),
        // 其余 final 都投影为冻结安全错误。
        _ => Err(final_error(&response)),
    }
}

// 向当前元素输入文本并只接受不回显原文的 completed 成功。
#[allow(clippy::too_many_arguments)]
pub(crate) fn type_confirmed(
    // 借用公开 session identity。
    session_id: &str,
    // 借用当前公开 page identity。
    page_id: &str,
    // 借用当前公开 element identity。
    element_id: &str,
    // 借用有界 UTF-8 文本。
    text: &str,
    // 接收显式替换语义。
    replace: bool,
    // 接收完整调用预算。
    timeout_ms: u32,
) -> AppResult<BrowserSessionTypeAction> {
    // 执行 confirmation-first type exchange。
    let response = exchange_request(
        // 只借用公开 target、文本与替换语义。
        BrowserSessionExchange::TypeConfirmed {
            // 绑定公开 session。
            session_id,
            // 绑定当前 page。
            page_id,
            // 绑定当前 element。
            element_id,
            // 绑定完整文本但不保存在结果。
            text,
            // 保留显式替换语义。
            replace,
        },
        // 传递覆盖完整物理交换的总预算。
        timeout_ms,
    )?;
    // 只接受 completed type 专属投影。
    match (response.outcome(), response.success()) {
        // strict decoder 已验证 request-bound identity、typed、字节数与代际。
        (
            // final 必须可信完成。
            Some(BrowserSessionBrokerOutcome::Completed),
            // data 必须是 type 专属投影。
            Some(BrowserSessionBrokerSuccess::Type {
                // 借用当前 page。
                page_id,
                // 借用当前 element。
                element_id,
                // 借用正导航代际。
                generation,
                // 借用有界字节数。
                utf8_bytes,
            }),
        ) => Ok(BrowserSessionTypeAction {
            // 保存不含原始文本的动作事实。
            action: BrowserSessionElementAction {
                // 复制公开 page。
                page_id: page_id.to_owned(),
                // 复制公开 element。
                element_id: element_id.to_owned(),
                // 复制正代际。
                generation: *generation,
            },
            // 复制 UTF-8 字节数。
            utf8_bytes: *utf8_bytes,
        }),
        // 其余 final 都投影为冻结安全错误。
        _ => Err(final_error(&response)),
    }
}

// 捕获当前页面并只接受有界 request-bound PNG。
pub(crate) fn screenshot(
    // 借用公开 session identity。
    session_id: &str,
    // 借用当前公开 page identity。
    page_id: &str,
    // 接收完整调用预算。
    timeout_ms: u32,
) -> AppResult<BrowserSessionPageScreenshot> {
    // 执行无确认的 screenshot Query exchange。
    let response = exchange_request(
        // 只借用公开 session/page target。
        BrowserSessionExchange::Screenshot {
            // 绑定公开 session。
            session_id,
            // 绑定当前 page。
            page_id,
        },
        // 传递覆盖完整物理交换的总预算。
        timeout_ms,
    )?;
    // 只接受 completed screenshot 专属投影。
    match (response.outcome(), response.success()) {
        // strict decoder 已验证 PNG 对象边界与 request binding。
        (
            // final 必须可信完成。
            Some(BrowserSessionBrokerOutcome::Completed),
            // data 必须是 screenshot 专属投影。
            Some(BrowserSessionBrokerSuccess::Screenshot(data)),
        ) => screenshot_result(data),
        // 其余 final 都投影为冻结安全错误。
        _ => Err(final_error(&response)),
    }
}

// 将 strict decoder 验证过的截图对象转为窄 Rust 结果。
fn screenshot_result(data: &Value) -> AppResult<BrowserSessionPageScreenshot> {
    // 读取已验证的截图对象。
    let object = data.as_object().ok_or_else(invalid_success)?;
    // 构造不含 provider 私有事实的窄结果。
    Ok(BrowserSessionPageScreenshot {
        // 复制公开 page identity。
        page_id: string_field(object, "pageId")?.to_owned(),
        // 无损收窄正导航代际。
        generation: u32_field(object, "navigationGeneration")?,
        // 复制固定 PNG MIME。
        mime_type: string_field(object, "mimeType")?.to_owned(),
        // 复制有界 PNG Base64。
        png_base64: string_field(object, "pngBase64")?.to_owned(),
        // 复制原始 PNG 字节数。
        png_bytes: u64_field(object, "pngBytes")?,
        // 无损收窄 IHDR 宽度。
        width: u32_field(object, "width")?,
        // 无损收窄 IHDR 高度。
        height: u32_field(object, "height")?,
        // 复制稳定摘要。
        digest: string_field(object, "digest")?.to_owned(),
    })
}

// 从截图对象读取字符串字段。
fn string_field<'a>(
    // 借用 strict decoder 验证过的对象。
    object: &'a serde_json::Map<String, Value>,
    // 接收固定字段名。
    key: &str,
) -> AppResult<&'a str> {
    // 缺失或类型漂移时关闭 broker 结果。
    object
        // 读取固定字段。
        .get(key)
        // 只接受字符串。
        .and_then(Value::as_str)
        // 不泄漏内部值。
        .ok_or_else(invalid_success)
}

// 从截图对象读取 u64 字段。
fn u64_field(
    // 借用 strict decoder 验证过的对象。
    object: &serde_json::Map<String, Value>,
    // 接收固定字段名。
    key: &str,
) -> AppResult<u64> {
    // 缺失或类型漂移时关闭 broker 结果。
    object
        // 读取固定字段。
        .get(key)
        // 只接受非负整数。
        .and_then(Value::as_u64)
        // 不泄漏内部值。
        .ok_or_else(invalid_success)
}

// 从截图对象无损收窄 u32 字段。
fn u32_field(
    // 借用 strict decoder 验证过的对象。
    object: &serde_json::Map<String, Value>,
    // 接收固定字段名。
    key: &str,
) -> AppResult<u32> {
    // 先读取 u64，再拒绝溢出。
    u32::try_from(u64_field(object, key)?)
        // 不泄漏内部值。
        .map_err(|_| invalid_success())
}

// 构造 strict success 内部漂移的安全错误。
fn invalid_success() -> AppControlError {
    // 返回不含 PNG、target 或 native 事实的固定错误。
    AppControlError::new(
        // 使用稳定 broker 不可用错误码。
        "BROKER_UNAVAILABLE",
        // 固定说明 strict final 无法安全投影。
        "The browser session broker returned an invalid final result.",
    )
}
