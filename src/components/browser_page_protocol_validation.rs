//! 负责页面操作自身的 provider-neutral 有界验证。

// 导入父协议的私有类型与边界。
use super::{
    // 导入 selector 类型。
    BrowserElementSelector,
    // 导入页面操作类型。
    BrowserPageOperation,
    // 导入协议失败类型。
    BrowserPageProtocolFailure,
    // 导入 wait 条件类型。
    BrowserWaitCondition,
    // 导入查询数量上限。
    MAXIMUM_QUERY_RESULTS,
    // 导入 selector 文本上限。
    MAXIMUM_SELECTOR_TEXT_BYTES,
    // 导入输入文本上限。
    MAXIMUM_TYPE_TEXT_BYTES,
    // 导入 URL 上限。
    MAXIMUM_URL_BYTES,
    // 导入参数失败构造器。
    invalid_argument,
    // 导入 element ref 验证器。
    is_element_ref,
};

// 验证操作自身有界语义。
pub(super) fn validate_operation(
    // 借用待验证页面操作。
    operation: &BrowserPageOperation,
) -> Result<(), BrowserPageProtocolFailure> {
    // 按操作种类验证。
    let valid = match operation {
        // 导航只接受有界 http/https URL。
        BrowserPageOperation::Navigate { url } => {
            !url.is_empty()
                && url.len() <= MAXIMUM_URL_BYTES
                && (url.starts_with("https://") || url.starts_with("http://"))
        }
        // wait 条件必须合法。
        BrowserPageOperation::Wait { condition } => validate_wait(condition),
        // query selector 与数量必须有界。
        BrowserPageOperation::Query {
            selector,
            max_results,
        } => validate_selector(selector) && (1..=MAXIMUM_QUERY_RESULTS).contains(max_results),
        // 点击要求确认和 canonical element ref。
        BrowserPageOperation::Click {
            confirmed,
            element_ref,
        } => *confirmed && is_element_ref(element_ref),
        // 输入要求确认、canonical ref 与有界非空文本。
        BrowserPageOperation::Type {
            confirmed,
            element_ref,
            text,
            ..
        } => {
            *confirmed
                && is_element_ref(element_ref)
                && !text.is_empty()
                && text.len() <= MAXIMUM_TYPE_TEXT_BYTES
        }
        // 截图没有调用方扩展字段。
        BrowserPageOperation::Screenshot {} => true,
    };
    // 非法操作失败。
    if !valid {
        // 返回参数失败。
        return Err(invalid_argument());
    }
    // 操作合法。
    Ok(())
}

// 验证 wait 条件。
fn validate_wait(
    // 借用 wait 条件。
    condition: &BrowserWaitCondition,
) -> bool {
    // 按条件种类验证。
    match condition {
        // document-ready 无额外字段。
        BrowserWaitCondition::DocumentReady {} => true,
        // 元素条件复用 selector 验证。
        BrowserWaitCondition::ElementPresent { selector } => validate_selector(selector),
        // 文本条件要求有界非空文本。
        BrowserWaitCondition::TextPresent { text, .. } => {
            !text.is_empty() && text.len() <= MAXIMUM_SELECTOR_TEXT_BYTES
        }
    }
}

// 验证 provider-neutral selector。
fn validate_selector(
    // 借用 provider-neutral selector。
    selector: &BrowserElementSelector,
) -> bool {
    // 至少一个语义字段非空。
    let any = selector
        .role
        .as_deref()
        .is_some_and(|value| !value.is_empty())
        || selector
            .name
            .as_deref()
            .is_some_and(|value| !value.is_empty())
        || selector
            .text
            .as_deref()
            .is_some_and(|value| !value.is_empty());
    // 所有可选字段都必须有界且非空。
    any && [&selector.role, &selector.name, &selector.text]
        // 遍历可选字段。
        .into_iter()
        // 每个存在字段都验证。
        .all(|field| {
            // 空值跳过，存在值核对边界。
            field.as_deref().is_none_or(|value| {
                // 存在值必须非空有界。
                !value.is_empty() && value.len() <= MAXIMUM_SELECTOR_TEXT_BYTES
            })
        })
}
