//! 验证 sequence execution journal 的原子替换、扫描与失败闭合。

// 导入被测私有实现。
use super::*;
// 导入 fixture 文件与唯一序列工具。
use std::{
    // 导入自有 fixture 文件操作。
    fs,
    // 导入 fixture 路径类型。
    path::PathBuf,
    // 导入并行测试安全序列。
    sync::atomic::{AtomicU64, Ordering},
};

// 为并行测试分配不冲突目录。
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
// 固定第一个 canonical execution identity。
const FIRST_EXECUTION: &str = "s2:q:00000000000000000000000000000001";
// 固定第二个 canonical execution identity。
const SECOND_EXECUTION: &str = "s2:q:00000000000000000000000000000002";

// 拥有一个精确临时目录并在作用域结束后清理。
struct FixtureDirectory {
    // 保存当前测试唯一目录。
    path: PathBuf,
}

// 构造唯一 fixture 目录。
impl FixtureDirectory {
    // 创建带固定标签的自有目录。
    fn new(label: &str) -> Self {
        // 取得当前进程内唯一序列。
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 构造仅当前测试拥有的精确路径。
        let path = std::env::temp_dir().join(format!(
            // 使用固定 toolkit 测试前缀。
            "act-sequence-journal-{}-{label}-{sequence}",
            // 注入当前测试进程 ID。
            std::process::id(),
        ));
        // 清除同名异常旧 fixture。
        let _ = fs::remove_dir_all(&path);
        // 创建精确目录。
        fs::create_dir(&path)
            // 创建失败时保留测试诊断。
            .unwrap_or_else(|error| panic!("fixture directory creation failed: {error}"));
        // 返回目录唯一所有者。
        Self { path }
    }

    // 打开受测 journal。
    fn journal(&self) -> SequenceExecutionJournal {
        // fixture 目录必须满足真实目录边界。
        SequenceExecutionJournal::open(&self.path)
            // 打开失败时保留封闭错误类别。
            .unwrap_or_else(|error| panic!("journal open failed: {error:?}"))
    }

    // 拼接固定测试文件名。
    fn join(&self, name: &str) -> PathBuf {
        // 只在自有目录内构造路径。
        self.path.join(name)
    }
}

