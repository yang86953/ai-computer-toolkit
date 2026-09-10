//! 验证 sequence execution 固定私有目录链与 journal 接线。

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
// 固定测试 execution identity。
const EXECUTION_ID: &str = "s2:q:00000000000000000000000000000001";

// 拥有一个可安全清理的真实临时根。
struct FixtureDirectory {
    // 保存精确 fixture 路径。
    path: PathBuf,
}

// 构造唯一测试根。
impl FixtureDirectory {
    // 创建当前测试独占目录。
    fn new() -> Self {
        // 取得进程内唯一序列。
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 构造固定前缀路径。
        let path = std::env::temp_dir().join(format!(
            // 使用固定 toolkit 测试标签。
            "act-sequence-storage-{}-{sequence}",
            // 注入当前测试进程 ID。
            std::process::id(),
        ));
        // 清理同名异常旧 fixture。
        let _ = fs::remove_dir_all(&path);
        // 创建真实根目录。
        fs::create_dir(&path)
            // 失败时保留测试诊断。
            .unwrap_or_else(|error| panic!("fixture creation failed: {error}"));
        // 返回根目录所有者。
        Self { path }
    }
}

// 作用域结束时回收自有目录树。
impl Drop for FixtureDirectory {
    // 删除精确 fixture。
    fn drop(&mut self) {
        // owner-only DACL 保留当前用户删除权限。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 验证固定两级目录可幂等打开并承载原子 journal。
#[test]
fn fixed_owner_only_chain_opens_journal() {
    // 创建自有模拟 LocalAppData 根。
    let fixture = FixtureDirectory::new();
    // 首次建立固定私有目录链。
    let journal = open_sequence_execution_journal_under(&fixture.path)
        // 创建和权限回读必须成功。
        .unwrap_or_else(|error| panic!("storage open failed: {error:?}"));
    // 通过返回 journal 原子建立测试记录。
    journal
        // 持久最小有界字节。
        .persist(EXECUTION_ID, br#"{"recordRevision":0}"#)
        // 提交必须成功。
        .unwrap_or_else(|error| panic!("storage persist failed: {error:?}"));
    // 重开必须幂等加固同一目录链。
    let reopened = open_sequence_execution_journal_under(&fixture.path)
        // 既有目录权限验证必须成功。
        .unwrap_or_else(|error| panic!("storage reopen failed: {error:?}"));
    // 重开后仍能恢复同一 final。
    let contents = reopened
        // 严格扫描 journal。
        .load()
        // 扫描必须成功。
        .unwrap_or_else(|error| panic!("storage load failed: {error:?}"));
    // 只恢复一条精确记录。
    assert_eq!(contents.documents().len(), 1);
    // 文件名绑定 identity 保持稳定。
    assert_eq!(contents.documents()[0].execution_id(), EXECUTION_ID);
}

// 验证非目录根不会触发宽路径创建或权限修改。
#[test]
fn non_directory_root_is_rejected() {
    // 创建自有 fixture。
    let fixture = FixtureDirectory::new();
    // 在 fixture 内创建普通文件根候选。
    let file = fixture.path.join("not-a-directory");
    // 写入固定占位内容。
    fs::write(&file, b"file")
        // fixture 写入必须成功。
        .unwrap_or_else(|error| panic!("root file creation failed: {error}"));
    // 普通文件不得作为 Known Folder 根。
    assert!(matches!(
        open_sequence_execution_journal_under(&file),
        Err(SequenceExecutionStorageError::Directory(_))
    ));
}
