#![cfg(target_os = "windows")]

// 导入公开控制服务、请求模型和读取动词。
use ai_computer_toolkit::{
    // 导入生产组合根。
    AppControlService,
    // 导入领域请求、隔离要求与动词。
    domain::{CommandRequest, IsolationRequirement, Verb},
};
// 导入 provider-neutral JSON 容器和值构造器。
use serde_json::{Map, Value, json};

// 构造确认后的 canonical 媒体控制请求。
fn confirmed_control_request() -> CommandRequest {
    // 创建封闭字段的控制请求。
    let mut request = CommandRequest {
        // 选择变更动词。
        verb: Verb::Run,
        // 选择媒体 capability surface。
        app: "media-session".to_owned(),
        // 选择目录中存在的控制操作。
        operation: Some("toggle-play-pause".to_owned()),
        // 初始化 opaque 目标容器。
        target: Map::new(),
        // 控制操作不接受额外参数。
        args: Map::new(),
        // 保持有界读取默认值。
        max_items: 50,
        // 保持有界树深度默认值。
        max_depth: 4,
        // 通过 confirmation-first 门禁。
        confirmed: true,
        // 媒体后台控制不申请前台同意。
        foreground_consent: false,
        // 使用标准隔离要求以验证生产 provider 选择点。
        isolation_requirement: IsolationRequirement::Standard,
    };
    // 提供 canonical opaque 媒体目标。
    request
        // 访问目标字段。
        .target
        // 使用不指向真实会话的稳定测试值。
        .insert("sessionId".to_owned(), json!("s2:m:0000000000000000"));
    // 返回完整请求。
    request
}

// 阶段六 worker 认证前，所有媒体读取都必须在 provider 解析点失败闭合。
#[test]
// 覆盖状态、会话发现和精确读取三个只读入口。
fn uncertified_media_session_reads_are_unavailable() -> Result<(), Box<dyn std::error::Error>> {
    // 构造只装配已认证 adapter 的生产 System。
    let service = AppControlService::new();
    // 构造精确目标读取请求。
    let mut inspect = CommandRequest::read(Verb::Inspect, "media-session");
    // 使用 canonical opaque 媒体目标，避免旧 media 前缀重新取得兼容权。
    inspect
        // 访问目标字段。
        .target
        // 固定无真实会话含义的测试目标。
        .insert("sessionId".to_owned(), json!("s2:m:0000000000000000"));
    // 覆盖无目标诊断、会话目录和精确读取三条入口。
    let requests = [
        // 状态入口不得访问主进程 WinRT。
        CommandRequest::read(Verb::Status, "media-session"),
        // 会话目录不得枚举 GSMTC。
        CommandRequest::read(Verb::Sessions, "media-session"),
        // 精确读取不得解析媒体 provider。
        inspect,
    ];
    // 对每条入口验证同一结构化不可用语义。
    for request in requests {
        // 执行请求并拒绝任何意外成功。
        let error = match service.execute(request) {
            // 未认证路径成功属于安全回归。
            Ok(_) => return Err(std::io::Error::other("uncertified media read must fail").into()),
            // 保存结构化失败供断言。
            Err(error) => error,
        };
        // 已发布但未装配认证实现时必须报告后台不可用。
        assert_eq!(error.code, "BACKGROUND_OPERATION_UNAVAILABLE");
    }
    // 返回测试成功。
    Ok(())
}

// doctor 必须聚合未认证媒体的结构化错误，不得伪造成功 provider。
#[test]
// 固定 CLI 聚合信封、退出码与唯一嵌套错误码。
fn uncertified_media_doctor_preserves_aggregated_unavailability()
-> Result<(), Box<dyn std::error::Error>> {
    // 通过公开 CLI 路由执行精确媒体诊断。
    let output = ai_computer_toolkit::cli::run(vec![
        // 选择聚合诊断命令。
        "doctor".to_owned(),
        // 限定为未认证媒体 surface。
        "media-session".to_owned(),
    ])?;
    // doctor 聚合失败使用公开通用失败退出码二。
    assert_eq!(output.exit_code, 2);
    // 顶层不得把嵌套错误伪装成成功。
    assert_eq!(output.json["ok"], false);
    // 读取固定 results 数组。
    let results = output.json["results"]
        // 聚合结果必须保持数组形状。
        .as_array()
        // 缺失数组属于契约回归。
        .ok_or_else(|| std::io::Error::other("media doctor omitted results"))?;
    // 精确 surface 只允许一个诊断结果。
    assert_eq!(results.len(), 1);
    // 唯一结果必须明确失败。
    assert_eq!(results[0]["ok"], false);
    // 嵌套错误保持稳定后台不可用语义。
    assert_eq!(
        // 读取公开错误码。
        results[0]["error"]["code"],
        // 禁止用成功或其他错误掩盖未认证 provider。
        "BACKGROUND_OPERATION_UNAVAILABLE"
    );
    // 返回测试成功。
    Ok(())
}

// 确认只授权一次尝试，不得让未认证媒体实现重新进入生产组合根。
#[test]
// 验证确认后的控制仍在 provider 选择点失败闭合。
fn confirmed_media_session_control_remains_unavailable() -> Result<(), Box<dyn std::error::Error>> {
    // 构造生产 System。
    let service = AppControlService::new();
    // 执行请求并拒绝任何意外控制成功。
    let error = match service.execute(confirmed_control_request()) {
        // 已确认也不能装配未认证 provider。
        Ok(_) => return Err(std::io::Error::other("uncertified media control must fail").into()),
        // 保存结构化失败供断言。
        Err(error) => error,
    };
    // provider 解析点必须报告已发布能力缺少认证实现。
    assert_eq!(error.code, "BACKGROUND_OPERATION_UNAVAILABLE");
    // 返回测试成功。
    Ok(())
}

// 阶段六认证前，媒体生产入口必须固定经过 Rust Policy 并失败闭合。
#[test]
// 核对版本化迁移契约与 launcher 静态路由。
fn uncertified_media_launcher_route_is_rust_fail_closed() -> Result<(), Box<dyn std::error::Error>>
{
    // 解析媒体控制迁移契约。
    let policy: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "contracts/media-control-migration-policy.json"
    ))?;
    // 嵌入生产 launcher 以防止 opaque 目标重新触发 C++。
    let launcher = include_str!("../tools/windows/Invoke-ComputerControl.ps1");
    // 契约必须禁止历史 C++ 执行。
    assert_eq!(policy["cppExecutionEnabled"], false);
    // 契约必须禁止未认证 Rust provider 执行。
    assert_eq!(policy["rustPublicRouteEnabled"], false);
    // launcher 仍需进入 Rust Policy 以保留确认优先语义。
    assert_eq!(policy["launcherRoute"], "rust-policy-fail-closed");
    // 生产 launcher 必须只定位 Rust runtime。
    assert!(launcher.contains("'target\\debug'"));
    // 生产入口不得继续定位历史 C++ 构建目录。
    assert!(!launcher.contains("build\\cpp-main"));
    // 生产入口不得继续命名历史 C++ executable。
    assert!(!launcher.contains("ai-computer-toolkit-cpp.exe"));
    // 已删除的语言选择状态不得恢复。
    assert!(!launcher.contains("$useCpp"));
    // 缺 runtime 只允许公开 Rust 兼容错误码。
    assert!(!launcher.contains("CPP_RUNTIME_UNAVAILABLE"));
    // 返回安全联锁一致成功。
    Ok(())
}
