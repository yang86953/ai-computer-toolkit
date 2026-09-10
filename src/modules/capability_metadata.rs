// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "capability_metadata_error.rs"]
mod error_code;

// 导入方法目录与稳定结果类型。
use crate::{AppResult, methods};
// 导入当前 Module 私有封闭错误码。
use error_code::CapabilityMetadataErrorCode;
// 导入 provider-neutral JSON 构造器和值。
use serde_json::{Value, json};

// 保存旧 method 对应的 C++ 迁移状态和安全边界。
const METHOD_MIGRATION: &[(&str, &str, &str)] = &[
    // 应用专用 API 仍是显式扩展点。
    ("app-api", "extension-point", "named-versioned-adapter-only"),
    // 命令行适配器只允许固定程序和参数 schema。
    (
        "command-line",
        "extension-point",
        "fixed-program-and-argument-schema",
    ),
    // COM 仅保留 Rust 兼容实现，不开放原生成员。
    (
        "com-automation",
        "rust-compatibility-only",
        "no-progid-member-or-script-in-public-input",
    ),
    // IPC 必须基于已发布协议注册。
    ("ipc", "extension-point", "published-protocol-only"),
    // 阶段六 Rust worker 认证前禁止发布媒体 method 可用性。
    (
        "media-session",
        "candidate-not-certified",
        "stage-six-rust-worker-required",
    ),
    // CDP 只允许显式远程调试端点。
    ("cdp", "extension-point", "explicit-debug-endpoint-only"),
    // UIA 在 C++ 中只认证读取。
    (
        "uia",
        "available-read-only",
        "no-value-text-bounds-or-write-patterns",
    ),
    // Win32 标准消息禁止任意消息与指针协议。
    (
        "win32-message",
        "available-confirmed",
        "no-arbitrary-message-or-pointer-protocol",
    ),
    // WGC 只接受确认后的 opaque 目标且不得激活。
    (
        "windows-graphics-capture",
        "available-confirmed",
        "confirmation-opaque-target-no-activation",
    ),
    // Media Foundation 编码器只允许 provider-neutral 固定参数。
    (
        "media-foundation-h264",
        "available-confirmed",
        "no-arbitrary-encoder-command",
    ),
    // headless browser 必须使用隔离临时 profile。
    (
        "headless-browser",
        "available-confirmed",
        "isolated-profile-fixed-arguments-no-user-browser-attachment",
    ),
    // 文件自动化仍只保留 Rust 兼容实现。
    (
        "file-automation",
        "rust-compatibility-only",
        "ownership-validation-and-readback-required",
    ),
    // 前台输入仍只保留 Rust 兼容实现并要求双重同意。
    (
        "foreground-input",
        "rust-compatibility-only",
        "confirmation-and-foreground-consent-required",
    ),
];

// 为迁移元数据添加版本化 control 信封。
fn envelope(data: Value) -> Value {
    // 返回与 C++ from_module 同形状但诚实声明 Rust 实现的结果。
    json!({
        // 标记查询成功。
        "ok": true,
        // 固定公开控制契约版本。
        "contractVersion": "act/control/v1",
        // 声明当前直接调用实现。
        "implementation": "rust",
        // 承载独立于运行诊断的迁移元数据。
        "data": data,
    })
}

