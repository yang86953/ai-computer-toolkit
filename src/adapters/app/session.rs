use serde_json::{Value, json};

use crate::{
    // 导入版本化 capability 运行时注册表。
    capabilities,
    // 导入调用对象需要公开给 facade 的强类型执行域。
    domain::{AppControlError, AppResult, CommandRequest, ExecutionRealm},
    policy,
};

// 表示 App 私有 session Adapter 允许产生的封闭错误码集合。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppSessionErrorCode {
    // 表示内部 provider 返回重复 opaque session。
    AmbiguousTarget,
    // 表示 capability 不属于精确 App surface registry。
    CapabilityUnsupported,
    // 表示调用方请求字段违反公开契约。
    InvalidArgument,
    // 表示内部 provider payload 违反 App session 契约。
    OperationFailed,
}

// 提供 App 私有 Adapter 错误码与公开协议文本的唯一映射。
impl AppSessionErrorCode {
    // 返回版本化公开错误码文本。
    const fn as_str(self) -> &'static str {
        // 穷举封闭集合并保持既有文本逐字不变。
        match self {
            // 映射目标歧义。
            Self::AmbiguousTarget => "AMBIGUOUS_TARGET",
            // 映射 capability 缺口。
            Self::CapabilityUnsupported => "CAPABILITY_UNSUPPORTED",
            // 映射参数拒绝。
            Self::InvalidArgument => "INVALID_ARGUMENT",
            // 映射内部 provider 契约失败。
            Self::OperationFailed => "OPERATION_FAILED",
        }
    }

    // 使用当前封闭错误码构造普通公开错误。
    fn error(self, message: impl Into<String>) -> AppControlError {
        // 隐藏 Adapter 私有类型并复用统一 error envelope。
        AppControlError::new(self.as_str(), message)
    }

    // 使用当前封闭错误码构造带公开证据的错误。
    fn with_details(
        // 接收稳定公开消息。
        self,
        // 接收可转换为拥有型字符串的消息。
        message: impl Into<String>,
        // 接收 provider-neutral 公开证据。
        details: Value,
    ) -> AppControlError {
        // 隐藏 Adapter 私有类型并保持既有 details 形状。
        AppControlError::with_details(self.as_str(), message, details)
    }
}

pub(crate) fn capability_descriptor(id: &str, constraints: Value) -> Value {
    // descriptor 只允许从运行时单一注册表投影。
    // 只允许注册表中存在的 ID 进入公开 descriptor。
    let Some(definition) =
        capabilities::definition_for_surface(capabilities::CapabilitySurface::App, id)
    else {
        // 未注册 ID 表示内部 provider 违反契约，立即失败而不是输出漂移 descriptor。
        unreachable!("capability descriptor id must be registered");
    };
    // 序列化稳定公开 descriptor。
    json!({
        "id": definition.id,
        "version": 1,
        "verb": definition.action.as_str(),
        "availability": "available",
        "execution": definition.execution,
        // 输出与 assessment 和运行时策略共享的精确 realm。
        "executionRealm": definition.execution_realm,
        // 仅 mutation 动作要求逐操作确认。
        "requiresConfirmation": definition.action.mutates(),
        "requiresForegroundConsent": definition.requires_foreground_consent,
        "inputSchema": definition.input_schema,
        "constraints": constraints,
    })
}

pub(super) fn required_session_id(request: &CommandRequest) -> AppResult<&str> {
    request
        .target
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            // 使用 App 私有 Adapter 参数错误码。
            AppSessionErrorCode::InvalidArgument.error("target.sessionId is required.")
        })
}

// 表示已由 App 私有边界完整认证的一次 capability 调用。
#[derive(Clone, Copy)]
pub(super) struct CapabilityInvocation {
    // 持有精确 App surface 的静态 registry 定义。
    definition: &'static capabilities::CapabilityDefinition,
}

