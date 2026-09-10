//! 拥有独立交互会话 route 的纯 Policy 验证与执行域覆盖。

// 导入 provider-neutral JSON 构造。
use serde_json::{Value, json};

// 导入固定 command 路由、opaque ID 与统一请求类型。
use crate::{
    // 复用 command 协议拥有的 capability 到 operation 契约。
    components::{
        // 导入封闭 command 路由。
        interactive_command_protocol::capability_route,
        // 导入 canonical opaque 目标解析。
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    },
    // 导入统一错误、执行域和请求事实。
    domain::{
        AppControlError, AppResult, CommandRequest, ExecutionRealm, IsolationRequirement, Verb,
    },
};

// 固定认证独立交互会话 endpoint 的生产 broker 文件名。
pub(crate) const BROKER_FILE_NAME: &str =
    // 与 Cargo 自动发现的 Rust binary 名逐字一致。
    "ai-computer-toolkit-interactive-session-broker.exe";

// 构造不含 native endpoint 事实的稳定 Policy 错误。
fn route_error(
    // 接收封闭公开错误码。
    code: &'static str,
    // 接收不回显目标或 input 的固定消息。
    message: &'static str,
    // 接收完整请求供稳定公开投影。
    request: &CommandRequest,
) -> AppControlError {
    // 只发布调用方已经提供的公开路由事实。
    AppControlError::with_details(
        // 使用封闭公开错误码。
        code,
        // 使用固定安全消息。
        message,
        // 固定证明尚未进入 endpoint 或本地 provider。
        json!({
            // 输出公开 app surface。
            "app": request.app,
            // 输出公开 operation。
            "operation": request.operation,
            // 本地 provider 尚未调用。
            "localProviderInvoked": false,
            // 当前桌面 fallback 尚未发生。
            "foregroundFallbackUsed": false
        }),
    )
}

// 判断请求是否显式选择了独立交互会话 endpoint。
pub(crate) fn requested(request: &CommandRequest) -> bool {
    // 字段存在即触发专属失败闭合路线，不能由值类型错误绕回本地 provider。
    request.target.contains_key("interactiveSessionId")
}

// 读取非空公开字符串字段但不访问任何 provider。
fn required_string<'a>(
    // 接收公开 JSON 对象。
    object: &'a serde_json::Map<String, Value>,
    // 接收固定字段名。
    field: &'static str,
    // 接收安全错误消息。
    message: &'static str,
    // 接收完整请求供稳定错误投影。
    request: &CommandRequest,
) -> AppResult<&'a str> {
    // 只接受非空字符串。
    object
        // 读取固定字段。
        .get(field)
        // 要求 JSON 字符串。
        .and_then(Value::as_str)
        // 拒绝空白值。
        .filter(|value| !value.trim().is_empty())
        // 缺失或类型错误收敛为统一参数错误。
        .ok_or_else(|| route_error("INVALID_ARGUMENT", message, request))
}

