//! 严格解析浏览器页面导航、查询、元素动作与截图的公开 provider-neutral 输入。

// 导入 JSON 对象和值类型。
use serde_json::{Map, Value};

// 导入 broker 同源的公开类型与验证器。
use crate::components::browser_session_broker_protocol::{
    // 构造 provider-neutral selector。
    BrowserSemanticSelector,
    // 构造有限等待条件。
    BrowserWaitCondition,
    // 验证 canonical 公开元素 identity。
    canonical_element_id,
    // 验证 canonical 公开页面 identity。
    canonical_page_id,
    // 验证完整 HTTP(S) URL 边界。
    public_http_url,
};
// 导入统一公开错误与结果边界。
use crate::domain::{AppControlError, AppResult};

// 固定三项页面操作共享的默认总预算。
pub(crate) const DEFAULT_TIMEOUT_MS: u32 = 5_000;
// 固定页面操作共享的最大总预算。
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
// 固定 selector 与文本等待的字符上限。
const MAXIMUM_SEMANTIC_TEXT_CHARACTERS: usize = 1_024;
// 固定 query 默认结果上限。
const DEFAULT_MAX_RESULTS: u16 = 100;
// 固定页面文本输入的 UTF-8 字节上限。
const MAXIMUM_TYPE_TEXT_BYTES: usize = 16 * 1024;

// 保存已经完整验证的页面导航输入。
pub(crate) struct BrowserPageNavigateInput {
    // 保存不会被公开回显的目标 URL。
    url: String,
    // 保存覆盖完整调用链的总预算。
    timeout_ms: u32,
}

// 为导航输入提供窄只读投影。
impl BrowserPageNavigateInput {
    // 返回已验证 URL。
    pub(crate) fn url(&self) -> &str {
        // 借用 Component 自有字符串。
        &self.url
    }

    // 返回总预算。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        // 复制有界整数。
        self.timeout_ms
    }
}

// 保存已经完整验证的页面等待输入。
pub(crate) struct BrowserPageWaitInput {
    // 保存当前公开页面 identity。
    page_id: String,
    // 保存有限 provider-neutral 条件。
    condition: BrowserWaitCondition,
    // 保存覆盖完整调用链的总预算。
    timeout_ms: u32,
}

// 为等待输入提供窄只读投影。
impl BrowserPageWaitInput {
    // 返回当前公开页面 identity。
    pub(crate) fn page_id(&self) -> &str {
        // 借用 Component 自有字符串。
        &self.page_id
    }

    // 返回有限等待条件。
    pub(crate) const fn condition(&self) -> &BrowserWaitCondition {
        // 借用强类型条件。
        &self.condition
    }

    // 返回总预算。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        // 复制有界整数。
        self.timeout_ms
    }
}

// 保存已经完整验证的页面查询输入。
pub(crate) struct BrowserPageQueryInput {
    // 保存当前公开页面 identity。
    page_id: String,
    // 保存 provider-neutral selector。
    selector: BrowserSemanticSelector,
    // 保存单次公开结果上限。
    max_results: u16,
    // 保存覆盖完整调用链的总预算。
    timeout_ms: u32,
}

// 为查询输入提供窄只读投影。
impl BrowserPageQueryInput {
    // 返回当前公开页面 identity。
    pub(crate) fn page_id(&self) -> &str {
        // 借用 Component 自有字符串。
        &self.page_id
    }

    // 返回 provider-neutral selector。
    pub(crate) const fn selector(&self) -> &BrowserSemanticSelector {
        // 借用强类型 selector。
        &self.selector
    }

    // 返回有界结果上限。
    pub(crate) const fn max_results(&self) -> u16 {
        // 复制小整数。
        self.max_results
    }

    // 返回总预算。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        // 复制有界整数。
        self.timeout_ms
    }
}

// 保存已经完整验证的元素点击输入。
pub(crate) struct BrowserElementClickInput {
    // 保存当前公开页面 identity。
    page_id: String,
    // 保存当前公开元素 identity。
    element_id: String,
    // 保存覆盖完整调用链的总预算。
    timeout_ms: u32,
}

// 为元素点击输入提供窄只读投影。
impl BrowserElementClickInput {
    // 返回当前公开页面 identity。
    pub(crate) fn page_id(&self) -> &str {
        // 借用 Component 自有字符串。
        &self.page_id
    }

