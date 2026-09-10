//! application discovery Module 的稳定夹具与实机只读测试。

// 导入父 Module 私有测试入口。
use super::*;

// 导入有序集合以执行契约字段集门禁。
use std::collections::BTreeSet;

// 导入仅供 schema 夹具解析失败使用的公开错误 envelope。
use crate::domain::AppControlError;

// 构造稳定应用夹具。
fn fixture_application() -> InstalledApplicationRecord {
    // 返回仅含 opaque 公开目标与私有提示的记录。
    InstalledApplicationRecord {
        // 使用合法 canonical 应用目标。
        session_id: "s2:a:1111111111111111".to_owned(),
        // 保存公开显示名。
        display_name: "Fixture App".to_owned(),
        // 保存公开版本。
        version: "1.0".to_owned(),
        // 保存公开发布者。
        publisher: "Fixture Publisher".to_owned(),
        // 保存公开来源。
        discovery_sources: vec![
            // 保留传统目录来源。
            "registry-uninstall".to_owned(),
            // 标记私有 identity 来自公开 AppsFolder。
            "shell-apps-folder".to_owned(),
        ],
        // 精确匹配 fixture.exe。
        process_match_hints: vec!["fixture".to_owned()],
        // 保存不得公开的私有启动 identity。
        launch_identity: "shell:private-aumid".to_owned(),
    }
}

// 构造稳定进程夹具。
fn fixture_process() -> ProcessRecord {
    // 返回带单一窗口关系的进程。
    ProcessRecord {
        // 使用合法 canonical 进程目标。
        session_id: "s2:p:2222222222222222".to_owned(),
        // 与应用提示精确匹配。
        process_name: "fixture.exe".to_owned(),
        // 标记生命周期身份可靠。
        identity_reliable: true,
        // 标记元数据可用。
        metadata_access: ProcessMetadataAccess::Available,
        // 标记相同完整性。
        integrity_relation: IntegrityRelation::Same,
        // 保存窗口关系。
        window_session_ids: vec!["s2:w:3333333333333333".to_owned()],
        // 保存私有 PID。
        process_id: 42,
        // 保存私有创建时间。
        process_creation_time: 123,
    }
}

// 构造稳定窗口夹具。
fn fixture_window() -> WindowRecord {
    // 返回只供 render 使用的私有窗口事实。
    WindowRecord {
        // legacy 字段禁止进入 Module 输出。
        session_id: "window:4660".to_owned(),
        // 保存私有 HWND。
        hwnd: 4660,
        // 保存非空公开标题。
        title: "Fixture Window".to_owned(),
        // 保存不得公开的窗口类名。
        class_name: "PrivateClass".to_owned(),
        // 与进程夹具 PID 对齐。
        process_id: 42,
        // 保存公开安全进程名。
        process_name: Some("fixture.exe".to_owned()),
        // 标记窗口可见。
        visible: true,
        // 与 opaque 窗口 golden 输入对齐。
        process_creation_time: 123,
    }
}

// 构造完整稳定快照。
fn fixture_snapshot() -> InventorySnapshot {
    // 返回所有来源完整且前景未变的快照。
    InventorySnapshot {
        // 使用合法 canonical host 目标。
        host_session_id: "s2:h:4444444444444444".to_owned(),
        // 保存应用夹具。
        applications: vec![fixture_application()],
        // 保存进程夹具。
        processes: vec![fixture_process()],
        // 保存窗口夹具。
        windows: vec![fixture_window()],
        // 标记 Registry 可用。
        registry_source_available: true,
        // 标记 Shell 可用。
        shell_source_available: true,
        // 标记固定 Start Menu 可用。
        start_menu_source_available: true,
        // 标记应用完整。
        applications_complete: true,
        // 标记进程完整。
        processes_complete: true,
        // 标记窗口完整。
        windows_complete: true,
        // 标记前景未变。
        foreground_unchanged: true,
    }
}

// 递归检查禁止字段名。
fn contains_key(value: &Value, forbidden: &str) -> bool {
    // 按 JSON 类型递归扫描。
    match value {
        // 对象检查当前键及所有子值。
        Value::Object(object) => {
            // 当前或子树任一命中即返回真。
            object.contains_key(forbidden)
                // 扫描全部子值。
                || object
                    // 取得值迭代器。
                    .values()
                    // 递归检查。
                    .any(|child| contains_key(child, forbidden))
        }
        // 数组扫描全部元素。
        Value::Array(values) => values
            // 取得迭代器。
            .iter()
            // 递归检查。
            .any(|child| contains_key(child, forbidden)),
        // 标量没有字段名。
        _ => false,
    }
}

