//! 精确窗口首帧元数据探针领域 Module。

// 导入有界 worker deadline。
use std::time::Duration;

// 导入 JSON 构造与验证类型。
use serde_json::{Value, json};

// 导入窗口门禁、隔离进程与领域契约。
use crate::{
    // 导入私有窗口事实边界。
    adapters::{
        // 导入窗口枚举、唯一解析与前景门禁。
        window::{capture_visible_titled_windows, ensure_foreground_unchanged, resolve_window},
        // 导入零帧 eligibility 预检。
        window_capture_preflight_windows,
        // 导入前景只读快照。
        windows::foreground_hwnd,
    },
    // 导入稳定 capability ID。
    capabilities,
    // 导入窄 Component。
    components::{
        // 导入进程级取消状态。
        cancellation,
        // 导入 canonical opaque 解析器。
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
        // 导入 Job 约束 companion worker 执行器。
        worker_process,
    },
    // 导入语言中立错误与结果。
    domain::{AppControlError, AppResult},
};

// 固定 companion worker 文件名。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-capture-worker.exe";
// 固定 worker 协议版本。
const WORKER_CONTRACT_VERSION: &str = "act/capture-worker/v1";
// 固定 worker operation。
const WORKER_OPERATION: &str = "window-frame-metadata";
// 限制 worker stdout 为 64KiB。
const MAXIMUM_WORKER_OUTPUT_BYTES: usize = 64 * 1024;

// 表示首帧元数据探针 Module 允许产生或转发的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameProbeErrorCode {
    // 表示 opaque 目标无法唯一解析。
    AmbiguousTarget,
    // 表示捕获设备创建或使用失败。
    CaptureDeviceFailed,
    // 表示首帧元数据读取或关闭失败。
    CaptureFrameMetadataFailed,
    // 表示捕获会话或线程启动失败。
    CaptureStartFailed,
    // 表示 WGC 无法绑定精确目标。
    CaptureTargetFailed,
    // 表示零帧预检拒绝目标。
    CaptureTargetIneligible,
    // 表示首帧等待超过 deadline。
    CaptureTimeout,
    // 表示系统捕获 runtime 不可用。
    CaptureUnavailable,
    // 表示调用方没有逐操作确认。
    ConfirmationRequired,
    // 表示主进程或 worker 观察到前景变化。
    HostInterferenceDetected,
    // 表示请求参数不满足公开契约。
    InvalidArgument,
    // 表示 opaque 目标已失效或身份改变。
    StaleSession,
    // 表示 worker envelope 违反封闭协议。
    WorkerProtocolViolation,
}

// 提供 Module 私有错误码与公开协议文本的唯一映射。
impl FrameProbeErrorCode {
    // 固定 capture worker 失败 envelope 允许转发的十一种类别。
    const WORKER_ALLOWED: [Self; 11] = [
        // 允许失效目标。
        Self::StaleSession,
        // 允许歧义目标。
        Self::AmbiguousTarget,
        // 允许确认缺失。
        Self::ConfirmationRequired,
        // 允许不可捕获目标。
        Self::CaptureTargetIneligible,
        // 允许捕获 runtime 缺失。
        Self::CaptureUnavailable,
        // 允许捕获目标失败。
        Self::CaptureTargetFailed,
        // 允许捕获设备失败。
        Self::CaptureDeviceFailed,
        // 允许捕获启动失败。
        Self::CaptureStartFailed,
        // 允许首帧超时。
        Self::CaptureTimeout,
        // 允许首帧元数据失败。
        Self::CaptureFrameMetadataFailed,
        // 允许前景干扰。
        Self::HostInterferenceDetected,
    ];

    // 返回版本化公开错误码文本。
    const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射歧义目标。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射捕获设备失败。
            Self::CaptureDeviceFailed => "CAPTURE_DEVICE_FAILED",
            // 映射首帧元数据失败。
            Self::CaptureFrameMetadataFailed => "CAPTURE_FRAME_METADATA_FAILED",
            // 映射捕获启动失败。
            Self::CaptureStartFailed => "CAPTURE_START_FAILED",
            // 映射捕获目标失败。
            Self::CaptureTargetFailed => "CAPTURE_TARGET_FAILED",
            // 映射不可捕获目标。
            Self::CaptureTargetIneligible => "CAPTURE_TARGET_INELIGIBLE",
            // 映射捕获超时。
            Self::CaptureTimeout => "CAPTURE_TIMEOUT",
            // 映射捕获 runtime 不可用。
            Self::CaptureUnavailable => "CAPTURE_UNAVAILABLE",
            // 映射确认缺失。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 映射前景干扰。
            Self::HostInterferenceDetected => "HOST_INTERFERENCE_DETECTED",
            // 映射参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射失效目标。
            Self::StaleSession => "STALE_SESSION",
            // 映射 worker 协议违规。
            Self::WorkerProtocolViolation => "WORKER_PROTOCOL_VIOLATION",
        }
    }

    // 从 capture worker 失败 envelope 解析允许转发的封闭类别。
    fn from_worker(code: &str) -> Option<Self> {
        // 只从封闭类型白名单中查找逐字匹配项。
        Self::WORKER_ALLOWED
            // 按值遍历复制型私有枚举。
            .into_iter()
            // 公共字符串只由 as_str 唯一映射。
            .find(|candidate| candidate.as_str() == code)
    }

    // 使用当前封闭错误码构造产品级公开错误。
    fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Module 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }
}

