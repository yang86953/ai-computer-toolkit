// 导入共享所有权以持有可替换的只读安全姿态端口。
use std::sync::Arc;

use serde_json::{Value, json};

use crate::{
    adapters::{AdapterRegistry, security_context_windows::WindowsSecurityContextProbe},
    // 读取版本化 capability 的副作用分类。
    capabilities,
    catalog,
    // 投影 Browser Session 在 System provider 前失败的公开生命周期事实。
    components::browser_session_lifecycle_error,
    // 导入请求、错误、动词与调用方隔离要求。
    domain::{AppResult, CommandRequest, IsolationRequirement, Verb, error_json},
    methods,
    // 导入由 System 协调的领域 Modules。
    modules::{
        accessibility,
        application_discovery,
        // 导入由 System 唯一创建并持有的浏览器会话 Module。
        browser_session::BrowserSessionModule,
        build_info,
        capability_assessment,
        capability_metadata,
        // 导入认证独立交互会话发现与 command Module。
        interactive_isolation,
        // 导入固定 broker 的长操作 status/cancel Module。
        long_operation_client,
        // 导入受保护上下文授权端口与访问类别。
        permission_boundary::{self, SecurityContextProbe, TargetAccessKind},
        window_capture_frame_probe,
        window_capture_preflight,
        // 导入长操作业务接受前的窗口录制纯验证。
        window_record,
    },
    policy,
};

// 把错误码实现保留为 ComputerControlSystem 的普通私有类型。
mod error_code;
// 导入当前 System 私有封闭错误码。
use error_code::ComputerControlSystemErrorCode;

// 从已经通过 Policy 的请求推导安全姿态门禁访问类别。
fn target_access_kind(request: &CommandRequest) -> Option<TargetAccessKind> {
    // 独立会话 route 在 host 侧只执行 WTS 与认证 pipe 读取。
    if policy::uses_interactive_session(request) {
        // 真正 mutation 由目标 session 内 System 再执行 mutation 授权。
        return Some(TargetAccessKind::Read);
    }
    // 按顶层 verb 区分无目标查询、精确读取和运行路线。
    match request.verb {
        // 精确 inspect 始终读取目标。
        Verb::Inspect => Some(TargetAccessKind::Read),
        // run 可能承载 registry 中的只读等待或 mutation。
        Verb::Run => {
            // 只对统一 app facade 的版本化 capability 读取精确副作用事实。
            let app_capability = (request.app == "app")
                // 从参数中取得公开 capability ID。
                .then(|| request.args.get("capability").and_then(Value::as_str))
                // 展平可选值。
                .flatten()
                // 只从 App surface 注册表查找定义。
                .and_then(|capability| {
                    // 阻止跨 surface 定义影响 app 请求分类。
                    capabilities::definition_for_surface(
                        // 限定统一 App surface。
                        capabilities::CapabilitySurface::App,
                        // 传入公开 capability ID。
                        capability,
                    )
                });
            // 只读 action 使用目标读取类别，其他和未知路线保守视为 mutation。
            Some(
                if app_capability.is_some_and(|definition| !definition.action.mutates()) {
                    // registry 明确证明无副作用时返回读取。
                    TargetAccessKind::Read
                } else {
                    // legacy、未知或变更 action 均按 mutation 处理。
                    TargetAccessKind::Mutation
                },
            )
        }
        // status 与 session 发现不解析精确目标。
        Verb::Status | Verb::Sessions => None,
    }
}

// 注册 ComputerControlSystem 私有的 sequence Workflow 实现。
mod sequence;
// 注册 sequence Workflow 私有的跨步骤输入绑定契约与应用。
mod sequence_bindings;
// 注册 sequence Workflow 私有的绑定终止错误投影。
mod sequence_binding_errors;
// 注册 sequence Workflow 私有的结构化模板验证与渲染 Module。
mod sequence_templates;
// 注册 sequence Workflow 私有的跨步骤前置条件契约与判定。
mod sequence_preconditions;
// 注册 sequence Workflow 私有的本步骤后置条件契约与判定。
mod sequence_postconditions;
// 保持公开 sequence 输入类型与边界常量位于 service 路径。
pub use sequence::*;
// 公开严格 sequence 输入需要反序列化的绑定类型。
pub use sequence_bindings::{SequenceBinding, SequenceBindingDestination};
// 公开严格 sequence 输入需要反序列化的模板段和硬边界。
pub use sequence_templates::*;
// 公开严格 sequence 输入需要反序列化的前置条件类型。
pub use sequence_preconditions::SequencePrecondition;
// 公开严格 sequence 输入需要反序列化的后置条件类型。
pub use sequence_postconditions::SequencePostcondition;
// 注册仅供固定浏览器 broker 调用的 System 到 Module 协调边界。
#[path = "service/browser_session_broker.rs"]
pub(crate) mod browser_session_broker;

