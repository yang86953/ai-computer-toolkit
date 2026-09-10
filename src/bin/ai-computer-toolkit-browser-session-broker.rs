//! 启动固定同会话 browser-session broker。

// 运行长期 broker 进程。
fn main() {
    // 安装只更新原子状态的控制台取消处理器。
    ai_computer_toolkit::install_cancellation_handler();
    // 执行固定 broker 并传播结构化退出码。
    std::process::exit(ai_computer_toolkit::browser_session_broker::run());
}
