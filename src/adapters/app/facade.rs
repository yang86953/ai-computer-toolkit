// 把错误码实现保留为当前 facade Adapter 的普通私有类型。
#[path = "facade_error.rs"]
mod error_code;
// 导入 Browser Session 固定 capability 的 provider 与公开目标规则。
use super::facade_browser_session::{selects_direct_browser_session_provider, targeting_label};
// 导入 Browser Session provider 前后错误阶段投影。
use super::facade_browser_session_error::{after_provider, before_provider};
// 导入 canonical stale 目标的独占 provider 选择规则。
use super::facade_stale_session::selects_provider_owned_stale_target;
use serde_json::{Value, json};
use std::sync::Arc;

// 导入当前 facade Adapter 私有封闭错误码。
use error_code::AppFacadeErrorCode;

use crate::{
    adapters::{
        AppAdapter,
        app::{
            CapabilityProvider,
            browser_session::BrowserSessionProvider,
            desktop::DesktopProvider,
            host::HostProvider,
            process::ProcessProvider,
            // 导入一次性强类型调用解析与精确 session 校验。
            session::parse_capability_invocation,
            session::required_session_id,
            standard_edit::StandardEditProvider,
            text::TextDocumentProvider,
        },
        photoshop::PhotoshopProvider,
        window::foreground_snapshot,
        windows::foreground_hwnd,
    },
    // 导入 capability 执行域以区分后台不变量与显式前台操作。
    capabilities,
    components::opaque_id::OpaqueTargetId,
    // 导入稳定错误、请求与强类型执行域。
    domain::{AppResult, CommandRequest, ExecutionRealm},
};

pub struct AppFacadeAdapter {
    providers: Vec<Arc<dyn CapabilityProvider>>,
}

struct ResolvedSession<'a> {
    provider: &'a Arc<dyn CapabilityProvider>,
    capabilities: Vec<String>,
}

// 保存 capability assessment 从 app provider 重新解析出的最小事实。
pub(crate) struct AssessmentSession {
    // 保存精确 session 当前发布的 capability 集合。
    pub(crate) capabilities: Vec<String>,
}

// 要求一个公开 session 在全部 provider 中唯一重新解析。
fn require_unique_provider_match<Resolved>(
    // 接收各 provider 独立重新解析后的命中集合。
    mut matches: Vec<Resolved>,
    // 只用于结构化错误证据的公开 session ID。
    session_id: &str,
) -> AppResult<Resolved> {
    // 以命中数量明确区分唯一、过期和跨 provider 歧义。
    match matches.len() {
        // 唯一 provider 命中时返回该解析结果。
        1 => matches.pop().ok_or_else(|| {
            // 防御性保留不可达的空集合错误。
            AppFacadeErrorCode::TargetNotFound.error("The app session is unavailable.")
            // 结束防御性错误构造。
        }),
        // 零 provider 命中表示目标已过期或不属于当前 inventory。
        0 => Err(AppFacadeErrorCode::TargetNotFound.with_details(
            // 解释使用时重新发现未命中。
            "The app session is stale or no longer available.",
            // 仅返回调用方已知的不透明 ID。
            json!({ "sessionId": session_id }),
        )),
        // 两个或更多 provider 命中可能源于指纹碰撞或 provider 重叠。
        _ => Err(AppFacadeErrorCode::AmbiguousTarget.with_details(
            // 明确不会任取一个 provider。
            "More than one internal provider resolved the same opaque session.",
            // 仅返回调用方已知的不透明 ID。
            json!({ "sessionId": session_id }),
        )),
        // 结束跨 provider 命中分类。
    }
    // 结束全 provider 唯一解析门禁。
}

impl AppFacadeAdapter {
    pub fn new() -> Self {
        Self {
            providers: vec![
                Arc::new(HostProvider),
                Arc::new(BrowserSessionProvider),
                Arc::new(ProcessProvider),
                Arc::new(TextDocumentProvider),
                Arc::new(StandardEditProvider),
                Arc::new(DesktopProvider),
                Arc::new(PhotoshopProvider),
            ],
        }
    }