// 保存经过严格协议验证的 worker 帧元数据。
#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkerFrame {
    // 保存正宽度。
    width: u32,
    // 保存正高度。
    height: u32,
    // 保存封闭驱动分类。
    device_driver: &'static str,
}

// 对 canonical 窗口目标执行确认式隔离首帧元数据探针。
pub(crate) fn probe(
    // 接收 opaque 窗口目标。
    session_id: &str,
    // 接收逐操作显式确认。
    confirmed: bool,
    // 接收 worker deadline。
    timeout_ms: u32,
) -> AppResult<Value> {
    // 确认必须在目标解析、窗口枚举或 worker 定位前完成。
    if !confirmed {
        // 缺少确认时立即失败。
        return Err(FrameProbeErrorCode::ConfirmationRequired.error(
            // 不回显目标。
            "Frame metadata capture requires explicit confirmation.",
        ));
    }
    // 验证 worker deadline 封闭范围。
    if !(1..=30_000).contains(&timeout_ms) {
        // 拒绝零或无界等待。
        return Err(FrameProbeErrorCode::InvalidArgument.error(
            // 说明允许范围。
            "Capture worker timeout must be 1..30000ms.",
        ));
    }
    // 严格解析 canonical opaque 目标。
    let target = OpaqueTargetId::parse(session_id);
    // 只允许第二代窗口目标。
    if target.map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        // 拒绝 native、旧版本和其他目标类别。
        return Err(FrameProbeErrorCode::InvalidArgument.error(
            // 不公开任何原生事实。
            "Frame metadata capture requires a canonical s2:w target.",
        ));
    }
    // 记录主进程预检前的前景窗口。
    let foreground_before = foreground_hwnd();
    // 每次使用时重新枚举当前窗口快照。
    let windows = capture_visible_titled_windows()?;
    // 在主进程内唯一重新解析 opaque 目标。
    let window = resolve_window(session_id, &windows)?;
    // 在启动 worker 前执行零帧 eligibility 预检。
    let preflight = window_capture_preflight_windows::inspect(window.hwnd)?;
    // 不可捕获目标禁止启动 worker。
    if preflight.eligibility != "eligible-for-certified-capture-route" {
        // 失败闭合且不尝试激活窗口。
        return Err(FrameProbeErrorCode::CaptureTargetIneligible.error(
            // 只公开封闭 eligibility 分类。
            format!("Capture target is ineligible: {}.", preflight.eligibility),
        ));
    }
    // 定位固定 sibling worker。
    let worker = worker_process::sibling_companion_path(
        // 传入固定文件名。
        WORKER_FILE_NAME,
        // 传入安全描述。
        "capture worker",
    )?;
    // 构造版本化 worker 请求。
    let request = json!({
        // 固定内部协议版本。
        "contractVersion": WORKER_CONTRACT_VERSION,
        // 固定唯一 operation。
        "operation": WORKER_OPERATION,
        // 只传递 opaque 目标。
        "sessionId": session_id,
        // 重复传递确认供 worker 独立核对。
        "confirmed": true,
        // 传递有界 deadline。
        "timeoutMs": timeout_ms,
    });
    // 运行结果先保存，确保错误路径同样执行前景核验。
    let worker_result = worker_process::run_companion(
        // 使用精确 sibling 路径。
        &worker,
        // worker 不接受命令行参数。
        &[],
        // 通过 stdin 传递严格请求。
        &request,
        // 让进程 Job 使用调用方 deadline。
        Duration::from_millis(u64::from(timeout_ms)),
        // 限制 stdout。
        MAXIMUM_WORKER_OUTPUT_BYTES,
        // 轮询统一取消状态。
        cancellation::is_cancelled,
    );
    // 记录 worker 完成或回收后的前景窗口。
    let foreground_after = foreground_hwnd();
    // 前景变化优先于 worker 结果并失败闭合。
    ensure_foreground_unchanged(foreground_before, foreground_after)?;
    // 传播 worker 进程级失败。
    let worker_output = worker_result?;
    // 严格解析 worker 协议 envelope。
    let frame = parse_worker_envelope(worker_output.exit_code, &worker_output.envelope)?;
    // 投影稳定公开 control envelope。
    Ok(render(session_id, timeout_ms, &frame))
}

