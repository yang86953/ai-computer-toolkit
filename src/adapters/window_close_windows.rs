//! 精确窗口固定 `WM_CLOSE` 与有界等待 Windows Adapter。

// 导入轮询所需线程与单调时钟。
use std::{
    // 使用短暂休眠避免忙等。
    thread,
    // 使用单调时钟实现调用方 deadline。
    time::{Duration, Instant},
};

// 导入固定系统消息、窗口身份核对和错误分类所需 Win32 API。
use windows::Win32::{
    // 导入最后错误与消息参数。
    Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS, GetLastError, LPARAM, SetLastError, WPARAM},
    // 只使用固定 WM_CLOSE。
    UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE},
};

// 导入只在 Adapter 内使用的共享身份 Component 与窗口事实。
use crate::adapters::{
    // 复用窗口 mutation 的精确代际核对。
    window_identity_windows::{native_window, same_window_identity},
    // 只接受重新发现的私有窗口记录。
    windows::WindowRecord,
};

// 表示精确窗口关闭的封闭平台失败分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowCloseFailure {
    // 目标句柄、进程或代际已经变化。
    Stale,
    // Windows 明确拒绝固定消息。
    PermissionDenied,
    // 固定消息因其他平台错误未排队。
    OperationFailed,
    // 消息已排队但目标未在 deadline 内消失。
    Timeout,
}

// 保存关闭成功的最小平台证据。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowCloseEvidence {
    // 目标的同一进程代际窗口已经失效。
    pub(crate) closed: bool,
}

// 只发送固定 WM_CLOSE，并等待同一窗口身份有界失效。
pub(crate) fn close(
    // 接收 Module 已唯一解析的私有目标。
    target: &WindowRecord,
    // 接收已验证的 1..30000ms deadline。
    timeout_ms: u32,
) -> Result<WindowCloseEvidence, WindowCloseFailure> {
    // 写前再次核对窗口身份。
    if !same_window_identity(target) {
        // 禁止向复用句柄发送关闭消息。
        return Err(WindowCloseFailure::Stale);
    }
    // 恢复私有窗口句柄。
    let window = native_window(target);
    // 清空线程最后错误以区分 UIPI 拒绝。
    unsafe { SetLastError(ERROR_SUCCESS) };
    // 只发送系统固定 WM_CLOSE，不接受任意消息或输入。
    if unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_err() {
        // 读取紧邻失败调用的 Win32 错误。
        let error = unsafe { GetLastError() };
        // 明确权限拒绝使用稳定分类。
        if error == ERROR_ACCESS_DENIED {
            // 返回权限失败且不公开 native 事实。
            return Err(WindowCloseFailure::PermissionDenied);
        }
        // 其他排队失败保持通用执行错误。
        return Err(WindowCloseFailure::OperationFailed);
    }
    // 从消息成功排队后开始计算 deadline。
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    // 轮询同一窗口身份是否仍存在。
    while same_window_identity(target) {
        // 到达 deadline 后结果未知，因为消息可能稍后处理。
        if Instant::now() >= deadline {
            // 返回封闭超时分类。
            return Err(WindowCloseFailure::Timeout);
        }
        // 使用与 C++ 参考一致的短轮询间隔。
        thread::sleep(Duration::from_millis(25));
    }
    // 返回目标身份已经失效的最小证据。
    Ok(WindowCloseEvidence { closed: true })
}
