//! 把只读进程观察组合为统一 app facade 的进程生命周期 provider。

// 引入 provider 私有封闭错误码。
#[path = "process_error.rs"]
mod error_code;

// 导入 JSON 值与构造器。
use serde_json::{Value, json};

// 导入只读进程 Adapter、capability、风险模式与领域 Module。
use crate::{
    // 复用现有只读进程 surface，不复制 inventory 逻辑。
    adapters::{AppAdapter, ProcessAdapter, app::CapabilityProvider},
    // 导入两个固定进程终止 ID。
    capabilities,
    // 导入 capability 选择的封闭风险模式。
    components::process_termination_contract::ProcessTerminationMode,
    // 导入 provider-neutral 请求与结果。
    domain::{AppResult, CommandRequest},
    // 所有 mutation 语义由 Process Lifecycle Module 拥有。
    modules::process_termination,
};

// 导入当前 provider 私有错误类型。
use error_code::AppProcessProviderErrorCode;

// 导入公共 descriptor helper。
use super::capability_descriptor;

// 声明无状态 Windows 进程 provider。
pub(super) struct ProcessProvider;

// 保存本 provider 唯一可执行的两个独立风险 capability。
const PROCESS_CAPABILITIES: &[&str] = &[
    // 优雅终止固定投递顶层窗口关闭请求。
    capabilities::PROCESS_TERMINATE_GRACEFUL,
    // 强制终止固定调用内核终止。
    capabilities::PROCESS_TERMINATE_FORCE,
];

// 构造进程 termination capability 的公共 descriptor。
fn termination_descriptors() -> Value {
    // 返回稳定两项数组，风险不能由 input 切换。
    json!([
        // 优雅终止 descriptor。
        capability_descriptor(
            // 使用独立高风险 ID。
            capabilities::PROCESS_TERMINATE_GRACEFUL,
            // 公开 provider-neutral 约束。
            json!({
                // 只接受精确进程生命周期目标。
                "target": "exact-process-lifetime",
                // 风险由 capability 固定。
                "riskLevel": "high",
                // 只使用通用顶层窗口关闭请求。
                "mechanism": "top-level-window-close-request",
                // 没有顶层窗口时结构化不支持。
                "requiresTopLevelWindow": true,
                // 同步 deadline 保持硬边界。
                "timeoutMs": { "min": 1, "max": 30_000, "default": 5_000 },
                // 禁止强制回退或软件专用适配。
                "fallback": "none",
            }),
        ),
        // 强制终止 descriptor。
        capability_descriptor(
            // 使用独立 critical 风险 ID。
            capabilities::PROCESS_TERMINATE_FORCE,
            // 公开 provider-neutral 约束。
            json!({
                // 只接受精确进程生命周期目标。
                "target": "exact-process-lifetime",
                // 风险由 capability 固定。
                "riskLevel": "critical",
                // 只使用内核进程终止。
                "mechanism": "kernel-process-termination",
                // 同步 deadline 保持硬边界。
                "timeoutMs": { "min": 1, "max": 30_000, "default": 5_000 },
                // 固定保护当前工具、系统、critical 与权限目标。
                "protectedTargets": true,
                // 禁止其他协议或软件专用适配。
                "fallback": "none",
            }),
        ),
    ])
}

// 为单个公开进程观察附加 app session 元数据。
fn augment_process_session(process: &mut Value) -> AppResult<()> {
    // 进程观察必须是对象。
    let object = process.as_object_mut().ok_or_else(|| {
        // provider 内部形状错误不得泄漏实现细节。
        AppProcessProviderErrorCode::CapabilityUnsupported
            .error("The process observation cannot be exposed as an app session.")
    })?;
    // 只允许具备进程生命周期代际的目标发布 mutation 能力。
    let exact_lifetime = object
        // 读取现有只读身份新鲜度。
        .get("identityFreshness")
        // 只接受稳定文本。
        .and_then(Value::as_str)
        // 只认证进程生命周期身份。
        .is_some_and(|value| value == "process-lifetime");
    // 标记统一 app facade 稳定类别。
    object.insert("kind".to_owned(), json!("process"));
    // 只有精确生命周期目标才发布两个风险 capability。
    object.insert(
        // 使用统一 capability 字段。
        "capabilities".to_owned(),
        // 非精确快照保持 provider 拥有但不宣称可执行 mutation。
        if exact_lifetime {
            // 发布两个独立终止 descriptor。
            termination_descriptors()
        } else {
            // best-effort 身份只保留空能力集合。
            json!([])
        },
    );
    // 返回增强成功。
    Ok(())
}

// 从只读 inspect 结果中取得并增强唯一进程对象。
fn augment_inspection(result: &mut Value) -> AppResult<()> {
    // 读取固定 process 字段。
    let process = result.get_mut("process").ok_or_else(|| {
        // 缺失进程事实表示内部契约漂移。
        AppProcessProviderErrorCode::CapabilityUnsupported
            .error("The process observation returned no exact process.")
    })?;
    // 复用单 session 增强逻辑。
    augment_process_session(process)
}

