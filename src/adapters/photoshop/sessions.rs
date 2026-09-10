use serde_json::{Value, json};

use crate::{
    adapters::{
        app::capability_descriptor,
        window::foreground_snapshot,
        // 导入进程枚举、创建时间与前景只读事实。
        windows::{enumerate_processes, foreground_hwnd, process_creation_time},
    },
    // 导入版本化 capability 单一注册表。
    capabilities,
    // 导入 provider-neutral s2 应用和文档目标 Component。
    components::opaque_id::{
        // 导入统一碰撞分类与匹配函数。
        OpaqueTargetId,
        OpaqueTargetKind,
        OpaqueTargetMatch,
        match_opaque_target,
    },
    domain::{AppControlError, AppResult},
};

use super::{
    connection::{PhotoshopConnection, parse_script_json, run_com_task},
    scripts::DOCUMENTS_SCRIPT,
};

// 固定与 C++ StructuredImageBackend 相同的 provider identity。
const STRUCTURED_IMAGE_PROVIDER_ID: &str = "structured-image-editor";

// 保存一次 Photoshop 重新发现得到的同代际应用与文档事实。
struct PhotoshopInventory {
    // 保存仅供内部重新解析使用的原生进程 ID。
    process_id: u32,
    // 保存抵抗 PID 复用的进程创建 FILETIME。
    process_creation_time: u64,
    // 保存同一 COM 快照返回的文档事实。
    documents: Vec<Value>,
    // 结束 Photoshop inventory 数据定义。
}

pub(super) fn discover_sessions() -> AppResult<Vec<Value>> {
    let before = foreground_hwnd();
    let Some(inventory) = document_inventory()? else {
        return Ok(Vec::new());
    };
    let after = foreground_hwnd();
    let mut sessions = vec![json!({
        "sessionId": application_session_id_for_instance(
            inventory.process_id,
            inventory.process_creation_time,
        ),
        "kind": "application",
        "title": "Adobe Photoshop",
        "state": "running",
        "capabilities": [capability_descriptor(
            capabilities::IMAGE_CANVAS_CREATE,
            json!({
                "coordinateOrigin": "top-left",
                "geometryUnit": "px",
                "textUnit": "pt",
                "resolutionUnit": "dpi"
            }),
        )],
        "foreground": foreground_snapshot(before, after),
    })];

    for document in inventory.documents {
        let mut public_document = document.clone();
        // 删除只参与重新发现身份的原生文档 ID 与源路径。
        remove_private_document_fields(&mut public_document);
        sessions.push(json!({
            "sessionId": document_session_id_for_instance(
                inventory.process_id,
                inventory.process_creation_time,
                &document,
            )?,
            "kind": "document",
            "title": document.get("name").cloned().unwrap_or(Value::Null),
            "state": if document.get("saved") == Some(&Value::Bool(true)) { "saved" } else { "modified" },
            "document": public_document,
            "capabilities": document_capabilities(),
            "foreground": foreground_snapshot(before, after),
        }));
    }
    Ok(sessions)
}

pub(super) fn running_photoshop_pid() -> AppResult<u32> {
    // 枚举当前全部 Photoshop 进程候选。
    let matches = running_photoshop_pids()?;
    match matches.as_slice() {
        [process_id] => Ok(*process_id),
        [] => Err(AppControlError::new(
            "APPLICATION_NOT_RUNNING",
            "No running structured image application is available.",
        )),
        _ => Err(AppControlError::new(
            "AMBIGUOUS_TARGET",
            "More than one Photoshop process is running; the COM instance is not unique.",
        )),
    }
}

// 枚举全部 Photoshop PID，供身份预筛选与唯一执行门禁共用。
fn running_photoshop_pids() -> AppResult<Vec<u32>> {
    // 只保留精确可执行文件名匹配的进程。
    Ok(enumerate_processes()?
        // 消费当前只读进程快照。
        .into_iter()
        // 使用不区分大小写的 Windows 文件名比较。
        .filter(|process| process.process_name.eq_ignore_ascii_case("Photoshop.exe"))
        // 仅把 PID 保留在 provider 私有边界内。
        .map(|process| process.process_id)
        // 收集当前应用实例候选。
        .collect::<Vec<_>>())
    // 结束 Photoshop PID 枚举。
}

