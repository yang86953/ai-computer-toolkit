// 把错误码实现保留为当前 text document provider Adapter 的普通私有类型。
#[path = "text_error.rs"]
mod error_code;

use serde_json::{Map, Value, json};

// 导入当前 text document provider Adapter 私有封闭错误码。
use error_code::AppTextErrorCode;

use crate::{
    adapters::{AppAdapter, NotepadAdapter, app::CapabilityProvider},
    // 导入版本化 capability 单一注册表。
    capabilities,
    // 导入与 C++ 对照实现等价的 s2 目标身份原语。
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    domain::{AppResult, CommandRequest},
};

// 导入 app facade 的能力描述与精确目标读取工具。
use super::{capability_descriptor, session::required_session_id};

pub(super) struct TextDocumentProvider;

// 从注册表投影文本 provider 公布的能力集合。
const TEXT_CAPABILITIES: &[&str] = &[capabilities::TEXT_DOCUMENT_CREATE];

impl CapabilityProvider for TextDocumentProvider {
    fn provider_id(&self) -> &'static str {
        "text-document"
    }

    fn capabilities(&self) -> &'static [&'static str] {
        TEXT_CAPABILITIES
    }

    fn accepts_session(&self, session_id: &str) -> AppResult<bool> {
        // 先拒绝旧版本、未知类别和 stale 创建器目标。
        if !is_current_application_session_id(session_id) {
            return Ok(false);
        }
        NotepadAdapter.status().map(|_| true)
    }

    fn status(&self) -> AppResult<Value> {
        // 项目内部适配器负责探测可执行文件；facade 只保留领域能力状态。
        NotepadAdapter.status()?;
        Ok(json!({
            "ok": true,
            "capabilities": TEXT_CAPABILITIES,
        }))
    }

    fn sessions(&self, _: &CommandRequest) -> AppResult<Value> {
        self.status()?;
        Ok(session_payload())
    }

    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        let session_id = required_session_id(request)?;
        if !self.accepts_session(session_id)? {
            // 当前创建器目标以 provider 私有过期分类失败。
            return Err(
                // 构造稳定 provider-neutral 公开错误。
                AppTextErrorCode::StaleSession
                    // 保持既有目标不可用消息。
                    .error("The text document application session is unavailable."),
            );
        }
        Ok(json!({
            "kind": "application",
            "state": "available",
            "capabilities": TEXT_CAPABILITIES,
        }))
    }

    fn execute(&self, capability: &str, request: &CommandRequest) -> AppResult<Value> {
        // 只接受注册表声明的文本文档创建 capability。
        if capability != capabilities::TEXT_DOCUMENT_CREATE {
            // 未登记 capability 以 provider 私有缺口分类失败。
            return Err(
                // 构造稳定 provider-neutral 公开错误。
                AppTextErrorCode::CapabilityUnsupported
                    // 保持既有 capability 缺口消息。
                    .error("The text document session supports text.document.create@1 only."),
            );
        }
        let delegated = delegated_create_request(request)?;
        public_document_result(NotepadAdapter.run(&delegated)?)
    }
}

fn application_session_id() -> String {
    // 使用与 C++ TextDocumentModule 相同的私有稳定身份生成 s2:a 目标。
    OpaqueTargetId::new(OpaqueTargetKind::Application, "text-document-creator").to_string()
}

// 精确比较当前创建器目标，避免为旧 ID 提供宽松别名。
fn is_current_application_session_id(session_id: &str) -> bool {
    // 静态创建器仍在每次检查时重新生成 canonical 当前身份。
    session_id == application_session_id()
}

fn session_payload() -> Value {
    json!({
        "ok": true,
        "sessions": [{
            "sessionId": application_session_id(),
            "kind": "application",
            "title": "Text document creator",
            "state": "available",
            "capabilities": [capability_descriptor(
                capabilities::TEXT_DOCUMENT_CREATE,
                json!({
                    "encoding": "utf-8",
                    "artifactMediaType": "text/plain",
                    "required": ["text"],
                }),
            )],
        }],
    })
}

fn delegated_create_request(request: &CommandRequest) -> AppResult<CommandRequest> {
    let text = request
        .args
        .get("input")
        .and_then(Value::as_object)
        .and_then(|input| input.get("text"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            // 使用 text document provider 私有输入拒绝分类。
            AppTextErrorCode::InvalidArgument.error("args.input.text is required.")
        })?;
    let mut delegated = request.clone();
    delegated.app = "notepad".to_owned();
    delegated.operation = Some("open-and-write-text".to_owned());
    delegated.target = Map::new();
    delegated.args = Map::from_iter([("text".to_owned(), Value::String(text.to_owned()))]);
    Ok(delegated)
}

