//! 在当前用户 LocalAppData Known Folder 中打开私有长操作 journal。

// 导入目录、Windows 元数据与路径工具。
use std::{
    // 导入精确目录生命周期操作。
    fs,
    // 读取 reparse point 属性。
    os::windows::fs::MetadataExt,
    // 保存 Known Folder 私有路径。
    path::{Path, PathBuf},
};

// 导入 Windows Known Folder 与配对释放接口。
use windows::Win32::{
    // 释放 Shell 分配的路径缓冲区。
    System::Com::CoTaskMemFree,
    // 取得当前用户 LocalAppData 固定目录。
    UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
};

// 导入原子 journal Component。
use super::long_operation_journal::{LongOperationJournal, LongOperationJournalError};

// 固定 toolkit 私有目录名称。
const TOOLKIT_DIRECTORY_NAME: &str = "ai-computer-toolkit";
// 固定长操作 journal 版本目录名称。
const JOURNAL_DIRECTORY_NAME: &str = "long-operation-v1";
// 固定 Windows reparse point 属性值。
const FILE_ATTRIBUTE_REPARSE_POINT_VALUE: u32 = 0x0000_0400;

// 表示 Known Folder 与 journal 打开阶段的封闭失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LongOperationStorageError {
    // 当前用户 LocalAppData 无法认证。
    KnownFolderUnavailable,
    // 固定目录不存在且无法创建。
    DirectoryUnavailable,
    // 固定目录是链接、reparse point 或特殊项目。
    DirectoryUntrusted,
    // 原子 journal 无法打开。
    Journal(LongOperationJournalError),
}

// 将 journal 失败提升为存储失败。
impl From<LongOperationJournalError> for LongOperationStorageError {
    // 保留封闭错误类别。
    fn from(error: LongOperationJournalError) -> Self {
        // 包装而不公开路径或平台错误。
        Self::Journal(error)
    }
}

// 打开当前用户固定 Known Folder 下的私有 journal。
pub(crate) fn open_long_operation_journal()
-> Result<LongOperationJournal, LongOperationStorageError> {
    // 解析不受环境变量覆盖的 LocalAppData Known Folder。
    let local_app_data = local_app_data_path()?;
    // 验证 Known Folder 自身是真实目录。
    validate_directory(&local_app_data)?;
    // 建立或验证 toolkit 固定目录。
    let toolkit = ensure_child_directory(&local_app_data, TOOLKIT_DIRECTORY_NAME)?;
    // 建立并验证最终 journal 叶目录，拒绝 junction 与 reparse point。
    let journal_directory = ensure_child_directory(&toolkit, JOURNAL_DIRECTORY_NAME)?;
    // 打开并验证最终 journal 目录。
    LongOperationJournal::open(&journal_directory).map_err(LongOperationStorageError::from)
}

// 解析当前用户 LocalAppData Known Folder。
fn local_app_data_path() -> Result<PathBuf, LongOperationStorageError> {
    // 请求当前用户上下文中的固定 Known Folder。
    let pointer = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None) }
        // 解析失败保持 broker 不可用。
        .map_err(|_| LongOperationStorageError::KnownFolderUnavailable)?;
    // 空指针不能形成可信目录。
    if pointer.0.is_null() {
        // 返回封闭 Known Folder 错误。
        return Err(LongOperationStorageError::KnownFolderUnavailable);
    }
    // 在释放前严格复制 UTF-16 路径。
    let value = unsafe { pointer.to_string() }
        // 异常 UTF-16 不得进入路径操作。
        .map_err(|_| LongOperationStorageError::KnownFolderUnavailable);
    // 与 Shell allocator 配对释放缓冲区。
    unsafe { CoTaskMemFree(Some(pointer.0.cast())) };
    // 转换或拒绝空固定路径。
    value.and_then(|path| {
        // 空 Known Folder 不得进入路径操作。
        if path.is_empty() {
            // 返回封闭 Known Folder 错误。
            return Err(LongOperationStorageError::KnownFolderUnavailable);
        }
        // 转换为私有 Windows 路径。
        Ok(PathBuf::from(path))
    })
}

// 在可信父目录中建立或验证一个固定单段子目录。
fn ensure_child_directory(
    // 借用已经验证的真实父目录。
    parent: &Path,
    // 接收编译期固定目录名称。
    child_name: &'static str,
) -> Result<PathBuf, LongOperationStorageError> {
    // 防御固定名称未来漂移成路径。
    if child_name.is_empty() || child_name.contains(['\\', '/']) {
        // 拒绝非单段固定名称。
        return Err(LongOperationStorageError::DirectoryUnavailable);
    }
    // 只在可信父目录下拼接固定单段。
    let directory = parent.join(child_name);
    // 尝试只创建精确目录。
    match fs::create_dir(&directory) {
        // 首次创建成功。
        Ok(()) => {}
        // 已存在进入严格验证。
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        // 其他失败保持封闭。
        Err(_) => return Err(LongOperationStorageError::DirectoryUnavailable),
    }
    // 验证最终目录不是链接或 reparse point。
    validate_directory(&directory)?;
    // 返回私有目录。
    Ok(directory)
}

// 非跟随验证一个真实、非 reparse 目录。
fn validate_directory(directory: &Path) -> Result<(), LongOperationStorageError> {
    // 读取不跟随最终链接的元数据。
    let metadata = fs::symlink_metadata(directory)
        // 读取失败保持目录不可用。
        .map_err(|_| LongOperationStorageError::DirectoryUnavailable)?;
    // 拒绝符号链接、junction 和非目录项目。
    if metadata.file_type().is_symlink()
        // 普通目录形状必须成立。
        || !metadata.is_dir()
        // Windows reparse point 不得改变存储根。
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT_VALUE != 0
    {
        // 返回独立不可信目录错误。
        return Err(LongOperationStorageError::DirectoryUntrusted);
    }
    // 返回目录可信。
    Ok(())
}

// 声明 Known Folder 存储边界回归测试。
#[cfg(test)]
// 将 fixture 放入独立文件控制 Component 规模。
#[path = "long_operation_storage_windows_tests.rs"]
mod tests;
