#![cfg(target_os = "windows")]

//! 验证原样生产 launcher 在业务接受前关闭页面动作错误矩阵。

// 导入真实 worker 路径与有界回收时间。
use std::{path::Path, time::Duration};

// 导入 provider-neutral JSON 构造器和值。
use serde_json::{Value, json};

// 导入父集成测试的原样 launcher、生命周期与资源见证。
use super::{
    // 安装原样 launcher 与全部 Rust siblings。
    LauncherLayout,
    // 串行化固定 Broker endpoint 场景。
    TEST_LOCK,
    // 使用真实 Rust Browser Session worker。
    WORKER_SOURCE,
    // 验证公开 close 成功。
    assert_close_success,
    // 验证并取得公开 open identity。
    assert_open_success,
    // 发现唯一公开 host identity。
    discover_host,
    // 证明业务前拒绝没有启动固定 Broker。
    matching_processes,
    // 解析唯一公开 stdout JSON。
    output_json,
    // 复用页面导航、查询与 success 投影。
    page::{TIMEOUT_MS, page_success, run_navigate},
    // 复用页面 accepted 前错误断言与调用器。
    page_errors::{assert_preaccepted, run_page},
    // 经原样 launcher 执行生命周期 operation。
    run_lifecycle,
    // 等待精确镜像进程退出。
    wait_for_no_process,
    // 绑定精确镜像进程见证。
    wait_for_process,
    // 等待工具自有 profile 完整回收。
    wait_for_profiles_clean,
};

// 固定 canonical 但不存在的 Browser Session identity。
const SESSION_ID: &str = "s2:bs:11111111111111111111111111111111";
// 固定 canonical 但不存在的 Browser Page identity。
const PAGE_ID: &str = "s2:bp:22222222222222222222222222222222";
// 固定 canonical 但不存在的 Browser Element identity。
const ELEMENT_ID: &str = "s2:be:33333333333333333333333333333333";

