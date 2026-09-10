// 导入线程间夹具协调与有界等待工具。
use std::{
    // 导入跨线程停止标记。
    sync::{
        // 导入原子句柄与停止标记。
        Arc,
        // 导入原子类型与内存顺序。
        atomic::{AtomicBool, AtomicIsize, Ordering},
        // 导入夹具就绪通道。
        mpsc,
    },
    // 导入独立 UI 线程。
    thread,
    // 导入短轮询时长。
    time::Duration,
};

// 导入自有 no-activate 窗口夹具所需固定 Windows API。
use windows::{
    // 导入窗口过程句柄与消息参数。
    Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    // 导入窗口创建、子类化、消息泵与固定 WM_CLOSE。
    Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        GWLP_WNDPROC, IsWindow, MSG, PM_REMOVE, PeekMessageW, SetWindowLongPtrW, TranslateMessage,
        WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WNDPROC, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_OVERLAPPED, WS_VISIBLE,
    },
    // 导入编译期 UTF-16 字符串宏。
    core::w,
};

// 导入被测 Module 私有 helper。
use super::*;
// 导入正式 app facade 与 Adapter 接口。
use crate::adapters::{AppAdapter, AppFacadeAdapter};
// 导入请求与动词类型。
use crate::domain::{CommandRequest, Verb};

// 保存忽略 WM_CLOSE 夹具的系统原始窗口过程。
static IGNORE_CLOSE_ORIGINAL_PROC: AtomicIsize = AtomicIsize::new(0);

// 构造不触碰真实桌面的稳定私有窗口记录。
fn window_record(handle: isize) -> WindowRecord {
    // 返回测试专用私有事实。
    WindowRecord {
        // legacy session 不参与正式匹配。
        session_id: format!("window:{handle}"),
        // 保存测试句柄值。
        hwnd: handle,
        // 使用无敏感含义标题。
        title: "Fixture".to_owned(),
        // 使用无敏感含义类名。
        class_name: "FixtureClass".to_owned(),
        // 使用稳定测试 PID。
        process_id: 42,
        // 使用安全进程名。
        process_name: Some("fixture.exe".to_owned()),
        // 标记夹具可见。
        visible: true,
        // 使用稳定测试进程代际。
        process_creation_time: 123,
    }
}

// 仅忽略认证 WM_CLOSE，其余消息继续交给系统 STATIC 原始过程。
unsafe extern "system" fn ignore_close_window_proc(
    // 接收自有窗口句柄。
    window: HWND,
    // 接收窗口消息。
    message: u32,
    // 接收 WPARAM。
    word: WPARAM,
    // 接收 LPARAM。
    value: LPARAM,
) -> LRESULT {
    // 让固定关闭请求保持未完成以触发真实 deadline。
    if message == WM_CLOSE {
        // 明确吞掉自有夹具关闭消息。
        return LRESULT(0);
    }
    // 读取系统 STATIC 原始窗口过程。
    let original = IGNORE_CLOSE_ORIGINAL_PROC.load(Ordering::Acquire);
    // 缺失原始过程时使用 DefWindowProc fail safe。
    if original == 0 {
        // 调用系统默认窗口过程。
        return unsafe { DefWindowProcW(window, message, word, value) };
    }
    // 将系统返回值恢复为封闭 WNDPROC 类型。
    let procedure = unsafe { std::mem::transmute::<isize, WNDPROC>(original) };
    // 转发所有非关闭消息。
    unsafe { CallWindowProcW(procedure, window, message, word, value) }
}

