//! 构造 browser-session broker v1 的导航、等待与查询 request frame。

// 导入 JSON 构造宏和值类型。
use serde_json::{Value, json};

// 导入父 wire Component 的严格 request frame。
use super::BrowserSessionBrokerRequestFrame;
// 导入同源 canonical 编码与协议类型。
use super::super::{
    // 导入 provider-neutral selector。
    BrowserSemanticSelector,
    // 导入协议失败。
    BrowserSessionBrokerProtocolFailure,
    // 导入有限等待条件。
    BrowserWaitCondition,
    // 导入固定协议版本。
    CONTRACT_VERSION,
    // 导入长度前缀编码 helper。
    append_canonical_piece,
    // 导入同源语义摘要 helper。
    fingerprint_for_canonical_key,
};

// 为严格 request frame 扩展页面导航与只读查询 builder。
impl BrowserSessionBrokerRequestFrame {
    // 构造只能表达 confirmed=true 的页面导航 command。
    pub(crate) fn navigate_confirmed(
        // 接收 canonical request nonce。
        request_nonce: &str,
        // 接收当前 broker epoch。
        expected_broker_epoch: &str,
        // 接收剩余 deadline 预算。
        remaining_timeout_ms: u32,
        // 接收唯一公开 session target。
        session_id: &str,
        // 接收已经受限的 HTTP(S) URL。
        url: &str,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // 计算与 parser 完全同源的 navigate 指纹。
        let fingerprint = navigate_fingerprint(expected_broker_epoch, session_id, url);
        // 构造精确字段集合并固化显式确认。
        let value = json!({
            // 固定 request frame kind。
            "kind": "request",
            // 固定协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 保存 request nonce。
            "requestNonce": request_nonce,
            // 保存同源 semantic fingerprint。
            "semanticFingerprint": fingerprint,
            // 绑定 expected broker epoch。
            "expectedBrokerEpoch": expected_broker_epoch,
            // 保存剩余 deadline 预算。
            "remainingTimeoutMs": remaining_timeout_ms,
            // 固定 navigate operation。
            "operation": "navigate",
            // 保存唯一公开 session target。
            "sessionId": session_id,
            // 保存目标 URL。
            "url": url,
            // command builder 只能表达 confirmed=true。
            "confirmed": true
        });
        // 使用同源 parser 复核 URL、确认、target 与指纹。
        Self::from_value(value, expected_broker_epoch)
    }

    // 构造不含 confirmed 的页面等待 Query。
    pub(crate) fn wait(
        // 接收 canonical request nonce。
        request_nonce: &str,
        // 接收当前 broker epoch。
        expected_broker_epoch: &str,
        // 接收剩余 deadline 预算。
        remaining_timeout_ms: u32,
        // 接收唯一公开 session target。
        session_id: &str,
        // 接收当前导航代际的公开 page target。
        page_id: &str,
        // 借用有限等待条件。
        condition: &BrowserWaitCondition,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // 计算与 parser 完全同源的 wait 指纹。
        let fingerprint = wait_fingerprint(
            // 绑定 live epoch。
            expected_broker_epoch,
            // 绑定公开 session。
            session_id,
            // 绑定公开 page。
            page_id,
            // 绑定完整等待条件。
            condition,
        );
        // 构造精确 Query 字段集合。
        let value = json!({
            // 固定 request frame kind。
            "kind": "request",
            // 固定协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 保存 request nonce。
            "requestNonce": request_nonce,
            // 保存同源 semantic fingerprint。
            "semanticFingerprint": fingerprint,
            // 绑定 expected broker epoch。
            "expectedBrokerEpoch": expected_broker_epoch,
            // 保存剩余 deadline 预算。
            "remainingTimeoutMs": remaining_timeout_ms,
            // 固定 wait operation。
            "operation": "wait",
            // 保存唯一公开 session target。
            "sessionId": session_id,
            // 保存当前公开 page target。
            "pageId": page_id,
            // 编码有限等待条件。
            "condition": condition_value(condition)
        });
        // 使用同源 parser 复核条件、target 与指纹。
        Self::from_value(value, expected_broker_epoch)
    }

