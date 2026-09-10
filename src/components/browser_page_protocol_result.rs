//! 验证 browser page command 的 provider-neutral 成功数据与安全错误。

// 导入 JSON 值。
use serde_json::Value;

// 导入父协议的固定类型、边界和身份验证器。
use super::{
    // 导入操作种类。
    BrowserPageOperationKind,
    // 导入固定结果边界。
    MAXIMUM_PNG_BYTES,
    MAXIMUM_QUERY_RESULTS,
    MAXIMUM_SELECTOR_TEXT_BYTES,
    MAXIMUM_TYPE_TEXT_BYTES,
    // 导入私有身份验证。
    is_element_ref,
    is_page_ref,
};

// 验证 completed 数据与操作匹配且不含 native 字段。
pub(super) fn validate_success_data(operation: BrowserPageOperationKind, value: &Value) -> bool {
    // 数据必须是对象。
    let Some(object) = value.as_object() else {
        // 非对象拒绝。
        return false;
    };
    // kind 必须匹配预期操作。
    let kind = object.get("kind").and_then(Value::as_str);
    // 按操作验证封闭字段。
    match operation {
        // 导航返回新 page ref。
        BrowserPageOperationKind::Navigate => {
            exact_keys(object, &["kind", "pageRef", "navigated"])
                && kind == Some("navigate")
                && object
                    // 读取 page ref。
                    .get("pageRef")
                    // 转换字符串。
                    .and_then(Value::as_str)
                    // 核对私有形状。
                    .is_some_and(is_page_ref)
                && object.get("navigated").and_then(Value::as_bool) == Some(true)
        }
        // wait 只返回满足事实。
        BrowserPageOperationKind::Wait => {
            exact_keys(object, &["kind", "conditionMet"])
                && kind == Some("wait")
                && object.get("conditionMet").and_then(Value::as_bool) == Some(true)
        }
        // query 返回有界 provider-neutral matches。
        BrowserPageOperationKind::Query => validate_query_data(object, kind),
        // click 只返回动作完成事实。
        BrowserPageOperationKind::Click => {
            exact_keys(object, &["kind", "clicked"])
                && kind == Some("click")
                && object.get("clicked").and_then(Value::as_bool) == Some(true)
        }
        // type 返回完成和字节计数。
        BrowserPageOperationKind::Type => {
            exact_keys(object, &["kind", "typed", "utf8Bytes"])
                && kind == Some("type")
                && object.get("typed").and_then(Value::as_bool) == Some(true)
                && object
                    // 读取字节数。
                    .get("utf8Bytes")
                    // 转换为整数。
                    .and_then(Value::as_u64)
                    // 核对输入上限。
                    .is_some_and(|bytes| bytes <= MAXIMUM_TYPE_TEXT_BYTES as u64)
        }
        // screenshot 返回有界 PNG 和摘要。
        BrowserPageOperationKind::Screenshot => validate_screenshot_data(object, kind),
    }
}

// 验证 query 成功数据。
fn validate_query_data(object: &serde_json::Map<String, Value>, kind: Option<&str>) -> bool {
    // 只允许固定字段。
    if !exact_keys(object, &["kind", "matches", "matchCount", "truncated"])
        // kind 必须匹配。
        || kind != Some("query")
    {
        // 拒绝字段漂移。
        return false;
    }
    // 取得 matches 数组。
    let Some(matches) = object.get("matches").and_then(Value::as_array) else {
        // 非数组拒绝。
        return false;
    };
    // 数组必须有界。
    if matches.len() > usize::from(MAXIMUM_QUERY_RESULTS) {
        // 拒绝过多结果。
        return false;
    }
    // matchCount 必须不小于返回数组。
    let count_valid = object
        // 读取总命中数。
        .get("matchCount")
        // 转换为整数。
        .and_then(Value::as_u64)
        // 核对不小于数组且有界到 u32。
        .is_some_and(|count| count >= matches.len() as u64 && count <= u64::from(u32::MAX));
    // truncated 必须是布尔值。
    let truncated_valid = object.get("truncated").and_then(Value::as_bool).is_some();
    // 每个 match 只允许 provider-neutral 摘要。
    count_valid
        && truncated_valid
        && matches.iter().all(|item| {
            // match 必须是对象。
            let Some(item) = item.as_object() else {
                // 非对象拒绝。
                return false;
            };
            // 核对固定字段。
            exact_keys(item, &["elementRef", "role", "name", "text", "enabled"])
                // element ref 必须 canonical。
                && item
                    // 读取 ref。
                    .get("elementRef")
                    // 转换字符串。
                    .and_then(Value::as_str)
                    // 核对私有形状。
                    .is_some_and(is_element_ref)
                // 可选文本必须有界。
                && ["role", "name", "text"].into_iter().all(|key| {
                    // null 或有界字符串合法。
                    item.get(key).is_some_and(|value| {
                        // 允许 null。
                        value.is_null()
                            // 或有界字符串。
                            || value
                                // 转换字符串。
                                .as_str()
                                // 核对文本边界。
                                .is_some_and(|text| text.len() <= MAXIMUM_SELECTOR_TEXT_BYTES)
                    })
                })
                // enabled 必须布尔。
                && item.get("enabled").and_then(Value::as_bool).is_some()
        })
}

