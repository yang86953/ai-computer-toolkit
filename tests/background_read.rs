#![cfg(target_os = "windows")]

// 导入公共服务与只读请求类型。
use ai_computer_toolkit::{
    // 导入统一服务入口。
    AppControlService,
    // 导入命令请求与 verb。
    domain::{CommandRequest, Verb},
};
// 导入通用 JSON 值以递归执行隐私扫描。
use serde_json::Value;

// 列出任何诊断输出都不得公开的原生字段。
const NATIVE_DIAGNOSTIC_FIELDS: &[&str] = &[
    // 禁止 HWND。
    "hwnd",
    // 禁止 PID。
    "processId",
    // 禁止窗口矩形。
    "bounds",
    // 禁止 provider 私有身份。
    "providerId",
    // 禁止原生 handle 别名。
    "nativeHandle",
    // 禁止原生窗口别名。
    "nativeWindow",
    // 禁止原生进程别名。
    "nativeProcessId",
    // 禁止前景 HWND 起点。
    "before",
    // 禁止前景 HWND 终点。
    "after",
    // 禁止启动后原生 PID。
    "launcherProcessId",
];

// 递归判断 JSON 是否包含指定字段名。
fn contains_key(value: &Value, forbidden: &str) -> bool {
    // 按 JSON 值类别递归。
    match value {
        // 对象检查当前键与全部子值。
        Value::Object(object) => object
            // 遍历键值对。
            .iter()
            // 任一命中即返回真。
            .any(|(key, value)| key == forbidden || contains_key(value, forbidden)),
        // 数组递归检查全部元素。
        Value::Array(items) => items
            // 遍历数组元素。
            .iter()
            // 任一子树命中即返回真。
            .any(|value| contains_key(value, forbidden)),
        // 标量没有字段名。
        _ => false,
    }
}

// 核对公共诊断树不含原生字段或 legacy 窗口 ID。
fn assert_diagnostic_private(value: &Value) {
    // 逐项检查稳定禁止字段。
    for field in NATIVE_DIAGNOSTIC_FIELDS {
        // 输出中不得出现原生字段名。
        assert!(
            !contains_key(value, field),
            "native diagnostic field leaked: {field}"
        );
    }
    // 序列化完整结果检查 legacy 目标值。
    let serialized = value.to_string();
    // 禁止旧 window:<HWND>。
    assert!(!serialized.contains("\"window:"));
    // 禁止旧 UIA 窗口目标。
    assert!(!serialized.contains("uia:window:"));
    // 禁止旧 Win32 control 窗口目标。
    assert!(!serialized.contains("win32-control:window:"));
}

// 验证窗口发现通过版本化契约报告前景不变。
#[test]
fn window_discovery_reports_foreground_invariance() -> Result<(), Box<dyn std::error::Error>> {
    // 创建默认 Rust 服务。
    let service = AppControlService::new();
    // 执行只读窗口发现。
    let result = service.execute(CommandRequest::read(Verb::Sessions, "window"))?;
    // 结果必须声明版本化窗口发现 capability。
    assert_eq!(result["capability"], "window.discover@1");
    // window-observation v1 必须直接声明前景未改变。
    assert_eq!(result["foregroundUnchanged"], true);
    // 完成集成验证。
    Ok(())
}