pub struct AppControlService {
    registry: AdapterRegistry,
    // 持有 Permission Assessment 使用的只读平台端口。
    security_context: Arc<dyn SecurityContextProbe>,
    // 唯一持有浏览器会话 registry、worker、Job 与 stdio 生命周期。
    // #2059 分派边界只经 System 的 crate-private 协调方法读取该字段。
    // 保存 System 私有浏览器会话 Module。
    browser_sessions: BrowserSessionModule,
}

impl Default for AppControlService {
    fn default() -> Self {
        Self::new()
    }
}

impl AppControlService {
    pub fn new() -> Self {
        Self {
            registry: AdapterRegistry::adaptive(),
            // 生产 System 固定装配 Rust Windows 只读安全姿态 Adapter。
            security_context: Arc::new(WindowsSecurityContextProbe),
            // 只允许 System 在构造时创建浏览器会话 Module。
            browser_sessions: BrowserSessionModule::new(),
        }
    }

    // 仅供同模块回归测试替换平台事实，不形成公开注入入口。
    #[cfg(test)]
    fn with_security_context(security_context: Arc<dyn SecurityContextProbe>) -> Self {
        // 生产 provider 集保持不变，只替换只读探针端口。
        Self {
            // 使用完整生产 provider registry。
            registry: AdapterRegistry::adaptive(),
            // 保存合成测试探针。
            security_context,
            // 测试 System 仍使用真实的空浏览器会话 Module 所有权边界。
            browser_sessions: BrowserSessionModule::new(),
        }
    }

    // 在任何 provider 或精确目标解析前执行安全姿态授权。
    fn authorize_target_access(&self, access_kind: TargetAccessKind) -> AppResult<()> {
        // 委托 Permission Boundary Module 拥有授权结论。
        permission_boundary::authorize(self.security_context.as_ref(), access_kind)
    }

    // 协调只读构建元数据 Module。
    pub fn build_info(&self) -> Value {
        // Module 拥有字段语义，System 仅转发。
        build_info::render()
    }

    // 协调只读 capability surface 迁移元数据 Module。
    pub fn capability_surface(&self) -> Value {
        // Module 拥有迁移字段语义，System 仅转发。
        capability_metadata::surface()
    }

    // 协调全部或单个 method 的迁移元数据 Module。
    pub fn method_capabilities(&self, method_id: Option<&str>) -> AppResult<Value> {
        // Module 从旧 method 目录投影独立 C++ 状态。
        capability_metadata::method(method_id)
    }

    // 协调 app 或 operation descriptor 的迁移元数据 Module。
    pub fn descriptor_capabilities(
        // 只借用无状态协调 service。
        &self,
        // 接收旧公开 app ID。
        app_id: &str,
        // 接收可选 operation ID。
        operation_id: Option<&str>,
    ) -> AppResult<Value> {
        // Module 负责精确目录解析与状态注入。
        capability_metadata::descriptor(app_id, operation_id)
    }

    pub fn catalog(&self, app_id: Option<&str>) -> AppResult<Value> {
        let entries = match app_id {
            Some(id) => vec![catalog::app(id).ok_or_else(|| {
                ComputerControlSystemErrorCode::InvalidArgument.error(format!("未知应用 '{}'.", id))
            })?],
            None => catalog::apps().iter().collect(),
        };
        Ok(json!({ "ok": true, "policy": "background-preferred", "apps": entries }))
    }

    pub fn describe(&self, app_id: &str, operation_id: Option<&str>) -> AppResult<Value> {
        let app = catalog::app(app_id).ok_or_else(|| {
            ComputerControlSystemErrorCode::InvalidArgument.error(format!("未知应用 '{}'.", app_id))
        })?;
        let descriptor = match operation_id {
            Some(id) => serde_json::to_value(catalog::operation(app_id, id).ok_or_else(|| {
                ComputerControlSystemErrorCode::InvalidArgument
                    .error(format!("未知认证操作 '{}.{}'.", app_id, id))
            })?),
            None => serde_json::to_value(app),
        }
        .map_err(|error| {
            ComputerControlSystemErrorCode::SerializationFailed.error(error.to_string())
        })?;
        Ok(json!({ "ok": true, "descriptor": descriptor }))
    }

