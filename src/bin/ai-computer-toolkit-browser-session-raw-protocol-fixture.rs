//! 固定 browser-session raw 协议集成测试入口。

// 执行无 argv、无 stdin 的封闭协议场景并传播稳定退出码。
fn main() {
    // 安装只更新原子状态的控制台取消处理器。
    ai_computer_toolkit::install_cancellation_handler();
    // 将隐藏 fixture 的安全退出码返回给测试父进程。
    std::process::exit(
        // 不接受 caller path、endpoint、argv、stdin 或环境覆盖。
        ai_computer_toolkit::browser_session_broker_raw_protocol_fixture::run_stdio(),
    );
}