    // 返回当前公开元素 identity。
    pub(crate) fn element_id(&self) -> &str {
        // 借用 Component 自有字符串。
        &self.element_id
    }

    // 返回总预算。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        // 复制有界整数。
        self.timeout_ms
    }
}

// 保存已经完整验证且不对外回显原文的元素文本输入。
pub(crate) struct BrowserElementTypeInput {
    // 保存当前公开页面 identity。
    page_id: String,
    // 保存当前公开元素 identity。
    element_id: String,
    // 保存有界 UTF-8 文本供唯一 Broker 请求使用。
    text: String,
    // 保存调用方显式选择的替换语义。
    replace: bool,
    // 保存覆盖完整调用链的总预算。
    timeout_ms: u32,
}

// 为元素文本输入提供窄只读投影。
impl BrowserElementTypeInput {
    // 返回当前公开页面 identity。
    pub(crate) fn page_id(&self) -> &str {
        // 借用 Component 自有字符串。
        &self.page_id
    }

    // 返回当前公开元素 identity。
    pub(crate) fn element_id(&self) -> &str {
        // 借用 Component 自有字符串。
        &self.element_id
    }

    // 返回不应进入公开结果的输入文本。
    pub(crate) fn text(&self) -> &str {
        // 只在 Module 到固定 Broker 的调用期间借用。
        &self.text
    }

    // 返回显式替换语义。
    pub(crate) const fn replace(&self) -> bool {
        // 复制布尔值。
        self.replace
    }

    // 返回总预算。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        // 复制有界整数。
        self.timeout_ms
    }
}

// 保存已经完整验证的页面截图输入。
pub(crate) struct BrowserPageScreenshotInput {
    // 保存当前公开页面 identity。
    page_id: String,
    // 保存覆盖完整调用链的总预算。
    timeout_ms: u32,
}

// 为页面截图输入提供窄只读投影。
impl BrowserPageScreenshotInput {
    // 返回当前公开页面 identity。
    pub(crate) fn page_id(&self) -> &str {
        // 借用 Component 自有字符串。
        &self.page_id
    }

    // 返回总预算。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        // 复制有界整数。
        self.timeout_ms
    }
}

// 构造不回显调用方输入的统一参数错误。
fn invalid_input() -> AppControlError {
    // 只使用公开契约已经登记的错误码与安全说明。
    AppControlError::new("INVALID_ARGUMENT", "The browser page input is invalid.")
}

// 取得唯一公开 input 对象。
fn required_object(input: Option<&Value>) -> AppResult<&Map<String, Value>> {
    // input 缺失、null 或非对象都不得使用默认空对象。
    input
        // 只接受 JSON object。
        .and_then(Value::as_object)
        // 返回不回显内容的统一错误。
        .ok_or_else(invalid_input)
}

// 核对对象字段集合是否严格封闭。
fn exact_keys(object: &Map<String, Value>, required: &[&str], optional: &[&str]) -> bool {
    // 所有必填字段都必须存在。
    required.iter().all(|name| object.contains_key(*name))
        // 实际字段只允许来自两个声明集合。
        && object
            // 遍历调用方字段名但不读取未知值。
            .keys()
            // 每个名称都必须已公开。
            .all(|name| required.contains(&name.as_str()) || optional.contains(&name.as_str()))
}

// 解析共享总预算并应用默认值。
fn timeout_ms(object: &Map<String, Value>) -> AppResult<u32> {
    // 缺失时使用冻结默认值。
    let Some(value) = object.get("timeoutMs") else {
        // 返回固定五秒总预算。
        return Ok(DEFAULT_TIMEOUT_MS);
    };
    // 只接受可无损收窄的非负 JSON 整数。
    let value = value
        // 拒绝浮点数和负数。
        .as_u64()
        // 无损收窄为公开 u32 范围。
        .and_then(|value| u32::try_from(value).ok())
        // 类型或范围错误统一拒绝。
        .ok_or_else(invalid_input)?;
    // 总预算必须落在冻结闭区间。
    if !(1..=MAXIMUM_TIMEOUT_MS).contains(&value) {
        // 零值和超限值失败闭合。
        return Err(invalid_input());
    }
    // 返回已验证预算。
    Ok(value)
}

