//! 精确窗口捕获能力的隔离 worker 协议入口。

// 导入有界标准输入读取与单行标准输出写入。
use std::io::{Read, Write};
// 导入封闭文件路径类型。
use std::path::{Path, PathBuf};
// 导入有界等待时长。
use std::time::Duration;

// 导入严格请求反序列化。
use serde::Deserialize;
// 导入语言中立 JSON 数据。
use serde_json::{Value, json};

// 把错误码实现保留为 Capture Worker 协议边界的普通私有类型。
#[path = "capture_worker_error.rs"]
mod error_code;
// 导入当前 Worker 私有封闭错误码。
use error_code::CaptureWorkerErrorCode;

// 导入窗口重新发现、捕获 Component 与领域错误。
use crate::{
    // 导入私有 Windows adapter 边界。
    adapters::{
        // 导入窗口枚举、唯一解析与前景门禁。
        window::{capture_visible_titled_windows, ensure_foreground_unchanged, resolve_window},
        // 导入元数据探针与有界 PNG 捕获 Component。
        window_capture::{capture_window_png_bounded, probe_window_frame_metadata},
        // 导入前景只读快照。
        windows::foreground_hwnd,
    },
    // 导入 canonical opaque 目标解析器。
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    // 导入语言中立错误与结果。
    domain::{AppControlError, AppResult},
};

// 固定 capture worker 协议版本。
const CONTRACT_VERSION: &str = "act/capture-worker/v1";
// 固定只读首帧元数据 operation。
const FRAME_METADATA_OPERATION: &str = "window-frame-metadata";
// 固定原子窗口截图 operation。
const SCREENSHOT_OPERATION: &str = "window-screenshot";
// 限制请求为 64KiB。
const MAXIMUM_REQUEST_BYTES: u64 = 64 * 1024;
// 限制公开截图的单边像素。
const MAXIMUM_SCREENSHOT_DIMENSION: u32 = 4_096;
// 限制公开 PNG 候选为 64MiB。
const MAXIMUM_PNG_BYTES: u64 = 64 * 1024 * 1024;
// 固定 PNG 文件签名。
const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

// 定义严格版本化 worker 请求。
#[derive(Debug, Deserialize)]
// 使用 camelCase 并拒绝协议外字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkerRequest {
    // 保存协议版本。
    contract_version: String,
    // 保存固定操作名。
    operation: String,
    // 保存 canonical opaque 窗口目标。
    session_id: String,
    // 保存逐操作确认状态。
    confirmed: bool,
    // 保存首帧与进程总 deadline。
    timeout_ms: u32,
    // 只为截图保存由父 Module 独占的 staging 路径。
    staging_path: Option<String>,
}

// 严格解析并按确认优先顺序验证请求。
fn parse_request(text: &str) -> AppResult<WorkerRequest> {
    // 兼容 Windows stdio 调用方可能附带的单个 UTF-8 BOM。
    let normalized = text.trim_start_matches('\u{feff}').trim();
    // 只接受一个完整 JSON 对象。
    let request = serde_json::from_str::<WorkerRequest>(normalized).map_err(|_| {
        // 不回显可能敏感的输入。
        CaptureWorkerErrorCode::InvalidArgument.error(
            // 标明固定协议版本。
            "The capture worker request violates protocol v1.",
        )
    })?;
    // worker 必须先独立核对显式确认。
    if !request.confirmed {
        // 在解析或接触目标前拒绝。
        return Err(CaptureWorkerErrorCode::ConfirmationRequired.error(
            // 不回显目标。
            "Frame metadata capture requires explicit confirmation.",
        ));
    }
    // 拒绝未知协议或 operation。
    if request.contract_version != CONTRACT_VERSION
        || !matches!(
            request.operation.as_str(),
            FRAME_METADATA_OPERATION | SCREENSHOT_OPERATION
        )
    {
        // 不把 worker 扩展为第二控制面。
        return Err(CaptureWorkerErrorCode::CapabilityGap.error(
            // 说明固定协议表面。
            "The capture worker only accepts certified capture protocol v1 operations.",
        ));
    }
    // 截图使用正式契约的 250ms 下限，元数据探针保留 1ms 下限。
    let minimum_timeout_ms = if request.operation == SCREENSHOT_OPERATION {
        // 返回截图下限。
        250
    } else {
        // 返回探针下限。
        1
    };
    // 验证 operation 对应的封闭 deadline 范围。
    if !(minimum_timeout_ms..=30_000).contains(&request.timeout_ms) {
        // 拒绝无界或零 deadline。
        return Err(CaptureWorkerErrorCode::InvalidArgument.error(
            // 说明允许范围。
            "The capture worker timeout is outside the certified operation range.",
        ));
    }
    // 元数据探针不得携带文件写入字段。
    if request.operation == FRAME_METADATA_OPERATION && request.staging_path.is_some() {
        // 拒绝把只读探针扩展为隐式写入。
        return Err(CaptureWorkerErrorCode::InvalidArgument.error(
            // 不回显路径。
            "Frame metadata capture does not accept output fields.",
        ));
    }
    // 截图必须显式携带父 Module 已独占的 staging 路径。
    if request.operation == SCREENSHOT_OPERATION && request.staging_path.is_none() {
        // 拒绝不完整的写入请求。
        return Err(CaptureWorkerErrorCode::InvalidArgument.error(
            // 只说明协议缺口。
            "Window screenshot requires a reserved stagingPath.",
        ));
    }
    // 严格解析 canonical opaque 目标。
    let target = OpaqueTargetId::parse(&request.session_id);
    // 只允许第二代窗口目标。
    if target.map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        // 拒绝 native、旧版本和其他类别。
        return Err(CaptureWorkerErrorCode::InvalidArgument.error(
            // 不公开任何原生事实。
            "The capture worker requires a canonical s2:w target.",
        ));
    }
    // 返回完全验证后的请求。
    Ok(request)
}

