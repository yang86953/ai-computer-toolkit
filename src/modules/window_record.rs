//! 组合精确窗口发现、隔离编码与多产物事务的录制 Module。

// 把错误码与 worker 白名单实现保留为当前 Module 的普通私有类型。
#[path = "window_record_error.rs"]
mod error_code;

// 导入文件检查与 worker deadline。
use std::{
    // 验证父 Module 持有的实际视频候选。
    fs,
    // 保存公开与 staging 路径。
    path::PathBuf,
    // 构造有界 worker deadline。
    time::Duration,
};

// 导入语言中立 JSON 类型。
use serde_json::{Map, Value, json};

// 导入窗口事实、窄 Component 与领域错误。
use crate::{
    // 访问私有 Windows adapter 边界。
    adapters::{
        // 访问窗口枚举、唯一解析与前景门禁。
        window::{capture_visible_titled_windows, ensure_foreground_unchanged, resolve_window},
        // 访问零帧录制资格预检。
        window_capture_preflight_windows,
        // 访问前景只读事实。
        windows::foreground_hwnd,
    },
    // 导入稳定 capability ID。
    capabilities,
    // 导入 staging、取消、自有编码器、opaque ID 与 worker Component。
    components::{
        // 轮询统一取消状态。
        cancellation,
        // 私下检查 Media Foundation H.264 编码能力。
        media_foundation_encoder::MediaFoundationEncoder,
        // 严格解析 canonical opaque ID。
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
        // 执行 MP4 与分析目录事务。
        recording_artifacts::{
            // 导入多产物持有者。
            RecordingArtifacts,
            // 导入封闭提交错误。
            RecordingArtifactsError,
        },
        // 在 kill-on-close Job 中运行固定 worker。
        worker_process,
    },
    // 导入 provider-neutral 错误与结果。
    domain::{AppControlError, AppResult},
    // 复用录制参数边界。
    recording::RecordingConfig,
};

// 导入当前 Module 私有封闭错误码与 worker 白名单。
use error_code::WindowRecordErrorCode;

// 固定 Rust recording worker 文件名。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-recording-worker.exe";
// 固定内部 worker 协议版本。
const WORKER_CONTRACT_VERSION: &str = "act/recording-worker/v2";
// 限制 worker stdout 为 64KiB。
const MAXIMUM_WORKER_OUTPUT_BYTES: usize = 64 * 1024;
// 为 Job 回收与编码器退出保留固定宽限。
const WORKER_REAP_GRACE_MS: u64 = 5_000;
// 限制 MP4 候选为 2GiB。
const MAXIMUM_VIDEO_BYTES: u64 = 2 * 1024 * 1024 * 1024;

// 保存严格解析后的 worker 成功事实。
struct WorkerRecording {
    // 保存 MP4 字节数。
    bytes: u64,
    // 保存源宽度。
    source_width: u32,
    // 保存源高度。
    source_height: u32,
    // 保存编码宽度。
    width: u32,
    // 保存编码高度。
    height: u32,
    // 保存帧率。
    fps: u32,
    // 保存请求时长。
    duration_ms: u64,
    // 保存编码帧数。
    encoded_frames: u64,
    // 保存捕获帧数。
    captured_frames: u64,
    // 保存封闭设备分类。
    device_driver: &'static str,
    // 保存分析关键帧数量。
    keyframe_count: usize,
}

// 在业务接受前验证确认、输入与 canonical target 外壳。
pub(crate) fn validate_submission(
    // 接收 canonical opaque 窗口目标。
    session_id: &str,
    // 接收逐操作显式确认。
    confirmed: bool,
    // 接收 provider-neutral input 对象。
    input: &Value,
) -> AppResult<()> {
    // 复用私有配置解析但不向 System 暴露领域配置类型。
    parse_submission(session_id, confirmed, input).map(|_| ())
}

