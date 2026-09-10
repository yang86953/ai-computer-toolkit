#![cfg(target_os = "windows")]

//! 验证原样生产 launcher 在页面业务接受后的失败、未知与资源恢复真值。

// 导入路径、进程与有界等待工具。
use std::{
    // 定位真实 Rust worker sibling。
    path::Path,
    // 启动原样 PowerShell launcher 并捕获输出。
    process::{Command, Output, Stdio},
    // 限制 launcher 等待轮询 CPU。
    thread,
    // 建立不会重置的测试进程 deadline。
    time::{Duration, Instant},
};
// 导入 Windows 子进程组构造扩展。
use std::os::windows::process::CommandExt;

// 导入 provider-neutral JSON 构造器和值。
use serde_json::{Value, json};
// 导入只用于生产 launcher 取消验收的 Windows 控制事件原语。
use windows::Win32::{
    // 导入定向 Ctrl+Break 事件。
    System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent},
    // 导入独立控制台进程组标志。
    System::Threading::CREATE_NEW_PROCESS_GROUP,
};

// 导入父集成测试的生产 launcher、生命周期与进程所有权基础设施。
use super::{
    // 限制单个 launcher 子进程总预算。
    LAUNCHER_PROCESS_TIMEOUT,
    // 安装原样 launcher 与全部 Rust siblings。
    LauncherLayout,
    // 串行化固定 Broker endpoint 场景。
    TEST_LOCK,
    // 使用真实 Rust Browser Session worker。
    WORKER_SOURCE,
    // 验证公开 Browser Session close 成功。
    assert_close_success,
    // 递归拒绝私有实现文本。
    assert_no_private_text,
    // 验证并取得公开 open identity。
    assert_open_success,
    // 发现唯一 host identity。
    discover_host,
    // 写入生命周期 structured input。
    lifecycle_input,
    // 解析唯一公开 stdout JSON。
    output_json,
    // 经原样 launcher 执行普通环境调用。
    run_launcher,
    // 经原样 launcher 执行生命周期 operation。
    run_lifecycle,
    // 等待精确镜像进程清零。
    wait_for_no_process,
    // 绑定精确镜像进程见证。
    wait_for_process,
};
// 复用页面公开 input、成功和业务前错误断言。
use super::{
    // 复用页面严格 wrapper 与 success validator。
    page::{PAGE_URL, TIMEOUT_MS, assert_exact_keys, page_input, page_success},
    // 复用 accepted 前错误矩阵断言。
    page_errors::assert_preaccepted,
};

// 表示一个固定 runtime 故障与预期公开 outcome。
#[derive(Clone, Copy)]
enum FaultScenario {
    // CDP 明确拒绝导航，形成可信 failed mutation final。
    NavigateFailed,
    // CDP 明确拒绝语义查询，形成可信 failed Query final。
    QueryFailed,
    // 导航业务接受后总 deadline 到达，形成 unknown。
    NavigateDeadline,
    // 导航业务接受后调用方显式取消，形成不可猜测终态的 unknown。
    NavigateCancelled,
    // 导航业务接受后浏览器连接断开，形成 unknown。
    NavigateDisconnected,
}

