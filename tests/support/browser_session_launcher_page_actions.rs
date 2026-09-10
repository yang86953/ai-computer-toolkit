#![cfg(target_os = "windows")]

//! 验证原样生产 launcher 的公开元素点击、文本输入与页面截图成功纵切。

// 导入生产 worker 路径与有界等待类型。
use std::{path::Path, time::Duration};

// 导入 provider-neutral JSON 构造器和值。
use serde_json::{Value, json};

// 导入父集成测试已经验证的原样 launcher 基础设施。
use super::{
    // 安装原样 launcher 与 Rust siblings。
    LauncherLayout,
    // 串行化固定 Broker endpoint 场景。
    TEST_LOCK,
    // 使用真实 Rust Browser Session worker。
    WORKER_SOURCE,
    // 验证公开 close 成功。
    assert_close_success,
    // 递归拒绝私有实现文本。
    assert_no_private_text,
    // 验证并取得公开 open identity。
    assert_open_success,
    // 发现唯一公开 host identity。
    discover_host,
    // 解析唯一公开 stdout JSON。
    output_json,
    // 导入页面测试的共享公开输入和成功校验。
    page::{
        // 使用 fixture 固定页面 URL。
        PAGE_URL,
        // 使用覆盖完整调用链的总预算。
        TIMEOUT_MS,
        // 断言精确字段集合。
        assert_exact_keys,
        // 写入统一 App structured input。
        page_input,
        // 验证页面导航或查询成功。
        page_success,
        // 经原样 launcher 执行页面导航。
        run_navigate,
        // 经原样 launcher 执行页面查询。
        run_page_query,
    },
    // 经原样 launcher 执行固定 argv。
    run_launcher,
    // 经原样 launcher 执行生命周期 operation。
    run_lifecycle,
    // 等待精确镜像进程退出。
    wait_for_no_process,
    // 绑定精确镜像进程见证。
    wait_for_process,
    // 等待工具自有 profile 完整回收。
    wait_for_profiles_clean,
};

// 经原样生产 launcher 执行页面元素 mutation。
fn run_element_action(
    // 接收当前测试独占布局。
    layout: &LauncherLayout,
    // 接收测试文件标签。
    label: &str,
    // 接收稳定元素 capability。
    capability: &str,
    // 接收公开 Browser Session identity。
    session_id: &str,
    // 接收严格 provider-neutral 元素 input。
    input: Value,
) -> std::process::Output {
    // 写入不含 launcher 私有控制字段的 structured input。
    let input = page_input(layout, label, capability, session_id, input);
    // 转换为 PowerShell argv 文本。
    let input = input.to_string_lossy().into_owned();
    // click/type 通过 generic apply、显式确认与严格隔离执行。
    run_launcher(
        // 使用隔离生产布局。
        layout,
        // 使用冻结公开参数。
        &[
            // 进入通用运行 surface。
            "run",
            // 使用统一 App facade。
            "app",
            // 元素 mutation 使用 apply。
            "apply",
            // 传递 structured input 文件。
            "--input",
            // 传递测试独占路径。
            &input,
            // mutation 显式确认。
            "--confirm",
            // 页面动作要求严格零打扰。
            "--strict-isolation",
        ],
    )
}

// 经原样生产 launcher 执行页面截图 Query。
fn run_screenshot(
    // 接收当前测试独占布局。
    layout: &LauncherLayout,
    // 接收公开 Browser Session identity。
    session_id: &str,
    // 接收当前公开 page identity。
    page_id: &str,
) -> std::process::Output {
    // 写入只含当前页面与总预算的冻结 input。
    let input = page_input(
        // 使用当前布局。
        layout,
        // 使用截图文件标签。
        "screenshot",
        // 绑定稳定截图 capability。
        "browser.page.screenshot@1",
        // 绑定当前 live session。
        session_id,
        // 不传路径、格式或私有 CDP 参数。
        json!({"pageId":page_id,"timeoutMs":TIMEOUT_MS}),
    );
    // 转换为 PowerShell argv 文本。
    let input = input.to_string_lossy().into_owned();
    // screenshot 使用 standard/background-preferred 的 generic read。
    run_launcher(
        // 使用隔离生产布局。
        layout,
        // Query 不携带确认或严格 flag。
        &["run", "app", "read", "--input", &input],
    )
}

