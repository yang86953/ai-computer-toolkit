//! 固定 browser-session accepted-only OutcomeUnknown 集成测试入口。

// 执行无 argv、无 stdin 的封闭 OutcomeUnknown 客户端场景并传播稳定退出码。
fn main() {
    // 安装只更新原子状态的控制台取消处理器。
    ai_computer_toolkit::install_cancellation_handler();
    // 将隐藏夹具的固定安全退出码返回给测试父进程。
    std::process::exit(
        // 不接受 caller path、endpoint、argv、stdin 或环境覆盖。
        ai_computer_toolkit::browser_session_broker_outcome_unknown_fixture::run_stdio(),
    );
}