// 验证确认、identity 与严格 input 在 Broker 访问前失败闭合。
#[test]
fn production_launcher_rejects_page_action_inputs_before_broker_access() {
    // 独占当前登录会话的固定 Broker endpoint。
    let _test_guard = TEST_LOCK
        // 中毒表示前一场景已经失败。
        .lock()
        // 不隐藏并发测试根因。
        .unwrap_or_else(|error| panic!("browser launcher lock should be available: {error:?}"));
    // 安装原样 launcher 与真实 Rust siblings，但不启动任何进程。
    let layout = LauncherLayout::install(Path::new(WORKER_SOURCE));
    // 未确认 click 使用 wrong-kind target 与非法 input 证明确认优先。
    let unconfirmed_click = run_page(
        // 使用隔离布局。
        &layout,
        // 使用独立输入标签。
        "unconfirmed-click",
        // click 使用 apply。
        "apply",
        // 绑定稳定 click capability。
        "browser.element.click@1",
        // 使用 wrong-kind 通用窗口 identity。
        "s2:w:1111111111111111",
        // 同时提供非法 identity 与私有字段。
        json!({"pageId":"private-page","elementId":"private-element","credential":"private-secret"}),
        // 不确认 mutation。
        false,
        // 显式要求严格隔离。
        true,
    );
    // 确认错误不得回显 wrong-kind target 或 input。
    assert_preaccepted(
        // 传递公开输出。
        &unconfirmed_click,
        // 锁定 confirmation-first。
        "CONFIRMATION_REQUIRED",
        // 绑定原 capability。
        "browser.element.click@1",
        // wrong-kind target 不得回显。
        None,
    );
    // 未确认请求不得启动固定 Broker。
    assert!(matching_processes(&layout.broker, false).is_empty());
    // 未确认 type 必须先于敏感文本和 malformed target 解析。
    let unconfirmed_type = run_page(
        // 使用同一隔离布局。
        &layout,
        // 使用独立输入标签。
        "unconfirmed-type",
        // type 使用 apply。
        "apply",
        // 绑定稳定 type capability。
        "browser.element.type@1",
        // 使用 malformed Browser Session identity。
        "s2:bs:ABCDEF",
        // 提供必须完全保密的文本。
        json!({"pageId":PAGE_ID,"elementId":ELEMENT_ID,"text":"do-not-echo-private-text","replace":true}),
        // 不确认 mutation。
        false,
        // 显式要求严格隔离。
        true,
    );
    // 确认必须保持最高优先级。
    assert_preaccepted(
        // 传递公开输出。
        &unconfirmed_type,
        // 锁定 confirmation-first。
        "CONFIRMATION_REQUIRED",
        // 绑定原 capability。
        "browser.element.type@1",
        // malformed target 不得回显。
        None,
    );
    // 完整公开 JSON 不得回显 type 文本。
    assert!(
        !output_json(&unconfirmed_type)
            .to_string()
            .contains("do-not-echo-private-text")
    );
    // 未确认 type 不得启动固定 Broker。
    assert!(matching_processes(&layout.broker, false).is_empty());
    // 已确认 click 必须在 Broker 前拒绝 malformed element 与私有扩张。
    let invalid_click = run_page(
        // 使用同一隔离布局。
        &layout,
        // 使用独立输入标签。
        "invalid-click",
        // click 使用 apply。
        "apply",
        // 绑定稳定 click capability。
        "browser.element.click@1",
        // 使用 canonical caller target。
        SESSION_ID,
        // 提供 malformed element 和私有 worker ref。
        json!({"pageId":PAGE_ID,"elementId":"s2:be:ABCDEF","elementRef":"w1:be:private"}),
        // 已确认 mutation。
        true,
        // 使用 strict 进入 input parser。
        true,
    );
    // 公开 parser 必须返回不回显 input 的参数错误。
    assert_preaccepted(
        // 传递公开输出。
        &invalid_click,
        // 固定参数错误。
        "INVALID_ARGUMENT",
        // 绑定原 capability。
        "browser.element.click@1",
        // canonical target 可安全回显。
        Some(SESSION_ID),
    );
    // input parser 不得连接或启动 Broker。
    assert!(matching_processes(&layout.broker, false).is_empty());
    // 已确认 type 必须在 Broker 前按 UTF-8 字节拒绝超限文本。
    let invalid_type = run_page(
        // 使用同一隔离布局。
        &layout,
        // 使用独立输入标签。
        "invalid-type",
        // type 使用 apply。
        "apply",
        // 绑定稳定 type capability。
        "browser.element.type@1",
        // 使用 canonical caller target。
        SESSION_ID,
        // 多字节文本超过 16384 UTF-8 bytes。
        json!({"pageId":PAGE_ID,"elementId":ELEMENT_ID,"text":"界".repeat(5_462),"replace":false}),
        // 已确认 mutation。
        true,
        // 使用 strict 进入 input parser。
        true,
    );
    // 超限文本只公开固定参数错误。
    assert_preaccepted(
        // 传递公开输出。
        &invalid_type,
        // 固定参数错误。
        "INVALID_ARGUMENT",
        // 绑定原 capability。
        "browser.element.type@1",
        // canonical target 可安全回显。
        Some(SESSION_ID),
    );
    // 超限文本不得进入公开 JSON。
    assert!(!output_json(&invalid_type).to_string().contains('界'));
    // type parser 不得启动固定 Broker。
    assert!(matching_processes(&layout.broker, false).is_empty());
    // screenshot 必须拒绝路径、格式和 CDP 扩张。
    let invalid_screenshot = run_page(
        // 使用同一隔离布局。
        &layout,
        // 使用独立输入标签。
        "invalid-screenshot",
        // screenshot 使用 read。
        "read",
        // 绑定稳定 screenshot capability。
        "browser.page.screenshot@1",
        // 使用 canonical caller target。
        SESSION_ID,
        // 提供全部禁止的输出控制字段。
        json!({"pageId":PAGE_ID,"path":"private.png","format":"jpeg","cdpMethod":"Page.captureScreenshot"}),
        // Query 不需要确认。
        false,
        // Query 使用 standard。
        false,
    );
    // screenshot 私有扩张必须在 Broker 前失败。
    assert_preaccepted(
        // 传递公开输出。
        &invalid_screenshot,
        // 固定参数错误。
        "INVALID_ARGUMENT",
        // 绑定原 capability。
        "browser.page.screenshot@1",
        // canonical target 可安全回显。
        Some(SESSION_ID),
    );
    // screenshot parser 不得启动固定 Broker。
    assert!(matching_processes(&layout.broker, false).is_empty());
    // screenshot wrong-kind target 必须由专属 Module 业务前拒绝。
    let wrong_kind = run_page(
        // 使用同一隔离布局。
        &layout,
        // 使用独立输入标签。
        "wrong-kind-screenshot",
        // screenshot 使用 read。
        "read",
        // 绑定稳定 screenshot capability。
        "browser.page.screenshot@1",
        // 使用普通窗口 identity。
        "s2:w:1111111111111111",
        // input 本身合法以聚焦 target 门禁。
        json!({"pageId":PAGE_ID}),
        // Query 不需要确认。
        false,
        // Query 使用 standard。
        false,
    );
    // wrong-kind target 必须固定为参数错误且不回显。
    assert_preaccepted(
        // 传递公开输出。
        &wrong_kind,
        // 固定参数错误。
        "INVALID_ARGUMENT",
        // 绑定原 capability。
        "browser.page.screenshot@1",
        // wrong-kind target 不得回显。
        None,
    );
    // 全部业务前错误都不得启动固定 Broker。
    assert!(matching_processes(&layout.broker, false).is_empty());
    // 删除测试独占布局。
    layout.finish();
}