// 在 worker 内重新发现目标并执行固定捕获 operation。
fn execute_request(request: WorkerRequest) -> AppResult<Value> {
    // 记录 worker 操作前的前景窗口。
    let foreground_before = foreground_hwnd();
    // 每次使用时重新枚举当前窗口快照。
    let windows = capture_visible_titled_windows()?;
    // 在 worker 内唯一重新解析 opaque 目标。
    let window = resolve_window(&request.session_id, &windows)?;
    // 按固定 operation 分派封闭领域行为。
    match request.operation.as_str() {
        // 执行只读首帧元数据探针。
        FRAME_METADATA_OPERATION => {
            execute_frame_metadata(&request, window.hwnd, foreground_before)
        }
        // 执行原子 PNG 截图。
        SCREENSHOT_OPERATION => execute_screenshot(&request, window.hwnd, foreground_before),
        // parse_request 已经拒绝未知 operation。
        _ => Err(CaptureWorkerErrorCode::CapabilityGap.error(
            // 不公开请求内容。
            "The capture worker operation is unavailable.",
        )),
    }
}

// 执行只读首帧元数据 operation。
fn execute_frame_metadata(
    // 借用已经验证的请求。
    request: &WorkerRequest,
    // 接收重新解析后的私有句柄。
    hwnd: isize,
    // 接收 worker 操作前的前景事实。
    foreground_before: isize,
) -> AppResult<Value> {
    // 只把私有句柄交给同进程 capture Component。
    let frame = probe_window_frame_metadata(
        // 传入私有句柄。
        hwnd,
        // 传入有界首帧等待时间。
        Duration::from_millis(u64::from(request.timeout_ms)),
    )?;
    // 记录操作后的前景窗口。
    let foreground_after = foreground_hwnd();
    // 前景变化必须结构化失败。
    ensure_foreground_unchanged(foreground_before, foreground_after)?;
    // 返回不含 surface、像素、文件或原生目标的 worker data。
    Ok(json!({
        // 输出首帧宽度。
        "frameWidth": frame.width,
        // 输出首帧高度。
        "frameHeight": frame.height,
        // 输出封闭驱动分类。
        "deviceDriver": frame.device_driver,
        // 明确未访问 frame surface。
        "frameSurfaceAccessed": false,
        // 明确未持久化像素。
        "pixelsPersisted": false,
        // 明确未写文件。
        "fileWritten": false,
        // 明确 worker 内前景不变。
        "foregroundUnchanged": true,
        // WGC 隐私指示器可能由系统显示。
        "privacyIndicatorMayHaveAppeared": true,
    }))
}

