//! 新任务机器入口由 UIX 的真实 CLI 模式承载；旧 v1 单次命令保持兼容。

use uix::app::{App, AppMode, Cli, CliArgs};

pub(crate) fn try_run(arguments: &[String]) -> Option<i32> {
    if arguments.first().map(String::as_str) != Some("serve") {
        return None;
    }
    // 机器通道使用封闭的启动语法，不能让人类帮助文本混入 JSON Lines。
    let valid = arguments.len() == 3
        && arguments
            .iter()
            .filter(|argument| argument.as_str() == "--stdio")
            .count()
            == 1
        && arguments
            .iter()
            .filter(|argument| {
                argument
                    .strip_prefix("--grant-file=")
                    .is_some_and(|path| !path.is_empty() && path.len() <= 4096)
            })
            .count()
            == 1;
    if !valid {
        return Some(ai_computer_toolkit::task_control::reject_startup());
    }
    ai_computer_toolkit::install_cancellation_handler();
    let mut commands = Cli::new();
    commands.command("serve", serve, "版本化非图像后台任务控制通道");
    Some(App::new().mode(AppMode::CLI).cli(commands).run())
}

fn serve(arguments: &CliArgs) -> i32 {
    match arguments.get("grant-file") {
        Some(path) if arguments.has("stdio") => ai_computer_toolkit::task_control::run_stdio(path),
        _ => ai_computer_toolkit::task_control::reject_startup(),
    }
}