    pub fn methods(&self, method_id: Option<&str>) -> AppResult<Value> {
        let entries = match method_id {
            Some(id) => vec![methods::find(id).ok_or_else(|| {
                ComputerControlSystemErrorCode::InvalidArgument
                    .error(format!("未知操作方式 '{}'.", id))
            })?],
            None => methods::all().iter().collect(),
        };
        Ok(json!({ "ok": true, "methods": entries }))
    }

    pub fn doctor(&self, app_id: Option<&str>) -> Value {
        let ids = app_id.map_or_else(
            || catalog::apps().iter().map(|app| app.id).collect::<Vec<_>>(),
            |id| vec![id],
        );
        let results = ids
            .into_iter()
            .map(
                |id| match self.execute(CommandRequest::read(Verb::Status, id)) {
                    Ok(result) => result,
                    Err(error) => error_json(&error),
                },
            )
            .collect::<Vec<_>>();
        json!({
            "ok": results.iter().all(|result| result.get("ok") == Some(&Value::Bool(true))),
            "policy": "background-preferred",
            "results": results,
        })
    }

    // 协调 Rust application.discover@1 Module，不承载领域枚举行为。
    pub fn discover_app(
        // 只借用无状态协调 service。
        &self,
        // 接收应用上限。
        maximum_applications: usize,
        // 接收进程上限。
        maximum_processes: usize,
        // 接收窗口上限。
        maximum_windows: usize,
    ) -> AppResult<Value> {
        // 委托独立 Module 完成捕获、门禁与投影。
        application_discovery::discover(
            // 转发应用边界。
            maximum_applications,
            // 转发进程边界。
            maximum_processes,
            // 转发窗口边界。
            maximum_windows,
        )
    }

    // 协调只返回双向认证 endpoint 的独立交互会话发现。
    pub fn discover_interactive_sessions(&self) -> AppResult<Value> {
        // 在 WTS 枚举和固定 pipe 握手前执行 host 只读安全门禁。
        self.authorize_target_access(TargetAccessKind::Read)?;
        // Module 拥有 endpoint 认证与 provider-neutral 投影。
        interactive_isolation::discover()
    }

    // 协调无副作用长操作 status Query。
    pub fn long_operation_status(&self, operation_id: &str, timeout_ms: u32) -> AppResult<Value> {
        // Module 拥有 handle 验证、broker 协议与响应语义。
        long_operation_client::status(operation_id, timeout_ms)
    }

    // 协调有界长操作 await Query，不改变任务状态或保留期。
    pub fn long_operation_await(&self, operation_id: &str, timeout_ms: u32) -> AppResult<Value> {
        // Module 拥有总 deadline、只读轮询和终态判定。
        long_operation_client::await_terminal(operation_id, timeout_ms)
    }

    // 协调幂等长操作 cancel Command。
    pub fn long_operation_cancel(&self, operation_id: &str, timeout_ms: u32) -> AppResult<Value> {
        // Module 拥有取消重发与业务接受边界。
        long_operation_client::cancel(operation_id, timeout_ms)
    }

    // 协调异步窗口录制 submit Command。
    pub fn long_operation_start(
        // 只借用无状态 System。
        &self,
        // 接收固定版本化 capability。
        capability_id: &str,
        // 接收 canonical opaque 窗口目标。
        session_id: &str,
        // 接收完整 provider-neutral 输入。
        input: &Value,
        // 接收逐操作显式确认。
        confirmed: bool,
        // 接收总 transport 预算。
        timeout_ms: u32,
    ) -> AppResult<Value> {
        // Module 拥有字段门禁、非幂等投递与 acceptance-aware 响应。
        long_operation_client::start(capability_id, session_id, input, confirmed, timeout_ms)
    }

    // 在 broker 建立业务接受事实前验证异步窗口录制请求。
    pub(crate) fn validate_long_operation_window_record(
        // 只借用无状态 System。
        &self,
        // 接收 canonical opaque 窗口目标。
        session_id: &str,
        // 接收 provider-neutral 录制输入。
        input: &Value,
    ) -> AppResult<()> {
        // 构造与同步生产入口相同的通用 app Command。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 选择 catalog 中的录制 operation。
        request.operation = Some("record".to_owned());
        // 写入唯一公开精确目标。
        request
            // 访问目标对象。
            .target
            // 保存 canonical sessionId。
            .insert("sessionId".to_owned(), json!(session_id));
        // 写入固定版本化 capability。
        request.args.insert(
            // 使用通用 facade 的 capability 字段。
            "capability".to_owned(),
            // 固定首个异步 capability。
            json!(capabilities::WINDOW_RECORD),
        );
        // 写入完整领域输入对象。
        request
            // 访问参数对象。
            .args
            // 保持输入为 provider-neutral JSON。
            .insert("input".to_owned(), input.clone());
        // broker 协议已经完成 confirmation-first 验证。
        request.confirmed = true;
        // 在路径、目标发现和 worker 前执行同一 Policy 门禁。
        policy::validate(&request)?;
        // 在目标重新发现和文件操作前执行 mutation 安全姿态授权。
        self.authorize_target_access(TargetAccessKind::Mutation)?;
        // 最后执行仍不产生 I/O 的领域输入与 target 外壳验证。
        window_record::validate_submission(session_id, request.confirmed, input)
    }

