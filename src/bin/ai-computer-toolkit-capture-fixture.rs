#[cfg(not(target_os = "windows"))]
fn main() {
    // Windows 专用 fixture 在 Linux 上只返回结构化能力缺口。
    println!(
        "{}",
        r#"{"ok":false,"error":{"code":"CAPABILITY_UNAVAILABLE","message":"This fixture requires a certified Windows provider.","details":{"platform":"linux","executionRealm":"none","fallback":"none"}}}"#
    );
    std::process::exit(2);
}

#[cfg(target_os = "windows")]
mod windows_fixture {
    //! 首帧探针端到端门禁专用的 no-activate 自有窗口。

    // 导入固定控制通道、线程与有界轮询工具。
    use std::{
        // 只读取一次固定 transition 命令并写入固定完成标记。
        io::{BufRead, Write},
        // 在线程间传递一次性状态转换请求。
        sync::{
            // 保存自绘窗口的原始系统窗口过程。
            atomic::{AtomicIsize, Ordering},
            // 在线程间传递一次性状态转换请求。
            mpsc::{self, Receiver, TryRecvError},
        },
        // 独立读取控制输入并短暂让出消息泵。
        thread,
        // 限制空消息泵轮询频率。
        time::Duration,
    };

    // 导入固定 Win32 窗口创建与消息泵接口。
    use windows::{
        // 导入窗口句柄、绘制颜色、消息参数与矩形。
        Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        // 导入自绘夹具使用的窄 GDI 绘制接口。
        Win32::Graphics::Gdi::{
            // 开始处理 WM_PAINT。
            BeginPaint,
            // 创建固定纯色画刷。
            CreateSolidBrush,
            // 释放当前绘制创建的画刷。
            DeleteObject,
            // 完成 WM_PAINT。
            EndPaint,
            // 填充固定自绘区域。
            FillRect,
            // 保存删除画刷所需的通用 GDI 句柄。
            HGDIOBJ,
            // 保存窗口绘制状态。
            PAINTSTRUCT,
        },
        // 导入系统 STATIC 窗口与无激活显示接口。
        Win32::UI::WindowsAndMessaging::{
            // 使用默认窗口位置。
            CW_USEDEFAULT,
            // 调用系统 STATIC 原始窗口过程。
            CallWindowProcW,
            // 创建系统类窗口。
            CreateWindowExW,
            // 未取得原始过程时使用系统默认行为。
            DefWindowProcW,
            // 仅在创建线程销毁工具自有窗口。
            DestroyWindow,
            // 分发消息。
            DispatchMessageW,
            // 访问窗口过程槽位。
            GWLP_WNDPROC,
            // 只读核对工具自有窗口仍然存在。
            IsWindow,
            // 保存消息。
            MSG,
            // 读取当前线程待处理消息。
            PM_REMOVE,
            // 非阻塞读取消息，允许轮询封闭控制通道。
            PeekMessageW,
            // 隐藏窗口且不激活其他目标。
            SW_HIDE,
            // 以最小化且不激活方式显示窗口。
            SW_SHOWMINNOACTIVE,
            // 显示窗口且不激活。
            SW_SHOWNOACTIVATE,
            // 安装自绘窗口过程。
            SetWindowLongPtrW,
            // 显示窗口。
            ShowWindow,
            // 翻译键盘消息。
            TranslateMessage,
            // 自绘窗口重绘消息。
            WM_PAINT,
            // 保存强类型窗口过程。
            WNDPROC,
            // 创建工具自有标准子控件。
            WS_CHILD,
            // 禁止激活窗口。
            WS_EX_NOACTIVATE,
            // 避免进入普通应用切换面板。
            WS_EX_TOOLWINDOW,
            // 使用标准顶层窗口样式。
            WS_OVERLAPPEDWINDOW,
            // 创建后立即可见。
            WS_VISIBLE,
        },
        // 导入宽字符串指针与静态宽字符串宏。
        core::{PCWSTR, w},
    };

    // 保存自绘夹具替换前的系统 STATIC 窗口过程。
    static CUSTOM_DRAWN_ORIGINAL_PROC: AtomicIsize = AtomicIsize::new(0);

    // 固定夹具允许的五种封闭状态模式。
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FixtureMode {
        // 创建普通 no-activate 可见窗口。
        Visible,
        // 创建 no-activate 最小化窗口。
        Minimized,
        // 收到固定命令后隐藏当前窗口。
        HideOnCommand,
        // 收到固定命令后替换当前 fixture 窗口实例。
        RecreateOnCommand,
        // 创建无标准子控件的自绘窗口。
        CustomDrawn,
    }

