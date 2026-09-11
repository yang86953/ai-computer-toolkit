//! MCP（Model Context Protocol）stdio 控制面。
//!
//! 公开控制面只有这一条：标准 MCP JSON-RPC 服务，工具直接操作前台桌面。
//! 这里不提供后台隔离路线，也不把应用级脚本执行当作键鼠操作的替代品。

mod broker;
// 供 Windows owner-only 目录组件测试接入真实目录名生成器做同一生产校验。
pub(crate) mod desktop;
mod failure;
mod server;
mod tools;

pub use server::run_stdio;
pub use tools::tool_catalog;

#[cfg(test)]
mod tests;
