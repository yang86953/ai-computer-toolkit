//! 验证原子文件 Component 的首次提交、覆盖、失败与清理语义。

// 导入被测私有实现。
use super::*;
// 导入测试文件与路径工具。
use std::{
    // 导入自有 fixture 文件操作。
    fs,
    // 导入 fixture 路径类型。
    path::{Path, PathBuf},
};

// 为每项测试提供独占临时目录与作用域清理。
struct FixtureDirectory {
    // 保存当前测试唯一目录。
    path: PathBuf,
}

// 构造自有临时目录。
impl FixtureDirectory {
    // 创建带固定标签的唯一目录。
    fn new(label: &str) -> Self {
        // 使用同一原子序列避免并行测试碰撞。
        let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 构造当前进程独占路径。
        let path = std::env::temp_dir().join(format!(
            // 使用固定 toolkit 测试前缀。
            "act-atomic-file-{}-{label}-{sequence}",
            // 注入当前测试进程 ID。
            std::process::id(),
        ));
        // 清除同路径的异常旧 fixture。
        let _ = fs::remove_dir_all(&path);
        // 创建精确 fixture 目录。
        fs::create_dir(&path)
            // 创建失败时给出测试诊断。
            .unwrap_or_else(|error| panic!("fixture directory creation failed: {error}"));
        // 返回目录所有者。
        Self { path }
    }

    // 组合当前 fixture 内的文件路径。
    fn join(&self, name: &str) -> PathBuf {
        // 仅拼接固定测试文件名。
        self.path.join(name)
    }
}

