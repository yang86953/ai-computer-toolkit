//! 统一检查文件与目录输出目标的覆盖许可。

// 导入非跟随元数据、错误分类和路径类型。
use std::{fs, io::ErrorKind, path::Path};

// 定义 Component 自有、无平台类型的封闭错误集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputGuardError {
    // 既有输出需要独立覆盖许可。
    ConfirmationRequired,
    // 目标状态无法被可靠读取。
    InspectionFailed,
    // 既有目标不是允许覆盖的真实目标类型。
    InvalidTargetType,
}

// 描述分析目录在门禁通过后的稳定状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputDirectoryState {
    // 目录尚不存在。
    Missing,
    // 目录存在且为空。
    Empty,
    // 目录非空且已取得覆盖许可。
    NonEmptyConfirmed,
}

// 检查单文件输出是否可以进入 writer。
pub(crate) fn guard_file_output(
    // 接收不会被 Component 修改的输出路径。
    path: &Path,
    // 接收上层已经解析的独立覆盖许可。
    overwrite_confirmed: bool,
) -> Result<(), OutputGuardError> {
    // 使用非跟随元数据区分缺失、链接和真实文件。
    let metadata = match fs::symlink_metadata(path) {
        // 保存既有目标的非跟随元数据。
        Ok(metadata) => metadata,
        // 缺失目标允许首次输出。
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        // 其他检查失败必须失败闭合。
        Err(_) => return Err(OutputGuardError::InspectionFailed),
    };
    // 只允许覆盖非符号链接普通文件。
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        // 拒绝目录、链接和其他特殊目标。
        return Err(OutputGuardError::InvalidTargetType);
    }
    // 普通文件存在时必须取得独立覆盖许可。
    if !overwrite_confirmed {
        // 不触碰既有文件并要求确认。
        return Err(OutputGuardError::ConfirmationRequired);
    }
    // 已确认的真实普通文件允许进入具体 writer。
    Ok(())
}

// 检查多产物分析目录是否可以进入 writer。
pub(crate) fn guard_directory_output(
    // 接收不会被 Component 修改的目录路径。
    path: &Path,
    // 接收上层已经解析的独立覆盖许可。
    overwrite_confirmed: bool,
) -> Result<OutputDirectoryState, OutputGuardError> {
    // 使用非跟随元数据区分缺失、链接和真实目录。
    let metadata = match fs::symlink_metadata(path) {
        // 保存既有目标的非跟随元数据。
        Ok(metadata) => metadata,
        // 缺失目录交还上层验证父目录。
        Err(error) if error.kind() == ErrorKind::NotFound => {
            // 返回明确缺失状态。
            return Ok(OutputDirectoryState::Missing);
        }
        // 其他检查失败必须失败闭合。
        Err(_) => return Err(OutputGuardError::InspectionFailed),
    };
    // 只允许使用非符号链接真实目录。
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        // 拒绝文件、链接和其他特殊目标。
        return Err(OutputGuardError::InvalidTargetType);
    }
    // 只读取首个目录项以判断是否非空。
    let mut entries = fs::read_dir(path).map_err(|_| OutputGuardError::InspectionFailed)?;
    // 对首个条目执行完整错误检查。
    let has_entries = entries
        // 读取至多一个条目。
        .next()
        // 把条目读取失败映射为封闭检查错误。
        .transpose()
        // 不泄漏路径或原生 I/O 错误。
        .map_err(|_| OutputGuardError::InspectionFailed)?
        // 只保留是否存在条目的事实。
        .is_some();
    // 空目录无需覆盖许可。
    if !has_entries {
        // 返回可直接使用的空目录状态。
        return Ok(OutputDirectoryState::Empty);
    }
    // 非空目录必须取得独立覆盖许可。
    if !overwrite_confirmed {
        // 不触碰目录内容并要求确认。
        return Err(OutputGuardError::ConfirmationRequired);
    }
    // 返回已确认的非空目录状态。
    Ok(OutputDirectoryState::NonEmptyConfirmed)
}

// 编译 Component 的独立文件系统门禁测试。
#[cfg(test)]
#[path = "output_guard_tests.rs"]
mod tests;
