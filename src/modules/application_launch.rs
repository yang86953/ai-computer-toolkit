//! 动态已安装应用认证与精确启动 Module。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "application_launch_error.rs"]
mod error_code;

// 导入 JSON 构造接口。
use serde_json::{Value, json};

// 导入当前 Module 私有封闭错误码。
use error_code::ApplicationLaunchErrorCode;

// 导入只读目录、窄 Shell Component 与结构化边界。
use crate::{
    // 组合现有只读 Adapter 与单一启动 Component。
    adapters::{
        // 重新枚举 Registry、AppsFolder 与固定 Start Menu。
        installed_applications::{InstalledApplicationRecord, enumerate_installed_applications},
        // 只把私有 parsing identity 交给窄启动 Component。
        shell_application_launch,
        // 读取前景仅用于公开布尔影响证据。
        windows::foreground_hwnd,
    },
    // 使用单一 capability 注册表 ID。
    capabilities,
    // 严格验证 canonical application 目标类别。
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    // 返回稳定 JSON over stdio 错误和请求。
    domain::{AppResult, CommandRequest},
};

// 固定执行重新发现的完整应用硬上限。
const MAXIMUM_APPLICATIONS: usize = 4096;

// 表示当前目录解析的零、唯一或歧义状态。
enum ApplicationMatch {
    // 表示当前目录没有该 opaque 目标。
    Missing,
    // 表示当前目录唯一命中。
    Unique(InstalledApplicationRecord),
}

// 判断应用记录是否携带认证 Shell identity。
pub(crate) fn is_launchable(application: &InstalledApplicationRecord) -> bool {
    // 只有两个封闭 Shell 来源提供的非空私有 identity 可启动。
    application
        // 检查公开来源集合。
        .discovery_sources
        // 只读遍历来源。
        .iter()
        // 要求明确 AppsFolder 或固定 Start Menu 来源。
        .any(|source| matches!(source.as_str(), "shell-apps-folder" | "shell-start-menu"))
        // 同时要求私有 parsing identity 非空。
        && !application.launch_identity.is_empty()
}

// 把当前应用记录投影为不含 provider identity 的公开检查结果。
fn public_application(application: &InstalledApplicationRecord) -> Value {
    // 只构造契约允许的普通字段。
    json!({
        // 输出 canonical s2:a。
        "sessionId": application.session_id,
        // 输出稳定已安装应用类别。
        "targetKind": "installed-application",
        // 输出公开目录显示名。
        "displayName": application.display_name,
        // 输出可用版本文本。
        "version": application.version,
        // 输出目录声明发布者。
        "publisher": application.publisher,
        // 输出公开来源，不输出 AUMID 或 parsing identity。
        "discoverySources": application.discovery_sources,
        // 输出认证能力状态。
        "launchCapability": if is_launchable(application) {
            // Shell identity 已经认证但执行仍需逐操作确认。
            "available-confirmed"
        } else {
            // 缺失安全 Shell identity 时保持不可用。
            "unavailable"
        },
    })
}

// 从给定记录中按 canonical ID 执行碰撞安全解析。
fn resolve_records(
    // 接收完整当前目录记录。
    records: Vec<InstalledApplicationRecord>,
    // 接收调用方已知 opaque ID。
    session_id: &str,
) -> AppResult<ApplicationMatch> {
    // 保存全部精确命中以检测哈希碰撞或目录重复。
    let mut matches = records
        // 消费本次只读快照。
        .into_iter()
        // 只保留字节级相同 canonical ID。
        .filter(|application| application.session_id == session_id)
        // 收集完整候选而不任取首项。
        .collect::<Vec<_>>();
    // 多命中必须 fail closed。
    if matches.len() > 1 {
        // 返回不含候选内容的稳定歧义错误。
        return Err(ApplicationLaunchErrorCode::AmbiguousTarget.error(
            // 不公开显示名、AUMID 或 Shell identity。
            "The opaque application target resolves to multiple current records.",
        ));
    }
    // 唯一候选安全移出集合。
    if let Some(application) = matches.pop() {
        // 返回唯一当前记录。
        return Ok(ApplicationMatch::Unique(application));
    }
    // 零命中保持显式状态。
    Ok(ApplicationMatch::Missing)
}

