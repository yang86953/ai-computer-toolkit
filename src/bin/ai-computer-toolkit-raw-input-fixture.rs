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
    //! Raw Input 与 SendInput 接收边界专用的工具自有窗口夹具。

    // 导入窗口过程共享状态与有界消息泵等待。
    use std::{
        // 保存原始窗口过程与首次 Raw Input 到达状态。
        sync::atomic::{AtomicBool, AtomicIsize, Ordering},
        // 在空消息泵时短暂让出执行。
        thread,
        // 限制轮询频率。
        time::Duration,
    };

    // 导入固定 Raw Input 注册与 Win32 窗口接口。
    use windows::{
        // 导入窗口句柄与消息参数。
        Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        // 导入固定 Raw Input 设备注册与消息头读取类型。
        Win32::UI::Input::{
            // 读取 WM_INPUT 对应的 Raw Input 头。
            GetRawInputData,
            // 保存 WM_INPUT 私有消息句柄。
            HRAWINPUT,
            // 保存固定设备注册。
            RAWINPUTDEVICE,
            // 保存不公开的 Raw Input 消息头。
            RAWINPUTHEADER,
            // 只读取 Raw Input 头部。
            RID_HEADER,
            // 禁止夹具接收 legacy 输入消息。
            RIDEV_NOLEGACY,
            // 注册固定 mouse 或 keyboard usage。
            RegisterRawInputDevices,
        },
        // 导入系统 STATIC 窗口、子类化与消息泵接口。
        Win32::UI::WindowsAndMessaging::{
            // 使用系统默认窗口位置。
            CW_USEDEFAULT,
            // 调用系统 STATIC 原始窗口过程。
            CallWindowProcW,
            // 创建工具自有顶层窗口。
            CreateWindowExW,
            // 无法取得原始过程时使用系统默认行为。
            DefWindowProcW,
            // 分发当前线程消息。
            DispatchMessageW,
            // 访问窗口过程槽位。
            GWLP_WNDPROC,
            // 读取当前标题长度。
            GetWindowTextLengthW,
            // 读取当前工具自有标题。
            GetWindowTextW,
            // 保存消息对象。
            MSG,
            // 非阻塞移除消息。
            PM_REMOVE,
            // 非阻塞读取当前线程消息。
            PeekMessageW,
            // 无激活显示普通窗口。
            SW_SHOWNOACTIVATE,
            // 安装工具自有 Raw Input 窗口过程。
            SetWindowLongPtrW,
            // 发布 Raw Input 到达状态标题。
            SetWindowTextW,
            // 显示窗口但不在启动时抢占前景。
            ShowWindow,
            // 翻译键盘消息。
            TranslateMessage,
            // Raw Input 到达消息。
            WM_INPUT,
            // 保存强类型窗口过程。
            WNDPROC,
            // 避免测试窗口进入普通应用切换面板。
            WS_EX_TOOLWINDOW,
            // 使用标准可前景顶层窗口样式。
            WS_OVERLAPPEDWINDOW,
            // 创建后立即可见。
            WS_VISIBLE,
        },
        // 导入宽字符串指针与系统类宏。
        core::{PCWSTR, w},
    };

    // 保存替换前的系统 STATIC 窗口过程。
    static ORIGINAL_WINDOW_PROC: AtomicIsize = AtomicIsize::new(0);
    // 保证 Raw Input 到达状态只发布一次。
    static RAW_INPUT_RECEIVED: AtomicBool = AtomicBool::new(false);
    // 固定无设备身份的合成到达状态标题后缀。
    const SYNTHETIC_SUFFIX: &str = "-raw-received-synthetic";
    // 固定带设备身份的到达状态标题后缀。
    const DEVICE_SUFFIX: &str = "-raw-received-device";

    // 固定夹具只允许鼠标或键盘 Raw Input usage。
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FixtureMode {
        // 注册 Generic Desktop Mouse usage。
        Mouse,
        // 注册 Generic Desktop Keyboard usage。
        Keyboard,
    }

    // 返回当前模式的固定 usage ID。
    impl FixtureMode {
        // 映射到 Generic Desktop Usage Page 内的固定设备 usage。
        fn usage(self) -> u16 {
            // 只允许两个封闭设备类别。
            match self {
                // Mouse usage ID。
                Self::Mouse => 0x02,
                // Keyboard usage ID。
                Self::Keyboard => 0x06,
            }
        }
    }

    // 解析并验证夹具唯一标题。
    fn fixture_title() -> Option<String> {
        // 读取第一个标题参数。
        let title = std::env::args().nth(1)?;
        // 限制固定前缀、ASCII 与长度。
        if !title.starts_with("act-rust-raw-input-fixture-")
        // 要求标题全部为 ASCII。
        || !title.is_ascii()
        // 为状态后缀保留有界空间。
        || title.len() > 96
        {
            // 非测试标题必须拒绝。
            return None;
        }
        // 返回已验证标题。
        Some(title)
    }

    // 解析封闭 Raw Input 模式。
    fn fixture_mode() -> Option<FixtureMode> {
        // 只读取唯一模式参数。
        let arguments = std::env::args().skip(2).collect::<Vec<_>>();
        // 拒绝任何额外参数。
        match arguments.as_slice() {
            // 固定鼠标注册模式。
            [mode] if mode == "--mouse" => Some(FixtureMode::Mouse),
            // 固定键盘注册模式。
            [mode] if mode == "--keyboard" => Some(FixtureMode::Keyboard),
            // 其他形状全部拒绝。
            _ => None,
        }
    }

    // 调用替换前的系统 STATIC 窗口过程。
    unsafe fn call_original(
        // 接收工具自有窗口句柄。
        window: HWND,
        // 接收窗口消息。
        message: u32,
        // 接收消息字参数。
        word: WPARAM,
        // 接收消息长参数。
        value: LPARAM,
    ) -> LRESULT {
        // 读取安装时保存的原始过程。
        let original = ORIGINAL_WINDOW_PROC.load(Ordering::Acquire);
        // 缺失原始过程时使用系统默认过程失败安全。
        if original == 0 {
            // 不猜测系统 STATIC 私有状态。
            return unsafe { DefWindowProcW(window, message, word, value) };
        }
        // 将系统返回值恢复为强类型窗口过程。
        let procedure = unsafe { std::mem::transmute::<isize, WNDPROC>(original) };
        // 保留系统行为并让 WM_INPUT 完成必要清理。
        unsafe { CallWindowProcW(procedure, window, message, word, value) }
    }

    // 把首次 Raw Input 到达事实发布为可经公开发现读取的标题后缀。
    fn publish_received(window: HWND, device_backed: bool) {
        // 只允许第一次 WM_INPUT 更新标题。
        if RAW_INPUT_RECEIVED.swap(true, Ordering::AcqRel) {
            // 后续消息不重复分配或修改窗口文本。
            return;
        }
        // 读取当前标题 UTF-16 单元上限。
        let length = unsafe { GetWindowTextLengthW(window) };
        // 负长度不可信，保持失败闭合。
        if length < 0 {
            // 不发布不完整状态。
            return;
        }
        // 为当前标题、后缀与终止零分配有界缓冲区。
        let mut title = vec![0_u16; length as usize + 1];
        // 读取当前工具自有标题。
        let copied = unsafe { GetWindowTextW(window, &mut title) };
        // 空标题或读取失败不得发布模糊状态。
        if copied <= 0 {
            // 保留原标题。
            return;
        }
        // 截断到实际标题内容。
        title.truncate(copied as usize);
        // 选择不泄漏设备身份的封闭到达类别。
        let suffix = if device_backed {
            // 消息头携带非空设备来源。
            DEVICE_SUFFIX
        } else {
            // 消息头没有设备来源，只能归为合成到达。
            SYNTHETIC_SUFFIX
        };
        // 追加固定到达状态后缀。
        title.extend(suffix.encode_utf16());
        // 追加 Win32 终止零。
        title.push(0);
        // 只更新当前夹具拥有的窗口标题。
        unsafe {
            // 发布不含设备或原生身份的状态。
            let _ = SetWindowTextW(window, PCWSTR(title.as_ptr()));
        }
    }

    // 读取 Raw Input 消息头并判断是否携带设备来源。
    fn raw_input_device_backed(value: LPARAM) -> Option<bool> {
        // 初始化不公开的 Raw Input 消息头。
        let mut header = RAWINPUTHEADER::default();
        // 固定头部字节长度。
        let mut size = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        // 从 WM_INPUT lParam 恢复私有消息句柄。
        let input = HRAWINPUT(value.0 as *mut std::ffi::c_void);
        // 只读取固定头部，不读取或公开设备数据。
        let copied = unsafe {
            // 调用公开 Windows Raw Input 读取接口。
            GetRawInputData(
                // 传入当前消息句柄。
                input,
                // 只请求头部。
                RID_HEADER,
                // 写入固定栈上结构。
                Some((&mut header as *mut RAWINPUTHEADER).cast()),
                // 传入并接收结构长度。
                &mut size,
                // 冻结头部 ABI 大小。
                std::mem::size_of::<RAWINPUTHEADER>() as u32,
            )
        };
        // 只接受恰好一个完整头部。
        if copied != std::mem::size_of::<RAWINPUTHEADER>() as u32
        // Windows 不得改写为其他大小。
        || size != std::mem::size_of::<RAWINPUTHEADER>() as u32
        {
            // 不完整消息不形成证据。
            return None;
        }
        // 非空 hDevice 才能声明带设备来源。
        Some(!header.hDevice.is_invalid())
    }

    // 记录 WM_INPUT 到达并保留系统清理语义。
    unsafe extern "system" fn raw_input_window_proc(
        // 接收工具自有窗口句柄。
        window: HWND,
        // 接收窗口消息。
        message: u32,
        // 接收消息字参数。
        word: WPARAM,
        // 接收消息长参数。
        value: LPARAM,
    ) -> LRESULT {
        // WM_INPUT 到达即形成目标接收事实。
        if message == WM_INPUT {
            // 只在消息头完整时发布一次稳定状态。
            if let Some(device_backed) = raw_input_device_backed(value) {
                // 区分合成消息与带设备来源的 Raw Input。
                publish_received(window, device_backed);
            }
        }
        // 调用原始过程以完成前景 Raw Input 清理。
        unsafe { call_original(window, message, word, value) }
    }

    // 创建固定 Raw Input 窗口并运行消息泵。
    fn run() -> windows::core::Result<()> {
        // 验证唯一安全标题。
        let title = fixture_title().ok_or_else(windows::core::Error::from_thread)?;
        // 验证唯一封闭模式。
        let mode = fixture_mode().ok_or_else(windows::core::Error::from_thread)?;
        // 编码标题并追加终止零。
        let encoded_title = title
            // 编码为 UTF-16。
            .encode_utf16()
            // 追加终止零。
            .chain(std::iter::once(0))
            // 固定拥有型缓冲区生命周期。
            .collect::<Vec<_>>();
        // 创建可成为前景但启动时不激活的工具自有顶层窗口。
        let window = unsafe {
            // 使用系统 STATIC 类避免注册任意窗口类。
            CreateWindowExW(
                // 仅隐藏普通任务切换入口，不禁止后续前景激活。
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
                // 固定夹具宽度。
                320,
                // 固定夹具高度。
                200,
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
        // 安装只记录 WM_INPUT 的封闭窗口过程。
        let original = unsafe {
            // 替换当前工具自有窗口过程。
            SetWindowLongPtrW(
                // 绑定工具自有窗口。
                window,
                // 替换窗口过程槽位。
                GWLP_WNDPROC,
                // 先转为代码指针再转成 Win32 整数槽位。
                raw_input_window_proc as *const () as usize as isize,
            )
        };
        // 零值表示无法建立可恢复窗口过程。
        if original == 0 {
            // 返回固定启动失败。
            return Err(windows::core::Error::from_thread());
        }
        // 发布原始过程供所有消息转发。
        ORIGINAL_WINDOW_PROC.store(original, Ordering::Release);
        // 构造唯一固定 Raw Input 设备注册。
        let device = RAWINPUTDEVICE {
            // 使用 Generic Desktop Usage Page。
            usUsagePage: 0x01,
            // 使用当前封闭 mouse 或 keyboard usage。
            usUsage: mode.usage(),
            // 禁止该设备为夹具生成 legacy 输入消息。
            dwFlags: RIDEV_NOLEGACY,
            // 只把 Raw Input 路由到当前工具自有窗口。
            hwndTarget: window,
        };
        // 向 Windows 注册唯一设备类别。
        unsafe {
            // 传入严格结构大小。
            RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32)?;
        }
        // 无激活显示，后续只允许生产输入路线取得前景。
        unsafe {
            // 显示当前工具自有窗口。
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        }
        // 初始化消息对象。
        let mut message = MSG::default();
        // 运行直到测试 harness 终止精确子进程。
        loop {
            // 处理当前线程队列中的全部待处理消息。
            while unsafe { PeekMessageW(&mut message, Some(HWND::default()), 0, 0, PM_REMOVE) }
                .as_bool()
            {
                // 保持标准消息翻译与分发语义。
                unsafe {
                    // 翻译键盘消息。
                    let _ = TranslateMessage(&message);
                    // 分发到工具自有窗口过程。
                    DispatchMessageW(&message);
                }
            }
            // 避免无消息时忙等。
            thread::sleep(Duration::from_millis(2));
        }
    }

    // 执行仅供兼容门禁使用的固定夹具。
    pub(super) fn entry() {
        // 运行失败时以固定非零码退出且不写标准流。
        if run().is_err() {
            // 返回夹具启动失败。
            std::process::exit(2);
        }
    }
}

#[cfg(target_os = "windows")]
fn main() {
    windows_fixture::entry();
}
