//! capability assessment 的纯决策与隐私门禁测试。

// 导入父模块的封闭接口。
use super::*;

// 构造不接触真实桌面的目标夹具。
fn target(
    // 接收目标类别。
    kind: OpaqueTargetKind,
    // 接收当前可用性。
    availability: AssessmentAvailability,
    // 接收发布 capability。
    capabilities: &[&str],
) -> ResolvedTarget {
    // 返回最小纯事实。
    ResolvedTarget {
        // 保存类别。
        kind: AssessmentTargetKind::Opaque(kind),
        // 保存可用性。
        availability,
        // 构造有序发布集合。
        capabilities: capabilities
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
    }
}

// 构造不接触真实 Broker 的 Browser Session 目标夹具。
fn browser_session_target(capabilities: &[&str]) -> ResolvedTarget {
    // 返回最小 live 会话事实。
    ResolvedTarget {
        // 使用 assessment 私有类别而不扩大全局 opaque 协议。
        kind: AssessmentTargetKind::BrowserSession,
        // 测试目标已经由合成 inspect 认证为 live。
        availability: AssessmentAvailability::Available,
        // 构造有序发布集合。
        capabilities: capabilities
            // 遍历调用方提供的稳定 capability ID。
            .iter()
            // 复制为目标自有集合。
            .map(|value| (*value).to_owned())
            // 完成去重收集。
            .collect(),
    }
}

// 验证 schema 的八种 decision 都有稳定文本。
#[test]
fn all_eight_decisions_are_closed_and_unique() {
    // 列出全部封闭变体。
    let values = [
        // 后台可执行。
        Decision::ExecutableBackground,
        // 需要确认。
        Decision::ConfirmationRequired,
        // 需要前台同意。
        Decision::ForegroundConsentRequired,
        // 需要隔离。
        Decision::IsolationRequired,
        // 权限阻塞。
        Decision::PermissionBlocked,
        // 不可用。
        Decision::Unavailable,
        // 不支持。
        Decision::Unsupported,
        // capability 缺口。
        Decision::CapabilityGap,
    ];
    // 投影稳定字符串并去重。
    let unique = values
        .iter()
        .map(|value| value.as_str())
        .collect::<BTreeSet<_>>();
    // 必须恰好保留八种决策。
    assert_eq!(unique.len(), 8);
}

