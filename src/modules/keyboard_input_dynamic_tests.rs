//! 通过项目自有顶层窗口夹具验证通用键盘生产路由与安全释放。

// 导入夹具线程、原子证据、互斥串行化与有界等待。
use std::{
    // 导入跨线程共享与原子状态。
    sync::{
        // 导入引用计数停止标记与夹具互斥锁。
        Arc,
        Mutex,
        MutexGuard,
        // 导入原子计数与内存顺序。
        atomic::{AtomicBool, AtomicIsize, Ordering},
        // 导入夹具就绪通道。
        mpsc,
    },
    // 导入独立 UI 与协调线程。
    thread,
    // 导入有界等待与单调时钟。
    time::{Duration, Instant},
};

// 导入 JSON 构造宏与值类型。
use serde_json::{Value, json};
// 导入项目自有窗口夹具所需固定 Windows API。
use windows::{
    // 导入窗口句柄和消息参数。
    Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    // 导入仅测试夹具使用的前景输入队列协调 API。
    Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId},
    // 导入通用虚拟键常量。
    Win32::UI::Input::KeyboardAndMouse::{
        // 导入控制、Shift、功能键和 Unicode packet 常量。
        VK_CONTROL,
        VK_F6,
        VK_PACKET,
        VK_SHIFT,
    },
    // 导入窗口创建、子类化、消息泵与键盘消息。
    Win32::UI::WindowsAndMessaging::{
        // 导入前景建立调用。
        BringWindowToTop,
        // 导入原过程调用和内建默认过程。
        CallWindowProcW,
        // 导入窗口创建与清理。
        CreateWindowExW,
        DefWindowProcW,
        DestroyWindow,
        // 导入消息分派。
        DispatchMessageW,
        // 导入窗口过程槽位与前景读取。
        GWLP_WNDPROC,
        GetForegroundWindow,
        // 导入线程和窗口状态读取。
        GetWindowThreadProcessId,
        IsWindow,
        // 导入消息容器与有界轮询。
        MSG,
        PM_REMOVE,
        PeekMessageW,
        // 导入前景设置与子类化。
        SetForegroundWindow,
        SetWindowLongPtrW,
        // 导入键盘翻译与消息常量。
        TranslateMessage,
        // 导入窗口样式。
        WINDOW_EX_STYLE,
        WM_CHAR,
        WM_KEYDOWN,
        WM_KEYUP,
        WM_SYSKEYDOWN,
        WM_SYSKEYUP,
        WNDPROC,
        WS_OVERLAPPEDWINDOW,
        WS_VISIBLE,
    },
    // 导入编译期 UTF-16 字符串宏。
    core::w,
};

// 导入父 Module 私有生产状态机与领域类型。
use super::*;
// 导入正式 ComputerControlSystem facade。
use crate::AppControlService;
// 导入正式窗口发现与 opaque ID 投影。
use crate::adapters::windows::{enumerate_windows, opaque_window_session_id};
// 导入正式请求领域类型。
use crate::domain::{CommandRequest, Verb};

// 串行化会真实改变前景和键盘状态的动态测试。
static KEYBOARD_FIXTURE_LOCK: Mutex<()> = Mutex::new(());
// 保存系统 STATIC 原始窗口过程。
static FIXTURE_ORIGINAL_PROC: AtomicIsize = AtomicIsize::new(0);
// 保存确定性取消探针状态。
static TEST_CANCELLED: AtomicBool = AtomicBool::new(false);
// 保存项目自有窗口收到的有界键盘事件。
static KEYBOARD_EVENTS: Mutex<Vec<KeyboardEvent>> = Mutex::new(Vec::new());

// 保存一个测试内部键盘消息事实。
#[derive(Clone, Copy, Debug)]
struct KeyboardEvent {
    // 保存 Win32 消息类别，仅限测试内部。
    message: u32,
    // 保存消息级虚拟键或 UTF-16 单元，仅限测试内部。
    value: usize,
    // 保存接收消息的单调时刻。
    observed_at: Instant,
}

