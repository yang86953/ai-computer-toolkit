//! 将 legacy Notepad surface 映射到 Rust Text Document Module。

// 导入语言中立 JSON 构造器。
use serde_json::json;

// 引入 Notepad Adapter 私有封闭错误码实现。
#[path = "notepad_error.rs"]
mod error_code;

// 导入 Notepad Adapter 私有封闭错误码。
use error_code::AppNotepadErrorCode;

// 导入适配器契约、请求结果类型与文本创建 Module。
use crate::{
    // 导入统一适配器 trait。
    adapters::AppAdapter,
    // 导入语言中立请求与结果类型。
    domain::{AppResult, CommandRequest},
    // 导入文本创建领域 Module。
    modules::text_document,
};

// 定义无状态 Notepad 兼容适配器。
pub struct NotepadAdapter;

impl AppAdapter for NotepadAdapter {
    fn app_id(&self) -> &'static str {
        "notepad"
    }

    fn status(&self) -> AppResult<serde_json::Value> {
        Ok(json!({
            "ok": true,
            "app": self.app_id(),
            "backend": "atomic UTF-8 document write + system notepad.exe",
            "backgroundPolicy": "guaranteed",
            // 只发布固定 runtime 是否可达，不泄漏系统路径。
            "runtimeDetected": text_document::runtime_path().is_ok(),
            "certifiedOperations": ["open-and-write-text"],
        }))
    }

    fn sessions(&self, _: &CommandRequest) -> AppResult<serde_json::Value> {
        // 明确拒绝附着用户现有 Notepad 会话。
        Err(AppNotepadErrorCode::BackgroundOperationUnavailable.error(
            // 保持公开会话边界消息逐字不变。
            "Notepad 仅创建并打开本次任务的文档，不附着用户现有会话。",
        ))
    }

    fn inspect(&self, _: &CommandRequest) -> AppResult<serde_json::Value> {
        // 明确拒绝观察或修改用户现有文档。
        Err(AppNotepadErrorCode::BackgroundOperationUnavailable.error(
            // 保持公开检查边界消息逐字不变。
            "Notepad 当前未认证对用户现有文档的检查或修改。",
        ))
    }

    fn run(&self, request: &CommandRequest) -> AppResult<serde_json::Value> {
        match request.operation.as_deref() {
            Some("open-and-write-text") => text_document::create_and_open(request),
            Some(operation) => Err(AppNotepadErrorCode::BackgroundOperationUnavailable.error(
                // 仅回显调用方提供的 operation 身份。
                format!("notepad.{operation} 未认证为后台操作。"),
            )),
            None => Err(AppNotepadErrorCode::InvalidArgument.error(
                // 保持必需字段消息逐字不变。
                "run 缺少 operation。",
            )),
        }
    }
}

// 声明不启动真实 Notepad 的 Adapter 门禁测试。
#[cfg(test)]
mod tests {
    // 导入 Adapter trait、请求与动词类型。
    use crate::{
        // 导入统一 Adapter trait 以调用被测接口。
        adapters::AppAdapter,
        // 导入语言中立请求与动词。
        domain::{CommandRequest, Verb},
    };

    // 导入被测 Notepad Adapter。
    use super::NotepadAdapter;

    // 验证现有会话观察门禁保持稳定且不访问真实应用。
    #[test]
    fn notepad_observation_gates_keep_stable_errors() {
        // 构造不含目标的会话请求。
        let sessions_request = CommandRequest::read(Verb::Sessions, "notepad");
        // sessions 必须固定拒绝附着现有会话。
        let sessions_error = NotepadAdapter
            // 调用纯 Adapter 门禁。
            .sessions(&sessions_request)
            // 拒绝路径不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("Notepad sessions must remain unavailable"));
        // 保持稳定后台操作缺口码。
        assert_eq!(sessions_error.code, "BACKGROUND_OPERATION_UNAVAILABLE");
        // 保持既有会话边界消息。
        assert_eq!(
            sessions_error.message,
            "Notepad 仅创建并打开本次任务的文档，不附着用户现有会话。"
        );

        // 构造不含目标的 inspect 请求。
        let inspect_request = CommandRequest::read(Verb::Inspect, "notepad");
        // inspect 必须固定拒绝现有文档访问。
        let inspect_error = NotepadAdapter
            // 调用纯 Adapter 门禁。
            .inspect(&inspect_request)
            // 拒绝路径不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("Notepad inspect must remain unavailable"));
        // 保持稳定后台操作缺口码。
        assert_eq!(inspect_error.code, "BACKGROUND_OPERATION_UNAVAILABLE");
        // 保持既有检查边界消息。
        assert_eq!(
            inspect_error.message,
            "Notepad 当前未认证对用户现有文档的检查或修改。"
        );
    }

    // 验证 operation 路由门禁在真实文本创建前保持稳定。
    #[test]
    fn notepad_run_gates_fail_before_real_application_access() {
        // 构造无 operation 的基础 run 请求。
        let missing_request = CommandRequest::read(Verb::Run, "notepad");
        // 缺失 operation 必须在文本创建 Module 前失败。
        let missing_error = NotepadAdapter
            // 调用纯 Adapter 路由门禁。
            .run(&missing_request)
            // 缺失 operation 不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("missing Notepad operation must fail"));
        // 保持稳定参数错误码。
        assert_eq!(missing_error.code, "INVALID_ARGUMENT");
        // 保持既有必需字段消息。
        assert_eq!(missing_error.message, "run 缺少 operation。");

        // 克隆请求并提供未认证 operation。
        let mut unsupported_request = missing_request;
        // 使用不会进入文本创建 Module 的合成 operation。
        unsupported_request.operation = Some("replace-existing-text".to_owned());
        // 未认证 operation 必须在真实应用访问前失败。
        let unsupported_error = NotepadAdapter
            // 调用纯 Adapter 路由门禁。
            .run(&unsupported_request)
            // 未认证 operation 不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("unsupported Notepad operation must fail"));
        // 保持稳定后台操作缺口码。
        assert_eq!(unsupported_error.code, "BACKGROUND_OPERATION_UNAVAILABLE");
        // 保持仅回显 operation 身份的既有消息。
        assert_eq!(
            unsupported_error.message,
            "notepad.replace-existing-text 未认证为后台操作。"
        );
    }
}
