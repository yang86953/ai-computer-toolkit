//! 验证特殊窗口与输入兼容矩阵的版本化 provider-neutral 契约。

// 导入有序集合以验证八类状态唯一且完整。
use std::collections::BTreeSet;

// 导入 JSON 对象和值类型。
use serde_json::{Map, Value};

// 嵌入版本化 JSON Schema。
const SCHEMA: &str = include_str!("../../contracts/v1/special-window-input-compatibility.schema.json");
// 嵌入当前证据快照。
const MATRIX: &str = include_str!("../../contracts/v1/special-window-input-compatibility.json");

// 固定八类特殊窗口与输入状态。
const STATES: [&str; 8] = [
    // 自绘窗口。
    "custom-drawn",
    // Raw Input 窗口。
    "raw-input",
    // 独占全屏窗口。
    "exclusive-fullscreen",
    // 最小化窗口。
    "minimized",
    // 隐藏窗口。
    "hidden",
    // 重建后窗口。
    "window-recreated",
    // 混合 DPI 环境。
    "mixed-dpi",
    // 多显示器环境。
    "multi-monitor",
];
// 固定五个 capability 结论维度。
const CAPABILITY_DIMENSIONS: [&str; 5] = [
    // 公开发现。
    "discoverable",
    // 公开观察。
    "observable",
    // provider-neutral 语义动作。
    "semanticAction",
    // 指针输入。
    "pointerInput",
    // 键盘输入。
    "keyboardInput",
];
// 固定六种结论。
const CONCLUSIONS: [&str; 6] = [
    // 已有窄机器证据。
    "supported",
    // 被精确门禁阻止。
    "blocked",
    // 当前状态不可认证。
    "unavailable",
    // 公开契约不承诺。
    "unsupported",
    // 仍缺证据。
    "gap",
    // 只允许具名主人验收。
    "human-gate",
];
// 固定三种证据来源。
const EVIDENCE_KINDS: [&str; 3] = [
    // Rust 契约或定向 Rust 回归。
    "rust-contract",
    // 原样生产 launcher fixture。
    "production-launcher-fixture",
    // 具名人类门禁。
    "human-gate",
];

// 解析嵌入 JSON 并提供固定诊断。
fn parse(source: &str, name: &str) -> Value {
    // 契约文件必须是合法 JSON。
    serde_json::from_str(source).unwrap_or_else(|error| panic!("{name} must parse: {error:?}"))
}

// 读取必须存在的 JSON object。
fn object<'a>(value: &'a Value, context: &str) -> &'a Map<String, Value> {
    // 非对象表示契约形状漂移。
    value
        // 只接受对象。
        .as_object()
        // 缺失时提供固定上下文。
        .unwrap_or_else(|| panic!("{context} must be an object"))
}

// 验证 JSON object 字段集合逐字闭合。
fn assert_exact_keys(object: &Map<String, Value>, expected: &[&str]) {
    // 字段数量必须精确一致。
    assert_eq!(object.len(), expected.len());
    // 每个实际字段都必须来自冻结集合。
    assert!(object.keys().all(|key| expected.contains(&key.as_str())));
}

// 从矩阵中按固定状态读取唯一 entry。
fn entry<'a>(matrix: &'a Value, state: &str) -> &'a Value {
    // 读取 entries 数组。
    matrix["entries"]
        // entries 必须是数组。
        .as_array()
        // 缺失数组必须失败。
        .unwrap_or_else(|| panic!("entries must be an array"))
        // 遍历查找逐字状态。
        .iter()
        // 只接受唯一固定 state。
        .find(|entry| entry["state"].as_str() == Some(state))
        // 八类状态缺一不可。
        .unwrap_or_else(|| panic!("missing state {state}"))
}

