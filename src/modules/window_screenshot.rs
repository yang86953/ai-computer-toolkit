//! 组合精确窗口发现、隔离捕获与原子 PNG 提交的截图 Module。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "window_screenshot_error.rs"]
mod error_code;

// 导入文件路径与有界 worker deadline。
use std::{path::PathBuf, time::Duration};

// 导入语言中立 JSON 类型。
use serde_json::{Value, json};

// 导入当前 Module 私有封闭错误码。
use error_code::WindowScreenshotErrorCode;

// 导入窗口事实、窄 Component 与领域错误。
use crate::{
    // 导入私有 Windows adapter 边界。
    adapters::{
        // 导入窗口枚举、唯一解析与前景门禁。
        window::{capture_visible_titled_windows, ensure_foreground_unchanged, resolve_window},
        // 导入零帧截图 eligibility 预检。
        window_capture_preflight_windows,
        // 导入前景只读快照。
        windows::foreground_hwnd,
    },
    // 导入稳定 capability ID。
    capabilities,
    // 导入原子输出、取消、opaque ID 与 worker Component。
    components::{
        // 导入原子单文件提交边界。
        atomic_file::{AtomicFileError, StagedFile},
        // 导入统一取消状态。
        cancellation,
        // 导入 canonical opaque ID 解析器。
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
        // 导入写前覆盖门禁。
        output_guard::{OutputGuardError, guard_file_output},
        // 导入 Job 约束 companion worker 执行器。
        worker_process,
    },
    // 导入语言中立错误与结果。
    domain::{AppControlError, AppResult},
};

// 固定 Rust capture worker 文件名。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-capture-worker.exe";
// 固定内部 capture worker 协议版本。
const WORKER_CONTRACT_VERSION: &str = "act/capture-worker/v1";
// 固定窗口截图 operation。
const WORKER_OPERATION: &str = "window-screenshot";
// 限制 worker stdout 为 64KiB。
const MAXIMUM_WORKER_OUTPUT_BYTES: usize = 64 * 1024;
// 限制 PNG 为 64MiB。
const MAXIMUM_PNG_BYTES: u64 = 64 * 1024 * 1024;
// 固定默认截图 deadline。
const DEFAULT_TIMEOUT_MS: u32 = 5_000;

// 保存确认后解析的 provider-neutral 截图输入。
#[derive(Clone, Debug, Eq, PartialEq)]
struct ScreenshotInput {
    // 保存 UTF-8 最终输出路径。
    path: String,
    // 保存隔离 worker deadline。
    timeout_ms: u32,
    // 保存独立覆盖许可。
    overwrite: bool,
}

// 保存经过严格协议验证的截图候选事实。
#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkerScreenshot {
    // 保存正宽度。
    width: u32,
    // 保存正高度。
    height: u32,
    // 保存 PNG 字节数。
    bytes: u64,
    // 保存封闭设备分类。
    device_driver: &'static str,
    // 保存原始 RGBA 像素摘要。
    pixel_digest: String,
}

