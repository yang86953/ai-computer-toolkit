//! 在固定真实目录中原子持久化有界 sequence execution 记录。

// 导入文件、非跟随 Windows 打开与路径原语。
use std::{
    // 导入精确文件系统操作。
    fs::{self, OpenOptions},
    // 导入完整读取所需接口。
    io::Read,
    // 导入 Windows 文件属性与打开选项扩展。
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    // 导入稳定路径类型。
    path::{Path, PathBuf},
};

// 导入统一原子提交与 sequence identity 原语。
use super::{
    // 复用同目录 staging、durable flush 与 write-through rename。
    atomic_file::{AtomicFileError, StagedFile},
    // 复用 forgotten index 唯一字节预算。
    sequence_execution_forgotten::MAX_SEQUENCE_FORGOTTEN_INDEX_BYTES,
    // 复用 execution identity 唯一格式权威。
    sequence_execution_identity::{execution_fingerprint, execution_id_from_fingerprint},
};

// 固定单条完整 journal 文档最大字节数。
pub(crate) const MAX_SEQUENCE_EXECUTION_RECORD_BYTES: usize = 35_651_584;
// 固定一个 journal 最多保留的活动 execution 数量。
pub(crate) const MAX_SEQUENCE_EXECUTION_RECORDS: usize = 4_096;
// 固定启动扫描的 final 与 stale staging 总项目预算。
const MAX_SEQUENCE_JOURNAL_DIRECTORY_ENTRIES: usize = 8_193;
// 固定 Windows reparse point 属性值。
const FILE_ATTRIBUTE_REPARSE_POINT_VALUE: u32 = 0x0000_0400;
// 固定 Windows 非跟随打开 reparse point 标志。
const FILE_FLAG_OPEN_REPARSE_POINT_VALUE: u32 = 0x0020_0000;
// 固定永久 tombstone index 文件名。
const FORGOTTEN_INDEX_FILE_NAME: &str = "forgotten-v1.json";

// 表示 sequence journal 封闭且不泄漏路径的失败集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SequenceExecutionJournalError {
    // journal 根目录不可用或不再可信。
    Unavailable,
    // execution identity 不是 canonical s2:q。
    InvalidExecution,
    // 目录中存在未知、链接、reparse 或特殊项目。
    InvalidEntry,
    // 目录扫描超过固定项目预算。
    TooManyEntries,
    // 活动 execution 数量超过固定容量。
    TooManyRecords,
    // 单条记录为空或超过固定字节预算。
    RecordTooLarge,
    // 无法完整、非跟随读取既有记录。
    ReadFailed,
    // 无法建立或写入 staging。
    WriteFailed,
    // 无法 durable flush 或原子提交完整记录。
    CommitFailed,
    // 无法清理 stale staging 或精确记录。
    CleanupFailed,
}

// 保存从固定文件名绑定并严格加载的一条 journal 文档。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SequenceExecutionJournalDocument {
    // 保存由文件名恢复的 canonical execution identity。
    execution_id: String,
    // 保存尚待 Workflow Module 严格解码的完整 JSON 字节。
    bytes: Vec<u8>,
}

// 为 journal 文档提供只读投影。
impl SequenceExecutionJournalDocument {
    // 返回由固定文件名绑定的 execution identity。
    pub(crate) fn execution_id(&self) -> &str {
        // 借用 canonical identity。
        &self.execution_id
    }

    // 返回完整且有界的记录字节。
    pub(crate) fn bytes(&self) -> &[u8] {
        // 借用已读取文档。
        &self.bytes
    }
}

// 保存一次严格目录扫描得到的全部持久事实。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SequenceExecutionJournalContents {
    // 保存按 execution identity 排序的活动记录。
    documents: Vec<SequenceExecutionJournalDocument>,
    // 保存可选完整 forgotten index 字节。
    forgotten_index: Option<Vec<u8>>,
}

// 为扫描结果提供只读领域无关投影。
impl SequenceExecutionJournalContents {
    // 返回全部活动 execution 文档。
    pub(crate) fn documents(&self) -> &[SequenceExecutionJournalDocument] {
        // 借用有界排序切片。
        &self.documents
    }

    // 返回可选完整 forgotten index 字节。
    pub(crate) fn forgotten_index(&self) -> Option<&[u8]> {
        // 借用已非跟随读取的完整字节。
        self.forgotten_index.as_deref()
    }
}

// 拥有一个已由上层固定选择的 sequence execution journal 目录。
#[derive(Clone, Debug)]
pub(crate) struct SequenceExecutionJournal {
    // 保存固定真实目录路径。
    directory: PathBuf,
}

