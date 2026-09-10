use std::path::Path;

use serde_json::{Value, json};

use crate::{
    adapters::windows::foreground_hwnd,
    domain::{AppControlError, AppResult, CommandRequest},
};

use super::{
    add_foreground,
    connection::run_script_json,
    domain::{
        CanvasInput, CloseInput, ExportInput, LayersInput, SaveInput, js_string, parse_input,
        validate_output_path, verify_non_empty_file,
    },
    required_session_id,
    scripts::{APPLY_SCRIPT, CLOSE_SCRIPT, CREATE_SCRIPT, EXPORT_SCRIPT, SAVE_SCRIPT},
    sessions::{
        application_session_id, document_capabilities, document_session_id,
        document_session_id_for_instance, document_state_by_id, resolve_document_id,
        running_photoshop_pid,
    },
};

pub(super) fn create_canvas(request: &CommandRequest) -> AppResult<Value> {
    let before = foreground_hwnd();
    let process_id = running_photoshop_pid()?;
    if required_session_id(request)? != application_session_id(process_id) {
        return Err(AppControlError::new(
            "INVALID_TARGET_KIND",
            "image.canvas.create@1 requires the current application session.",
        ));
    }
    let input: CanvasInput = parse_input(request)?;
    input.validate()?;
    let script = CREATE_SCRIPT
        .replace("__WIDTH__", &input.width.to_string())
        .replace("__HEIGHT__", &input.height.to_string())
        .replace("__RESOLUTION__", &input.resolution.to_string())
        .replace("__NAME__", &js_string(&input.name)?);
    let mut data = run_script_json(script)?;
    let session_id = document_session_id(process_id, &data)?;
    data.as_object_mut().map(|object| object.remove("id"));
    Ok(add_foreground(
        json!({
            "session": {
                "sessionId": session_id,
                "kind": "document",
                "title": input.name,
                "capabilities": document_capabilities(),
            },
            "document": data,
        }),
        before,
        foreground_hwnd(),
    ))
}

pub(super) fn apply_layers(request: &CommandRequest) -> AppResult<Value> {
    let before = foreground_hwnd();
    let document_id = resolve_document_id(required_session_id(request)?)?;
    let input: LayersInput = parse_input(request)?;
    input.validate()?;
    let script = APPLY_SCRIPT
        .replace("__DOCUMENT_ID__", &document_id.to_string())
        .replace("__OPERATIONS__", &input.script()?);
    let mut data = run_script_json(script)?;
    data.as_object_mut().map(|object| object.remove("id"));
    Ok(add_foreground(data, before, foreground_hwnd()))
}

pub(super) fn save_artifact(request: &CommandRequest) -> AppResult<Value> {
    let before = foreground_hwnd();
    let document_id = resolve_document_id(required_session_id(request)?)?;
    let input: SaveInput = parse_input(request)?;
    validate_output_path(&input.path, "psd", input.overwrite)?;
    let script = SAVE_SCRIPT
        .replace("__DOCUMENT_ID__", &document_id.to_string())
        .replace("__PATH__", &js_string(&input.path)?);
    let mut data = run_script_json(script)?;
    verify_non_empty_file(&input.path)?;

    // 不只信任 COM 返回：回读 Photoshop 文档状态并核对 source-of-truth 路径。
    // 同时保留该 inventory 的进程创建时间以生成同代际文档身份。
    let (process_id, process_creation_time, state) = document_state_by_id(document_id)?;
    verify_saved_document(&input.path, &state)?;
    // 使用同一验证快照生成保存后的 canonical s2:d。
    let current_session = document_session_id_for_instance(
        // 绑定当前 Photoshop PID。
        process_id,
        // 绑定当前 Photoshop 进程创建 FILETIME。
        process_creation_time,
        // 绑定保存后重新读取的文档事实。
        &state,
        // 结束保存后文档身份生成。
    )?;
    data.as_object_mut().map(|object| {
        object.remove("id");
        object.insert(
            "verification".to_owned(),
            json!({
                "fileNonEmpty": true,
                "documentSaved": true,
                "pathMatched": true,
                "sessionId": current_session,
            }),
        )
    });
    Ok(add_foreground(data, before, foreground_hwnd()))
}

pub(super) fn export_image(request: &CommandRequest) -> AppResult<Value> {
    let before = foreground_hwnd();
    let document_id = resolve_document_id(required_session_id(request)?)?;
    let input: ExportInput = parse_input(request)?;
    if !input.format.eq_ignore_ascii_case("png") {
        return Err(AppControlError::new(
            "CAPABILITY_UNSUPPORTED",
            "image.export@1 currently certifies PNG output only.",
        ));
    }
    validate_output_path(&input.path, "png", input.overwrite)?;
    let script = EXPORT_SCRIPT
        .replace("__DOCUMENT_ID__", &document_id.to_string())
        .replace("__PATH__", &js_string(&input.path)?);
    let mut data = run_script_json(script)?;
    verify_non_empty_file(&input.path)?;
    data.as_object_mut().map(|object| {
        object.remove("id");
        object.insert("verification".to_owned(), json!({ "fileNonEmpty": true }))
    });
    Ok(add_foreground(data, before, foreground_hwnd()))
}

pub(super) fn close_document(request: &CommandRequest) -> AppResult<Value> {
    let before = foreground_hwnd();
    let document_id = resolve_document_id(required_session_id(request)?)?;
    let input: CloseInput = parse_input(request)?;
    if input.save_changes {
        return Err(AppControlError::new(
            "CAPABILITY_UNSUPPORTED",
            "document.close@1 certifies discard-only cleanup; save first through artifact.save@1.",
        ));
    }
    let script = CLOSE_SCRIPT.replace("__DOCUMENT_ID__", &document_id.to_string());
    let mut data = run_script_json(script)?;
    data.as_object_mut().map(|object| object.remove("id"));
    Ok(add_foreground(data, before, foreground_hwnd()))
}

fn verify_saved_document(expected_path: &str, state: &Value) -> AppResult<()> {
    if state.get("saved") != Some(&Value::Bool(true)) {
        return Err(AppControlError::new(
            "OUTPUT_VERIFICATION_FAILED",
            "The file exists but Photoshop still reports the document as modified.",
        ));
    }
    let actual_path = state.get("path").and_then(Value::as_str).ok_or_else(|| {
        AppControlError::new(
            "OUTPUT_VERIFICATION_FAILED",
            "Photoshop did not report a source path after save.",
        )
    })?;
    let expected = std::fs::canonicalize(Path::new(expected_path))
        .map_err(|error| AppControlError::new("OUTPUT_VERIFICATION_FAILED", error.to_string()))?;
    let actual = std::fs::canonicalize(Path::new(actual_path))
        .map_err(|error| AppControlError::new("OUTPUT_VERIFICATION_FAILED", error.to_string()))?;
    if expected != actual {
        return Err(AppControlError::with_details(
            "OUTPUT_VERIFICATION_FAILED",
            "Photoshop saved a different source document path than requested.",
            json!({ "expected": expected, "actual": actual }),
        ));
    }
    Ok(())
}