// 重新枚举完整动态目录并解析精确应用。
fn resolve_current(session_id: &str) -> AppResult<ApplicationMatch> {
    // 先拒绝 host、process、window 与旧目标格式。
    if OpaqueTargetId::parse(session_id)
        // 只接受 canonical application kind。
        .is_none_or(|target| target.kind() != OpaqueTargetKind::Application)
    {
        // 非应用目标不属于本 Module。
        return Ok(ApplicationMatch::Missing);
    }
    // 读取完整有界动态目录。
    let inventory = enumerate_installed_applications(MAXIMUM_APPLICATIONS);
    // 来源未完整且目标未命中时不能把缺口误报为 stale。
    let complete = inventory.complete;
    // 对当前记录执行精确碰撞安全解析。
    let resolved = resolve_records(inventory.records, session_id)?;
    // 不完整目录中的零命中必须 fail closed。
    if !complete && matches!(resolved, ApplicationMatch::Missing) {
        // 返回稳定目录不可用错误。
        return Err(
            ApplicationLaunchErrorCode::BackgroundOperationUnavailable.error(
                // 不说明哪个私有来源失败。
                "The installed application inventory is incomplete.",
            ),
        );
    }
    // 返回当前解析状态。
    Ok(resolved)
}

// 返回当前 host provider 的只读目录状态。
pub(crate) fn status() -> AppResult<Value> {
    // 重新枚举动态应用目录。
    let inventory = enumerate_installed_applications(MAXIMUM_APPLICATIONS);
    // 统计带认证 Shell identity 的目标。
    let launchable = inventory
        // 遍历当前记录。
        .records
        // 创建只读迭代器。
        .iter()
        // 只保留可启动记录。
        .filter(|application| is_launchable(application))
        // 取得稳定计数。
        .count();
    // 返回不含目标或原生 identity 的聚合状态。
    Ok(json!({
        // 标记只读状态成功。
        "ok": true,
        // 声明动态来源而不是静态白名单。
        "model": "dynamic-installed-application-inventory",
        // 输出完整当前记录数。
        "discoveredApplications": inventory.records.len(),
        // 输出单独认证目标数。
        "launchableApplications": launchable,
        // 标记目录是否自然完整。
        "complete": inventory.complete,
        // 输出逐来源可用性。
        "sources": {
            // 输出传统 Registry 来源状态。
            "registry": inventory.registry_source_available,
            // 输出公开 Shell AppsFolder 来源状态。
            "shellAppsFolder": inventory.shell_source_available,
            // 输出固定 Start Menu Programs 来源状态。
            "shellStartMenu": inventory.start_menu_source_available,
        },
        // 明确状态调用没有启动应用。
        "launchesDispatched": 0,
    }))
}

// 重新发现 session 并返回其当前 capability 集合。
pub(crate) fn session_capabilities(session_id: &str) -> AppResult<Option<Vec<String>>> {
    // 解析当前动态目录。
    match resolve_current(session_id)? {
        // 零命中表示不属于本 provider。
        ApplicationMatch::Missing => Ok(None),
        // 唯一命中按认证状态发布能力。
        ApplicationMatch::Unique(application) => {
            // 可启动目标发布唯一 application.open。
            if is_launchable(&application) {
                // 返回当前认证 capability。
                return Ok(Some(vec![capabilities::APPLICATION_OPEN.to_owned()]));
            }
            // 不可启动目标仍被动态目录识别，但没有执行能力。
            Ok(Some(Vec::new()))
        }
    }
}

// 精确检查当前已安装应用的公开元数据与认证状态。
pub(crate) fn inspect(session_id: &str) -> AppResult<Value> {
    // 解析当前动态目录。
    match resolve_current(session_id)? {
        // 零命中按使用时 stale 返回。
        ApplicationMatch::Missing => Err(ApplicationLaunchErrorCode::StaleSession.error(
            // 不回显 opaque ID 之外的任何私有信息。
            "The opaque application target no longer resolves.",
        )),
        // 唯一命中只输出安全公开投影。
        ApplicationMatch::Unique(application) => Ok(json!({
            // 标记检查成功。
            "ok": true,
            // 标记无副作用。
            "readOnly": true,
            // 输出安全应用对象。
            "application": public_application(&application),
        })),
    }
}

