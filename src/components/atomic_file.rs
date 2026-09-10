//! 以同目录 CREATE_NEW staging 和 write-through rename 提交单个文件。

// 导入文件、路径、进程与原子序列工具。
use std::{
    // 导入保持原始文件名编码的字符串类型。
    ffi::OsString,
    // 导入保留 staging 所需文件接口。
    fs::{self, OpenOptions},
    // 导入路径契约类型。
    path::{Path, PathBuf},
    // 导入进程内无锁唯一序列。
    sync::atomic::{AtomicU64, Ordering},
};

// 导入 Windows 原子移动接口与固定标志。
use windows::{
    // 导入原子文件移动 API。
    Win32::Storage::FileSystem::{
        // 导入只在确认覆盖时启用的替换标志。
        MOVEFILE_REPLACE_EXISTING,
        // 导入提交返回前落盘的 write-through 标志。
        MOVEFILE_WRITE_THROUGH,
        // 导入 Windows 原子提交函数。
        MoveFileExW,
    },
    // 导入仅限 Component 内部的宽字符串指针。
    core::PCWSTR,
};

// 限制同一次 reservation 的碰撞重试次数。
const MAXIMUM_STAGING_ATTEMPTS: u64 = 64;
// 为当前进程内 staging 名称提供单调唯一序列。
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 定义 Component 自有、无平台类型的封闭错误集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AtomicFileError {
    // 目标或父目录不满足真实文件边界。
    InvalidDestination,
    // 无法以 CREATE_NEW 取得 staging 所有权。
    StagingCreationFailed,
    // staged path 在提交前丢失或不再是普通文件。
    InvalidStaging,
    // 未授权覆盖时目标已经存在或在提交竞态中出现。
    TargetExists,
    // staged file 无法完成 durable flush。
    SyncFailed,
    // Windows 原子移动失败且结果未建立。
    CommitFailed,
}

// 返回已建立的单文件提交事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AtomicCommitEvidence {
    // 标记提交是否原子替换了既有普通文件。
    pub(crate) replaced_existing: bool,
}

// 独占一个同目录 staging 文件直到提交或作用域清理。
pub(crate) struct StagedFile {
    // 保存仅供 writer 使用的私有 staging 路径。
    path: PathBuf,
    // 冻结 reservation 对应的目标路径。
    destination: PathBuf,
}

// 提供 staging 生命周期与原子提交操作。
impl StagedFile {
    /// 将已编码字节写入本组件预留的 staging，沿用提交和清理生命周期。
    pub(crate) fn write_all(&self, bytes: &[u8]) -> Result<(), AtomicFileError> {
        use std::{io::Write, os::windows::fs::OpenOptionsExt};
        let mut file = OpenOptions::new()
            .write(true)
            .custom_flags(windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(&self.path)
            .map_err(|_| AtomicFileError::InvalidStaging)?;
        let metadata = file
            .metadata()
            .map_err(|_| AtomicFileError::InvalidStaging)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(AtomicFileError::InvalidStaging);
        }
        file.set_len(0)
            .map_err(|_| AtomicFileError::InvalidStaging)?;
        file.write_all(bytes)
            .map_err(|_| AtomicFileError::InvalidStaging)
    }

