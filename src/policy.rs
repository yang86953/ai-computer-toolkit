use serde_json::{Value, json};

use crate::{
    // 读取 app capability 的单一运行时定义。
    capabilities::{self, CapabilitySurface},
    catalog,
    // 导入统一输出覆盖门禁与 companion 可达性检查。
    components::{
        // 导入非跟随输出检查与封闭错误。
        output_guard::{OutputGuardError, guard_file_output},
        // 只读检查认证 observation companion 是否可达。
        worker_process::sibling_companion_available,
    },
    // 导入结构化请求、错误与强类型执行策略。
    domain::{
        AppControlError, AppResult, CommandRequest, ExecutionRealm, HostImpactPolicy,
        IsolationRequirement, Verb,
    },
    recording::RecordingConfig,
};

// 注册独立交互会话专属的纯 Policy 边界，控制主文件规模。
#[path = "policy_interactive_session.rs"]
mod interactive_session_policy;
// 注册 Browser Session 生命周期专属的固定 Policy 事实。
#[path = "policy_browser_session.rs"]
mod browser_session_policy;

// 固定当前唯一已认证 Rust 隔离执行 companion。
const OBSERVATION_WORKER_FILE_NAME: &str = "ai-computer-toolkit-observation-worker.exe";
// 固定已认证 Rust 捕获 companion。
const CAPTURE_WORKER_FILE_NAME: &str = "ai-computer-toolkit-capture-worker.exe";
// 固定已认证 Rust 浏览器截图与会话 Broker companion。
const BROWSER_WORKER_FILE_NAME: &str = "ai-computer-toolkit-browser-worker.exe";
// 固定已认证 Rust 精确窗口录制 companion。
const RECORDING_WORKER_FILE_NAME: &str = "ai-computer-toolkit-recording-worker.exe";

// 表示 Policy 边界允许产生的封闭公开错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PolicyErrorCode {
    // 表示 provider 结果或安全检查无法完成。
    OperationFailed,
    // 表示请求不在公开 capability 目录内。
    CapabilityGap,
    // 表示调用方参数不满足公开契约。
    InvalidArgument,
    // 表示 generic app capability 未登记。
    CapabilityUnsupported,
    // 表示目录声明了未知后台执行策略。
    BackgroundOperationUnavailable,
    // 表示写操作缺少逐操作确认。
    ConfirmationRequired,
    // 表示严格隔离需要的认证 companion 当前不可达。
    IsolatedWorkerUnavailable,
    // 表示请求的严格隔离无法由当前执行域满足。
    IsolationRequired,
    // 表示安全后台路线不可用且缺少前台同意。
    ForegroundConsentRequired,
    // 表示输出文件已存在但缺少覆盖许可。
    OverwriteConfirmationRequired,
}