// 返回与 C++ directory 对照一致的 capability surface。
pub(crate) fn surface() -> Value {
    // 保持对照实现的顺序、字段和值，避免把 Rust-only registry 伪装成 C++ 状态。
    envelope(json!({
        // C++ 对照当前把已迁移 surface 汇总为 app。
        "surface": "app",
        // 保留产品级降级承诺文本。
        "productPromise": "broad-general-control-with-capability-degradation",
        // 保留当前确认式后台支持等级。
        "supportLevel": "L2-confirmed-background",
        // 发布逐 capability 的 C++ 迁移状态。
        "capabilities": [
            // 已认证的主机无头应用发现。
            {
                // 使用版本化 capability ID。
                "id": "application.discover@1",
                // 声明可用状态。
                "status": "available",
                // 声明只读风险。
                "risk": "read",
                // 声明执行域。
                "executionDomain": "host-headless",
                // 声明发现范围约束。
                "constraint": "partial-installed-plus-running-inventory",
            },
            // 已认证的进程发现。
            {
                // 使用版本化 capability ID。
                "id": "process.discover@1",
                // 声明可用状态。
                "status": "available",
                // 声明只读风险。
                "risk": "read",
                // 声明执行域。
                "executionDomain": "host-headless",
                // 声明进程清单覆盖范围。
                "constraint": "running-processes-including-no-window",
            },
            // 已认证的窗口发现。
            {
                // 使用版本化 capability ID。
                "id": "window.discover@1",
                // 声明可用状态。
                "status": "available",
                // 声明只读风险。
                "risk": "read",
                // 声明执行域。
                "executionDomain": "host-headless",
                // 声明可见顶层窗口约束。
                "constraint": "visible-titled-top-level-windows",
            },
            // 已认证的窗口元数据读取。
            {
                // 使用版本化 capability ID。
                "id": "window.metadata.read@1",
                // 声明可用状态。
                "status": "available",
                // 声明只读风险。
                "risk": "read",
                // 声明执行域。
                "executionDomain": "host-headless",
                // 限定公开字段。
                "constraint": "opaque-id-title-application-and-visibility-only",
            },
            // 已认证的进程元数据读取。
            {
                // 使用版本化 capability ID。
                "id": "process.metadata.read@1",
                // 声明可用状态。
                "status": "available",
                // 声明只读风险。
                "risk": "read",
                // 声明执行域。
                "executionDomain": "host-headless",
                // 禁止公开原生身份与路径。
                "constraint": "no-native-id-path-token-or-sid-exposure",
            },
            // 已认证的隔离可访问性树读取。
            {
                // 使用版本化 capability ID。
                "id": "accessibility.tree.read@1",
                // 声明可用状态。
                "status": "available",
                // 声明只读风险。
                "risk": "read",
                // 声明隔离 worker 域。
                "executionDomain": "isolated-worker",
                // 限定树读取内容。
                "constraint": "same-session-bounded-tree-without-value-text-or-bounds",
            },
            // 已认证的窗口捕获预检。
            {
                // 使用版本化 capability ID。
                "id": "window.capture.preflight@1",
                // 声明可用状态。
                "status": "available",
                // 声明只读风险。
                "risk": "read",
                // 声明执行域。
                "executionDomain": "host-headless",
                // 禁止像素、文件与激活副作用。
                "constraint": "metadata-only-no-pixels-no-files-no-activation",
            },
            // 已认证但必须确认的帧探测。
            {
                // 使用版本化 capability ID。
                "id": crate::capabilities::WINDOW_CAPTURE_FRAME_PROBE,
                // 声明可用状态。
                "status": "available",
                // 声明敏感读取风险。
                "risk": "read-sensitive",
                // 声明隔离 worker 域。
                "executionDomain": "isolated-worker",
                // 要求显式确认。
                "requiresConfirmation": true,
                // 限定只返回帧元数据。
                "constraint": "frame-metadata-only-no-surface-read-no-file-no-activation",
            },
            // 已认证的隔离浏览器截图。
            {
                // 使用版本化 capability ID。
                "id": "browser.screenshot@1",
                // 声明确认式可用状态。
                "status": "available-confirmed",
                // 声明敏感读取风险。
                "risk": "read-sensitive",
                // 声明隔离 worker 域。
                "executionDomain": "isolated-worker",
                // 要求显式确认。
                "requiresConfirmation": true,
                // 禁止附着用户浏览器。
                "constraint": "temporary-profile-fixed-arguments-no-user-browser",
            },
            // 应用启动仍只保留 Rust 兼容实现。
            {
                // 使用版本化 capability ID。
                "id": "application.open@1",
                // 声明兼容状态。
                "status": "rust-compatibility-only",
                // 声明变更风险。
                "risk": "mutation",
                // 要求显式确认。
                "requiresConfirmation": true,
            },
            // 已认证的精确窗口截图。
            {
                // 使用版本化 capability ID。
                "id": "window.screenshot@1",
                // 声明确认式可用状态。
                "status": "available-confirmed",
                // 截图读取真实像素，标记为敏感读取。
                "risk": "read-sensitive",
                // 声明隔离 worker 域。
                "executionDomain": "isolated-worker",
                // 要求显式确认。
                "requiresConfirmation": true,
                // 限定 Rust worker、opaque 目标与原子 PNG。
                "constraint": "opaque-s2-window-rust-worker-atomic-png",
            },
            // 已认证的标准 Edit 文本输入。
            {
                // 使用版本化 capability ID。
                "id": "ui.text.input@1",
                // 声明确认式可用状态。
                "status": "available-confirmed",
                // 声明变更风险。
                "risk": "mutation",
                // 声明同会话无焦点执行域。
                "executionDomain": "same-session-no-focus",
                // 要求显式确认。
                "requiresConfirmation": true,
                // 限定 opaque 控件和回读。
                "constraint": "opaque-s2-control-only-confirmed-readback",
            },
            // 已认证的标准窗口关闭。
            {
                // 使用版本化 capability ID。
                "id": "window.close@1",
                // 声明确认式可用状态。
                "status": "available-confirmed",
                // 声明变更风险。
                "risk": "mutation",
                // 声明同会话无焦点执行域。
                "executionDomain": "same-session-no-focus",
                // 要求显式确认。
                "requiresConfirmation": true,
                // 拒绝关闭当前前景目标。
                "constraint": "opaque-s2-window-only-refuses-current-foreground",
            },
            // 已认证的完整通用键盘输入。
            {
                // 使用版本化 capability ID。
                "id": "ui.input.key@1",
                // 声明确认式可用状态。
                "status": "available-confirmed",
                // 声明变更风险。
                "risk": "mutation",
                // 要求显式确认。
                "requiresConfirmation": true,
                // 额外要求前台同意。
                "requiresForegroundConsent": true,
                // 声明主机前景执行域。
                "executionDomain": "host-foreground",
                // 冻结精确目标、完整键集与请求内释放边界。
                "constraint": "opaque-s2-window-complete-keyset-request-scoped-release",
            },
            // 媒体会话发现等待阶段六 Rust worker 认证。
            {
                // 使用版本化 capability ID。
                "id": "media.session.discover@1",
                // 声明候选但未认证状态。
                "status": "candidate-not-certified",
                // 声明敏感读取风险。
                "risk": "read-sensitive",
                // 声明隔离 worker 域。
                "executionDomain": "isolated-worker",
                // 禁止公开来源应用原生身份。
                "constraint": "opaque-session-no-source-application-id",
            },
            // 媒体播放状态读取等待阶段六 Rust worker 认证。
            {
                // 使用版本化 capability ID。
                "id": "media.playback.state.read@1",
                // 声明候选但未认证状态。
                "status": "candidate-not-certified",
                // 声明敏感读取风险。
                "risk": "read-sensitive",
                // 声明隔离 worker 域。
                "executionDomain": "isolated-worker",
                // 限定读取范围。
                "constraint": "exact-session-metadata-and-control-availability-only",
            },
            // 媒体播放控制等待阶段六 Rust worker 认证。
            {
                // 使用版本化 capability ID。
                "id": "media.playback.control@1",
                // 声明候选但未认证状态。
                "status": "candidate-not-certified",
                // 声明变更风险。
                "risk": "mutation",
                // 声明隔离 worker 域。
                "executionDomain": "isolated-worker",
                // 要求显式确认。
                "requiresConfirmation": true,
                // 限定 opaque 目标且不得激活。
                "constraint": "opaque-s2-media-target-no-activation-no-native-id",
            },
        ],
    }))
}

