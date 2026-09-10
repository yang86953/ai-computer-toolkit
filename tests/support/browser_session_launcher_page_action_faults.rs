#![cfg(target_os = "windows")]

//! 验证原样生产 launcher 在页面动作 accepted 后的失败、未知与资源恢复真值。

// 导入路径、进程与有界等待工具。
use std::{
    // 定位真实 Rust worker sibling。
    path::Path,
    // 保存原样 launcher 输出。
    process::Output,
    // 限制 marker 等待轮询 CPU。
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
    // 解析唯一公开 stdout JSON。
    output_json,
    // 复用页面严格 wrapper 与 success validator。
    page::{TIMEOUT_MS, assert_exact_keys, page_input, page_success},
    // 复用 accepted 前错误断言与调用器。
    page_errors::{assert_preaccepted, run_page},
    // 复用带 runtime 模式的 launcher 与 setup helpers。
    page_faults::{
        // 有界收集自定义 launcher 输出。
        collect_launcher_output,
        // 构造带封闭 runtime mode 的原样 launcher。
        launcher_mode_command,
        // 创建继承封闭 runtime mode 的 live session。
        open_with_mode,
        // 建立 current page。
        run_navigate,
        // 查询当前 page 并签发 element。
        run_query,
    },
    // 经原样 launcher 执行普通环境调用。
    run_launcher,
    // 经原样 launcher执行生命周期 operation。
    run_lifecycle,
    // 等待精确镜像进程清零。
    wait_for_no_process,
    // 绑定精确镜像进程见证。
    wait_for_process,
    // 等待工具自有 profile 完整回收。
    wait_for_profiles_clean,
};

// 表示一个固定页面动作故障与预期公开 outcome。
#[derive(Clone, Copy)]
enum ActionFaultScenario {
    // click 的 CDP 阶段明确拒绝，形成可信 failed mutation final。
    ClickFailed,
    // type 的 CDP 阶段明确拒绝，形成可信 failed mutation final。
    TypeFailed,
    // screenshot 的 CDP 阶段明确拒绝，形成可信 failed Query final。
    ScreenshotFailed,
    // type 业务接受后总 deadline 到达，形成 unknown。
    TypeDeadline,
    // click 业务接受后调用方显式取消，形成不可猜测终态的 unknown。
    ClickCancelled,
    // screenshot 业务接受后浏览器连接断开，形成只读 unknown。
    ScreenshotDisconnected,
}

