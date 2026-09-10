//! 独立交互会话固定 command worker 的生产候选入口。

// 执行一次封闭 JSON over stdio 请求。
fn main() {
    // 将协议退出码传回未来的认证 session broker。
    std::process::exit(ai_computer_toolkit::interactive_command_worker::run_stdio());
}
