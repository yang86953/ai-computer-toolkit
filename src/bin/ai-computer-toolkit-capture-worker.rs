//! 隔离捕获探针 companion worker 二进制入口。

// 执行严格 JSON-over-stdio 协议。
fn main() {
    // 让协议实现决定结构化退出码。
    std::process::exit(ai_computer_toolkit::capture_worker::run_stdio());
}
