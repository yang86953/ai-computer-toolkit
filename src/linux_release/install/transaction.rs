//! Linux Release System 的互斥锁、事务 journal 与持久化原语 Component。

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    thread,
    time::Duration,
};

use rustix::fs::{FlockOperation, OFlags, flock};
use serde::{Deserialize, Serialize};

use super::InstallLayout;
use crate::linux_release::{ReleaseResult, bounded_read, sync_directory};

const JOURNAL_CONTRACT: &str = "act/linux-install-transaction/v1";

/// 同用户所有 release mutation 共用的 advisory lock 所有者。
pub(super) struct InstallLock {
    _file: File,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Operation {
    Install,
    Rollback,
    Uninstall,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum State {
    Prepared,
    Committing,
    Committed,
}

/// journal 只保存固定根内相对事实，不保存授权、原生错误或任意路径。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Journal {
    contract_version: String,
    pub(super) operation: Operation,
    pub(super) state: State,
    pub(super) old_current: Option<String>,
    pub(super) old_rollback: Option<String>,
    pub(super) new_current: Option<String>,
    pub(super) new_rollback: Option<String>,
    pub(super) release_id: Option<String>,
    pub(super) staging_name: Option<String>,
    pub(super) archive_digest: Option<String>,
    pub(super) old_launcher_present: bool,
}

impl Journal {
    pub(super) fn install(
        old_current: Option<String>,
        old_rollback: Option<String>,
        release_id: String,
        archive_digest: String,
        old_launcher_present: bool,
    ) -> Self {
        Self {
            contract_version: JOURNAL_CONTRACT.to_owned(),
            operation: Operation::Install,
            state: State::Prepared,
            old_current: old_current.clone(),
            old_rollback,
            new_current: Some(format!("releases/{release_id}")),
            new_rollback: old_current,
            staging_name: Some(format!(".staging-{release_id}")),
            release_id: Some(release_id),
            archive_digest: Some(archive_digest),
            old_launcher_present,
        }
    }

    pub(super) fn rollback(current: String, rollback: String) -> Self {
        Self {
            contract_version: JOURNAL_CONTRACT.to_owned(),
            operation: Operation::Rollback,
            state: State::Prepared,
            old_current: Some(current.clone()),
            old_rollback: Some(rollback.clone()),
            new_current: Some(rollback),
            new_rollback: Some(current),
            release_id: None,
            staging_name: None,
            archive_digest: None,
            old_launcher_present: true,
        }
    }

    pub(super) fn uninstall(current: String, rollback: Option<String>) -> Self {
        Self {
            contract_version: JOURNAL_CONTRACT.to_owned(),
            operation: Operation::Uninstall,
            state: State::Prepared,
            old_current: Some(current),
            old_rollback: rollback,
            new_current: None,
            new_rollback: None,
            release_id: None,
            staging_name: None,
            archive_digest: None,
            old_launcher_present: true,
        }
    }