// 查找旧 method 的迁移状态。
fn method_migration(id: &str) -> AppResult<(&'static str, &'static str)> {
    // 只从封闭映射中解析迁移元数据。
    METHOD_MIGRATION
        // 遍历全部对照项。
        .iter()
        // 查找精确 method ID。
        .find(|(candidate, _, _)| *candidate == id)
        // 只投影状态与安全边界。
        .map(|(_, status, boundary)| (*status, *boundary))
        // 映射缺口必须失败闭合，不能返回不完整目录。
        .ok_or_else(|| {
            // 构造稳定内部失败。
            CapabilityMetadataErrorCode::OperationFailed.error(
                // 不泄漏实现细节。
                "The method capability metadata is incomplete.",
            )
        })
}

// 投影单个旧 method 及其 C++ 状态。
fn method_value(method: &methods::MethodDescriptor) -> AppResult<Value> {
    // 读取迁移状态与安全边界。
    let (cpp_status, safety_boundary) = method_migration(method.id)?;
    // 返回与 C++ method_json 同形状的对象。
    Ok(json!({
        // 保留旧 method ID。
        "id": method.id,
        // 保留旧执行范围。
        "executionScope": method.execution_scope,
        // 保留旧目录可用性。
        "availability": method.availability,
        // 保留旧摘要。
        "summary": method.summary,
        // 补充独立 C++ 迁移状态。
        "cppStatus": cpp_status,
        // 补充安全边界。
        "safetyBoundary": safety_boundary,
    }))
}