// 验证 screenshot 成功数据。
fn validate_screenshot_data(
    // 借用数据对象。
    object: &serde_json::Map<String, Value>,
    // 借用 kind。
    kind: Option<&str>,
) -> bool {
    // 核对固定字段和 MIME。
    exact_keys(
        // 借用对象。
        object,
        // 只允许 provider-neutral PNG 事实。
        &["kind", "mimeType", "pngBase64", "pngBytes", "width", "height", "digest"],
    )
        // kind 必须匹配。
        && kind == Some("screenshot")
        // MIME 固定 PNG。
        && object.get("mimeType").and_then(Value::as_str) == Some("image/png")
        // base64 必须有界且只含标准字符。
        && object
            // 读取 base64。
            .get("pngBase64")
            // 转换字符串。
            .and_then(Value::as_str)
            // 核对有界字符集。
            .is_some_and(|text| {
                // 非空且不超过编码后上限。
                !text.is_empty()
                    && text.len() <= ((MAXIMUM_PNG_BYTES as usize).saturating_add(2) / 3 * 4)
                    // 只允许标准 base64 字符。
                    && text.bytes().all(|byte| {
                        // 核对字符集。
                        byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
                    })
            })
        // 原始 PNG 字节必须有界非零。
        && object
            // 读取字节数。
            .get("pngBytes")
            // 转换整数。
            .and_then(Value::as_u64)
            // 核对范围。
            .is_some_and(|bytes| (1..=MAXIMUM_PNG_BYTES).contains(&bytes))
        // 尺寸必须位于固定范围。
        && ["width", "height"].into_iter().all(|key| {
            // 读取尺寸并核对。
            object
                // 读取字段。
                .get(key)
                // 转换整数。
                .and_then(Value::as_u64)
                // 采用现有浏览器视口硬上限。
                .is_some_and(|value| (1..=10_000).contains(&value))
        })
        // digest 只接受固定 16 hex FNV 摘要。
        && object
            // 读取摘要。
            .get("digest")
            // 转换字符串。
            .and_then(Value::as_str)
            // 核对十六进制形状。
            .is_some_and(|value| {
                // 长度与字符集必须同时成立。
                value.len() == 16 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
}

// 判断对象字段是否与固定集合完全相等。
fn exact_keys(object: &serde_json::Map<String, Value>, expected: &[&str]) -> bool {
    // 数量和逐项成员都必须匹配。
    object.len() == expected.len()
        // 所有实际键都必须在固定集合中。
        && object.keys().all(|key| expected.contains(&key.as_str()))
}

// 验证安全错误对象。
pub(super) fn is_safe_error(value: &Value) -> bool {
    // 错误必须是对象。
    let Some(object) = value.as_object() else {
        // 非对象拒绝。
        return false;
    };
    // 只允许 code、message 与 details。
    if object
        // 遍历键。
        .keys()
        // 拒绝未知字段。
        .any(|key| !matches!(key.as_str(), "code" | "message" | "details"))
    {
        // 字段漂移拒绝。
        return false;
    }
    // code 必须是有界大写标识。
    let code_valid = object
        // 读取 code。
        .get("code")
        // 转换字符串。
        .and_then(Value::as_str)
        // 核对形状。
        .is_some_and(|code| {
            // 长度非空有界。
            !code.is_empty()
                && code.len() <= 64
                // 首字符大写。
                && code.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
                // 字符集封闭。
                && code
                    // 遍历字节。
                    .bytes()
                    // 只允许大写、数字和下划线。
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        });
    // message 必须非空有界。
    let message_valid = object
        // 读取 message。
        .get("message")
        // 转换字符串。
        .and_then(Value::as_str)
        // 核对长度。
        .is_some_and(|message| !message.is_empty() && message.len() <= 512);
    // 必需字段都合法。
    code_valid && message_valid
}
