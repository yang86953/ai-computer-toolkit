// 导入 JSON 构造与值。
use serde_json::{Value, json};

// 导入父协议类型和解析器。
use super::{
    // 导入操作和 outcome 类型。
    BrowserPageOperationKind,
    BrowserPageOutcome,
    // 导入错误码和输入 parser。
    BrowserPageProtocolErrorCode,
    BrowserPageWorkerInput,
    // 导入固定版本和输出上限。
    CONTRACT_VERSION,
    MAXIMUM_OUTPUT_BYTES,
    // 导入输出观察器。
    observe_output,
};

// 嵌入同源 JSON Schema 供边界回归。
const SCHEMA: &str =
    include_str!("../../contracts/internal/browser-page-command-worker-v1.schema.json");

// 固定测试 session ID。
const SESSION_ID: &str = "s2:bs:0123456789abcdef0123456789abcdef";
// 固定测试 request nonce。
const REQUEST_NONCE: &str = "fedcba9876543210fedcba9876543210";
// 固定测试 page ref。
const PAGE_REF: &str = "w1:bp:11111111111111111111111111111111";
// 固定测试 element ref。
const ELEMENT_REF: &str = "w1:be:22222222222222222222222222222222";

// 解析 JSON 值为严格输入。
fn parse(value: Value) -> Result<BrowserPageWorkerInput, BrowserPageProtocolErrorCode> {
    // 序列化固定 fixture。
    let text = serde_json::to_string(&value)
        // fixture 必须可序列化。
        .unwrap_or_else(|error| panic!("fixture serialization failed: {error}"));
    // 解析并只投影稳定错误码。
    BrowserPageWorkerInput::parse_line(&text).map_err(|error| error.code())
}

// 构造页面 command fixture。
fn command(operation: Value, page_ref: Option<&str>) -> Value {
    // 返回冻结 command 形状。
    json!({
        // 使用命令变体。
        "kind": "command",
        // 使用固定版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联固定 session。
        "sessionId": SESSION_ID,
        // 关联固定 nonce。
        "requestNonce": REQUEST_NONCE,
        // 使用有界 deadline。
        "timeoutMs": 5000,
        // 注入可选 page ref。
        "pageRef": page_ref,
        // 使用当前导航代际。
        "navigationGeneration": 3,
        // 注入强类型操作。
        "operation": operation,
    })
}

// 构造 accepted 帧。
fn accepted(operation: &str) -> String {
    // 序列化冻结 accepted。
    serde_json::to_string(&json!({
        // 标记 accepted。
        "kind": "command-accepted",
        // 固定版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联 nonce。
        "requestNonce": REQUEST_NONCE,
        // 关联操作。
        "operation": operation,
        // 声明 dispatch。
        "dispatchAccepted": true,
        // accepted 尚未完成。
        "completed": false,
    }))
    // fixture 必须可序列化。
    .unwrap_or_else(|error| panic!("accepted serialization failed: {error}"))
}

// 构造 completed final。
fn completed(operation: &str, generation: u64, data: Value) -> String {
    // 序列化冻结完成终态。
    serde_json::to_string(&json!({
        // 标记 final。
        "kind": "command-final",
        // 固定版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联 nonce。
        "requestNonce": REQUEST_NONCE,
        // 关联操作。
        "operation": operation,
        // 声明完成。
        "outcome": "completed",
        // 完成事实为真。
        "completed": true,
        // dispatch 后不自动重试。
        "retrySafe": false,
        // 明确已接受。
        "acceptedMayHaveOccurred": true,
        // 注入导航代际。
        "navigationGeneration": generation,
        // 注入成功数据。
        "data": data,
        // 成功无错误。
        "error": Value::Null,
    }))
    // fixture 必须可序列化。
    .unwrap_or_else(|error| panic!("final serialization failed: {error}"))
}