// 验证四个 Windows 诊断别名只发布 opaque 目标与布尔前景事实。
#[test]
fn windows_diagnostic_surfaces_hide_native_targets() -> Result<(), Box<dyn std::error::Error>> {
    // 创建统一 Rust 服务。
    let service = AppControlService::new();
    // 依次验证三个 catalog 公开诊断 surface 的状态和 sessions。
    for surface in ["window", "uia", "desktop"] {
        // 读取状态或 doctor 使用的同一静态结果。
        let status = service.execute(CommandRequest::read(Verb::Status, surface))?;
        // 状态不得公开原生字段。
        assert_diagnostic_private(&status);
        // 构造有界 sessions 请求。
        let mut request = CommandRequest::read(Verb::Sessions, surface);
        // 将实时输出限制为 8 项。
        request.max_items = 8;
        // 执行只读发现。
        let sessions = service.execute(request)?;
        // sessions 不得公开原生字段。
        assert_diagnostic_private(&sessions);
        // 窗口清单不允许 Win32 class 字段。
        assert!(!contains_key(&sessions, "className"));
    }
    // 读取 canonical 窗口目标清单。
    let mut request = CommandRequest::read(Verb::Sessions, "window");
    // 只需一个当前目标完成精确检查。
    request.max_items = 1;
    // 执行窗口发现。
    let sessions = service.execute(request)?;
    // 读取可选首个窗口。
    let Some(session_id) = sessions["sessions"]
        // 要求数组形状。
        .as_array()
        // 读取首项。
        .and_then(|items| items.first())
        // 读取 canonical ID。
        .and_then(|item| item["sessionId"].as_str())
    else {
        // 无可见窗口时 sessions 隐私门禁已经完成。
        return Ok(());
    };
    // 固定复制目标，避免借用跨越后续结果。
    let session_id = session_id.to_owned();
    // window 与 desktop 精确检查都不得公开 Win32 class。
    for surface in ["window", "desktop"] {
        // 构造精确检查请求。
        let mut inspect = CommandRequest::read(Verb::Inspect, surface);
        // 只传 canonical opaque 目标。
        inspect
            // 写入公开目标字段。
            .target
            // 插入精确 ID。
            .insert("sessionId".to_owned(), Value::String(session_id.clone()));
        // 执行当前使用时重新发现。
        let inspected = service.execute(inspect)?;
        // 递归检查原生字段和 legacy ID。
        assert_diagnostic_private(&inspected);
        // Win32 class 不得进入窗口诊断结果。
        assert!(!contains_key(&inspected, "className"));
    }
    // worker 结果由同目录正式 binary 的 launcher 实机门禁覆盖。
    // 完成进程内诊断隐私门禁。
    Ok(())
}