// 核对公开对象字段集与 schema required 集合完全一致。
fn assert_required_keys(value: &Value, required: &Value) {
    // 收集实际对象键。
    let actual = value
        // 要求对象形状。
        .as_object()
        // 测试夹具固定为对象。
        .into_iter()
        // 遍历键。
        .flat_map(|object| object.keys())
        // 转换为字符串切片。
        .map(String::as_str)
        // 收集稳定集合。
        .collect::<BTreeSet<_>>();
    // 收集 schema required 字段。
    let expected = required
        // 要求数组形状。
        .as_array()
        // 测试 schema 固定为数组。
        .into_iter()
        // 展开元素。
        .flatten()
        // 读取字符串。
        .filter_map(Value::as_str)
        // 收集稳定集合。
        .collect::<BTreeSet<_>>();
    // additionalProperties=false 下两侧必须完全一致。
    assert_eq!(actual, expected);
}

// 验证精确关系、完整 schema 形状与私有事实隔离。
#[test]
fn fixture_graph_is_related_bounded_and_private() -> AppResult<()> {
    // 渲染稳定快照。
    let value = render(fixture_snapshot())?;
    // 验证 envelope 版本与实现。
    assert_eq!(value["implementation"], "rust");
    // 验证双向关系。
    assert_eq!(
        value["data"]["applications"][0]["runningProcessSessionIds"][0],
        "s2:p:2222222222222222"
    );
    // 验证反向关系。
    assert_eq!(
        value["data"]["processes"][0]["relatedApplicationIds"][0],
        "s2:a:1111111111111111"
    );
    // 验证运行与匹配状态。
    assert_eq!(value["data"]["applications"][0]["state"], "running");
    // 验证精确 Shell 目标发布确认型启动能力。
    assert_eq!(
        // 读取统一目录中的启动能力分类。
        value["data"]["applications"][0]["launchCapability"],
        // 认证只表示可在确认后执行。
        "available-confirmed"
    );
    // 验证窗口只关联 opaque 进程目标。
    assert_eq!(
        value["data"]["windows"][0]["processSessionId"],
        "s2:p:2222222222222222"
    );
    // 验证计数来自实际数组。
    assert_eq!(value["data"]["counts"]["runningProcesses"], 1);
    // 扫描全部禁止原生字段。
    for forbidden in [
        // 禁止 HWND。
        "hwnd",
        // 禁止 PID。
        "processId",
        // 禁止类名。
        "className",
        // 禁止路径。
        "path",
        // 禁止 AUMID。
        "aumid",
        // 禁止 provider identity。
        "providerId",
        // 禁止创建时间。
        "creationTime",
    ] {
        // 任一禁止字段都不得存在。
        assert!(!contains_key(&value, forbidden));
    }
    // 序列化后检查私有值未泄漏。
    let text = value.to_string();
    // 禁止 legacy HWND 目标、类名与启动 identity。
    assert!(!text.contains("window:4660"));
    // 禁止窗口类名。
    assert!(!text.contains("PrivateClass"));
    // 禁止 Shell parsing identity。
    assert!(!text.contains("private-aumid"));
    // 完成夹具测试。
    Ok(())
}