// 执行 confirmation-first 的精确窗口截图。
pub(crate) fn screenshot(
    // 接收调用方 canonical opaque 窗口目标。
    session_id: &str,
    // 接收逐操作显式确认。
    confirmed: bool,
    // 接收 provider-neutral input 对象。
    input: &Value,
) -> AppResult<Value> {
    // 确认必须先于输入路径解析、目标发现和任何文件操作。
    if !confirmed {
        // 缺少确认时立即失败。
        return Err(WindowScreenshotErrorCode::ConfirmationRequired.error(
            // 不回显目标或路径。
            "Exact window screenshot requires explicit confirmation.",
        ));
    }
    // 确认后解析封闭输入集合。
    let input = parse_input(input)?;
    // 验证 canonical s2:w 目标。
    validate_target(session_id)?;
    // 记录主进程预检前的前景窗口。
    let foreground_before = foreground_hwnd();
    // 每次调用重新枚举当前窗口快照。
    let windows = capture_visible_titled_windows()?;
    // 在主进程内唯一重新解析 opaque 目标。
    let window = resolve_window(session_id, &windows)?;
    // 在创建 staging 或 worker 前执行零帧 eligibility 预检。
    let preflight = window_capture_preflight_windows::inspect(window.hwnd)?;
    // 不可捕获目标禁止创建任何输出候选。
    if preflight.eligibility != "eligible-for-certified-capture-route" {
        // 将两个稳定窗口状态投影为既有精确公开分类。
        let error_code = match preflight.eligibility {
            // 隐藏目标使用稳定隐藏分类。
            "window-not-visible" => WindowScreenshotErrorCode::CaptureTargetHidden,
            // 最小化目标使用稳定最小化分类。
            "window-minimized" => WindowScreenshotErrorCode::CaptureTargetMinimized,
            // 其余资格缺口保持通用封闭分类。
            _ => WindowScreenshotErrorCode::CaptureTargetIneligible,
        };
        // 失败闭合且不激活窗口。
        return Err(error_code.error(
            // 只公开封闭分类。
            format!("Capture target is ineligible: {}.", preflight.eligibility),
        ));
    }
    // 转换最终输出路径。
    let destination = PathBuf::from(&input.path);
    // 在创建 staging 前执行统一覆盖门禁。
    guard_file_output(&destination, input.overwrite).map_err(output_guard_error)?;
    // 独占目标同目录的私有 staging 直到最终提交。
    let staged = StagedFile::reserve(&destination).map_err(atomic_file_error)?;
    // 内部 JSON 协议只接受可表示为 UTF-8 的 staging 路径。
    let staging_path = staged.path().to_str().ok_or_else(|| {
        // 返回稳定输出路径错误。
        WindowScreenshotErrorCode::InvalidOutputPath.error(
            // 不公开平台路径编码。
            "The screenshot staging path cannot cross the UTF-8 worker boundary.",
        )
    })?;
    // 定位固定 Rust sibling worker。
    let worker = worker_process::sibling_companion_path(
        // 使用固定文件名。
        WORKER_FILE_NAME,
        // 使用安全诊断描述。
        "capture worker",
    )?;
    // 构造版本化 worker 请求。
    let request = json!({
        // 固定内部协议版本。
        "contractVersion": WORKER_CONTRACT_VERSION,
        // 固定截图 operation。
        "operation": WORKER_OPERATION,
        // 只传递 canonical opaque 目标。
        "sessionId": session_id,
        // 重复传递确认供 worker 独立核对。
        "confirmed": true,
        // 传递有界 deadline。
        "timeoutMs": input.timeout_ms,
        // 只传递父 Module 独占的 staging 路径。
        "stagingPath": staging_path,
    });
    // 运行结果先保存，确保错误路径同样执行前景核验和 staging 清理。
    let worker_result = worker_process::run_companion(
        // 使用精确 sibling 路径。
        &worker,
        // worker 不接受命令行参数。
        &[],
        // 通过 stdin 传递严格 JSON 请求。
        &request,
        // 使用调用方有界 deadline。
        Duration::from_millis(u64::from(input.timeout_ms)),
        // 限制 worker stdout。
        MAXIMUM_WORKER_OUTPUT_BYTES,
        // 轮询统一取消状态。
        cancellation::is_cancelled,
    );
    // 记录 worker 完成或回收后的前景窗口。
    let foreground_after = foreground_hwnd();
    // 前景变化优先于 worker 结果并阻止最终提交。
    ensure_foreground_unchanged(foreground_before, foreground_after)?;
    // 传播 worker 进程级失败。
    let worker_output = worker_result?;
    // 严格解析 worker 协议 envelope。
    let capture = parse_worker_envelope(worker_output.exit_code, &worker_output.envelope)?;
    // worker 报告字节数必须匹配父 Module 持有的实际 staging。
    let staged_bytes = std::fs::metadata(staged.path())
        // 不公开 staging 路径。
        .map_err(|_| {
            // 返回稳定写入错误。
            WindowScreenshotErrorCode::ScreenshotWriteFailed.error(
                // 说明候选丢失。
                "The screenshot candidate disappeared before commit.",
            )
        })?
        // 只读取字节长度。
        .len();
    // 双边字节事实必须一致且不超过公开上限。
    if staged_bytes != capture.bytes || !(1..=MAXIMUM_PNG_BYTES).contains(&staged_bytes) {
        // 保持既有目标并由 staging Drop 清理候选。
        return Err(worker_protocol_error());
    }
    // 所有主进程与 worker 门禁通过后才原子提交最终文件。
    let commit = staged
        // 独立覆盖许可只传给最终提交点。
        .commit(input.overwrite)
        // 映射提交竞态或持久化失败。
        .map_err(atomic_file_error)?;
    // 投影公开 window.screenshot@1 schema。
    Ok(render(
        // 回显 canonical opaque 目标。
        session_id,
        // 回显调用方最终路径。
        &input.path,
        // 回显独立覆盖许可。
        input.overwrite,
        // 传递严格 worker 事实。
        &capture,
        // 保留提交替换证据供内部一致性核对。
        commit.replaced_existing,
    ))
}

