//! 为长操作 registry 提供有界、原子、单记录持久化。

// 导入文件系统与路径原语。
use std::{
    // 导入 journal 文件操作。
    fs,
    // 导入稳定路径类型。
    path::{Path, PathBuf},
};

// 导入原子文件提交与 opaque ID 原语。
use super::{
    // 复用项目统一的同目录 write-through 原子提交。
    atomic_file::{AtomicFileError, StagedFile},
    // 严格验证 canonical operation handle。
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
};

// 固定单条 journal 文档的最大字节数。
pub(crate) const MAX_JOURNAL_DOCUMENT_BYTES: usize = 1_114_112;
// 固定一次启动最多检查的 final 与 staging 项目总数。
const MAX_JOURNAL_DIRECTORY_ENTRIES: usize = 256;

// 表示 Component 封闭且不泄漏路径的失败集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LongOperationJournalError {
    // journal 根目录不可用或不是普通目录。
    Unavailable,
    // operation handle 不是 canonical s2:o。
    InvalidOperation,
    // 目录中存在未知、链接或特殊项目。
    InvalidEntry,
    // 目录项目数量超过固定扫描预算。
    TooManyEntries,
    // 单条记录超过固定文档预算。
    RecordTooLarge,
    // 无法完整读取既有记录。
    ReadFailed,
    // 无法建立或写入 staging。
    WriteFailed,
    // 无法原子提交完整记录。
    CommitFailed,
    // 无法清理已撤销记录或 stale staging。
    CleanupFailed,
}

// 保存严格加载的一条私有 journal 文档。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LongOperationJournalDocument {
    // 保存从固定文件名恢复的 canonical operation handle。
    operation_id: String,
    // 保存尚待 Module 验证 schema 的完整 JSON 字节。
    bytes: Vec<u8>,
}

// 为 journal 文档提供只读投影。
impl LongOperationJournalDocument {
    // 返回由文件名绑定的 operation handle。
    pub(crate) fn operation_id(&self) -> &str {
        // 借用 canonical handle。
        &self.operation_id
    }

    // 返回完整且有界的文档字节。
    pub(crate) fn bytes(&self) -> &[u8] {
        // 借用已读取字节。
        &self.bytes
    }
}

// 拥有一个固定目录内的长操作单记录 journal。
#[derive(Clone, Debug)]
pub(crate) struct LongOperationJournal {
    // 保存由上层配置选择的私有目录。
    directory: PathBuf,
}

