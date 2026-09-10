//! 工具自有子进程上的精确进程终止动态夹具。

// 导入子进程、取消协调与有界等待工具。
use std::{
    // 启动当前 Rust 测试二进制作为唯一可终止子进程。
    process::{Child, Command, Stdio},
    // 导入跨线程取消状态。
    sync::atomic::{AtomicBool, AtomicIsize, Ordering},
    // 导入取消线程与短轮询。
    thread,
    // 导入有界等待时长。
    time::{Duration, Instant},
};

// 导入项目自有 no-activate 窗口夹具所需 Win32 API。
use windows::{
    // 导入窗口过程与消息类型。
    Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    // 导入窗口创建、消息泵与子类化 API。
    Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        GWLP_WNDPROC, IsWindow, MSG, PM_REMOVE, PeekMessageW, SetWindowLongPtrW, TranslateMessage,
        WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WNDPROC, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_OVERLAPPED, WS_VISIBLE,
    },
    // 导入固定 UTF-16 字符串宏。
    core::w,
};

// 导入父 Module 状态机与正式 System 路由。
use super::*;
// 导入统一 System 与请求类型。
use crate::{
    // 通过 launcher 使用的同一 System 入口执行动态测试。
    AppControlService,
    // 使用稳定 capability ID。
    capabilities,
    // 构造 JSON over stdio 等价请求。
    domain::{AppControlError, AppResult, CommandRequest, Verb},
};

// 固定子进程测试的完整 test harness 名称。
const CHILD_TEST_NAME: &str =
    "modules::process_termination::dynamic_tests::process_termination_fixture_child";
// 固定子进程夹具环境变量。
const CHILD_MODE_ENV: &str = "ACT_PROCESS_TERMINATION_FIXTURE_MODE";
// 保存忽略 WM_CLOSE 时的系统原始窗口过程。
static ORIGINAL_WINDOW_PROC: AtomicIsize = AtomicIsize::new(0);
// 保存父测试注入的接受后取消状态。
static TEST_CANCELLED: AtomicBool = AtomicBool::new(false);

// 只吞掉自有子进程窗口的固定 WM_CLOSE。
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
    // 只忽略固定关闭请求以形成可控 timeout/cancellation。
    if message == WM_CLOSE {
        // 明确保持子进程运行。
        return LRESULT(0);
    }
    // 读取系统 STATIC 原始窗口过程。
    let original = ORIGINAL_WINDOW_PROC.load(Ordering::Acquire);
    // 缺失原始过程时使用系统默认处理。
    if original == 0 {
        // 调用 DefWindowProcW。
        return unsafe { DefWindowProcW(window, message, word, value) };
    }
    // 恢复封闭 WNDPROC 类型。
    let procedure = unsafe { std::mem::transmute::<isize, WNDPROC>(original) };
    // 转发所有非关闭消息。
    unsafe { CallWindowProcW(procedure, window, message, word, value) }
}

// 运行仅由父测试启动的子进程窗口夹具。
#[test]
#[ignore = "spawned only as a tool-owned child process fixture"]
fn process_termination_fixture_child() {
    // 未设置专用环境变量时立即返回，避免全 ignored 测试误阻塞。
    let Ok(mode) = std::env::var(CHILD_MODE_ENV) else {
        // 当前进程不是父测试创建的 fixture。
        return;
    };
    // 合并 no-activate 与 tool-window 样式。
    let extended = WINDOW_EX_STYLE(WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0);
    // 创建可枚举但屏幕外的顶层窗口。
    let style = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_VISIBLE.0);
    // 使用系统 STATIC 类创建项目自有窗口。
    let window = unsafe {
        CreateWindowExW(
            // 禁止夹具抢占前景。
            extended,
            // 使用系统 STATIC 类。
            w!("STATIC"),
            // 提供唯一非空测试标题。
            w!("Rust Process Termination Owned Child Fixture"),
            // 使用顶层可见样式供当前 inventory 发现。
            style,
            // 放到虚拟桌面外。
            -32_000,
            // 放到虚拟桌面外。
            -32_000,
            // 使用最小固定宽度。
            32,
            // 使用最小固定高度。
            32,
            // 顶层窗口没有父级。
            None,
            // 不提供菜单。
            None,
            // 使用当前模块实例。
            None,
            // 不传任意指针。
            None,
        )
    }
    // 子进程创建失败应让 harness 失败而不是静默等待。
    .unwrap_or_else(|error| panic!("owned process fixture window failed: {error}"));
    // ignore 模式只吞掉固定 WM_CLOSE。
    if mode == "ignore-close" {
        // 子类化仅发生在当前工具自有子进程窗口。
        let original = unsafe {
            SetWindowLongPtrW(
                // 传入自有窗口。
                window,
                // 替换窗口过程。
                GWLP_WNDPROC,
                // 传入固定测试过程。
                ignore_close_window_proc as *const () as usize as isize,
            )
        };
        // 发布原始过程给同一子进程回调。
        ORIGINAL_WINDOW_PROC.store(original, Ordering::Release);
    }
    // 建立最长一分钟的夹具自清理 deadline。
    let deadline = Instant::now() + Duration::from_secs(60);
    // 初始化消息结构。
    let mut message = MSG::default();
    // 运行到默认 WM_CLOSE 销毁窗口或父测试强制终止进程。
    while unsafe { IsWindow(Some(window)) }.as_bool() && Instant::now() < deadline {
        // 处理当前队列中的全部消息。
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            // 执行系统消息翻译。
            let _ = unsafe { TranslateMessage(&message) };
            // 分派到默认或忽略关闭过程。
            unsafe { DispatchMessageW(&message) };
        }
        // 避免空消息泵忙等。
        thread::sleep(Duration::from_millis(5));
    }
    // 自清理 deadline 到达时只销毁当前进程自有窗口。
    if unsafe { IsWindow(Some(window)) }.as_bool() {
        // DestroyWindow 仍在创建线程调用。
        let _ = unsafe { DestroyWindow(window) };
    }
    // 清空子进程测试状态。
    ORIGINAL_WINDOW_PROC.store(0, Ordering::Release);
}