// 为 Policy 错误码提供既有公开文本与统一错误构造。
impl PolicyErrorCode {
    // 返回 error envelope 使用的稳定大写下划线文本。
    const fn as_str(self) -> &'static str {
        // 映射全部封闭 Policy 错误码。
        match self {
            // 输出操作失败码。
            Self::OperationFailed => "OPERATION_FAILED",
            // 输出 capability 目录缺口码。
            Self::CapabilityGap => "CAPABILITY_GAP",
            // 输出参数错误码。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 输出 capability 不支持码。
            Self::CapabilityUnsupported => "CAPABILITY_UNSUPPORTED",
            // 输出后台操作不可用码。
            Self::BackgroundOperationUnavailable => "BACKGROUND_OPERATION_UNAVAILABLE",
            // 输出逐操作确认缺失码。
            Self::ConfirmationRequired => "CONFIRMATION_REQUIRED",
            // 输出隔离 worker 不可用码。
            Self::IsolatedWorkerUnavailable => "ISOLATED_WORKER_UNAVAILABLE",
            // 输出严格隔离要求码。
            Self::IsolationRequired => "ISOLATION_REQUIRED",
            // 输出前台同意缺失码。
            Self::ForegroundConsentRequired => "FOREGROUND_CONSENT_REQUIRED",
            // 输出覆盖确认缺失码。
            Self::OverwriteConfirmationRequired => "OVERWRITE_CONFIRMATION_REQUIRED",
        }
    }

    // 构造不携带 details 的统一公开错误。
    fn error(self, message: impl Into<String>) -> AppControlError {
        // 只把封闭码映射到既有通用 error envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 构造携带 provider-neutral details 的统一公开错误。
    fn with_details(self, message: impl Into<String>, details: Value) -> AppControlError {
        // details 仍由具体 Policy 判定拥有，错误码只由本枚举选择。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

// 保存 System 在 provider 解析前冻结的执行计划。
#[derive(Debug, Clone, Copy)]
pub(crate) struct ExecutionPlan {
    // 保存 catalog 或 capability 契约要求的 realm。
    required_realm: ExecutionRealm,
    // 保存当前 Rust 路由真实使用的 realm。
    runtime_realm: ExecutionRealm,
    // 保存主机影响策略。
    host_impact_policy: HostImpactPolicy,
    // 保存调用方隔离要求。
    isolation_requirement: IsolationRequirement,
    // 标记隔离路由是否经过认证。
    isolated_route_certified: bool,
    // 标记认证 companion 是否当前可达。
    isolated_worker_available: bool,
}

// 为冻结计划提供结果证明与测试读取。
impl ExecutionPlan {
    // 把执行计划附加到成功结果且拒绝 provider 自报冲突。
    pub(crate) fn attest_result(self, result: &mut Value) -> AppResult<()> {
        // 成功结果必须保持对象形状。
        let object = result.as_object_mut().ok_or_else(|| {
            // provider 非对象结果违反公开契约。
            PolicyErrorCode::OperationFailed.error(
                // 不回显 provider 私有输出。
                "The provider returned a non-object execution result.",
            )
        })?;
        // 序列化当前真实执行域。
        let runtime_realm = json!(self.runtime_realm);
        // provider 若已声明 realm 则必须与 System 计划一致。
        if object
            // 读取可选 provider realm。
            .get("executionRealm")
            // 仅冲突值触发失败。
            .is_some_and(|value| value != &runtime_realm)
        {
            // 拒绝 provider 自报的域漂移。
            return Err(PolicyErrorCode::OperationFailed.error(
                // 不公开 provider 名称或路径。
                "The provider execution realm conflicts with the frozen policy plan.",
            ));
        }
        // 输出实际执行域。
        object.insert("executionRealm".to_owned(), runtime_realm);
        // 输出契约要求的执行域。
        object.insert(
            // 使用稳定字段名。
            "requiredExecutionRealm".to_owned(),
            // 序列化强类型要求。
            json!(self.required_realm),
        );
        // 输出调用方隔离要求。
        object.insert(
            // 使用稳定字段名。
            "isolationRequirement".to_owned(),
            // 序列化强类型要求。
            json!(self.isolation_requirement),
        );
        // 输出不可由 provider 放宽的主机影响策略。
        object.insert(
            // 使用稳定字段名。
            "hostImpactPolicy".to_owned(),
            // 序列化强类型策略。
            json!(self.host_impact_policy),
        );
        // 输出 realm 是否由当前路由真实满足。
        object.insert(
            // 使用稳定字段名。
            "executionRealmCertified".to_owned(),
            // 非隔离域按真实域相等认证；隔离域还要求 companion 可达。
            Value::Bool(
                self.required_realm == self.runtime_realm
                    && (self.required_realm != ExecutionRealm::IsolatedWorker
                        || (self.isolated_route_certified && self.isolated_worker_available)),
            ),
        );
        // 成功附加 System 证据。
        Ok(())
    }
}

pub fn validate(request: &CommandRequest) -> AppResult<()> {
    let descriptor = catalog::app(&request.app).ok_or_else(|| {
        PolicyErrorCode::CapabilityGap.with_details(
            format!("公共 capability 目录中没有应用 '{}'.", request.app),
            json!({
                "app": request.app,
                "verb": request.verb,
                "operation": request.operation,
                "target": request.target,
                "availableApps": catalog::apps().iter().map(|app| app.id).collect::<Vec<_>>(),
            }),
        )
    })?;

    if !descriptor.public_verbs.contains(&request.verb) {
        return Err(PolicyErrorCode::CapabilityGap.with_details(
            format!(
                "{}.{} 不在公共 capability 目录中。",
                request.app,
                request.verb.as_str()
            ),
            json!({
                "app": request.app,
                "verb": request.verb,
                "operation": request.operation,
                "target": request.target,
                "allowedVerbs": descriptor.public_verbs,
            }),
        ));
    }

    if request.verb != Verb::Run {
        // 严格读取也必须在 adapter 解析前验证 host-headless 或 companion realm。
        if request.isolation_requirement == IsolationRequirement::Strict {
            // 冻结读取计划。
            let plan = build_read_execution_plan(request, descriptor);
            // 执行同一严格状态机。
            enforce_strict_isolation(request, plan)?;
        }
        // 普通读取不需要 operation 目录校验。
        return Ok(());
    }

    let operation_id = request.operation.as_deref().ok_or_else(|| {
        // 缺失 operation 只允许产生封闭参数错误。
        PolicyErrorCode::InvalidArgument.error("run 操作必须给出 operation。")
    })?;
    let operation = catalog::operation(&request.app, operation_id).ok_or_else(|| {
        PolicyErrorCode::CapabilityGap.with_details(
            format!("操作 '{}.{}' 不在公共 capability 目录中。", request.app, operation_id),
            json!({
                "app": request.app,
                "operation": operation_id,
                "target": request.target,
                "availableOperations": descriptor.operations.iter().map(|item| item.operation).collect::<Vec<_>>(),
            }),
        )
    })?;

    // 浏览器会话与页面 Command 必须在任何 strict companion 探测前先确认。
    if !request.confirmed
        // 只对固定 Broker 承接的 mutation 提前确认。
        && browser_session_policy::requires_confirmation_first(requested_capability(request))
    {
        // 固定公开确认错误，禁止 sibling 缺口抢先暴露。
        return Err(PolicyErrorCode::ConfirmationRequired.error("状态变更操作需要 --confirm。"));
    }
    // 在专属 Policy 子模块冻结页面导航隔离要求。
    browser_session_policy::validate_isolation(
        requested_capability(request),
        request.isolation_requirement,
    )?;
    // 其他严格模式在 provider 解析、确认和前台同意之前冻结并验证执行计划。
    // 独立会话字段只允许进入冻结的四条严格 mutation 路由。
    interactive_session_policy::validate(request)?;

    if request.isolation_requirement == IsolationRequirement::Strict {
        // 构造只依赖 catalog、registry 与固定 companion 可达性的计划。
        let plan = build_run_execution_plan(request, operation)?;
        // 禁止不满足零打扰的 realm 或 worker 缺口继续。
        enforce_strict_isolation(request, plan)?;
    }

    if operation.background_policy == "foreground-consent" && !request.foreground_consent {
        return Err(foreground_consent_required(
            request,
            "该通用操作没有可认证的后台协议。",
            json!({ "backgroundPolicy": operation.background_policy }),
        ));
    }
    if !matches!(
        operation.background_policy,
        "guaranteed" | "best-effort" | "prefer-background-then-consent" | "foreground-consent"
    ) {
        return Err(
            PolicyErrorCode::BackgroundOperationUnavailable.error("操作声明了未知的执行策略。")
        );
    }
    if operation.requires_confirmation && !request.confirmed {
        return Err(PolicyErrorCode::ConfirmationRequired.error("状态变更操作需要 --confirm。"));
    }
    // 使用 catalog 同源字段契约验证并封闭完整 target 集合。
    validate_catalog_fields(&request.target, operation.target_fields, "target")?;
    // 使用同一 catalog 定义验证并封闭完整 args 集合。
    validate_catalog_fields(&request.args, operation.argument_fields, "args")?;
    validate_known_arguments(request, operation.operation)?;
    Ok(())
}

// 为已经通过通用目录校验的请求生成可证明执行计划。
pub(crate) fn execution_plan(request: &CommandRequest) -> AppResult<Option<ExecutionPlan>> {
    // 从同一公开 catalog 解析 app descriptor。
    let descriptor = catalog::app(&request.app).ok_or_else(|| {
        // 保持目录缺口错误。
        PolicyErrorCode::CapabilityGap.error(
            // 不输出整个目录。
            format!("公共 capability 目录中没有应用 '{}'.", request.app),
        )
    })?;
    // 非运行请求使用强类型读取计划。
    if request.verb != Verb::Run {
        // 返回主机无头或专用隔离读取计划。
        return Ok(Some(build_read_execution_plan(request, descriptor)));
    }
    // 读取已由 validate 要求的 operation。
    let operation_id = request.operation.as_deref().ok_or_else(|| {
        // 保持缺失 operation 的稳定错误。
        PolicyErrorCode::InvalidArgument.error("run 操作必须给出 operation。")
    })?;
    // 从同一 catalog 解析操作描述。
    let operation = catalog::operation(&request.app, operation_id).ok_or_else(|| {
        // 理论上的目录漂移必须 fail closed。
        PolicyErrorCode::CapabilityGap.error(
            // 不重复输出整个目录。
            "The validated operation is no longer present in the capability catalog.",
        )
    })?;
    // 返回冻结计划。
    build_run_execution_plan(request, operation).map(Some)
}

// 告知 ComputerControlSystem 当前请求是否选择独立会话 route。
pub(crate) fn uses_interactive_session(request: &CommandRequest) -> bool {
    // 复用 Policy 私有字段所有权，避免 System 复制字段判定。
    interactive_session_policy::requested(request)
}

// 从 generic app capability 或直接操作目录解析契约要求 realm。
fn required_execution_realm(
    // 接收完整 provider-neutral 请求。
    request: &CommandRequest,
    // 接收已解析的操作描述。
    operation: &catalog::OperationDescriptor,
) -> AppResult<ExecutionRealm> {
    // 独立会话 route 覆盖原 capability 的本地执行域。
    if let Some(realm) = interactive_session_policy::execution_realm(request)? {
        // 返回已经完整验证的隔离域。
        return Ok(realm);
    }
    // 非 generic app 操作直接使用 catalog 的强类型 realm。
    if request.app != "app" {
        // 返回目录事实。
        return Ok(operation.execution_realm);
    }
    // generic app verb 必须携带版本化 capability。
    let capability = request
        // 读取 provider-neutral args。
        .args
        // 读取 capability 字段。
        .get("capability")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 拒绝空值。
        .filter(|value| !value.is_empty())
        // 返回稳定参数错误。
        .ok_or_else(|| {
            // 缺失 capability 只允许产生封闭参数错误。
            PolicyErrorCode::InvalidArgument.error("args.capability is required.")
        })?;
    // 只允许 app surface 的 Rust 单一 registry 定义。
    let definition = capabilities::definition_for_surface(CapabilitySurface::App, capability)
        // 未登记 capability 不进入任何 provider。
        .ok_or_else(|| {
            // 返回稳定 capability 错误。
            PolicyErrorCode::CapabilityUnsupported.error(
                // 不列出内部 provider。
                format!("Unknown capability '{capability}'."),
            )
        })?;
    // 返回 registry 精确 realm。
    Ok(definition.execution_realm)
}

// 读取 generic app 请求的 capability ID。
fn requested_capability(request: &CommandRequest) -> Option<&str> {
    // 仅 generic app surface 携带 capability。
    (request.app == "app")
        // 按条件继续读取字段。
        .then(|| request.args.get("capability"))
        // 展平可选 JSON 值。
        .flatten()
        // 只接受字符串。
        .and_then(Value::as_str)
}

// 返回当前 Rust 隔离路线认证的固定 companion 文件名。
fn isolated_worker_file_name(request: &CommandRequest) -> Option<&'static str> {
    // 独立会话 route 使用固定长期 broker，而不是直接启动 command worker。
    if interactive_session_policy::requested(request) {
        // 返回认证 endpoint 的固定生产 companion。
        return Some(interactive_session_policy::BROKER_FILE_NAME);
    }
    // legacy browser screenshot 固定使用 Job-bounded Rust browser worker。
    if request.app == "browser" && request.operation.as_deref() == Some("screenshot") {
        // 返回固定浏览器 companion。
        return Some(BROWSER_WORKER_FILE_NAME);
    }
    // 浏览器会话生命周期与页面操作只能使用固定长期 Rust Broker。
    if browser_session_policy::uses_fixed_broker(requested_capability(request)) {
        // 返回唯一已认证的 browser-session Broker sibling。
        return Some(browser_session_policy::BROKER_FILE_NAME);
    }
    // provider-neutral UIA 元素定位与等待复用 Job-bounded observation worker。
    if matches!(
        // 读取请求 capability。
        requested_capability(request),
        // 接受两个已认证只读 UIA Query。
        Some(capabilities::UI_ELEMENT_LOCATE | capabilities::UI_ELEMENT_WAIT)
    ) {
        // 返回固定 observation companion。
        return Some(OBSERVATION_WORKER_FILE_NAME);
    }
    // provider-neutral 精确窗口截图使用 Job-bounded capture worker。
    if requested_capability(request) == Some(capabilities::WINDOW_SCREENSHOT) {
        // 返回固定 capture companion。
        return Some(CAPTURE_WORKER_FILE_NAME);
    }
    // provider-neutral 精确窗口录制使用 Job-bounded recording worker。
    if requested_capability(request) == Some(capabilities::WINDOW_RECORD) {
        // 返回固定录制 companion。
        return Some(RECORDING_WORKER_FILE_NAME);
    }
    // legacy desktop canonical screenshot 已固定委托同一 Rust Module。
    if request.app == "desktop"
        // 只匹配 screenshot operation。
        && request.operation.as_deref() == Some("screenshot")
        // 只有 canonical 目标会进入正式 Rust Module。
        && request
            // 读取 legacy surface 的 sessionId。
            .target
            // 访问固定字段。
            .get("sessionId")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 核对 canonical 窗口前缀。
            .is_some_and(|value| value.starts_with("s2:w:"))
    {
        // 返回固定 capture companion。
        return Some(CAPTURE_WORKER_FILE_NAME);
    }
    // legacy desktop record 已固定委托同一 Rust Recording Module。
    if request.app == "desktop" && request.operation.as_deref() == Some("record") {
        // 返回固定 recording companion。
        return Some(RECORDING_WORKER_FILE_NAME);
    }
    // 其他隔离候选尚未完成认证。
    None
}

// 构造不触碰 provider、目标清单或用户应用的执行计划。
fn build_run_execution_plan(
    // 接收完整请求。
    request: &CommandRequest,
    // 接收 catalog 操作。
    operation: &catalog::OperationDescriptor,
) -> AppResult<ExecutionPlan> {
    // 解析契约要求的强类型 realm。
    let required_realm = required_execution_realm(request, operation)?;
    // 只为隔离 realm 选择当前认证的固定 companion。
    let isolated_worker_file_name = (required_realm == ExecutionRealm::IsolatedWorker)
        // 延迟读取具体 companion。
        .then(|| isolated_worker_file_name(request))
        // 展平条件与认证结果。
        .flatten();
    // 冻结运行计划。
    Ok(freeze_execution_plan(
        // 传入调用方要求。
        request,
        // 传入契约 realm。
        required_realm,
        // 传入隔离路由认证的固定 companion。
        isolated_worker_file_name,
    ))
}

// 为 status、sessions 与 inspect 构造强类型读取计划。
fn build_read_execution_plan(
    // 接收完整只读请求。
    request: &CommandRequest,
    // 接收公开 app descriptor。
    descriptor: &catalog::AppDescriptor,
) -> ExecutionPlan {
    // UIA 精确 inspect 必须进入 observation companion。
    let isolated_inspect = request.app == "uia" && request.verb == Verb::Inspect;
    // 选择实际读取 realm。
    let required_realm = if isolated_inspect {
        // UIA inspect 使用隔离 worker。
        ExecutionRealm::IsolatedWorker
    } else {
        // 其他读取使用 app descriptor 的强类型域。
        descriptor.read_execution_realm
    };
    // 冻结读取计划。
    freeze_execution_plan(
        // 传入调用方要求。
        request,
        // 传入读取 realm。
        required_realm,
        // 只有 UIA inspect 认证当前 observation companion。
        isolated_inspect.then_some(OBSERVATION_WORKER_FILE_NAME),
    )
}

// 把执行域与固定 companion 状态冻结为不可变计划。
fn freeze_execution_plan(
    // 接收完整请求。
    request: &CommandRequest,
    // 接收契约要求 realm。
    required_realm: ExecutionRealm,
    // 接收隔离路由认证的固定 companion。
    isolated_worker_file_name: Option<&str>,
) -> ExecutionPlan {
    // companion 文件名存在即表示当前路由已认证。
    let isolated_route_certified = isolated_worker_file_name.is_some();
    // 只对认证路由检查对应固定 sibling companion。
    let isolated_worker_available = isolated_worker_file_name
        // 读取文件存在性，不启动 worker。
        .is_some_and(sibling_companion_available);
    // 未认证的隔离候选当前实际仍是主机后台兼容路径。
    let runtime_realm = if required_realm == ExecutionRealm::IsolatedWorker
        // 只有认证 worker 路由才可声明真实隔离域。
        && !isolated_route_certified
    {
        // 明确兼容路径实际位于主机后台域。
        ExecutionRealm::HostBackground
    } else {
        // 其他路径的实际域与要求一致。
        required_realm
    };
    // 返回不可变计划。
    ExecutionPlan {
        // 保存要求域。
        required_realm,
        // 保存实际域。
        runtime_realm,
        // 从隔离要求推导主机影响策略。
        host_impact_policy: HostImpactPolicy::from(request.isolation_requirement),
        // 保存调用方要求。
        isolation_requirement: request.isolation_requirement,
        // 保存路由认证事实。
        isolated_route_certified,
        // 保存 companion 可达事实。
        isolated_worker_available,
    }
}

// 在任何 provider 解析前执行严格零打扰状态机。
fn enforce_strict_isolation(request: &CommandRequest, plan: ExecutionPlan) -> AppResult<()> {
    // 非严格请求无需该门禁。
    if plan.isolation_requirement != IsolationRequirement::Strict {
        // 保持标准兼容行为。
        return Ok(());
    }
    // 主机无头域天然满足严格零打扰。
    if plan.required_realm == ExecutionRealm::HostHeadless {
        // 允许继续。
        return Ok(());
    }
    // 隔离 worker 域必须同时具有认证路由和当前 companion。
    if plan.required_realm == ExecutionRealm::IsolatedWorker {
        // 完整 worker 状态允许继续。
        if plan.isolated_route_certified && plan.isolated_worker_available {
            // 允许进入 provider。
            return Ok(());
        }
        // 缺失或未迁回的 worker 不得进入进程内兼容路径。
        return Err(PolicyErrorCode::IsolatedWorkerUnavailable.with_details(
            // 明确不进行前台或进程内降级。
            "Strict isolation requires a certified companion worker that is currently unavailable.",
            // 只输出 provider-neutral 策略证据。
            json!({
                // 输出调用 surface。
                "app": request.app,
                // 输出公开 operation。
                "operation": request.operation,
                // 输出契约要求 realm。
                "requiredExecutionRealm": plan.required_realm,
                // 标记路由认证事实。
                "isolatedRouteCertified": plan.isolated_route_certified,
                // 标记 companion 可达事实。
                "isolatedWorkerAvailable": plan.isolated_worker_available,
                // 明确前台同意不会被复用。
                "foregroundConsentIgnored": request.foreground_consent,
            }),
        ));
    }
    // 同会话、后台可见、前台与无路径均不满足严格零打扰。
    Err(PolicyErrorCode::IsolationRequired.with_details(
        // 明确必须改用认证无头或隔离 worker 路径。
        "Strict isolation permits only certified host-headless or isolated-worker execution.",
        // 输出不含目标或原生身份的策略证据。
        json!({
            // 输出调用 surface。
            "app": request.app,
            // 输出公开 operation。
            "operation": request.operation,
            // 输出不满足要求的 realm。
            "requiredExecutionRealm": plan.required_realm,
            // 输出强制主机影响策略。
            "hostImpactPolicy": plan.host_impact_policy,
            // 明确前台同意不会被复用。
            "foregroundConsentIgnored": request.foreground_consent,
        }),
    ))
}

pub fn foreground_consent_required(
    request: &CommandRequest,
    reason: &str,
    evidence: Value,
) -> AppControlError {
    PolicyErrorCode::ForegroundConsentRequired.with_details(
        "后台路径不可用；请先向用户说明将激活目标窗口并发送输入，取得明确同意后以 --allow-foreground 重试。",
        json!({
            "reason": reason,
            "app": request.app,
            "operation": request.operation,
            "target": request.target,
            "requiredFlags": ["--confirm", "--allow-foreground"],
            "evidence": evidence,
        }),
    )
}

// 判断必填 JSON 值是否保留迁移前的“有值”语义。
fn required_value_present(value: &Value) -> bool {
    // 空字符串、空数组和 null 不构成必填值；空对象仍可表达默认输入。
    match value {
        // 字符串必须至少包含一个非空白字符。
        Value::String(value) => !value.trim().is_empty(),
        // 数组必须至少包含一个元素。
        Value::Array(items) => !items.is_empty(),
        // null 始终视为缺失。
        Value::Null => false,
        // 数值、布尔和对象保持既有“已提供”语义。
        Value::Bool(_) | Value::Number(_) | Value::Object(_) => true,
    }
}

// 从单一 catalog 字段定义验证存在性与基础 JSON 类型。
fn validate_catalog_field(
    // 接收 target 或 args 的封闭 JSON 对象。
    map: &serde_json::Map<String, Value>,
    // 接收同时驱动 catalog 展示与校验的字段定义。
    field: &catalog::FieldDescriptor,
    // 接收公开错误中的字段组名。
    group: &str,
) -> AppResult<()> {
    // 读取调用方是否实际提供该字段。
    let Some(value) = map.get(field.name) else {
        // 可选字段缺失时无需验证。
        if !field.required {
            // 保持可选字段默认语义。
            return Ok(());
        }
        // 必填字段缺失保持既有错误码和消息。
        return Err(PolicyErrorCode::InvalidArgument.error(
            // 保持既有必填错误文本。
            format!("{}.{} 是必填字段。", group, field.name),
        ));
    };
    // 必填字段的空值保持既有缺失语义。
    if field.required && !required_value_present(value) {
        // 返回与完全缺失相同的稳定错误。
        return Err(PolicyErrorCode::InvalidArgument.error(
            // 保持既有必填错误文本。
            format!("{}.{} 是必填字段。", group, field.name),
        ));
    }
    // 已提供字段必须满足 catalog 声明的基础 JSON 类型。
    if !field.value_type.accepts_type(value) {
        // 类型错误不得继续进入 capability-specific 边界。
        return Err(PolicyErrorCode::InvalidArgument.error(
            // 只输出字段路径和公开类型描述，不回显敏感值。
            format!(
                "{}.{} 必须是 {}。",
                group,
                field.name,
                field.value_type.as_str()
            ),
        ));
    }
    // 基础类型正确后再验证 catalog 拥有的数值范围。
    if !field.value_type.satisfies_constraint(value) {
        // 有界类型必定提供由同一声明生成的范围说明。
        let constraint = field
            // 读取字段强类型。
            .value_type
            // 生成稳定公开范围。
            .constraint_description()
            // 理论上的声明漂移必须 fail closed。
            .unwrap_or_else(|| "declared catalog constraint".to_owned());
        // 范围错误不得继续进入 capability-specific 边界。
        return Err(PolicyErrorCode::InvalidArgument.error(
            // 只输出字段路径和公开约束，不回显调用值。
            format!("{}.{} 必须满足 {}。", group, field.name, constraint),
        ));
    }
    // Catalog 门禁通过，路径与精确目标等领域语义留给既有专属边界。
    Ok(())
}

// 验证一个 operation 外层对象只包含 catalog 声明字段。
fn validate_catalog_fields(
    // 接收 target 或 args 的完整外层对象。
    map: &serde_json::Map<String, Value>,
    // 接收唯一公开允许字段集合。
    fields: &[catalog::FieldDescriptor],
    // 接收稳定字段组名。
    group: &str,
) -> AppResult<()> {
    // 先按 catalog 顺序验证必填与类型，保持既有失败优先级。
    for field in fields {
        // 复用单字段同源门禁。
        validate_catalog_field(map, field, group)?;
    }
    // 检测任意未在 catalog 中声明的外层字段。
    let contains_unknown = map
        // 遍历调用方字段名但不读取值。
        .keys()
        // 任一名称未被 catalog 拥有即视为未知。
        .any(|name| !fields.iter().any(|field| field.name == name));
    // 未知字段必须在 provider 或 capability-specific 校验前失败闭合。
    if contains_unknown {
        // 只返回公开允许集合，不回显调用方未知名称或值。
        return Err(PolicyErrorCode::InvalidArgument.with_details(
            // 只指出公开字段组含未知项。
            format!("{group} 包含不在该 operation 公开 catalog 中的字段。"),
            // 证据只由 catalog 公共事实构成。
            json!({
                "fieldGroup": group,
                "allowedFields": fields.iter().map(|field| field.name).collect::<Vec<_>>(),
            }),
        ));
    }
    // 完整外层字段集合通过。
    Ok(())
}

// 把 Component 输出门禁结果映射为稳定公开策略错误。
fn guard_policy_file_output(path: &std::path::Path, overwrite_confirmed: bool) -> AppResult<()> {
    // 执行不修改文件系统的统一目标检查。
    match guard_file_output(path, overwrite_confirmed) {
        // 缺失目标或已确认普通文件允许继续。
        Ok(()) => Ok(()),
        // 既有普通文件缺少覆盖许可时返回固定错误码。
        Err(OutputGuardError::ConfirmationRequired) => {
            // 使用公开覆盖确认错误码。
            Err(PolicyErrorCode::OverwriteConfirmationRequired.error(
                // 不公开完整输出路径。
                "输出文件已存在；请增加 --arg overwrite=true。",
            ))
        }
        // 目录、链接和特殊目标不得被覆盖许可放宽。
        Err(OutputGuardError::InvalidTargetType) => {
            // 使用参数错误表示目标类型不符合文件契约。
            Err(PolicyErrorCode::InvalidArgument.error(
                // 返回不含路径的稳定诊断。
                "输出路径已存在但不是可覆盖的普通文件。",
            ))
        }
        // 不能可靠读取目标状态时必须失败闭合。
        Err(OutputGuardError::InspectionFailed) => {
            // 保持公开封闭 envelope 内的操作失败码。
            Err(PolicyErrorCode::OperationFailed.error(
                // 不泄漏原生 I/O 错误或路径。
                "无法安全检查输出路径。",
            ))
        }
    }
}

fn validate_known_arguments(request: &CommandRequest, operation: &str) -> AppResult<()> {
    if request.app == "desktop" && operation == "screenshot" {
        let path = request
            .args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                // 路径类型错误只允许产生封闭参数错误。
                PolicyErrorCode::InvalidArgument.error("args.path 必须是 PNG 路径。")
            })?;
        let output = std::path::Path::new(path);
        if output.extension().and_then(|value| value.to_str()) != Some("png") {
            return Err(
                PolicyErrorCode::InvalidArgument.error("desktop.screenshot 当前只写入 .png 文件。")
            );
        }
        let parent = output.parent().ok_or_else(|| {
            // 缺失父目录只允许产生封闭参数错误。
            PolicyErrorCode::InvalidArgument.error("截图路径必须包含父目录。")
        })?;
        if !parent.is_dir() {
            return Err(PolicyErrorCode::InvalidArgument
                .error(format!("截图父目录不存在：{}", parent.display())));
        }
        // 使用统一非跟随门禁检查截图覆盖许可。
        guard_policy_file_output(
            // 检查精确输出路径。
            output,
            // 只有显式布尔 true 才表示覆盖许可。
            request.args.get("overwrite") == Some(&Value::Bool(true)),
        )?;
    }
    if request.app == "desktop" && operation == "record" {
        RecordingConfig::from_args(&request.args)?;
    }
    if request.app == "browser" && operation == "screenshot" {
        let url = request
            .target
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !(url.starts_with("https://")
            || url.starts_with("http://")
            || url.starts_with("file://"))
        {
            return Err(
                PolicyErrorCode::InvalidArgument.error("浏览器仅接受 http、https 或 file URL。")
            );
        }
        if let Some(path) = request.args.get("path").and_then(Value::as_str) {
            let output = std::path::Path::new(path);
            // 使用统一非跟随门禁检查浏览器截图覆盖许可。
            guard_policy_file_output(
                // 检查精确输出路径。
                output,
                // 只有显式布尔 true 才表示覆盖许可。
                request.args.get("overwrite") == Some(&Value::Bool(true)),
            )?;
        }
    }
    Ok(())
}