// 解析可选有界语义字符串。
fn optional_semantic_text(object: &Map<String, Value>, name: &str) -> AppResult<Option<String>> {
    // 缺失字段表示未指定该语义维度。
    let Some(value) = object.get(name) else {
        // 返回显式 None。
        return Ok(None);
    };
    // 显式字段只能是字符串。
    let text = value.as_str().ok_or_else(invalid_input)?;
    // 空值和超限值都不构成合法 selector。
    if text.is_empty() || text.chars().count() > MAXIMUM_SEMANTIC_TEXT_CHARACTERS {
        // 不回显敏感 selector 文本。
        return Err(invalid_input());
    }
    // 复制为强类型 selector 所有权。
    Ok(Some(text.to_owned()))
}

// 解析 provider-neutral selector 并拒绝私有查询语言。
fn selector(value: Option<&Value>) -> AppResult<BrowserSemanticSelector> {
    // selector 必须是对象。
    let object = value
        // 只接受 JSON object。
        .and_then(Value::as_object)
        // 其他形状统一拒绝。
        .ok_or_else(invalid_input)?;
    // 只允许三个语义字符串与 exact。
    if !exact_keys(object, &[], &["role", "name", "text", "exact"]) {
        // CSS、XPath、脚本和其他字段失败闭合。
        return Err(invalid_input());
    }
    // 分别解析三个可选语义字段。
    let role = optional_semantic_text(object, "role")?;
    // 解析可访问名称。
    let name = optional_semantic_text(object, "name")?;
    // 解析可见文本。
    let text = optional_semantic_text(object, "text")?;
    // 至少一个语义维度必须存在。
    if role.is_none() && name.is_none() && text.is_none() {
        // 空 selector 不得扩张为任意元素。
        return Err(invalid_input());
    }
    // exact 缺省 false，显式值必须为布尔。
    let exact = match object.get("exact") {
        // 缺失使用公开默认值。
        None => false,
        // 显式值只接受布尔。
        Some(value) => value.as_bool().ok_or_else(invalid_input)?,
    };
    // 构造不含 provider/native 查询语言的 selector。
    Ok(BrowserSemanticSelector::new(role, name, text, exact))
}

// 解析有限 wait 条件。
fn wait_condition(value: Option<&Value>) -> AppResult<BrowserWaitCondition> {
    // condition 必须是对象。
    let object = value
        // 只接受 JSON object。
        .and_then(Value::as_object)
        // 其他形状统一拒绝。
        .ok_or_else(invalid_input)?;
    // 按封闭 kind 解析精确字段集合。
    match object.get("kind").and_then(Value::as_str) {
        // document-ready 不接受其他 payload。
        Some("document-ready") if exact_keys(object, &["kind"], &[]) => {
            // 返回无 payload 条件。
            Ok(BrowserWaitCondition::DocumentReady)
        }
        // element-present 只接受 provider-neutral selector。
        Some("element-present") if exact_keys(object, &["kind", "selector"], &[]) => {
            // 解析并包装强类型 selector。
            Ok(BrowserWaitCondition::ElementPresent(selector(
                // 传递唯一 selector 字段。
                object.get("selector"),
            )?))
        }
        // text-present 接受有界文本与可选 exact。
        Some("text-present") if exact_keys(object, &["kind", "text"], &["exact"]) => {
            // 读取必填文本。
            let text = object
                // 读取固定字段。
                .get("text")
                // 只接受字符串。
                .and_then(Value::as_str)
                // 非字符串统一拒绝。
                .ok_or_else(invalid_input)?;
            // 文本必须非空且有界。
            if text.is_empty() || text.chars().count() > MAXIMUM_SEMANTIC_TEXT_CHARACTERS {
                // 不回显调用文本。
                return Err(invalid_input());
            }
            // exact 缺省 false，显式值必须是布尔。
            let exact = match object.get("exact") {
                // 缺失使用公开默认值。
                None => false,
                // 显式值只接受布尔。
                Some(value) => value.as_bool().ok_or_else(invalid_input)?,
            };
            // 构造有限文本条件。
            Ok(BrowserWaitCondition::TextPresent {
                // 复制有界文本。
                text: text.to_owned(),
                // 保存匹配方式。
                exact,
            })
        }
        // 未知 kind 或字段漂移统一拒绝。
        _ => Err(invalid_input()),
    }
}

