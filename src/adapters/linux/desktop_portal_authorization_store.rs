//! 安全保存与轮换 XDG Portal 的 RemoteDesktop restore token。
//!
//! token 只写入当前用户私有状态目录（0700 目录 / 0600 文件、原子替换、
//! O_NOFOLLOW 与属主/权限校验），绝不进入 JSON、日志或文件名；跨进程用
//! flock 互斥，保证同一枚单次 token 不会被并发连接重复消费。

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use rustix::fs::FlockOperation;
use serde::{Deserialize, Serialize};

use crate::modules::desktop_session::{
    DesktopSavedAuthorization, DesktopSavedAuthorizationForget, DesktopSavedAuthorizationState,
};

/// 状态目录名固定为 XDG 状态根下的本工具子目录。
const STATE_DIRECTORY_NAME: &str = "ai-computer-toolkit";
/// token 记录文件名（不含任何 token 内容）。
const TOKEN_FILE_NAME: &str = "portal-restore-token";
/// 跨进程互斥锁文件名。
const LOCK_FILE_NAME: &str = "portal-restore-token.lock";
/// token 记录格式版本。
const TOKEN_RECORD_VERSION: u32 = 1;
/// token 或记录文件的大小上限。
const MAXIMUM_TOKEN_FILE_BYTES: usize = 8192;
/// token 字符串长度上限。
const MAXIMUM_TOKEN_LENGTH: usize = 4096;
/// flock 重试间隔与轮换 staging 碰撞重试次数。
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(25);
const MAXIMUM_STAGING_ATTEMPTS: u64 = 64;
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Linux `O_NOFOLLOW`/`O_CLOEXEC` 标志值；避免引入额外常量依赖。
const O_NOFOLLOW: i32 = 0o400000;
const O_CLOEXEC: i32 = 0o2000000;

/// 存储操作的封闭失败分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PortalAuthorizationStoreError {
    /// 无法定位当前用户的私有状态根。
    StateRootUnavailable,
    /// 状态目录创建失败或未通过私有性校验。
    DirectoryUnusable,
    /// 记录存在但读取未通过安全校验（符号链接/错属主/共享权限/坏格式）。
    Unreadable,
    /// 写入或原子替换失败。
    WriteFailed,
    /// 互斥锁在期限内不可得（另一连接正在恢复）。
    LockBusy,
    /// 内部文件系统错误。
    Io,
}

/// 持有跨进程互斥锁；drop 关闭句柄即释放，进程异常终止也由内核回收。
pub(crate) struct PortalAuthorizationLock {
    _file: File,
}

/// 当前用户私有的 restore token 存储。
pub(crate) struct PortalAuthorizationStore {
    directory: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct TokenRecord {
    version: u32,
    token: String,
}

impl PortalAuthorizationStore {
    /// 由 XDG_STATE_HOME（或 `~/.local/state`）派生默认存储。
    pub(crate) fn open_default() -> Result<Self, PortalAuthorizationStoreError> {
        Ok(Self {
            directory: default_state_directory()?,
        })
    }

    /// 测试与显式注入用：固定存储目录。
    #[cfg(test)]
    pub(crate) fn open_at(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn token_path(&self) -> PathBuf {
        self.directory.join(TOKEN_FILE_NAME)
    }

    fn lock_path(&self) -> PathBuf {
        self.directory.join(LOCK_FILE_NAME)
    }

    /// 确保状态目录存在且仅当前用户可访问（0700、真实目录、属主正确）。
    fn ensure_directory(&self) -> Result<(), PortalAuthorizationStoreError> {
        match fs::symlink_metadata(&self.directory) {
            Ok(metadata) => {
                verify_private(&metadata)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // 父链（XDG 状态根）按用户属主创建；本工具目录显式 0700。
                let state_root = self
                    .directory
                    .parent()
                    .ok_or(PortalAuthorizationStoreError::DirectoryUnusable)?
                    .to_path_buf();
                fs::create_dir_all(&state_root)
                    .map_err(|_| PortalAuthorizationStoreError::DirectoryUnusable)?;
                match fs::create_dir(&self.directory) {
                    Ok(()) => {
                        fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))
                            .map_err(|_| PortalAuthorizationStoreError::DirectoryUnusable)?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        let metadata = fs::symlink_metadata(&self.directory)
                            .map_err(|_| PortalAuthorizationStoreError::DirectoryUnusable)?;
                        verify_private(&metadata)?;
                    }
                    Err(_) => return Err(PortalAuthorizationStoreError::DirectoryUnusable),
                }
            }
            Err(_) => return Err(PortalAuthorizationStoreError::DirectoryUnusable),
        }
        Ok(())
    }