    // 在目标同目录以 CREATE_NEW 预留唯一 staging 文件。
    pub(crate) fn reserve(destination: &Path) -> Result<Self, AtomicFileError> {
        // 拒绝缺少文件名的目标。
        let file_name = destination
            // 读取最终文件名。
            .file_name()
            // 缺失时返回 Component 自有错误。
            .ok_or(AtomicFileError::InvalidDestination)?;
        // 相对单文件目标使用当前目录作为明确父目录。
        let parent = destination
            // 读取语法父目录。
            .parent()
            // 空父目录映射为当前目录。
            .filter(|value| !value.as_os_str().is_empty())
            // 保持既有相对路径兼容性。
            .unwrap_or_else(|| Path::new("."));
        // 读取父目录的非跟随元数据。
        let parent_metadata = fs::symlink_metadata(parent)
            // 不公开父目录或原生错误。
            .map_err(|_| AtomicFileError::InvalidDestination)?;
        // staging 父目录必须是真实目录，禁止符号链接改变提交卷。
        if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
            // 返回封闭目标错误。
            return Err(AtomicFileError::InvalidDestination);
        }
        // 保留目标扩展名，便于固定编码器识别容器。
        let extension = destination.extension();
        // 在有限次数内处理真实 CREATE_NEW 名称碰撞。
        for _ in 0..MAXIMUM_STAGING_ATTEMPTS {
            // 取得当前进程内唯一序列。
            let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            // 以点前缀隐藏 toolkit 私有 staging。
            let mut staging_name = OsString::from(".");
            // 保留原始目标文件名编码。
            staging_name.push(file_name);
            // 追加进程与序列身份以及 part 标记。
            staging_name.push(format!(".{}.{}.part", std::process::id(), sequence));
            // 容器扩展名存在时继续追加。
            if let Some(extension) = extension {
                // 插入扩展分隔符。
                staging_name.push(".");
                // 保留原始扩展编码。
                staging_name.push(extension);
            }
            // 只在目标同目录构造 staging 路径。
            let path = parent.join(staging_name);
            // 以 CREATE_NEW 等价语义抢占 staging 所有权。
            match OpenOptions::new()
                // 允许后续固定 writer 写入。
                .write(true)
                // 禁止覆盖任何碰撞文件。
                .create_new(true)
                // 创建精确 staging。
                .open(&path)
            {
                // 成功时立即关闭 reservation handle，由外部 writer 重新打开。
                Ok(file) => {
                    // 关闭 reservation handle。
                    drop(file);
                    // 返回唯一所有者。
                    return Ok(Self {
                        // 保存 staging 路径。
                        path,
                        // 冻结目标路径。
                        destination: destination.to_path_buf(),
                    });
                }
                // 名称碰撞时只尝试下一个内部序列。
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                // 其他创建失败不泄漏路径或系统错误。
                Err(_) => return Err(AtomicFileError::StagingCreationFailed),
            }
        }
        // 有界尝试耗尽后显式失败。
        Err(AtomicFileError::StagingCreationFailed)
    }

    // 只借用 staging 路径给固定 writer。
    pub(crate) fn path(&self) -> &Path {
        // 不转移 staging 生命周期所有权。
        &self.path
    }

    // 在 writer 完成后 durable flush 并原子提交。
    pub(crate) fn commit(
        // 消耗唯一 staging 所有权。
        mut self,
        // 接收上层已经解析的覆盖许可。
        overwrite: bool,
    ) -> Result<AtomicCommitEvidence, AtomicFileError> {
        // staged path 必须仍是非符号链接普通文件。
        let staged_metadata = fs::symlink_metadata(&self.path)
            // 丢失 staging 时拒绝提交。
            .map_err(|_| AtomicFileError::InvalidStaging)?;
        // 拒绝目录、链接或其他特殊文件。
        if staged_metadata.file_type().is_symlink() || !staged_metadata.is_file() {
            // 返回封闭 staging 错误。
            return Err(AtomicFileError::InvalidStaging);
        }
        // 读取提交前目标状态并区分不存在。
        let replaced_existing = match fs::symlink_metadata(&self.destination) {
            // 既有目标只允许真实普通文件。
            Ok(metadata) => {
                // 禁止覆盖链接、目录或特殊文件。
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    // 返回无效目标。
                    return Err(AtomicFileError::InvalidDestination);
                }
                // 缺少覆盖许可时不得触碰 staging 之外的文件。
                if !overwrite {
                    // 返回独立覆盖冲突。
                    return Err(AtomicFileError::TargetExists);
                }
                // 记录将发生替换。
                true
            }
            // 不存在目标允许首次提交。
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            // 其他元数据失败按无效目标处理。
            Err(_) => return Err(AtomicFileError::InvalidDestination),
        };
        // 以写权限重新打开已关闭的 staged file 以执行 sync_all。
        let staged_file = OpenOptions::new()
            // 请求 durable flush 所需写权限。
            .write(true)
            // 打开精确 staging。
            .open(&self.path)
            // 转换为 Component 自有同步错误。
            .map_err(|_| AtomicFileError::SyncFailed)?;
        // 在提交前把 staged 内容刷入稳定存储。
        staged_file
            // 使用 Rust 的 FlushFileBuffers 等价实现。
            .sync_all()
            // 不泄漏底层 I/O 错误。
            .map_err(|_| AtomicFileError::SyncFailed)?;
        // 关闭 staged handle，避免移动时留下私有写句柄。
        drop(staged_file);
        // 把 staging 路径编码为 NUL 结尾 UTF-16。
        let staged_wide = wide_path(&self.path);
        // 把目标路径编码为 NUL 结尾 UTF-16。
        let destination_wide = wide_path(&self.destination);
        // 默认首次提交只要求 write-through。
        let flags = if overwrite {
            // 已确认覆盖时原子替换并等待写入完成。
            MOVEFILE_WRITE_THROUGH | MOVEFILE_REPLACE_EXISTING
        } else {
            // 未确认覆盖时禁止 replace flag。
            MOVEFILE_WRITE_THROUGH
        };
        // 在同目录执行单个 Windows 原子移动。
        let committed = unsafe {
            // 原生类型只存在于当前 Component。
            MoveFileExW(
                // 传入 staging 宽字符串。
                PCWSTR(staged_wide.as_ptr()),
                // 传入目标宽字符串。
                PCWSTR(destination_wide.as_ptr()),
                // 传入封闭提交标志。
                flags,
            )
        };
        // 处理提交竞态与其他失败。
        if committed.is_err() {
            // 未确认路径若目标此时出现，返回覆盖冲突。
            if !overwrite && fs::symlink_metadata(&self.destination).is_ok() {
                // 保持目标不变并由 Drop 清理 staging。
                return Err(AtomicFileError::TargetExists);
            }
            // 其他失败保持未建立事实。
            return Err(AtomicFileError::CommitFailed);
        }
        // 清空路径，禁止 Drop 删除已经提交的目标。
        self.path = PathBuf::new();
        // 返回已建立提交证据。
        Ok(AtomicCommitEvidence { replaced_existing })
    }
}

// 未提交 staging 的唯一所有者负责确定性清理。
impl Drop for StagedFile {
    // 回收私有 staging 文件。
    fn drop(&mut self) {
        // 已提交实例不再拥有 staging 路径。
        if !self.path.as_os_str().is_empty() {
            // 只删除当前实例以 CREATE_NEW 取得的精确文件。
            let _ = fs::remove_file(&self.path);
        }
    }
}

// 把 Windows 路径编码为 NUL 结尾 UTF-16。
fn wide_path(path: &Path) -> Vec<u16> {
    // 导入 Windows OsStr 编码扩展。
    use std::os::windows::ffi::OsStrExt;
    // 保留原始 Windows 路径编码并附加终止符。
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

// 编译 Component 的独立文件生命周期测试。
#[cfg(test)]
#[path = "atomic_file_tests.rs"]
mod tests;
