use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::{Map, Value};

// 把错误码实现保留为 RecordingConfig 普通领域边界的私有类型。
#[path = "recording_error.rs"]
mod error_code;
// 导入当前录制配置私有封闭错误码。
use error_code::RecordingConfigErrorCode;

use crate::{
    // 导入统一文件与目录覆盖门禁。
    components::output_guard::{
        // 导入目录存在状态。
        OutputDirectoryState,
        // 导入封闭门禁错误。
        OutputGuardError,
        // 导入分析目录门禁。
        guard_directory_output,
        // 导入主视频文件门禁。
        guard_file_output,
    },
    // 导入 recording 领域错误与结果。
    domain::{AppControlError, AppResult},
};

pub(crate) const DEFAULT_DURATION_MS: u64 = 30_000;
pub(crate) const DEFAULT_FPS: u32 = 2;
pub(crate) const DEFAULT_MAX_WIDTH: u32 = 960;
// 固定 provider-neutral 默认编码质量。
pub(crate) const DEFAULT_QUALITY: u8 = 75;
pub(crate) const DEFAULT_MAX_KEYFRAMES: usize = 8;
pub(crate) const DEFAULT_CHANGE_THRESHOLD: f64 = 0.035;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 5_000;

// 固定录制配置只开放这些有界动态槽，不允许调用方扩展编码器实现。
const RECORDING_ARGUMENT_FIELDS: [&str; 10] = [
    // 公开 MP4 输出路径。
    "path",
    // 有界录制时长。
    "durationMs",
    // 有界帧率。
    "fps",
    // 有界最大宽度。
    "maxWidth",
    // 有界 provider-neutral 编码质量。
    "quality",
    // 有界分析关键帧数量。
    "maxKeyframes",
    // 有界变化阈值。
    "changeThreshold",
    // 公开分析目录。
    "analysisDir",
    // 有界单帧等待时间。
    "timeoutMs",
    // 独立覆盖许可。
    "overwrite",
];

#[derive(Debug, Clone)]
pub(crate) struct RecordingConfig {
    // 保存 worker 实际写入的私有 MP4 路径。
    pub output_path: PathBuf,
    // 保存 worker 实际写入的私有分析目录。
    pub analysis_dir: PathBuf,
    // 保存 manifest 与公开结果使用的最终 MP4 路径。
    pub public_output_path: PathBuf,
    // 保存 manifest 与公开结果使用的最终分析目录。
    pub public_analysis_dir: PathBuf,
    pub duration: Duration,
    pub fps: u32,
    pub max_width: u32,
    // 保存一到一百的 provider-neutral 编码质量。
    pub quality: u8,
    pub max_keyframes: usize,
    pub change_threshold: f64,
    pub timeout: Duration,
    pub overwrite: bool,
}

impl RecordingConfig {
    pub(crate) fn from_args(args: &Map<String, Value>) -> AppResult<Self> {
        // 先执行不触碰文件系统的字段与路径外壳解析。
        let config = Self::from_args_without_output_access(args)?;
        // 同步执行入口继续在返回前完成输出状态门禁。
        config.validate_output_access()?;
        // 返回已经完成全部同步预检的配置。
        Ok(config)
    }

