//! 在当前用户 LocalAppData Known Folder 中打开 owner-only sequence execution journal。

// 导入固定路径类型。
use std::path::{Path, PathBuf};

// 导入 Windows Known Folder 与配对释放接口。
use windows::Win32::{
    // 释放 Shell 分配的路径缓冲区。
    System::Com::CoTaskMemFree,
    // 取得当前用户 LocalAppData 固定目录。
    UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
};

// 导入 owner-only 目录与原子 journal Component。
use super::{
    // 创建时即安装受保护 DACL。
    owner_only_directory_windows::{
        OwnerOnlyDirectoryError, ensure_owner_only_child, validate_real_directory,
    },
    // 打开严格 sequence execution journal。
    sequence_execution_journal::{SequenceExecutionJournal, SequenceExecutionJournalError},
};

// 固定 toolkit 私有目录名称。
const TOOLKIT_DIRECTORY_NAME: &str = "ai-computer-toolkit";
// 固定 sequence execution journal 版本目录名称。
const JOURNAL_DIRECTORY_NAME: &str = "sequence-execution-v1";

// 表示 Known Folder 与 owner-only journal 打开失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SequenceExecutionStorageError {
    // 当前用户 LocalAppData 无法认证。
    KnownFolderUnavailable,
    // 固定目录形状或 owner-only 权限不可用。
    Directory(OwnerOnlyDirectoryError),
    // 原子 journal 无法打开。
    Journal(SequenceExecutionJournalError),
}

// 将目录失败提升为存储失败。
impl From<OwnerOnlyDirectoryError> for SequenceExecutionStorageError {
    // 保留封闭错误类别。
    fn from(error: OwnerOnlyDirectoryError) -> Self {
        // 包装而不公开路径或平台错误。
        Self::Directory(error)
    }
}

// 将 journal 失败提升为存储失败。
impl From<SequenceExecutionJournalError> for SequenceExecutionStorageError {
    // 保留封闭错误类别。
    fn from(error: SequenceExecutionJournalError) -> Self {
        // 包装而不公开路径或平台错误。
        Self::Journal(error)
    }
}

// 打开当前用户固定 Known Folder 下的 owner-only journal。
pub(crate) fn open_sequence_execution_journal()
-> Result<SequenceExecutionJournal, SequenceExecutionStorageError> {
    // 解析不受环境变量覆盖的 LocalAppData Known Folder。
    let local_app_data = local_app_data_path()?;
    // 在固定根下建立 owner-only toolkit 与 journal 目录。
    open_sequence_execution_journal_under(&local_app_data)
}

// 在已经选择的真实 LocalAppData 根下建立固定私有目录链。
fn open_sequence_execution_journal_under(
    // 借用已由 Known Folder 或测试 fixture 选择的根。
    local_app_data: &Path,
) -> Result<SequenceExecutionJournal, SequenceExecutionStorageError> {
    // 根目录必须是真实非 reparse 目录。
    validate_real_directory(local_app_data)?;
    // 创建时即加固 toolkit 固定目录，阻止宽松父权限继续继承。
    let toolkit = ensure_owner_only_child(local_app_data, TOOLKIT_DIRECTORY_NAME)?;
    // 创建并加固最终版本化 journal 叶目录。
    let journal_directory = ensure_owner_only_child(&toolkit, JOURNAL_DIRECTORY_NAME)?;
    // 打开严格原子 journal。
    SequenceExecutionJournal::open(&journal_directory)
        // 保留封闭 journal 类别。
        .map_err(SequenceExecutionStorageError::from)
}

// 解析当前用户 LocalAppData Known Folder。
fn local_app_data_path() -> Result<PathBuf, SequenceExecutionStorageError> {
    // 请求当前用户上下文中的固定 Known Folder。
    let pointer = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None) }
        // 解析失败保持 broker 不可用。
        .map_err(|_| SequenceExecutionStorageError::KnownFolderUnavailable)?;
    // 空指针不能形成可信目录。
    if pointer.0.is_null() {
        // 返回封闭 Known Folder 错误。
        return Err(SequenceExecutionStorageError::KnownFolderUnavailable);
    }
    // 在释放前严格复制 UTF-16 路径。
    let value = unsafe { pointer.to_string() }
        // 异常 UTF-16 不得进入路径操作。
        .map_err(|_| SequenceExecutionStorageError::KnownFolderUnavailable);
    // 与 Shell allocator 配对释放缓冲区。
    unsafe { CoTaskMemFree(Some(pointer.0.cast())) };
    // 转换或拒绝空固定路径。
    value.and_then(|path| {
        // 空 Known Folder 不得进入路径操作。
        if path.is_empty() {
            // 返回封闭 Known Folder 错误。
            return Err(SequenceExecutionStorageError::KnownFolderUnavailable);
        }
        // 转换为私有 Windows 路径。
        Ok(PathBuf::from(path))
    })
}

// 声明固定 owner-only 存储根测试。
#[cfg(test)]
#[path = "sequence_execution_storage_windows_tests.rs"]
mod tests;
