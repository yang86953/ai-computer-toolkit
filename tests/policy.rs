#![cfg(target_os = "windows")]

use ai_computer_toolkit::{
    // 导入 service 与请求类型。
    AppControlService,
    // 导入请求及强类型隔离要求。
    domain::{CommandRequest, ExecutionRealm, IsolationRequirement},
};
// 导入 JSON 对象、构造器和通用值。
use serde_json::{Map, Value, json};

fn run_request(app: &str, operation: &str) -> CommandRequest {
    CommandRequest {
        verb: ai_computer_toolkit::domain::Verb::Run,
        app: app.to_owned(),
        operation: Some(operation.to_owned()),
        target: Map::new(),
        args: Map::new(),
        max_items: 50,
        max_depth: 4,
        confirmed: false,
        foreground_consent: false,
        // 测试夹具默认保持标准策略。
        isolation_requirement: IsolationRequirement::Standard,
    }
}

fn confirmed_app_apply(capability: &str, input: serde_json::Value) -> CommandRequest {
    let mut request = run_request("app", "apply");
    request.confirmed = true;
    request
        .target
        .insert("sessionId".to_owned(), json!("s1:c2:0000000000000000"));
    request
        .args
        .insert("capability".to_owned(), json!(capability));
    request.args.insert("input".to_owned(), input);
    request
}

#[test]
fn unknown_app_is_denied_before_adapter_dispatch() -> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let error = match service.execute(run_request("unknown-app", "do-anything")) {
        Ok(_) => return Err(std::io::Error::other("unknown app must be denied").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "CAPABILITY_GAP");
    assert_eq!(error.details["app"], "unknown-app");
    assert!(error.details["availableApps"].is_array());
    Ok(())
}

#[test]
fn operation_absent_from_public_catalog_is_a_capability_gap()
-> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let request = run_request("desktop", "uncataloged-operation");
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("uncataloged operation must fail").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "CAPABILITY_GAP");
    assert_eq!(error.details["app"], "desktop");
    assert_eq!(error.details["operation"], "uncataloged-operation");
    assert!(error.details["availableOperations"].is_array());
    Ok(())
}

#[test]
fn mutating_operation_requires_confirmation() -> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let mut request = run_request("browser", "screenshot");
    request
        .target
        .insert("url".to_owned(), json!("https://example.com"));
    request
        .args
        .insert("path".to_owned(), json!("C:\\temp\\page.png"));
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("confirmation must be required").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    Ok(())
}

// 验证公开目录不会新增绕过基础确认门禁的变更操作。
#[test]
// 遍历目录并在解析目标与参数前验证确认错误。
fn all_catalog_mutations_require_confirmation_before_request_fields()
-> Result<(), Box<dyn std::error::Error>> {
    // 逐应用检查公开操作描述与 System 策略的一致性。
    for app in ai_computer_toolkit::catalog::apps() {
        // 逐操作覆盖未来新增的目录项，避免只测试固定样例。
        for operation in app.operations {
            // 变更属性必须与基础确认要求保持双向一致。
            assert_eq!(
                operation.requires_confirmation, operation.mutates,
                "公开操作 {}.{} 的变更与确认声明不一致",
                app.id, operation.operation
            );
            // 只对变更操作验证运行时的确认优先错误。
            if !operation.mutates {
                // 读取操作已由上面的双向不变量保证不要求变更确认。
                continue;
            }
            // 构造不含目标与参数的最小请求，证明字段解析尚未发生。
            let mut request = run_request(app.id, operation.operation);
            // 前台许可是独立门禁，此处显式满足它以单独检验基础确认。
            request.foreground_consent = true;
            // 直接调用 System 策略，排除 provider 与领域 Module 的影响。
            let error = match ai_computer_toolkit::policy::validate(&request) {
                // 意外通过说明该公开变更操作绕过了确认门禁。
                Ok(_) => return Err(std::io::Error::other("公开变更操作必须要求确认").into()),
                // 保存结构化错误以核对稳定错误码。
                Err(error) => error,
            };
            // 未确认错误必须先于缺失目标、参数与 provider 解析返回。
            assert_eq!(
                error.code, "CONFIRMATION_REQUIRED",
                "公开操作 {}.{} 未保持确认优先",
                app.id, operation.operation
            );
        }
    }
    // 所有公开目录项均满足确认优先不变量。
    Ok(())
}

