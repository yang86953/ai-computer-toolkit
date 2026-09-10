#![cfg(target_os = "windows")]

//! 通过原样生产 launcher 验证 SendInput 完成不等于目标收到 Raw Input。

// 导入文件、路径、进程、串行锁与有界等待工具。
use std::{
    // 创建并清理工具自有结构化请求。
    fs,
    // 保存工具自有临时路径。
    path::PathBuf,
    // 启动 Raw Input 夹具与生产 PowerShell launcher。
    process::{Child, Command, Output, Stdio},
    // 串行化真实前景与光标影响。
    sync::{Mutex, MutexGuard},
    // 在实时窗口清单之间短暂等待。
    thread,
    // 生成唯一标题并限制发现时间。
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// 导入公开 JSON 值与构造宏。
use serde_json::{Value, json};
// 导入测试私有前景与光标恢复接口。
use windows::{
    // 导入测试私有 Win32 接口。
    Win32::{
        // 保存光标位置与窗口句柄。
        Foundation::{HWND, POINT},
        // 临时附加当前前景输入队列。
        System::Threading::{AttachThreadInput, GetCurrentThreadId},
        // 恢复光标与原前景窗口。
        UI::{
            // 导入前景窗口读取、恢复与线程查询接口。
            WindowsAndMessaging::{
                // 把原前景窗口恢复到顶层。
                BringWindowToTop,
                // 按唯一工具自有标题取得测试私有窗口句柄。
                FindWindowW,
                // 读取当前光标位置。
                GetCursorPos,
                // 读取当前前景窗口。
                GetForegroundWindow,
                // 读取窗口所属线程。
                GetWindowThreadProcessId,
                // 核对原前景窗口仍然存在。
                IsWindow,
                // 恢复原光标位置。
                SetCursorPos,
                // 请求原窗口恢复前景。
                SetForegroundWindow,
            },
        },
    },
    // 编码测试私有窗口标题。
    core::PCWSTR,
};

// 固定 Cargo 构建的工具自有 Raw Input 夹具。
const RAW_INPUT_FIXTURE: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-raw-input-fixture");
// 串行化真实桌面前景与光标状态。
static RAW_INPUT_LOCK: Mutex<()> = Mutex::new(());
// 固定夹具发布的合成 Raw Input 到达后缀。
const SYNTHETIC_SUFFIX: &str = "-raw-received-synthetic";
// 固定夹具发布的带设备来源到达后缀。
const DEVICE_SUFFIX: &str = "-raw-received-device";

// 封闭 Raw Input 到达证据类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RawInputEvidence {
    // 没有完整 WM_INPUT 消息头到达。
    None,
    // WM_INPUT 到达但消息头没有设备来源。
    Synthetic,
    // WM_INPUT 到达且消息头携带设备来源。
    Device,
}

// 固定夹具只允许鼠标或键盘 Raw Input 模式。
#[derive(Clone, Copy)]
enum FixtureMode {
    // 注册 Generic Desktop Mouse usage。
    Mouse,
    // 注册 Generic Desktop Keyboard usage。
    Keyboard,
}

// 提供封闭模式参数。
impl FixtureMode {
    // 返回夹具唯一允许的模式参数。
    fn argument(self) -> &'static str {
        // 映射固定设备类别。
        match self {
            // 选择鼠标 Raw Input。
            Self::Mouse => "--mouse",
            // 选择键盘 Raw Input。
            Self::Keyboard => "--keyboard",
        }
    }
}

// 拥有一个工具自有 Raw Input 窗口进程。
struct FixtureWindow {
    // 保存公开发现使用的唯一安全标题。
    title: String,
    // 保存精确子进程生命周期。
    child: Child,
}

