use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use serde_json::{Value, json};
use windows::Win32::{
    Foundation::HWND,
    UI::{
        Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
            MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT, SendInput, VIRTUAL_KEY, VK_BACK,
            VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F4, VK_HOME, VK_LEFT, VK_MENU,
            VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
        },
        WindowsAndMessaging::{
            IsWindowVisible, SW_RESTORE, SetCursorPos, SetForegroundWindow, ShowWindowAsync,
        },
    },
};

// 引入 Desktop Adapter 私有封闭错误码实现。
#[path = "desktop_error.rs"]
mod error_code;

// 导入 Desktop Adapter 私有封闭错误码。
use error_code::DesktopAdapterErrorCode;

use crate::{
    adapters::{
        AppAdapter,
        // 公共结果只通过安全窗口投影与布尔前景证据。
        window::{ensure_foreground_unchanged, foreground_snapshot, public_window_observation},
        window_capture::capture_window_png,
        windows::{
            // legacy 目标仅在私有重新发现阶段使用。
            WindowRecord,
            filter_windows,
            foreground_hwnd,
            opaque_control_session_id,
            set_standard_edit_text,
            standard_edit_children,
            unique_window,
        },
    },
    // 只读取项目自有 Media Foundation 编码可达事实。
    components::media_foundation_encoder::MediaFoundationEncoder,
    // 导入请求与统一结果类型。
    domain::{AppResult, CommandRequest},
    // opaque 写入、截图与录制兼容路线只通过正式领域 Module。
    modules::{standard_edit, window_close, window_record, window_screenshot},
    policy,
};

pub struct DesktopAdapter;