// 保存父测试唯一拥有并负责清理的子进程。
struct OwnedChild {
    // 保存 Rust 测试 harness 子进程。
    child: Child,
}

// 保证失败测试也只清理自己启动的子进程。
impl Drop for OwnedChild {
    // 尝试回收工具自有子进程。
    fn drop(&mut self) {
        // 仅在子进程仍运行时执行 fixture cleanup。
        if self.child.try_wait().ok().flatten().is_none() {
            // Child::kill 只作用于本对象直接启动的测试进程。
            let _ = self.child.kill();
            // 等待句柄回收，避免孤儿测试进程。
            let _ = self.child.wait();
        }
    }
}

// 启动当前 Rust 测试二进制中的唯一 fixture test。
fn spawn_owned_child(mode: &str) -> Result<OwnedChild, Box<dyn std::error::Error>> {
    // 定位当前测试二进制。
    let executable = std::env::current_exe()?;
    // 启动精确 ignored fixture test，禁止继承交互 stdio。
    let child = Command::new(executable)
        // 只运行固定子进程夹具。
        .args([
            // 要求精确 test 名称。
            "--exact",
            // 使用编译期固定名称。
            CHILD_TEST_NAME,
            // 允许显式运行 ignored fixture。
            "--ignored",
            // 子进程只使用一个测试线程。
            "--test-threads=1",
        ])
        // 传入固定夹具模式。
        .env(CHILD_MODE_ENV, mode)
        // 不继承父测试 stdin。
        .stdin(Stdio::null())
        // 隐藏测试 harness 输出。
        .stdout(Stdio::null())
        // 隐藏测试 harness 错误流。
        .stderr(Stdio::null())
        // 启动工具自有子进程。
        .spawn()?;
    // 返回带失败清理的所有者。
    Ok(OwnedChild { child })
}

// 等待子进程代际与顶层窗口进入正式 inventory。
fn wait_for_session(child: &mut OwnedChild) -> AppResult<String> {
    // 建立两秒 fixture 启动 deadline。
    let deadline = Instant::now() + Duration::from_secs(2);
    // 轮询正式只读 inventory。
    loop {
        // 子进程提前退出表示 fixture 未建立。
        if child
            // 查询当前 child 状态。
            .child
            // 调用失败映射为 fixture 错误。
            .try_wait()
            .map_err(|error| AppControlError::new("FIXTURE_UNAVAILABLE", error.to_string()))?
            // 已退出时返回 true。
            .is_some()
        {
            // 返回不含原生进程值的稳定错误。
            return Err(AppControlError::new(
                "FIXTURE_UNAVAILABLE",
                "The owned child process exited before discovery.",
            ));
        }
        // 捕获当前完整进程 inventory。
        let inventory = enumerate_process_inventory(INVENTORY_LIMIT)?;
        // 查找父测试直接拥有的 PID。
        if let Some(process) = inventory.records.iter().find(|process| {
            // PID 比较只发生在测试内部。
            process.process_id == child.child.id()
                // 必须具有可重新解析创建代际。
                && process.identity_reliable
        }) {
            // 枚举当前顶层窗口以证明优雅协议已就绪。
            let windows = enumerate_windows()?;
            // 查找同一 PID 与创建代际的自有窗口。
            let ready = windows.iter().any(|window| {
                // 核对 child PID。
                window.process_id == process.process_id
                    // 核对同一创建代际。
                    && window.process_creation_time == process.process_creation_time
            });
            // 窗口已就绪时返回正式 opaque 目标。
            if ready {
                // 只返回 canonical session ID。
                return Ok(process.session_id.clone());
            }
        }
        // 到达 deadline 后结构化失败。
        if Instant::now() >= deadline {
            // 不公开 PID、句柄或路径。
            return Err(AppControlError::new(
                "FIXTURE_UNAVAILABLE",
                "The owned child process was not discovered with a top-level window.",
            ));
        }
        // 短暂等待子进程窗口建立。
        thread::sleep(Duration::from_millis(20));
    }
}