    // 解析业务接受前可验证且不触碰输出文件系统的配置。
    pub(crate) fn from_args_without_output_access(args: &Map<String, Value>) -> AppResult<Self> {
        // 在读取任何值前拒绝 codec、filter、argv 等未认证字段。
        reject_unknown_recording_arguments(args)?;
        let output_path = PathBuf::from(required_string(args, "path")?);
        let duration_ms = bounded_u64(args, "durationMs", DEFAULT_DURATION_MS, 1_000, 300_000)?;
        let fps = bounded_u64(args, "fps", u64::from(DEFAULT_FPS), 1, 10)? as u32;
        let max_width =
            bounded_u64(args, "maxWidth", u64::from(DEFAULT_MAX_WIDTH), 320, 1_920)? as u32;
        // 解析一到一百的 provider-neutral 质量等级。
        let quality = bounded_u64(args, "quality", u64::from(DEFAULT_QUALITY), 1, 100)? as u8;
        let max_keyframes =
            bounded_u64(args, "maxKeyframes", DEFAULT_MAX_KEYFRAMES as u64, 2, 20)? as usize;
        let change_threshold = bounded_f64(
            args,
            "changeThreshold",
            DEFAULT_CHANGE_THRESHOLD,
            0.005,
            0.5,
        )?;
        let timeout_ms = bounded_u64(args, "timeoutMs", DEFAULT_TIMEOUT_MS, 250, 30_000)?;
        // overwrite 必须是显式布尔值，禁止类型错误静默降级为 false。
        let overwrite = match args.get("overwrite") {
            // 缺失时使用安全默认值。
            None => false,
            // 接受真实 JSON boolean。
            Some(Value::Bool(value)) => *value,
            // 其他类型全部失败闭合。
            Some(_) => {
                // 返回稳定参数错误。
                return Err(RecordingConfigErrorCode::InvalidArgument.error(
                    // 说明必需类型。
                    "args.overwrite 必须是布尔值。",
                ));
            }
        };
        let analysis_dir = args
            .get("analysisDir")
            .map(|value| {
                value
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        // 使用配置边界的封闭参数错误。
                        RecordingConfigErrorCode::InvalidArgument
                            // 保持既有安全消息。
                            .error("args.analysisDir 必须是非空字符串。")
                    })
            })
            .transpose()?
            .map(PathBuf::from)
            .unwrap_or_else(|| default_analysis_dir(&output_path));

        let config = Self {
            // 首次解析时物理路径就是调用方最终路径。
            output_path: output_path.clone(),
            // 首次解析时物理分析目录就是调用方最终目录。
            analysis_dir: analysis_dir.clone(),
            // 冻结公开视频路径供隔离 staging 使用。
            public_output_path: output_path,
            // 冻结公开分析目录供隔离 staging 使用。
            public_analysis_dir: analysis_dir,
            duration: Duration::from_millis(duration_ms),
            fps,
            max_width,
            // 保存 provider-neutral 编码质量。
            quality,
            max_keyframes,
            change_threshold,
            timeout: Duration::from_millis(timeout_ms),
            overwrite,
        };
        // 只验证扩展名与父路径外壳，不查询文件系统状态。
        config.validate_path_shape()?;
        // 返回尚未执行输出访问门禁的配置。
        Ok(config)
    }

    // 把已独立验证的公开配置改写为父 Module 独占的 worker staging。
    pub(crate) fn with_worker_staging(
        // 接收父 Module 独占的 MP4 staging 路径。
        mut self,
        // 接收父 Module 独占的 MP4 staging 路径。
        output_path: PathBuf,
        // 接收父 Module 独占的分析 staging 目录。
        analysis_dir: PathBuf,
    ) -> Self {
        // worker 只能写入父 Module 预留的视频候选。
        self.output_path = output_path;
        // worker 只能写入父 Module 预留的分析候选目录。
        self.analysis_dir = analysis_dir;
        // staging 都由父 Module 独占，因此允许内部替换空 reservation。
        self.overwrite = true;
        // 返回物理路径已收窄但公开路径未改变的配置。
        self
    }

    pub(crate) fn duration_ms(&self) -> u64 {
        u64::try_from(self.duration.as_millis()).unwrap_or(u64::MAX)
    }

    // 验证路径外壳而不读取文件系统状态。
    fn validate_path_shape(&self) -> AppResult<()> {
        if !has_extension(&self.output_path, "mp4") {
            // 使用录制配置边界的封闭参数错误。
            return Err(RecordingConfigErrorCode::InvalidArgument
                .error("window.record@1 当前只写入 .mp4 文件。"));
        }
        let parent = self
            .output_path
            .parent()
            // 使用录制配置边界的封闭参数错误。
            .ok_or_else(|| {
                // 保持既有路径要求消息。
                RecordingConfigErrorCode::InvalidArgument.error("视频路径必须包含父目录。")
            })?;
        // 空父路径不构成可接受的输出外壳。
        if parent.as_os_str().is_empty() {
            // 使用稳定参数错误。
            return Err(RecordingConfigErrorCode::InvalidArgument.error("视频路径必须包含父目录。"));
        }
        // 分析目录必须具有父路径外壳。
        let analysis_parent = self.public_analysis_dir.parent().ok_or_else(|| {
            // 使用稳定参数错误。
            RecordingConfigErrorCode::InvalidArgument.error("分析目录必须包含父目录。")
        })?;
        // 空父路径同样不得延迟到业务接受后。
        if analysis_parent.as_os_str().is_empty() {
            // 使用稳定参数错误。
            return Err(RecordingConfigErrorCode::InvalidArgument.error("分析目录必须包含父目录。"));
        }
        // 路径外壳验证完成。
        Ok(())
    }

    // 在业务接受后、目标发现和捕获前验证输出文件系统状态。
    pub(crate) fn validate_output_access(&self) -> AppResult<()> {
        // 视频父目录已由路径外壳验证存在。
        let parent = self.public_output_path.parent().ok_or_else(|| {
            // 理论漂移失败闭合。
            RecordingConfigErrorCode::InvalidArgument.error("视频路径必须包含父目录。")
        })?;
        if !parent.is_dir() {
            // 使用录制配置边界的封闭参数错误。
            return Err(RecordingConfigErrorCode::InvalidArgument
                .error(format!("视频父目录不存在：{}", parent.display())));
        }
        // 在捕获前使用统一非跟随门禁检查主视频覆盖许可。
        guard_file_output(&self.public_output_path, self.overwrite)
            .map_err(recording_file_guard_error)?;
        // 在捕获前检查分析目录状态和覆盖许可。
        let analysis_state = guard_directory_output(&self.public_analysis_dir, self.overwrite)
            // 映射为 recording 领域错误。
            .map_err(recording_directory_guard_error)?;
        // 缺失分析目录时继续验证其父目录。
        if analysis_state == OutputDirectoryState::Missing {
            let parent = self.public_analysis_dir.parent().ok_or_else(|| {
                // 使用录制配置边界的封闭参数错误。
                RecordingConfigErrorCode::InvalidArgument.error("分析目录必须包含父目录。")
            })?;
            if !parent.is_dir() {
                // 使用录制配置边界的封闭参数错误。
                return Err(RecordingConfigErrorCode::InvalidArgument
                    .error(format!("分析目录的父目录不存在：{}", parent.display())));
            }
        }
        Ok(())
    }
}

