#![cfg(target_os = "windows")]

//! 通过真实显示拓扑验证生产窗口生命周期的多屏与 DPI 边界。

// 导入文件、路径、进程、串行锁与有界等待工具。
use std::{
    // 创建并清理工具自有结构化请求。
    fs,
    // 保存工具自有临时路径。
    path::PathBuf,
    // 启动工具自有窗口与生产 PowerShell launcher。
    process::{Child, Command, Output, Stdio},
    // 串行化真实显示拓扑窗口移动。
    sync::{Mutex, MutexGuard},
    // 在实时窗口清单之间短暂等待。
    thread,
    // 生成唯一标题并限制发现时间。
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// 导入公开 JSON 值与构造宏。
use serde_json::{Value, json};
// 导入仅供测试枚举显示拓扑和核对窗口 DPI 的 Win32 接口。
use windows::Win32::{
    // 保存回调布尔值、有符号参数和物理矩形。
    Foundation::{HWND, LPARAM, RECT},
    // 枚举当前活动显示器。
    Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR},
    // 读取当前窗口 DPI 与前景窗口。
    UI::{
        // 读取移动后窗口的当前 DPI。
        HiDpi::GetDpiForWindow,
        // 只读核对前景窗口没有变化。
        WindowsAndMessaging::{FindWindowW, GetForegroundWindow, GetWindowRect},
    },
};
// 编码唯一工具自有窗口标题。
use windows::core::{BOOL, PCWSTR};

// 固定 Cargo 构建的工具自有自绘窗口夹具。
const WINDOW_FIXTURE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-capture-fixture");
// 串行化当前真实显示拓扑上的工具自有窗口移动。
static DISPLAY_TOPOLOGY_LOCK: Mutex<()> = Mutex::new(());

// 保存 provider-neutral 物理显示器矩形。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MonitorBounds {
    // 保存左边界。
    left: i32,
    // 保存上边界。
    top: i32,
    // 保存右侧开边界。
    right: i32,
    // 保存下侧开边界。
    bottom: i32,
}

// 提供显示器尺寸和窗口落点计算。
impl MonitorBounds {
    // 返回严格正宽度。
    fn width(self) -> i32 {
        // 使用右侧开边界计算宽度。
        self.right - self.left
    }

    // 返回严格正高度。
    fn height(self) -> i32 {
        // 使用下侧开边界计算高度。
        self.bottom - self.top
    }

    // 返回保持窗口与当前显示器相交的安全左上角。
    fn fixture_origin(self) -> (i32, i32) {
        // 把 640 像素夹具尽量居中。
        let x = self.left + (self.width() - 640).max(0) / 2;
        // 把 360 像素夹具尽量居中。
        let y = self.top + (self.height() - 360).max(0) / 2;
        // 返回 provider-neutral 物理坐标。
        (x, y)
    }
}

// 保存生产窗口生命周期公开读回。
#[derive(Clone, Copy, Debug)]
struct LifecycleObservation {
    // 保存最终窗口左边界。
    x: i32,
    // 保存最终窗口上边界。
    y: i32,
    // 保存最终窗口 DPI。
    dpi: u32,
    // 保存当前虚拟桌面矩形。
    virtual_screen: MonitorBounds,
}

// 拥有一个工具自有可移动窗口进程。
struct FixtureWindow {
    // 保存公开发现使用的唯一安全标题。
    title: String,
    // 保存精确子进程生命周期。
    child: Child,
}

