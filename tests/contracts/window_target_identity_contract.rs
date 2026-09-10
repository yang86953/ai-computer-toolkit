//! 验证窗口目标身份强度、公开投影与重建矩阵停止线。

// 导入有序集合以核对严格字段集合。
use std::collections::BTreeSet;

// 导入 JSON 值。
use serde_json::Value;

// 嵌入窗口身份强度 Schema。
const IDENTITY_SCHEMA: &str = include_str!("../../contracts/v1/window-target-identity.schema.json");
// 嵌入只读窗口发现 Schema。
const WINDOW_SCHEMA: &str = include_str!("../../contracts/v1/window-observation.schema.json");
// 嵌入统一 app session Schema。
const APP_SCHEMA: &str = include_str!("../../contracts/v1/application-session-discovery.schema.json");
// 嵌入窗口生命周期结果 Schema。
const LIFECYCLE_SCHEMA: &str = include_str!("../../contracts/v1/window-lifecycle-result.schema.json");
// 嵌入特殊窗口矩阵快照。
const COMPATIBILITY_MATRIX: &str =
    include_str!("../../contracts/v1/special-window-input-compatibility.json");
// 嵌入生产身份材料调用点。
const WINDOWS_ADAPTER: &str = include_str!("../../src/adapters/windows.rs");
// 嵌入统一 app 窗口 provider 投影。
const APP_WINDOW_PROVIDER: &str = include_str!("../../src/adapters/app/desktop.rs");
// 嵌入直接 window surface 投影。
const WINDOW_ADAPTER: &str = include_str!("../../src/adapters/window.rs");
// 嵌入 capability assessment 投影。
const ASSESSMENT_MODULE: &str = include_str!("../../src/modules/capability_assessment.rs");
// 嵌入窗口生命周期 mutation 投影。
const LIFECYCLE_MODULE: &str = include_str!("../../src/modules/window_lifecycle.rs");

// 解析嵌入 JSON 并提供固定诊断。
fn parse(source: &str, context: &str) -> Value {
    // 契约文件必须是合法 JSON。
    serde_json::from_str(source)
        // 解析失败时保留契约名称。
        .unwrap_or_else(|error| panic!("{context} must parse: {error}"))
}

// 把 JSON 字符串数组转换为有序集合。
fn string_set(value: &Value, context: &str) -> BTreeSet<String> {
    // 读取数组形状。
    value
        // 只接受数组。
        .as_array()
        // 缺失时提供固定诊断。
        .unwrap_or_else(|| panic!("{context} must be an array"))
        // 遍历全部字符串。
        .iter()
        // 每项必须是字符串。
        .map(|value| {
            // 读取字符串并建立所有权。
            value
                // 只接受字符串。
                .as_str()
                // 非字符串时失败。
                .unwrap_or_else(|| panic!("{context} item must be text"))
                // 建立集合所有权。
                .to_owned()
        })
        // 收集去重有序集合。
        .collect()
}

// 验证身份强度 Schema 严格冻结七项 provider-neutral 事实。
#[test]
fn identity_schema_is_closed_and_embedded_by_window_surfaces() {
    // 解析身份 Schema。
    let identity = parse(IDENTITY_SCHEMA, "window identity schema");
    // 固定版本化 Schema ID。
    assert_eq!(
        identity["$id"],
        // 使用仓库本地稳定 URL。
        "https://local.ai-computer-toolkit/contracts/v1/window-target-identity.schema.json"
    );
    // 根对象必须拒绝未知字段。
    assert_eq!(identity["additionalProperties"], false);
    // 固定七个必需字段。
    let required = string_set(&identity["required"], "identity required");
    // 与完整字段集合比较。
    assert_eq!(
        required,
        // 构造逐字字段集合。
        [
            // 版本。
            "contractVersion",
            // 保证类别。
            "assurance",
            // 进程复用结论。
            "processReuse",
            // 同时存活窗口结论。
            "simultaneouslyLiveWindows",
            // 同进程 token 回收结论。
            "sameProcessRecycledWindowToken",
            // 代际 owner 状态。
            "generationOwner",
            // mutation 停止线。
            "mutationStopline",
        ]
        // 转换为迭代器。
        .into_iter()
        // 建立拥有型字符串。
        .map(str::to_owned)
        // 收集有序集合。
        .collect()
    );
    // 必须明确同进程 token 回收尚无保证。
    assert_eq!(
        identity["properties"]["sameProcessRecycledWindowToken"]["const"],
        // 使用保守结论。
        "not-guaranteed"
    );
    // 当前 generation owner 必须明确缺失。
    assert_eq!(identity["properties"]["generationOwner"]["const"], "none");
    // mutation 停止线必须要求平台生命周期绑定的原子 dispatch。
    assert_eq!(
        identity["properties"]["mutationStopline"]["const"],
        // 不把异步历史 owner 误报为充分条件。
        "lifetime-bound-atomic-dispatch-unavailable-for-arbitrary-windows"
    );
    // 解析直接窗口 Schema。
    let window = parse(WINDOW_SCHEMA, "window observation schema");
    // 每个窗口 session 必须要求身份强度。
    assert!(
        string_set(
            &window["properties"]["sessions"]["items"]["required"],
            // 提供数组上下文。
            "window session required"
        )
        // 查找目标字段。
        .contains("targetIdentityStrength")
    );
    // 直接窗口 Schema 必须引用单一身份 Schema。
    assert_eq!(
        window["properties"]["sessions"]["items"]["properties"]["targetIdentityStrength"]["$ref"],
        // 使用相对契约引用。
        "window-target-identity.schema.json"
    );
    // 解析统一 app Schema。
    let app = parse(APP_SCHEMA, "application session schema");
    // app 窗口 session 同样引用单一身份 Schema。
    assert_eq!(
        app["$defs"]["session"]["properties"]["targetIdentityStrength"]["$ref"],
        // 使用相同相对契约引用。
        "window-target-identity.schema.json"
    );
    // kind=window 条件必须要求身份强度。
    assert_eq!(
        app["$defs"]["session"]["allOf"][0]["then"]["required"],
        // 固定单字段条件要求。
        serde_json::json!(["targetIdentityStrength"])
    );
    // 解析窗口生命周期结果 Schema。
    let lifecycle = parse(LIFECYCLE_SCHEMA, "window lifecycle result schema");
    // mutation 成功结果必须要求同一身份强度对象。
    assert!(
        string_set(
            &lifecycle["properties"]["data"]["required"],
            // 提供数组上下文。
            "window lifecycle data required"
        )
        // 查找目标字段。
        .contains("targetIdentityStrength")
    );
    // mutation Schema 必须引用单一身份 Schema。
    assert_eq!(
        lifecycle["properties"]["data"]["properties"]["targetIdentityStrength"]["$ref"],
        // 使用相同相对契约引用。
        "window-target-identity.schema.json"
    );
    // 旧的绝对窗口代际声明不得继续存在。
    assert!(!LIFECYCLE_SCHEMA.contains("sameWindowGenerationVerified"));
}