// 提供 journal 生命周期、原子提交与严格加载。
impl LongOperationJournal {
    // 打开或创建精确 journal 叶目录。
    pub(crate) fn open(directory: &Path) -> Result<Self, LongOperationJournalError> {
        // 要求上层先提供真实父目录，避免隐式创建宽路径。
        let parent = directory
            // 读取语法父目录。
            .parent()
            // 拒绝缺少父目录的配置。
            .ok_or(LongOperationJournalError::Unavailable)?;
        // 非跟随读取父目录元数据。
        let parent_metadata = fs::symlink_metadata(parent)
            // 封闭底层路径错误。
            .map_err(|_| LongOperationJournalError::Unavailable)?;
        // 父目录必须是真实目录。
        if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
            // 拒绝链接或特殊父项目。
            return Err(LongOperationJournalError::Unavailable);
        }
        // 只创建精确 journal 叶目录。
        match fs::create_dir(directory) {
            // 新目录创建成功。
            Ok(()) => {}
            // 既有目录进入后续严格验证。
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            // 其他创建失败保持封闭。
            Err(_) => return Err(LongOperationJournalError::Unavailable),
        }
        // 非跟随验证 journal 自身。
        let metadata = fs::symlink_metadata(directory)
            // 封闭底层读取失败。
            .map_err(|_| LongOperationJournalError::Unavailable)?;
        // journal 不得是链接或非目录项目。
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            // 拒绝不安全根目录。
            return Err(LongOperationJournalError::Unavailable);
        }
        // 返回精确目录所有者。
        Ok(Self {
            // 冻结私有目录路径。
            directory: directory.to_path_buf(),
        })
    }

    // 原子建立或替换一条完整记录。
    pub(crate) fn persist(
        // 借用 journal 所有者。
        &self,
        // 接收 canonical operation handle。
        operation_id: &str,
        // 接收 Module 已完成 schema 验证的 JSON 文档。
        bytes: &[u8],
    ) -> Result<(), LongOperationJournalError> {
        // 空文档与超限文档均不得进入 staging。
        if bytes.is_empty() || bytes.len() > MAX_JOURNAL_DOCUMENT_BYTES {
            // 返回独立预算错误。
            return Err(LongOperationJournalError::RecordTooLarge);
        }
        // 生成只由 canonical handle 决定的固定目标。
        let destination = self.record_path(operation_id)?;
        // 以 CREATE_NEW 取得唯一 staging 所有权。
        let staged = StagedFile::reserve(&destination)
            // 映射封闭原子文件错误。
            .map_err(map_reservation_error)?;
        // 完整覆盖零长度 reservation。
        fs::write(staged.path(), bytes)
            // 不泄漏 staging 路径与系统错误。
            .map_err(|_| LongOperationJournalError::WriteFailed)?;
        // 允许以 write-through 原子替换同一记录。
        staged
            // 提交后才建立新的持久事实。
            .commit(true)
            // 映射封闭提交错误。
            .map_err(map_commit_error)?;
        // 返回持久事实已建立。
        Ok(())
    }

    // 严格加载全部 final 记录并清理可识别的 stale staging。
    pub(crate) fn load(
        // 借用 journal 所有者。
        &self,
    ) -> Result<Vec<LongOperationJournalDocument>, LongOperationJournalError> {
        // 打开精确目录枚举器。
        let entries = fs::read_dir(&self.directory)
            // 不泄漏目录路径。
            .map_err(|_| LongOperationJournalError::Unavailable)?;
        // 预留不超过 registry 上限的结果容器。
        let mut documents = Vec::new();
        // 记录扫描项目数以封闭启动成本。
        let mut entry_count = 0_usize;
        // 逐项执行非跟随验证。
        for entry in entries {
            // 计入 final、staging 与未知项目。
            entry_count = entry_count.saturating_add(1);
            // 超出预算立即失败闭合。
            if entry_count > MAX_JOURNAL_DIRECTORY_ENTRIES {
                // 拒绝无界目录扫描。
                return Err(LongOperationJournalError::TooManyEntries);
            }
            // 读取目录项但不公开底层错误。
            let entry = entry.map_err(|_| LongOperationJournalError::ReadFailed)?;
            // 要求文件名可无损表示为固定 ASCII。
            let file_name = entry
                // 取得平台文件名。
                .file_name()
                // 转换为 UTF-8。
                .into_string()
                // 拒绝非 UTF-8 名称。
                .map_err(|_| LongOperationJournalError::InvalidEntry)?;
            // 非跟随读取项目类型。
            let metadata = fs::symlink_metadata(entry.path())
                // 封闭元数据读取失败。
                .map_err(|_| LongOperationJournalError::ReadFailed)?;
            // 所有可识别项目都必须是真实普通文件。
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                // 拒绝链接、目录与特殊文件。
                return Err(LongOperationJournalError::InvalidEntry);
            }
            // 崩溃遗留的自有 staging 不建立持久接受事实。
            if is_staging_file_name(&file_name) {
                // 只删除通过完整固定名称验证的 stale staging。
                fs::remove_file(entry.path())
                    // 清理失败需要结构化停止启动。
                    .map_err(|_| LongOperationJournalError::CleanupFailed)?;
                // 跳过非 final 项目。
                continue;
            }
            // 从固定 final 文件名恢复 operation handle。
            let operation_id = operation_from_record_file_name(&file_name)
                // 未知项目或宽松名称失败闭合。
                .ok_or(LongOperationJournalError::InvalidEntry)?;
            // 先以元数据拒绝明显超限记录。
            if metadata.len() > MAX_JOURNAL_DOCUMENT_BYTES as u64 {
                // 返回固定预算错误。
                return Err(LongOperationJournalError::RecordTooLarge);
            }
            // 完整读取单条文档。
            let bytes = fs::read(entry.path())
                // 不泄漏底层路径或 I/O 错误。
                .map_err(|_| LongOperationJournalError::ReadFailed)?;
            // 防御文件大小读取竞态。
            if bytes.is_empty() || bytes.len() > MAX_JOURNAL_DOCUMENT_BYTES {
                // 拒绝空记录或读时增长的超限记录。
                return Err(LongOperationJournalError::RecordTooLarge);
            }
            // 收集文件名绑定与文档字节。
            documents.push(LongOperationJournalDocument {
                // 保存 canonical handle。
                operation_id,
                // 保存完整文档。
                bytes,
            });
        }
        // 返回有限且经过文件边界验证的记录。
        Ok(documents)
    }

    // 删除一条已经从可查询索引撤销的记录。
    pub(crate) fn remove(
        // 借用 journal 所有者。
        &self,
        // 接收 canonical operation handle。
        operation_id: &str,
    ) -> Result<(), LongOperationJournalError> {
        // 生成固定 final 路径。
        let path = self.record_path(operation_id)?;
        // 非跟随检查既有项目。
        match fs::symlink_metadata(&path) {
            // 真实普通文件允许精确删除。
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
            // 不存在已经满足幂等清理。
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            // 链接、目录、特殊项目或读取失败均拒绝。
            _ => return Err(LongOperationJournalError::InvalidEntry),
        }
        // 删除精确 final 文件。
        fs::remove_file(path)
            // 不泄漏底层错误。
            .map_err(|_| LongOperationJournalError::CleanupFailed)
    }

    // 将 canonical handle 映射为固定 final 路径。
    fn record_path(&self, operation_id: &str) -> Result<PathBuf, LongOperationJournalError> {
        // 严格提取 operation 指纹。
        let fingerprint = operation_fingerprint(operation_id)
            // 拒绝其他 opaque 类别和宽松文本。
            .ok_or(LongOperationJournalError::InvalidOperation)?;
        // 仅拼接固定 ASCII 文件名。
        Ok(self.directory.join(format!("op-{fingerprint}.json")))
    }
}