    pub(super) fn validate(&self) -> ReleaseResult<()> {
        if self.contract_version != JOURNAL_CONTRACT
            || self
                .old_current
                .iter()
                .chain(self.old_rollback.iter())
                .chain(self.new_current.iter())
                .chain(self.new_rollback.iter())
                .any(|target| !valid_release_target(target))
            || self
                .release_id
                .as_deref()
                .is_some_and(|value| !valid_release_id(value))
            || self
                .staging_name
                .as_deref()
                .is_some_and(|value| !value.starts_with(".staging-") || value.contains('/'))
            || self.archive_digest.as_deref().is_some_and(|value| {
                value.len() != 64
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            })
        {
            return Err("install transaction journal is invalid".to_owned());
        }
        match self.operation {
            Operation::Install
                if self.release_id.is_some()
                    && self.staging_name.is_some()
                    && self.archive_digest.is_some()
                    && self.new_current.is_some() =>
            {
                Ok(())
            }
            Operation::Rollback
                if self.release_id.is_none()
                    && self.staging_name.is_none()
                    && self.archive_digest.is_none()
                    && self.old_current.is_some()
                    && self.old_rollback.is_some()
                    && self.new_current == self.old_rollback
                    && self.new_rollback == self.old_current =>
            {
                Ok(())
            }
            Operation::Uninstall
                if self.release_id.is_none()
                    && self.staging_name.is_none()
                    && self.archive_digest.is_none()
                    && self.old_current.is_some()
                    && self.new_current.is_none()
                    && self.new_rollback.is_none()
                    && self.old_launcher_present =>
            {
                Ok(())
            }
            _ => Err("install transaction journal facts are inconsistent".to_owned()),
        }
    }
}

/// 先验证锁的 type/owner/mode/link，再以非阻塞独占 flock 拒绝并发操作。
pub(super) fn acquire(layout: &InstallLayout) -> ReleaseResult<InstallLock> {
    let path = &layout.lock;
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(OFlags::NOFOLLOW.bits() as i32)
        .open(path)
    {
        Ok(file) => {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|_| "install lock mode could not be set".to_owned())?;
            file.sync_all()
                .map_err(|_| "install lock could not be synchronized".to_owned())?;
            sync_directory(
                path.parent()
                    .ok_or_else(|| "install lock parent is invalid".to_owned())?,
            )?;
            file
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(OFlags::NOFOLLOW.bits() as i32)
            .open(path)
            .map_err(|_| "install lock is unavailable".to_owned())?,
        Err(_) => return Err("install lock could not be created".to_owned()),
    };
    validate_owned_regular(
        &file
            .metadata()
            .map_err(|_| "install lock metadata is unavailable")?,
        0o600,
        "install lock",
    )?;
    flock(&file, FlockOperation::NonBlockingLockExclusive)
        .map_err(|_| "another Linux release operation is active".to_owned())?;
    if let Ok(value) = std::env::var("ACT_RELEASE_TEST_HOLD_LOCK_MS") {
        let milliseconds = value
            .parse::<u64>()
            .map_err(|_| "test lock hold duration is invalid".to_owned())?;
        if milliseconds > 5_000 {
            return Err("test lock hold duration is out of range".to_owned());
        }
        thread::sleep(Duration::from_millis(milliseconds));
    }
    Ok(InstallLock { _file: file })
}

pub(super) fn read_journal(layout: &InstallLayout) -> ReleaseResult<Option<Journal>> {
    cleanup_or_reject_journal_staging(layout)?;
    match fs::symlink_metadata(&layout.journal) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("install transaction journal metadata is unavailable".to_owned()),
        Ok(_) => {
            let bytes = bounded_read_owned(
                &layout.journal,
                16 * 1024,
                0o600,
                "install transaction journal",
            )?;
            let journal: Journal = serde_json::from_slice(&bytes)
                .map_err(|_| "install transaction journal is malformed".to_owned())?;
            journal.validate()?;
            Ok(Some(journal))
        }
    }
}

pub(super) fn write_journal(layout: &InstallLayout, journal: &Journal) -> ReleaseResult<()> {
    journal.validate()?;
    let bytes = serde_json::to_vec(journal)
        .map_err(|_| "install transaction journal serialization failed".to_owned())?;
    durable_atomic_write(
        &layout.journal,
        &layout.journal_staging,
        &bytes,
        0o600,
        "journal",
    )
}

pub(super) fn remove_journal(layout: &InstallLayout) -> ReleaseResult<()> {
    checkpoint("journal:before-remove")?;
    fs::remove_file(&layout.journal)
        .map_err(|_| "install transaction journal could not be removed".to_owned())?;
    checkpoint("journal:after-remove")?;
    sync_directory_labeled(
        layout
            .journal
            .parent()
            .ok_or_else(|| "install transaction journal parent is invalid".to_owned())?,
        "journal-parent",
    )
}