    // 协调 Rust accessibility.tree.read@1 Module，不在 service 触碰 UIA。
    pub fn inspect_accessibility_tree(
        // 只借用无状态 service。
        &self,
        // 接收 canonical s2:w 目标。
        session_id: &str,
        // 接收深度硬边界。
        maximum_depth: usize,
        // 接收节点数量硬边界。
        maximum_items: usize,
        // 接收 control/raw view。
        view: &str,
        // 接收 worker deadline。
        timeout_ms: u32,
        // 接收不可由 worker 放宽的隔离要求。
        isolation_requirement: IsolationRequirement,
    ) -> AppResult<Value> {
        // 构造进入同一 Policy Module 的 UIA 读取请求。
        let mut request = CommandRequest::read(Verb::Inspect, "uia");
        // 冻结调用方隔离要求。
        request.isolation_requirement = isolation_requirement;
        // 在 worker 解析 target 前执行严格隔离门禁。
        policy::validate(&request)?;
        // 在 worker 路径和精确窗口解析前认证当前交互安全姿态。
        self.authorize_target_access(TargetAccessKind::Read)?;
        // 冻结只读隔离 worker 计划。
        let plan = policy::execution_plan(&request)?.ok_or_else(|| {
            // 理论上的计划缺失必须失败闭合。
            ComputerControlSystemErrorCode::OperationFailed.error(
                // 不公开 worker 或安装路径。
                "The accessibility request has no frozen execution plan.",
            )
        })?;
        // 委托可访问性 Module 完成重新发现、隔离与投影。
        let mut result = accessibility::inspect_tree(
            // 转发 opaque 目标。
            session_id,
            // 转发深度边界。
            maximum_depth,
            // 转发数量边界。
            maximum_items,
            // 转发 view。
            view,
            // 转发 deadline。
            timeout_ms,
        )?;
        // 由 System 附加并核验强类型执行证明。
        plan.attest_result(&mut result)?;
        // 返回经过策略证明的树结果。
        Ok(result)
    }

    // 协调无副作用的 Rust capability assessment Module。
    pub fn assess_capability(
        // 借用无状态 service。
        &self,
        // 接收版本化 capability ID。
        capability: &str,
        // 接收 canonical opaque session。
        session_id: &str,
    ) -> AppResult<Value> {
        // 在任何 session provider 重新发现前认证当前交互安全姿态。
        self.authorize_target_access(TargetAccessKind::Read)?;
        // Module 负责重新发现、唯一解析和封闭决策。
        capability_assessment::assess(capability, session_id)
    }

    // 协调 Rust window.capture.preflight@1 Module，不在 System 内触碰 Win32 或 WGC。
    pub fn preflight_capture(&self, session_id: &str) -> AppResult<Value> {
        // 在读取精确窗口、DWM 或 WGC 元数据前认证安全姿态。
        self.authorize_target_access(TargetAccessKind::Read)?;
        // 委托领域 Module 完成重新发现、只读探测和前景门禁。
        window_capture_preflight::preflight(session_id)
    }

    // 协调 Rust window.capture.frame.probe@1 Module，不在 System 内触碰 Win32 或 WGC。
    pub fn probe_capture_frame(
        // 借用无状态 service。
        &self,
        // 接收 canonical opaque 窗口目标。
        session_id: &str,
        // 接收逐操作确认。
        confirmed: bool,
        // 接收隔离 worker deadline。
        timeout_ms: u32,
    ) -> AppResult<Value> {
        // 在创建隔离 worker 或读取精确窗口前认证安全姿态。
        self.authorize_target_access(TargetAccessKind::Read)?;
        // 委托领域 Module 完成确认、重新发现、预检、隔离执行与前景门禁。
        window_capture_frame_probe::probe(session_id, confirmed, timeout_ms)
    }

