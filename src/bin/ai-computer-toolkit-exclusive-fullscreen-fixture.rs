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
    //! DXGI 独占全屏兼容边界专用的工具自有窗口夹具。

    // 导入固定控制通道、窗口状态与有界恢复工具。
    use std::{
        // 只读取固定 enter/exit 控制命令。
        io::BufRead,
        // 在线程间传递封闭控制命令。
        sync::mpsc::{self, Receiver, TryRecvError},
        // 独立读取控制输入并短暂让出消息泵。
        thread,
        // 限制独占状态最长十秒。
        time::{Duration, Instant},
    };

    // 导入 D3D11、DXGI 与固定 Win32 窗口接口。
    use windows::{
        // 导入布尔、模块和窗口句柄。
        Win32::Foundation::{DXGI_STATUS_MODE_CHANGE_IN_PROGRESS, HMODULE, HWND},
        // 导入 D3D11 设备与 swap chain 创建接口。
        Win32::Graphics::{
            // 只允许硬件 D3D11 设备进入真实独占状态。
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            // 创建 D3D11 设备、上下文和 swap chain。
            Direct3D11::{
                // 保留 BGRA 互操作能力。
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                // 固定 SDK 版本。
                D3D11_SDK_VERSION,
                // 一次创建硬件设备与 swap chain。
                D3D11CreateDeviceAndSwapChain,
            },
            // 导入 DXGI swap chain 描述和独占状态接口。
            Dxgi::{
                // 导入固定颜色格式与采样描述。
                Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_DESC, DXGI_SAMPLE_DESC},
                // 当前会话无法进入独占时的官方可继续错误。
                DXGI_ERROR_NOT_CURRENTLY_AVAILABLE,
                // 保存 swap chain 描述。
                DXGI_SWAP_CHAIN_DESC,
                // 允许显式显示模式切换。
                DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH,
                // 使用传统 discard swap effect。
                DXGI_SWAP_EFFECT_DISCARD,
                // 允许 back buffer 作为呈现目标。
                DXGI_USAGE_RENDER_TARGET_OUTPUT,
                // 保存已创建 swap chain。
                IDXGISwapChain,
            },
        },
        // 导入系统 STATIC 窗口与消息泵接口。
        Win32::UI::WindowsAndMessaging::{
            // 使用系统默认窗口位置。
            CW_USEDEFAULT,
            // 创建工具自有顶层窗口。
            CreateWindowExW,
            // 仅在夹具线程销毁工具自有窗口。
            DestroyWindow,
            // 分发当前线程消息。
            DispatchMessageW,
            // 保存消息对象。
            MSG,
            // 非阻塞移除消息。
            PM_REMOVE,
            // 非阻塞读取当前线程消息。
            PeekMessageW,
            // 启动时不抢前景。
            SW_SHOWNOACTIVATE,
            // 发布封闭独占状态标题。
            SetWindowTextW,
            // 无激活显示普通窗口。
            ShowWindow,
            // 翻译键盘消息。
            TranslateMessage,
            // 避免测试窗口进入普通应用切换面板。
            WS_EX_TOOLWINDOW,
            // 使用标准可前景顶层窗口样式。
            WS_OVERLAPPEDWINDOW,
            // 创建后立即可见。
            WS_VISIBLE,
        },
        // 导入宽字符串指针与系统类宏。
        core::{BOOL, HRESULT, Interface, PCWSTR, w},
    };

    // 固定独占状态最长持续时间。
    const MAXIMUM_FULLSCREEN_DURATION: Duration = Duration::from_secs(10);
    // 固定成功进入独占的标题后缀。
    const EXCLUSIVE_SUFFIX: &str = "-exclusive";
    // 固定当前会话不可进入独占的标题后缀。
    const UNAVAILABLE_SUFFIX: &str = "-exclusive-unavailable";
    // 固定独占状态意外丢失的标题后缀。
    const LOST_SUFFIX: &str = "-exclusive-lost";
    // 固定意外平台错误的标题后缀。
    const FAILED_SUFFIX: &str = "-exclusive-failed";

    // 固定夹具只接受进入或退出命令。
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ControlCommand {
        // 请求进入真实 DXGI 独占状态。
        Enter,
        // 请求切回 windowed 并结束夹具。
        Exit,
    }

    // 保存独占状态进入结果。
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum EnterState {
        // SetFullscreenState 和 GetFullscreenState 均证明独占。
        Exclusive,
        // 当前会话或转换状态暂时不可用。
        Unavailable,
        // 返回不在官方可继续集合内的意外错误。
        Failed,
    }

    // 验证夹具标题仅来自测试固定前缀与 ASCII 后缀。
    fn fixture_title() -> Option<String> {
        // 读取唯一标题参数。
        let arguments = std::env::args().skip(1).collect::<Vec<_>>();
        // 只接受一个固定标题参数。
        let [title] = arguments.as_slice() else {
            // 额外参数全部拒绝。
            return None;
        };
        // 限制前缀、ASCII 与长度。
        if !title.starts_with("act-rust-exclusive-fixture-")
        // 要求全部 ASCII。
        || !title.is_ascii()
        // 为状态后缀保留有界空间。
        || title.len() > 96
        {
            // 非测试标题必须拒绝。
            return None;
        }
        // 返回已验证标题。
        Some(title.to_owned())
    }

    // 启动只接受 enter/exit 的固定控制线程。
    fn control_receiver() -> Receiver<ControlCommand> {
        // 创建进程内控制通道。
        let (sender, receiver) = mpsc::channel();
        // 独立阻塞读取 stdin，避免阻塞窗口消息泵。
        thread::spawn(move || {
            // 锁定当前进程标准输入。
            let input = std::io::stdin();
            // 建立逐行读取句柄。
            let input = input.lock();
            // 只处理固定短命令。
            for line in input.lines() {
                // 读取失败立即请求安全退出。
                let Ok(line) = line else {
                    // 接收端退出时无需延长夹具生命周期。
                    let _ = sender.send(ControlCommand::Exit);
                    // 结束控制线程。
                    return;
                };
                // 映射逐字命令。
                let command = match line.trim() {
                    // 只允许进入独占。
                    "enter" => ControlCommand::Enter,
                    // 只允许退出并恢复。
                    "exit" => ControlCommand::Exit,
                    // 未知输入按安全退出处理。
                    _ => ControlCommand::Exit,
                };
                // 主消息泵退出时忽略发送失败。
                if sender.send(command).is_err() {
                    // 结束控制线程。
                    return;
                }
                // exit 后禁止读取更多命令。
                if command == ControlCommand::Exit {
                    // 结束控制线程。
                    return;
                }
            }
            // stdin 关闭必须请求切回 windowed。
            let _ = sender.send(ControlCommand::Exit);
        });
        // 返回主消息泵拥有的接收端。
        receiver
    }

    // 编码并设置不含原生事实的封闭状态标题。
    fn set_state_title(window: HWND, base: &str, suffix: &str) {
        // 构造固定状态标题。
        let title = format!("{base}{suffix}");
        // 编码为 UTF-16 并追加终止零。
        let title = title
            // 编码安全 ASCII 标题。
            .encode_utf16()
            // 追加 Win32 终止零。
            .chain(std::iter::once(0))
            // 固定拥有型缓冲区生命周期。
            .collect::<Vec<_>>();
        // 只修改当前夹具拥有的窗口标题。
        unsafe {
            // 发布封闭状态供公开窗口发现读取。
            let _ = SetWindowTextW(window, PCWSTR(title.as_ptr()));
        }
    }

    // 创建 windowed D3D11 swap chain。
    fn create_swap_chain(window: HWND) -> windows::core::Result<IDXGISwapChain> {
        // 构造官方建议的 windowed 初始描述。
        let description = DXGI_SWAP_CHAIN_DESC {
            // 让 DXGI 从当前窗口推断桌面尺寸和刷新率。
            BufferDesc: DXGI_MODE_DESC {
                // 零宽度从窗口推断。
                Width: 0,
                // 零高度从窗口推断。
                Height: 0,
                // 使用固定 BGRA 格式。
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                // 其余刷新率与扫描属性由 DXGI 选择。
                ..Default::default()
            },
            // 禁止多重采样。
            SampleDesc: DXGI_SAMPLE_DESC {
                // 使用单样本。
                Count: 1,
                // 单样本没有质量索引。
                Quality: 0,
            },
            // 允许呈现 back buffer。
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            // 传统 discard 使用单 back buffer。
            BufferCount: 1,
            // 绑定当前工具自有窗口。
            OutputWindow: window,
            // 必须先以 windowed 创建。
            Windowed: BOOL::from(true),
            // 使用兼容性稳定的 discard swap effect。
            SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
            // 允许 DXGI 显式显示模式切换。
            Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH.0 as u32,
        };
        // 保存 D3D11 创建结果。
        let mut swap_chain = None;
        // 保存设备以维持 swap chain 生命周期。
        let mut device = None;
        // 保存立即上下文以维持设备完整创建。
        let mut context = None;
        // 一次创建硬件设备与 windowed swap chain。
        unsafe {
            // 调用 D3D11 固定入口。
            D3D11CreateDeviceAndSwapChain(
                // 由硬件驱动选择默认 adapter。
                None,
                // 只允许硬件设备。
                D3D_DRIVER_TYPE_HARDWARE,
                // 不提供软件模块。
                HMODULE::default(),
                // 保留 BGRA 支持。
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                // 让系统选择 feature level。
                None,
                // 使用固定 SDK 版本。
                D3D11_SDK_VERSION,
                // 传入 windowed swap chain 描述。
                Some(&description),
                // 接收 swap chain。
                Some(&mut swap_chain),
                // 接收设备并在本函数返回前由 swap chain 持有底层引用。
                Some(&mut device),
                // 不读取公开 feature level。
                None,
                // 接收并立即释放未使用的上下文引用。
                Some(&mut context),
            )?;
        }
        // 缺失 swap chain 表示不可恢复的夹具创建失败。
        swap_chain.ok_or_else(windows::core::Error::from_thread)
    }

    // 查询 swap chain 当前是否真正处于独占状态。
    fn is_exclusive(swap_chain: &IDXGISwapChain) -> windows::core::Result<bool> {
        // 初始化状态值。
        let mut fullscreen = BOOL::from(false);
        // 只读取布尔状态，不保留输出对象。
        unsafe {
            // 查询当前独占状态。
            swap_chain.GetFullscreenState(Some(&mut fullscreen), None)?;
        }
        // 返回强类型布尔值。
        Ok(fullscreen.as_bool())
    }

    // 保留 SetFullscreenState 的原始 HRESULT 以区分正值 DXGI 状态。
    fn set_fullscreen_state(swap_chain: &IDXGISwapChain, fullscreen: bool) -> HRESULT {
        // 直接调用生成绑定的相同 COM vtable 槽位。
        unsafe {
            // 保留原始 HRESULT，不让 Result 转换吞掉成功状态码。
            (swap_chain.vtable().SetFullscreenState)(
                // 传入当前 swap chain 的 COM this 指针。
                swap_chain.as_raw(),
                // 转换固定独占布尔值。
                BOOL::from(fullscreen),
                // 让 DXGI 按当前窗口位置选择输出。
                std::ptr::null_mut(),
            )
        }
    }

    // 尝试进入独占并收敛为封闭状态。
    fn enter_exclusive(swap_chain: &IDXGISwapChain) -> EnterState {
        // 调用唯一真实独占入口并保留原始状态。
        let result = set_fullscreen_state(swap_chain, true);
        // 官方可继续不可用结果保持 windowed。
        if matches!(
            result,
            // 当前会话无法进入独占。
            DXGI_ERROR_NOT_CURRENTLY_AVAILABLE
            // 显示模式仍在转换中。
            | DXGI_STATUS_MODE_CHANGE_IN_PROGRESS
        ) {
            // 返回当前环境不可用。
            return EnterState::Unavailable;
        }
        // 其他失败属于夹具错误。
        if result.is_err() {
            // 禁止把未知错误伪装为环境限制。
            return EnterState::Failed;
        }
        // 成功后必须再由 GetFullscreenState 证明。
        match is_exclusive(swap_chain) {
            // 只有 TRUE 才接受为独占。
            Ok(true) => EnterState::Exclusive,
            // FALSE 或查询失败都不能伪装成功。
            Ok(false) | Err(_) => EnterState::Failed,
        }
    }

    // 无条件尝试切回 windowed 并核对状态。
    fn leave_exclusive(swap_chain: &IDXGISwapChain) -> bool {
        // 调用官方要求的释放前恢复入口。
        if set_fullscreen_state(swap_chain, false).is_err() {
            // 恢复失败不得声称成功。
            return false;
        }
        // 只有状态明确为 false 才算恢复。
        matches!(is_exclusive(swap_chain), Ok(false))
    }

    // 创建窗口、swap chain 并运行有界控制循环。
    fn run() -> windows::core::Result<()> {
        // 验证唯一安全标题。
        let title = fixture_title().ok_or_else(windows::core::Error::from_thread)?;
        // 编码初始标题并追加终止零。
        let encoded_title = title
            // 编码为 UTF-16。
            .encode_utf16()
            // 追加终止零。
            .chain(std::iter::once(0))
            // 固定拥有型缓冲区生命周期。
            .collect::<Vec<_>>();
        // 创建可成为前景但启动时不激活的工具自有窗口。
        let window = unsafe {
            // 使用系统 STATIC 类避免注册任意窗口类。
            CreateWindowExW(
                // 仅隐藏普通任务切换入口。
                WS_EX_TOOLWINDOW,
                // 使用系统 STATIC 类。
                w!("STATIC"),
                // 使用已验证唯一标题。
                PCWSTR(encoded_title.as_ptr()),
                // 使用可发现的标准顶层窗口样式。
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                // 让系统选择横坐标。
                CW_USEDEFAULT,
                // 让系统选择纵坐标。
                CW_USEDEFAULT,
                // 固定 windowed 宽度。
                640,
                // 固定 windowed 高度。
                360,
                // 不设置父窗口。
                None,
                // 不设置菜单。
                None,
                // 系统类不需要模块句柄。
                None,
                // 不传递创建参数。
                None,
            )?
        };
        // 无激活显示，后续只由显式测试建立前景。
        unsafe {
            // 显示当前工具自有窗口。
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        }
        // 创建官方建议的 windowed swap chain。
        let swap_chain = create_swap_chain(window)?;
        // 启动固定控制输入线程。
        let control = control_receiver();
        // 标记是否已经处理 enter。
        let mut entered = false;
        // 保存独占开始时刻供强制恢复。
        let mut exclusive_since = None;
        // 初始化消息对象。
        let mut message = MSG::default();
        // 运行直到固定退出命令或强制恢复。
        loop {
            // 处理当前线程队列中的全部待处理消息。
            while unsafe { PeekMessageW(&mut message, Some(HWND::default()), 0, 0, PM_REMOVE) }
                .as_bool()
            {
                // 保持标准消息翻译与分发语义。
                unsafe {
                    // 翻译键盘消息。
                    let _ = TranslateMessage(&message);
                    // 分发到系统 STATIC 窗口过程。
                    DispatchMessageW(&message);
                }
            }
            // 读取当前固定控制命令。
            match control.try_recv() {
                // 首次 enter 尝试真实独占。
                Ok(ControlCommand::Enter) if !entered => {
                    // 禁止第二次 enter。
                    entered = true;
                    // 收敛实际进入结果。
                    match enter_exclusive(&swap_chain) {
                        // 真实独占成功。
                        EnterState::Exclusive => {
                            // 发布成功状态标题。
                            set_state_title(window, &title, EXCLUSIVE_SUFFIX);
                            // 启动十秒强制恢复预算。
                            exclusive_since = Some(Instant::now());
                        }
                        // 当前平台不可用。
                        EnterState::Unavailable => {
                            // 发布可继续不可用状态。
                            set_state_title(window, &title, UNAVAILABLE_SUFFIX);
                        }
                        // 意外错误。
                        EnterState::Failed => {
                            // 发布固定失败状态。
                            set_state_title(window, &title, FAILED_SUFFIX);
                        }
                    }
                }
                // exit 始终先恢复 windowed 再结束。
                Ok(ControlCommand::Exit) => {
                    // 尽最大努力满足释放前恢复要求。
                    let _ = leave_exclusive(&swap_chain);
                    // 结束控制循环。
                    break;
                }
                // 第二次 enter 违反封闭协议，按安全退出处理。
                Ok(ControlCommand::Enter) => {
                    // 恢复 windowed。
                    let _ = leave_exclusive(&swap_chain);
                    // 结束控制循环。
                    break;
                }
                // 尚无命令时继续消息泵。
                Err(TryRecvError::Empty) => {}
                // 控制通道关闭必须恢复退出。
                Err(TryRecvError::Disconnected) => {
                    // 恢复 windowed。
                    let _ = leave_exclusive(&swap_chain);
                    // 结束控制循环。
                    break;
                }
            }
            // 独占期间持续核对状态没有丢失。
            if exclusive_since.is_some() && !matches!(is_exclusive(&swap_chain), Ok(true)) {
                // 发布状态丢失事实。
                set_state_title(window, &title, LOST_SUFFIX);
                // 恢复 windowed 后保留窗口供父测试读取状态。
                let _ = leave_exclusive(&swap_chain);
                // 停止重复核对已经丢失的独占状态。
                exclusive_since = None;
            }
            // 超过十秒必须自动恢复，防止显示模式残留。
            if exclusive_since
                // 只处理成功进入独占的场景。
                .is_some_and(|started| started.elapsed() >= MAXIMUM_FULLSCREEN_DURATION)
            {
                // 切回 windowed。
                let _ = leave_exclusive(&swap_chain);
                // 结束控制循环。
                break;
            }
            // 避免无消息时忙等。
            thread::sleep(Duration::from_millis(2));
        }
        // 最后一次强制满足释放前恢复要求。
        let restored = leave_exclusive(&swap_chain);
        // 在释放 swap chain 前销毁工具自有窗口。
        unsafe {
            // 只销毁当前夹具拥有的窗口。
            DestroyWindow(window)?;
        }
        // 恢复失败必须以非零状态报告。
        if !restored {
            // 返回固定线程错误。
            return Err(windows::core::Error::from_thread());
        }
        // 正常完成。
        Ok(())
    }

    // 执行仅供兼容门禁使用的固定夹具。
    pub(super) fn entry() {
        // 运行失败时以固定非零码退出且不写标准流。
        if run().is_err() {
            // 返回夹具启动或恢复失败。
            std::process::exit(2);
        }
    }
}

#[cfg(target_os = "windows")]
fn main() {
    windows_fixture::entry();
}