/// 内容写入、chmod、文件 fsync、rename 和父目录 fsync 均可单独故障注入。
pub(super) fn durable_atomic_write(
    path: &Path,
    staging: &Path,
    bytes: &[u8],
    mode: u32,
    label: &str,
) -> ReleaseResult<()> {
    if fs::symlink_metadata(staging).is_ok() {
        return Err(format!("{label} staging already exists"));
    }
    checkpoint(&format!("{label}:before-write"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(OFlags::NOFOLLOW.bits() as i32)
        .open(staging)
        .map_err(|_| format!("{label} staging could not be created"))?;
    file.write_all(bytes)
        .map_err(|_| format!("{label} staging could not be written"))?;
    checkpoint(&format!("{label}:after-write"))?;
    checkpoint(&format!("{label}:before-chmod"))?;
    fs::set_permissions(staging, fs::Permissions::from_mode(mode))
        .map_err(|_| format!("{label} staging mode could not be set"))?;
    checkpoint(&format!("{label}:after-chmod"))?;
    checkpoint(&format!("{label}:before-file-fsync"))?;
    file.sync_all()
        .map_err(|_| format!("{label} staging could not be synchronized"))?;
    checkpoint(&format!("{label}:after-file-fsync"))?;
    validate_owned_regular(
        &file
            .metadata()
            .map_err(|_| format!("{label} staging metadata is unavailable"))?,
        mode,
        label,
    )?;
    checkpoint(&format!("{label}:before-rename"))?;
    fs::rename(staging, path).map_err(|_| format!("{label} commit failed"))?;
    checkpoint(&format!("{label}:after-rename"))?;
    sync_directory_labeled(
        path.parent()
            .ok_or_else(|| format!("{label} parent is invalid"))?,
        &format!("{label}-parent"),
    )
}

pub(super) fn sync_directory_labeled(path: &Path, label: &str) -> ReleaseResult<()> {
    checkpoint(&format!("{label}:before-dir-fsync"))?;
    sync_directory(path)?;
    checkpoint(&format!("{label}:after-dir-fsync"))
}

pub(super) fn checkpoint(point: &str) -> ReleaseResult<()> {
    if std::env::var("ACT_RELEASE_TEST_CRASH_AT").ok().as_deref() == Some(point) {
        std::process::exit(86);
    }
    if std::env::var("ACT_RELEASE_TEST_FAIL_AT").ok().as_deref() == Some(point) {
        return Err("release transaction fault was injected".to_owned());
    }
    Ok(())
}

pub(super) fn validate_owned_path(path: &Path, mode: u32, label: &str) -> ReleaseResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| format!("{label} is unavailable"))?;
    validate_owned_regular(&metadata, mode, label)
}

fn validate_owned_regular(metadata: &fs::Metadata, mode: u32, label: &str) -> ReleaseResult<()> {
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != rustix::process::getuid().as_raw()
        || metadata.permissions().mode() & 0o777 != mode
    {
        return Err(format!("{label} ownership or mode is invalid"));
    }
    Ok(())
}

fn bounded_read_owned(path: &Path, maximum: u64, mode: u32, label: &str) -> ReleaseResult<Vec<u8>> {
    validate_owned_path(path, mode, label)?;
    bounded_read(path, maximum)
}

fn cleanup_or_reject_journal_staging(layout: &InstallLayout) -> ReleaseResult<()> {
    match fs::symlink_metadata(&layout.journal_staging) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("install transaction staging metadata is unavailable".to_owned()),
        Ok(metadata) => {
            validate_owned_regular(&metadata, 0o600, "install transaction staging")?;
            fs::remove_file(&layout.journal_staging)
                .map_err(|_| "install transaction staging could not be removed".to_owned())?;
            sync_directory(
                layout
                    .journal_staging
                    .parent()
                    .ok_or_else(|| "install transaction staging parent is invalid".to_owned())?,
            )
        }
    }
}

fn valid_release_target(value: &str) -> bool {
    let Some(identifier) = value.strip_prefix("releases/") else {
        return false;
    };
    valid_release_id(identifier)
}

fn valid_release_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}