    // 执行普通统一请求且不暴露内部 dispatch 观察点。
    pub fn execute(&self, request: CommandRequest) -> AppResult<Value> {
        // 普通调用使用永远成功的空观察器，保持既有行为。
        self.execute_with_dispatch_hook(request, || Ok(()))
    }

    // 在全部 System 门禁后且领域调用前触发一次 crate 私有观察器。
    pub(crate) fn execute_with_dispatch_hook<F>(
        // 借用唯一 System 协调器。
        &self,
        // 接收统一 provider-neutral 请求。
        request: CommandRequest,
        // 接收不得绕过门禁且可以失败闭合的单次观察器。
        dispatch_hook: F,
    ) -> AppResult<Value>
    where
        // 观察器只能调用一次并返回统一错误。
        F: FnOnce() -> AppResult<()>,
    {
        // Policy Module 在 provider 解析前完成目录、确认与严格隔离门禁。
        policy::validate(&request).map_err(|error| {
            // Browser Session Policy 拒绝必须显式证明尚未业务派发。
            browser_session_lifecycle_error::project_before_dispatch(error, &request)
        })?;
        // 精确目标读取和 mutation 必须先通过受保护上下文门禁。
        if let Some(access_kind) = target_access_kind(&request) {
            // 使用 registry 副作用事实执行一次安全授权。
            self.authorize_target_access(access_kind).map_err(|error| {
                // 权限拒绝发生在 provider 与固定 broker 之前。
                browser_session_lifecycle_error::project_before_dispatch(error, &request)
            })?;
        }
        // 冻结运行或读取请求的强类型执行计划。
        let execution_plan = policy::execution_plan(&request).map_err(|error| {
            // 计划冻结失败同样不允许冒充空 details。
            browser_session_lifecycle_error::project_before_dispatch(error, &request)
        })?;
        // 独立会话 route 不得解析或调用 host 本地 app provider。
        if policy::uses_interactive_session(&request) {
            // 所有已验证运行请求都必须具有冻结计划。
            let plan = execution_plan.ok_or_else(|| {
                // 理论上的计划漂移失败闭合。
                ComputerControlSystemErrorCode::OperationFailed.error(
                    // 不公开 endpoint 或安装路径。
                    "The independent interactive session request has no frozen execution plan.",
                )
            })?;
            // 只有全部门禁和计划冻结后才允许公布 dispatch accepted。
            dispatch_hook()?;
            // Module 通过新握手、新 lease 与固定 worker 执行唯一 command。
            let mut result = interactive_isolation::execute(&request)?;
            // 由 System 核验并附加隔离 worker 证明。
            plan.attest_result(&mut result)?;
            // 返回经过 System 证明且无 host provider fallback 的结果。
            return Ok(result);
        }
        // 策略完成后才允许解析 provider。
        let adapter = self.registry.get(&request.app).map_err(|error| {
            // registry 未解析时尚未调用任何 provider。
            browser_session_lifecycle_error::project_before_dispatch(error, &request)
        })?;
        // provider 已唯一解析后且调用任何领域方法前发布单次观察。
        dispatch_hook().map_err(|error| {
            // hook 位于 provider 调用前，失败时保持未派发事实。
            browser_session_lifecycle_error::project_before_dispatch(error, &request)
        })?;
        // 执行已通过策略的 adapter 请求。
        let mut result = match request.verb {
            // 执行只读状态查询。
            Verb::Status => adapter.status()?,
            // 执行只读 session 发现。
            Verb::Sessions => adapter.sessions(&request)?,
            // 执行只读精确检查。
            Verb::Inspect => adapter.inspect(&request)?,
            // 执行已通过确认和隔离策略的操作。
            Verb::Run => adapter.run(&request)?,
        };
        // 所有 service 请求都必须具有冻结计划。
        let plan = execution_plan
            // provider 已返回后仍必须取得原先冻结的执行计划。
            .ok_or_else(|| {
                // 理论上的 System 状态漂移必须 fail closed。
                ComputerControlSystemErrorCode::OperationFailed.error(
                    // 不公开 provider 私有信息。
                    "The service request has no frozen execution plan.",
                )
            })
            // Browser Session provider 已执行时必须保留 accepted 失败真值。
            .map_err(|error| {
                // 其他 capability 由 Component 原样返回。
                browser_session_lifecycle_error::project_after_provider(error, &request)
            })?;
        // 核验 provider 自报 realm 并附加 System 策略证据。
        plan.attest_result(&mut result).map_err(|error| {
            // 证明失败发生在 provider 返回之后，禁止伪装成未派发。
            browser_session_lifecycle_error::project_after_provider(error, &request)
        })?;
        // 返回经过 System 证明的结果。
        Ok(result)
    }
}