// 解析确认、输入与 canonical target 外壳并返回领域配置。
fn parse_submission(
    // 接收 canonical opaque 窗口目标。
    session_id: &str,
    // 接收逐操作显式确认。
    confirmed: bool,
    // 接收 provider-neutral input 对象。
    input: &Value,
) -> AppResult<RecordingConfig> {
    // 确认必须先于路径解析和目标语义。
    if !confirmed {
        // 缺少确认时立即失败。
        return Err(WindowRecordErrorCode::ConfirmationRequired.error(
            // 不回显目标或路径。
            "Exact window recording requires explicit confirmation.",
        ));
    }
    // 输入必须保持对象形状。
    let input_object = input.as_object().ok_or_else(|| {
        // 返回稳定参数错误。
        WindowRecordErrorCode::InvalidArgument.error("Window recording input must be an object.")
    })?;
    // 确认后解析封闭参数与路径外壳，但不触碰输出文件系统。
    let config = RecordingConfig::from_args_without_output_access(input_object)?;
    // 验证 canonical s2:w 目标。
    validate_target(session_id)?;
    // 返回无 I/O 的验证配置。
    Ok(config)
}

// 执行 confirmation-first 的精确窗口录制。
pub(crate) fn record(
    // 接收 canonical opaque 窗口目标。
    session_id: &str,
    // 接收逐操作显式确认。
    confirmed: bool,
    // 接收 provider-neutral input 对象。
    input: &Value,
) -> AppResult<Value> {
    // 同步公共路线复用进程级取消信号。
    record_with_cancellation(session_id, confirmed, input, cancellation::is_cancelled)
}