// 提供 journal 原子提交、严格扫描与精确删除。
impl SequenceExecutionJournal {
    // 打开一个已经存在的真实、非 reparse journal 目录。
    pub(crate) fn open(directory: &Path) -> Result<Self, SequenceExecutionJournalError> {
        // journal 自身必须先通过非跟随目录验证。
        validate_directory(directory)?;
        // 冻结由上层选择的精确路径。
        Ok(Self {
            // 不从环境变量派生任何部分。
            directory: directory.to_path_buf(),
        })
    }

    // 原子建立或替换一条完整 execution 记录。
    pub(crate) fn persist(
        // 借用 journal 所有者。
        &self,
        // 接收 canonical execution identity。
        execution_id: &str,
        // 接收 Module 已完成严格编码的完整记录。
        bytes: &[u8],
    ) -> Result<(), SequenceExecutionJournalError> {
        // 生成只由 canonical identity 决定的固定目标。
        let destination = self.record_path(execution_id)?;
        // 委托统一有界原子提交。
        self.persist_document(&destination, bytes, MAX_SEQUENCE_EXECUTION_RECORD_BYTES)
    }

    // 原子建立或替换完整 forgotten tombstone index。
    pub(crate) fn persist_forgotten_index(
        // 借用 journal 所有者。
        &self,
        // 接收 Forgotten Component 已严格编码的完整索引。
        bytes: &[u8],
    ) -> Result<(), SequenceExecutionJournalError> {
        // 只使用固定保留文件名。
        let destination = self.directory.join(FORGOTTEN_INDEX_FILE_NAME);
        // 委托统一有界原子提交。
        self.persist_document(&destination, bytes, MAX_SEQUENCE_FORGOTTEN_INDEX_BYTES)
    }

    // 严格加载全部 final 记录并清理可识别的 stale staging。
    pub(crate) fn load(
        // 借用 journal 所有者。
        &self,
    ) -> Result<SequenceExecutionJournalContents, SequenceExecutionJournalError> {
        // 每次扫描前重新验证固定目录。
        validate_directory(&self.directory)?;
        // 打开精确目录枚举器。
        let entries = fs::read_dir(&self.directory)
            // 不泄漏目录路径或系统错误。
            .map_err(|_| SequenceExecutionJournalError::Unavailable)?;
        // 预留不超过活动 execution 上限的结果容器。
        let mut documents = Vec::new();
        // 初始尚未发现 forgotten index。
        let mut forgotten_index = None;
        // 记录 final、staging 与未知项目总数。
        let mut entry_count = 0_usize;
        // 逐项执行非跟随验证。
        for entry in entries {
            // 饱和计数防止异常目录造成回绕。
            entry_count = entry_count.saturating_add(1);
            // 超出固定启动预算立即失败闭合。
            if entry_count > MAX_SEQUENCE_JOURNAL_DIRECTORY_ENTRIES {
                // 禁止无界枚举。
                return Err(SequenceExecutionJournalError::TooManyEntries);
            }
            // 读取目录项但不公开底层错误。
            let entry = entry.map_err(|_| SequenceExecutionJournalError::ReadFailed)?;
            // 文件名必须可无损表达为固定 ASCII。
            let file_name = entry
                // 取得平台文件名。
                .file_name()
                // 转换为 UTF-8。
                .into_string()
                // 非 UTF-8 名称失败闭合。
                .map_err(|_| SequenceExecutionJournalError::InvalidEntry)?;
            // 非跟随读取项目元数据。
            let metadata = fs::symlink_metadata(entry.path())
                // 读取竞态保持封闭。
                .map_err(|_| SequenceExecutionJournalError::ReadFailed)?;
            // 所有可识别项目都必须是真实非 reparse 普通文件。
            if !is_plain_file(&metadata) {
                // 拒绝链接、目录与特殊项目。
                return Err(SequenceExecutionJournalError::InvalidEntry);
            }
            // 崩溃遗留自有 staging 不建立持久接受事实。
            if is_staging_file_name(&file_name) {
                // 只删除完整匹配固定名称的 stale staging。
                fs::remove_file(entry.path())
                    // 清理失败必须阻止恢复。
                    .map_err(|_| SequenceExecutionJournalError::CleanupFailed)?;
                // 不把 staging 当作 final record。
                continue;
            }
            // 固定保留文件保存永久去重 tombstone。
            if file_name == FORGOTTEN_INDEX_FILE_NAME {
                // 同一目录只能存在一个固定文件名，防御重复观察。
                if forgotten_index.is_some() {
                    // 拒绝歧义索引。
                    return Err(SequenceExecutionJournalError::InvalidEntry);
                }
                // 使用独立紧凑索引预算非跟随读取。
                forgotten_index = Some(read_bounded_file(
                    // 读取精确固定路径。
                    &entry.path(),
                    // 应用 forgotten index 上限。
                    MAX_SEQUENCE_FORGOTTEN_INDEX_BYTES,
                )?);
                // 不把索引计入活动 execution 容量。
                continue;
            }
            // 从固定 final 名称恢复 execution identity。
            let execution_id = execution_from_record_file_name(&file_name)
                // 未知名称与宽松别名失败闭合。
                .ok_or(SequenceExecutionJournalError::InvalidEntry)?;
            // final 数量不得超过活动容量。
            if documents.len() >= MAX_SEQUENCE_EXECUTION_RECORDS {
                // 固定容量耗尽而不是继续占用内存。
                return Err(SequenceExecutionJournalError::TooManyRecords);
            }
            // 通过非跟随 handle 完整读取有界记录。
            let bytes = read_bounded_file(
                // 读取精确 execution final。
                &entry.path(),
                // 应用完整 execution 记录预算。
                MAX_SEQUENCE_EXECUTION_RECORD_BYTES,
            )?;
            // 收集文件名绑定与完整字节。
            documents.push(SequenceExecutionJournalDocument {
                // 保存 canonical execution identity。
                execution_id,
                // 保存 Module 后续严格解码输入。
                bytes,
            });
        }
        // 使用 identity 排序消除文件系统枚举顺序差异。
        documents.sort_by(|left, right| left.execution_id.cmp(&right.execution_id));
        // 返回有限且经过文件边界验证的全部事实。
        Ok(SequenceExecutionJournalContents {
            // 保存排序活动记录。
            documents,
            // 保存可选永久 tombstone index。
            forgotten_index,
        })
    }