#[test]
fn desktop_screenshot_requires_confirmation_and_exact_session()
-> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let path = std::env::temp_dir().join("ai-computer-toolkit-policy-screenshot.png");
    let mut request = run_request("desktop", "screenshot");
    request
        .target
        .insert("sessionId".to_owned(), json!("window:1"));
    request.args.insert("path".to_owned(), json!(path));
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("confirmation must be required").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");

    let mut request = run_request("desktop", "screenshot");
    request.confirmed = true;
    request.args.insert(
        "path".to_owned(),
        json!(std::env::temp_dir().join("ai-computer-toolkit-policy-screenshot.png")),
    );
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("session id must be required").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "INVALID_ARGUMENT");
    Ok(())
}

#[test]
fn desktop_screenshot_stays_on_background_policy_path() -> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let mut request = run_request("desktop", "screenshot");
    request.confirmed = true;
    request
        .target
        .insert("sessionId".to_owned(), json!("window:1"));
    request.args.insert(
        "path".to_owned(),
        json!(std::env::temp_dir().join("ai-computer-toolkit-policy-screenshot.png")),
    );
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("unknown window must fail").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "TARGET_NOT_FOUND");
    Ok(())
}

#[test]
fn desktop_screenshot_refuses_unconfirmed_overwrite() -> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let path = std::env::temp_dir().join(format!(
        "ai-computer-toolkit-overwrite-{}.png",
        std::process::id()
    ));
    std::fs::write(&path, b"existing")?;
    let mut request = run_request("desktop", "screenshot");
    request.confirmed = true;
    request
        .target
        .insert("sessionId".to_owned(), json!("window:1"));
    request.args.insert("path".to_owned(), json!(&path));
    let result = service.execute(request);
    let _ = std::fs::remove_file(&path);
    let error = match result {
        Ok(_) => return Err(std::io::Error::other("overwrite must be denied").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "OVERWRITE_CONFIRMATION_REQUIRED");
    Ok(())
}

#[test]
fn notepad_write_requires_confirmation() -> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let mut request = run_request("notepad", "open-and-write-text");
    request.args.insert("text".to_owned(), json!("后台测试"));
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("confirmation must be required").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    Ok(())
}

#[test]
fn exact_session_id_is_required_for_win32_write() -> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let mut request = run_request("win32-control", "set-text");
    request.confirmed = true;
    request.args.insert("text".to_owned(), json!("Hello"));
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("session id must be required").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "INVALID_ARGUMENT");
    Ok(())
}

#[test]
fn foreground_operation_requires_explicit_consent() -> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let mut request = run_request("desktop", "press-key");
    request.confirmed = true;
    request
        .target
        .insert("sessionId".to_owned(), json!("window:1"));
    request.args.insert("key".to_owned(), json!("ENTER"));
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("consent must be required").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "FOREGROUND_CONSENT_REQUIRED");
    Ok(())
}

#[test]
fn foreground_consent_reaches_the_target_resolution_gate() -> Result<(), Box<dyn std::error::Error>>
{
    let service = AppControlService::new();
    let mut request = run_request("desktop", "press-key");
    request.confirmed = true;
    request.foreground_consent = true;
    request
        .target
        .insert("sessionId".to_owned(), json!("window:1"));
    request.args.insert("key".to_owned(), json!("ENTER"));
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("unknown window must fail").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "TARGET_NOT_FOUND");
    Ok(())
}

#[test]
fn media_session_operation_requires_confirmation() -> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let mut request = run_request("media-session", "toggle-play-pause");
    request
        .target
        // 使用阶段六公开契约规定的 canonical opaque 目标。
        .insert("sessionId".to_owned(), json!("s2:m:0000000000000000"));
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("confirmation must be required").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    Ok(())
}

#[test]
fn operation_methods_are_discoverable() -> Result<(), Box<dyn std::error::Error>> {
    let methods = AppControlService::new().methods(Some("media-session"))?;
    assert_eq!(
        methods["methods"][0]["availability"],
        "candidate-not-certified"
    );
    let descriptor = AppControlService::new().describe("desktop", Some("screenshot"))?;
    assert_eq!(
        descriptor["descriptor"]["methods"][0],
        "windows-graphics-capture"
    );
    assert_eq!(descriptor["descriptor"]["background_policy"], "best-effort");
    Ok(())
}

