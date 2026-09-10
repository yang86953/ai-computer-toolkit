//! 验证持久窗口代际协议、状态机符号与未接线停止线。

// 导入 JSON 值。
use serde_json::Value;

// 嵌入内部协议 Schema。
const SCHEMA: &str = include_str!("../contracts/internal/window-generation-broker-v1.schema.json");
// 嵌入严格 parser 源码。
const PROTOCOL_SOURCE: &str = include_str!("../src/components/window_generation_protocol.rs");
// 嵌入领域 registry 源码。
const REGISTRY_SOURCE: &str = include_str!("../src/modules/window_generation_registry.rs");
// 嵌入当前公开身份强度 Component。
const PUBLIC_IDENTITY_SOURCE: &str = include_str!("../src/components/window_target_identity.rs");
// 嵌入当前生产窗口 Adapter。
const WINDOWS_ADAPTER: &str = include_str!("../src/adapters/windows.rs");

// 解析嵌入 JSON 并保留固定诊断。
fn parse(source: &str, context: &str) -> Value {
    // Schema 必须是合法 JSON。
    serde_json::from_str(source)
        // 失败时提供契约名称。
        .unwrap_or_else(|error| panic!("{context} must parse: {error}"))
}

// 验证私有 Schema 严格冻结版本、操作、预算与非数字 64 位事实。
#[test]
fn schema_freezes_closed_wire_and_fixed_budgets() {
    // 解析 Schema。
    let schema = parse(SCHEMA, "window generation broker schema");
    // 固定 Schema ID。
    assert_eq!(
        schema["$id"],
        // 使用仓库本地稳定 URL。
        "https://local.ai-computer-toolkit/contracts/internal/window-generation-broker-v1.schema.json"
    );
    // 根只接受五类封闭 frame。
    assert_eq!(
        schema["oneOf"]
            // 必须是数组。
            .as_array()
            // 缺失时失败。
            .unwrap_or_else(|| panic!("oneOf must be an array"))
            // 读取长度。
            .len(),
        // request/ready/health/resolve/rejection。
        5
    );
    // 请求拒绝未知字段。
    assert_eq!(schema["$defs"]["request"]["additionalProperties"], false);
    // 固定两个操作。
    assert_eq!(
        schema["$defs"]["request"]["properties"]["operation"]["enum"],
        // 保持 health 与 resolve-snapshot。
        serde_json::json!(["health", "resolve-snapshot"])
    );
    // 快照预算固定 16,384。
    assert_eq!(
        schema["$defs"]["request"]["properties"]["windows"]["maxItems"],
        // 不允许无界 frame。
        16_384
    );
    // token 使用固定十六进制而非 JSON number。
    assert_eq!(
        schema["$defs"]["fact64"]["pattern"],
        // 固定完整 64 位宽度。
        "^[0-9a-f]{16}$"
    );
    // live registry 固定容量。
    assert_eq!(
        schema["$defs"]["healthResult"]["properties"]["liveWindows"]["maximum"],
        // 与 Module 常量一致。
        65_536
    );
}

// 验证状态机冻结三阶段、序列屏障与全部关键 poison 原因。
#[test]
fn registry_source_owns_barrier_generation_and_fail_closed_states() {
    // registry 必须拥有三个领域阶段。
    for phase in ["Bootstrapping", "Live", "Poisoned"] {
        // 任一阶段缺失都使生命周期不完整。
        assert!(REGISTRY_SOURCE.contains(phase));
    }
    // registry 必须覆盖关键连续性失败。
    for reason in [
        // 事件序列缺口。
        "EventSequenceGap",
        // 固定队列溢出。
        "EventQueueOverflow",
        // bootstrap 验证不一致。
        "SnapshotBarrierMismatch",
        // live 重复 create。
        "DuplicateLiveCreate",
        // 未知 destroy。
        "UnknownLiveDestroy",
        // 容量耗尽。
        "RegistryCapacityExhausted",
    ] {
        // 任一缺失都表示 fail-closed 矩阵不完整。
        assert!(REGISTRY_SOURCE.contains(reason));
    }
    // 身份材料必须同时绑定随机 epoch 与 owner generation。
    assert!(REGISTRY_SOURCE.split_whitespace().collect::<String>().contains("self.broker_epoch.as_str(),self.owner_generation"));
    // protocol parser 必须拒绝未知字段。
    assert!(PROTOCOL_SOURCE.contains("deny_unknown_fields"));
    // protocol parser 必须区分 windows:null 与缺失。
    assert!(PROTOCOL_SOURCE.contains("is_some_and(Value::is_null)"));
}

// 保留生产身份强度和窗口 adapter 的未接线证据。
#[test]
fn public_identity_preserves_generation_owner_gap() {
    // 公开身份强度仍必须报告 owner 缺失。
    assert!(PUBLIC_IDENTITY_SOURCE.contains("\"generationOwner\": \"none\""));
    // 生产窗口 Adapter 尚不得引用未实现 registry。
    assert!(!WINDOWS_ADAPTER.contains("window_generation_registry"));
}
