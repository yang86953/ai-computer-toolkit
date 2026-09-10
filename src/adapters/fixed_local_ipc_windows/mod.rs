//! 为固定本机 broker 提供 Windows message pipe 与 peer 身份认证 Adapter。

// 导出固定进程、主体、完整性与 session 认证 Component。
pub(crate) mod identity;
// 导出固定 local-only named-pipe 消息 Component。
pub(crate) mod pipe;