// 验证 application.open 不接受任何公共启动参数。
fn validate_input(request: &CommandRequest) -> AppResult<()> {
    // 缺失 input 表示空 provider-neutral 输入。
    let Some(input) = request.args.get("input") else {
        // 允许继续精确目标执行。
        return Ok(());
    };
    // 只接受空 JSON object。
    if input.as_object().is_some_and(serde_json::Map::is_empty) {
        // 空输入不会承载 path、argv 或 identity。
        return Ok(());
    }
    // 其他输入形状全部拒绝。
    Err(ApplicationLaunchErrorCode::InvalidArgument.error(
        // 明确 capability 不接受公共启动参数。
        "application.open@1 accepts an empty input object only.",
    ))
}

// 执行确认后的精确动态应用启动。
pub(crate) fn execute(session_id: &str, request: &CommandRequest) -> AppResult<Value> {
    // 在重新发现或 Shell 调用前拒绝公共路径与参数。
    validate_input(request)?;
    // 重新发现精确应用目标。
    let application = match resolve_current(session_id)? {
        // 零命中表示目标已过期。
        ApplicationMatch::Missing => {
            // 返回稳定 stale 错误。
            return Err(ApplicationLaunchErrorCode::StaleSession.error(
                // 不回显私有目录事实。
                "The opaque application target no longer resolves.",
            ));
        }
        // 唯一命中移交认证检查。
        ApplicationMatch::Unique(application) => application,
    };
    // 缺失任一认证 Shell 来源 identity 时禁止执行。
    if !is_launchable(&application) {
        // 返回能力不可用且不降级到路径。
        return Err(ApplicationLaunchErrorCode::CapabilityUnavailable.error(
            // 不说明 Registry 或 Shell 私有记录。
            "The exact installed application has no certified Shell launch route.",
        ));
    }
    // 记录启动调度前的私有前景句柄。
    let foreground_before = foreground_hwnd();
    // 只把私有 parsing identity 传给窄 Shell Component。
    let evidence = shell_application_launch::launch(&application.launch_identity)?;
    // 记录 Shell 返回后的私有前景句柄。
    let foreground_after = foreground_hwnd();
    // 返回 provider-neutral 启动证据。
    Ok(json!({
        // 回显调用方已知的 canonical 目标。
        "targetId": application.session_id,
        // 输出 Shell 调度成功事实。
        "launchDispatched": evidence.dispatched,
        // 只输出是否观察到进程句柄，不输出 PID 或 handle。
        "processObserved": evidence.process_observed,
        // 输出真实前景是否未变化。
        "foregroundUnchanged": foreground_before == foreground_after,
        // 启动的应用可以自行呈现前景。
        "foregroundMayChange": true,
        // 明确公共结果不含 native identity。
        "nativeIdentifiersExposed": false,
        // 明确公共结果不含 runtime path。
        "runtimePathExposed": false,
        // 供 facade 输出稳定兼容形状。
        "compatibilityShape": "secured-opaque-application-launch-v1",
    }))
}

