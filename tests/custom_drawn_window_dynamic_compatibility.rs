#![cfg(target_os = "windows")]

//! 通过原样生产 launcher 验证自绘窗口的发现、截图与语义失败闭合。

// 导入文件、路径、进程、串行锁与有界等待工具。
use std::{
    // 创建并清理工具自有请求与截图目录。
    fs,
    // 保存工具自有临时路径。
    path::PathBuf,
    // 启动自绘夹具与生产 PowerShell launcher。
    process::{Child, Command, Output, Stdio},
    // 串行化主机前景不变断言。
    sync::{Mutex, MutexGuard},
    // 在实时窗口清单之间短暂等待。
    thread,
    // 生成唯一标题并限制发现时间。
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// 导入 PNG 解码入口。
use image::ImageReader;
// 导入公开 JSON 值与构造宏。
use serde_json::{Value, json};
// 导入只读前景窗口查询，原生值不进入公开结果。
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

// 固定 Cargo 构建的工具自有自绘窗口夹具。
const WINDOW_FIXTURE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-capture-fixture");
// 固定生产窗口截图 worker，确保测试产物布局完整。
const CAPTURE_WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-capture-worker");
// 固定生产语义动作 worker，确保测试产物布局完整。
const SEMANTIC_WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-semantic-action-worker");
// 串行化当前真实桌面上的 no-activate 夹具。
static CUSTOM_DRAWN_LOCK: Mutex<()> = Mutex::new(());

// 拥有一个工具自有自绘窗口进程。
struct FixtureWindow {
    // 保存公开发现使用的唯一安全标题。
    title: String,
    // 保存精确子进程生命周期。
    child: Child,
}

// 提供自绘窗口启动与发现。
impl FixtureWindow {
    // 启动无标准子控件的 no-activate 自绘窗口。
    fn start() -> Self {
        // 读取唯一时间戳。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时间。
            .duration_since(UNIX_EPOCH)
            // 测试时钟必须可用。
            .unwrap_or_else(|error| panic!("fixture clock failed: {error}"))
            // 使用纳秒降低碰撞概率。
            .as_nanos();
        // 构造夹具允许的唯一 ASCII 标题。
        let title = format!("act-rust-capture-fixture-{}-{stamp}", std::process::id());
        // 启动固定自绘模式。
        let child = Command::new(WINDOW_FIXTURE)
            // 传入已受夹具校验的标题。
            .arg(&title)
            // 只允许固定自绘模式参数。
            .arg("--custom-drawn")
            // 禁止继承输入通道。
            .stdin(Stdio::null())
            // 夹具不输出原生事实。
            .stdout(Stdio::null())
            // 夹具不写诊断信息。
            .stderr(Stdio::null())
            // 创建工具自有子进程。
            .spawn()
            // 启动失败时提供固定测试诊断。
            .unwrap_or_else(|error| panic!("custom-drawn fixture launch failed: {error}"));
        // 返回精确生命周期所有者。
        Self { title, child }
    }

    // 经原样生产 launcher 有界等待 canonical session。
    fn session_id(&mut self) -> String {
        // 设置五秒发现边界。
        let deadline = Instant::now() + Duration::from_secs(5);
        // 轮询实时公开窗口清单。
        loop {
            // 获取当前生产窗口发现快照。
            let inventory = window_inventory();
            // 按唯一标题查找当前 session。
            if let Some(session) = inventory["sessions"]
                // 要求 sessions 数组形状。
                .as_array()
                // 精确匹配工具自有标题。
                .and_then(|sessions| {
                    // 返回唯一标题对应的公开 session。
                    sessions
                        .iter()
                        .find(|session| session["title"] == self.title)
                })
            {
                // 读取 canonical opaque window ID。
                return session["sessionId"]
                    // 要求字符串形状。
                    .as_str()
                    // 缺失 ID 表示生产契约漂移。
                    .unwrap_or_else(|| panic!("custom-drawn session omitted sessionId"))
                    // 建立独立字符串所有权。
                    .to_owned();
            }
            // 夹具提前退出必须失败闭合。
            if self.child.try_wait().ok().flatten().is_some() {
                // 禁止回落到任意真实用户窗口。
                panic!("custom-drawn fixture exited before discovery");
            }
            // 超出边界时失败。
            if Instant::now() >= deadline {
                // 报告固定发现失败。
                panic!("custom-drawn fixture was not discovered");
            }
            // 短暂等待下一次生产快照。
            thread::sleep(Duration::from_millis(50));
        }
    }
}

// 作用域结束时只回收本测试创建的窗口进程。
impl Drop for FixtureWindow {
    // 终止并等待精确子进程。
    fn drop(&mut self) {
        // 只终止当前实例持有的夹具。
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
            "act-custom-drawn-dynamic-{}-{stamp}",
            // 加入当前测试进程 ID。
            std::process::id(),
        ));
        // 创建精确工具自有目录。
        fs::create_dir(&path)
            // 创建失败时提供固定测试诊断。
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
            "act-custom-drawn-request-{}-{stamp}.json",
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
        .join("tools")
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

// 经统一 app facade 执行一个完整结构化请求。
fn run_app(operation: &str, request: &Value, strict: bool) -> Output {
    // 写入工具自有结构化请求文件。
    let request = RequestFile::create(request);
    // 转换请求路径供 launcher 使用。
    let path = request.path.display().to_string();
    // 构造固定生产 launcher 参数。
    let mut arguments = vec![
        // 使用统一 run 动词。
        "run",     // 使用 app facade。
        "app",     // 传入调用方封闭操作。
        operation, // 使用结构化请求文件。
        "--input", // 传入工具自有路径。
        &path,
    ];
    // 严格截图路线追加零干扰要求。
    if strict {
        // 禁止静默回退当前桌面输入。
        arguments.push("--strict-isolation");
    }
    // 经原样生产 launcher 执行请求。
    launcher(&arguments)
}

// 读取当前前景窗口的测试私有数值快照。
fn foreground_snapshot() -> isize {
    // 原生值只用于相等性断言，永不进入公开 JSON。
    unsafe { GetForegroundWindow() }.0 as isize
}

// 取得可恢复 poisoned 状态的动态测试串行锁。
fn fixture_guard() -> MutexGuard<'static, ()> {
    // 请求唯一自绘动态窗口所有权。
    CUSTOM_DRAWN_LOCK
        // 锁定当前真实桌面场景。
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

// 统计截图中固定自绘标记的近似像素数量。
fn marker_pixel_count(path: &PathBuf) -> usize {
    // 从原子提交后的固定 PNG 路径创建解码器。
    let image = ImageReader::open(path)
        // 文件必须可读取。
        .unwrap_or_else(|error| panic!("custom-drawn screenshot open failed: {error}"))
        // 解码 PNG 像素。
        .decode()
        // 文件必须是有效图像。
        .unwrap_or_else(|error| panic!("custom-drawn screenshot decode failed: {error}"))
        // 统一转换为 RGB8。
        .to_rgb8();
    // 统计接近固定 RGB 31、127、223 的像素。
    image
        // 遍历完整截图像素。
        .pixels()
        // 允许捕获路径上的窄颜色转换误差。
        .filter(|pixel| {
            // 红色通道必须接近固定值。
            pixel[0].abs_diff(31) <= 4
                // 绿色通道必须接近固定值。
                && pixel[1].abs_diff(127) <= 4
                // 蓝色通道必须接近固定值。
                && pixel[2].abs_diff(223) <= 4
        })
        // 返回命中像素总数。
        .count()
}

// 验证自绘内容可发现和截图，但不会被猜测为可语义动作控件。
#[test]
fn custom_drawn_window_is_observable_and_semantic_action_fails_closed() {
    // 串行化真实桌面前景不变断言。
    let _guard = fixture_guard();
    // Cargo 必须构建两个固定生产 worker。
    assert!(PathBuf::from(CAPTURE_WORKER).is_file());
    // 语义动作 worker 同样必须存在。
    assert!(PathBuf::from(SEMANTIC_WORKER).is_file());
    // 启动无标准子控件的工具自有自绘窗口。
    let mut window = FixtureWindow::start();
    // 经生产窗口发现取得 canonical opaque ID。
    let session_id = window.session_id();
    // 记录所有操作前的前景窗口。
    let foreground_before = foreground_snapshot();
    // 创建精确工具自有输出目录。
    let directory = FixtureDirectory::create();
    // 构造唯一截图输出路径。
    let screenshot_path = directory.join("custom-drawn.png");
    // 构造已确认严格截图请求。
    let screenshot_request = json!({
        // 只传 canonical opaque 目标。
        "target": { "sessionId": session_id },
        // 传入稳定 capability 与封闭输入。
        "args": {
            // 使用正式窗口截图 capability。
            "capability": "window.screenshot@1",
            // 只允许固定输出路径与有界 deadline。
            "input": { "path": screenshot_path, "timeoutMs": 5000 }
        },
        // 提供逐操作确认。
        "confirmed": true,
        // 要求严格零干扰路线。
        "isolationRequirement": "strict"
    });
    // 经原样生产 launcher 执行截图。
    let screenshot = run_app("screenshot", &screenshot_request, true);
    // 自绘可见窗口截图必须成功。
    assert!(
        screenshot.status.success(),
        // 失败时提供受控标准流诊断。
        "custom-drawn screenshot failed: stdout={} stderr={}",
        // 转换 stdout。
        String::from_utf8_lossy(&screenshot.stdout),
        // 转换 stderr。
        String::from_utf8_lossy(&screenshot.stderr)
    );
    // 解析公开截图结果。
    let screenshot = output_json(&screenshot);
    // 核对稳定 capability ID。
    assert_eq!(screenshot["capability"], "window.screenshot@1");
    // 核对结果绑定原 canonical target。
    assert_eq!(screenshot["targetId"], session_id);
    // worker 必须在 facade data 内声明前景不变。
    assert_eq!(screenshot["data"]["foregroundUnchanged"], true);
    // facade 自身也必须认证执行前后前景不变。
    assert_eq!(screenshot["meta"]["foreground"]["unchanged"], true);
    // 原子输出必须已经提交 PNG。
    assert!(screenshot_path.is_file());
    // 固定自绘区域必须产生大量可识别像素。
    assert!(
        marker_pixel_count(&screenshot_path) >= 8_000,
        // 失败时说明捕获没有观察到自绘内容。
        "custom-drawn marker was not present in the production screenshot"
    );
    // 构造不存在的自绘语义节点动作请求。
    let semantic_request = json!({
        // 复用同一 canonical opaque 目标。
        "target": { "sessionId": session_id },
        // 使用正式语义动作 capability。
        "args": {
            // 绑定稳定 capability ID。
            "capability": "ui.element.action@1",
            // 只传 provider-neutral selector 与封闭动作。
            "input": {
                // 自绘像素没有对应标准语义节点。
                "selector": { "name": format!("{}-painted-action", window.title) },
                // 请求标准 Invoke，不允许指针替代。
                "action": { "type": "invoke" },
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
        // 显式确认当前 mutation 尝试。
        "confirmed": true
    });
    // 经原样生产 launcher 执行语义动作。
    let semantic = run_app("apply", &semantic_request, false);
    // 不存在的自绘节点必须结构化失败。
    assert!(!semantic.status.success());
    // 解析公开失败 envelope。
    let semantic = output_json(&semantic);
    // 完整树零命中必须保持稳定错误码。
    assert_eq!(semantic["error"]["code"], "ELEMENT_NOT_FOUND", "{semantic}");
    // 禁止把自绘像素静默转换为指针 fallback。
    assert!(!semantic.to_string().contains("pointerFallbackUsed\":true"));
    // 禁止错误结果携带屏幕坐标。
    assert!(!semantic.to_string().contains("screen-px"));
    // 核对截图结果的公开隐私边界。
    assert_public(&screenshot);
    // 核对语义失败的公开隐私边界。
    assert_public(&semantic);
    // 发现、截图与失败路线都不得改变主机前景。
    assert_eq!(foreground_snapshot(), foreground_before);
}
