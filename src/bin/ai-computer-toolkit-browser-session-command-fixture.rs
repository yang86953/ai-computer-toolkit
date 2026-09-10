//! 固定 browser-session command 客户端集成测试入口。

// 执行固定 open/close command fixture。
fn main() {
    // 安装只更新原子状态的控制台取消处理器。
    ai_computer_toolkit::install_cancellation_handler();
    // 传播隐藏 fixture 退出码。
    std::process::exit(
        // 不接受 caller path、endpoint、argv 或环境覆盖。
        ai_computer_toolkit::browser_session_broker_command_fixture::run_stdio(),
    );
}