// 验证实际公开字段集与版本化 schema 完全一致。
#[test]
fn fixture_graph_matches_versioned_schema_field_sets() -> AppResult<()> {
    // 编译时嵌入并解析 schema，阻止无效 JSON 合入。
    let schema: Value = serde_json::from_str(include_str!(
        // 从 Module 目录解析正式契约。
        "../../contracts/v1/application-inventory.schema.json"
    ))
    // 将解析失败转为结构化测试错误。
    .map_err(|error| AppControlError::new("TEST_SCHEMA_INVALID", error.to_string()))?;
    // 渲染稳定公开夹具。
    let value = render(fixture_snapshot())?;
    // 核对顶层字段集。
    assert_required_keys(&value, &schema["required"]);
    // 核对 data 字段集。
    assert_required_keys(&value["data"], &schema["properties"]["data"]["required"]);
    // 核对 coverage 字段集。
    assert_required_keys(
        &value["data"]["coverage"],
        &schema["$defs"]["coverage"]["required"],
    );
    // 核对 complete 字段集。
    assert_required_keys(
        &value["data"]["complete"],
        &schema["$defs"]["complete"]["required"],
    );
    // 核对 counts 字段集。
    assert_required_keys(
        &value["data"]["counts"],
        &schema["$defs"]["counts"]["required"],
    );
    // 核对应用字段集。
    assert_required_keys(
        &value["data"]["applications"][0],
        &schema["$defs"]["application"]["required"],
    );
    // 核对进程字段集。
    assert_required_keys(
        &value["data"]["processes"][0],
        &schema["$defs"]["process"]["required"],
    );
    // 核对窗口字段集。
    assert_required_keys(
        &value["data"]["windows"][0],
        &schema["$defs"]["window"]["required"],
    );
    // Rust 必须属于 schema 明确允许的实现集合。
    assert!(
        schema["properties"]["implementation"]["enum"]
            // 读取实现数组。
            .as_array()
            // 检查 rust 值。
            .is_some_and(|values| values.contains(&json!("rust")))
    );
    // 完成 schema 字段集门禁。
    Ok(())
}

// 验证无精确提示的进程保持 unassociated。
#[test]
fn unmatched_process_is_not_guessed() -> AppResult<()> {
    // 构造快照。
    let mut snapshot = fixture_snapshot();
    // 移除应用的精确提示。
    snapshot.applications[0].process_match_hints.clear();
    // 渲染快照。
    let value = render(snapshot)?;
    // 应用保持 installed。
    assert_eq!(value["data"]["applications"][0]["state"], "installed");
    // 进程保持 unassociated。
    assert_eq!(
        value["data"]["processes"][0]["relationshipStatus"],
        "unassociated"
    );
    // 聚合计数显式记录缺口。
    assert_eq!(value["data"]["counts"]["unassociatedProcesses"], 1);
    // 完成保守关系测试。
    Ok(())
}

// 验证前景变化使快照关闭失败。
#[test]
fn foreground_change_fails_closed() {
    // 构造快照。
    let mut snapshot = fixture_snapshot();
    // 模拟前景变化。
    snapshot.foreground_unchanged = false;
    // 渲染必须失败。
    let error = match render(snapshot) {
        // 成功表示前景门禁失效。
        Ok(_) => panic!("foreground change must fail"),
        // 保存结构化错误。
        Err(error) => error,
    };
    // 验证稳定错误码。
    assert_eq!(error.code, "HOST_INTERFERENCE_DETECTED");
}

// 验证所有三类边界拒绝零值与超限值。
#[test]
fn discovery_limits_are_closed() {
    // 零值必须失败。
    let zero_error = match validate_limit(0, "max-applications") {
        // 成功表示零边界被错误接受。
        Ok(()) => panic!("zero must fail"),
        // 保存结构化错误。
        Err(error) => error,
    };
    // 验证零边界错误码。
    assert_eq!(
        zero_error.code,
        // 使用稳定参数错误。
        "INVALID_ARGUMENT"
    );
    // 4096 合法。
    assert!(validate_limit(4096, "max-processes").is_ok());
    // 超限必须失败。
    assert!(validate_limit(4097, "max-windows").is_err());
}

// 验证实机只读来源返回有界、可解析且不泄漏原生字段的关系图。
#[test]
fn live_discovery_is_bounded_and_private() -> AppResult<()> {
    // 使用小边界执行全部真实只读来源。
    let value = discover(64, 64, 64)?;
    // 验证成功 envelope。
    assert_eq!(value["ok"], true);
    // 验证三类数组不超过调用边界。
    assert!(
        value["data"]["applications"]
            .as_array()
            .is_some_and(|items| items.len() <= 64)
    );
    // 验证进程边界。
    assert!(
        value["data"]["processes"]
            .as_array()
            .is_some_and(|items| items.len() <= 64)
    );
    // 验证窗口边界。
    assert!(
        value["data"]["windows"]
            .as_array()
            .is_some_and(|items| items.len() <= 64)
    );
    // 禁止原生 PID 字段。
    assert!(!contains_key(&value, "processId"));
    // 禁止 HWND 字段。
    assert!(!contains_key(&value, "hwnd"));
    // 完成实机只读测试。
    Ok(())
}