// 验证生产发现、统一 provider 与 assessment 都投影同一强度对象。
#[test]
fn production_boundaries_use_one_identity_strength_component() {
    // 私有材料必须由窄 Component 单点构造。
    assert!(WINDOWS_ADAPTER.contains("window_target_identity::material"));
    // 直接 window surface 必须投影强度。
    assert!(WINDOW_ADAPTER.contains(
        // 查找同一 Component 调用。
        "\"targetIdentityStrength\": window_target_identity::public_assurance()"
    ));
    // 统一 app 窗口 provider 必须投影相同强度。
    assert!(APP_WINDOW_PROVIDER.contains(
        // 查找相同字段与 Component。
        "object.insert(\"targetIdentityStrength\".to_owned(), window_target_identity::public_assurance())"
    ));
    // capability assessment 必须把强度放在公开 evidence。
    assert!(ASSESSMENT_MODULE.contains(
        // 查找窗口条件投影字段。
        "\"targetIdentityStrength\": matches!(target_kind"
    ));
    // 窗口生命周期 mutation 必须投影相同强度对象。
    assert!(LIFECYCLE_MODULE.contains(
        // 查找同一 Component 调用。
        "\"targetIdentityStrength\": window_target_identity::public_assurance()"
    ));
    // mutation 结果不得继续输出绝对窗口代际声明。
    assert!(!LIFECYCLE_MODULE.contains("sameWindowGenerationVerified"));
    // 生产材料注释不得继续声称绝对抵抗窗口 handle 复用。
    assert!(!WINDOWS_ADAPTER.contains("抵抗句柄和 PID 复用"));
}

// 验证一般窗口重建保持 gap，distinct-token 动态证据仍被保留。
#[test]
fn recreated_window_matrix_separates_distinct_token_evidence_from_general_gap() {
    // 解析特殊窗口矩阵。
    let matrix = parse(COMPATIBILITY_MATRIX, "special compatibility matrix");
    // 查找窗口重建 entry。
    let recreated = matrix["entries"]
        // 读取数组。
        .as_array()
        // 缺失时失败。
        .unwrap_or_else(|| panic!("matrix entries must exist"))
        // 查找逐字状态。
        .iter()
        // 选择重建状态。
        .find(|entry| entry["state"] == "window-recreated")
        // 缺失时失败。
        .unwrap_or_else(|| panic!("window-recreated entry must exist"));
    // 新窗口发现仍有 distinct-token 证据。
    assert_eq!(recreated["capabilities"]["discoverable"], "supported");
    // 一般观察不得从 distinct-token fixture 外推。
    assert_eq!(recreated["capabilities"]["observable"], "gap");
    // 一般语义动作保持 gap。
    assert_eq!(recreated["capabilities"]["semanticAction"], "gap");
    // 一般指针操作保持 gap。
    assert_eq!(recreated["capabilities"]["pointerInput"], "gap");
    // 一般键盘操作保持 gap。
    assert_eq!(recreated["capabilities"]["keyboardInput"], "gap");
    // 结构化失败覆盖同样不得冒充完整。
    assert_eq!(recreated["structuredFailure"]["conclusion"], "gap");
    // 证据必须同时记录动态窄结论与一般身份缺口。
    let references = recreated["evidence"]
        // 读取证据数组。
        .as_array()
        // 缺失时失败。
        .unwrap_or_else(|| panic!("recreated evidence must exist"))
        // 读取引用。
        .iter()
        // 提取字符串引用。
        .filter_map(|evidence| evidence["reference"].as_str())
        // 收集有序集合。
        .collect::<BTreeSet<_>>();
    // 保留 #2324 distinct-token 生产纵切。
    assert!(references.iter().any(|reference| {
        // 查找动态重建测试。
        reference.contains("recreated_window_gets_new_identity")
    }));
    // 增加 #2329 一般身份强度契约。
    assert!(references.contains("contracts/v1/window-target-identity.md"));
}