    // 构造不含 confirmed 的 provider-neutral 元素查询。
    pub(crate) fn query(
        // 接收 canonical request nonce。
        request_nonce: &str,
        // 接收当前 broker epoch。
        expected_broker_epoch: &str,
        // 接收剩余 deadline 预算。
        remaining_timeout_ms: u32,
        // 接收唯一公开 session target。
        session_id: &str,
        // 接收当前导航代际的公开 page target。
        page_id: &str,
        // 借用 provider-neutral selector。
        selector: &BrowserSemanticSelector,
        // 接收有界结果上限。
        max_results: u16,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // 计算与 parser 完全同源的 query 指纹。
        let fingerprint = query_fingerprint(
            // 绑定 live epoch。
            expected_broker_epoch,
            // 绑定公开 session。
            session_id,
            // 绑定公开 page。
            page_id,
            // 绑定完整 selector。
            selector,
            // 绑定结果上限。
            max_results,
        );
        // 构造精确 Query 字段集合。
        let value = json!({
            // 固定 request frame kind。
            "kind": "request",
            // 固定协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 保存 request nonce。
            "requestNonce": request_nonce,
            // 保存同源 semantic fingerprint。
            "semanticFingerprint": fingerprint,
            // 绑定 expected broker epoch。
            "expectedBrokerEpoch": expected_broker_epoch,
            // 保存剩余 deadline 预算。
            "remainingTimeoutMs": remaining_timeout_ms,
            // 固定 query operation。
            "operation": "query",
            // 保存唯一公开 session target。
            "sessionId": session_id,
            // 保存当前公开 page target。
            "pageId": page_id,
            // 编码 provider-neutral selector。
            "selector": selector_value(selector),
            // 保存有界结果上限。
            "maxResults": max_results
        });
        // 使用同源 parser 复核 selector、上限、target 与指纹。
        Self::from_value(value, expected_broker_epoch)
    }

    // 构造只能表达 confirmed=true 的当前元素点击命令。
    pub(crate) fn click_confirmed(
        // 接收 canonical request nonce。
        request_nonce: &str,
        // 接收当前 broker epoch。
        expected_broker_epoch: &str,
        // 接收剩余 deadline 预算。
        remaining_timeout_ms: u32,
        // 接收唯一公开 session target。
        session_id: &str,
        // 接收当前导航代际的公开 page target。
        page_id: &str,
        // 接收当前页面签发的公开 element target。
        element_id: &str,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // 计算与 parser 完全同源的 click 指纹。
        let fingerprint = click_fingerprint(
            // 绑定 live epoch。
            expected_broker_epoch,
            // 绑定公开 session。
            session_id,
            // 绑定公开 page。
            page_id,
            // 绑定公开 element。
            element_id,
        );
        // 构造精确字段集合并固化显式确认。
        let value = json!({
            // 固定 request frame kind。
            "kind": "request",
            // 固定协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 保存 request nonce。
            "requestNonce": request_nonce,
            // 保存同源 semantic fingerprint。
            "semanticFingerprint": fingerprint,
            // 绑定 expected broker epoch。
            "expectedBrokerEpoch": expected_broker_epoch,
            // 保存剩余 deadline 预算。
            "remainingTimeoutMs": remaining_timeout_ms,
            // 固定 click operation。
            "operation": "click",
            // 保存唯一公开 session target。
            "sessionId": session_id,
            // 保存当前公开 page target。
            "pageId": page_id,
            // 保存当前公开 element target。
            "elementId": element_id,
            // command builder 只能表达 confirmed=true。
            "confirmed": true
        });
        // 使用同源 parser 复核确认、三级目标与指纹。
        Self::from_value(value, expected_broker_epoch)
    }