// 为故障场景提供封闭测试事实。
impl ActionFaultScenario {
    // 返回只由工具 runtime fixture 消费的故障模式。
    const fn mode(self) -> &'static str {
        // 穷举六项固定模式。
        match self {
            // click 确定失败模式。
            Self::ClickFailed => "click-failed",
            // type 确定失败模式。
            Self::TypeFailed => "type-failed",
            // screenshot 确定失败模式。
            Self::ScreenshotFailed => "screenshot-failed",
            // type 延迟直到总期限。
            Self::TypeDeadline => "type-delay",
            // 显式取消使用可中断的 click 延迟。
            Self::ClickCancelled => "click-delay",
            // screenshot 接受后断开 socket。
            Self::ScreenshotDisconnected => "screenshot-disconnect",
        }
    }

    // 返回受测公开页面 capability。
    const fn capability(self) -> &'static str {
        // 按场景选择稳定 ID。
        match self {
            // 两项 click 场景绑定 click。
            Self::ClickFailed | Self::ClickCancelled => "browser.element.click@1",
            // 两项 type 场景绑定 type。
            Self::TypeFailed | Self::TypeDeadline => "browser.element.type@1",
            // 两项 screenshot 场景绑定 screenshot。
            Self::ScreenshotFailed | Self::ScreenshotDisconnected => "browser.page.screenshot@1",
        }
    }

    // 返回公开 generic verb。
    const fn verb(self) -> &'static str {
        // screenshot 使用 read，其余 mutation 使用 apply。
        match self {
            // 两项 screenshot 都是 Query。
            Self::ScreenshotFailed | Self::ScreenshotDisconnected => "read",
            // click/type 都是 Command。
            _ => "apply",
        }
    }

    // 返回公开 error code。
    const fn code(self) -> &'static str {
        // 确定失败与未知分别收敛。
        match self {
            // 三项 provider rejection 都是可信操作失败。
            Self::ClickFailed | Self::TypeFailed | Self::ScreenshotFailed => "OPERATION_FAILED",
            // deadline、显式取消和断线都不能猜测 final。
            Self::TypeDeadline | Self::ClickCancelled | Self::ScreenshotDisconnected => {
                "OUTCOME_UNKNOWN"
            }
        }
    }

    // 返回公开 outcome。
    const fn outcome(self) -> &'static str {
        // 根据是否取得可信 failed final 选择。
        match self {
            // provider rejection 已形成可信失败。
            Self::ClickFailed | Self::TypeFailed | Self::ScreenshotFailed => "failed",
            // deadline、取消与断线没有可信 final。
            Self::TypeDeadline | Self::ClickCancelled | Self::ScreenshotDisconnected => "unknown",
        }
    }

    // 返回是否取得可信 final。
    const fn final_state_reached(self) -> bool {
        // 只有三项确定失败场景取得 final。
        matches!(
            // 穷举可信 provider rejection。
            self,
            // click 确定失败。
            Self::ClickFailed
                // type 确定失败。
                | Self::TypeFailed
                // screenshot 确定失败。
                | Self::ScreenshotFailed
        )
    }

    // 返回 target 是否可能被页面 operation 改变。
    const fn target_may_have_mutated(self) -> bool {
        // screenshot Query 始终无隐藏副作用。
        !matches!(
            // 只匹配 screenshot 场景。
            self,
            // 确定 screenshot 失败。
            Self::ScreenshotFailed
                // screenshot 断线 unknown。
                | Self::ScreenshotDisconnected
        )
    }

    // 返回 operation 总预算。
    const fn timeout_ms(self) -> u32 {
        // deadline 场景使用足以观察 accepted 的短预算。
        match self {
            // 五十毫秒后触发协作取消与 unknown。
            Self::TypeDeadline => 50,
            // 其他场景使用标准总预算。
            _ => TIMEOUT_MS,
        }
    }

    // 返回场景是否由真实 launcher 控制事件触发协作取消。
    const fn uses_launcher_cancellation(self) -> bool {
        // 只有显式取消场景使用进程组控制事件。
        matches!(self, Self::ClickCancelled)
    }

    // 返回确定失败后 session 是否仍可显式关闭。
    const fn retains_session(self) -> bool {
        // 确定 failed final 保持 live session。
        self.final_state_reached()
            // 客户端本地取消未知也不授权自动关闭既有 session。
            || self.uses_launcher_cancellation()
    }
}

// 构造当前场景的严格公开 action input。
fn action_input(
    // 接收当前场景。
    scenario: ActionFaultScenario,
    // 接收 current page identity。
    page_id: &str,
    // 接收 current element identity。
    element_id: &str,
) -> Value {
    // 按 capability 输出字段封闭 input。
    match scenario {
        // click 只携带 page、element 与总预算。
        ActionFaultScenario::ClickFailed | ActionFaultScenario::ClickCancelled => {
            // 构造 click input。
            json!({"pageId":page_id,"elementId":element_id,"timeoutMs":scenario.timeout_ms()})
        }
        // type 携带有界私密文本与显式 replace。
        ActionFaultScenario::TypeFailed | ActionFaultScenario::TypeDeadline => {
            // 构造 type input。
            json!({"pageId":page_id,"elementId":element_id,"text":"private-action-text","replace":true,"timeoutMs":scenario.timeout_ms()})
        }
        // screenshot 只携带 current page 与总预算。
        ActionFaultScenario::ScreenshotFailed | ActionFaultScenario::ScreenshotDisconnected => {
            // 构造 screenshot input。
            json!({"pageId":page_id,"timeoutMs":scenario.timeout_ms()})
        }
    }
}