impl AppAdapter for DesktopAdapter {
    fn app_id(&self) -> &'static str {
        "desktop"
    }

    fn status(&self) -> AppResult<Value> {
        Ok(json!({
            "ok": true,
            "app": self.app_id(),
            "controlPolicy": "background preferred; foreground requires explicit consent",
            "backgroundAttempts": ["Windows Graphics Capture screenshot/recording", "standard Edit WM_SETTEXT", "exact-window WM_CLOSE"],
            "foregroundOperations": ["restore-and-activate", "type-text", "press-key", "click"],
            "launch": "direct executable start; no shell or focus API",
            "videoRecording": {
                // 只公开系统内建编码链路的布尔可达事实。
                "available": MediaFoundationEncoder::is_h264_available().unwrap_or(false),
                "capture": "Windows.Graphics.Capture exact window",
                "encoder": "Windows Media Foundation H.264",
                "defaults": { "fps": 2, "maxWidth": 960, "quality": 75, "maxKeyframes": 8 },
                "aiAnalysis": "storyboard-first temporal-difference keyframes"
            },
        }))
    }

    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        // 记录只读发现前的私有前景句柄。
        let before = foreground_hwnd();
        // 兼容解析旧输入过滤器，但不序列化私有记录。
        let mut sessions = filter_windows(&request.target)?;
        // 诊断 surface 只发布可见且有标题的顶层窗口。
        sessions.retain(|window| window.visible && !window.title.is_empty());
        // 保存应用公开边界前的总数。
        let total = sessions.len();
        // 应用调用方有界数量。
        sessions.truncate(request.max_items);
        // 把每项投影为 canonical s2:w 与安全语义字段。
        let sessions = sessions
            // 遍历私有当前快照。
            .iter()
            // 禁止 WindowRecord 自动序列化。
            .map(public_window_observation)
            // 收集安全公开结果。
            .collect::<Vec<_>>();
        // 记录发现后的私有前景句柄。
        let after = foreground_hwnd();
        // 只读发现不得改变前景。
        ensure_foreground_unchanged(before, after)?;
        // 返回兼容顶层形状与安全 sessions。
        Ok(json!({
            // 标记成功。
            "ok": true,
            // 保持 direct desktop surface。
            "app": self.app_id(),
            // 标记无副作用读取。
            "readOnly": true,
            // 输出本次返回数。
            "count": sessions.len(),
            // 输出完整安全候选数。
            "total": total,
            // 标记数量边界是否截断。
            "truncated": total > sessions.len(),
            // 输出 canonical 安全窗口。
            "sessions": sessions,
            // 只输出布尔前景证据。
            "foreground": foreground_snapshot(before, after),
        }))
    }

    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        // 记录只读检查前的私有前景句柄。
        let before = foreground_hwnd();
        // 输入兼容路线可接受旧目标，当前使用时仍要求唯一命中。
        let window = unique_window(&request.target)?;
        // 诊断面不把不可见或无标题内部窗口提升为公共目标。
        if !window.visible || window.title.is_empty() {
            // 返回稳定不可用结果。
            return Err(DesktopAdapterErrorCode::TargetNotFound.error(
                // 使用目标缺失码。
                "The desktop diagnostic target is not a visible titled top-level window.",
            ));
        }
        // 只投影 canonical 窗口元数据，不公开子控件 provider 记录。
        let window = public_window_observation(&window);
        // 记录检查后的私有前景句柄。
        let after = foreground_hwnd();
        // 只读检查不得改变前景。
        ensure_foreground_unchanged(before, after)?;
        // 返回安全精确诊断。
        Ok(json!({
            // 标记成功。
            "ok": true,
            // 保持 direct desktop surface。
            "app": self.app_id(),
            // 标记无副作用读取。
            "readOnly": true,
            // 输出安全窗口事实。
            "window": window,
            // 只输出布尔前景证据。
            "foreground": foreground_snapshot(before, after),
        }))
    }

    fn run(&self, request: &CommandRequest) -> AppResult<Value> {
        match request.operation.as_deref() {
            Some("screenshot") => screenshot(request),
            Some("record") => record(request),
            Some("close") => close_window(request),
            Some("type-text") => type_text(request),
            Some("press-key") => press_key(request),
            Some("click") => click(request),
            Some("launch") => launch(request),
            // 未认证 operation 由 Adapter 私有封闭码拒绝。
            Some(operation) => Err(
                DesktopAdapterErrorCode::BackgroundOperationUnavailable.error(
                    // 只回显请求的 operation 名称。
                    format!("desktop.{operation} 未认证为控制操作。"),
                ),
            ),
            // 缺失 operation 保持稳定参数错误。
            None => Err(DesktopAdapterErrorCode::InvalidArgument.error(
                // 保持既有兼容消息。
                "run 缺少 operation。",
            )),
        }
    }
}

// 把私有标准 Edit 记录投影为精确 opaque 控件目标。
fn public_standard_edit_target(control: &WindowRecord) -> Value {
    // 只返回 provider-neutral 语义字段。
    json!({
        // 从当前私有事实生成 canonical s2:c。
        "sessionId": opaque_control_session_id(control),
        // 标记通用控件类别。
        "kind": "control",
        // 标记已认证的标准 Edit 类型。
        "targetKind": "standard-edit-control",
        // 只公开进程文件名提示。
        "applicationName": control.process_name.as_deref().unwrap_or(""),
        // 公开当前可见性事实。
        "visible": control.visible,
    })
}

fn record(request: &CommandRequest) -> AppResult<Value> {
    // confirmation 必须先于 target、path 和任何文件系统访问。
    if !request.confirmed {
        // 返回稳定确认错误。
        return Err(DesktopAdapterErrorCode::ConfirmationRequired.error(
            // 使用确认错误码。
            "Exact window recording requires explicit confirmation.",
        ));
    }
    // legacy surface 也只接受 canonical opaque 窗口目标。
    let session_id = required_string(&request.target, "sessionId")?;
    // 把 legacy 参数对象投影为 provider-neutral input。
    let input = Value::Object(request.args.clone());
    // 委托正式 Rust Module 完成隔离编码与多产物事务。
    let mut result = window_record::record(session_id, request.confirmed, &input)?;
    // 成功结果必须保持对象形状。
    let object = result.as_object_mut().ok_or_else(|| {
        // 内部投影失效时返回稳定错误。
        DesktopAdapterErrorCode::OperationFailed.error(
            // 保持既有内部投影失败消息。
            "The recording module returned invalid data.",
        )
    })?;
    // 为 legacy desktop surface 补充应用 ID。
    object.insert("app".to_owned(), Value::String("desktop".to_owned()));
    // 为 legacy desktop surface 补充 operation。
    object.insert("operation".to_owned(), Value::String("record".to_owned()));
    // 返回不含原生标识的兼容形状。
    Ok(result)
}

