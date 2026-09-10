//! 冻结浏览器页面导航、等待与查询的公开输入契约候选。

// 导入 JSON 对象、构造器与值类型。
use serde_json::{Map, Value, json};

// 绑定 navigate 输入 Schema。
const NAVIGATE_SCHEMA: &str = include_str!(
    // 只读取本测试负责的公开候选。
    "../contracts/v1/browser-page-navigate-input.schema.json",
);
// 绑定 wait 输入 Schema。
const WAIT_SCHEMA: &str = include_str!(
    // 只读取本测试负责的公开候选。
    "../contracts/v1/browser-page-wait-input.schema.json",
);
// 绑定 query 输入 Schema。
const QUERY_SCHEMA: &str = include_str!(
    // 只读取本测试负责的公开候选。
    "../contracts/v1/browser-page-query-input.schema.json",
);
// 绑定成功结果 Schema。
const RESULT_SCHEMA: &str = include_str!(
    // 核对同批结果候选的 JSON 语法与变体数量。
    "../contracts/v1/browser-page-read-result.schema.json",
);
// 固定合法公开页面目标。
const PAGE_ID: &str = "s2:bp:0123456789abcdef0123456789abcdef";

// 核对对象是否只含允许字段。
fn exact_keys(
    // 借用待核对对象。
    object: &Map<String, Value>,
    // 借用允许字段集合。
    expected: &[&str],
) -> bool {
    // 字段数量与名称必须同时完全一致。
    object.len() == expected.len()
        // 拒绝任意未声明字段。
        && object.keys().all(|key| expected.contains(&key.as_str()))
}