// 拒绝不属于版本化录制配置的任意外部进程参数。
fn reject_unknown_recording_arguments(args: &Map<String, Value>) -> AppResult<()> {
    // 逐字段验证调用方输入，避免静默忽略未知命令片段。
    for name in args.keys() {
        // 只有认证的有界动态槽可以继续进入 Recording Module。
        if !RECORDING_ARGUMENT_FIELDS.contains(&name.as_str()) {
            // 未知字段在启动编码器前失败闭合。
            return Err(RecordingConfigErrorCode::InvalidArgument.error(
                // 不回显潜在命令内容，只报告字段身份。
                format!("window.record@1 不接受未知字段 args.{name}。"),
            ));
        }
    }
    // 所有字段均属于固定录制配置。
    Ok(())
}

// 把单文件 Component 错误映射为 recording 领域错误。
fn recording_file_guard_error(error: OutputGuardError) -> AppControlError {
    // 保持覆盖、类型和检查失败可以区分。
    match error {
        // 既有主视频缺少覆盖许可。
        OutputGuardError::ConfirmationRequired => {
            // 使用封闭覆盖确认类别翻译 Component 错误。
            RecordingConfigErrorCode::OverwriteConfirmationRequired.error(
                // 提示调用者提供独立覆盖许可。
                "视频文件已存在；请增加 overwrite=true。",
            )
        }
        // 主视频目标不是可替换普通文件。
        OutputGuardError::InvalidTargetType => RecordingConfigErrorCode::InvalidArgument.error(
            // 不泄漏具体输出路径。
            "视频输出已存在但不是可覆盖的普通文件。",
        ),
        // 主视频目标状态无法可靠检查。
        OutputGuardError::InspectionFailed => RecordingConfigErrorCode::OperationFailed.error(
            // 不泄漏原生 I/O 错误。
            "无法安全检查视频输出路径。",
        ),
    }
}

