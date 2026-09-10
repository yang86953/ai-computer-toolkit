// 导入被测 Policy 私有路由、固定 companion 与请求类型。
use super::super::*;
// 导入已拆分的固定 Browser Session Broker 名称。
use super::super::browser_session_policy::BROKER_FILE_NAME as BROWSER_SESSION_BROKER_FILE_NAME;

// 浏览器会话生命周期与页面操作必须固定到同一个认证长期 Broker。
// 将下面的纯路由断言注册为 Rust 单元测试。
#[test]
// 使用统一过滤名称定位公开浏览器会话路由回归。
fn browser_session_public_route_uses_certified_broker() {
    // 分别覆盖八个公开 generic operation 与精确 capability 的组合。
    for (operation, capability) in [
        // 打开会话使用 create。
        ("create", capabilities::BROWSER_SESSION_OPEN),
        // 关闭会话使用 close。
        ("close", capabilities::BROWSER_SESSION_CLOSE),
        // 页面导航使用 apply。
        ("apply", capabilities::BROWSER_PAGE_NAVIGATE),
        // 页面等待使用 read。
        ("read", capabilities::BROWSER_PAGE_WAIT),
        // 页面查询使用 read。
        ("read", capabilities::BROWSER_PAGE_QUERY),
        // 元素点击使用 apply。
        ("apply", capabilities::BROWSER_ELEMENT_CLICK),
        // 元素输入使用 apply。
        ("apply", capabilities::BROWSER_ELEMENT_TYPE),
        // 页面截图使用 read。
        ("read", capabilities::BROWSER_PAGE_SCREENSHOT),
    ] {
        // 从不访问 provider 的统一 app 请求开始。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 绑定当前公开 generic operation。
        request.operation = Some(operation.to_owned());
        // 写入唯一决定固定隔离路线的 capability。
        request.args.insert(
            // 使用统一 capability 参数名。
            "capability".to_owned(),
            // 写入本次受测的稳定 capability ID。
            json!(capability),
        );
        // 固定路线必须不依赖当前 sibling 是否存在。
        assert_eq!(
            // 读取纯 Policy companion 选择。
            isolated_worker_file_name(&request),
            // 生命周期与页面操作都只能进入同一固定 Broker。
            Some(BROWSER_SESSION_BROKER_FILE_NAME)
        );
    }
}