// 验证公开页面 identity。
fn canonical_page(
    // 借用待验证文本。
    value: &str,
) -> bool {
    // 只接受公开 s2:bp 前缀。
    value.strip_prefix("s2:bp:").is_some_and(|suffix| {
        // 后缀必须恰为三十二位。
        suffix.len() == 32
            // 每位只能是小写十六进制。
            && suffix
                // 按字节验证 ASCII 集合。
                .bytes()
                // 拒绝大写与其他字符。
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

// 验证可选总预算。
fn valid_timeout(
    // 借用输入对象。
    object: &Map<String, Value>,
) -> bool {
    // 缺省使用契约默认值。
    object.get("timeoutMs").is_none_or(|value| {
        // 显式值必须是 1..30000 的整数。
        value
            .as_u64()
            .is_some_and(|value| (1..=30_000).contains(&value))
    })
}

// 验证公开 HTTP(S) URL 外层门禁。
fn valid_http_url(
    // 借用待验证 URL。
    value: &str,
) -> bool {
    // URL 必须非空且有界。
    if value.is_empty()
        // 以 Unicode 字符数对齐公开契约。
        || value.chars().count() > 8_192
        // 禁止 userinfo。
        || value.contains('@')
        // 禁止 Windows 路径分隔歧义。
        || value.contains('\\')
        // 禁止控制字符与空白。
        || value.chars().any(|character| character.is_control() || character.is_whitespace())
    {
        // 任一外层边界失败都拒绝。
        return false;
    }
    // 只接受小写 HTTP 或 HTTPS scheme。
    let Some(after_scheme) = value
        // 尝试 HTTP。
        .strip_prefix("http://")
        // 或尝试 HTTPS。
        .or_else(|| value.strip_prefix("https://"))
    else {
        // 拒绝其他 scheme。
        return false;
    };
    // authority 必须存在。
    after_scheme
        // 取路径、查询或 fragment 前的 authority。
        .split(['/', '?', '#'])
        // 读取第一段。
        .next()
        // 空 authority 必须失败。
        .is_some_and(|authority| !authority.is_empty())
}

// 验证 provider-neutral selector。
fn valid_selector(
    // 借用待验证值。
    value: &Value,
) -> bool {
    // selector 必须是对象。
    let Some(object) = value.as_object() else {
        // 非对象拒绝。
        return false;
    };
    // 只允许四个公开语义字段的任意子集。
    if object
        // 遍历实际键。
        .keys()
        // 检测任何私有查询语言。
        .any(|key| !["role", "name", "text", "exact"].contains(&key.as_str()))
    {
        // 额外字段失败闭合。
        return false;
    }
    // exact 若存在必须为布尔值。
    if object.get("exact").is_some_and(|value| !value.is_boolean()) {
        // 拒绝字符串等隐式转换。
        return false;
    }
    // 至少一个语义字段必须存在且有界非空。
    ["role", "name", "text"].into_iter().any(|name| {
        // 只接受实际字符串。
        object.get(name).and_then(Value::as_str).is_some_and(|text| {
            // 字符串必须非空且不超过 1024 字符。
            !text.is_empty() && text.chars().count() <= 1_024
        })
    })
        // 全部存在的语义字段都必须合法。
        && ["role", "name", "text"].into_iter().all(|name| {
            // 缺失字段允许使用默认 None。
            object.get(name).is_none_or(|value| {
                // 显式字段必须是有界非空字符串。
                value.as_str().is_some_and(|text| {
                    // 实施同一字符边界。
                    !text.is_empty() && text.chars().count() <= 1_024
                })
            })
        })
}

// 验证封闭 wait 条件。
fn valid_wait_condition(
    // 借用待验证值。
    value: &Value,
) -> bool {
    // 条件必须是对象。
    let Some(object) = value.as_object() else {
        // 非对象拒绝。
        return false;
    };
    // 按封闭 kind 验证精确字段。
    match object.get("kind").and_then(Value::as_str) {
        // document-ready 不允许 payload。
        Some("document-ready") => exact_keys(object, &["kind"]),
        // element-present 只接受 selector。
        Some("element-present") => {
            // 字段集合必须精确且 selector 合法。
            exact_keys(object, &["kind", "selector"])
                // 只接受 provider-neutral selector。
                && object.get("selector").is_some_and(valid_selector)
        }
        // text-present 接受文本与可选 exact。
        Some("text-present") => {
            // exact 可缺省或显式提供。
            (exact_keys(object, &["kind", "text"])
                // 显式 exact 构成第二合法形状。
                || exact_keys(object, &["kind", "text", "exact"]))
                // 文本必须有界非空。
                && object.get("text").and_then(Value::as_str).is_some_and(|text| {
                    // 实施 1..1024 字符边界。
                    !text.is_empty() && text.chars().count() <= 1_024
                })
                // exact 若存在必须为布尔值。
                && object.get("exact").is_none_or(Value::is_boolean)
        }
        // 拒绝未知条件。
        _ => false,
    }
}

// 验证 navigate 公开输入。
fn valid_navigate_input(
    // 借用待验证值。
    value: &Value,
) -> bool {
    // 输入必须是对象。
    let Some(object) = value.as_object() else {
        // 非对象拒绝。
        return false;
    };
    // 只允许 url 与可选 timeout。
    (exact_keys(object, &["url"]) || exact_keys(object, &["url", "timeoutMs"]))
        // URL 必须通过外层门禁。
        && object.get("url").and_then(Value::as_str).is_some_and(valid_http_url)
        // 总预算必须合法。
        && valid_timeout(object)
}

// 验证 wait 公开输入。
fn valid_wait_input(
    // 借用待验证值。
    value: &Value,
) -> bool {
    // 输入必须是对象。
    let Some(object) = value.as_object() else {
        // 非对象拒绝。
        return false;
    };
    // 只允许 page、condition 与可选 timeout。
    (exact_keys(object, &["pageId", "condition"])
        // 允许显式总预算。
        || exact_keys(object, &["pageId", "condition", "timeoutMs"]))
        // page 必须是公开 opaque identity。
        && object.get("pageId").and_then(Value::as_str).is_some_and(canonical_page)
        // condition 必须属于封闭集合。
        && object.get("condition").is_some_and(valid_wait_condition)
        // 总预算必须合法。
        && valid_timeout(object)
}

// 验证 query 公开输入。
fn valid_query_input(
    // 借用待验证值。
    value: &Value,
) -> bool {
    // 输入必须是对象。
    let Some(object) = value.as_object() else {
        // 非对象拒绝。
        return false;
    };
    // 实际键只能来自四个公开字段。
    if object
        // 遍历实际字段。
        .keys()
        // 拒绝额外字段。
        .any(|key| !["pageId", "selector", "maxResults", "timeoutMs"].contains(&key.as_str()))
    {
        // 私有字段失败闭合。
        return false;
    }
    // page 与 selector 都必须存在。
    object.get("pageId").and_then(Value::as_str).is_some_and(canonical_page)
        // selector 必须属于公开语义集合。
        && object.get("selector").is_some_and(valid_selector)
        // maxResults 缺省 100，显式值必须为 1..100。
        && object.get("maxResults").is_none_or(|value| {
            // 只接受整数。
            value.as_u64().is_some_and(|value| (1..=100).contains(&value))
        })
        // 总预算必须合法。
        && valid_timeout(object)
}

// 验证全部输入正例、默认值和负例。
#[test]
fn public_inputs_are_strict_and_provider_neutral() {
    // navigate 最小输入必须通过。
    assert!(valid_navigate_input(
        &json!({"url":"https://example.test/path"})
    ));
    // navigate 显式总预算必须通过。
    assert!(valid_navigate_input(
        &json!({"url":"http://127.0.0.1:8080/","timeoutMs":30000})
    ));
    // 非 HTTP(S) 必须拒绝。
    assert!(!valid_navigate_input(&json!({"url":"file:///private"})));
    // userinfo 必须拒绝。
    assert!(!valid_navigate_input(
        &json!({"url":"https://user@example.test/"})
    ));
    // confirmed 不属于业务输入。
    assert!(!valid_navigate_input(
        &json!({"url":"https://example.test/","confirmed":true})
    ));
    // endpoint 不属于业务输入。
    assert!(!valid_navigate_input(
        &json!({"url":"https://example.test/","endpoint":"ws://127.0.0.1"})
    ));
    // document-ready 最小 wait 必须通过。
    assert!(valid_wait_input(
        &json!({"pageId":PAGE_ID,"condition":{"kind":"document-ready"}})
    ));
    // element-present 语义 wait 必须通过。
    assert!(valid_wait_input(
        &json!({"pageId":PAGE_ID,"condition":{"kind":"element-present","selector":{"role":"button","exact":true}},"timeoutMs":1})
    ));
    // text-present 语义 wait 必须通过。
    assert!(valid_wait_input(
        &json!({"pageId":PAGE_ID,"condition":{"kind":"text-present","text":"Ready"}})
    ));
    // 未知 wait kind 必须拒绝。
    assert!(!valid_wait_input(
        &json!({"pageId":PAGE_ID,"condition":{"kind":"network-idle"}})
    ));
    // CSS selector 必须拒绝。
    assert!(!valid_wait_input(
        &json!({"pageId":PAGE_ID,"condition":{"kind":"element-present","selector":{"css":"#submit"}}})
    ));
    // worker page ref 必须拒绝。
    assert!(!valid_wait_input(
        &json!({"pageId":PAGE_ID,"condition":{"kind":"document-ready"},"pageRef":"w1:bp:0123456789abcdef0123456789abcdef"})
    ));
    // query 最小语义必须通过。
    assert!(valid_query_input(
        &json!({"pageId":PAGE_ID,"selector":{"name":"Submit"}})
    ));
    // query 组合 selector 与上限必须通过。
    assert!(valid_query_input(
        &json!({"pageId":PAGE_ID,"selector":{"role":"button","text":"Save","exact":false},"maxResults":100,"timeoutMs":5000})
    ));
    // 空 selector 必须拒绝。
    assert!(!valid_query_input(&json!({"pageId":PAGE_ID,"selector":{}})));
    // XPath 必须拒绝。
    assert!(!valid_query_input(
        &json!({"pageId":PAGE_ID,"selector":{"xpath":"//button"}})
    ));
    // CDP method 必须拒绝。
    assert!(!valid_query_input(
        &json!({"pageId":PAGE_ID,"selector":{"role":"button"},"cdpMethod":"Runtime.evaluate"})
    ));
    // 零结果上限必须拒绝。
    assert!(!valid_query_input(
        &json!({"pageId":PAGE_ID,"selector":{"role":"button"},"maxResults":0})
    ));
}

// 验证四份 JSON Schema 的标识、严格性与当前公开状态。
#[test]
fn schemas_match_published_page_capabilities() {
    // 解析 navigate Schema。
    let navigate = serde_json::from_str::<Value>(NAVIGATE_SCHEMA)
        // 语法失败必须直接中止测试。
        .unwrap_or_else(|error| panic!("navigate schema must parse: {error}"));
    // 解析 wait Schema。
    let wait = serde_json::from_str::<Value>(WAIT_SCHEMA)
        // 语法失败必须直接中止测试。
        .unwrap_or_else(|error| panic!("wait schema must parse: {error}"));
    // 解析 query Schema。
    let query = serde_json::from_str::<Value>(QUERY_SCHEMA)
        // 语法失败必须直接中止测试。
        .unwrap_or_else(|error| panic!("query schema must parse: {error}"));
    // 解析 result Schema。
    let result = serde_json::from_str::<Value>(RESULT_SCHEMA)
        // 语法失败必须直接中止测试。
        .unwrap_or_else(|error| panic!("result schema must parse: {error}"));
    // 核对三个输入 schema ID。
    assert_eq!(navigate["$id"], json!("schema://browser/page-navigate/v1"));
    // 核对 wait schema ID。
    assert_eq!(wait["$id"], json!("schema://browser/page-wait/v1"));
    // 核对 query schema ID。
    assert_eq!(query["$id"], json!("schema://browser/page-query/v1"));
    // 全部根对象都必须拒绝额外字段。
    for schema in [&navigate, &wait, &query, &result] {
        // 根 additionalProperties 必须为 false。
        assert_eq!(schema["additionalProperties"], json!(false));
    }
    // result 必须封闭三个成功变体。
    assert_eq!(result["oneOf"].as_array().map(Vec::len), Some(3));
    // 全部公开 Schema 都不得定义私有实现字段。
    for schema in [NAVIGATE_SCHEMA, WAIT_SCHEMA, QUERY_SCHEMA, RESULT_SCHEMA] {
        // 逐项检查典型 transport、worker 与 native 字段。
        for private in [
            "workerRef",
            "pageRef",
            "cdpMethod",
            "profilePath",
            "nativeId",
        ] {
            // JSON Schema 不能把私有名称变成公开 property。
            assert!(!schema.contains(&format!("\"{private}\"")));
        }
    }
}