// 验证独立会话 route 的 confirmation-first、封闭 capability 与 canonical 目标。
pub(crate) fn validate(request: &CommandRequest) -> AppResult<()> {
    // 未选择 endpoint 的请求保持原 Policy 路径。
    if !requested(request) {
        // 不改变普通请求语义。
        return Ok(());
    }
    // 该字段只属于 generic app.run facade。
    if request.app != "app" || request.verb != Verb::Run {
        // 禁止其他 surface 借用 endpoint 路由。
        return Err(route_error(
            // 使用稳定参数错误码。
            "INVALID_ARGUMENT",
            // 不列出内部 route。
            "interactiveSessionId is available only on generic app.run requests.",
            // 附加公开请求事实。
            request,
        ));
    }
    // confirmation 必须先于 capability、target 和 input 语义。
    if !request.confirmed {
        // 缺少逐操作确认时立即失败。
        return Err(route_error(
            // 使用既有确认错误码。
            "CONFIRMATION_REQUIRED",
            // 使用固定安全消息。
            "Independent interactive session mutation requires explicit confirmation.",
            // 附加零副作用事实。
            request,
        ));
    }
    // 读取版本化 capability。
    let capability = required_string(
        // 从统一参数对象读取。
        &request.args,
        // 使用固定字段名。
        "capability",
        // 不回显任意输入。
        "The independent interactive session route requires a versioned capability.",
        // 附加公开请求事实。
        request,
    )?;
    // 只接受 command 协议已经冻结的四条 mutation。
    let (expected_operation, foreground_required) =
        capability_route(capability).ok_or_else(|| {
            // 未登记 capability 不得转发到任意软件或 provider。
            route_error(
                // 使用 provider-neutral capability 缺口码。
                "CAPABILITY_GAP",
                // 不公开内部 provider 清单。
                "The capability is not available through independent interactive sessions.",
                // 附加公开请求事实。
                request,
            )
        })?;
    // capability 必须绑定唯一 generic operation。
    if request.operation.as_deref() != Some(expected_operation) {
        // 禁止 capability 借用其他 operation。
        return Err(route_error(
            // 使用稳定 capability 缺口码。
            "CAPABILITY_GAP",
            // 不把错配请求转发给 endpoint。
            "The capability does not match the independent interactive session operation.",
            // 附加零副作用事实。
            request,
        ));
    }
    // 前景型 mutation 需要独立会话自己的显式许可。
    if foreground_required && !request.foreground_consent {
        // 不复用 host 当前桌面的隐式许可。
        return Err(route_error(
            // 使用既有前景许可错误码。
            "FOREGROUND_CONSENT_REQUIRED",
            // 说明许可作用域但不回显目标。
            "The independent interactive session mutation requires explicit foreground consent.",
            // 附加零副作用事实。
            request,
        ));
    }
    // 独立 endpoint 永远只接受严格零干扰要求。
    if request.isolation_requirement != IsolationRequirement::Strict {
        // 标准模式不能通过添加字段提升权限。
        return Err(route_error(
            // 使用既有严格隔离错误码。
            "ISOLATION_REQUIRED",
            // 说明固定要求。
            "Independent interactive session execution requires strict isolation.",
            // 附加零副作用事实。
            request,
        ));
    }
    // 读取 worker 会话内重新解析的原窗口目标。
    let session_id = required_string(
        // 从公开 target 读取。
        &request.target,
        // 使用固定字段名。
        "sessionId",
        // 不回显其他 target 字段。
        "The independent interactive session route requires a canonical window sessionId.",
        // 附加零副作用事实。
        request,
    )?;
    // 原领域目标必须是 canonical s2:w。
    if OpaqueTargetId::parse(session_id).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window)
    {
        // native、旧版或其他 opaque 类别都失败闭合。
        return Err(route_error(
            // 使用稳定参数错误码。
            "INVALID_ARGUMENT",
            // 指明公开类别而不回显原值。
            "The independent interactive session route requires a canonical s2:w target.",
            // 附加零副作用事实。
            request,
        ));
    }
    // 读取调用方通过发现取得的授权代际。
    let interactive_session_id = required_string(
        // 从公开 target 读取。
        &request.target,
        // 使用固定字段名。
        "interactiveSessionId",
        // 不回显其他 endpoint。
        "The independent interactive session route requires a discovered endpoint ID.",
        // 附加零副作用事实。
        request,
    )?;
    // endpoint 必须是 canonical s2:i。
    if OpaqueTargetId::parse(interactive_session_id).map(OpaqueTargetId::kind)
        != Some(OpaqueTargetKind::InteractiveSession)
    {
        // native session ID 或其他 opaque 类别都失败闭合。
        return Err(route_error(
            // 使用稳定参数错误码。
            "INVALID_ARGUMENT",
            // 指明公开类别而不回显原值。
            "interactiveSessionId must be a canonical s2:i authorization generation.",
            // 附加零副作用事实。
            request,
        ));
    }
    // 所有纯 Policy 门禁通过。
    Ok(())
}

