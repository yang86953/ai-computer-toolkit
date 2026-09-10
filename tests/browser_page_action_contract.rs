//! 验证浏览器点击、输入与页面截图公开候选契约。

// 导入 JSON 构造和值类型。
use serde_json::{Map, Value, json};

// 嵌入点击输入 Schema。
const CLICK_SCHEMA: &str = include_str!("../contracts/v1/browser-element-click-input.schema.json");
// 嵌入输入文本 Schema。
const TYPE_SCHEMA: &str = include_str!("../contracts/v1/browser-element-type-input.schema.json");
// 嵌入页面截图 Schema。
const SCREENSHOT_SCHEMA: &str =
    include_str!("../contracts/v1/browser-page-screenshot-input.schema.json");
// 嵌入成功结果 Schema。
const RESULT_SCHEMA: &str = include_str!("../contracts/v1/browser-page-action-result.schema.json");
// 固定合法页面 identity。
const PAGE_ID: &str = "s2:bp:0123456789abcdef0123456789abcdef";
// 固定合法元素 identity。
const ELEMENT_ID: &str = "s2:be:0123456789abcdef0123456789abcdef";

// 判断 JSON object 是否只含允许字段。
fn exact_keys(object: &Map<String, Value>, allowed: &[&str]) -> bool {
    // 字段数量必须精确匹配允许集合的子集。
    object.len() <= allowed.len()
        // 每个输入字段都必须在白名单内。
        && object
            // 遍历全部键。
            .keys()
            // 检查逐个白名单命中。
            .all(|key| allowed.contains(&key.as_str()))
}