// 验证 ComputerControlSystem 公共查询拒绝保持既有 envelope。
#[test]
fn system_query_errors_keep_public_envelope() -> Result<(), Box<dyn std::error::Error>> {
    // 构造不持有外部生命周期的 System 协调器。
    let service = AppControlService::new();
    // 汇总 app、operation 与 method 的全部公共拒绝入口。
    let cases = [
        // catalog 必须保持未知 app 消息。
        (
            service.catalog(Some("missing-catalog-app")),
            "未知应用 'missing-catalog-app'.",
        ),
        // describe 必须保持未知 app 消息。
        (
            service.describe("missing-descriptor-app", None),
            "未知应用 'missing-descriptor-app'.",
        ),
        // describe 必须保持未知 operation 消息。
        (
            service.describe("desktop", Some("missing-operation")),
            "未知认证操作 'desktop.missing-operation'.",
        ),
        // methods 必须保持未知 method 消息。
        (
            service.methods(Some("missing-method")),
            "未知操作方式 'missing-method'.",
        ),
    ];
    // 逐项验证公开错误码、消息与空详情。
    for (result, expected_message) in cases {
        // 把意外成功转为明确测试失败。
        let error = result
            // 提取预期的 System 查询拒绝。
            .err()
            // 报告未按契约拒绝的查询。
            .ok_or_else(|| std::io::Error::other("system query unexpectedly succeeded"))?;
        // 所有公共查询拒绝继续使用稳定参数错误码。
        assert_eq!(error.code, "INVALID_ARGUMENT");
        // 每个入口的既有消息必须逐字保持。
        assert_eq!(error.message, expected_message);
        // 普通 System 查询拒绝不得制造额外详情。
        assert!(error.details.is_null());
    }
    // 报告所有公共入口均通过契约核对。
    Ok(())
}

#[test]
fn unified_app_facade_is_capability_driven() -> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let catalog = service.catalog(Some("app"))?;
    let operations = catalog["apps"][0]["operations"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("app operations must be an array"))?;
    let names = operations
        .iter()
        .filter_map(|operation| operation["operation"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            // provider-neutral 只读 capability 通过独立 verb 调度。
            "read",
            "create",
            "apply",
            "save",
            "export",
            "close",
            "screenshot",
            "record"
        ]
    );
    assert!(catalog.to_string().contains("opaque-session"));
    assert!(
        !service
            .catalog(None)?
            .to_string()
            .contains("\"id\":\"photoshop\"")
    );
    Ok(())
}

#[test]
fn desktop_record_has_bounded_token_efficient_defaults() -> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let mut request = run_request("desktop", "record");
    request.confirmed = true;
    request
        .target
        // 正式 Rust 录制只接受 canonical s2:w。
        .insert("sessionId".to_owned(), json!("s2:w:0000000000000000"));
    request.args.insert(
        "path".to_owned(),
        json!(std::env::temp_dir().join("ai-computer-toolkit-policy-record.mp4")),
    );
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("unknown window must fail").into()),
        Err(error) => error,
    };
    // 语法有效但不存在的 canonical 目标必须返回 stale。
    assert_eq!(error.code, "STALE_SESSION");

    let mut request = run_request("desktop", "record");
    request.confirmed = true;
    request
        .target
        // 使用 canonical stale 目标确保数值门禁先失败。
        .insert("sessionId".to_owned(), json!("s2:w:0000000000000000"));
    request.args.insert(
        "path".to_owned(),
        json!(std::env::temp_dir().join("ai-computer-toolkit-policy-record.mp4")),
    );
    request.args.insert("fps".to_owned(), json!(30));
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("unbounded fps must fail").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "INVALID_ARGUMENT");
    Ok(())
}

#[test]
fn unified_app_record_capability_uses_record_verb() -> Result<(), Box<dyn std::error::Error>> {
    let mut request = run_request("app", "record");
    request.confirmed = true;
    request
        .target
        // 使用 canonical stale 窗口验证正式 app route。
        .insert("sessionId".to_owned(), json!("s2:w:0000000000000000"));
    request
        .args
        .insert("capability".to_owned(), json!("window.record@1"));
    request.args.insert(
        "input".to_owned(),
        json!({ "path": std::env::temp_dir().join("ai-computer-toolkit-app-record.mp4") }),
    );
    let error = match AppControlService::new().execute(request) {
        Ok(_) => return Err(std::io::Error::other("fake session must fail").into()),
        Err(error) => error,
    };
    // app facade 在 provider 唯一解析阶段返回目标缺失。
    assert_eq!(error.code, "TARGET_NOT_FOUND");
    Ok(())
}

