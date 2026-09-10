//! 管理录制视频与分析目录的同批 staging、安装和回滚。

// 导入精确文件系统操作、路径与唯一序列。
use std::{
    // 执行只针对自有路径的文件系统操作。
    fs,
    // 分类目录名称碰撞。
    io::ErrorKind,
    // 保存最终与 staging 路径。
    path::{Path, PathBuf},
    // 生成进程内唯一目录序列。
    sync::atomic::{AtomicU64, Ordering},
    // 加入低碰撞时间戳。
    time::{SystemTime, UNIX_EPOCH},
};

// 导入原子视频文件生命周期。
use super::atomic_file::{AtomicCommitEvidence, AtomicFileError, StagedFile};

// 保存进程内目录序列。
static DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

// 定义 Component 自有的封闭错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecordingArtifactsError {
    // 最终路径或候选类型无效。
    InvalidDestination,
    // 无法创建独占分析 staging。
    StagingCreationFailed,
    // worker 产生了协议外或不完整分析候选。
    InvalidCandidate,
    // 分析产物无法安装。
    InstallFailed,
    // 安装失败后无法恢复旧分析产物。
    RollbackFailed,
    // 视频原子提交失败。
    Video(AtomicFileError),
}

// 返回双产物成功提交证据。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RecordingCommitEvidence {
    // 标记 MP4 是否替换了既有普通文件。
    pub(crate) replaced_existing_video: bool,
    // 标记分析目录是否在本次首次建立。
    pub(crate) created_analysis_directory: bool,
}

// 持有父 Module 独占的视频与分析候选。
pub(crate) struct RecordingArtifacts {
    // 保存视频同目录原子 staging。
    video: Option<StagedFile>,
    // 保存最终分析目录。
    analysis_destination: PathBuf,
    // 保存本次独占分析 staging。
    analysis_staging: PathBuf,
    // 保存覆盖事务中创建的独占备份目录。
    backup: Option<PathBuf>,
}