// 声明不启动真实应用的纯解析与隐私测试。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造器与请求类型。
    use serde_json::json;

    // 导入父 Module 私有纯函数。
    use super::{
        ApplicationMatch, is_launchable, public_application, resolve_records, validate_input,
    };
    // 导入记录和请求类型。
    use crate::{
        adapters::installed_applications::InstalledApplicationRecord, domain::CommandRequest,
    };

    // 构造不接触真实目录的应用夹具。
    fn record(session_id: &str, identity: &str) -> InstalledApplicationRecord {
        // 返回最小 Shell 来源记录。
        InstalledApplicationRecord {
            // 保存合成 canonical ID。
            session_id: session_id.to_owned(),
            // 保存公开显示名。
            display_name: "Owned fixture".to_owned(),
            // 保存公开版本。
            version: "1.0".to_owned(),
            // 保存公开发布者。
            publisher: "Toolkit".to_owned(),
            // 标记 Shell AppsFolder 来源。
            discovery_sources: vec!["shell-apps-folder".to_owned()],
            // 不需要进程匹配提示。
            process_match_hints: Vec::new(),
            // 保存仅供测试的私有 identity。
            launch_identity: identity.to_owned(),
        }
    }

    // 验证认证必须同时具有 Shell 来源和私有 identity。
    #[test]
    fn launchability_is_closed() {
        // 非空 Shell identity 可进入确认型能力。
        assert!(is_launchable(&record(
            "s2:a:0000000000000001",
            "owned-private"
        )));
        // 空 identity 保持不可用。
        assert!(!is_launchable(&record("s2:a:0000000000000001", "")));
        // 构造固定 Start Menu 认证路由。
        let mut start_menu = record("s2:a:0000000000000002", "owned-start-menu");
        // 只保留固定 Start Menu 来源。
        start_menu.discovery_sources = vec!["shell-start-menu".to_owned()];
        // 固定来源与非空私有 identity 同样可进入确认型能力。
        assert!(is_launchable(&start_menu));
        // 构造只有传统卸载来源的记录。
        let mut registry = record("s2:a:0000000000000003", "untrusted-route");
        // 移除全部认证 Shell 来源。
        registry.discovery_sources = vec!["registry-uninstall".to_owned()];
        // 私有字符串本身不得绕过封闭来源认证。
        assert!(!is_launchable(&registry));
    }

    // 验证碰撞不会任取首项。
    #[test]
    fn duplicate_opaque_application_is_ambiguous() {
        // 构造两个相同公开 ID 的私有记录。
        let records = vec![
            // 添加第一个候选。
            record("s2:a:0000000000000001", "first"),
            // 添加第二个候选。
            record("s2:a:0000000000000001", "second"),
        ];
        // 执行纯解析并取得错误。
        let error = resolve_records(records, "s2:a:0000000000000001").err();
        // 必须返回稳定歧义错误。
        assert_eq!(error.map(|value| value.code), Some("AMBIGUOUS_TARGET"));
    }

    // 验证公开投影不包含 Shell identity。
    #[test]
    fn public_projection_hides_launch_identity() {
        // 投影带私有 identity 的记录。
        let value = public_application(&record(
            // 使用 canonical 合成目标。
            "s2:a:0000000000000001",
            // 使用可检测私有值。
            "secret-shell-identity",
        ));
        // 序列化公开结果。
        let serialized = value.to_string();
        // 禁止私有 identity 进入结果。
        assert!(!serialized.contains("secret-shell-identity"));
        // 公开状态必须表明确认型能力可用。
        assert_eq!(value["launchCapability"], "available-confirmed");
    }

    // 验证公共输入不能携带路径或参数。
    #[test]
    fn public_launch_input_must_be_empty() {
        // 构造普通读取请求作为输入容器。
        let mut request = CommandRequest::read(crate::domain::Verb::Run, "app");
        // 空 object 必须允许。
        request.args.insert("input".to_owned(), json!({}));
        // 验证空输入通过。
        assert!(validate_input(&request).is_ok());
        // 插入禁止的路径字段。
        request
            .args
            .insert("input".to_owned(), json!({"path": "forbidden"}));
        // 非空输入必须被拒绝。
        assert_eq!(
            // 读取结构化错误码。
            validate_input(&request).err().map(|value| value.code),
            // 使用稳定参数错误码。
            Some("INVALID_ARGUMENT")
        );
    }

    // 验证唯一记录解析不会复制或变换公开 ID。
    #[test]
    fn unique_application_resolves_exactly() {
        // 解析唯一合成记录。
        let resolved = resolve_records(
            // 提供唯一当前记录。
            vec![record("s2:a:0000000000000001", "owned-private")],
            // 使用相同 canonical ID。
            "s2:a:0000000000000001",
        );
        // 必须得到唯一分支。
        assert!(matches!(resolved, Ok(ApplicationMatch::Unique(_))));
    }
}