// 为故障场景提供封闭测试事实。
impl FaultScenario {
    // 返回只由工具 runtime fixture 消费的故障模式。
    const fn mode(self) -> &'static str {
        // 穷举四项固定模式。
        match self {
            // 导航确定失败模式。
            Self::NavigateFailed => "page-failed",
            // 查询确定失败模式。
            Self::QueryFailed => "query-failed",
            // 导航延迟直到总期限。
            Self::NavigateDeadline => "page-delay",
            // 显式取消同样使用可中断的导航延迟。
            Self::NavigateCancelled => "page-delay",
            // 导航接收后断开 socket。
            Self::NavigateDisconnected => "page-disconnect",
        }
    }

    // 返回受测公开页面 capability。
    const fn capability(self) -> &'static str {
        // 只有 QueryFailed 使用 query。
        match self {
            // 查询故障绑定 query capability。
            Self::QueryFailed => "browser.page.query@1",
            // 其余场景都绑定 navigate。
            _ => "browser.page.navigate@1",
        }
    }

    // 返回公开 error code。
    const fn code(self) -> &'static str {
        // 确定失败与未知分别收敛。
        match self {
            // 两项 CDP rejection 都是可信操作失败。
            Self::NavigateFailed | Self::QueryFailed => "OPERATION_FAILED",
            // deadline 与断线都不能猜测 final。
            Self::NavigateDeadline
            // 显式取消在 accepted 后同样不得伪造确定取消终态。
            | Self::NavigateCancelled
            // 浏览器断线同样缺失可信 final。
            | Self::NavigateDisconnected => "OUTCOME_UNKNOWN",
        }
    }

    // 返回公开 outcome。
    const fn outcome(self) -> &'static str {
        // 根据是否取得可信 failed final 选择。
        match self {
            // provider rejection 已形成可信失败。
            Self::NavigateFailed | Self::QueryFailed => "failed",
            // deadline 与断线没有可信 final。
            Self::NavigateDeadline
            // accepted 后取消保持 unknown。
            | Self::NavigateCancelled
            // 断线保持 unknown。
            | Self::NavigateDisconnected => "unknown",
        }
    }

    // 返回是否取得可信 final。
    const fn final_state_reached(self) -> bool {
        // 只有确定失败场景取得 final。
        matches!(self, Self::NavigateFailed | Self::QueryFailed)
    }

    // 返回 target 是否可能被页面 operation 改变。
    const fn target_may_have_mutated(self) -> bool {
        // 只有页面导航是 mutation。
        !matches!(self, Self::QueryFailed)
    }

    // 返回 operation 总预算。
    const fn timeout_ms(self) -> u32 {
        // deadline 场景使用足以观察 accepted 的短预算。
        match self {
            // 五十毫秒后触发协作取消与 unknown。
            Self::NavigateDeadline => 50,
            // 其他场景使用标准五秒预算。
            _ => 5_000,
        }
    }

    // 返回场景是否需要先导航出 current page。
    const fn needs_current_page(self) -> bool {
        // query failure 需要合法 current page 才能越过业务接受。
        matches!(self, Self::QueryFailed)
    }

    // 返回场景是否由真实 launcher 控制事件触发协作取消。
    const fn uses_launcher_cancellation(self) -> bool {
        // 只有显式取消场景使用进程组控制事件。
        matches!(self, Self::NavigateCancelled)
    }

    // 返回确定失败后 session 是否仍可显式关闭。
    const fn retains_session(self) -> bool {
        // 确定 failed final 保持 live session。
        self.final_state_reached()
            // 客户端本地取消未知也不授权自动关闭既有 session。
            || self.uses_launcher_cancellation()
    }
}

// 等待当前故障场景的工具 profile 根缺失或为空。
fn wait_for_profile_cleanup(layout: &LauncherLayout, scenario: FaultScenario) {
    // 建立固定回收预算。
    let deadline = Instant::now() + Duration::from_secs(5);
    // 轮询工具自有 profile 根。
    loop {
        // 根缺失或已经没有入口时视为完整清理。
        let profiles_clean = std::fs::read_dir(layout.profile_root())
            // 成功枚举时确认没有剩余入口。
            .map(|mut entries| entries.next().is_none())
            // 根被回收时同样满足资源收敛。
            .unwrap_or(true);
        // 清理完成时返回。
        if profiles_clean {
            // 当前场景所有 profile 生命周期已经收敛。
            return;
        }
        // 超时必须报告具体故障模式。
        assert!(
            // 只允许在固定预算内继续。
            Instant::now() < deadline,
            // 不输出 profile 路径或私有 session，只输出固定测试模式。
            "browser profile should be cleaned after {}",
            // 使用封闭模式文本定位所有权缺口。
            scenario.mode(),
        );
        // 限制文件轮询 CPU。
        thread::sleep(Duration::from_millis(10));
    }
}