// 构造正式统一 app.close 请求。
fn termination_request(
    // 接收工具自有子进程 opaque 目标。
    session_id: &str,
    // 接收两个固定 capability 之一。
    capability: &str,
    // 接收有界 deadline。
    timeout_ms: u32,
) -> CommandRequest {
    // 从统一 app.run 请求开始。
    let mut request = CommandRequest::read(Verb::Run, "app");
    // 两个进程终止 capability 均使用 generic close。
    request.operation = Some("close".to_owned());
    // 写入精确 opaque 目标。
    request
        // 访问目标对象。
        .target
        // 插入固定 sessionId。
        .insert("sessionId".to_owned(), json!(session_id));
    // 写入版本化 capability。
    request
        // 访问参数对象。
        .args
        // 插入固定 capability 字段。
        .insert("capability".to_owned(), json!(capability));
    // 写入严格 provider-neutral input。
    request
        // 访问参数对象。
        .args
        // 插入唯一 timeoutMs。
        .insert("input".to_owned(), json!({ "timeoutMs": timeout_ms }));
    // 提供逐操作确认。
    request.confirmed = true;
    // 返回完整请求。
    request
}

// 返回动态测试注入的取消状态。
fn test_cancelled() -> bool {
    // 以 Acquire 顺序读取取消线程发布的值。
    TEST_CANCELLED.load(Ordering::Acquire)
}

// 验证正式 facade 优雅终止工具自有子进程。
#[test]
#[ignore = "requires an explicit serial tool-owned child process fixture run"]
fn production_route_gracefully_terminates_owned_child() -> Result<(), Box<dyn std::error::Error>> {
    // 启动默认处理 WM_CLOSE 的工具自有子进程。
    let mut child = spawn_owned_child("graceful")?;
    // 等待正式 inventory 产生 opaque 目标。
    let session_id = wait_for_session(&mut child)?;
    // 构造优雅终止请求。
    let request = termination_request(
        // 传入精确子进程目标。
        &session_id,
        // 选择优雅 capability。
        capabilities::PROCESS_TERMINATE_GRACEFUL,
        // 使用五秒 deadline。
        5_000,
    );
    // 通过 launcher 使用的生产 System 入口执行。
    let result = AppControlService::new().execute(request)?;
    // 核对顶层 capability。
    assert_eq!(
        result["capability"],
        capabilities::PROCESS_TERMINATE_GRACEFUL
    );
    // 核对最终状态。
    assert_eq!(result["data"]["state"], "exited");
    // 核对没有强制回退。
    assert_eq!(result["data"]["gracefulFallbackToForce"], false);
    // 核对 facade 前景不变证据。
    assert_eq!(result["meta"]["foreground"]["unchanged"], true);
    // 子进程应由正式 Module 完成回收。
    assert!(child.child.try_wait()?.is_some());
    // 返回测试成功。
    Ok(())
}

// 验证正式 facade 强制终止忽略关闭的工具自有子进程。
#[test]
#[ignore = "requires an explicit serial tool-owned child process fixture run"]
fn production_route_force_terminates_owned_child() -> Result<(), Box<dyn std::error::Error>> {
    // 启动忽略 WM_CLOSE 的工具自有子进程。
    let mut child = spawn_owned_child("ignore-close")?;
    // 等待正式 inventory 产生 opaque 目标。
    let session_id = wait_for_session(&mut child)?;
    // 构造独立强制终止请求。
    let request = termination_request(
        // 传入精确子进程目标。
        &session_id,
        // 选择强制 capability。
        capabilities::PROCESS_TERMINATE_FORCE,
        // 使用五秒 deadline。
        5_000,
    );
    // 通过 launcher 使用的生产 System 入口执行。
    let result = AppControlService::new().execute(request)?;
    // 核对 critical 风险。
    assert_eq!(result["data"]["riskLevel"], "critical");
    // 核对固定内核机制。
    assert_eq!(result["data"]["mechanism"], "kernel-process-termination");
    // 子进程应已退出。
    assert!(child.child.try_wait()?.is_some());
    // 返回测试成功。
    Ok(())
}