    fn resolve_session(&self, request: &CommandRequest) -> AppResult<ResolvedSession<'_>> {
        let session_id = required_session_id(request)?;
        let mut matches = Vec::new();
        for provider in &self.providers {
            if let Some(capabilities) = provider.session_capabilities(session_id)? {
                matches.push(ResolvedSession {
                    provider,
                    capabilities,
                });
            }
        }
        // 将跨 provider 碰撞处理收敛到独立可测试门禁。
        require_unique_provider_match(matches, session_id)
    }

    // 为拥有专属过期语义的 Module 保留权威 stale 或缺失结果。
    fn resolve_execution_session(
        // 借用 facade provider 集合。
        &self,
        // 接收完整执行请求。
        request: &CommandRequest,
        // 接收已经过 verb 门禁的 capability。
        capability: &str,
    ) -> AppResult<ResolvedSession<'_>> {
        // Browser Session operation 必须直接选择 provider，不能另发 freshness Query 重置总 deadline。
        if required_session_id(request).is_ok_and(|session_id| {
            // 只按 capability 与 canonical identity 执行纯路由判定。
            selects_direct_browser_session_provider(capability, session_id)
        }) {
            // 只收集声明该固定 capability 的 provider，不访问任何 session。
            let matches = self
                // 遍历 facade 组装期固定 provider 集。
                .providers
                // 取得迭代器。
                .iter()
                // 只保留唯一 Browser Session provider。
                .filter(|provider| provider.capabilities().contains(&capability))
                // 构造不触发 freshness 的最小解析结果。
                .map(|provider| ResolvedSession {
                    // 保存唯一已选 provider。
                    provider,
                    // 只发布调用方请求的固定 capability。
                    capabilities: vec![capability.to_owned()],
                })
                // 收集给既有唯一性门禁。
                .collect::<Vec<_>>();
            // 配置重复同样 fail closed。
            return require_unique_provider_match(matches, required_session_id(request)?);
        }
        // 优先使用当前实时 session 的正常唯一解析。
        match self.resolve_session(request) {
            // 实时命中直接返回。
            Ok(resolved) => Ok(resolved),
            // canonical stale 写目标由独占 provider 交给领域 Module 分类。
            Err(error)
                if AppFacadeErrorCode::TargetNotFound.matches(&error)
                    // capability 与 opaque 类别必须是已审计的封闭组合。
                    && required_session_id(request).is_ok_and(|session_id| {
                        // 委托窄 Component 核对 capability 与 opaque 类别固定配对。
                        selects_provider_owned_stale_target(capability, session_id)
                    }) =>
            {
                // 收集唯一发布该 capability 的 provider。
                let matches = self
                    // 遍历内部 provider 集合。
                    .providers
                    // 取得迭代器。
                    .iter()
                    // 只保留声明该专属 capability 的 provider。
                    .filter(|provider| provider.capabilities().contains(&capability))
                    // 构造最小执行解析结果。
                    .map(|provider| ResolvedSession {
                        // 保存独占 provider。
                        provider,
                        // 只赋予调用方请求的固定 capability。
                        capabilities: vec![capability.to_owned()],
                    })
                    // 收集供唯一门禁检查。
                    .collect::<Vec<_>>();
                // 即使配置错误出现多 provider 也必须 fail closed。
                require_unique_provider_match(matches, required_session_id(request)?)
            }
            // 其他未命中、歧义或 provider 错误保持原样。
            Err(error) => Err(error),
        }
    }

    // 重新查询全部 app provider，并为 assess 返回唯一 session 的最小事实。
    pub(crate) fn resolve_assessment_session(
        // 借用无状态 facade。
        &self,
        // 接收调用方提供的 canonical session。
        session_id: &str,
    ) -> AppResult<Option<AssessmentSession>> {
        // 非 canonical 目标不属于任何 app provider。
        if OpaqueTargetId::parse(session_id).is_none()
            // browser session 使用独立三十二位身份，不进入通用 opaque parser。
            && crate::components::browser_session_identity::classify_browser_session_id(session_id)
                != crate::components::browser_session_identity::BrowserSessionIdentityShape::Canonical
        {
            // 由上层统一返回 stale 语义。
            return Ok(None);
        }
        // 保存所有 provider 的独立重新发现命中。
        let mut matches = Vec::new();
        // 逐 provider 执行当前 session 解析。
        for provider in &self.providers {
            // 仅保留明确接受该 session 的 provider。
            if let Some(capabilities) = provider.session_capabilities(session_id)? {
                // 保存 provider-neutral 最小事实。
                matches.push(AssessmentSession {
                    // 保存精确目标当前发布集合。
                    capabilities,
                });
            }
        }
        // 零命中允许上层继续查询只读关系图。
        if matches.is_empty() {
            // 不把未命中误报成 provider 错误。
            return Ok(None);
        }
        // 一项命中返回当前 session；多项命中 fail closed。
        require_unique_provider_match(matches, session_id).map(Some)
    }
}