    // 删除一条已经由上层从权威索引撤销的记录。
    pub(crate) fn remove(
        // 借用 journal 所有者。
        &self,
        // 接收 canonical execution identity。
        execution_id: &str,
    ) -> Result<(), SequenceExecutionJournalError> {
        // 删除前重新验证固定目录未被替换。
        validate_directory(&self.directory)?;
        // 生成固定 final 路径。
        let path = self.record_path(execution_id)?;
        // 非跟随检查既有项目。
        match fs::symlink_metadata(&path) {
            // 真实非 reparse 普通文件允许精确删除。
            Ok(metadata) if is_plain_file(&metadata) => {}
            // 不存在已经满足幂等清理。
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            // 链接、目录、特殊项目或读取失败均拒绝。
            _ => return Err(SequenceExecutionJournalError::InvalidEntry),
        }
        // 删除精确 final 文件。
        fs::remove_file(path)
            // 不泄漏底层错误。
            .map_err(|_| SequenceExecutionJournalError::CleanupFailed)
    }

    // 将 canonical execution identity 映射为固定 ASCII final 路径。
    fn record_path(
        // 借用 journal 所有者。
        &self,
        // 接收待验证 identity。
        execution_id: &str,
    ) -> Result<PathBuf, SequenceExecutionJournalError> {
        // 严格提取 execution 指纹。
        let fingerprint = execution_fingerprint(execution_id)
            // 拒绝其他 opaque 类别和宽松文本。
            .ok_or(SequenceExecutionJournalError::InvalidExecution)?;
        // 仅拼接固定 ASCII 文件名。
        Ok(self.directory.join(format!("execution-{fingerprint}.json")))
    }

    // 以统一同目录 staging 原子提交一份有界文档。
    fn persist_document(
        // 借用 journal 所有者。
        &self,
        // 借用固定目标路径。
        destination: &Path,
        // 借用完整文档字节。
        bytes: &[u8],
        // 接收该文档类别的唯一字节上限。
        maximum_bytes: usize,
    ) -> Result<(), SequenceExecutionJournalError> {
        // 每次操作前重新验证固定目录未被替换。
        validate_directory(&self.directory)?;
        // 空文档与超限文档不得进入 staging。
        if bytes.is_empty() || bytes.len() > maximum_bytes {
            // 返回独立资源边界错误。
            return Err(SequenceExecutionJournalError::RecordTooLarge);
        }
        // 若目标已经存在则必须仍是真实普通文件。
        validate_optional_record(destination)?;
        // 以 CREATE_NEW 取得同目录 staging 唯一所有权。
        let staged = StagedFile::reserve(destination)
            // reservation 失败不泄漏底层路径。
            .map_err(map_reservation_error)?;
        // 完整覆盖零长度 reservation。
        fs::write(staged.path(), bytes)
            // 写入失败由 staging 所有者清理。
            .map_err(|_| SequenceExecutionJournalError::WriteFailed)?;
        // 允许以 write-through 原子替换同一逻辑记录。
        staged
            // 只有成功返回才建立新持久事实。
            .commit(true)
            // durable flush 或 rename 失败统一封闭。
            .map_err(map_commit_error)?;
        // 报告完整文档已提交。
        Ok(())
    }
}