#[test]
fn unified_app_write_requires_confirmation_and_exact_session()
-> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let mut request = run_request("app", "apply");
    request
        .target
        .insert("sessionId".to_owned(), json!("s1:deadbeefdeadbeef"));
    request
        .args
        .insert("capability".to_owned(), json!("image.layers.apply@1"));
    request.args.insert(
        "input".to_owned(),
        json!({ "operations": [{ "op": "layer.addRect" }] }),
    );
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("unconfirmed facade write must fail").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");

    let mut request = run_request("app", "apply");
    request.confirmed = true;
    request
        .args
        .insert("capability".to_owned(), json!("image.layers.apply@1"));
    request.args.insert(
        "input".to_owned(),
        json!({ "operations": [{ "op": "layer.addRect" }] }),
    );
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("missing session must fail").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "INVALID_ARGUMENT");
    Ok(())
}

// 验证 Window Close 的空 input 对象可到达 capability Module 默认 deadline。
#[test]
fn unified_app_window_close_accepts_empty_input_object() -> Result<(), Box<dyn std::error::Error>> {
    // 构造 provider-neutral app.close 请求。
    let mut request = run_request("app", "close");
    // 提供 canonical 窗口目标形状。
    request.target.insert(
        // 使用固定 sessionId 字段。
        "sessionId".to_owned(),
        // 使用不会命中真实窗口的测试 opaque ID。
        json!("s2:w:0000000000000000"),
    );
    // 声明精确窗口关闭 capability。
    request.args.insert(
        // 使用固定 capability 字段。
        "capability".to_owned(),
        // 使用版本化 ID。
        json!("window.close@1"),
    );
    // 空 input 表示使用契约默认 timeout。
    request
        // 访问参数对象。
        .args
        // 插入合法空对象。
        .insert("input".to_owned(), json!({}));
    // 提供逐操作确认。
    request.confirmed = true;
    // 通用 policy 只校验存在与对象类型。
    if let Err(error) = ai_computer_toolkit::policy::validate(&request) {
        // 把意外拒绝转换为测试失败。
        return Err(std::io::Error::other(error.to_string()).into());
    }
    // 返回成功。
    Ok(())
}

#[test]
fn unified_app_foreground_capabilities_require_consent_before_session_resolution()
-> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    for (capability, input) in [
        ("ui.input.key@1", json!({ "key": "ENTER" })),
        ("ui.input.pointer@1", json!({ "x": 10, "y": 20 })),
        // 窗口生命周期同样必须在 session 解析前要求前景影响同意。
        ("window.lifecycle@1", json!({ "action": "restore" })),
    ] {
        let error = match service.execute(confirmed_app_apply(capability, input)) {
            Ok(_) => {
                return Err(std::io::Error::other(format!(
                    "{capability} must require foreground consent"
                ))
                .into());
            }
            Err(error) => error,
        };
        assert_eq!(error.code, "FOREGROUND_CONSENT_REQUIRED");
        assert_eq!(error.details["evidence"]["capability"], capability);
    }
    Ok(())
}

#[test]
fn unified_app_text_input_does_not_require_upfront_foreground_consent()
-> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let request = confirmed_app_apply("ui.text.input@1", json!({ "text": "background first" }));
    let error = match service.execute(request) {
        Ok(_) => return Err(std::io::Error::other("fake session must not resolve").into()),
        Err(error) => error,
    };
    assert_eq!(error.code, "TARGET_NOT_FOUND");
    Ok(())
}