// 为进程生命周期实现统一 provider 接口。
impl CapabilityProvider for ProcessProvider {
    // 返回仅用于进程内诊断的 provider 名称。
    fn provider_id(&self) -> &'static str {
        // 不进入公开 session 路由键。
        "windows-process-lifecycle"
    }

    // 返回两个独立终止 capability。
    fn capabilities(&self) -> &'static [&'static str] {
        // 复用稳定静态切片。
        PROCESS_CAPABILITIES
    }

    // 通过只读精确 inspect 判断当前 session 是否仍存在。
    fn accepts_session(&self, session_id: &str) -> AppResult<bool> {
        // 复用直接能力查询避免第二次全量枚举。
        self.session_capabilities(session_id)
            // 任一 Some 表示本 provider 唯一拥有当前进程目标。
            .map(|capabilities| capabilities.is_some())
    }

    // 返回只读进程 backend 与新增生命周期能力状态。
    fn status(&self) -> AppResult<Value> {
        // 组合现有只读状态，不执行保护或写探针。
        Ok(json!({
            // 标记 provider 可用。
            "ok": true,
            // 内部 backend 名称只留在 status 聚合内部。
            "backend": self.provider_id(),
            // 标记进程范围。
            "scope": "process-lifecycle",
            // 输出本 provider 两个能力。
            "capabilities": self.capabilities(),
            // 嵌入只读进程状态。
            "result": ProcessAdapter.status()?,
        }))
    }

    // 将只读进程 inventory 投影为 app process sessions。
    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        // 复用进程观察的完整 inventory、隐私与前景门禁。
        let mut result = ProcessAdapter.sessions(request)?;
        // 读取固定 sessions 数组。
        let sessions = result
            // 访问公开数组字段。
            .get_mut("sessions")
            // 只接受数组形状。
            .and_then(Value::as_array_mut)
            // 缺失时返回内部 provider 形状错误。
            .ok_or_else(|| {
                AppProcessProviderErrorCode::CapabilityUnsupported
                    .error("The process observation returned no session collection.")
            })?;
        // 逐项附加 kind 与 capability descriptor。
        for session in sessions {
            // 增强当前进程 session。
            augment_process_session(session)?;
        }
        // 返回统一 facade 可合并的 sessions 结果。
        Ok(result)
    }

    // 精确检查当前进程并附加能力描述。
    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        // 复用只读进程重新发现与隐私投影。
        let mut result = ProcessAdapter.inspect(request)?;
        // 附加进程 session 元数据。
        augment_inspection(&mut result)?;
        // 返回增强结果。
        Ok(result)
    }

    // 将两个固定 capability 原样委托 Process Lifecycle Module。
    fn execute(&self, capability: &str, request: &CommandRequest) -> AppResult<Value> {
        // capability ID 唯一选择风险模式。
        let mode = match capability {
            // 映射优雅终止。
            capabilities::PROCESS_TERMINATE_GRACEFUL => ProcessTerminationMode::Graceful,
            // 映射强制终止。
            capabilities::PROCESS_TERMINATE_FORCE => ProcessTerminationMode::Force,
            // 其他 capability 不属于本 provider。
            _ => {
                // 返回稳定能力缺口。
                return Err(AppProcessProviderErrorCode::CapabilityUnsupported.error(
                    "The process lifecycle provider supports two fixed termination capabilities only.",
                ));
            }
        };
        // 保留目标缺失状态供 Module 在确认后分类。
        let session_id = request
            // 访问 provider-neutral target。
            .target
            // 读取固定 sessionId。
            .get("sessionId")
            // 只接受字符串。
            .and_then(Value::as_str);
        // 保留 input 缺失状态供 Module 在确认后分类。
        let input = request.args.get("input");
        // 委托唯一领域所有者执行完整生命周期。
        process_termination::perform(
            // 传播可能缺失的目标。
            session_id,
            // 传播逐操作确认。
            request.confirmed,
            // 传播可能缺失的输入。
            input,
            // 传播 capability 固定风险。
            mode,
        )
    }

    // 使用精确只读 inspect 重新解析当前进程能力。
    fn session_capabilities(&self, session_id: &str) -> AppResult<Option<Vec<String>>> {
        // 构造只含 opaque 目标的 inspect 请求。
        let mut request = CommandRequest::read(crate::domain::Verb::Inspect, "process");
        // 写入调用方公开 sessionId。
        request
            // 访问目标对象。
            .target
            // 插入固定字段。
            .insert("sessionId".to_owned(), json!(session_id));
        // 执行现有只读进程重新发现。
        match ProcessAdapter.inspect(&request) {
            // 当前目标存在时根据身份新鲜度发布 mutation 能力。
            Ok(result) => {
                // 只认证进程生命周期身份。
                let exact_lifetime = result["process"]["identityFreshness"]
                    // 只接受稳定文本。
                    .as_str()
                    // 核对精确生命周期值。
                    .is_some_and(|value| value == "process-lifetime");
                // 保持 provider 对目标的所有权，即使当前能力集合为空。
                Ok(Some(if exact_lifetime {
                    // 投影静态 ID 为拥有型集合。
                    PROCESS_CAPABILITIES
                        // 遍历两个固定能力。
                        .iter()
                        // 复制公开字符串。
                        .map(|value| (*value).to_owned())
                        // 收集供 facade 能力门禁。
                        .collect()
                } else {
                    // best-effort 目标不发布终止能力。
                    Vec::new()
                }))
            }
            // stale 表示当前 provider 不命中。
            Err(error) if error.code == "STALE_SESSION" => Ok(None),
            // 歧义、宿主干扰或 inventory 失败保持原样。
            Err(error) => Err(error),
        }
    }
}