// 验证 Standard Edit 公开只读路径保持静态权限决策与汇总一致。
#[test]
// 该测试只枚举安全元数据，不执行任何写探针或用户应用写入。
fn standard_edit_static_permissions_are_safe_and_counted() -> Result<(), Box<dyn std::error::Error>>
{
    // 创建统一 Rust 服务。
    let service = AppControlService::new();
    // 构造公开上限内的只读 Standard Edit session 请求。
    let mut request = CommandRequest::read(Verb::Sessions, "win32-control");
    // 使用公开最大条数降低测试环境截断概率。
    request.max_items = 4_096;
    // 执行只读权限观察。
    let result = service.execute(request)?;
    // 整个公开树不得泄漏原生身份。
    assert_diagnostic_private(&result);
    // 只读观察必须保持前景不变。
    assert_eq!(result["foreground"]["unchanged"], true);
    // 读取公开 session 数组。
    let sessions = result["sessions"]
        // 要求 schema 数组形状。
        .as_array()
        // 缺失数组时返回测试错误而不是 panic。
        .ok_or_else(|| std::io::Error::other("Standard Edit sessions must be an array"))?;
    // 初始化公开决策计数。
    let mut requires_confirmation = 0_u64;
    // 初始化权限阻塞计数。
    let mut permission_blocked = 0_u64;
    // 初始化不确定计数。
    let mut indeterminate = 0_u64;
    // 逐 session 核对公开 assessment。
    for session in sessions {
        // 读取 assessment 对象。
        let assessment = &session["assessment"];
        // 静态评估永不授权立即执行。
        assert_eq!(assessment["safeToExecuteNow"], false);
        // 所有结果仍要求逐操作确认。
        assert_eq!(assessment["requiresConfirmation"], true);
        // 认证 Standard Edit 路线不要求前台。
        assert_eq!(assessment["foregroundRequired"], false);
        // 只读评估不得执行主动写探针。
        assert_eq!(assessment["activeWriteProbePerformed"], false);
        // 按封闭决策核对原因并累计计数。
        match assessment["decision"].as_str() {
            // 同级或较低完整性只报告需要确认。
            Some("requires-confirmation") => {
                // 原因必须表示未观察到静态完整性阻塞。
                assert_eq!(
                    assessment["permissionRelation"],
                    "no-static-integrity-block-observed"
                );
                // 累计需要确认项。
                requires_confirmation += 1;
            }
            // 较高完整性或元数据权限拒绝报告阻塞。
            Some("permission-blocked") => {
                // 读取封闭权限原因。
                let relation = assessment["permissionRelation"].as_str();
                // 只接受两个已发布的权限阻塞原因。
                assert!(matches!(
                    relation,
                    Some("target-higher-integrity" | "target-metadata-permission-blocked")
                ));
                // 累计权限阻塞项。
                permission_blocked += 1;
            }
            // 元数据或完整性未知保持不确定。
            Some("indeterminate") => {
                // 不确定决策只能发布未知关系。
                assert_eq!(assessment["permissionRelation"], "unknown");
                // 累计不确定项。
                indeterminate += 1;
            }
            // 未知决策违反封闭 schema。
            other => {
                return Err(
                    std::io::Error::other(format!("unexpected decision: {other:?}")).into(),
                );
            }
        }
    }
    // 顶层 count 必须与公开数组长度一致。
    assert_eq!(result["count"].as_u64(), Some(sessions.len() as u64));
    // 三类决策必须覆盖全部公开 session。
    assert_eq!(
        requires_confirmation + permission_blocked + indeterminate,
        sessions.len() as u64
    );
    // 读取真实 Module 状态汇总。
    let status = service.execute(CommandRequest::read(Verb::Status, "win32-control"))?;
    // 状态结果也不得泄漏原生身份。
    assert_diagnostic_private(&status);
    // 读取同源安全汇总数据。
    let data = &status["data"];
    // 状态查询必须证明主动写探针为零。
    assert_eq!(data["activeWriteProbes"], 0);
    // 读取需要确认计数并要求整数形状。
    let status_requires_confirmation = data["requiresConfirmationCount"]
        // 读取无符号计数。
        .as_u64()
        // 缺失字段必须使集成门禁失败。
        .ok_or_else(|| std::io::Error::other("status omitted requiresConfirmationCount"))?;
    // 读取权限阻塞计数并要求整数形状。
    let status_permission_blocked = data["permissionBlockedCount"]
        // 读取无符号计数。
        .as_u64()
        // 缺失字段必须使集成门禁失败。
        .ok_or_else(|| std::io::Error::other("status omitted permissionBlockedCount"))?;
    // 读取不确定计数并要求整数形状。
    let status_indeterminate = data["indeterminateCount"]
        // 读取无符号计数。
        .as_u64()
        // 缺失字段必须使集成门禁失败。
        .ok_or_else(|| std::io::Error::other("status omitted indeterminateCount"))?;
    // 读取控件总数并要求整数形状。
    let control_count = data["controlCount"]
        // 读取无符号计数。
        .as_u64()
        // 缺失字段必须使集成门禁失败。
        .ok_or_else(|| std::io::Error::other("status omitted controlCount"))?;
    // 合并三类封闭决策计数。
    let status_total = status_requires_confirmation
        // 加上权限阻塞项。
        + status_permission_blocked
        // 加上不确定项。
        + status_indeterminate;
    // 三类状态计数必须完整覆盖控件总数。
    assert_eq!(status_total, control_count);
    // 未截断时逐类状态计数必须与 session 事实一致。
    if result["truncated"] == false {
        // 核对需要确认数量。
        assert_eq!(status_requires_confirmation, requires_confirmation);
        // 核对权限阻塞数量。
        assert_eq!(status_permission_blocked, permission_blocked);
        // 核对不确定数量。
        assert_eq!(status_indeterminate, indeterminate);
    }
    // 完成静态权限公开路径门禁。
    Ok(())
}