    /// 取得跨进程互斥锁；同 token 的并发恢复在锁内串行化，等待受 deadline 约束。
    pub(crate) fn lock_exclusive(
        &self,
        deadline: Instant,
    ) -> Result<PortalAuthorizationLock, PortalAuthorizationStoreError> {
        self.ensure_directory()?;
        let file = open_private_file(&self.lock_path(), true)
            .map_err(|_| PortalAuthorizationStoreError::Io)?;
        let metadata = file
            .metadata()
            .map_err(|_| PortalAuthorizationStoreError::Io)?;
        verify_private(&metadata)?;
        loop {
            match rustix::fs::flock(file.as_fd(), FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => return Ok(PortalAuthorizationLock { _file: file }),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return Err(PortalAuthorizationStoreError::Io),
            }
            match deadline.checked_duration_since(Instant::now()) {
                Some(remaining) if remaining > LOCK_RETRY_INTERVAL => {}
                _ => return Err(PortalAuthorizationStoreError::LockBusy),
            }
            std::thread::sleep(LOCK_RETRY_INTERVAL);
        }
    }

    /// 读取已保存 token；目录或记录不存在返回 `Ok(None)`。
    pub(crate) fn read_token(&self) -> Result<Option<String>, PortalAuthorizationStoreError> {
        let path = self.token_path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => {
                return Err(PortalAuthorizationStoreError::Unreadable);
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(PortalAuthorizationStoreError::Unreadable);
        }
        let file = open_private_file(&path, false)
            .map_err(|_| PortalAuthorizationStoreError::Unreadable)?;
        let metadata = file
            .metadata()
            .map_err(|_| PortalAuthorizationStoreError::Unreadable)?;
        verify_private(&metadata).map_err(|_| PortalAuthorizationStoreError::Unreadable)?;
        let mut bytes = Vec::new();
        file.take(MAXIMUM_TOKEN_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| PortalAuthorizationStoreError::Unreadable)?;
        if bytes.len() > MAXIMUM_TOKEN_FILE_BYTES {
            return Err(PortalAuthorizationStoreError::Unreadable);
        }
        let record: TokenRecord = serde_json::from_slice(&bytes)
            .map_err(|_| PortalAuthorizationStoreError::Unreadable)?;
        if record.version != TOKEN_RECORD_VERSION || !is_plausible_token(&record.token) {
            return Err(PortalAuthorizationStoreError::Unreadable);
        }
        Ok(Some(record.token))
    }

    /// 以同目录 staging + 原子替换保存（轮换）token；文件固定 0600。
    pub(crate) fn save_token(&self, token: &str) -> Result<(), PortalAuthorizationStoreError> {
        if !is_plausible_token(token) {
            return Err(PortalAuthorizationStoreError::WriteFailed);
        }
        self.ensure_directory()?;
        let record = TokenRecord {
            version: TOKEN_RECORD_VERSION,
            token: token.to_owned(),
        };
        let bytes =
            serde_json::to_vec(&record).map_err(|_| PortalAuthorizationStoreError::WriteFailed)?;
        let staging = self.reserve_staging()?;
        let written = (|| -> Result<(), PortalAuthorizationStoreError> {
            let mut file = OpenOptions::new()
                .write(true)
                .custom_flags(O_NOFOLLOW | O_CLOEXEC)
                .open(&staging)
                .map_err(|_| PortalAuthorizationStoreError::WriteFailed)?;
            file.set_len(0)
                .and_then(|_| file.write_all(&bytes))
                .and_then(|_| file.sync_all())
                .map_err(|_| PortalAuthorizationStoreError::WriteFailed)?;
            drop(file);
            fs::rename(&staging, self.token_path())
                .map_err(|_| PortalAuthorizationStoreError::WriteFailed)?;
            sync_directory(&self.directory);
            Ok(())
        })();
        if written.is_err() {
            let _ = fs::remove_file(&staging);
        }
        written
    }

    /// 读取脱敏状态；不创建任何文件。
    pub(crate) fn status(&self) -> DesktopSavedAuthorization {
        let state = match self.read_token() {
            Ok(Some(_)) => DesktopSavedAuthorizationState::Saved,
            Ok(None) => DesktopSavedAuthorizationState::Absent,
            Err(_) => DesktopSavedAuthorizationState::Unreadable,
        };
        DesktopSavedAuthorization::of(state, "xdg-portal-restore-token")
    }