// 验证只读、确认、前台、权限、未迁移、不支持与 gap 路径。
#[test]
fn evaluator_is_fail_closed_across_rule_states() {
    // 构造可用 host。
    let host = target(
        OpaqueTargetKind::Host,
        AssessmentAvailability::Available,
        &[
            capabilities::APPLICATION_DISCOVER,
            // host 同时发布统一应用 session 聚合发现。
            capabilities::APPLICATION_SESSION_DISCOVER,
        ],
    );
    // 只读发现可后台执行。
    assert_eq!(
        evaluate(
            capabilities::APPLICATION_DISCOVER,
            "s2:h:0000000000000000",
            &host
        )["decision"],
        "executable-background"
    );
    // 统一应用 session 聚合也必须按只读后台路径执行。
    assert_eq!(
        // 使用同一 canonical host 目标评估新迁回能力。
        evaluate(
            // 选择稳定 session 聚合 capability。
            capabilities::APPLICATION_SESSION_DISCOVER,
            // 使用合成 canonical host 目标。
            "s2:h:0000000000000000",
            // 传播包含精确发布集合的 host 事实。
            &host,
        )["decision"],
        // 只读聚合不得要求确认或前台同意。
        "executable-background"
    );
    // 构造带动态 Shell 启动能力的精确应用目标。
    let application = target(
        // application.open 只允许 canonical application kind。
        OpaqueTargetKind::Application,
        // 认证目录目标当前可用。
        AssessmentAvailability::Available,
        // 精确目标发布启动能力。
        &[capabilities::APPLICATION_OPEN],
    );
    // 精确应用启动需要逐操作确认。
    assert_eq!(
        evaluate(
            capabilities::APPLICATION_OPEN,
            "s2:a:0000000000000000",
            &application
        )["decision"],
        "confirmation-required"
    );
    // 构造同时发布两级终止能力的精确进程目标。
    let process = target(
        // 终止规则只允许 canonical 进程类别。
        OpaqueTargetKind::Process,
        // 测试同级且可访问的当前进程事实。
        AssessmentAvailability::Available,
        // 同一目标分别发布优雅与强制终止能力。
        &[
            // 发布固定优雅终止能力。
            capabilities::PROCESS_TERMINATE_GRACEFUL,
            // 发布固定强制终止能力。
            capabilities::PROCESS_TERMINATE_FORCE,
        ],
    );
    // 两级终止都必须保持独立确认门禁与主机后台域。
    for capability in [
        // 覆盖优雅终止规则。
        capabilities::PROCESS_TERMINATE_GRACEFUL,
        // 覆盖强制终止规则。
        capabilities::PROCESS_TERMINATE_FORCE,
    ] {
        // 评估当前精确目标上的终止能力。
        let result = evaluate(capability, "s2:p:0000000000000000", &process);
        // mutation 不得在未确认时直接可执行。
        assert_eq!(result["decision"], "confirmation-required");
        // 两级能力都不需要前台输入或激活。
        assert_eq!(result["executionRealm"], "host-background");
        // 明确记录非只读分类。
        assert_eq!(result["constraints"]["readOnly"], false);
    }
    // 前台输入与窗口生命周期需要单独同意。
    let window = target(
        OpaqueTargetKind::Window,
        AssessmentAvailability::Available,
        // 同一窗口目标发布两个主机前景能力。
        &[capabilities::UI_INPUT_KEY, capabilities::WINDOW_LIFECYCLE],
    );
    // 验证 foreground consent 决策。
    assert_eq!(
        evaluate(capabilities::UI_INPUT_KEY, "s2:w:0000000000000000", &window)["decision"],
        "foreground-consent-required"
    );
    // 窗口 assessment 必须公开当前身份强度缺口。
    assert_eq!(
        evaluate(capabilities::UI_INPUT_KEY, "s2:w:0000000000000000", &window)["evidence"]["targetIdentityStrength"]
            ["sameProcessRecycledWindowToken"],
        // 不得把当前三字段身份误报为绝对窗口代际。
        "not-guaranteed"
    );
    // 窗口生命周期必须得到相同前景同意决策。
    assert_eq!(
        // 使用精确 opaque 窗口评估生命周期 capability。
        evaluate(
            // 选择通用状态与几何控制能力。
            capabilities::WINDOW_LIFECYCLE,
            // 使用 canonical 窗口目标。
            "s2:w:0000000000000000",
            // 传播发布生命周期能力的窗口事实。
            &window,
        )["decision"],
        // 确认存在但前景同意缺失时必须阻塞。
        "foreground-consent-required"
    );
    // 权限阻塞优先于 capability 可用性。
    let blocked = target(
        OpaqueTargetKind::Process,
        AssessmentAvailability::PermissionBlocked,
        &[capabilities::PROCESS_METADATA_READ],
    );
    // 验证权限阻塞且 realm none。
    let blocked_result = evaluate(
        capabilities::PROCESS_METADATA_READ,
        "s2:p:0000000000000000",
        &blocked,
    );
    // 验证权限决策。
    assert_eq!(blocked_result["decision"], "permission-blocked");
    // 验证不进入执行域。
    assert_eq!(blocked_result["executionRealm"], "none");
    // 构造发布 Rust 帧探针的窗口目标。
    let probe_window = target(
        // 使用窗口类别。
        OpaqueTargetKind::Window,
        // 当前窗口事实可用。
        AssessmentAvailability::Available,
        // 发布确认式首帧探针。
        &[capabilities::WINDOW_CAPTURE_FRAME_PROBE],
    );
    // 已迁移的帧探针必须保持确认门禁。
    let probe = evaluate(
        // 使用稳定探针 ID。
        capabilities::WINDOW_CAPTURE_FRAME_PROBE,
        // 使用 canonical 窗口目标。
        "s2:w:0000000000000000",
        // 传入发布该 capability 的目标。
        &probe_window,
    );
    // 敏感读取必须要求逐操作确认。
    assert_eq!(probe["decision"], "confirmation-required");
    // 探针必须在隔离 worker 域执行。
    assert_eq!(probe["executionRealm"], "isolated-worker");
    // 构造发布 Rust 零帧预检的窗口目标。
    let preflight_window = target(
        // 使用窗口类别。
        OpaqueTargetKind::Window,
        // 当前只读事实可用。
        AssessmentAvailability::Available,
        // 发布预检 capability。
        &[capabilities::WINDOW_CAPTURE_PREFLIGHT],
    );
    // 评估已迁移的零帧预检。
    let preflight = evaluate(
        // 使用稳定预检 ID。
        capabilities::WINDOW_CAPTURE_PREFLIGHT,
        // 使用 canonical 窗口目标。
        "s2:w:0000000000000000",
        // 传入发布该能力的窗口事实。
        &preflight_window,
    );
    // 只读预检必须可在后台执行。
    assert_eq!(preflight["decision"], "executable-background");
    // 预检不得进入捕获 worker 域。
    assert_eq!(preflight["executionRealm"], "host-headless");
    // 预检必须保持只读。
    assert_eq!(preflight["constraints"]["readOnly"], true);
    // 精确目标类别不匹配时 unsupported。
    assert_eq!(
        evaluate(
            capabilities::PROCESS_METADATA_READ,
            "s2:w:0000000000000000",
            &window
        )["decision"],
        "unsupported"
    );
    // 未登记 capability 返回 capability-gap。
    assert_eq!(
        evaluate("unknown.read@1", "s2:w:0000000000000000", &window)["decision"],
        "capability-gap"
    );
}