// 验证 s2:a 是否精确对应当前唯一 Photoshop 进程实例。
pub(super) fn accepts_application_session(public_session: &str) -> AppResult<bool> {
    // 读取当前全部 Photoshop 进程候选。
    let process_ids = running_photoshop_pids()?;
    // 使用统一 Component 从当前 PID 代际事实重新解析。
    let matched = match_opaque_target(public_session, &process_ids, |process_id| {
        // 每次匹配都重新读取创建 FILETIME。
        Some(application_session_id(*process_id))
        // 结束 Photoshop 应用候选身份生成。
    });
    // 同时保留 attach-only COM 必须只有一个进程的边界。
    match (process_ids.len(), matched) {
        // 唯一进程且唯一身份命中时接受。
        (1, OpaqueTargetMatch::Unique(_)) => Ok(true),
        // 零命中表示其他 provider 目标或已过期目标。
        (_, OpaqueTargetMatch::Missing) => Ok(false),
        // 多进程或多指纹命中都不得任取 COM 实例。
        _ => Err(AppControlError::new(
            // 使用稳定歧义错误码。
            "AMBIGUOUS_TARGET",
            // 解释拒绝原因而不公开进程标识。
            "More than one Photoshop process is running; the COM instance is not unique.",
        )),
        // 结束 Photoshop 应用重新解析分类。
    }
    // 结束应用 session 接受门禁。
}

// 从当前进程实例生成运行中 Photoshop 应用目标。
pub(super) fn application_session_id(process_id: u32) -> String {
    // 每次调用都重新读取创建 FILETIME，拒绝 PID 复用后的旧身份。
    application_session_id_for_instance(process_id, process_creation_time(process_id))
}

// 从同一 inventory 快照的应用事实生成 canonical s2:a。
fn application_session_id_for_instance(process_id: u32, creation_time: u64) -> String {
    // 组合固定 provider key、十进制 PID 与进程创建 FILETIME。
    let identity = format!("{STRUCTURED_IMAGE_PROVIDER_ID}:{process_id}:{creation_time}");
    // 只返回 provider-neutral opaque 应用指纹。
    OpaqueTargetId::new(
        // 使用运行中应用目标类别。
        OpaqueTargetKind::Application,
        // 传入不越过 JSON 边界的私有身份。
        &identity,
    )
    // 输出 canonical s2:a 文本。
    .to_string()
    // 结束应用身份生成。
}

// 从当前进程实例与文档事实生成结构化文档目标。
pub(super) fn document_session_id(process_id: u32, document: &Value) -> AppResult<String> {
    // 每次调用都重新读取创建 FILETIME，避免跨进程代际重绑。
    document_session_id_for_instance(process_id, process_creation_time(process_id), document)
}

// 从同一 inventory 快照生成 canonical s2:d 文档目标。
pub(super) fn document_session_id_for_instance(
    // 接收 provider 私有 PID。
    process_id: u32,
    // 接收同代际进程创建 FILETIME。
    creation_time: u64,
    // 接收当前 COM 文档事实。
    document: &Value,
) -> AppResult<String> {
    let id = document
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| AppControlError::new("OPERATION_FAILED", "Document has no native id."))?;
    let name = document
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let path = document
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // 组合 PID、创建 FILETIME、原生文档 ID、名称与路径作为私有身份。
    let identity = format!("{process_id}\0{creation_time}\0{id}\0{name}\0{path}");
    // 返回不泄漏任一私有字段的 canonical s2:d。
    Ok(OpaqueTargetId::new(OpaqueTargetKind::Document, &identity).to_string())
}

