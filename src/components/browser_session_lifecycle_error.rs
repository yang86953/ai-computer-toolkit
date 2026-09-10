//! 投影 Browser Session 公共操作在 provider 前后的封闭错误事实。

// 导入 JSON 对象和值。
use serde_json::{Map, Value};

// 导入稳定 capability 与统一请求错误边界。
use crate::{
    // 只识别八个固定 Browser Session capability。
    capabilities,
    // 复用独立 Browser Session 与通用 host 身份分类。
    components::{
        // 分类 128 位 Browser Session 身份。
        browser_session_identity::{BrowserSessionIdentityShape, classify_browser_session_id},
        // 解析既有 64 位通用 opaque host 身份。
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    },
    // 复用产品统一错误与请求类型。
    domain::{AppControlError, CommandRequest, Verb},
};

// 表示公共生命周期错误发生的封闭执行阶段。
#[derive(Clone, Copy)]
enum LifecycleFailureStage {
    // 表示固定 broker 尚未取得业务接受。
    BeforeDispatch,
    // 表示 provider 已成功返回但 facade 证据未通过。
    AfterProvider,
}

// 只返回与 capability 种类匹配的 canonical 公共 target。
pub(crate) fn canonical_target_for_capability<'a>(
    // 接收待认证的公开 capability。
    capability: &str,
    // 接收 caller 提供的原始 target 文本。
    target_id: &'a str,
) -> Option<&'a str> {
    // 按 capability 的不同 identity 协议严格认证。
    match capability {
        // open 只能回显 canonical 通用 host identity。
        capabilities::BROWSER_SESSION_OPEN
            if OpaqueTargetId::parse(target_id)
                // 读取解析后的封闭 kind。
                .is_some_and(|target| target.kind() == OpaqueTargetKind::Host) =>
        {
            // 返回已认证公共 host target。
            Some(target_id)
        }
        // close 与页面操作只能回显 canonical 128 位 Browser Session identity。
        capabilities::BROWSER_SESSION_CLOSE
        | capabilities::BROWSER_PAGE_NAVIGATE
        | capabilities::BROWSER_PAGE_WAIT
        | capabilities::BROWSER_PAGE_QUERY
        | capabilities::BROWSER_ELEMENT_CLICK
        | capabilities::BROWSER_ELEMENT_TYPE
        | capabilities::BROWSER_PAGE_SCREENSHOT
            if classify_browser_session_id(target_id) == BrowserSessionIdentityShape::Canonical =>
        {
            // 返回已认证公共 Browser Session target。
            Some(target_id)
        }
        // 缺失、畸形或 wrong-kind target 不得进入冻结详情。
        _ => None,
    }
}

// 从请求中只读取与 capability 种类匹配的 canonical 公共 target。
fn canonical_target_id<'a>(request: &'a CommandRequest, capability: &str) -> Option<&'a str> {
    // 读取 caller 提供的字符串目标，不为缺失值造 sentinel。
    let target_id = request.target.get("sessionId").and_then(Value::as_str)?;
    // 委托同源 capability-aware 认证，避免各投影器发生漂移。
    canonical_target_for_capability(capability, target_id)
}

// 从调用请求读取已登记的 Browser Session capability。
fn lifecycle_capability(request: &CommandRequest) -> Option<&str> {
    // 只有统一 App run surface 可以产生本公共 lifecycle envelope。
    if request.app != "app" || request.verb != Verb::Run {
        // 其他 surface 不得被 args 中的同名字符串误分类。
        return None;
    }
    // 只读取 caller 已提供的公开 capability 字段，不解析 target 或连接 broker。
    match request.args.get("capability").and_then(Value::as_str) {
        // capability identity 在 generic verb 校验前已经足以绑定生命周期错误。
        Some(capability @ capabilities::BROWSER_SESSION_OPEN)
        | Some(capability @ capabilities::BROWSER_SESSION_CLOSE)
        | Some(capability @ capabilities::BROWSER_PAGE_NAVIGATE)
        | Some(capability @ capabilities::BROWSER_PAGE_WAIT)
        | Some(capability @ capabilities::BROWSER_PAGE_QUERY)
        | Some(capability @ capabilities::BROWSER_ELEMENT_CLICK)
        | Some(capability @ capabilities::BROWSER_ELEMENT_TYPE)
        | Some(capability @ capabilities::BROWSER_PAGE_SCREENSHOT) => {
            // 返回调用方已知的稳定 ID。
            Some(capability)
        }
        // 其他 capability 不属于本 Component。
        _ => None,
    }
}

