//! 固定 sequence step worker 的生产 JSON Lines stdio 入口。

// 执行一次受双阶段协议约束的统一 System 请求。
fn main() {
    // 将结构化完成、拒绝或 transport 失败退出码返回父进程。
    std::process::exit(ai_computer_toolkit::sequence_step_worker::run_stdio());
}