fn screenshot(request: &CommandRequest) -> AppResult<Value> {
    // confirmation 必须先于 target、path 和任何文件系统访问。
    if !request.confirmed {
        // 返回稳定确认错误。
        return Err(DesktopAdapterErrorCode::ConfirmationRequired.error(
            // 使用确认错误码。
            "Exact window screenshot requires explicit confirmation.",
        ));
    }
    // 读取调用方 session 字符串。
    let session_id = required_string(&request.target, "sessionId")?;
    // canonical opaque 窗口调用方固定进入正式 Rust Module。
    if session_id.starts_with("s2:w:") {
        // 把 legacy CLI 参数对象投影为 provider-neutral input。
        let input = Value::Object(request.args.clone());
        // 委托隔离 worker 与原子提交边界。
        return window_screenshot::screenshot(session_id, request.confirmed, &input);
    }
    let window = unique_window(&request.target)?;
    let path = PathBuf::from(required_string(&request.args, "path")?);
    let timeout_ms = request
        .args
        .get("timeoutMs")
        .and_then(Value::as_u64)
        .unwrap_or(5_000);
    let before = foreground_hwnd();
    let capture = capture_window_png(window.hwnd, &path, Duration::from_millis(timeout_ms))?;
    let after = foreground_hwnd();
    if before != after {
        // 使用同一私有类型构造带安全详情的前景变化错误。
        return Err(DesktopAdapterErrorCode::ForegroundChanged.with_details(
            // 保持既有公开消息。
            "后台窗口截图期间前景窗口发生变化；结果未认证为后台不干扰证据。",
            // 只返回调用方产物路径与布尔前景事实。
            json!({ "path": capture.path, "foregroundUnchanged": false }),
        ));
    }
    Ok(json!({
        "ok": true,
        "app": "desktop",
        "operation": "screenshot",
        "executionMode": "background-windows-graphics-capture",
        "captureMethod": "Windows.Graphics.Capture",
        // 禁止输出私有 WindowRecord。
        "target": public_window_observation(&window),
        "path": capture.path,
        "bytes": capture.bytes,
        "width": capture.width,
        "height": capture.height,
        "deviceDriver": capture.device_driver,
        "cursorCaptured": false,
        "systemCaptureIndicatorMayAppear": true,
        "foreground": foreground_snapshot(before, after),
    }))
}

