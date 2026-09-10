//! 负责隔离 Chromium 的固定、无 shell 命令模板。

// 导入路径与子进程模板。
use std::{
    // 导入路径借用。
    path::Path,
    // 导入进程命令与标准流策略。
    process::{Command, Stdio},
};

// 构造固定、无 shell、无调用方参数的 Chromium 命令。
pub(super) fn browser_command(
    // 借用已认证 runtime 路径。
    runtime: &Path,
    // 借用工具自有空 profile 路径。
    profile: &Path,
) -> Command {
    // 直接启动认证 runtime。
    let mut command = Command::new(runtime);
    // 安装固定无头与隐私参数。
    command.args([
        // 使用当前 Chromium 无头实现。
        "--headless=new",
        // 禁止首次运行流程。
        "--no-first-run",
        // 禁止默认浏览器提示。
        "--no-default-browser-check",
        // 禁止后台网络组件污染空 profile。
        "--disable-background-networking",
        // 让 Chromium 在自有 profile 内选择回环调试端口。
        "--remote-debugging-port=0",
        // 打开固定空白页，不接收调用方 URL。
        "about:blank",
    ]);
    // 只传入 worker 自己创建的空 profile。
    command.arg(format!("--user-data-dir={}", profile.display()));
    // stdout 不进入 JSON Lines 协议。
    command.stdout(Stdio::null());
    // stderr 不进入 JSON Lines 协议。
    command.stderr(Stdio::null());
    // Windows 上禁止创建控制台窗口。
    #[cfg(windows)]
    // 限制平台私有命令标志。
    {
        // 导入 Windows CommandExt。
        use std::os::windows::process::CommandExt;
        // 固定 CREATE_NO_WINDOW。
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // 安装无窗口标志。
        command.creation_flags(CREATE_NO_WINDOW);
    }
    // 返回封闭命令模板。
    command
}
