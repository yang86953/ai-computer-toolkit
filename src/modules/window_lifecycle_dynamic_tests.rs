//! 通过项目自有 no-activate 顶层窗口验证生命周期生产路由与接受后语义。

// 导入窗口句柄转换、线程协调和有界等待工具。
use std::{
    // 导入私有窗口句柄转换类型。
    ffi::c_void,
    // 导入跨线程状态与串行锁。
    sync::{
        // 导入引用计数与互斥锁所有权。
        Arc,
        Mutex,
        MutexGuard,
        // 导入原子状态与内存顺序。
        atomic::{AtomicBool, AtomicIsize, Ordering},
        // 导入夹具就绪通道。
        mpsc,
    },
    // 导入独立 UI 线程。
    thread,
    // 导入单调时钟与有界等待。
    time::{Duration, Instant},
};

// 导入公开 JSON 值与构造宏。
use serde_json::{Value, json};
// 导入项目自有窗口夹具所需固定 Windows API。
use windows::{
    // 导入窗口句柄、消息参数与返回值。
    Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    // 导入仅测试夹具使用的前景输入队列协调 API。
    Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId},
    // 导入窗口创建、状态、消息泵与系统命令。
    Win32::UI::WindowsAndMessaging::{
        // 导入仅测试夹具使用的前景提升请求。
        BringWindowToTop,
        // 导入窗口过程调用与创建销毁。
        CallWindowProcW,
        CreateWindowExW,
        DefWindowProcW,
        DestroyWindow,
        // 导入线程消息分派。
        DispatchMessageW,
        // 导入窗口过程替换索引。
        GWLP_WNDPROC,
        // 导入前景、虚拟桌面指标与窗口状态检查。
        GetForegroundWindow,
        GetSystemMetrics,
        GetWindowThreadProcessId,
        IsIconic,
        IsWindow,
        // 导入消息容器与非阻塞读取标志。
        MSG,
        PM_REMOVE,
        PeekMessageW,
        // 导入固定最小化系统命令。
        SC_MINIMIZE,
        // 导入虚拟桌面位置指标。
        SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
        // 导入前景请求、窗口过程替换与消息翻译。
        SetForegroundWindow,
        SetWindowLongPtrW,
        TranslateMessage,
        // 导入窗口扩展样式与普通样式类型。
        WINDOW_EX_STYLE,
        WINDOW_STYLE,
        // 导入系统命令消息与原始窗口过程类型。
        WM_SYSCOMMAND,
        WNDPROC,
        // 导入固定窗口装饰、状态按钮与可见样式。
        WS_CAPTION,
        // 导入不激活与工具窗口扩展样式。
        WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW,
        WS_MAXIMIZEBOX,
        WS_MINIMIZEBOX,
        WS_OVERLAPPED,
        WS_OVERLAPPEDWINDOW,
        WS_SYSMENU,
        WS_VISIBLE,
    },
    // 导入编译期 UTF-16 字符串宏。
    core::w,
};

// 导入父 Module 私有生产状态机。
use super::*;
// 导入正式 System、窗口发现与请求类型。
use crate::{
    // 导入正式 ComputerControlSystem facade。
    AppControlService,
    // 导入正式窗口发现、前景事实与 opaque ID 投影。
    adapters::windows::{enumerate_windows, foreground_hwnd, opaque_window_session_id},
    // 导入统一请求、错误和动词。
    domain::{AppControlError, AppResult, CommandRequest, Verb},
};

// 串行化会改变项目自有可见窗口状态的动态测试。
static WINDOW_LIFECYCLE_FIXTURE_LOCK: Mutex<()> = Mutex::new(());
// 保存系统 STATIC 原始窗口过程。
static FIXTURE_ORIGINAL_PROC: AtomicIsize = AtomicIsize::new(0);
// 决定夹具是否吞掉最小化系统命令。
static IGNORE_MINIMIZE: AtomicBool = AtomicBool::new(false);
// 记录夹具是否已经观察到最小化命令。
static MINIMIZE_COMMAND_SEEN: AtomicBool = AtomicBool::new(false);

// 保存项目自有窗口夹具的封闭创建选项。
#[derive(Clone, Copy)]
struct FixtureOptions {
    // 决定是否提供可调整大小样式。
    resizable: bool,
    // 决定是否吞掉固定最小化命令。
    ignore_minimize: bool,
    // 决定是否为状态动作建立确定性前景起点。
    foreground: bool,
}