// 浏览器页面 capability 必须共享隔离域、零前台与严格零打扰计划。
// 将下面的同源策略断言注册为 Rust 单元测试。
#[test]
// 使用统一过滤名称定位浏览器页面策略元数据回归。
fn browser_session_public_route_page_policy_is_isolated_and_never_foreground() {
    // 分别覆盖严格页面 Command 与两项语义 Query 的强类型 action。
    for (operation, capability, expected_action) in [
        // 导航使用会改变页面代际的 apply。
        (
            "apply",
            capabilities::BROWSER_PAGE_NAVIGATE,
            capabilities::CapabilityAction::Apply,
        ),
        // 等待使用无隐藏副作用的 read。
        (
            "read",
            capabilities::BROWSER_PAGE_WAIT,
            capabilities::CapabilityAction::Read,
        ),
        // 元素点击使用会改变页面状态的 apply。
        (
            // 使用 generic apply。
            "apply",
            // 绑定确认式点击 capability。
            capabilities::BROWSER_ELEMENT_CLICK,
            // Registry 必须声明 apply action。
            capabilities::CapabilityAction::Apply,
        ),
        // 元素输入使用会改变页面状态的 apply。
        (
            // 使用 generic apply。
            "apply",
            // 绑定确认式输入 capability。
            capabilities::BROWSER_ELEMENT_TYPE,
            // Registry 必须声明 apply action。
            capabilities::CapabilityAction::Apply,
        ),
        // 查询使用无隐藏副作用的 read。
        (
            "read",
            capabilities::BROWSER_PAGE_QUERY,
            capabilities::CapabilityAction::Read,
        ),
    ] {
        // 从 App surface 单一 registry 读取精确定义。
        let definition = capabilities::definition_for_surface(
            // 页面 capability 只属于统一 App surface。
            CapabilitySurface::App,
            // 使用当前受测稳定 capability。
            capability,
        )
        // 缺失定义表示公开路由没有完成登记。
        .unwrap_or_else(|| panic!("missing browser page registry entry: {capability}"));
        // action 必须与公开 generic operation 保持同源。
        assert_eq!(definition.action, expected_action);
        // 五项严格页面操作都只能在隔离 worker 域执行。
        assert_eq!(definition.execution_realm, ExecutionRealm::IsolatedWorker);
        // 不得声明或请求前台影响同意。
        assert!(!definition.requires_foreground_consent);
        // 不得用预先前台同意改变路线。
        assert!(!definition.requires_upfront_foreground_consent);
        // 构造不触碰 provider 的严格统一 App 请求。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 绑定与 action 同源的 generic operation。
        request.operation = Some(operation.to_owned());
        // 要求严格零打扰。
        request.isolation_requirement = IsolationRequirement::Strict;
        // 写入唯一决定隔离路线的 capability。
        request.args.insert(
            // 使用统一 capability 参数名。
            "capability".to_owned(),
            // 写入当前稳定 ID。
            json!(capability),
        );
        // 从公开 catalog 读取对应 generic operation。
        let operation = catalog::operation("app", operation)
            // 缺失 operation 表示 catalog 与 registry 已漂移。
            .unwrap_or_else(|| panic!("missing app operation for {capability}"));
        // 冻结不解析 target 或 input 的纯 Policy 计划。
        let plan = build_run_execution_plan(&request, operation)
            // 已登记 capability 必须能形成执行计划。
            .unwrap_or_else(|error| panic!("browser page policy plan failed: {error}"));
        // 契约要求域必须保持隔离 worker。
        assert_eq!(plan.required_realm, ExecutionRealm::IsolatedWorker);
        // 固定 Broker 路由必须真实认证为同一隔离域。
        assert_eq!(plan.runtime_realm, ExecutionRealm::IsolatedWorker);
        // 严格请求必须冻结零打扰主机影响策略。
        assert_eq!(
            plan.host_impact_policy,
            HostImpactPolicy::StrictNoInterference
        );
        // companion 可达性不得影响固定路由认证事实。
        assert!(plan.isolated_route_certified);
    }
}

// 页面截图必须保持 standard、后台优先和只读隔离计划。
// 将下面的截图策略断言注册为 Rust 单元测试。
#[test]
// 使用统一过滤名称定位页面截图 Policy 回归。
fn browser_session_public_route_screenshot_is_standard_background_query() {
    // 从 App surface 单一 registry 读取页面截图定义。
    let definition = capabilities::definition_for_surface(
        // 页面截图只属于统一 App surface。
        CapabilitySurface::App,
        // 使用稳定页面截图 ID。
        capabilities::BROWSER_PAGE_SCREENSHOT,
    )
    // 缺失定义表示公开路线没有完成登记。
    .unwrap_or_else(|| panic!("missing browser page screenshot registry entry"));
    // 页面截图必须投影到 generic read。
    assert_eq!(definition.action, capabilities::CapabilityAction::Read);
    // 公开 descriptor 必须声明后台优先。
    assert_eq!(definition.execution, "background-preferred");
    // 实际路线必须固定隔离 worker。
    assert_eq!(definition.execution_realm, ExecutionRealm::IsolatedWorker);
    // 页面截图不得要求前台影响同意。
    assert!(!definition.requires_foreground_consent);
    // 不得以前台同意改变路线。
    assert!(!definition.requires_upfront_foreground_consent);
    // 构造不触碰 provider 的 standard read 请求。
    let mut request = CommandRequest::read(Verb::Run, "app");
    // 绑定 generic read。
    request.operation = Some("read".to_owned());
    // 显式保持 standard isolation。
    request.isolation_requirement = IsolationRequirement::Standard;
    // 写入唯一决定固定 Broker 路由的 capability。
    request.args.insert(
        // 使用统一 capability 参数名。
        "capability".to_owned(),
        // 写入稳定页面截图 ID。
        json!(capabilities::BROWSER_PAGE_SCREENSHOT),
    );
    // 从公开 catalog 读取 generic read operation。
    let operation = catalog::operation("app", "read")
        // 缺失 operation 表示 catalog 与 registry 漂移。
        .unwrap_or_else(|| panic!("missing app read operation"));
    // 冻结不解析 target 或 input 的纯 Policy 计划。
    let plan = build_run_execution_plan(&request, operation)
        // 已登记 screenshot 必须形成标准执行计划。
        .unwrap_or_else(|error| panic!("browser page screenshot policy plan failed: {error}"));
    // 契约要求域固定隔离 worker。
    assert_eq!(plan.required_realm, ExecutionRealm::IsolatedWorker);
    // 固定 Broker 路由必须认证同一隔离域。
    assert_eq!(plan.runtime_realm, ExecutionRealm::IsolatedWorker);
    // standard Query 必须保持后台优先主机影响策略。
    assert_eq!(
        plan.host_impact_policy,
        HostImpactPolicy::BackgroundPreferred
    );
    // companion 可达性不得改变固定路由认证事实。
    assert!(plan.isolated_route_certified);
}

