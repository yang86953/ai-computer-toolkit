//! 验证固定 journal 子目录创建与链接拒绝语义。

// 导入被测私有实现。
use super::*;
// 导入 fixture 文件与唯一序列工具。
use std::{
    // 导入测试文件操作。
    fs,
    // 导入 fixture 路径。
    path::PathBuf,
    // 导入并行测试唯一序列。
    sync::atomic::{AtomicU64, Ordering},
};

// 为测试目录提供唯一序列。
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 独占一个可安全清理的真实临时目录。
struct FixtureDirectory {
    // 保存精确 fixture 路径。
    path: PathBuf,
}

// 构造测试目录。
impl FixtureDirectory {
    // 创建唯一真实目录。
    fn new(label: &str) -> Self {
        // 取得进程内唯一编号。
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 使用系统临时目录与固定安全前缀。
        let path = std::env::temp_dir().join(format!(
            // 固定前缀、进程、标签与序列。
            "act-long-operation-storage-{}-{label}-{sequence}",
            // 注入测试进程 ID。
            std::process::id(),
        ));
        // 清理相同精确旧 fixture。
        let _ = fs::remove_dir_all(&path);
        // 创建真实目录。
        fs::create_dir(&path)
            // 失败时给出测试诊断。
            .unwrap_or_else(|error| panic!("fixture creation failed: {error}"));
        // 返回目录所有者。
        Self { path }
    }
}

// 作用域结束时清理精确 fixture。
impl Drop for FixtureDirectory {
    // 删除当前实例创建的目录。
    fn drop(&mut self) {
        // 只删除唯一固定前缀路径。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 验证固定子目录可幂等创建且名称不能注入路径。
#[test]
fn fixed_child_directory_is_created_and_reopened() {
    // 创建独占 fixture。
    let fixture = FixtureDirectory::new("fixed");
    // 首次建立固定目录。
    let first = ensure_child_directory(&fixture.path, "journal")
        // 创建必须成功。
        .unwrap_or_else(|error| panic!("first directory creation failed: {error:?}"));
    // 第二次幂等打开同一目录。
    let second = ensure_child_directory(&fixture.path, "journal")
        // 重开必须成功。
        .unwrap_or_else(|error| panic!("directory reopen failed: {error:?}"));
    // 两次结果必须是同一固定路径。
    assert_eq!(first, second);
    // 拒绝分隔符注入。
    assert_eq!(
        // 尝试提供多段名称。
        ensure_child_directory(&fixture.path, "nested\\journal"),
        // 返回固定目录错误。
        Err(LongOperationStorageError::DirectoryUnavailable)
    );
}

// 验证普通文件不能冒充 storage 目录。
#[test]
fn non_directory_storage_entry_is_rejected() {
    // 创建独占 fixture。
    let fixture = FixtureDirectory::new("file");
    // 构造固定文件路径。
    let path = fixture.path.join("journal");
    // 写入普通文件。
    fs::write(&path, b"not-a-directory")
        // fixture 写入必须成功。
        .unwrap_or_else(|error| panic!("fixture write failed: {error}"));
    // 普通文件不得被当成目录。
    assert_eq!(
        // 尝试幂等打开。
        ensure_child_directory(&fixture.path, "journal"),
        // 返回不可信目录错误。
        Err(LongOperationStorageError::DirectoryUntrusted)
    );
}
