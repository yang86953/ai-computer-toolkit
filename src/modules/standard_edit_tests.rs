// 导入线程间夹具协调与有界等待工具。
use std::{
    // 导入原子停止信号。
    sync::{
        // 导入无锁停止标记。
        Arc,
        // 导入原子布尔与顺序。
        atomic::{AtomicBool, AtomicIsize, Ordering},
        // 导入有界就绪通道。
        mpsc,
    },
    // 导入独立 UI 线程。
    thread,
    // 导入短轮询与有界接收时长。
    time::Duration,
};

// 导入真实自有 Standard Edit 夹具所需固定 Windows API。
use windows::{
    // 导入自定义测试窗口过程所需句柄与消息参数。
    Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    // 导入窗口创建、消息泵和文本只读查询。
    Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        ES_AUTOHSCROLL, GWLP_WNDPROC, MSG, PM_REMOVE, PeekMessageW, SetWindowLongPtrW,
        TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_SETTEXT, WNDPROC, WS_CHILD,
        WS_OVERLAPPED, WS_VISIBLE,
    },
    // 导入编译期 UTF-16 字符串宏。
    core::w,
};

// 导入被测 Module 私有函数。
use super::*;
// 导入正式 app facade 和统一 Adapter 接口。
use crate::adapters::{AppAdapter, AppFacadeAdapter};
// 导入构造 provider-neutral 请求所需类型。
use crate::domain::{CommandRequest, Verb};

// 保存挂起夹具原始 Edit 窗口过程，仅在单个测试进程内使用。
static HUNG_EDIT_ORIGINAL_PROC: AtomicIsize = AtomicIsize::new(0);

// 仅阻塞 WM_SETTEXT 的测试窗口过程，其余消息继续交给系统 Edit。
unsafe extern "system" fn hung_edit_window_proc(
    // 接收自有 Edit 句柄。
    window: HWND,
    // 接收窗口消息。
    message: u32,
    // 接收消息 WPARAM。
    word: WPARAM,
    // 接收消息 LPARAM。
    value: LPARAM,
) -> LRESULT {
    // 仅让认证 mutation 超过测试 deadline。
    if message == WM_SETTEXT {
        // 模拟目标处理 mutation 时暂时挂起。
        thread::sleep(Duration::from_millis(500));
    }
    // 读取系统 Edit 原始窗口过程。
    let original = HUNG_EDIT_ORIGINAL_PROC.load(Ordering::Acquire);
    // 缺失原始过程时使用系统默认过程 fail safe。
    if original == 0 {
        // 调用系统默认窗口过程。
        return unsafe { DefWindowProcW(window, message, word, value) };
    }
    // 将 SetWindowLongPtrW 返回值恢复为封闭函数指针类型。
    let procedure = unsafe { std::mem::transmute::<isize, WNDPROC>(original) };
    // 将消息转交系统 Edit 原始过程。
    unsafe { CallWindowProcW(procedure, window, message, word, value) }
}

// 构造不接触系统窗口的合成控件记录。
fn control(hwnd: isize) -> WindowRecord {
    // 返回固定进程代际内的合成记录。
    WindowRecord {
        // legacy session 字段不参与 s2 匹配。
        session_id: format!("window:{hwnd}"),
        // 保存合成句柄。
        hwnd,
        // 不公开标题。
        title: String::new(),
        // 固定为认证类名。
        class_name: "Edit".to_owned(),
        // 保存合成 PID。
        process_id: 7,
        // 保存安全应用名。
        process_name: Some("fixture.exe".to_owned()),
        // 标记可见。
        visible: true,
        // 保存合成创建时间。
        process_creation_time: 9,
    }
}