    // 验证 fixture 标题仅来自测试固定前缀与 ASCII 后缀。
    fn fixture_title() -> Option<String> {
        // 读取唯一标题参数。
        let title = std::env::args().nth(1)?;
        // 限制前缀、ASCII 与长度，避免 fixture 成为通用窗口工具。
        if !title.starts_with("act-rust-capture-fixture-")
        // 要求全部 ASCII。
        || !title.is_ascii()
        // 限制标题长度。
        || title.len() > 96
        {
            // 非测试标题必须拒绝。
            return None;
        }
        // 返回已验证标题供父窗口与子控件派生使用。
        Some(title)
    }

    // 验证可选封闭模式参数。
    fn fixture_mode() -> Option<FixtureMode> {
        // 收集固定小型参数集合。
        let arguments = std::env::args().skip(2).collect::<Vec<_>>();
        // 只接受无模式或单一固定模式。
        match arguments.as_slice() {
            // 默认保持普通 no-activate 窗口。
            [] => Some(FixtureMode::Visible),
            // 固定参数仅供最小化位置契约门禁。
            [mode] if mode == "--minimized" => Some(FixtureMode::Minimized),
            // 固定参数仅供隐藏状态动态矩阵。
            [mode] if mode == "--hide-on-command" => Some(FixtureMode::HideOnCommand),
            // 固定参数仅供窗口重建动态矩阵。
            [mode] if mode == "--recreate-on-command" => Some(FixtureMode::RecreateOnCommand),
            // 固定参数仅供自绘窗口动态矩阵。
            [mode] if mode == "--custom-drawn" => Some(FixtureMode::CustomDrawn),
            // 任何其他参数都拒绝，避免 fixture 成为通用窗口工具。
            _ => None,
        }
    }

    // 为动态模式启动只接受一次固定命令的控制线程。
    fn transition_receiver(mode: FixtureMode) -> Option<Receiver<()>> {
        // 静态与最小化模式不读取控制输入。
        if matches!(
            mode,
            // 静态、自绘与最小化模式都没有控制状态转换。
            FixtureMode::Visible | FixtureMode::Minimized | FixtureMode::CustomDrawn
        ) {
            // 返回无控制通道。
            return None;
        }
        // 创建一次性进程内控制通道。
        let (sender, receiver) = mpsc::channel();
        // 独立阻塞读取 stdin，避免阻塞窗口消息泵。
        thread::spawn(move || {
            // 锁定当前进程标准输入。
            let input = std::io::stdin();
            // 建立可逐行读取的句柄。
            let mut input = input.lock();
            // 为唯一允许命令分配有界文本缓冲区。
            let mut command = String::new();
            // 只接受逐字 transition 并忽略关闭或其他输入。
            if input.read_line(&mut command).is_ok() && command.trim() == "transition" {
                // 接收端退出时无需延长夹具生命周期。
                let _ = sender.send(());
            }
        });
        // 返回主消息泵拥有的接收端。
        Some(receiver)
    }

    // 调用自绘窗口替换前的系统 STATIC 窗口过程。
    unsafe fn call_custom_drawn_original(
        // 接收工具自有窗口句柄。
        window: HWND,
        // 接收窗口消息。
        message: u32,
        // 接收消息字参数。
        word: WPARAM,
        // 接收消息长参数。
        value: LPARAM,
    ) -> LRESULT {
        // 读取安装自绘过程时保存的原始过程。
        let original = CUSTOM_DRAWN_ORIGINAL_PROC.load(Ordering::Acquire);
        // 缺失原始过程时使用系统默认过程失败安全。
        if original == 0 {
            // 不猜测系统 STATIC 私有状态。
            return unsafe { DefWindowProcW(window, message, word, value) };
        }
        // 将系统返回值恢复为强类型窗口过程。
        let procedure = unsafe { std::mem::transmute::<isize, WNDPROC>(original) };
        // 保留所有非绘制消息的系统窗口行为。
        unsafe { CallWindowProcW(procedure, window, message, word, value) }
    }

