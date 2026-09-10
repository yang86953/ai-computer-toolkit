#![cfg(target_os = "windows")]

//! 验证原样生产 launcher 的公开页面导航、等待与查询成功纵切。

// 导入文件、路径与有界等待类型。
use std::{
    // 写入测试独占的结构化输入。
    fs,
    // 定位生产 worker 并保存输入路径。
    path::{Path, PathBuf},
    // 等待 owned worker 与 runtime 收敛。
    time::Duration,
};

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

// 固定 runtime fixture 接受的页面导航 URL。
pub(super) const PAGE_URL: &str = "https://example.test/page";
// 固定覆盖完整公开调用链的总预算。
pub(super) const TIMEOUT_MS: u32 = 30_000;

// 写入只含 target、capability 与页面 input 的统一 App wrapper。
pub(super) fn page_input(
    // 接收当前测试独占布局。
    layout: &LauncherLayout,
    // 接收不会进入公开请求的测试文件标签。
    label: &str,
    // 接收稳定页面 capability。
    capability: &str,
    // 接收公开 Browser Session identity。
    session_id: &str,
    // 接收严格 provider-neutral 页面 input。
    input: Value,
) -> PathBuf {
    // 在测试独占 TEMP 下构造精确输入路径。
    let path = layout.temporary.join(format!("page-{label}.json"));
    // 构造统一 App facade structured wrapper。
    let value = json!({
        // 顶层只传当前公开 Browser Session target。
        "target": { "sessionId": session_id },
        // args 只传稳定 capability 与业务 input。
        "args": { "capability": capability, "input": input },
    });
    // 序列化为 UTF-8 JSON 字节。
    let bytes = serde_json::to_vec(&value)
        // 固定测试值必须可序列化。
        .unwrap_or_else(|error| panic!("page input should serialize: {error:?}"));
    // 写入当前测试唯一精确文件。
    fs::write(&path, bytes)
        // 失败时保留文件系统上下文。
        .unwrap_or_else(|error| panic!("page input should be written: {error:?}"));
    // 返回只由原样 launcher 读取的输入路径。
    path
}

// 验证公开对象字段集合逐字闭合。
pub(super) fn assert_exact_keys(value: &Value, expected: &[&str]) {
    // 公开 envelope 与 data 都必须是对象。
    let object = value
        // 只接受 JSON object。
        .as_object()
        // 非对象表示公开形状漂移。
        .unwrap_or_else(|| panic!("public page value should be an object"));
    // 实际键数量必须与冻结集合一致。
    assert_eq!(object.len(), expected.len());
    // 每个实际键都必须来自冻结集合。
    assert!(
        // 遍历调用结果字段名。
        object.keys().all(|key| expected.contains(&key.as_str())),
        // 只在测试失败时输出字段集合。
        "public page keys drifted: {:?}",
        // 输出字段名但不输出调用值。
        object.keys().collect::<Vec<_>>(),
    );
}

// 经原样生产 launcher 执行 strict confirmed 页面导航。
pub(super) fn run_navigate(
    // 接收当前测试布局。
    layout: &LauncherLayout,
    // 接收公开 Browser Session identity。
    session_id: &str,
) -> std::process::Output {
    // 写入冻结导航 input。
    let input = page_input(
        // 使用当前布局。
        layout,
        // 使用测试文件标签。
        "navigate",
        // 绑定稳定页面导航 capability。
        "browser.page.navigate@1",
        // 绑定当前 live session。
        session_id,
        // 只传 URL 与单一总预算。
        json!({ "url": PAGE_URL, "timeoutMs": TIMEOUT_MS }),
    );
    // 转换为 PowerShell argv 文本。
    let input = input.to_string_lossy().into_owned();
    // 通过原样 launcher 显式要求确认和严格隔离。
    run_launcher(
        // 使用隔离安装布局。
        layout,
        // 使用冻结 generic apply 与公开 flags。
        &[
            // 进入通用运行 surface。
            "run",
            // 使用统一 App facade。
            "app",
            // 页面导航使用 apply。
            "apply",
            // 传递 structured input 文件。
            "--input",
            // 传递测试独占路径。
            &input,
            // mutation 显式确认。
            "--confirm",
            // 导航契约要求严格零打扰。
            "--strict-isolation",
        ],
    )
}