// 执行确认后的精确窗口原子截图 operation。
fn execute_screenshot(
    // 借用已经验证的请求。
    request: &WorkerRequest,
    // 接收重新解析后的私有句柄。
    hwnd: isize,
    // 接收 worker 操作前的前景事实。
    foreground_before: isize,
) -> AppResult<Value> {
    // 读取 parse_request 已保证存在的 staging 路径。
    let staging_text = request.staging_path.as_deref().ok_or_else(|| {
        // 防御内部状态漂移。
        CaptureWorkerErrorCode::InvalidArgument.error("Window screenshot output is missing.")
    })?;
    // 把 UTF-8 协议路径转换为平台无关路径类型。
    let staging_path = PathBuf::from(staging_text);
    // 验证固定 PNG 扩展名与非空文件名。
    validate_png_path(&staging_path)?;
    // worker 只接受父 Module 已创建的真实普通 staging 文件。
    validate_reserved_staging(&staging_path)?;
    // 只向私有 staging 捕获并编码 PNG。
    let capture = capture_window_png_bounded(
        // 传入 worker 内重新解析的私有句柄。
        hwnd,
        // writer 只能看见本次 staging。
        &staging_path,
        // 使用有界首帧等待。
        Duration::from_millis(u64::from(request.timeout_ms)),
        // 应用公开截图单边限制。
        MAXIMUM_SCREENSHOT_DIMENSION,
    )?;
    // 候选大小必须处于公开 schema 范围。
    if capture.bytes == 0 || capture.bytes > MAXIMUM_PNG_BYTES {
        // 父 Module 持有的 staging guard 将确定性清理超限候选。
        return Err(CaptureWorkerErrorCode::ScreenshotWriteFailed.error(
            // 不公开路径。
            "The encoded PNG is outside the certified size range.",
        ));
    }
    // 固定编码器结果仍需验证 PNG signature。
    validate_png_signature(&staging_path)?;
    // 在向父 Module 返回候选前记录捕获后的前景窗口。
    let foreground_after = foreground_hwnd();
    // 任何前景变化都由父 Module 清理 staging。
    ensure_foreground_unchanged(foreground_before, foreground_after)?;
    // 返回不含路径或原生目标的封闭截图候选事实。
    Ok(json!({
        // 输出捕获帧宽度。
        "frameWidth": capture.width,
        // 输出捕获帧高度。
        "frameHeight": capture.height,
        // 输出 PNG 字节数。
        "pngBytes": capture.bytes,
        // 输出封闭设备分类。
        "deviceDriver": capture.device_driver,
        // 输出原始 RGBA 像素摘要。
        "pixelDigest": capture.pixel_digest,
        // 声明候选已经完整写入父 Module 的 staging。
        "candidateWritten": true,
        // 声明未捕获鼠标指针。
        "cursorCaptured": false,
        // 声明 worker 内前景保持不变。
        "foregroundUnchanged": true,
        // WGC 隐私指示器可能由系统显示。
        "privacyIndicatorMayHaveAppeared": true,
    }))
}

// 验证 worker 收到的 staging 已由父 Module 独占。
fn validate_reserved_staging(path: &Path) -> AppResult<()> {
    // 使用非跟随元数据拒绝链接、目录和缺失路径。
    let metadata = std::fs::symlink_metadata(path).map_err(|_| {
        // 不公开 staging 路径。
        CaptureWorkerErrorCode::InvalidOutputPath.error(
            // 说明 staging 生命周期不成立。
            "The screenshot staging file is not reserved.",
        )
    })?;
    // 只接受真实普通文件。
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        // 拒绝可能改写目标边界的文件类型。
        return Err(CaptureWorkerErrorCode::InvalidOutputPath.error(
            // 不公开文件系统事实。
            "The screenshot staging target is not a regular file.",
        ));
    }
    // 返回 staging 类型验证成功。
    Ok(())
}

// 验证截图输出路径的稳定语法边界。
fn validate_png_path(path: &Path) -> AppResult<()> {
    // 路径必须具有真实文件名且扩展名严格为小写 png。
    let valid = path.file_name().is_some()
        // 读取扩展名。
        && path.extension().and_then(|value| value.to_str()) == Some("png");
    // 非 PNG 路径不得创建 staging。
    if !valid {
        // 返回稳定参数错误。
        return Err(CaptureWorkerErrorCode::InvalidOutputPath.error(
            // 不回显调用方路径。
            "Window screenshot output must use the .png extension.",
        ));
    }
    // 返回语法验证成功。
    Ok(())
}

// 验证已编码候选的固定 PNG signature。
fn validate_png_signature(path: &Path) -> AppResult<()> {
    // 打开唯一 staging 候选。
    let mut file = std::fs::File::open(path).map_err(|_| {
        // 不泄漏 staging 路径。
        CaptureWorkerErrorCode::ScreenshotWriteFailed.error(
            // 保持既有安全消息。
            "The staged PNG could not be verified.",
        )
    })?;
    // 准备固定八字节签名缓冲区。
    let mut signature = [0_u8; 8];
    // 完整读取候选签名。
    file.read_exact(&mut signature).map_err(|_| {
        // 截断候选失败闭合。
        CaptureWorkerErrorCode::ScreenshotWriteFailed.error(
            // 保持既有安全消息。
            "The staged PNG signature is incomplete.",
        )
    })?;
    // 候选必须逐字节匹配 PNG 签名。
    if signature != PNG_SIGNATURE {
        // 拒绝错误容器。
        return Err(CaptureWorkerErrorCode::ScreenshotWriteFailed.error(
            // 不公开候选内容。
            "The staged screenshot is not a valid PNG container.",
        ));
    }
    // 返回容器签名验证成功。
    Ok(())
}

// 构造 worker 成功 envelope。
fn success_envelope(data: Value) -> Value {
    // 返回固定协议形状。
    json!({
        // 标记成功。
        "ok": true,
        // 输出 worker 协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 输出封闭 data。
        "data": data,
    })
}

