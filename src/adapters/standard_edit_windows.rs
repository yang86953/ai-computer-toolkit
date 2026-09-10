//! 标准 Edit 固定消息与有界回读 Windows Adapter。

// 导入固定 Win32 消息调用所需类型和函数。
use windows::Win32::{
    // 导入线程最后错误读取与稳定错误常量。
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_SUCCESS, ERROR_TIMEOUT, GetLastError, HWND, LPARAM,
        SetLastError, WPARAM,
    },
    // 导入固定消息、窗口身份验证和超时标志。
    UI::WindowsAndMessaging::{
        GetClassNameW, GetWindowThreadProcessId, IsWindow, SMTO_ABORTIFHUNG, SMTO_BLOCK,
        SendMessageTimeoutW, WM_GETTEXT, WM_GETTEXTLENGTH, WM_SETTEXT,
    },
};

// 导入私有窗口记录；该类型不得越过 Module 公共边界。
use crate::adapters::windows::WindowRecord;

// 限制一次回读最多接受的 UTF-16 code unit 数。
const MAXIMUM_READBACK_UNITS: usize = 65_536;

// 表示固定消息链的封闭失败分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StandardEditWriteFailure {
    // 表示精确控件已经过期或身份变化。
    Stale,
    // 表示同步消息超过调用方 deadline。
    Timeout,
    // 表示 Windows 明确拒绝固定消息。
    PermissionDenied,
    // 表示固定消息被其他原因拒绝。
    BackgroundUnavailable,
    // 表示回读长度超过认证边界。
    ReadbackTooLong,
    // 表示回读不是有效 UTF-16。
    InvalidReadback,
    // 表示回读内容与请求不一致。
    ReadbackMismatch,
}

// 保存成功写入的最小验证事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StandardEditWriteEvidence {
    // 标记固定 WM_GETTEXT 回读已经逐值匹配。
    pub(crate) verified_by_readback: bool,
}

// 将 HWND 数值转换为私有 Windows 句柄类型。
fn native_window(control: &WindowRecord) -> HWND {
    // 句柄只在 Adapter 内短暂存在。
    HWND(control.hwnd as *mut std::ffi::c_void)
}

// 读取当前窗口类名以重新确认该 token 仍满足固定 Edit provider 分类。
fn current_class_name(window: HWND) -> String {
    // 使用固定容量读取系统类名。
    let mut buffer = [0_u16; 256];
    // 执行只读类名查询。
    let written = unsafe { GetClassNameW(window, &mut buffer) };
    // 查询失败返回空值并由调用方按 stale 拒绝。
    if written <= 0 {
        // 返回空类名。
        return String::new();
    }
    // 将有效 UTF-16 区间解码为 Rust 字符串。
    String::from_utf16_lossy(&buffer[..usize::try_from(written).unwrap_or_default()])
}

// 发送一条认证固定消息并分类同步失败。
fn send_timeout(
    // 接收已验证窗口句柄。
    window: HWND,
    // 接收由 Adapter 内部选择的固定消息号。
    message: u32,
    // 接收由 Adapter 内部构造的 WPARAM。
    word: WPARAM,
    // 接收由 Adapter 内部构造的 LPARAM。
    value: LPARAM,
    // 接收 1..30000ms deadline。
    timeout_ms: u32,
) -> Result<usize, StandardEditWriteFailure> {
    // 清空线程最后错误以区分真实 timeout。
    unsafe { SetLastError(ERROR_SUCCESS) };
    // 保存 Windows 消息返回值。
    let mut message_result = 0_usize;
    // 只使用阻塞且遇到挂起即中止的固定标志。
    let sent = unsafe {
        // 执行同步有界消息。
        SendMessageTimeoutW(
            // 传入精确目标。
            window,
            // 传入内部固定消息。
            message,
            // 传入内部固定或有界参数。
            word,
            // 传入内部构造数据。
            value,
            // 禁止调用方控制消息标志。
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            // 传入已验证 deadline。
            timeout_ms,
            // 接收消息结果。
            Some(&mut message_result),
        )
    };
    // 非零表示消息已在 deadline 内返回。
    if sent.0 != 0 {
        // 返回内部消息结果。
        return Ok(message_result);
    }
    // 读取紧邻调用的线程错误。
    let error = unsafe { GetLastError() };
    // 精确映射 Windows timeout。
    if error == ERROR_TIMEOUT {
        // 返回 outcome-unknown 分类。
        return Err(StandardEditWriteFailure::Timeout);
    }
    // 精确映射访问拒绝。
    if error == ERROR_ACCESS_DENIED {
        // 返回权限拒绝分类。
        return Err(StandardEditWriteFailure::PermissionDenied);
    }
    // 其他零返回均不得猜测目标状态。
    Err(StandardEditWriteFailure::BackgroundUnavailable)
}