impl Default for AppFacadeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn public_status(
    registered_providers: usize,
    available_providers: usize,
    degraded_providers: usize,
) -> Value {
    // facade 自有基础 verb 不属于 capability registry。
    let mut public_verbs = vec!["sessions", "inspect"];
    // generic verb 只从 App surface 的强类型 registry 投影。
    public_verbs.extend(capabilities::action_verbs_for_surface(
        // 排除 Browser、Process 等其他公开 surface。
        capabilities::CapabilitySurface::App,
    ));
    // 构造保持现有字段和值顺序的公开状态。
    json!({
        "ok": true,
        "app": "app",
        "model": "opaque-session + versioned-capability",
        // 合并 facade 基础 verb 与 registry 驱动的 generic verb。
        "publicVerbs": public_verbs,
        "providers": {
            "registered": registered_providers,
            "available": available_providers,
            "degraded": degraded_providers,
        },
    })
}

fn ensure_session_capability(
    session_id: &str,
    capabilities: &[String],
    capability: &str,
) -> AppResult<()> {
    if capabilities.iter().any(|available| available == capability) {
        return Ok(());
    }
    Err(AppFacadeErrorCode::CapabilityUnsupported.with_details(
        "The target session does not support the requested capability.",
        json!({
            "sessionId": session_id,
            "capability": capability,
            "availableCapabilities": capabilities,
        }),
    ))
}

fn sanitize_public_value(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(Value::Object(foreground)) = object.get_mut("foreground") {
                foreground.remove("before");
                foreground.remove("after");
            }
            for child in object.values_mut() {
                sanitize_public_value(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                sanitize_public_value(item);
            }
        }
        _ => {}
    }
}

// 判定执行域是否承诺保持主机前景不变。
fn requires_foreground_invariance(execution_realm: ExecutionRealm) -> bool {
    // 只有显式主机前台域允许前景身份发生变化。
    !matches!(execution_realm, ExecutionRealm::HostForeground)
}

// 在 facade 边界拒绝违反声明的主机前景影响。
fn ensure_foreground_invariance(
    // 接收执行前仅在进程内使用的私有前景身份。
    before: isize,
    // 接收执行后仅在进程内使用的私有前景身份。
    after: isize,
    // 接收可公开的 capability ID；只读 facade 操作使用空值。
    capability: Option<&str>,
    // 标记 provider 是否已成功返回一个变更操作。
    mutation_completed: bool,
) -> AppResult<()> {
    // 相同身份满足前景不变承诺。
    if before == after {
        // 返回无副作用成功。
        return Ok(());
    }
    // 已完成变更时必须保留不可自动重试的结果事实。
    let details = if mutation_completed {
        // 不公开前后 HWND，只公开调用方已知 capability 与 outcome。
        json!({
            // 回显稳定 capability ID。
            "capability": capability,
            // provider 已成功返回，但结果因主机干扰不能认证。
            "outcome": "completed",
            // 禁止因错误响应重复执行变更。
            "retrySafe": false,
            // 保守声明目标可能已经发生变化。
            "targetMayHaveMutated": true,
        })
    } else {
        // 只读请求只需要声明受干扰结果，不产生 mutation outcome。
        json!({
            // 统一保留可选稳定 capability ID。
            "capability": capability,
            // 只读请求可以由调用方在重新发现后重试。
            "retrySafe": true,
            // 明确本门禁本身没有改变目标。
            "targetMayHaveMutated": false,
        })
    };
    // 前景变化必须以主契约错误拒绝成功结果。
    Err(AppFacadeErrorCode::HostInterferenceDetected.with_details(
        // 不泄漏任何原生前景身份。
        "The foreground target changed during a non-foreground app operation.",
        // 附加重试与 mutation 结果事实。
        details,
    ))
}