// 编译 ComputerControlSystem dispatch 观察顺序的独立回归。
#[cfg(test)]
mod dispatch_hook_tests;

// 静态验证 System 到 Browser Session Module/Process 的 RAII 所有权链。
#[cfg(test)]
mod browser_session_ownership_tests {
    // 导入无需启动进程的析构需求检查。
    use std::mem::needs_drop;

    // 导入 RAII 链末端的私有浏览器进程 Component。
    use crate::components::browser_session_process::BrowserSessionProcess;
    // 导入 System 唯一持有的浏览器会话 Module。
    use crate::modules::browser_session::BrowserSessionModule;

    // 导入被测 System。
    use super::AppControlService;

    // 证明 System 直接拥有 Module，且 Module/Process 都沿 Rust Drop 链释放。
    #[test]
    fn system_owns_browser_session_raii_chain() {
        // Process 自定义 Drop 必须保持有效，负责强制回收 worker/Job/stdio。
        assert!(needs_drop::<BrowserSessionProcess>());
        // Module 的 session registry 必须保持析构语义以向下释放 Process。
        assert!(needs_drop::<BrowserSessionModule>());
        // 构造不启动真实浏览器的空 System。
        let system = AppControlService::new();
        // 直接字段类型证明 broker 无需且不应取得 Module accessor。
        let _: &BrowserSessionModule = &system.browser_sessions;
        // 销毁 System 即沿字段所有权释放空 Module；live entry 会继续触发 Process Drop。
        drop(system);
    }
}

// 声明 ComputerControlSystem 安全门禁调用顺序的纯合成回归。
#[cfg(test)]
mod security_boundary_tests {
    // 导入共享所有权与原子计数器。
    use std::sync::{
        // 保存探针及其可观察调用计数。
        Arc,
        // 使用顺序无关计数验证调用次数。
        atomic::{AtomicUsize, Ordering},
    };

    // 导入 JSON 构造器。
    use serde_json::json;

    // 导入 System 请求、隔离与错误结果类型。
    use crate::{
        // 导入请求和公开动词。
        domain::{CommandRequest, IsolationRequirement, Verb},
        // 导入合成平台端口所需封闭事实。
        modules::permission_boundary::{
            DesktopSecurityState, HostSecurityFacts, SecurityContextProbe, SessionSecurityState,
        },
    };

    // 导入被测 ComputerControlSystem。
    use super::{AppControlService, target_access_kind};

    // 提供记录调用次数的固定安全姿态探针。
    struct CountingProbe {
        // 保存每次调用返回的封闭事实。
        facts: HostSecurityFacts,
        // 保存调用次数。
        calls: AtomicUsize,
    }

    // 为合成探针实现 Module 端口。
    impl SecurityContextProbe for CountingProbe {
        // 返回固定事实并记录调用。
        fn probe(&self) -> HostSecurityFacts {
            // 递增无锁测试计数。
            self.calls.fetch_add(1, Ordering::Relaxed);
            // 返回复制的封闭事实。
            self.facts
        }
    }

    // 构造位于受保护或非输入桌面的测试 System。
    fn protected_service() -> (AppControlService, Arc<CountingProbe>) {
        // 构造活动会话中的非输入桌面事实。
        let probe = Arc::new(CountingProbe {
            // 保存受保护上下文。
            facts: HostSecurityFacts {
                // 会话本身保持活动以隔离桌面原因。
                session: SessionSecurityState::ActiveInteractive,
                // 桌面明确不允许目标访问。
                desktop: DesktopSecurityState::ProtectedOrNonInput,
            },
            // 初始化零调用计数。
            calls: AtomicUsize::new(0),
        });
        // 使用完整生产 registry 与合成探针构造 System。
        let service = AppControlService::with_security_context(probe.clone());
        // 返回 System 与计数观察点。
        (service, probe)
    }

    // 构造通过 Policy 的通用进程终止请求。
    fn confirmed_mutation_request() -> CommandRequest {
        // 从统一 app.run 请求开始。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 使用两个进程终止 capability 共用的 generic close。
        request.operation = Some("close".to_owned());
        // 提供不会命中真实进程的 canonical opaque 目标。
        request
            // 访问目标对象。
            .target
            // 写入固定 sessionId。
            .insert("sessionId".to_owned(), json!("s2:p:0000000000000000"));
        // 提供注册表中的强制终止 capability。
        request.args.insert(
            // 使用稳定 capability 字段。
            "capability".to_owned(),
            // 选择固定版本化 ID。
            json!("process.terminate.force@1"),
        );
        // 提供严格但为空的领域 input。
        request
            // 访问参数对象。
            .args
            // 写入 input。
            .insert("input".to_owned(), json!({}));
        // 满足逐操作确认，使安全门禁成为下一条决策。
        request.confirmed = true;
        // 返回尚未触碰 provider 的请求。
        request
    }