// 持有自有窗口和 UI 线程生命周期。
struct KeyboardFixture {
    // 保存仅供测试内部使用的原生句柄值。
    handle: isize,
    // 保存跨线程停止标记。
    running: Arc<AtomicBool>,
    // 保存可回收 UI 线程。
    worker: Option<thread::JoinHandle<()>>,
}

// 确保测试失败时也只清理项目自有窗口。
impl Drop for KeyboardFixture {
    // 请求 UI 线程销毁自有窗口并回收线程。
    fn drop(&mut self) {
        // 发布停止请求。
        self.running.store(false, Ordering::Release);
        // 取出唯一 join 所有权。
        if let Some(worker) = self.worker.take() {
            // 测试清理忽略已经传播的夹具 panic。
            let _ = worker.join();
        }
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

// 记录自有窗口收到的通用键盘消息。
unsafe extern "system" fn keyboard_fixture_proc(
    // 接收自有窗口句柄。
    window: HWND,
    // 接收窗口消息。
    message: u32,
    // 接收 WPARAM。
    word: WPARAM,
    // 接收 LPARAM。
    value: LPARAM,
) -> LRESULT {
    // 只记录键盘阶段和 Unicode 字符消息。
    if matches!(
        message,
        // 覆盖普通与系统按键上下阶段。
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP | WM_CHAR
    ) {
        // 取得无业务状态的事件互斥锁。
        let mut events = match KEYBOARD_EVENTS.lock() {
            // 返回正常锁守卫。
            Ok(events) => events,
            // 前一测试 panic 时继续保留诊断证据。
            Err(poisoned) => poisoned.into_inner(),
        };
        // 只保留固定上限避免异常消息流无界增长。
        if events.len() < 4_096 {
            // 追加测试内部事实。
            events.push(KeyboardEvent {
                // 保存消息类别。
                message,
                // 保存消息参数数值。
                value: word.0,
                // 保存单调时刻。
                observed_at: Instant::now(),
            });
        }
    }
    // 保留系统 STATIC 的默认行为。
    unsafe { call_original(window, message, word, value) }
}

// 清空一个动态测试的全部消息证据。
fn reset_fixture_evidence() {
    // 清除取消探针。
    TEST_CANCELLED.store(false, Ordering::Release);
    // 清除状态机已接受命名键按下计数。
    TEST_ACCEPTED_KEY_DOWNS.store(0, Ordering::Release);
    // 清除 Adapter 已接受命名键释放计数。
    TEST_ACCEPTED_KEY_UPS.store(0, Ordering::Release);
    // 清除已接受 Unicode scalar 计数。
    TEST_ACCEPTED_UNICODE_SCALARS.store(0, Ordering::Release);
    // 取得事件互斥锁。
    let mut events = match KEYBOARD_EVENTS.lock() {
        // 返回正常锁守卫。
        Ok(events) => events,
        // 前一测试 panic 时恢复守卫。
        Err(poisoned) => poisoned.into_inner(),
    };
    // 清除前一请求事件。
    events.clear();
}

// 读取当前事件快照。
fn event_snapshot() -> Vec<KeyboardEvent> {
    // 取得事件互斥锁。
    let events = match KEYBOARD_EVENTS.lock() {
        // 返回正常锁守卫。
        Ok(events) => events,
        // 前一测试 panic 时恢复守卫。
        Err(poisoned) => poisoned.into_inner(),
    };
    // 复制有界测试事实。
    events.clone()
}

// 仅为项目自有动态夹具建立确定性前景起点。
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

// 创建可见项目自有窗口并运行有界消息泵。
fn spawn_fixture() -> Result<KeyboardFixture, String> {
    // 创建夹具就绪通道。
    let (ready_tx, ready_rx) = mpsc::channel::<Result<isize, String>>();
    // 创建跨线程停止标记。
    let running = Arc::new(AtomicBool::new(true));
    // 克隆停止标记给 UI 线程。
    let thread_running = Arc::clone(&running);
    // 启动项目自有 UI 线程。
    let worker = thread::spawn(move || {
        // 创建普通可见系统 STATIC 顶层窗口。
        let window = unsafe {
            // 调用固定系统类，不注册应用专用类。
            CreateWindowExW(
                // 不添加特殊扩展样式。
                WINDOW_EX_STYLE(0),
                // 使用 Windows 内建 STATIC 类。
                w!("STATIC"),
                // 使用唯一且无敏感含义的测试标题。
                w!("Rust Keyboard Input General Fixture"),
                // 使用普通可激活顶层窗口样式。
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                // 使用稳定可见横坐标。
                120,
                // 使用稳定可见纵坐标。
                120,
                // 使用足够观察标题的宽度。
                520,
                // 使用普通测试高度。
                280,
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
        // 安装测试窗口过程并保存系统原始过程。
        let original = unsafe {
            // 子类化只作用于本进程自有 STATIC 窗口。
            SetWindowLongPtrW(
                window,
                GWLP_WNDPROC,
                keyboard_fixture_proc as *const () as usize as isize,
            )
        };
        // 缺失原始过程表示子类化未完成。
        if original == 0 {
            // 向测试线程报告稳定错误。
            let _ = ready_tx.send(Err("keyboard fixture subclass failed".to_owned()));
            // 在创建线程销毁自有窗口。
            let _ = unsafe { DestroyWindow(window) };
            // 结束 UI 线程。
            return;
        }
        // 发布原始过程给窗口回调。
        FIXTURE_ORIGINAL_PROC.store(original, Ordering::Release);
        // 主动把自有夹具置于前景以建立确定性技术起点。
        establish_fixture_foreground(window);
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
                // 执行系统键盘翻译以生成 WM_CHAR。
                let _ = unsafe { TranslateMessage(&message) };
                // 分派到自有测试窗口过程。
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
        // 清除跨测试原始过程。
        FIXTURE_ORIGINAL_PROC.store(0, Ordering::Release);
    });
    // 有界等待 UI 线程返回创建结果并保留回收责任。
    let handle = match ready_rx.recv_timeout(Duration::from_secs(3)) {
        // 保存有效自有句柄。
        Ok(Ok(handle)) => handle,
        // 子线程显式报告创建失败。
        Ok(Err(error)) => {
            // 发布停止请求，覆盖子线程尚未退出的边界。
            running.store(false, Ordering::Release);
            // 回收 UI 线程。
            let _ = worker.join();
            // 返回原始稳定错误。
            return Err(error);
        }
        // 就绪通道超时或断开。
        Err(error) => {
            // 发布停止请求，禁止分离窗口线程泄漏。
            running.store(false, Ordering::Release);
            // 回收 UI 线程。
            let _ = worker.join();
            // 返回稳定协调错误。
            return Err(error.to_string());
        }
    };
    // 返回拥有完整清理责任的夹具。
    Ok(KeyboardFixture {
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
                "The project-owned keyboard fixture was not discovered.",
            ));
        }
        // 在下一次实时枚举前短等待。
        thread::sleep(Duration::from_millis(10));
    }
}

// 构造通过正式 app.apply 路由的已确认键盘请求。
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
    // 写入版本化键盘 capability。
    request
        // 访问参数对象。
        .args
        // 插入 capability。
        .insert("capability".to_owned(), json!(capabilities::UI_INPUT_KEY));
    // 写入 provider-neutral 输入。
    request
        // 访问参数对象。
        .args
        // 插入输入值。
        .insert("input".to_owned(), input);
    // 提供逐操作确认。
    request.confirmed = true;
    // 显式同意当前操作改变主机前景。
    request.foreground_consent = true;
    // 返回完整正式请求。
    request
}

// 即使前一测试 panic 也取得夹具串行锁并继续提供独立诊断。
fn fixture_guard() -> MutexGuard<'static, ()> {
    // 恢复被前一失败标记为 poisoned 的无数据互斥锁。
    match KEYBOARD_FIXTURE_LOCK.lock() {
        // 返回正常锁守卫。
        Ok(guard) => guard,
        // 该锁不保护业务数据，安全取回唯一守卫。
        Err(poisoned) => poisoned.into_inner(),
    }
}