// 持有项目自有窗口和 UI 线程生命周期。
struct WindowLifecycleFixture {
    // 保存仅供测试内部使用的原生句柄值。
    handle: isize,
    // 保存跨线程停止标记。
    running: Arc<AtomicBool>,
    // 保存可回收 UI 线程。
    worker: Option<thread::JoinHandle<()>>,
}

// 确保测试失败时也只清理项目自有窗口。
impl Drop for WindowLifecycleFixture {
    // 请求 UI 线程销毁自有窗口并回收线程。
    fn drop(&mut self) {
        // 发布停止请求。
        self.running.store(false, Ordering::Release);
        // 取出唯一 join 所有权。
        if let Some(worker) = self.worker.take() {
            // 测试清理忽略已经传播的夹具 panic。
            let _ = worker.join();
        }
        // 清除跨测试窗口过程状态。
        FIXTURE_ORIGINAL_PROC.store(0, Ordering::Release);
        // 清除命令忽略开关。
        IGNORE_MINIMIZE.store(false, Ordering::Release);
        // 清除命令观察证据。
        MINIMIZE_COMMAND_SEEN.store(false, Ordering::Release);
    }
}

// 调用系统 STATIC 原始窗口过程。
unsafe fn call_original(
    // 接收自有窗口句柄。
    window: HWND,
    // 接收窗口消息。
    message: u32,
    // 接收 WPARAM。
    word: WPARAM,
    // 接收 LPARAM。
    value: LPARAM,
) -> LRESULT {
    // 读取已发布的系统窗口过程。
    let original = FIXTURE_ORIGINAL_PROC.load(Ordering::Acquire);
    // 缺失原始过程时使用系统默认过程 fail safe。
    if original == 0 {
        // 调用系统默认窗口过程。
        return unsafe { DefWindowProcW(window, message, word, value) };
    }
    // 将系统返回值恢复为封闭 WNDPROC 类型。
    let procedure = unsafe { std::mem::transmute::<isize, WNDPROC>(original) };
    // 转发消息给系统 STATIC 行为。
    unsafe { CallWindowProcW(procedure, window, message, word, value) }
}

// 记录并可选择吞掉项目自有窗口的最小化系统命令。
unsafe extern "system" fn lifecycle_fixture_proc(
    // 接收自有窗口句柄。
    window: HWND,
    // 接收窗口消息。
    message: u32,
    // 接收 WPARAM。
    word: WPARAM,
    // 接收 LPARAM。
    value: LPARAM,
) -> LRESULT {
    // 只检查固定系统命令消息。
    if message == WM_SYSCOMMAND {
        // 屏蔽系统命令低四位保留位。
        let command = word.0 as u32 & 0xfff0;
        // 只记录认证最小化命令。
        if command == SC_MINIMIZE {
            // 发布平台命令已被夹具消息泵观察到的事实。
            MINIMIZE_COMMAND_SEEN.store(true, Ordering::Release);
            // 需要挂起状态变化时吞掉项目自有消息。
            if IGNORE_MINIMIZE.load(Ordering::Acquire) {
                // 返回已处理，不调用真实窗口状态变化。
                return LRESULT(0);
            }
        }
    }
    // 其余消息保持系统 STATIC 默认行为。
    unsafe { call_original(window, message, word, value) }
}

// 返回固定可调整或固定尺寸窗口样式。
fn fixture_style(resizable: bool) -> WINDOW_STYLE {
    // 可调整夹具使用普通重叠窗口完整样式。
    if resizable {
        // 合并可见位供正式窗口发现。
        return WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0);
    }
    // 固定尺寸夹具保留标题、系统菜单与状态按钮，但不包含 WS_SIZEBOX。
    WINDOW_STYLE(
        // 保留普通顶层窗口基位。
        WS_OVERLAPPED.0
            // 保留可发现的普通标题栏。
            | WS_CAPTION.0
            // 保留系统菜单。
            | WS_SYSMENU.0
            // 保留最小化支持。
            | WS_MINIMIZEBOX.0
            // 保留最大化支持。
            | WS_MAXIMIZEBOX.0
            // 使窗口进入正式可见清单。
            | WS_VISIBLE.0,
    )
}

