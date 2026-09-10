#![cfg(target_os = "windows")]

// 导入集合与通用错误类型。
use std::{collections::BTreeSet, error::Error};
// 导入公开 CLI、目录、方法与 System 入口。
use ai_computer_toolkit::{AppControlService, catalog, cli, methods};
// 导入通用 JSON 值。
use serde_json::Value;

// 返回 C++ with_status 对精确 operation 的预期状态。
fn expected_operation_state(id: &str) -> (&'static str, bool) {
    // 使用对照实现的封闭分组。
    match id {
        // 阶段 5 的旧截图与录制 operation 已退出 C++ 执行目录。
        "desktop.screenshot"
        // 合并 app 截图旧入口。
        | "app.screenshot"
        // 合并浏览器截图旧入口。
        | "browser.screenshot"
        // 合并 desktop 录制旧入口。
        | "desktop.record"
        // 合并 app 录制旧入口。
        | "app.record" => ("retired-rust-primary", false),
        // 这些其余 operation 仍保留历史 C++ opaque 目标状态。
        "app.apply"
        | "win32-control.set-text"
        | "desktop.type-text"
        | "app.close"
        | "app.create" => ("available-confirmed-opaque-target", true),
        // Notepad 文本创建不使用 opaque session。
        "notepad.open-and-write-text" => ("available-confirmed", true),
        // 阶段六完成前五种媒体控制都保持未认证。
        value if value.starts_with("media-session.") => {
            // 历史 C++ worker 与旧 Rust adapter 都不得获得生产执行权。
            ("candidate-not-certified", false)
        }
        // 其余 operation 保持 Rust 兼容实现。
        _ => ("rust-compatibility-only", false),
    }
}

// 验证所有成功结果都使用同一版本化信封。
fn assert_envelope(value: &Value) {
    // 顶层必须成功。
    assert_eq!(value["ok"], true);
    // 固定 control contract 版本。
    assert_eq!(value["contractVersion"], "act/control/v1");
    // 直接 Rust 调用必须诚实声明实现。
    assert_eq!(value["implementation"], "rust");
    // 顶层字段必须封闭。
    assert_eq!(value.as_object().map(|object| object.len()), Some(4));
}

// capability surface 必须保持 C++ directory 的完整字段和值。
#[test]
fn surface_matches_the_cpp_reference_shape() -> Result<(), Box<dyn Error>> {
    // 解析版本化公开 schema。
    let schema: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../contracts/v1/capabilities.schema.json"
    ))?;
    // 编译期嵌入 C++ directory 对照源码。
    let cpp = include_str!("../cpp/src/modules/capability_directory_module.cpp");
    // 取得 Rust capability surface。
    let result = AppControlService::new().capability_surface();
    // 验证版本化信封。
    assert_envelope(&result);
    // schema 必须封闭顶层字段。
    assert_eq!(schema["additionalProperties"], false);
    // schema 必须固定 control contract。
    assert_eq!(
        schema["properties"]["contractVersion"]["const"],
        "act/control/v1"
    );
    // 读取 surface 数据。
    let data = &result["data"];
    // 固定 C++ 对照 surface。
    assert_eq!(data["surface"], "app");
    // 固定产品承诺。
    assert_eq!(
        data["productPromise"],
        "broad-general-control-with-capability-degradation"
    );
    // 固定支持等级。
    assert_eq!(data["supportLevel"], "L2-confirmed-background");
    // 读取 capability 数组。
    let entries = data["capabilities"]
        // 要求数组形状。
        .as_array()
        // 测试夹具缺失应立即失败。
        .ok_or("capabilities must be an array")?;
    // C++ directory 当前固定发布 17 项。
    assert_eq!(entries.len(), 17);
    // 保存 ID 以验证唯一性。
    let mut ids = BTreeSet::new();
    // 逐项核对静态 C++ 对照字面量。
    for entry in entries {
        // 读取版本化 ID。
        let id = entry["id"].as_str().ok_or("capability id must be text")?;
        // ID 不得重复。
        assert!(ids.insert(id));
        // C++ directory 必须包含同一 ID。
        assert!(cpp.contains(&format!("\"{id}\"")));
        // 读取状态文本。
        let status = entry["status"]
            // 只接受字符串。
            .as_str()
            // 缺失时失败。
            .ok_or("capability status must be text")?;
        // 已认证或兼容状态仍需匹配历史 C++ 目录。
        if status != "candidate-not-certified" {
            // 未认证候选不得反向要求已退役 C++ 增补状态。
            assert!(cpp.contains(&format!("\"{status}\"")));
        }
        // 公开对象不得包含原生身份字段。
        assert!(entry.get("hwnd").is_none());
        // 公开对象不得包含原生进程字段。
        assert!(entry.get("pid").is_none());
        // 公开对象不得包含路径字段。
        assert!(entry.get("path").is_none());
    }
    // 阶段六完成前，全部媒体 capability 都必须保持候选未认证。
    for media_id in [
        // 媒体会话发现。
        "media.session.discover@1",
        // 媒体播放状态读取。
        "media.playback.state.read@1",
        // 媒体播放控制。
        "media.playback.control@1",
    ] {
        // 查找精确媒体 capability。
        let media = entries
            // 遍历只读目录。
            .iter()
            // 匹配稳定 ID。
            .find(|entry| entry["id"] == media_id)
            // 缺失项立即失败。
            .ok_or("media capability must be present")?;
        // 不得把尚未认证的 worker 宣称为可用。
        assert_eq!(media["status"], "candidate-not-certified");
    }
    // 静态 surface 等价门禁完成。
    Ok(())
}