// 返回全部或单个旧 method 的迁移状态。
pub(crate) fn method(method_id: Option<&str>) -> AppResult<Value> {
    // 按可选 ID 选择稳定 method 集合。
    let selected = match method_id {
        // 精确 ID 只返回单项。
        Some(id) => vec![methods::find(id).ok_or_else(|| {
            // 与 C++ 对照保持相同错误码和文本。
            CapabilityMetadataErrorCode::InvalidArgument.error(
                // 保持跨实现稳定消息。
                "The requested control method is unknown.",
            )
        })?],
        // 缺省返回完整旧 method 目录。
        None => methods::all().iter().collect(),
    };
    // 投影全部选中项并在任何映射缺口上失败闭合。
    let projected = selected
        // 转换为拥有迭代器。
        .into_iter()
        // 补充迁移字段。
        .map(method_value)
        // 收集稳定 JSON 数组。
        .collect::<AppResult<Vec<_>>>()?;
    // 返回版本化只读信封。
    Ok(envelope(json!({
        // 标记查询无副作用。
        "readOnly": true,
        // 固定 capability-first 策略。
        "policy": "capability-first-no-silent-fallback",
        // 返回选中 method 列表。
        "methods": projected,
    })))
}

// 返回 operation 对应的 C++ 状态与执行开关。
fn operation_migration(id: &str) -> (&'static str, bool) {
    // 按 C++ with_status 的封闭分组计算状态。
    match id {
        // 阶段 5 的截图与录制旧 operation 已退出 C++ 执行目录。
        "desktop.screenshot"
        // 合并 app 截图旧入口。
        | "app.screenshot"
        // 合并浏览器截图旧入口。
        | "browser.screenshot"
        // 合并 desktop 录制旧入口。
        | "desktop.record"
        // 合并 app 录制旧入口。
        | "app.record" => ("retired-rust-primary", false),
        // 标准 Edit、窗口关闭和桌面文本仍处于历史 C++ 兼容清单。
        "app.apply"
        | "win32-control.set-text"
        | "desktop.type-text"
        | "app.close"
        | "app.create" => ("available-confirmed-opaque-target", true),
        // 独立 Notepad 文本创建不使用 opaque session。
        "notepad.open-and-write-text" => ("available-confirmed", true),
        // 阶段六完成前五种媒体控制都没有已认证生产实现。
        value if value.starts_with("media-session.") => {
            // 禁止把历史 C++ worker 或旧 Rust 兼容 adapter 解释为可执行。
            ("candidate-not-certified", false)
        }
        // 其余 operation 尚未通过 C++ 等价门禁。
        _ => ("rust-compatibility-only", false),
    }
}

// 解析并验证 C++ 使用的版本化 legacy companion。
fn legacy_companion() -> AppResult<Value> {
    // 从编译期固定资源读取 companion，运行时不访问任意路径。
    let value = serde_json::from_str::<Value>(include_str!(
        // 资源相对当前 Module 的位置固定。
        "../../contracts/compat/legacy-public-catalog-v1.json"
    ))
    // 解析失败必须结构化关闭。
    .map_err(|_| {
        // 构造与 C++ directory_failure 一致的失败。
        CapabilityMetadataErrorCode::OperationFailed.error(
            // 不泄漏资源路径或解析器细节。
            "The legacy public catalog companion is invalid.",
        )
    })?;
    // 同时验证 contract、policy 与 apps 基础形状。
    let valid = value["contractVersion"] == "act/legacy-public-catalog/v1"
        // 固定旧目录策略。
        && value["policy"] == "background-preferred"
        // apps 必须是数组。
        && value["apps"].is_array();
    // 非法 companion 不得降级到 Rust runtime catalog。
    if !valid {
        // 返回与解析失败相同的封闭错误。
        return Err(CapabilityMetadataErrorCode::OperationFailed.error(
            // 保持 C++ 对照消息。
            "The legacy public catalog companion is invalid.",
        ));
    }
    // 返回已验证 companion。
    Ok(value)
}