fn close_window(request: &CommandRequest) -> AppResult<Value> {
    // confirmation 必须先于目标解析和任何关闭请求。
    if !request.confirmed {
        // 返回统一确认错误。
        return Err(DesktopAdapterErrorCode::ConfirmationRequired.error(
            // 使用稳定确认错误码。
            "Exact window close requires confirmation.",
        ));
    }
    // 读取可选 sessionId 以识别正式 opaque 窗口路线。
    let session_id = request
        // 访问目标对象。
        .target
        // 读取固定字段。
        .get("sessionId")
        // 只接受字符串。
        .and_then(Value::as_str);
    // canonical s2:w 必须进入正式 Window Close Module。
    if session_id.is_some_and(|value| value.starts_with("s2:w:")) {
        // 把 direct desktop args 作为封闭 timeout 输入验证。
        let timeout_ms = window_close::provider_input(&Value::Object(request.args.clone()))?;
        // 取得已经识别为 s2:w 的目标。
        let opaque_target = session_id.ok_or_else(|| {
            // 该分支理论上不可达，仍保持结构化失败。
            DesktopAdapterErrorCode::InvalidArgument.error("target.sessionId is required.")
        })?;
        // 执行唯一目标固定关闭。
        let result = window_close::close(
            // 传入 opaque 目标。
            opaque_target,
            // 传入逐操作确认。
            request.confirmed,
            // 传入有界 deadline。
            timeout_ms,
        )?;
        // 返回 legacy desktop mapper 的安全兼容形状。
        return Ok(json!({
            // 标记命令成功。
            "ok": true,
            // 保持 direct surface ID。
            "app": "desktop",
            // 保持旧操作名。
            "operation": "close",
            // 声明固定后台消息路线。
            "executionMode": "background-wm-close",
            // 只回显调用方 opaque 目标。
            "targetId": opaque_target,
            // 返回同一窗口身份已经失效。
            "closed": result["closed"],
            // 返回稳定前景证据。
            "foreground": { "unchanged": result["foregroundUnchanged"] },
            // 声明 provider-neutral 兼容形状。
            "compatibilityShape": "provider-neutral-window-close-v1",
        }));
    }
    // 旧 native window session 仅保留当前 Rust direct 兼容路线。
    let window = unique_window(&request.target)?;
    // 恢复旧兼容窗口句柄。
    let target = HWND(window.hwnd as *mut std::ffi::c_void);
    // 记录写前前景句柄。
    let before = foreground_hwnd();
    // 旧兼容路线仍只发送固定 WM_CLOSE。
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            Some(target),
            windows::Win32::UI::WindowsAndMessaging::WM_CLOSE,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(0),
        )
    }
    // 把固定 WM_CLOSE 发送失败映射为 Adapter 自有稳定错误。
    .map_err(|error| DesktopAdapterErrorCode::WindowCloseFailed.error(error.to_string()))?;
    // 保存旧兼容轮询结果。
    let mut closed = false;
    // 保持既有 2 秒直接兼容等待。
    for _ in 0..40 {
        // 只读检查旧目标是否仍存在。
        if !unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(target)) }.as_bool() {
            // 标记旧目标已经关闭。
            closed = true;
            // 结束轮询。
            break;
        }
        // 使用既有 50ms 轮询间隔。
        thread::sleep(Duration::from_millis(50));
    }
    // 返回旧 direct 兼容结果；正式 s2:w 不会进入此分支。
    Ok(json!({
        "ok": true,
        "app": "desktop",
        "operation": "close",
        "executionMode": "background-wm-close",
        // legacy 输入也只回显 canonical 安全窗口。
        "target": public_window_observation(&window),
        "closed": closed,
        "foreground": foreground_snapshot(before, foreground_hwnd()),
    }))
}