// 验证 Browser Session 生命周期与页面路线共享规则、身份与只读解析语义。
#[test]
fn browser_session_public_route_assessment_is_fail_closed() {
    // 构造当前 host 发布的打开能力。
    let host = target(
        // open 只能使用通用 canonical host 类别。
        OpaqueTargetKind::Host,
        // 当前 host 可用。
        AssessmentAvailability::Available,
        // 发布唯一 Browser Session 打开能力。
        &[capabilities::BROWSER_SESSION_OPEN],
    );
    // 评估当前 host 上的 Browser Session 打开。
    let open = evaluate(
        // 使用稳定 open capability。
        capabilities::BROWSER_SESSION_OPEN,
        // 使用 canonical host fixture。
        "s2:h:0000000000000000",
        // 传递当前 host 事实。
        &host,
    );
    // mutation 必须等待逐操作确认。
    assert_eq!(open["decision"], "confirmation-required");
    // 固定 Broker 路线必须保持隔离 worker 域。
    assert_eq!(open["executionRealm"], "isolated-worker");
    // 打开不要求任何前台影响同意。
    assert_eq!(open["requiresForegroundConsent"], false);
    // 打开不得伪装成只读操作。
    assert_eq!(open["constraints"]["readOnly"], false);
    // 构造已由合成 inspect 认证的 live Browser Session。
    let browser_session = browser_session_target(&[
        // 发布会话关闭 Command。
        capabilities::BROWSER_SESSION_CLOSE,
        // 发布页面导航 Command。
        capabilities::BROWSER_PAGE_NAVIGATE,
        // 发布页面等待 Query。
        capabilities::BROWSER_PAGE_WAIT,
        // 发布页面查询 Query。
        capabilities::BROWSER_PAGE_QUERY,
        // 发布元素点击 Command。
        capabilities::BROWSER_ELEMENT_CLICK,
        // 发布元素输入 Command。
        capabilities::BROWSER_ELEMENT_TYPE,
        // 发布页面截图 Query。
        capabilities::BROWSER_PAGE_SCREENSHOT,
    ]);
    // 评估精确 live 会话关闭。
    let close = evaluate(
        // 使用稳定 close capability。
        capabilities::BROWSER_SESSION_CLOSE,
        // 使用 canonical Browser Session fixture。
        "s2:bs:00000000000000000000000000000000",
        // 传递 live 会话事实。
        &browser_session,
    );
    // close 也必须等待逐操作确认。
    assert_eq!(close["decision"], "confirmation-required");
    // close 必须使用同一隔离 worker 域。
    assert_eq!(close["executionRealm"], "isolated-worker");
    // close 不要求前台影响同意。
    assert_eq!(close["requiresForegroundConsent"], false);
    // 公开 evidence 只能出现 provider-neutral Browser Session 类别。
    assert_eq!(close["evidence"]["targetKind"], "browser-session");
    // 评估当前 live 会话上的页面导航。
    let navigate = evaluate(
        // 使用稳定页面导航 capability。
        capabilities::BROWSER_PAGE_NAVIGATE,
        // 使用同一个 canonical Browser Session fixture。
        "s2:bs:00000000000000000000000000000000",
        // 传递 live 会话事实。
        &browser_session,
    );
    // 页面 mutation 必须 confirmation-first。
    assert_eq!(navigate["decision"], "confirmation-required");
    // 页面导航固定在隔离 worker 域执行。
    assert_eq!(navigate["executionRealm"], "isolated-worker");
    // 页面导航不得要求或允许前台影响回退。
    assert_eq!(navigate["requiresForegroundConsent"], false);
    // 页面导航不得伪装成只读操作。
    assert_eq!(navigate["constraints"]["readOnly"], false);
    // 分别核对两项元素 mutation 的确认与零前台语义。
    for capability in [
        // 点击改变当前页面状态。
        capabilities::BROWSER_ELEMENT_CLICK,
        // 输入改变当前页面状态。
        capabilities::BROWSER_ELEMENT_TYPE,
    ] {
        // 评估当前 live 会话上的元素 mutation。
        let element_action = evaluate(
            // 传递当前受测 capability。
            capability,
            // 使用同一个 canonical Browser Session fixture。
            "s2:bs:00000000000000000000000000000000",
            // 传递 live 会话事实。
            &browser_session,
        );
        // mutation 必须 confirmation-first。
        assert_eq!(element_action["decision"], "confirmation-required");
        // 元素动作固定在隔离 worker 域执行。
        assert_eq!(element_action["executionRealm"], "isolated-worker");
        // 元素动作不得引入前台回退。
        assert_eq!(element_action["requiresForegroundConsent"], false);
        // 元素动作不得伪装成只读操作。
        assert_eq!(element_action["constraints"]["readOnly"], false);
    }
    // 分别核对三项页面 Query 的无确认与零前台语义。
    for capability in [
        // 页面等待只观察封闭条件。
        capabilities::BROWSER_PAGE_WAIT,
        // 页面查询只返回有界语义结果。
        capabilities::BROWSER_PAGE_QUERY,
        // 页面截图只返回有界 PNG。
        capabilities::BROWSER_PAGE_SCREENSHOT,
    ] {
        // 评估当前 live 会话上的只读页面 capability。
        let page_query = evaluate(
            // 传递当前受测 capability。
            capability,
            // 使用同一个 canonical Browser Session fixture。
            "s2:bs:00000000000000000000000000000000",
            // 传递 live 会话事实。
            &browser_session,
        );
        // Query 必须直接给出后台可执行结论。
        assert_eq!(page_query["decision"], "executable-background");
        // Query 仍固定在隔离 worker 域执行。
        assert_eq!(page_query["executionRealm"], "isolated-worker");
        // Query 不得引入前台影响同意。
        assert_eq!(page_query["requiresForegroundConsent"], false);
        // Query 必须公开只读约束。
        assert_eq!(page_query["constraints"]["readOnly"], true);
    }
    // close 不能在 host 目标上获得支持。
    assert_eq!(
        // 对 host 使用 close 规则。
        evaluate(
            // 使用 close capability。
            capabilities::BROWSER_SESSION_CLOSE,
            // 保持原 host identity。
            "s2:h:0000000000000000",
            // 传递 host 事实。
            &host,
        )["decision"],
        // 精确目标种类不匹配必须失败闭合。
        "unsupported"
    );
    // open 不能在 Browser Session 目标上获得支持。
    assert_eq!(
        // 对 Browser Session 使用 open 规则。
        evaluate(
            // 使用 open capability。
            capabilities::BROWSER_SESSION_OPEN,
            // 保持原 Browser Session identity。
            "s2:bs:00000000000000000000000000000000",
            // 传递 live 会话事实。
            &browser_session,
        )["decision"],
        // 精确目标种类不匹配必须失败闭合。
        "unsupported"
    );
    // 保存合成 Query 的调用计数以证明只解析一次。
    let mut inspect_calls = 0_u8;
    // 使用窄只读 Query 解析 canonical live 会话。
    let resolved = resolve_browser_session_with(
        // 传递 canonical Browser Session identity。
        "s2:bs:00000000000000000000000000000000",
        // 注入不可能执行 open 或 close 的只读 callable。
        |session_id, timeout_ms| {
            // 记录唯一 inspect 调用。
            inspect_calls += 1;
            // Query 必须接收原 canonical identity。
            assert_eq!(session_id, "s2:bs:00000000000000000000000000000000");
            // Query 必须使用统一固定预算。
            assert_eq!(timeout_ms, browser_session_client::DEFAULT_TIMEOUT_MS);
            // 建立 live 事实。
            Ok(())
        },
    )
    // 合成 live Query 不应失败。
    .unwrap_or_else(|error| panic!("live browser session inspect failed: {error}"));
    // 只允许一次 session.inspect。
    assert_eq!(inspect_calls, 1);
    // 解析结果必须发布当前 live 会话的生命周期与页面 capability。
    assert_eq!(
        resolved.capabilities,
        BTreeSet::from([
            // 保持生命周期关闭 capability。
            capabilities::BROWSER_SESSION_CLOSE.to_owned(),
            // 发布页面导航 capability。
            capabilities::BROWSER_PAGE_NAVIGATE.to_owned(),
            // 发布页面等待 capability。
            capabilities::BROWSER_PAGE_WAIT.to_owned(),
            // 发布页面查询 capability。
            capabilities::BROWSER_PAGE_QUERY.to_owned(),
            // 发布元素点击 capability。
            capabilities::BROWSER_ELEMENT_CLICK.to_owned(),
            // 发布元素输入 capability。
            capabilities::BROWSER_ELEMENT_TYPE.to_owned(),
            // 发布页面截图 capability。
            capabilities::BROWSER_PAGE_SCREENSHOT.to_owned(),
        ])
    );
    // stale 必须保持原结构化错误码。
    let stale = resolve_browser_session_with(
        // 使用另一 canonical identity。
        "s2:bs:11111111111111111111111111111111",
        // 注入私有 Query 的 stale 结果。
        |_, _| {
            // 返回 Broker Module 冻结的 stale 错误。
            Err(AppControlError::new("STALE_SESSION", "fixture stale"))
        },
    )
    // stale 不得产生成功目标。
    .expect_err("stale browser session must fail");
    // stale 不得降级为 unsupported 或 unavailable。
    assert_eq!(stale.code, "STALE_SESSION");
    // transport、timeout 与未知结果必须逐码传播。
    for code in ["BROKER_UNAVAILABLE", "TIMEOUT", "OUTCOME_UNKNOWN"] {
        // 注入当前结构化下层错误。
        let error = resolve_browser_session_with(
            // 使用 canonical fixture identity。
            "s2:bs:22222222222222222222222222222222",
            // 让只读 Query 返回指定错误码。
            |_, _| Err(AppControlError::new(code, "fixture inspect failure")),
        )
        // 下层失败不得形成成功 assessment。
        .expect_err("browser session inspect failure must propagate");
        // 不得把下层错误压成 stale 或 unsupported。
        assert_eq!(error.code, code);
    }
    // 八条 assessment 规则必须与运行时 registry 元数据保持同源。
    for (capability, expected_kind) in [
        // open 绑定通用 host 类别。
        (
            capabilities::BROWSER_SESSION_OPEN,
            AssessmentTargetKind::Opaque(OpaqueTargetKind::Host),
        ),
        // close 绑定 assessment 私有 Browser Session 类别。
        (
            capabilities::BROWSER_SESSION_CLOSE,
            AssessmentTargetKind::BrowserSession,
        ),
        // navigate 绑定 assessment 私有 Browser Session 类别。
        (
            capabilities::BROWSER_PAGE_NAVIGATE,
            AssessmentTargetKind::BrowserSession,
        ),
        // wait 绑定 assessment 私有 Browser Session 类别。
        (
            capabilities::BROWSER_PAGE_WAIT,
            AssessmentTargetKind::BrowserSession,
        ),
        // query 绑定 assessment 私有 Browser Session 类别。
        (
            capabilities::BROWSER_PAGE_QUERY,
            AssessmentTargetKind::BrowserSession,
        ),
        // click 绑定 assessment 私有 Browser Session 类别。
        (
            // 使用元素点击 capability。
            capabilities::BROWSER_ELEMENT_CLICK,
            // 顶层 exact target 仍是 live Browser Session。
            AssessmentTargetKind::BrowserSession,
        ),
        // type 绑定 assessment 私有 Browser Session 类别。
        (
            // 使用元素输入 capability。
            capabilities::BROWSER_ELEMENT_TYPE,
            // 顶层 exact target 仍是 live Browser Session。
            AssessmentTargetKind::BrowserSession,
        ),
        // screenshot 绑定 assessment 私有 Browser Session 类别。
        (
            // 使用页面截图 capability。
            capabilities::BROWSER_PAGE_SCREENSHOT,
            // 顶层 exact target 仍是 live Browser Session。
            AssessmentTargetKind::BrowserSession,
        ),
    ] {
        // 读取唯一静态 assessment 规则。
        let rule = find_rule(capability)
            // 缺失规则表示公开路线尚未完整登记。
            .unwrap_or_else(|| panic!("missing browser session assessment rule: {capability}"));
        // 读取唯一运行时 registry 定义。
        let definition = capabilities::definition(capability)
            // 缺失定义表示 assessment 不得宣称 Rust 可用。
            .unwrap_or_else(|| panic!("missing browser session registry entry: {capability}"));
        // 目标类别必须保持每条 capability 的固定风险语义。
        assert_eq!(rule.target_kind, expected_kind);
        // 执行域必须由 registry 与 assessment 一致声明。
        assert_eq!(rule.execution_realm, definition.execution_realm);
        // mutation 分类必须与 generic action 同源。
        assert_eq!(rule.requires_confirmation, definition.action.mutates());
        // 前台影响要求必须与 registry 一致。
        assert_eq!(
            rule.requires_foreground_consent,
            definition.requires_foreground_consent
        );
    }
    // 畸形 Browser Session 必须在任何 Broker Query 前失败。
    let malformed = resolve_target("s2:bs:ABCDEF")
        // 畸形输入意外成功表示通用 opaque parser 错误接管。
        .expect_err("malformed browser session must fail before broker");
    // 输入外壳错误必须与 canonical stale 明确区分。
    assert_eq!(malformed.code, "INVALID_ARGUMENT");
}