// 严格提取 canonical operation handle 的十六进制指纹。
fn operation_fingerprint(operation_id: &str) -> Option<&str> {
    // 使用共享 opaque ID parser 验证完整外壳。
    let parsed = OpaqueTargetId::parse(operation_id)?;
    // 只接受 operation 类别。
    if parsed.kind() != OpaqueTargetKind::Operation {
        // 拒绝其他 target 类型。
        return None;
    }
    // 固定外壳验证后安全借用十六进制尾部。
    operation_id.strip_prefix("s2:o:")
}

// 从固定 final 文件名恢复 canonical operation handle。
fn operation_from_record_file_name(file_name: &str) -> Option<String> {
    // 去除固定前缀与后缀。
    let fingerprint = file_name
        // 要求 final 前缀。
        .strip_prefix("op-")?
        // 要求 JSON 后缀。
        .strip_suffix(".json")?;
    // 组合 canonical handle 候选。
    let operation_id = format!("s2:o:{fingerprint}");
    // 复用严格 parser 拒绝长度和字符别名。
    operation_fingerprint(&operation_id)?;
    // 返回已验证 handle。
    Some(operation_id)
}

// 识别项目统一 StagedFile 生成的长操作私有 staging 名称。
fn is_staging_file_name(file_name: &str) -> bool {
    // 去除 staging 点前缀。
    let Some(rest) = file_name.strip_prefix(".op-") else {
        // 非自有前缀不是可清理 staging。
        return false;
    };
    // 拆分固定六段形状。
    let parts = rest.split('.').collect::<Vec<_>>();
    // 严格验证指纹、容器、进程、序列与 part 标记。
    parts.len() == 6
        // 指纹必须形成 canonical operation handle。
        && operation_fingerprint(&format!("s2:o:{}", parts[0])).is_some()
        // 原目标扩展名固定为 JSON。
        && parts[1] == "json"
        // 进程段必须是非空十进制。
        && is_ascii_decimal(parts[2])
        // 序列段必须是非空十进制。
        && is_ascii_decimal(parts[3])
        // staging 标记固定。
        && parts[4] == "part"
        // 保留的容器扩展名固定。
        && parts[5] == "json"
}

// 验证非空十进制名称段。
fn is_ascii_decimal(value: &str) -> bool {
    // 要求至少一个字符且全部为 ASCII 数字。
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

// 映射 staging reservation 失败。
fn map_reservation_error(error: AtomicFileError) -> LongOperationJournalError {
    // 所有 reservation 失败都表示无法建立写入。
    match error {
        // 目标边界无效。
        AtomicFileError::InvalidDestination
        // staging 创建失败。
        | AtomicFileError::StagingCreationFailed
        // 其余变体在 reserve 当前不会返回但仍保持穷举。
        | AtomicFileError::InvalidStaging
        // 未授权覆盖冲突当前不会返回。
        | AtomicFileError::TargetExists
        // 同步失败当前不会返回。
        | AtomicFileError::SyncFailed
        // 提交失败当前不会返回。
        | AtomicFileError::CommitFailed => LongOperationJournalError::WriteFailed,
    }
}

// 映射原子提交失败。
fn map_commit_error(error: AtomicFileError) -> LongOperationJournalError {
    // 提交阶段失败统一表示新事实未可靠建立。
    match error {
        // 目标边界在提交前变化。
        AtomicFileError::InvalidDestination
        // reservation 失败当前不会到达提交阶段。
        | AtomicFileError::StagingCreationFailed
        // staging 被替换或丢失。
        | AtomicFileError::InvalidStaging
        // overwrite=true 时不应产生未授权覆盖冲突。
        | AtomicFileError::TargetExists
        // durable flush 失败。
        | AtomicFileError::SyncFailed
        // write-through move 失败。
        | AtomicFileError::CommitFailed => LongOperationJournalError::CommitFailed,
    }
}

// 声明 journal 文件边界回归测试。
#[cfg(test)]
// 将 fixture 放入独立文件控制生产 Component 规模。
#[path = "long_operation_journal_tests.rs"]
mod tests;