// 仅为项目自有状态动作夹具建立确定性前景起点。
fn establish_fixture_foreground(window: HWND) {
    // 读取当前主机前景窗口。
    let previous = unsafe { GetForegroundWindow() };
    // 读取当前夹具 UI 线程 ID。
    let fixture_thread = unsafe { GetCurrentThreadId() };
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
        && previous_thread != fixture_thread
        // 请求测试夹具临时共享前景资格。
        && unsafe { AttachThreadInput(fixture_thread, previous_thread, true) }.as_bool();
    // 把项目自有窗口提升到顶层。
    let _ = unsafe { BringWindowToTop(window) };
    // 请求项目自有窗口成为前景。
    let _ = unsafe { SetForegroundWindow(window) };
    // 已附加时始终恢复原输入队列边界。
    if attached {
        // 分离临时输入队列关联。
        let _ = unsafe { AttachThreadInput(fixture_thread, previous_thread, false) };
    }
}

// 创建可见项目自有窗口并按选项建立前景起点与有界消息泵。
fn spawn_fixture(options: FixtureOptions) -> AppResult<WindowLifecycleFixture> {
    // 清除前一动态测试的命令证据。
    MINIMIZE_COMMAND_SEEN.store(false, Ordering::Release);
    // 发布当前夹具的最小化忽略策略。
    IGNORE_MINIMIZE.store(options.ignore_minimize, Ordering::Release);
    // 创建夹具就绪通道。
    let (ready_tx, ready_rx) = mpsc::channel::<Result<isize, String>>();
    // 创建跨线程停止标记。
    let running = Arc::new(AtomicBool::new(true));
    // 克隆停止标记给 UI 线程。
    let thread_running = Arc::clone(&running);
    // 启动项目自有 UI 线程。
    let worker = thread::spawn(move || {
        // 读取虚拟桌面左边界以支持负坐标显示器布局。
        let virtual_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        // 读取虚拟桌面上边界。
        let virtual_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        // 前景状态夹具使用普通 tool-window，后台几何夹具禁止激活。
        let extended = if options.foreground {
            // 允许状态夹具成为显式前景。
            WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0)
        } else {
            // 后台夹具保持 no-activate 语义。
            WINDOW_EX_STYLE(WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0)
        };
        // 创建普通系统 STATIC 顶层窗口。
        let window = unsafe {
            // 调用固定系统类，不注册软件专用类。
            CreateWindowExW(
                // 禁止夹具取得前景。
                extended,
                // 使用 Windows 内建 STATIC 类。
                w!("STATIC"),
                // 使用唯一且无敏感含义的测试标题。
                w!("Rust Window Lifecycle General Fixture"),
                // 选择封闭窗口样式。
                fixture_style(options.resizable),
                // 放在当前虚拟桌面可见区域内。
                virtual_x.saturating_add(80),
                // 放在当前虚拟桌面可见区域内。
                virtual_y.saturating_add(80),
                // 使用足够验证缩放的宽度。
                480,
                // 使用足够验证缩放的高度。
                320,
                // 顶层窗口没有父窗口。
                None,
                // 不提供菜单。
                None,
                // 使用当前模块实例。
                None,
                // 不传任意应用指针。
                None,
            )
        };
        // 创建失败时发送稳定错误并结束。
        let window = match window {
            // 保存有效窗口。
            Ok(window) => window,
            // 投影创建错误。
            Err(error) => {
                // 忽略接收端提前退出。
                let _ = ready_tx.send(Err(error.to_string()));
                // 结束 UI 线程。
                return;
            }
        };
        // 安装仅记录固定系统命令的测试窗口过程。
        let original = unsafe {
            // 子类化只作用于本进程自有 STATIC 窗口。
            SetWindowLongPtrW(
                // 传入自有窗口。
                window,
                // 替换窗口过程。
                GWLP_WNDPROC,
                // 传入固定测试过程。
                lifecycle_fixture_proc as *const () as usize as isize,
            )
        };
        // 缺失原始过程表示子类化未完成。
        if original == 0 {
            // 向测试线程报告稳定错误。
            let _ = ready_tx.send(Err("window lifecycle fixture subclass failed".to_owned()));
            // 在创建线程销毁自有窗口。
            let _ = unsafe { DestroyWindow(window) };
            // 结束 UI 线程。
            return;
        }
        // 发布原始过程给窗口回调。
        FIXTURE_ORIGINAL_PROC.store(original, Ordering::Release);
        // 状态夹具在自己的 UI 线程建立确定性前景起点。
        if options.foreground {
            // 仅显式 ignored 动态测试会进入此前景分支。
            establish_fixture_foreground(window);
        }
        // 把私有句柄交给测试线程。
        let _ = ready_tx.send(Ok(window.0 as isize));
        // 初始化线程消息容器。
        let mut message = MSG::default();
        // 运行到测试请求清理或窗口被销毁。
        while thread_running.load(Ordering::Acquire)
            // 只读检查自有窗口仍然存在。
            && unsafe { IsWindow(Some(window)) }.as_bool()
        {
            // 处理当前队列中的全部消息。
            while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                // 执行系统消息翻译。
                let _ = unsafe { TranslateMessage(&message) };
                // 分派到系统或测试窗口过程。
                unsafe { DispatchMessageW(&message) };
            }
            // 避免空消息泵忙等。
            thread::sleep(Duration::from_millis(2));
        }
        // 测试结束后只销毁本进程自有窗口。
        if unsafe { IsWindow(Some(window)) }.as_bool() {
            // DestroyWindow 保持在创建窗口的 UI 线程。
            let _ = unsafe { DestroyWindow(window) };
        }
    });
    // 有界等待 UI 线程返回创建结果。
    let handle = ready_rx
        // 最多等待三秒。
        .recv_timeout(Duration::from_secs(3))
        // 映射协调超时。
        .map_err(|error| AppControlError::new("FIXTURE_UNAVAILABLE", error.to_string()))?
        // 映射窗口创建失败。
        .map_err(|error| AppControlError::new("FIXTURE_UNAVAILABLE", error))?;
    // 返回拥有完整清理责任的夹具。
    Ok(WindowLifecycleFixture {
        // 保存私有句柄值。
        handle,
        // 保存停止标记。
        running,
        // 保存唯一 UI 线程所有权。
        worker: Some(worker),
    })
}