// 验证规则覆盖 Rust registry 与显式待迁移清单。
#[test]
fn rules_cover_runtime_registry_without_duplicates() {
    // 收集全部规则 ID。
    let ids = RULES.iter().map(|rule| rule.id).collect::<Vec<_>>();
    // 去重后数量必须一致。
    let unique = ids.iter().copied().collect::<BTreeSet<_>>();
    // 禁止重复规则产生不稳定首命中。
    assert_eq!(unique.len(), ids.len());
    // 每个 Rust registry capability 必须具有 assessment 规则。
    for definition in capabilities::ALL {
        // 断言规则存在。
        assert!(
            unique.contains(definition.id),
            "missing assessment rule: {}",
            definition.id
        );
    }
}

// 验证公共结果不含原生目标字段。
#[test]
fn public_assessment_contains_no_native_identifiers() {
    // 构造窗口只读结果。
    let window = target(
        OpaqueTargetKind::Window,
        AssessmentAvailability::Available,
        &[capabilities::WINDOW_METADATA_READ],
    );
    // 执行纯评估。
    let result = evaluate(
        capabilities::WINDOW_METADATA_READ,
        "s2:w:0000000000000000",
        &window,
    );
    // 序列化用于禁止字段扫描。
    let text = result.to_string().to_ascii_lowercase();
    // 禁止 HWND、PID、路径和 provider identity。
    for forbidden in ["hwnd", "processid", "native", "path", "providerid"] {
        // 任一泄漏都使测试失败。
        assert!(
            !text.contains(forbidden),
            "forbidden assessment field: {forbidden}"
        );
    }
}