fn type_text(request: &CommandRequest) -> AppResult<Value> {
    // 读取可选 sessionId 以识别正式 opaque 控件路线。
    let session_id = request
        // 访问目标对象。
        .target
        // 读取固定字段。
        .get("sessionId")
        // 只接受字符串。
        .and_then(Value::as_str);
    // s2:c 精确控件不得进入窗口级前台回退逻辑。
    if session_id.is_some_and(|value| value.starts_with("s2:c:")) {
        // confirmation 必须先于目标重新发现。
        if !request.confirmed {
            // 返回统一确认错误。
            return Err(DesktopAdapterErrorCode::ConfirmationRequired.error(
                // 使用确认错误码。
                "Standard Edit text mutation requires confirmation.",
            ));
        }
        // 读取必需文本。
        let text = required_string(&request.args, "text")?;
        // 读取可选 deadline，缺失时使用 2000ms。
        let timeout_ms = match request.args.get("timeoutMs") {
            // 缺失时使用契约默认。
            None => standard_edit::DEFAULT_TIMEOUT_MS,
            // 只接受可转换为 u32 的整数。
            Some(value) => value
                // 读取无符号整数。
                .as_u64()
                // 转换为 u32。
                .and_then(|value| u32::try_from(value).ok())
                // 映射参数错误。
                .ok_or_else(|| {
                    // 返回稳定参数错误。
                    DesktopAdapterErrorCode::InvalidArgument.error(
                        // 使用通用参数错误码。
                        "args.timeoutMs must be an integer.",
                    )
                })?,
        };
        // 取得已经识别为 s2:c 的目标。
        let opaque_target = session_id.ok_or_else(|| {
            // 该分支理论上不可达，仍保持结构化失败。
            DesktopAdapterErrorCode::InvalidArgument.error("target.sessionId is required.")
        })?;
        // 执行正式唯一目标固定 mutation。
        let result = standard_edit::set_text(
            // 传入 opaque 目标。
            opaque_target,
            // 传入 UTF-8 文本。
            text,
            // 传入逐操作确认。
            request.confirmed,
            // 传入有界 deadline。
            timeout_ms,
        )?;
        // 返回旧 desktop 兼容形状但不公开 native 标识。
        return Ok(json!({
            // 标记成功。
            "ok": true,
            // 保留旧 app ID。
            "app": "desktop",
            // 保留旧 operation。
            "operation": "type-text",
            // 标记后台固定消息路径。
            "executionMode": "background-wm-settext",
            // 只回显 opaque 目标。
            "target": { "sessionId": opaque_target },
            // 返回固定回读证据。
            "verifiedByReadback": result["verifiedByReadback"],
            // 只返回前景不变布尔值。
            "foreground": { "unchanged": true },
            // 标记安全兼容形状。
            "compatibilityShape": "secured-desktop-standard-edit-v1",
        }));
    }
    let window = unique_window(&request.target)?;
    let text = required_string(&request.args, "text")?;
    let before = foreground_hwnd();
    let edit_controls = standard_edit_children(&window)?;
    if edit_controls.len() == 1 {
        set_standard_edit_text(&edit_controls[0], text)?;
        let after = foreground_hwnd();
        if before != after {
            // 使用 Adapter 自有封闭码拒绝宿主前景变化。
            return Err(DesktopAdapterErrorCode::ForegroundChanged.error(
                // 保持既有后台写入消息。
                "后台 WM_SETTEXT 意外改变了前景窗口；已拒绝前台回退。",
            ));
        }
        return Ok(json!({
            "ok": true,
            "app": "desktop",
            "operation": "type-text",
            "executionMode": "background-wm-settext",
            // 只输出 canonical 窗口事实。
            "target": public_window_observation(&window),
            // 只输出 canonical 控件事实。
            "control": public_standard_edit_target(&edit_controls[0]),
            "foreground": foreground_snapshot(before, after),
        }));
    }
    if !request.foreground_consent {
        let reason = if edit_controls.is_empty() {
            "目标窗口没有可认证的标准 Edit 控件。"
        } else {
            "目标窗口包含多个标准 Edit 控件，无法安全选择后台写入目标。"
        };
        return Err(policy::foreground_consent_required(
            request,
            reason,
            // 只返回计数与 canonical 窗口，不回显 native 候选。
            json!({ "candidateEditControls": edit_controls.len(), "window": public_window_observation(&window) }),
        ));
    }
    activate(window.hwnd)?;
    let inputs = unicode_inputs(text);
    send_inputs(&inputs)?;
    Ok(json!({
        "ok": true,
        "app": "desktop",
        "operation": "type-text",
        "executionMode": "foreground-sendinput",
        // 前台兼容路径仍禁止输出私有窗口事实。
        "target": public_window_observation(&window),
        "inputCount": inputs.len(),
        "foreground": foreground_snapshot(before, foreground_hwnd()),
    }))
}