// 经原样生产 launcher 执行 standard 页面 Query。
pub(super) fn run_page_query(
    // 接收当前测试布局。
    layout: &LauncherLayout,
    // 接收测试文件标签。
    label: &str,
    // 接收 wait 或 query capability。
    capability: &str,
    // 接收公开 Browser Session identity。
    session_id: &str,
    // 接收冻结页面 input。
    input: Value,
) -> std::process::Output {
    // 写入 provider-neutral structured input。
    let input = page_input(layout, label, capability, session_id, input);
    // 转换为 PowerShell argv 文本。
    let input = input.to_string_lossy().into_owned();
    // Query 不携带确认或严格 flag，保留 standard/background-preferred。
    run_launcher(
        // 使用隔离生产布局。
        layout,
        // 使用冻结 generic read 参数。
        &["run", "app", "read", "--input", &input],
    )
}

// 解析并验证三项页面成功共有的公开 envelope。
pub(super) fn page_success(
    // 接收 launcher 子进程输出。
    output: &std::process::Output,
    // 接收预期 capability。
    capability: &str,
    // 接收预期 generic verb。
    verb: &str,
    // 接收预期顶层 target。
    session_id: &str,
) -> Value {
    // 页面 operation 必须成功退出。
    assert!(
        output.status.success(),
        "public page operation should succeed"
    );
    // 解析唯一公开 JSON 文档。
    let value = output_json(output);
    // 顶层成功 envelope 只能包含冻结字段。
    assert_exact_keys(
        // 传递完整公开结果。
        &value,
        // 对齐 browser-page-read-result schema。
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
    // 核对统一 App facade。
    assert_eq!(value.get("app").and_then(Value::as_str), Some("app"));
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
    // 三项页面操作都必须由认证隔离域完成。
    assert_eq!(
        // 读取真实执行域。
        value.get("executionRealm").and_then(Value::as_str),
        // 固定为 isolated worker 公共枚举。
        Some("isolated-worker"),
    );
    // 按 Command 或 Query 核对冻结隔离与宿主影响策略。
    let (isolation, host_impact) = if capability == "browser.page.navigate@1" {
        // 导航固定使用严格零打扰。
        ("strict", "strict-no-interference")
    } else {
        // 页面 Query 固定使用 standard 与后台优先。
        ("standard", "background-preferred")
    };
    // 顶层必须回显调用方冻结的隔离要求。
    assert_eq!(
        // 读取公开隔离字段。
        value.get("isolationRequirement").and_then(Value::as_str),
        // 比较当前 operation 的冻结值。
        Some(isolation),
    );
    // 顶层必须回显对应宿主影响策略。
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
    // 页面 success 不得泄漏私有实现文本。
    assert_no_private_text(&value);
    // 按 capability 锁定 data 字段集合。
    let data_keys: &[&str] = match capability {
        // 导航包含页面换代与 Command 真值。
        "browser.page.navigate@1" => &[
            // 固定 capability。
            "capability",
            // 固定领域动作。
            "action",
            // 固定可信 outcome。
            "outcome",
            // 固定 dispatch 终态。
            "dispatchState",
            // 固定业务接受事实。
            "accepted",
            // 固定可信 final。
            "finalStateReached",
            // 固定 mutation 事实。
            "targetMayHaveMutated",
            // 固定导航完成事实。
            "navigated",
            // 保存新公开页面。
            "pageId",
            // 保存正导航代际。
            "navigationGeneration",
            // 保存前景不变事实。
            "foregroundUnchanged",
            // 保存确认已先行。
            "confirmationEvaluatedBeforeDispatch",
            // 保存前景策略已先行。
            "foregroundConsentEvaluatedBeforeDispatch",
            // 固定不可安全重试。
            "retrySafe",
            // 固定禁止自动重派。
            "automaticRetryProhibited",
        ],
        // wait 包含当前页面、代际和条件满足。
        "browser.page.wait@1" => &[
            // 固定 capability。
            "capability",
            // 固定领域动作。
            "action",
            // 固定可信 outcome。
            "outcome",
            // 固定 dispatch 终态。
            "dispatchState",
            // 固定业务接受事实。
            "accepted",
            // 固定可信 final。
            "finalStateReached",
            // Query 不改变目标。
            "targetMayHaveMutated",
            // 固定只读事实。
            "readOnly",
            // 保存当前公开页面。
            "pageId",
            // 保存正导航代际。
            "navigationGeneration",
            // 固定条件满足事实。
            "conditionMet",
            // 保存前景不变事实。
            "foregroundUnchanged",
            // 保存确认策略已先行。
            "confirmationEvaluatedBeforeDispatch",
            // 保存前景策略已先行。
            "foregroundConsentEvaluatedBeforeDispatch",
            // 固定不可自动重试。
            "retrySafe",
            // 固定禁止自动重派。
            "automaticRetryProhibited",
        ],
        // query 包含当前页面、代际和有界命中。
        "browser.page.query@1" => &[
            // 固定 capability。
            "capability",
            // 固定领域动作。
            "action",
            // 固定可信 outcome。
            "outcome",
            // 固定 dispatch 终态。
            "dispatchState",
            // 固定业务接受事实。
            "accepted",
            // 固定可信 final。
            "finalStateReached",
            // Query 不改变目标。
            "targetMayHaveMutated",
            // 固定只读事实。
            "readOnly",
            // 保存当前公开页面。
            "pageId",
            // 保存正导航代际。
            "navigationGeneration",
            // 保存有界公开命中。
            "matches",
            // 保存总命中数。
            "matchCount",
            // 保存截断事实。
            "truncated",
            // 保存前景不变事实。
            "foregroundUnchanged",
            // 保存确认策略已先行。
            "confirmationEvaluatedBeforeDispatch",
            // 保存前景策略已先行。
            "foregroundConsentEvaluatedBeforeDispatch",
            // 固定不可自动重试。
            "retrySafe",
            // 固定禁止自动重派。
            "automaticRetryProhibited",
        ],
        // helper 只允许三项冻结 capability。
        _ => panic!("unexpected public page capability"),
    };
    // 取得公开 data 供字段和 operation 断言。
    let data = value
        // 读取 data。
        .get("data")
        // success 缺失 data 必须失败。
        .unwrap_or_else(|| panic!("public page success should contain data"));
    // data 不得含兼容或私有扩张字段。
    assert_exact_keys(data, data_keys);
    // 返回冻结 data 供 operation 专属断言。
    data
        // 读取 data 对象。
        .clone()
}

// 验证原样生产 launcher 可完成 open、navigate、wait、query 与 close。
#[test]
fn production_launcher_routes_public_page_navigation_and_reads() {
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
    // 导航必须回显严格隔离。
    assert_eq!(
        // success data 的父级不含策略，使用输出重新解析前已验证执行域。
        navigate.get("navigated"),
        // 固定可信导航完成。
        Some(&Value::Bool(true)),
    );
    // 提取新签发的公开页面 identity。
    let page_id = navigate
        // 读取页面 identity。
        .get("pageId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 成功必须换发页面 identity。
        .unwrap_or_else(|| panic!("navigate should return pageId"))
        // 保留给独立 Query launcher。
        .to_owned();
    // 页面 identity 必须保持 canonical opaque 形状。
    assert!(page_id.starts_with("s2:bp:") && page_id.len() == 38);
    // 提取正导航代际。
    let generation = navigate
        // 读取导航代际。
        .get("navigationGeneration")
        // 只接受正整数。
        .and_then(Value::as_u64)
        // 成功必须携带代际。
        .unwrap_or_else(|| panic!("navigate should return generation"));
    // 首次导航代际必须为正。
    assert!(generation >= 1);
    // 经 standard read 等待 document ready。
    let wait = run_page_query(
        // 使用同一固定 Broker 代际。
        &layout,
        // 使用测试文件标签。
        "wait",
        // 绑定页面等待 capability。
        "browser.page.wait@1",
        // 绑定当前 live session。
        &session_id,
        // 绑定刚签发页面与有限条件。
        json!({
            // 只接受当前公开页面。
            "pageId": page_id,
            // 等待固定 document-ready 条件。
            "condition": { "kind": "document-ready" },
            // 使用单一总预算。
            "timeoutMs": TIMEOUT_MS,
        }),
    );
    // 验证等待公开 envelope。
    let wait = page_success(
        // 传递 launcher 输出。
        &wait,
        // 绑定稳定 wait capability。
        "browser.page.wait@1",
        // Query 使用 generic read。
        "read",
        // 保留顶层 session target。
        &session_id,
    );
    // wait 必须回显同一页面 identity。
    assert_eq!(
        wait.get("pageId").and_then(Value::as_str),
        Some(page_id.as_str())
    );
    // wait 必须回显同一导航代际。
    assert_eq!(
        wait.get("navigationGeneration").and_then(Value::as_u64),
        Some(generation)
    );
    // completed wait 必须表达条件满足。
    assert_eq!(wait.get("conditionMet"), Some(&Value::Bool(true)));
    // 经 standard read 查询两个固定按钮。
    let query = run_page_query(
        // 使用同一固定 Broker 代际。
        &layout,
        // 使用测试文件标签。
        "query",
        // 绑定页面 query capability。
        "browser.page.query@1",
        // 绑定当前 live session。
        &session_id,
        // 只传 provider-neutral role selector。
        json!({
            // 绑定当前公开页面。
            "pageId": page_id,
            // 查询固定 button role。
            "selector": { "role": "button" },
            // 限制为两个公开结果。
            "maxResults": 2,
            // 使用单一总预算。
            "timeoutMs": TIMEOUT_MS,
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
    // query 必须回显同一页面 identity。
    assert_eq!(
        query.get("pageId").and_then(Value::as_str),
        Some(page_id.as_str())
    );
    // query 必须回显同一导航代际。
    assert_eq!(
        query.get("navigationGeneration").and_then(Value::as_u64),
        Some(generation)
    );
    // runtime fixture 提供两个 button 命中。
    assert_eq!(query.get("matchCount").and_then(Value::as_u64), Some(2));
    // maxResults=2 不应截断两个命中。
    assert_eq!(query.get("truncated"), Some(&Value::Bool(false)));
    // 第二次导航必须换发新页面并立即使第一代页面 stale。
    let second_navigate = run_navigate(&layout, &session_id);
    // 验证第二次导航公开 envelope。
    let second_navigate = page_success(
        // 传递第二个独立 launcher 输出。
        &second_navigate,
        // 绑定稳定导航 capability。
        "browser.page.navigate@1",
        // 导航仍使用 generic apply。
        "apply",
        // 顶层仍绑定同一 live session。
        &session_id,
    );
    // 提取第二代公开页面 identity。
    let second_page_id = second_navigate
        // 读取新页面 identity。
        .get("pageId")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 导航成功必须签发 identity。
        .unwrap_or_else(|| panic!("second navigate should return pageId"))
        // 保留给后续 Query。
        .to_owned();
    // 二次导航不得复用旧公开页面 identity。
    assert_ne!(second_page_id, page_id);
    // 提取第二次导航代际。
    let second_generation = second_navigate
        // 读取正导航代际。
        .get("navigationGeneration")
        // 只接受整数。
        .and_then(Value::as_u64)
        // 成功必须报告代际。
        .unwrap_or_else(|| panic!("second navigate should return generation"));
    // 同一 session 导航恰好推进一代。
    assert_eq!(second_generation, generation + 1);
    // 使用第一代页面执行 Query 必须在业务接受前 stale。
    let stale = run_page_query(
        // 使用同一 Broker 代际。
        &layout,
        // 使用独立测试文件标签。
        "stale-query",
        // 绑定页面 query capability。
        "browser.page.query@1",
        // 绑定仍 live 的顶层 session。
        &session_id,
        // 传递已经失效的第一代页面。
        json!({
            // 旧页面只用于证明 stale，不得被自动重绑。
            "pageId": page_id,
            // 使用合法 provider-neutral selector。
            "selector": { "role": "button" },
            // 使用完整总预算。
            "timeoutMs": TIMEOUT_MS,
        }),
    );
    // stale 页面必须非零退出。
    assert!(!stale.status.success());
    // 解析唯一公开 stale 错误。
    let stale = output_json(&stale);
    // 错误 envelope 只能包含 ok 与 error。
    assert_exact_keys(&stale, &["ok", "error"]);
    // stale page 使用冻结错误码。
    assert_eq!(
        // 读取错误码。
        stale.pointer("/error/code").and_then(Value::as_str),
        // 不得降级为 stale session 或通用失败。
        Some("STALE_PAGE"),
    );
    // 业务接受前 stale 必须明确未派发。
    assert_eq!(
        // 读取接受事实。
        stale.pointer("/error/details/accepted"),
        // 固定尚未业务接受。
        Some(&Value::Bool(false)),
    );
    // Query stale 不得误标目标可能改变。
    assert_eq!(
        // 读取 mutation 事实。
        stale.pointer("/error/details/targetMayHaveMutated"),
        // Query 始终无隐藏副作用。
        Some(&Value::Bool(false)),
    );
    // stale page 输出不得泄漏第一代私有映射或输入。
    assert_no_private_text(&stale);
    // 使用第二代 current page 查询不存在的语义名称。
    let zero = run_page_query(
        // 使用同一固定 Broker。
        &layout,
        // 使用独立测试文件标签。
        "zero-query",
        // 绑定页面 query capability。
        "browser.page.query@1",
        // 绑定同一 live session。
        &session_id,
        // 传递当前页面与零命中 selector。
        json!({
            // 使用第二代 current page。
            "pageId": second_page_id,
            // 固定不存在的可访问名称。
            "selector": { "name": "Missing Public Name" },
            // 使用冻结结果上限。
            "maxResults": 2,
            // 使用单一总预算。
            "timeoutMs": TIMEOUT_MS,
        }),
    );
    // 验证零命中仍是可信公开 success。
    let zero = page_success(
        // 传递独立 launcher 输出。
        &zero,
        // 绑定页面 query capability。
        "browser.page.query@1",
        // Query 使用 generic read。
        "read",
        // 顶层绑定当前 session。
        &session_id,
    );
    // 零命中必须回显当前第二代页面。
    assert_eq!(
        // 读取页面 identity。
        zero.get("pageId").and_then(Value::as_str),
        // 比较第二代页面。
        Some(second_page_id.as_str()),
    );
    // 零命中必须回显当前第二代导航代际。
    assert_eq!(
        // 读取导航代际。
        zero.get("navigationGeneration").and_then(Value::as_u64),
        // 比较第二代代际。
        Some(second_generation),
    );
    // 零命中必须返回空数组而不是失败或 null。
    assert_eq!(zero.get("matches"), Some(&json!([])));
    // 总命中数必须为零。
    assert_eq!(zero.get("matchCount").and_then(Value::as_u64), Some(0));
    // 零命中不得声称截断。
    assert_eq!(zero.get("truncated"), Some(&Value::Bool(false)));
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