// 解析并验证三项页面动作成功共有的公开 envelope。
fn action_success(
    // 接收 launcher 子进程输出。
    output: &std::process::Output,
    // 接收预期 capability。
    capability: &str,
    // 接收预期 generic verb。
    verb: &str,
    // 接收预期顶层 target。
    session_id: &str,
) -> Value {
    // 页面动作必须成功退出。
    assert!(output.status.success(), "public page action should succeed");
    // 解析唯一公开 JSON 文档。
    let value = output_json(output);
    // 顶层成功 envelope 只能包含冻结字段。
    assert_exact_keys(
        // 传递完整公开结果。
        &value,
        // 对齐 browser-page-action-result schema。
        &[
            // System 证明真实执行域。
            "executionRealm",
            // System 证明要求域。
            "requiredExecutionRealm",
            // System 证明域已认证。
            "executionRealmCertified",
            // 回显调用隔离要求。
            "isolationRequirement",
            // 回显宿主影响策略。
            "hostImpactPolicy",
            // 固定成功标记。
            "ok",
            // 固定 App surface。
            "app",
            // 固定 generic verb。
            "verb",
            // 固定页面 capability。
            "capability",
            // 回显 exact Browser Session target。
            "targetId",
            // 保存 operation data。
            "data",
            // 保存前景与 targeting 证据。
            "meta",
        ],
    );
    // 统一 App facade 必须成功。
    assert_eq!(value.get("ok"), Some(&Value::Bool(true)));
    // 核对 generic verb。
    assert_eq!(value.get("verb").and_then(Value::as_str), Some(verb));
    // 核对稳定 capability。
    assert_eq!(
        // 读取顶层 capability。
        value.get("capability").and_then(Value::as_str),
        // 比较当前受测 ID。
        Some(capability),
    );
    // 顶层 target 必须保持调用方公开 session。
    assert_eq!(
        // 读取 facade target。
        value.get("targetId").and_then(Value::as_str),
        // 比较原公开 session。
        Some(session_id),
    );
    // 三项操作都必须由认证隔离域完成。
    assert_eq!(
        // 读取真实执行域。
        value.get("executionRealm").and_then(Value::as_str),
        // 固定为 isolated worker 公共枚举。
        Some("isolated-worker"),
    );
    // Command 使用 strict，Query 使用 standard。
    let (isolation, host_impact) = if verb == "apply" {
        // click/type 固定严格零干扰。
        ("strict", "strict-no-interference")
    } else {
        // screenshot 固定后台优先。
        ("standard", "background-preferred")
    };
    // 核对冻结隔离要求。
    assert_eq!(
        // 读取公开隔离字段。
        value.get("isolationRequirement").and_then(Value::as_str),
        // 比较当前 operation 的冻结值。
        Some(isolation),
    );
    // 核对冻结宿主影响策略。
    assert_eq!(
        // 读取公开宿主影响字段。
        value.get("hostImpactPolicy").and_then(Value::as_str),
        // 比较当前 operation 的冻结值。
        Some(host_impact),
    );
    // 页面 facade 使用专属目标描述。
    assert_eq!(
        // 读取公开 targeting 元数据。
        value.pointer("/meta/targeting").and_then(Value::as_str),
        // 对齐冻结结果 schema。
        Some("opaque exact browser session"),
    );
    // 主机前景必须保持不变。
    assert_eq!(
        // 读取公开前景证据。
        value.pointer("/meta/foreground/unchanged"),
        // 固定零前景影响。
        Some(&Value::Bool(true)),
    );
    // 公开 action success 不得泄漏私有实现文本。
    assert_no_private_text(&value);
    // 取得公开 data 供 operation 专属断言。
    value
        // 读取 data。
        .get("data")
        // success 缺失 data 必须失败。
        .unwrap_or_else(|| panic!("public page action success should contain data"))
        // 复制以脱离完整 envelope。
        .clone()
}

