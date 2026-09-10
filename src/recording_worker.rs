//! 在隔离 Job worker 中执行固定精确窗口录制。

// 导入有界 stdin、文件读取与路径验证。
use std::{
    // 读取 MP4 与 PNG 文件头。
    fs::{self, File},
    // 限制 stdin 并读取固定签名。
    io::{Read, Write},
    // 保存父 Module 独占 staging 路径。
    path::{Path, PathBuf},
};

// 导入严格请求反序列化。
use serde::Deserialize;
// 导入语言中立 JSON 数据。
use serde_json::{Value, json};

// 把错误码实现保留为 Recording Worker 协议边界的普通私有类型。
#[path = "recording_worker_error.rs"]
mod error_code;
// 导入当前 Worker 私有封闭错误码。
use error_code::RecordingWorkerErrorCode;

// 导入窗口发现、录制 adapter、自有编码 Component 与领域错误。
use crate::{
    // 访问私有 Windows adapter 边界。
    adapters::{
        // 访问固定 WGC 录制实现。
        video_recording::record_window_mp4,
        // 访问 canonical 窗口重新发现与前景门禁。
        window::{capture_visible_titled_windows, ensure_foreground_unchanged, resolve_window},
        // 访问当前前景只读事实。
        windows::foreground_hwnd,
    },
    // 访问私有 opaque ID 原语。
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    // 使用 provider-neutral 错误类型。
    domain::{AppControlError, AppResult},
    // 复用公开录制参数边界。
    recording::RecordingConfig,
};

// 固定 recording worker 协议版本。
const CONTRACT_VERSION: &str = "act/recording-worker/v2";
// 限制请求为 64KiB。
const MAXIMUM_REQUEST_BYTES: u64 = 64 * 1024;
// 限制成功或错误输出为单行小 envelope。
const MAXIMUM_VIDEO_BYTES: u64 = 2 * 1024 * 1024 * 1024;
// 固定 PNG 文件签名。
const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

// 定义严格版本化 worker 请求。
#[derive(Debug, Deserialize)]
// 使用 camelCase 并拒绝协议外字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkerRequest {
    // 保存协议版本。
    contract_version: String,
    // 保存 canonical opaque 窗口目标。
    session_id: String,
    // 保存逐操作显式确认。
    confirmed: bool,
    // 保存完整 provider-neutral 录制输入。
    input: Value,
    // 保存父 Module 独占的视频 staging 路径。
    video_staging_path: String,
    // 保存父 Module 独占的分析 staging 目录。
    analysis_staging_dir: String,
}

// 解析单个严格 worker 请求。
fn parse_request(input: &str) -> AppResult<WorkerRequest> {
    // 只接受一个 JSON object 且拒绝尾随文档。
    let request: WorkerRequest = serde_json::from_str(input).map_err(|_| {
        // 不回显可能包含路径的原始输入。
        RecordingWorkerErrorCode::InvalidArgument.error(
            // 保持既有安全错误消息。
            "The recording worker request is invalid.",
        )
    })?;
    // 协议版本必须精确匹配。
    if request.contract_version != CONTRACT_VERSION {
        // 协议漂移失败闭合。
        return Err(RecordingWorkerErrorCode::InvalidArgument.error(
            // 不回显未知版本。
            "The recording worker contract version is unsupported.",
        ));
    }
    // confirmation 必须先于目标发现、配置路径解析和 staging 检查。
    if !request.confirmed {
        // 缺少确认立即停止。
        return Err(RecordingWorkerErrorCode::ConfirmationRequired.error(
            // 说明敏感读取边界。
            "Exact window recording requires explicit confirmation.",
        ));
    }
    // 严格解析第二代 opaque ID。
    let target = OpaqueTargetId::parse(&request.session_id);
    // 只接受 canonical 窗口类别。
    if target.map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        // 拒绝 native、旧 ID 与其他 target kind。
        return Err(RecordingWorkerErrorCode::InvalidArgument.error(
            // 不回显原生目标。
            "The recording worker requires a canonical s2:w target.",
        ));
    }
    // 返回版本、确认和目标都通过的请求。
    Ok(request)
}