// 验证优雅请求接受后的 deadline 返回 OutcomeUnknown。
#[test]
#[ignore = "requires an explicit serial tool-owned child process fixture run"]
fn accepted_graceful_timeout_is_outcome_unknown() -> Result<(), Box<dyn std::error::Error>> {
    // 启动忽略 WM_CLOSE 的工具自有子进程。
    let mut child = spawn_owned_child("ignore-close")?;
    // 等待正式 opaque 目标。
    let session_id = wait_for_session(&mut child)?;
    // 使用短 deadline 构造优雅请求。
    let request = termination_request(
        // 传入精确子进程目标。
        &session_id,
        // 选择优雅 capability。
        capabilities::PROCESS_TERMINATE_GRACEFUL,
        // 使用一百毫秒 deadline。
        100,
    );
    // 通过生产 System 取得预期错误。
    let error = AppControlService::new()
        // 执行 launcher 使用的正式路由。
        .execute(request)
        // 成功表示忽略关闭夹具失效。
        .err()
        // 使用显式 panic 保留上下文。
        .unwrap_or_else(|| panic!("ignored graceful close must not complete"));
    // 核对统一未知结果。
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    // 平台已接受固定关闭请求。
    assert_eq!(error.details["accepted"], true);
    // 禁止自动重试。
    assert_eq!(error.details["retrySafe"], false);
    // 明确没有强制回退。
    assert_eq!(error.details["gracefulFallbackToForce"], false);
    // 忽略关闭的子进程必须仍在运行，证明未升级强制终止。
    assert!(child.child.try_wait()?.is_none());
    // 返回测试成功，Drop 只清理该工具自有子进程。
    Ok(())
}

// 验证优雅请求接受后的取消返回 OutcomeUnknown。
#[test]
#[ignore = "requires an explicit serial tool-owned child process fixture run"]
fn accepted_graceful_cancellation_is_outcome_unknown() -> Result<(), Box<dyn std::error::Error>> {
    // 清空跨测试取消状态。
    TEST_CANCELLED.store(false, Ordering::Release);
    // 启动忽略 WM_CLOSE 的工具自有子进程。
    let mut child = spawn_owned_child("ignore-close")?;
    // 等待正式 opaque 目标。
    let session_id = wait_for_session(&mut child)?;
    // 启动短延迟取消线程，确保平台先接受。
    let canceller = thread::spawn(|| {
        // 等待固定一百毫秒。
        thread::sleep(Duration::from_millis(100));
        // 发布取消状态。
        TEST_CANCELLED.store(true, Ordering::Release);
    });
    // 直接调用同一生产状态机并注入测试取消探针。
    let error = perform_with_cancel_probe(
        // 提供精确子进程目标。
        Some(&session_id),
        // 提供逐操作确认。
        true,
        // 使用五秒 deadline。
        Some(&json!({ "timeoutMs": 5_000 })),
        // 选择优雅 capability 风险。
        ProcessTerminationMode::Graceful,
        // 注入原子取消探针。
        test_cancelled,
    )
    // 已取消请求不得成功。
    .err()
    // 使用显式 panic 保留上下文。
    .unwrap_or_else(|| panic!("accepted cancellation must be unknown"));
    // 等待取消线程完成。
    canceller
        // panic 转为测试错误。
        .join()
        // 不泄漏线程状态。
        .map_err(|_| "process cancellation fixture thread failed")?;
    // 恢复取消状态避免污染同进程测试。
    TEST_CANCELLED.store(false, Ordering::Release);
    // 核对统一未知结果。
    assert_eq!(error.code, "OUTCOME_UNKNOWN");
    // 核对接受事实。
    assert_eq!(error.details["accepted"], true);
    // 核对禁止自动重试。
    assert_eq!(error.details["automaticRetryProhibited"], true);
    // 接受后取消不得暗中升级强制终止。
    assert!(child.child.try_wait()?.is_none());
    // 返回测试成功，Drop 清理唯一工具自有子进程。
    Ok(())
}