// 构造带固定 runtime 故障模式的原样生产 launcher 命令。
pub(super) fn launcher_mode_command(
    // 接收隔离生产布局。
    layout: &LauncherLayout,
    // 接收公开 CLI 参数。
    arguments: &[&str],
    // 接收只由 runtime fixture 消费的模式。
    mode: &str,
) -> Command {
    // 启动 Windows 原生 PowerShell 7。
    let mut command = Command::new("pwsh.exe");
    // 固定不加载用户 profile 且禁止控制事件进入交互调试器。
    command.args(["-NoProfile", "-NonInteractive", "-File"]);
    // 传递原样生产 launcher 路径。
    command.arg(&layout.launcher);
    // 传递公开 CLI 参数。
    command.args(arguments);
    // 让生产 worker 只发现工具自有 runtime fixture。
    command.env("AI_COMPUTER_TOOLKIT_BROWSER_PATH", &layout.runtime);
    // 注入仅由测试 runtime fixture 读取的封闭模式。
    command.env("ACT_BROWSER_SESSION_RUNTIME_FIXTURE_MODE", mode);
    // 隔离当前场景 TEMP。
    command.env("TEMP", &layout.temporary);
    // 同步隔离 TMP。
    command.env("TMP", &layout.temporary);
    // 捕获唯一公开 stdout。
    command.stdout(Stdio::piped());
    // 捕获失败诊断但不投影到公开断言。
    command.stderr(Stdio::piped());
    // 返回尚未启动且可继续设置测试生命周期的命令。
    command
}

// 有界等待一个已经启动的原样生产 launcher 并收集输出。
pub(super) fn collect_launcher_output(
    // 接收当前测试唯一拥有的 launcher 进程。
    mut child: std::process::Child,
) -> Output {
    // 建立不会因轮询重置的总 deadline。
    let deadline = Instant::now() + LAUNCHER_PROCESS_TIMEOUT;
    // 在固定预算内等待唯一 launcher 退出。
    loop {
        // 只观察当前 owned 子进程。
        match child
            // 非阻塞读取退出事实。
            .try_wait()
            // 等待失败时提供固定上下文。
            .unwrap_or_else(|error| panic!("production launcher should be waitable: {error:?}"))
        {
            // 已退出时收集完整有界输出。
            Some(_) => {
                // 返回公开进程结果。
                return child
                    // 收集 stdout 与 stderr 管道。
                    .wait_with_output()
                    // 输出收集失败不能伪造结果。
                    .unwrap_or_else(|error| panic!("launcher output should exist: {error:?}"));
            }
            // 预算内继续短轮询。
            None if Instant::now() < deadline => {
                // 限制轮询 CPU。
                thread::sleep(Duration::from_millis(10));
            }
            // 超出预算时回收 owned launcher。
            None => {
                // 终止仅由当前测试启动的精确子进程。
                child
                    // 请求终止 owned 进程。
                    .kill()
                    // 终止失败表示无法证明收敛。
                    .unwrap_or_else(|error| panic!("timed out launcher should stop: {error:?}"));
                // 等待终止完成并关闭捕获管道。
                let _ = child.wait_with_output();
                // 明确报告超出冻结预算。
                panic!("production launcher exceeded the fixed test deadline");
            }
        }
    }
}