// method 元数据必须复用旧目录，同时允许退役后新增 Rust-only method。
#[test]
fn methods_match_the_legacy_directory_and_retired_cpp_reference() -> Result<(), Box<dyn Error>> {
    // 编译期嵌入 C++ directory 对照源码。
    let cpp = include_str!("../cpp/src/modules/capability_directory_module.cpp");
    // 查询完整 method 迁移目录。
    let result = AppControlService::new().method_capabilities(None)?;
    // 验证版本化信封。
    assert_envelope(&result);
    // 查询必须只读。
    assert_eq!(result["data"]["readOnly"], true);
    // 固定无静默降级策略。
    assert_eq!(
        result["data"]["policy"],
        "capability-first-no-silent-fallback"
    );
    // 读取 method 数组。
    let entries = result["data"]["methods"]
        // 要求数组形状。
        .as_array()
        // 缺失时失败。
        .ok_or("methods must be an array")?;
    // 数量必须与旧 Rust method 单一目录一致。
    assert_eq!(entries.len(), methods::all().len());
    // 逐项核对旧字段和 C++ 对照字段。
    for (entry, legacy) in entries.iter().zip(methods::all()) {
        // method ID 必须来自旧目录。
        assert_eq!(entry["id"], legacy.id);
        // 执行范围必须来自旧目录。
        assert_eq!(entry["executionScope"], legacy.execution_scope);
        // 可用性必须来自旧目录。
        assert_eq!(entry["availability"], legacy.availability);
        // 摘要必须来自旧目录。
        assert_eq!(entry["summary"], legacy.summary);
        // 识别 C++ 退役后新增的项目自有 Rust 编码方法。
        let rust_only = legacy.id == "media-foundation-h264";
        // 历史 method 仍必须存在于冻结 C++ 对照源码。
        if !rust_only {
            // 只对历史 method 执行冻结对照。
            assert!(cpp.contains(&format!("\"{}\"", legacy.id)));
        } else {
            // 禁止为了新 Rust 编码方法继续扩展已退役 C++ 目录。
            assert!(!cpp.contains(&format!("\"{}\"", legacy.id)));
        }
        // 读取当前迁移状态。
        let status = entry["cppStatus"].as_str().unwrap_or_default();
        // 未认证候选不再要求已退役 C++ 目录增补新状态或边界。
        if status != "candidate-not-certified" && !rust_only {
            // 历史 C++ 源码必须包含迁移状态。
            assert!(cpp.contains(&format!("\"{status}\"")));
            // 历史 C++ 源码必须包含安全边界。
            assert!(cpp.contains(&format!(
                "\"{}\"",
                entry["safetyBoundary"].as_str().unwrap_or_default()
            )));
        }
    }
    // 单项过滤必须保持同一形状。
    let filtered = AppControlService::new().method_capabilities(Some("media-session"))?;
    // 过滤结果只包含精确项。
    assert_eq!(
        filtered["data"]["methods"].as_array().map(Vec::len),
        Some(1)
    );
    // 过滤结果状态必须稳定。
    assert_eq!(
        filtered["data"]["methods"][0]["cppStatus"],
        "candidate-not-certified"
    );
    // 未认证媒体 method 必须公开阶段六 worker 前置条件。
    assert_eq!(
        filtered["data"]["methods"][0]["safetyBoundary"],
        "stage-six-rust-worker-required"
    );
    // 静态 method 等价门禁完成。
    Ok(())
}