fn public_document_result(result: Value) -> AppResult<Value> {
    let path = result.get("path").cloned().ok_or_else(|| {
        // 使用 text document provider 私有成功投影失败分类。
        AppTextErrorCode::OperationFailed
            // 保持既有 artifact path 缺失消息。
            .error("The text document adapter returned no artifact path.")
    })?;
    let text = result.get("text").cloned().ok_or_else(|| {
        // 使用 text document provider 私有成功投影失败分类。
        AppTextErrorCode::OperationFailed
            // 保持既有 verified content 缺失消息。
            .error("The text document adapter returned no verified content.")
    })?;
    let foreground_unchanged = result
        .get("foreground")
        .and_then(|foreground| foreground.get("unchanged"))
        .and_then(Value::as_bool);
    Ok(json!({
        "kind": "text-document",
        "state": "created",
        "path": path,
        "encoding": "utf-8",
        "mediaType": "text/plain",
        "text": text,
        "foreground": { "unchanged": foreground_unchanged },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Verb;

    fn create_request(input: Value) -> CommandRequest {
        let mut request = CommandRequest::read(Verb::Run, "app");
        request.operation = Some("create".to_owned());
        request
            .target
            .insert("sessionId".to_owned(), json!(application_session_id()));
        request.args.insert("input".to_owned(), input);
        request
    }

    #[test]
    fn application_session_publishes_text_create_capability() {
        let payload = session_payload();
        let session = &payload["sessions"][0];
        let id = session["sessionId"].as_str().unwrap_or_default();
        assert_eq!(session["kind"], "application");
        // Rust 必须发布与 C++ 文本创建器逐字节相同的 canonical 目标。
        assert_eq!(id, "s2:a:7b71b19b11d72b2d");
        assert!(!id.contains("notepad"));
        assert_eq!(
            session["capabilities"][0]["id"],
            capabilities::TEXT_DOCUMENT_CREATE
        );
        assert_eq!(session["capabilities"][0]["verb"], "create");
    }

    // 验证 provider 不再接受旧 s1 或未知 s2 创建器目标。
    #[test]
    // 该测试不探测或启动 Notepad，只验证纯身份门禁。
    fn only_current_s2_creator_identity_is_accepted() {
        // 接受当前 canonical 文本创建器目标。
        assert!(is_current_application_session_id(&application_session_id()));
        // 拒绝反向迁移前的旧 s1 目标。
        assert!(!is_current_application_session_id("s1:c3:7b71b19b11d72b2d"));
        // 拒绝同类别但指纹不匹配的 stale 目标。
        assert!(!is_current_application_session_id("s2:a:0000000000000000"));
    }

    #[test]
    fn provider_neutral_input_maps_to_the_existing_adapter() -> AppResult<()> {
        let delegated = delegated_create_request(&create_request(json!({ "text": "hello" })))?;
        assert_eq!(delegated.app, "notepad");
        assert_eq!(delegated.operation.as_deref(), Some("open-and-write-text"));
        assert_eq!(delegated.args.get("text"), Some(&json!("hello")));
        assert!(delegated.target.is_empty());
        Ok(())
    }

    #[test]
    fn public_result_hides_provider_process_and_native_details() -> AppResult<()> {
        let result = public_document_result(json!({
            "ok": true,
            "app": "notepad",
            "operation": "open-and-write-text",
            "path": "C:\\Temp\\note.txt",
            "launcherProcessId": 42,
            "text": "hello",
            "inputMethod": "file automation",
            "notepadPath": "C:\\Windows\\System32\\notepad.exe",
            "foreground": { "before": 7, "after": 7, "unchanged": true },
        }))?;
        let serialized = result.to_string();
        assert_eq!(result["path"], "C:\\Temp\\note.txt");
        assert_eq!(result["text"], "hello");
        assert!(!serialized.contains("notepad"));
        assert!(!serialized.contains("launcherProcessId"));
        assert!(!serialized.contains("inputMethod"));
        assert!(!serialized.contains("System32"));
        assert!(!serialized.contains("before"));
        assert!(!serialized.contains("after"));
        Ok(())
    }

    #[test]
    fn dispatch_rejects_unknown_capability_without_launching_notepad() {
        let error = match TextDocumentProvider.execute(
            "text.document.replace@1",
            &create_request(json!({ "text": "hello" })),
        ) {
            Ok(_) => panic!("unknown capability must fail before adapter dispatch"),
            Err(error) => error,
        };
        assert_eq!(error.code, "CAPABILITY_UNSUPPORTED");
    }

    #[test]
    fn dispatch_requires_provider_neutral_text_without_launching_notepad() {
        let error = match TextDocumentProvider.execute(
            capabilities::TEXT_DOCUMENT_CREATE,
            &create_request(json!({})),
        ) {
            Ok(_) => panic!("missing input must fail before adapter dispatch"),
            Err(error) => error,
        };
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }

    // 验证下层 Adapter 的不完整成功结果保持 provider 自有失败语义。
    #[test]
    fn public_result_rejects_missing_verified_document_evidence() {
        // 缺少 artifact path 必须失败闭合。
        let missing_path = public_document_result(
            // 仅提供已验证文本。
            json!({ "text": "hello" }),
        )
        // 不完整结果必须返回错误。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("missing artifact path must fail"));
        // 保持稳定执行失败码。
        assert_eq!(missing_path.code, "OPERATION_FAILED");
        // 保持既有 artifact path 消息。
        assert_eq!(
            missing_path.message,
            "The text document adapter returned no artifact path."
        );

        // 缺少 verified content 同样必须失败闭合。
        let missing_text = public_document_result(
            // 仅提供 artifact path。
            json!({ "path": "C:\\Temp\\note.txt" }),
        )
        // 不完整结果必须返回错误。
        .err()
        // 使用显式 panic 保留失败上下文。
        .unwrap_or_else(|| panic!("missing verified content must fail"));
        // 保持稳定执行失败码。
        assert_eq!(missing_text.code, "OPERATION_FAILED");
        // 保持既有 verified content 消息。
        assert_eq!(
            missing_text.message,
            "The text document adapter returned no verified content."
        );
    }
}
