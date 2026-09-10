//! Windows 动态已安装应用 provider。

// 导入 JSON 值与构造器。
use serde_json::{Value, json};

// 引入 host provider 私有封闭错误码实现。
#[path = "host_error.rs"]
mod error_code;

// 导入 host provider 私有封闭错误码。
use error_code::AppHostErrorCode;

// 导入 capability、动态启动 Module 与公共边界。
use crate::{
    // 导入 provider trait 与主机身份 Component。
    adapters::{
        // 导入 capability provider 抽象。
        app::CapabilityProvider,
        // 导入当前 Windows host opaque 身份。
        windows::current_host_session_id,
    },
    // 导入稳定 application 与 browser session 生命周期 ID。
    capabilities,
    // 导入请求与结构化结果边界。
    domain::{AppResult, CommandRequest},
    // 导入动态应用启动与 browser session client Module。
    modules::{application_launch, browser_session_client},
};

// 声明无状态 Windows host provider。
pub(super) struct HostProvider;

// 保存 provider 可执行的 host 与动态应用 capability。
const HOST_CAPABILITIES: &[&str] = &[
    // 只允许动态应用启动。
    capabilities::APPLICATION_OPEN,
    // 只允许当前 host 创建 browser session。
    capabilities::BROWSER_SESSION_OPEN,
];

// 构造当前 host 唯一发布的 browser session open descriptor。
fn browser_session_open_descriptor() -> Value {
    // 从单一注册表投影公开 descriptor。
    super::capability_descriptor(
        // 使用稳定 browser session open capability。
        capabilities::BROWSER_SESSION_OPEN,
        // 不暴露固定 broker 的私有连接事实。
        json!({
            // open 只接受当前 host 身份。
            "target": "current-host",
            // 固定总 deadline 公开范围。
            "timeoutMs": { "min": 1, "max": 30_000, "default": 5_000 },
            // 固定 Rust 路线不允许回退。
            "fallback": "none",
        }),
    )
}

// 构造不含静态应用白名单的公开主机 session。
fn public_host_session() -> Value {
    // 只输出 host 观察身份，不把启动能力错误绑定到 host。
    json!({
        // 输出当前 canonical s2:h。
        "sessionId": host_session_id(),
        // 输出稳定 host 类别。
        "kind": "host",
        // 输出普通显示标题。
        "title": "Windows application host",
        // 输出当前可用状态。
        "state": "available",
        // browser open 只发布在当前精确 host target。
        "capabilities": [browser_session_open_descriptor()],
    })
}