// 执行独立重验证和固定录制行为。
fn execute_request(request: WorkerRequest) -> AppResult<Value> {
    // 输入必须保持对象形状。
    let input = request.input.as_object().ok_or_else(|| {
        // 返回稳定参数错误。
        RecordingWorkerErrorCode::InvalidArgument.error(
            // 保持既有公开安全消息。
            "Window recording input must be an object.",
        )
    })?;
    // 复用公开边界独立验证全部数值、路径与覆盖许可。
    let public_config = RecordingConfig::from_args(input)?;
    // 冻结父 Module 传入的视频候选路径。
    let video_staging_path = PathBuf::from(&request.video_staging_path);
    // 冻结父 Module 传入的分析候选目录。
    let analysis_staging_dir = PathBuf::from(&request.analysis_staging_dir);
    // 验证视频 staging 是既有真实普通文件。
    validate_video_staging(&video_staging_path)?;
    // 验证分析 staging 是既有空真实目录。
    validate_analysis_staging(&analysis_staging_dir)?;
    // staging 不得与任一最终公开路径相同。
    if video_staging_path == public_config.public_output_path
        // 分析 staging 也不得等于最终目录。
        || analysis_staging_dir == public_config.public_analysis_dir
    {
        // 拒绝绕过父 Module 事务边界。
        return Err(invalid_staging());
    }
    // 在窗口重新发现前记录前景事实。
    let foreground_before = foreground_hwnd();
    // 每次执行都重新枚举当前可见有标题窗口。
    let windows = capture_visible_titled_windows()?;
    // worker 独立唯一解析 canonical 目标。
    let window = resolve_window(&request.session_id, &windows)?;
    // worker 二次 preflight 由录制 adapter 在同一 MTA 捕获线程内执行。
    // 把物理写入路径收窄到父 Module 独占 staging。
    let worker_config = public_config.with_worker_staging(
        // 使用视频 staging。
        video_staging_path.clone(),
        // 使用分析 staging。
        analysis_staging_dir.clone(),
    );
    // 在当前 Job 内保持单 WGC session 并使用内嵌 Media Foundation 编码。
    let recording = record_window_mp4(window.hwnd, worker_config)?;
    // 验证 MP4 候选签名、长度与 worker 报告一致。
    validate_mp4(&video_staging_path, recording.bytes)?;
    // 验证分析候选的固定文件集合与签名。
    validate_analysis(
        // 传入独占分析目录。
        &analysis_staging_dir,
        // 传入 worker 报告的关键帧数量。
        recording.keyframe_count,
    )?;
    // 记录全部编码生命周期结束后的前景事实。
    let foreground_after = foreground_hwnd();
    // 前景变化阻止父 Module 提交候选。
    ensure_foreground_unchanged(foreground_before, foreground_after)?;
    // 返回不含路径、句柄、PID 或运行时位置的成功事实。
    Ok(json!({
        // 标记 worker 成功。
        "ok": true,
        // 输出固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 输出真实候选字节数。
        "bytes": recording.bytes,
        // 输出源宽度。
        "sourceWidth": recording.source_width,
        // 输出源高度。
        "sourceHeight": recording.source_height,
        // 输出编码宽度。
        "width": recording.width,
        // 输出编码高度。
        "height": recording.height,
        // 输出固定有界帧率。
        "fps": recording.fps,
        // 输出请求录制时长。
        "durationMs": recording.duration_ms,
        // 输出编码帧数。
        "encodedFrames": recording.encoded_frames,
        // 输出实际捕获帧数。
        "capturedFrames": recording.captured_frames,
        // 输出封闭设备分类。
        "deviceDriver": recording.device_driver,
        // 输出分析关键帧数量。
        "keyframeCount": recording.keyframe_count,
        // 明确音频未捕获。
        "audioCaptured": false,
        // 明确光标未捕获。
        "cursorCaptured": false,
        // 输出稳定前景证据。
        "foregroundUnchanged": true,
    }))
}