// 把目录 Component 错误映射为 recording 领域错误。
fn recording_directory_guard_error(error: OutputGuardError) -> AppControlError {
    // 保持覆盖、类型和检查失败可以区分。
    match error {
        // 非空分析目录缺少覆盖许可。
        OutputGuardError::ConfirmationRequired => {
            // 使用封闭覆盖确认类别翻译 Component 错误。
            RecordingConfigErrorCode::OverwriteConfirmationRequired.error(
                // 提示调用者改用空目录或明确确认。
                "分析目录非空；请改用空目录或增加 overwrite=true。",
            )
        }
        // 分析目标不是可复用真实目录。
        OutputGuardError::InvalidTargetType => RecordingConfigErrorCode::InvalidArgument.error(
            // 不泄漏具体分析路径。
            "args.analysisDir 已存在但不是可复用的真实目录。",
        ),
        // 分析目录状态无法可靠检查。
        OutputGuardError::InspectionFailed => RecordingConfigErrorCode::OperationFailed.error(
            // 不泄漏原生目录枚举错误。
            "无法安全检查分析目录。",
        ),
    }
}

fn required_string<'a>(args: &'a Map<String, Value>, name: &str) -> AppResult<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            // 使用录制配置边界的封闭参数错误。
            RecordingConfigErrorCode::InvalidArgument
                // 保持字段名可定位但不回显字段值。
                .error(format!("args.{name} 是必填字符串。"))
        })
}

fn bounded_u64(
    args: &Map<String, Value>,
    name: &str,
    default: u64,
    min: u64,
    max: u64,
) -> AppResult<u64> {
    match args.get(name) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .filter(|value| (min..=max).contains(value))
            .ok_or_else(|| {
                // 使用录制配置边界的封闭参数错误。
                RecordingConfigErrorCode::InvalidArgument
                    .error(format!("args.{name} 必须是 {min}..={max} 的整数。"))
            }),
    }
}

fn bounded_f64(
    args: &Map<String, Value>,
    name: &str,
    default: f64,
    min: f64,
    max: f64,
) -> AppResult<f64> {
    match args.get(name) {
        None => Ok(default),
        Some(value) => value
            .as_f64()
            .filter(|value| value.is_finite() && (min..=max).contains(value))
            .ok_or_else(|| {
                // 使用录制配置边界的封闭参数错误。
                RecordingConfigErrorCode::InvalidArgument
                    .error(format!("args.{name} 必须位于 {min}..={max}。"))
            }),
    }
}

fn default_analysis_dir(output_path: &Path) -> PathBuf {
    let stem = output_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("recording");
    output_path.with_file_name(format!("{stem}.analysis"))
}