// 执行可由长操作任务独立取消的精确窗口录制。
pub(crate) fn record_with_cancellation(
    // 接收 canonical opaque 窗口目标。
    session_id: &str,
    // 接收逐操作显式确认。
    confirmed: bool,
    // 接收 provider-neutral input 对象。
    input: &Value,
    // 接收当前任务私有取消观察函数。
    cancelled: impl Fn() -> bool,
) -> AppResult<Value> {
    // 复用业务接受前的私有解析并取得领域配置。
    let config = parse_submission(session_id, confirmed, input)?;
    // 业务接受后、目标发现与捕获前验证输出文件系统状态。
    config.validate_output_access()?;
    // 记录主进程预检前的前景窗口。
    let foreground_before = foreground_hwnd();
    // 每次调用重新枚举当前窗口快照。
    let windows = capture_visible_titled_windows()?;
    // 在主进程内唯一重新解析 opaque 目标。
    let window = resolve_window(session_id, &windows)?;
    // 在创建 staging 或 worker 前执行零帧资格预检。
    let preflight = window_capture_preflight_windows::inspect(window.hwnd)?;
    // 不可捕获目标禁止创建任何输出候选。
    if preflight.eligibility != "eligible-for-certified-capture-route" {
        // 失败闭合且不激活窗口。
        return Err(WindowRecordErrorCode::CaptureTargetIneligible.error(
            // 只公开封闭分类。
            format!("Capture target is ineligible: {}.", preflight.eligibility),
        ));
    }
    // 主进程私下确认系统内建 H.264 编码能力可达。
    let encoder_available = MediaFoundationEncoder::is_h264_available()
        // 原生运行时错误统一映射为不可用。
        .unwrap_or(false);
    // 缺失编码能力时明确失败且不创建候选。
    if !encoder_available {
        // 返回稳定可达性错误。
        return Err(WindowRecordErrorCode::VideoEncoderUnavailable.error(
            // 不公开 COM、MFT 或系统组件身份。
            "No certified Media Foundation H.264 encoder is available.",
        ));
    }
    // 取得最终公开视频路径。
    let video_destination = config.public_output_path.clone();
    // 取得最终公开分析目录。
    let analysis_destination = config.public_analysis_dir.clone();
    // 同批预留视频与分析候选。
    let artifacts = RecordingArtifacts::reserve(&video_destination, &analysis_destination)
        // 映射封闭 staging 错误。
        .map_err(artifacts_error)?;
    // 内部 JSON 协议只接受 UTF-8 视频 staging 路径。
    let video_staging_path = artifacts
        // 借用父 Module 独占路径。
        .video_path()
        // 转为 UTF-8。
        .to_str()
        // 无法跨边界时失败闭合。
        .ok_or_else(invalid_output_path)?;
    // 内部 JSON 协议只接受 UTF-8 分析 staging 路径。
    let analysis_staging_dir = artifacts
        // 借用父 Module 独占目录。
        .analysis_path()
        // 转为 UTF-8。
        .to_str()
        // 无法跨边界时失败闭合。
        .ok_or_else(invalid_output_path)?;
    // 定位固定 Rust sibling worker。
    let worker = worker_process::sibling_companion_path(
        // 使用编译期固定文件名。
        WORKER_FILE_NAME,
        // 使用不含路径的安全描述。
        "recording worker",
    )?;
    // 构造严格版本化 worker 请求。
    let worker_request = json!({
        // 固定内部协议版本。
        "contractVersion": WORKER_CONTRACT_VERSION,
        // 只传递 canonical opaque 目标。
        "sessionId": session_id,
        // 重复传递确认供 worker 独立核对。
        "confirmed": true,
        // 传递完整封闭输入供 worker 独立验证。
        "input": input,
        // 只传递父 Module 独占视频 staging。
        "videoStagingPath": video_staging_path,
        // 只传递父 Module 独占分析 staging。
        "analysisStagingDir": analysis_staging_dir,
    });
    // 外层 deadline 包含录制时长、单帧等待上限与回收宽限。
    let worker_deadline = config
        // 读取录制时长。
        .duration
        // 加入首帧等待上限。
        .saturating_add(config.timeout)
        // 加入固定 Job 回收宽限。
        .saturating_add(Duration::from_millis(WORKER_REAP_GRACE_MS));
    // 运行结果先保存，确保错误路径同样核验前景和清理 staging。
    let worker_result = worker_process::run_companion(
        // 使用精确 sibling 路径。
        &worker,
        // worker 不接受命令行参数。
        &[],
        // 通过 stdin 传递严格 JSON。
        &worker_request,
        // 使用完整录制 deadline。
        worker_deadline,
        // 限制 worker stdout。
        MAXIMUM_WORKER_OUTPUT_BYTES,
        // 轮询统一取消状态。
        cancelled,
    );
    // 记录 worker 完成或整树回收后的前景窗口。
    let foreground_after = foreground_hwnd();
    // 任何前景变化都阻止最终提交。
    ensure_foreground_unchanged(foreground_before, foreground_after)?;
    // 将外层 watchdog 超时映射为录制 deadline。
    let worker_output = worker_result.map_err(map_worker_process_error)?;
    // 严格解析 worker 成功或失败 envelope。
    let recording = parse_worker_envelope(worker_output.exit_code, &worker_output.envelope)?;
    // 读取父 Module 实际持有的视频候选长度。
    let actual_bytes = fs::metadata(artifacts.video_path())
        // 候选消失时失败闭合。
        .map_err(|_| invalid_video())?
        // 只读取长度。
        .len();
    // worker 与父 Module 字节事实必须一致且有界。
    if actual_bytes != recording.bytes || !(12..=MAXIMUM_VIDEO_BYTES).contains(&actual_bytes) {
        // 协议漂移时清理全部候选并保留旧目标。
        return Err(worker_protocol_error());
    }
    // 父 Module 独立核对请求、worker 与 manifest 的交叉事实。
    verify_recording_consistency(
        // 传递已验证公开配置。
        &config,
        // 传递严格 worker 事实。
        &recording,
        // 传递父 Module 独占分析候选。
        artifacts.analysis_path(),
    )?;
    // 全部门禁通过后提交分析目录并原子提交 MP4。
    let commit = artifacts
        // 独立覆盖许可只进入最终事务点。
        .commit(config.overwrite)
        // 映射安装、回滚或视频错误。
        .map_err(artifacts_error)?;
    // 投影 provider-neutral window.record@1 结果。
    Ok(render(
        // 回显 canonical opaque 目标。
        session_id,
        // 传递公开视频路径。
        &video_destination,
        // 传递公开分析目录。
        &analysis_destination,
        // 传递严格 worker 事实。
        &recording,
        // 传递事务提交证据。
        commit.replaced_existing_video,
        // 传递分析目录建立证据。
        commit.created_analysis_directory,
    ))
}

// 验证 canonical 精确窗口目标。
fn validate_target(session_id: &str) -> AppResult<()> {
    // 严格解析第二代 opaque ID。
    let target = OpaqueTargetId::parse(session_id);
    // 只允许窗口类别。
    if target.map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        // 拒绝 native、旧版本与其他目标。
        return Err(WindowRecordErrorCode::InvalidArgument.error(
            // 不公开原生目标。
            "Window recording requires a canonical s2:w target.",
        ));
    }
    // 返回目标语法验证成功。
    Ok(())
}

