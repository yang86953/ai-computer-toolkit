#![cfg(target_os = "windows")]

//! 验证原样生产 launcher 在 Browser Session Broker 访问前关闭页面错误矩阵。

// 导入生产 worker 路径类型与子进程输出。
use std::{
    // 定位真实 Rust worker sibling。
    path::Path,
    // 保存原样 launcher 输出。
    process::Output,
};

// 导入 provider-neutral JSON 构造器和值。
use serde_json::{Value, json};

// 导入父集成测试的原样 launcher 与进程见证基础设施。
use super::{
    // 安装原样 launcher 与全部 Rust siblings。
    LauncherLayout,
    // 串行化固定 Broker endpoint 场景。
    TEST_LOCK,
    // 使用真实 Rust Browser Session worker 镜像。
    WORKER_SOURCE,
    // 递归拒绝公开输出中的私有实现文本。
    assert_no_private_text,
    // 证明业务前拒绝没有启动固定 Broker。
    matching_processes,
    // 解析唯一公开 stdout JSON。
    output_json,
    // 执行原样生产 launcher。
    run_launcher,
};
// 复用页面测试的严格 structured input 与精确字段断言。
use super::page::{PAGE_URL, TIMEOUT_MS, assert_exact_keys, page_input};

// 固定 canonical 但不存在的 Browser Session identity。
const SESSION_ID: &str = "s2:bs:11111111111111111111111111111111";
// 固定 canonical 但不存在的 Browser Page identity。
const PAGE_ID: &str = "s2:bp:22222222222222222222222222222222";

// 经原样 launcher 执行任意页面公开调用形状。
pub(super) fn run_page(
    // 接收隔离安装布局。
    layout: &LauncherLayout,
    // 接收测试文件标签。
    label: &str,
    // 接收 generic apply 或 read。
    verb: &str,
    // 接收稳定页面 capability。
    capability: &str,
    // 接收调用方顶层目标。
    session_id: &str,
    // 接收待验证公开 input。
    input: Value,
    // 接收逐操作确认事实。
    confirmed: bool,
    // 接收严格隔离事实。
    strict: bool,
) -> Output {
    // 写入只含公开 wrapper 的测试输入。
    let input = page_input(layout, label, capability, session_id, input);
    // 转换为 PowerShell argv 文本。
    let input = input.to_string_lossy().into_owned();
    // 按两个公开 flag 的封闭组合选择 argv。
    match (confirmed, strict) {
        // 导航成功候选显式确认并要求严格隔离。
        (true, true) => run_launcher(
            // 使用原样生产 launcher。
            layout,
            // 传递完整公开参数。
            &[
                // 进入通用运行 surface。
                "run",
                // 使用统一 App facade。
                "app",
                // 传递 generic verb。
                verb,
                // 指定 structured input。
                "--input",
                // 传递测试独占路径。
                &input,
                // 显式确认 mutation。
                "--confirm",
                // 要求严格零打扰。
                "--strict-isolation",
            ],
        ),
        // 未确认导航仍显式要求严格隔离以验证确认优先。
        (false, true) => run_launcher(
            // 使用原样生产 launcher。
            layout,
            // 不携带 confirm。
            &[
                // 进入通用运行 surface。
                "run",
                // 使用统一 App facade。
                "app",
                // 传递 generic verb。
                verb,
                // 指定 structured input。
                "--input",
                // 传递测试独占路径。
                &input,
                // 仍明确要求 strict。
                "--strict-isolation",
            ],
        ),
        // standard confirmed 用于证明导航隔离要求在 Broker 前拒绝。
        (true, false) => run_launcher(
            // 使用原样生产 launcher。
            layout,
            // 只携带 confirm。
            &["run", "app", verb, "--input", &input, "--confirm"],
        ),
        // 页面 Query 不需要确认或严格 flag。
        (false, false) => run_launcher(
            // 使用原样生产 launcher。
            layout,
            // 使用 standard read 参数。
            &["run", "app", verb, "--input", &input],
        ),
    }
}

