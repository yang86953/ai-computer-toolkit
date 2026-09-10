//! 已安装应用只读 Component 的测试夹具。

// 导入被测组件的私有辅助函数与类型。
use super::*;

// 构造单项来源清单。
fn source(record: InstalledApplicationRecord) -> SourceInventory {
    // 返回可用且完整的夹具来源。
    SourceInventory {
        // 保存单项记录。
        records: vec![record],
        // 标记可用。
        available: true,
        // 标记完整。
        complete: true,
    }
}

// 构造可用且完整的空来源清单。
fn empty_source() -> SourceInventory {
    // 返回不会影响合并结果的来源快照。
    SourceInventory {
        // 不提供任何应用记录。
        records: Vec::new(),
        // 标记测试来源可用。
        available: true,
        // 标记测试来源完整。
        complete: true,
    }
}

// 构造稳定应用夹具。
fn fixture(id: &str, display_name: &str, source_name: &str) -> InstalledApplicationRecord {
    // 返回无私有路径的测试记录。
    InstalledApplicationRecord {
        // 保存夹具目标。
        session_id: id.to_owned(),
        // 保存显示名。
        display_name: display_name.to_owned(),
        // 保存版本。
        version: "1.2.3".to_owned(),
        // 保存发布者。
        publisher: "Example Publisher".to_owned(),
        // 保存来源。
        discovery_sources: vec![source_name.to_owned()],
        // 保存规范化提示。
        process_match_hints: vec![normalized_name(display_name)],
        // 默认没有启动 identity。
        launch_identity: String::new(),
    }
}

// 验证跨语言应用目标字节布局。
#[test]
fn application_ids_match_cpp_goldens() {
    // 验证 Registry identity golden。
    assert_eq!(
        registry_application_id("Fixture App", "Example Publisher", "1.2.3"),
        "s2:a:4ff7f0853b13fe47"
    );
    // 验证 Shell identity golden。
    assert_eq!(
        shell_application_id("Fixture App", "Example.App_123!App"),
        "s2:a:671fab8e658f1aad"
    );
}

// 验证 DisplayIcon 只产生私有规范化文件名提示。
#[test]
fn display_icon_hint_hides_path_and_arguments() {
    // 路径、引号、扩展名与图标索引都必须被移除。
    assert_eq!(
        icon_process_hint("\"C:\\Program Files\\Fixture\\Fixture.EXE\",0"),
        "fixture"
    );
}

// 验证唯一显示名匹配会合并来源与启动候选。
#[test]
fn unique_display_name_merge_is_conservative() {
    // 构造 Registry 记录。
    let registry = source(fixture(
        // 使用 Registry 夹具 ID。
        "s2:a:1111111111111111",
        // 使用相同显示名。
        "Fixture App",
        // 标记 Registry 来源。
        "registry-uninstall",
    ));
    // 构造 Shell 记录。
    let mut shell_record = fixture(
        // 使用不同 Shell ID。
        "s2:a:2222222222222222",
        // 大小写和空格规范化后唯一相同。
        "Fixture App",
        // 标记 Shell 来源。
        "shell-apps-folder",
    );
    // 提供私有启动 identity。
    shell_record.launch_identity = "shell-private-launch-id".to_owned();
    // 执行合并。
    let merged = merge_sources(registry, source(shell_record), empty_source(), 8);
    // 唯一匹配只保留一个公开应用。
    assert_eq!(merged.records.len(), 1);
    // 两个来源都必须保留。
    assert_eq!(
        merged.records[0].discovery_sources,
        ["registry-uninstall", "shell-apps-folder"]
    );
    // 私有启动 identity 只保存在内部记录。
    assert_eq!(merged.records[0].launch_identity, "shell-private-launch-id");
}