// 严格解析 worker 成功或失败 envelope。
fn parse_worker_envelope(exit_code: u32, envelope: &Value) -> AppResult<WorkerRecording> {
    // envelope 必须是对象。
    let object = envelope.as_object().ok_or_else(worker_protocol_error)?;
    // 协议版本必须精确匹配。
    if object.get("contractVersion").and_then(Value::as_str) != Some(WORKER_CONTRACT_VERSION) {
        // 拒绝其他 worker 或协议漂移。
        return Err(worker_protocol_error());
    }
    // 读取严格成功标记。
    let ok = object
        // 访问固定字段。
        .get("ok")
        // 只接受布尔值。
        .and_then(Value::as_bool)
        // 缺失或类型错误为协议错误。
        .ok_or_else(worker_protocol_error)?;
    // 错误 envelope 使用固定三字段形状。
    if !ok {
        // 非零退出码是结构化失败的必要条件。
        if exit_code == 0 || object.len() != 3 {
            // 拒绝冲突状态。
            return Err(worker_protocol_error());
        }
        // 错误对象必须存在。
        let error = object
            // 读取固定错误字段。
            .get("error")
            // 只接受对象。
            .and_then(Value::as_object)
            // 缺失为协议错误。
            .ok_or_else(worker_protocol_error)?;
        // 错误对象只允许 code 与 message。
        if error.len() != 2 {
            // 拒绝路径或原生详情泄漏。
            return Err(worker_protocol_error());
        }
        // 读取并白名单化错误码。
        let code = error
            // 访问固定 code。
            .get("code")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 缺失为协议错误。
            .ok_or_else(worker_protocol_error)
            // 收窄为公开白名单。
            .and_then(worker_error_code)?;
        // 读取有界安全消息。
        let message = error
            // 访问固定 message。
            .get("message")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 限制消息长度。
            .filter(|value| !value.is_empty() && value.len() <= 512)
            // 缺失或超长为协议错误。
            .ok_or_else(worker_protocol_error)?;
        // 返回稳定领域错误。
        return Err(code.error(message));
    }
    // 成功必须使用零退出码和精确字段集合。
    if exit_code != 0 || object.len() != 16 {
        // 拒绝额外路径、PID 或原生字段。
        return Err(worker_protocol_error());
    }
    // 固定布尔隐私与前景证据必须全部成立。
    if object.get("audioCaptured").and_then(Value::as_bool) != Some(false)
        // 光标不得被捕获。
        || object.get("cursorCaptured").and_then(Value::as_bool) != Some(false)
        // 前景必须保持不变。
        || object.get("foregroundUnchanged").and_then(Value::as_bool) != Some(true)
    {
        // 拒绝不满足契约的成功。
        return Err(worker_protocol_error());
    }
    // 读取封闭设备分类。
    let device_driver = match object.get("deviceDriver").and_then(Value::as_str) {
        // 允许硬件设备事实。
        Some("hardware") => "hardware",
        // 允许 WARP 软件设备事实。
        Some("warp") => "warp",
        // 其他分类拒绝。
        _ => return Err(worker_protocol_error()),
    };
    // 返回全部有界成功事实。
    Ok(WorkerRecording {
        // 读取 MP4 字节数。
        bytes: bounded_u64(object, "bytes", 12, MAXIMUM_VIDEO_BYTES)?,
        // 读取源宽度。
        source_width: bounded_u32(object, "sourceWidth", 1, 16_384)?,
        // 读取源高度。
        source_height: bounded_u32(object, "sourceHeight", 1, 16_384)?,
        // 读取编码宽度。
        width: bounded_u32(object, "width", 2, 1_920)?,
        // 读取编码高度。
        height: bounded_u32(object, "height", 2, 16_384)?,
        // 读取帧率。
        fps: bounded_u32(object, "fps", 1, 10)?,
        // 读取时长。
        duration_ms: bounded_u64(object, "durationMs", 1_000, 300_000)?,
        // 读取编码帧数。
        encoded_frames: bounded_u64(object, "encodedFrames", 1, 3_000)?,
        // 读取捕获帧数。
        captured_frames: bounded_u64(object, "capturedFrames", 1, 3_001)?,
        // 保存封闭设备分类。
        device_driver,
        // 读取关键帧数量。
        keyframe_count: usize::try_from(bounded_u64(object, "keyframeCount", 1, 20)?)
            // 当前平台转换失败为协议错误。
            .map_err(|_| worker_protocol_error())?,
    })
}

