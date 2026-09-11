//! 验证 owner-only 目录创建、加固与固定名称边界。

// 导入被测私有实现。
use super::*;
// 导入 fixture 文件与唯一序列工具。
use std::{
    // 导入测试文件操作。
    fs,
    // 导入 fixture 路径。
    path::PathBuf,
    // 导入并行测试安全序列。
    sync::atomic::{AtomicU64, Ordering},
};

// 为并行测试提供唯一目录序列。
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 拥有一个可安全清理的真实临时目录。
struct FixtureDirectory {
    // 保存精确 fixture 路径。
    path: PathBuf,
}

// 构造唯一测试目录。
impl FixtureDirectory {
    // 创建带固定标签的真实目录。
    fn new(label: &str) -> Self {
        // 取得进程内唯一序列。
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 构造当前测试独占路径。
        let path = std::env::temp_dir().join(format!(
            // 使用固定 toolkit 测试前缀。
            "act-owner-only-directory-{}-{label}-{sequence}",
            // 注入当前测试进程 ID。
            std::process::id(),
        ));
        // 清理同名异常旧 fixture。
        let _ = fs::remove_dir_all(&path);
        // 创建真实父目录。
        fs::create_dir(&path)
            // 失败时保留测试诊断。
            .unwrap_or_else(|error| panic!("fixture creation failed: {error}"));
        // 返回目录所有者。
        Self { path }
    }
}

// 作用域结束时回收自有 fixture。
impl Drop for FixtureDirectory {
    // 删除精确目录树。
    fn drop(&mut self) {
        // 当前用户仍有完全访问，允许确定性清理。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 验证创建时安装权限且重复加固保持可用。
#[test]
fn child_is_created_owner_only_and_reopened() {
    // 创建自有真实父目录。
    let fixture = FixtureDirectory::new("create");
    // 首次创建固定 owner-only 子目录。
    let first = ensure_owner_only_child(&fixture.path, "sequence-execution-v1")
        // 创建和回读权限必须成功。
        .unwrap_or_else(|error| panic!("owner-only creation failed: {error:?}"));
    // 重复调用必须幂等加固同一目录。
    let second = ensure_owner_only_child(&fixture.path, "sequence-execution-v1")
        // 既有目录权限验证必须成功。
        .unwrap_or_else(|error| panic!("owner-only reopen failed: {error:?}"));
    // 两次结果必须指向同一精确目录。
    assert_eq!(first, second);
    // 当前用户必须保留子项写入权限。
    fs::write(first.join("probe.json"), b"private")
        // 写入失败说明 DACL 没有授权当前用户。
        .unwrap_or_else(|error| panic!("owner-only write failed: {error}"));
    // 最终项目仍必须是真实非 reparse 目录。
    assert_eq!(validate_real_directory(&first), Ok(()));
}

// 验证路径注入、别名与非目录占位失败闭合。
#[test]
fn invalid_names_and_existing_files_are_rejected() {
    // 创建自有真实父目录。
    let fixture = FixtureDirectory::new("reject");
    // 分隔符不得进入固定子目录。
    assert_eq!(
        ensure_owner_only_child(&fixture.path, "nested\\journal"),
        Err(OwnerOnlyDirectoryError::InvalidName)
    );
    // 大写别名不属于固定字符集。
    assert_eq!(
        ensure_owner_only_child(&fixture.path, "Sequence"),
        Err(OwnerOnlyDirectoryError::InvalidName)
    );
    // 创建普通文件占据目标名称。
    fs::write(fixture.path.join("journal"), b"not-a-directory")
        // fixture 写入必须成功。
        .unwrap_or_else(|error| panic!("file fixture failed: {error}"));
    // 普通文件不得被权限加固成目录。
    assert_eq!(
        ensure_owner_only_child(&fixture.path, "journal"),
        Err(OwnerOnlyDirectoryError::DirectoryUnavailable)
    );
}

// 验证 MCP 真实生成的捕获目录名通过同一生产创建、加固与回读管线。
#[test]
fn real_generated_capture_directory_names_pass_production_validation() {
    // 创建自有真实父目录。
    let fixture = FixtureDirectory::new("real-name");
    // 覆盖当前进程号与最大 u32 进程号的真实生成名。
    for pid in [std::process::id(), u32::MAX] {
        // 经生产生成器取得真实单段名。
        let name = crate::mcp::desktop::capture_directory_name(pid);
        // 真实名必须落在组件 64 字节单段上限内。
        assert!(name.len() <= 64, "generated name exceeds the bound: {name}");
        // 完整生产管线：创建即安装受保护 DACL 并回读逐值验证。
        let directory = ensure_owner_only_child(&fixture.path, &name)
            .unwrap_or_else(|error| panic!("real generated name rejected: {error:?}"));
        // 当前用户必须保留子项写入权限。
        fs::write(directory.join("probe.png"), b"private")
            .unwrap_or_else(|error| panic!("owner-only write failed: {error}"));
        // 最终项目仍必须是真实非 reparse 目录。
        assert_eq!(validate_real_directory(&directory), Ok(()));
    }
}

// 验证 64/65/96 字节长度与路径、非法字符边界固定闭合。
#[test]
fn name_length_and_charset_boundaries_fail_closed() {
    // 创建自有真实父目录。
    let fixture = FixtureDirectory::new("boundaries");
    // 64 字节固定字符集名称是单段上限内的合法边界。
    let exact = "a".repeat(64);
    assert_eq!(
        ensure_owner_only_child(&fixture.path, &exact),
        Ok(fixture.path.join(&exact))
    );
    // 65 与 96 字节超限名称必须以 InvalidName 拒绝。
    for length in [65_usize, 96] {
        assert_eq!(
            ensure_owner_only_child(&fixture.path, &"a".repeat(length)),
            Err(OwnerOnlyDirectoryError::InvalidName)
        );
    }
    // 空名称、两种路径分隔符与其他非法字符同样拒绝。
    for name in ["", "nested/journal", "colon:name", "dot.name", "space name"] {
        assert_eq!(
            ensure_owner_only_child(&fixture.path, name),
            Err(OwnerOnlyDirectoryError::InvalidName)
        );
    }
}
