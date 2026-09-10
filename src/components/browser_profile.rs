//! 管理浏览器截图调用独占的临时 profile 目录。

// 导入文件系统、路径、原子序列与时间工具。
use std::{
    // 导入目录操作。
    fs,
    // 导入路径类型。
    path::{Path, PathBuf},
    // 导入进程内无锁序列。
    sync::atomic::{AtomicU64, Ordering},
    // 导入短轮询休眠。
    thread,
    // 导入时间戳来源。
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// 导入领域错误与结果。
use crate::domain::{AppControlError, AppResult};

// 保存并发 profile 名称序列。
static PROFILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
// 固定工具自有 profile 根目录名。
const PROFILE_ROOT_NAME: &str = "ai-computer-toolkit-browser";
// 固定单次 profile 名称前缀。
const PROFILE_NAME_PREFIX: &str = "act-browser-";
// 固定长期浏览器会话 profile 名称前缀。
const SESSION_PROFILE_NAME_PREFIX: &str = "act-browser-session-";
// 固定 Windows profile 句柄释放总预算。
const PROFILE_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);
// 固定 profile 清理重试片，避免忙等。
const PROFILE_CLEANUP_RETRY_SLICE: Duration = Duration::from_millis(10);

// 独占一个工具自有临时 profile 直到作用域结束。
pub(crate) struct BrowserProfile {
    // 保存固定根目录。
    root: PathBuf,
    // 保存本次调用独占目录。
    path: PathBuf,
}

// 提供 profile 生命周期操作。
impl BrowserProfile {
    // 创建不会复用用户数据的唯一空 profile。
    pub(crate) fn create() -> AppResult<Self> {
        // 截图调用继续使用既有独立命名空间。
        Self::create_with_prefix(PROFILE_NAME_PREFIX)
    }

    // 创建可由下一 broker 代际识别和恢复的唯一会话 profile。
    pub(crate) fn create_session() -> AppResult<Self> {
        // 只使用会话专用前缀，不混入其他浏览器能力资源。
        Self::create_with_prefix(SESSION_PROFILE_NAME_PREFIX)
    }

