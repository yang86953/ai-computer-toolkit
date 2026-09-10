//! 固定 browser-session transport 客户端集成测试入口。

// 执行无参数、无覆盖入口的固定 ready 探针。
fn main() {
    // 安装只更新原子状态的控制台取消处理器。
    ai_computer_toolkit::install_cancellation_handler();
    // 传播隐藏 fixture 退出码。
    std::process::exit(
        // 不接受 caller path、endpoint、argv 或环境覆盖。
        ai_computer_toolkit::browser_session_broker_transport_fixture::run_stdio(),
    );
}