// 从实时窗口清单为自有句柄生成 canonical opaque ID。
fn fixture_session(handle: isize) -> AppResult<String> {
    // 设置有界发现截止时刻。
    let deadline = Instant::now() + Duration::from_secs(3);
    // 等待自有窗口进入正式发现链。
    loop {
        // 枚举实时私有窗口事实。
        let windows = enumerate_windows()?;
        // 仅在测试内部按已知自有句柄匹配。
        if let Some(record) = windows.iter().find(|record| record.hwnd == handle) {
            // 使用正式投影生成公开 opaque ID。
            return Ok(opaque_window_session_id(record));
        }
        // 到期后返回结构化夹具错误。
        if Instant::now() >= deadline {
            // 返回稳定错误，不公开句柄。
            return Err(AppControlError::new(
                // 使用测试夹具错误码。
                "FIXTURE_UNAVAILABLE",
                // 说明正式发现链未命中。
                "The project-owned lifecycle fixture was not discovered.",
            ));
        }
        // 在下一次实时枚举前短等待。
        thread::sleep(Duration::from_millis(10));
    }
}

// 构造通过正式 app.apply 路由的已确认生命周期请求。
fn app_request(session_id: &str, input: Value) -> CommandRequest {
    // 构造 app.run 请求。
    let mut request = CommandRequest::read(Verb::Run, "app");
    // 选择统一 apply operation。
    request.operation = Some("apply".to_owned());
    // 写入 canonical opaque 目标。
    request
        // 访问目标对象。
        .target
        // 插入原样 sessionId。
        .insert("sessionId".to_owned(), json!(session_id));
    // 写入版本化生命周期 capability。
    request
        // 访问参数对象。
        .args
        // 插入固定 capability。
        .insert(
            "capability".to_owned(),
            json!(capabilities::WINDOW_LIFECYCLE),
        );
    // 写入严格 provider-neutral input。
    request
        // 访问参数对象。
        .args
        // 插入调用方动作。
        .insert("input".to_owned(), input);
    // 提供逐操作确认。
    request.confirmed = true;
    // 提供预先前景影响同意。
    request.foreground_consent = true;
    // 返回完整正式请求。
    request
}

// 经正式 ComputerControlSystem 执行一次窗口生命周期动作。
fn execute_action(service: &AppControlService, session_id: &str, input: Value) -> AppResult<Value> {
    // 委托正式 System、Policy、provider 与 Module 路由。
    service.execute(app_request(session_id, input))
}

