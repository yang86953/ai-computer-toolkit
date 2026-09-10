//! 固定 pipe 首实例 owner-preserving 生命周期结果。

// 导入 listener、连接 owner 与安全错误。
use super::{ConnectedPipe, ServerPipe};
// 导入统一错误类型。
use crate::domain::AppControlError;

// 表示可复用 listener 的一次 accept 结果。
pub(crate) enum PersistentServerAccept {
    // 保存已经连接且仍拥有首实例 handle 的 pipe。
    Connected(ConnectedPipe),
    // 保存取消后仍未释放的 listener。
    Cancelled(ServerPipe),
    // 保存失败后仍未释放的 listener 与安全错误。
    Failed(ServerPipe, AppControlError),
}

// 表示断开当前连接并恢复同一 listener 的封闭结果。
// #2059 后续迁移仍需保留 owner-preserving relisten 边界，当前生产 dispatcher 已改用 secondary factory。
#[allow(dead_code)]
pub(crate) enum PersistentServerRelisten {
    // 保存已经恢复监听模式的首实例 handle。
    Listening(ServerPipe),
    // 保存失败后仍未释放的连接 owner 与安全错误。
    Failed(ConnectedPipe, AppControlError),
}