// 返回确定性动态测试取消状态。
fn test_cancelled() -> bool {
    // 读取仅当前调用持有的测试探针。
    TEST_CANCELLED.load(Ordering::Acquire)
}

// 等待状态机取得至少一个按键释放责任。
fn wait_for_owned_down() -> bool {
    // 设置短技术证据等待截止时刻。
    let deadline = Instant::now() + Duration::from_secs(2);
    // 轮询状态机发布的原子证据。
    while Instant::now() < deadline {
        // 观察到按下即成功。
        if TEST_ACCEPTED_KEY_DOWNS.load(Ordering::Acquire) >= 1 {
            // 返回成功。
            return true;
        }
        // 避免协调线程忙等。
        thread::sleep(Duration::from_millis(2));
    }
    // 返回最终一次读取结果。
    TEST_ACCEPTED_KEY_DOWNS.load(Ordering::Acquire) >= 1
}

// 在首次命名键按下后发布确定性取消。
fn cancel_after_key_down() -> thread::JoinHandle<bool> {
    // 启动独立协调线程。
    thread::spawn(|| {
        // 等待正式状态机取得释放责任。
        if !wait_for_owned_down() {
            // 未观察到按下时不伪造取消证据。
            return false;
        }
        // 在长按等待期间发布取消。
        TEST_CANCELLED.store(true, Ordering::Release);
        // 返回成功。
        true
    })
}