// 向序列化 descriptor 注入独立迁移字段。
fn descriptor_with_status(mut descriptor: Value, operation: bool) -> AppResult<Value> {
    // descriptor 必须是公开对象。
    let fields = descriptor.as_object_mut().ok_or_else(|| {
        // 非对象表示内部目录破坏。
        CapabilityMetadataErrorCode::SerializationFailed.error(
            // 不返回不完整对象。
            "The capability descriptor is not an object.",
        )
    })?;
    // operation 与 app descriptor 使用不同状态字段。
    if operation {
        // C++ legacy companion 不含 Rust 运行时新增的强类型执行域。
        fields.remove("executionRealm");
        // 读取完整 operation ID。
        let id = fields
            // 查找 ID 字段。
            .get("id")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 缺失时失败闭合。
            .ok_or_else(|| {
                // 构造稳定目录失败。
                CapabilityMetadataErrorCode::SerializationFailed.error(
                    // 说明内部 descriptor 缺少 ID。
                    "The operation descriptor omitted its stable ID.",
                )
            })?;
        // 计算对照迁移状态。
        let (status, enabled) = operation_migration(id);
        // 写入 C++ 状态。
        fields.insert("cppStatus".to_owned(), Value::String(status.to_owned()));
        // 写入 C++ 执行开关。
        fields.insert("cppExecutionEnabled".to_owned(), Value::Bool(enabled));
    } else {
        // C++ legacy companion 不含 Rust 运行时新增的读取执行域。
        fields.remove("readExecutionRealm");
        // app descriptor 只声明兼容目录状态。
        fields.insert(
            // 使用独立目录状态字段。
            "cppDirectoryStatus".to_owned(),
            // 保持 C++ 对照文本。
            Value::String("compatibility-descriptor".to_owned()),
        );
    }
    // 返回补充后的 descriptor。
    Ok(descriptor)
}

// 返回 app 或 operation descriptor 的 C++ 迁移状态。
pub(crate) fn descriptor(app_id: &str, operation_id: Option<&str>) -> AppResult<Value> {
    // 读取并验证 C++ 版本化 companion。
    let companion = legacy_companion()?;
    // 读取 app 数组。
    let apps = companion["apps"].as_array().ok_or_else(|| {
        // companion 形状漂移必须失败闭合。
        CapabilityMetadataErrorCode::OperationFailed.error(
            // 保持 C++ 对照消息。
            "The legacy public catalog companion is invalid.",
        )
    })?;
    // 精确解析 app，未知目标保持 C++ 错误语义。
    let app = apps
        // 遍历 companion app。
        .iter()
        // 只接受精确 ID。
        .find(|entry| entry["id"].as_str() == Some(app_id))
        // 未知 app 返回参数错误。
        .ok_or_else(|| {
            // 构造稳定参数错误。
            CapabilityMetadataErrorCode::InvalidArgument.error(
                // 保持跨实现稳定消息。
                "The requested application descriptor is unknown.",
            )
        })?;
    // 按可选 operation 选择公开 descriptor。
    let (descriptor, operation) = match operation_id {
        // operation 查询必须在精确 app 内解析。
        Some(id) => (
            // 从 companion 中读取 operation 数组。
            app["operations"]
                // 要求数组形状。
                .as_array()
                // 目录缺口必须失败闭合。
                .ok_or_else(|| {
                    // 构造稳定 companion 失败。
                    CapabilityMetadataErrorCode::OperationFailed.error(
                        // 保持 C++ 对照消息。
                        "The application descriptor omitted operations.",
                    )
                })?
                // 遍历精确 app 的 operations。
                .iter()
                // 只匹配公开短 operation ID。
                .find(|entry| entry["operation"].as_str() == Some(id))
                // 未知 operation 返回参数错误。
                .ok_or_else(|| {
                    // 构造稳定参数错误。
                    CapabilityMetadataErrorCode::InvalidArgument.error(
                        // 保持跨实现稳定消息。
                        "The requested operation descriptor is unknown.",
                    )
                })?
                // 克隆版本化 companion 对象。
                .clone(),
            // 标记为 operation descriptor。
            true,
        ),
        // app 查询直接复用 C++ companion 对象。
        None => (
            // 克隆版本化 companion app。
            app.clone(),
            // 标记为 app descriptor。
            false,
        ),
    };
    // 注入独立迁移字段。
    let projected = descriptor_with_status(descriptor, operation)?;
    // 返回版本化只读信封。
    Ok(envelope(json!({
        // 标记查询无副作用。
        "readOnly": true,
        // 返回带 C++ 状态的旧 descriptor。
        "descriptor": projected,
    })))
}