// 使用固定 WM_SETTEXT 并通过有界 WM_GETTEXT 链验证结果。
pub(crate) fn write_and_readback(
    // 接收刚由 Module 唯一重新发现的私有控件事实。
    control: &WindowRecord,
    // 接收已验证 UTF-8 文本。
    text: &str,
    // 接收已验证 deadline。
    timeout_ms: u32,
) -> Result<StandardEditWriteEvidence, StandardEditWriteFailure> {
    // 转换私有句柄。
    let window = native_window(control);
    // 目标必须仍是有效窗口且类名严格等于系统 Edit。
    if !unsafe { IsWindow(Some(window)).as_bool() } || current_class_name(window) != "Edit" {
        // 拒绝 stale 或类名复用目标。
        return Err(StandardEditWriteFailure::Stale);
    }
    // 读取当前窗口所属进程。
    let mut process_id = 0_u32;
    // 执行只读身份查询。
    unsafe { GetWindowThreadProcessId(window, Some(&mut process_id)) };
    // PID 必须与刚重新发现的记录一致。
    if process_id != control.process_id {
        // 拒绝当前 token 已转移到其他 PID；完全相同 token 的同进程回收仍受身份停止线约束。
        return Err(StandardEditWriteFailure::Stale);
    }
    // 把合法 Rust UTF-8 编码为 NUL 结尾 UTF-16。
    let mut wide_text = text.encode_utf16().collect::<Vec<_>>();
    // 追加 WM_SETTEXT 所需终止符。
    wide_text.push(0);
    // 发送固定 WM_SETTEXT，拒绝任意消息或参数注入。
    let _ = send_timeout(
        // 传入精确控件。
        window,
        // 固定 mutation 消息。
        WM_SETTEXT,
        // 固定 WPARAM 为零。
        WPARAM(0),
        // 传入本函数拥有的只读 UTF-16 缓冲区。
        LPARAM(wide_text.as_ptr() as isize),
        // 传入有界 deadline。
        timeout_ms,
    )?;
    // 读取固定 WM_GETTEXTLENGTH 结果。
    let length = send_timeout(
        // 使用同一精确控件。
        window,
        // 固定只读长度消息。
        WM_GETTEXTLENGTH,
        // 固定 WPARAM 为零。
        WPARAM(0),
        // 固定 LPARAM 为零。
        LPARAM(0),
        // 复用有界 deadline。
        timeout_ms,
    )?;
    // 回读长度不得超过认证上限。
    if length > MAXIMUM_READBACK_UNITS {
        // 拒绝分配无界缓冲区。
        return Err(StandardEditWriteFailure::ReadbackTooLong);
    }
    // 为回读内容和 NUL 终止符分配有界缓冲区。
    let mut readback = vec![0_u16; length + 1];
    // 将容量转换为 WPARAM；认证上限保证不会溢出。
    let capacity = WPARAM(readback.len());
    // 发送固定 WM_GETTEXT。
    let copied = send_timeout(
        // 使用同一精确控件。
        window,
        // 固定只读内容消息。
        WM_GETTEXT,
        // 只传入本地缓冲区容量。
        capacity,
        // 只传入本函数拥有的可写缓冲区。
        LPARAM(readback.as_mut_ptr() as isize),
        // 复用有界 deadline。
        timeout_ms,
    )?;
    // 返回长度不得越过已分配缓冲区。
    if copied > length {
        // 将异常返回分类为无效回读。
        return Err(StandardEditWriteFailure::InvalidReadback);
    }
    // 只解码消息明确报告的 code unit。
    readback.truncate(copied);
    // 严格解码 UTF-16，禁止替换字符掩盖差异。
    let decoded = String::from_utf16(&readback)
        // 将无效 UTF-16 映射为封闭失败。
        .map_err(|_| StandardEditWriteFailure::InvalidReadback)?;
    // 回读必须与原 UTF-8 文本逐值相同。
    if decoded != text {
        // 返回验证不一致。
        return Err(StandardEditWriteFailure::ReadbackMismatch);
    }
    // 返回最小成功证据。
    Ok(StandardEditWriteEvidence {
        // 标记已完成固定回读比较。
        verified_by_readback: true,
    })
}
