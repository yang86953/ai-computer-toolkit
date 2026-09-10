//! 固定 browser-session Job 客户端集成测试入口。

// 执行封闭测试驱动并传播退出码。
fn main() {
    // 不在二进制入口扩展任意路径或参数。
    std::process::exit(ai_computer_toolkit::browser_session_client_fixture::run_stdio());
}