// 提供录制多产物生命周期操作。
impl RecordingArtifacts {
    // 在最终目标相邻位置预留全部候选。
    pub(crate) fn reserve(
        // 接收最终 MP4 路径。
        video_destination: &Path,
        // 接收最终分析目录。
        analysis_destination: &Path,
    ) -> Result<Self, RecordingArtifactsError> {
        // 先取得视频 staging，后续失败由其 RAII 清理。
        let video = StagedFile::reserve(video_destination)
            // 保留精确原子文件错误。
            .map_err(RecordingArtifactsError::Video)?;
        // 分析目录必须具有明确且已经存在的父目录。
        let parent = analysis_destination
            // 读取语法父目录。
            .parent()
            // 拒绝根或缺失父目录。
            .filter(|value| !value.as_os_str().is_empty())
            // 映射为无效目标。
            .ok_or(RecordingArtifactsError::InvalidDestination)?;
        // 父路径必须是不跟随链接的真实目录。
        let metadata = fs::symlink_metadata(parent)
            // 无法检查时拒绝。
            .map_err(|_| RecordingArtifactsError::InvalidDestination)?;
        // 链接和非目录都不能作为递归清理边界。
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            // 返回稳定目标错误。
            return Err(RecordingArtifactsError::InvalidDestination);
        }
        // 创建与最终分析目录同父的独占候选目录。
        let analysis_staging = create_owned_directory(parent, "analysis-staging")?;
        // 返回全部候选的唯一所有者。
        Ok(Self {
            // 保存视频候选。
            video: Some(video),
            // 冻结最终分析目录。
            analysis_destination: analysis_destination.to_path_buf(),
            // 保存分析候选目录。
            analysis_staging,
            // 提交前尚未创建备份。
            backup: None,
        })
    }

    // 借用 worker 可写的视频 staging 路径。
    pub(crate) fn video_path(&self) -> &Path {
        // reservation 生命周期仍由本实例持有。
        self.video
            // 提交前必然存在。
            .as_ref()
            // 暴露只读路径借用。
            .map(StagedFile::path)
            // 缺失仅表示内部生命周期错误。
            .unwrap_or(Path::new(""))
    }

    // 借用 worker 可写的分析 staging 目录。
    pub(crate) fn analysis_path(&self) -> &Path {
        // 不转移递归清理所有权。
        &self.analysis_staging
    }

    // 校验候选后先安装可回滚分析文件，最后原子提交 MP4。
    pub(crate) fn commit(
        // 消耗唯一候选所有权。
        mut self,
        // 接收上层已经解析的覆盖许可。
        overwrite: bool,
    ) -> Result<RecordingCommitEvidence, RecordingArtifactsError> {
        // 在触碰最终目录前验证 worker 只能产生封闭分析文件集。
        let candidates = validate_candidates(&self.analysis_staging)?;
        // 安装分析目录并保存回滚状态。
        let created_analysis_directory = self.install_analysis(&candidates, overwrite)?;
        // 取得视频候选唯一所有权。
        let video = self
            // 取出 Option 以便原子提交消费。
            .video
            // 从持有者中移除。
            .take()
            // 缺失表示内部候选无效。
            .ok_or(RecordingArtifactsError::InvalidCandidate)?;
        // MP4 是事务最后一个提交点。
        let video_commit = video.commit(overwrite);
        // 视频提交失败时必须恢复分析产物。
        let AtomicCommitEvidence { replaced_existing } = match video_commit {
            // 成功后可以永久保留已安装分析文件。
            Ok(evidence) => evidence,
            // 原子视频失败时回滚分析安装。
            Err(error) => {
                // 恢复失败意味着公开结果不确定，使用更强错误。
                if self
                    .rollback_analysis(created_analysis_directory, &candidates)
                    .is_err()
                {
                    // 阻止把原始视频错误误报为完整回滚。
                    return Err(RecordingArtifactsError::RollbackFailed);
                }
                // 返回精确视频提交错误。
                return Err(RecordingArtifactsError::Video(error));
            }
        };
        // 成功后删除只包含旧自有分析文件的备份目录。
        if let Some(backup) = self.backup.take() {
            // 仅递归删除本实例创建的精确备份目录。
            let _ = fs::remove_dir_all(backup);
        }
        // 返回双产物提交证据。
        Ok(RecordingCommitEvidence {
            // 传播视频替换事实。
            replaced_existing_video: replaced_existing,
            // 传播分析目录建立事实。
            created_analysis_directory,
        })
    }

    // 安装候选分析文件并保留可恢复状态。
    fn install_analysis(
        // 借用事务持有者。
        &mut self,
        // 接收已验证的固定文件名列表。
        candidates: &[String],
        // 接收独立覆盖许可。
        overwrite: bool,
    ) -> Result<bool, RecordingArtifactsError> {
        // 检查最终分析目标的当前非跟随状态。
        match fs::symlink_metadata(&self.analysis_destination) {
            // 既有目标必须是真实目录且已获得覆盖许可。
            Ok(metadata) => {
                // 拒绝链接和非目录。
                if metadata.file_type().is_symlink() || !metadata.is_dir() || !overwrite {
                    // 返回稳定目标错误。
                    return Err(RecordingArtifactsError::InvalidDestination);
                }
                // 创建同父独占备份目录。
                let parent = self
                    // 读取已在 reserve 验证的父目录。
                    .analysis_destination
                    // 取得父路径。
                    .parent()
                    // 生命周期内必然存在。
                    .ok_or(RecordingArtifactsError::InvalidDestination)?;
                // 保存备份目录供失败回滚与成功清理。
                self.backup = Some(create_owned_directory(parent, "analysis-backup")?);
                // 把既有工具自有文件移动到独占备份。
                // 备份中途失败时先恢复已经移动的旧文件。
                if self.backup_owned_files().is_err() {
                    // 只恢复备份，不删除尚未安装的最终候选名。
                    self.restore_backup()?;
                    // 返回稳定安装失败。
                    return Err(RecordingArtifactsError::InstallFailed);
                }
                // 把全部已验证候选移动到最终目录。
                if self.move_candidates(candidates).is_err() {
                    // 部分安装失败时立即恢复。
                    self.rollback_analysis(false, candidates)?;
                    // 返回稳定安装错误。
                    return Err(RecordingArtifactsError::InstallFailed);
                }
                // 标记复用了既有目录。
                Ok(false)
            }
            // 不存在时用同卷目录 rename 原子建立。
            Err(error) if error.kind() == ErrorKind::NotFound => {
                // 原子移动整个候选目录到最终位置。
                fs::rename(&self.analysis_staging, &self.analysis_destination)
                    // 竞态或移动失败保持未提交。
                    .map_err(|_| RecordingArtifactsError::InstallFailed)?;
                // 清空 staging，防止 Drop 删除已提交目录。
                self.analysis_staging = PathBuf::new();
                // 标记首次建立目录。
                Ok(true)
            }
            // 其他状态无法安全判断。
            Err(_) => Err(RecordingArtifactsError::InvalidDestination),
        }
    }

    // 把既有工具自有分析文件移动到独占备份。
    fn backup_owned_files(&self) -> Result<(), RecordingArtifactsError> {
        // 取得已创建备份目录。
        let backup = self
            // 借用备份路径。
            .backup
            // 转为引用。
            .as_ref()
            // 缺失表示内部生命周期错误。
            .ok_or(RecordingArtifactsError::InstallFailed)?;
        // 枚举最终目录中的直接子项。
        for entry in fs::read_dir(&self.analysis_destination)
            // 无法枚举时禁止继续。
            .map_err(|_| RecordingArtifactsError::InstallFailed)?
        {
            // 读取单个目录项。
            let entry = entry.map_err(|_| RecordingArtifactsError::InstallFailed)?;
            // 只处理工具明确拥有的文件名。
            let name = entry.file_name();
            // 将名称投影为 UTF-8 进行封闭匹配。
            let Some(name_text) = name.to_str() else {
                // 非 UTF-8 未拥有文件保持原位。
                continue;
            };
            // 跳过调用方未拥有的文件。
            if !is_owned_name(name_text) {
                // 保留未知文件不变。
                continue;
            }
            // 既有自有目标也必须是真实普通文件。
            let metadata = fs::symlink_metadata(entry.path())
                // 无法检查时失败闭合。
                .map_err(|_| RecordingArtifactsError::InstallFailed)?;
            // 禁止移动链接、目录或特殊文件。
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                // 返回无效最终目录。
                return Err(RecordingArtifactsError::InvalidDestination);
            }
            // 移入本次独占备份目录。
            fs::rename(entry.path(), backup.join(name))
                // 失败时由上层回滚已移动文件。
                .map_err(|_| RecordingArtifactsError::InstallFailed)?;
        }
        // 全部既有自有文件已备份。
        Ok(())
    }

    // 把已验证候选逐项安装到最终目录。
    fn move_candidates(&self, candidates: &[String]) -> Result<(), RecordingArtifactsError> {
        // 使用已验证文件名，不重新接受目录项。
        for name in candidates {
            // 从独占 staging 移到真实最终目录。
            fs::rename(
                // 组合精确候选文件。
                self.analysis_staging.join(name),
                // 组合精确最终文件。
                self.analysis_destination.join(name),
            )
            // 保留安装失败分类。
            .map_err(|_| RecordingArtifactsError::InstallFailed)?;
        }
        // 全部候选已经安装。
        Ok(())
    }

    // 恢复分析目录到事务前状态。
    fn rollback_analysis(
        // 接收可变持有者以恢复 staging 生命周期。
        &mut self,
        // 标记本次是否首次建立整个目录。
        created_directory: bool,
        // 接收候选文件名列表。
        candidates: &[String],
    ) -> Result<(), RecordingArtifactsError> {
        // 首次建立时把完整目录移回原 staging 路径。
        if created_directory {
            // 创建新的唯一 staging 路径容器名称。
            let parent = self
                // 读取最终目录父路径。
                .analysis_destination
                // 取得父目录。
                .parent()
                // 缺失表示无法回滚。
                .ok_or(RecordingArtifactsError::RollbackFailed)?;
            // 先创建独占占位目录。
            let staging = create_owned_directory(parent, "analysis-rollback")?;
            // 删除空占位以允许目录 rename。
            fs::remove_dir(&staging).map_err(|_| RecordingArtifactsError::RollbackFailed)?;
            // 把已安装目录移回私有 staging。
            fs::rename(&self.analysis_destination, &staging)
                // 失败意味着结果不确定。
                .map_err(|_| RecordingArtifactsError::RollbackFailed)?;
            // 恢复 Drop 清理所有权。
            self.analysis_staging = staging;
            // 回滚完成。
            return Ok(());
        }
        // 删除本次可能已安装的候选文件。
        for name in candidates {
            // 组合精确工具自有目标。
            let path = self.analysis_destination.join(name);
            // 只在真实普通文件存在时删除。
            match fs::symlink_metadata(&path) {
                // 删除本次安装的普通文件。
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    // 无法删除表示无法完整回滚。
                    fs::remove_file(path)
                        // 返回强回滚错误。
                        .map_err(|_| RecordingArtifactsError::RollbackFailed)?;
                }
                // 缺失表示该候选尚未安装。
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                // 其他类型或读取失败都不能安全修复。
                _ => return Err(RecordingArtifactsError::RollbackFailed),
            }
        }
        // 将备份的旧自有文件恢复到最终目录。
        // 恢复全部旧自有文件。
        self.restore_backup()?;
        // 既有目录恢复完成。
        Ok(())
    }

    // 只把本次备份的旧自有文件恢复到最终目录。
    fn restore_backup(&self) -> Result<(), RecordingArtifactsError> {
        // 没有备份表示无需恢复。
        let Some(backup) = self.backup.as_ref() else {
            // 返回空恢复成功。
            return Ok(());
        };
        // 枚举只由本实例创建的备份目录。
        for entry in fs::read_dir(backup)
            // 无法枚举表示回滚失败。
            .map_err(|_| RecordingArtifactsError::RollbackFailed)?
        {
            // 读取备份项。
            let entry = entry.map_err(|_| RecordingArtifactsError::RollbackFailed)?;
            // 恢复到原文件名。
            fs::rename(
                entry.path(),
                self.analysis_destination.join(entry.file_name()),
            )
            // 任一恢复失败都升级为不确定结果。
            .map_err(|_| RecordingArtifactsError::RollbackFailed)?;
        }
        // 备份已完整恢复。
        Ok(())
    }
}