// 在导航已经越过 accepted 后向原样生产 launcher 发送显式取消。
fn run_cancelled_navigate(
    // 接收隔离生产布局。
    layout: &LauncherLayout,
    // 借用当前公开 session identity。
    session_id: &str,
) -> Output {
    // 构造 confirmed strict 导航 input。
    let input = page_input(
        // 绑定当前布局。
        layout,
        // 使用 apply。
        "apply",
        // 绑定稳定导航 capability。
        "browser.page.navigate@1",
        // 绑定当前 session。
        session_id,
        // 使用长预算以便显式取消先于 deadline。
        json!({ "url": PAGE_URL, "timeoutMs": TIMEOUT_MS }),
    );
    // 转换为公开 launcher argv 文本。
    let input = input.to_string_lossy().into_owned();
    // 使用布局内固定文件作为 accepted 后 CDP dispatch 见证。
    let marker = layout.temporary.join("page-command-accepted.marker");
    // 构造原样生产 launcher 命令。
    let mut command = launcher_mode_command(
        // 使用当前隔离布局。
        layout,
        // 传递 confirmed strict 导航。
        &[
            // 进入统一运行 surface。
            "run",
            // 使用 App facade。
            "app",
            // 导航使用 apply。
            "apply",
            // 指定 structured input。
            "--input",
            // 传递测试独占 input 路径。
            &input,
            // mutation 显式确认。
            "--confirm",
            // 导航必须使用 strict isolation。
            "--strict-isolation",
        ],
        // runtime 保持导航调用在途。
        "page-delay",
    );
    // 建立可被精确定向 Ctrl+Break 的独立进程组。
    command.creation_flags(CREATE_NEW_PROCESS_GROUP.0);
    // 启动当前测试唯一拥有的原样 launcher。
    let mut child = command
        // 启动精确 PowerShell 子进程。
        .spawn()
        // 启动失败时保留测试上下文。
        .unwrap_or_else(|error| panic!("cancelled launcher should start: {error:?}"));
    // accepted 见证等待使用同一冻结 launcher 总预算。
    let marker_deadline = Instant::now() + LAUNCHER_PROCESS_TIMEOUT;
    // 在页面实际越过 accepted 并到达 CDP 前不得发送取消。
    while !marker.is_file() {
        // 只执行非阻塞 owned 进程观察。
        let exited = child
            // 查询当前 launcher 状态。
            .try_wait()
            // 等待失败不得伪造 accepted。
            .unwrap_or_else(|error| panic!("cancelled launcher should be waitable: {error:?}"))
            // 映射退出事实。
            .is_some();
        // launcher 提前退出时收集唯一公开 stdout 供诊断。
        if exited {
            // 取得已经退出进程的完整管道输出。
            let output = child
                // 等待并收集已结束进程。
                .wait_with_output()
                // 输出收集失败不得掩盖原根因。
                .unwrap_or_else(|error| {
                    panic!("cancelled launcher output should exist: {error:?}")
                });
            // 只公开本就属于 launcher 公共边界的 stdout。
            panic!(
                // 固定说明后附加公开 JSON。
                "launcher exited before accepted dispatch: {}",
                // 使用宽容 UTF-8 仅用于测试诊断。
                String::from_utf8_lossy(&output.stdout),
            );
        }
        // marker 等待不得扩张测试总预算。
        assert!(
            // 核对单调期限。
            Instant::now() < marker_deadline,
            // 使用固定诊断。
            "page command did not reach accepted dispatch before the test deadline",
        );
        // 限制 marker 轮询 CPU。
        thread::sleep(Duration::from_millis(10));
    }
    // 向当前 owned PowerShell 进程组发送 Ctrl+Break。
    let delivered = unsafe {
        // 只定向当前测试建立的进程组 ID。
        GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id())
    };
    // 控制事件必须成功进入 launcher 取消路径。
    assert!(
        delivered.is_ok(),
        "launcher cancellation event should be delivered"
    );
    // 在原总预算内收集结构化取消结果。
    collect_launcher_output(child)
}

// 用故障环境启动固定 Broker 并创建 live Browser Session。
pub(super) fn open_with_mode(
    // 接收隔离生产布局。
    layout: &LauncherLayout,
    // 接收公开 host target。
    host_id: &str,
    // 接收 runtime fixture 模式。
    mode: &str,
) -> Output {
    // 写入严格 lifecycle input。
    let input = lifecycle_input(
        // 使用当前布局。
        layout,
        // open 使用 create。
        "create",
        // 绑定稳定 open capability。
        "browser.session.open@1",
        // 绑定当前 host。
        host_id,
        // 使用完整打开预算。
        TIMEOUT_MS,
    );
    // 转换为 PowerShell argv 文本。
    let input = input.to_string_lossy().into_owned();
    // 使用布局内固定文件作为后续页面 accepted dispatch 见证。
    let marker = layout.temporary.join("page-command-accepted.marker");
    // 理论陈旧 marker 不得让后续取消场景提前发送事件。
    let _ = std::fs::remove_file(&marker);
    // 构造会启动固定 Broker 的首个原样 launcher 命令。
    let mut command = launcher_mode_command(
        // 使用隔离布局。
        layout,
        // 传递 confirmed create 参数。
        &["run", "app", "create", "--input", &input, "--confirm"],
        // 传递封闭 runtime 模式。
        mode,
    );
    // 在 Broker 启动前注入只由测试 runtime fixture 消费的见证路径。
    command.env(
        // 使用固定测试环境变量。
        "ACT_BROWSER_SESSION_RUNTIME_FIXTURE_ACCEPTED_MARKER",
        // 传入布局内私有 marker。
        &marker,
    );
    // 启动会被当前测试有界回收的首个 launcher。
    let child = command
        // 启动精确 PowerShell 子进程。
        .spawn()
        // 启动失败时保留测试上下文。
        .unwrap_or_else(|error| panic!("production launcher should start: {error:?}"));
    // 在固定预算内收集公开 open 结果。
    collect_launcher_output(child)
}