// 验证父 Module 独占的视频 reservation。
fn validate_video_staging(path: &Path) -> AppResult<()> {
    // 使用不跟随链接元数据。
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid_staging())?;
    // 只接受真实普通文件且初始长度为零。
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != 0 {
        // 拒绝复用任意既有输出。
        return Err(invalid_staging());
    }
    // staging 文件通过验证。
    Ok(())
}

// 验证父 Module 独占的空分析目录。
fn validate_analysis_staging(path: &Path) -> AppResult<()> {
    // 使用不跟随链接元数据。
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid_staging())?;
    // 只接受真实目录。
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        // 拒绝链接或其他类型。
        return Err(invalid_staging());
    }
    // 初始目录必须为空，禁止覆盖父 Module 未授权内容。
    let mut entries = fs::read_dir(path).map_err(|_| invalid_staging())?;
    // 任一既有子项都使请求无效。
    if entries.next().is_some() {
        // 返回稳定 staging 错误。
        return Err(invalid_staging());
    }
    // 空目录通过验证。
    Ok(())
}

// 验证 worker 产生的 MP4 候选。
fn validate_mp4(path: &Path, reported_bytes: u64) -> AppResult<()> {
    // 读取不跟随链接的最终候选状态。
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid_video())?;
    // 候选必须是真实普通文件且长度有界。
    if metadata.file_type().is_symlink()
        // 禁止目录与特殊文件。
        || !metadata.is_file()
        // 长度必须和 worker 内部结果一致。
        || metadata.len() != reported_bytes
        // MP4 至少应包含 box header。
        || !(12..=MAXIMUM_VIDEO_BYTES).contains(&reported_bytes)
    {
        // 返回稳定视频候选错误。
        return Err(invalid_video());
    }
    // 打开精确候选读取文件头。
    let mut file = File::open(path).map_err(|_| invalid_video())?;
    // 保存前十二字节。
    let mut header = [0_u8; 12];
    // 读取完整 MP4 起始 box。
    file.read_exact(&mut header).map_err(|_| invalid_video())?;
    // ISO BMFF 必须在偏移四处声明 ftyp box。
    if &header[4..8] != b"ftyp" {
        // 拒绝空壳或错误格式。
        return Err(invalid_video());
    }
    // MP4 候选通过验证。
    Ok(())
}

// 验证分析候选文件集合和聚合 PNG。
fn validate_analysis(directory: &Path, reported_keyframes: usize) -> AppResult<()> {
    // 关键帧必须落在公开上限内。
    if !(1..=20).contains(&reported_keyframes) {
        // 拒绝协议漂移。
        return Err(invalid_analysis());
    }
    // 保存实际关键帧数量。
    let mut frame_count = 0_usize;
    // 保存 manifest 是否存在。
    let mut manifest_present = false;
    // 保存 storyboard 是否存在。
    let mut storyboard_present = false;
    // 枚举独占目录直接子项。
    for entry in fs::read_dir(directory).map_err(|_| invalid_analysis())? {
        // 读取目录项。
        let entry = entry.map_err(|_| invalid_analysis())?;
        // 文件名必须是 UTF-8。
        let name = entry
            // 取得文件名。
            .file_name()
            // 转换为 UTF-8。
            .into_string()
            // 非 UTF-8 拒绝。
            .map_err(|_| invalid_analysis())?;
        // 每项必须是真实非空普通文件。
        let metadata = fs::symlink_metadata(entry.path()).map_err(|_| invalid_analysis())?;
        // 拒绝链接、目录与空文件。
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
            // 返回固定分析错误。
            return Err(invalid_analysis());
        }
        // 按封闭名称分类。
        match name.as_str() {
            // 保存 manifest 存在事实。
            "manifest.json" => manifest_present = true,
            // 验证 storyboard PNG 签名。
            "storyboard.png" => {
                // 读取固定 PNG 签名。
                validate_png(&entry.path())?;
                // 标记 storyboard 存在。
                storyboard_present = true;
            }
            // 验证固定关键帧形状与 PNG 签名。
            value if value.starts_with("frame-") && value.ends_with("ms.png") => {
                // 读取固定 PNG 签名。
                validate_png(&entry.path())?;
                // 增加实际关键帧数量。
                frame_count = frame_count.saturating_add(1);
            }
            // 其他文件全部拒绝。
            _ => return Err(invalid_analysis()),
        }
    }
    // 必需聚合文件和关键帧数量必须精确一致。
    if !manifest_present || !storyboard_present || frame_count != reported_keyframes {
        // 返回不完整分析错误。
        return Err(invalid_analysis());
    }
    // 解析 manifest 为单个 JSON object。
    let manifest_bytes =
        fs::read(directory.join("manifest.json")).map_err(|_| invalid_analysis())?;
    // 拒绝大于 1MiB 的异常 manifest。
    if manifest_bytes.len() > 1024 * 1024 {
        // 返回有界分析错误。
        return Err(invalid_analysis());
    }
    // manifest 必须是 JSON object。
    let manifest: Value =
        serde_json::from_slice(&manifest_bytes).map_err(|_| invalid_analysis())?;
    // 顶层非对象表示协议失效。
    if !manifest.is_object() {
        // 返回固定分析错误。
        return Err(invalid_analysis());
    }
    // 分析候选通过验证。
    Ok(())
}

