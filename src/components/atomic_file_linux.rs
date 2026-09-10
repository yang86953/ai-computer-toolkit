//! Linux 同目录原子单文件提交 Component。

use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAXIMUM_STAGING_ATTEMPTS: u64 = 64;
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AtomicFileError {
    InvalidDestination,
    StagingCreationFailed,
    WriteFailed,
    TargetExists,
    SyncFailed,
    CommitFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AtomicCommitEvidence {
    pub(crate) replaced_existing: bool,
}

/// 唯一拥有 staging handle，并在 Drop 时回收未提交候选。
pub(crate) struct StagedFile {
    file: Option<File>,
    path: PathBuf,
    destination: PathBuf,
    parent: PathBuf,
}

impl StagedFile {
    pub(crate) fn reserve(destination: &Path) -> Result<Self, AtomicFileError> {
        let file_name = destination
            .file_name()
            .ok_or(AtomicFileError::InvalidDestination)?;
        let parent = destination
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent_metadata =
            fs::symlink_metadata(parent).map_err(|_| AtomicFileError::InvalidDestination)?;
        if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
            return Err(AtomicFileError::InvalidDestination);
        }
        for _ in 0..MAXIMUM_STAGING_ATTEMPTS {
            let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let mut staging_name = OsString::from(".");
            staging_name.push(file_name);
            staging_name.push(format!(".{}.{}.part", std::process::id(), sequence));
            let path = parent.join(staging_name);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        file: Some(file),
                        path,
                        destination: destination.to_path_buf(),
                        parent: parent.to_path_buf(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(AtomicFileError::StagingCreationFailed),
            }
        }
        Err(AtomicFileError::StagingCreationFailed)
    }

    pub(crate) fn write_all(&mut self, bytes: &[u8]) -> Result<(), AtomicFileError> {
        self.file
            .as_mut()
            .ok_or(AtomicFileError::WriteFailed)?
            .write_all(bytes)
            .map_err(|_| AtomicFileError::WriteFailed)
    }

    pub(crate) fn commit(
        mut self,
        overwrite: bool,
    ) -> Result<AtomicCommitEvidence, AtomicFileError> {
        let file = self.file.take().ok_or(AtomicFileError::WriteFailed)?;
        file.sync_all().map_err(|_| AtomicFileError::SyncFailed)?;
        drop(file);
        let replaced_existing = match fs::symlink_metadata(&self.destination) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(AtomicFileError::InvalidDestination);
                }
                if !overwrite {
                    return Err(AtomicFileError::TargetExists);
                }
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => return Err(AtomicFileError::InvalidDestination),
        };
        if overwrite {
            fs::rename(&self.path, &self.destination).map_err(|_| AtomicFileError::CommitFailed)?;
        } else {
            fs::hard_link(&self.path, &self.destination).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    AtomicFileError::TargetExists
                } else {
                    AtomicFileError::CommitFailed
                }
            })?;
            // 最终名称已原子建立；私有 staging 删除失败不改变已提交结果。
            let _ = fs::remove_file(&self.path);
        }
        // 最终名称已经原子建立；目录 fsync 仅增强掉电持久性，不把已完成提交伪报为未完成。
        let _ = File::open(&self.parent).and_then(|directory| directory.sync_all());
        self.path = PathBuf::new();
        Ok(AtomicCommitEvidence { replaced_existing })
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        self.file.take();
        if !self.path.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::StagedFile;

    fn fixture_directory() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ai-computer-toolkit-atomic-linux-{}-{}",
            std::process::id(),
            super::STAGING_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap_or_else(|error| panic!("建立 fixture 目录失败：{error}"));
        path
    }

    #[test]
    fn fixture_commits_new_file_without_overwrite_fallback() {
        let directory = fixture_directory();
        let destination = directory.join("result.png");
        let mut staged = StagedFile::reserve(&destination)
            .unwrap_or_else(|error| panic!("预留 staging 失败：{error:?}"));
        staged
            .write_all(b"fixture")
            .unwrap_or_else(|error| panic!("写入 staging 失败：{error:?}"));
        let evidence = staged
            .commit(false)
            .unwrap_or_else(|error| panic!("提交 staging 失败：{error:?}"));
        assert!(!evidence.replaced_existing);
        assert_eq!(
            fs::read(&destination).unwrap_or_else(|error| panic!("读取结果失败：{error}")),
            b"fixture"
        );
        fs::remove_file(destination).unwrap_or_else(|error| panic!("清理结果失败：{error}"));
        fs::remove_dir(directory).unwrap_or_else(|error| panic!("清理目录失败：{error}"));
    }
}