// descriptor 元数据必须与 C++ legacy companion 基线逐对象一致。
#[test]
fn descriptors_match_the_cpp_companion_and_status_rules() -> Result<(), Box<dyn Error>> {
    // 解析 C++ 使用的版本化 companion manifest。
    let companion: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../contracts/compat/legacy-public-catalog-v1.json"
    ))?;
    // 读取 companion apps。
    let apps = companion["apps"]
        // 要求数组形状。
        .as_array()
        // 缺失时失败。
        .ok_or("companion apps must be an array")?;
    // companion 必须与 Rust 旧目录数量一致。
    assert_eq!(apps.len(), catalog::apps().len());
    // 逐 app 核对目录与 operation 状态。
    for app in apps {
        // 读取精确 app ID。
        let app_id = app["id"].as_str().ok_or("app id must be text")?;
        // 查询 app descriptor 迁移元数据。
        let app_result = AppControlService::new().descriptor_capabilities(app_id, None)?;
        // 验证版本化信封。
        assert_envelope(&app_result);
        // 克隆实际 descriptor 以移除新增状态字段。
        let mut actual_app = app_result["data"]["descriptor"].clone();
        // 移除独立目录状态后必须与 companion 完全一致。
        actual_app
            // 要求对象形状。
            .as_object_mut()
            // 缺失时失败。
            .ok_or("app descriptor must be an object")?
            // 移除唯一新增字段。
            .remove("cppDirectoryStatus");
        // 基础 app descriptor 必须逐字段等价。
        assert_eq!(&actual_app, app);
        // 读取 companion operations。
        let operations = app["operations"]
            // 要求数组形状。
            .as_array()
            // 缺失时失败。
            .ok_or("app operations must be an array")?;
        // 逐 operation 核对基础字段与状态。
        for operation in operations {
            // 读取短 operation ID。
            let operation_id = operation["operation"]
                // 只接受字符串。
                .as_str()
                // 缺失时失败。
                .ok_or("operation name must be text")?;
            // 读取完整稳定 ID。
            let stable_id = operation["id"]
                // 只接受字符串。
                .as_str()
                // 缺失时失败。
                .ok_or("operation id must be text")?;
            // 查询精确 operation 迁移元数据。
            let result = AppControlService::new()
                // 调用无状态 System。
                .descriptor_capabilities(app_id, Some(operation_id))?;
            // 读取实际 descriptor 对象。
            let actual = result["data"]["descriptor"]
                // 克隆以移除新增字段。
                .clone();
            // 计算 C++ 对照状态。
            let (status, enabled) = expected_operation_state(stable_id);
            // 状态必须逐项一致。
            assert_eq!(actual["cppStatus"], status);
            // 执行开关必须逐项一致。
            assert_eq!(actual["cppExecutionEnabled"], enabled);
            // 克隆实际对象供基础字段比较。
            let mut base = actual;
            // 取得可变对象。
            let fields = base
                // 要求对象形状。
                .as_object_mut()
                // 缺失时失败。
                .ok_or("operation descriptor must be an object")?;
            // 移除 C++ 状态字段。
            fields.remove("cppStatus");
            // 移除 C++ 执行开关。
            fields.remove("cppExecutionEnabled");
            // 基础 operation descriptor 必须逐字段等价。
            assert_eq!(&base, operation);
        }
    }
    // companion 逐对象等价门禁完成。
    Ok(())
}

// 非法 capabilities 参数必须保持 C++ 对照错误语义。
#[test]
fn cli_rejects_invalid_capability_metadata_queries() {
    // method 不得接受两个 ID。
    let method_result = cli::run(vec![
        // 模拟程序名。
        "tool".to_owned(),
        // 选择 capabilities 命令。
        "capabilities".to_owned(),
        // 选择 method 子命令。
        "method".to_owned(),
        // 提供第一个 ID。
        "media-session".to_owned(),
        // 提供非法额外 ID。
        "uia".to_owned(),
    ]);
    // 必须失败。
    assert!(method_result.is_err());
    // 只在错误分支核对稳定错误码。
    if let Err(method_error) = method_result {
        // 错误码必须稳定。
        assert_eq!(method_error.code, "INVALID_ARGUMENT");
    }
    // descriptor 缺少 app 必须失败。
    let descriptor_result = cli::run(vec![
        // 模拟程序名。
        "tool".to_owned(),
        // 选择 capabilities 命令。
        "capabilities".to_owned(),
        // 选择 descriptor 子命令。
        "descriptor".to_owned(),
    ]);
    // 必须失败。
    assert!(descriptor_result.is_err());
    // 只在错误分支核对稳定错误码。
    if let Err(descriptor_error) = descriptor_result {
        // 错误码必须稳定。
        assert_eq!(descriptor_error.code, "INVALID_ARGUMENT");
    }
    // 未知 surface 必须失败。
    let surface_result = cli::run(vec![
        // 模拟程序名。
        "tool".to_owned(),
        // 选择 capabilities 命令。
        "capabilities".to_owned(),
        // 提供未知 surface。
        "unknown".to_owned(),
    ]);
    // 必须失败。
    assert!(surface_result.is_err());
    // 只在错误分支核对稳定错误码。
    if let Err(surface_error) = surface_result {
        // 错误码必须稳定。
        assert_eq!(surface_error.code, "INVALID_ARGUMENT");
    }
}