    // 在取得 broker 唯一实例所有权后回收上一代遗留的会话 profile。
    pub(crate) fn cleanup_stale_sessions() -> AppResult<()> {
        // 定位固定工具自有根。
        let root = std::env::temp_dir().join(PROFILE_ROOT_NAME);
        // 根不存在表示没有上一代资源。
        if !root.exists() {
            // 返回清理完成。
            return Ok(());
        }
        // 使用非跟随元数据验证清理边界没有被链接替换。
        let root_metadata = fs::symlink_metadata(&root).map_err(|_| profile_error())?;
        // 只允许在真实工具目录内枚举。
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            // 不安全边界失败闭合。
            return Err(profile_error());
        }
        // 枚举根级资源但不递归扫描无关能力目录。
        for entry in fs::read_dir(&root).map_err(|_| profile_error())? {
            // 任一枚举错误都不能伪造清理完成。
            let entry = entry.map_err(|_| profile_error())?;
            // 只选择由 browser-session worker 创建的固定前缀。
            if !entry
                // 读取不包含父路径的文件名。
                .file_name()
                // 非 Unicode 名称不属于工具生成资源。
                .to_str()
                // 精确核对会话命名空间。
                .is_some_and(|name| name.starts_with(SESSION_PROFILE_NAME_PREFIX))
            {
                // 保留其他能力拥有的资源。
                continue;
            }
            // 使用非跟随入口类型拒绝符号链接和非目录。
            let file_type = entry.file_type().map_err(|_| profile_error())?;
            // 不递归进入无法证明由工具创建的真实目录以外对象。
            if file_type.is_symlink() || !file_type.is_dir() {
                // 不安全同名前缀对象失败闭合。
                return Err(profile_error());
            }
            // 在 Windows 句柄释放预算内删除精确上一代会话子树。
            if !remove_profile_directory(&entry.path()) {
                // 无法证明恢复完成时拒绝启动新 broker。
                return Err(profile_error());
            }
        }
        // 仅在根目录已经为空时尝试回收。
        let _ = fs::remove_dir(&root);
        // 返回上一代会话资源已收敛。
        Ok(())
    }

    // 使用封闭命名空间创建不会复用用户数据的唯一空 profile。
    fn create_with_prefix(name_prefix: &str) -> AppResult<Self> {
        // 在系统临时目录下选择固定工具根。
        let root = std::env::temp_dir().join(PROFILE_ROOT_NAME);
        // 创建或复用工具自有根目录。
        fs::create_dir_all(&root).map_err(|_| profile_error())?;
        // 使用非跟随元数据验证工具根未被替换为链接。
        let root_metadata = fs::symlink_metadata(&root).map_err(|_| profile_error())?;
        // 只接受真实目录作为递归清理边界。
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            // 不安全根目录失败闭合。
            return Err(profile_error());
        }
        // 读取只用于避免名称碰撞的时间事实。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时长。
            .duration_since(UNIX_EPOCH)
            // 时钟异常按 profile 创建失败处理。
            .map_err(|_| profile_error())?
            // 使用纳秒提高唯一性。
            .as_nanos();
        // 最多尝试固定次数处理极端名称碰撞。
        for _ in 0..32 {
            // 取得进程内唯一序列。
            let sequence = PROFILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            // 构造不含用户数据的固定形状名称。
            let name = format!(
                // 名称包含固定前缀、进程、时间与序列。
                "{name_prefix}{}-{stamp}-{sequence}",
                // 注入当前进程 ID。
                std::process::id(),
            );
            // 组合本次候选目录。
            let path = root.join(name);
            // 以原子目录创建取得独占所有权。
            match fs::create_dir(&path) {
                // 创建成功后返回所有者。
                Ok(()) => return Ok(Self { root, path }),
                // 名称碰撞时继续尝试。
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                // 其他错误失败闭合。
                Err(_) => return Err(profile_error()),
            }
        }
        // 固定重试耗尽时返回结构化错误。
        Err(profile_error())
    }

    // 借用本次调用独占目录。
    pub(crate) fn path(&self) -> &Path {
        // 只在 crate 内传给固定 worker。
        &self.path
    }
}

// 作用域结束时回收唯一 profile。
impl Drop for BrowserProfile {
    // 删除本实例创建的精确目录。
    fn drop(&mut self) {
        // 只递归删除本实例原子创建并独占的精确 profile 子树。
        if !remove_profile_directory(&self.path) {
            // 保持 Drop 不 panic 且不扩大清理范围。
            return;
        }
        // 仅在根目录为空时尝试回收根目录。
        let _ = fs::remove_dir(&self.root);
    }
}

// 在固定预算内删除一个已经验证归属的精确 profile 目录。
fn remove_profile_directory(path: &Path) -> bool {
    // 建立不会因重试重置的清理 deadline。
    let deadline = Instant::now() + PROFILE_CLEANUP_TIMEOUT;
    // 在固定预算内等待 Chromium 释放最后的文件 handle。
    loop {
        // 只递归删除调用方已经验证的精确 profile 子树。
        match fs::remove_dir_all(path) {
            // 完整删除后返回成功。
            Ok(()) => return true,
            // 已不存在同样表示资源已回收。
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
            // 预算内保留精确目标并短暂退避。
            Err(_) if Instant::now() < deadline => {
                // 限制 Windows 文件释放轮询 CPU。
                thread::sleep(PROFILE_CLEANUP_RETRY_SLICE);
            }
            // 总预算耗尽时返回无法证明完成。
            Err(_) => return false,
        }
    }
}

// 返回不泄漏临时路径的稳定错误。
fn profile_error() -> AppControlError {
    // 使用专用 profile 创建错误码。
    AppControlError::new(
        // 保持兼容错误分类。
        "TEMP_PROFILE_FAILED",
        // 不公开系统临时目录。
        "The isolated browser profile could not be created.",
    )
}