// 读取有界 u64 worker 字段。
fn bounded_u64(
    // 接收 envelope 对象。
    object: &Map<String, Value>,
    // 接收固定字段名。
    name: &str,
    // 接收下界。
    minimum: u64,
    // 接收上界。
    maximum: u64,
) -> AppResult<u64> {
    // 只接受 JSON 无符号整数。
    let value = object
        // 访问固定字段。
        .get(name)
        // 只接受整数。
        .and_then(Value::as_u64)
        // 缺失或错误类型为协议错误。
        .ok_or_else(worker_protocol_error)?;
    // 应用闭区间边界。
    if !(minimum..=maximum).contains(&value) {
        // 越界表示协议漂移。
        return Err(worker_protocol_error());
    }
    // 返回有界值。
    Ok(value)
}

// 读取有界 u32 worker 字段。
fn bounded_u32(
    // 接收 envelope 对象。
    object: &Map<String, Value>,
    // 接收固定字段名。
    name: &str,
    // 接收下界。
    minimum: u32,
    // 接收上界。
    maximum: u32,
) -> AppResult<u32> {
    // 先按 u64 读取并应用 u32 边界。
    let value = bounded_u64(object, name, u64::from(minimum), u64::from(maximum))?;
    // 边界保证转换成功。
    u32::try_from(value).map_err(|_| worker_protocol_error())
}

