//! 通过项目自有顶层窗口夹具验证通用指针生产路由与安全释放。

// 导入夹具线程、原子证据、互斥串行化与有界等待。
use std::{
    // 导入原生窗口句柄转换类型。
    ffi::c_void,
    // 导入跨线程共享与原子状态。
    sync::{
        // 导入引用计数停止标记与夹具互斥锁。
        Arc,
        Mutex,
        MutexGuard,
        // 导入原子计数与内存顺序。
        atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicUsize, Ordering},
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
    // 导入窗口句柄、消息参数、点与矩形。
    Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    // 导入屏幕到 client 坐标转换。
    Win32::Graphics::Gdi::ScreenToClient,
    // 导入仅测试夹具使用的前景输入队列协调 API。
    Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId},
    // 导入鼠标捕获 API。
    Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
    // 导入窗口创建、子类化、定位、消息泵与鼠标消息。
    Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow,
        DispatchMessageW, GWLP_WNDPROC, GetCursorPos, GetForegroundWindow, GetSystemMetrics,
        GetWindowRect, GetWindowThreadProcessId, IsWindow, MSG, PM_REMOVE, PeekMessageW,
        SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
        SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, TranslateMessage, WINDOW_EX_STYLE,
        WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
        WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP, WNDPROC,
        WS_OVERLAPPEDWINDOW, WS_VISIBLE,
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

// 串行化会真实改变前景和光标的动态指针测试。
static POINTER_FIXTURE_LOCK: Mutex<()> = Mutex::new(());
// 保存系统 STATIC 原始窗口过程。
static FIXTURE_ORIGINAL_PROC: AtomicIsize = AtomicIsize::new(0);
// 保存确定性取消探针状态。
static TEST_CANCELLED: AtomicBool = AtomicBool::new(false);
// 统计鼠标移动消息。
static MOVE_COUNT: AtomicUsize = AtomicUsize::new(0);
// 统计左键按下或双击按下消息。
static LEFT_DOWN_COUNT: AtomicUsize = AtomicUsize::new(0);
// 统计左键释放消息。
static LEFT_UP_COUNT: AtomicUsize = AtomicUsize::new(0);
// 统计右键按下消息。
static RIGHT_DOWN_COUNT: AtomicUsize = AtomicUsize::new(0);
// 统计右键释放消息。
static RIGHT_UP_COUNT: AtomicUsize = AtomicUsize::new(0);
// 统计中键按下消息。
static MIDDLE_DOWN_COUNT: AtomicUsize = AtomicUsize::new(0);
// 统计中键释放消息。
static MIDDLE_UP_COUNT: AtomicUsize = AtomicUsize::new(0);
// 统计垂直滚轮消息。
static VERTICAL_SCROLL_COUNT: AtomicUsize = AtomicUsize::new(0);
// 统计水平滚轮消息。
static HORIZONTAL_SCROLL_COUNT: AtomicUsize = AtomicUsize::new(0);
// 保存最后鼠标移动消息的 client x。
static LAST_CLIENT_X: AtomicI32 = AtomicI32::new(i32::MIN);
// 保存最后鼠标移动消息的 client y。
static LAST_CLIENT_Y: AtomicI32 = AtomicI32::new(i32::MIN);

// 持有自有窗口和 UI 线程生命周期。
struct PointerFixture {
    // 保存仅供测试内部使用的原生句柄值。
    handle: isize,
    // 保存跨线程停止标记。
    running: Arc<AtomicBool>,
    // 保存可回收 UI 线程。
    worker: Option<thread::JoinHandle<()>>,
}