// 未提交候选的唯一所有者负责确定性清理。
impl Drop for RecordingArtifacts {
    // 回收私有 staging 与备份目录。
    fn drop(&mut self) {
        // 非空 staging 仍由本实例拥有。
        if !self.analysis_staging.as_os_str().is_empty() {
            // 只递归删除本实例原子创建的精确目录。
            let _ = fs::remove_dir_all(&self.analysis_staging);
        }
        // 未被成功清除的备份目录仍由本实例拥有。
        if let Some(backup) = self.backup.as_ref() {
            // 只递归删除本实例创建的精确备份。
            let _ = fs::remove_dir_all(backup);
        }
    }
}

// 创建指定父目录下的进程独占目录。
fn create_owned_directory(
    // 接收已验证真实父目录。
    parent: &Path,
    // 接收编译期用途标签。
    purpose: &str,
) -> Result<PathBuf, RecordingArtifactsError> {
    // 获取仅用于避免名称碰撞的时间戳。
    let stamp = SystemTime::now()
        // 转为 Unix 相对时长。
        .duration_since(UNIX_EPOCH)
        // 时钟异常使用零值但仍由序列保证进程内唯一。
        .map_or(0, |value| value.as_nanos());
    // 使用有界重试处理极少数名称碰撞。
    for _ in 0..32 {
        // 取得进程内唯一序列。
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 构造固定工具前缀的私有目录名。
        let path = parent.join(format!(
            // 名称不包含调用方命令或脚本片段。
            ".act-recording-{purpose}-{}-{stamp}-{sequence}",
            // 加入当前进程 ID。
            std::process::id(),
        ));
        // 以原子 create_dir 取得唯一所有权。
        match fs::create_dir(&path) {
            // 成功后返回精确目录。
            Ok(()) => return Ok(path),
            // 名称碰撞时继续下一个序列。
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            // 其他错误失败闭合。
            Err(_) => return Err(RecordingArtifactsError::StagingCreationFailed),
        }
    }
    // 有界重试耗尽时显式失败。
    Err(RecordingArtifactsError::StagingCreationFailed)
}