// 创建位于屏幕外且不激活的自有窗口，并运行有界消息泵。
fn spawn_fixture(
    // 决定是否忽略固定 WM_CLOSE。
    ignore_close: bool,
) -> (
    // 返回跨线程停止标记。
    Arc<AtomicBool>,
    // 返回窗口句柄接收端。
    mpsc::Receiver<Result<isize, String>>,
    // 返回 UI 线程 join handle。
    thread::JoinHandle<()>,
) {
    // 创建就绪通道。
    let (ready_tx, ready_rx) = mpsc::channel::<Result<isize, String>>();
    // 创建停止标记。
    let running = Arc::new(AtomicBool::new(true));
    // 克隆停止标记给 UI 线程。
    let thread_running = Arc::clone(&running);
    // 启动自有 UI 线程。
    let fixture_thread = thread::spawn(move || {
        // 合并 no-activate 与 tool-window 扩展样式。
        let extended = WINDOW_EX_STYLE(WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0);
        // 合并顶层与 visible 样式，使正式 sessions 可发现。
        let style = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_VISIBLE.0);
        // 创建位于屏幕外的系统 STATIC 自有窗口。
        let window = unsafe {
            // 调用固定系统类，不注册任意应用类。
            CreateWindowExW(
                // 禁止夹具激活前景。
                extended,
                // 使用系统 STATIC 类。
                w!("STATIC"),
                // 使用测试专属非空标题。
                w!("Rust Window Close NoActivate Fixture"),
                // 使窗口进入顶层枚举但保持屏幕外。
                style,
                // 放置到可视桌面外。
                -32_000,
                // 放置到可视桌面外。
                -32_000,
                // 使用最小有界宽度。
                32,
                // 使用最小有界高度。
                32,
                // 顶层窗口无父级。
                None,
                // 不提供菜单。
                None,
                // 使用当前模块实例。
                None,
                // 不传自定义指针。
                None,
            )
        };
        // 创建失败时通知主测试并退出。
        let window = match window {
            // 保存有效句柄。
            Ok(window) => window,
            // 发送稳定错误文本。
            Err(error) => {
                // 忽略接收端提前退出。
                let _ = ready_tx.send(Err(error.to_string()));
                // 结束夹具线程。
                return;
            }
        };
        // 挂起夹具需要只忽略固定 WM_CLOSE。
        if ignore_close {
            // 安装测试窗口过程并保存系统原始过程。
            let original = unsafe {
                // 子类化仅限本进程自有 STATIC 窗口。
                SetWindowLongPtrW(
                    window,
                    GWLP_WNDPROC,
                    ignore_close_window_proc as *const () as usize as isize,
                )
            };
            // 发布原始过程给测试回调。
            IGNORE_CLOSE_ORIGINAL_PROC.store(original, Ordering::Release);
        }
        // 把自有句柄交给主测试，不经过公共边界。
        let _ = ready_tx.send(Ok(window.0 as isize));
        // 初始化线程消息。
        let mut message = MSG::default();
        // 运行到窗口关闭或测试请求清理。
        while thread_running.load(Ordering::Acquire)
            // 只读检查自有窗口是否仍存在。
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
            thread::sleep(Duration::from_millis(5));
        }
        // 测试失败或超时后只清理本进程自有窗口。
        if unsafe { IsWindow(Some(window)) }.as_bool() {
            // DestroyWindow 仅在创建窗口的同一线程调用。
            let _ = unsafe { DestroyWindow(window) };
        }
        // 清空跨测试原始过程状态。
        IGNORE_CLOSE_ORIGINAL_PROC.store(0, Ordering::Release);
    });
    // 返回夹具协调对象。
    (running, ready_rx, fixture_thread)
}

// 从实时清单为已知自有句柄生成 canonical s2:w。
fn fixture_session(handle: isize) -> AppResult<String> {
    // 枚举当前窗口私有事实。
    let windows = enumerate_windows()?;
    // 只匹配测试线程已经返回的自有句柄。
    let record = windows
        // 遍历当前清单。
        .iter()
        // 比较私有句柄只发生在测试内部。
        .find(|record| record.hwnd == handle)
        // 缺失表示夹具未进入正式发现链。
        .ok_or_else(|| {
            // 返回结构化测试错误。
            AppControlError::new(
                // 使用测试夹具错误码。
                "FIXTURE_UNAVAILABLE",
                // 不公开句柄值。
                "The owned no-activate window fixture was not discovered.",
            )
        })?;
    // 从实时私有事实生成公开 opaque ID。
    Ok(opaque_window_session_id(record))
}

