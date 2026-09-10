//! 语义元素动作隔离 worker 的固定生产入口。

// 执行一次 JSON over stdio 请求。
fn main() {
    // 将协议退出码传回 parent worker Component。
    std::process::exit(ai_computer_toolkit::semantic_action_worker::run_stdio());
}