    // 构造只能表达 confirmed=true 的当前元素文本输入命令。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn type_confirmed(
        // 接收 canonical request nonce。
        request_nonce: &str,
        // 接收当前 broker epoch。
        expected_broker_epoch: &str,
        // 接收剩余 deadline 预算。
        remaining_timeout_ms: u32,
        // 接收唯一公开 session target。
        session_id: &str,
        // 接收当前导航代际的公开 page target。
        page_id: &str,
        // 接收当前页面签发的公开 element target。
        element_id: &str,
        // 接收有界 UTF-8 文本。
        text: &str,
        // 接收显式替换语义。
        replace: bool,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // 计算与 parser 完全同源的 type 指纹。
        let fingerprint = type_fingerprint(
            // 绑定 live epoch。
            expected_broker_epoch,
            // 绑定公开 session。
            session_id,
            // 绑定公开 page。
            page_id,
            // 绑定公开 element。
            element_id,
            // 绑定完整文本而不做归一化。
            text,
            // 绑定显式替换语义。
            replace,
        );
        // 构造精确字段集合并固化显式确认。
        let value = json!({
            // 固定 request frame kind。
            "kind": "request",
            // 固定协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 保存 request nonce。
            "requestNonce": request_nonce,
            // 保存同源 semantic fingerprint。
            "semanticFingerprint": fingerprint,
            // 绑定 expected broker epoch。
            "expectedBrokerEpoch": expected_broker_epoch,
            // 保存剩余 deadline 预算。
            "remainingTimeoutMs": remaining_timeout_ms,
            // 固定 type operation。
            "operation": "type",
            // 保存唯一公开 session target。
            "sessionId": session_id,
            // 保存当前公开 page target。
            "pageId": page_id,
            // 保存当前公开 element target。
            "elementId": element_id,
            // 保存完整调用方文本。
            "text": text,
            // 保存显式替换语义。
            "replace": replace,
            // command builder 只能表达 confirmed=true。
            "confirmed": true
        });
        // 使用同源 parser 复核确认、文本边界、三级目标与指纹。
        Self::from_value(value, expected_broker_epoch)
    }

    // 构造不含 confirmed 的当前页面 PNG 截图 Query。
    pub(crate) fn screenshot(
        // 接收 canonical request nonce。
        request_nonce: &str,
        // 接收当前 broker epoch。
        expected_broker_epoch: &str,
        // 接收剩余 deadline 预算。
        remaining_timeout_ms: u32,
        // 接收唯一公开 session target。
        session_id: &str,
        // 接收当前导航代际的公开 page target。
        page_id: &str,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // 计算与 parser 完全同源的 screenshot 指纹。
        let fingerprint = screenshot_fingerprint(expected_broker_epoch, session_id, page_id);
        // 构造不含格式、路径、质量或 CDP 参数的 Query。
        let value = json!({
            // 固定 request frame kind。
            "kind": "request",
            // 固定协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 保存 request nonce。
            "requestNonce": request_nonce,
            // 保存同源 semantic fingerprint。
            "semanticFingerprint": fingerprint,
            // 绑定 expected broker epoch。
            "expectedBrokerEpoch": expected_broker_epoch,
            // 保存剩余 deadline 预算。
            "remainingTimeoutMs": remaining_timeout_ms,
            // 固定 screenshot operation。
            "operation": "screenshot",
            // 保存唯一公开 session target。
            "sessionId": session_id,
            // 保存当前公开 page target。
            "pageId": page_id
        });
        // 使用同源 parser 复核 Query 字段、目标与指纹。
        Self::from_value(value, expected_broker_epoch)
    }
}

// 计算 navigate 的完整 canonical 指纹。
fn navigate_fingerprint(epoch: &str, session_id: &str, url: &str) -> String {
    // 初始化版本化 canonical key。
    let mut key = String::from(CONTRACT_VERSION);
    // 追加 expected epoch。
    append_canonical_piece(&mut key, epoch);
    // 追加 operation。
    append_canonical_piece(&mut key, "navigate");
    // 追加公开 session。
    append_canonical_piece(&mut key, session_id);
    // 追加完整 URL。
    append_canonical_piece(&mut key, url);
    // 使用 parser 同源 FNV-1a helper。
    fingerprint_for_canonical_key(&key)
}