// 严格浏览器会话命令必须在读取 sibling 可达性前要求确认。
// 将下面的确认优先断言注册为 Rust 单元测试。
#[test]
// 使用统一过滤名称定位公开浏览器会话确认顺序回归。
fn browser_session_public_route_requires_confirmation_before_strict_broker_probe() {
    // 分别覆盖五项 mutation capability 在 strict 计划前的确认门禁。
    for (operation, capability) in [
        // 打开会话使用 create。
        ("create", capabilities::BROWSER_SESSION_OPEN),
        // 关闭会话使用 close。
        ("close", capabilities::BROWSER_SESSION_CLOSE),
        // 页面导航使用 apply。
        ("apply", capabilities::BROWSER_PAGE_NAVIGATE),
        // 元素点击使用 apply。
        ("apply", capabilities::BROWSER_ELEMENT_CLICK),
        // 元素输入使用 apply。
        ("apply", capabilities::BROWSER_ELEMENT_TYPE),
    ] {
        // 构造尚未确认的严格统一 app 请求。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 绑定当前公开 generic operation。
        request.operation = Some(operation.to_owned());
        // 要求 strict 以覆盖原先会探测 sibling 的路径。
        request.isolation_requirement = IsolationRequirement::Strict;
        // 写入唯一决定固定 Broker 路由的 capability。
        request.args.insert(
            // 使用统一 capability 参数名。
            "capability".to_owned(),
            // 写入本次受测的稳定 capability ID。
            json!(capability),
        );
        // 缺少 target 与 input 不得抢在确认错误前报告。
        let error = validate(&request).expect_err("unconfirmed browser session command must fail");
        // sibling 缺失、不可达或字段缺口均不得越过 confirmation-first 边界。
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }
}

// 页面导航必须在确认之后拒绝 standard，避免形成契约外成功元数据。
#[test]
// 使用统一过滤名称定位导航隔离门禁。
fn browser_session_public_route_navigate_requires_strict_after_confirmation() {
    // 从尚未确认的 standard App 请求开始。
    let mut request = CommandRequest::read(Verb::Run, "app");
    // 绑定页面导航 generic operation。
    request.operation = Some("apply".to_owned());
    // 写入唯一页面导航 capability。
    request.args.insert(
        // 使用统一 capability 字段名。
        "capability".to_owned(),
        // 写入稳定页面导航 ID。
        json!(capabilities::BROWSER_PAGE_NAVIGATE),
    );
    // 未确认仍必须先返回确认错误。
    let error = validate(&request).expect_err("confirmation must precede isolation");
    // 锁定 confirmation-first 顺序。
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    // 完成逐操作确认以进入隔离要求门禁。
    request.confirmed = true;
    // standard 页面导航必须在任何 Broker 探测前失败。
    let error = validate(&request).expect_err("standard navigate must fail closed");
    // 公共契约把调用模式错误归类为参数错误。
    assert_eq!(error.code, "INVALID_ARGUMENT");
}