// 独立核对请求、worker envelope 与 manifest 的关键事实。
fn verify_recording_consistency(
    // 接收公开请求配置。
    config: &RecordingConfig,
    // 接收严格 worker 成功事实。
    recording: &WorkerRecording,
    // 接收父 Module 独占分析 staging。
    analysis_staging: &std::path::Path,
) -> AppResult<()> {
    // 根据同一公开算法计算预期编码帧数。
    let expected_frames = config
        // 读取毫秒时长。
        .duration_ms()
        // 乘以有界帧率。
        .saturating_mul(u64::from(config.fps))
        // 向上取整到帧。
        .div_ceil(1_000)
        // 至少编码一帧。
        .max(1);
    // worker 必须精确回显请求并保持帧与尺寸边界。
    if recording.fps != config.fps
        // 时长必须精确一致。
        || recording.duration_ms != config.duration_ms()
        // 编码帧数必须与固定调度算法一致。
        || recording.encoded_frames != expected_frames
        // 捕获帧数不能超过编码帧数。
        || recording.captured_frames > recording.encoded_frames
        // H.264 yuv420p 宽度必须是正偶数且不超过请求上限。
        || !recording.width.is_multiple_of(2)
        // 编码宽度不得超过 maxWidth。
        || recording.width > config.max_width
        // H.264 yuv420p 高度必须是正偶数。
        || !recording.height.is_multiple_of(2)
        // 关键帧不得超过请求上限。
        || recording.keyframe_count > config.max_keyframes
    {
        // 任一交叉事实不一致都视为 worker 协议失效。
        return Err(worker_protocol_error());
    }
    // 读取候选 manifest。
    let manifest_bytes = fs::read(analysis_staging.join("manifest.json"))
        // 丢失或不可读时拒绝提交。
        .map_err(|_| worker_protocol_error())?;
    // manifest 限制为 1MiB。
    if manifest_bytes.is_empty() || manifest_bytes.len() > 1024 * 1024 {
        // 拒绝空或异常大候选。
        return Err(worker_protocol_error());
    }
    // 解析单个 JSON manifest。
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        // 无效 JSON 拒绝提交。
        .map_err(|_| worker_protocol_error())?;
    // 读取固定 video 与 analysis 对象。
    let video = manifest
        // 访问 video 字段。
        .get("video")
        // 只接受对象。
        .and_then(Value::as_object)
        // 缺失为协议错误。
        .ok_or_else(worker_protocol_error)?;
    // 读取固定 analysis 对象。
    let analysis = manifest
        // 访问 analysis 字段。
        .get("analysis")
        // 只接受对象。
        .and_then(Value::as_object)
        // 缺失为协议错误。
        .ok_or_else(worker_protocol_error)?;
    // 读取 selectedKeyframes 数组。
    let selected = analysis
        // 访问固定字段。
        .get("selectedKeyframes")
        // 只接受数组。
        .and_then(Value::as_array)
        // 缺失为协议错误。
        .ok_or_else(worker_protocol_error)?;
    // manifest 必须精确回显关键公开事实。
    if manifest.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        // 视频路径必须是最终公开路径而不是 staging。
        || video.get("path") != Some(&json!(config.public_output_path))
        // 视频字节数必须与实际候选一致。
        || video.get("bytes").and_then(Value::as_u64) != Some(recording.bytes)
        // 编码器必须声明固定 H.264。
        || video.get("codec").and_then(Value::as_str) != Some("H.264")
        // 容器必须声明固定 MP4。
        || video.get("container").and_then(Value::as_str) != Some("MP4")
        // 帧率必须一致。
        || video.get("fps").and_then(Value::as_u64) != Some(u64::from(recording.fps))
        // 时长必须一致。
        || video.get("durationMs").and_then(Value::as_u64) != Some(recording.duration_ms)
        // 源宽度必须一致。
        || video.get("sourceWidth").and_then(Value::as_u64)
            != Some(u64::from(recording.source_width))
        // 源高度必须一致。
        || video.get("sourceHeight").and_then(Value::as_u64)
            != Some(u64::from(recording.source_height))
        // 编码宽度必须一致。
        || video.get("width").and_then(Value::as_u64) != Some(u64::from(recording.width))
        // 编码高度必须一致。
        || video.get("height").and_then(Value::as_u64) != Some(u64::from(recording.height))
        // 编码帧数必须一致。
        || video.get("encodedFrames").and_then(Value::as_u64)
            != Some(recording.encoded_frames)
        // 捕获帧数必须一致。
        || video.get("capturedFrames").and_then(Value::as_u64)
            != Some(recording.captured_frames)
        // provider-neutral 质量必须与请求一致。
        || video.get("quality").and_then(Value::as_u64) != Some(u64::from(config.quality))
        // 音频必须关闭。
        || video.get("audioCaptured").and_then(Value::as_bool) != Some(false)
        // 光标必须关闭。
        || video.get("cursorCaptured").and_then(Value::as_bool) != Some(false)
        // 分析策略必须保持固定。
        || analysis.get("strategy").and_then(Value::as_str)
            != Some("temporal-difference-keyframes")
        // 变化阈值必须与请求一致。
        || analysis.get("changeThreshold").and_then(Value::as_f64)
            != Some(config.change_threshold)
        // 最大关键帧必须与请求一致。
        || analysis.get("maxKeyframes").and_then(Value::as_u64)
            != Some(u64::try_from(config.max_keyframes).unwrap_or(u64::MAX))
        // storyboard 必须引用最终公开路径。
        || analysis.get("storyboardPath")
            != Some(&json!(config.public_analysis_dir.join("storyboard.png")))
        // selected 数量必须与 worker 一致。
        || selected.len() != recording.keyframe_count
    {
        // 任一 manifest 漂移阻止最终提交。
        return Err(worker_protocol_error());
    }
    // 保存已经验证的关键帧文件名以拒绝重复引用。
    let mut selected_names = Vec::with_capacity(selected.len());
    // 每个 selectedKeyframe 只能引用最终分析目录中的固定 PNG 名称。
    for frame in selected {
        // 读取公开路径字符串。
        let path = frame
            // 访问固定 path。
            .get("path")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 缺失为协议错误。
            .map(PathBuf::from)
            // 转换失败为协议错误。
            .ok_or_else(worker_protocol_error)?;
        // 路径父目录必须精确等于最终分析目录。
        let valid_parent = path.parent() == Some(config.public_analysis_dir.as_path());
        // 文件名必须属于固定关键帧形状。
        let file_name = path
            // 读取文件名。
            .file_name()
            // 转为 UTF-8。
            .and_then(|value| value.to_str());
        // 文件名必须属于固定关键帧形状。
        let valid_name = file_name
            // 核对固定前后缀。
            .is_some_and(|value| value.starts_with("frame-") && value.ends_with("ms.png"));
        // 任一越界路径阻止提交。
        if !valid_parent || !valid_name {
            // 返回统一协议错误。
            return Err(worker_protocol_error());
        }
        // 取得已经通过形状验证的文件名。
        let name = file_name.ok_or_else(worker_protocol_error)?;
        // manifest 不得重复引用同一关键帧。
        if selected_names.iter().any(|value| value == name) {
            // 重复引用表示分析事实不完整。
            return Err(worker_protocol_error());
        }
        // 候选目录中必须存在同名真实非空普通文件。
        let metadata = fs::symlink_metadata(analysis_staging.join(name))
            // 丢失候选为协议错误。
            .map_err(|_| worker_protocol_error())?;
        // 拒绝链接、目录与空关键帧。
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
            // 阻止提交无效候选。
            return Err(worker_protocol_error());
        }
        // 保存唯一文件名。
        selected_names.push(name.to_owned());
    }
    // 全部交叉事实一致。
    Ok(())
}