// 验证 Schema 冻结版本、严格对象和八类状态。
#[test]
fn schema_freezes_version_states_dimensions_and_evidence_kinds() {
    // 解析版本化 Schema。
    let schema = parse(SCHEMA, "special compatibility schema");
    // Schema ID 必须固定到 v1 文件。
    assert_eq!(
        // 读取 Schema ID。
        schema["$id"].as_str(),
        // 比较稳定本地契约 URL。
        Some(
            "https://local.ai-computer-toolkit/contracts/v1/special-window-input-compatibility.schema.json"
        ),
    );
    // 根对象必须拒绝额外字段。
    assert_eq!(schema["additionalProperties"], Value::Bool(false));
    // entry 同样必须拒绝额外字段。
    assert_eq!(
        schema["$defs"]["entry"]["additionalProperties"],
        Value::Bool(false)
    );
    // 状态枚举必须逐字等于八类集合。
    let states = schema["$defs"]["entry"]["properties"]["state"]["enum"]
        // 读取枚举数组。
        .as_array()
        // 缺失表示 Schema 漂移。
        .unwrap_or_else(|| panic!("state enum must exist"))
        // 转换为字符串集合。
        .iter()
        // 每项必须是字符串。
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("state must be text"))
        })
        // 收集有序集合便于比较。
        .collect::<BTreeSet<_>>();
    // 与固定八类状态比较。
    assert_eq!(states, STATES.into_iter().collect());
    // capability 维度必须逐字封闭。
    let dimensions = object(&schema["$defs"]["capabilities"]["properties"], "dimensions");
    // 核对五个必填维度。
    assert_exact_keys(dimensions, &CAPABILITY_DIMENSIONS);
    // 证据 kind 必须只允许三种来源。
    let kinds = schema["$defs"]["evidence"]["properties"]["kind"]["enum"]
        // 读取枚举数组。
        .as_array()
        // 缺失时失败。
        .unwrap_or_else(|| panic!("evidence kind enum must exist"))
        // 转换为字符串集合。
        .iter()
        // 每项必须是字符串。
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("kind must be text"))
        })
        // 收集有序集合。
        .collect::<BTreeSet<_>>();
    // 与固定三种来源比较。
    assert_eq!(kinds, EVIDENCE_KINDS.into_iter().collect());
}