// 经原样 launcher 执行指定总预算的 strict confirmed 导航。
pub(super) fn run_navigate(
    // 接收隔离生产布局。
    layout: &LauncherLayout,
    // 接收公开 Browser Session target。
    session_id: &str,
    // 接收覆盖完整调用链的总预算。
    timeout_ms: u32,
) -> Output {
    // 写入冻结页面导航 input。
    let input = page_input(
        // 使用当前布局。
        layout,
        // 使用稳定测试文件标签。
        "fault-navigate",
        // 绑定稳定导航 capability。
        "browser.page.navigate@1",
        // 绑定当前 session。
        session_id,
        // 只传固定 URL 与总预算。
        json!({ "url": PAGE_URL, "timeoutMs": timeout_ms }),
    );
    // 转换为 PowerShell argv 文本。
    let input = input.to_string_lossy().into_owned();
    // 导航必须显式确认并要求 strict isolation。
    run_launcher(
        // 使用原样生产 launcher。
        layout,
        // 传递冻结公开参数。
        &[
            // 进入通用运行 surface。
            "run",
            // 使用统一 App facade。
            "app",
            // 导航使用 apply。
            "apply",
            // 指定 structured input。
            "--input",
            // 传递测试独占路径。
            &input,
            // 显式确认 mutation。
            "--confirm",
            // 固定严格零打扰。
            "--strict-isolation",
        ],
    )
}

// 经原样 launcher 执行会触发 runtime fixture 的页面查询。
pub(super) fn run_query(
    // 接收隔离生产布局。
    layout: &LauncherLayout,
    // 接收公开 Browser Session target。
    session_id: &str,
    // 接收 current page identity。
    page_id: &str,
) -> Output {
    // 写入严格 provider-neutral query input。
    let input = page_input(
        // 使用当前布局。
        layout,
        // 使用稳定测试文件标签。
        "fault-query",
        // 绑定稳定 query capability。
        "browser.page.query@1",
        // 绑定当前 live session。
        session_id,
        // 查询固定 button role。
        json!({
            // 绑定 current page。
            "pageId": page_id,
            // 使用 provider-neutral selector。
            "selector": { "role": "button" },
            // 使用固定结果上限。
            "maxResults": 2,
            // 使用完整总预算。
            "timeoutMs": TIMEOUT_MS,
        }),
    );
    // 转换为 PowerShell argv 文本。
    let input = input.to_string_lossy().into_owned();
    // Query 不携带确认或 strict flag。
    run_launcher(
        // 使用原样生产 launcher。
        layout,
        // 传递 standard read 参数。
        &["run", "app", "read", "--input", &input],
    )
}