// 严格解析 worker 成功或失败 envelope。
fn parse_worker_envelope(exit_code: u32, envelope: &Value) -> AppResult<WorkerFrame> {
    // 要求顶层对象。
    let object = envelope.as_object().ok_or_else(worker_protocol_error)?;
    // 验证固定协议版本。
    if object.get("contractVersion").and_then(Value::as_str) != Some(WORKER_CONTRACT_VERSION) {
        // 未知版本按协议违规处理。
        return Err(worker_protocol_error());
    }
    // 读取布尔成功标记。
    let ok = object
        // 读取 ok 字段。
        .get("ok")
        // 要求布尔值。
        .and_then(Value::as_bool)
        // 缺失时失败。
        .ok_or_else(worker_protocol_error)?;
    // 失败 envelope 只能配合退出码二。
    if !ok {
        // 校验顶层精确字段与进程码。
        if exit_code != 2 || object.len() != 3 || !object.contains_key("error") {
            // 拒绝不一致协议。
            return Err(worker_protocol_error());
        }
        // 要求严格 error 对象。
        let error = object
            // 读取 error。
            .get("error")
            // 要求对象。
            .and_then(Value::as_object)
            // 缺失时失败。
            .ok_or_else(worker_protocol_error)?;
        // error 只允许 code 与 message。
        if error.len() != 2 {
            // 拒绝额外 provider 事实。
            return Err(worker_protocol_error());
        }
        // 读取稳定错误码。
        let code = error
            // 读取 code。
            .get("code")
            // 要求字符串。
            .and_then(Value::as_str)
            // 缺失时失败。
            .ok_or_else(worker_protocol_error)?;
        // 读取安全错误消息。
        let message = error
            // 读取 message。
            .get("message")
            // 要求字符串。
            .and_then(Value::as_str)
            // 缺失时失败。
            .ok_or_else(worker_protocol_error)?;
        // 只转发允许的稳定错误类别。
        return Err(worker_error_code(code)?.error(message));
    }
    // 成功 envelope 必须配合退出码零和精确字段。
    if exit_code != 0 || object.len() != 3 || !object.contains_key("data") {
        // 拒绝不一致协议。
        return Err(worker_protocol_error());
    }
    // 要求严格 data 对象。
    let data = object
        // 读取 data。
        .get("data")
        // 要求对象。
        .and_then(Value::as_object)
        // 缺失时失败。
        .ok_or_else(worker_protocol_error)?;
    // data 必须恰好包含八个封闭字段。
    if data.len() != 8 {
        // 拒绝原生或协议外字段。
        return Err(worker_protocol_error());
    }
    // 验证所有安全常量。
    for (field, expected) in [
        // frame surface 未访问。
        ("frameSurfaceAccessed", false),
        // 像素未持久化。
        ("pixelsPersisted", false),
        // 文件未写入。
        ("fileWritten", false),
        // worker 前景保持不变。
        ("foregroundUnchanged", true),
        // 系统隐私指示器可能出现。
        ("privacyIndicatorMayHaveAppeared", true),
    ] {
        // 每个字段必须存在且值精确匹配。
        if data.get(field).and_then(Value::as_bool) != Some(expected) {
            // 拒绝伪造或缺失安全证明。
            return Err(worker_protocol_error());
        }
    }
    // 读取正宽度。
    let width = positive_u32(data.get("frameWidth"))?;
    // 读取正高度。
    let height = positive_u32(data.get("frameHeight"))?;
    // 验证封闭驱动分类。
    let device_driver = match data.get("deviceDriver").and_then(Value::as_str) {
        // 接受硬件分类。
        Some("hardware") => "hardware",
        // 接受 WARP 分类。
        Some("warp") => "warp",
        // 拒绝 provider 私有驱动名称。
        _ => return Err(worker_protocol_error()),
    };
    // 返回严格验证后的封闭帧事实。
    Ok(WorkerFrame {
        // 保存宽度。
        width,
        // 保存高度。
        height,
        // 保存驱动分类。
        device_driver,
    })
}

