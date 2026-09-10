//! 将公开 Browser Session 生命周期与页面操作连接到固定 Rust Broker。

// 导入 JSON 值与构造器。
use serde_json::{Value, json};

// 导入统一 provider 接口与 descriptor 构造器。
use crate::adapters::app::{CapabilityProvider, capability_descriptor};
// 导入 browser session identity、公开 capability、统一错误与 client Module。
use crate::{
    capabilities,
    components::browser_session_identity::{
        BrowserSessionIdentityShape, classify_browser_session_id,
    },
    domain::{AppControlError, AppResult, CommandRequest},
    modules::browser_session_client,
};

// 声明无字段 browser session provider，不持有 session、worker 或 broker 状态。
pub(super) struct BrowserSessionProvider;

// 保存本 provider 可执行的固定 Broker capability。
const BROWSER_SESSION_CAPABILITIES: &[&str] = &[
    // 关闭当前 live 会话。
    capabilities::BROWSER_SESSION_CLOSE,
    // 导航当前 live 会话并换发页面 identity。
    capabilities::BROWSER_PAGE_NAVIGATE,
    // 等待当前页面的有限条件。
    capabilities::BROWSER_PAGE_WAIT,
    // 查询当前页面的有界语义元素。
    capabilities::BROWSER_PAGE_QUERY,
    // 点击当前查询签发的精确元素。
    capabilities::BROWSER_ELEMENT_CLICK,
    // 向当前查询签发的精确元素输入文本。
    capabilities::BROWSER_ELEMENT_TYPE,
    // 捕获当前精确页面的有界 PNG。
    capabilities::BROWSER_PAGE_SCREENSHOT,
];