// 核对 OutcomeUnknown 包含不可重试与安全释放证据。
fn assert_safe_release(error: &AppControlError, cause_code: &str) {
    // 核对保守结果错误码。
    assert_eq!(error.code, KeyboardInputErrorCode::OutcomeUnknown.as_str());
    // 核对原始中断原因。
    assert_eq!(error.details["causeCode"], cause_code);
    // 核对已经尝试释放按键。
    assert_eq!(error.details["safeReleaseAttempted"], true);
    // 核对 Windows 接受了释放。
    assert_eq!(error.details["safeReleaseSucceeded"], true);
    // 核对没有残留工具按键所有权。
    assert_eq!(error.details["keysHeldByTool"], json!([]));
    // 核对不得自动重试。
    assert_eq!(error.details["retrySafe"], false);
    // 核对明确禁止自动重放。
    assert_eq!(error.details["automaticRetryProhibited"], true);
}

// 过滤不含 Unicode packet 的命名键上下事件。
fn named_key_events(events: &[KeyboardEvent]) -> Vec<KeyboardEvent> {
    // 只保留键盘上下消息并排除 Unicode packet。
    events
        // 迭代事件快照。
        .iter()
        // 复制小型测试事实。
        .copied()
        // 筛选普通和系统按键阶段。
        .filter(|event| {
            // 必须是按键上下消息。
            matches!(
                event.message,
                WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP
            )
                // Unicode packet 单独由 WM_CHAR 证明。
                && event.value != usize::from(VK_PACKET.0)
        })
        // 收集有界结果。
        .collect()
}