// 计算 wait 的完整 canonical 指纹。
fn wait_fingerprint(
    // 借用 expected epoch。
    epoch: &str,
    // 借用公开 session。
    session_id: &str,
    // 借用公开 page。
    page_id: &str,
    // 借用完整等待条件。
    condition: &BrowserWaitCondition,
) -> String {
    // 初始化版本化 canonical key。
    let mut key = String::from(CONTRACT_VERSION);
    // 追加 expected epoch。
    append_canonical_piece(&mut key, epoch);
    // 追加 operation。
    append_canonical_piece(&mut key, "wait");
    // 追加公开 session。
    append_canonical_piece(&mut key, session_id);
    // 追加公开 page。
    append_canonical_piece(&mut key, page_id);
    // 追加有限条件。
    append_condition_key(&mut key, condition);
    // 使用 parser 同源 FNV-1a helper。
    fingerprint_for_canonical_key(&key)
}

// 计算 query 的完整 canonical 指纹。
fn query_fingerprint(
    // 借用 expected epoch。
    epoch: &str,
    // 借用公开 session。
    session_id: &str,
    // 借用公开 page。
    page_id: &str,
    // 借用完整 selector。
    selector: &BrowserSemanticSelector,
    // 接收有界结果上限。
    max_results: u16,
) -> String {
    // 初始化版本化 canonical key。
    let mut key = String::from(CONTRACT_VERSION);
    // 追加 expected epoch。
    append_canonical_piece(&mut key, epoch);
    // 追加 operation。
    append_canonical_piece(&mut key, "query");
    // 追加公开 session。
    append_canonical_piece(&mut key, session_id);
    // 追加公开 page。
    append_canonical_piece(&mut key, page_id);
    // 追加完整 selector。
    append_selector_key(&mut key, selector);
    // 追加结果上限。
    append_canonical_piece(&mut key, &max_results.to_string());
    // 使用 parser 同源 FNV-1a helper。
    fingerprint_for_canonical_key(&key)
}

// 计算 click 的完整 canonical 指纹。
fn click_fingerprint(epoch: &str, session_id: &str, page_id: &str, element_id: &str) -> String {
    // 初始化版本化 canonical key。
    let mut key = String::from(CONTRACT_VERSION);
    // 追加 expected epoch。
    append_canonical_piece(&mut key, epoch);
    // 追加 operation。
    append_canonical_piece(&mut key, "click");
    // 追加公开 session。
    append_canonical_piece(&mut key, session_id);
    // 追加公开 page。
    append_canonical_piece(&mut key, page_id);
    // 追加公开 element。
    append_canonical_piece(&mut key, element_id);
    // 使用 parser 同源 FNV-1a helper。
    fingerprint_for_canonical_key(&key)
}

// 计算 type 的完整 canonical 指纹。
fn type_fingerprint(
    // 借用 expected epoch。
    epoch: &str,
    // 借用公开 session。
    session_id: &str,
    // 借用公开 page。
    page_id: &str,
    // 借用公开 element。
    element_id: &str,
    // 借用完整文本。
    text: &str,
    // 接收显式替换语义。
    replace: bool,
) -> String {
    // 初始化版本化 canonical key。
    let mut key = String::from(CONTRACT_VERSION);
    // 追加 expected epoch。
    append_canonical_piece(&mut key, epoch);
    // 追加 operation。
    append_canonical_piece(&mut key, "type");
    // 追加公开 session。
    append_canonical_piece(&mut key, session_id);
    // 追加公开 page。
    append_canonical_piece(&mut key, page_id);
    // 追加公开 element。
    append_canonical_piece(&mut key, element_id);
    // 追加完整文本。
    append_canonical_piece(&mut key, text);
    // 追加布尔语义。
    append_canonical_piece(&mut key, if replace { "1" } else { "0" });
    // 使用 parser 同源 FNV-1a helper。
    fingerprint_for_canonical_key(&key)
}