// 在确认后解析 provider-neutral 输入。
fn parse_input(input: &Value) -> AppResult<ScreenshotInput> {
    // 输入必须是对象。
    let object = input.as_object().ok_or_else(|| {
        // 返回稳定参数错误。
        WindowScreenshotErrorCode::InvalidArgument
            .error("Window screenshot input must be an object.")
    })?;
    // 只允许固定 path、timeoutMs 与 overwrite 字段。
    if object
        // 遍历全部调用方字段。
        .keys()
        // 检测协议外字段。
        .any(|key| !matches!(key.as_str(), "path" | "timeoutMs" | "overwrite"))
    {
        // 拒绝任意 provider 或命令字段。
        return Err(WindowScreenshotErrorCode::InvalidArgument.error(
            // 说明封闭输入集合。
            "Window screenshot input accepts path, timeoutMs and overwrite only.",
        ));
    }
    // 读取必需 UTF-8 路径。
    let path = object
        // 访问 path 字段。
        .get("path")
        // 只接受 JSON 字符串。
        .and_then(Value::as_str)
        // 要求最短 a.png 且限制 Windows 路径长度。
        .filter(|value| (5..=32_767).contains(&value.len()))
        // 缺失或超限时返回参数错误。
        .ok_or_else(|| {
            // 不回显路径内容。
            WindowScreenshotErrorCode::InvalidOutputPath.error(
                // 说明固定 PNG 路径要求。
                "Window screenshot requires a bounded UTF-8 .png path.",
            )
        })?;
    // 扩展名必须严格为小写 png。
    if PathBuf::from(path)
        .extension()
        .and_then(|value| value.to_str())
        != Some("png")
    {
        // 在文件系统检查前拒绝错误容器。
        return Err(WindowScreenshotErrorCode::InvalidOutputPath.error(
            // 不回显路径。
            "Window screenshot output must use the .png extension.",
        ));
    }
    // 读取可选 deadline。
    let timeout_ms = match object.get("timeoutMs") {
        // 缺失时使用固定默认值。
        None => DEFAULT_TIMEOUT_MS,
        // 只接受可无损转换的无符号整数。
        Some(value) => value
            // 读取 JSON 整数。
            .as_u64()
            // 转换为 u32。
            .and_then(|value| u32::try_from(value).ok())
            // 类型或范围前置转换失败。
            .ok_or_else(|| {
                // 返回稳定参数错误。
                WindowScreenshotErrorCode::InvalidArgument.error(
                    // 说明整数类型。
                    "Window screenshot timeoutMs must be an integer.",
                )
            })?,
    };
    // 正式截图 deadline 必须为 250..30000ms。
    if !(250..=30_000).contains(&timeout_ms) {
        // 拒绝过短或无界 worker。
        return Err(WindowScreenshotErrorCode::InvalidArgument.error(
            // 说明允许范围。
            "Window screenshot timeoutMs must be 250..30000ms.",
        ));
    }
    // 读取可选独立覆盖许可。
    let overwrite = match object.get("overwrite") {
        // 缺失时禁止覆盖。
        None => false,
        // 只接受布尔值。
        Some(value) => value.as_bool().ok_or_else(|| {
            // 返回稳定参数错误。
            WindowScreenshotErrorCode::InvalidArgument.error(
                // 说明布尔类型。
                "Window screenshot overwrite must be a boolean.",
            )
        })?,
    };
    // 返回完全验证后的输入。
    Ok(ScreenshotInput {
        // 冻结路径字符串。
        path: path.to_owned(),
        // 冻结 deadline。
        timeout_ms,
        // 冻结覆盖许可。
        overwrite,
    })
}