// 构造固定 Browser Session 与页面 descriptor。
fn browser_session_descriptors() -> Value {
    // 从单一运行时注册表投影公开 descriptor。
    json!([
        // 只允许 canonical browser session 关闭。
        capability_descriptor(
            // 使用稳定 close capability。
            capabilities::BROWSER_SESSION_CLOSE,
            // 不暴露 broker、worker 或 native 路由。
            json!({
                // 只接受独立 browser session identity。
                "target": "exact-browser-session",
                // 所有 lifecycle 路线使用同一总 deadline。
                "timeoutMs": { "min": 1, "max": 30_000, "default": 5_000 },
                // 固定 Rust broker，禁止回退。
                "fallback": "none",
            }),
        ),
        // 只允许点击当前 canonical page 签发的 canonical element。
        capability_descriptor(
            // 使用稳定元素点击 capability。
            capabilities::BROWSER_ELEMENT_CLICK,
            // 不暴露 worker ref、DOM 节点或 native identity。
            json!({
                // 顶层目标只接受当前 live Browser Session identity。
                "target": "exact-browser-session",
                // input 只接受当前公开页面 identity。
                "page": "current-exact-browser-page",
                // input 只接受当前页面签发的公开元素 identity。
                "element": "current-exact-browser-element",
                // 所有页面路线使用同一总 deadline。
                "timeoutMs": { "min": 1, "max": 30_000, "default": 5_000 },
                // 固定 Rust Broker，禁止回退。
                "fallback": "none",
            }),
        ),
        // 只允许向当前 canonical element 输入有界文本。
        capability_descriptor(
            // 使用稳定元素文本输入 capability。
            capabilities::BROWSER_ELEMENT_TYPE,
            // 不暴露文本、credential、worker ref 或 native identity。
            json!({
                // 顶层目标只接受当前 live Browser Session identity。
                "target": "exact-browser-session",
                // input 只接受当前公开页面 identity。
                "page": "current-exact-browser-page",
                // input 只接受当前页面签发的公开元素 identity。
                "element": "current-exact-browser-element",
                // UTF-8 文本必须非空且按字节有界。
                "utf8Bytes": { "min": 1, "max": 16_384 },
                // replace 必须由调用方显式给出。
                "replace": "required-boolean",
                // 所有页面路线使用同一总 deadline。
                "timeoutMs": { "min": 1, "max": 30_000, "default": 5_000 },
                // 固定 Rust Broker，禁止回退。
                "fallback": "none",
            }),
        ),
        // 只允许捕获当前 canonical page 的有界 PNG。
        capability_descriptor(
            // 使用稳定页面截图 capability。
            capabilities::BROWSER_PAGE_SCREENSHOT,
            // 不暴露路径、格式、质量或私有 CDP 参数。
            json!({
                // 顶层目标只接受当前 live Browser Session identity。
                "target": "exact-browser-session",
                // input 只接受当前公开页面 identity。
                "page": "current-exact-browser-page",
                // 输出格式固定为有界 PNG Base64。
                "result": "bounded-image-png-base64",
                // 所有页面路线使用同一总 deadline。
                "timeoutMs": { "min": 1, "max": 30_000, "default": 5_000 },
                // 固定 Rust Broker，禁止回退。
                "fallback": "none",
            }),
        ),
        // 只允许导航当前 canonical live Browser Session。
        capability_descriptor(
            // 使用稳定页面导航 capability。
            capabilities::BROWSER_PAGE_NAVIGATE,
            // 不暴露 URL、Broker、worker 或 native 路由。
            json!({
                // 顶层目标只接受当前 live Browser Session identity。
                "target": "exact-browser-session",
                // 成功换发新的 opaque 页面 identity 与代际。
                "result": "new-browser-page-generation",
                // 所有页面路线使用同一总 deadline。
                "timeoutMs": { "min": 1, "max": 30_000, "default": 5_000 },
                // 固定 Rust Broker，禁止回退。
                "fallback": "none",
            }),
        ),
        // 只允许等待当前 canonical page 的有限条件。
        capability_descriptor(
            // 使用稳定页面等待 capability。
            capabilities::BROWSER_PAGE_WAIT,
            // 不暴露私有 page ref 或查询语言。
            json!({
                // 顶层目标只接受当前 live Browser Session identity。
                "target": "exact-browser-session",
                // input 只接受当前公开页面 identity。
                "page": "current-exact-browser-page",
                // 只接受三类冻结条件。
                "conditions": ["document-ready", "element-present", "text-present"],
                // 所有页面路线使用同一总 deadline。
                "timeoutMs": { "min": 1, "max": 30_000, "default": 5_000 },
                // 固定 Rust Broker，禁止回退。
                "fallback": "none",
            }),
        ),
        // 只允许查询当前 canonical page 的 provider-neutral 语义元素。
        capability_descriptor(
            // 使用稳定页面查询 capability。
            capabilities::BROWSER_PAGE_QUERY,
            // 不暴露 CSS、XPath、脚本或 native selector。
            json!({
                // 顶层目标只接受当前 live Browser Session identity。
                "target": "exact-browser-session",
                // input 只接受当前公开页面 identity。
                "page": "current-exact-browser-page",
                // selector 只允许三个语义字符串和 exact。
                "selector": ["role", "name", "text", "exact"],
                // 固定公开结果上限。
                "maxResults": { "min": 1, "max": 100, "default": 100 },
                // 所有页面路线使用同一总 deadline。
                "timeoutMs": { "min": 1, "max": 30_000, "default": 5_000 },
                // 固定 Rust Broker，禁止回退。
                "fallback": "none",
            }),
        ),
    ])
}

// 从请求读取唯一公开 session ID。
fn required_session_id(request: &CommandRequest) -> AppResult<&str> {
    // 只接受 facade 固定 target 字段。
    request
        // 读取公开 target。
        .target
        // 读取 sessionId。
        .get("sessionId")
        // 只接受非空字符串。
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        // 不向调用方暴露 provider 细节。
        .ok_or_else(|| AppControlError::new("INVALID_ARGUMENT", "target.sessionId is required."))
}

// 构造不含 broker 私有事实的 stale 错误。
fn stale_session() -> AppControlError {
    // 使用已登记公开错误码。
    AppControlError::new(
        // 将非 canonical 或已关闭身份统一为 stale。
        "STALE_SESSION",
        // 不区分 identity 形状或 broker generation。
        "The browser session is stale or no longer available.",
    )
}