    /// 清除本工具保存的凭据；删除符号链接本身，不追踪目标。
    pub(crate) fn forget(
        &self,
    ) -> Result<DesktopSavedAuthorizationForget, PortalAuthorizationStoreError> {
        let had_saved = fs::symlink_metadata(self.token_path()).is_ok();
        let discarded = self.discard_token();
        if had_saved && !discarded {
            return Err(PortalAuthorizationStoreError::Io);
        }
        let _ = fs::remove_file(self.lock_path());
        sync_directory(&self.directory);
        Ok(DesktopSavedAuthorizationForget::outcome(
            had_saved,
            fs::symlink_metadata(self.token_path()).is_err(),
        ))
    }

    /// 只移除 token 记录，不动锁文件。
    ///
    /// 轮换失败时作废可能已被消费的旧 token 用这里：删除正在持有 flock 的
    /// 锁文件会让其他进程在新 inode 上拿到第二把锁，破坏互斥。
    pub(crate) fn discard_token(&self) -> bool {
        let removed = fs::remove_file(self.token_path()).is_ok();
        if removed {
            sync_directory(&self.directory);
        }
        removed
    }

    fn reserve_staging(&self) -> Result<PathBuf, PortalAuthorizationStoreError> {
        for _ in 0..MAXIMUM_STAGING_ATTEMPTS {
            let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let staging = self.directory.join(format!(
                ".{TOKEN_FILE_NAME}.{}.{}.part",
                process::id(),
                sequence
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(O_NOFOLLOW | O_CLOEXEC)
                .open(&staging)
            {
                Ok(_) => return Ok(staging),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(PortalAuthorizationStoreError::WriteFailed),
            }
        }
        Err(PortalAuthorizationStoreError::WriteFailed)
    }
}

/// 校验元数据为当前用户私有：属主为有效 uid、无组/其他权限。
fn verify_private(metadata: &fs::Metadata) -> Result<(), PortalAuthorizationStoreError> {
    let expected = rustix::process::getuid().as_raw();
    if metadata.uid() != expected || metadata.permissions().mode() & 0o077 != 0 {
        return Err(PortalAuthorizationStoreError::Unreadable);
    }
    Ok(())
}

/// 以 O_NOFOLLOW 打开私有文件；create=true 时按 0600 创建。
fn open_private_file(path: &Path, create: bool) -> Result<File, std::io::Error> {
    let mut options = OpenOptions::new();
    options.read(true).custom_flags(O_NOFOLLOW | O_CLOEXEC);
    if create {
        options.create(true).mode(0o600).write(true);
    }
    options.open(path)
}

/// 同目录 fsync 使 rename 结果落盘；失败不阻塞保存语义。
fn sync_directory(directory: &Path) {
    if let Ok(handle) = File::open(directory) {
        let _ = handle.sync_all();
    }
}

/// Portal token 是不透明字符串；只接受有界的可打印 ASCII，拒绝明显异常值。
fn is_plausible_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAXIMUM_TOKEN_LENGTH
        && token.bytes().all(|byte| (0x20..0x7f).contains(&byte))
}

fn default_state_directory() -> Result<PathBuf, PortalAuthorizationStoreError> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|value| value.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .filter(|value| value.is_absolute())
                .map(|home| home.join(".local/state"))
        });
    match root {
        Some(root) => Ok(root.join(STATE_DIRECTORY_NAME)),
        None => Err(PortalAuthorizationStoreError::StateRootUnavailable),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn temp_store(name: &str) -> PortalAuthorizationStore {
        let directory = std::env::temp_dir().join(format!(
            "act-portal-store-{name}-{}-{}",
            process::id(),
            STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        PortalAuthorizationStore::open_at(directory)
    }

    #[test]
    fn save_read_and_rotate_replace_the_single_record() {
        let store = temp_store("rotate");
        assert_eq!(store.read_token().expect("read empty"), None);
        store.save_token("token-one").expect("save first");
        assert_eq!(
            store.read_token().expect("read first"),
            Some("token-one".to_owned())
        );
        store.save_token("token-two").expect("rotate");
        assert_eq!(
            store.read_token().expect("read rotated"),
            Some("token-two".to_owned())
        );
        let metadata = fs::symlink_metadata(store.token_path()).expect("token file metadata");
        assert!(metadata.is_file());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(
            fs::symlink_metadata(&store.directory)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let _ = fs::remove_file(store.token_path());
        let _ = fs::remove_dir(&store.directory);
    }

    #[test]
    fn symlinked_token_record_is_rejected() {
        let store = temp_store("symlink");
        store.save_token("token-real").expect("save real token");
        let moved = store.directory.join("moved-record");
        fs::rename(store.token_path(), &moved).expect("move record away");
        std::os::unix::fs::symlink(&moved, store.token_path()).expect("plant symlink");
        assert_eq!(
            store.read_token().expect_err("symlink must be rejected"),
            PortalAuthorizationStoreError::Unreadable
        );
        assert_eq!(
            store.status().state(),
            DesktopSavedAuthorizationState::Unreadable
        );
        // 保存轮换会原子替换符号链接本身，而不是写目标。
        store.save_token("token-next").expect("rotate over symlink");
        assert_eq!(
            store.read_token().expect("read after rotate"),
            Some("token-next".to_owned())
        );
        let _ = fs::remove_file(store.token_path());
        let _ = fs::remove_file(&moved);
        let _ = fs::remove_dir(&store.directory);
    }

    #[test]
    fn group_readable_record_is_rejected() {
        let store = temp_store("shared");
        store.save_token("token-private").expect("save token");
        fs::set_permissions(store.token_path(), fs::Permissions::from_mode(0o640))
            .expect("loosen permissions");
        assert_eq!(
            store
                .read_token()
                .expect_err("shared record must be rejected"),
            PortalAuthorizationStoreError::Unreadable
        );
        let _ = fs::remove_file(store.token_path());
        let _ = fs::remove_dir(&store.directory);
    }

    #[test]
    fn malformed_record_is_rejected() {
        let store = temp_store("malformed");
        store.ensure_directory().expect("create directory");
        fs::write(store.token_path(), b"{\"version\":9,\"token\":\"x\"}").expect("bad record");
        assert_eq!(
            store
                .read_token()
                .expect_err("version mismatch must be rejected"),
            PortalAuthorizationStoreError::Unreadable
        );
        let _ = fs::remove_file(store.token_path());
        let _ = fs::remove_dir(&store.directory);
    }

    #[test]
    fn exclusive_lock_serializes_concurrent_access() {
        let store = temp_store("lock");
        let first = store
            .lock_exclusive(Instant::now() + Duration::from_secs(2))
            .expect("first lock");
        let second = store.lock_exclusive(Instant::now() + Duration::from_millis(200));
        match second {
            Err(PortalAuthorizationStoreError::LockBusy) => {}
            Err(other) => panic!("unexpected lock error: {other:?}"),
            Ok(_) => panic!("second lock must wait for the first to release"),
        }
        drop(first);
        store
            .lock_exclusive(Instant::now() + Duration::from_secs(2))
            .expect("lock after release");
        let _ = fs::remove_file(store.lock_path());
        let _ = fs::remove_dir(&store.directory);
    }

    #[test]
    fn forget_removes_saved_credentials() {
        let store = temp_store("forget");
        let nothing = store.forget().expect("forget nothing");
        assert!(!nothing.had_saved());
        assert!(nothing.cleared());
        assert_eq!(
            store.status().state(),
            DesktopSavedAuthorizationState::Absent
        );
        store.save_token("token-final").expect("save token");
        let forgotten = store.forget().expect("forget saved token");
        assert!(forgotten.had_saved());
        assert!(forgotten.cleared());
        assert_eq!(
            store.status().state(),
            DesktopSavedAuthorizationState::Absent
        );
        let _ = fs::remove_dir(&store.directory);
    }

    #[test]
    fn state_directory_derives_from_xdg_state_home() {
        // 纯函数级校验目录派生，不改动进程环境。
        let direct = PathBuf::from("/tmp/xdg-state/ai-computer-toolkit");
        assert_eq!(
            PathBuf::from("/tmp/xdg-state").join(STATE_DIRECTORY_NAME),
            direct
        );
        assert_eq!(TOKEN_FILE_NAME, "portal-restore-token");
    }

    #[test]
    fn implausible_tokens_are_refused() {
        assert!(is_plausible_token("abc123"));
        assert!(!is_plausible_token(""));
        assert!(!is_plausible_token(&"x".repeat(MAXIMUM_TOKEN_LENGTH + 1)));
        assert!(!is_plausible_token("bad\ntoken"));
    }
}