// 提供工具自有窗口启动、发现与私有 DPI 核对。
impl FixtureWindow {
    // 启动 no-activate 固定自绘窗口。
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
            // 只允许固定自绘模式。
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
            .unwrap_or_else(|error| panic!("display fixture launch failed: {error}"));
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
            if let Some(session) = session_for_title(&inventory, &self.title) {
                // 读取 canonical opaque window ID。
                return session["sessionId"]
                    // 要求字符串形状。
                    .as_str()
                    // 缺失 ID 表示生产契约漂移。
                    .unwrap_or_else(|| panic!("display fixture omitted sessionId"))
                    // 建立独立字符串所有权。
                    .to_owned();
            }
            // 夹具提前退出必须失败闭合。
            if self.child.try_wait().ok().flatten().is_some() {
                // 禁止回落到任意真实用户窗口。
                panic!("display fixture exited before discovery");
            }
            // 超出边界时失败。
            if Instant::now() >= deadline {
                // 报告固定发现失败。
                panic!("display fixture was not discovered");
            }
            // 短暂等待下一次生产快照。
            thread::sleep(Duration::from_millis(50));
        }
    }

    // 按唯一标题读取仅供测试交叉核对的窗口句柄。
    fn native_window(&self) -> HWND {
        // 编码唯一工具自有标题并追加终止零。
        let title = self
            // 借用当前安全标题。
            .title
            // 编码为 UTF-16。
            .encode_utf16()
            // 追加 Win32 终止零。
            .chain(std::iter::once(0))
            // 固定拥有型缓冲区生命周期。
            .collect::<Vec<_>>();
        // 只按当前测试拥有的唯一标题查找顶层窗口。
        unsafe { FindWindowW(None, PCWSTR(title.as_ptr())) }
            // 公开发现成功后私有查找也必须成功。
            .unwrap_or_else(|error| panic!("display fixture native lookup failed: {error}"))
    }

    // 读取当前窗口物理左上角供正常路径恢复。
    fn current_origin(&self) -> (i32, i32) {
        // 初始化窗口矩形。
        let mut rectangle = RECT::default();
        // 只读当前工具自有窗口外框。
        unsafe { GetWindowRect(self.native_window(), &mut rectangle) }
            // 读取失败表示夹具生命周期失效。
            .unwrap_or_else(|error| panic!("display fixture bounds failed: {error}"));
        // 返回 provider-neutral 物理坐标。
        (rectangle.left, rectangle.top)
    }

    // 按唯一标题读取移动后窗口 DPI。
    fn current_dpi(&self) -> u32 {
        // 读取当前窗口实际 DPI。
        let dpi = unsafe { GetDpiForWindow(self.native_window()) };
        // 零表示窗口无效或 DPI 上下文不可用。
        assert_ne!(dpi, 0, "display fixture DPI is unavailable");
        // 返回 provider-neutral DPI 数值。
        dpi
    }
}