// 从 JSON 字段读取正 u32。
fn positive_u32(value: Option<&Value>) -> AppResult<u32> {
    // 读取无符号整数并执行无损转换。
    let value = value
        // 要求 JSON 无符号整数。
        .and_then(Value::as_u64)
        // 转换到 u32。
        .and_then(|value| u32::try_from(value).ok())
        // 缺失或溢出时失败。
        .ok_or_else(worker_protocol_error)?;
    // schema 要求尺寸为正。
    if value == 0 {
        // 拒绝零尺寸。
        return Err(worker_protocol_error());
    }
    // 返回正尺寸。
    Ok(value)
}

// 把 worker 错误码限制在稳定白名单。
fn worker_error_code(code: &str) -> AppResult<FrameProbeErrorCode> {
    // 未知 provider 错误统一收敛为 Module 协议违规。
    FrameProbeErrorCode::from_worker(code).ok_or_else(worker_protocol_error)
}

// 构造统一 worker 协议错误。
fn worker_protocol_error() -> AppControlError {
    // 返回不含 worker 输出内容的安全诊断。
    FrameProbeErrorCode::WorkerProtocolViolation.error(
        // 不回显潜在原生或敏感字段。
        "The capture worker returned an invalid protocol envelope.",
    )
}

// 把封闭帧事实投影为公开 capability schema。
fn render(session_id: &str, timeout_ms: u32, frame: &WorkerFrame) -> Value {
    // 返回稳定 control envelope。
    json!({
        // 标记成功。
        "ok": true,
        // 固定公开控制协议版本。
        "contractVersion": "act/control/v1",
        // 声明 Rust 实现。
        "implementation": "rust",
        // 输出首帧探针数据。
        "data": {
            // 输出稳定 capability ID。
            "capability": capabilities::WINDOW_CAPTURE_FRAME_PROBE,
            // 仅回显 canonical opaque 目标。
            "targetId": session_id,
            // 固定窗口目标类别。
            "targetKind": "application-window",
            // 声明只读。
            "readOnly": true,
            // 声明必须确认。
            "confirmationRequired": true,
            // 成功路径已满足确认。
            "confirmationSatisfied": true,
            // 声明严格隔离域。
            "executionDomain": "isolated-worker",
            // 后台捕获不需要前景。
            "foregroundRequired": false,
            // 主进程与 worker 均通过前景门禁。
            "foregroundUnchanged": true,
            // 输出封闭帧元数据。
            "frame": {
                // 输出正宽度。
                "width": frame.width,
                // 输出正高度。
                "height": frame.height,
                // 输出硬件或 WARP 分类。
                "deviceDriver": frame.device_driver,
            },
            // 输出可机器核验的安全证明。
            "safety": {
                // 已取得首帧对象。
                "frameAcquired": true,
                // 未访问 frame surface。
                "frameSurfaceAccessed": false,
                // 未持久化像素。
                "pixelsPersisted": false,
                // 未写文件。
                "fileWritten": false,
                // 未激活窗口。
                "windowActivated": false,
                // 未发送输入。
                "inputSent": false,
                // 未公开原生目标。
                "nativeTargetExposed": false,
                // 系统隐私指示器可能出现。
                "privacyIndicatorMayHaveAppeared": true,
                // 输出实际 worker deadline。
                "workerTimeoutMs": timeout_ms,
                // 声明统一取消能力。
                "workerCancellable": true,
            },
        },
    })
}

// 声明纯协议与投影测试。
#[cfg(test)]
mod tests {
    // 导入父模块纯函数与类型。
    use super::{FrameProbeErrorCode, WorkerFrame, parse_worker_envelope, render};
    // 导入 JSON 构造器。
    use serde_json::json;