// 为 browser session 实现无状态统一 provider。
impl CapabilityProvider for BrowserSessionProvider {
    // 返回只用于内部聚合的稳定 provider 名称。
    fn provider_id(&self) -> &'static str {
        // 该值不进入公共 response。
        "browser-session"
    }

    // 返回唯一 close capability。
    fn capabilities(&self) -> &'static [&'static str] {
        // 使用静态闭合集合。
        BROWSER_SESSION_CAPABILITIES
    }

    // 通过只读 fixed broker inspect 判断 session 是否仍属于当前代际。
    fn accepts_session(&self, session_id: &str) -> AppResult<bool> {
        // 复用单次 freshness 路由。
        self.session_capabilities(session_id)
            // Some 表示本 provider 当前唯一拥有目标。
            .map(|capabilities| capabilities.is_some())
    }

    // 报告 provider 可用，不探测或创建任何 browser session。
    fn status(&self) -> AppResult<Value> {
        // 无状态 provider 不持有 runtime 或 transport 诊断。
        Ok(json!({ "ok": true, "scope": "browser-session-and-page" }))
    }

    // browser session 没有公开枚举能力，禁止扫描或伪造 session inventory。
    fn sessions(&self, _: &CommandRequest) -> AppResult<Value> {
        // 只返回空集合。
        Ok(json!({ "ok": true, "sessions": [] }))
    }

    // 只读 inspect 通过固定 broker 建立当前 live 事实。
    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        // 读取调用方目标。
        let session_id = required_session_id(request)?;
        // 重新认证 session，stale 不得成为成功 inspect。
        let Some(_) = self.session_capabilities(session_id)? else {
            // 保持统一 stale 结果。
            return Err(stale_session());
        };
        // 只投影公开 identity、live 和固定 descriptor。
        Ok(json!({
            // 回显调用方已知 session。
            "sessionId": session_id,
            // 标记独立公开类别。
            "kind": "browser-session",
            // inspect 只在当前 registry live 时成功。
            "state": "live",
            // 返回当前固定生命周期与页面 descriptor。
            "capabilities": browser_session_descriptors(),
        }))
    }

    // 执行 Browser Session 生命周期或页面操作，不复制领域规则。
    fn execute(&self, capability: &str, request: &CommandRequest) -> AppResult<Value> {
        // 读取唯一允许的 Browser Session target。
        let session_id = required_session_id(request)?;
        // 按 registry capability 选择同一 client Module 的封闭入口。
        match capability {
            // close 保持 confirmation-first 与固定回收语义。
            capabilities::BROWSER_SESSION_CLOSE => browser_session_client::close(
                // 传播 Policy 已审核的 confirmation。
                request.confirmed,
                // 传递严格公开 input。
                request.args.get("input"),
                // 传递公开 session identity。
                session_id,
            ),
            // navigate 保持 confirmation-first 且只经固定 Broker。
            capabilities::BROWSER_PAGE_NAVIGATE => browser_session_client::navigate(
                // 传播 Policy 已审核的 confirmation。
                request.confirmed,
                // 传递严格页面导航 input。
                request.args.get("input"),
                // 传递公开 session identity。
                session_id,
            ),
            // wait 是无隐藏副作用的 Query。
            capabilities::BROWSER_PAGE_WAIT => browser_session_client::wait(
                // 传递严格页面等待 input。
                request.args.get("input"),
                // 传递公开 session identity。
                session_id,
            ),
            // query 是无隐藏副作用的有界 Query。
            capabilities::BROWSER_PAGE_QUERY => browser_session_client::query(
                // 传递严格页面查询 input。
                request.args.get("input"),
                // 传递公开 session identity。
                session_id,
            ),
            // click 是 confirmation-first 且禁止自动重派的 mutation。
            capabilities::BROWSER_ELEMENT_CLICK => browser_session_client::click(
                // 传播 Policy 已审核的 confirmation。
                request.confirmed,
                // 传递严格元素点击 input。
                request.args.get("input"),
                // 传递公开 session identity。
                session_id,
            ),
            // type 是不回显原文且禁止自动重派的 mutation。
            capabilities::BROWSER_ELEMENT_TYPE => browser_session_client::type_text(
                // 传播 Policy 已审核的 confirmation。
                request.confirmed,
                // 传递严格元素输入 input。
                request.args.get("input"),
                // 传递公开 session identity。
                session_id,
            ),
            // screenshot 是无隐藏副作用的有界 Query。
            capabilities::BROWSER_PAGE_SCREENSHOT => browser_session_client::screenshot(
                // 传递严格页面截图 input。
                request.args.get("input"),
                // 传递公开 session identity。
                session_id,
            ),
            // 其他 capability 不属于本 provider。
            _ => Err(AppControlError::new(
                // 使用公开 capability 缺口码。
                "CAPABILITY_UNSUPPORTED",
                // 不列出内部路由或 provider 集合。
                "The browser session provider does not support this capability.",
            )),
        }
    }

    // 覆盖默认枚举实现，避免无状态 provider 从 sessions 伪推 session。
    fn session_capabilities(&self, session_id: &str) -> AppResult<Option<Vec<String>>> {
        // 非 canonical browser identity 不触碰 broker。
        if classify_browser_session_id(session_id) != BrowserSessionIdentityShape::Canonical {
            // 该 provider 不接受其他 opaque target。
            return Ok(None);
        }
        // 固定 broker inspect 成功才证明当前 live。
        match browser_session_client::inspect_session(
            // 只传公开 session。
            session_id,
            // assessment 与 provider 共用固定默认 deadline。
            browser_session_client::DEFAULT_TIMEOUT_MS,
        ) {
            // live session 发布全部固定 Broker capability。
            Ok(()) => Ok(Some(
                // 复制静态字符串，避免 provider 保存状态。
                BROWSER_SESSION_CAPABILITIES
                    .iter()
                    .map(|capability| (*capability).to_owned())
                    .collect(),
            )),
            // stale 表示当前 provider 不命中。
            Err(error) if error.code == "STALE_SESSION" => Ok(None),
            // transport 或认证问题不能伪装成 stale。
            Err(error) => Err(error),
        }
    }
}

