#![cfg(target_os = "windows")]

//! 通过真实 DXGI 独占窗口验证生产发现与截图兼容边界。

// 导入文件、控制输入、路径、进程、串行锁与有界等待工具。
use std::{
    // 创建并清理工具自有请求和截图目录。
    fs,
    // 向固定夹具写入 enter/exit 命令。
    io::Write,
    // 保存工具自有临时路径。
    path::PathBuf,
    // 启动独占夹具与生产 PowerShell launcher。
    process::{Child, ChildStdin, Command, Output, Stdio},
    // 串行化真实前景与显示模式影响。
    sync::{Mutex, MutexGuard},
    // 在实时窗口清单之间短暂等待。
    thread,
    // 生成唯一标题并限制状态等待时间。
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// 导入公开 JSON 值与构造宏。
use serde_json::{Value, json};
// 导入仅供测试建立和恢复前景的 Win32 接口。
use windows::{
    // 导入测试私有 Win32 接口。
    Win32::{
        // 保存窗口句柄。
        Foundation::HWND,
        // 临时附加当前前景输入队列。
        System::Threading::{AttachThreadInput, GetCurrentThreadId},
        // 导入前景窗口读取、恢复与线程查询接口。
        UI::WindowsAndMessaging::{
            // 把窗口恢复到顶层。
            BringWindowToTop,
            // 按唯一工具自有标题取得测试私有窗口句柄。
            FindWindowW,
            // 读取当前前景窗口。
            GetForegroundWindow,
            // 读取窗口所属线程。
            GetWindowThreadProcessId,
            // 核对原前景窗口仍然存在。
            IsWindow,
            // 请求窗口成为前景。
            SetForegroundWindow,
        },
    },
    // 编码唯一工具自有窗口标题。
    core::PCWSTR,
};

// 固定 Cargo 构建的工具自有 DXGI 独占夹具。
const EXCLUSIVE_FIXTURE: &str =
    env!("CARGO_BIN_EXE_ai-computer-toolkit-exclusive-fullscreen-fixture");
// 固定生产窗口截图 worker，确保测试产物布局完整。
const CAPTURE_WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-capture-worker");
// 固定成功进入独占的标题后缀。
const EXCLUSIVE_SUFFIX: &str = "-exclusive";
// 固定当前会话不可进入独占的标题后缀。
const UNAVAILABLE_SUFFIX: &str = "-exclusive-unavailable";
// 固定独占状态意外丢失的标题后缀。
const LOST_SUFFIX: &str = "-exclusive-lost";
// 固定意外平台错误的标题后缀。
const FAILED_SUFFIX: &str = "-exclusive-failed";
// 串行化真实桌面前景与显示模式状态。
static EXCLUSIVE_LOCK: Mutex<()> = Mutex::new(());

// 封闭 DXGI 独占进入状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FullscreenState {
    // GetFullscreenState 已证明真实独占。
    Exclusive,
    // DXGI 明确报告当前会话不可用。
    Unavailable,
    // 独占在验证期间意外丢失。
    Lost,
    // 夹具遇到非官方可继续错误。
    Failed,
}

// 拥有一个工具自有 DXGI 独占窗口进程。
struct FixtureWindow {
    // 保存公开发现使用的唯一安全标题。
    title: String,
    // 保存固定控制输入通道。
    stdin: Option<ChildStdin>,
    // 保存精确子进程生命周期。
    child: Child,
}