// 验证当前矩阵八项完整、字段封闭且不泄漏平台私有目标。
#[test]
fn matrix_is_complete_closed_and_provider_neutral() {
    // 解析当前证据快照。
    let matrix = parse(MATRIX, "special compatibility matrix");
    // 根字段必须逐字闭合。
    assert_exact_keys(
        // 读取根对象。
        object(&matrix, "matrix"),
        // 对齐版本化快照字段。
        &[
            "contractVersion",
            "scope",
            "evidenceDate",
            "humanGates",
            "entries",
        ],
    );
    // contractVersion 必须固定。
    assert_eq!(
        matrix["contractVersion"],
        "act/special-window-input-compatibility/v1"
    );
    // human gate 必须保留 #2006。
    assert_eq!(matrix["humanGates"], serde_json::json!(["#2006"]));
    // entries 必须恰好八项。
    let entries = matrix["entries"]
        // 读取数组。
        .as_array()
        // 缺失时失败。
        .unwrap_or_else(|| panic!("entries must exist"));
    // 禁止重复或附加状态。
    assert_eq!(entries.len(), STATES.len());
    // 收集实际状态。
    let actual_states = entries
        // 遍历全部 entry。
        .iter()
        // state 必须是字符串。
        .map(|entry| {
            entry["state"]
                .as_str()
                .unwrap_or_else(|| panic!("state must be text"))
        })
        // 收集唯一集合。
        .collect::<BTreeSet<_>>();
    // 实际状态必须逐字完整。
    assert_eq!(actual_states, STATES.into_iter().collect());
    // 逐 entry 验证封闭维度与证据。
    for entry in entries {
        // entry 字段集合必须严格。
        assert_exact_keys(
            // 读取 entry 对象。
            object(entry, "entry"),
            // 对齐 Schema required 字段。
            &[
                "state",
                "capabilityIds",
                "capabilities",
                "foregroundRequirement",
                "structuredFailure",
                "evidence",
            ],
        );
        // capabilities 必须恰好五维。
        let capabilities = object(&entry["capabilities"], "capabilities");
        // 核对维度字段集合。
        assert_exact_keys(capabilities, &CAPABILITY_DIMENSIONS);
        // 每个结论必须来自封闭枚举。
        assert!(capabilities.values().all(|value| {
            value
                .as_str()
                .is_some_and(|value| CONCLUSIONS.contains(&value))
        }));
        // structured failure 也必须使用同一结论枚举。
        assert!(
            entry["structuredFailure"]["conclusion"]
                // 只接受字符串。
                .as_str()
                // 核对封闭枚举。
                .is_some_and(|value| CONCLUSIONS.contains(&value))
        );
        // 每项必须至少绑定一条允许证据。
        let evidence = entry["evidence"]
            // 读取证据数组。
            .as_array()
            // 缺失时失败。
            .unwrap_or_else(|| panic!("evidence must be an array"));
        // 空证据不得形成结论。
        assert!(!evidence.is_empty());
        // 逐条验证来源与隐私。
        for evidence in evidence {
            // evidence 字段必须严格。
            assert_exact_keys(
                object(evidence, "evidence"),
                &["kind", "reference", "claim"],
            );
            // kind 必须来自三种来源。
            assert!(
                evidence["kind"]
                    // 只接受字符串。
                    .as_str()
                    // 核对来源枚举。
                    .is_some_and(|value| EVIDENCE_KINDS.contains(&value))
            );
            // 引用与说明不得出现平台私有目标类别。
            let public_text =
                format!("{} {}", evidence["reference"], evidence["claim"]).to_ascii_lowercase();
            // 禁止 native handle 与 UIA 私有 identity。
            for forbidden in ["hwnd", "runtimeid", "native handle", "provider private"] {
                // 任一命中都表示矩阵泄漏实现细节。
                assert!(
                    !public_text.contains(forbidden),
                    "matrix leaked {forbidden}"
                );
            }
        }
    }
}