fn press_key(request: &CommandRequest) -> AppResult<Value> {
    ensure_foreground_consent(request, "press-key")?;
    let window = unique_window(&request.target)?;
    let key = required_string(&request.args, "key")?;
    let hold_ms = optional_hold_ms(request.args.get("holdMs"))?;
    let phase = optional_key_phase(request.args.get("phase"))?;
    if phase != "press" && hold_ms != 0 {
        // 拒绝与 phase 不兼容的持续时间。
        return Err(DesktopAdapterErrorCode::InvalidArgument.error(
            // 保持既有参数组合消息。
            "args.holdMs is only valid when args.phase is 'press'.",
        ));
    }
    let before = foreground_hwnd();
    let inputs = chord_inputs(key)?;
    let release_index = inputs.len() / 2;
    match phase {
        "press" => {
            activate(window.hwnd)?;
            if hold_ms == 0 {
                send_inputs(&inputs)?;
            } else {
                send_inputs(&inputs[..release_index])?;
                thread::sleep(Duration::from_millis(hold_ms));
                let foreground_before_release = foreground_hwnd();
                send_inputs(&inputs[release_index..])?;
                if foreground_before_release != window.hwnd {
                    // 按键已安全释放但前景变化必须结构化报告。
                    return Err(DesktopAdapterErrorCode::ForegroundChanged.error(
                        // 保持既有部分执行消息。
                        "The foreground window changed while the key was held; the key was released safely.",
                    ));
                }
            }
        }
        "down" => {
            activate(window.hwnd)?;
            send_inputs(&inputs[..release_index])?;
        }
        "up" => {
            let foreground_before_release = foreground_hwnd();
            send_inputs(&inputs[release_index..])?;
            if foreground_before_release != window.hwnd {
                // 释放后仍报告目标已不在前景。
                return Err(DesktopAdapterErrorCode::ForegroundChanged.error(
                    // 保持既有安全释放消息。
                    "The key was released safely, but the target was no longer the foreground window.",
                ));
            }
        }
        _ => {
            // 防御性拒绝封闭集合之外的阶段。
            return Err(DesktopAdapterErrorCode::InvalidArgument.error(
                // 保持既有 phase 消息。
                "args.phase must be 'press', 'down', or 'up'.",
            ));
        }
    }
    Ok(json!({
        "ok": true,
        "app": "desktop",
        "operation": "press-key",
        "executionMode": "foreground-sendinput",
        // 前台兼容路径仍禁止输出私有窗口事实。
        "target": public_window_observation(&window),
        "key": key,
        "phase": phase,
        "holdMs": hold_ms,
        "foreground": foreground_snapshot(before, foreground_hwnd()),
    }))
}

fn click(request: &CommandRequest) -> AppResult<Value> {
    ensure_foreground_consent(request, "click")?;
    let window = unique_window(&request.target)?;
    let x = required_i32(&request.args, "x")?;
    let y = required_i32(&request.args, "y")?;
    let before = foreground_hwnd();
    activate(window.hwnd)?;
    unsafe { SetCursorPos(x, y) }
        // 把 Windows 光标移动失败映射为 Adapter 自有稳定错误。
        .map_err(|error| DesktopAdapterErrorCode::CursorMoveFailed.error(error.to_string()))?;
    let inputs = [
        mouse_input(MOUSEEVENTF_LEFTDOWN),
        mouse_input(MOUSEEVENTF_LEFTUP),
    ];
    send_inputs(&inputs)?;
    Ok(json!({
        "ok": true,
        "app": "desktop",
        "operation": "click",
        "executionMode": "foreground-sendinput",
        // 前台兼容路径仍禁止输出私有窗口事实。
        "target": public_window_observation(&window),
        "point": { "x": x, "y": y },
        "foreground": foreground_snapshot(before, foreground_hwnd()),
    }))
}

fn ensure_foreground_consent(request: &CommandRequest, operation: &str) -> AppResult<()> {
    if request.foreground_consent {
        return Ok(());
    }
    Err(policy::foreground_consent_required(
        request,
        "The desktop operation always activates a native window and sends foreground input.",
        json!({ "operation": operation, "execution": "foreground" }),
    ))
}