// 提供独占窗口启动、发现、控制与有界恢复。
impl FixtureWindow {
    // 启动 windowed 初始状态的 D3D11/DXGI 夹具。
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
        let title = format!(
            // 使用固定安全前缀。
            "act-rust-exclusive-fixture-{}-{stamp}",
            // 加入当前测试进程 ID。
            std::process::id(),
        );
        // 启动固定 D3D11/DXGI 夹具。
        let mut child = Command::new(EXCLUSIVE_FIXTURE)
            // 传入已受夹具校验的标题。
            .arg(&title)
            // 建立固定控制输入通道。
            .stdin(Stdio::piped())
            // 夹具不输出原生事实。
            .stdout(Stdio::null())
            // 夹具不写诊断信息。
            .stderr(Stdio::null())
            // 创建工具自有子进程。
            .spawn()
            // 启动失败时提供固定测试诊断。
            .unwrap_or_else(|error| panic!("exclusive fixture launch failed: {error}"));
        // 取得唯一控制输入通道。
        let stdin = child
            // 从子进程取出 stdin 所有权。
            .stdin
            // Cargo 启用 piped 后必须存在。
            .take()
            // 缺失通道表示夹具启动契约失效。
            .unwrap_or_else(|| panic!("exclusive fixture omitted control stdin"));
        // 返回精确生命周期所有者。
        Self {
            // 保存唯一标题。
            title,
            // 保存控制通道。
            stdin: Some(stdin),
            // 保存子进程。
            child,
        }
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
                return session_id(session, "exclusive fixture");
            }
            // 夹具提前退出必须失败闭合。
            if self.child.try_wait().ok().flatten().is_some() {
                // 禁止回落到任意真实用户窗口。
                panic!("exclusive fixture exited before discovery");
            }
            // 超出边界时失败。
            if Instant::now() >= deadline {
                // 报告固定发现失败。
                panic!("exclusive fixture was not discovered");
            }
            // 短暂等待下一次生产快照。
            thread::sleep(Duration::from_millis(50));
        }
    }

    // 按唯一初始标题取得仅供测试建立前景的窗口句柄。
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
            .unwrap_or_else(|error| panic!("exclusive fixture native lookup failed: {error}"))
    }

    // 请求夹具调用唯一真实独占入口。
    fn enter(&mut self) {
        // 控制通道在首次 enter 前必须存在。
        let stdin = self
            // 借用当前控制通道。
            .stdin
            // 取得可变引用。
            .as_mut()
            // 缺失通道表示测试生命周期错误。
            .unwrap_or_else(|| panic!("exclusive fixture control channel is closed"));
        // 写入唯一固定进入命令。
        writeln!(stdin, "enter")
            // 写入失败表示夹具已经异常退出。
            .unwrap_or_else(|error| panic!("exclusive fixture enter failed: {error}"));
        // 立即刷新控制命令。
        stdin
            // 刷新管道缓冲区。
            .flush()
            // 刷新失败表示夹具无法继续。
            .unwrap_or_else(|error| panic!("exclusive fixture enter flush failed: {error}"));
    }

    // 等待夹具发布封闭独占状态并核对 canonical ID 稳定。
    fn wait_state(&mut self, expected_session_id: &str) -> FullscreenState {
        // 设置五秒状态边界。
        let deadline = Instant::now() + Duration::from_secs(5);
        // 派生全部固定状态标题。
        let states = [
            // 真实独占成功。
            (EXCLUSIVE_SUFFIX, FullscreenState::Exclusive),
            // 当前会话不可用。
            (UNAVAILABLE_SUFFIX, FullscreenState::Unavailable),
            // 独占意外丢失。
            (LOST_SUFFIX, FullscreenState::Lost),
            // 非官方错误。
            (FAILED_SUFFIX, FullscreenState::Failed),
        ];
        // 轮询实时公开窗口清单。
        loop {
            // 获取当前生产窗口发现快照。
            let inventory = window_inventory();
            // 检查每个封闭状态标题。
            for (suffix, state) in states {
                // 派生当前状态的精确标题。
                let title = format!("{}{suffix}", self.title);
                // 只接受唯一精确标题。
                if let Some(session) = session_for_title(&inventory, &title) {
                    // 标题变化不得重发 canonical target。
                    assert_eq!(session_id(session, "exclusive state"), expected_session_id);
                    // 返回已证明状态。
                    return state;
                }
            }
            // 夹具提前退出必须失败闭合。
            if self.child.try_wait().ok().flatten().is_some() {
                // 禁止把进程退出解释为环境不可用。
                panic!("exclusive fixture exited before publishing state");
            }
            // 超出边界时失败。
            if Instant::now() >= deadline {
                // 报告固定状态发现失败。
                panic!("exclusive fixture did not publish a fullscreen state");
            }
            // 短暂等待下一次生产快照。
            thread::sleep(Duration::from_millis(50));
        }
    }

    // 请求 windowed 恢复并有界等待夹具正常退出。
    fn finish(&mut self) {
        // 仍有控制通道时发送固定退出命令。
        if let Some(mut stdin) = self.stdin.take() {
            // 写入失败由后续退出状态统一判断。
            let _ = writeln!(stdin, "exit");
            // 尽力立即发送恢复命令。
            let _ = stdin.flush();
        }
        // 正常恢复最多等待三秒。
        let deadline = Instant::now() + Duration::from_secs(3);
        // 轮询精确子进程退出。
        loop {
            // 夹具退出时核对成功状态。
            if let Some(status) = self.child.try_wait().ok().flatten() {
                // 恢复失败必须令测试失败。
                assert!(status.success(), "exclusive fixture restoration failed");
                // 完成正常回收。
                return;
            }
            // 超时表示恢复协议失效。
            if Instant::now() >= deadline {
                // 禁止在可能独占时静默杀进程。
                panic!("exclusive fixture did not restore within three seconds");
            }
            // 短暂等待窗口消息泵处理退出。
            thread::sleep(Duration::from_millis(20));
        }
    }
}