// 经原样生产 launcher 执行当前页面动作故障。
fn run_action(
    // 接收隔离生产布局。
    layout: &LauncherLayout,
    // 接收当前场景。
    scenario: ActionFaultScenario,
    // 接收公开 Browser Session target。
    session_id: &str,
    // 接收 current page identity。
    page_id: &str,
    // 接收 current element identity。
    element_id: &str,
) -> Output {
    // 写入严格 provider-neutral action input。
    let input = page_input(
        // 使用当前布局。
        layout,
        // 使用封闭故障模式作为测试文件标签。
        scenario.mode(),
        // 绑定场景 capability。
        scenario.capability(),
        // 绑定当前 live session。
        session_id,
        // 构造当前动作 input。
        action_input(scenario, page_id, element_id),
    );
    // 转换为 PowerShell argv 文本。
    let input = input.to_string_lossy().into_owned();
    // 按 Command 或 Query 选择公开 flags。
    if scenario.verb() == "apply" {
        // click/type 必须显式确认并要求 strict isolation。
        run_launcher(
            // 使用原样生产 launcher。
            layout,
            // 传递冻结公开参数。
            &[
                // 进入通用运行 surface。
                "run",
                // 使用统一 App facade。
                "app",
                // 元素 mutation 使用 apply。
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
    } else {
        // screenshot Query 不携带确认或 strict flag。
        run_launcher(
            // 使用原样生产 launcher。
            layout,
            // 传递 standard read 参数。
            &["run", "app", "read", "--input", &input],
        )
    }
}

// 在 click 已经越过 accepted 后向原样生产 launcher 发送显式取消。
fn run_cancelled_click(
    // 接收隔离生产布局。
    layout: &LauncherLayout,
    // 借用当前公开 session identity。
    session_id: &str,
    // 借用 current page identity。
    page_id: &str,
    // 借用 current element identity。
    element_id: &str,
) -> Output {
    // 写入 confirmed strict click input。
    let input = page_input(
        // 绑定当前布局。
        layout,
        // 使用取消测试标签。
        "cancelled-click",
        // 绑定稳定 click capability。
        "browser.element.click@1",
        // 绑定当前 session。
        session_id,
        // 使用长预算以便显式取消先于 deadline。
        json!({"pageId":page_id,"elementId":element_id,"timeoutMs":TIMEOUT_MS}),
    );
    // 转换为公开 launcher argv 文本。
    let input = input.to_string_lossy().into_owned();
    // 使用布局内固定文件作为 accepted 后 CDP dispatch 见证。
    let marker = layout.temporary.join("page-command-accepted.marker");
    // 构造原样生产 launcher 命令。
    let mut command = launcher_mode_command(
        // 使用当前隔离布局。
        layout,
        // 传递 confirmed strict click。
        &[
            // 进入统一运行 surface。
            "run",
            // 使用 App facade。
            "app",
            // click 使用 apply。
            "apply",
            // 指定 structured input。
            "--input",
            // 传递测试独占 input 路径。
            &input,
            // mutation 显式确认。
            "--confirm",
            // click 必须使用 strict isolation。
            "--strict-isolation",
        ],
        // runtime 保持 click 调用在途。
        "click-delay",
    );
    // 建立可被精确定向 Ctrl+Break 的独立进程组。
    command.creation_flags(CREATE_NEW_PROCESS_GROUP.0);
    // 启动当前测试唯一拥有的原样 launcher。
    let mut child = command
        // 启动精确 PowerShell 子进程。
        .spawn()
        // 启动失败时保留测试上下文。
        .unwrap_or_else(|error| panic!("cancelled click launcher should start: {error:?}"));
    // accepted 见证等待使用同一冻结 launcher 总预算。
    let marker_deadline = Instant::now() + LAUNCHER_PROCESS_TIMEOUT;
    // 在 click 实际越过 accepted 并到达 CDP 前不得发送取消。
    while !marker.is_file() {
        // 只执行非阻塞 owned 进程观察。
        let exited = child
            // 查询当前 launcher 状态。
            .try_wait()
            // 等待失败不得伪造 accepted。
            .unwrap_or_else(|error| {
                panic!("cancelled click launcher should be waitable: {error:?}")
            })
            // 映射退出事实。
            .is_some();
        // launcher 提前退出时收集唯一公开 stdout 供诊断。
        if exited {
            // 取得已经退出进程的完整管道输出。
            let output = child
                // 等待并收集已结束进程。
                .wait_with_output()
                // 输出收集失败不得掩盖原根因。
                .unwrap_or_else(|error| panic!("cancelled click output should exist: {error:?}"));
            // 只公开本就属于 launcher 公共边界的 stdout。
            panic!(
                // 固定说明后附加公开 JSON。
                "click launcher exited before accepted dispatch: {}",
                // 使用宽容 UTF-8 仅用于测试诊断。
                String::from_utf8_lossy(&output.stdout),
            );
        }
        // marker 等待不得扩张测试总预算。
        assert!(
            // 核对单调期限。
            Instant::now() < marker_deadline,
            // 使用固定诊断。
            "click did not reach accepted dispatch before the test deadline",
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
        "click cancellation event should be delivered"
    );
    // 在原总预算内收集结构化取消结果。
    collect_launcher_output(child)
}

// 验证 accepted 后错误的完整公开真值。
fn assert_accepted_action_error(
    // 接收原样 launcher 输出。
    output: &Output,
    // 接收当前故障场景。
    scenario: ActionFaultScenario,
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
        .unwrap_or_else(|| panic!("accepted page action failure should contain error"));
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
        .unwrap_or_else(|| panic!("accepted page action failure should contain details"));
    // accepted 后详情必须包含禁止自动重派字段。
    assert_exact_keys(
        // 传递详情对象。
        details,
        // 对齐公开页面动作错误矩阵。
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
    // 六项场景都必须已经业务接受。
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
    // click/type 保守标记 mutation，screenshot 始终只读。
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
    // 公开错误不得泄漏故障模式、文本输入或实现事实。
    assert_no_private_text(&value);
    // type 故障不得回显全部或部分输入文本。
    assert!(!value.to_string().contains("private-action-text"));
    // 失败结果不得携带部分成功 data。
    assert!(value.pointer("/error/data").is_none());
}

// 验证确定失败、期限、取消与断线保持 accepted 真值并收敛资源。
#[test]
fn production_launcher_projects_page_action_failures_and_unknown_without_retry() {
    // 独占当前登录会话的固定 Broker endpoint。
    let _test_guard = TEST_LOCK
        // 中毒表示前一场景已失败。
        .lock()
        // 不隐藏并发测试根因。
        .unwrap_or_else(|error| panic!("browser launcher lock should be available: {error:?}"));
    // 遍历封闭故障集合。
    for scenario in [
        // 覆盖 accepted click 确定失败。
        ActionFaultScenario::ClickFailed,
        // 覆盖 accepted type 确定失败。
        ActionFaultScenario::TypeFailed,
        // 覆盖 accepted screenshot 确定失败且只读。
        ActionFaultScenario::ScreenshotFailed,
        // 覆盖 accepted type 总 deadline unknown。
        ActionFaultScenario::TypeDeadline,
        // 覆盖 accepted click 显式取消 unknown。
        ActionFaultScenario::ClickCancelled,
        // 覆盖 accepted screenshot 浏览器断线 unknown。
        ActionFaultScenario::ScreenshotDisconnected,
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
        let page_id = navigate
            // 读取公开页面 identity。
            .get("pageId")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 成功必须签发页面。
            .unwrap_or_else(|| panic!("action fault setup should return pageId"))
            // 保留给下一 launcher。
            .to_owned();
        // 查询当前 page 并取得 request-bound element。
        let query = run_query(&layout, &session_id, &page_id);
        // 验证公开 query success。
        let query = page_success(
            // 传递 launcher 输出。
            &query,
            // 绑定稳定 query capability。
            "browser.page.query@1",
            // query 使用 read。
            "read",
            // 顶层绑定当前 session。
            &session_id,
        );
        // 提取首个可用 element identity。
        let element_id = query
            // 读取首个公开命中。
            .pointer("/matches/0/elementId")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 成功必须签发 element。
            .unwrap_or_else(|| panic!("action fault setup should return elementId"))
            // 保留给受测 launcher。
            .to_owned();
        // 执行当前受测页面 action。
        let output = if scenario.uses_launcher_cancellation() {
            // 通过真实 PowerShell/Rust launcher 取消链中断 accepted click。
            run_cancelled_click(&layout, &session_id, &page_id, &element_id)
        } else {
            // 其余故障通过封闭 runtime 模式同步执行。
            run_action(&layout, scenario, &session_id, &page_id, &element_id)
        };
        // 核对 accepted、final、mutation 与自动重派真值。
        assert_accepted_action_error(&output, scenario, &session_id);
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
            // 使用同一 caller session 发起显式 screenshot Query。
            let stale = run_page(
                // 使用同一隔离布局。
                &layout,
                // 使用独立 stale 标签。
                "stale-after-action-unknown",
                // screenshot 使用 read。
                "read",
                // 绑定稳定 screenshot capability。
                "browser.page.screenshot@1",
                // 绑定已经失效的 session。
                &session_id,
                // 使用旧 page 只验证 session 优先级。
                json!({"pageId":page_id,"timeoutMs":TIMEOUT_MS}),
                // Query 不需要确认。
                false,
                // Query 使用 standard。
                false,
            );
            // 必须在业务接受前返回 stale session。
            assert_preaccepted(
                // 传递公开错误输出。
                &stale,
                // 固定 stale session 分类。
                "STALE_SESSION",
                // 绑定 screenshot capability。
                "browser.page.screenshot@1",
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
        // 工具 profile 必须完整清理。
        wait_for_profiles_clean(&layout);
        // 终止本测试拥有的固定 Broker。
        broker.terminate_owned();
        // endpoint owner 必须退出。
        wait_for_no_process(&layout.broker);
        // 删除当前场景独占布局。
        layout.finish();
    }
}