// 验证六种强类型操作都能解析且保持种类。
#[test]
// 覆盖 navigate、wait、query、click、type 与 screenshot。
fn all_page_operations_are_closed_and_typed() {
    // 构造六种操作及预期种类。
    let cases = [
        // 导航允许尚无 page ref。
        (
            command(
                json!({ "kind": "navigate", "url": "https://example.test/" }),
                None,
            ),
            BrowserPageOperationKind::Navigate,
        ),
        // 等待文档完成。
        (
            command(
                json!({ "kind": "wait", "condition": { "kind": "document-ready" } }),
                Some(PAGE_REF),
            ),
            BrowserPageOperationKind::Wait,
        ),
        // 查询语义 selector。
        (
            command(
                json!({
                    "kind": "query",
                    "selector": { "role": "button", "name": "Save", "text": null, "exact": true },
                    "maxResults": 10
                }),
                Some(PAGE_REF),
            ),
            BrowserPageOperationKind::Query,
        ),
        // 点击要求确认。
        (
            command(
                json!({ "kind": "click", "confirmed": true, "elementRef": ELEMENT_REF }),
                Some(PAGE_REF),
            ),
            BrowserPageOperationKind::Click,
        ),
        // 输入要求确认和文本。
        (
            command(
                json!({
                    "kind": "type",
                    "confirmed": true,
                    "elementRef": ELEMENT_REF,
                    "text": "hello",
                    "replace": true
                }),
                Some(PAGE_REF),
            ),
            BrowserPageOperationKind::Type,
        ),
        // 截图不接受格式或路径参数。
        (
            command(json!({ "kind": "screenshot" }), Some(PAGE_REF)),
            BrowserPageOperationKind::Screenshot,
        ),
    ];
    // 逐项核对 parser 投影。
    for (value, expected) in cases {
        // 解析当前操作。
        let input = parse(value)
            // 合法 fixture 不得失败。
            .unwrap_or_else(|code| panic!("operation parse failed: {code:?}"));
        // 核对操作种类。
        assert_eq!(input.operation_kind(), Some(expected));
        // 核对请求 nonce。
        assert_eq!(input.request_nonce(), REQUEST_NONCE);
    }
}

// 验证原生、脚本、凭据和用户 profile 字段全部拒绝。
#[test]
// 覆盖最敏感的协议扩展面。
fn native_script_and_credential_fields_are_rejected() {
    // 构造一组危险字段及值。
    let fields = [
        // 拒绝 CDP target ID。
        ("targetId", json!("native-target")),
        // 拒绝 CSS selector。
        ("css", json!("#save")),
        // 拒绝 XPath。
        ("xpath", json!("//button")),
        // 拒绝脚本。
        ("script", json!("document.cookie")),
        // 拒绝 Cookie。
        ("cookie", json!("secret")),
        // 拒绝 profile 路径。
        ("profilePath", json!("C:/Users/example")),
    ];
    // 逐一向 query operation 注入未知字段。
    for (field, value) in fields {
        // 构造正常 query。
        let mut operation = json!({
            "kind": "query",
            "selector": { "role": "button", "name": null, "text": null, "exact": false },
            "maxResults": 1
        });
        // 注入危险字段。
        operation
            // 取得对象。
            .as_object_mut()
            // fixture 形状固定为对象。
            .unwrap_or_else(|| panic!("operation fixture must be object"))
            // 插入当前字段。
            .insert(field.to_owned(), value);
        // parser 必须拒绝。
        assert!(matches!(
            // 解析危险字段输入。
            parse(command(operation, Some(PAGE_REF))),
            // 只接受固定参数失败。
            Err(BrowserPageProtocolErrorCode::InvalidArgument)
        ));
    }
}

// 验证 page、element、确认和 selector 边界。
#[test]
// 覆盖 stale 前置形状和零命中 selector 输入。
fn identity_confirmation_and_selector_bounds_fail_closed() {
    // 非导航缺少 page ref 被拒绝。
    assert!(parse(command(json!({ "kind": "screenshot" }), None)).is_err());
    // 原生 page ID 被拒绝。
    assert!(parse(command(json!({ "kind": "screenshot" }), Some("CDP-page-7"))).is_err());
    // 未确认点击被拒绝。
    assert!(
        parse(command(
            json!({ "kind": "click", "confirmed": false, "elementRef": ELEMENT_REF }),
            Some(PAGE_REF)
        ))
        .is_err()
    );
    // 空 selector 被拒绝。
    assert!(
        parse(command(
            json!({
                "kind": "query",
                "selector": { "role": null, "name": null, "text": null, "exact": false },
                "maxResults": 1
            }),
            Some(PAGE_REF)
        ))
        .is_err()
    );
    // 非 HTTP 导航被拒绝。
    assert!(
        parse(command(
            json!({ "kind": "navigate", "url": "file:///secret" }),
            None
        ))
        .is_err()
    );
}