// 计算 screenshot 的完整 canonical 指纹。
fn screenshot_fingerprint(epoch: &str, session_id: &str, page_id: &str) -> String {
    // 初始化版本化 canonical key。
    let mut key = String::from(CONTRACT_VERSION);
    // 追加 expected epoch。
    append_canonical_piece(&mut key, epoch);
    // 追加 operation。
    append_canonical_piece(&mut key, "screenshot");
    // 追加公开 session。
    append_canonical_piece(&mut key, session_id);
    // 追加公开 page。
    append_canonical_piece(&mut key, page_id);
    // 使用 parser 同源 FNV-1a helper。
    fingerprint_for_canonical_key(&key)
}

// 把 selector 编码为严格 JSON 值。
fn selector_value(selector: &BrowserSemanticSelector) -> Value {
    // 保留显式 nullable 字段与 exact 标记。
    json!({
        // 编码可选 role。
        "role": selector.role(),
        // 编码可选名称。
        "name": selector.name(),
        // 编码可选文本。
        "text": selector.text(),
        // 编码逐字匹配要求。
        "exact": selector.exact()
    })
}

// 把有限 wait 条件编码为严格 JSON 值。
fn condition_value(condition: &BrowserWaitCondition) -> Value {
    // 穷举全部允许条件。
    match condition {
        // 文档 ready 不携带额外字段。
        BrowserWaitCondition::DocumentReady => json!({ "kind": "document-ready" }),
        // 元素出现携带 provider-neutral selector。
        BrowserWaitCondition::ElementPresent(selector) => json!({
            // 固定条件类别。
            "kind": "element-present",
            // 编码完整 selector。
            "selector": selector_value(selector)
        }),
        // 文本出现携带有界文本与匹配方式。
        BrowserWaitCondition::TextPresent { text, exact } => json!({
            // 固定条件类别。
            "kind": "text-present",
            // 保存目标文本。
            "text": text,
            // 保存逐字匹配要求。
            "exact": exact
        }),
    }
}

// 向 canonical key 追加 selector。
fn append_selector_key(key: &mut String, selector: &BrowserSemanticSelector) {
    // 写入 selector 域标签。
    append_canonical_piece(key, "selector");
    // 写入 role 存在位与内容。
    append_optional_key(key, selector.role());
    // 写入 name 存在位与内容。
    append_optional_key(key, selector.name());
    // 写入 text 存在位与内容。
    append_optional_key(key, selector.text());
    // 写入 exact 标记。
    append_canonical_piece(key, if selector.exact() { "1" } else { "0" });
}

// 以显式存在位追加可选字符串。
fn append_optional_key(key: &mut String, value: Option<&str>) {
    // 按存在性编码，避免 sentinel 冲突。
    match value {
        // 缺失只写入存在位零。
        None => append_canonical_piece(key, "0"),
        // 存在时写入存在位一与内容。
        Some(value) => {
            // 写入存在位一。
            append_canonical_piece(key, "1");
            // 写入完整内容。
            append_canonical_piece(key, value);
        }
    }
}

// 向 canonical key 追加有限等待条件。
fn append_condition_key(key: &mut String, condition: &BrowserWaitCondition) {
    // 穷举全部条件类别。
    match condition {
        // 文档 ready 只写入类别。
        BrowserWaitCondition::DocumentReady => append_canonical_piece(key, "document-ready"),
        // 元素出现写入类别与 selector。
        BrowserWaitCondition::ElementPresent(selector) => {
            // 写入条件类别。
            append_canonical_piece(key, "element-present");
            // 写入完整 selector。
            append_selector_key(key, selector);
        }
        // 文本出现写入类别、文本与 exact。
        BrowserWaitCondition::TextPresent { text, exact } => {
            // 写入条件类别。
            append_canonical_piece(key, "text-present");
            // 写入完整文本。
            append_canonical_piece(key, text);
            // 写入 exact 标记。
            append_canonical_piece(key, if *exact { "1" } else { "0" });
        }
    }
}

// 注册页面 request builder 的纯单元测试。
#[cfg(test)]
// 将测试留在独立文件以保持 Component 聚焦。
#[path = "browser_session_broker_wire_page_tests.rs"]
mod tests;
