#![cfg(target_os = "windows")]

//! 通过原样生产 launcher 验证最小化、隐藏与窗口重建动态兼容矩阵。

// 导入文件、进程、串行锁与有界等待工具。
use std::{
    // 创建和清理工具自有结构化请求与候选目录。
    fs,
    // 写入动态夹具的唯一封闭控制命令。
    io::Write,
    // 保存工具自有临时路径。
    path::PathBuf,
    // 启动工具自有窗口与生产 PowerShell launcher。
    process::{Child, ChildStdin, Command, Output, Stdio},
    // 串行化会观察主机前景状态的三个场景。
    sync::{Mutex, MutexGuard},
    // 在实时窗口清单之间短暂等待。
    thread,
    // 生成唯一标题并限制所有轮询时间。
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// 导入公开 JSON 值与构造宏。
use serde_json::{Value, json};
// 导入只读前景窗口查询，测试结果不公开原生值。
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

// 固定 Cargo 构建的工具自有窗口 fixture。
const WINDOW_FIXTURE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-capture-fixture");
// 串行化三个 no-activate 前景不变场景。
static DYNAMIC_WINDOW_LOCK: Mutex<()> = Mutex::new(());

// 固定动态测试允许的三个 fixture 模式。
#[derive(Clone, Copy)]
enum FixtureMode {
    // 启动后保持最小化。
    Minimized,
    // 收到命令后隐藏。
    HideOnCommand,
    // 收到命令后创建新代际并退役旧代际。
    RecreateOnCommand,
}

// 拥有一个工具自有窗口进程及其封闭控制输入。
struct FixtureWindow {
    // 保存唯一安全标题。
    title: String,
    // 保存自有子进程。
    child: Child,
    // 动态模式保存一次性 stdin 控制端。
    control: Option<ChildStdin>,
}

// 提供工具自有窗口创建、发现与转换。
impl FixtureWindow {
    // 启动一个固定模式的 no-activate 窗口。
    fn start(mode: FixtureMode) -> Self {
        // 读取唯一时间戳。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时间。
            .duration_since(UNIX_EPOCH)
            // 测试时钟必须可用。
            .unwrap_or_else(|error| panic!("fixture clock failed: {error}"))
            // 使用纳秒降低碰撞概率。
            .as_nanos();
        // 构造 fixture 接受的唯一 ASCII 标题。
        let title = format!("act-rust-capture-fixture-{}-{stamp}", std::process::id());
        // 创建固定 fixture 命令。
        let mut command = Command::new(WINDOW_FIXTURE);
        // 传入已受 fixture 校验的标题。
        command.arg(&title);
        // 传入封闭模式参数。
        command.arg(match mode {
            // 请求最小化 no-activate 窗口。
            FixtureMode::Minimized => "--minimized",
            // 请求收到命令后隐藏。
            FixtureMode::HideOnCommand => "--hide-on-command",
            // 请求收到命令后重建代际。
            FixtureMode::RecreateOnCommand => "--recreate-on-command",
        });
        // 只有动态模式需要可写 stdin。
        command.stdin(match mode {
            // 最小化模式不读取控制输入。
            FixtureMode::Minimized => Stdio::null(),
            // 隐藏与重建模式接收一次固定命令。
            FixtureMode::HideOnCommand | FixtureMode::RecreateOnCommand => Stdio::piped(),
        });
        // fixture 完成标记不进入产品证据。
        command.stdout(Stdio::null());
        // fixture 不写诊断信息。
        command.stderr(Stdio::null());
        // 启动工具自有窗口进程。
        let mut child = command
            // 创建子进程。
            .spawn()
            // 启动失败时提供固定测试诊断。
            .unwrap_or_else(|error| panic!("window fixture launch failed: {error}"));
        // 只取得动态模式的控制输入。
        let control = child.stdin.take();
        // 返回精确生命周期所有者。
        Self {
            // 保存公开匹配标题。
            title,
            // 保存唯一子进程。
            child,
            // 保存可选一次性控制端。
            control,
        }
    }

    // 经生产 launcher 有界等待当前公开 opaque session。
    fn session_id(&mut self) -> String {
        // 设置五秒发现边界。
        let deadline = Instant::now() + Duration::from_secs(5);
        // 轮询实时公开窗口清单。
        loop {
            // 获取当前生产窗口清单。
            let inventory = window_inventory();
            // 按唯一标题查找当前 session。
            if let Some(session_id) = session_for_title(&inventory, &self.title) {
                // 生产 window surface 必须公开版本化身份强度。
                assert_window_identity_strength(&inventory, &self.title);
                // 返回独立拥有的 opaque ID。
                return session_id;
            }
            // fixture 提前退出必须失败闭合。
            if self.child.try_wait().ok().flatten().is_some() {
                // 禁止回落真实用户窗口。
                panic!("window fixture exited before discovery");
            }
            // 超出边界时失败。
            if Instant::now() >= deadline {
                // 报告固定发现失败。
                panic!("window fixture was not discovered");
            }
            // 短暂等待下一次公开快照。
            thread::sleep(Duration::from_millis(50));
        }
    }

    // 发送唯一固定状态转换命令。
    fn transition(&mut self) {
        // 动态模式必须拥有控制管道。
        let control = self
            // 访问唯一 stdin 所有权。
            .control
            // 以可变引用写入。
            .as_mut()
            // 缺失控制端表示测试模式漂移。
            .unwrap_or_else(|| panic!("dynamic fixture omitted control pipe"));
        // 写入 fixture 唯一允许的逐字命令。
        control
            // 发送带换行命令供逐行读取。
            .write_all(b"transition\n")
            // 写入失败时提供固定诊断。
            .unwrap_or_else(|error| panic!("fixture transition write failed: {error}"));
        // 立即刷新以触发 UI 线程轮询。
        control
            // 刷新子进程管道。
            .flush()
            // 刷新失败时提供固定诊断。
            .unwrap_or_else(|error| panic!("fixture transition flush failed: {error}"));
        // 关闭唯一控制端，禁止第二次命令。
        self.control.take();
    }
}

// 作用域结束时只回收本测试创建的窗口进程。
impl Drop for FixtureWindow {
    // 终止并等待精确子进程。
    fn drop(&mut self) {
        // 只终止当前实例持有的 fixture。
        let _ = self.child.kill();
        // 回收子进程句柄，避免残留。
        let _ = self.child.wait();
    }
}

// 拥有一个工具自有临时目录。
struct FixtureDirectory {
    // 保存精确目录路径。
    path: PathBuf,
}

// 提供工具自有临时目录创建与路径组合。
impl FixtureDirectory {
    // 创建当前测试独占目录。
    fn create() -> Self {
        // 读取唯一时间戳。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时间。
            .duration_since(UNIX_EPOCH)
            // 测试时钟必须可用。
            .unwrap_or_else(|error| panic!("directory clock failed: {error}"))
            // 使用纳秒降低碰撞概率。
            .as_nanos();
        // 只在系统临时目录下组合固定前缀路径。
        let path = std::env::temp_dir().join(format!(
            // 固定工具自有目录名。
            "act-special-window-dynamic-{}-{stamp}",
            // 加入当前测试进程 ID。
            std::process::id(),
        ));
        // 创建精确工具自有目录。
        fs::create_dir(&path)
            // 创建失败时提供固定诊断。
            .unwrap_or_else(|error| panic!("fixture directory failed: {error}"));
        // 返回唯一目录所有者。
        Self { path }
    }

    // 组合目录下固定文件名。
    fn join(&self, name: &str) -> PathBuf {
        // 返回当前工具自有目录内路径。
        self.path.join(name)
    }
}

// 作用域结束时清理精确工具自有目录。
impl Drop for FixtureDirectory {
    // 删除当前实例创建的目录树。
    fn drop(&mut self) {
        // 测试清理失败不覆盖主要断言。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 拥有一个工具自有结构化请求文件。
struct RequestFile {
    // 保存精确请求路径。
    path: PathBuf,
}

// 提供结构化请求文件创建。
impl RequestFile {
    // 写入一个完整公开 app 请求。
    fn create(request: &Value) -> Self {
        // 读取唯一时间戳。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时间。
            .duration_since(UNIX_EPOCH)
            // 测试时钟必须可用。
            .unwrap_or_else(|error| panic!("request clock failed: {error}"))
            // 使用纳秒降低碰撞概率。
            .as_nanos();
        // 构造精确临时请求路径。
        let path = std::env::temp_dir().join(format!(
            // 使用固定工具自有前缀。
            "act-special-window-request-{}-{stamp}.json",
            // 加入当前测试进程 ID。
            std::process::id(),
        ));
        // 严格序列化请求。
        let bytes = serde_json::to_vec(request)
            // 序列化失败时提供固定诊断。
            .unwrap_or_else(|error| panic!("request serialization failed: {error}"));
        // 写入精确请求文件。
        fs::write(&path, bytes)
            // 写入失败时提供固定诊断。
            .unwrap_or_else(|error| panic!("request write failed: {error}"));
        // 返回唯一请求文件所有者。
        Self { path }
    }
}

// 作用域结束时删除精确请求文件。
impl Drop for RequestFile {
    // 删除当前实例创建的文件。
    fn drop(&mut self) {
        // 测试清理失败不覆盖主要断言。
        let _ = fs::remove_file(&self.path);
    }
}

// 返回仓库内原样生产 launcher 路径。
fn launcher_path() -> PathBuf {
    // 从固定 manifest 根组合生产脚本。
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        // 进入 tools 目录。
        .join("tools/windows")
        // 选择唯一正式入口。
        .join("Invoke-ComputerControl.ps1")
}

// 经原样生产 PowerShell launcher 执行参数。
fn launcher(arguments: &[&str]) -> Output {
    // 启动 Windows PowerShell 生产脚本。
    Command::new("powershell")
        // 禁止加载用户 profile 并允许仓库脚本。
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        // 传入原样生产 launcher。
        .arg(launcher_path())
        // 传入调用方封闭参数。
        .args(arguments)
        // 收集完整标准流。
        .output()
        // 启动失败时提供固定诊断。
        .unwrap_or_else(|error| panic!("production launcher failed: {error}"))
}

// 解析 launcher stdout JSON。
fn output_json(output: &Output) -> Value {
    // 只接受 UTF-8 JSON envelope。
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        // 解析失败时提供受控输出诊断。
        panic!(
            // 保持固定诊断模板。
            "launcher JSON failed: {error}; stdout={}",
            // 仅测试失败时显示标准输出。
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

// 获取当前生产窗口发现快照。
fn window_inventory() -> Value {
    // 经生产 launcher 请求最大公开窗口清单。
    let output = launcher(&["sessions", "window", "--max-items", "4096"]);
    // 发现必须成功。
    assert!(
        output.status.success(),
        // 失败时提供标准错误诊断。
        "window discovery failed: {}",
        // 只在失败时显示 launcher stderr。
        String::from_utf8_lossy(&output.stderr)
    );
    // 解析并返回公开 JSON。
    output_json(&output)
}

// 从公开窗口清单按精确标题读取 opaque ID。
fn session_for_title(inventory: &Value, title: &str) -> Option<String> {
    // 读取 sessions 数组。
    inventory["sessions"]
        // 要求数组形状。
        .as_array()
        // 查找标题精确匹配的窗口。
        .and_then(|sessions| sessions.iter().find(|session| session["title"] == title))
        // 读取 canonical opaque ID。
        .and_then(|session| session["sessionId"].as_str())
        // 建立独立字符串所有权。
        .map(str::to_owned)
}

// 核对公开窗口 session 的保守身份强度。
fn assert_window_identity_strength(inventory: &Value, title: &str) {
    // 查找精确标题对应的公开窗口 session。
    let session = inventory["sessions"]
        // 要求数组形状。
        .as_array()
        // 查找唯一工具自有标题。
        .and_then(|sessions| sessions.iter().find(|session| session["title"] == title))
        // 调用方已确认 session 存在。
        .unwrap_or_else(|| panic!("identity strength target must exist"));
    // 绑定版本化身份强度契约。
    assert_eq!(
        session["targetIdentityStrength"]["contractVersion"],
        // 使用稳定版本。
        "act/window-target-identity/v1"
    );
    // 禁止把完全相同 token 回收误报为已保证。
    assert_eq!(
        session["targetIdentityStrength"]["sameProcessRecycledWindowToken"],
        // 保持保守结论。
        "not-guaranteed"
    );
    // 当前短命 launcher 没有持久 generation owner。
    assert_eq!(session["targetIdentityStrength"]["generationOwner"], "none");
    // 公开身份强度不得泄漏原生字段。
    assert_public(&session["targetIdentityStrength"]);
}

// 有界等待标题从公开清单消失。
fn wait_until_absent(title: &str) {
    // 设置五秒等待边界。
    let deadline = Instant::now() + Duration::from_secs(5);
    // 轮询公开快照直到缺失。
    loop {
        // 当前标题缺失时完成。
        if session_for_title(&window_inventory(), title).is_none() {
            // 结束有界等待。
            return;
        }
        // 到期时报告固定失败。
        if Instant::now() >= deadline {
            // 窗口不得被静默重新显示。
            panic!("window remained discoverable after hidden transition");
        }
        // 短暂等待下一快照。
        thread::sleep(Duration::from_millis(50));
    }
}

// 有界等待同标题窗口取得不同公开代际。
fn wait_for_recreated_session(title: &str, previous: &str) -> String {
    // 设置五秒重发现边界。
    let deadline = Instant::now() + Duration::from_secs(5);
    // 轮询公开窗口快照。
    loop {
        // 读取当前同标题 session。
        if let Some(session_id) = session_for_title(&window_inventory(), title) {
            // 只有不同 opaque ID 才表示新代际。
            if session_id != previous {
                // 返回新代际 ID。
                return session_id;
            }
        }
        // 到期时失败，禁止把旧 identity 当作重建成功。
        if Instant::now() >= deadline {
            // 报告固定重建失败。
            panic!("recreated window did not obtain a new opaque identity");
        }
        // 短暂等待下一快照。
        thread::sleep(Duration::from_millis(50));
    }
}

// 经公开 window metadata surface 检查精确目标。
fn inspect_window(session_id: &str) -> Output {
    // 组合公开 target 参数。
    let target = format!("sessionId={session_id}");
    // 经生产 launcher 执行只读检查。
    launcher(&["inspect", "window", "--target", &target])
}

// 经统一 app facade 执行只读或敏感读取 capability。
fn run_app(
    // 接收 read 或 apply 操作。
    operation: &str,
    // 接收稳定 capability ID。
    capability: &str,
    // 接收 canonical opaque 窗口目标。
    session_id: &str,
    // 接收 provider-neutral 输入。
    input: Value,
    // 标记是否提供逐操作确认。
    confirmed: bool,
) -> Output {
    // 构造完整结构化 facade 请求。
    let request = json!({
        // 只传 canonical opaque 目标。
        "target": { "sessionId": session_id },
        // 传入稳定 capability 与封闭输入。
        "args": { "capability": capability, "input": input },
        // 保存逐操作确认事实。
        "confirmed": confirmed,
        // 要求正式严格隔离路线。
        "isolationRequirement": "strict"
    });
    // 写入工具自有结构化请求文件。
    let request = RequestFile::create(&request);
    // 转换请求路径供 launcher 使用。
    let path = request.path.display().to_string();
    // 确认请求同时通过 CLI 固定开关表达。
    if confirmed {
        // 经生产 launcher 执行已确认调用。
        return launcher(&[
            // 使用统一 run 动词。
            "run",
            // 使用 app facade。
            "app",
            // 传入 read 或 apply。
            operation,
            // 使用结构化请求文件。
            "--input",
            // 传入工具自有路径。
            &path,
            // 要求严格隔离。
            "--strict-isolation",
            // 提供逐操作确认。
            "--confirm",
        ]);
    }
    // 经生产 launcher 执行未确认只读调用。
    launcher(&[
        // 使用统一 run 动词。
        "run",
        // 使用 app facade。
        "app",
        // 传入 read 操作。
        operation,
        // 使用结构化请求文件。
        "--input",
        // 传入工具自有路径。
        &path,
        // 要求严格隔离。
        "--strict-isolation",
    ])
}

// 读取当前前景窗口的测试私有数值快照。
fn foreground_snapshot() -> isize {
    // 原生值只用于相等性断言，永不进入公开 JSON。
    unsafe { GetForegroundWindow() }.0 as isize
}

// 取得可恢复 poisoned 状态的动态测试串行锁。
fn fixture_guard() -> MutexGuard<'static, ()> {
    // 请求唯一动态窗口所有权。
    DYNAMIC_WINDOW_LOCK
        // 锁定三个场景。
        .lock()
        // 先前 panic 后仍允许清理验证继续。
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// 递归检查公开 JSON 是否含禁止字段。
fn contains_key(value: &Value, forbidden: &str) -> bool {
    // 按 JSON 类型递归。
    match value {
        // 对象检查当前键与全部子值。
        Value::Object(object) => object
            // 遍历键值对。
            .iter()
            // 任一键或子树命中即返回真。
            .any(|(key, value)| key == forbidden || contains_key(value, forbidden)),
        // 数组递归检查全部元素。
        Value::Array(items) => items
            // 遍历数组元素。
            .iter()
            // 任一子树命中即返回真。
            .any(|value| contains_key(value, forbidden)),
        // 标量不包含字段名。
        _ => false,
    }
}

// 核对公开结果不含原生或 provider 私有目标。
fn assert_public(value: &Value) {
    // 逐项检查稳定禁止字段。
    for field in ["hwnd", "processId", "nativeHandle", "providerId"] {
        // 任一命中都违反公开边界。
        assert!(!contains_key(value, field), "public result leaked {field}");
    }
    // 序列化完整公开结果。
    let text = value.to_string();
    // 禁止 legacy 原生窗口目标。
    assert!(!text.contains("\"window:"));
    // 禁止 UIA provider 私有目标。
    assert!(!text.contains("uia:window:"));
}

// 验证最小化窗口仍可发现，但定位和截图不猜测或恢复目标。
#[test]
fn minimized_window_remains_discoverable_and_observation_fails_closed() {
    // 串行化前景不变断言。
    let _guard = fixture_guard();
    // 启动最小化且不激活的工具自有窗口。
    let mut window = FixtureWindow::start(FixtureMode::Minimized);
    // 经生产窗口发现取得 canonical ID。
    let session_id = window.session_id();
    // 记录读取前前景窗口。
    let foreground_before = foreground_snapshot();
    // 经统一 app.read 定位窗口根元素。
    let location = run_app(
        // 使用只读操作。
        "read",
        // 使用 provider-neutral 定位 capability。
        "ui.element.locate@1",
        // 绑定当前精确窗口。
        &session_id,
        // 使用标题精确 selector 与有界搜索。
        json!({
            // 选择窗口根标题。
            "selector": { "name": window.title },
            // 使用最大支持深度。
            "maximumDepth": 20,
            // 使用完整性门禁节点上限。
            "maximumItems": 4096,
            // 使用 ControlView。
            "view": "control",
            // 使用有界 worker deadline。
            "timeoutMs": 5000
        }),
        // 定位是只读操作。
        false,
    );
    // 定位命令必须成功返回不可用几何。
    assert!(location.status.success());
    // 解析公开定位结果。
    let location = output_json(&location);
    // 核对 provider-neutral 状态分类。
    assert_eq!(location["data"]["windowState"], "minimized");
    // 禁止猜测当前桌面命中区域。
    assert_eq!(
        location["data"]["geometry"]["hitRegion"]["state"],
        "unavailable"
    );
    // 核对稳定最小化原因。
    assert_eq!(
        location["data"]["geometry"]["hitRegion"]["reason"],
        // 使用公开 provider-neutral 原因。
        "window-minimized"
    );
    // 创建精确工具自有候选目录。
    let directory = FixtureDirectory::create();
    // 构造不得生成的截图目标。
    let screenshot_path = directory.join("minimized.png");
    // 经已确认正式截图路线执行预检。
    let screenshot = run_app(
        // 敏感读取通过 apply 路由。
        "screenshot",
        // 使用正式截图 capability。
        "window.screenshot@1",
        // 绑定同一最小化窗口。
        &session_id,
        // 只传封闭截图输入。
        json!({ "path": screenshot_path, "timeoutMs": 1000 }),
        // 提供逐操作确认。
        true,
    );
    // 最小化目标必须失败闭合。
    assert!(!screenshot.status.success());
    // 解析公开失败 envelope。
    let screenshot = output_json(&screenshot);
    // 保留稳定最小化错误码。
    assert_eq!(
        screenshot["error"]["code"],
        // 对齐稳定最小化错误码。
        "CAPTURE_TARGET_MINIMIZED",
        // 失败时显示安全公开 envelope。
        "{screenshot}"
    );
    // 预检失败不得创建文件。
    assert!(!screenshot_path.exists());
    // 当前窗口仍须以同一 ID 可发现，证明未自动恢复。
    assert_eq!(
        session_for_title(&window_inventory(), &window.title).as_deref(),
        // 比较原 canonical ID。
        Some(session_id.as_str())
    );
    // 核对两个公开结果的隐私边界。
    assert_public(&location);
    // 失败结果同样不得泄漏目标。
    assert_public(&screenshot);
    // 整条路线不得改变主机前景。
    assert_eq!(foreground_snapshot(), foreground_before);
}

// 验证隐藏窗口从公开清单消失且旧目标稳定 stale。
#[test]
fn hidden_window_is_not_reshown_and_old_target_becomes_stale() {
    // 串行化前景不变断言。
    let _guard = fixture_guard();
    // 启动可见 no-activate 动态窗口。
    let mut window = FixtureWindow::start(FixtureMode::HideOnCommand);
    // 取得隐藏前 canonical ID。
    let session_id = window.session_id();
    // 记录转换前前景窗口。
    let foreground_before = foreground_snapshot();
    // 请求唯一隐藏转换。
    window.transition();
    // 有界等待标题从公开清单消失。
    wait_until_absent(&window.title);
    // 经公开 metadata read 使用旧 ID。
    let inspected = inspect_window(&session_id);
    // 隐藏窗口不得由旧目标重新显示或解析。
    assert!(!inspected.status.success());
    // 解析公开 stale envelope。
    let inspected = output_json(&inspected);
    // 使用时重新发现必须返回稳定 stale。
    assert_eq!(inspected["error"]["code"], "STALE_SESSION");
    // 创建精确工具自有候选目录。
    let directory = FixtureDirectory::create();
    // 构造不得生成的截图目标。
    let screenshot_path = directory.join("hidden.png");
    // 旧目标截图同样必须在创建候选前失败。
    let screenshot = run_app(
        // 敏感读取通过 apply 路由。
        "screenshot",
        // 使用正式截图 capability。
        "window.screenshot@1",
        // 绑定隐藏前旧 ID。
        &session_id,
        // 传入封闭截图输入。
        json!({ "path": screenshot_path, "timeoutMs": 1000 }),
        // 提供逐操作确认。
        true,
    );
    // 隐藏旧目标必须失败闭合。
    assert!(!screenshot.status.success());
    // 解析公开截图失败。
    let screenshot = output_json(&screenshot);
    // 公开清单缺失时保持 stale，不尝试显示目标。
    assert_eq!(
        screenshot["error"]["code"],
        // 对齐稳定 stale 错误码。
        "STALE_SESSION",
        // 失败时显示安全公开 envelope。
        "{screenshot}"
    );
    // 不得生成截图候选。
    assert!(!screenshot_path.exists());
    // 再次采样仍不得重新发现隐藏窗口。
    assert!(session_for_title(&window_inventory(), &window.title).is_none());
    // 核对公开 metadata 错误隐私。
    assert_public(&inspected);
    // 核对公开截图错误隐私。
    assert_public(&screenshot);
    // 隐藏与失败路线不得改变主机前景。
    assert_eq!(foreground_snapshot(), foreground_before);
}

// 验证窗口重建产生新身份且旧目标不会静默重绑定。
#[test]
fn recreated_window_gets_new_identity_and_old_target_stays_stale() {
    // 串行化前景不变断言。
    let _guard = fixture_guard();
    // 启动可见 no-activate 动态窗口。
    let mut window = FixtureWindow::start(FixtureMode::RecreateOnCommand);
    // 取得旧窗口 canonical ID。
    let previous = window.session_id();
    // 记录转换前前景窗口。
    let foreground_before = foreground_snapshot();
    // 请求 fixture 创建一个保持并发存活屏障的不同窗口 token。
    window.transition();
    // 有界等待同标题新窗口取得不同 ID。
    let current = wait_for_recreated_session(&window.title, &previous);
    // 新旧 opaque identity 必须逐字不同。
    assert_ne!(current, previous);
    // 旧 ID 经公开 metadata read 必须 stale。
    let old_inspected = inspect_window(&previous);
    // 禁止旧 ID 绑定同标题新窗口。
    assert!(!old_inspected.status.success());
    // 解析旧目标失败 envelope。
    let old_inspected = output_json(&old_inspected);
    // 使用时重解析必须稳定 stale。
    assert_eq!(old_inspected["error"]["code"], "STALE_SESSION");
    // 新 ID 经同一公开 metadata read 必须成功。
    let new_inspected = inspect_window(&current);
    // 新代际必须可独立读取。
    assert!(new_inspected.status.success());
    // 解析新目标成功 envelope。
    let new_inspected = output_json(&new_inspected);
    // 核对版本化 metadata capability。
    assert_eq!(new_inspected["capability"], "window.metadata.read@1");
    // 核对返回目标只使用新 opaque ID。
    assert_eq!(new_inspected["window"]["sessionId"], current);
    // 经统一 app.read 尝试旧目标语义定位。
    let old_location = run_app(
        // 使用只读操作。
        "read",
        // 使用 provider-neutral 定位 capability。
        "ui.element.locate@1",
        // 绑定旧代际目标。
        &previous,
        // 使用同标题 selector，证明不会靠标题重绑定。
        json!({
            // 查找新旧共享标题。
            "selector": { "name": window.title },
            // 使用有界树深度。
            "maximumDepth": 20,
            // 使用完整性节点上限。
            "maximumItems": 4096,
            // 使用 ControlView。
            "view": "control",
            // 使用有界 deadline。
            "timeoutMs": 5000
        }),
        // 只读定位无需确认。
        false,
    );
    // 旧目标必须在 worker 前失败闭合。
    assert!(!old_location.status.success());
    // 解析公开定位错误。
    let old_location = output_json(&old_location);
    // 同标题不得覆盖旧 identity 的 stale 语义。
    assert_eq!(
        old_location["error"]["code"],
        // 对齐稳定 stale 错误码。
        "STALE_SESSION",
        // 失败时显示安全公开 envelope。
        "{old_location}"
    );
    // 核对三个公开结果的隐私边界。
    assert_public(&old_inspected);
    // 新 metadata 同样不得泄漏原生目标。
    assert_public(&new_inspected);
    // app facade 错误不得泄漏 provider 细节。
    assert_public(&old_location);
    // 整条重建与检查路线不得改变主机前景。
    assert_eq!(foreground_snapshot(), foreground_before);
}