// 验证 journal 目录是真实、非 reparse 目录。
fn validate_directory(directory: &Path) -> Result<(), SequenceExecutionJournalError> {
    // 非跟随读取最终目录元数据。
    let metadata = fs::symlink_metadata(directory)
        // 读取失败统一视为 journal 不可用。
        .map_err(|_| SequenceExecutionJournalError::Unavailable)?;
    // 拒绝链接、junction、reparse point 与非目录项目。
    if metadata.file_type().is_symlink()
        // 普通目录形状必须成立。
        || !metadata.is_dir()
        // Windows reparse point 不得改变存储根。
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT_VALUE != 0
    {
        // 返回封闭目录错误。
        return Err(SequenceExecutionJournalError::Unavailable);
    }
    // 固定目录可信。
    Ok(())
}

// 验证可选既有目标不是链接、reparse 或特殊项目。
fn validate_optional_record(path: &Path) -> Result<(), SequenceExecutionJournalError> {
    // 非跟随检查精确目标。
    match fs::symlink_metadata(path) {
        // 既有目标必须是真实普通文件。
        Ok(metadata) if is_plain_file(&metadata) => Ok(()),
        // 不存在允许首次原子创建。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        // 其他形状或读取失败均拒绝。
        _ => Err(SequenceExecutionJournalError::InvalidEntry),
    }
}

// 判断元数据是否表示真实非 reparse 普通文件。
fn is_plain_file(metadata: &fs::Metadata) -> bool {
    // 同时验证符号链接、普通文件与 Windows 属性。
    !metadata.file_type().is_symlink()
        && metadata.is_file()
        && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT_VALUE == 0
}

// 使用非跟随、无共享 handle 完整读取一条有界记录。
fn read_bounded_file(
    // 借用精确 final 路径。
    path: &Path,
    // 接收当前文档类别的唯一字节上限。
    maximum_bytes: usize,
) -> Result<Vec<u8>, SequenceExecutionJournalError> {
    // 打开 reparse point 本身而不是跟随目标。
    let file = OpenOptions::new()
        // 只请求读取权限。
        .read(true)
        // 恢复期间不允许其他句柄修改或删除精确记录。
        .share_mode(0)
        // 禁止打开过程跟随最终 reparse point。
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT_VALUE)
        // 打开精确 final 路径。
        .open(path)
        // 不泄漏底层打开错误。
        .map_err(|_| SequenceExecutionJournalError::ReadFailed)?;
    // 从已打开 handle 重新核对文件事实。
    let metadata = file
        // 查询绑定到同一 handle 的元数据。
        .metadata()
        // 查询失败保持封闭。
        .map_err(|_| SequenceExecutionJournalError::ReadFailed)?;
    // 只接受有内容且不超限的真实普通文件。
    if !is_plain_file(&metadata)
        // 空记录从不建立恢复事实。
        || metadata.len() == 0
        // 先以元数据拒绝明显超限文档。
        || metadata.len() > maximum_bytes as u64
    {
        // 区分普通文件大小与形状错误。
        return if is_plain_file(&metadata) {
            // 普通文件的空或超限属于记录预算错误。
            Err(SequenceExecutionJournalError::RecordTooLarge)
        } else {
            // 链接或特殊项目属于目录污染。
            Err(SequenceExecutionJournalError::InvalidEntry)
        };
    }
    // 以已验证元数据容量预留有限缓冲区。
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    // 最多读取上限加一字节以检测并发增长。
    file
        // 限制读取器而不信任旧元数据。
        .take((maximum_bytes + 1) as u64)
        // 完整读取受限字节。
        .read_to_end(&mut bytes)
        // I/O 失败保持封闭。
        .map_err(|_| SequenceExecutionJournalError::ReadFailed)?;
    // 防御读时增长和异常空读。
    if bytes.is_empty() || bytes.len() > maximum_bytes {
        // 返回固定记录预算错误。
        return Err(SequenceExecutionJournalError::RecordTooLarge);
    }
    // 返回完整拥有型文档。
    Ok(bytes)
}

