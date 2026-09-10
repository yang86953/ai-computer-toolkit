mod desktop;
mod facade;
// 把 browser session close 的无查询选择规则拆离 facade 主文件。
mod facade_browser_session;
// Browser Session facade 错误阶段投影保持独立窄边界。
mod facade_browser_session_error;
// 把 canonical stale 目标的独占 provider 选择规则拆离 facade 主文件。
mod facade_stale_session;
mod host;
// 注册无状态 browser session close 与 freshness provider。
mod browser_session;
// 注册通用进程生命周期 provider。
mod process;
mod session;
// 注册 Standard Edit 精确控件 provider。
mod standard_edit;
mod text;
// 注册窗口状态与几何生命周期 app 路由。
mod window_lifecycle;

use serde_json::Value;

use crate::domain::{AppResult, CommandRequest, Verb};

pub use facade::AppFacadeAdapter;
// 只重导出当前 s2 provider 共用的 capability 描述生成器。
pub(crate) use session::capability_descriptor;

pub(crate) trait CapabilityProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    fn capabilities(&self) -> &'static [&'static str];
    fn accepts_session(&self, session_id: &str) -> AppResult<bool>;
    fn status(&self) -> AppResult<Value>;
    fn sessions(&self, request: &CommandRequest) -> AppResult<Value>;
    fn inspect(&self, request: &CommandRequest) -> AppResult<Value>;
    fn execute(&self, capability: &str, request: &CommandRequest) -> AppResult<Value>;

    fn session_capabilities(&self, session_id: &str) -> AppResult<Option<Vec<String>>> {
        if !self.accepts_session(session_id)? {
            return Ok(None);
        }
        let mut discovery = CommandRequest::read(Verb::Sessions, "app");
        discovery.max_items = 10_000;
        let result = self.sessions(&discovery)?;
        session::capability_ids_for_session(&result, session_id)
    }
}