    // 绘制固定像素块但不创建任何标准子控件或语义节点。
    unsafe extern "system" fn custom_drawn_window_proc(
        // 接收工具自有窗口句柄。
        window: HWND,
        // 接收窗口消息。
        message: u32,
        // 接收消息字参数。
        word: WPARAM,
        // 接收消息长参数。
        value: LPARAM,
    ) -> LRESULT {
        // 只接管自绘消息，其他行为仍由系统 STATIC 过程拥有。
        if message == WM_PAINT {
            // 初始化当前绘制状态。
            let mut paint = PAINTSTRUCT::default();
            // 开始当前工具自有窗口绘制。
            let context = unsafe { BeginPaint(window, &mut paint) };
            // 创建浅色背景画刷。
            let background = unsafe { CreateSolidBrush(COLORREF(0x00f5f5f5)) };
            // 填充系统声明的无效区域。
            unsafe {
                // 自绘完整待刷新背景。
                FillRect(context, &paint.rcPaint, background);
            }
            // 删除当前消息创建的背景画刷。
            unsafe {
                // 转换为通用 GDI 对象后释放。
                let _ = DeleteObject(HGDIOBJ(background.0));
            }
            // 固定自绘标记位于 client 区域内部。
            let marker = RECT {
                // 固定左边界。
                left: 32,
                // 固定上边界。
                top: 32,
                // 固定右边界。
                right: 224,
                // 固定下边界。
                bottom: 128,
            };
            // 创建唯一蓝色标记画刷，RGB 为 31、127、223。
            let marker_brush = unsafe { CreateSolidBrush(COLORREF(0x00df7f1f)) };
            // 绘制不对应任何标准子窗口的像素块。
            unsafe {
                // 填充固定自绘标记。
                FillRect(context, &marker, marker_brush);
            }
            // 删除当前消息创建的标记画刷。
            unsafe {
                // 转换为通用 GDI 对象后释放。
                let _ = DeleteObject(HGDIOBJ(marker_brush.0));
            }
            // 完成本次窗口绘制并验证无效区域。
            unsafe {
                // 结束当前 WM_PAINT 生命周期。
                let _ = EndPaint(window, &paint);
            }
            // 自绘消息已完整处理。
            return LRESULT(0);
        }
        // 非绘制消息沿用系统 STATIC 行为。
        unsafe { call_custom_drawn_original(window, message, word, value) }
    }

    // 向测试 harness 写入不含原生目标的固定完成标记。
    fn acknowledge_transition() -> windows::core::Result<()> {
        // 锁定标准输出以保证单行原子写入。
        let output = std::io::stdout();
        // 建立可刷新输出句柄。
        let mut output = output.lock();
        // 写入固定完成标记，不泄漏窗口或进程身份。
        writeln!(output, "transition-complete").map_err(|_| windows::core::Error::from_thread())?;
        // 立即刷新，解除父测试的有界等待。
        output
            // 执行显式刷新。
            .flush()
            // 收敛为固定 Windows 错误类型。
            .map_err(|_| windows::core::Error::from_thread())
    }