// 从固定 final 文件名恢复 canonical execution identity。
fn execution_from_record_file_name(file_name: &str) -> Option<String> {
    // 去除固定前缀与后缀。
    let fingerprint = file_name
        // 要求 sequence execution 前缀。
        .strip_prefix("execution-")?
        // 要求 JSON 后缀。
        .strip_suffix(".json")?;
    // 只通过 identity Component 组合 canonical 文本。
    execution_id_from_fingerprint(fingerprint)
}

// 识别统一 StagedFile 生成的 sequence execution staging 名称。
fn is_staging_file_name(file_name: &str) -> bool {
    // execution 与 forgotten index 各使用固定 staging 形状。
    is_execution_staging_file_name(file_name) || is_forgotten_staging_file_name(file_name)
}

// 识别 execution final 对应的统一 staging 名称。
fn is_execution_staging_file_name(file_name: &str) -> bool {
    // 去除 staging 点与固定目标前缀。
    let Some(rest) = file_name.strip_prefix(".execution-") else {
        // 非自有前缀不是可清理 staging。
        return false;
    };
    // 拆分指纹、目标扩展、进程、序列、part 与保留扩展。
    let parts = rest.split('.').collect::<Vec<_>>();
    // 严格验证完整固定六段形状。
    parts.len() == 6
        // 指纹必须形成 canonical execution identity。
        && execution_id_from_fingerprint(parts[0]).is_some()
        // 原目标扩展名固定为 JSON。
        && parts[1] == "json"
        // 进程段必须是非空十进制。
        && is_ascii_decimal(parts[2])
        // 序列段必须是非空十进制。
        && is_ascii_decimal(parts[3])
        // staging 标记固定。
        && parts[4] == "part"
        // 保留容器扩展名固定。
        && parts[5] == "json"
}

// 识别 forgotten index 对应的统一 staging 名称。
fn is_forgotten_staging_file_name(file_name: &str) -> bool {
    // 去除固定目标与 staging 点前缀。
    let Some(rest) = file_name.strip_prefix(".forgotten-v1.json.") else {
        // 非自有前缀不是可清理 staging。
        return false;
    };
    // 拆分进程、序列、part 与保留扩展。
    let parts = rest.split('.').collect::<Vec<_>>();
    // 严格验证固定四段形状。
    parts.len() == 4
        // 进程段必须是非空十进制。
        && is_ascii_decimal(parts[0])
        // 序列段必须是非空十进制。
        && is_ascii_decimal(parts[1])
        // staging 标记固定。
        && parts[2] == "part"
        // 保留容器扩展名固定。
        && parts[3] == "json"
}

// 验证非空十进制名称段。
fn is_ascii_decimal(value: &str) -> bool {
    // 要求至少一个字符且全部为 ASCII 数字。
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

// 映射 staging reservation 失败。
fn map_reservation_error(error: AtomicFileError) -> SequenceExecutionJournalError {
    // reservation 阶段只公开写入类别。
    match error {
        // 目标边界无效。
        AtomicFileError::InvalidDestination
        // staging 创建失败。
        | AtomicFileError::StagingCreationFailed
        // 其余变体保持穷举兼容。
        | AtomicFileError::InvalidStaging
        // reserve 当前不会返回覆盖冲突。
        | AtomicFileError::TargetExists
        // reserve 当前不会执行同步。
        | AtomicFileError::SyncFailed
        // reserve 当前不会执行提交。
        | AtomicFileError::CommitFailed => SequenceExecutionJournalError::WriteFailed,
    }
}

// 映射 durable flush 与原子提交失败。
fn map_commit_error(error: AtomicFileError) -> SequenceExecutionJournalError {
    // 提交失败统一表示新持久事实未可靠建立。
    match error {
        // 提交前目标边界变化。
        AtomicFileError::InvalidDestination
        // reservation 变体保持穷举。
        | AtomicFileError::StagingCreationFailed
        // staging 被替换或丢失。
        | AtomicFileError::InvalidStaging
        // overwrite=true 不应返回未授权冲突。
        | AtomicFileError::TargetExists
        // durable flush 失败。
        | AtomicFileError::SyncFailed
        // write-through rename 失败。
        | AtomicFileError::CommitFailed => SequenceExecutionJournalError::CommitFailed,
    }
}

// 声明固定目录 journal 文件生命周期测试。
#[cfg(test)]
#[path = "sequence_execution_journal_tests.rs"]
mod tests;