    // 构造通过确认门禁的 Browser Session close 请求。
    fn confirmed_browser_session_close_request() -> CommandRequest {
        // 从统一 app.run 请求开始。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // close capability 固定使用 generic close。
        request.operation = Some("close".to_owned());
        // 写入 caller 已知 canonical Browser Session target。
        request.target.insert(
            // 使用统一目标字段。
            "sessionId".to_owned(),
            // 使用不触碰真实 broker 的固定测试身份。
            json!("s2:bs:0123456789abcdef0123456789abcdef"),
        );
        // 写入固定生命周期 capability。
        request.args.insert(
            // 使用统一 capability 字段。
            "capability".to_owned(),
            // 选择已登记 close ID。
            json!(crate::capabilities::BROWSER_SESSION_CLOSE),
        );
        // 越过确认门禁，使安全姿态成为下一条决策。
        request.confirmed = true;
        // 返回尚未触碰 provider 的请求。
        request
    }

    // 验证 Browser Session 在不可认证安全姿态下保留完整未派发真值。
    #[test]
    fn browser_session_indeterminate_security_context_is_publicly_not_dispatched() {
        // 构造无法认证当前 Windows session 的只读探针。
        let probe = Arc::new(CountingProbe {
            // 保存不可确定会话事实。
            facts: HostSecurityFacts {
                // 阻止 System 猜测当前 session 是否安全。
                session: SessionSecurityState::Indeterminate,
                // 桌面事实保持允许以隔离会话原因。
                desktop: DesktopSecurityState::ActiveDefaultInput,
            },
            // 初始化零调用计数。
            calls: AtomicUsize::new(0),
        });
        // 使用完整生产 registry 与合成探针构造 System。
        let service = AppControlService::with_security_context(probe.clone());
        // 执行已确认生命周期请求。
        let error = service
            // 调用唯一生产 System 编排。
            .execute(confirmed_browser_session_close_request())
            // 不可认证姿态必须失败闭合。
            .expect_err("indeterminate security context must fail");
        // 保留全局登记的安全姿态错误码。
        assert_eq!(error.code, "CAPABILITY_ASSESSMENT_UNAVAILABLE");
        // 明确证明固定 broker 尚未业务接受。
        assert_eq!(error.details["accepted"], false);
        // 不可认证失败本身是可信终态。
        assert_eq!(error.details["finalStateReached"], true);
        // 原安全探针详情不得穿过 lifecycle envelope。
        assert!(error.details.get("reason").is_none());
        // 每个请求只能读取一次安全姿态。
        assert_eq!(probe.calls.load(Ordering::Relaxed), 1);
    }

    // 验证通用 execute 在 provider 和目标解析前阻塞读取与 mutation。
    #[test]
    fn execute_security_gate_precedes_all_exact_provider_access() {
        // 构造受保护上下文 System。
        let (service, probe) = protected_service();
        // 构造缺少目标的窗口检查；若 provider 被调用会返回参数错误。
        let read_request = CommandRequest::read(Verb::Inspect, "window");
        // 执行精确目标读取请求。
        let read_error = service
            // 调用唯一生产 execute 编排。
            .execute(read_request)
            // 取得预期错误。
            .err()
            // 意外成功时明确测试失败。
            .unwrap_or_else(|| panic!("protected target read must fail"));
        // 安全门禁必须早于 provider 参数解析。
        assert_eq!(read_error.code, "PERMISSION_DENIED");
        // 错误必须保留读取分类。
        assert_eq!(read_error.details["accessKind"], "target-read");
        // 第一次请求只探测一次。
        assert_eq!(probe.calls.load(Ordering::Relaxed), 1);

        // 执行已确认且 Policy 合法的 mutation 请求。
        let mutation_error = service
            // 调用同一生产 execute 编排。
            .execute(confirmed_mutation_request())
            // 取得预期错误。
            .err()
            // 意外成功时明确测试失败。
            .unwrap_or_else(|| panic!("protected target mutation must fail"));
        // 安全门禁必须早于 stale 进程解析。
        assert_eq!(mutation_error.code, "PERMISSION_DENIED");
        // 错误必须保留 mutation 分类。
        assert_eq!(mutation_error.details["accessKind"], "target-mutation");
        // 两个请求总共执行两次独立快照。
        assert_eq!(probe.calls.load(Ordering::Relaxed), 2);
    }