// 确保测试失败时也只清理项目自有窗口。
impl Drop for PointerFixture {
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

// 从 LPARAM 低字提取有符号 client x。
fn client_x(value: LPARAM) -> i32 {
    // 按 Win32 鼠标消息的有符号 16 位语义扩展。
    (value.0 as u32 & 0xffff) as u16 as i16 as i32
}

// 从 LPARAM 高字提取有符号 client y。
fn client_y(value: LPARAM) -> i32 {
    // 按 Win32 鼠标消息的有符号 16 位语义扩展。
    ((value.0 as u32 >> 16) & 0xffff) as u16 as i16 as i32
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

// 记录自有窗口收到的通用鼠标消息并维护拖拽捕获。
unsafe extern "system" fn pointer_fixture_proc(
    // 接收自有窗口句柄。
    window: HWND,
    // 接收窗口消息。
    message: u32,
    // 接收 WPARAM。
    word: WPARAM,
    // 接收 LPARAM。
    value: LPARAM,
) -> LRESULT {
    // 按固定公开原语对应的消息更新技术证据。
    match message {
        // 鼠标移动记录 client 坐标。
        WM_MOUSEMOVE => {
            // 增加移动计数。
            MOVE_COUNT.fetch_add(1, Ordering::AcqRel);
            // 保存当前 client x。
            LAST_CLIENT_X.store(client_x(value), Ordering::Release);
            // 保存当前 client y。
            LAST_CLIENT_Y.store(client_y(value), Ordering::Release);
        }
        // 左键普通按下和双击按下都取得捕获。
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            // 增加左键按下计数。
            LEFT_DOWN_COUNT.fetch_add(1, Ordering::AcqRel);
            // 让窗口移动后的拖拽消息仍路由到自有夹具。
            let _ = unsafe { SetCapture(window) };
        }
        // 左键释放结束捕获。
        WM_LBUTTONUP => {
            // 增加左键释放计数。
            LEFT_UP_COUNT.fetch_add(1, Ordering::AcqRel);
            // 释放当前线程鼠标捕获。
            let _ = unsafe { ReleaseCapture() };
        }
        // 右键按下取得捕获。
        WM_RBUTTONDOWN => {
            // 增加右键按下计数。
            RIGHT_DOWN_COUNT.fetch_add(1, Ordering::AcqRel);
            // 保持成对消息路由。
            let _ = unsafe { SetCapture(window) };
        }
        // 右键释放结束捕获。
        WM_RBUTTONUP => {
            // 增加右键释放计数。
            RIGHT_UP_COUNT.fetch_add(1, Ordering::AcqRel);
            // 释放当前线程鼠标捕获。
            let _ = unsafe { ReleaseCapture() };
        }
        // 中键按下取得捕获。
        WM_MBUTTONDOWN => {
            // 增加中键按下计数。
            MIDDLE_DOWN_COUNT.fetch_add(1, Ordering::AcqRel);
            // 保持成对消息路由。
            let _ = unsafe { SetCapture(window) };
        }
        // 中键释放结束捕获。
        WM_MBUTTONUP => {
            // 增加中键释放计数。
            MIDDLE_UP_COUNT.fetch_add(1, Ordering::AcqRel);
            // 释放当前线程鼠标捕获。
            let _ = unsafe { ReleaseCapture() };
        }
        // 垂直滚轮只记录固定消息。
        WM_MOUSEWHEEL => {
            // 增加垂直滚轮计数。
            VERTICAL_SCROLL_COUNT.fetch_add(1, Ordering::AcqRel);
        }
        // 水平滚轮只记录固定消息。
        WM_MOUSEHWHEEL => {
            // 增加水平滚轮计数。
            HORIZONTAL_SCROLL_COUNT.fetch_add(1, Ordering::AcqRel);
        }
        // 其余消息不产生测试状态。
        _ => {}
    }
    // 保留系统 STATIC 的默认行为。
    unsafe { call_original(window, message, word, value) }
}

// 清空一个动态测试的全部消息证据。
fn reset_fixture_evidence() {
    // 清除取消探针。
    TEST_CANCELLED.store(false, Ordering::Release);
    // 清除状态机已接受按钮按下计数。
    TEST_ACCEPTED_BUTTON_DOWNS.store(0, Ordering::Release);
    // 清除 Adapter 已接受按钮释放计数。
    TEST_ACCEPTED_BUTTON_UPS.store(0, Ordering::Release);
    // 清除移动计数。
    MOVE_COUNT.store(0, Ordering::Release);
    // 清除左键按下计数。
    LEFT_DOWN_COUNT.store(0, Ordering::Release);
    // 清除左键释放计数。
    LEFT_UP_COUNT.store(0, Ordering::Release);
    // 清除右键按下计数。
    RIGHT_DOWN_COUNT.store(0, Ordering::Release);
    // 清除右键释放计数。
    RIGHT_UP_COUNT.store(0, Ordering::Release);
    // 清除中键按下计数。
    MIDDLE_DOWN_COUNT.store(0, Ordering::Release);
    // 清除中键释放计数。
    MIDDLE_UP_COUNT.store(0, Ordering::Release);
    // 清除垂直滚轮计数。
    VERTICAL_SCROLL_COUNT.store(0, Ordering::Release);
    // 清除水平滚轮计数。
    HORIZONTAL_SCROLL_COUNT.store(0, Ordering::Release);
    // 清除最后 client x。
    LAST_CLIENT_X.store(i32::MIN, Ordering::Release);
    // 清除最后 client y。
    LAST_CLIENT_Y.store(i32::MIN, Ordering::Release);
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
fn spawn_fixture() -> Result<PointerFixture, String> {
    // 创建夹具就绪通道。
    let (ready_tx, ready_rx) = mpsc::channel::<Result<isize, String>>();
    // 创建跨线程停止标记。
    let running = Arc::new(AtomicBool::new(true));
    // 克隆停止标记给 UI 线程。
    let thread_running = Arc::clone(&running);
    // 启动项目自有 UI 线程。
    let worker = thread::spawn(move || {
        // 读取虚拟桌面左上角以兼容负坐标显示器布局。
        let virtual_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        // 读取虚拟桌面顶部。
        let virtual_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        // 创建普通可见系统 STATIC 顶层窗口。
        let window = unsafe {
            // 调用固定系统类，不注册应用专用类。
            CreateWindowExW(
                // 不添加特殊扩展样式。
                WINDOW_EX_STYLE(0),
                // 使用 Windows 内建 STATIC 类。
                w!("STATIC"),
                // 使用唯一且无敏感含义的测试标题。
                w!("Rust Pointer Input General Fixture"),
                // 使用普通可激活顶层窗口样式。
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                // 放在虚拟桌面可见区域内。
                virtual_x.saturating_add(80),
                // 放在虚拟桌面可见区域内。
                virtual_y.saturating_add(80),
                // 使用足够覆盖全部 client 测试点的宽度。
                480,
                // 使用足够覆盖全部 client 测试点的高度。
                360,
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
                pointer_fixture_proc as *const () as usize as isize,
            )
        };
        // 缺失原始过程表示子类化未完成。
        if original == 0 {
            // 向测试线程报告稳定错误。
            let _ = ready_tx.send(Err("pointer fixture subclass failed".to_owned()));
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
                // 执行系统消息翻译。
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
    Ok(PointerFixture {
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
                "The project-owned pointer fixture was not discovered.",
            ));
        }
        // 在下一次实时枚举前短等待。
        thread::sleep(Duration::from_millis(10));
    }
}

// 构造通过正式 app.apply 路由的已确认指针请求。
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
    // 写入版本化指针 capability。
    request
        // 访问参数对象。
        .args
        // 插入 capability。
        .insert(
            "capability".to_owned(),
            json!(capabilities::UI_INPUT_POINTER),
        );
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

// 等待指定消息计数达到下限。
fn wait_for_count(counter: &AtomicUsize, expected: usize) -> bool {
    // 设置短技术证据等待截止时刻。
    let deadline = Instant::now() + Duration::from_secs(2);
    // 轮询消息泵发布的原子证据。
    while Instant::now() < deadline {
        // 达到期望计数即成功。
        if counter.load(Ordering::Acquire) >= expected {
            // 返回成功。
            return true;
        }
        // 避免协调线程忙等。
        thread::sleep(Duration::from_millis(2));
    }
    // 返回最终一次读取结果。
    counter.load(Ordering::Acquire) >= expected
}

// 即使前一测试 panic 也取得夹具串行锁并继续提供独立诊断。
fn fixture_guard() -> MutexGuard<'static, ()> {
    // 恢复被前一失败标记为 poisoned 的无数据互斥锁。
    match POINTER_FIXTURE_LOCK.lock() {
        // 返回正常锁守卫。
        Ok(guard) => guard,
        // 该锁不保护业务数据，安全取回唯一守卫。
        Err(poisoned) => poisoned.into_inner(),
    }
}

// 在拖拽左键按下后移动自有窗口以验证逐点重解析。
fn move_fixture_after_drag_press(
    handle: isize,
    expected_left_down: usize,
) -> thread::JoinHandle<bool> {
    // 启动独立协调线程，避免阻塞正式同步 Command。
    thread::spawn(move || {
        // 等待正式状态机取得拖拽按钮释放责任。
        if !wait_for_count(&TEST_ACCEPTED_BUTTON_DOWNS, expected_left_down) {
            // 未观察到 Adapter 接受拖拽按下时不移动窗口。
            return false;
        }
        // 恢复测试内部原生窗口句柄。
        let window = HWND(handle as *mut c_void);
        // 读取移动前窗口矩形。
        let mut rectangle = RECT::default();
        // 查询失败时返回技术失败。
        if unsafe { GetWindowRect(window, &mut rectangle) }.is_err() {
            // 返回失败。
            return false;
        }
        // 在不改变大小、Z 顺序或前景的情况下移动自有窗口。
        unsafe {
            // 调用固定窗口定位 API。
            SetWindowPos(
                // 只移动项目自有窗口。
                window,
                // 保持原 Z 顺序。
                None,
                // 水平移动固定像素。
                rectangle.left.saturating_add(40),
                // 垂直移动固定像素。
                rectangle.top.saturating_add(20),
                // SWP_NOSIZE 忽略宽度。
                0,
                // SWP_NOSIZE 忽略高度。
                0,
                // 保持大小、Z 顺序和前景。
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        // 把 Windows 调度结果投影为布尔技术证据。
        .is_ok()
    })
}

// 读取当前真实光标并转换为自有窗口 client 坐标。
fn cursor_client_point(handle: isize) -> Result<POINT, String> {
    // 初始化屏幕物理点。
    let mut point = POINT::default();
    // 读取真实系统光标位置。
    unsafe { GetCursorPos(&mut point) }.map_err(|error| error.to_string())?;
    // 恢复测试内部自有窗口句柄。
    let window = HWND(handle as *mut c_void);
    // 转换为窗口当前 client 坐标。
    if !unsafe { ScreenToClient(window, &mut point) }.as_bool() {
        // 返回稳定坐标转换错误。
        return Err("pointer fixture screen-to-client conversion failed".to_owned());
    }
    // 返回真实 client 物理点。
    Ok(point)
}

// 返回确定性动态测试取消状态。
fn test_cancelled() -> bool {
    // 读取仅当前调用持有的测试探针。
    TEST_CANCELLED.load(Ordering::Acquire)
}

// 在首次左键按下后发布确定性取消。
fn cancel_after_left_press() -> thread::JoinHandle<bool> {
    // 启动独立协调线程。
    thread::spawn(|| {
        // 等待正式状态机已经取得按钮释放责任。
        if !wait_for_count(&TEST_ACCEPTED_BUTTON_DOWNS, 1) {
            // 未观察到按下时不伪造取消证据。
            return false;
        }
        // 在下一个拖拽采样前发布取消。
        TEST_CANCELLED.store(true, Ordering::Release);
        // 返回成功。
        true
    })
}

// 核对 OutcomeUnknown 包含不可重试与安全释放证据。
fn assert_safe_release(error: &AppControlError, cause_code: &str) {
    // 核对保守结果错误码。
    assert_eq!(error.code, PointerInputErrorCode::OutcomeUnknown.as_str());
    // 核对原始中断原因。
    assert_eq!(error.details["causeCode"], cause_code);
    // 核对已经尝试释放按钮。
    assert_eq!(error.details["safeReleaseAttempted"], true);
    // 核对 Windows 接受了释放。
    assert_eq!(error.details["safeReleaseSucceeded"], true);
    // 核对没有残留工具按钮所有权。
    assert_eq!(error.details["buttonsHeldByTool"], json!([]));
    // 核对不得自动重试。
    assert_eq!(error.details["retrySafe"], false);
    // 核对明确禁止自动重放。
    assert_eq!(error.details["automaticRetryProhibited"], true);
}

// 验证正式 System 路由覆盖全部通用原语、client 坐标与窗口移动重解析。
#[test]
// 普通并行测试不得改变主人当前前景和光标，需显式串行运行。
#[ignore = "requires an explicit serial foreground-changing fixture run"]
fn production_route_drives_all_general_pointer_primitives() -> Result<(), Box<dyn std::error::Error>>
{
    // 串行化真实前景和光标影响。
    let _guard = fixture_guard();
    // 清空前一动态测试证据。
    reset_fixture_evidence();
    // 创建项目自有可见窗口。
    let fixture = spawn_fixture()?;
    // 生成正式 canonical opaque 目标。
    let session_id = fixture_session(fixture.handle)?;
    // 在右键、双击和中键之后等待拖拽第五次按钮按下并移动窗口。
    let mover = move_fixture_after_drag_press(fixture.handle, 5);
    // 构造覆盖全部五类步骤和三类按钮的请求。
    let input = json!({
        // 使用会受窗口移动影响的 client 物理坐标。
        "coordinateSpace": "window-client-physical-px",
        // 给正式前景激活和动作序列留出有界时间。
        "timeoutMs": 5_000,
        // 按稳定顺序执行通用原语。
        "steps": [
            // 覆盖纯移动。
            { "type": "move", "point": { "x": 80, "y": 80 } },
            // 覆盖右键显式按下。
            { "type": "button", "button": "right", "phase": "down", "point": { "x": 100, "y": 90 } },
            // 覆盖右键显式释放。
            { "type": "button", "button": "right", "phase": "up", "point": { "x": 100, "y": 90 } },
            // 覆盖左键双击宏。
            { "type": "click", "button": "left", "count": 2, "intervalMs": 60, "point": { "x": 120, "y": 110 } },
            // 覆盖中键单击宏。
            { "type": "click", "button": "middle", "count": 1, "point": { "x": 140, "y": 130 } },
            // 覆盖垂直滚轮。
            { "type": "scroll", "axis": "vertical", "ticks": 1, "point": { "x": 160, "y": 150 } },
            // 覆盖水平滚轮。
            { "type": "scroll", "axis": "horizontal", "ticks": -1, "point": { "x": 180, "y": 170 } },
            // 覆盖窗口移动期间的请求内左键拖拽。
            { "type": "drag", "button": "left", "start": { "x": 200, "y": 180 }, "end": { "x": 300, "y": 220 }, "durationMs": 360, "samples": 6 }
        ]
    });
    // 通过正式 ComputerControlSystem app.apply 路由执行并保留协调线程回收责任。
    let result = AppControlService::new().execute(app_request(&session_id, input));
    // 回收窗口移动协调线程。
    let moved = mover.join().map_err(|_| "pointer mover panicked")?;
    // 协调线程回收后再传播正式路由错误。
    let result = result?;
    // 核对窗口确实在拖拽期间移动。
    assert!(moved);
    // 核对公开完成状态。
    assert_eq!(result["data"]["outcome"], "completed");
    // 核对八个公开步骤全部完成。
    assert_eq!(result["data"]["stepCount"], 8);
    // 核对请求内按钮所有权已经平衡。
    assert_eq!(result["data"]["buttonOwnership"], "request-scoped-balanced");
    // 核对没有残留按钮。
    assert_eq!(result["data"]["buttonsHeldByTool"], json!([]));
    // 核对 Adapter 接受了右键、双击、中键和拖拽的五次按下。
    assert_eq!(TEST_ACCEPTED_BUTTON_DOWNS.load(Ordering::Acquire), 5);
    // 核对 Adapter 接受了对应五次释放。
    assert_eq!(TEST_ACCEPTED_BUTTON_UPS.load(Ordering::Acquire), 5);
    // 读取 Windows 当前真实光标的最终 client 坐标。
    let final_point = cursor_client_point(fixture.handle)?;
    // 核对窗口移动后最终 client x 仍等于公开终点。
    assert_eq!(final_point.x, 300);
    // 核对窗口移动后最终 client y 仍等于公开终点。
    assert_eq!(final_point.y, 220);
    // 返回成功并由 Drop 清理自有窗口。
    Ok(())
}

// 验证拖拽期间取消会报告 OutcomeUnknown 并优先释放按钮。
#[test]
// 普通并行测试不得改变主人当前前景和光标，需显式串行运行。
#[ignore = "requires an explicit serial foreground-changing fixture run"]
fn cancellation_during_drag_releases_owned_button() -> Result<(), Box<dyn std::error::Error>> {
    // 串行化真实前景和光标影响。
    let _guard = fixture_guard();
    // 清空前一动态测试证据。
    reset_fixture_evidence();
    // 创建项目自有可见窗口。
    let fixture = spawn_fixture()?;
    // 生成正式 canonical opaque 目标。
    let session_id = fixture_session(fixture.handle)?;
    // 在拖拽取得按钮责任后发布确定性取消。
    let canceller = cancel_after_left_press();
    // 构造足够长的单拖拽请求。
    let input = json!({
        // 使用窗口 client 物理坐标。
        "coordinateSpace": "window-client-physical-px",
        // 保持 timeout 晚于确定性取消。
        "timeoutMs": 3_000,
        // 仅执行一个具有完整生命周期的拖拽。
        "steps": [
            // 首个采样等待给取消探针留下确定性窗口。
            { "type": "drag", "button": "left", "start": { "x": 100, "y": 100 }, "end": { "x": 280, "y": 200 }, "durationMs": 1_000, "samples": 10 }
        ]
    });
    // 使用显式测试探针执行同一正式状态机并保留协调线程回收责任。
    let result = perform_with_cancel_probe(&session_id, true, &input, test_cancelled);
    // 回收取消协调线程。
    let cancelled = canceller.join().map_err(|_| "pointer canceller panicked")?;
    // 协调线程回收后要求取消阻止正常完成。
    let error = match result {
        // 成功表示取消传播失效。
        Ok(_) => return Err("cancelled drag unexpectedly completed".into()),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对协调线程确实观察到按钮释放责任。
    assert!(cancelled);
    // 核对取消原因和安全释放证据。
    assert_safe_release(&error, PointerInputErrorCode::Cancelled.as_str());
    // 核对 Adapter 接受一次拖拽按下。
    assert_eq!(TEST_ACCEPTED_BUTTON_DOWNS.load(Ordering::Acquire), 1);
    // 核对 Adapter 接受一次 best-effort 安全释放。
    assert_eq!(TEST_ACCEPTED_BUTTON_UPS.load(Ordering::Acquire), 1);
    // 清除确定性探针避免污染后续测试。
    TEST_CANCELLED.store(false, Ordering::Release);
    // 返回成功并由 Drop 清理自有窗口。
    Ok(())
}

// 验证拖拽 deadline 到期同样优先释放按钮且禁止重试。
#[test]
// 普通并行测试不得改变主人当前前景和光标，需显式串行运行。
#[ignore = "requires an explicit serial foreground-changing fixture run"]
fn timeout_during_drag_releases_owned_button() -> Result<(), Box<dyn std::error::Error>> {
    // 串行化真实前景和光标影响。
    let _guard = fixture_guard();
    // 清空前一动态测试证据。
    reset_fixture_evidence();
    // 创建项目自有可见窗口。
    let fixture = spawn_fixture()?;
    // 生成正式 canonical opaque 目标。
    let session_id = fixture_session(fixture.handle)?;
    // 构造 deadline 会在首次拖拽采样前到期的请求。
    let input = json!({
        // 使用窗口 client 物理坐标。
        "coordinateSpace": "window-client-physical-px",
        // 保证激活完成后仍会在长拖拽等待中到期。
        "timeoutMs": 800,
        // 仅执行一个长拖拽。
        "steps": [
            // 首个采样为一点五秒，晚于全局 deadline。
            { "type": "drag", "button": "left", "start": { "x": 100, "y": 100 }, "end": { "x": 280, "y": 200 }, "durationMs": 3_000, "samples": 2 }
        ]
    });
    // 执行正式生产取消探针路径并显式区分结果。
    let error = match perform(&session_id, true, &input) {
        // 成功表示 deadline 传播失效。
        Ok(_) => return Err("timed out drag unexpectedly completed".into()),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对 timeout 原因和安全释放证据。
    assert_safe_release(&error, PointerInputErrorCode::Timeout.as_str());
    // 核对 Adapter 接受一次拖拽按下。
    assert_eq!(TEST_ACCEPTED_BUTTON_DOWNS.load(Ordering::Acquire), 1);
    // 核对 Adapter 接受一次 best-effort 安全释放。
    assert_eq!(TEST_ACCEPTED_BUTTON_UPS.load(Ordering::Acquire), 1);
    // 返回成功并由 Drop 清理自有窗口。
    Ok(())
}