// 验证无状态 provider 不把普通 opaque ID 带入 broker。
#[cfg(test)]
mod tests {
    // 导入稳定 capability ID。
    use crate::capabilities;
    // 导入 provider trait。
    use crate::adapters::app::CapabilityProvider;

    // 导入被测 provider。
    use super::BrowserSessionProvider;

    // 验证 browser_session_public_route 非浏览器目标不访问 broker。
    #[test]
    fn browser_session_public_route_rejects_non_browser_session_without_broker() {
        // 构造无状态 provider。
        let provider = BrowserSessionProvider;
        // 通用窗口 target 不得作为浏览器 session 解析。
        assert_eq!(
            // 请求能力集合。
            provider
                .session_capabilities("s2:w:0123456789abcdef")
                // 非 browser identity 不应失败。
                .expect("non-browser target must not call broker"),
            // 只返回无命中。
            None,
        );
    }

    // 验证 browser_session_public_route provider 自身不持有 discovery 状态。
    #[test]
    fn browser_session_public_route_does_not_enumerate_sessions() {
        // 构造无状态 provider。
        let provider = BrowserSessionProvider;
        // 读取 status，不触发 broker。
        let status = provider.status().expect("status must be local");
        // 不得发布 provider 私有 key。
        assert_eq!(status["scope"], "browser-session-and-page");
    }

    // 验证 provider 逐字发布三项新增页面动作。
    #[test]
    fn browser_session_public_route_provider_publishes_page_actions() {
        // 构造无状态 provider。
        let provider = BrowserSessionProvider;
        // 读取组装期固定 capability 集合。
        let published = provider.capabilities();
        // click 必须由唯一 Browser Session provider 发布。
        assert!(published.contains(&capabilities::BROWSER_ELEMENT_CLICK));
        // type 必须由同一 provider 发布。
        assert!(published.contains(&capabilities::BROWSER_ELEMENT_TYPE));
        // screenshot 必须由同一 provider 发布。
        assert!(published.contains(&capabilities::BROWSER_PAGE_SCREENSHOT));
        // provider 集合必须保持七项固定 Broker capability。
        assert_eq!(published.len(), 7);
    }
}