// 作用域结束时先请求恢复，再等待夹具的十秒强制恢复边界。
impl Drop for FixtureWindow {
    // 回收精确子进程且不在可能独占时立即终止。
    fn drop(&mut self) {
        // 仍有控制通道时请求固定安全退出。
        if let Some(mut stdin) = self.stdin.take() {
            // 写入失败表示子进程可能已经退出。
            let _ = writeln!(stdin, "exit");
            // 尽力立即刷新恢复命令。
            let _ = stdin.flush();
        }
        // 覆盖夹具十秒独占上限并留出消息泵余量。
        let deadline = Instant::now() + Duration::from_secs(12);
        // 有界等待精确子进程退出。
        loop {
            // 已退出时完成回收。
            if self.child.try_wait().ok().flatten().is_some() {
                // 结束 Drop。
                return;
            }
            // 超过夹具强制恢复边界后才允许终止残留进程。
            if Instant::now() >= deadline {
                // 只终止当前测试拥有的精确子进程。
                let _ = self.child.kill();
                // 回收精确子进程句柄。
                let _ = self.child.wait();
                // 结束 Drop。
                return;
            }
            // 短暂等待固定恢复协议。
            thread::sleep(Duration::from_millis(20));
        }
    }
}

// 保存测试开始前的真实前景窗口并负责恢复。
struct DesktopState {
    // 保存原前景窗口，只用于测试私有恢复。
    foreground: HWND,
}

// 提供前景状态快照与恢复。
impl DesktopState {
    // 读取当前前景窗口。
    fn capture() -> Self {
        // 保存测试开始前的精确前景窗口。
        Self {
            // 读取当前前景窗口。
            foreground: unsafe { GetForegroundWindow() },
        }
    }