// 验证 accepted 后错误的完整公开真值。
fn assert_accepted_error(
    // 接收原样 launcher 输出。
    output: &Output,
    // 接收当前故障场景。
    scenario: FaultScenario,
    // 接收公开 Browser Session target。
    session_id: &str,
) {
    // accepted 后失败必须非零退出。
    assert!(!output.status.success());
    // 解析唯一公开 JSON 文档。
    let value = output_json(output);
    // 顶层错误 envelope 只允许 ok 与 error。
    assert_exact_keys(&value, &["ok", "error"]);
    // 取得统一 error 对象。
    let error = value
        // 读取 error。
        .get("error")
        // 缺失 error 必须失败。
        .unwrap_or_else(|| panic!("accepted page failure should contain error"));
    // error 只允许 code、message 与 details。
    assert_exact_keys(error, &["code", "message", "details"]);
    // 核对场景公开错误码。
    assert_eq!(
        // 读取 error code。
        error.get("code").and_then(Value::as_str),
        // 比较冻结分类。
        Some(scenario.code()),
    );
    // 取得 accepted 后公共详情。
    let details = error
        // 读取 details。
        .get("details")
        // accepted 错误必须携带真值。
        .unwrap_or_else(|| panic!("accepted page failure should contain details"));
    // accepted 后详情必须包含禁止自动重派字段。
    assert_exact_keys(
        // 传递详情对象。
        details,
        // 对齐公开页面错误矩阵。
        &[
            // 绑定原 capability。
            "capability",
            // 回显 caller 已知 session。
            "targetId",
            // 保存 failed 或 unknown。
            "outcome",
            // 明确已业务接受。
            "accepted",
            // 保存可信 final 事实。
            "finalStateReached",
            // accepted 后不可安全重试。
            "retrySafe",
            // 保存 mutation 保守事实。
            "targetMayHaveMutated",
            // 禁止 facade 自动生成新意图。
            "automaticRetryProhibited",
        ],
    );
    // capability 必须绑定场景 operation。
    assert_eq!(
        // 读取 capability。
        details.get("capability").and_then(Value::as_str),
        // 比较场景 ID。
        Some(scenario.capability()),
    );
    // target 必须逐字保持 caller session。
    assert_eq!(
        // 读取 targetId。
        details.get("targetId").and_then(Value::as_str),
        // 比较原 session。
        Some(session_id),
    );
    // outcome 必须区分 failed 与 unknown。
    assert_eq!(
        // 读取 outcome。
        details.get("outcome").and_then(Value::as_str),
        // 比较场景预期。
        Some(scenario.outcome()),
    );
    // 四项场景都必须已经业务接受。
    assert_eq!(details.get("accepted"), Some(&Value::Bool(true)));
    // 只有确定 provider rejection 取得可信 final。
    assert_eq!(
        // 读取 final 事实。
        details.get("finalStateReached"),
        // 比较场景预期。
        Some(&Value::Bool(scenario.final_state_reached())),
    );
    // accepted 后一律不可安全重试。
    assert_eq!(details.get("retrySafe"), Some(&Value::Bool(false)));
    // navigate 保守标记 mutation，Query 始终只读。
    assert_eq!(
        // 读取 mutation 事实。
        details.get("targetMayHaveMutated"),
        // 比较 action 同源预期。
        Some(&Value::Bool(scenario.target_may_have_mutated())),
    );
    // facade 必须禁止任何自动重派。
    assert_eq!(
        // 读取自动重派门禁。
        details.get("automaticRetryProhibited"),
        // 固定禁止。
        Some(&Value::Bool(true)),
    );
    // 公开错误不得泄漏故障模式、输入或实现事实。
    assert_no_private_text(&value);
}

