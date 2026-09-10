//! 隔离浏览器会话 companion worker 二进制入口。

// 执行固定 JSON Lines worker 并传播稳定退出码。
fn main() {
    // 不在二进制入口扩展协议或命令行控制面。
    std::process::exit(ai_computer_toolkit::browser_session_worker::run_stdio());
}