// 取得可恢复 poisoned 状态的动态夹具串行锁。
fn fixture_guard() -> MutexGuard<'static, ()> {
    // 测试 panic 后仍允许后续显式串行夹具执行清理验证。
    WINDOW_LIFECYCLE_FIXTURE_LOCK
        // 请求唯一动态窗口所有权。
        .lock()
        // poison 时恢复内部 guard。
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// 返回确定性从不取消探针。
const fn never_cancelled() -> bool {
    // 固定报告未取消。
    false
}

// 在平台命令被自有窗口观察后请求取消。
fn cancel_after_minimize_seen() -> bool {
    // 只读取项目自有命令证据。
    MINIMIZE_COMMAND_SEEN.load(Ordering::Acquire)
}

// 有界等待自有窗口消息泵观察最小化命令。
fn wait_for_minimize_command() -> bool {
    // 设置一秒技术证据 deadline。
    let deadline = Instant::now() + Duration::from_secs(1);
    // 等待原子证据或 deadline。
    while Instant::now() < deadline {
        // 已观察到命令时立即成功。
        if MINIMIZE_COMMAND_SEEN.load(Ordering::Acquire) {
            // 返回技术证据成立。
            return true;
        }
        // 避免忙循环。
        thread::sleep(Duration::from_millis(2));
    }
    // 返回 deadline 时的最终原子状态。
    MINIMIZE_COMMAND_SEEN.load(Ordering::Acquire)
}

// 核对正式成功 envelope 的通用生命周期事实。
fn assert_completed(result: &Value, session_id: &str, action: &str) {
    // 核对顶层 capability。
    assert_eq!(result["capability"], capabilities::WINDOW_LIFECYCLE);
    // 核对调用方 opaque 目标回显。
    assert_eq!(result["targetId"], session_id);
    // 核对主机前景执行域。
    assert_eq!(result["executionRealm"], "host-foreground");
    // 核对具体动作。
    assert_eq!(result["data"]["action"], action);
    // 核对平台已接受。
    assert_eq!(result["data"]["accepted"], true);
    // 核对精确最终状态已读回。
    assert_eq!(result["data"]["finalStateReached"], true);
    // 核对 mutation 结果公开当前身份材料停止线。
    assert_eq!(
        result["data"]["targetIdentityStrength"]["sameProcessRecycledWindowToken"],
        // 当前实现不得声称完全相同 token 回收已解决。
        "not-guaranteed"
    );
    // 核对 Per-Monitor-V2 坐标上下文。
    assert_eq!(
        result["data"]["coordinateContext"],
        "per-monitor-v2-virtual-screen-physical-px"
    );
    // 核对多显示器带符号坐标承诺。
    assert_eq!(result["data"]["signedVirtualScreenCoordinates"], true);
    // 成功 mutation 也不得授权自动重试。
    assert_eq!(result["data"]["retrySafe"], false);
    // 序列化结果检查 native 字段泄漏。
    let serialized = result.to_string().to_ascii_lowercase();
    // 禁止 HWND。
    assert!(!serialized.contains("hwnd"));
    // 禁止 PID。
    assert!(!serialized.contains("processid"));
    // 禁止窗口样式字段。
    assert!(!serialized.contains("style"));
}

