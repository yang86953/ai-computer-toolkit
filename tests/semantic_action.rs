#![cfg(target_os = "windows")]

//! 验证通用语义元素动作的公开契约、生产 launcher 与工具自有窗口证据。

// 导入文件、路径、进程与有界等待工具。
use std::{
    // 写入逐测试请求。
    fs,
    // 保存请求与 worker 路径。
    path::{Path, PathBuf},
    // 启动 fixture 与生产 launcher。
    process::{Child, Command, Output, Stdio},
    // 短暂等待窗口进入 inventory。
    thread,
    // 生成唯一文件名与有界等待。
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 固定工具自有语义动作窗口 fixture。
const WINDOW_FIXTURE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-semantic-action-fixture");
// 固定生产 semantic action worker。
const SEMANTIC_WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-semantic-action-worker");

// 拥有一个工具自有窗口。
struct FixtureWindow {
    // 保存唯一标题。
    title: String,
    // 保存自有子进程。
    child: Child,
}

// 提供工具自有窗口生命周期。
impl FixtureWindow {
    // 启动 no-activate 标准控件窗口。
    fn start() -> Self {
        // 读取唯一时间戳。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时间。
            .duration_since(UNIX_EPOCH)
            // 测试时钟必须可用。
            .unwrap_or_else(|error| panic!("fixture clock failed: {error}"))
            // 使用纳秒降低碰撞。
            .as_nanos();
        // 构造 fixture 允许的唯一 ASCII 标题。
        let title = format!("act-rust-semantic-fixture-{}-{stamp}", std::process::id());
        // 启动固定 fixture。
        let child = Command::new(WINDOW_FIXTURE)
            // 传入受限标题。
            .arg(&title)
            // 禁止继承输入。
            .stdin(Stdio::null())
            // 静默 fixture 输出。
            .stdout(Stdio::null())
            // 静默 fixture 诊断。
            .stderr(Stdio::null())
            // 启动自有进程。
            .spawn()
            // 失败时输出测试诊断。
            .unwrap_or_else(|error| panic!("window fixture launch failed: {error}"));
        // 返回生命周期所有者。
        Self { title, child }
    }

    // 经生产 launcher 有界等待 canonical session。
    fn session_id(&mut self) -> String {
        // 最多等待五秒。
        for _ in 0..50 {
            // 枚举统一 app session。
            let output = launcher(&["sessions", "app", "--max-items", "4096"]);
            // 发现命令必须成功。
            assert!(
                output.status.success(),
                // 仅输出受控 stderr。
                "session discovery failed: {}",
                // 转换为可诊断文本。
                String::from_utf8_lossy(&output.stderr)
            );
            // 解析公开结果。
            let value = output_json(&output);
            // 查找标题精确匹配的窗口。
            if let Some(session) = value["sessions"]
                // 要求数组。
                .as_array()
                // 查找工具自有标题。
                .and_then(|sessions| {
                    sessions
                        .iter()
                        .find(|session| session["title"] == self.title)
                })
            {
                // 查找语义动作 descriptor。
                let descriptor = session["capabilities"]
                    // 要求 capability 数组。
                    .as_array()
                    // 查找稳定 ID。
                    .and_then(|capabilities| {
                        // 返回唯一 descriptor 借用。
                        capabilities.iter().find(|capability| {
                            // 精确匹配版本化 ID。
                            capability["id"] == "ui.element.action@1"
                        })
                    })
                    // 生产窗口必须发布语义动作。
                    .unwrap_or_else(|| panic!("fixture session omitted ui.element.action@1"));
                // descriptor 必须要求确认。
                assert_eq!(descriptor["requiresConfirmation"], true);
                // 精确执行域必须是同会话无焦点。
                assert_eq!(descriptor["executionRealm"], "same-session-no-focus");
                // 不得要求前台输入同意。
                assert_eq!(descriptor["requiresForegroundConsent"], false);
                // 不支持时不得发布 fallback。
                assert_eq!(descriptor["constraints"]["fallback"], "none");
                // 返回 canonical 窗口 ID。
                return session["sessionId"]
                    // 要求字符串。
                    .as_str()
                    // fixture 必须具有 ID。
                    .unwrap_or_else(|| panic!("fixture session omitted sessionId"))
                    // 建立独立所有权。
                    .to_owned();
            }
            // fixture 提前退出必须失败。
            if self.child.try_wait().ok().flatten().is_some() {
                // 禁止回落真实用户窗口。
                panic!("window fixture exited before discovery");
            }
            // 短暂等待下一次 inventory。
            thread::sleep(Duration::from_millis(100));
        }
        // 超出边界必须失败。
        panic!("window fixture was not discovered")
    }
}

// 作用域结束时只回收当前测试窗口。
impl Drop for FixtureWindow {
    // 终止并等待精确子进程。
    fn drop(&mut self) {
        // 只终止本实例拥有的 fixture。
        let _ = self.child.kill();
        // 回收进程句柄。
        let _ = self.child.wait();
    }
}

// 拥有一个工具自有临时请求文件。
struct RequestFile {
    // 保存精确路径。
    path: PathBuf,
}

// 提供临时请求文件生命周期。
impl RequestFile {
    // 写入任意结构化请求。
    fn create(request: &Value) -> Self {
        // 读取唯一时间戳。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时间。
            .duration_since(UNIX_EPOCH)
            // 测试时钟必须可用。
            .unwrap_or_else(|error| panic!("request clock failed: {error}"))
            // 使用纳秒降低碰撞。
            .as_nanos();
        // 在系统临时目录构造精确文件名。
        let path = std::env::temp_dir().join(format!(
            // 固定工具自有前缀。
            "act-semantic-action-{}-{stamp}.json",
            // 加入当前进程 ID。
            std::process::id(),
        ));
        // 序列化结构化请求。
        let bytes = serde_json::to_vec(request)
            // 序列化失败时提供诊断。
            .unwrap_or_else(|error| panic!("request serialization failed: {error}"));
        // 写入精确临时文件。
        fs::write(&path, bytes)
            // 写入失败时提供诊断。
            .unwrap_or_else(|error| panic!("request write failed: {error}"));
        // 返回生命周期所有者。
        Self { path }
    }
}

// 作用域结束时删除精确请求文件。
impl Drop for RequestFile {
    // 清理工具自有文件。
    fn drop(&mut self) {
        // 清理失败不覆盖主要断言。
        let _ = fs::remove_file(&self.path);
    }
}

// 返回仓库生产 launcher 路径。
fn launcher_path() -> PathBuf {
    // 从 Cargo 根组合固定脚本。
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        // 进入 tools 目录。
        .join("tools")
        // 选择唯一生产入口。
        .join("Invoke-ComputerControl.ps1")
}

// 经正式 PowerShell launcher 执行请求。
fn launcher(arguments: &[&str]) -> Output {
    // 启动生产脚本。
    Command::new("powershell")
        // 禁止加载用户 profile。
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        // 传入固定 launcher。
        .arg(launcher_path())
        // 传入调用参数。
        .args(arguments)
        // 收集标准流。
        .output()
        // 启动失败时提供诊断。
        .unwrap_or_else(|error| panic!("production launcher failed to start: {error}"))
}

// 解析 launcher stdout JSON。
fn output_json(output: &Output) -> Value {
    // 只接受 UTF-8 JSON。
    serde_json::from_slice(&output.stdout)
        // 解析失败时附带受控 stdout。
        .unwrap_or_else(|error| {
            // 输出失败诊断。
            panic!(
                // 固定消息模板。
                "launcher JSON failed: {error}; stdout={}",
                // 转换 stdout。
                String::from_utf8_lossy(&output.stdout)
            )
        })
}

// 构造统一语义动作请求。
fn action_request(
    // 接收 canonical 窗口 ID。
    session_id: &str,
    // 接收 selector。
    selector: Value,
    // 接收封闭动作。
    action: Value,
    // 接收确认状态。
    confirmed: bool,
) -> Value {
    // 返回完整结构化请求。
    json!({
        // 绑定原 canonical 窗口。
        "target": { "sessionId": session_id },
        // 选择 capability 与输入。
        "args": {
            // 使用稳定版本化 ID。
            "capability": "ui.element.action@1",
            // 使用完整有界输入。
            "input": {
                // 传入 selector。
                "selector": selector,
                // 传入动作。
                "action": action,
                // 覆盖工具窗口完整树。
                "maximumDepth": 20,
                // 保留协议最大节点数。
                "maximumItems": 4096,
                // 使用 ControlView。
                "view": "control",
                // 使用五秒 worker deadline。
                "timeoutMs": 5000
            }
        },
        // 固定逐操作确认。
        "confirmed": confirmed
    })
}

// 经 production launcher 执行 app.apply。
fn apply(request: &Value, strict: bool) -> Output {
    // 写入结构化请求文件。
    let request_file = RequestFile::create(request);
    // 转换路径为拥有型文本。
    let path = request_file.path.display().to_string();
    // 构造固定 launcher 参数。
    let mut arguments = vec![
        // 使用 run 动词。
        "run",     // 使用统一 app facade。
        "app",     // 使用 apply generic verb。
        "apply",   // 传入结构化文件。
        "--input", // 传入精确路径。
        &path,
    ];
    // 严格用例追加不可降级要求。
    if strict {
        // 请求严格零打扰。
        arguments.push("--strict-isolation");
    }
    // 执行生产 launcher。
    launcher(&arguments)
}

// 验证生产 launcher 可 Invoke 且缺口不静默降级。
#[test]
fn production_launcher_invokes_and_reports_semantic_gaps() {
    // Cargo 必须构建固定 sibling worker。
    assert!(Path::new(SEMANTIC_WORKER).is_file());
    // 启动 no-activate 工具窗口。
    let mut window = FixtureWindow::start();
    // 取得实时 canonical 窗口 ID。
    let session_id = window.session_id();
    // 派生唯一标准按钮名称。
    let button_name = format!("{}-invoke", window.title);
    // 构造确认式 Invoke。
    let invoke = action_request(
        // 使用原窗口目标。
        &session_id,
        // 通过公共名称定位按钮。
        json!({ "name": button_name }),
        // 调用默认动作。
        json!({ "type": "invoke" }),
        // 显式确认。
        true,
    );
    // 经生产 launcher 执行。
    let output = apply(&invoke, false);
    // Invoke 必须成功。
    assert!(
        output.status.success(),
        // 输出受控标准流诊断。
        "semantic invoke failed: stdout={} stderr={}",
        // 转换 stdout。
        String::from_utf8_lossy(&output.stdout),
        // 转换 stderr。
        String::from_utf8_lossy(&output.stderr)
    );
    // 解析公开 facade 结果。
    let result = output_json(&output);
    // 顶层 capability 必须稳定。
    assert_eq!(result["capability"], "ui.element.action@1");
    // generic verb 必须为 apply。
    assert_eq!(result["verb"], "apply");
    // System 必须认证同会话无焦点域。
    assert_eq!(result["executionRealm"], "same-session-no-focus");
    // 动作必须明确完成。
    assert_eq!(result["data"]["outcome"], "completed");
    // 禁止自动重试。
    assert_eq!(result["data"]["automaticRetryProhibited"], true);
    // 前景必须保持不变。
    assert_eq!(result["data"]["foregroundUnchanged"], true);
    // 不得静默使用指针。
    assert_eq!(result["data"]["pointerFallbackUsed"], false);
    // snapshot element ID 不得出现在结果中。
    assert!(result.to_string().find("s2:e:").is_none());

    // 固定其余四种真实标准控件动作。
    let remaining = [
        // 标准 Edit 通过 AutomationId 暴露 ValuePattern。
        (
            "1002",
            json!({ "type": "value", "value": "semantic-value" }),
        ),
        // 标准自动复选框暴露 TogglePattern。
        ("1003", json!({ "type": "toggle" })),
        // 标准自动单选按钮暴露 SelectionItemPattern。
        ("1004", json!({ "type": "select" })),
        // 带足够条目的标准列表暴露 ScrollPattern。
        (
            "1005",
            json!({ "type": "scroll", "vertical": "small-increment" }),
        ),
    ];
    // 逐项通过生产 launcher 执行真实 provider 调用。
    for (automation_id, action) in remaining {
        // 构造当前确认式动作请求。
        let request = action_request(
            // 复用同一实时窗口。
            &session_id,
            // 使用 provider-neutral AutomationId。
            json!({ "automationId": automation_id }),
            // 传入当前封闭动作。
            action,
            // 显式确认每一次 mutation。
            true,
        );
        // 经生产 launcher 执行当前动作。
        let output = apply(&request, false);
        // 每种标准模式都必须完成。
        assert!(
            output.status.success(),
            // 输出受控诊断。
            "semantic action for automationId {automation_id} failed: stdout={} stderr={}",
            // 转换 stdout。
            String::from_utf8_lossy(&output.stdout),
            // 转换 stderr。
            String::from_utf8_lossy(&output.stderr)
        );
        // 解析统一 facade 结果。
        let value = output_json(&output);
        // 每种动作都必须明确完成。
        assert_eq!(value["data"]["outcome"], "completed");
        // 每种动作都禁止自动重试。
        assert_eq!(value["data"]["automaticRetryProhibited"], true);
        // 每种动作都不得使用指针 fallback。
        assert_eq!(value["data"]["pointerFallbackUsed"], false);
    }

    // 对同一按钮请求 Toggle 模式。
    let unsupported = action_request(
        // 复用当前窗口。
        &session_id,
        // 复用唯一按钮名称。
        json!({ "name": format!("{}-invoke", window.title) }),
        // 按钮不发布 TogglePattern。
        json!({ "type": "toggle" }),
        // 显式确认。
        true,
    );
    // 执行显式缺口用例。
    let output = apply(&unsupported, false);
    // 不支持必须使用非零退出码。
    assert!(!output.status.success());
    // 解析结构化错误。
    let error = output_json(&output);
    // 保持动作不支持分类。
    assert_eq!(error["error"]["code"], "ACTION_UNSUPPORTED");
    // 错误中也不得出现指针 fallback 或坐标。
    assert!(error.to_string().find("screen-px").is_none());

    // 构造完整零匹配 selector。
    let missing = action_request(
        // 复用实时窗口。
        &session_id,
        // 使用不可能命中的名称。
        json!({ "name": "act-semantic-action-definitely-missing" }),
        // 请求 Invoke。
        json!({ "type": "invoke" }),
        // 显式确认。
        true,
    );
    // 执行零匹配用例。
    let output = apply(&missing, false);
    // 元素缺失必须失败。
    assert!(!output.status.success());
    // 解析结构化错误。
    let error = output_json(&output);
    // 保持完整搜索零命中语义。
    assert_eq!(error["error"]["code"], "ELEMENT_NOT_FOUND");
}

// 验证确认与严格隔离均先于 provider dispatch。
#[test]
fn policy_gates_semantic_action_before_provider_dispatch() {
    // 未确认请求故意缺少有效 target 与 input。
    let unconfirmed = json!({
        // 仅提供 capability 外壳。
        "args": {
            // 使用已注册 mutation capability。
            "capability": "ui.element.action@1",
            // 故意提供无效 input。
            "input": null
        }
    });
    // 执行未确认请求。
    let output = apply(&unconfirmed, false);
    // 未确认必须失败。
    assert!(!output.status.success());
    // 解析结构化错误。
    let error = output_json(&output);
    // 确认错误必须先于 target/input 错误。
    assert_eq!(error["error"]["code"], "CONFIRMATION_REQUIRED");

    // 构造 confirmed 但严格隔离请求。
    let strict = action_request(
        // 使用形状合法但无需真实解析的窗口 ID。
        "s2:w:0000000000000000",
        // 使用合法 selector。
        json!({ "automationId": "ready" }),
        // 使用 Invoke。
        json!({ "type": "invoke" }),
        // 显式确认。
        true,
    );
    // 严格模式必须在 target/provider 前拒绝 same-session route。
    let output = apply(&strict, true);
    // 严格请求必须失败。
    assert!(!output.status.success());
    // 解析结构化错误。
    let error = output_json(&output);
    // 使用稳定隔离错误。
    assert_eq!(error["error"]["code"], "ISOLATION_REQUIRED");
}

// 验证输入与成功 schema 冻结五种动作和禁重试语义。
#[test]
fn schemas_freeze_actions_target_and_retry_contracts() -> Result<(), Box<dyn std::error::Error>> {
    // 解析版本化输入 schema。
    let input: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../contracts/v1/ui-element-action-input.schema.json"
    ))?;
    // 解析版本化结果 schema。
    let result: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../contracts/v1/ui-element-action.schema.json"
    ))?;
    // 顶层输入拒绝未知字段。
    assert_eq!(input["additionalProperties"], false);
    // selector 拒绝 native/provider 字段。
    assert_eq!(input["$defs"]["selector"]["additionalProperties"], false);
    // 五种动作保持封闭。
    assert_eq!(
        input["$defs"]["action"]["oneOf"].as_array().map(Vec::len),
        Some(5)
    );
    // 结果固定 capability ID。
    assert_eq!(
        result["properties"]["capability"]["const"],
        "ui.element.action@1"
    );
    // 结果固定禁止自动重试。
    assert_eq!(
        result["properties"]["data"]["properties"]["automaticRetryProhibited"]["const"],
        // 必须为真。
        true
    );
    // 结果固定无指针 fallback。
    assert_eq!(
        result["properties"]["data"]["properties"]["pointerFallbackUsed"]["const"],
        // 必须为假。
        false
    );
    // 返回契约检查成功。
    Ok(())
}