// 验证 canonical 精确窗口目标。
fn validate_target(session_id: &str) -> AppResult<()> {
    // 严格解析第二代 opaque ID。
    let target = OpaqueTargetId::parse(session_id);
    // 只允许窗口类别。
    if target.map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        // 拒绝 native、旧版本与其他目标。
        return Err(WindowScreenshotErrorCode::InvalidArgument.error(
            // 不公开原生目标。
            "Window screenshot requires a canonical s2:w target.",
        ));
    }
    // 返回目标语法验证成功。
    Ok(())
}

// 严格解析 worker 成功或失败 envelope。
fn parse_worker_envelope(exit_code: u32, envelope: &Value) -> AppResult<WorkerScreenshot> {
    // 顶层必须是对象。
    let object = envelope.as_object().ok_or_else(worker_protocol_error)?;
    // 验证固定协议版本。
    if object.get("contractVersion").and_then(Value::as_str) != Some(WORKER_CONTRACT_VERSION) {
        // 未知版本必须失败闭合。
        return Err(worker_protocol_error());
    }
    // 读取布尔成功标记。
    let ok = object
        // 访问 ok 字段。
        .get("ok")
        // 只接受布尔值。
        .and_then(Value::as_bool)
        // 缺失时返回协议错误。
        .ok_or_else(worker_protocol_error)?;
    // 失败 envelope 只允许固定形状和退出码二。
    if !ok {
        // 验证顶层失败形状。
        if exit_code != 2 || object.len() != 3 || !object.contains_key("error") {
            // 拒绝不一致进程事实。
            return Err(worker_protocol_error());
        }
        // 读取严格 error 对象。
        let error = object
            // 访问 error 字段。
            .get("error")
            // 只接受对象。
            .and_then(Value::as_object)
            // 缺失时返回协议错误。
            .ok_or_else(worker_protocol_error)?;
        // error 只允许 code 和 message。
        if error.len() != 2 {
            // 拒绝额外 provider 事实。
            return Err(worker_protocol_error());
        }
        // 读取稳定错误码。
        let code = error
            // 访问 code 字段。
            .get("code")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 缺失时返回协议错误。
            .ok_or_else(worker_protocol_error)?;
        // 读取安全错误消息。
        let message = error
            // 访问 message 字段。
            .get("message")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 缺失时返回协议错误。
            .ok_or_else(worker_protocol_error)?;
        // 只转发固定白名单错误。
        return Err(worker_error_code(code)?.error(message));
    }
    // 成功 envelope 必须配合退出码零和精确字段。
    if exit_code != 0 || object.len() != 3 || !object.contains_key("data") {
        // 拒绝不一致成功事实。
        return Err(worker_protocol_error());
    }
    // 读取严格 data 对象。
    let data = object
        // 访问 data 字段。
        .get("data")
        // 只接受对象。
        .and_then(Value::as_object)
        // 缺失时返回协议错误。
        .ok_or_else(worker_protocol_error)?;
    // 截图候选 data 必须恰好包含九个字段。
    if data.len() != 9 {
        // 拒绝路径、原生目标或额外 provider 事实。
        return Err(worker_protocol_error());
    }
    // 验证全部布尔安全常量。
    for (field, expected) in [
        // worker 已完整写入父 Module staging。
        ("candidateWritten", true),
        // 截图不捕获指针。
        ("cursorCaptured", false),
        // worker 内前景保持不变。
        ("foregroundUnchanged", true),
        // 系统隐私指示器可能出现。
        ("privacyIndicatorMayHaveAppeared", true),
    ] {
        // 每个字段必须存在且精确匹配。
        if data.get(field).and_then(Value::as_bool) != Some(expected) {
            // 拒绝伪造或缺失安全证明。
            return Err(worker_protocol_error());
        }
    }
    // 读取并限制宽度。
    let width = bounded_u32(data.get("frameWidth"), 4_096)?;
    // 读取并限制高度。
    let height = bounded_u32(data.get("frameHeight"), 4_096)?;
    // 读取并限制 PNG 字节数。
    let bytes = data
        // 访问 pngBytes。
        .get("pngBytes")
        // 只接受无符号整数。
        .and_then(Value::as_u64)
        // 应用公开字节范围。
        .filter(|value| (1..=MAXIMUM_PNG_BYTES).contains(value))
        // 缺失或越界时失败。
        .ok_or_else(worker_protocol_error)?;
    // 验证封闭设备分类。
    let device_driver = match data.get("deviceDriver").and_then(Value::as_str) {
        // 接受硬件分类。
        Some("hardware") => "hardware",
        // 接受 WARP 分类。
        Some("warp") => "warp",
        // 拒绝 provider 私有设备名称。
        _ => return Err(worker_protocol_error()),
    };
    // 读取十六位小写像素摘要。
    let pixel_digest = data
        // 访问 pixelDigest。
        .get("pixelDigest")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 应用精确十六位语法。
        .filter(|value| {
            // 长度必须恰好十六。
            value.len() == 16
                // 每个字符必须是小写十六进制。
                && value
                    // 遍历 ASCII 字节。
                    .bytes()
                    // 只接受数字或 a..f。
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        // 协议漂移时失败。
        .ok_or_else(worker_protocol_error)?
        // 冻结为领域事实。
        .to_owned();
    // 返回严格验证后的截图候选。
    Ok(WorkerScreenshot {
        // 保存宽度。
        width,
        // 保存高度。
        height,
        // 保存字节数。
        bytes,
        // 保存设备分类。
        device_driver,
        // 保存像素摘要。
        pixel_digest,
    })
}

// 读取处于 1..=maximum 的 u32 字段。
fn bounded_u32(value: Option<&Value>, maximum: u32) -> AppResult<u32> {
    // 读取并无损转换无符号整数。
    let value = value
        // 只接受 JSON 无符号整数。
        .and_then(Value::as_u64)
        // 转换为 u32。
        .and_then(|value| u32::try_from(value).ok())
        // 缺失或溢出时失败。
        .ok_or_else(worker_protocol_error)?;
    // 应用正数与上限边界。
    if !(1..=maximum).contains(&value) {
        // 返回协议错误。
        return Err(worker_protocol_error());
    }
    // 返回有界整数。
    Ok(value)
}

// 把 worker 错误码限制在稳定白名单。
fn worker_error_code(code: &str) -> AppResult<WindowScreenshotErrorCode> {
    // 未知 provider 错误统一收敛为 Module 协议违规。
    WindowScreenshotErrorCode::from_worker(code).ok_or_else(worker_protocol_error)
}

// 构造统一 worker 协议错误。
fn worker_protocol_error() -> AppControlError {
    // 返回不含 worker 输出内容的安全诊断。
    WindowScreenshotErrorCode::WorkerProtocolViolation.error(
        // 不回显潜在原生或敏感字段。
        "The capture worker returned an invalid screenshot protocol envelope.",
    )
}

// 映射统一输出门禁错误。
fn output_guard_error(error: OutputGuardError) -> AppControlError {
    // 保持覆盖确认与路径错误分类稳定。
    match error {
        // 既有文件需要独立覆盖许可。
        OutputGuardError::ConfirmationRequired => {
            WindowScreenshotErrorCode::OverwriteConfirmationRequired.error(
                // 不公开路径。
                "Existing screenshot output requires overwrite confirmation.",
            )
        }
        // 无法安全检查或目标类型无效。
        OutputGuardError::InspectionFailed | OutputGuardError::InvalidTargetType => {
            // 返回稳定输出路径错误。
            WindowScreenshotErrorCode::InvalidOutputPath.error(
                // 不公开底层文件系统事实。
                "The screenshot output target is not a safe regular file path.",
            )
        }
    }
}

// 映射原子提交 Component 错误。
fn atomic_file_error(error: AtomicFileError) -> AppControlError {
    // 保持提交竞态与一般输出失败分类稳定。
    match error {
        // 目标在提交竞态中出现时重新要求覆盖许可。
        AtomicFileError::TargetExists => WindowScreenshotErrorCode::OverwriteConfirmationRequired
            .error(
                // 不公开路径。
                "Existing screenshot output requires overwrite confirmation.",
            ),
        // 无效目标保持路径错误。
        AtomicFileError::InvalidDestination => WindowScreenshotErrorCode::InvalidOutputPath.error(
            // 不公开底层路径或权限事实。
            "The screenshot output destination is invalid.",
        ),
        // 其余 staging、同步和提交失败统一为写入失败。
        AtomicFileError::StagingCreationFailed
        | AtomicFileError::InvalidStaging
        | AtomicFileError::SyncFailed
        | AtomicFileError::CommitFailed => WindowScreenshotErrorCode::ScreenshotWriteFailed.error(
            // 不公开系统 I/O 文本。
            "The screenshot could not be committed atomically.",
        ),
    }
}

// 把截图事实投影为公开 capability schema。
fn render(
    // 接收 canonical opaque 目标。
    session_id: &str,
    // 接收最终输出路径。
    path: &str,
    // 接收独立覆盖许可。
    overwrite: bool,
    // 接收严格 worker 事实。
    capture: &WorkerScreenshot,
    // 接收内部原子替换事实。
    replaced_existing: bool,
) -> Value {
    // 覆盖发生必须已经取得覆盖许可。
    debug_assert!(!replaced_existing || overwrite);
    // 返回恰好匹配 window-screenshot schema 的字段。
    json!({
        // 输出稳定 capability ID。
        "capability": capabilities::WINDOW_SCREENSHOT,
        // 只回显 canonical opaque 目标。
        "targetId": session_id,
        // 固定公开目标类别。
        "targetKind": "application-window",
        // 声明严格隔离 worker 域。
        "executionDomain": "isolated-worker",
        // 声明逐操作确认必需。
        "confirmationRequired": true,
        // 成功路径已经满足确认。
        "confirmationSatisfied": true,
        // 回显独立覆盖许可。
        "overwriteConfirmed": overwrite,
        // 回显最终输出路径。
        "path": path,
        // 输出 PNG 字节数。
        "bytes": capture.bytes,
        // 输出帧宽度。
        "width": capture.width,
        // 输出帧高度。
        "height": capture.height,
        // 输出封闭设备分类。
        "deviceDriver": capture.device_driver,
        // 输出原始 RGBA 像素摘要。
        "pixelDigest": capture.pixel_digest,
        // 声明同目录原子提交。
        "atomicOutput": true,
        // 声明不捕获鼠标指针。
        "cursorCaptured": false,
        // 声明系统捕获指示器可能出现。
        "systemCaptureIndicatorMayAppear": true,
        // 声明主进程与 worker 门禁均保持前景不变。
        "foregroundUnchanged": true,
    })
}

// 声明纯输入、协议与投影测试。
#[cfg(test)]
mod tests {
    // 导入父模块纯函数与事实类型。
    use super::{WorkerScreenshot, parse_input, parse_worker_envelope, render};
    // 导入 JSON 构造器。
    use serde_json::json;

    // 验证 provider 输入默认值与封闭字段。
    #[test]
    fn input_defaults_are_bounded() {
        // 解析最小合法输入。
        let Ok(input) = parse_input(&json!({ "path": "a.png" })) else {
            // 合法最小输入意外失败时立即终止测试。
            panic!("input should pass");
        };
        // 核对默认 deadline。
        assert_eq!(input.timeout_ms, 5_000);
        // 核对默认禁止覆盖。
        assert!(!input.overwrite);
        // 未知字段必须失败。
        assert!(parse_input(&json!({ "path": "a.png", "command": "x" })).is_err());
    }

    // 验证 worker 失败 envelope 只转发十三种封闭错误码。
    #[test]
    fn worker_failure_envelope_uses_closed_error_whitelist() {
        // 遍历既有 capture worker 错误白名单。
        for code in [
            // 允许失效目标。
            "STALE_SESSION",
            // 允许歧义目标。
            "AMBIGUOUS_TARGET",
            // 允许确认缺失。
            "CONFIRMATION_REQUIRED",
            // 允许不可捕获目标。
            "CAPTURE_TARGET_INELIGIBLE",
            // 允许捕获 runtime 缺失。
            "CAPTURE_UNAVAILABLE",
            // 允许捕获目标失败。
            "CAPTURE_TARGET_FAILED",
            // 允许捕获设备失败。
            "CAPTURE_DEVICE_FAILED",
            // 允许捕获启动失败。
            "CAPTURE_START_FAILED",
            // 允许截图超时。
            "CAPTURE_TIMEOUT",
            // 允许像素读取失败。
            "CAPTURE_READBACK_FAILED",
            // 允许截图写入失败。
            "SCREENSHOT_WRITE_FAILED",
            // 允许 staging 路径失效。
            "INVALID_OUTPUT_PATH",
            // 允许前景干扰。
            "HOST_INTERFERENCE_DETECTED",
        ] {
            // 构造精确失败 envelope。
            let envelope = json!({
                // 标记 worker 失败。
                "ok": false,
                // 固定协议版本。
                "contractVersion": "act/capture-worker/v1",
                // 传入当前白名单错误。
                "error": {
                    // 使用当前稳定错误码。
                    "code": code,
                    // 使用安全固定消息。
                    "message": "safe worker failure",
                },
            });
            // 白名单错误必须转发为同一个公开码。
            assert_eq!(
                // 解析并取得失败码。
                parse_worker_envelope(2, &envelope)
                    // 成功结果不满足测试前提。
                    .err()
                    // 失败必须存在。
                    .map(|error| error.code),
                // 期望逐字保持 worker 稳定码。
                Some(code)
            );
        }
        // 构造未知 provider 错误 envelope。
        let unknown = json!({
            // 标记 worker 失败。
            "ok": false,
            // 固定协议版本。
            "contractVersion": "act/capture-worker/v1",
            // 注入协议外错误码。
            "error": {
                // 未知码不得越过 Module 边界。
                "code": "UNKNOWN_PROVIDER_ERROR",
                // 未知 provider 消息不得被信任。
                "message": "private provider details",
            },
        });
        // 未知码必须收敛为不回显 provider 内容的协议违规。
        let error = parse_worker_envelope(2, &unknown)
            // 成功结果不满足测试前提。
            .err()
            // 未知码必须返回结构化错误。
            .unwrap_or_else(|| panic!("unknown worker code must fail closed"));
        // 核对稳定协议违规码。
        assert_eq!(error.code, "WORKER_PROTOCOL_VIOLATION");
        // 禁止回显未知 provider 消息。
        assert!(!error.message.contains("private provider details"));
    }

    // 验证严格 worker envelope。
    #[test]
    fn worker_envelope_requires_exact_candidate_proof() {
        // 构造固定合法 worker 结果。
        let envelope = json!({
            // 标记成功。
            "ok": true,
            // 固定协议版本。
            "contractVersion": "act/capture-worker/v1",
            // 提供精确九字段 data。
            "data": {
                // 提供宽度。
                "frameWidth": 640,
                // 提供高度。
                "frameHeight": 480,
                // 提供 PNG 字节数。
                "pngBytes": 1024,
                // 提供设备分类。
                "deviceDriver": "hardware",
                // 提供像素摘要。
                "pixelDigest": "0123456789abcdef",
                // 声明候选已写入。
                "candidateWritten": true,
                // 声明未捕获指针。
                "cursorCaptured": false,
                // 声明前景不变。
                "foregroundUnchanged": true,
                // 声明隐私指示器可能出现。
                "privacyIndicatorMayHaveAppeared": true,
            },
        });
        // 合法 envelope 必须通过。
        let Ok(capture) = parse_worker_envelope(0, &envelope) else {
            // 合法 worker envelope 意外失败时立即终止测试。
            panic!("worker envelope should pass");
        };
        // 核对封闭事实。
        assert_eq!(capture.width, 640);
        // 核对像素摘要。
        assert_eq!(capture.pixel_digest, "0123456789abcdef");
    }

    // 验证公开结果精确匹配截图 schema 必需字段。
    #[test]
    fn render_exposes_no_native_or_staging_facts() {
        // 构造封闭截图事实。
        let capture = WorkerScreenshot {
            // 设置宽度。
            width: 800,
            // 设置高度。
            height: 600,
            // 设置 PNG 字节数。
            bytes: 2048,
            // 设置设备分类。
            device_driver: "warp",
            // 设置像素摘要。
            pixel_digest: "fedcba9876543210".to_owned(),
        };
        // 投影公开结果。
        let value = render(
            // 使用 canonical 目标。
            "s2:w:0123456789abcdef",
            // 使用固定路径。
            "build/test.png",
            // 禁止覆盖。
            false,
            // 传递截图事实。
            &capture,
            // 声明未替换。
            false,
        );
        // 核对 capability。
        assert_eq!(value["capability"], "window.screenshot@1");
        // 核对原子输出。
        assert_eq!(value["atomicOutput"], true);
        // 序列化后禁止出现原生与 staging 字段。
        let text = value.to_string();
        // 禁止 HWND。
        assert!(!text.contains("hwnd"));
        // 禁止 staging 路径字段。
        assert!(!text.contains("staging"));
    }
}