// 构造 worker 失败 envelope。
fn error_envelope(error: &AppControlError) -> Value {
    // 返回不含 provider 或原生事实的错误。
    json!({
        // 标记失败。
        "ok": false,
        // 输出 worker 协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 输出稳定错误。
        "error": {
            // 输出错误码。
            "code": error.code,
            // 输出安全消息。
            "message": error.message,
        },
    })
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
            Err(CaptureWorkerErrorCode::InvalidArgument.error(
                // 不回显输入。
                "The capture worker request exceeded 64 KiB.",
            ))
        }
        // 成功读取后解析并执行。
        Ok(_) => parse_request(&input).and_then(execute_request),
        // 读取失败时结构化返回。
        Err(_) => Err(CaptureWorkerErrorCode::InvalidArgument.error(
            // 不公开 I/O 细节。
            "The capture worker could not read its request.",
        )),
    };
    // 将结果投影为 worker envelope 与退出码。
    let (envelope, exit_code) = match result {
        // 成功返回零。
        Ok(data) => (success_envelope(data), 0),
        // 失败返回二。
        Err(error) => (error_envelope(&error), 2),
    };
    // 序列化单行 JSON。
    let text = match serde_json::to_string(&envelope) {
        // 保存成功文本。
        Ok(text) => text,
        // 极端序列化失败只能以进程码报告。
        Err(_) => return 2,
    };
    // 只向 stdout 输出结果。
    if writeln!(std::io::stdout(), "{text}").is_err() {
        // 管道关闭时返回失败码。
        return 2;
    }
    // 返回协议退出码。
    exit_code
}

// 声明纯协议解析测试。
#[cfg(test)]
mod tests {
    // 导入 JSON 夹具构造宏。
    use serde_json::json;

    // 导入父模块错误封装、解析器与私有错误类型。
    use super::{CaptureWorkerErrorCode, error_envelope, parse_request};

    // 验证 Worker 自有错误保持 v1 失败 envelope。
    #[test]
    fn worker_owned_error_keeps_v1_envelope() {
        // 构造不含 provider 详情的协议参数失败。
        let error = CaptureWorkerErrorCode::InvalidArgument.error("worker fixture");
        // 把领域错误投影为固定 worker envelope。
        let envelope = error_envelope(&error);
        // 核对协议版本、稳定码与安全消息。
        assert_eq!(
            // 核对实际 envelope。
            envelope,
            // 固定预期的 v1 失败形状。
            json!({
                // 标记失败。
                "ok": false,
                // 保持固定协议版本。
                "contractVersion": "act/capture-worker/v1",
                // 只输出稳定码与安全消息。
                "error": {
                    // 保持封闭参数错误码。
                    "code": "INVALID_ARGUMENT",
                    // 保持调用方提供的安全消息。
                    "message": "worker fixture",
                },
            })
        );
    }

    // 验证 worker 自身保持确认优先。
    #[test]
    fn missing_confirmation_is_rejected_before_target_validation() {
        // 使用同时缺少确认且目标非法的请求。
        let Err(error) = parse_request(
            // 构造固定协议 JSON。
            r#"{"contractVersion":"act/capture-worker/v1","operation":"window-frame-metadata","sessionId":"native:1","confirmed":false,"timeoutMs":5000}"#,
        ) else {
            // 请求意外成功时立即失败。
            panic!("missing confirmation must fail");
        };
        // 确认错误必须优先。
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    // 验证 canonical 目标和 deadline 可通过纯解析。
    #[test]
    fn canonical_confirmed_request_is_accepted() {
        // 解析固定合法请求。
        let Ok(request) = parse_request(
            // 构造 canonical s2:w JSON。
            r#"{"contractVersion":"act/capture-worker/v1","operation":"window-frame-metadata","sessionId":"s2:w:0123456789abcdef","confirmed":true,"timeoutMs":5000}"#,
        ) else {
            // 合法请求意外失败时立即失败。
            panic!("canonical request should pass");
        };
        // 核对 deadline。
        assert_eq!(request.timeout_ms, 5_000);
    }

    // 验证截图请求必须携带父 Module 预留的 staging 字段。
    #[test]
    fn screenshot_requires_reserved_staging_field() {
        // 解析缺少输出字段的截图请求。
        let Err(error) = parse_request(
            // 构造 canonical 但不完整的截图 JSON。
            r#"{"contractVersion":"act/capture-worker/v1","operation":"window-screenshot","sessionId":"s2:w:0123456789abcdef","confirmed":true,"timeoutMs":5000}"#,
        ) else {
            // 不完整请求意外成功时立即失败。
            panic!("screenshot output fields must be required");
        };
        // 核对稳定参数错误。
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }
}