fn launch(request: &CommandRequest) -> AppResult<Value> {
    let path = Path::new(required_string(&request.args, "path")?);
    if !path.is_file() {
        // 拒绝不存在或非文件的可执行目标。
        return Err(DesktopAdapterErrorCode::InvalidArgument.error(
            // 保持既有路径消息。
            "args.path 必须是存在的可执行文件。",
        ));
    }
    let argv = request
        .args
        .get("argv")
        .map(parse_argv)
        .transpose()?
        .unwrap_or_default();
    let before = foreground_hwnd();
    // 保留进程 handle 直到结果构造完成，但不公开 PID。
    let _child = Command::new(path)
        .args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        // 把固定直接启动失败映射为 Adapter 自有稳定错误。
        .map_err(|error| DesktopAdapterErrorCode::ProcessStartFailed.error(error.to_string()))?;
    let after = foreground_hwnd();
    Ok(json!({
        "ok": true,
        "app": "desktop",
        "operation": "launch",
        "executionMode": "background-direct-process",
        "path": path,
        "argv": argv,
        "foreground": foreground_snapshot(before, after),
    }))
}

fn activate(hwnd: isize) -> AppResult<()> {
    let target = HWND(hwnd as *mut std::ffi::c_void);
    if !unsafe { IsWindowVisible(target) }.as_bool() {
        // 仅由已获授权的前台操作调用；异步恢复避免等待目标 UI 线程。
        let _ = unsafe { ShowWindowAsync(target, SW_RESTORE) };
        thread::sleep(Duration::from_millis(80));
    }
    for _ in 0..5 {
        if (foreground_hwnd() == hwnd || unsafe { SetForegroundWindow(target) }.as_bool())
            && foreground_hwnd() == hwnd
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    // 穷尽恢复重试后返回稳定前景激活失败。
    Err(DesktopAdapterErrorCode::ForegroundActivationFailed.error(
        // 保持既有零输入保证消息。
        "目标窗口无法恢复并成为前景窗口；未发送任何输入。",
    ))
}

fn send_inputs(inputs: &[INPUT]) -> AppResult<()> {
    let size = i32::try_from(std::mem::size_of::<INPUT>())
        // 把结构尺寸转换失败映射为稳定输入错误。
        .map_err(|_| DesktopAdapterErrorCode::InputFailed.error("INPUT 结构长度超出 i32。"))?;
    let sent = unsafe { SendInput(inputs, size) };
    if sent != u32::try_from(inputs.len()).unwrap_or_default() {
        // 不把部分输入报告为确定成功。
        return Err(DesktopAdapterErrorCode::InputBlockedOrPartial.error(
            // 保持既有权限与拦截说明。
            "Windows 未完整接收输入；可能被权限隔离或其他程序拦截。",
        ));
    }
    Ok(())
}

fn unicode_inputs(text: &str) -> Vec<INPUT> {
    text.encode_utf16()
        .flat_map(|unit| {
            [
                keyboard_input(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE),
                keyboard_input(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            ]
        })
        .collect()
}

fn chord_inputs(spec: &str) -> AppResult<Vec<INPUT>> {
    let parts = spec.split('+').map(str::trim).collect::<Vec<_>>();
    let (key, modifiers) = parts.split_last().ok_or_else(|| {
        // 空组合键必须返回稳定参数错误。
        DesktopAdapterErrorCode::InvalidArgument.error("args.key 必须包含一个按键名称。")
    })?;
    let mut keys = modifiers
        .iter()
        .map(|part| modifier_key(part).or_else(|_| named_key(part)))
        .collect::<AppResult<Vec<_>>>()?;
    keys.push(named_key(key)?);
    let mut inputs = keys
        .iter()
        .copied()
        .map(|key| keyboard_input(key, 0, Default::default()))
        .collect::<Vec<_>>();
    inputs.extend(
        keys.into_iter()
            .rev()
            .map(|key| keyboard_input(key, 0, KEYEVENTF_KEYUP)),
    );
    Ok(inputs)
}

fn modifier_key(name: &str) -> AppResult<VIRTUAL_KEY> {
    match name.trim().to_ascii_uppercase().as_str() {
        "CTRL" | "CONTROL" => Ok(VK_CONTROL),
        "ALT" => Ok(VK_MENU),
        "SHIFT" => Ok(VK_SHIFT),
        // 未知修饰键必须在输入构造前拒绝。
        _ => Err(DesktopAdapterErrorCode::InvalidArgument.error(
            // 保持回显被拒绝名称的既有消息。
            format!("不支持的修饰键 '{name}'。"),
        )),
    }
}

fn named_key(name: &str) -> AppResult<VIRTUAL_KEY> {
    let normalized = name.trim().to_ascii_uppercase();
    let key = match normalized.as_str() {
        "ENTER" | "RETURN" => VK_RETURN,
        "TAB" => VK_TAB,
        "ESC" | "ESCAPE" => VK_ESCAPE,
        "BACKSPACE" | "BACK" => VK_BACK,
        "DELETE" | "DEL" => VK_DELETE,
        "SPACE" => VK_SPACE,
        "LEFT" => VK_LEFT,
        "UP" => VK_UP,
        "RIGHT" => VK_RIGHT,
        "DOWN" => VK_DOWN,
        "HOME" => VK_HOME,
        "END" => VK_END,
        "F4" => VK_F4,
        _ if normalized.len() == 1 && normalized.is_ascii() => {
            VIRTUAL_KEY(u16::from(normalized.as_bytes()[0]))
        }
        _ => {
            // 未知按键必须在任何 SendInput 前拒绝。
            return Err(DesktopAdapterErrorCode::InvalidArgument.error(
                // 保持既有按键允许集消息。
                "args.key 支持单个 ASCII 字符或 ENTER、TAB、ESC、BACKSPACE、DELETE、SPACE、方向键、HOME、END。",
            ));
        }
    };
    Ok(key)
}

fn keyboard_input(
    key: VIRTUAL_KEY,
    scan: u16,
    flags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS,
) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn mouse_input(flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn required_string<'a>(args: &'a serde_json::Map<String, Value>, name: &str) -> AppResult<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            // 缺失或空字符串使用统一参数错误。
            DesktopAdapterErrorCode::InvalidArgument.error(format!("args.{name} 是必填字符串。"))
        })
}