    // 验证绕开通用 execute 的四个精确只读入口同样先执行门禁。
    #[test]
    fn specialized_target_reads_share_the_same_predispatch_gate() {
        // 构造受保护上下文 System。
        let (service, probe) = protected_service();
        // 保存每条入口的错误码供统一核对。
        let errors = [
            // capability assessment 会重新解析精确 session。
            service
                // 使用固定无效 opaque ID，若下层运行会产生其他错误。
                .assess_capability("window.close@1", "s2:w:0000000000000000")
                // 只保留错误。
                .err(),
            // capture preflight 会读取精确窗口和 WGC 元数据。
            service
                // 使用固定无效窗口目标。
                .preflight_capture("s2:w:0000000000000000")
                // 只保留错误。
                .err(),
            // frame probe 会创建隔离 worker 并读取精确窗口。
            service
                // 满足确认以隔离安全门禁。
                .probe_capture_frame("s2:w:0000000000000000", true, 1_000)
                // 只保留错误。
                .err(),
            // UIA tree 会创建 observation worker 并读取精确窗口。
            service
                // 使用最小合法读取边界。
                .inspect_accessibility_tree(
                    // 提供固定窗口目标。
                    "s2:w:0000000000000000",
                    // 使用最小深度。
                    0,
                    // 使用最小数量。
                    1,
                    // 使用合法 control view。
                    "control",
                    // 使用有界 deadline。
                    1_000,
                    // 使用普通隔离策略，避免严格门禁提前失败。
                    IsolationRequirement::Standard,
                )
                // 只保留错误。
                .err(),
        ];
        // 逐入口核对统一前置错误。
        for error in errors {
            // 每个入口都必须失败。
            let error = error.unwrap_or_else(|| panic!("protected target read must fail"));
            // 所有入口共享权限拒绝码。
            assert_eq!(error.code, "PERMISSION_DENIED");
            // 所有入口共享只读分类。
            assert_eq!(error.details["accessKind"], "target-read");
            // 所有入口证明未尝试目标读取。
            assert_eq!(error.details["targetReadAttempted"], false);
        }
        // 四个入口各自只捕获一次平台快照。
        assert_eq!(probe.calls.load(Ordering::Relaxed), 4);
    }

    // 验证 app.run 中的只读等待不会被错误标记为 mutation。
    #[test]
    fn run_access_kind_comes_from_the_app_capability_registry() {
        // 构造通用 app.run 请求。
        let mut read_request = CommandRequest::read(Verb::Run, "app");
        // 使用只读等待对应的 generic verb。
        read_request.operation = Some("read".to_owned());
        // 写入 registry 中明确无副作用的 capability。
        read_request.args.insert(
            // 使用稳定 capability 字段。
            "capability".to_owned(),
            // 使用精确窗口关闭等待。
            json!("window.closed.wait@1"),
        );
        // registry 只读 action 必须映射为目标读取。
        assert_eq!(
            target_access_kind(&read_request),
            Some(crate::modules::permission_boundary::TargetAccessKind::Read)
        );

        // 构造已确认进程终止 mutation。
        let mutation_request = confirmed_mutation_request();
        // registry 关闭 action 必须映射为目标 mutation。
        assert_eq!(
            target_access_kind(&mutation_request),
            Some(crate::modules::permission_boundary::TargetAccessKind::Mutation)
        );

        // legacy run 未提供版本化 capability 时必须保守视为 mutation。
        let legacy_request = CommandRequest::read(Verb::Run, "desktop");
        // 未知路线不得获得只读放宽。
        assert_eq!(
            target_access_kind(&legacy_request),
            Some(crate::modules::permission_boundary::TargetAccessKind::Mutation)
        );

        // 构造独立会话 mutation 路由。
        let mut isolated_request = mutation_request;
        // 添加公开 endpoint 选择字段。
        isolated_request.target.insert(
            // 使用固定字段名。
            "interactiveSessionId".to_owned(),
            // 值类别由 Policy 在授权前验证。
            json!("s2:i:0000000000000001"),
        );
        // host 侧只允许执行 endpoint 发现读取。
        assert_eq!(
            // 读取 System 安全分类。
            target_access_kind(&isolated_request),
            // 真正 mutation 会在目标 worker 内再次授权。
            Some(crate::modules::permission_boundary::TargetAccessKind::Read)
        );
    }
}