// 把 worker 错误码限制在稳定白名单。
fn worker_error_code(code: &str) -> AppResult<WindowRecordErrorCode> {
    // 未知 provider 错误统一收敛为 Module 协议违规。
    WindowRecordErrorCode::from_worker(code).ok_or_else(worker_protocol_error)
}

// 投影公开成功结果。
fn render(
    // 接收调用方已知目标。
    session_id: &str,
    // 接收最终视频路径。
    video_path: &PathBuf,
    // 接收最终分析目录。
    analysis_directory: &PathBuf,
    // 接收严格 worker 事实。
    recording: &WorkerRecording,
    // 接收视频替换事实。
    replaced_existing_video: bool,
    // 接收分析目录建立事实。
    created_analysis_directory: bool,
) -> Value {
    // 只返回 provider-neutral 字段和调用方已知路径。
    json!({
        // 标记成功。
        "ok": true,
        // 标记稳定 capability。
        "capability": capabilities::WINDOW_RECORD,
        // 回显 canonical opaque 目标。
        "targetId": session_id,
        // 保留 desktop 兼容目标形状且不含原生字段。
        "target": { "sessionId": session_id },
        // 标记隔离后台执行。
        "executionMode": "isolated-windows-graphics-capture",
        // 标记固定捕获 API。
        "captureMethod": "Windows.Graphics.Capture",
        // 标记固定编码器契约而不公开路径。
        "encoder": "windows-media-foundation-h264",
        // 回显调用方最终 MP4 路径。
        "path": video_path,
        // 输出真实 MP4 字节数。
        "bytes": recording.bytes,
        // 输出源宽度。
        "sourceWidth": recording.source_width,
        // 输出源高度。
        "sourceHeight": recording.source_height,
        // 输出编码宽度。
        "width": recording.width,
        // 输出编码高度。
        "height": recording.height,
        // 输出帧率。
        "fps": recording.fps,
        // 输出请求时长。
        "durationMs": recording.duration_ms,
        // 输出编码帧数。
        "encodedFrames": recording.encoded_frames,
        // 输出实际捕获帧数。
        "capturedFrames": recording.captured_frames,
        // 输出封闭设备分类。
        "deviceDriver": recording.device_driver,
        // 明确音频未捕获。
        "audioCaptured": false,
        // 明确光标未捕获。
        "cursorCaptured": false,
        // 声明 WGC 系统隐私指示器可能出现。
        "systemCaptureIndicatorMayAppear": true,
        // 输出分析产物事实。
        "analysis": {
            // 回显最终分析目录。
            "directory": analysis_directory,
            // 输出最终 storyboard 路径。
            "storyboardPath": analysis_directory.join("storyboard.png"),
            // 输出最终 manifest 路径。
            "manifestPath": analysis_directory.join("manifest.json"),
            // 输出关键帧数量。
            "keyframeCount": recording.keyframe_count,
            // 推荐先使用 storyboard。
            "recommendedAiInput": "storyboardPath",
            // 完整视频只用于审计。
            "fullVideoRole": "audit-only",
        },
        // 输出前景未变证据。
        "foreground": { "unchanged": true },
        // 输出事务覆盖证据。
        "replacedExistingVideo": replaced_existing_video,
        // 输出分析目录建立证据。
        "createdAnalysisDirectory": created_analysis_directory,
    })
}