// 验证正式 System 路由覆盖五类状态与几何动作及前景不变。
#[test]
// 普通并行测试不得改变可见窗口状态，需显式串行运行。
#[ignore = "requires an explicit serial visible-window lifecycle fixture run"]
fn production_route_drives_all_window_lifecycle_actions() -> Result<(), Box<dyn std::error::Error>>
{
    // 串行化项目自有可见窗口影响。
    let _guard = fixture_guard();
    // 创建可调整且正常处理状态命令的自有夹具。
    let fixture = spawn_fixture(FixtureOptions {
        // 允许正式 resize。
        resizable: true,
        // 不吞掉状态命令。
        ignore_minimize: false,
        // 几何动作夹具保持后台 no-activate。
        foreground: false,
    })?;
    // 从正式发现链取得 canonical opaque 目标。
    let session_id = fixture_session(fixture.handle)?;
    // 记录 no-activate 夹具创建后的宿主前景。
    let foreground_before = foreground_hwnd();
    // 夹具不得意外成为前景。
    assert_ne!(foreground_before, fixture.handle);
    // 创建正式 ComputerControlSystem facade。
    let service = AppControlService::new();
    // 读取当前虚拟桌面带符号左边界。
    let virtual_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    // 读取当前虚拟桌面带符号上边界。
    let virtual_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    // 选择当前虚拟桌面内的确定性横坐标。
    let move_x = virtual_x.saturating_add(140);
    // 选择当前虚拟桌面内的确定性纵坐标。
    let move_y = virtual_y.saturating_add(120);
    // 经正式生产路由移动自有窗口。
    let moved = execute_action(
        // 使用同一 System。
        &service,
        // 使用 canonical 自有目标。
        &session_id,
        // 提供带符号虚拟桌面物理坐标。
        json!({
            "action": "move",
            "coordinateSpace": "screen-physical-px",
            "x": move_x,
            "y": move_y,
            "timeoutMs": 2_000
        }),
    )?;
    // 核对通用完成证据。
    assert_completed(&moved, &session_id, "move");
    // 核对精确移动读回。
    assert_eq!(moved["data"]["bounds"]["x"], move_x);
    // 核对纵坐标读回。
    assert_eq!(moved["data"]["bounds"]["y"], move_y);
    // 经正式生产路由调整自有窗口外框。
    let resized = execute_action(
        // 使用同一 System。
        &service,
        // 使用 canonical 自有目标。
        &session_id,
        // 提供安全普通尺寸。
        json!({
            "action": "resize",
            "coordinateSpace": "screen-physical-px",
            "width": 520,
            "height": 360,
            "timeoutMs": 2_000
        }),
    )?;
    // 核对通用完成证据。
    assert_completed(&resized, &session_id, "resize");
    // 核对精确宽度读回。
    assert_eq!(resized["data"]["bounds"]["width"], 520);
    // 核对精确高度读回。
    assert_eq!(resized["data"]["bounds"]["height"], 360);
    // 后台几何动作必须保持宿主前景身份。
    assert_eq!(foreground_hwnd(), foreground_before);
    // 在创建前景状态夹具前完整回收后台窗口与全局测试状态。
    drop(fixture);
    // 创建显式前景且正常处理状态命令的自有夹具。
    let state_fixture = spawn_fixture(FixtureOptions {
        // 保留完整普通窗口状态样式。
        resizable: true,
        // 不吞掉状态命令。
        ignore_minimize: false,
        // 为 host-foreground 状态动作建立确定性授权起点。
        foreground: true,
    })?;
    // 从正式发现链取得状态夹具 canonical opaque 目标。
    let state_session_id = fixture_session(state_fixture.handle)?;
    // 状态动作只在项目自有目标已是前景时开始。
    assert_eq!(foreground_hwnd(), state_fixture.handle);
    // 最大化项目自有窗口。
    let maximized = execute_action(
        // 使用同一 System。
        &service,
        // 使用 canonical 前景状态目标。
        &state_session_id,
        // 提供单一状态动作。
        json!({ "action": "maximize", "timeoutMs": 2_000 }),
    )?;
    // 核对最大化完成证据。
    assert_completed(&maximized, &state_session_id, "maximize");
    // 核对最终状态。
    assert_eq!(maximized["data"]["state"], "maximized");
    // 从最大化恢复到普通状态。
    let restored_from_maximize = execute_action(
        // 使用同一 System。
        &service,
        // 使用 canonical 前景状态目标。
        &state_session_id,
        // 提供恢复动作。
        json!({ "action": "restore", "timeoutMs": 2_000 }),
    )?;
    // 核对恢复完成证据。
    assert_completed(&restored_from_maximize, &state_session_id, "restore");
    // 核对普通最终状态。
    assert_eq!(restored_from_maximize["data"]["state"], "normal");
    // 最小化项目自有窗口。
    let minimized = execute_action(
        // 使用同一 System。
        &service,
        // 使用 canonical 前景状态目标。
        &state_session_id,
        // 提供最小化动作。
        json!({ "action": "minimize", "timeoutMs": 2_000 }),
    )?;
    // 核对最小化完成证据。
    assert_completed(&minimized, &state_session_id, "minimize");
    // 核对最小化最终状态。
    assert_eq!(minimized["data"]["state"], "minimized");
    // 从后台最小化状态恢复项目自有窗口。
    let restored_from_minimize = execute_action(
        // 使用同一 System。
        &service,
        // 使用 canonical 状态目标。
        &state_session_id,
        // 提供恢复动作。
        json!({ "action": "restore", "timeoutMs": 2_000 }),
    )?;
    // 核对后台恢复完成证据。
    assert_completed(
        // 传入生产结果。
        &restored_from_minimize,
        // 传入调用方 opaque 目标。
        &state_session_id,
        // 核对恢复动作。
        "restore",
    );
    // 核对后台最小化窗口恢复为普通状态。
    assert_eq!(restored_from_minimize["data"]["state"], "normal");
    // 返回成功。
    Ok(())
}