// 验证正式 System 路由覆盖快捷键、长按、重复、Unicode 与显式阶段。
#[test]
// 普通并行测试不得改变主人当前前景，需显式串行运行。
#[ignore = "requires an explicit serial foreground-changing fixture run"]
fn production_route_drives_complete_keyboard_primitives() -> Result<(), Box<dyn std::error::Error>>
{
    // 串行化真实前景和键盘影响。
    let _guard = fixture_guard();
    // 清空前一动态测试证据。
    reset_fixture_evidence();
    // 创建项目自有可见窗口。
    let fixture = spawn_fixture()?;
    // 生成正式 canonical opaque 目标。
    let session_id = fixture_session(fixture.handle)?;
    // 构造覆盖三类步骤与显式状态机的请求。
    let input = json!({
        // 给正式前景激活和动作序列留出有界时间。
        "timeoutMs": 5_000,
        // 按稳定顺序执行通用原语。
        "steps": [
            // 覆盖三键快捷键的有序按下与逆序释放。
            { "type": "chord", "keys": ["left-control", "left-shift", "k"], "holdMs": 20 },
            // 覆盖可观察长按。
            { "type": "key", "key": "b", "phase": "press", "holdMs": 80 },
            // 覆盖有界重复。
            { "type": "key", "key": "f6", "phase": "press", "repeat": 3, "intervalMs": 10 },
            // 覆盖非 ASCII 与代理对 Unicode 文本。
            { "type": "text", "text": "通🙂" },
            // 覆盖显式 down。
            { "type": "key", "key": "left-control", "phase": "down" },
            // 持有修饰键期间执行成对单键。
            { "type": "key", "key": "s", "phase": "press" },
            // 覆盖显式 up 并配平所有权。
            { "type": "key", "key": "left-control", "phase": "up" }
        ]
    });
    // 通过正式 ComputerControlSystem app.apply 路由执行。
    let result = AppControlService::new().execute(app_request(&session_id, input))?;
    // 给项目自有消息泵一个短暂排空窗口。
    thread::sleep(Duration::from_millis(100));
    // 读取有界事件快照。
    let events = event_snapshot();
    // 过滤命名键事件。
    let named = named_key_events(&events);
    // 核对公开完成状态。
    assert_eq!(result["data"]["outcome"], "completed");
    // 核对七个公开步骤全部完成。
    assert_eq!(result["data"]["stepCount"], 7);
    // 核对请求内按键所有权已经平衡。
    assert_eq!(result["data"]["keyOwnership"], "request-scoped-balanced");
    // 核对没有残留按键。
    assert_eq!(result["data"]["keysHeldByTool"], json!([]));
    // 至少包含快捷键六事件、长按两事件、重复六事件和显式四事件。
    assert!(named.len() >= 18);
    // 快捷键前三项按 Control、Shift、K 顺序按下。
    assert_eq!(
        named[..3]
            .iter()
            .map(|event| event.value)
            .collect::<Vec<_>>(),
        vec![
            usize::from(VK_CONTROL.0),
            usize::from(VK_SHIFT.0),
            usize::from(b'K')
        ]
    );
    // 快捷键后三项按 K、Shift、Control 逆序释放。
    assert_eq!(
        named[3..6]
            .iter()
            .map(|event| event.value)
            .collect::<Vec<_>>(),
        vec![
            usize::from(b'K'),
            usize::from(VK_SHIFT.0),
            usize::from(VK_CONTROL.0)
        ]
    );
    // 长按 B 的释放时间必须晚于按下至少六十毫秒。
    assert!(named[7].observed_at.duration_since(named[6].observed_at) >= Duration::from_millis(60));
    // F6 重复必须让项目自有窗口收到三次明确释放。
    assert_eq!(
        named
            // 迭代全部命名键事件。
            .iter()
            // 输入法可能把按下规范化为 PROCESSKEY，释放仍保留 F6。
            .filter(|event| event.message == WM_KEYUP && event.value == usize::from(VK_F6.0))
            // 取得事件数量。
            .count(),
        3,
        // 失败时只输出项目自有测试窗口的内部消息事实。
        "named keyboard events: {named:?}"
    );
    // 收集非 ASCII WM_CHAR 单元以排除快捷键控制字符。
    let unicode_units = events
        // 迭代事件快照。
        .iter()
        // 只保留非 ASCII 字符消息。
        .filter(|event| event.message == WM_CHAR && event.value > 0x7f)
        // 安全收窄为 UTF-16 单元。
        .map(|event| u16::try_from(event.value))
        // 收集并传播异常值。
        .collect::<Result<Vec<_>, _>>()?;
    // 核对 Unicode scalar 通过消息泵还原。
    assert_eq!(String::from_utf16(&unicode_units)?, "通🙂");
    // 核对状态机接受预期九次命名键按下。
    assert_eq!(TEST_ACCEPTED_KEY_DOWNS.load(Ordering::Acquire), 9);
    // 核对对应九次释放全部被 Adapter 接受。
    assert_eq!(TEST_ACCEPTED_KEY_UPS.load(Ordering::Acquire), 9);
    // 核对两个 Unicode scalar 完整完成。
    assert_eq!(TEST_ACCEPTED_UNICODE_SCALARS.load(Ordering::Acquire), 2);
    // 返回成功并由 Drop 清理自有窗口。
    Ok(())
}