// 只向 facade 暴露执行编排所需的窄只读事实。
impl CapabilityInvocation {
    // 返回规范版本化 capability ID。
    pub(super) const fn capability(self) -> &'static str {
        // ID 来自已经核对 surface 的 registry 定义。
        self.definition.id
    }

    // 返回 capability 对应的规范 generic verb。
    pub(super) const fn verb(self) -> &'static str {
        // 动词由封闭 action 枚举投影。
        self.definition.action.as_str()
    }

    // 返回策略和前景不变量使用的强类型执行域。
    pub(super) const fn execution_realm(self) -> ExecutionRealm {
        // 执行域与路由定义保持同源。
        self.definition.execution_realm
    }

    // 返回动作是否已经可能改变外部状态。
    pub(super) const fn mutates(self) -> bool {
        // 副作用语义由 registry action 唯一拥有。
        self.definition.action.mutates()
    }

    // 在不重复查 registry 的情况下执行预先前台同意门禁。
    pub(super) fn ensure_foreground_consent(self, request: &CommandRequest) -> AppResult<()> {
        // 复用稳定错误形状，只替换事实来源。
        ensure_foreground_consent_requirement(
            // 回显公开 capability ID。
            self.definition.id,
            // 使用同一定义的预先同意策略。
            self.definition.requires_upfront_foreground_consent,
            // 传递完整请求供公共策略错误生成证据。
            request,
        )
    }
}

// 一次解析 app.run 的字符串输入并产出强类型调用事实。
pub(super) fn parse_capability_invocation(
    // 接收尚未进入 provider 的统一请求。
    request: &CommandRequest,
) -> AppResult<CapabilityInvocation> {
    // 保持缺失 generic verb 的稳定参数错误。
    let verb = request.operation.as_deref().ok_or_else(|| {
        // 使用既有公开错误码和消息。
        AppSessionErrorCode::InvalidArgument.error("run app requires a generic verb.")
    })?;
    // 从封闭参数对象读取版本化 capability ID。
    let capability = request
        // 访问调用参数。
        .args
        // 读取固定字段。
        .get("capability")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 拒绝空字符串。
        .filter(|value| !value.is_empty())
        // 保持缺失 capability 的稳定参数错误。
        .ok_or_else(|| {
            // 使用既有公开错误码和消息。
            AppSessionErrorCode::InvalidArgument.error("args.capability is required.")
        })?;
    // 只从精确 App surface registry 取得一次定义。
    let definition =
        capabilities::definition_for_surface(capabilities::CapabilitySurface::App, capability)
            // 未知或其他 surface 的 ID 使用相同公开缺口语义。
            .ok_or_else(|| {
                // 保持未知 capability 的结构化错误兼容。
                AppSessionErrorCode::CapabilityUnsupported.error(
                    // 只回显调用方已经提供的公开 ID。
                    format!("Unknown capability '{capability}'."),
                )
            })?;
    // 从封闭动作类别取得唯一规范 verb。
    let expected = definition.action.as_str();
    // 拒绝调用 verb 与 capability 动作不一致。
    if verb != expected {
        // 保持既有错误码、消息与详情字段。
        return Err(AppSessionErrorCode::InvalidArgument.with_details(
            // 说明 generic verb 不匹配。
            "The capability is not valid for this generic verb.",
            // 输出调用值和同源期望值。
            json!({ "verb": verb, "capability": capability, "expectedVerb": expected }),
        ));
    }
    // 返回封闭强类型调用，后续不得再次解析字符串事实。
    Ok(CapabilityInvocation { definition })
}

pub(super) fn capability_requires_foreground_consent(capability: &str) -> bool {
    // 未注册 capability 按不放宽权限的既有调用顺序保持 false，未知值会先被 verb 门禁拒绝。
    capabilities::definition_for_surface(capabilities::CapabilitySurface::App, capability)
        // 读取注册表中的预先确认策略。
        .is_some_and(|definition| definition.requires_upfront_foreground_consent)
}

// 按已经解析的 capability 策略执行统一前台同意门禁。
fn ensure_foreground_consent_requirement(
    // 接收允许进入公开错误证据的 capability ID。
    capability: &str,
    // 接收 registry 已决定的预先同意要求。
    required: bool,
    // 接收公共策略错误需要的请求上下文。
    request: &CommandRequest,
) -> AppResult<()> {
    // 无要求或已有同意时允许继续。
    if !required || request.foreground_consent {
        // 返回无附加事实的成功。
        return Ok(());
    }
    // 保持既有前台同意错误及其公开证据。
    Err(policy::foreground_consent_required(
        // 传递原始请求供统一策略层生成证据。
        request,
        // 保持既有稳定错误原因。
        "The requested capability always activates a native window and sends foreground input.",
        // 只输出 provider-neutral capability 与执行类别。
        json!({
            "capability": capability,
            "execution": "foreground",
        }),
    ))
}