// 作用域结束时只回收本测试创建的窗口进程。
impl Drop for FixtureWindow {
    // 终止并等待精确子进程。
    fn drop(&mut self) {
        // 只终止当前实例持有的夹具。
        let _ = self.child.kill();
        // 回收子进程句柄，避免残留窗口。
        let _ = self.child.wait();
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
            "act-display-topology-request-{}-{stamp}.json",
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

// 接收 EnumDisplayMonitors 的 provider-private回调参数。
unsafe extern "system" fn collect_monitor(
    // 忽略原生显示器句柄。
    _monitor: HMONITOR,
    // 忽略未使用的设备上下文。
    _device_context: HDC,
    // 读取当前显示器物理矩形。
    rectangle: *mut RECT,
    // 接收测试私有集合指针。
    state: LPARAM,
) -> BOOL {
    // 缺失矩形或集合指针必须停止枚举。
    if rectangle.is_null() || state.0 == 0 {
        // 返回失败关闭的 FALSE。
        return BOOL::from(false);
    }
    // 读取当前回调物理矩形。
    let rectangle = unsafe { *rectangle };
    // 只接受严格正面积显示器。
    if rectangle.right <= rectangle.left || rectangle.bottom <= rectangle.top {
        // 忽略异常项并继续枚举其他显示器。
        return BOOL::from(true);
    }
    // 恢复当前测试私有集合引用。
    let monitors = unsafe { &mut *(state.0 as *mut Vec<MonitorBounds>) };
    // 只保存 provider-neutral 矩形，不保留句柄或设备身份。
    monitors.push(MonitorBounds {
        // 保存左边界。
        left: rectangle.left,
        // 保存上边界。
        top: rectangle.top,
        // 保存右侧开边界。
        right: rectangle.right,
        // 保存下侧开边界。
        bottom: rectangle.bottom,
    });
    // 继续枚举全部活动显示器。
    BOOL::from(true)
}

// 枚举当前活动显示器的 provider-neutral 物理矩形。
fn monitor_topology() -> Vec<MonitorBounds> {
    // 创建测试私有结果集合。
    let mut monitors: Vec<MonitorBounds> = Vec::new();
    // 把当前集合地址传给同步枚举回调。
    let state = LPARAM((&mut monitors as *mut Vec<MonitorBounds>) as isize);
    // 枚举当前虚拟桌面的全部活动显示器。
    let completed = unsafe { EnumDisplayMonitors(None, None, Some(collect_monitor), state) };
    // 枚举失败不得解释为单显示器环境。
    assert!(completed.as_bool(), "display monitor enumeration failed");
    // 当前交互桌面至少必须有一个活动显示器。
    assert!(
        !monitors.is_empty(),
        "display monitor enumeration was empty"
    );
    // 使用有符号物理位置建立稳定顺序。
    monitors.sort_by_key(|monitor| (monitor.left, monitor.top, monitor.right, monitor.bottom));
    // 删除完全重复的矩形。
    monitors.dedup();
    // 返回 provider-neutral 拓扑。
    monitors
}

// 计算全部活动显示器的有符号虚拟桌面包围矩形。
fn virtual_screen(monitors: &[MonitorBounds]) -> MonitorBounds {
    // 读取首个已验证显示器。
    let first = monitors
        // 取得首项。
        .first()
        // monitor_topology 已保证非空。
        .copied()
        // 缺失表示调用方违反测试前置条件。
        .unwrap_or_else(|| panic!("monitor topology must not be empty"));
    // 聚合全部显示器边界。
    monitors
        .iter()
        .copied()
        .fold(first, |bounds, monitor| MonitorBounds {
            // 取最小左边界。
            left: bounds.left.min(monitor.left),
            // 取最小上边界。
            top: bounds.top.min(monitor.top),
            // 取最大右边界。
            right: bounds.right.max(monitor.right),
            // 取最大下边界。
            bottom: bounds.bottom.max(monitor.bottom),
        })
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

// 从公开窗口清单按精确标题读取 session。
fn session_for_title<'a>(inventory: &'a Value, title: &str) -> Option<&'a Value> {
    // 读取 sessions 数组。
    inventory["sessions"]
        // 要求数组形状。
        .as_array()
        // 查找标题精确匹配的窗口。
        .and_then(|sessions| sessions.iter().find(|session| session["title"] == title))
}

// 经统一 app facade 执行一个确认式窗口移动。
fn move_window(session_id: &str, x: i32, y: i32) -> Value {
    // 构造完整结构化 facade 请求。
    let request = json!({
        // 只传 canonical opaque 目标。
        "target": { "sessionId": session_id },
        // 传入稳定 capability 与 provider-neutral 输入。
        "args": {
            // 使用正式窗口生命周期 capability。
            "capability": "window.lifecycle@1",
            // 请求有符号虚拟桌面物理像素移动。
            "input": {
                // 只执行不激活移动。
                "action": "move",
                // 明确物理坐标空间。
                "coordinateSpace": "screen-physical-px",
                // 传入有符号左边界。
                "x": x,
                // 传入有符号上边界。
                "y": y,
                // 使用五秒最终读回边界。
                "timeoutMs": 5000
            }
        },
        // 提供逐操作确认。
        "confirmed": true,
        // 显式允许可见窗口几何变化。
        "foregroundConsent": true
    });
    // 写入工具自有结构化请求文件。
    let request = RequestFile::create(&request);
    // 转换请求路径供 launcher 使用。
    let path = request.path.display().to_string();
    // 经原样生产 launcher 执行移动。
    let output = launcher(&[
        // 使用统一 run 动词。
        "run",
        // 使用 app facade。
        "app",
        // 使用 generic apply。
        "apply",
        // 传入结构化请求文件。
        "--input",
        // 传入工具自有路径。
        &path,
        // 同时表达逐操作确认。
        "--confirm",
        // 同时表达前景同意。
        "--allow-foreground",
    ]);
    // 生产窗口生命周期必须完整成功。
    assert!(
        output.status.success(),
        // 失败时提供受控标准流诊断。
        "production lifecycle failed: stdout={} stderr={}",
        // 转换 stdout。
        String::from_utf8_lossy(&output.stdout),
        // 转换 stderr。
        String::from_utf8_lossy(&output.stderr)
    );
    // 解析公开结果。
    output_json(&output)
}

// 从公开窗口生命周期结果读取 provider-neutral观察值。
fn lifecycle_observation(result: &Value) -> LifecycleObservation {
    // 读取最终 bounds。
    let bounds = &result["data"]["bounds"];
    // 读取最终 virtualScreen。
    let virtual_screen = &result["data"]["virtualScreen"];
    // 构造强类型公开观察。
    LifecycleObservation {
        // 读取最终左边界。
        x: json_i32(&bounds["x"], "bounds.x"),
        // 读取最终上边界。
        y: json_i32(&bounds["y"], "bounds.y"),
        // 读取最终 DPI。
        dpi: result["data"]["dpi"]
            // 要求无符号整数形状。
            .as_u64()
            // 转换为契约允许的 u32。
            .and_then(|dpi| u32::try_from(dpi).ok())
            // 缺失表示公开契约漂移。
            .unwrap_or_else(|| panic!("lifecycle result omitted dpi")),
        // 保存有符号虚拟桌面矩形。
        virtual_screen: MonitorBounds {
            // 读取虚拟桌面左边界。
            left: json_i32(&virtual_screen["x"], "virtualScreen.x"),
            // 读取虚拟桌面上边界。
            top: json_i32(&virtual_screen["y"], "virtualScreen.y"),
            // 组合右侧开边界。
            right: json_i32(&virtual_screen["x"], "virtualScreen.x")
                // 加上正宽度。
                + json_i32(&virtual_screen["width"], "virtualScreen.width"),
            // 组合下侧开边界。
            bottom: json_i32(&virtual_screen["y"], "virtualScreen.y")
                // 加上正高度。
                + json_i32(&virtual_screen["height"], "virtualScreen.height"),
        },
    }
}

// 把 JSON 整数严格收敛为 i32。
fn json_i32(value: &Value, field: &str) -> i32 {
    // 读取有符号 JSON 整数。
    value
        // 要求整数形状。
        .as_i64()
        // 转换为公开坐标范围。
        .and_then(|value| i32::try_from(value).ok())
        // 缺失或越界表示契约漂移。
        .unwrap_or_else(|| panic!("lifecycle result omitted {field}"))
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
    for field in [
        "hwnd",
        "processId",
        "nativeHandle",
        "providerId",
        "deviceId",
        "monitorId",
    ] {
        // 任一命中都违反公开边界。
        assert!(!contains_key(value, field), "public result leaked {field}");
    }
    // 序列化完整公开结果。
    let text = value.to_string();
    // 禁止 legacy 原生窗口目标。
    assert!(!text.contains("\"window:"));
    // 禁止 Windows 显示设备名。
    assert!(!text.contains("DISPLAY\\"));
}

// 取得可恢复 poisoned 状态的动态测试串行锁。
fn fixture_guard() -> MutexGuard<'static, ()> {
    // 请求唯一显示拓扑动态窗口所有权。
    DISPLAY_TOPOLOGY_LOCK
        // 锁定真实显示拓扑场景。
        .lock()
        // 先前 panic 后仍允许清理验证继续。
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// 验证生产窗口生命周期与当前真实多屏/DPI 拓扑一致。
#[test]
#[ignore = "explicitly moves a toolkit-owned window across the current real display topology"]
fn production_window_lifecycle_matches_current_display_topology() {
    // 串行化真实桌面上的工具自有窗口移动。
    let _guard = fixture_guard();
    // 枚举当前活动显示器物理矩形。
    let monitors = monitor_topology();
    // 计算当前有符号虚拟桌面矩形。
    let expected_virtual_screen = virtual_screen(&monitors);
    // 启动 no-activate 工具自有窗口。
    let mut window = FixtureWindow::start();
    // 经生产窗口发现取得 canonical opaque ID。
    let session_id = window.session_id();
    // 保存工具自有窗口原始位置供正常路径恢复。
    let original_origin = window.current_origin();
    // 记录全部移动前的真实前景窗口。
    let foreground_before = unsafe { GetForegroundWindow() };
    // 保存每个活动显示器上的生产 DPI 读回。
    let mut dpis = Vec::new();
    // 逐一把工具自有窗口移入当前活动显示器。
    for monitor in &monitors {
        // 计算当前显示器内的安全窗口位置。
        let (x, y) = monitor.fixture_origin();
        // 经原样生产 launcher 执行不激活移动。
        let result = move_window(&session_id, x, y);
        // 核对稳定 capability ID。
        assert_eq!(result["capability"], "window.lifecycle@1");
        // 核对结果绑定原 canonical target。
        assert_eq!(result["targetId"], session_id);
        // 核对动作完整达到最终状态。
        assert_eq!(result["data"]["outcome"], "completed");
        // 核对生产 Adapter 保持 Per-Monitor-V2 物理语义。
        assert_eq!(
            result["data"]["coordinateContext"],
            "per-monitor-v2-virtual-screen-physical-px"
        );
        // 核对有符号多屏坐标承诺。
        assert_eq!(result["data"]["signedVirtualScreenCoordinates"], true);
        // 移动不得改变真实前景窗口。
        assert_eq!(result["meta"]["foreground"]["unchanged"], true);
        // 解析公开读回。
        let observation = lifecycle_observation(&result);
        // 最终左边界必须精确匹配请求。
        assert_eq!(observation.x, x);
        // 最终上边界必须精确匹配请求。
        assert_eq!(observation.y, y);
        // 生产虚拟桌面必须匹配真实显示器包围矩形。
        assert_eq!(observation.virtual_screen, expected_virtual_screen);
        // 生产 DPI 必须匹配移动后窗口的当前 DPI。
        assert_eq!(observation.dpi, window.current_dpi());
        // 保存 provider-neutral DPI 供混合 DPI 分类。
        dpis.push(observation.dpi);
        // 核对公开隐私边界。
        assert_public(&result);
        // 标题变化或移动不得重发 canonical ID。
        let inventory = window_inventory();
        // 当前窗口必须仍可由唯一标题发现。
        let current = session_for_title(&inventory, &window.title)
            // 缺失表示跨屏后发现不稳定。
            .unwrap_or_else(|| panic!("display fixture disappeared after lifecycle move"));
        // canonical ID 必须保持稳定。
        assert_eq!(current["sessionId"], session_id);
    }
    // 经同一生产路线恢复工具自有窗口原始位置。
    let restoration = move_window(&session_id, original_origin.0, original_origin.1);
    // 解析恢复后的公开读回。
    let restored = lifecycle_observation(&restoration);
    // 核对恢复后的左边界。
    assert_eq!(restored.x, original_origin.0);
    // 核对恢复后的上边界。
    assert_eq!(restored.y, original_origin.1);
    // 核对恢复结果隐私边界。
    assert_public(&restoration);
    // 移动、恢复和生产发现全过程不得改变主机前景。
    assert_eq!(unsafe { GetForegroundWindow() }, foreground_before);
    // 对 DPI 集合去重以判断当前环境是否真实 mixed-DPI。
    dpis.sort_unstable();
    // 删除重复 DPI。
    dpis.dedup();
    // 输出本次显式验证的 provider-neutral 环境结论。
    eprintln!(
        // 只输出显示器数量、DPI 集合和负原点事实。
        "display topology: monitors={}, dpis={dpis:?}, negative_origin={}",
        // 输出活动显示器数量。
        monitors.len(),
        // 输出虚拟桌面是否含负坐标。
        expected_virtual_screen.left < 0 || expected_virtual_screen.top < 0
    );
    // 单显示器只形成当前环境停止线，不外推多屏支持。
    if monitors.len() == 1 {
        // 当前测试仍已证明生产读回与单显示器真实拓扑一致。
        assert_eq!(dpis.len(), 1);
    }
    // 多显示器同 DPI 只闭合跨屏移动，不外推 mixed-DPI。
    if monitors.len() > 1 && dpis.len() == 1 {
        // 保留唯一当前 DPI 的明确环境事实。
        assert!(dpis[0] > 0);
    }
}