// 作用域结束时删除完整自有 fixture。
impl Drop for FixtureDirectory {
    // 清理测试目录。
    fn drop(&mut self) {
        // 只删除当前实例创建的精确目录。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 向已预留 staging 写入固定测试字节。
fn write_staged(staged: &StagedFile, bytes: &[u8]) {
    // 覆盖零长度 reservation 文件。
    fs::write(staged.path(), bytes)
        // 写入失败时给出测试诊断。
        .unwrap_or_else(|error| panic!("staged fixture write failed: {error}"));
}

// 统计 fixture 内尚未清理的 part 文件。
fn staging_count(directory: &Path) -> usize {
    // 枚举自有 fixture 目录。
    fs::read_dir(directory)
        // 枚举失败时给出测试诊断。
        .unwrap_or_else(|error| panic!("fixture enumeration failed: {error}"))
        // 只保留成功读取的目录项。
        .filter_map(Result::ok)
        // 只统计文件名含 part 标记的路径。
        .filter(|entry| entry.file_name().to_string_lossy().contains(".part"))
        // 返回残留数量。
        .count()
}

// 验证 CREATE_NEW reservation 在并发候选间保持唯一。
#[test]
fn reservations_are_collision_free_and_drop_cleaned() {
    // 创建自有 fixture。
    let fixture = FixtureDirectory::new("reserve");
    // 定义共同目标。
    let output = fixture.join("capture.mp4");
    // 创建第一个 reservation。
    let first = StagedFile::reserve(&output)
        // reservation 必须成功。
        .unwrap_or_else(|error| panic!("first reservation failed: {error:?}"));
    // 创建第二个 reservation。
    let second = StagedFile::reserve(&output)
        // 第二个 reservation 必须选择不同路径。
        .unwrap_or_else(|error| panic!("second reservation failed: {error:?}"));
    // 两个 staging 路径必须不同。
    assert_ne!(first.path(), second.path());
    // 两个 CREATE_NEW 文件必须同时存在。
    assert_eq!(staging_count(&fixture.path), 2);
    // 释放两个未提交所有者。
    drop((first, second));
    // Drop 必须清除全部 staging。
    assert_eq!(staging_count(&fixture.path), 0);
}

// 验证首次提交建立目标并清除 staging。
#[test]
fn first_commit_is_atomic_and_cleanup_complete() {
    // 创建自有 fixture。
    let fixture = FixtureDirectory::new("first");
    // 定义首次输出。
    let output = fixture.join("capture.mp4");
    // 预留 staging。
    let staged = StagedFile::reserve(&output)
        // reservation 必须成功。
        .unwrap_or_else(|error| panic!("reservation failed: {error:?}"));
    // 写入固定新内容。
    write_staged(&staged, b"new-video");
    // 未要求覆盖地提交首次产物。
    let evidence = staged
        // 执行 write-through 原子移动。
        .commit(false)
        // 提交必须成功。
        .unwrap_or_else(|error| panic!("first commit failed: {error:?}"));
    // 首次提交不得报告替换。
    assert!(!evidence.replaced_existing);
    // 目标必须包含完整新内容。
    assert_eq!(
        fs::read(&output).ok().as_deref(),
        Some(b"new-video".as_slice())
    );
    // 成功后不得留下 part 文件。
    assert_eq!(staging_count(&fixture.path), 0);
}

// 验证未确认覆盖拒绝且原件保持不变。
#[test]
fn unconfirmed_overwrite_preserves_original() {
    // 创建自有 fixture。
    let fixture = FixtureDirectory::new("refuse");
    // 定义既有输出。
    let output = fixture.join("capture.mp4");
    // 写入原始内容。
    fs::write(&output, b"original")
        // fixture 写入必须成功。
        .unwrap_or_else(|error| panic!("original write failed: {error}"));
    // 预留新 staging。
    let staged = StagedFile::reserve(&output)
        // reservation 必须成功。
        .unwrap_or_else(|error| panic!("reservation failed: {error:?}"));
    // 写入候选内容。
    write_staged(&staged, b"replacement");
    // 缺少覆盖许可时必须拒绝。
    assert_eq!(staged.commit(false), Err(AtomicFileError::TargetExists));
    // 原件必须保持字节不变。
    assert_eq!(
        fs::read(&output).ok().as_deref(),
        Some(b"original".as_slice())
    );
    // 被拒 staging 必须由 Drop 清理。
    assert_eq!(staging_count(&fixture.path), 0);
}

// 验证 reservation 后出现的目标按提交竞态拒绝。
#[test]
fn target_race_after_reservation_preserves_winner() {
    // 创建自有 fixture。
    let fixture = FixtureDirectory::new("race");
    // 定义初始不存在的输出。
    let output = fixture.join("capture.mp4");
    // 在目标缺失时预留 staging。
    let staged = StagedFile::reserve(&output)
        // reservation 必须成功。
        .unwrap_or_else(|error| panic!("reservation failed: {error:?}"));
    // 写入当前请求的候选内容。
    write_staged(&staged, b"candidate");
    // 模拟另一个提交者在 reservation 后赢得目标路径。
    fs::write(&output, b"race-winner")
        // 竞态 fixture 写入必须成功。
        .unwrap_or_else(|error| panic!("race winner write failed: {error}"));
    // 未确认覆盖时提交必须把竞态分类为 TargetExists。
    assert_eq!(staged.commit(false), Err(AtomicFileError::TargetExists));
    // 已建立的竞态赢家必须保持不变。
    assert_eq!(
        fs::read(&output).ok().as_deref(),
        Some(b"race-winner".as_slice())
    );
    // 被拒候选必须清理。
    assert_eq!(staging_count(&fixture.path), 0);
}

// 验证确认覆盖使用原子替换而非预先删除。
#[test]
fn confirmed_overwrite_replaces_existing_file() {
    // 创建自有 fixture。
    let fixture = FixtureDirectory::new("replace");
    // 定义既有输出。
    let output = fixture.join("capture.mp4");
    // 写入原始内容。
    fs::write(&output, b"original")
        // fixture 写入必须成功。
        .unwrap_or_else(|error| panic!("original write failed: {error}"));
    // 预留新 staging。
    let staged = StagedFile::reserve(&output)
        // reservation 必须成功。
        .unwrap_or_else(|error| panic!("reservation failed: {error:?}"));
    // 写入候选内容。
    write_staged(&staged, b"replacement");
    // 带覆盖许可执行原子替换。
    let evidence = staged
        // 只通过 Component 提交。
        .commit(true)
        // 替换必须成功。
        .unwrap_or_else(|error| panic!("replacement failed: {error:?}"));
    // 证据必须报告既有文件被替换。
    assert!(evidence.replaced_existing);
    // 目标必须完整切换为新内容。
    assert_eq!(
        fs::read(&output).ok().as_deref(),
        Some(b"replacement".as_slice())
    );
    // 替换后不得留下 part 文件。
    assert_eq!(staging_count(&fixture.path), 0);
}

// 验证提交前 staging 失效不会删除既有目标。
#[test]
fn failed_commit_never_deletes_original() {
    // 创建自有 fixture。
    let fixture = FixtureDirectory::new("failure");
    // 定义既有输出。
    let output = fixture.join("capture.mp4");
    // 写入原始内容。
    fs::write(&output, b"original")
        // fixture 写入必须成功。
        .unwrap_or_else(|error| panic!("original write failed: {error}"));
    // 预留新 staging。
    let staged = StagedFile::reserve(&output)
        // reservation 必须成功。
        .unwrap_or_else(|error| panic!("reservation failed: {error:?}"));
    // 写入候选内容。
    write_staged(&staged, b"replacement");
    // 保存精确 staging 路径用于故障注入。
    let staging_path = staged.path().to_path_buf();
    // 删除 staging 模拟 writer 完成前失败。
    fs::remove_file(&staging_path)
        // 故障注入必须成功。
        .unwrap_or_else(|error| panic!("staging fault injection failed: {error}"));
    // 即使已有覆盖许可也必须拒绝未建立候选。
    assert_eq!(staged.commit(true), Err(AtomicFileError::InvalidStaging));
    // 原件不得因为失败路径被预先删除。
    assert_eq!(
        fs::read(&output).ok().as_deref(),
        Some(b"original".as_slice())
    );
    // 故障路径不得留下 part 文件。
    assert_eq!(staging_count(&fixture.path), 0);
}