// 验证长按期间取消会释放修饰键且后续请求恢复。
#[test]
// 普通并行测试不得改变主人当前前景，需显式串行运行。
#[ignore = "requires an explicit serial foreground-changing fixture run"]
fn cancellation_releases_owned_key_and_allows_recovery() -> Result<(), Box<dyn std::error::Error>> {
    // 串行化真实前景和键盘影响。
    let _guard = fixture_guard();
    // 清空前一动态测试证据。
    reset_fixture_evidence();
    // 创建项目自有可见窗口。
    let fixture = spawn_fixture()?;
    // 生成正式 canonical opaque 目标。
    let session_id = fixture_session(fixture.handle)?;
    // 在状态机取得释放责任后发布取消。
    let canceller = cancel_after_key_down();
    // 构造足够长的按键请求。
    let input = json!({
        // 保持 timeout 晚于确定性取消。
        "timeoutMs": 3_000,
        // 长按左控制键供取消命中。
        "steps": [{ "type": "key", "key": "left-control", "phase": "press", "holdMs": 1_000 }]
    });
    // 使用显式测试探针执行同一正式状态机。
    let result = perform_with_cancel_probe(&session_id, true, &input, test_cancelled);
    // 回收取消协调线程。
    let cancelled = canceller
        .join()
        .map_err(|_| "keyboard canceller panicked")?;
    // 协调线程回收后要求取消阻止正常完成。
    let error = match result {
        // 成功表示取消传播失效。
        Ok(_) => return Err("cancelled keyboard press unexpectedly completed".into()),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对协调线程确实观察到按键释放责任。
    assert!(cancelled);
    // 核对取消原因和安全释放证据。
    assert_safe_release(&error, KeyboardInputErrorCode::Cancelled.as_str());
    // 核对一次按下对应一次 best-effort 释放。
    assert_eq!(TEST_ACCEPTED_KEY_DOWNS.load(Ordering::Acquire), 1);
    // 核对 Adapter 接受安全释放。
    assert_eq!(TEST_ACCEPTED_KEY_UPS.load(Ordering::Acquire), 1);
    // 清除确定性探针以允许恢复请求。
    TEST_CANCELLED.store(false, Ordering::Release);
    // 通过正式 System 路由执行后续短按。
    let recovered = AppControlService::new().execute(app_request(
        &session_id,
        // 使用独立功能键避免快捷键副作用。
        json!({ "steps": [{ "type": "key", "key": "f6", "phase": "press" }] }),
    ))?;
    // 核对后续请求完整完成。
    assert_eq!(recovered["data"]["outcome"], "completed");
    // 返回成功并由 Drop 清理自有窗口。
    Ok(())
}

// 验证长按 deadline 到期同样优先释放按键且禁止重试。
#[test]
// 普通并行测试不得改变主人当前前景，需显式串行运行。
#[ignore = "requires an explicit serial foreground-changing fixture run"]
fn timeout_releases_owned_key() -> Result<(), Box<dyn std::error::Error>> {
    // 串行化真实前景和键盘影响。
    let _guard = fixture_guard();
    // 清空前一动态测试证据。
    reset_fixture_evidence();
    // 创建项目自有可见窗口。
    let fixture = spawn_fixture()?;
    // 生成正式 canonical opaque 目标。
    let session_id = fixture_session(fixture.handle)?;
    // 构造 deadline 会在长按等待中到期的请求。
    let input = json!({
        // 给前景激活留出时间但早于完整长按。
        "timeoutMs": 800,
        // 持有右 Shift 直到总 deadline 到期。
        "steps": [{ "type": "key", "key": "right-shift", "phase": "press", "holdMs": 3_000 }]
    });
    // 执行正式生产取消探针路径并显式区分结果。
    let error = match perform(&session_id, true, &input) {
        // 成功表示 deadline 传播失效。
        Ok(_) => return Err("timed out keyboard press unexpectedly completed".into()),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对 timeout 原因和安全释放证据。
    assert_safe_release(&error, KeyboardInputErrorCode::Timeout.as_str());
    // 核对一次按下对应一次 best-effort 释放。
    assert_eq!(TEST_ACCEPTED_KEY_DOWNS.load(Ordering::Acquire), 1);
    // 核对 Adapter 接受安全释放。
    assert_eq!(TEST_ACCEPTED_KEY_UPS.load(Ordering::Acquire), 1);
    // 返回成功并由 Drop 清理自有窗口。
    Ok(())
}