// 为仍需独立防御的 provider 保留按 capability ID 校验入口。
pub(super) fn ensure_capability_foreground_consent(
    capability: &str,
    request: &CommandRequest,
) -> AppResult<()> {
    // 兼容 provider 自主防御门禁时再按公开 ID 查询策略。
    ensure_foreground_consent_requirement(
        // 回显调用方已经提供的 capability ID。
        capability,
        // 从精确 App registry 读取预先同意要求。
        capability_requires_foreground_consent(capability),
        // 传递完整请求供策略层生成证据。
        request,
    )
}

pub(super) fn capability_ids_for_session(
    result: &Value,
    session_id: &str,
) -> AppResult<Option<Vec<String>>> {
    let sessions = result
        .get("sessions")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AppSessionErrorCode::OperationFailed
                .error("An internal provider returned an invalid sessions payload.")
        })?;
    let matches = sessions
        .iter()
        .filter(|session| session.get("sessionId").and_then(Value::as_str) == Some(session_id))
        .collect::<Vec<_>>();
    let session = match matches.as_slice() {
        [] => return Ok(None),
        [session] => *session,
        _ => {
            return Err(AppSessionErrorCode::AmbiguousTarget
                .error("An internal provider returned duplicate opaque sessions."));
        }
    };
    let capabilities = session
        .get("capabilities")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AppSessionErrorCode::OperationFailed
                .error("An internal provider returned a session without capabilities.")
        })?;
    capabilities
        .iter()
        .map(|descriptor| {
            descriptor
                .get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    AppSessionErrorCode::OperationFailed
                        .error("An internal provider returned an invalid capability descriptor.")
                })
        })
        .collect::<AppResult<Vec<_>>>()
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    // 导入测试请求使用的公开顶层动词。
    use crate::domain::Verb;

    // 验证四种 App session Adapter 错误码的完整稳定映射。
    #[test]
    fn all_app_session_error_codes_keep_stable_public_text() {
        // 固定完整类型与公开文本对照表。
        let mappings = [
            // 保持目标歧义码。
            (AppSessionErrorCode::AmbiguousTarget, "AMBIGUOUS_TARGET"),
            // 保持 capability 缺口码。
            (
                AppSessionErrorCode::CapabilityUnsupported,
                "CAPABILITY_UNSUPPORTED",
            ),
            // 保持参数拒绝码。
            (AppSessionErrorCode::InvalidArgument, "INVALID_ARGUMENT"),
            // 保持内部 provider 失败码。
            (AppSessionErrorCode::OperationFailed, "OPERATION_FAILED"),
        ];
        // 核对完整集合规模。
        assert_eq!(mappings.len(), 4);
        // 逐项核对唯一映射。
        for (code, expected) in mappings {
            // 公开错误文本必须逐字稳定。
            assert_eq!(code.as_str(), expected);
        }
    }

    // 构造尚未进入 provider 的 app.run 调用请求。
    fn invocation_request(
        // 接收可选 generic verb。
        operation: Option<&str>,
        // 接收可选 capability JSON 值以覆盖错误类型。
        capability: Option<Value>,
    ) -> CommandRequest {
        // 从统一 app.run 请求开始。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 仅在调用方提供时写入 generic verb。
        request.operation = operation.map(ToOwned::to_owned);
        // 仅在调用方提供时写入 capability 字段。
        if let Some(capability) = capability {
            // 使用固定公开字段名。
            request
                // 访问封闭参数对象。
                .args
                // 写入测试值。
                .insert("capability".to_owned(), capability);
        }
        // 返回尚未解析的请求。
        request
    }

    // 验证有效请求只产生一个精确 App registry 调用事实。
    #[test]
    fn capability_invocation_projects_one_registered_definition() -> AppResult<()> {
        // 构造画布创建调用。
        let request = invocation_request(
            // 使用 registry action 对应的 generic verb。
            Some("create"),
            // 使用精确 App surface capability。
            Some(json!(capabilities::IMAGE_CANVAS_CREATE)),
        );
        // 通过生产解析边界取得强类型调用。
        let invocation = parse_capability_invocation(&request)?;
        // ID 必须来自精确 registry 定义。
        assert_eq!(invocation.capability(), capabilities::IMAGE_CANVAS_CREATE);
        // verb 必须由同一定义的 action 投影。
        assert_eq!(invocation.verb(), "create");
        // 执行域必须与 registry 保持同源。
        assert_eq!(invocation.execution_realm(), ExecutionRealm::HostBackground);
        // 创建动作必须保留 mutation 语义。
        assert!(invocation.mutates());
        // 返回测试成功。
        Ok(())
    }

    // 验证缺失 verb 与 capability 保持既有失败形状。
    #[test]
    fn capability_invocation_rejects_missing_request_fields() {
        // 构造缺失 generic verb 的请求。
        let missing_verb = invocation_request(
            // 不提供 operation。
            None,
            // 提供合法 capability 以隔离失败原因。
            Some(json!(capabilities::IMAGE_CANVAS_CREATE)),
        );
        // 解析必须在 provider 前失败。
        let verb_error = parse_capability_invocation(&missing_verb)
            // 测试期成功表示门禁失效。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("missing verb must fail"));
        // 保持既有错误码。
        assert_eq!(verb_error.code, "INVALID_ARGUMENT");
        // 保持既有公开消息。
        assert_eq!(verb_error.message, "run app requires a generic verb.");
        // 构造缺失 capability 的请求。
        let missing_capability = invocation_request(
            // 提供合法 generic verb。
            Some("create"),
            // 不提供 capability。
            None,
        );
        // 解析必须在 provider 前失败。
        let capability_error = parse_capability_invocation(&missing_capability)
            // 测试期成功表示门禁失效。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("missing capability must fail"));
        // 保持既有错误码。
        assert_eq!(capability_error.code, "INVALID_ARGUMENT");
        // 保持既有公开消息。
        assert_eq!(capability_error.message, "args.capability is required.");
    }

    // 验证精确 session ID 缺失时保持既有参数错误。
    #[test]
    fn required_session_id_keeps_stable_missing_target_error() {
        // 构造不含 sessionId 的只读 App 请求。
        let request = CommandRequest::read(Verb::Inspect, "app");
        // 目标解析必须在 provider 前失败。
        let error = required_session_id(&request)
            // 测试期成功表示精确目标门禁失效。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("missing sessionId must fail"));
        // 保持参数错误码。
        assert_eq!(error.code, "INVALID_ARGUMENT");
        // 保持既有公开消息。
        assert_eq!(error.message, "target.sessionId is required.");
    }

    // 验证未知和非 App surface capability 使用同一失败闭合语义。
    #[test]
    fn capability_invocation_rejects_unregistered_or_wrong_surface_ids() {
        // 依次覆盖未知 ID 与属于 Window surface 的真实 ID。
        for capability in ["unknown.create@1", capabilities::WINDOW_DISCOVER] {
            // 构造表面上 verb 合法的调用。
            let request = invocation_request(
                // 使用 create 避免提前触发 verb 缺失。
                Some("create"),
                // 写入待拒绝 ID。
                Some(json!(capability)),
            );
            // 解析必须拒绝未由 App surface 拥有的 ID。
            let error = parse_capability_invocation(&request)
                // 测试期成功表示 surface 门禁失效。
                .err()
                // 使用显式 panic 标记当前 ID。
                .unwrap_or_else(|| panic!("{capability} must fail"));
            // 保持既有 capability 缺口错误码。
            assert_eq!(error.code, "CAPABILITY_UNSUPPORTED");
            // 只回显调用方提供的公开 ID。
            assert_eq!(error.message, format!("Unknown capability '{capability}'."));
        }
    }

    // 验证 generic verb 不匹配时输出 registry 同源期望值。
    #[test]
    fn capability_invocation_rejects_mismatched_generic_verb() {
        // 将读取 verb 与创建 capability 故意错配。
        let request = invocation_request(
            // 提供错误 generic verb。
            Some("read"),
            // 提供合法 App capability。
            Some(json!(capabilities::IMAGE_CANVAS_CREATE)),
        );
        // 解析必须在 provider 前失败。
        let error = parse_capability_invocation(&request)
            // 测试期成功表示动作门禁失效。
            .err()
            // 使用显式 panic 保留失败原因。
            .unwrap_or_else(|| panic!("mismatched verb must fail"));
        // 保持既有错误码。
        assert_eq!(error.code, "INVALID_ARGUMENT");
        // 保持既有公开消息。
        assert_eq!(
            // 读取实际消息。
            error.message,
            // 核对稳定消息。
            "The capability is not valid for this generic verb."
        );
        // 回显调用 verb。
        assert_eq!(error.details["verb"], "read");
        // 回显公开 capability。
        assert_eq!(
            error.details["capability"],
            capabilities::IMAGE_CANVAS_CREATE
        );
        // 期望 verb 必须来自 registry action。
        assert_eq!(error.details["expectedVerb"], "create");
    }

    // 验证强类型调用复用同一 registry 前台策略。
    #[test]
    fn capability_invocation_carries_upfront_foreground_policy() {
        // 构造必须预先取得前台同意的键盘输入调用。
        let request = invocation_request(
            // 使用对应 apply 动词。
            Some("apply"),
            // 使用原生键盘输入 capability。
            Some(json!(capabilities::UI_INPUT_KEY)),
        );
        // 解析强类型调用。
        let invocation = parse_capability_invocation(&request)
            // 静态 registry 项缺失属于测试失败。
            .unwrap_or_else(|error| panic!("valid invocation failed: {error}"));
        // 未取得前台同意时必须失败。
        let error = invocation
            // 使用同一解析结果执行策略门禁。
            .ensure_foreground_consent(&request)
            // 测试期成功表示策略事实丢失。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("foreground consent must be required"));
        // 保持统一前台同意错误码。
        assert_eq!(error.code, "FOREGROUND_CONSENT_REQUIRED");
        // 证据只回显公开 capability。
        assert_eq!(
            error.details["evidence"]["capability"],
            capabilities::UI_INPUT_KEY
        );
    }

    // 验证每个 registry 项都能投影完整且一致的公开 descriptor 元数据。
    #[test]
    fn every_registered_capability_has_consistent_public_descriptor_metadata() {
        // 逐项生成不含领域约束的最小 descriptor。
        for definition in capabilities::ALL
            .iter()
            .filter(|definition| definition.surface == capabilities::CapabilitySurface::App)
        {
            // 通过生产 helper 投影公开字段。
            let descriptor = capability_descriptor(definition.id, json!({}));
            // ID 必须保持稳定。
            assert_eq!(descriptor["id"], definition.id);
            // generic verb 必须来自 registry action。
            assert_eq!(descriptor["verb"], definition.action.as_str());
            // 执行方式必须来自 registry。
            assert_eq!(descriptor["execution"], definition.execution);
            // 精确执行域必须来自同一 registry 定义。
            assert_eq!(
                // 读取公开 realm 字段。
                descriptor["executionRealm"],
                // 序列化强类型 registry realm。
                json!(definition.execution_realm)
            );
            // 执行方式必须属于公开契约允许的封闭集合。
            assert!(matches!(
                definition.execution,
                "background" | "background-preferred" | "foreground" | "isolated-worker"
            ));
            // confirmation 声明必须与动作副作用一致。
            assert_eq!(
                descriptor["requiresConfirmation"],
                definition.action.mutates()
            );
            // 前台确认声明必须来自 registry。
            assert_eq!(
                descriptor["requiresForegroundConsent"],
                definition.requires_foreground_consent
            );
            // 预先强制前台确认只能是公开声明可能需要前台确认的子集。
            assert!(
                !definition.requires_upfront_foreground_consent
                    || definition.requires_foreground_consent
            );
            // schema ID 必须来自 registry 且非空。
            assert_eq!(descriptor["inputSchema"], definition.input_schema);
            // 所有当前公开输入 schema 都必须保持版本一命名。
            assert!(definition.input_schema.starts_with("schema://"));
            // capability ID 与 descriptor version 必须保持版本一一致。
            assert!(definition.id.ends_with("@1"));
            // descriptor version 继续输出数值一。
            assert_eq!(descriptor["version"], 1);
        }
    }

    // 验证未注册 capability 无法生成公开 descriptor。
    #[test]
    #[should_panic(expected = "capability descriptor id must be registered")]
    fn unregistered_capability_descriptor_fails_closed() {
        // 使用明确未登记的版本化 ID 触发内部契约门禁。
        let _ = capability_descriptor("unregistered.read@1", json!({}));
    }

    #[test]
    fn advertised_capabilities_are_read_from_the_exact_session() -> AppResult<()> {
        let result = json!({
            "sessions": [
                { "sessionId": "s1:one", "capabilities": [{ "id": capabilities::IMAGE_CANVAS_CREATE }] },
                { "sessionId": "s1:two", "capabilities": [{ "id": capabilities::ARTIFACT_SAVE }] }
            ]
        });
        assert_eq!(
            capability_ids_for_session(&result, "s1:one")?,
            Some(vec![capabilities::IMAGE_CANVAS_CREATE.to_owned()])
        );
        Ok(())
    }

    // 验证内部 provider payload 的四条失败分支保持既有错误语义。
    #[test]
    fn invalid_session_payloads_keep_stable_adapter_errors() {
        // 固定缺失 sessions 数组的 provider payload。
        let missing_sessions = json!({});
        // 缺失 sessions 必须映射为内部操作失败。
        let missing_sessions_error = capability_ids_for_session(&missing_sessions, "s1:one")
            // 测试期成功表示 payload 门禁失效。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("missing sessions must fail"));
        // 核对稳定错误码。
        assert_eq!(missing_sessions_error.code, "OPERATION_FAILED");
        // 核对稳定公开消息。
        assert_eq!(
            missing_sessions_error.message,
            "An internal provider returned an invalid sessions payload."
        );

        // 固定重复 opaque session 的 provider payload。
        let duplicate_sessions = json!({
            // 提供两个相同 sessionId。
            "sessions": [
                // 提供第一个合法候选。
                { "sessionId": "s1:one", "capabilities": [] },
                // 提供第二个冲突候选。
                { "sessionId": "s1:one", "capabilities": [] },
            ],
        });
        // 重复目标必须失败闭合。
        let duplicate_error = capability_ids_for_session(&duplicate_sessions, "s1:one")
            // 测试期成功表示唯一性门禁失效。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("duplicate sessions must fail"));
        // 核对目标歧义码。
        assert_eq!(duplicate_error.code, "AMBIGUOUS_TARGET");
        // 核对稳定公开消息。
        assert_eq!(
            duplicate_error.message,
            "An internal provider returned duplicate opaque sessions."
        );

        // 固定缺失 capability 数组的精确 session。
        let missing_capabilities = json!({
            // 提供目标但省略 capabilities。
            "sessions": [{ "sessionId": "s1:one" }],
        });
        // 缺失 capability 集合必须失败。
        let missing_capabilities_error =
            capability_ids_for_session(&missing_capabilities, "s1:one")
                // 测试期成功表示 descriptor 门禁失效。
                .err()
                // 使用显式 panic 保留失败上下文。
                .unwrap_or_else(|| panic!("missing capabilities must fail"));
        // 核对稳定错误码。
        assert_eq!(missing_capabilities_error.code, "OPERATION_FAILED");
        // 核对稳定公开消息。
        assert_eq!(
            missing_capabilities_error.message,
            "An internal provider returned a session without capabilities."
        );

        // 固定缺失 descriptor ID 的 capability 数组。
        let invalid_descriptor = json!({
            // 提供精确 session 与无 ID descriptor。
            "sessions": [{ "sessionId": "s1:one", "capabilities": [{}] }],
        });
        // 无效 descriptor 必须失败。
        let descriptor_error = capability_ids_for_session(&invalid_descriptor, "s1:one")
            // 测试期成功表示 descriptor 门禁失效。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("invalid descriptor must fail"));
        // 核对稳定错误码。
        assert_eq!(descriptor_error.code, "OPERATION_FAILED");
        // 核对稳定公开消息。
        assert_eq!(
            descriptor_error.message,
            "An internal provider returned an invalid capability descriptor."
        );
    }

    #[test]
    fn registered_foreground_capabilities_require_upfront_consent() {
        assert!(capability_requires_foreground_consent(
            capabilities::UI_INPUT_KEY
        ));
        assert!(capability_requires_foreground_consent(
            capabilities::UI_INPUT_POINTER
        ));
        // 通用窗口状态与几何变化也属于显式前景影响域。
        assert!(capability_requires_foreground_consent(
            // 核对 registry 中的生命周期能力。
            capabilities::WINDOW_LIFECYCLE
        ));
        assert!(!capability_requires_foreground_consent(
            capabilities::UI_TEXT_INPUT
        ));
        assert!(!capability_requires_foreground_consent(
            capabilities::WINDOW_SCREENSHOT
        ));
    }
}