// 提供 Raw Input 窗口启动、发现与到达状态读取。
impl FixtureWindow {
    // 启动一个固定设备类别的 Raw Input 窗口。
    fn start(mode: FixtureMode) -> Self {
        // 读取唯一时间戳。
        let stamp = SystemTime::now()
            // 转换为 Unix 相对时间。
            .duration_since(UNIX_EPOCH)
            // 测试时钟必须可用。
            .unwrap_or_else(|error| panic!("fixture clock failed: {error}"))
            // 使用纳秒降低碰撞概率。
            .as_nanos();
        // 构造夹具允许的唯一 ASCII 标题。
        let title = format!(
            // 使用固定安全前缀。
            "act-rust-raw-input-fixture-{}-{stamp}",
            // 加入当前测试进程 ID。
            std::process::id(),
        );
        // 启动固定 Raw Input 夹具。
        let child = Command::new(RAW_INPUT_FIXTURE)
            // 传入已受夹具校验的标题。
            .arg(&title)
            // 传入唯一固定设备模式。
            .arg(mode.argument())
            // 禁止继承输入通道。
            .stdin(Stdio::null())
            // 夹具不输出原生事实。
            .stdout(Stdio::null())
            // 夹具不写诊断信息。
            .stderr(Stdio::null())
            // 创建工具自有子进程。
            .spawn()
            // 启动失败时提供固定测试诊断。
            .unwrap_or_else(|error| panic!("raw-input fixture launch failed: {error}"));
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
                    .unwrap_or_else(|| panic!("raw-input session omitted sessionId"))
                    // 建立独立字符串所有权。
                    .to_owned();
            }
            // 夹具提前退出必须失败闭合。
            if self.child.try_wait().ok().flatten().is_some() {
                // 禁止回落到任意真实用户窗口。
                panic!("raw-input fixture exited before discovery");
            }
            // 超出边界时失败。
            if Instant::now() >= deadline {
                // 报告固定发现失败。
                panic!("raw-input fixture was not discovered");
            }
            // 短暂等待下一次生产快照。
            thread::sleep(Duration::from_millis(50));
        }
    }

    // 按唯一标题取得仅供测试建立前景的窗口句柄。
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
            .unwrap_or_else(|error| panic!("raw-input fixture native lookup failed: {error}"))
    }

    // 有界检查夹具是否实际收到 WM_INPUT。
    fn received(&mut self) -> RawInputEvidence {
        // 派生不含设备身份的合成到达标题。
        let synthetic_title = format!("{}{SYNTHETIC_SUFFIX}", self.title);
        // 派生不含具体设备身份的设备到达标题。
        let device_title = format!("{}{DEVICE_SUFFIX}", self.title);
        // 最多等待一秒让窗口消息泵处理合成输入。
        let deadline = Instant::now() + Duration::from_secs(1);
        // 轮询公开窗口标题状态。
        loop {
            // 读取当前生产窗口清单。
            let inventory = window_inventory();
            // 合成状态标题出现表示消息没有设备来源。
            if session_for_title(&inventory, &synthetic_title).is_some() {
                // 返回合成到达事实。
                return RawInputEvidence::Synthetic;
            }
            // 设备状态标题出现表示消息携带设备来源。
            if session_for_title(&inventory, &device_title).is_some() {
                // 返回设备到达事实。
                return RawInputEvidence::Device;
            }
            // 夹具提前退出不得解释为未接收。
            if self.child.try_wait().ok().flatten().is_some() {
                // 报告固定夹具生命周期失败。
                panic!("raw-input fixture exited while awaiting evidence");
            }
            // 到期表示当前合成输入未产生 WM_INPUT。
            if Instant::now() >= deadline {
                // 返回未接收事实供兼容断言。
                return RawInputEvidence::None;
            }
            // 短暂等待下一次窗口消息与公开快照。
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

// 保存测试开始前的真实桌面状态并负责恢复。
struct DesktopState {
    // 保存原前景窗口，只用于测试私有恢复。
    foreground: HWND,
    // 保存原光标位置。
    cursor: POINT,
}

// 提供桌面状态快照与恢复。
impl DesktopState {
    // 读取当前前景和光标位置。
    fn capture() -> Self {
        // 读取原前景窗口。
        let foreground = unsafe { GetForegroundWindow() };
        // 初始化光标位置。
        let mut cursor = POINT::default();
        // 当前交互桌面必须允许读取光标。
        assert!(unsafe { GetCursorPos(&mut cursor) }.is_ok());
        // 返回精确恢复状态。
        Self { foreground, cursor }
    }

    // 恢复光标和仍然存活的原前景窗口。
    fn restore(&self) {
        // 先恢复原光标位置。
        let _ = unsafe { SetCursorPos(self.cursor.x, self.cursor.y) };
        // 缺失或已销毁原前景窗口时不猜测替代目标。
        if self.foreground.is_invalid() || !unsafe { IsWindow(Some(self.foreground)) }.as_bool() {
            // 结束窗口恢复。
            return;
        }
        // 读取当前前景窗口。
        let current = unsafe { GetForegroundWindow() };
        // 读取当前测试线程 ID。
        let caller_thread = unsafe { GetCurrentThreadId() };
        // 读取当前前景窗口线程 ID。
        let current_thread = if current.is_invalid() {
            // 无前景窗口时不附加输入队列。
            0
        } else {
            // 只读取线程 ID，不读取进程 ID。
            unsafe { GetWindowThreadProcessId(current, None) }
        };
        // 仅在不同有效线程间临时附加输入队列。
        let attached = current_thread != 0
            // 同线程不需要附加。
            && current_thread != caller_thread
            // 请求测试恢复临时共享前景资格。
            && unsafe { AttachThreadInput(caller_thread, current_thread, true) }.as_bool();
        // 把原前景窗口恢复到顶层。
        let _ = unsafe { BringWindowToTop(self.foreground) };
        // 请求原窗口恢复前景。
        let _ = unsafe { SetForegroundWindow(self.foreground) };
        // 已附加时始终恢复输入队列边界。
        if attached {
            // 分离临时输入队列关联。
            let _ = unsafe { AttachThreadInput(caller_thread, current_thread, false) };
        }
    }
}

// 测试提前失败时也尝试恢复真实桌面状态。
impl Drop for DesktopState {
    // 执行 best-effort 恢复。
    fn drop(&mut self) {
        // 复用同一恢复逻辑。
        self.restore();
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
            "act-raw-input-request-{}-{stamp}.json",
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

// 从公开窗口清单按精确标题读取 session。
fn session_for_title<'a>(inventory: &'a Value, title: &str) -> Option<&'a Value> {
    // 读取 sessions 数组。
    inventory["sessions"]
        // 要求数组形状。
        .as_array()
        // 查找标题精确匹配的窗口。
        .and_then(|sessions| sessions.iter().find(|session| session["title"] == title))
}

// 经生产 launcher 执行一个确认式前景输入请求。
fn run_input(session_id: &str, capability: &str, input: Value) -> Value {
    // 构造完整结构化 facade 请求。
    let request = json!({
        // 只传 canonical opaque 目标。
        "target": { "sessionId": session_id },
        // 传入稳定 capability 与 provider-neutral 输入。
        "args": { "capability": capability, "input": input },
        // 提供逐操作确认。
        "confirmed": true,
        // 显式允许工具自有窗口成为前景。
        "foregroundConsent": true
    });
    // 写入工具自有结构化请求文件。
    let request = RequestFile::create(&request);
    // 转换请求路径供 launcher 使用。
    let path = request.path.display().to_string();
    // 经原样生产 launcher 执行输入。
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
    // SendInput dispatch 必须由生产路线完整接受。
    assert!(
        output.status.success(),
        // 失败时提供受控标准流诊断。
        "production input failed: stdout={} stderr={}",
        // 转换 stdout。
        String::from_utf8_lossy(&output.stdout),
        // 转换 stderr。
        String::from_utf8_lossy(&output.stderr)
    );
    // 解析公开输入结果。
    let value = output_json(&output);
    // 核对稳定 capability ID。
    assert_eq!(value["capability"], capability);
    // 核对生产 Module 只声明 dispatch 完成。
    assert_eq!(value["data"]["outcome"], "completed");
    // 返回完整公开结果供隐私断言。
    value
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
    ] {
        // 任一命中都违反公开边界。
        assert!(!contains_key(value, field), "public result leaked {field}");
    }
    // 序列化完整公开结果。
    let text = value.to_string();
    // 禁止 legacy 原生窗口目标。
    assert!(!text.contains("\"window:"));
    // 禁止 Raw Input 设备事实。
    assert!(!text.contains("WM_INPUT"));
}

// 取得可恢复 poisoned 状态的动态测试串行锁。
fn fixture_guard() -> MutexGuard<'static, ()> {
    // 请求唯一 Raw Input 动态窗口所有权。
    RAW_INPUT_LOCK
        // 锁定真实前景与光标场景。
        .lock()
        // 先前 panic 后仍允许清理验证继续。
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// 仅为工具自有 Raw Input 夹具建立确定性前景起点。
fn establish_fixture_foreground(window: HWND) {
    // 读取当前主机前景窗口。
    let previous = unsafe { GetForegroundWindow() };
    // 读取当前测试线程 ID。
    let caller_thread = unsafe { GetCurrentThreadId() };
    // 读取原前景窗口线程 ID。
    let previous_thread = if previous.is_invalid() {
        // 无前景窗口时不附加输入队列。
        0
    } else {
        // 只读取线程 ID，不读取进程 ID。
        unsafe { GetWindowThreadProcessId(previous, None) }
    };
    // 仅在不同有效线程间临时附加输入队列。
    let attached = previous_thread != 0
        // 同线程不需要附加。
        && previous_thread != caller_thread
        // 请求测试夹具临时共享前景资格。
        && unsafe { AttachThreadInput(caller_thread, previous_thread, true) }.as_bool();
    // 把工具自有窗口提升到顶层。
    let _ = unsafe { BringWindowToTop(window) };
    // 请求工具自有窗口成为前景。
    let _ = unsafe { SetForegroundWindow(window) };
    // 已附加时始终恢复原输入队列边界。
    if attached {
        // 分离临时输入队列关联。
        let _ = unsafe { AttachThreadInput(caller_thread, previous_thread, false) };
    }
    // 生产输入前必须确认目标确实成为前景。
    assert_eq!(unsafe { GetForegroundWindow() }, window);
}

// 通过固定 mouse 或 keyboard 夹具执行一个生产输入场景。
fn scenario(mode: FixtureMode, capability: &str, input: Value) -> RawInputEvidence {
    // 启动当前固定设备类别夹具。
    let mut window = FixtureWindow::start(mode);
    // 经生产窗口发现取得 canonical opaque ID。
    let session_id = window.session_id();
    // 取得仅测试私有的精确窗口句柄。
    let native_window = window.native_window();
    // 在调用生产 launcher 前建立确定性前景起点。
    establish_fixture_foreground(native_window);
    // 经原样生产 launcher 完成 SendInput dispatch。
    let result = run_input(&session_id, capability, input);
    // 核对公开输入结果隐私边界。
    assert_public(&result);
    // 返回目标收到的分层 Raw Input 证据。
    window.received()
}

// 验证生产 SendInput 完成不能外推为 Raw Input 设备数据到达。
#[test]
#[ignore = "explicitly changes the real foreground and cursor for toolkit-owned Raw Input fixtures"]
fn production_send_input_is_not_raw_device_delivery() {
    // 串行化真实桌面影响。
    let _guard = fixture_guard();
    // 保存并保证恢复原前景与光标。
    let desktop = DesktopState::capture();
    // 对 Raw Mouse 夹具执行窗口 client 左键单击。
    let mouse_received = scenario(
        // 只注册 mouse usage。
        FixtureMode::Mouse,
        // 使用正式指针 capability。
        "ui.input.pointer@1",
        // 使用固定窗口 client 点和有界序列。
        json!({
            // 避免依赖真实屏幕坐标。
            "coordinateSpace": "window-client-physical-px",
            // 只执行一次完整单击。
            "steps": [{
                // 使用固定 click 原语。
                "type": "click",
                // 只使用左键。
                "button": "left",
                // 使用夹具 client 内部固定点。
                "point": { "x": 64, "y": 64 }
            }],
            // 使用有界前景执行预算。
            "timeoutMs": 5000
        }),
    );
    // 对独立 Raw Keyboard 夹具执行一个命名键 press。
    let keyboard_received = scenario(
        // 只注册 keyboard usage。
        FixtureMode::Keyboard,
        // 使用正式键盘 capability。
        "ui.input.key@1",
        // 使用固定无系统副作用的 F6 键。
        json!({ "key": "F6", "timeoutMs": 5000 }),
    );
    // 在断言前恢复主人原前景与光标。
    desktop.restore();
    // 鼠标 SendInput 只能形成没有设备来源的 Raw Input 消息。
    assert_eq!(mouse_received, RawInputEvidence::Synthetic);
    // 键盘 SendInput 同样只能形成没有设备来源的 Raw Input 消息。
    assert_eq!(keyboard_received, RawInputEvidence::Synthetic);
}
