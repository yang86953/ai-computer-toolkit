//! 固定 Start Menu 应用来源的文件系统边界测试。

// 导入可调试错误、文件系统、路径与唯一序号工具。
use std::{
    // 为统一测试解包保留底层错误诊断。
    fmt::Debug,
    // 创建和更新工具自有临时快捷方式夹具。
    fs,
    // 保存精确临时根目录。
    path::{Path, PathBuf},
    // 生成并行测试不冲突的序号。
    sync::atomic::{AtomicU32, Ordering},
};

// 导入父 Component 的私有纯函数与来源常量。
use super::{START_MENU_SOURCE, enumerate_roots, is_supported_entry, record_for_entry};

// 为当前测试进程中的每个根目录分配唯一后缀。
static NEXT_ROOT_ID: AtomicU32 = AtomicU32::new(1);

// 为不同错误类型提供统一且不触发 expect lint 的测试解包。
trait TestResultExt<T> {
    // 成功时返回值，失败时附带调用点消息终止测试。
    fn must(self, message: &str) -> T;
}

// 对任意可调试 Result 实现测试解包。
impl<T, E: Debug> TestResultExt<T> for Result<T, E> {
    // 保留底层错误诊断。
    fn must(self, message: &str) -> T {
        // 只在测试失败时构造 panic 文本。
        self.unwrap_or_else(|error| panic!("{message}: {error:?}"))
    }
}

// 保存并在作用域结束时删除一个工具自有临时目录。
struct TestRoot {
    // 保存精确绝对路径。
    path: PathBuf,
}

// 提供唯一测试根目录构造。
impl TestRoot {
    // 创建不会与并行测试冲突的目录。
    fn create(label: &str) -> Self {
        // 取得当前进程内唯一序号。
        let sequence = NEXT_ROOT_ID.fetch_add(1, Ordering::Relaxed);
        // 在系统临时目录构造工具自有路径。
        let path = std::env::temp_dir().join(format!(
            // 使用固定测试前缀、PID、标签和序号。
            "act-start-menu-source-{}-{label}-{sequence}",
            // 当前测试进程 ID 不进入产品结果。
            std::process::id(),
        ));
        // 创建空根目录。
        fs::create_dir(&path).must("test root should be created");
        // 返回唯一所有者。
        Self { path }
    }

    // 返回根目录借用。
    fn path(&self) -> &Path {
        // 不转移清理所有权。
        &self.path
    }
}

