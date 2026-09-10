//! Linux 可复现 archive 与用户级原子安装工具入口。

#[cfg(target_os = "linux")]
fn main() {
    std::process::exit(ai_computer_toolkit::linux_release::run());
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("ai-computer-toolkit-linux-release is only available on Linux");
    std::process::exit(2);
}