// 构造不含 Policy、provider 或 transport 私有字段的生命周期详情。
fn lifecycle_details(request: &CommandRequest, stage: LifecycleFailureStage) -> Option<Value> {
    // 非 Browser Session 请求保持原错误不变。
    let capability = lifecycle_capability(request)?;
    // 从冻结公共字段开始构造严格对象。
    let mut details = Map::new();
    // 回显调用方明确请求的 capability。
    details.insert(
        // 使用冻结详情字段名。
        "capability".to_owned(),
        // 仅复制公开 capability 文本。
        Value::String(capability.to_owned()),
    );
    // 仅在 caller 实际提供字符串目标时原样回显，不猜测缺失身份。
    if let Some(target_id) = canonical_target_id(request, capability) {
        // 目标只是 caller 已知 opaque 值，不进行 native 解析。
        details.insert("targetId".to_owned(), Value::String(target_id.to_owned()));
    }
    // 按真实阶段冻结接受、终态与重试事实。
    match stage {
        // provider 前失败明确从未进入固定 broker 业务接受点。
        LifecycleFailureStage::BeforeDispatch => {
            // 输出未派发状态。
            details.insert(
                // 使用冻结 outcome 字段。
                "outcome".to_owned(),
                // 使用稳定未派发文本。
                Value::String("not-dispatched".to_owned()),
            );
            // 业务尚未接受。
            details.insert("accepted".to_owned(), Value::Bool(false));
            // 未派发拒绝本身是可信终态。
            details.insert("finalStateReached".to_owned(), Value::Bool(true));
            // 修正确认、输入或环境后可安全人工重试。
            details.insert("retrySafe".to_owned(), Value::Bool(true));
            // provider 前失败不改变目标。
            details.insert("targetMayHaveMutated".to_owned(), Value::Bool(false));
        }
        // provider 已返回后任何 facade 拒绝都不得自动重复 mutation。
        LifecycleFailureStage::AfterProvider => {
            // 公共调用最终失败，但领域 provider 已取得可信返回。
            details.insert("outcome".to_owned(), Value::String("failed".to_owned()));
            // provider 已越过业务接受点。
            details.insert("accepted".to_owned(), Value::Bool(true));
            // provider 返回构成可信终态。
            details.insert("finalStateReached".to_owned(), Value::Bool(true));
            // 已接受 mutation 不得安全重试。
            details.insert("retrySafe".to_owned(), Value::Bool(false));
            // mutation 保守标记可能变化，Query 保持零副作用。
            details.insert(
                // 使用冻结 mutation 字段。
                "targetMayHaveMutated".to_owned(),
                // 从单一 registry 的强类型 action 推导。
                Value::Bool(
                    capabilities::definition(capability)
                        // 已登记 Browser Session capability 必须存在定义。
                        .is_some_and(|definition| definition.action.mutates()),
                ),
            );
            // 显式禁止任何自动重派。
            details.insert("automaticRetryProhibited".to_owned(), Value::Bool(true));
        }
    }
    // 返回严格公共对象。
    Some(Value::Object(details))
}

// 为 Policy、权限、registry 与 facade 解析失败附加明确未派发事实。
pub(crate) fn project_before_dispatch(
    // 接收待投影的结构化错误。
    error: AppControlError,
    // 借用完整调用请求以取得公开 capability 与 target。
    request: &CommandRequest,
) -> AppControlError {
    // 非 Browser Session 请求必须逐字保持原错误。
    let Some(details) = lifecycle_details(request, LifecycleFailureStage::BeforeDispatch) else {
        // 返回未经改写的其他业务错误。
        return error;
    };
    // 将 facade 的内部路由分类收敛到 lifecycle 已冻结的公开集合。
    let (code, message) = match error.code {
        // 通用 inventory 未命中表示 caller 的 canonical target 已 stale。
        "TARGET_NOT_FOUND" => (
            // 返回稳定 stale 错误码。
            "STALE_SESSION",
            // 不公开是哪个 provider 未命中。
            "The browser session lifecycle target is stale or no longer available.".to_owned(),
        ),
        // capability 与 target kind 不匹配属于公开调用输入错误。
        "CAPABILITY_UNSUPPORTED" => (
            // 使用契约已经登记的参数错误码。
            "INVALID_ARGUMENT",
            // 不公开命中的其他 provider 或 capability inventory。
            "The browser session lifecycle target is invalid for this capability.".to_owned(),
        ),
        // provider 歧义或目录漂移表示固定能力路线不可用。
        "AMBIGUOUS_TARGET"
        | "CAPABILITY_GAP"
        | "BACKGROUND_OPERATION_UNAVAILABLE"
        | "OPERATION_FAILED" => (
            // 使用冻结的能力不可用分类。
            "CAPABILITY_UNAVAILABLE",
            // 不公开 provider 数量、目录或内部计划。
            "The browser session lifecycle capability is unavailable.".to_owned(),
        ),
        // 严格隔离无法满足时统一保持固定 worker 路线不可用。
        "ISOLATION_REQUIRED" => (
            // 使用契约登记的隔离 worker 不可用码。
            "ISOLATED_WORKER_UNAVAILABLE",
            // 不公开 companion 路径或进程探测事实。
            "The fixed browser session isolation route is unavailable.".to_owned(),
        ),
        // 其他已登记错误保持自身公共分类。
        _ => (error.code, error.message),
    };
    // 丢弃下层诊断详情，只保留冻结公共生命周期事实。
    AppControlError::with_details(code, message, details)
}