// 在测试结束时只删除本实例创建的精确目录。
impl Drop for TestRoot {
    // 执行局部递归清理。
    fn drop(&mut self) {
        // 测试根目录已绑定 PID、标签和序号，清理失败不覆盖主要断言。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 从固定路径读取普通文件元数据并构造认证记录。
fn record(path: &Path) -> crate::adapters::installed_applications::InstalledApplicationRecord {
    // 读取夹具文件元数据。
    let metadata = fs::symlink_metadata(path).must("fixture metadata should be readable");
    // 构造支持项记录。
    match record_for_entry(path, &metadata) {
        // 返回认证记录。
        Ok(Some(record)) => record,
        // 支持扩展名不得被自然跳过。
        Ok(None) => panic!("supported fixture was unexpectedly skipped"),
        // 无损路径和元数据不得认证失败。
        Err(()) => panic!("supported fixture could not be certified"),
    }
}

// 验证来源只接受 Windows 应用快捷方式类型。
#[test]
fn supported_entry_types_are_closed() {
    // 普通 Shell link 必须接受。
    assert!(is_supported_entry(Path::new("Fixture.LNK")));
    // ClickOnce application reference 必须接受。
    assert!(is_supported_entry(Path::new("Fixture.appref-ms")));
    // URL shortcut 不得冒充应用启动目标。
    assert!(!is_supported_entry(Path::new("Fixture.url")));
    // 可执行文件本身不得绕过固定 Shell shortcut 来源。
    assert!(!is_supported_entry(Path::new("Fixture.exe")));
    // 无扩展名文件不得进入来源。
    assert!(!is_supported_entry(Path::new("Fixture")));
}

// 验证快捷方式文件代际变化会使旧 opaque 目标 stale。
#[test]
fn shortcut_generation_changes_application_identity() {
    // 创建工具自有根目录。
    let root = TestRoot::create("generation");
    // 构造固定快捷方式路径。
    let shortcut = root.path().join("Fixture.lnk");
    // 写入第一代测试内容。
    fs::write(&shortcut, b"first").must("first shortcut generation should be written");
    // 构造第一代认证记录。
    let first = record(&shortcut);
    // 写入大小不同的第二代内容，避免依赖文件系统时间精度。
    fs::write(&shortcut, b"second-generation")
        // 写入失败必须终止测试。
        .must("second shortcut generation should be written");
    // 构造第二代认证记录。
    let second = record(&shortcut);
    // 同路径文件内容代际变化必须产生新 s2:a。
    assert_ne!(first.session_id, second.session_id);
    // 公开显示名仍来自文件 stem。
    assert_eq!(second.display_name, "Fixture");
    // 来源标签必须固定且不含路径。
    assert_eq!(second.discovery_sources, [START_MENU_SOURCE]);
}

// 验证嵌套目录只发布两类支持入口且保持有界完整性。
#[test]
fn nested_start_menu_inventory_is_bounded_and_filtered() {
    // 创建工具自有根目录。
    let root = TestRoot::create("nested");
    // 创建普通后代目录。
    let group = root.path().join("Group");
    // 创建后代目录夹具。
    fs::create_dir(&group).must("fixture group should be created");
    // 写入普通 Shell link。
    fs::write(group.join("Desktop App.lnk"), b"desktop")
        // 写入失败必须终止测试。
        .must("desktop shortcut should be written");
    // 写入 ClickOnce application reference。
    fs::write(group.join("ClickOnce App.appref-ms"), b"clickonce")
        // 写入失败必须终止测试。
        .must("ClickOnce shortcut should be written");
    // 写入不支持的 URL shortcut。
    fs::write(group.join("Website.url"), b"https://example.invalid")
        // 写入失败必须终止测试。
        .must("URL fixture should be written");
    // 写入不支持的直接 executable 占位文件。
    fs::write(group.join("Direct.exe"), b"not-an-executable")
        // 写入失败必须终止测试。
        .must("direct executable fixture should be written");
    // 枚举单一固定测试根目录。
    let inventory = enumerate_roots(vec![root.path().to_path_buf()], 8);
    // 测试根目录应被视为可用来源。
    assert!(inventory.available);
    // 全部目录自然结束且未触及边界。
    assert!(inventory.complete);
    // 只返回两类支持入口。
    assert_eq!(inventory.records.len(), 2);
    // 全部记录必须使用固定来源标签。
    assert!(inventory.records.iter().all(|record| {
        // 每条记录只发布一个 provider-neutral 来源。
        record.discovery_sources == [START_MENU_SOURCE]
    }));
    // 全部记录必须携带私有启动 identity。
    assert!(inventory.records.iter().all(|record| {
        // 私有路径不得为空。
        !record.launch_identity.is_empty()
    }));
}

// 验证记录上限触及时来源明确标记不完整。
#[test]
fn source_limit_marks_inventory_incomplete() {
    // 创建工具自有根目录。
    let root = TestRoot::create("limit");
    // 写入第一条支持入口。
    fs::write(root.path().join("First.lnk"), b"first")
        // 写入失败必须终止测试。
        .must("first bounded shortcut should be written");
    // 写入第二条支持入口。
    fs::write(root.path().join("Second.lnk"), b"second")
        // 写入失败必须终止测试。
        .must("second bounded shortcut should be written");
    // 使用单记录硬上限枚举。
    let inventory = enumerate_roots(vec![root.path().to_path_buf()], 1);
    // 只返回边界允许的一条记录。
    assert_eq!(inventory.records.len(), 1);
    // 未自然完成必须明确标记。
    assert!(!inventory.complete);
}

// 验证零上限不访问目录且不能声明完整。
#[test]
fn zero_limit_is_incomplete_without_enumeration() {
    // 创建工具自有根目录。
    let root = TestRoot::create("zero-limit");
    // 使用零记录边界枚举。
    let inventory = enumerate_roots(vec![root.path().to_path_buf()], 0);
    // 零边界不能返回记录。
    assert!(inventory.records.is_empty());
    // 根路径仍已由调用方认证。
    assert!(inventory.available);
    // 潜在来源没有被完整枚举。
    assert!(!inventory.complete);
}