// 验证纯 provider 投影与能力边界。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造器。
    use serde_json::json;

    // 导入 provider trait 与稳定 capability。
    use crate::{adapters::app::CapabilityProvider, capabilities, domain::CommandRequest};

    // 导入被测 helper 与 provider。
    use super::{ProcessProvider, augment_process_session, termination_descriptors};

    // 锁定两级风险 descriptor 互不回退。
    #[test]
    fn descriptors_keep_distinct_risk_and_mechanism() {
        // 构造公开 descriptor 数组。
        let descriptors = termination_descriptors();
        // 优雅风险固定为 high。
        assert_eq!(descriptors[0]["constraints"]["riskLevel"], "high");
        // 强制风险固定为 critical。
        assert_eq!(descriptors[1]["constraints"]["riskLevel"], "critical");
        // 优雅路径明确没有 fallback。
        assert_eq!(descriptors[0]["constraints"]["fallback"], "none");
        // 两个 ID 必须互异。
        assert_ne!(descriptors[0]["id"], descriptors[1]["id"]);
    }

    // 验证进程 session 增强不产生原生目标字段。
    #[test]
    fn process_session_projection_is_opaque_and_capable() -> crate::domain::AppResult<()> {
        // 构造最小只读进程观察。
        let mut process = json!({
            // 使用 canonical 测试目标。
            "sessionId": "s2:p:0000000000000000",
            // 标记具备精确进程生命周期身份。
            "identityFreshness": "process-lifetime",
        });
        // 附加 app session 元数据。
        augment_process_session(&mut process)?;
        // 固定统一类别。
        assert_eq!(process["kind"], "process");
        // 发布两个独立能力。
        assert_eq!(process["capabilities"].as_array().map(Vec::len), Some(2));
        // 禁止 PID 或句柄泄漏。
        let serialized = process.to_string().to_ascii_lowercase();
        // 扫描原生标识字段。
        for forbidden in ["processid", "pid", "handle", "creationtime"] {
            // 任一原生字段都必须缺失。
            assert!(!serialized.contains(forbidden));
        }
        // 返回测试成功。
        Ok(())
    }

    // 验证 best-effort 快照仍由 provider 拥有但不发布 mutation 能力。
    #[test]
    fn best_effort_process_session_has_no_termination_capability() -> crate::domain::AppResult<()> {
        // 构造缺少可靠创建代际的公开进程观察。
        let mut process = json!({
            // 使用 canonical 测试目标。
            "sessionId": "s2:p:0000000000000000",
            // 标记只能代表当前快照。
            "identityFreshness": "best-effort-current-snapshot",
        });
        // 执行同一 app session 增强。
        augment_process_session(&mut process)?;
        // provider 仍明确拥有进程类别。
        assert_eq!(process["kind"], "process");
        // 不可靠身份不得发布任一终止能力。
        assert_eq!(process["capabilities"], json!([]));
        // 返回测试成功。
        Ok(())
    }

    // 验证 provider 只接受两个固定 capability。
    #[test]
    fn provider_rejects_unknown_capability_before_request_fields() {
        // 构造空请求以证明不会读取目标或 input。
        let request = CommandRequest::read(crate::domain::Verb::Run, "app");
        // 调用未知 capability。
        let error = ProcessProvider
            // 执行固定 provider 边界。
            .execute("process.terminate.auto@1", &request)
            // 未知能力不得成功。
            .err()
            // 使用显式 panic 保留上下文。
            .unwrap_or_else(|| panic!("unknown process capability must fail"));
        // 核对能力缺口码。
        assert_eq!(error.code, "CAPABILITY_UNSUPPORTED");
        // provider 稳定集合保持两项。
        assert_eq!(ProcessProvider.capabilities().len(), 2);
        // 核对正式 ID 均已发布。
        assert!(
            ProcessProvider
                .capabilities()
                .contains(&capabilities::PROCESS_TERMINATE_GRACEFUL)
        );
    }
}