// 验证确定失败、期限与断线在公开 launcher 上保持 accepted 真值并收敛资源。
#[test]
fn production_launcher_projects_page_failures_and_unknown_without_retry() {
    // 独占当前登录会话的固定 Broker endpoint。
    let _test_guard = TEST_LOCK
        // 中毒表示前一场景已失败。
        .lock()
        // 不隐藏并发测试根因。
        .unwrap_or_else(|error| panic!("browser launcher lock should be available: {error:?}"));
    // 遍历封闭故障集合。
    for scenario in [
        // 覆盖 accepted navigate 确定失败。
        FaultScenario::NavigateFailed,
        // 覆盖 accepted Query 确定失败且只读。
        FaultScenario::QueryFailed,
        // 覆盖 accepted navigate 总 deadline unknown。
        FaultScenario::NavigateDeadline,
        // 覆盖 accepted navigate 显式取消 unknown。
        FaultScenario::NavigateCancelled,
        // 覆盖 accepted navigate 浏览器断线 unknown。
        FaultScenario::NavigateDisconnected,
    ] {
        // 为当前场景安装独立原样 launcher 与 Rust siblings。
        let layout = LauncherLayout::install(Path::new(WORKER_SOURCE));
        // 经公开 sessions 发现当前 host。
        let host_id = discover_host(&layout);
        // 第一次会启动 Broker 的 open 注入当前固定 runtime 模式。
        let open = open_with_mode(&layout, &host_id, scenario.mode());
        // 验证并取得公开 Browser Session identity。
        let session_id = assert_open_success(&open, &host_id);
        // 绑定本测试启动的固定 Broker 供最终确定回收。
        let broker = wait_for_process(&layout.broker, true);
        // 绑定当前 live worker 供资源见证。
        let worker = wait_for_process(&layout.worker, false);
        // 绑定 runtime fixture descendant 供资源见证。
        let runtime = wait_for_process(&layout.runtime, false);
        // Query failure 需要先产生 current page。
        let current_page = if scenario.needs_current_page() {
            // 经独立 launcher 完成正常 strict navigate。
            let navigate = run_navigate(&layout, &session_id, TIMEOUT_MS);
            // 验证公开 success 并取得 data。
            let navigate = page_success(
                // 传递 launcher 输出。
                &navigate,
                // 绑定稳定导航 capability。
                "browser.page.navigate@1",
                // 导航使用 apply。
                "apply",
                // 顶层绑定当前 session。
                &session_id,
            );
            // 提取 current page identity。
            Some(
                navigate
                    // 读取公开页面 identity。
                    .get("pageId")
                    // 只接受字符串。
                    .and_then(Value::as_str)
                    // 成功必须签发页面。
                    .unwrap_or_else(|| panic!("query fault setup should return pageId"))
                    // 保留给下一 launcher。
                    .to_owned(),
            )
        } else {
            // 导航故障不需要预先页面。
            None
        };
        // 执行当前受测页面 operation。
        let output = if scenario.uses_launcher_cancellation() {
            // 通过真实 PowerShell/Rust launcher 取消链中断 accepted 导航。
            run_cancelled_navigate(&layout, &session_id)
        } else {
            // 其余故障通过封闭 runtime 模式同步执行。
            match current_page.as_deref() {
                // Query failure 使用 current page。
                Some(page_id) => run_query(&layout, &session_id, page_id),
                // 导航故障使用场景总预算。
                None => run_navigate(&layout, &session_id, scenario.timeout_ms()),
            }
        };
        // 核对 accepted、final、mutation 与自动重派真值。
        assert_accepted_error(&output, scenario, &session_id);
        // 确定失败或客户端本地取消保持 live session，必须显式 close。
        if scenario.retains_session() {
            // 经全新 launcher 回收当前 live session。
            let close = run_lifecycle(
                // 使用同一固定 Broker 代际。
                &layout,
                // close 使用 generic close。
                "close",
                // 绑定稳定 close capability。
                "browser.session.close@1",
                // 绑定当前 session。
                &session_id,
                // mutation 显式确认。
                true,
                // 使用完整回收预算。
                TIMEOUT_MS,
            );
            // 核对可信 close success。
            assert_close_success(&close, &session_id);
        }
        // unknown 强制回收后，同一 session 必须 stale 且不得自动重建 worker。
        if !scenario.retains_session() {
            // 等待当前 worker 完整退出后再验证 stale。
            assert!(worker.wait_exited(Duration::from_secs(5)));
            // 等待 runtime descendant 完整退出。
            assert!(runtime.wait_exited(Duration::from_secs(5)));
            // 使用同一 caller session 发起显式新调用。
            let stale = run_navigate(&layout, &session_id, TIMEOUT_MS);
            // 必须在业务接受前返回 stale session。
            assert_preaccepted(
                // 传递公开错误输出。
                &stale,
                // 固定 stale session 分类。
                "STALE_SESSION",
                // 绑定原导航 capability。
                "browser.page.navigate@1",
                // caller canonical target 可安全回显。
                Some(&session_id),
            );
        } else {
            // 显式 close 后同一 worker 必须退出。
            assert!(worker.wait_exited(Duration::from_secs(5)));
            // 显式 close 后 runtime descendant 必须退出。
            assert!(runtime.wait_exited(Duration::from_secs(5)));
        }
        // 精确 worker 镜像必须收敛为零。
        wait_for_no_process(&layout.worker);
        // 精确 runtime 镜像必须收敛为零。
        wait_for_no_process(&layout.runtime);
        // 工具自有 profile 必须完整清理。
        wait_for_profile_cleanup(&layout, scenario);
        // 终止本测试拥有的固定 Broker。
        broker.terminate_owned();
        // endpoint owner 必须退出。
        wait_for_no_process(&layout.broker);
        // 删除当前场景独占布局。
        layout.finish();
    }
}