    // 创建 no-activate 自有窗口并运行消息泵。
    fn run() -> windows::core::Result<()> {
        // 验证并编码唯一标题。
        let title = fixture_title().ok_or_else(windows::core::Error::from_thread)?;
        // 编码父窗口标题并追加终止零。
        let encoded_title = title
            // 编码为 Win32 宽字符。
            .encode_utf16()
            // 追加终止零。
            .chain(std::iter::once(0))
            // 固定拥有型缓冲区生命周期。
            .collect::<Vec<_>>();
        // 派生逐窗口唯一的标准按钮名称。
        let button_title = format!("{title}-button");
        // 编码按钮名称并追加终止零。
        let encoded_button_title = button_title
            // 编码为 Win32 宽字符。
            .encode_utf16()
            // 追加终止零。
            .chain(std::iter::once(0))
            // 固定拥有型缓冲区生命周期。
            .collect::<Vec<_>>();
        // 验证固定夹具模式。
        let mode = fixture_mode().ok_or_else(windows::core::Error::from_thread)?;
        // 使用系统 STATIC 类创建固定尺寸窗口。
        let window = unsafe {
            // 创建不激活且不进入任务切换面板的可见窗口。
            CreateWindowExW(
                // 组合无激活扩展样式。
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                // 使用系统 STATIC 类。
                w!("STATIC"),
                // 传入已验证标题。
                PCWSTR(encoded_title.as_ptr()),
                // 使用标准可捕获顶层窗口样式。
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                // 让系统选择横坐标。
                CW_USEDEFAULT,
                // 让系统选择纵坐标。
                CW_USEDEFAULT,
                // 固定 fixture 宽度。
                320,
                // 固定 fixture 高度。
                200,
                // 不设置父窗口。
                None,
                // 不设置菜单。
                None,
                // 系统 STATIC 类不需要自定义模块句柄。
                None,
                // 不传递创建参数。
                None,
            )?
        };
        // 自绘模式禁止创建可被 UIA 暴露为标准控件的子窗口。
        let _button = if mode == FixtureMode::CustomDrawn {
            // 显式保留无标准子控件事实。
            None
        } else {
            // 其余既有模式保留标准按钮夹具。
            Some(unsafe {
                // 创建可见标准 BUTTON 子窗口。
                CreateWindowExW(
                    // 子控件不需要额外扩展样式。
                    Default::default(),
                    // 使用系统 BUTTON 类而非自定义控件。
                    w!("BUTTON"),
                    // 使用逐窗口唯一名称供语义 selector 定位。
                    PCWSTR(encoded_button_title.as_ptr()),
                    // 创建可见子控件且不扩展顶层行为。
                    WS_CHILD | WS_VISIBLE,
                    // 固定相对横坐标。
                    24,
                    // 固定相对纵坐标。
                    24,
                    // 固定按钮宽度。
                    220,
                    // 固定按钮高度。
                    48,
                    // 绑定工具自有父窗口。
                    Some(window),
                    // 不设置菜单或控件 ID。
                    None,
                    // 系统 BUTTON 类不需要自定义模块句柄。
                    None,
                    // 不传递创建参数。
                    None,
                )?
            })
        };
        // 自绘模式在显示前安装封闭窗口过程。
        if mode == FixtureMode::CustomDrawn {
            // 替换工具自有系统 STATIC 过程并保存原始过程。
            let original = unsafe {
                // 仅修改当前夹具拥有的窗口。
                SetWindowLongPtrW(
                    // 绑定工具自有窗口。
                    window,
                    // 替换窗口过程槽位。
                    GWLP_WNDPROC,
                    // 先转为代码指针再转成 Win32 整数槽位。
                    custom_drawn_window_proc as *const () as usize as isize,
                )
            };
            // 零值表示无法建立可恢复的自绘过程。
            if original == 0 {
                // 销毁刚创建的工具自有窗口。
                unsafe {
                    // 不保留半初始化夹具。
                    DestroyWindow(window)?;
                }
                // 返回固定启动失败。
                return Err(windows::core::Error::from_thread());
            }
            // 发布原始系统过程供自绘过程转发非绘制消息。
            CUSTOM_DRAWN_ORIGINAL_PROC.store(original, Ordering::Release);
        }
        // 明确使用无激活显示模式。
        unsafe {
            // 按固定模式显示工具自有窗口且不抢焦点。
            let _ = ShowWindow(
                // 传入自有窗口。
                window,
                // 最小化用例与普通用例使用封闭分支。
                if mode == FixtureMode::Minimized {
                    // 最小化且不激活。
                    SW_SHOWMINNOACTIVE
                } else {
                    // 普通显示且不激活。
                    SW_SHOWNOACTIVATE
                },
            );
        }
        // 动态模式建立一次性固定控制通道。
        let transition = transition_receiver(mode);
        // 保存当前仍由消息泵拥有的顶层窗口。
        let mut current_window = window;
        // 标记动态转换最多执行一次。
        let mut transitioned = false;
        // 初始化消息对象。
        let mut message = MSG::default();
        // 运行窗口消息泵直到进程被测试 harness 终止。
        while unsafe { IsWindow(Some(current_window)) }.as_bool() {
            // 尚未转换时轮询一次性控制请求。
            if !transitioned {
                // 读取可选动态通道的当前状态。
                let requested = transition
                    // 只处理动态模式。
                    .as_ref()
                    // 非阻塞接收唯一请求。
                    .map(Receiver::try_recv);
                // 收到固定转换请求时执行封闭状态分支。
                match requested {
                    // 隐藏态只改变当前自有窗口可见性。
                    Some(Ok(())) if mode == FixtureMode::HideOnCommand => unsafe {
                        // 隐藏窗口且不恢复、不显示、不激活其他窗口。
                        let _ = ShowWindow(current_window, SW_HIDE);
                        // 标记转换已完成。
                        transitioned = true;
                        // 通知父测试公开清单可以重新采样。
                        acknowledge_transition()?;
                    },
                    // 重建态先创建不可见替代代际以保证句柄不同。
                    Some(Ok(())) if mode == FixtureMode::RecreateOnCommand => {
                        // 创建同标题但尚不可见的替代顶层窗口。
                        let replacement = unsafe {
                            // 调用同一系统 STATIC 类和封闭样式。
                            CreateWindowExW(
                                // 保持无激活与工具窗口边界。
                                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                                // 使用系统 STATIC 类。
                                w!("STATIC"),
                                // 保持相同公开标题以模拟窗口重建。
                                PCWSTR(encoded_title.as_ptr()),
                                // 创建时不带可见位，避免旧新窗口同时进入公开清单。
                                WS_OVERLAPPEDWINDOW,
                                // 让系统选择横坐标。
                                CW_USEDEFAULT,
                                // 让系统选择纵坐标。
                                CW_USEDEFAULT,
                                // 保持固定 fixture 宽度。
                                320,
                                // 保持固定 fixture 高度。
                                200,
                                // 不设置父窗口。
                                None,
                                // 不设置菜单。
                                None,
                                // 系统类不需要模块句柄。
                                None,
                                // 不传递任意创建参数。
                                None,
                            )?
                        };
                        // 在替代代际内创建同名标准按钮。
                        let _replacement_button = unsafe {
                            // 创建替代窗口的可见子控件。
                            CreateWindowExW(
                                // 子控件不需要扩展样式。
                                Default::default(),
                                // 使用系统 BUTTON 类。
                                w!("BUTTON"),
                                // 保持逐窗口唯一按钮名称。
                                PCWSTR(encoded_button_title.as_ptr()),
                                // 子控件随父窗口显示。
                                WS_CHILD | WS_VISIBLE,
                                // 保持固定相对横坐标。
                                24,
                                // 保持固定相对纵坐标。
                                24,
                                // 保持固定按钮宽度。
                                220,
                                // 保持固定按钮高度。
                                48,
                                // 绑定替代父窗口。
                                Some(replacement),
                                // 不设置菜单或控件 ID。
                                None,
                                // 系统类不需要模块句柄。
                                None,
                                // 不传递任意创建参数。
                                None,
                            )?
                        };
                        // 在创建线程销毁旧代际。
                        unsafe {
                            // 旧窗口从此必须无法按旧 opaque ID 重新解析。
                            DestroyWindow(current_window)?;
                            // 替代代际仅以无激活方式进入公开清单。
                            let _ = ShowWindow(replacement, SW_SHOWNOACTIVATE);
                        }
                        // 消息泵转移到替代代际。
                        current_window = replacement;
                        // 标记转换已完成。
                        transitioned = true;
                        // 通知父测试可以验证旧 stale 与新发现。
                        acknowledge_transition()?;
                    }
                    // 控制输入关闭后保持现有窗口直到 harness 清理。
                    Some(Err(TryRecvError::Disconnected)) => {
                        // 禁止把控制通道关闭解释为状态转换。
                        transitioned = true;
                    }
                    // 尚未收到命令或静态模式时继续消息泵。
                    Some(Err(TryRecvError::Empty)) | None => {}
                    // 模式与命令组合由解析器封闭，此分支只保持失败安全。
                    Some(Ok(())) => {
                        // 未知组合不得执行窗口变更。
                        transitioned = true;
                    }
                }
            }
            // 处理当前线程队列中的全部待处理消息。
            while unsafe { PeekMessageW(&mut message, Some(HWND::default()), 0, 0, PM_REMOVE) }
                .as_bool()
            {
                // 翻译并分发系统消息。
                unsafe {
                    // 保持标准消息语义。
                    let _ = TranslateMessage(&message);
                    // 分发到系统 STATIC 过程。
                    DispatchMessageW(&message);
                }
            }
            // 避免无消息时忙等。
            thread::sleep(Duration::from_millis(2));
        }
        // 消息泵正常结束。
        Ok(())
    }

    // 执行仅供门禁使用的 fixture。
    pub(super) fn entry() {
        // 运行失败时以固定非零码退出且不写 stdout/stderr。
        if run().is_err() {
            // 返回 fixture 启动失败。
            std::process::exit(2);
        }
    }
}

#[cfg(target_os = "windows")]
fn main() {
    windows_fixture::entry();
}
