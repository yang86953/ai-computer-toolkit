//! Rust 隔离观察 worker 二进制入口。

// 执行一次 JSON over stdio worker 请求。
fn main() {
    // 让协议实现决定结构化退出码。
    std::process::exit(ai_computer_toolkit::observation_worker::run_stdio());
}