// 验证业务接受前页面错误的精确公开真值。
pub(super) fn assert_preaccepted(
    // 接收原样 launcher 输出。
    output: &Output,
    // 接收预期公开错误码。
    code: &str,
    // 接收预期 capability。
    capability: &str,
    // 接收仅在 canonical 时允许回显的 target。
    target_id: Option<&str>,
) {
    // 业务前拒绝必须非零退出。
    assert!(!output.status.success());
    // 解析唯一公开 JSON 文档。
    let value = output_json(output);
    // 顶层错误 envelope 只允许 ok 与 error。
    assert_exact_keys(&value, &["ok", "error"]);
    // error 只允许 code、message 与公共 details。
    let error = value
        // 读取统一 error 对象。
        .get("error")
        // 缺失错误对象必须失败。
        .unwrap_or_else(|| panic!("public page error should contain error"));
    // 错误对象不得扩张内部阶段字段。
    assert_exact_keys(error, &["code", "message", "details"]);
    // 核对稳定公开错误码。
    assert_eq!(error.get("code").and_then(Value::as_str), Some(code));
    // 取得冻结错误详情。
    let details = error
        // 读取 details。
        .get("details")
        // details 必须存在。
        .unwrap_or_else(|| panic!("public page error should contain details"));
    // canonical target 决定是否允许 targetId 字段。
    let detail_keys: &[&str] = if target_id.is_some() {
        // canonical target 必须逐字回显。
        &[
            // 回显公开 capability。
            "capability",
            // 回显 caller 已知 target。
            "targetId",
            // 固定未派发 outcome。
            "outcome",
            // 固定未接受。
            "accepted",
            // 拒绝本身是可信 final。
            "finalStateReached",
            // 修正调用后可安全人工重试。
            "retrySafe",
            // 业务前拒绝不改变目标。
            "targetMayHaveMutated",
        ]
    } else {
        // wrong-kind target 不得被错误回显。
        &[
            // 回显公开 capability。
            "capability",
            // 固定未派发 outcome。
            "outcome",
            // 固定未接受。
            "accepted",
            // 拒绝本身是可信 final。
            "finalStateReached",
            // 修正调用后可安全人工重试。
            "retrySafe",
            // 业务前拒绝不改变目标。
            "targetMayHaveMutated",
        ]
    };
    // details 不得包含 Broker、阶段或自动重试扩张字段。
    assert_exact_keys(details, detail_keys);
    // capability 必须绑定原调用。
    assert_eq!(
        // 读取 capability。
        details.get("capability").and_then(Value::as_str),
        // 比较预期 ID。
        Some(capability),
    );
    // 只在 canonical 时逐字回显 caller target。
    assert_eq!(
        // 读取可选 targetId。
        details.get("targetId").and_then(Value::as_str),
        // 比较预期可选值。
        target_id,
    );
    // 业务前拒绝必须固定 not-dispatched。
    assert_eq!(
        // 读取 outcome。
        details.get("outcome").and_then(Value::as_str),
        // 固定未派发。
        Some("not-dispatched"),
    );
    // 尚未进入 Broker 业务接受点。
    assert_eq!(details.get("accepted"), Some(&Value::Bool(false)));
    // 拒绝本身是可信 final。
    assert_eq!(details.get("finalStateReached"), Some(&Value::Bool(true)));
    // 修正确认、隔离或 input 后可以人工重试。
    assert_eq!(details.get("retrySafe"), Some(&Value::Bool(true)));
    // 业务前拒绝绝不改变 session/page。
    assert_eq!(
        // 读取 mutation 事实。
        details.get("targetMayHaveMutated"),
        // 固定没有变化。
        Some(&Value::Bool(false)),
    );
    // 错误不得回显输入或任何私有实现文本。
    assert_no_private_text(&value);
}