// 验证固定 PNG 文件签名。
fn validate_png(path: &Path) -> AppResult<()> {
    // 打开精确候选。
    let mut file = File::open(path).map_err(|_| invalid_analysis())?;
    // 保存签名字节。
    let mut signature = [0_u8; 8];
    // 读取完整签名。
    file.read_exact(&mut signature)
        // 缺失签名视为分析失败。
        .map_err(|_| invalid_analysis())?;
    // 必须精确匹配 PNG 签名。
    if signature != PNG_SIGNATURE {
        // 拒绝伪装文件。
        return Err(invalid_analysis());
    }
    // PNG 签名通过。
    Ok(())
}

// 构造不泄漏路径的 staging 错误。
fn invalid_staging() -> AppControlError {
    // 返回固定候选路径错误。
    RecordingWorkerErrorCode::InvalidOutputPath.error(
        // 不回显私有路径。
        "The recording staging boundary is invalid.",
    )
}

// 构造不泄漏路径的视频候选错误。
fn invalid_video() -> AppControlError {
    // 返回固定视频缺失错误。
    RecordingWorkerErrorCode::VideoOutputMissing.error(
        // 不回显私有路径。
        "The isolated recorder did not produce a valid MP4 candidate.",
    )
}

// 构造不泄漏路径的分析候选错误。
fn invalid_analysis() -> AppControlError {
    // 返回固定分析失败错误。
    RecordingWorkerErrorCode::VideoAnalysisFailed.error(
        // 不回显候选目录。
        "The isolated recorder produced an invalid analysis candidate.",
    )
}

// 把领域错误投影为严格 worker 失败 envelope。
fn error_envelope(error: AppControlError) -> Value {
    // 只输出协议版本、稳定错误码与安全消息。
    json!({
        // 标记失败。
        "ok": false,
        // 输出固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 输出封闭错误。
        "error": {
            // 输出稳定错误码。
            "code": error.code,
            // 输出不含原生标识的消息。
            "message": error.message,
        },
    })
}

// 构造不泄漏请求数据的固定序列化失败响应。
fn serialization_error_text() -> String {
    // 从封闭 Worker 错误类型取得稳定协议文本。
    let code = RecordingWorkerErrorCode::WorkerProtocolError.as_str();
    // 保持既有最小单行 JSON 形状逐字不变。
    format!(
        // 只包含版本、稳定错误码与安全消息。
        "{{\"ok\":false,\"contractVersion\":\"{CONTRACT_VERSION}\",\"error\":{{\"code\":\"{code}\",\"message\":\"The recording worker could not serialize its response.\"}}}}"
    )
}