// 验证环境越界与不可缩放样式在 dispatch 前失败闭合。
#[test]
// 普通并行测试不得创建可见项目窗口，需显式串行运行。
#[ignore = "requires an explicit serial visible-window lifecycle fixture run"]
fn environment_geometry_and_fixed_window_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    // 串行化项目自有可见窗口影响。
    let _guard = fixture_guard();
    // 创建没有 WS_SIZEBOX 的固定尺寸自有夹具。
    let fixture = spawn_fixture(FixtureOptions {
        // 禁止正式 resize。
        resizable: false,
        // 不吞掉状态命令。
        ignore_minimize: false,
        // 写前失败夹具保持后台 no-activate。
        foreground: false,
    })?;
    // 从正式发现链取得 canonical opaque 目标。
    let session_id = fixture_session(fixture.handle)?;
    // 记录 no-activate 夹具创建后的宿主前景。
    let foreground_before = foreground_hwnd();
    // 创建正式 ComputerControlSystem facade。
    let service = AppControlService::new();
    // 请求固定样式不支持的 resize。
    let fixed_error = match execute_action(
        // 使用正式 System。
        &service,
        // 使用 canonical 自有目标。
        &session_id,
        // 提供普通合法协议尺寸。
        json!({
            "action": "resize",
            "coordinateSpace": "screen-physical-px",
            "width": 400,
            "height": 300
        }),
    ) {
        // 成功表示样式门禁失效。
        Ok(_) => return Err("fixed window unexpectedly resized".into()),
        // 保存预期错误。
        Err(error) => error,
    };
    // 固定窗口必须结构化报告不支持。
    assert_eq!(fixed_error.code, "CAPABILITY_UNSUPPORTED");
    // 请求完全离开当前虚拟桌面的移动。
    let range_error = match execute_action(
        // 使用正式 System。
        &service,
        // 使用 canonical 自有目标。
        &session_id,
        // 使用协议合法但环境不可达的带符号坐标。
        json!({
            "action": "move",
            "coordinateSpace": "screen-physical-px",
            "x": i32::MAX,
            "y": i32::MAX
        }),
    ) {
        // 成功表示环境范围门禁失效。
        Ok(_) => return Err("out-of-range window move unexpectedly succeeded".into()),
        // 保存预期错误。
        Err(error) => error,
    };
    // 环境越界保持稳定参数错误。
    assert_eq!(range_error.code, "INVALID_ARGUMENT");
    // 核对平台尚未接受动作。
    assert_eq!(range_error.details["accepted"], false);
    // 核对稳定环境原因。
    assert_eq!(
        range_error.details["reason"],
        "window-geometry-out-of-range"
    );
    // 两项写前失败不得改变宿主前景。
    assert_eq!(foreground_hwnd(), foreground_before);
    // 返回成功。
    Ok(())
}