// 验证 Policy 封闭错误码集合逐字保持既有公开文本。
#[cfg(test)]
#[test]
fn policy_error_codes_preserve_public_contract() {
    // 按策略边界定义顺序列出全部强类型错误码。
    let actual = [
        // provider 结果或安全检查失败。
        PolicyErrorCode::OperationFailed,
        // capability 目录缺口。
        PolicyErrorCode::CapabilityGap,
        // 调用方参数错误。
        PolicyErrorCode::InvalidArgument,
        // generic capability 不支持。
        PolicyErrorCode::CapabilityUnsupported,
        // 后台执行策略不可用。
        PolicyErrorCode::BackgroundOperationUnavailable,
        // 写操作缺少确认。
        PolicyErrorCode::ConfirmationRequired,
        // 隔离 worker 不可用。
        PolicyErrorCode::IsolatedWorkerUnavailable,
        // 严格隔离要求不满足。
        PolicyErrorCode::IsolationRequired,
        // 前台同意缺失。
        PolicyErrorCode::ForegroundConsentRequired,
        // 覆盖确认缺失。
        PolicyErrorCode::OverwriteConfirmationRequired,
    ]
    // 把每个强类型值投影为公开文本。
    .map(PolicyErrorCode::as_str);
    // 全部十种文本必须逐字兼容既有 error envelope。
    assert_eq!(
        // 对比实际投影集合。
        actual,
        // 锁定调用方已经依赖的完整 Policy 错误码集合。
        [
            // 保持操作失败码。
            "OPERATION_FAILED",
            // 保持 capability 目录缺口码。
            "CAPABILITY_GAP",
            // 保持参数错误码。
            "INVALID_ARGUMENT",
            // 保持 capability 不支持码。
            "CAPABILITY_UNSUPPORTED",
            // 保持后台操作不可用码。
            "BACKGROUND_OPERATION_UNAVAILABLE",
            // 保持逐操作确认缺失码。
            "CONFIRMATION_REQUIRED",
            // 保持隔离 worker 不可用码。
            "ISOLATED_WORKER_UNAVAILABLE",
            // 保持严格隔离要求码。
            "ISOLATION_REQUIRED",
            // 保持前台同意缺失码。
            "FOREGROUND_CONSENT_REQUIRED",
            // 保持覆盖确认缺失码。
            "OVERWRITE_CONFIRMATION_REQUIRED",
        ]
    );
}

#[cfg(test)]
mod tests;