// 返回独立会话 route 对原 capability realm 的强制覆盖。
pub(crate) fn execution_realm(request: &CommandRequest) -> AppResult<Option<ExecutionRealm>> {
    // 未选择 endpoint 时不覆盖 registry。
    if !requested(request) {
        // 保持原 capability realm。
        return Ok(None);
    }
    // 即使调用方绕过 validate 直接取计划也必须完整失败闭合。
    validate(request)?;
    // 固定使用跨会话认证 worker 域。
    Ok(Some(ExecutionRealm::IsolatedWorker))
}

// 声明纯 Policy 边界测试。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 导入被测边界和统一请求类型。
    use super::*;

    // 构造不触碰 endpoint 的合法严格请求。
    fn valid_request() -> CommandRequest {
        // 从 generic app.run 开始。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 选择统一 apply operation。
        request.operation = Some("apply".to_owned());
        // 提供原 canonical 窗口目标。
        request
            // 访问 target 对象。
            .target
            // 写入固定窗口 ID。
            .insert("sessionId".to_owned(), json!("s2:w:0000000000000001"));
        // 提供发现取得的 endpoint 授权代际。
        request.target.insert(
            // 使用固定字段名。
            "interactiveSessionId".to_owned(),
            // 使用 canonical s2:i。
            json!("s2:i:0000000000000002"),
        );
        // 提供冻结 capability。
        request.args.insert(
            // 使用固定字段名。
            "capability".to_owned(),
            // 使用键盘输入路线。
            json!("ui.input.key@1"),
        );
        // 提供逐操作确认。
        request.confirmed = true;
        // 提供独立会话前景许可。
        request.foreground_consent = true;
        // 冻结严格隔离。
        request.isolation_requirement = IsolationRequirement::Strict;
        // 返回纯 JSON 请求。
        request
    }

    // 验证合法路线覆盖为隔离 worker。
    #[test]
    fn valid_route_requires_isolated_worker() {
        // 构造合法请求。
        let request = valid_request();
        // 完整 Policy 必须通过。
        assert!(validate(&request).is_ok());
        // 执行域必须强制覆盖。
        assert_eq!(
            // 读取强类型覆盖。
            execution_realm(&request).unwrap_or_else(|error| panic!("route rejected: {error}")),
            // 对比固定隔离域。
            Some(ExecutionRealm::IsolatedWorker)
        );
    }

    // 验证确认门禁先于 capability 和目标语义。
    #[test]
    fn confirmation_precedes_route_semantics() {
        // 从合法请求开始。
        let mut request = valid_request();
        // 移除确认。
        request.confirmed = false;
        // 同时注入不支持 capability。
        request.args.insert(
            // 使用固定字段名。
            "capability".to_owned(),
            // 使用任意未知命令。
            json!("arbitrary.command@1"),
        );
        // 取得预期错误。
        let error = validate(&request)
            // 必须失败。
            .err()
            // 意外成功时终止测试。
            .unwrap_or_else(|| panic!("unconfirmed route was accepted"));
        // 必须先返回确认错误。
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    // 验证 endpoint 类别不能借用窗口或 native ID。
    #[test]
    fn endpoint_id_requires_interactive_session_kind() {
        // 从合法请求开始。
        let mut request = valid_request();
        // 用窗口 opaque ID 替换 endpoint。
        request.target.insert(
            // 使用固定字段名。
            "interactiveSessionId".to_owned(),
            // 故意使用错误类别。
            json!("s2:w:0000000000000002"),
        );
        // 取得预期错误。
        let error = validate(&request)
            // 必须失败。
            .err()
            // 意外成功时终止测试。
            .unwrap_or_else(|| panic!("wrong endpoint kind was accepted"));
        // 保持稳定参数错误。
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }
}
