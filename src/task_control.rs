//! uix-app CLI 使用的任务控制公开 stdio 入口；v1 单次 CLI 保持原契约。

mod execution;
mod media;
mod policy;
mod protocol;
mod session;
mod transport;

/// 从受信任启动端指定的授权文件建立一个有界任务会话；普通请求不能授予权限。
pub fn run_stdio(grant_file: &str) -> i32 {
    transport::run(grant_file)
}

/// 机器入口参数拒绝也保持 v2 JSON，不混入框架的人类帮助文本。
pub fn reject_startup() -> i32 {
    transport::reject_startup()
}