    // 恢复仍然存活的原前景窗口。
    fn restore(&self) {
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

// 测试提前失败时也尝试恢复真实前景。
impl Drop for DesktopState {
    // 执行 best-effort 恢复。
    fn drop(&mut self) {
        // 复用同一恢复逻辑。
        self.restore();
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
            "act-exclusive-dynamic-{}-{stamp}",
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

// 作用域结束时清理精确工具自有临时目录。
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
            "act-exclusive-request-{}-{stamp}.json",
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

// 从公开 session 读取 canonical opaque ID。
fn session_id(session: &Value, context: &str) -> String {
    // 要求字符串形状并建立独立所有权。
    session["sessionId"]
        // 读取字符串。
        .as_str()
        // 缺失 ID 表示生产契约漂移。
        .unwrap_or_else(|| panic!("{context} omitted sessionId"))
        // 建立独立字符串所有权。
        .to_owned()
}

// 经统一 app facade 执行已确认严格截图。
fn run_screenshot(session_id: &str, path: &PathBuf) -> Output {
    // 构造完整结构化 facade 请求。
    let request = json!({
        // 只传 canonical opaque 目标。
        "target": { "sessionId": session_id },
        // 传入稳定 capability 与封闭输入。
        "args": {
            // 使用正式窗口截图 capability。
            "capability": "window.screenshot@1",
            // 只允许固定输出路径与有界 deadline。
            "input": { "path": path, "timeoutMs": 5000 }
        },
        // 提供逐操作确认。
        "confirmed": true,
        // 要求严格零干扰路线。
        "isolationRequirement": "strict"
    });
    // 写入工具自有结构化请求文件。
    let request = RequestFile::create(&request);
    // 转换请求路径供 launcher 使用。
    let request_path = request.path.display().to_string();
    // 经原样生产 launcher 执行截图。
    launcher(&[
        // 使用统一 run 动词。
        "run",
        // 使用 app facade。
        "app",
        // 使用正式截图操作。
        "screenshot",
        // 传入结构化请求文件。
        "--input",
        // 传入工具自有路径。
        &request_path,
        // 禁止静默回退当前桌面输入。
        "--strict-isolation",
        // 同时表达逐操作确认。
        "--confirm",
    ])
}

// 取得可恢复 poisoned 状态的动态测试串行锁。
fn fixture_guard() -> MutexGuard<'static, ()> {
    // 请求唯一独占全屏动态窗口所有权。
    EXCLUSIVE_LOCK
        // 锁定真实前景与显示模式场景。
        .lock()
        // 先前 panic 后仍允许清理验证继续。
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// 仅为工具自有 DXGI 夹具建立确定性前景起点。
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
    // 进入独占前必须确认目标确实成为前景。
    assert_eq!(unsafe { GetForegroundWindow() }, window);
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
    // 禁止泄露 DXGI provider 类型。
    assert!(!text.contains("IDXGISwapChain"));
}

// 验证真实独占窗口可经生产路线发现并截图，或明确报告当前环境不可用。
#[test]
#[ignore = "explicitly changes the real foreground and display mode for a toolkit-owned DXGI fixture"]
fn production_discovery_and_capture_respect_exclusive_fullscreen_boundary() {
    // 串行化真实桌面影响。
    let _guard = fixture_guard();
    // Cargo 必须构建固定生产截图 worker。
    assert!(PathBuf::from(CAPTURE_WORKER).is_file());
    // 保存并保证恢复原前景窗口。
    let desktop = DesktopState::capture();
    // 启动 windowed 初始状态的工具自有 DXGI 夹具。
    let mut window = FixtureWindow::start();
    // 经原样生产发现取得 canonical opaque ID。
    let initial_session_id = window.session_id();
    // 取得仅供测试建立前景的精确窗口句柄。
    let native_window = window.native_window();
    // 为真实独占调用建立确定性前景起点。
    establish_fixture_foreground(native_window);
    // 请求夹具调用 SetFullscreenState(TRUE)。
    window.enter();
    // 经生产窗口发现读取封闭进入状态。
    let state = window.wait_state(&initial_session_id);
    // 当前会话明确不可用是官方允许的平台结论。
    if state == FullscreenState::Unavailable {
        // 输出本次显式验证的环境结论。
        eprintln!("exclusive fullscreen is unavailable in the current session");
        // 要求夹具先恢复 windowed 再正常退出。
        window.finish();
        // 恢复原前景窗口。
        desktop.restore();
        // 完成当前环境不可用分支。
        return;
    }
    // 非官方错误不得伪装为环境不可用。
    assert_eq!(
        state,
        FullscreenState::Exclusive,
        "unexpected state: {state:?}"
    );
    // 创建精确工具自有输出目录。
    let directory = FixtureDirectory::create();
    // 构造唯一截图输出路径。
    let screenshot_path = directory.join("exclusive-fullscreen.png");
    // 经原样生产 launcher 执行严格截图。
    let screenshot = run_screenshot(&initial_session_id, &screenshot_path);
    // 真实独占窗口截图必须成功或返回可审计结构化失败。
    let screenshot = output_json(&screenshot);
    // 核对稳定 capability ID 或结构化错误形状。
    if screenshot.get("error").is_some() {
        // 失败 envelope 必须有稳定非空错误码。
        assert!(
            screenshot["error"]["code"]
                // 读取字符串错误码。
                .as_str()
                // 要求非空。
                .is_some_and(|code| !code.is_empty()),
            // 输出完整公开结果供失败诊断。
            "exclusive screenshot omitted a structured error code: {screenshot}"
        );
        // 结构化失败不得留下伪成功截图。
        assert!(!screenshot_path.exists());
        // 输出本次显式验证的截图边界。
        eprintln!("exclusive screenshot result: {screenshot}");
    } else {
        // 成功结果必须绑定稳定 capability。
        assert_eq!(screenshot["capability"], "window.screenshot@1");
        // 成功结果必须绑定原 canonical target。
        assert_eq!(screenshot["targetId"], initial_session_id);
        // worker 必须声明截图没有改变前景。
        assert_eq!(screenshot["data"]["foregroundUnchanged"], true);
        // facade 必须认证执行前后前景不变。
        assert_eq!(screenshot["meta"]["foreground"]["unchanged"], true);
        // 原子输出必须已经提交 PNG。
        assert!(screenshot_path.is_file());
        // 输出本次显式验证的成功边界。
        eprintln!("exclusive screenshot succeeded through the production launcher");
    }
    // 核对截图公开结果隐私边界。
    assert_public(&screenshot);
    // 截图后再次通过生产发现核对独占没有被静默破坏。
    let state_after_capture = window.wait_state(&initial_session_id);
    // 生产截图必须保留真实独占状态。
    assert_eq!(
        state_after_capture,
        FullscreenState::Exclusive,
        "production capture disrupted exclusive fullscreen"
    );
    // 要求夹具先恢复 windowed 再正常退出。
    window.finish();
    // 恢复原前景窗口。
    desktop.restore();
}