// 验证确认、隔离和严格 input 错误均在 Broker 访问前失败闭合。
#[test]
fn production_launcher_rejects_page_inputs_before_broker_access() {
    // 独占当前登录会话的固定 Broker endpoint。
    let _test_guard = TEST_LOCK
        // 中毒表示前一场景已经失败。
        .lock()
        // 不隐藏并发测试根因。
        .unwrap_or_else(|error| panic!("browser launcher lock should be available: {error:?}"));
    // 安装原样 launcher 与真实 Rust siblings，但不启动任何进程。
    let layout = LauncherLayout::install(Path::new(WORKER_SOURCE));
    // 未确认导航使用 wrong-kind target 与非法 input 证明确认优先。
    let unconfirmed = run_page(
        // 使用隔离布局。
        &layout,
        // 使用独立输入标签。
        "unconfirmed",
        // 导航使用 apply。
        "apply",
        // 绑定稳定导航 capability。
        "browser.page.navigate@1",
        // 使用 wrong-kind 通用窗口 identity。
        "s2:w:1111111111111111",
        // 同时提供非法 scheme 与私有字段。
        json!({ "url": "file:///private", "credential": "private-secret" }),
        // 不确认 mutation。
        false,
        // 显式要求严格隔离。
        true,
    );
    // 确认错误不得回显 wrong-kind target 或 input。
    assert_preaccepted(
        // 传递公开输出。
        &unconfirmed,
        // 锁定 confirmation-first。
        "CONFIRMATION_REQUIRED",
        // 绑定原 capability。
        "browser.page.navigate@1",
        // wrong-kind target 不得回显。
        None,
    );
    // 未确认请求不得启动固定 Broker。
    assert!(matching_processes(&layout.broker, false).is_empty());
    // 已确认 standard 导航必须在 target 与 Broker 探测前拒绝。
    let standard = run_page(
        // 使用同一隔离布局。
        &layout,
        // 使用独立输入标签。
        "standard",
        // 导航使用 apply。
        "apply",
        // 绑定稳定导航 capability。
        "browser.page.navigate@1",
        // 使用 canonical caller target。
        SESSION_ID,
        // 输入本身合法，聚焦隔离门禁。
        json!({ "url": PAGE_URL, "timeoutMs": TIMEOUT_MS }),
        // 已确认 mutation。
        true,
        // 故意使用 standard。
        false,
    );
    // standard 导航属于公开调用参数错误。
    assert_preaccepted(
        // 传递公开输出。
        &standard,
        // 使用页面错误矩阵允许的分类。
        "INVALID_ARGUMENT",
        // 绑定原 capability。
        "browser.page.navigate@1",
        // canonical target 可安全回显。
        Some(SESSION_ID),
    );
    // 隔离门禁不得启动固定 Broker。
    assert!(matching_processes(&layout.broker, false).is_empty());
    // strict confirmed 导航仍必须拒绝非法 URL 和私有扩张。
    let invalid_navigate = run_page(
        // 使用同一布局。
        &layout,
        // 使用独立输入标签。
        "invalid-navigate",
        // 导航使用 apply。
        "apply",
        // 绑定稳定导航 capability。
        "browser.page.navigate@1",
        // 使用 canonical caller target。
        SESSION_ID,
        // 提供禁止的 scheme 与 Cookie 字段。
        json!({ "url": "file:///private", "cookie": "private-cookie", "timeoutMs": TIMEOUT_MS }),
        // 已确认 mutation。
        true,
        // 使用 strict 进入 input parser。
        true,
    );
    // 公开 parser 必须返回不回显 input 的参数错误。
    assert_preaccepted(
        // 传递公开输出。
        &invalid_navigate,
        // 固定参数错误。
        "INVALID_ARGUMENT",
        // 绑定原 capability。
        "browser.page.navigate@1",
        // canonical target 可安全回显。
        Some(SESSION_ID),
    );
    // input parser 不得连接或启动 Broker。
    assert!(matching_processes(&layout.broker, false).is_empty());
    // 页面 Query 必须拒绝 CSS 与 credential 扩张。
    let invalid_query = run_page(
        // 使用同一布局。
        &layout,
        // 使用独立输入标签。
        "invalid-query",
        // Query 使用 read。
        "read",
        // 绑定页面 query capability。
        "browser.page.query@1",
        // 使用 canonical caller target。
        SESSION_ID,
        // 提供 canonical page 但禁止的 selector/private 字段。
        json!({
            // page identity 形状合法以聚焦 selector。
            "pageId": PAGE_ID,
            // CSS 不属于 provider-neutral selector。
            "selector": { "css": "#private" },
            // credential 不得进入公开 input。
            "credential": "private-secret",
            // 使用合法预算。
            "timeoutMs": TIMEOUT_MS,
        }),
        // Query 不需要确认。
        false,
        // Query 保持 standard。
        false,
    );
    // Query input 错误保持只读业务前真值。
    assert_preaccepted(
        // 传递公开输出。
        &invalid_query,
        // 固定参数错误。
        "INVALID_ARGUMENT",
        // 绑定原 capability。
        "browser.page.query@1",
        // canonical target 可安全回显。
        Some(SESSION_ID),
    );
    // 私有 selector 拒绝不得启动 Broker。
    assert!(matching_processes(&layout.broker, false).is_empty());
    // 页面 wait 必须拒绝未冻结条件与 native 扩张。
    let invalid_wait = run_page(
        // 使用同一布局。
        &layout,
        // 使用独立输入标签。
        "invalid-wait",
        // Query 使用 read。
        "read",
        // 绑定页面 wait capability。
        "browser.page.wait@1",
        // 使用 canonical caller target。
        SESSION_ID,
        // 提供禁止的条件和 native 字段。
        json!({
            // page identity 形状合法。
            "pageId": PAGE_ID,
            // network-idle 不在冻结条件集合。
            "condition": { "kind": "network-idle" },
            // native ID 不得进入公开 input。
            "nativeId": "private-native",
            // 使用合法预算。
            "timeoutMs": TIMEOUT_MS,
        }),
        // Query 不需要确认。
        false,
        // Query 保持 standard。
        false,
    );
    // wait input 错误保持只读业务前真值。
    assert_preaccepted(
        // 传递公开输出。
        &invalid_wait,
        // 固定参数错误。
        "INVALID_ARGUMENT",
        // 绑定原 capability。
        "browser.page.wait@1",
        // canonical target 可安全回显。
        Some(SESSION_ID),
    );
    // native input 拒绝不得启动 Broker。
    assert!(matching_processes(&layout.broker, false).is_empty());
    // 没有进程启动时隔离布局应可直接删除。
    layout.finish();
}