pub(super) fn resolve_document_id(public_session: &str) -> AppResult<i64> {
    let Some(inventory) = document_inventory()? else {
        return Err(AppControlError::new(
            "STALE_SESSION",
            "The image document session no longer exists.",
        ));
    };
    // 在同一应用代际的当前文档 inventory 中唯一匹配。
    match match_opaque_target(public_session, &inventory.documents, |document| {
        // 只使用同一 inventory 的 PID、FILETIME 和文档事实。
        document_session_id_for_instance(
            // 传入当前 Photoshop PID。
            inventory.process_id,
            // 传入当前 Photoshop 进程创建 FILETIME。
            inventory.process_creation_time,
            // 传入当前文档事实。
            document,
            // 结束文档身份生成。
        )
        // 无法生成身份的畸形文档不参与命中。
        .ok()
        // 结束文档候选身份生成。
    }) {
        // 唯一命中后才恢复 provider 私有文档 ID。
        OpaqueTargetMatch::Unique(document) => document
            // 读取同一命中候选的原生 ID。
            .get("id")
            // 要求原生 ID 保持整数形状。
            .and_then(Value::as_i64)
            // 畸形命中不得进入执行。
            .ok_or_else(|| {
                // 返回 provider 数据错误。
                AppControlError::new("OPERATION_FAILED", "Document has no native id.")
                // 结束文档 ID 错误构造。
            }),
        // 零命中明确表示目标已在使用时过期。
        OpaqueTargetMatch::Missing => Err(AppControlError::new(
            "STALE_SESSION",
            "The image document session no longer exists or its identity changed.",
        )),
        // 两个或更多文档生成同一公开指纹时 fail closed。
        OpaqueTargetMatch::Ambiguous => Err(AppControlError::new(
            "AMBIGUOUS_TARGET",
            "The document session resolved to more than one document.",
        )),
        // 结束文档重新解析分类。
    }
}

pub(super) fn document_state_by_id(document_id: i64) -> AppResult<(u32, u64, Value)> {
    let Some(inventory) = document_inventory()? else {
        return Err(AppControlError::new(
            "APPLICATION_NOT_RUNNING",
            "The structured image application stopped before verification.",
        ));
    };
    let matches = inventory
        .documents
        .into_iter()
        .filter(|document| document.get("id").and_then(Value::as_i64) == Some(document_id))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [document] => Ok((
            inventory.process_id,
            inventory.process_creation_time,
            document.clone(),
        )),
        [] => Err(AppControlError::new(
            "STALE_SESSION",
            "The document disappeared before verification.",
        )),
        _ => Err(AppControlError::new(
            "AMBIGUOUS_TARGET",
            "The native document id resolved to more than one document.",
        )),
    }
}