// 验证 live session 的 stale page/element 在业务接受前失败并可确定回收。
#[test]
fn production_launcher_rejects_stale_page_and_element_before_action_acceptance() {
    // 独占当前登录会话的固定 Broker endpoint。
    let _test_guard = TEST_LOCK
        // 中毒表示前一场景已经失败。
        .lock()
        // 不隐藏并发测试根因。
        .unwrap_or_else(|error| panic!("browser launcher lock should be available: {error:?}"));
    // 安装原样 launcher、生产 Broker、真实 worker 与 runtime fixture。
    let layout = LauncherLayout::install(Path::new(WORKER_SOURCE));
    // 经公开 sessions 发现当前 host。
    let host_id = discover_host(&layout);
    // 经原样 launcher 创建 live Browser Session。
    let open = run_lifecycle(
        // 使用当前隔离布局。
        &layout,
        // open 使用 create。
        "create",
        // 绑定稳定 open capability。
        "browser.session.open@1",
        // 绑定当前 host。
        &host_id,
        // mutation 显式确认。
        true,
        // 使用完整总预算。
        TIMEOUT_MS,
    );
    // 验证并取得公开 session identity。
    let session_id = assert_open_success(&open, &host_id);
    // 绑定本测试启动的固定 Broker 供最终确定回收。
    let broker = wait_for_process(&layout.broker, true);
    // 绑定当前 live worker 供资源见证。
    let worker = wait_for_process(&layout.worker, false);
    // 绑定 runtime fixture descendant 供资源见证。
    let runtime = wait_for_process(&layout.runtime, false);
    // 完成第一代页面导航。
    let first = run_navigate(&layout, &session_id);
    // 验证第一代页面成功。
    let first = page_success(&first, "browser.page.navigate@1", "apply", &session_id);
    // 提取第一代页面 identity。
    let first_page = first
        // 读取公开 pageId。
        .get("pageId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 导航成功必须签发 page。
        .unwrap_or_else(|| panic!("first navigate should return pageId"))
        // 保留给 stale 调用。
        .to_owned();
    // 完成第二代页面导航并使第一代立即 stale。
    let second = run_navigate(&layout, &session_id);
    // 验证第二代页面成功。
    let second = page_success(&second, "browser.page.navigate@1", "apply", &session_id);
    // 提取当前页面 identity。
    let current_page = second
        // 读取公开 pageId。
        .get("pageId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 第二次导航必须签发 page。
        .unwrap_or_else(|| panic!("second navigate should return pageId"))
        // 保留给后续调用。
        .to_owned();
    // 使用第一代页面与 fake element 点击。
    let stale_page = run_page(
        // 使用同一固定 Broker。
        &layout,
        // 使用独立输入标签。
        "stale-click-page",
        // click 使用 apply。
        "apply",
        // 绑定稳定 click capability。
        "browser.element.click@1",
        // 绑定当前 live session。
        &session_id,
        // 第一代 page 必须优先于 element 判 stale。
        json!({"pageId":first_page,"elementId":ELEMENT_ID,"timeoutMs":TIMEOUT_MS}),
        // 已确认 mutation。
        true,
        // 使用 strict isolation。
        true,
    );
    // stale page 必须保持业务前真值。
    assert_preaccepted(
        // 传递公开输出。
        &stale_page,
        // 固定 stale page 分类。
        "STALE_PAGE",
        // 绑定原 capability。
        "browser.element.click@1",
        // caller canonical session 可安全回显。
        Some(&session_id),
    );
    // 使用当前页面与不存在元素执行 type。
    let stale_element = run_page(
        // 使用同一固定 Broker。
        &layout,
        // 使用独立输入标签。
        "stale-type-element",
        // type 使用 apply。
        "apply",
        // 绑定稳定 type capability。
        "browser.element.type@1",
        // 绑定当前 live session。
        &session_id,
        // fake canonical element 必须形成 STALE_ELEMENT。
        json!({"pageId":current_page,"elementId":ELEMENT_ID,"text":"never-dispatched-private-text","replace":true,"timeoutMs":TIMEOUT_MS}),
        // 已确认 mutation。
        true,
        // 使用 strict isolation。
        true,
    );
    // stale element 必须保持业务前真值。
    assert_preaccepted(
        // 传递公开输出。
        &stale_element,
        // 固定 stale element 分类。
        "STALE_ELEMENT",
        // 绑定原 capability。
        "browser.element.type@1",
        // caller canonical session 可安全回显。
        Some(&session_id),
    );
    // stale element 输出不得回显 type 文本。
    assert!(
        !output_json(&stale_element)
            .to_string()
            .contains("never-dispatched-private-text")
    );
    // screenshot 使用第一代页面同样必须 stale 且无 mutation。
    let stale_screenshot = run_page(
        // 使用同一固定 Broker。
        &layout,
        // 使用独立输入标签。
        "stale-screenshot-page",
        // screenshot 使用 read。
        "read",
        // 绑定稳定 screenshot capability。
        "browser.page.screenshot@1",
        // 绑定当前 live session。
        &session_id,
        // 使用第一代 stale page。
        json!({"pageId":first_page,"timeoutMs":TIMEOUT_MS}),
        // Query 不需要确认。
        false,
        // Query 使用 standard。
        false,
    );
    // stale screenshot page 必须保持业务前只读真值。
    assert_preaccepted(
        // 传递公开输出。
        &stale_screenshot,
        // 固定 stale page 分类。
        "STALE_PAGE",
        // 绑定原 capability。
        "browser.page.screenshot@1",
        // caller canonical session 可安全回显。
        Some(&session_id),
    );
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
    // 显式 close 后同一 worker 必须退出。
    assert!(worker.wait_exited(Duration::from_secs(5)));
    // 显式 close 后 runtime descendant 必须退出。
    assert!(runtime.wait_exited(Duration::from_secs(5)));
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
