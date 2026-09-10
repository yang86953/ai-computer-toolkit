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
    //! 语义元素动作生产门禁专用的 no-activate 标准控件窗口。

    // 导入 Win32 窗口、消息与控件接口。
    use windows::{
        // 导入句柄与消息参数。
        Win32::Foundation::{HWND, LPARAM, WPARAM},
        // 导入系统窗口接口。
        Win32::UI::WindowsAndMessaging::{
            // 导入标准按钮样式。
            BS_AUTOCHECKBOX,
            BS_AUTORADIOBUTTON,
            // 使用系统默认窗口位置。
            CW_USEDEFAULT,
            // 创建系统类窗口。
            CreateWindowExW,
            // 分发消息。
            DispatchMessageW,
            // 读取消息。
            GetMessageW,
            // 导入控件菜单 ID 句柄。
            HMENU,
            // 导入列表项添加消息。
            LB_ADDSTRING,
            // 导入列表样式。
            LBS_NOTIFY,
            // 保存消息。
            MSG,
            // 以无激活方式显示窗口。
            SW_SHOWNOACTIVATE,
            // 向自有控件发送同步初始化消息。
            SendMessageW,
            // 显示窗口。
            ShowWindow,
            // 翻译消息。
            TranslateMessage,
            // 导入通用窗口样式。
            WINDOW_STYLE,
            WS_CHILD,
            WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW,
            WS_OVERLAPPEDWINDOW,
            WS_VISIBLE,
            WS_VSCROLL,
        },
        // 导入宽字符串构造与指针。
        core::{PCWSTR, w},
    };

    // 固定 Invoke 控件 ID。
    const INVOKE_ID: usize = 1_001;
    // 固定 Value 控件 ID。
    const VALUE_ID: usize = 1_002;
    // 固定 Toggle 控件 ID。
    const TOGGLE_ID: usize = 1_003;
    // 固定 Select 控件 ID。
    const SELECT_ID: usize = 1_004;
    // 固定 Scroll 控件 ID。
    const SCROLL_ID: usize = 1_005;

    // 验证 fixture 标题只来自测试固定前缀与 ASCII 后缀。
    fn fixture_title() -> Option<String> {
        // 读取唯一标题参数。
        let title = std::env::args().nth(1)?;
        // 限制前缀、ASCII 与长度。
        if !title.starts_with("act-rust-semantic-fixture-")
        // 要求全部 ASCII。
        || !title.is_ascii()
        // 限制标题长度。
        || title.len() > 96
        {
            // 非测试标题必须拒绝。
            return None;
        }
        // 返回已验证标题。
        Some(title)
    }

    // 把测试文本编码为 NUL 结尾 UTF-16。
    fn wide(value: &str) -> Vec<u16> {
        // 编码并追加终止零。
        value.encode_utf16().chain(Some(0)).collect()
    }

    // 把固定控件 ID 转换为 child-window menu 句柄。
    fn control_id(value: usize) -> HMENU {
        // child window 的 hMenu 字段按 Win32 契约承载整数 ID。
        HMENU(value as *mut std::ffi::c_void)
    }

    // 创建一个测试专用标准子控件。
    fn create_control(
        // 接收系统类名。
        class_name: PCWSTR,
        // 接收窗口文本。
        text: PCWSTR,
        // 接收附加控件样式。
        style: WINDOW_STYLE,
        // 接收相对位置。
        position: (i32, i32),
        // 接收控件尺寸。
        size: (i32, i32),
        // 接收父窗口。
        parent: HWND,
        // 接收固定控件 ID。
        id: usize,
    ) -> windows::core::Result<HWND> {
        // 解构相对位置。
        let (x, y) = position;
        // 解构控件尺寸。
        let (width, height) = size;
        // 创建系统标准 child control。
        unsafe {
            // 调用固定 Win32 创建接口。
            CreateWindowExW(
                // 子控件不需要额外扩展样式。
                Default::default(),
                // 使用调用方系统类。
                class_name,
                // 使用测试文本。
                text,
                // 合并可见 child 与控件样式。
                WS_CHILD | WS_VISIBLE | style,
                // 设置相对横坐标。
                x,
                // 设置相对纵坐标。
                y,
                // 设置宽度。
                width,
                // 设置高度。
                height,
                // 绑定自有父窗口。
                Some(parent),
                // 绑定固定 AutomationId 来源。
                Some(control_id(id)),
                // 系统类不需要模块句柄。
                None,
                // 不传创建参数。
                None,
            )
        }
    }

    // 建立标准控件窗口并运行消息泵。
    fn run() -> windows::core::Result<()> {
        // 读取受限唯一标题。
        let title = fixture_title().ok_or_else(windows::core::Error::from_thread)?;
        // 编码顶层标题。
        let encoded_title = wide(&title);
        // 创建 no-activate 顶层系统 STATIC 窗口。
        let window = unsafe {
            // 调用系统窗口创建接口。
            CreateWindowExW(
                // 禁止激活并隐藏任务切换项。
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                // 使用系统 STATIC 类。
                w!("STATIC"),
                // 传入唯一标题。
                PCWSTR(encoded_title.as_ptr()),
                // 使用标准可见顶层样式。
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                // 让系统选择横坐标。
                CW_USEDEFAULT,
                // 让系统选择纵坐标。
                CW_USEDEFAULT,
                // 固定窗口宽度。
                520,
                // 固定窗口高度。
                460,
                // 不设置父窗口。
                None,
                // 不设置菜单。
                None,
                // 系统类不需要模块句柄。
                None,
                // 不传创建参数。
                None,
            )?
        };
        // 派生 Invoke 按钮名称。
        let invoke_text = wide(&format!("{title}-invoke"));
        // 创建标准 InvokePattern 按钮。
        let _invoke = create_control(
            // 使用 BUTTON 类。
            w!("BUTTON"),
            // 使用唯一按钮名称。
            PCWSTR(invoke_text.as_ptr()),
            // 普通按钮无需附加样式。
            WINDOW_STYLE::default(),
            // 固定相对位置。
            (20, 20),
            // 固定控件尺寸。
            (220, 36),
            // 绑定父窗口。
            window,
            // 绑定固定 ID。
            INVOKE_ID,
        )?;
        // 创建标准 ValuePattern Edit。
        let _value = create_control(
            // 使用 EDIT 类。
            w!("EDIT"),
            // 初始值为空。
            w!(""),
            // 普通单行 Edit 无附加样式。
            WINDOW_STYLE::default(),
            // 固定相对位置。
            (20, 72),
            // 固定控件尺寸。
            (220, 32),
            // 绑定父窗口。
            window,
            // 绑定固定 ID。
            VALUE_ID,
        )?;
        // 派生 Toggle 名称。
        let toggle_text = wide(&format!("{title}-toggle"));
        // 创建标准自动复选框。
        let _toggle = create_control(
            // 使用 BUTTON 类。
            w!("BUTTON"),
            // 使用唯一名称。
            PCWSTR(toggle_text.as_ptr()),
            // 使用自动复选框样式。
            WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
            // 固定相对位置。
            (20, 120),
            // 固定控件尺寸。
            (220, 32),
            // 绑定父窗口。
            window,
            // 绑定固定 ID。
            TOGGLE_ID,
        )?;
        // 派生 Select 名称。
        let select_text = wide(&format!("{title}-select"));
        // 创建标准自动单选按钮。
        let _select = create_control(
            // 使用 BUTTON 类。
            w!("BUTTON"),
            // 使用唯一名称。
            PCWSTR(select_text.as_ptr()),
            // 使用自动单选样式。
            WINDOW_STYLE(BS_AUTORADIOBUTTON as u32),
            // 固定相对位置。
            (20, 164),
            // 固定控件尺寸。
            (220, 32),
            // 绑定父窗口。
            window,
            // 绑定固定 ID。
            SELECT_ID,
        )?;
        // 创建带垂直滚动条的标准列表框。
        let list = create_control(
            // 使用 LISTBOX 类。
            w!("LISTBOX"),
            // 列表容器不需要公开名称。
            w!(""),
            // 启用通知与垂直滚动条。
            WINDOW_STYLE(LBS_NOTIFY as u32) | WS_VSCROLL,
            // 固定相对位置。
            (270, 20),
            // 固定控件尺寸。
            (210, 260),
            // 绑定父窗口。
            window,
            // 绑定固定 ID。
            SCROLL_ID,
        )?;
        // 添加足够条目使 ScrollPattern 可用。
        for index in 0..80 {
            // 构造当前唯一条目文本。
            let item = wide(&format!("semantic-item-{index:02}"));
            // 向自有列表同步添加条目。
            let _ = unsafe {
                // 使用系统 LISTBOX 消息。
                SendMessageW(
                    // 传入自有列表。
                    list,
                    // 发送添加字符串消息。
                    LB_ADDSTRING,
                    // 不使用 wParam。
                    Some(WPARAM::default()),
                    // 传入当前 UTF-16 指针。
                    Some(LPARAM(item.as_ptr() as isize)),
                )
            };
        }
        // 明确以无激活方式显示窗口。
        unsafe {
            // 显示自有顶层窗口。
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        }
        // 初始化消息结构。
        let mut message = MSG::default();
        // 运行标准消息泵。
        loop {
            // 读取下一条消息。
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
            // WM_QUIT 正常结束。
            if result.0 == 0 {
                // 退出消息泵。
                break;
            }
            // -1 表示读取失败。
            if result.0 == -1 {
                // 传播线程最后错误。
                return Err(windows::core::Error::from_thread());
            }
            // 翻译并分发消息。
            unsafe {
                // 保持标准键盘翻译语义。
                let _ = TranslateMessage(&message);
                // 分发到系统窗口过程。
                DispatchMessageW(&message);
            }
        }
        // 消息泵正常结束。
        Ok(())
    }

    // 执行仅供生产门禁使用的 fixture。
    pub(super) fn entry() {
        // 失败时使用非零退出码且不输出敏感诊断。
        if run().is_err() {
            // 返回固定失败码。
            std::process::exit(2);
        }
    }
}

#[cfg(target_os = "windows")]
fn main() {
    windows_fixture::entry();
}