fn required_i32(args: &serde_json::Map<String, Value>, name: &str) -> AppResult<i32> {
    args.get(name)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| {
            // 缺失或越界坐标使用统一参数错误。
            DesktopAdapterErrorCode::InvalidArgument.error(format!("args.{name} 必须是 i32 坐标。"))
        })
}

fn optional_hold_ms(value: Option<&Value>) -> AppResult<u64> {
    match value {
        None => Ok(0),
        Some(value) => value
            .as_u64()
            .filter(|hold_ms| *hold_ms <= 5_000)
            .ok_or_else(|| {
                // 越界持续时间使用统一参数错误。
                DesktopAdapterErrorCode::InvalidArgument.error(
                    // 保持既有范围说明。
                    "args.holdMs must be an integer between 0 and 5000.",
                )
            }),
    }
}

fn optional_key_phase(value: Option<&Value>) -> AppResult<&str> {
    match value {
        None => Ok("press"),
        Some(Value::String(phase)) if matches!(phase.as_str(), "press" | "down" | "up") => {
            Ok(phase.as_str())
        }
        // 未知类型或阶段使用统一参数错误。
        Some(_) => Err(DesktopAdapterErrorCode::InvalidArgument.error(
            // 保持既有封闭阶段消息。
            "args.phase must be 'press', 'down', or 'up'.",
        )),
    }
}

fn parse_argv(value: &Value) -> AppResult<Vec<&str>> {
    value
        .as_array()
        // 非数组 argv 使用统一参数错误。
        .ok_or_else(|| {
            DesktopAdapterErrorCode::InvalidArgument.error("args.argv 必须是字符串数组。")
        })?
        .iter()
        .map(|item| {
            item.as_str().ok_or_else(|| {
                // 非字符串元素使用同一参数错误类型。
                DesktopAdapterErrorCode::InvalidArgument.error("args.argv 只能包含字符串。")
            })
        })
        .collect()
}

// 仅在测试构建中载入独立回归文件。
#[cfg(test)]
// 把物理测试文件绑定为当前 Adapter 的私有子模块。
#[path = "desktop_tests.rs"]
mod tests;