// 把 worker process 错误映射到录制领域。
fn map_worker_process_error(error: AppControlError) -> AppControlError {
    // 外层 watchdog 超时使用稳定录制超时码。
    if error.code == WindowRecordErrorCode::WorkerProcessTimeout.as_str() {
        // 返回不泄漏进程信息的 deadline 错误。
        return WindowRecordErrorCode::VideoRecordingTimeout.error(
            // 明确整棵 Job 已回收。
            "The isolated recording deadline elapsed and the worker job was terminated.",
        );
    }
    // 其他进程错误保持结构化分类。
    error
}

// 把多产物 Component 错误映射为领域错误。
fn artifacts_error(error: RecordingArtifactsError) -> AppControlError {
    // 按错误分类返回稳定公开语义。
    match error {
        // 视频覆盖竞态仍要求独立覆盖确认。
        RecordingArtifactsError::Video(crate::components::atomic_file::AtomicFileError::TargetExists) => {
            // 返回覆盖确认错误。
            WindowRecordErrorCode::OverwriteConfirmationRequired.error(
                // 不公开路径。
                "Existing recording output requires overwrite confirmation.",
            )
        }
        // 无效最终目标或 staging 都映射为输出路径错误。
        RecordingArtifactsError::InvalidDestination
        // 合并 staging 创建失败。
        | RecordingArtifactsError::StagingCreationFailed
        // 合并底层原子无效目标。
        | RecordingArtifactsError::Video(
            crate::components::atomic_file::AtomicFileError::InvalidDestination
            | crate::components::atomic_file::AtomicFileError::StagingCreationFailed
            | crate::components::atomic_file::AtomicFileError::InvalidStaging,
        ) => invalid_output_path(),
        // worker 候选无效映射为分析失败。
        RecordingArtifactsError::InvalidCandidate => WindowRecordErrorCode::VideoAnalysisFailed.error(
            // 不公开 staging 内容。
            "The isolated recording analysis candidate is invalid.",
        ),
        // 分析安装失败保持事务未提交语义。
        RecordingArtifactsError::InstallFailed => WindowRecordErrorCode::VideoArtifactCommitFailed.error(
            // 说明公开产物未整批建立。
            "The recording artifacts could not be committed as one batch.",
        ),
        // 回滚失败必须明确结果不确定。
        RecordingArtifactsError::RollbackFailed => WindowRecordErrorCode::VideoArtifactResultUnknown.error(
            // 要求调用方人工检查精确目标。
            "Recording artifact rollback failed; the final output state requires inspection.",
        ),
        // 其他原子视频失败统一为提交失败。
        RecordingArtifactsError::Video(_) => WindowRecordErrorCode::VideoArtifactCommitFailed.error(
            // 不公开底层路径或 OS 错误。
            "The recording video candidate could not be committed.",
        ),
    }
}

// 构造不泄漏路径的输出错误。
fn invalid_output_path() -> AppControlError {
    // 返回固定路径错误。
    WindowRecordErrorCode::InvalidOutputPath.error(
        // 不公开最终或 staging 路径。
        "The recording output path cannot be used by the isolated transaction.",
    )
}

// 构造不泄漏路径的视频候选错误。
fn invalid_video() -> AppControlError {
    // 返回固定视频缺失错误。
    WindowRecordErrorCode::VideoOutputMissing.error(
        // 不公开候选路径。
        "The isolated recorder did not produce a valid MP4 candidate.",
    )
}

// 构造严格 worker 协议错误。
fn worker_protocol_error() -> AppControlError {
    // 返回不包含 worker 输出的稳定错误。
    WindowRecordErrorCode::WorkerProtocolError.error(
        // 不回显可能含私有信息的 envelope。
        "The recording worker returned an invalid response.",
    )
}

// 声明纯协议与投影测试。
#[cfg(test)]
// 将 Module 私有契约测试拆分到独立源文件以满足单文件行数上限。
#[path = "window_record_tests.rs"]
mod tests;