// 验证 Command 成功共有的冻结真值。
fn assert_command_truth(data: &Value, capability: &str, action: &str) {
    // capability 必须逐字回显。
    assert_eq!(
        data.get("capability").and_then(Value::as_str),
        Some(capability)
    );
    // 领域动作必须逐字回显。
    assert_eq!(data.get("action").and_then(Value::as_str), Some(action));
    // 成功必须业务接受。
    assert_eq!(data.get("accepted"), Some(&Value::Bool(true)));
    // 成功必须取得可信 final。
    assert_eq!(data.get("finalStateReached"), Some(&Value::Bool(true)));
    // 元素 mutation 保守标记目标可能改变。
    assert_eq!(data.get("targetMayHaveMutated"), Some(&Value::Bool(true)));
    // 固定路线不得改变宿主前景。
    assert_eq!(data.get("foregroundUnchanged"), Some(&Value::Bool(true)));
    // accepted mutation 不可安全重试。
    assert_eq!(data.get("retrySafe"), Some(&Value::Bool(false)));
    // facade 不得自动重派新意图。
    assert_eq!(
        data.get("automaticRetryProhibited"),
        Some(&Value::Bool(true))
    );
}

// 验证原样生产 launcher 可完成 open、navigate、query、click、type、screenshot 与 close。
#[test]
fn production_launcher_routes_public_page_actions_and_screenshot() {
    // 独占当前登录会话的固定 Broker endpoint。
    let _test_guard = TEST_LOCK
        // 中毒表示前一场景已失败。
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
    // 绑定本测试启动的真实 worker。
    let worker = wait_for_process(&layout.worker, false);
    // 绑定 worker 启动的 runtime fixture。
    let runtime = wait_for_process(&layout.runtime, false);
    // 经公开 strict apply 导航固定测试页面。
    let navigate = run_navigate(&layout, &session_id);
    // 验证导航共有 envelope 并取得 data。
    let navigate = page_success(
        // 传递 launcher 输出。
        &navigate,
        // 绑定稳定导航 capability。
        "browser.page.navigate@1",
        // 导航使用 generic apply。
        "apply",
        // 保留顶层 session target。
        &session_id,
    );
    // fixture 导航必须使用固定公开 URL。
    assert_eq!(PAGE_URL, "https://example.test/page");
    // 提取新签发的公开页面 identity。
    let page_id = navigate
        // 读取页面 identity。
        .get("pageId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 成功必须换发页面 identity。
        .unwrap_or_else(|| panic!("navigate should return pageId"))
        // 保留给后续独立 launcher。
        .to_owned();
    // 提取正导航代际。
    let generation = navigate
        // 读取导航代际。
        .get("navigationGeneration")
        // 只接受正整数。
        .and_then(Value::as_u64)
        // 成功必须携带代际。
        .unwrap_or_else(|| panic!("navigate should return generation"));
    // 经 standard read 查询可点击且可输入的固定首个按钮。
    let query = run_page_query(
        // 使用同一固定 Broker 代际。
        &layout,
        // 使用测试文件标签。
        "action-query",
        // 绑定页面 query capability。
        "browser.page.query@1",
        // 绑定当前 live session。
        &session_id,
        // 查询首个可用 Submit 按钮。
        json!({
            // 绑定当前公开页面。
            "pageId":page_id,
            // 使用 provider-neutral selector。
            "selector":{"role":"button","name":"Submit","exact":true},
            // 只需要首个有效命中。
            "maxResults":1,
            // 使用单一总预算。
            "timeoutMs":TIMEOUT_MS,
        }),
    );
    // 验证查询公开 envelope。
    let query = page_success(
        // 传递 launcher 输出。
        &query,
        // 绑定稳定 query capability。
        "browser.page.query@1",
        // Query 使用 generic read。
        "read",
        // 保留顶层 session target。
        &session_id,
    );
    // 提取 query 签发的唯一公开元素 identity。
    let element_id = query
        // 读取首个有界命中。
        .pointer("/matches/0/elementId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 成功查询必须签发元素 identity。
        .unwrap_or_else(|| panic!("query should return elementId"))
        // 保留给后续独立 launcher。
        .to_owned();
    // 经 confirmation-first strict apply 点击当前元素。
    let click = run_element_action(
        // 使用同一隔离布局。
        &layout,
        // 使用点击文件标签。
        "click",
        // 绑定稳定 click capability。
        "browser.element.click@1",
        // 绑定当前 live session。
        &session_id,
        // 只传当前 page、element 与总预算。
        json!({"pageId":page_id,"elementId":element_id,"timeoutMs":TIMEOUT_MS}),
    );
    // 验证点击公开 envelope。
    let click = action_success(
        // 传递 launcher 输出。
        &click,
        // 绑定稳定 click capability。
        "browser.element.click@1",
        // 元素 mutation 使用 apply。
        "apply",
        // 保留顶层 session target。
        &session_id,
    );
    // 点击 data 字段必须逐字闭合。
    assert_exact_keys(
        &click,
        &[
            // 固定结果字段集合由冻结 schema 拥有。
            "capability",
            "action",
            "outcome",
            "dispatchState",
            "accepted",
            // 保留可信 final 与 mutation 事实。
            "finalStateReached",
            "targetMayHaveMutated",
            "clicked",
            "pageId",
            // 保留 request-bound element 与代际。
            "elementId",
            "navigationGeneration",
            "foregroundUnchanged",
            // 保留策略先行与重试事实。
            "confirmationEvaluatedBeforeDispatch",
            "foregroundConsentEvaluatedBeforeDispatch",
            // 禁止安全重试与自动重派。
            "retrySafe",
            "automaticRetryProhibited",
        ],
    );
    // 核对点击共有 Command 真值。
    assert_command_truth(&click, "browser.element.click@1", "click");
    // 点击必须明确完成。
    assert_eq!(click.get("clicked"), Some(&Value::Bool(true)));
    // 点击必须保持 request-bound 页面。
    assert_eq!(
        click.get("pageId").and_then(Value::as_str),
        Some(page_id.as_str())
    );
    // 点击必须保持 request-bound 元素。
    assert_eq!(
        click.get("elementId").and_then(Value::as_str),
        Some(element_id.as_str())
    );
    // 点击必须保持当前导航代际。
    assert_eq!(
        click.get("navigationGeneration").and_then(Value::as_u64),
        Some(generation)
    );
    // 经 confirmation-first strict apply 向同一元素输入 fixture 文本。
    let typed = run_element_action(
        // 使用同一隔离布局。
        &layout,
        // 使用文本输入文件标签。
        "type",
        // 绑定稳定 type capability。
        "browser.element.type@1",
        // 绑定当前 live session。
        &session_id,
        // 文本只在 structured input 中出现。
        json!({"pageId":page_id,"elementId":element_id,"text":"fixture text","replace":true,"timeoutMs":TIMEOUT_MS}),
    );
    // 验证 type 公开 envelope。
    let typed = action_success(
        // 传递 launcher 输出。
        &typed,
        // 绑定稳定 type capability。
        "browser.element.type@1",
        // 元素 mutation 使用 apply。
        "apply",
        // 保留顶层 session target。
        &session_id,
    );
    // type data 字段必须逐字闭合。
    assert_exact_keys(
        &typed,
        &[
            // 固定结果字段集合由冻结 schema 拥有。
            "capability",
            "action",
            "outcome",
            "dispatchState",
            "accepted",
            // 保留可信 final 与 mutation 事实。
            "finalStateReached",
            "targetMayHaveMutated",
            "typed",
            "utf8Bytes",
            "pageId",
            // 保留 request-bound element 与代际。
            "elementId",
            "navigationGeneration",
            "foregroundUnchanged",
            // 保留策略先行与重试事实。
            "confirmationEvaluatedBeforeDispatch",
            "foregroundConsentEvaluatedBeforeDispatch",
            // 禁止安全重试与自动重派。
            "retrySafe",
            "automaticRetryProhibited",
        ],
    );
    // 核对 type 共有 Command 真值。
    assert_command_truth(&typed, "browser.element.type@1", "type");
    // type 必须明确完成。
    assert_eq!(typed.get("typed"), Some(&Value::Bool(true)));
    // 只回显 UTF-8 字节数。
    assert_eq!(typed.get("utf8Bytes").and_then(Value::as_u64), Some(12));
    // 公开结果禁止回显全部或部分输入文本。
    assert!(!typed.to_string().contains("fixture text"));
    // 经 standard read 捕获当前页面有界 PNG。
    let screenshot = run_screenshot(&layout, &session_id, &page_id);
    // 验证截图公开 envelope。
    let screenshot = action_success(
        // 传递 launcher 输出。
        &screenshot,
        // 绑定稳定截图 capability。
        "browser.page.screenshot@1",
        // 页面截图使用 read。
        "read",
        // 保留顶层 session target。
        &session_id,
    );
    // screenshot data 字段必须逐字闭合。
    assert_exact_keys(
        &screenshot,
        &[
            // 固定结果字段集合由冻结 schema 拥有。
            "capability",
            "action",
            "outcome",
            "dispatchState",
            "accepted",
            // 保留可信 final、只读与 request-bound 页面事实。
            "finalStateReached",
            "targetMayHaveMutated",
            "readOnly",
            "pageId",
            // 保留代际与有界 PNG 字段。
            "navigationGeneration",
            "mimeType",
            "pngBase64",
            "pngBytes",
            "width",
            "height",
            // 保留摘要、前景与策略先行事实。
            "digest",
            "foregroundUnchanged",
            "confirmationEvaluatedBeforeDispatch",
            // 保留前景同意与重试事实。
            "foregroundConsentEvaluatedBeforeDispatch",
            "retrySafe",
            "automaticRetryProhibited",
        ],
    );
    // 截图 capability 与动作必须固定。
    assert_eq!(
        screenshot.get("capability").and_then(Value::as_str),
        Some("browser.page.screenshot@1")
    );
    // 截图必须是只读 Query。
    assert_eq!(screenshot.get("readOnly"), Some(&Value::Bool(true)));
    // 截图不得改变目标。
    assert_eq!(
        screenshot.get("targetMayHaveMutated"),
        Some(&Value::Bool(false))
    );
    // 截图 MIME 固定为 PNG。
    assert_eq!(
        screenshot.get("mimeType").and_then(Value::as_str),
        Some("image/png")
    );
    // fixture 返回一像素宽度。
    assert_eq!(screenshot.get("width").and_then(Value::as_u64), Some(1));
    // fixture 返回一像素高度。
    assert_eq!(screenshot.get("height").and_then(Value::as_u64), Some(1));
    // 截图 Base64 必须非空且有界。
    assert!(
        screenshot
            .get("pngBase64")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty() && value.len() <= 16_777_216)
    );
    // 稳定摘要必须是 16 位小写十六进制。
    assert!(
        screenshot
            .get("digest")
            .and_then(Value::as_str)
            .is_some_and(|value| value.len() == 16
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    );
    // 经全新 launcher 回收 live Browser Session。
    let close = run_lifecycle(
        // 使用同一布局。
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
    // 核对公开可信 close。
    assert_close_success(&close, &session_id);
    // 同一 worker 必须在 close 后退出。
    assert!(worker.wait_exited(Duration::from_secs(5)));
    // 同一 runtime descendant 必须在 close 后退出。
    assert!(runtime.wait_exited(Duration::from_secs(5)));
    // worker 镜像必须收敛为零。
    wait_for_no_process(&layout.worker);
    // runtime 镜像必须收敛为零。
    wait_for_no_process(&layout.runtime);
    // 工具 profile 必须完整清理。
    wait_for_profiles_clean(&layout);
    // 终止本测试拥有的固定 Broker。
    broker.terminate_owned();
    // endpoint owner 必须退出。
    wait_for_no_process(&layout.broker);
    // 删除测试独占布局。
    layout.finish();
}