// 为动态已安装应用实现统一 provider 接口。
impl CapabilityProvider for HostProvider {
    // 返回内部 provider 名称。
    fn provider_id(&self) -> &'static str {
        // 名称只用于进程内状态组合。
        "windows-host"
    }

    // 返回本 provider 可能执行的 capability 集合。
    fn capabilities(&self) -> &'static [&'static str] {
        // 返回动态 application open 与 host browser open。
        HOST_CAPABILITIES
    }

    // 判断当前 session 是否属于 host 或动态应用目录。
    fn accepts_session(&self, session_id: &str) -> AppResult<bool> {
        // 当前 host session 始终属于本 provider。
        if session_id == host_session_id() {
            // 返回唯一 host 命中。
            return Ok(true);
        }
        // 动态应用只在当前目录重新发现后命中。
        application_launch::session_capabilities(session_id)
            // 把可选 capability 集合映射为 provider 命中。
            .map(|capabilities| capabilities.is_some())
    }

    // 返回动态目录与认证计数状态。
    fn status(&self) -> AppResult<Value> {
        // 由领域 Module 统一读取动态来源。
        application_launch::status()
    }

    // 返回兼容 app 聚合中的 host 观察 session。
    fn sessions(&self, _: &CommandRequest) -> AppResult<Value> {
        // 动态应用清单由 discover app 发布，避免在 sessions app 中复制来源。
        Ok(json!({
            // 标记 provider 读取成功。
            "ok": true,
            // 只返回当前 host session。
            "sessions": [public_host_session()],
        }))
    }

    // 精确检查 host 或动态应用。
    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        // 读取调用方的唯一 sessionId。
        let session_id = request
            // 访问 provider-neutral 目标对象。
            .target
            // 读取固定目标字段。
            .get("sessionId")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 拒绝空值。
            .filter(|value| !value.is_empty())
            // 缺失目标返回稳定参数错误。
            .ok_or_else(|| {
                // 构造不含 provider 细节的错误。
                AppHostErrorCode::InvalidArgument.error("target.sessionId is required.")
            })?;
        // host 精确检查复用聚合状态。
        if session_id == host_session_id() {
            // 返回动态目录状态。
            return self.status();
        }
        // 应用目标交给动态目录 Module。
        application_launch::inspect(session_id)
    }

    // 执行确认后的动态精确应用启动。
    fn execute(&self, capability: &str, request: &CommandRequest) -> AppResult<Value> {
        // host browser open 不读取动态应用目录。
        if capability == capabilities::BROWSER_SESSION_OPEN {
            // 读取 facade 已唯一解析的当前 host target。
            let session_id = request
                // 访问 provider-neutral target。
                .target
                // 读取固定 sessionId。
                .get("sessionId")
                // 只接受字符串。
                .and_then(Value::as_str)
                // 拒绝空目标。
                .filter(|value| !value.is_empty())
                // 缺失目标返回参数错误。
                .ok_or_else(|| {
                    // 构造稳定参数错误。
                    AppHostErrorCode::InvalidArgument.error("target.sessionId is required.")
                })?;
            // 仅在 confirmation 之后防御性重验当前 host，保持 confirmation-first。
            if request.confirmed && session_id != host_session_id() {
                // 不把其他 canonical 或非 canonical target 交给 broker。
                return Err(AppHostErrorCode::InvalidArgument
                    .error("browser.session.open@1 requires the current host session."));
            }
            // Client Module 保持 confirmation-first 与严格 input/broker 路由。
            return browser_session_client::open(
                // 传播 facade 已认证 confirmation。
                request.confirmed,
                // 传递唯一公开 lifecycle input。
                request.args.get("input"),
                // 传递调用方已知 host target。
                session_id,
            );
        }
        // 只接受既有动态 application.open。
        if capability != capabilities::APPLICATION_OPEN {
            // 其他能力稳定拒绝。
            return Err(AppHostErrorCode::CapabilityUnsupported.error(
                // 不列出内部 provider 路由。
                "The Windows application provider supports application.open@1 and browser.session.open@1 only.",
            ));
        }
        // 读取 facade 已唯一解析的精确应用目标。
        let session_id = request
            // 访问 provider-neutral target。
            .target
            // 读取固定 sessionId。
            .get("sessionId")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 拒绝空目标。
            .filter(|value| !value.is_empty())
            // 缺失目标返回参数错误。
            .ok_or_else(|| {
                // 构造稳定参数错误。
                AppHostErrorCode::InvalidArgument.error("target.sessionId is required.")
            })?;
        // 由 Module 重新发现并调度 Shell Component。
        application_launch::execute(session_id, request)
    }

    // 避免默认实现通过 host-only sessions 二次推断动态应用能力。
    fn session_capabilities(&self, session_id: &str) -> AppResult<Option<Vec<String>>> {
        // host 只发布 browser session open，不发布动态 application capability。
        if session_id == host_session_id() {
            // 返回当前 host 固定发布的唯一 lifecycle capability。
            return Ok(Some(vec![capabilities::BROWSER_SESSION_OPEN.to_owned()]));
        }
        // 应用能力来自当前动态目录的单独认证。
        application_launch::session_capabilities(session_id)
    }
}

// 返回当前 Windows 登录会话的 canonical host 身份。
fn host_session_id() -> String {
    // 复用跨实现稳定 Component。
    current_host_session_id()
}