#[test]
fn unified_app_sessions_use_opaque_ids() -> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let result = service.execute(CommandRequest::read(
        ai_computer_toolkit::domain::Verb::Sessions,
        "app",
    ))?;
    // 统一 app session 聚合必须发布版本化只读 capability。
    assert_eq!(result["capability"], "application.session.discover@1");
    // 公开 surface 必须保持 provider-neutral app。
    assert_eq!(result["surface"], "app");
    // session 聚合不得执行写操作。
    assert_eq!(result["readOnly"], true);
    // 当前实机只读聚合必须保持前景不变。
    assert_eq!(result["foregroundUnchanged"], true);
    // 目标身份只能声明 opaque 版本化 session。
    assert_eq!(result["targetIdentity"], "opaque-versioned-session-id");
    // C++ 兼容 data 外壳必须来自同一 capability 事实。
    for field in [
        // 核对稳定 capability ID。
        "capability",
        // 核对只读分类。
        "readOnly",
        // 核对前景事实。
        "foregroundUnchanged",
        // 核对目标身份声明。
        "targetIdentity",
        // 核对返回数量。
        "count",
        // 核对完整数量。
        "total",
        // 核对截断标志。
        "truncated",
        // 核对完整 session 投影。
        "sessions",
        // 核对 provider 降级警告。
        "warnings",
    ] {
        // 顶层与 data 必须逐字段完全一致。
        assert_eq!(result[field], result["data"][field]);
    }
    let sessions = result["sessions"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("sessions must be an array"))?;
    for session in sessions {
        let id = session["sessionId"]
            .as_str()
            .ok_or_else(|| std::io::Error::other("sessionId must be a string"))?;
        // 精确识别本批已迁移的文本创建器 capability。
        let is_text_creator = session["capabilities"]
            // 只在能力数组中检查固定 capability ID。
            .as_array()
            // 不把缺失或错误形状误判为已迁移目标。
            .is_some_and(|capabilities| {
                // 查找文本创建器的唯一公开 capability。
                capabilities
                    // 遍历当前 session 发布的能力描述。
                    .iter()
                    // 仅按稳定 capability ID 判定 provider。
                    .any(|capability| capability["id"] == "text.document.create@1")
                // 结束精确 capability 判定。
            });
        // 精确识别不再携带静态启动白名单的 Windows 主机 session。
        let is_host = session["kind"] == "host";
        // 精确识别本批迁移的 Windows 窗口 provider。
        let is_window = session["capabilities"]
            // 只在能力数组中检查固定窗口 capability ID。
            .as_array()
            // 不把缺失或错误形状误判为窗口目标。
            .is_some_and(|capabilities| {
                // 查找窗口截图的稳定 capability ID。
                capabilities
                    // 遍历当前 session 发布的能力描述。
                    .iter()
                    // 仅按稳定 capability ID 判定 provider。
                    .any(|capability| capability["id"] == "window.screenshot@1")
                // 结束精确窗口 capability 判定。
            });
        // 精确识别结构化图像应用 session。
        let is_structured_image_application = session["kind"] == "application"
            // 同时要求发布图像画布创建 capability。
            && session["capabilities"]
                // 只在能力数组中检查稳定 capability ID。
                .as_array()
                // 不把错误形状误判为结构化图像应用。
                .is_some_and(|capabilities| {
                    // 遍历当前 session 发布的能力描述。
                    capabilities
                        // 只按固定 capability ID 判定 provider。
                        .iter()
                        // 画布创建能力只由结构化图像应用发布。
                        .any(|capability| capability["id"] == "image.canvas.create@1")
                    // 结束结构化图像应用判定。
                });
        // 精确识别结构化图像文档 session。
        let is_structured_image_document = session["kind"] == "document"
            // 同时要求发布固定图层操作 capability。
            && session["capabilities"]
                // 只在能力数组中检查稳定 capability ID。
                .as_array()
                // 不把错误形状误判为结构化图像文档。
                .is_some_and(|capabilities| {
                    // 遍历当前 session 发布的能力描述。
                    capabilities
                        // 只按固定 capability ID 判定 provider。
                        .iter()
                        // 图层操作能力只由结构化图像文档发布。
                        .any(|capability| capability["id"] == "image.layers.apply@1")
                    // 结束结构化图像文档判定。
                });
        // 对已迁移 provider 要求与 C++ 逐字节相同的 canonical s2 ID。
        if is_host {
            // 主机身份必须使用 canonical s2:h 与固定摘要长度。
            assert!(id.starts_with("s2:h:"));
            // 防止附加 Windows session ID 或用户名片段。
            assert_eq!(id.len(), 21);
        // 文本创建器继续保持跨实现 golden。
        } else if is_text_creator {
            // 固定创建器身份必须保持跨实现 golden。
            assert_eq!(id, "s2:a:7b71b19b11d72b2d");
        // 窗口 provider 必须发布 canonical s2:w 指纹。
        } else if is_window {
            // 固定窗口 kind 前缀并限制为 64 位小写十六进制摘要。
            assert!(id.starts_with("s2:w:"));
            // 防止截断、扩展或附加原生目标片段。
            assert_eq!(id.len(), 21);
            // 取得不含版本和 kind 的摘要部分。
            let digest = &id[5..];
            // 只允许 canonical 小写十六进制字符。
            assert!(
                // 逐字符验证稳定公共表示。
                digest
                    // 遍历固定长度摘要。
                    .chars()
                    // 拒绝大写十六进制和非十六进制字符。
                    .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character))
                // 结束 canonical 摘要断言。
            );
        // 结构化图像应用必须发布 canonical s2:a 指纹。
        } else if is_structured_image_application {
            // 固定结构化图像应用 kind 前缀。
            assert!(id.starts_with("s2:a:"));
            // 防止附加 PID 或进程创建时间片段。
            assert_eq!(id.len(), 21);
        // 结构化图像文档必须发布 canonical s2:d 指纹。
        } else if is_structured_image_document {
            // 固定结构化图像文档 kind 前缀。
            assert!(id.starts_with("s2:d:"));
            // 防止附加原生文档 ID、名称或路径。
            assert_eq!(id.len(), 21);
        // 任何未单独分类的后续 provider 也不得恢复 s1。
        } else {
            // 所有公开 app session 必须保持 canonical s2 版本。
            assert!(id.starts_with("s2:"));
            // 防止附加原生路由或身份片段。
            assert_eq!(id.len(), 21);
            // 结束统一 s2 身份断言。
        }
        assert!(!id.contains("window:"));
        assert!(session["capabilities"].is_array());
        assert!(session.get("hwnd").is_none());
        assert!(session.get("processId").is_none());
        assert!(session.get("className").is_none());
        // 私有进程创建时间不得进入统一 app JSON。
        assert!(session.get("processCreationTime").is_none());
        // 禁止以 C++ 字段别名泄漏创建时间。
        assert!(session.get("creationTime").is_none());
        // 主机私有 Windows session ID 不得进入统一 app JSON。
        assert!(session.get("windowsSessionId").is_none());
        // 主机私有用户名不得进入统一 app JSON。
        assert!(session.get("userName").is_none());
        if let Some(document) = session.get("document") {
            assert!(document.get("id").is_none());
            // 原生文档源路径只可用于 provider 内部重新解析。
            assert!(document.get("path").is_none());
        }
    }
    Ok(())
}