// 解析页面导航公开 input。
pub(crate) fn parse_navigate(input: Option<&Value>) -> AppResult<BrowserPageNavigateInput> {
    // 取得严格对象。
    let object = required_object(input)?;
    // 只允许 url 与可选总预算。
    if !exact_keys(object, &["url"], &["timeoutMs"]) {
        // 任意附加字段失败闭合。
        return Err(invalid_input());
    }
    // URL 必须是字符串。
    let url = object
        // 读取必填字段。
        .get("url")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 其他形状统一拒绝。
        .ok_or_else(invalid_input)?;
    // 使用 broker parser 同源完整 URL 验证。
    if !public_http_url(url) {
        // 不回显调用 URL。
        return Err(invalid_input());
    }
    // 构造完整强类型输入。
    Ok(BrowserPageNavigateInput {
        // 复制调用 URL 供唯一 Broker 请求使用。
        url: url.to_owned(),
        // 应用共享总预算边界。
        timeout_ms: timeout_ms(object)?,
    })
}

// 解析页面等待公开 input。
pub(crate) fn parse_wait(input: Option<&Value>) -> AppResult<BrowserPageWaitInput> {
    // 取得严格对象。
    let object = required_object(input)?;
    // 只允许 page、condition 与可选总预算。
    if !exact_keys(object, &["pageId", "condition"], &["timeoutMs"]) {
        // 任意附加字段失败闭合。
        return Err(invalid_input());
    }
    // 页面 identity 必须是字符串。
    let page_id = object
        // 读取必填页面字段。
        .get("pageId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 其他形状统一拒绝。
        .ok_or_else(invalid_input)?;
    // 只接受 canonical 当前页面外壳。
    if !canonical_page_id(page_id) {
        // 大写、短值或其他 kind 失败闭合。
        return Err(invalid_input());
    }
    // 构造完整强类型输入。
    Ok(BrowserPageWaitInput {
        // 复制公开页面 identity。
        page_id: page_id.to_owned(),
        // 解析有限条件。
        condition: wait_condition(object.get("condition"))?,
        // 应用共享总预算边界。
        timeout_ms: timeout_ms(object)?,
    })
}

// 解析页面查询公开 input。
pub(crate) fn parse_query(input: Option<&Value>) -> AppResult<BrowserPageQueryInput> {
    // 取得严格对象。
    let object = required_object(input)?;
    // 只允许页面、selector、结果上限与总预算。
    if !exact_keys(
        // 传递调用对象。
        object,
        // 页面和 selector 必填。
        &["pageId", "selector"],
        // 两个有界整数可选。
        &["maxResults", "timeoutMs"],
    ) {
        // 任意附加字段失败闭合。
        return Err(invalid_input());
    }
    // 页面 identity 必须是字符串。
    let page_id = object
        // 读取必填页面字段。
        .get("pageId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 其他形状统一拒绝。
        .ok_or_else(invalid_input)?;
    // 只接受 canonical 当前页面外壳。
    if !canonical_page_id(page_id) {
        // 大写、短值或其他 kind 失败闭合。
        return Err(invalid_input());
    }
    // maxResults 缺省使用冻结上限。
    let max_results = match object.get("maxResults") {
        // 缺失使用默认一百项。
        None => DEFAULT_MAX_RESULTS,
        // 显式值必须可无损收窄为 u16。
        Some(value) => value
            // 只接受非负整数。
            .as_u64()
            // 无损收窄。
            .and_then(|value| u16::try_from(value).ok())
            // 非整数或过大值统一拒绝。
            .ok_or_else(invalid_input)?,
    };
    // 结果上限必须为 1..100。
    if !(1..=DEFAULT_MAX_RESULTS).contains(&max_results) {
        // 零或超限失败闭合。
        return Err(invalid_input());
    }
    // 构造完整强类型输入。
    Ok(BrowserPageQueryInput {
        // 复制公开页面 identity。
        page_id: page_id.to_owned(),
        // 解析 provider-neutral selector。
        selector: selector(object.get("selector"))?,
        // 保存有界结果上限。
        max_results,
        // 应用共享总预算边界。
        timeout_ms: timeout_ms(object)?,
    })
}

// 从严格对象读取并验证 canonical 页面 identity。
fn page_id(object: &Map<String, Value>) -> AppResult<String> {
    // 页面 identity 必须是字符串。
    let page_id = object
        // 读取固定页面字段。
        .get("pageId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 其他形状统一拒绝。
        .ok_or_else(invalid_input)?;
    // 只接受 canonical 当前页面外壳。
    if !canonical_page_id(page_id) {
        // 大写、短值或其他 kind 失败闭合。
        return Err(invalid_input());
    }
    // 复制经过验证的公开 identity。
    Ok(page_id.to_owned())
}

// 从严格对象读取并验证 canonical 元素 identity。
fn element_id(object: &Map<String, Value>) -> AppResult<String> {
    // 元素 identity 必须是字符串。
    let element_id = object
        // 读取固定元素字段。
        .get("elementId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 其他形状统一拒绝。
        .ok_or_else(invalid_input)?;
    // 只接受 canonical 当前元素外壳。
    if !canonical_element_id(element_id) {
        // 大写、短值或其他 kind 失败闭合。
        return Err(invalid_input());
    }
    // 复制经过验证的公开 identity。
    Ok(element_id.to_owned())
}

// 解析元素点击公开 input。
pub(crate) fn parse_click(input: Option<&Value>) -> AppResult<BrowserElementClickInput> {
    // 取得严格对象。
    let object = required_object(input)?;
    // 只允许页面、元素与可选总预算。
    if !exact_keys(object, &["pageId", "elementId"], &["timeoutMs"]) {
        // 任意附加字段失败闭合。
        return Err(invalid_input());
    }
    // 构造完整强类型输入。
    Ok(BrowserElementClickInput {
        // 解析当前公开页面 identity。
        page_id: page_id(object)?,
        // 解析当前公开元素 identity。
        element_id: element_id(object)?,
        // 应用共享总预算边界。
        timeout_ms: timeout_ms(object)?,
    })
}

// 解析元素文本输入公开 input。
pub(crate) fn parse_type(input: Option<&Value>) -> AppResult<BrowserElementTypeInput> {
    // 取得严格对象。
    let object = required_object(input)?;
    // 只允许页面、元素、文本、显式替换语义与可选总预算。
    if !exact_keys(
        // 传递调用对象。
        object,
        // 四个业务字段全部必填。
        &["pageId", "elementId", "text", "replace"],
        // 只有总预算可选。
        &["timeoutMs"],
    ) {
        // 任意附加字段失败闭合。
        return Err(invalid_input());
    }
    // 文本必须是字符串。
    let text = object
        // 读取唯一敏感输入字段。
        .get("text")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 其他形状统一拒绝。
        .ok_or_else(invalid_input)?;
    // 使用 Rust 字符串字节长度权威执行 UTF-8 门禁。
    if text.is_empty() || text.len() > MAXIMUM_TYPE_TEXT_BYTES {
        // 不回显全部或部分文本。
        return Err(invalid_input());
    }
    // replace 必须由调用方显式提供布尔值。
    let replace = object
        // 读取固定替换字段。
        .get("replace")
        // 只接受布尔值。
        .and_then(Value::as_bool)
        // 缺失或错误类型统一拒绝。
        .ok_or_else(invalid_input)?;
    // 构造完整强类型输入。
    Ok(BrowserElementTypeInput {
        // 解析当前公开页面 identity。
        page_id: page_id(object)?,
        // 解析当前公开元素 identity。
        element_id: element_id(object)?,
        // 仅保存给当前请求所有权。
        text: text.to_owned(),
        // 保存显式替换语义。
        replace,
        // 应用共享总预算边界。
        timeout_ms: timeout_ms(object)?,
    })
}

// 解析页面截图公开 input。
pub(crate) fn parse_screenshot(input: Option<&Value>) -> AppResult<BrowserPageScreenshotInput> {
    // 取得严格对象。
    let object = required_object(input)?;
    // 只允许页面与可选总预算。
    if !exact_keys(object, &["pageId"], &["timeoutMs"]) {
        // 路径、格式、质量、clip 和私有 CDP 字段全部失败闭合。
        return Err(invalid_input());
    }
    // 构造完整强类型输入。
    Ok(BrowserPageScreenshotInput {
        // 解析当前公开页面 identity。
        page_id: page_id(object)?,
        // 应用共享总预算边界。
        timeout_ms: timeout_ms(object)?,
    })
}

// 验证生产 parser 的默认值与私有字段拒绝边界。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造器。
    use serde_json::json;

    // 导入六个生产 parser。
    use super::{
        parse_click, parse_navigate, parse_query, parse_screenshot, parse_type, parse_wait,
    };

    // 固定 canonical 页面 fixture。
    const PAGE_ID: &str = "s2:bp:0123456789abcdef0123456789abcdef";
    // 固定 canonical 元素 fixture。
    const ELEMENT_ID: &str = "s2:be:0123456789abcdef0123456789abcdef";

    // 验证页面公开输入只接受冻结 provider-neutral 形状。
    #[test]
    fn browser_page_public_inputs_are_strict_and_defaulted() {
        // navigate 合法输入使用默认预算。
        let navigate = parse_navigate(Some(&json!({"url":"https://example.test/path"})))
            // 合法输入不得失败。
            .expect("navigate input must parse");
        // 默认预算固定五秒。
        assert_eq!(navigate.timeout_ms(), 5_000);
        // userinfo 必须在任何 Broker I/O 前拒绝。
        assert!(parse_navigate(Some(&json!({"url":"https://user@example.test/"}))).is_err());
        // confirmed 不属于业务 input。
        assert!(
            parse_navigate(Some(
                &json!({"url":"https://example.test/","confirmed":true})
            ))
            .is_err()
        );
        // wait 支持有限 document-ready 条件。
        assert!(
            parse_wait(Some(
                &json!({"pageId":PAGE_ID,"condition":{"kind":"document-ready"}})
            ))
            .is_ok()
        );
        // 未知 network-idle 条件不得扩张。
        assert!(
            parse_wait(Some(
                &json!({"pageId":PAGE_ID,"condition":{"kind":"network-idle"}})
            ))
            .is_err()
        );
        // query 合法语义 selector 使用默认结果上限。
        let query = parse_query(Some(
            &json!({"pageId":PAGE_ID,"selector":{"role":"button"}}),
        ))
        // 合法输入不得失败。
        .expect("query input must parse");
        // 默认最多返回一百项。
        assert_eq!(query.max_results(), 100);
        // CSS selector 必须失败闭合。
        assert!(
            parse_query(Some(
                &json!({"pageId":PAGE_ID,"selector":{"css":"#submit"}})
            ))
            .is_err()
        );
        // 大写页面 identity 不得被规范化。
        assert!(
            parse_query(Some(
                &json!({"pageId":"s2:bp:ABCDEF","selector":{"text":"ready"}})
            ))
            .is_err()
        );
    }

    // 验证页面动作公开输入的 identity、UTF-8、确认与私有字段边界。
    #[test]
    fn browser_page_action_inputs_are_strict_private_and_defaulted() {
        // click 合法输入使用默认预算。
        let click = parse_click(Some(&json!({"pageId":PAGE_ID,"elementId":ELEMENT_ID})))
            // 合法输入不得失败。
            .expect("click input must parse");
        // 默认预算固定五秒。
        assert_eq!(click.timeout_ms(), 5_000);
        // input 内确认不得绕过 facade Policy。
        assert!(
            parse_click(Some(
                &json!({"pageId":PAGE_ID,"elementId":ELEMENT_ID,"confirmed":true})
            ))
            .is_err()
        );
        // 大写元素 identity 不得被规范化。
        assert!(parse_click(Some(&json!({"pageId":PAGE_ID,"elementId":"s2:be:ABCDEF"}))).is_err());
        // type 必须接受有界多字节文本且不改变字节事实。
        let typed = parse_type(Some(
            &json!({"pageId":PAGE_ID,"elementId":ELEMENT_ID,"text":"世界","replace":true}),
        ))
        // 合法输入不得失败。
        .expect("type input must parse");
        // 字节长度必须按 UTF-8 计算。
        assert_eq!(typed.text().len(), 6);
        // replace 必须保留调用方显式语义。
        assert!(typed.replace());
        // 超过 16384 UTF-8 字节必须失败闭合。
        assert!(parse_type(Some(&json!({"pageId":PAGE_ID,"elementId":ELEMENT_ID,"text":"界".repeat(5_462),"replace":false}))).is_err());
        // 文本不得以 credential 扩张公开输入。
        assert!(parse_type(Some(&json!({"pageId":PAGE_ID,"elementId":ELEMENT_ID,"text":"secret","replace":false,"credentialId":"private"}))).is_err());
        // screenshot 只接受当前页面与共享预算。
        assert!(parse_screenshot(Some(&json!({"pageId":PAGE_ID,"timeoutMs":30_000}))).is_ok());
        // screenshot 不接受输出路径。
        assert!(parse_screenshot(Some(&json!({"pageId":PAGE_ID,"path":"capture.png"}))).is_err());
        // screenshot 不接受 CDP 控制字段。
        assert!(
            parse_screenshot(Some(
                &json!({"pageId":PAGE_ID,"cdpMethod":"Page.captureScreenshot"})
            ))
            .is_err()
        );
    }
}