// 判断 opaque identity 是否为指定公开类别。
fn opaque(value: Option<&Value>, prefix: &str) -> bool {
    // 只接受字符串。
    value
        // 读取字符串值。
        .and_then(Value::as_str)
        // 核对前缀和 32 位小写十六进制后缀。
        .and_then(|text| text.strip_prefix(prefix))
        // 逐字节验证 canonical 后缀。
        .is_some_and(|suffix| {
            // 长度必须固定。
            suffix.len() == 32
                // 全部字符必须是小写十六进制。
                && suffix
                    // 遍历 ASCII 字节。
                    .bytes()
                    // 拒绝大写和其他符号。
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

// 判断可选总期限是否位于公开范围。
fn timeout(object: &Map<String, Value>) -> bool {
    // 缺省期限合法。
    object.get("timeoutMs").is_none_or(|value| {
        // 显式期限必须是 1..30000 整数。
        value
            // 读取无符号整数。
            .as_u64()
            // 核对公开范围。
            .is_some_and(|milliseconds| (1..=30_000).contains(&milliseconds))
    })
}

// 判断点击输入是否符合冻结边界。
fn valid_click_input(value: &Value) -> bool {
    // 输入必须是对象。
    let Some(object) = value.as_object() else {
        // 拒绝其他 JSON 类型。
        return false;
    };
    // 字段集合、identity 和期限必须同时有效。
    exact_keys(object, &["pageId", "elementId", "timeoutMs"])
        // 页面必须 canonical。
        && opaque(object.get("pageId"), "s2:bp:")
        // 元素必须 canonical。
        && opaque(object.get("elementId"), "s2:be:")
        // 期限必须有界。
        && timeout(object)
}

// 判断文本输入是否符合冻结边界。
fn valid_type_input(value: &Value) -> bool {
    // 输入必须是对象。
    let Some(object) = value.as_object() else {
        // 拒绝其他 JSON 类型。
        return false;
    };
    // 读取待输入文本。
    let Some(text) = object.get("text").and_then(Value::as_str) else {
        // text 必须存在且为字符串。
        return false;
    };
    // 字段和 identity 必须严格。
    exact_keys(
        // 检查当前对象。
        object,
        // 只允许五个公开字段。
        &["pageId", "elementId", "text", "replace", "timeoutMs"],
    )
        // 页面必须 canonical。
        && opaque(object.get("pageId"), "s2:bp:")
        // 元素必须 canonical。
        && opaque(object.get("elementId"), "s2:be:")
        // 文本必须非空且按 UTF-8 字节有界。
        && !text.is_empty()
        // Rust parser 的字节门禁不得被 Unicode 字符数绕过。
        && text.len() <= 16_384
        // replace 必须由调用方显式提供布尔值。
        && object.get("replace").and_then(Value::as_bool).is_some()
        // 期限必须有界。
        && timeout(object)
}

// 判断页面截图输入是否符合冻结边界。
fn valid_screenshot_input(value: &Value) -> bool {
    // 输入必须是对象。
    let Some(object) = value.as_object() else {
        // 拒绝其他 JSON 类型。
        return false;
    };
    // 只允许页面和期限。
    exact_keys(object, &["pageId", "timeoutMs"])
        // 页面必须 canonical。
        && opaque(object.get("pageId"), "s2:bp:")
        // 期限必须有界。
        && timeout(object)
}

// 读取 Schema 对象并在语法错误时提供明确诊断。
fn parse_schema(source: &str, name: &str) -> Value {
    // 解析 JSON Schema。
    serde_json::from_str(source)
        // 语法错误必须让定向测试失败。
        .unwrap_or_else(|error| panic!("{name} schema must parse: {error}"))
}

// 读取字符串数组为借用切片。
fn string_array<'a>(value: &'a Value, field: &str) -> Vec<&'a str> {
    // 字段必须是数组。
    value[field]
        // 读取数组。
        .as_array()
        // 缺失时提供固定诊断。
        .unwrap_or_else(|| panic!("{field} must be an array"))
        // 遍历数组成员。
        .iter()
        // 每个成员必须是字符串。
        .map(|item| {
            // 转换字符串。
            item.as_str()
                // 类型漂移必须失败。
                .unwrap_or_else(|| panic!("{field} items must be strings"))
        })
        // 收集便于集合断言。
        .collect()
}

// 验证三项公开输入的正反例和业务前私有字段拒绝。
#[test]
fn public_inputs_are_strict_and_provider_neutral() {
    // 最小点击输入必须通过。
    assert!(valid_click_input(
        &json!({"pageId": PAGE_ID, "elementId": ELEMENT_ID})
    ));
    // 点击允许显式最小期限。
    assert!(valid_click_input(
        &json!({"pageId": PAGE_ID, "elementId": ELEMENT_ID, "timeoutMs": 1})
    ));
    // click 不接受 input 内确认。
    assert!(!valid_click_input(
        &json!({"pageId": PAGE_ID, "elementId": ELEMENT_ID, "confirmed": true})
    ));
    // click 不接受私有 worker ref。
    assert!(!valid_click_input(
        &json!({"pageId": PAGE_ID, "elementId": ELEMENT_ID, "elementRef": "w1:be:0123456789abcdef0123456789abcdef"})
    ));
    // type 最小显式请求必须通过。
    assert!(valid_type_input(
        &json!({"pageId": PAGE_ID, "elementId": ELEMENT_ID, "text": "hello", "replace": true})
    ));
    // type 允许追加语义。
    assert!(valid_type_input(
        &json!({"pageId": PAGE_ID, "elementId": ELEMENT_ID, "text": "世界", "replace": false, "timeoutMs": 30_000})
    ));
    // type 缺失 replace 必须拒绝。
    assert!(!valid_type_input(
        &json!({"pageId": PAGE_ID, "elementId": ELEMENT_ID, "text": "hello"})
    ));
    // type 空文本必须拒绝。
    assert!(!valid_type_input(
        &json!({"pageId": PAGE_ID, "elementId": ELEMENT_ID, "text": "", "replace": true})
    ));
    // 多字节文本必须按 UTF-8 字节上限拒绝。
    assert!(!valid_type_input(
        &json!({"pageId": PAGE_ID, "elementId": ELEMENT_ID, "text": "界".repeat(5_462), "replace": true})
    ));
    // type 不接受 credential 专用扩张。
    assert!(!valid_type_input(
        &json!({"pageId": PAGE_ID, "elementId": ELEMENT_ID, "text": "hello", "replace": true, "credentialId": "secret"})
    ));
    // screenshot 最小输入必须通过。
    assert!(valid_screenshot_input(&json!({"pageId": PAGE_ID})));
    // screenshot 不接受格式控制。
    assert!(!valid_screenshot_input(
        &json!({"pageId": PAGE_ID, "format": "jpeg"})
    ));
    // screenshot 不接受路径。
    assert!(!valid_screenshot_input(
        &json!({"pageId": PAGE_ID, "path": "capture.png"})
    ));
    // screenshot 不接受 CDP method。
    assert!(!valid_screenshot_input(
        &json!({"pageId": PAGE_ID, "cdpMethod": "Page.captureScreenshot"})
    ));
    // 三项操作都拒绝业务前零期限。
    assert!(!valid_screenshot_input(
        &json!({"pageId": PAGE_ID, "timeoutMs": 0})
    ));
}

// 验证 Schema 标识、严格字段和固定资源上限。
#[test]
fn schemas_freeze_ids_fields_and_resource_bounds() {
    // 解析 click Schema。
    let click = parse_schema(CLICK_SCHEMA, "click");
    // 解析 type Schema。
    let type_input = parse_schema(TYPE_SCHEMA, "type");
    // 解析 screenshot Schema。
    let screenshot = parse_schema(SCREENSHOT_SCHEMA, "screenshot");
    // 解析 result Schema。
    let result = parse_schema(RESULT_SCHEMA, "result");
    // 核对 click Schema ID。
    assert_eq!(click["$id"], json!("schema://browser/element-click/v1"));
    // 核对 type Schema ID。
    assert_eq!(type_input["$id"], json!("schema://browser/element-type/v1"));
    // 核对 screenshot Schema ID。
    assert_eq!(
        screenshot["$id"],
        json!("schema://browser/page-screenshot/v1")
    );
    // 全部 Schema 根对象必须拒绝额外字段。
    for schema in [&click, &type_input, &screenshot, &result] {
        // additionalProperties 必须固定 false。
        assert_eq!(schema["additionalProperties"], json!(false));
    }
    // type 必须显式要求 replace，避免隐式覆盖策略。
    let type_required = string_array(&type_input, "required");
    // replace 必须在 required 中。
    assert!(type_required.contains(&"replace"));
    // text 字符外层上限必须固定。
    assert_eq!(type_input["properties"]["text"]["maxLength"], json!(16_384));
    // result 必须封闭三个成功变体。
    assert_eq!(result["oneOf"].as_array().map(Vec::len), Some(3));
    // screenshot raw PNG 上限必须固定 12 MiB。
    assert_eq!(
        result["$defs"]["screenshotData"]["allOf"][1]["properties"]["pngBytes"]["maximum"],
        json!(12_582_912)
    );
    // screenshot Base64 上限必须固定 16 MiB。
    assert_eq!(
        result["$defs"]["screenshotData"]["allOf"][1]["properties"]["pngBase64"]["maxLength"],
        json!(16_777_216)
    );
    // 两项 Command 必须保守标记可能修改。
    assert_eq!(
        result["$defs"]["commandCommon"]["properties"]["targetMayHaveMutated"]["const"],
        json!(true)
    );
    // screenshot Query 必须只读。
    assert_eq!(
        result["$defs"]["queryCommon"]["properties"]["readOnly"]["const"],
        json!(true)
    );
    // 公开结果不能定义调用方文本或私有实现字段。
    for private in [
        "text",
        "replace",
        "workerRef",
        "pageRef",
        "elementRef",
        "cdpMethod",
        "profilePath",
        "nativeId",
        "path",
    ] {
        // Result Schema 不能把私有名称或敏感输入变成 property。
        assert!(!RESULT_SCHEMA.contains(&format!("\"{private}\"")));
    }
}