// 验证零命中返回 stale。
#[test]
fn opaque_target_missing_is_stale() {
    // 构造单一候选。
    let records = vec![control(11)];
    // 查询不同 opaque ID。
    let error = match resolve_control("s2:c:0000000000000000", &records) {
        // 成功表示门禁失效。
        Ok(_) => panic!("unknown opaque target must be stale"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对错误码。
    assert_eq!(error.code, "STALE_SESSION");
}

// 验证相同 canonical 指纹多命中 fail closed。
#[test]
fn opaque_target_collision_is_ambiguous() {
    // 构造两条相同私有身份记录。
    let records = vec![control(11), control(11)];
    // 生成调用方可见 opaque ID。
    let session_id = opaque_control_session_id(&records[0]);
    // 执行唯一解析。
    let error = match resolve_control(&session_id, &records) {
        // 成功表示任取了一个碰撞目标。
        Ok(_) => panic!("opaque collision must fail closed"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对歧义错误。
    assert_eq!(error.code, "AMBIGUOUS_TARGET");
}

// 验证 provider-neutral 缺省 timeout 为 2000ms。
#[test]
fn provider_input_uses_contract_default_timeout() -> AppResult<()> {
    // 解析最小输入。
    let input = json!({ "text": "hello" });
    // 解析具名输入。
    let (text, timeout) = provider_input(&input)?;
    // 核对文本。
    assert_eq!(text, "hello");
    // 核对默认 deadline。
    assert_eq!(timeout, DEFAULT_TIMEOUT_MS);
    // 返回成功。
    Ok(())
}

// 验证 UTF-8 上限按字节而不是字符计数。
#[test]
fn provider_input_limits_utf8_bytes() {
    // 构造超过 65536 bytes 的多字节文本。
    let text = "界".repeat(21_846);
    // 执行纯输入验证。
    let input = json!({ "text": text });
    // 执行纯输入验证。
    let error = match provider_input(&input) {
        // 意外成功使测试失败。
        Ok(_) => panic!("oversized UTF-8 input must fail"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对参数错误。
    assert_eq!(error.code, "INVALID_ARGUMENT");
}

// 验证 confirmation-first 先于 mutation 输入边界。
#[test]
fn mutation_validation_requires_confirmation_first() {
    // 同时提供超长文本和缺失确认。
    let error = match validate_mutation_input(
        // 提供超长文本。
        &"x".repeat(MAXIMUM_UTF8_BYTES + 1),
        // 缺失确认。
        false,
        // 提供无效 deadline。
        0,
    ) {
        // 意外成功使测试失败。
        Ok(()) => panic!("missing confirmation must fail"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 确认错误必须优先。
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

// 验证成功 session 不泄漏 native 字段。
#[test]
fn public_session_hides_native_identifiers() {
    // 构造安全 session JSON。
    let value = session_json(&control(42), None);
    // 序列化以检查递归字段。
    let serialized = value.to_string();
    // 不得公开句柄字段。
    assert!(!serialized.contains("hwnd"));
    // 不得公开进程 ID 字段。
    assert!(!serialized.contains("processId"));
    // 不得公开类名字段。
    assert!(!serialized.contains("className"));
    // 必须返回 canonical 控件目标。
    assert!(
        value["sessionId"]
            // 读取字符串。
            .as_str()
            // 检查 s2:c 前缀。
            .is_some_and(|session| session.starts_with("s2:c:"))
    );
}

// 验证 provider-neutral stale 控件由 Standard Edit Module 返回稳定错误。
#[test]
fn facade_preserves_standard_edit_stale_error() {
    // 构造不存在的 canonical 控件目标。
    let mut request = CommandRequest::read(Verb::Run, "app");
    // 选择统一 apply 动词。
    request.operation = Some("apply".to_owned());
    // 绑定不会命中当前控件清单的测试 opaque ID。
    request.target.insert(
        // 使用固定 sessionId 字段。
        "sessionId".to_owned(),
        // 提供 canonical 控件形状。
        json!("s2:c:0000000000000000"),
    );
    // 声明固定 Standard Edit capability。
    request.args.insert(
        // 使用固定 capability 字段。
        "capability".to_owned(),
        // 使用版本化 ID。
        json!(capabilities::UI_TEXT_INPUT),
    );
    // 提供有效 provider-neutral input。
    request.args.insert(
        // 使用固定 input 字段。
        "input".to_owned(),
        // 文本不会发送到任何目标。
        json!({ "text": "must-not-write" }),
    );
    // 提供逐操作确认。
    request.confirmed = true;
    // 执行正式 facade 并保存预期错误。
    let error = match AppFacadeAdapter::new().run(&request) {
        // 成功表示 stale 门禁失效。
        Ok(_) => panic!("stale Standard Edit must fail"),
        // 保存结构化错误。
        Err(error) => error,
    };
    // 核对契约 stale 错误码。
    assert_eq!(error.code, "STALE_SESSION");
}

// 验证真实自有 Edit 控件通过固定消息写入、回读且不改变前景。
#[test]
fn real_owned_edit_round_trips_without_foreground_change() -> AppResult<()> {
    // 创建夹具就绪通道。
    let (ready_tx, ready_rx) = mpsc::channel::<Result<isize, String>>();
    // 创建跨线程停止标记。
    let running = Arc::new(AtomicBool::new(true));
    // 克隆停止标记给 UI 线程。
    let thread_running = Arc::clone(&running);
    // 启动独立 UI 消息泵。
    let fixture_thread = thread::spawn(move || {
        // 创建隐藏顶层 STATIC 窗口作为自有父目标。
        let parent = unsafe {
            // 调用固定系统类，不注册自定义窗口过程。
            CreateWindowExW(
                // 不使用扩展激活样式。
                WINDOW_EX_STYLE(0),
                // 使用系统 STATIC 类。
                w!("STATIC"),
                // 使用测试专属标题。
                w!("Rust Standard Edit Fixture"),
                // 顶层窗口保持隐藏。
                WS_OVERLAPPED,
                // 使用默认横坐标。
                0,
                // 使用默认纵坐标。
                0,
                // 提供有界宽度。
                320,
                // 提供有界高度。
                80,
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
        let parent = match parent {
            // 保存有效窗口。
            Ok(parent) => parent,
            // 发送结构化测试错误。
            Err(error) => {
                // 忽略接收端提前退出。
                let _ = ready_tx.send(Err(error.to_string()));
                // 结束 UI 线程。
                return;
            }
        };
        // 组合 child、visible 与 Edit 自动横向滚动样式。
        let edit_style = WINDOW_STYLE(
            // 合并固定窗口样式位。
            WS_CHILD.0 | WS_VISIBLE.0 | u32::try_from(ES_AUTOHSCROLL).unwrap_or_default(),
        );
        // 创建系统标准 Edit 子控件。
        let edit = unsafe {
            // 调用固定系统类。
            CreateWindowExW(
                // 不使用扩展样式。
                WINDOW_EX_STYLE(0),
                // 使用精确系统 Edit 类。
                w!("Edit"),
                // 初始文本为空。
                w!(""),
                // 使用固定 child 样式。
                edit_style,
                // 设置子控件横坐标。
                0,
                // 设置子控件纵坐标。
                0,
                // 设置子控件宽度。
                300,
                // 设置子控件高度。
                30,
                // 绑定自有父窗口。
                Some(parent),
                // 不提供菜单或控件 ID。
                None,
                // 使用当前模块实例。
                None,
                // 不传自定义指针。
                None,
            )
        };
        // 创建失败时清理父窗口并通知主测试。
        let edit = match edit {
            // 保存有效 Edit。
            Ok(edit) => edit,
            // 处理创建错误。
            Err(error) => {
                // 只销毁本测试创建的父窗口。
                let _ = unsafe { DestroyWindow(parent) };
                // 忽略接收端提前退出。
                let _ = ready_tx.send(Err(error.to_string()));
                // 结束 UI 线程。
                return;
            }
        };
        // 通知主测试真实 Edit 句柄，仅限进程内测试通道。
        let _ = ready_tx.send(Ok(edit.0 as isize));
        // 初始化本线程消息结构。
        let mut message = MSG::default();
        // 在主测试完成前持续处理同步消息。
        while thread_running.load(Ordering::Acquire) {
            // 排空当前线程队列。
            while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() } {
                // 翻译键盘消息；夹具不会主动输入。
                let _ = unsafe { TranslateMessage(&message) };
                // 分派窗口消息。
                unsafe { DispatchMessageW(&message) };
            }
            // 短暂让出 CPU，保持 2s deadline 内可响应。
            thread::sleep(Duration::from_millis(5));
        }
        // 只销毁本测试创建的子控件。
        let _ = unsafe { DestroyWindow(edit) };
        // 只销毁本测试创建的父窗口。
        let _ = unsafe { DestroyWindow(parent) };
    });
    // 在有界时间内等待夹具就绪。
    let edit_handle = match ready_rx.recv_timeout(Duration::from_secs(5)) {
        // 取得有效句柄值。
        Ok(Ok(handle)) => handle,
        // 将窗口创建错误映射为测试领域错误。
        Ok(Err(message)) => {
            // 停止 UI 线程。
            running.store(false, Ordering::Release);
            // 等待线程回收。
            let _ = fixture_thread.join();
            // 返回结构化失败。
            return Err(AppControlError::new("FIXTURE_UNAVAILABLE", message));
        }
        // 将就绪超时映射为测试领域错误。
        Err(_) => {
            // 停止 UI 线程。
            running.store(false, Ordering::Release);
            // 等待线程回收。
            let _ = fixture_thread.join();
            // 返回结构化失败。
            return Err(AppControlError::new(
                // 使用测试夹具错误码。
                "FIXTURE_UNAVAILABLE",
                // 说明有界就绪失败。
                "The owned Standard Edit fixture did not become ready.",
            ));
        }
    };
    // 保存写前前景 token。
    let foreground_before = foreground_hwnd();
    // 在闭包内执行所有可能失败的领域步骤，以保证统一清理。
    let operation = (|| -> AppResult<(Value, Value, String, usize, usize)> {
        // 重新发现全部标准 Edit。
        let controls = enumerate_controls()?;
        // 找到本测试进程创建的唯一控件。
        let owned = controls
            // 遍历当前记录。
            .iter()
            // 仅比较进程内已知自有句柄。
            .find(|control| control.hwnd == edit_handle)
            // 缺失表示发现链失败。
            .ok_or_else(|| {
                // 返回结构化夹具失败。
                AppControlError::new(
                    // 使用测试夹具错误码。
                    "FIXTURE_UNAVAILABLE",
                    // 不公开句柄。
                    "The owned Standard Edit fixture was not discovered.",
                )
            })?;
        // 从实时私有事实生成 canonical s2:c。
        let session_id = opaque_control_session_id(owned);
        // 使用多字节 UTF-8 内容执行正式 Module。
        let expected = "Rust Module UTF-8 ✓";
        // 执行 confirmation-first 固定 mutation。
        let result = set_text(
            // 传入 opaque 目标。
            &session_id,
            // 传入测试自有文本。
            expected,
            // 提供逐操作确认。
            true,
            // 使用契约默认 deadline。
            DEFAULT_TIMEOUT_MS,
        )?;
        // 构造 provider-neutral app.apply 请求。
        let mut request = CommandRequest::read(Verb::Run, "app");
        // 选择统一 apply 动词。
        request.operation = Some("apply".to_owned());
        // 绑定同一 opaque 自有控件。
        request
            // 访问目标对象。
            .target
            // 插入固定 sessionId。
            .insert("sessionId".to_owned(), json!(session_id));
        // 声明 Standard Edit capability。
        request.args.insert(
            // 使用固定 capability 字段。
            "capability".to_owned(),
            // 传入版本化 ID。
            json!(capabilities::UI_TEXT_INPUT),
        );
        // 使用第二段多字节文本验证 facade 路线。
        let facade_expected = "Rust Facade UTF-8 ✓";
        // 写入 provider-neutral input。
        request.args.insert(
            // 使用固定 input 字段。
            "input".to_owned(),
            // 提供文本与默认 timeout。
            json!({ "text": facade_expected }),
        );
        // 提供逐操作确认。
        request.confirmed = true;
        // 通过正式 app facade 再次写入自有控件。
        let facade_result = AppFacadeAdapter::new().run(&request)?;
        // 返回两层验证所需安全事实。
        Ok((
            // 返回 Module 结果。
            result,
            // 返回 facade 结果。
            facade_result,
            // 返回 opaque 目标。
            session_id,
            // 返回 Module 文本字节数。
            expected.len(),
            // 返回 facade 文本字节数。
            facade_expected.len(),
        ))
    })();
    // 无论 Module 结果如何都停止夹具线程。
    running.store(false, Ordering::Release);
    // 等待自有线程清理窗口。
    let joined = fixture_thread.join();
    // UI 线程 panic 必须使测试失败。
    if joined.is_err() {
        // 返回结构化夹具失败。
        return Err(AppControlError::new(
            // 使用测试夹具错误码。
            "FIXTURE_UNAVAILABLE",
            // 说明线程异常。
            "The owned Standard Edit fixture thread failed.",
        ));
    }
    // 传播正式 Module 错误并取得安全验证事实。
    let (result, facade_result, session_id, expected_bytes, facade_expected_bytes) = operation?;
    // 核对回读验证。
    assert_eq!(result["verifiedByReadback"], true);
    // 核对只回显 opaque 目标。
    assert_eq!(result["sessionId"], session_id);
    // 核对 UTF-8 字节计数。
    assert_eq!(result["textBytes"], expected_bytes);
    // 核对 facade 顶层 capability。
    assert_eq!(facade_result["capability"], capabilities::UI_TEXT_INPUT);
    // 核对 facade 回显原 opaque 目标。
    assert_eq!(facade_result["targetId"], session_id);
    // 核对 facade 固定兼容形状。
    assert_eq!(
        facade_result["compatibilityShape"],
        "provider-neutral-standard-edit-v1"
    );
    // 核对 facade 回读证据。
    assert_eq!(facade_result["data"]["verifiedByReadback"], true);
    // 核对 facade 文本字节数。
    assert_eq!(facade_result["data"]["textBytes"], facade_expected_bytes);
    // 核对 facade 不公开 native 字段。
    let facade_serialized = facade_result.to_string();
    // 不得公开 HWND。
    assert!(!facade_serialized.contains("hwnd"));
    // 不得公开 PID。
    assert!(!facade_serialized.contains("processId"));
    // 不得公开 class。
    assert!(!facade_serialized.contains("className"));
    // 核对前景保持不变。
    assert_eq!(foreground_before, foreground_hwnd());
    // 返回成功。
    Ok(())
}

// 验证真实自有挂起 Edit 返回 outcome unknown 且禁止自动重试。
#[test]
fn real_owned_hung_edit_reports_unknown_timeout() -> AppResult<()> {
    // 创建夹具就绪通道。
    let (ready_tx, ready_rx) = mpsc::channel::<Result<isize, String>>();
    // 创建跨线程停止标记。
    let running = Arc::new(AtomicBool::new(true));
    // 克隆停止标记给 UI 线程。
    let thread_running = Arc::clone(&running);
    // 启动只在 WM_SETTEXT 内暂时阻塞的独立 UI 线程。
    let fixture_thread = thread::spawn(move || {
        // 创建隐藏顶层父窗口。
        let parent = unsafe {
            // 使用系统 STATIC 类避免自定义窗口过程。
            CreateWindowExW(
                // 不使用扩展样式。
                WINDOW_EX_STYLE(0),
                // 使用系统 STATIC 类。
                w!("STATIC"),
                // 使用测试专属标题。
                w!("Rust Hung Standard Edit Fixture"),
                // 保持顶层窗口隐藏。
                WS_OVERLAPPED,
                // 使用默认横坐标。
                0,
                // 使用默认纵坐标。
                0,
                // 设置有界宽度。
                320,
                // 设置有界高度。
                80,
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
        // 创建失败时通知主测试。
        let parent = match parent {
            // 保存有效窗口。
            Ok(parent) => parent,
            // 处理创建错误。
            Err(error) => {
                // 忽略接收端提前退出。
                let _ = ready_tx.send(Err(error.to_string()));
                // 结束夹具线程。
                return;
            }
        };
        // 组合固定 Edit child 样式。
        let edit_style = WINDOW_STYLE(
            // 合并 child、visible 与自动滚动位。
            WS_CHILD.0 | WS_VISIBLE.0 | u32::try_from(ES_AUTOHSCROLL).unwrap_or_default(),
        );
        // 创建系统标准 Edit 子控件。
        let edit = unsafe {
            // 调用固定系统类。
            CreateWindowExW(
                // 不使用扩展样式。
                WINDOW_EX_STYLE(0),
                // 使用精确系统 Edit 类。
                w!("Edit"),
                // 初始文本为空。
                w!(""),
                // 使用固定 child 样式。
                edit_style,
                // 设置横坐标。
                0,
                // 设置纵坐标。
                0,
                // 设置有界宽度。
                300,
                // 设置有界高度。
                30,
                // 绑定自有父窗口。
                Some(parent),
                // 不提供菜单或控件 ID。
                None,
                // 使用当前模块实例。
                None,
                // 不传自定义指针。
                None,
            )
        };
        // 创建失败时清理父窗口并通知主测试。
        let edit = match edit {
            // 保存有效控件。
            Ok(edit) => edit,
            // 处理创建错误。
            Err(error) => {
                // 只销毁本测试父窗口。
                let _ = unsafe { DestroyWindow(parent) };
                // 忽略接收端提前退出。
                let _ = ready_tx.send(Err(error.to_string()));
                // 结束夹具线程。
                return;
            }
        };
        // 将系统 Edit 子类化为仅阻塞 WM_SETTEXT 的测试过程。
        let original = unsafe {
            // 替换窗口过程并取得原始函数指针。
            SetWindowLongPtrW(
                // 传入自有 Edit。
                edit,
                // 只替换窗口过程槽位。
                GWLP_WNDPROC,
                // 传入测试窗口过程地址。
                hung_edit_window_proc as *const () as usize as isize,
            )
        };
        // 保存原始系统 Edit 窗口过程。
        HUNG_EDIT_ORIGINAL_PROC.store(original, Ordering::Release);
        // 通知主测试控件已创建。
        let _ = ready_tx.send(Ok(edit.0 as isize));
        // 初始化本线程消息结构。
        let mut message = MSG::default();
        // 持续处理除被测试过程暂时阻塞外的所有消息。
        while thread_running.load(Ordering::Acquire) {
            // 排空当前线程队列。
            while unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() } {
                // 翻译键盘消息；夹具不会主动输入。
                let _ = unsafe { TranslateMessage(&message) };
                // 分派窗口消息；WM_SETTEXT 会在测试过程内阻塞 500ms。
                unsafe { DispatchMessageW(&message) };
            }
            // 短暂让出 CPU。
            thread::sleep(Duration::from_millis(5));
        }
        // 恢复系统 Edit 原始窗口过程。
        let _ = unsafe { SetWindowLongPtrW(edit, GWLP_WNDPROC, original) };
        // 清空测试全局函数指针。
        HUNG_EDIT_ORIGINAL_PROC.store(0, Ordering::Release);
        // 只销毁本测试创建的子控件。
        let _ = unsafe { DestroyWindow(edit) };
        // 只销毁本测试创建的父窗口。
        let _ = unsafe { DestroyWindow(parent) };
    });
    // 在有界时间内等待夹具就绪。
    let edit_handle = match ready_rx.recv_timeout(Duration::from_secs(5)) {
        // 取得有效句柄值。
        Ok(Ok(handle)) => handle,
        // 传播窗口创建错误。
        Ok(Err(message)) => {
            // 停止夹具线程。
            running.store(false, Ordering::Release);
            // 等待夹具线程退出。
            let _ = fixture_thread.join();
            // 返回结构化夹具错误。
            return Err(AppControlError::new("FIXTURE_UNAVAILABLE", message));
        }
        // 处理就绪超时。
        Err(_) => {
            // 停止夹具线程。
            running.store(false, Ordering::Release);
            // 等待夹具线程退出。
            let _ = fixture_thread.join();
            // 返回结构化夹具错误。
            return Err(AppControlError::new(
                // 使用测试夹具错误码。
                "FIXTURE_UNAVAILABLE",
                // 说明有界就绪失败。
                "The owned hung Standard Edit fixture did not become ready.",
            ));
        }
    };
    // 保存 timeout 测试前景。
    let foreground_before = foreground_hwnd();
    // 在闭包内执行所有可能失败的领域步骤。
    let operation = (|| -> AppResult<Value> {
        // 重新发现全部标准 Edit。
        let controls = enumerate_controls()?;
        // 找到本测试拥有的挂起控件。
        let owned = controls
            // 遍历当前记录。
            .iter()
            // 仅匹配进程内已知自有句柄。
            .find(|control| control.hwnd == edit_handle)
            // 缺失表示发现链失败。
            .ok_or_else(|| {
                // 返回结构化夹具错误。
                AppControlError::new(
                    // 使用测试夹具错误码。
                    "FIXTURE_UNAVAILABLE",
                    // 不公开句柄。
                    "The owned hung Standard Edit fixture was not discovered.",
                )
            })?;
        // 从实时私有事实生成 canonical s2:c。
        let session_id = opaque_control_session_id(owned);
        // 执行短 deadline 固定 mutation 并保留预期错误。
        match set_text(
            // 传入 opaque 自有目标。
            &session_id,
            // 传入测试文本。
            "must-time-out",
            // 提供逐操作确认。
            true,
            // 使用短但合法 deadline。
            50,
        ) {
            // 成功表示挂起门禁失效。
            Ok(_) => Err(AppControlError::new(
                // 使用测试失败码。
                "FIXTURE_FAILED",
                // 说明意外成功。
                "The hung Standard Edit fixture unexpectedly accepted the message.",
            )),
            // 返回预期领域错误供测试核对。
            Err(error) => Ok(json!({
                // 保存错误码。
                "code": error.code,
                // 保存安全 details。
                "details": error.details,
            })),
        }
    })();
    // 无论测试结果如何都停止夹具线程。
    running.store(false, Ordering::Release);
    // 等待自有窗口清理。
    let joined = fixture_thread.join();
    // UI 线程 panic 必须使测试失败。
    if joined.is_err() {
        // 返回结构化夹具错误。
        return Err(AppControlError::new(
            // 使用测试夹具错误码。
            "FIXTURE_UNAVAILABLE",
            // 说明线程异常。
            "The owned hung Standard Edit fixture thread failed.",
        ));
    }
    // 传播发现或执行包装错误。
    let error = operation?;
    // 核对正式 timeout 错误码。
    assert_eq!(error["code"], "TIMEOUT");
    // 核对结果未知。
    assert_eq!(error["details"]["outcome"], "unknown");
    // 核对禁止自动重试。
    assert_eq!(error["details"]["retrySafe"], false);
    // 核对目标可能已经变更。
    assert_eq!(error["details"]["targetMayHaveMutated"], true);
    // 核对稳定原因。
    assert_eq!(
        error["details"]["reason"],
        "synchronous-window-message-timeout"
    );
    // 核对 timeout 测试未改变前景。
    assert_eq!(foreground_before, foreground_hwnd());
    // 返回成功。
    Ok(())
}