// 为 provider 已成功返回后的 facade 一致性失败附加不可重试事实。
pub(crate) fn project_after_provider(
    // 接收 facade 产生的结构化错误。
    error: AppControlError,
    // 借用原请求以绑定 caller 已知公开身份。
    request: &CommandRequest,
) -> AppControlError {
    // 非 Browser Session 请求继续使用既有 facade 错误形状。
    let Some(details) = lifecycle_details(request, LifecycleFailureStage::AfterProvider) else {
        // 返回其他 capability 的原错误。
        return error;
    };
    // 返回不含 provider、前景句柄或 transport 细节的公共失败。
    AppControlError::with_details(error.code, error.message, details)
}

// 验证阶段投影只在 Browser Session 请求上生效。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造器。
    use serde_json::json;

    // 导入稳定 capability 与请求类型。
    use crate::{
        // 导入稳定 Browser Session capability。
        capabilities,
        // 导入统一错误、请求与动词。
        domain::{AppControlError, CommandRequest, Verb},
    };

    // 导入两个阶段投影函数。
    use super::{project_after_provider, project_before_dispatch};

    // 构造包含调用方公开身份的 Browser Session close 请求。
    fn close_request() -> CommandRequest {
        // 从统一 App run 请求开始。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 使用冻结 generic close operation。
        request.operation = Some("close".to_owned());
        // 写入唯一公开 capability。
        request.args.insert(
            // 使用统一 capability 字段。
            "capability".to_owned(),
            // 写入稳定 close ID。
            json!(capabilities::BROWSER_SESSION_CLOSE),
        );
        // 写入 caller 已知 canonical opaque target。
        request.target.insert(
            // 使用统一目标字段。
            "sessionId".to_owned(),
            // 使用固定测试 identity。
            json!("s2:bs:0123456789abcdef0123456789abcdef"),
        );
        // 返回未确认请求供阶段测试使用。
        request
    }

    // 验证 browser_session_public_route 的前后阶段真值不会互相洗白。
    #[test]
    fn browser_session_public_route_error_projection_preserves_stage_truth() {
        // 投影 Policy 确认错误。
        let before = project_before_dispatch(
            // 模拟含不应公开详情的 Policy 错误。
            AppControlError::with_details(
                // 使用固定确认错误码。
                "CONFIRMATION_REQUIRED",
                // 使用安全测试消息。
                "confirm",
                // 注入必须被移除的私有详情。
                json!({"path":"private"}),
            ),
            // 绑定原公开请求。
            &close_request(),
        );
        // provider 前必须明确未接受。
        assert_eq!(before.details["accepted"], false);
        // 未派发错误允许安全人工重试。
        assert_eq!(before.details["retrySafe"], true);
        // 下层详情不得泄漏。
        assert!(before.details.get("path").is_none());
        // 投影 provider 返回后的 facade 错误。
        let after = project_after_provider(
            // 模拟前景证据不一致。
            AppControlError::new("HOST_INTERFERENCE_DETECTED", "interference"),
            // 使用相同公开请求。
            &close_request(),
        );
        // provider 返回后必须保守标记已接受。
        assert_eq!(after.details["accepted"], true);
        // 已接受错误禁止自动重派。
        assert_eq!(after.details["automaticRetryProhibited"], true);
        // 两个阶段都必须回显 caller 已知目标。
        assert_eq!(
            after.details["targetId"],
            "s2:bs:0123456789abcdef0123456789abcdef"
        );
        // 投影 target 与 capability kind 不匹配的 facade 错误。
        let wrong_kind = project_before_dispatch(
            // 模拟 facade 在其他 provider 上发现 capability 缺口。
            AppControlError::new("CAPABILITY_UNSUPPORTED", "private inventory"),
            // 绑定 lifecycle 请求。
            &close_request(),
        );
        // 内部 provider 分类必须收敛为公共输入错误。
        assert_eq!(wrong_kind.code, "INVALID_ARGUMENT");
        // 私有 inventory 消息不得离开 lifecycle 边界。
        assert!(!wrong_kind.message.contains("inventory"));
        // 构造 generic verb 与 capability 不匹配的未确认请求。
        let mut mismatched = close_request();
        // 使用错误的 create verb，仍不得绕过 confirmation-first error envelope。
        mismatched.operation = Some("create".to_owned());
        // 投影 Policy 已经先行选择的确认错误。
        let confirmation = project_before_dispatch(
            // 使用稳定确认错误码。
            AppControlError::new("CONFIRMATION_REQUIRED", "confirm"),
            // 绑定明确请求的 Browser Session capability。
            &mismatched,
        );
        // capability identity 必须在 verb 拒绝前保留生命周期详情。
        assert_eq!(
            confirmation.details["capability"],
            capabilities::BROWSER_SESSION_CLOSE
        );
    }
}