#[test]
fn windows_application_session_v2_is_registered_but_unavailable()
-> Result<(), Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let sessions = service.execute(CommandRequest::read(
        ai_computer_toolkit::domain::Verb::Sessions,
        "app",
    ))?;
    let host = sessions["sessions"]
        .as_array()
        .and_then(|sessions| sessions.iter().find(|session| session["kind"] == "host"))
        .and_then(|session| session["sessionId"].as_str())
        .ok_or("Windows host session must be discoverable")?;
    let assessment = service.assess_capability("application.session.discover@2", host)?;
    assert_eq!(assessment["decision"], "unavailable");
    assert_eq!(assessment["executionRealm"], "none");
    assert_eq!(assessment["requiresConfirmation"], false);
    assert_eq!(assessment["requiresForegroundConsent"], false);
    Ok(())
}

// 版本化策略清单必须与 Rust 强类型和公开错误保持一致。
#[test]
fn strict_isolation_manifest_matches_runtime_contract() -> Result<(), Box<dyn std::error::Error>> {
    // 解析仓库内的策略清单。
    let manifest: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径避免运行目录漂移。
        "contracts/strict-isolation-policy-v1.json"
    ))?;
    // 解析公开结果证据 schema。
    let schema: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径避免运行目录漂移。
        "../contracts/v1/execution-policy.schema.json"
    ))?;
    // 嵌入所有会接收 System 策略证明的封闭 domain 结果 schema。
    let domain_schema_sources = [
        // app session 聚合结果。
        include_str!("../contracts/v1/application-session-discovery.schema.json"),
        // 可访问性树结果。
        include_str!("../contracts/v1/accessibility-tree.schema.json"),
        // provider-neutral 元素定位结果。
        include_str!("../contracts/v1/ui-element-location.schema.json"),
        // provider-neutral 语义元素动作结果。
        include_str!("../contracts/v1/ui-element-action.schema.json"),
        // provider-neutral 通用指针输入结果。
        include_str!("../contracts/v1/pointer-input-result.schema.json"),
        // provider-neutral 通用键盘输入结果。
        include_str!("../contracts/v1/key-input-result.schema.json"),
        // provider-neutral 窗口状态与几何生命周期结果。
        include_str!("../contracts/v1/window-lifecycle-result.schema.json"),
        // provider-neutral 精确进程终止结果。
        include_str!("../contracts/v1/process-termination-result.schema.json"),
        // 窗口观察结果。
        include_str!("../contracts/v1/window-observation.schema.json"),
        // 进程观察结果。
        include_str!("../contracts/v1/process-observation.schema.json"),
        // 标准 Edit 观察结果。
        include_str!("../contracts/v1/standard-edit-observation.schema.json"),
        // 媒体 session 观察结果。
        include_str!("../contracts/v1/media-session-observation.schema.json"),
        // 媒体状态读取结果。
        include_str!("../contracts/v1/media-playback-state.schema.json"),
        // browser 截图结果。
        include_str!("../contracts/v1/browser-screenshot.schema.json"),
        // 窗口截图结果。
        include_str!("../contracts/v1/window-screenshot.schema.json"),
        // 窗口关闭结果。
        include_str!("../contracts/v1/window-close.schema.json"),
    ];
    // 从强类型生成公开 execution realm 顺序。
    let realms = [
        // 主机无头域。
        ExecutionRealm::HostHeadless,
        // 主机后台域。
        ExecutionRealm::HostBackground,
        // 同会话无焦点域。
        ExecutionRealm::SameSessionNoFocus,
        // 隔离 worker 域。
        ExecutionRealm::IsolatedWorker,
        // 主机前台域。
        ExecutionRealm::HostForeground,
        // 无执行域。
        ExecutionRealm::None,
    ]
    // 转换为 JSON 字符串数组。
    .map(|realm| Value::String(realm.as_str().to_owned()));
    // 清单不得偏离运行时强类型。
    assert_eq!(manifest["executionRealms"], Value::Array(realms.to_vec()));
    // schema 不得偏离同一组强类型。
    assert_eq!(
        schema["$defs"]["executionRealm"]["enum"],
        Value::Array(realms.to_vec())
    );
    // 严格模式只允许两个零打扰域。
    assert_eq!(
        manifest["strictAllowedRealms"],
        json!(["host-headless", "isolated-worker"])
    );
    // 同会话或前台域必须映射为隔离要求错误。
    assert_eq!(
        manifest["strictErrors"]["realmRejected"],
        "ISOLATION_REQUIRED"
    );
    // worker 缺失或不可用必须映射为独立错误。
    assert_eq!(
        manifest["strictErrors"]["workerUnavailable"],
        "ISOLATED_WORKER_UNAVAILABLE"
    );
    // 前台同意永远不得降低严格要求。
    assert_eq!(manifest["foregroundConsentCannotDowngradeStrict"], true);
    // 策略必须先于 provider 解析。
    assert_eq!(manifest["policyRunsBeforeProviderResolution"], true);
    // 清单必须指向唯一真实 launcher 门禁。
    assert_eq!(
        manifest["launcherGate"],
        "tools/Test-RustIsolationPolicy.ps1"
    );
    // 逐个解析封闭 domain schema 并防止拒绝 System 新增字段。
    for source in domain_schema_sources {
        // 解析当前 domain schema。
        let domain_schema: Value = serde_json::from_str(source)?;
        // 两个执行域字段必须复用同一封闭枚举。
        assert_eq!(
            domain_schema["properties"]["executionRealm"],
            schema["$defs"]["executionRealm"]
        );
        // 要求域不得使用不同枚举。
        assert_eq!(
            domain_schema["properties"]["requiredExecutionRealm"],
            schema["$defs"]["executionRealm"]
        );
        // 认证字段类型必须与独立策略 schema 一致。
        assert_eq!(
            domain_schema["properties"]["executionRealmCertified"],
            schema["properties"]["executionRealmCertified"]
        );
        // 隔离要求枚举必须与独立策略 schema 一致。
        assert_eq!(
            domain_schema["properties"]["isolationRequirement"],
            schema["properties"]["isolationRequirement"]
        );
        // 主机影响策略枚举必须与独立策略 schema 一致。
        assert_eq!(
            domain_schema["properties"]["hostImpactPolicy"],
            schema["properties"]["hostImpactPolicy"]
        );
    }
    // 返回契约一致成功。
    Ok(())
}
