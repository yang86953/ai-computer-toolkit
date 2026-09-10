use std::io::Write;

use ai_computer_toolkit::{cli, domain::error_json};

mod cli_host;

#[cfg(target_os = "linux")]
static WORKFLOW_FIXTURE_TERMINATION_SEEN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "linux")]
extern "C" fn workflow_fixture_signal_handler(_: libc::c_int) {
    WORKFLOW_FIXTURE_TERMINATION_SEEN.store(true, std::sync::atomic::Ordering::Release);
}

fn main() {
    if std::env::args().skip(1).eq(["--list-tools"]) {
        println!(
            "{}",
            serde_json::json!({"tools":ai_computer_toolkit::mcp::tool_catalog()})
        );
        return;
    }
    if let Some(exit_code) = cli_host::try_run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        std::process::exit(exit_code);
    }
    // 无参数即 MCP stdio 服务：MCP 客户端按标准方式直接启动本二进制。
    if std::env::args().len() == 1 {
        ai_computer_toolkit::install_cancellation_handler();
        std::process::exit(ai_computer_toolkit::mcp::run_stdio());
    }
    #[cfg(target_os = "linux")]
    if std::env::args()
        .skip(1)
        .eq([ai_computer_toolkit::linux_media_observation_worker::HIDDEN_ARGUMENT])
    {
        // MPRIS 只读 worker 只接受固定隐藏 argv 和严格 stdin 协议。
        std::process::exit(ai_computer_toolkit::linux_media_observation_worker::run_stdio());
    }
    #[cfg(target_os = "linux")]
    if std::env::args()
        .skip(1)
        .eq([ai_computer_toolkit::linux_media_control_worker::HIDDEN_ARGUMENT])
    {
        // 写 worker 不从 argv 接收目标、操作或总线地址。
        std::process::exit(ai_computer_toolkit::linux_media_control_worker::run_stdio());
    }
    #[cfg(target_os = "linux")]
    if std::env::args().skip(1).eq(["__sequence-step-worker-v1"]) {
        // 固定隐藏入口只接受严格 JSON Lines 协议，不形成任意 argv 执行面。
        ai_computer_toolkit::install_cancellation_handler();
        std::process::exit(ai_computer_toolkit::sequence_step_worker::run_stdio());
    }
    #[cfg(target_os = "linux")]
    if std::env::args()
        .skip(1)
        .eq(["__workflow-process-fixture-v1"])
    {
        run_workflow_process_fixture(b"act-flow-fix\0", None);
    }
    #[cfg(target_os = "linux")]
    if std::env::args()
        .skip(1)
        .eq(["__workflow-process-fixture-ignore-term-v1"])
    {
        run_workflow_process_fixture(b"act-flow-hang\0", Some(b"act-flow-seen\0"));
    }
    #[cfg(target_os = "linux")]
    if std::env::args()
        .skip(1)
        .eq(["__workflow-process-fixture-cancel-v1"])
    {
        run_workflow_process_fixture(b"act-flow-cancel\0", Some(b"act-flow-cseen\0"));
    }
    #[cfg(target_os = "linux")]
    if std::env::args()
        .skip(1)
        .eq(["__application-launch-fixture-v1"])
    {
        // 固定无窗口夹具只证明精确 argv 已到达当前 toolkit 映像。
        std::process::exit(0);
    }
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if std::env::args().skip(1).eq(["session-host", "desktop"]) {
        std::process::exit(ai_computer_toolkit::desktop_session_broker::run_stdio());
    }
    #[cfg(target_os = "linux")]
    if std::env::args()
        .skip(1)
        .eq(["session-host", "desktop", "--socket"])
    {
        std::process::exit(ai_computer_toolkit::desktop_session_broker::run_socket_server());
    }
    #[cfg(target_os = "linux")]
    if std::env::args()
        .skip(1)
        .take(2)
        .eq(["session-call", "desktop"])
    {
        std::process::exit(
            ai_computer_toolkit::desktop_session_broker::run_socket_client(
                std::env::args().skip(3).collect(),
            ),
        );
    }
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("__linux-release") {
        std::process::exit(ai_computer_toolkit::linux_release::run_arguments(
            std::env::args().skip(2).collect(),
        ));
    }
    // 安装只更新原子状态的控制台取消处理器。
    ai_computer_toolkit::install_cancellation_handler();
    let output = match cli::run(std::env::args().skip(1).collect()) {
        Ok(output) => output,
        Err(error) => cli::CliOutput {
            json: error_json(&error),
            pretty: false,
            exit_code: match error.code {
                "BACKGROUND_OPERATION_UNAVAILABLE" => 3,
                "FOREGROUND_CONSENT_REQUIRED" => 4,
                _ => 2,
            },
        },
    };
    let text = if output.pretty {
        serde_json::to_string_pretty(&output.json)
    } else {
        serde_json::to_string(&output.json)
    };
    match text {
        Ok(text) => {
            if writeln!(std::io::stdout(), "{text}").is_err() {
                std::process::exit(2);
            }
            if output.exit_code != 0 {
                std::process::exit(output.exit_code);
            }
        }
        Err(_) => std::process::exit(2),
    }
}

#[cfg(target_os = "linux")]
fn run_workflow_process_fixture(name: &'static [u8], seen_name: Option<&'static [u8]>) -> ! {
    // SAFETY: 两个调用都只使用编译期固定名称/信号，不读取调用方地址或原生身份。
    unsafe {
        let _ = libc::prctl(libc::PR_SET_NAME, name.as_ptr() as libc::c_ulong, 0, 0, 0);
        if seen_name.is_some() {
            let _ = libc::signal(
                libc::SIGTERM,
                workflow_fixture_signal_handler as *const () as libc::sighandler_t,
            );
        }
    }
    loop {
        if let Some(seen) = seen_name
            && WORKFLOW_FIXTURE_TERMINATION_SEEN.load(std::sync::atomic::Ordering::Acquire)
        {
            // SAFETY: 固定短名称含 NUL，且只在普通线程中更新项目夹具的 procfs comm。
            unsafe {
                let _ = libc::prctl(libc::PR_SET_NAME, seen.as_ptr() as libc::c_ulong, 0, 0, 0);
            }
        }
        std::thread::park_timeout(std::time::Duration::from_millis(10));
    }
}