// 验证导航与零/多命中成功数据保持 provider-neutral。
#[test]
// 覆盖代际、page ref 和 query match 形状。
fn completed_navigation_and_query_results_are_validated() {
    // 组合导航 accepted 与 final。
    let navigation = format!(
        // 使用两行输出。
        "{}\n{}",
        // accepted 行。
        accepted("navigate"),
        // completed 行。
        completed(
            "navigate",
            4,
            json!({ "kind": "navigate", "pageRef": PAGE_REF, "navigated": true })
        )
    );
    // 解析导航观察。
    let observed = observe_output(
        &navigation,
        REQUEST_NONCE,
        BrowserPageOperationKind::Navigate,
    )
    // 合法输出不得失败。
    .unwrap_or_else(|error| panic!("navigation output failed: {error}"));
    // accepted 必须成立。
    assert!(observed.accepted());
    // final 代际必须推进。
    assert_eq!(
        observed
            // 借用 final。
            .final_observation()
            // final 必须存在。
            .unwrap_or_else(|| panic!("navigation final missing"))
            // 读取代际。
            .navigation_generation(),
        4
    );
    // 构造两个 provider-neutral query matches。
    let matches = json!([
        {
            "elementRef": ELEMENT_REF,
            "role": "button",
            "name": "Save",
            "text": null,
            "enabled": true
        },
        {
            "elementRef": "w1:be:33333333333333333333333333333333",
            "role": "button",
            "name": "Save as",
            "text": "Save as",
            "enabled": false
        }
    ]);
    // 组合 query 输出。
    let query = format!(
        // 使用两行输出。
        "{}\n{}",
        // accepted 行。
        accepted("query"),
        // completed 行。
        completed(
            "query",
            4,
            json!({ "kind": "query", "matches": matches, "matchCount": 2, "truncated": false })
        )
    );
    // 多命中输出必须通过。
    assert!(observe_output(&query, REQUEST_NONCE, BrowserPageOperationKind::Query).is_ok());
    // 零命中同样是可信成功。
    let zero = format!(
        // 使用两行输出。
        "{}\n{}",
        // accepted 行。
        accepted("query"),
        // completed 行。
        completed(
            "query",
            4,
            json!({ "kind": "query", "matches": [], "matchCount": 0, "truncated": false })
        )
    );
    // 零命中必须通过。
    assert!(observe_output(&zero, REQUEST_NONCE, BrowserPageOperationKind::Query).is_ok());
}

// 验证 stale、未派发和未知结果组合。
#[test]
// 覆盖导航代际失效和双阶段不确定性。
fn stale_not_dispatched_and_unknown_outcomes_are_distinct() {
    // 构造 accepted 后 stale 确定失败。
    let stale = format!(
        // 使用两行输出。
        "{}\n{}",
        // accepted 行。
        accepted("click"),
        // final 行。
        serde_json::to_string(&json!({
            "kind": "command-final",
            "contractVersion": CONTRACT_VERSION,
            "requestNonce": REQUEST_NONCE,
            "operation": "click",
            "outcome": "failed",
            "completed": true,
            "retrySafe": false,
            "acceptedMayHaveOccurred": true,
            "navigationGeneration": 5,
            "data": Value::Null,
            "error": { "code": "STALE_ELEMENT", "message": "The element belongs to an older navigation generation." }
        }))
        // fixture 必须可序列化。
        .unwrap_or_else(|error| panic!("stale serialization failed: {error}"))
    );
    // stale 形状合法。
    let stale_observation = observe_output(&stale, REQUEST_NONCE, BrowserPageOperationKind::Click)
        // 合法输出不得失败。
        .unwrap_or_else(|error| panic!("stale output failed: {error}"));
    // outcome 必须确定失败。
    assert_eq!(
        stale_observation
            // 借用 final。
            .final_observation()
            // final 必须存在。
            .unwrap_or_else(|| panic!("stale final missing"))
            // 读取 outcome。
            .outcome(),
        BrowserPageOutcome::Failed
    );
    // stale 不得携带成功数据。
    assert!(
        stale_observation
            // 借用 final。
            .final_observation()
            // final 必须存在。
            .unwrap_or_else(|| panic!("stale final missing"))
            // 读取成功数据。
            .data()
            // 确认不存在。
            .is_none()
    );
    // stale 必须携带安全错误。
    assert_eq!(
        stale_observation
            // 借用 final。
            .final_observation()
            // final 必须存在。
            .unwrap_or_else(|| panic!("stale final missing"))
            // 读取错误。
            .error()
            // 读取错误码。
            .and_then(|error| error.get("code"))
            // 转换为字符串。
            .and_then(Value::as_str),
        // 核对固定 stale 错误。
        Some("STALE_ELEMENT")
    );
    // 构造 accepted-only 观察。
    let partial = observe_output(
        // 只传 accepted。
        &accepted("type"),
        // 关联 nonce。
        REQUEST_NONCE,
        // 关联操作。
        BrowserPageOperationKind::Type,
    )
    // accepted-only 自身是合法部分事实。
    .unwrap_or_else(|error| panic!("partial output failed: {error}"));
    // 必须 accepted 且没有 final。
    assert!(partial.accepted() && partial.final_observation().is_none());
    // 构造未派发 final。
    let not_dispatched = serde_json::to_string(&json!({
        "kind": "command-final",
        "contractVersion": CONTRACT_VERSION,
        "requestNonce": REQUEST_NONCE,
        "operation": "wait",
        "outcome": "not-dispatched",
        "completed": true,
        "retrySafe": true,
        "acceptedMayHaveOccurred": false,
        "navigationGeneration": 4,
        "data": Value::Null,
        "error": { "code": "STALE_PAGE", "message": "The page reference is stale." }
    }))
    // fixture 必须可序列化。
    .unwrap_or_else(|error| panic!("not-dispatched serialization failed: {error}"));
    // 未派发不要求 accepted。
    assert!(
        observe_output(
            &not_dispatched,
            REQUEST_NONCE,
            BrowserPageOperationKind::Wait
        )
        .is_ok()
    );
}