// 验证 worker 分析候选为固定直接子文件集合。
fn validate_candidates(directory: &Path) -> Result<Vec<String>, RecordingArtifactsError> {
    // 保存有界文件名列表。
    let mut names = Vec::new();
    // 保存关键帧数量。
    let mut frame_count = 0_usize;
    // 枚举父 Module 独占目录的直接子项。
    for entry in fs::read_dir(directory)
        // 无法枚举表示候选无效。
        .map_err(|_| RecordingArtifactsError::InvalidCandidate)?
    {
        // 读取目录项。
        let entry = entry.map_err(|_| RecordingArtifactsError::InvalidCandidate)?;
        // 文件名必须可跨 JSON 与 manifest 边界表示。
        let name = entry
            // 取得 OsString。
            .file_name()
            // 转为 UTF-8。
            .into_string()
            // 非 UTF-8 名称拒绝。
            .map_err(|_| RecordingArtifactsError::InvalidCandidate)?;
        // 只允许工具拥有的固定名称形状。
        if !is_owned_name(&name) {
            // 拒绝目录、日志或协议外文件。
            return Err(RecordingArtifactsError::InvalidCandidate);
        }
        // 每项必须是不跟随链接的普通文件。
        let metadata = fs::symlink_metadata(entry.path())
            // 无法检查时拒绝。
            .map_err(|_| RecordingArtifactsError::InvalidCandidate)?;
        // 链接、目录与空文件都无效。
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
            // 返回稳定候选错误。
            return Err(RecordingArtifactsError::InvalidCandidate);
        }
        // 统计有界关键帧。
        if name.starts_with("frame-") {
            // 增加关键帧计数。
            frame_count = frame_count.saturating_add(1);
        }
        // 保存已验证文件名。
        names.push(name);
    }
    // storyboard 与 manifest 各自必须恰好存在一次。
    let has_storyboard = names.iter().any(|name| name == "storyboard.png");
    // 检查 manifest 存在。
    let has_manifest = names.iter().any(|name| name == "manifest.json");
    // 关键帧必须落在公开上限内且总文件数精确匹配。
    if !has_storyboard
        // 要求 manifest。
        || !has_manifest
        // 至少需要一个分析关键帧。
        || !(1..=20).contains(&frame_count)
        // 禁止重复固定文件或其他同名形状。
        || names.len() != frame_count.saturating_add(2)
    {
        // 返回不完整候选错误。
        return Err(RecordingArtifactsError::InvalidCandidate);
    }
    // 使用稳定排序简化可重复提交。
    names.sort_unstable();
    // 返回完全验证的文件名列表。
    Ok(names)
}