// 验证多命中时不会猜测合并。
#[test]
fn ambiguous_display_name_does_not_merge() {
    // 构造两个规范化同名 Registry 记录。
    let registry = SourceInventory {
        // 保存两个独立应用。
        records: vec![
            // 第一记录。
            fixture("s2:a:1111111111111111", "Fixture App", "registry-uninstall"),
            // 第二记录。
            fixture("s2:a:2222222222222222", "Fixture App", "registry-uninstall"),
        ],
        // 标记来源可用。
        available: true,
        // 标记来源完整。
        complete: true,
    };
    // 构造同名 Shell 记录。
    let shell = source(fixture(
        // 使用独立 Shell ID。
        "s2:a:3333333333333333",
        // 使用相同显示名。
        "Fixture App",
        // 标记 Shell 来源。
        "shell-apps-folder",
    ));
    // 执行合并。
    let merged = merge_sources(registry, shell, empty_source(), 8);
    // 多命中时保留三个独立应用。
    assert_eq!(merged.records.len(), 3);
}

// 验证待合并来源自身多命中时不会按迭代顺序选择首项。
#[test]
fn ambiguous_incoming_display_name_does_not_merge() {
    // 构造唯一 Registry 基础记录。
    let registry = source(fixture(
        // 使用独立 Registry ID。
        "s2:a:1111111111111111",
        // 使用与两个候选相同的显示名。
        "Fixture App",
        // 标记 Registry 来源。
        "registry-uninstall",
    ));
    // 构造两个规范化同名的 Shell 候选。
    let shell = SourceInventory {
        // 保留两个独立 Shell 记录。
        records: vec![
            // 添加第一个候选。
            fixture("s2:a:2222222222222222", "Fixture App", "shell-apps-folder"),
            // 添加第二个候选。
            fixture("s2:a:3333333333333333", "Fixture App", "shell-apps-folder"),
        ],
        // 标记来源可用。
        available: true,
        // 标记来源完整。
        complete: true,
    };
    // 执行不含 Start Menu 记录的三来源合并。
    let merged = merge_sources(registry, shell, empty_source(), 8);
    // 两个候选均不得被猜测吸收到 Registry 记录。
    assert_eq!(merged.records.len(), 3);
    // Registry 记录不得获得未经唯一证明的 Shell 来源。
    assert_eq!(merged.records[0].discovery_sources, ["registry-uninstall"]);
}

// 验证空 ASCII 规范化结果不会把非 ASCII 名称误合并。
#[test]
fn empty_normalized_display_name_does_not_merge() {
    // 构造唯一但无 ASCII 字母数字的 Registry 应用。
    let registry = SourceInventory {
        // 保存中文显示名记录。
        records: vec![fixture(
            "s2:a:1111111111111111",
            "应用",
            "registry-uninstall",
        )],
        // 标记来源可用。
        available: true,
        // 标记来源完整。
        complete: true,
    };
    // 构造不同中文 Shell 应用。
    let shell = SourceInventory {
        // 保存另一空规范化显示名记录。
        records: vec![fixture(
            "s2:a:2222222222222222",
            "工具",
            "shell-apps-folder",
        )],
        // 标记来源可用。
        available: true,
        // 标记来源完整。
        complete: true,
    };
    // 合并来源。
    let inventory = merge_sources(registry, shell, empty_source(), 8);
    // 两条记录必须保持独立。
    assert_eq!(inventory.records.len(), 2);
}

// 验证实时来源枚举只读可用且不公开私有启动 identity。
#[test]
fn live_application_sources_are_bounded() {
    // 使用小边界执行真实只读枚举。
    let inventory = enumerate_installed_applications(64);
    // 至少一个公开来源应在正常 Windows 会话可用。
    assert!(
        inventory.registry_source_available
            // AppsFolder 可用同样满足实时来源要求。
            || inventory.shell_source_available
            // 固定 Start Menu 可用也满足实时来源要求。
            || inventory.start_menu_source_available
    );
    // 输出不得超过调用方边界。
    assert!(inventory.records.len() <= 64);
    // 所有公共目标必须 canonical。
    assert!(
        inventory
            // 遍历记录。
            .records
            // 检查全部目标。
            .iter()
            // 要求 s2:a。
            .all(|application| application.session_id.starts_with("s2:a:"))
    );
}