// 验证关联、顺序、数据种类和输出上限失败闭合。
#[test]
// 覆盖协议污染路径。
fn output_correlation_order_kind_and_budget_are_strict() {
    // query accepted 不得配 click final。
    let drift = format!(
        // 使用两行输出。
        "{}\n{}",
        // query accepted。
        accepted("query"),
        // click data final。
        completed("click", 3, json!({ "kind": "click", "clicked": true }))
    );
    // 操作漂移必须拒绝。
    assert!(observe_output(&drift, REQUEST_NONCE, BrowserPageOperationKind::Query).is_err());
    // final 后追加帧必须拒绝。
    let duplicate = format!(
        // 使用三行输出。
        "{}\n{}\n{}",
        // accepted。
        accepted("wait"),
        // completed。
        completed("wait", 3, json!({ "kind": "wait", "conditionMet": true })),
        // 重复 final。
        completed("wait", 3, json!({ "kind": "wait", "conditionMet": true }))
    );
    // 重复终态拒绝。
    assert!(observe_output(&duplicate, REQUEST_NONCE, BrowserPageOperationKind::Wait).is_err());
    // 输出预算必须为 16 MiB Base64 另加 128 KiB envelope。
    assert_eq!(MAXIMUM_OUTPUT_BYTES, (16 * 1024 * 1024) + (128 * 1024));
    // 超限输出返回专用错误码。
    let oversized = "x".repeat(MAXIMUM_OUTPUT_BYTES.saturating_add(1));
    // 核对输出过大类别。
    assert_eq!(
        observe_output(
            &oversized,
            REQUEST_NONCE,
            BrowserPageOperationKind::Screenshot
        )
        // 必须失败。
        .err()
        // 取得错误。
        .unwrap_or_else(|| panic!("oversized output must fail"))
        // 读取类别。
        .code(),
        BrowserPageProtocolErrorCode::OutputTooLarge
    );
}

// 验证 schema 保持冻结操作、身份和危险字段停止线。
#[test]
// 对 schema 源文本执行无网络结构断言。
fn schema_keeps_page_protocol_boundaries() {
    // schema 必须是合法 JSON。
    let schema = serde_json::from_str::<Value>(SCHEMA)
        // schema 漂移时提供明确诊断。
        .unwrap_or_else(|error| panic!("page schema failed: {error}"));
    // 固定顶层 oneOf 必须包含四类输入输出。
    assert_eq!(
        schema
            // 读取 oneOf。
            .get("oneOf")
            // 转换数组。
            .and_then(Value::as_array)
            // 读取长度。
            .map(Vec::len),
        // command、cancel、accepted、final。
        Some(4)
    );
    // schema 文本必须包含六种操作。
    for operation in ["navigate", "wait", "query", "click", "type", "screenshot"] {
        // 每个固定操作都必须出现。
        assert!(SCHEMA.contains(&format!("\"{operation}\"")));
    }
    // schema 不得引入原生或脚本字段。
    for forbidden in [
        // CDP target。
        "targetId",
        // DOM node。
        "nodeId",
        // CSS selector。
        "css",
        // XPath selector。
        "xpath",
        // 脚本。
        "script",
        // Cookie。
        "cookie",
        // 用户 profile。
        "profilePath",
    ] {
        // 危险字段不得出现。
        assert!(!SCHEMA.contains(&format!("\"{forbidden}\"")));
    }
}
