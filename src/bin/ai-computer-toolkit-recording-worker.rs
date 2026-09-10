//! 精确窗口录制隔离 worker 可执行入口。

// 执行严格 JSON-over-stdio worker 并传播稳定退出码。
fn main() {
    // 只调用库中的单次 worker 入口。
    std::process::exit(ai_computer_toolkit::recording_worker::run_stdio());
}