// 声明不执行启动的 host 路由测试。
#[cfg(test)]
mod tests {
    // 导入 capability、provider trait 与请求类型。
    use crate::{
        // 导入 provider trait。
        adapters::app::CapabilityProvider,
        // 导入正式动态应用启动 capability。
        capabilities,
        // 导入请求、结果与动词类型。
        domain::{AppResult, CommandRequest, Verb},
    };

    // 导入当前 host provider 与公开投影。
    use super::{HostProvider, host_session_id, public_host_session};

    // 验证当前主机身份 canonical 且稳定。
    #[test]
    fn host_session_is_canonical_and_stable() {
        // 第一次读取当前 host。
        let first = host_session_id();
        // 第二次重新读取同一 host。
        let second = host_session_id();
        // 同一登录会话内必须稳定。
        assert_eq!(first, second);
        // 必须使用 canonical s2:h。
        assert!(first.starts_with("s2:h:"));
        // 必须保持固定摘要长度。
        assert_eq!(first.len(), 21);
    }

    // 验证旧 host 目标被拒绝。
    #[test]
    fn provider_rejects_legacy_and_stale_host_sessions() -> AppResult<()> {
        // 读取当前 host。
        let current = host_session_id();
        // 当前身份必须命中。
        assert!(HostProvider.accepts_session(&current)?);
        // 旧 s1 身份必须拒绝。
        assert!(!HostProvider.accepts_session("s1:c1:0000000000000000")?);
        // 未知 canonical host 必须拒绝。
        assert!(!HostProvider.accepts_session("s2:h:0000000000000000")?);
        // 返回测试成功。
        Ok(())
    }

    // 验证 host 只发布 browser session open 而不发布动态应用启动。
    #[test]
    fn browser_session_public_route_host_publishes_open_only() {
        // 构造公开 host session。
        let session = public_host_session();
        // host 只发布 browser session open descriptor。
        assert_eq!(
            // 读取唯一 descriptor 的稳定 ID。
            session["capabilities"][0]["id"],
            // 不得发布动态 application open。
            capabilities::BROWSER_SESSION_OPEN,
        );
        // descriptor 集合必须保持唯一。
        assert_eq!(session["capabilities"].as_array().map(Vec::len), Some(1));
        // 禁止恢复 applicationIds 字段。
        assert!(session.get("applicationIds").is_none());
    }

    // 验证 capability 与目标门禁在动态应用访问前保持固定顺序。
    #[test]
    fn provider_gates_fail_before_dynamic_application_access() {
        // 构造不含目标的基础执行请求。
        let request = CommandRequest::read(Verb::Run, "app");
        // 未登记 capability 必须最先失败。
        let unsupported = HostProvider
            // 使用合成未知 capability，禁止进入动态应用 Module。
            .execute("application.close@1", &request)
            // 未登记 capability 不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("unknown capability must fail"));
        // 保持稳定 capability 缺口码。
        assert_eq!(unsupported.code, "CAPABILITY_UNSUPPORTED");
        // 保持既有 capability 缺口消息。
        assert_eq!(
            unsupported.message,
            "The Windows application provider supports application.open@1 and browser.session.open@1 only."
        );

        // inspect 缺少目标时必须在动态目录访问前失败。
        let inspect_error = HostProvider
            // 直接检查无目标请求。
            .inspect(&request)
            // 缺失目标不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("missing inspect target must fail"));
        // 保持稳定参数错误码。
        assert_eq!(inspect_error.code, "INVALID_ARGUMENT");
        // 保持既有目标缺失消息。
        assert_eq!(inspect_error.message, "target.sessionId is required.");

        // 有效 capability 缺少目标时同样必须在启动前失败。
        let execute_error = HostProvider
            // 使用正式 capability 与无目标请求。
            .execute(capabilities::APPLICATION_OPEN, &request)
            // 缺失目标不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("missing execute target must fail"));
        // 保持稳定参数错误码。
        assert_eq!(execute_error.code, "INVALID_ARGUMENT");
        // 保持既有目标缺失消息。
        assert_eq!(execute_error.message, "target.sessionId is required.");
    }
}