// 作用域结束时回收完整自有 fixture。
impl Drop for FixtureDirectory {
    // 删除精确测试目录。
    fn drop(&mut self) {
        // 忽略清理诊断以免覆盖测试失败。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 验证同一 execution 只发布完整最新记录并按 identity 稳定排序。
#[test]
fn persist_replaces_atomically_and_loads_stably() {
    // 创建自有 journal。
    let fixture = FixtureDirectory::new("persist");
    // 打开精确目录。
    let journal = fixture.journal();
    // 先写较大 identity 以排除枚举顺序依赖。
    journal
        // 持久第二条记录。
        .persist(SECOND_EXECUTION, br#"{"recordRevision":0}"#)
        // 首次提交必须成功。
        .unwrap_or_else(|error| panic!("second persist failed: {error:?}"));
    // 建立第一条初始记录。
    journal
        // 持久 revision 零。
        .persist(FIRST_EXECUTION, br#"{"recordRevision":0}"#)
        // 首次提交必须成功。
        .unwrap_or_else(|error| panic!("first persist failed: {error:?}"));
    // 原子替换第一条记录。
    journal
        // 持久 revision 一。
        .persist(FIRST_EXECUTION, br#"{"recordRevision":1}"#)
        // 替换提交必须成功。
        .unwrap_or_else(|error| panic!("replacement persist failed: {error:?}"));
    // 严格扫描全部 final 文件。
    let contents = journal
        // 加载固定目录。
        .load()
        // 扫描必须成功。
        .unwrap_or_else(|error| panic!("journal load failed: {error:?}"));
    // 借用排序活动记录。
    let documents = contents.documents();
    // 两个 execution 各只有一条 final。
    assert_eq!(documents.len(), 2);
    // 结果按 canonical identity 排序。
    assert_eq!(documents[0].execution_id(), FIRST_EXECUTION);
    // 第一条只暴露完整最新字节。
    assert_eq!(documents[0].bytes(), br#"{"recordRevision":1}"#);
    // 第二条保持原始内容。
    assert_eq!(documents[1].execution_id(), SECOND_EXECUTION);
}

// 验证 stale staging 只在完整固定名称匹配时清理。
#[test]
fn load_cleans_only_canonical_staging_names() {
    // 创建自有 journal。
    let fixture = FixtureDirectory::new("staging");
    // 写入完整匹配 StagedFile 形状的崩溃遗留文件。
    fs::write(
        // 使用 canonical 指纹和十进制进程/序列。
        fixture.join(".execution-00000000000000000000000000000001.json.10.20.part.json"),
        // 内容不建立任何 final 事实。
        b"stale",
    )
    // fixture 写入必须成功。
    .unwrap_or_else(|error| panic!("staging fixture write failed: {error}"));
    // 打开并扫描目录。
    let contents = fixture
        // 取得 journal。
        .journal()
        // 清理 stale staging。
        .load()
        // 规范 staging 清理必须成功。
        .unwrap_or_else(|error| panic!("staging cleanup failed: {error:?}"));
    // staging 不得成为恢复记录。
    assert!(contents.documents().is_empty());
    // staging 不得伪造 forgotten index。
    assert!(contents.forgotten_index().is_none());
    // 目录必须已经清空。
    assert_eq!(
        fs::read_dir(&fixture.path)
            // 枚举 fixture 必须成功。
            .unwrap_or_else(|error| panic!("fixture read failed: {error}"))
            // 统计残留项目。
            .count(),
        0
    );

    // 写入近似但非 canonical staging 名称。
    fs::write(
        // 非十进制序列禁止被当作自有清理目标。
        fixture.join(".execution-00000000000000000000000000000001.json.10.alias.part.json"),
        // 保存任意污染字节。
        b"unknown",
    )
    // fixture 写入必须成功。
    .unwrap_or_else(|error| panic!("invalid staging write failed: {error}"));
    // 未知名称必须阻止恢复而不是被宽松删除。
    assert_eq!(
        fixture.journal().load(),
        Err(SequenceExecutionJournalError::InvalidEntry)
    );
}

// 验证 identity、未知项目与记录预算失败闭合。
#[test]
fn invalid_identity_entry_and_record_size_are_rejected() {
    // 创建自有 journal。
    let fixture = FixtureDirectory::new("reject");
    // 打开精确目录。
    let journal = fixture.journal();
    // 其他 opaque 类别不得映射为文件。
    assert_eq!(
        journal.persist(
            "s2:o:00000000000000000000000000000001",
            br#"{"recordRevision":0}"#
        ),
        Err(SequenceExecutionJournalError::InvalidExecution)
    );
    // 空文档不得建立 final。
    assert_eq!(
        journal.persist(FIRST_EXECUTION, b""),
        Err(SequenceExecutionJournalError::RecordTooLarge)
    );
    // 未知文件名必须阻止恢复。
    fs::write(fixture.join("notes.txt"), b"not-a-record")
        // fixture 写入必须成功。
        .unwrap_or_else(|error| panic!("unknown entry write failed: {error}"));
    // 扫描必须失败闭合。
    assert_eq!(
        journal.load(),
        Err(SequenceExecutionJournalError::InvalidEntry)
    );
}

// 验证非跟随读取拒绝空 final 与稀疏超限记录。
#[test]
fn load_rejects_empty_and_oversized_final_records() {
    // 创建空记录 fixture。
    let empty = FixtureDirectory::new("empty");
    // 写入 canonical 名称的空 final。
    fs::write(
        // 使用固定 execution 文件名。
        empty.join("execution-00000000000000000000000000000001.json"),
        // 空字节不得恢复。
        b"",
    )
    // fixture 写入必须成功。
    .unwrap_or_else(|error| panic!("empty fixture write failed: {error}"));
    // 空 final 返回记录预算错误。
    assert_eq!(
        empty.journal().load(),
        Err(SequenceExecutionJournalError::RecordTooLarge)
    );

    // 创建超限记录 fixture。
    let oversized = FixtureDirectory::new("oversized");
    // 创建 canonical final 文件。
    let file = fs::File::create(
        // 使用固定 execution 文件名。
        oversized.join("execution-00000000000000000000000000000001.json"),
    )
    // 文件创建必须成功。
    .unwrap_or_else(|error| panic!("oversized fixture creation failed: {error}"));
    // 以稀疏长度建立超限元数据而不分配大内存。
    file.set_len((MAX_SEQUENCE_EXECUTION_RECORD_BYTES + 1) as u64)
        // 长度设置必须成功。
        .unwrap_or_else(|error| panic!("oversized fixture sizing failed: {error}"));
    // 关闭 fixture 写句柄后再执行独占读取。
    drop(file);
    // 超限 final 必须在读取前失败。
    assert_eq!(
        oversized.journal().load(),
        Err(SequenceExecutionJournalError::RecordTooLarge)
    );
}

// 验证精确删除幂等且不接受 identity 别名。
#[test]
fn remove_is_exact_and_idempotent() {
    // 创建自有 journal。
    let fixture = FixtureDirectory::new("remove");
    // 打开精确目录。
    let journal = fixture.journal();
    // 建立一条 final。
    journal
        // 持久固定字节。
        .persist(FIRST_EXECUTION, br#"{"recordRevision":0}"#)
        // 提交必须成功。
        .unwrap_or_else(|error| panic!("persist before remove failed: {error:?}"));
    // 首次精确删除成功。
    assert_eq!(journal.remove(FIRST_EXECUTION), Ok(()));
    // 重复删除保持幂等。
    assert_eq!(journal.remove(FIRST_EXECUTION), Ok(()));
    // 非 canonical identity 不得触碰目录。
    assert_eq!(
        journal.remove("s2:q:0000000000000000000000000000000A"),
        Err(SequenceExecutionJournalError::InvalidExecution)
    );
}

// 验证 forgotten index 与活动记录在同一严格目录中独立原子替换。
#[test]
fn forgotten_index_is_loaded_as_reserved_atomic_document() {
    // 创建自有 journal。
    let fixture = FixtureDirectory::new("forgotten");
    // 打开精确目录。
    let journal = fixture.journal();
    // 建立一条活动 execution 记录。
    journal
        // 持久固定字节。
        .persist(FIRST_EXECUTION, br#"{"recordRevision":0}"#)
        // execution 提交必须成功。
        .unwrap_or_else(|error| panic!("execution persist failed: {error:?}"));
    // 建立 revision 零 forgotten index。
    journal
        // 持久第一版紧凑字节。
        .persist_forgotten_index(br#"{"recordRevision":0,"records":[]}"#)
        // 首次 index 提交必须成功。
        .unwrap_or_else(|error| panic!("forgotten persist failed: {error:?}"));
    // 原子替换 forgotten index。
    journal
        // 持久后一完整版本。
        .persist_forgotten_index(br#"{"recordRevision":1,"records":[1]}"#)
        // 替换提交必须成功。
        .unwrap_or_else(|error| panic!("forgotten replacement failed: {error:?}"));
    // 严格扫描全部持久事实。
    let contents = journal
        // 加载同一目录。
        .load()
        // 扫描必须成功。
        .unwrap_or_else(|error| panic!("journal load failed: {error:?}"));
    // 活动 execution 仍独立存在。
    assert_eq!(contents.documents().len(), 1);
    // 只暴露最后完整 forgotten index。
    assert_eq!(
        contents.forgotten_index(),
        Some(br#"{"recordRevision":1,"records":[1]}"#.as_slice())
    );
}
