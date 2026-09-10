//! 验证长操作 journal 的原子替换、预算、严格枚举与清理。

// 导入被测私有 Component。
use super::*;
// 导入 fixture 文件与唯一序列工具。
use std::{
    // 导入测试文件操作。
    fs,
    // 导入 fixture 路径类型。
    path::{Path, PathBuf},
    // 导入并行测试唯一序列。
    sync::atomic::{AtomicU64, Ordering},
};

// 为测试目录提供进程内唯一序列。
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 独占一个可安全清理的临时父目录。
struct FixtureDirectory {
    // 保存当前测试精确路径。
    path: PathBuf,
}

// 构造 journal fixture。
impl FixtureDirectory {
    // 创建唯一测试父目录。
    fn new(label: &str) -> Self {
        // 取得进程内唯一编号。
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 在系统临时目录下使用固定安全前缀。
        let path = std::env::temp_dir().join(format!(
            // 固定前缀、进程、标签与序列。
            "act-long-operation-journal-{}-{label}-{sequence}",
            // 注入当前测试进程 ID。
            std::process::id(),
        ));
        // 清理相同精确旧 fixture。
        let _ = fs::remove_dir_all(&path);
        // 创建真实父目录。
        fs::create_dir(&path)
            // 失败时提供测试诊断。
            .unwrap_or_else(|error| panic!("fixture creation failed: {error}"));
        // 返回目录所有者。
        Self { path }
    }

    // 打开 fixture 内的 journal 叶目录。
    fn journal(&self) -> LongOperationJournal {
        // 使用固定叶目录名称。
        LongOperationJournal::open(&self.path.join("journal"))
            // 打开必须成功。
            .unwrap_or_else(|error| panic!("journal open failed: {error:?}"))
    }

    // 返回 journal 叶目录路径。
    fn journal_path(&self) -> PathBuf {
        // 只拼接固定叶目录名称。
        self.path.join("journal")
    }
}

// 作用域结束时清理精确 fixture。
impl Drop for FixtureDirectory {
    // 删除当前实例拥有的目录。
    fn drop(&mut self) {
        // 只删除带唯一前缀的精确路径。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 统计目录中的文件数量。
fn file_count(path: &Path) -> usize {
    // 枚举精确 fixture 目录。
    fs::read_dir(path)
        // 失败时提供测试诊断。
        .unwrap_or_else(|error| panic!("fixture enumeration failed: {error}"))
        // 只统计成功目录项。
        .filter_map(Result::ok)
        // 返回数量。
        .count()
}

// 验证同一记录以原子替换建立最新完整文档。
#[test]
fn persist_replaces_one_complete_record_without_staging() {
    // 创建独占 fixture。
    let fixture = FixtureDirectory::new("replace");
    // 打开 journal。
    let journal = fixture.journal();
    // 固定 canonical operation handle。
    let operation_id = "s2:o:0000000000000001";
    // 首次持久完整 JSON。
    journal
        // 写入初始文档。
        .persist(operation_id, br#"{"revision":1}"#)
        // 提交必须成功。
        .unwrap_or_else(|error| panic!("first persist failed: {error:?}"));
    // 原子替换同一记录。
    journal
        // 写入新完整文档。
        .persist(operation_id, br#"{"revision":2}"#)
        // 替换必须成功。
        .unwrap_or_else(|error| panic!("replacement persist failed: {error:?}"));
    // 严格加载 journal。
    let documents = journal
        // 加载 final 文件。
        .load()
        // 加载必须成功。
        .unwrap_or_else(|error| panic!("journal load failed: {error:?}"));
    // 只允许一个 final 记录。
    assert_eq!(documents.len(), 1);
    // 文件名必须恢复同一 handle。
    assert_eq!(documents[0].operation_id(), operation_id);
    // 内容必须是完整新版本而非部分拼接。
    assert_eq!(documents[0].bytes(), br#"{"revision":2}"#);
    // 目录不得留下 staging。
    assert_eq!(file_count(&fixture.journal_path()), 1);
}

// 验证只清理完整匹配的 stale staging 并拒绝未知项目。
#[test]
fn load_cleans_owned_staging_and_rejects_unknown_entries() {
    // 创建独占 fixture。
    let fixture = FixtureDirectory::new("entries");
    // 打开 journal 以创建叶目录。
    let journal = fixture.journal();
    // 构造项目统一 staging 名称。
    let staging = fixture
        // 取得 journal 目录。
        .journal_path()
        // 拼接完整自有 staging 名称。
        .join(".op-0000000000000002.json.123.7.part.json");
    // 写入模拟崩溃残留。
    fs::write(&staging, b"partial")
        // fixture 写入必须成功。
        .unwrap_or_else(|error| panic!("staging fixture write failed: {error}"));
    // 加载时清理 stale staging。
    let documents = journal
        // 严格加载目录。
        .load()
        // 清理必须成功。
        .unwrap_or_else(|error| panic!("staging cleanup failed: {error:?}"));
    // staging 不得形成接受记录。
    assert!(documents.is_empty());
    // staging 必须已经删除。
    assert!(!staging.exists());
    // 构造未知文件。
    let unknown = fixture.journal_path().join("notes.txt");
    // 写入未知项目。
    fs::write(&unknown, b"unknown")
        // fixture 写入必须成功。
        .unwrap_or_else(|error| panic!("unknown fixture write failed: {error}"));
    // 未知项目必须失败闭合。
    assert_eq!(journal.load(), Err(LongOperationJournalError::InvalidEntry));
}

// 验证 handle 与单文档预算在写入前失败闭合。
#[test]
fn persist_rejects_noncanonical_handles_and_oversized_documents() {
    // 创建独占 fixture。
    let fixture = FixtureDirectory::new("bounds");
    // 打开 journal。
    let journal = fixture.journal();
    // 拒绝错误 opaque 类别。
    assert_eq!(
        // 尝试持久窗口 handle。
        journal.persist("s2:w:0000000000000001", b"{}"),
        // 返回 operation 边界错误。
        Err(LongOperationJournalError::InvalidOperation)
    );
    // 构造超出一字节的文档。
    let oversized = vec![b'x'; MAX_JOURNAL_DOCUMENT_BYTES + 1];
    // 在 staging 前拒绝超限。
    assert_eq!(
        // 尝试持久超限文档。
        journal.persist("s2:o:0000000000000001", &oversized),
        // 返回固定预算错误。
        Err(LongOperationJournalError::RecordTooLarge)
    );
    // 失败不得留下任何文件。
    assert_eq!(file_count(&fixture.journal_path()), 0);
}
