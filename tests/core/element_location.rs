#![cfg(target_os = "windows")]

//! 验证 provider-neutral 元素定位的 schema、正式 launcher 与工具自有窗口证据。

// 导入文件、进程与有界等待工具。
use std::{
    // 写入逐测试结构化请求。
    fs,
    // 保存请求文件路径。
    path::PathBuf,
    // 启动工具自有窗口与正式 launcher。
    process::{Child, Command, Output, Stdio},
    // 短暂等待窗口进入公开 inventory。
    thread,
    // 生成有界等待和唯一文件名。
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 固定 no-activate 工具自有窗口 fixture。
const WINDOW_FIXTURE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-capture-fixture");

// 拥有一个工具自有测试窗口。
struct FixtureWindow {
    // 保存唯一可访问性名称与窗口标题。
    title: String,
    // 保存自有子进程。
    child: Child,
}

// 提供工具自有窗口生命周期。
impl FixtureWindow {
    // 启动普通或最小化 no-activate 窗口。
    fn start(minimized: bool) -> Self {
        // 读取唯一时间戳。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时间。
            .duration_since(UNIX_EPOCH)
            // 测试时钟必须可用。
            .unwrap_or_else(|error| panic!("fixture clock failed: {error}"))
            // 使用纳秒降低碰撞。
            .as_nanos();
        // 构造 fixture 允许的唯一 ASCII 标题。
        let title = format!("act-rust-capture-fixture-{}-{stamp}", std::process::id());
        // 构造固定 fixture 命令。
        let mut command = Command::new(WINDOW_FIXTURE);
        // 传入受前缀与长度校验的标题。
        command.arg(&title);
        // 最小化用例添加唯一允许模式。
        if minimized {
            // 请求 no-activate 最小化状态。
            command.arg("--minimized");
        }
        // 禁止继承输入并静默 fixture 标准流。
        let child = command
            // fixture 不读取 stdin。
            .stdin(Stdio::null())
            // fixture 不写 stdout。
            .stdout(Stdio::null())
            // fixture 不写 stderr。
            .stderr(Stdio::null())
            // 启动自有进程。
            .spawn()
            // 启动失败时给出测试诊断。
            .unwrap_or_else(|error| panic!("window fixture launch failed: {error}"));
        // 返回精确生命周期所有者。
        Self { title, child }
    }

    // 经正式 launcher 有界等待当前 canonical session。
    fn session_id(&mut self) -> String {
        // 最多等待五秒进入 app facade inventory。
        for _ in 0..50 {
            // 经生产 launcher 枚举统一 app session。
            let output = launcher(&["sessions", "app", "--max-items", "4096"]);
            // 发现命令必须成功。
            assert!(
                output.status.success(),
                "session discovery failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            // 解析公开结果。
            let value = output_json(&output);
            // 查找标题精确匹配的窗口 provider session。
            if let Some(session) = value["sessions"]
                // 要求 sessions 数组。
                .as_array()
                // 遍历安全公开 session。
                .and_then(|sessions| {
                    sessions
                        .iter()
                        .find(|session| session["title"] == self.title)
                })
            {
                // 统一 app facade 必须公开窗口目标身份强度。
                assert_eq!(
                    session["targetIdentityStrength"]["contractVersion"],
                    // 绑定版本化窗口身份契约。
                    "act/window-target-identity/v1"
                );
                // 当前实现不得声称完全相同 token 回收已解决。
                assert_eq!(
                    session["targetIdentityStrength"]["sameProcessRecycledWindowToken"],
                    // 保持保守停止线。
                    "not-guaranteed"
                );
                // 确认该 session 发布定位 capability。
                let published = session["capabilities"]
                    // 要求 capability 数组。
                    .as_array()
                    // 查找稳定版本化 ID。
                    .is_some_and(|capabilities| {
                        // 任一 descriptor 命中即可。
                        capabilities
                            .iter()
                            .any(|capability| capability["id"] == "ui.element.locate@1")
                    });
                // 生产 session 必须公布定位能力。
                assert!(published, "fixture session omitted ui.element.locate@1");
                // 读取 canonical session ID。
                return session["sessionId"]
                    // 要求字符串。
                    .as_str()
                    // fixture 必须具有公开 ID。
                    .unwrap_or_else(|| panic!("fixture session omitted sessionId"))
                    // 建立独立所有权。
                    .to_owned();
            }
            // fixture 提前退出表示创建失败。
            if self.child.try_wait().ok().flatten().is_some() {
                // 禁止回落真实用户窗口。
                panic!("window fixture exited before discovery");
            }
            // 短暂等待下一次 inventory。
            thread::sleep(Duration::from_millis(100));
        }
        // 超出边界时失败。
        panic!("window fixture was not discovered")
    }
}

// 作用域结束时只回收本测试创建的窗口。
impl Drop for FixtureWindow {
    // 终止并等待精确子进程。
    fn drop(&mut self) {
        // 只终止本实例持有的 fixture。
        let _ = self.child.kill();
        // 回收进程句柄。
        let _ = self.child.wait();
    }
}

// 拥有一个工具自有临时请求文件。
struct RequestFile {
    // 保存精确文件路径。
    path: PathBuf,
}

// 提供请求文件生命周期。
impl RequestFile {
    // 写入结构化 facade 请求。
    fn create(session_id: &str, selector: Value) -> Self {
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
            "act-element-location-{}-{stamp}.json",
            // 加入当前测试进程 ID。
            std::process::id(),
        ));
        // 构造统一 app.read 请求。
        let request = json!({
            // 绑定当前 canonical 窗口。
            "target": { "sessionId": session_id },
            // 选择定位 capability 与封闭输入。
            "args": {
                // 使用稳定版本化 ID。
                "capability": "ui.element.locate@1",
                // 使用 root-only 有界查询。
                "input": {
                    // 传入测试 selector。
                    "selector": selector,
                    // 覆盖工具自有窗口的完整有界 ControlView。
                    "maximumDepth": 20,
                    // 为完整性证明保留协议允许的节点上限。
                    "maximumItems": 4096,
                    // 使用 ControlView。
                    "view": "control",
                    // 使用有界 worker deadline。
                    "timeoutMs": 5000
                }
            },
            // 要求正式严格隔离路线。
            "isolationRequirement": "strict"
        });
        // 序列化固定 JSON。
        let bytes = serde_json::to_vec(&request)
            // 序列化失败时提供测试诊断。
            .unwrap_or_else(|error| panic!("request serialization failed: {error}"));
        // 写入工具自有临时文件。
        fs::write(&path, bytes)
            // 写入失败时提供测试诊断。
            .unwrap_or_else(|error| panic!("request write failed: {error}"));
        // 返回精确生命周期所有者。
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

// 返回仓库内生产 launcher 路径。
fn launcher_path() -> PathBuf {
    // 从 Cargo manifest 根组合固定脚本。
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        // 进入 tools 目录。
        .join("tools/windows")
        // 选择唯一生产入口。
        .join("Invoke-ComputerControl.ps1")
}

// 经正式 PowerShell launcher 执行请求。
fn launcher(arguments: &[&str]) -> Output {
    // 启动 Windows PowerShell 生产脚本。
    Command::new("powershell")
        // 禁止加载用户 profile。
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        // 传入固定 launcher 路径。
        .arg(launcher_path())
        // 传入调用方固定参数。
        .args(arguments)
        // 收集完整标准流。
        .output()
        // 启动失败时提供测试诊断。
        .unwrap_or_else(|error| panic!("production launcher failed to start: {error}"))
}

// 解析成功命令 stdout JSON。
fn output_json(output: &Output) -> Value {
    // 只接受 UTF-8 JSON。
    serde_json::from_slice(&output.stdout)
        // 解析失败时附带受控 stdout 诊断。
        .unwrap_or_else(|error| {
            panic!(
                "launcher JSON failed: {error}; stdout={}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
}

// 经生产 launcher 执行定位。
fn locate(session_id: &str, selector: Value) -> Value {
    // 写入逐操作结构化请求。
    let request = RequestFile::create(session_id, selector);
    // 将临时路径转换为拥有型文本。
    let path = request.path.display().to_string();
    // 经 production launcher 执行 app.read。
    let output = launcher(&[
        // 使用 run 动词。
        "run",
        // 使用统一 app facade。
        "app",
        // 使用只读 action。
        "read",
        // 传入结构化请求文件。
        "--input",
        // 传入精确临时路径。
        &path,
        // 再次从 CLI 要求严格隔离。
        "--strict-isolation",
    ]);
    // 定位命令必须成功。
    assert!(
        output.status.success(),
        "element location failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // 解析统一 app facade envelope。
    let value = output_json(&output);
    // 确认生产 mapper 返回成功。
    assert_eq!(value["ok"], true);
    // 确认顶层 capability 未被 provider data 覆盖。
    assert_eq!(value["capability"], "ui.element.locate@1");
    // 确认 System 在 facade envelope 认证隔离 worker 域。
    assert_eq!(value["executionRealm"], "isolated-worker");
    // 返回 provider-neutral capability data。
    value["data"].clone()
}

// 验证正式 launcher 在普通工具自有窗口上区分 unique 与 missing。
#[test]
fn production_launcher_locates_unique_element_and_reports_missing() {
    // 启动普通 no-activate 窗口。
    let mut window = FixtureWindow::start(false);
    // 取得实时 canonical 窗口 ID。
    let session_id = window.session_id();
    // 派生工具自有标准按钮名称。
    let button_name = format!("{}-button", window.title);
    // 按 provider-neutral 名称定位唯一标准按钮 element。
    let unique = locate(&session_id, json!({ "name": button_name }));
    // 固定 capability ID。
    assert_eq!(unique["capability"], "ui.element.locate@1");
    // 必须唯一命中。
    assert_eq!(unique["matchState"], "unique");
    // 坐标必须声明物理屏幕像素。
    assert_eq!(unique["coordinateSnapshot"]["unit"], "physical-screen-px");
    // 多显示器必须允许虚拟桌面带符号坐标。
    assert_eq!(
        unique["coordinateSnapshot"]["multiMonitor"],
        // 对比固定契约文本。
        "virtual-desktop-signed-coordinates"
    );
    // root 窗口必须返回有效 bounds。
    assert_eq!(unique["geometry"]["bounds"]["state"], "available");
    // root 窗口应由 UIA provider 返回可点击点。
    assert_eq!(unique["geometry"]["hitRegion"]["state"], "available");
    // snapshot element ID 只能用于观察。
    assert_eq!(unique["safety"]["elementIdentityUse"], "observation-only");
    // 写 pattern 查询必须为 false。
    assert_eq!(unique["safety"]["writePatternsQueried"], false);

    // 使用不存在的精确名称执行同一有界查询。
    let missing = locate(
        // 复用同一实时窗口当前身份材料。
        &session_id,
        // 使用不可能等于工具标题的名称。
        json!({ "name": "act-element-location-definitely-missing" }),
    );
    // 零匹配必须成功返回 missing 而不是 stale。
    assert_eq!(missing["matchState"], "missing");
    // element 必须为 null。
    assert!(missing["element"].is_null());
    // 命中区域必须明确不可用原因。
    assert_eq!(
        missing["geometry"]["hitRegion"]["reason"],
        "element-not-located"
    );
}

// 验证最小化窗口不会发布可点击候选。
#[test]
fn minimized_fixture_reports_explicit_unavailable_hit_region() {
    // 启动最小化且不激活的工具自有窗口。
    let mut window = FixtureWindow::start(true);
    // 取得实时 canonical 窗口 ID。
    let session_id = window.session_id();
    // 按 root 名称定位唯一 element。
    let result = locate(&session_id, json!({ "name": window.title }));
    // worker 必须明确识别最小化状态。
    assert_eq!(result["windowState"], "minimized");
    // 最小化窗口不得发布当前桌面点击点。
    assert_eq!(result["geometry"]["hitRegion"]["state"], "unavailable");
    // 原因必须独立于 offscreen 或 missing。
    assert_eq!(
        result["geometry"]["hitRegion"]["reason"],
        "window-minimized"
    );
    // 遮挡状态不得被猜测。
    assert_eq!(result["geometry"]["hitRegion"]["occlusion"], "unknown");
}

// 验证输入与结果 schema 保持封闭、provider-neutral 且允许多显示器负坐标。
#[test]
fn schemas_freeze_selector_coordinate_and_safety_contracts()
-> Result<(), Box<dyn std::error::Error>> {
    // 解析版本化输入 schema。
    let input: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径避免工作目录漂移。
        "../../contracts/v1/ui-element-locate-input.schema.json"
    ))?;
    // 解析版本化结果 schema。
    let result: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径避免工作目录漂移。
        "../../contracts/v1/ui-element-location.schema.json"
    ))?;
    // 顶层输入必须拒绝未知字段。
    assert_eq!(input["additionalProperties"], false);
    // selector 必须拒绝 XPath 等应用或 provider 特定字段。
    assert_eq!(input["$defs"]["selector"]["additionalProperties"], false);
    // 深度硬上限必须与 Rust 输入验证一致。
    assert_eq!(input["properties"]["maximumDepth"]["maximum"], 20);
    // 节点数硬上限必须与 worker 边界一致。
    assert_eq!(input["properties"]["maximumItems"]["maximum"], 4096);
    // 结果必须拒绝 native 或未登记字段。
    assert_eq!(result["additionalProperties"], false);
    // 结果必须固定 capability 版本。
    assert_eq!(
        result["properties"]["capability"]["const"],
        // 对比公开版本化 ID。
        "ui.element.locate@1"
    );
    // 虚拟桌面横坐标必须允许 i32 最小负坐标。
    assert_eq!(
        result["$defs"]["availableBounds"]["properties"]["left"]["minimum"],
        // 对比 Win32 有符号坐标下限。
        i64::from(i32::MIN)
    );
    // 命中区域遮挡不可用时必须保持 unknown。
    assert_eq!(
        result["$defs"]["unavailableHitRegion"]["properties"]["occlusion"]["const"],
        // 禁止猜测无遮挡。
        "unknown"
    );
    // 写 pattern 查询必须被 schema 固定为 false。
    assert_eq!(
        result["$defs"]["safety"]["properties"]["writePatternsQueried"]["const"],
        // 只读 Query 不允许写 pattern。
        false
    );
    // 返回契约检查成功。
    Ok(())
}