fn document_inventory() -> AppResult<Option<PhotoshopInventory>> {
    let data = run_com_task(|| {
        let Some(connection) = PhotoshopConnection::attach()? else {
            return Ok(None);
        };
        let text = connection.do_javascript(DOCUMENTS_SCRIPT)?;
        Ok(Some(parse_script_json(&text)?))
    })?;
    let Some(data) = data else {
        return Ok(None);
    };
    let documents = data
        .get("documents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // 精确解析唯一运行中 Photoshop 进程。
    let process_id = running_photoshop_pid()?;
    // 在同一 inventory 中绑定进程创建 FILETIME。
    let process_creation_time = process_creation_time(process_id);
    // 返回同代际应用与文档事实。
    Ok(Some(PhotoshopInventory {
        // 保存 provider 私有 PID。
        process_id,
        // 保存 provider 私有进程创建时间。
        process_creation_time,
        // 保存当前 COM 文档快照。
        documents,
        // 结束 Photoshop inventory 构造。
    }))
}

pub(super) fn document_capabilities() -> Value {
    json!([
        capability_descriptor(
            capabilities::IMAGE_LAYERS_APPLY,
            json!({
                "coordinateOrigin": "top-left",
                "geometryUnit": "px",
                "textUnit": "pt",
                "supportedOperations": ["layer.addRect", "layer.addPolygon", "layer.addText", "layer.remove"]
            }),
        ),
        capability_descriptor(
            capabilities::ARTIFACT_SAVE,
            json!({ "format": "psd", "overwriteProtection": true }),
        ),
        capability_descriptor(
            capabilities::IMAGE_EXPORT,
            json!({ "formats": ["png"], "overwriteProtection": true }),
        ),
        capability_descriptor(
            capabilities::DOCUMENT_CLOSE,
            json!({ "saveChanges": [false] }),
        ),
    ])
}

pub(super) fn remove_native_document_id(value: &mut Value) {
    if let Some(document) = value.get_mut("document").and_then(Value::as_object_mut) {
        // 删除 inspect 结果中的原生文档 ID。
        document.remove("id");
        // 删除 inspect 结果中的私有源文档路径。
        document.remove("path");
    }
}

// 清理 sessions 中直接序列化的文档私有身份字段。
fn remove_private_document_fields(document: &mut Value) {
    // 只处理 JSON object 形式的文档事实。
    if let Some(object) = document.as_object_mut() {
        // 删除原生文档 ID。
        object.remove("id");
        // 删除现有源文档路径；调用方输出路径由 artifact 结果单独处理。
        object.remove("path");
        // 结束文档私有字段清理。
    }
    // 结束 sessions 文档清理。
}

#[cfg(test)]
mod tests {
    use super::*;

    // 验证应用身份与 C++ provider key/PID/FILETIME 布局完全一致。
    #[test]
    // 同时覆盖进程代际变化后的 stale 防护。
    fn application_session_matches_cpp_golden_and_process_generation() {
        // 构造稳定的跨实现应用身份夹具。
        let current = application_session_id_for_instance(10, 123);
        // 断言 canonical s2:a 与独立 FNV-1a golden 相同。
        assert_eq!(current, "s2:a:df092f0662f4b7a5");
        // PID 变化必须产生不同应用目标。
        assert_ne!(current, application_session_id_for_instance(11, 123));
        // 进程创建 FILETIME 变化必须产生不同应用目标。
        assert_ne!(current, application_session_id_for_instance(10, 124));
        // 结束应用身份与进程代际测试。
    }

    // 验证文档身份与 C++ PID/FILETIME/document facts 布局完全一致。
    #[test]
    // 同时覆盖进程代际和文档事实变化。
    fn document_session_matches_cpp_golden_and_changes_with_identity() -> AppResult<()> {
        // 构造稳定的当前文档事实。
        let first = json!({ "id": 7, "name": "poster.psd", "path": "C:\\a\\poster.psd" });
        // 构造重命名并改变源路径后的文档事实。
        let renamed = json!({ "id": 7, "name": "poster-v2.psd", "path": "C:\\a\\poster-v2.psd" });
        // 生成稳定跨实现文档身份夹具。
        let current = document_session_id_for_instance(10, 123, &first)?;
        // 断言 canonical s2:d 与独立 FNV-1a golden 相同。
        assert_eq!(current, "s2:d:8566c36650cec9c1");
        // PID 变化必须产生不同文档目标。
        assert_ne!(current, document_session_id_for_instance(11, 123, &first)?);
        // 进程创建 FILETIME 变化必须产生不同文档目标。
        assert_ne!(current, document_session_id_for_instance(10, 124, &first)?);
        // 文档名称或源路径变化必须产生不同文档目标。
        assert_ne!(
            current,
            document_session_id_for_instance(10, 123, &renamed)?
        );
        // 报告文档身份测试成功。
        Ok(())
        // 结束文档身份与代际测试。
    }

    #[test]
    fn facade_removes_native_document_id_but_keeps_layer_ids() {
        let mut value = json!({
            "document": { "id": 246, "name": "poster.psd", "path": "C:\\private\\poster.psd" },
            "layers": [{ "id": 12, "name": "TITLE" }]
        });
        remove_native_document_id(&mut value);
        assert!(value["document"].get("id").is_none());
        // 私有源文档路径不得进入 inspect JSON。
        assert!(value["document"].get("path").is_none());
        assert_eq!(value["layers"][0]["id"], 12);
    }
}