// 从标准输入执行一次请求并向标准输出写一行 JSON。
pub fn run_stdio() -> i32 {
    // 创建请求缓冲区。
    let mut input = String::new();
    // 将读取限制为 64KiB 加一个溢出探针字节。
    let read_result = std::io::stdin()
        // 限制输入字节。
        .take(MAXIMUM_REQUEST_BYTES.saturating_add(1))
        // 读取 UTF-8 文本。
        .read_to_string(&mut input);
    // 解析或执行请求。
    let result = match read_result {
        // 超过硬上限时拒绝。
        Ok(_) if u64::try_from(input.len()).unwrap_or(u64::MAX) > MAXIMUM_REQUEST_BYTES => {
            // 返回参数错误。
            Err(RecordingWorkerErrorCode::InvalidArgument.error(
                // 不回显请求内容。
                "The recording worker request is too large.",
            ))
        }
        // 成功读取时严格解析并执行。
        Ok(_) => parse_request(input.trim_end()).and_then(execute_request),
        // stdin 读取失败时返回结构化错误。
        Err(_) => Err(RecordingWorkerErrorCode::InvalidArgument.error(
            // 不公开底层 I/O 细节。
            "The recording worker could not read its request.",
        )),
    };
    // 把结果封装为单个 JSON envelope。
    let envelope = match result {
        // 成功结果已经包含协议版本。
        Ok(value) => value,
        // 错误投影为安全 envelope。
        Err(error) => error_envelope(error),
    };
    // 序列化失败使用固定最小错误 envelope。
    let serialized = serde_json::to_string(&envelope).unwrap_or_else(|_| {
        // 返回不包含请求数据的固定 JSON。
        serialization_error_text()
    });
    // 只向 stdout 写入一行 JSON。
    let write_ok = writeln!(std::io::stdout(), "{serialized}").is_ok();
    // stdout 写入失败返回非零。
    if !write_ok {
        // 使用固定 I/O 失败退出码。
        return 2;
    }
    // 成功 envelope 使用零退出码，错误 envelope 使用一。
    if envelope.get("ok").and_then(Value::as_bool) == Some(true) {
        // 成功退出。
        0
    } else {
        // 结构化失败退出。
        1
    }
}

// 声明 Recording Worker 自有 envelope 的纯契约测试。
#[cfg(test)]
mod tests {
    // 导入 JSON 夹具宏。
    use serde_json::json;

    // 导入被测错误类型与纯投影函数。
    use super::{RecordingWorkerErrorCode, error_envelope, serialization_error_text};

    // 验证 Worker 自有错误保持 v2 失败 envelope。
    #[test]
    fn worker_owned_error_keeps_v2_envelope() {
        // 构造不含 provider 详情的协议参数失败。
        let error = RecordingWorkerErrorCode::InvalidArgument.error("worker fixture");
        // 把领域错误投影为固定 Worker envelope。
        let envelope = error_envelope(error);
        // 核对协议版本、稳定码与安全消息。
        assert_eq!(
            envelope,
            // 期望值不得包含 details 或原生事实。
            json!({
                // 标记失败。
                "ok": false,
                // 保持 Worker v2 协议版本。
                "contractVersion": "act/recording-worker/v2",
                // 保持最小错误对象。
                "error": {
                    // 保持封闭错误码。
                    "code": "INVALID_ARGUMENT",
                    // 保持安全消息。
                    "message": "worker fixture",
                },
            })
        );
    }

    // 验证极端序列化 fallback 也由封闭错误类型驱动。
    #[test]
    fn serialization_fallback_keeps_exact_v2_protocol_error() {
        // 核对完整单行响应文本逐字不变。
        assert_eq!(
            serialization_error_text(),
            // 保持既有稳定协议错误 envelope。
            r#"{"ok":false,"contractVersion":"act/recording-worker/v2","error":{"code":"WORKER_PROTOCOL_ERROR","message":"The recording worker could not serialize its response."}}"#
        );
    }
}