// 验证当前快照不会把尚未完成的真实兼容场景误报为 supported。
#[test]
fn snapshot_preserves_machine_gaps_and_named_human_gate() {
    // 解析当前证据快照。
    let matrix = parse(MATRIX, "special compatibility matrix");
    // 自绘窗口必须绑定真实截图与语义失败证据。
    let custom = entry(&matrix, "custom-drawn");
    // 固定自绘像素已经由原样生产 launcher 观察。
    assert_eq!(custom["capabilities"]["observable"], "supported");
    // 没有暴露语义节点的自绘内容保持当前不可用。
    assert_eq!(custom["capabilities"]["semanticAction"], "unavailable");
    // 自绘语义零命中必须使用正式错误码。
    assert!(
        custom["structuredFailure"]["codes"]
            // 读取错误码数组。
            .as_array()
            // 数组必须存在。
            .unwrap_or_else(|| panic!("custom-drawn codes must exist"))
            // 查找稳定元素缺失分类。
            .contains(&Value::String("ELEMENT_NOT_FOUND".to_owned()))
    );
    // 自绘证据必须绑定原样生产 launcher 动态测试。
    assert!(
        custom["evidence"]
            // 读取证据数组。
            .as_array()
            // 数组必须存在。
            .unwrap_or_else(|| panic!("custom-drawn evidence must exist"))
            // 查找精确动态测试引用。
            .iter()
            // 同时要求生产 launcher 证据类别。
            .any(|evidence| evidence["kind"] == "production-launcher-fixture"
                && evidence["reference"].as_str().is_some_and(|reference| {
                    // 绑定自绘纵切测试。
                    reference.contains("custom_drawn_window_is_observable")
                }))
    );
    // 自绘真实指针仍只由主人验收。
    assert_eq!(custom["capabilities"]["pointerInput"], "human-gate");
    // 自绘真实键盘仍只由主人验收。
    assert_eq!(custom["capabilities"]["keyboardInput"], "human-gate");
    // Raw Input 不得声称指针或键盘已通过。
    let raw = entry(&matrix, "raw-input");
    // 工具自有 Raw Input 顶层窗口已经可公开发现。
    assert_eq!(raw["capabilities"]["discoverable"], "supported");
    // 指针保持主人门禁。
    assert_eq!(raw["capabilities"]["pointerInput"], "human-gate");
    // 键盘保持主人门禁。
    assert_eq!(raw["capabilities"]["keyboardInput"], "human-gate");
    // 目标消费不可观测时不得伪造结构化失败码。
    assert_eq!(raw["structuredFailure"]["conclusion"], "unsupported");
    // Raw Input 必须绑定显式前景 production launcher fixture。
    assert!(
        raw["evidence"]
            // 读取证据数组。
            .as_array()
            // 数组必须存在。
            .unwrap_or_else(|| panic!("raw-input evidence must exist"))
            // 查找精确动态测试引用。
            .iter()
            // 要求生产 launcher 证据与测试名称同时匹配。
            .any(|evidence| evidence["kind"] == "production-launcher-fixture"
                && evidence["reference"].as_str().is_some_and(|reference| {
                    // 绑定 Raw Input 纵切测试。
                    reference.contains("production_send_input_is_not_raw_device_delivery")
                }))
    );
    // 独占全屏不得从当前环境不可用外推发现或观察支持。
    let fullscreen = entry(&matrix, "exclusive-fullscreen");
    // 发现仍是 gap。
    assert_eq!(fullscreen["capabilities"]["discoverable"], "gap");
    // 观察仍是 gap。
    assert_eq!(fullscreen["capabilities"]["observable"], "gap");
    // 独占全屏必须绑定真实 DXGI 进入尝试与生产发现纵切。
    assert!(
        fullscreen["evidence"]
            // 读取证据数组。
            .as_array()
            // 数组必须存在。
            .unwrap_or_else(|| panic!("exclusive-fullscreen evidence must exist"))
            // 查找精确动态测试引用。
            .iter()
            // 要求生产 launcher 证据与测试名称同时匹配。
            .any(|evidence| evidence["kind"] == "production-launcher-fixture"
                && evidence["reference"].as_str().is_some_and(|reference| {
                    // 绑定真实 DXGI 独占纵切测试。
                    reference.contains("production_discovery_and_capture_respect_exclusive")
                }))
    );
    // 最小化状态必须明确拒绝猜测观察和语义命中。
    let minimized = entry(&matrix, "minimized");
    // 观察当前不可用。
    assert_eq!(minimized["capabilities"]["observable"], "unavailable");
    // 语义动作当前不可用。
    assert_eq!(minimized["capabilities"]["semanticAction"], "unavailable");
    // 最小化必须绑定原样生产 launcher 动态证据。
    assert!(
        minimized["evidence"]
            // 读取证据数组。
            .as_array()
            // 数组必须存在。
            .unwrap_or_else(|| panic!("minimized evidence must exist"))
            // 查找精确动态测试引用。
            .iter()
            // 要求生产 launcher 证据与测试名称同时匹配。
            .any(|evidence| evidence["kind"] == "production-launcher-fixture"
                && evidence["reference"].as_str().is_some_and(|reference| {
                    // 绑定最小化纵切测试。
                    reference.contains("minimized_window_remains_discoverable")
                }))
    );
    // 隐藏状态必须使用正式公开错误码而非未实现别名。
    let hidden = entry(&matrix, "hidden");
    // 核对隐藏结构化失败包含正式分类。
    assert!(
        hidden["structuredFailure"]["codes"]
            // 读取错误码数组。
            .as_array()
            // 数组必须存在。
            .unwrap_or_else(|| panic!("hidden codes must exist"))
            // 查找稳定隐藏分类。
            .contains(&Value::String("CAPTURE_TARGET_HIDDEN".to_owned()))
    );
    // 隐藏必须绑定不重新显示的生产 launcher 动态证据。
    assert!(
        hidden["evidence"]
            // 读取证据数组。
            .as_array()
            // 数组必须存在。
            .unwrap_or_else(|| panic!("hidden evidence must exist"))
            // 查找精确动态测试引用。
            .iter()
            // 绑定隐藏纵切测试。
            .any(|evidence| evidence["reference"]
                .as_str()
                .is_some_and(|reference| reference.contains("hidden_window_is_not_reshown")))
    );
    // 窗口重建必须保留已实现 stale 错误集合。
    let recreated = entry(&matrix, "window-recreated");
    // 一般同进程 token 回收尚未由当前身份保证闭合。
    assert_eq!(recreated["capabilities"]["observable"], "gap");
    // 结构化失败覆盖不得从 distinct-token fixture 外推。
    assert_eq!(recreated["structuredFailure"]["conclusion"], "gap");
    // 旧 identity 必须失败关闭。
    assert!(
        recreated["structuredFailure"]["codes"]
            // 读取错误码数组。
            .as_array()
            // 数组必须存在。
            .unwrap_or_else(|| panic!("recreated codes must exist"))
            // 查找稳定 stale 分类。
            .contains(&Value::String("STALE_SESSION".to_owned()))
    );
    // 窗口重建必须绑定新旧 identity 动态证据。
    assert!(
        recreated["evidence"]
            // 读取证据数组。
            .as_array()
            // 数组必须存在。
            .unwrap_or_else(|| panic!("recreated evidence must exist"))
            // 查找精确动态测试引用。
            .iter()
            // 绑定窗口重建纵切测试。
            .any(|evidence| evidence["reference"]
                .as_str()
                .is_some_and(|reference| reference.contains("recreated_window_gets_new_identity")))
    );
    // 一般窗口 token 回收必须绑定身份强度契约。
    assert!(
        recreated["evidence"]
            // 读取证据数组。
            .as_array()
            // 数组必须存在。
            .unwrap_or_else(|| panic!("recreated evidence must exist"))
            // 查找身份强度契约。
            .iter()
            // 绑定固定契约路径。
            .any(|evidence| evidence["reference"] == "contracts/v1/window-target-identity.md")
    );
    // Mixed DPI 的真实指针仍只由主人验收。
    let mixed_dpi = entry(&matrix, "mixed-dpi");
    // 核对指针门禁不变。
    assert_eq!(mixed_dpi["capabilities"]["pointerInput"], "human-gate");
    // Mixed DPI 必须绑定真实当前显示拓扑纵切。
    assert!(
        mixed_dpi["evidence"]
            // 读取证据数组。
            .as_array()
            // 数组必须存在。
            .unwrap_or_else(|| panic!("mixed-dpi evidence must exist"))
            // 查找精确动态测试引用。
            .iter()
            // 绑定当前显示拓扑纵切。
            .any(
                |evidence| evidence["reference"].as_str().is_some_and(|reference| {
                    // 要求生产生命周期测试名称。
                    reference
                        .contains("production_window_lifecycle_matches_current_display_topology")
                })
            )
    );
    // 多显示器真实指针仍只由主人验收。
    let multi_monitor = entry(&matrix, "multi-monitor");
    // 核对指针门禁不变。
    assert_eq!(multi_monitor["capabilities"]["pointerInput"], "human-gate");
    // 多显示器必须绑定同一真实当前显示拓扑纵切。
    assert!(
        multi_monitor["evidence"]
            // 读取证据数组。
            .as_array()
            // 数组必须存在。
            .unwrap_or_else(|| panic!("multi-monitor evidence must exist"))
            // 查找精确动态测试引用。
            .iter()
            // 绑定当前显示拓扑纵切。
            .any(
                |evidence| evidence["reference"].as_str().is_some_and(|reference| {
                    // 要求生产生命周期测试名称。
                    reference
                        .contains("production_window_lifecycle_matches_current_display_topology")
                })
            )
    );
    // 人工验收边界由上面的矩阵断言验证，不依赖已移除的内部任务说明。
}