// 验证 confirmation-first 先于参数和发现。
#[test]
fn mutation_requires_confirmation_first() {
    // 使用空目标和非法 timeout 调用未确认操作。
    let error = match close("", false, 0) {
        // 成功表示确认门禁失效。
        Ok(_) => panic!("unconfirmed close must fail"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 确认错误必须优先返回。
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

// 验证零命中与多命中均 fail closed。
#[test]
fn opaque_window_resolution_is_unique() {
    // 构造一个稳定候选。
    let first = window_record(100);
    // 生成其 canonical 目标。
    let session_id = opaque_window_session_id(&first);
    // 空清单必须返回 stale。
    let missing = match resolve_window(&session_id, &[]) {
        // 成功表示零命中门禁失效。
        Ok(_) => panic!("missing target must fail"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对 stale 错误码。
    assert_eq!(missing.code, "STALE_SESSION");
    // 使用相同身份构造第二个碰撞候选。
    let second = first.clone();
    // 多命中必须返回歧义。
    let ambiguous = match resolve_window(&session_id, &[first, second]) {
        // 成功表示碰撞门禁失效。
        Ok(_) => panic!("collision must fail"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对歧义错误码。
    assert_eq!(ambiguous.code, "AMBIGUOUS_TARGET");
}

// 验证 provider 输入只接受可选 timeoutMs。
#[test]
fn provider_input_is_closed_and_bounded() {
    // 空对象使用契约默认值。
    let default_timeout = match provider_input(&json!({})) {
        // 保存已验证默认值。
        Ok(timeout) => timeout,
        // 测试输入固定合法，错误即失败。
        Err(error) => panic!("default timeout failed: {}", error.code),
    };
    // 核对契约默认值。
    assert_eq!(default_timeout, 2_000);
    // 任意消息字段必须被拒绝。
    let arbitrary = match provider_input(&json!({ "message": "WM_CLOSE" })) {
        // 成功表示任意消息越过封闭输入。
        Ok(_) => panic!("arbitrary message must fail"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对参数错误码。
    assert_eq!(arbitrary.code, "INVALID_ARGUMENT");
    // 超出上限必须被拒绝。
    let unbounded = match provider_input(&json!({ "timeoutMs": 30_001 })) {
        // 成功表示 deadline 边界失效。
        Ok(_) => panic!("unbounded timeout must fail"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对参数错误码。
    assert_eq!(unbounded.code, "INVALID_ARGUMENT");
}

// 验证 provider-neutral stale 窗口由 Window Close Module 返回稳定错误。
#[test]
fn facade_preserves_window_close_stale_error() {
    // 构造不存在的 canonical 窗口目标。
    let mut request = CommandRequest::read(Verb::Run, "app");
    // 选择统一 close 动词。
    request.operation = Some("close".to_owned());
    // 绑定不会命中当前窗口清单的测试 opaque ID。
    request.target.insert(
        // 使用固定 sessionId 字段。
        "sessionId".to_owned(),
        // 提供 canonical 窗口形状。
        json!("s2:w:0000000000000000"),
    );
    // 声明固定 Window Close capability。
    request.args.insert(
        // 使用固定 capability 字段。
        "capability".to_owned(),
        // 使用版本化 ID。
        json!(capabilities::WINDOW_CLOSE),
    );
    // 提供空 provider-neutral input。
    request
        // 访问参数对象。
        .args
        // 插入空输入对象。
        .insert("input".to_owned(), json!({}));
    // 提供逐操作确认。
    request.confirmed = true;
    // 执行正式 facade 并保存预期错误。
    let error = match AppFacadeAdapter::new().run(&request) {
        // 成功表示 stale 门禁失效。
        Ok(_) => panic!("stale window close must fail"),
        // 保存结构化错误。
        Err(error) => error,
    };
    // 核对契约 stale 错误码。
    assert_eq!(error.code, "STALE_SESSION");
}

// 验证正式 app facade 只关闭自有 no-activate 窗口且前景不变。
#[test]
fn real_owned_no_activate_window_closes_through_facade() -> AppResult<()> {
    // 创建正常处理 WM_CLOSE 的自有夹具。
    let (running, ready_rx, fixture_thread) = spawn_fixture(false);
    // 有界等待夹具就绪。
    let handle = ready_rx
        // 最多等待两秒。
        .recv_timeout(Duration::from_secs(2))
        // 映射通道超时。
        .map_err(|error| AppControlError::new("FIXTURE_UNAVAILABLE", error.to_string()))?
        // 映射窗口创建失败。
        .map_err(|error| AppControlError::new("FIXTURE_UNAVAILABLE", error))?;
    // 记录测试写前前景句柄。
    let foreground_before = foreground_hwnd();
    // 在清理保护范围内执行正式 facade。
    let operation = (|| -> AppResult<(Value, String)> {
        // 从当前清单生成自有 opaque 目标。
        let session_id = fixture_session(handle)?;
        // 构造 provider-neutral app.close 请求。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 选择统一 close 动词。
        request.operation = Some("close".to_owned());
        // 绑定精确 opaque 自有窗口。
        request
            // 访问目标对象。
            .target
            // 插入固定 sessionId。
            .insert("sessionId".to_owned(), json!(session_id));
        // 声明固定 window.close capability。
        request.args.insert(
            // 使用固定 capability 字段。
            "capability".to_owned(),
            // 传入版本化 ID。
            json!(capabilities::WINDOW_CLOSE),
        );
        // 提供空 provider-neutral input 以使用默认 deadline。
        request
            // 访问参数对象。
            .args
            // 插入空输入。
            .insert("input".to_owned(), json!({}));
        // 提供逐操作确认。
        request.confirmed = true;
        // 通过正式 app facade 关闭唯一自有窗口。
        let result = AppFacadeAdapter::new().run(&request)?;
        // 返回验证所需安全结果与 opaque 目标。
        Ok((result, session_id))
    })();
    // 无论结果如何都停止自有线程。
    running.store(false, Ordering::Release);
    // 等待夹具线程清理。
    fixture_thread
        // join panic 转为测试错误。
        .join()
        // 不公开线程内部状态。
        .map_err(|_| AppControlError::new("FIXTURE_UNAVAILABLE", "Fixture thread failed."))?;
    // 传播正式 facade 错误。
    let (result, session_id) = operation?;
    // 核对 capability。
    assert_eq!(result["capability"], capabilities::WINDOW_CLOSE);
    // 核对调用方 opaque 目标回显。
    assert_eq!(result["targetId"], session_id);
    // 核对固定兼容形状。
    assert_eq!(
        result["compatibilityShape"],
        "provider-neutral-window-close-v1"
    );
    // 核对关闭证据。
    assert_eq!(result["data"]["closed"], true);
    // 核对稳定状态。
    assert_eq!(result["data"]["state"], "closed");
    // 公共结果不得包含 native 标识。
    let serialized = result.to_string();
    // 禁止 HWND 字段。
    assert!(!serialized.to_ascii_lowercase().contains("hwnd"));
    // 禁止进程 ID 字段。
    assert!(!serialized.contains("processId"));
    // 核对前景身份保持不变。
    assert_eq!(foreground_before, foreground_hwnd());
    // 返回成功。
    Ok(())
}

// 验证真实已排队 WM_CLOSE 超时返回不可自动重试的未知结果。
#[test]
fn real_owned_ignored_close_reports_unknown_timeout() -> AppResult<()> {
    // 创建忽略 WM_CLOSE 的自有夹具。
    let (running, ready_rx, fixture_thread) = spawn_fixture(true);
    // 有界等待夹具就绪。
    let handle = ready_rx
        // 最多等待两秒。
        .recv_timeout(Duration::from_secs(2))
        // 映射通道超时。
        .map_err(|error| AppControlError::new("FIXTURE_UNAVAILABLE", error.to_string()))?
        // 映射窗口创建失败。
        .map_err(|error| AppControlError::new("FIXTURE_UNAVAILABLE", error))?;
    // 记录写前前景句柄。
    let foreground_before = foreground_hwnd();
    // 在清理保护范围内执行正式 Module。
    let operation = (|| -> AppResult<AppControlError> {
        // 从当前清单生成自有 opaque 目标。
        let session_id = fixture_session(handle)?;
        // 使用 50ms deadline 请求关闭。
        close(&session_id, true, 50)
            // 成功表示夹具未正确忽略 WM_CLOSE。
            .map(|_| {
                // 构造明确测试失败。
                AppControlError::new("FIXTURE_FAILED", "Ignored close unexpectedly succeeded.")
            })
            // 正式错误作为测试结果返回。
            .or_else(Ok)
    })();
    // 请求 UI 线程清理本进程自有窗口。
    running.store(false, Ordering::Release);
    // 等待夹具线程结束。
    fixture_thread
        // join panic 转为测试错误。
        .join()
        // 不公开线程内部状态。
        .map_err(|_| AppControlError::new("FIXTURE_UNAVAILABLE", "Fixture thread failed."))?;
    // 取得正式错误结果。
    let error = operation?;
    // 核对 timeout 错误码。
    assert_eq!(error.code, "TIMEOUT");
    // 核对结果未知。
    assert_eq!(error.details["outcome"], "unknown");
    // 核对禁止自动重试。
    assert_eq!(error.details["retrySafe"], false);
    // 核对目标可能稍后关闭。
    assert_eq!(error.details["targetMayCloseLater"], true);
    // 核对前景身份保持不变。
    assert_eq!(foreground_before, foreground_hwnd());
    // 返回成功。
    Ok(())
}