// 判断文件名是否属于录制分析契约。
fn is_owned_name(name: &str) -> bool {
    // 固定聚合产物或严格 frame 前后缀属于工具。
    matches!(name, "storyboard.png" | "manifest.json")
        // 关键帧只允许既有固定前后缀。
        || (name.starts_with("frame-") && name.ends_with("ms.png"))
}

// 声明多产物安装与回滚测试。
#[cfg(test)]
// 保持测试靠近 Component 私有生命周期。
mod tests {
    // 导入当前 Component 私有接口。
    use super::*;

    // 创建进程独占测试根目录。
    fn fixture_root(name: &str) -> PathBuf {
        // 使用组件唯一序列避免并发碰撞。
        let sequence = DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 构造系统临时目录下的精确测试路径。
        let path = std::env::temp_dir().join(format!(
            // 固定测试前缀与调用方用例名。
            "act-recording-artifacts-{name}-{}-{sequence}",
            // 加入当前进程 ID。
            std::process::id(),
        ));
        // 创建唯一测试根。
        fs::create_dir(&path).unwrap_or_else(|error| panic!("fixture create failed: {error}"));
        // 返回测试拥有路径。
        path
    }

    // 写入最小但非空的固定分析候选集合。
    fn write_candidates(artifacts: &RecordingArtifacts, marker: &[u8]) {
        // 写入关键帧候选。
        fs::write(
            artifacts.analysis_path().join("frame-01-000000ms.png"),
            marker,
        )
        // 测试写入必须成功。
        .unwrap_or_else(|error| panic!("frame fixture failed: {error}"));
        // 写入 storyboard 候选。
        fs::write(artifacts.analysis_path().join("storyboard.png"), marker)
            // 测试写入必须成功。
            .unwrap_or_else(|error| panic!("storyboard fixture failed: {error}"));
        // 写入 manifest 候选。
        fs::write(artifacts.analysis_path().join("manifest.json"), marker)
            // 测试写入必须成功。
            .unwrap_or_else(|error| panic!("manifest fixture failed: {error}"));
        // 写入视频候选。
        fs::write(artifacts.video_path(), b"0000ftypfixture")
            // 测试写入必须成功。
            .unwrap_or_else(|error| panic!("video fixture failed: {error}"));
    }