impl AppAdapter for AppFacadeAdapter {
    fn app_id(&self) -> &'static str {
        "app"
    }

    fn status(&self) -> AppResult<Value> {
        // 记录全部只读 provider 状态探测前的私有前景身份。
        let foreground_before = foreground_hwnd();
        let mut available = 0_usize;
        let mut degraded = 0_usize;
        for provider in &self.providers {
            match provider.status() {
                Ok(_) => available += 1,
                Err(_) => degraded += 1,
            }
        }
        // 读取状态聚合后的私有前景身份。
        let foreground_after = foreground_hwnd();
        // 受干扰状态不得作为成功诊断返回。
        ensure_foreground_invariance(foreground_before, foreground_after, None, false)?;
        Ok(public_status(self.providers.len(), available, degraded))
    }

    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        // 记录全部只读 provider 聚合前的前景窗口。
        let foreground_before = foreground_hwnd();
        // 保存各 provider 当前发布的公开 session。
        let mut sessions = Vec::new();
        // 保存单个 provider 降级而不破坏其他只读来源的结构化警告。
        let mut warnings = Vec::new();
        // 逐个调用已注册 provider 的只读 session 入口。
        for provider in &self.providers {
            // 单个 provider 失败只形成警告，聚合仍返回其他来源。
            match provider.sessions(request) {
                // 合并成功 provider 的公开 session 数组。
                Ok(result) => {
                    // 只接受 provider-neutral sessions 字段。
                    if let Some(items) = result.get("sessions").and_then(Value::as_array) {
                        // 在合并时统一清除任何私有前景句柄细节。
                        sessions.extend(items.iter().cloned().map(|mut item| {
                            // 递归净化公开对象。
                            sanitize_public_value(&mut item);
                            // 返回已净化 session。
                            item
                        }));
                    }
                }
                // 保存稳定错误码和消息，不暴露 provider 路由键。
                Err(error) => warnings.push(json!({
                    // 传播结构化错误码。
                    "code": error.code,
                    // 传播面向调用方的错误消息。
                    "message": error.message,
                })),
            }
        }
        // 使用固定 kind 顺序产生跨运行稳定的公共集合。
        sessions.sort_by_key(
            // 只读取 provider-neutral kind。
            |session| match session.get("kind").and_then(Value::as_str) {
                // 主机 session 始终排在首位。
                Some("host") => 0,
                // 应用 session 排在主机之后。
                Some("application") => 1,
                // 文档 session 排在应用之后。
                Some("document") => 2,
                // 窗口 session 排在结构化对象之后。
                Some("window") => 3,
                // 新类别保持在末尾且不改变既有顺序。
                _ => 4,
            },
        );
        // 保存截断前的完整公开 session 数量。
        let total = sessions.len();
        // 按调用方硬上限截断，不改变 provider 内部发现边界。
        sessions.truncate(request.max_items);
        // 聚合完成后读取前景窗口以报告真实不变性。
        let foreground_after = foreground_hwnd();
        // 受干扰 session 集合不得作为成功快照返回。
        ensure_foreground_invariance(foreground_before, foreground_after, None, false)?;
        // 构造与 C++ 对照基线共享的 provider-neutral 数据对象。
        let data = json!({
            "surface": self.app_id(),
            "capability": crate::capabilities::APPLICATION_SESSION_DISCOVER,
            "readOnly": true,
            "foregroundUnchanged": true,
            "targetIdentity": "opaque-versioned-session-id",
            "count": sessions.len(),
            "total": total,
            "truncated": total > sessions.len(),
            "sessions": sessions,
            "warnings": warnings,
        });
        // 从同一数据对象生成迁移期兼容 envelope，禁止两份逻辑漂移。
        let mut envelope = data.clone();
        // JSON 宏固定产生对象；保持防御性分支以避免 panic。
        let Some(fields) = envelope.as_object_mut() else {
            // 静态对象构造失败表示不可恢复的内部错误。
            unreachable!("application session discovery envelope must be an object");
        };
        // 补充 legacy 只读 surface 名称。
        fields.insert("app".to_owned(), Value::from(self.app_id()));
        // 补充统一成功标志。
        fields.insert("ok".to_owned(), Value::Bool(true));
        // 保留 C++ 兼容外壳的同源 data 副本。
        fields.insert("data".to_owned(), data);
        // 返回单一 Module 事实的兼容投影。
        Ok(envelope)
    }

    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        // 记录精确只读检查前的私有前景身份。
        let foreground_before = foreground_hwnd();
        // 先取得调用方已知的 opaque 目标，避免重复解析字段。
        let session_id = required_session_id(request)?;
        let resolved = self.resolve_session(request)?;
        let mut result = resolved.provider.inspect(request)?;
        // 读取 provider 精确检查后的私有前景身份。
        let foreground_after = foreground_hwnd();
        // 受干扰检查结果不得进入公开响应。
        ensure_foreground_invariance(foreground_before, foreground_after, None, false)?;
        sanitize_public_value(&mut result);
        Ok(json!({
            "ok": true,
            "app": self.app_id(),
            "sessionId": session_id,
            "data": result,
        }))
    }

    fn run(&self, request: &CommandRequest) -> AppResult<Value> {
        // 记录 provider 执行前的私有前景身份。
        let foreground_before = foreground_hwnd();
        // 一次解析并认证 generic verb、capability 与精确 App surface 定义。
        let invocation = parse_capability_invocation(request)
            .map_err(|error| before_provider(error, request))?;
        // 使用同一调用事实执行预先前台同意门禁。
        invocation
            .ensure_foreground_consent(request)
            .map_err(|error| before_provider(error, request))?;
        // 固定保存调用方已知 opaque 目标供 provider-neutral mapper 回显。
        let session_id =
            required_session_id(request).map_err(|error| before_provider(error, request))?;
        // 重新发现并唯一解析 provider。
        let resolved = self
            .resolve_execution_session(request, invocation.capability())
            .map_err(|error| before_provider(error, request))?;
        // 核对重新发现的精确 session 仍公开同一 capability。
        ensure_session_capability(
            // 核对调用方精确 opaque 目标。
            session_id,
            // 使用本次重新发现的 provider capability 集。
            &resolved.capabilities,
            // 使用已经认证的规范 capability ID。
            invocation.capability(),
        )
        .map_err(|error| before_provider(error, request))?;
        // 执行唯一 provider。
        let mut result = resolved
            .provider
            .execute(invocation.capability(), request)?;
        // 从 provider data 提取可选兼容形状供顶层 mapper 输出。
        let compatibility_shape = result
            // 只处理对象结果。
            .as_object_mut()
            // 移除内部 mapper 元数据，避免 data 重复。
            .and_then(|object| object.remove("compatibilityShape"))
            // 只接受字符串。
            .and_then(|value| value.as_str().map(ToOwned::to_owned));
        // 提取 provider 回显目标用于一致性核对。
        let provider_target = result
            // 只处理对象结果。
            .as_object_mut()
            // 从 data 移除重复 targetId。
            .and_then(|object| object.remove("targetId"))
            // 只接受字符串。
            .and_then(|value| value.as_str().map(ToOwned::to_owned));
        // provider 若回显目标，必须与调用方 opaque 目标完全一致。
        if provider_target
            .as_deref()
            .is_some_and(|value| value != session_id)
        {
            // 拒绝内部目标错配。
            return Err(after_provider(
                AppFacadeErrorCode::OperationFailed.error(
                    // 不公开 provider 或 native 事实。
                    "The app provider returned inconsistent target evidence.",
                ),
                request,
            ));
        }
        sanitize_public_value(&mut result);
        let foreground_after = foreground_hwnd();
        // 非前台执行域必须拒绝任何前景身份变化。
        if requires_foreground_invariance(invocation.execution_realm()) {
            // 变更动作已成功返回时保留不可重试的 outcome 证据。
            ensure_foreground_invariance(
                // 传入执行前私有身份。
                foreground_before,
                // 传入执行后私有身份。
                foreground_after,
                // 只回显公开 capability ID。
                Some(invocation.capability()),
                // 从单一注册表判定是否已经完成 mutation。
                invocation.mutates(),
            )
            .map_err(|error| after_provider(error, request))?;
        }
        let mut response = json!({
            "ok": true,
            "app": self.app_id(),
            "verb": invocation.verb(),
            "capability": invocation.capability(),
            // 回显调用方已知的精确 opaque 目标。
            "targetId": session_id,
            "data": result,
            "meta": {
                "foreground": foreground_snapshot(foreground_before, foreground_after),
                // 从稳定 capability 选择公开目标描述，不检查或回显 provider 事实。
                "targeting": targeting_label(invocation.capability()),
            },
        });
        // 仅在 provider 明确提供时输出 capability 兼容形状。
        if let Some(shape) = compatibility_shape {
            // response 构造保证顶层是 object。
            if let Some(object) = response.as_object_mut() {
                // 插入稳定 mapper 形状。
                object.insert("compatibilityShape".to_owned(), Value::String(shape));
            }
        }
        sanitize_public_value(&mut response);
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    // 导入有序集合以比较 provider 与 registry 的完整 capability 集。
    use std::collections::BTreeSet;

    use serde_json::json;

    // 导入版本化 capability 单一注册表。
    use crate::capabilities;
    // 导入强类型执行域以验证前台豁免边界。
    use crate::domain::ExecutionRealm;

    use super::{
        // 导入 facade 以核对全部 provider 公布集合。
        AppFacadeAdapter,
        // 导入 facade 前景不变门禁与执行域判定。
        ensure_foreground_invariance,
        // 导入跨 provider 唯一命中门禁。
        ensure_session_capability,
        public_status,
        require_unique_provider_match,
        requires_foreground_invariance,
        sanitize_public_value,
    };

    #[test]
    fn capability_gate_uses_the_resolved_session_list() {
        let application_capabilities = [capabilities::IMAGE_CANVAS_CREATE.to_owned()];
        let result = ensure_session_capability(
            "s1:c3:0000000000000000",
            &application_capabilities,
            capabilities::ARTIFACT_SAVE,
        );
        let error = match result {
            Ok(()) => panic!("application session must not accept document save"),
            Err(error) => error,
        };
        assert_eq!(error.code, "CAPABILITY_UNSUPPORTED");
    }

    // 验证 provider 公布集合与单一注册表双向完全覆盖。
    #[test]
    fn provider_capabilities_match_the_runtime_registry() {
        // 构造包含全部生产 provider 的 facade。
        let facade = AppFacadeAdapter::new();
        // 收集全部 provider 公布的 capability ID。
        let advertised = facade
            // 遍历生产 provider。
            .providers
            // 创建 provider 迭代器。
            .iter()
            // 展开每个 provider 的稳定能力切片。
            .flat_map(|provider| provider.capabilities().iter().copied())
            // 收集为有序集合并顺便去重。
            .collect::<BTreeSet<_>>();
        // 收集单一注册表中的 capability ID。
        let registered = capabilities::ALL
            // 创建 registry 迭代器。
            .iter()
            // 只比较统一 app facade 所属 capability。
            .filter(|definition| definition.surface == capabilities::CapabilitySurface::App)
            // 只投影稳定 ID。
            .map(|definition| definition.id)
            // 收集为可比较的有序集合。
            .collect::<BTreeSet<_>>();
        // 任一方向遗漏都会使门禁失败。
        assert_eq!(advertised, registered);
    }

    // 验证 status verb 兼容值与 provider 隐私边界。
    #[test]
    fn status_summary_preserves_verbs_and_hides_provider_payloads() {
        let status = public_status(3, 2, 1);
        // 公开 verb 必须保持基础入口在前、registry generic verb 在后。
        assert_eq!(
            // 读取完整公开数组。
            status["publicVerbs"],
            // 锁定现有 JSON over stdio 兼容值与顺序。
            json!([
                // facade 自有 session 聚合入口。
                "sessions",
                // facade 自有精确检查入口。
                "inspect",
                // registry 只读动作。
                "read",
                // registry 创建动作。
                "create",
                // registry 应用动作。
                "apply",
                // registry 保存动作。
                "save",
                // registry 导出动作。
                "export",
                // registry 关闭动作。
                "close",
                // registry 截图动作。
                "screenshot",
                // registry 录制动作。
                "record"
            ])
        );
        let text = status.to_string();
        assert!(!text.contains("backend"));
        assert!(!text.contains("provider_id"));
        assert!(!text.contains("result"));
    }

    #[test]
    fn facade_foreground_metadata_hides_native_window_handles() {
        let mut value = json!({
            "foreground": { "before": 42, "after": 84, "unchanged": false },
            "data": {
                "foreground": { "before": 42, "after": 42, "unchanged": true }
            }
        });

        sanitize_public_value(&mut value);

        let text = value.to_string();
        assert!(!text.contains("before"));
        assert!(!text.contains("after"));
        assert_eq!(value["foreground"]["unchanged"], false);
        assert_eq!(value["data"]["foreground"]["unchanged"], true);
    }

    // 验证相同前景身份对只读与 mutation 都保持成功。
    #[test]
    // 使用合成身份覆盖无真实窗口影响的成功路径。
    fn unchanged_foreground_satisfies_facade_invariance() {
        // 相同只读身份必须通过门禁。
        assert!(ensure_foreground_invariance(23, 23, None, false).is_ok());
        // 相同 mutation 身份也必须通过门禁。
        assert!(
            // 提供稳定公开 capability 以覆盖 mutation 成功路径。
            ensure_foreground_invariance(23, 23, Some(capabilities::UI_TEXT_INPUT), true).is_ok()
        );
    }

    // 验证只读 app facade 在前景变化时拒绝结果且不泄漏原生身份。
    #[test]
    // 使用合成前景身份覆盖无需操作真实窗口的失败路径。
    fn read_foreground_change_is_rejected_without_native_identity() {
        // 构造不同的私有前景身份并取得结构化错误。
        let error = match ensure_foreground_invariance(41, 42, None, false) {
            // 意外成功表示受干扰读取被错误认证。
            Ok(_) => panic!("foreground-changing read must be rejected"),
            // 保存预期错误以核对公开语义。
            Err(error) => error,
        };
        // Rust 主契约必须使用统一宿主干扰错误码。
        assert_eq!(error.code, "HOST_INTERFERENCE_DETECTED");
        // 只读失败允许重新发现后重试。
        assert_eq!(error.details["retrySafe"], true);
        // 只读门禁不得声称目标已被修改。
        assert_eq!(error.details["targetMayHaveMutated"], false);
        // 错误 JSON 不得包含任一合成原生身份。
        let serialized = error.details.to_string();
        // 前景前值必须保持私有。
        assert!(!serialized.contains("41"));
        // 前景后值必须保持私有。
        assert!(!serialized.contains("42"));
    }

    // 验证已返回的后台 mutation 在前景变化时禁止自动重试。
    #[test]
    // 使用公开 capability 与合成身份覆盖 outcome 语义。
    fn mutation_foreground_change_preserves_non_retryable_outcome() {
        // 构造已完成 mutation 的受干扰结果。
        let error = match ensure_foreground_invariance(
            // 使用合成执行前身份。
            7,
            // 使用不同合成执行后身份。
            9,
            // 使用稳定公开 capability。
            Some(capabilities::UI_TEXT_INPUT),
            // 标记 provider 已返回 mutation 成功。
            true,
        ) {
            // 意外成功表示受干扰 mutation 被错误认证。
            Ok(_) => panic!("foreground-changing mutation must be rejected"),
            // 保存预期错误。
            Err(error) => error,
        };
        // mutation 主错误仍使用宿主干扰错误码。
        assert_eq!(error.code, "HOST_INTERFERENCE_DETECTED");
        // provider 已返回时结果分类为已完成但未获认证。
        assert_eq!(error.details["outcome"], "completed");
        // 禁止调用方自动重试。
        assert_eq!(error.details["retrySafe"], false);
        // 保守声明目标可能已经变化。
        assert_eq!(error.details["targetMayHaveMutated"], true);
        // 只允许公开 capability 进入错误证据。
        assert_eq!(error.details["capability"], capabilities::UI_TEXT_INPUT);
    }

    // 验证只有显式 host-foreground 执行域豁免前景不变门禁。
    #[test]
    // 遍历全部强类型执行域锁定未来 capability 的默认失败闭合行为。
    fn only_host_foreground_realm_is_exempt_from_invariance() {
        // 构造所有公开执行域与期望门禁值。
        let cases = [
            // 主机无头域必须保持前景不变。
            (ExecutionRealm::HostHeadless, true),
            // 主机后台域必须保持前景不变。
            (ExecutionRealm::HostBackground, true),
            // 同会话无焦点域必须保持前景不变。
            (ExecutionRealm::SameSessionNoFocus, true),
            // 隔离 worker 域必须保持前景不变。
            (ExecutionRealm::IsolatedWorker, true),
            // 显式主机前台域允许可观察前景变化。
            (ExecutionRealm::HostForeground, false),
            // 无执行域不得隐式取得前台豁免。
            (ExecutionRealm::None, true),
        ];
        // 逐域核对门禁策略。
        for (realm, expected) in cases {
            // 任一新增或变更域都必须显式满足此不变量。
            assert_eq!(requires_foreground_invariance(realm), expected);
        }
    }

    // 验证跨 provider 门禁保持零命中与唯一命中的稳定分类。
    #[test]
    fn zero_and_single_provider_matches_keep_cardinality_semantics() {
        // 固定调用方已知的 opaque session ID。
        let session_id = "s2:a:0000000000000000";
        // 零命中必须返回带 session 证据的目标未找到错误。
        let error = require_unique_provider_match::<&str>(Vec::new(), session_id)
            // 测试期成功表示 stale 门禁失效。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("zero provider matches must fail"));
        // 保持稳定目标未找到码。
        assert_eq!(error.code, "TARGET_NOT_FOUND");
        // 保持既有 stale 消息。
        assert_eq!(
            error.message,
            "The app session is stale or no longer available."
        );
        // 证据只回显调用方已知 opaque ID。
        assert_eq!(error.details["sessionId"], session_id);

        // 唯一命中必须原样返回 provider 解析结果。
        let resolved = require_unique_provider_match(vec!["only"], session_id)
            // 唯一命中失败时立即终止测试。
            .unwrap_or_else(|error| panic!("single provider match failed: {error}"));
        // 核对未发生多余映射。
        assert_eq!(resolved, "only");
    }

    // 验证跨 provider 的同一公开指纹不会被任取一个。
    #[test]
    // 使用两个合成 provider 命中覆盖碰撞边界。
    fn duplicate_provider_matches_are_ambiguous() {
        // 构造两个 provider 都接受同一目标的合成结果。
        let result =
            require_unique_provider_match(vec!["first", "second"], "s2:a:0000000000000000");
        // 取得必须 fail closed 的结构化错误。
        let error = match result {
            // 意外唯一解析时立即使测试失败。
            Ok(_) => panic!("duplicate providers must be ambiguous"),
            // 保留预期的歧义错误以供继续断言。
            Err(error) => error,
            // 结束跨 provider 结果分类。
        };
        // 要求跨 provider 重叠返回稳定歧义错误码。
        assert_eq!(error.code, "AMBIGUOUS_TARGET");
        // 错误证据只保留调用方已知的不透明 ID。
        assert_eq!(error.details["sessionId"], "s2:a:0000000000000000");
        // 结束跨 provider 碰撞门禁测试。
    }
}
