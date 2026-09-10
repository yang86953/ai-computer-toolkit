//! 将 legacy browser surface 映射到 Rust Browser Screenshot Module。

// 导入语言中立 JSON 构造器。
use serde_json::json;

// 引入 Browser Adapter 私有封闭错误码实现。
#[path = "browser_error.rs"]
mod error_code;

// 导入 Browser Adapter 私有封闭错误码。
use error_code::AppBrowserErrorCode;

// 导入适配器契约、浏览器 runtime 探测、请求与领域 Module。
use crate::{
    // 导入统一适配器 trait。
    adapters::AppAdapter,
    // 导入固定 Chromium runtime Component 与 worker 可达性 Component。
    components::{browser_runtime, worker_process},
    // 导入语言中立请求与结果类型。
    domain::{AppResult, CommandRequest},
    // 导入浏览器截图领域 Module。
    modules::browser_screenshot,
};

// 固定 Rust 浏览器截图 companion 文件名。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-browser-worker.exe";

// 定义无状态 browser 兼容适配器。
pub struct BrowserAdapter;

// 实现 legacy browser surface。
impl AppAdapter for BrowserAdapter {
    // 返回固定 surface ID。
    fn app_id(&self) -> &'static str {
        // 保持公开 browser 名称。
        "browser"
    }

    // 只读报告 runtime 与隔离 worker 可达性。
    fn status(&self) -> AppResult<serde_json::Value> {
        // 私下探测认证 Chromium runtime。
        let runtime_detected = browser_runtime::find().is_some();
        // 私下探测固定 sibling worker。
        let worker_available = worker_process::sibling_companion_available(WORKER_FILE_NAME);
        // 返回不含路径、PID 或 profile 的安全状态。
        Ok(json!({
            // 标记状态查询成功。
            "ok": true,
            // 回显固定 surface。
            "app": self.app_id(),
            // 声明 Rust 隔离实现。
            "implementation": "rust-isolated-worker",
            // 标记 status 本身无副作用。
            "readOnly": true,
            // 声明固定 headless 后台策略。
            "backgroundPolicy": "guaranteed",
            // 只有 runtime 与 worker 同时存在才可执行。
            "available": runtime_detected && worker_available,
            // 公开布尔 runtime 事实。
            "runtimeDetected": runtime_detected,
            // 公开布尔 worker 事实。
            "workerAvailable": worker_available,
            // 明确不泄漏 runtime 路径。
            "runtimePathExposed": false,
            // 明确不返回原生标识。
            "nativeIdentifiersExposed": false,
            // 声明禁止触碰的用户浏览器资源。
            "blocked": ["user profile", "user tabs", "visible browser launch"],
        }))
    }

    // browser 不附着用户会话。
    fn sessions(&self, _: &CommandRequest) -> AppResult<serde_json::Value> {
        // 返回空会话投影。
        Ok(json!({
            // 标记查询成功。
            "ok": true,
            // 回显固定 surface。
            "app": self.app_id(),
            // 固定空数量。
            "count": 0,
            // 固定空集合。
            "sessions": [],
            // 解释隔离边界。
            "note": "browser 不附着用户浏览器，因此没有可列出的用户会话。",
        }))
    }

    // browser v1 不提供 DOM 检查。
    fn inspect(&self, _: &CommandRequest) -> AppResult<serde_json::Value> {
        // 返回明确 capability 缺口。
        Err(AppBrowserErrorCode::BackgroundOperationUnavailable.error(
            // 不暗示会附着用户浏览器。
            "browser v1 仅认证 screenshot；DOM 检查不在当前公开能力内。",
        ))
    }

    // 将唯一认证写操作委托给领域 Module。
    fn run(&self, request: &CommandRequest) -> AppResult<serde_json::Value> {
        // 按封闭 operation 分派。
        match request.operation.as_deref() {
            // 浏览器截图只通过 Rust 隔离 Module。
            Some("screenshot") => browser_screenshot::screenshot(
                // 重复传递逐操作确认供 Module 独立核对。
                request.confirmed,
                // 传递封闭目标对象。
                &request.target,
                // 传递封闭参数对象。
                &request.args,
            ),
            // 其他 operation 不在认证集合。
            Some(operation) => Err(AppBrowserErrorCode::BackgroundOperationUnavailable.error(
                // 只回显 operation 身份。
                format!("browser.{operation} 未认证为后台操作。"),
            )),
            // 缺失 operation 按参数错误处理。
            None => Err(AppBrowserErrorCode::InvalidArgument.error(
                // 说明必需字段。
                "run 缺少 operation。",
            )),
        }
    }
}

// 声明不访问 runtime、worker 或真实浏览器的 Adapter 门禁测试。
#[cfg(test)]
mod tests {
    // 导入 Adapter trait、请求与动词类型。
    use crate::{
        // 导入统一 Adapter trait 以调用被测接口。
        adapters::AppAdapter,
        // 导入语言中立请求与动词。
        domain::{CommandRequest, Verb},
    };

    // 导入被测 Browser Adapter。
    use super::BrowserAdapter;

    // 验证 Browser Adapter 自有门禁在 runtime 与 worker 访问前保持稳定。
    #[test]
    fn browser_owned_gates_fail_before_runtime_or_worker_access() {
        // 构造不含目标的 inspect 请求。
        let inspect_request = CommandRequest::read(Verb::Inspect, "browser");
        // DOM inspect 必须固定拒绝。
        let inspect_error = BrowserAdapter
            // 调用纯 Adapter 门禁。
            .inspect(&inspect_request)
            // 拒绝路径不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("Browser inspect must remain unavailable"));
        // 保持稳定后台操作缺口码。
        assert_eq!(inspect_error.code, "BACKGROUND_OPERATION_UNAVAILABLE");
        // 保持既有 DOM 检查缺口消息。
        assert_eq!(
            inspect_error.message,
            "browser v1 仅认证 screenshot；DOM 检查不在当前公开能力内。"
        );

        // 构造无 operation 的基础 run 请求。
        let missing_request = CommandRequest::read(Verb::Run, "browser");
        // 缺失 operation 必须在 Browser Screenshot Module 前失败。
        let missing_error = BrowserAdapter
            // 调用纯 Adapter 路由门禁。
            .run(&missing_request)
            // 缺失 operation 不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("missing Browser operation must fail"));
        // 保持稳定参数错误码。
        assert_eq!(missing_error.code, "INVALID_ARGUMENT");
        // 保持既有必需字段消息。
        assert_eq!(missing_error.message, "run 缺少 operation。");

        // 克隆请求并提供未认证 operation。
        let mut unsupported_request = missing_request;
        // 使用不会进入截图 Module 的合成 operation。
        unsupported_request.operation = Some("dom-read".to_owned());
        // 未认证 operation 必须在 runtime 和 worker 访问前失败。
        let unsupported_error = BrowserAdapter
            // 调用纯 Adapter 路由门禁。
            .run(&unsupported_request)
            // 未认证 operation 不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("unsupported Browser operation must fail"));
        // 保持稳定后台操作缺口码。
        assert_eq!(unsupported_error.code, "BACKGROUND_OPERATION_UNAVAILABLE");
        // 保持仅回显 operation 身份的既有消息。
        assert_eq!(
            unsupported_error.message,
            "browser.dom-read 未认证为后台操作。"
        );
    }
}
