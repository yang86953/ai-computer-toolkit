mod connection;
mod domain;
mod operations;
mod scripts;
mod sessions;

use serde_json::{Value, json};

use crate::{
    adapters::{app::CapabilityProvider, window::foreground_snapshot, windows::foreground_hwnd},
    // 导入版本化 capability 单一注册表。
    capabilities,
    // 导入严格 s2 解析器以按 provider-neutral kind 预筛选。
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    domain::{AppControlError, AppResult, CommandRequest},
};

use self::{
    connection::{PhotoshopConnection, photoshop_installed, run_com_task, run_script_json},
    operations::{apply_layers, close_document, create_canvas, export_image, save_artifact},
    scripts::INSPECT_SCRIPT,
    sessions::{
        accepts_application_session, application_session_id, discover_sessions,
        remove_native_document_id, resolve_document_id, running_photoshop_pid,
    },
};

pub struct PhotoshopProvider;

pub(super) const CAPABILITIES: &[&str] = &[
    // 公布画布创建 capability。
    capabilities::IMAGE_CANVAS_CREATE,
    // 公布图层变更 capability。
    capabilities::IMAGE_LAYERS_APPLY,
    // 公布制品保存 capability。
    capabilities::ARTIFACT_SAVE,
    // 公布图像导出 capability。
    capabilities::IMAGE_EXPORT,
    // 公布文档关闭 capability。
    capabilities::DOCUMENT_CLOSE,
];

impl CapabilityProvider for PhotoshopProvider {
    fn provider_id(&self) -> &'static str {
        "structured-image-editor"
    }

    fn capabilities(&self) -> &'static [&'static str] {
        CAPABILITIES
    }

    fn accepts_session(&self, session_id: &str) -> AppResult<bool> {
        // 严格拒绝旧 s1、非 canonical 和未知类别目标。
        let Some(target) = OpaqueTargetId::parse(session_id) else {
            // 不为不属于 s2 契约的目标探测 COM provider。
            return Ok(false);
            // 结束非 canonical 目标分支。
        };
        // 仅对 Photoshop 发布的应用和文档目标执行重新发现。
        match target.kind() {
            // 应用目标先通过进程快照比对，避免为其他 s2:a 探测 COM。
            OpaqueTargetKind::Application => accepts_application_session(session_id),
            // 文档目标必须在当前 COM inventory 中唯一重新解析。
            OpaqueTargetKind::Document => match resolve_document_id(session_id) {
                // 唯一文档命中表示当前目标可用。
                Ok(_) => Ok(true),
                // stale 文档属于当前 provider 的未命中结果。
                Err(error) if error.code == "STALE_SESSION" => Ok(false),
                // 未安装 provider 不应阻塞其他结构化文档 provider。
                Err(error) if error.code == "APPLICATION_NOT_INSTALLED" => Ok(false),
                // 其余权限、协议或歧义错误必须保留。
                Err(error) => Err(error),
                // 结束文档重新发现分支。
            },
            // 其他 provider-neutral kind 不属于 PhotoshopProvider。
            _ => Ok(false),
            // 结束目标类别分发。
        }
    }

    fn status(&self) -> AppResult<Value> {
        let before = foreground_hwnd();
        let status = run_com_task(|| {
            let connection = PhotoshopConnection::attach()?;
            match connection {
                Some(connection) => Ok(json!({
                    "ok": true,
                    "backend": "attach-only COM automation",
                    "installed": true,
                    "connected": true,
                    "version": connection.do_javascript("app.version;")?,
                    "capabilities": CAPABILITIES,
                })),
                None => Ok(json!({
                    "ok": true,
                    "backend": "attach-only COM automation",
                    "installed": photoshop_installed(),
                    "connected": false,
                    "capabilities": CAPABILITIES,
                })),
            }
        })?;
        Ok(add_foreground(status, before, foreground_hwnd()))
    }

    fn sessions(&self, _: &CommandRequest) -> AppResult<Value> {
        Ok(json!({ "ok": true, "sessions": discover_sessions()? }))
    }

    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        // 计时覆盖 session 重解析，避免把发现阶段的抢焦点漏出结果。
        let before = foreground_hwnd();
        let public_session = required_session_id(request)?;
        let process_id = running_photoshop_pid()?;
        if public_session == application_session_id(process_id) {
            return self.status();
        }
        let document_id = resolve_document_id(public_session)?;
        let script = INSPECT_SCRIPT
            .replace("__DOCUMENT_ID__", &document_id.to_string())
            .replace("__MAX_DEPTH__", &request.max_depth.clamp(1, 16).to_string())
            .replace(
                "__MAX_ITEMS__",
                &request.max_items.clamp(1, 1_000).to_string(),
            );
        let mut data = run_script_json(script)?;
        remove_native_document_id(&mut data);
        Ok(add_foreground(data, before, foreground_hwnd()))
    }

    fn execute(&self, capability: &str, request: &CommandRequest) -> AppResult<Value> {
        // 将注册表 capability 路由到结构化图像领域操作。
        match capability {
            // 路由画布创建。
            capabilities::IMAGE_CANVAS_CREATE => create_canvas(request),
            // 路由图层变更。
            capabilities::IMAGE_LAYERS_APPLY => apply_layers(request),
            // 路由制品保存。
            capabilities::ARTIFACT_SAVE => save_artifact(request),
            // 路由图像导出。
            capabilities::IMAGE_EXPORT => export_image(request),
            // 路由文档关闭。
            capabilities::DOCUMENT_CLOSE => close_document(request),
            _ => Err(AppControlError::new(
                "CAPABILITY_UNSUPPORTED",
                "The structured image provider does not support this capability.",
            )),
        }
    }
}

pub(super) fn required_session_id(request: &CommandRequest) -> AppResult<&str> {
    request
        .target
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppControlError::new("INVALID_ARGUMENT", "target.sessionId is required."))
}

pub(super) fn add_foreground(mut value: Value, before: isize, after: isize) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("foreground".to_owned(), foreground_snapshot(before, after));
    }
    value
}

// 仅验证无需 COM 写入的 Photoshop provider 身份预筛选。
#[cfg(test)]
// 声明 provider 路由测试集合。
mod tests {
    // 导入当前 provider 与 trait 接口。
    use super::*;

    // 验证旧版本、错误 kind 与 stale 应用目标都 fail closed。
    #[test]
    // 该测试只读取进程快照，不附着或启动 Photoshop。
    fn provider_rejects_legacy_wrong_kind_and_stale_application() -> AppResult<()> {
        // 构造 Photoshop provider 实例。
        let provider = PhotoshopProvider;
        // 拒绝反向迁移前的旧 s1:c3 路由。
        assert!(!provider.accepts_session("s1:c3:0000000000000000")?);
        // 拒绝属于窗口 provider 的 canonical s2 目标。
        assert!(!provider.accepts_session("s2:w:0000000000000000")?);
        // 拒绝未匹配当前 Photoshop 进程实例的应用目标。
        assert!(!provider.accepts_session("s2:a:0000000000000000")?);
        // 报告 provider 预筛选测试成功。
        Ok(())
        // 结束 Photoshop provider 身份门禁测试。
    }
    // 结束 provider 路由测试集合。
}