// 验证已接受最小化命令后的取消返回不可重试未知结果。
#[test]
// 普通并行测试不得创建可见项目窗口，需显式串行运行。
#[ignore = "requires an explicit serial visible-window lifecycle fixture run"]
fn accepted_cancellation_is_outcome_unknown() -> Result<(), Box<dyn std::error::Error>> {
    // 串行化项目自有可见窗口影响。
    let _guard = fixture_guard();
    // 创建吞掉最小化状态变化的自有夹具。
    let fixture = spawn_fixture(FixtureOptions {
        // 保留普通窗口样式。
        resizable: true,
        // 吞掉最小化命令以保持未完成状态。
        ignore_minimize: true,
        // 接受后取消夹具保持后台 no-activate。
        foreground: false,
    })?;
    // 从正式发现链取得 canonical opaque 目标。
    let session_id = fixture_session(fixture.handle)?;
    // 记录 no-activate 夹具创建后的宿主前景。
    let foreground_before = foreground_hwnd();
    // 构造有界最小化动作。
    let input = json!({ "action": "minimize", "timeoutMs": 2_000 });
    // 注入只在平台命令被观察后生效的取消探针。
    let error = match perform_with_cancel_probe(
        // 传入 canonical 自有目标。
        Some(&session_id),
        // 提供逐操作确认。
        true,
        // 提供前景影响同意。
        true,
        // 传入严格动作。
        Some(&input),
        // 仅在接受后取消。
        cancel_after_minimize_seen,
    ) {
        // 成功表示夹具未保持最终状态未完成。
        Ok(_) => return Err("accepted cancellation unexpectedly completed".into()),
        // 保存预期错误。
        Err(error) => error,
    };
    // 确认自有消息泵实际观察到平台命令。
    assert!(wait_for_minimize_command());
    // 接受后取消必须保持 OutcomeUnknown。
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    // 核对平台已接受。
    assert_eq!(error.details["accepted"], true);
    // 核对稳定取消原因。
    assert_eq!(error.details["reason"], "cancel-requested-after-acceptance");
    // 核对禁止自动重试。
    assert_eq!(error.details["retrySafe"], false);
    // 吞掉命令后窗口不得被最小化。
    let window = HWND(fixture.handle as *mut c_void);
    // 核对真实项目窗口仍为非最小化。
    assert!(!unsafe { IsIconic(window) }.as_bool());
    // 接受后取消不得改变宿主前景。
    assert_eq!(foreground_hwnd(), foreground_before);
    // 返回成功。
    Ok(())
}

// 验证已接受但未完成的最小化命令在 deadline 后返回未知结果。
#[test]
// 普通并行测试不得创建可见项目窗口，需显式串行运行。
#[ignore = "requires an explicit serial visible-window lifecycle fixture run"]
fn accepted_timeout_is_outcome_unknown() -> Result<(), Box<dyn std::error::Error>> {
    // 串行化项目自有可见窗口影响。
    let _guard = fixture_guard();
    // 创建吞掉最小化状态变化的自有夹具。
    let fixture = spawn_fixture(FixtureOptions {
        // 保留普通窗口样式。
        resizable: true,
        // 吞掉最小化命令以保持未完成状态。
        ignore_minimize: true,
        // 接受后 deadline 夹具保持后台 no-activate。
        foreground: false,
    })?;
    // 从正式发现链取得 canonical opaque 目标。
    let session_id = fixture_session(fixture.handle)?;
    // 记录 no-activate 夹具创建后的宿主前景。
    let foreground_before = foreground_hwnd();
    // 构造短 deadline 最小化动作。
    let input = json!({ "action": "minimize", "timeoutMs": 75 });
    // 使用从不取消探针执行正式状态机。
    let error = match perform_with_cancel_probe(
        // 传入 canonical 自有目标。
        Some(&session_id),
        // 提供逐操作确认。
        true,
        // 提供前景影响同意。
        true,
        // 传入严格动作。
        Some(&input),
        // 禁止测试取消干扰 deadline。
        never_cancelled,
    ) {
        // 成功表示夹具未保持最终状态未完成。
        Ok(_) => return Err("accepted timeout unexpectedly completed".into()),
        // 保存预期错误。
        Err(error) => error,
    };
    // 确认自有消息泵实际观察到平台命令。
    assert!(wait_for_minimize_command());
    // 接受后 deadline 必须保持 OutcomeUnknown。
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    // 核对平台已接受。
    assert_eq!(error.details["accepted"], true);
    // 核对稳定 deadline 原因。
    assert_eq!(error.details["reason"], "deadline-reached-after-acceptance");
    // 核对最终状态未认证。
    assert_eq!(error.details["finalStateReached"], false);
    // 核对禁止自动重试。
    assert_eq!(error.details["retrySafe"], false);
    // 吞掉命令后窗口不得被最小化。
    let window = HWND(fixture.handle as *mut c_void);
    // 核对真实项目窗口仍为非最小化。
    assert!(!unsafe { IsIconic(window) }.as_bool());
    // 接受后 timeout 不得改变宿主前景。
    assert_eq!(foreground_hwnd(), foreground_before);
    // 返回成功。
    Ok(())
}