    // 覆盖事务必须保留未拥有文件并替换全部自有分析文件。
    #[test]
    // 验证成功提交的所有权边界。
    fn commit_preserves_unowned_analysis_files() {
        // 创建独占测试根。
        let root = fixture_root("preserve");
        // 定义最终视频。
        let video = root.join("output.mp4");
        // 定义最终分析目录。
        let analysis = root.join("output.analysis");
        // 创建既有分析目录。
        fs::create_dir(&analysis).unwrap_or_else(|error| panic!("analysis create failed: {error}"));
        // 写入既有工具自有 manifest。
        fs::write(analysis.join("manifest.json"), b"old")
            // 测试写入必须成功。
            .unwrap_or_else(|error| panic!("old manifest failed: {error}"));
        // 写入调用方未拥有文件。
        fs::write(analysis.join("notes.txt"), b"keep")
            // 测试写入必须成功。
            .unwrap_or_else(|error| panic!("unowned fixture failed: {error}"));
        // 预留双产物候选。
        let artifacts = RecordingArtifacts::reserve(&video, &analysis)
            // reservation 必须成功。
            .unwrap_or_else(|error| panic!("reservation failed: {error:?}"));
        // 写入新候选。
        write_candidates(&artifacts, b"new");
        // 提交覆盖事务。
        let evidence = artifacts
            // 允许覆盖既有分析目录。
            .commit(true)
            // 提交必须成功。
            .unwrap_or_else(|error| panic!("commit failed: {error:?}"));
        // 首次视频不应标记替换。
        assert!(!evidence.replaced_existing_video);
        // 既有分析目录不应标记新建。
        assert!(!evidence.created_analysis_directory);
        // 未拥有文件必须逐字节保留。
        assert_eq!(
            fs::read(analysis.join("notes.txt")).ok().as_deref(),
            Some(&b"keep"[..])
        );
        // 自有 manifest 必须更新。
        assert_eq!(
            fs::read(analysis.join("manifest.json")).ok().as_deref(),
            Some(&b"new"[..])
        );
        // MP4 必须提交。
        assert_eq!(
            // 读取最终视频字节。
            fs::read(&video).ok().as_deref(),
            // 核对候选已经提交。
            Some(&b"0000ftypfixture"[..])
        );
        // 清理本测试精确根目录。
        fs::remove_dir_all(&root).unwrap_or_else(|error| panic!("fixture cleanup failed: {error}"));
    }

    // 视频提交失败时必须恢复旧分析文件且保留未拥有文件。
    #[test]
    // 验证事务最后提交点的回滚。
    fn video_commit_failure_rolls_back_analysis() {
        // 创建独占测试根。
        let root = fixture_root("rollback");
        // 定义最终视频。
        let video = root.join("output.mp4");
        // 定义最终分析目录。
        let analysis = root.join("output.analysis");
        // 创建既有分析目录。
        fs::create_dir(&analysis).unwrap_or_else(|error| panic!("analysis create failed: {error}"));
        // 写入旧自有 manifest。
        fs::write(analysis.join("manifest.json"), b"old")
            // 测试写入必须成功。
            .unwrap_or_else(|error| panic!("old manifest failed: {error}"));
        // 写入未拥有文件。
        fs::write(analysis.join("notes.txt"), b"keep")
            // 测试写入必须成功。
            .unwrap_or_else(|error| panic!("unowned fixture failed: {error}"));
        // 预留双产物候选。
        let artifacts = RecordingArtifacts::reserve(&video, &analysis)
            // reservation 必须成功。
            .unwrap_or_else(|error| panic!("reservation failed: {error:?}"));
        // 写入新候选。
        write_candidates(&artifacts, b"new");
        // 在最终视频位置制造目录竞态，迫使原子文件提交失败。
        fs::create_dir(&video).unwrap_or_else(|error| panic!("video race fixture failed: {error}"));
        // 提交必须返回视频目标错误。
        let error = artifacts.commit(true).err();
        // 回滚成功后保留原始视频错误分类。
        assert!(matches!(
            error,
            Some(RecordingArtifactsError::Video(
                AtomicFileError::InvalidDestination
            ))
        ));
        // 旧自有 manifest 必须恢复。
        assert_eq!(
            fs::read(analysis.join("manifest.json")).ok().as_deref(),
            Some(&b"old"[..])
        );
        // 未拥有文件必须保持。
        assert_eq!(
            fs::read(analysis.join("notes.txt")).ok().as_deref(),
            Some(&b"keep"[..])
        );
        // 新关键帧不得残留在最终目录。
        assert!(!analysis.join("frame-01-000000ms.png").exists());
        // 清理本测试精确根目录。
        fs::remove_dir_all(&root).unwrap_or_else(|error| panic!("fixture cleanup failed: {error}"));
    }
}