    // 验证 Module 私有封闭类型逐字保持十三个公开错误码。
    #[test]
    fn frame_probe_error_codes_preserve_public_contract() {
        // 按 Module 领域语义顺序收集全部封闭变体。
        let actual = [
            // opaque 目标歧义。
            FrameProbeErrorCode::AmbiguousTarget.as_str(),
            // 捕获设备失败。
            FrameProbeErrorCode::CaptureDeviceFailed.as_str(),
            // 首帧元数据失败。
            FrameProbeErrorCode::CaptureFrameMetadataFailed.as_str(),
            // 捕获启动失败。
            FrameProbeErrorCode::CaptureStartFailed.as_str(),
            // 捕获目标失败。
            FrameProbeErrorCode::CaptureTargetFailed.as_str(),
            // 零帧预检拒绝。
            FrameProbeErrorCode::CaptureTargetIneligible.as_str(),
            // 首帧等待超时。
            FrameProbeErrorCode::CaptureTimeout.as_str(),
            // 捕获 runtime 不可用。
            FrameProbeErrorCode::CaptureUnavailable.as_str(),
            // 逐操作确认缺失。
            FrameProbeErrorCode::ConfirmationRequired.as_str(),
            // 前景干扰。
            FrameProbeErrorCode::HostInterferenceDetected.as_str(),
            // 参数拒绝。
            FrameProbeErrorCode::InvalidArgument.as_str(),
            // opaque 目标失效。
            FrameProbeErrorCode::StaleSession.as_str(),
            // worker 协议违规。
            FrameProbeErrorCode::WorkerProtocolViolation.as_str(),
        ];
        // 公开文本必须与既有 error envelope 逐字一致。
        assert_eq!(
            actual,
            // 固定预期顺序使缺失、重复或误改都可诊断。
            [
                // 保持目标歧义码。
                "AMBIGUOUS_TARGET",
                // 保持捕获设备失败码。
                "CAPTURE_DEVICE_FAILED",
                // 保持首帧元数据失败码。
                "CAPTURE_FRAME_METADATA_FAILED",
                // 保持捕获启动失败码。
                "CAPTURE_START_FAILED",
                // 保持捕获目标失败码。
                "CAPTURE_TARGET_FAILED",
                // 保持不可捕获目标码。
                "CAPTURE_TARGET_INELIGIBLE",
                // 保持首帧超时码。
                "CAPTURE_TIMEOUT",
                // 保持捕获 runtime 不可用码。
                "CAPTURE_UNAVAILABLE",
                // 保持确认缺失码。
                "CONFIRMATION_REQUIRED",
                // 保持前景干扰码。
                "HOST_INTERFERENCE_DETECTED",
                // 保持参数拒绝码。
                "INVALID_ARGUMENT",
                // 保持目标失效码。
                "STALE_SESSION",
                // 保持 worker 协议违规码。
                "WORKER_PROTOCOL_VIOLATION",
            ]
        );
    }

    // 验证 worker 失败 envelope 只转发十一种封闭错误码。
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
            // 允许首帧超时。
            "CAPTURE_TIMEOUT",
            // 允许首帧元数据失败。
            "CAPTURE_FRAME_METADATA_FAILED",
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

    // 验证成功 worker envelope 的严格解析。
    #[test]
    fn worker_envelope_requires_exact_safety_proof() {
        // 构造合法 worker envelope。
        let envelope = json!({
            // 标记成功。
            "ok": true,
            // 固定协议版本。
            "contractVersion": "act/capture-worker/v1",
            // 提供精确八字段 data。
            "data": {
                // 提供宽度。
                "frameWidth": 640,
                // 提供高度。
                "frameHeight": 480,
                // 提供驱动分类。
                "deviceDriver": "hardware",
                // 声明未访问 surface。
                "frameSurfaceAccessed": false,
                // 声明未持久化像素。
                "pixelsPersisted": false,
                // 声明未写文件。
                "fileWritten": false,
                // 声明前景不变。
                "foregroundUnchanged": true,
                // 声明隐私指示器可能出现。
                "privacyIndicatorMayHaveAppeared": true,
            },
        });
        // 合法 envelope 必须通过。
        let Ok(frame) = parse_worker_envelope(0, &envelope) else {
            // 合法 envelope 意外失败时立即失败。
            panic!("valid worker envelope must pass");
        };
        // 核对封闭元数据。
        assert_eq!(
            // 比较解析后的事实。
            frame,
            // 构造期望事实。
            WorkerFrame {
                // 期望宽度。
                width: 640,
                // 期望高度。
                height: 480,
                // 期望驱动分类。
                device_driver: "hardware",
            }
        );
    }

    // 验证公开投影不含原生目标或像素事实。
    #[test]
    fn render_matches_public_probe_contract() {
        // 构造封闭帧事实。
        let frame = WorkerFrame {
            // 设置宽度。
            width: 800,
            // 设置高度。
            height: 600,
            // 设置 WARP 分类。
            device_driver: "warp",
        };
        // 投影公开结果。
        let value = render("s2:w:0123456789abcdef", 5_000, &frame);
        // 核对 capability。
        assert_eq!(value["data"]["capability"], "window.capture.frame.probe@1");
        // 核对实现。
        assert_eq!(value["implementation"], "rust");
        // 核对 surface 未访问。
        assert_eq!(value["data"]["safety"]["frameSurfaceAccessed"], false);
        // 序列化后禁止出现原生字段名。
        let text = value.to_string();
        // 禁止 HWND。
        assert!(!text.contains("hwnd"));
        // 禁止 native process ID。
        assert!(!text.contains("nativeProcessId"));
        // 禁止文件路径。
        assert!(!text.contains("path"));
    }
}