fn has_extension(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(expected))
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    // 导入 Output Guard 封闭错误夹具。
    use crate::components::output_guard::OutputGuardError;

    // 导入配置、默认值与两个受影响错误 mapper。
    use super::{
        DEFAULT_FPS, RecordingConfig, recording_directory_guard_error, recording_file_guard_error,
    };

    fn args_for(path: &std::path::Path) -> Map<String, serde_json::Value> {
        Map::from_iter([("path".to_owned(), json!(path))])
    }

    #[test]
    fn token_efficient_defaults_are_bounded() -> crate::domain::AppResult<()> {
        let path = std::env::temp_dir().join("recording-defaults.mp4");
        let config = RecordingConfig::from_args(&args_for(&path))?;
        assert_eq!(config.fps, DEFAULT_FPS);
        assert_eq!(config.max_width, 960);
        assert_eq!(config.max_keyframes, 8);
        // 默认质量不依赖任何具体编码器参数。
        assert_eq!(config.quality, 75);
        Ok(())
    }

    // 验证业务接受前解析不会读取输出文件系统状态。
    #[test]
    fn pre_acceptance_parse_does_not_require_existing_parent() {
        // 构造当前测试不会创建的唯一父目录。
        let path = std::env::temp_dir()
            // 使用进程绑定测试子目录。
            .join(format!(
                // 固定前缀并附加当前测试进程。
                "act-recording-pre-acceptance-missing-{}",
                // 防止与外部目录重叠。
                std::process::id(),
            ))
            // 使用合法 MP4 文件名。
            .join("capture.mp4");
        // 防御旧测试残留改变预期。
        let _ = std::fs::remove_dir_all(
            // 只清理当前测试固定子目录。
            path.parent()
                // 当前 join 必须产生父目录。
                .unwrap_or_else(|| panic!("pre-acceptance fixture parent is missing")),
        );
        // 纯解析不得要求父目录已经存在。
        assert!(RecordingConfig::from_args_without_output_access(&args_for(&path)).is_ok());
        // 完整同步解析仍必须执行输出状态门禁。
        assert!(RecordingConfig::from_args(&args_for(&path)).is_err());
    }

    #[test]
    fn rejects_unbounded_frame_and_token_inputs() {
        let path = std::env::temp_dir().join("recording-invalid.mp4");
        let mut args = args_for(&path);
        args.insert("fps".to_owned(), json!(30));
        assert!(RecordingConfig::from_args(&args).is_err());
        args.insert("fps".to_owned(), json!(2));
        args.insert("maxKeyframes".to_owned(), json!(100));
        assert!(RecordingConfig::from_args(&args).is_err());
    }

    // 验证任意编码器字段不会被静默忽略或转发给私有实现。
    #[test]
    // 逐个覆盖典型 codec、filter、argv 与 executable 注入字段。
    fn rejects_arbitrary_encoder_arguments() {
        // 使用缺失目标避免创建任何实际输出文件。
        let path = std::env::temp_dir().join("recording-fixed-arguments.mp4");
        // 覆盖所有明确禁止的任意命令入口。
        // 同时拒绝旧 provider-specific CRF，避免静默换算质量。
        for name in ["ffmpegArgs", "codec", "filter", "encoderPath", "crf"] {
            // 从唯一必填的安全路径开始构造输入。
            let mut args = args_for(&path);
            // 加入一个应在启动外部进程前拒绝的未知字段。
            args.insert(name.to_owned(), json!("arbitrary"));
            // 解析必须失败且不启动编码器。
            assert!(matches!(
                // 解析固定录制输入。
                RecordingConfig::from_args(&args),
                // 失败语义必须保持封闭参数错误。
                Err(error) if error.code == "INVALID_ARGUMENT"
            ));
        }
    }

    // overwrite 不能把错误类型静默解释为未确认。
    #[test]
    // 验证严格 JSON boolean 边界。
    fn rejects_non_boolean_overwrite() {
        // 使用缺失目标避免任何覆盖操作。
        let path = std::env::temp_dir().join("recording-overwrite-type.mp4");
        // 构造最小录制输入。
        let mut args = args_for(&path);
        // 注入字符串类型的伪布尔值。
        args.insert("overwrite".to_owned(), json!("true"));
        // 解析必须返回参数错误。
        assert!(matches!(
            // 调用严格录制配置解析。
            RecordingConfig::from_args(&args),
            // 核对稳定错误码。
            Err(error) if error.code == "INVALID_ARGUMENT"
        ));
    }

    // 验证文件与目录门禁使用同一录制配置错误分类。
    #[test]
    fn output_guard_errors_keep_recording_config_mapping() {
        // 固定 Component 错误与公开录制配置错误码对照表。
        let mappings = [
            // 缺少覆盖许可保持独立确认码。
            (
                OutputGuardError::ConfirmationRequired,
                "OVERWRITE_CONFIRMATION_REQUIRED",
            ),
            // 非普通目标保持参数拒绝码。
            (OutputGuardError::InvalidTargetType, "INVALID_ARGUMENT"),
            // 检查失败保持操作失败码。
            (OutputGuardError::InspectionFailed, "OPERATION_FAILED"),
        ];
        // 同时核对文件与目录 mapper，防止分类漂移。
        for (source, expected) in mappings {
            // 文件目标映射必须保持稳定。
            assert_eq!(recording_file_guard_error(source).code, expected);
            // 目录目标映射必须选择相同公开类别。
            assert_eq!(recording_directory_guard_error(source).code, expected);
        }
    }
}
