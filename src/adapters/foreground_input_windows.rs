//! 封装键鼠前景输入共享的精确 Windows 窗口生命周期调用。

// 导入 Win32 窗口句柄与前景生命周期接口。
use windows::Win32::{
    // 导入强类型窗口句柄。
    Foundation::HWND,
    // 导入窗口存在性、可见性、恢复与激活调用。
    UI::WindowsAndMessaging::{
        // 导入窗口状态读取。
        IsIconic,
        IsWindow,
        IsWindowVisible,
        // 导入恢复显示命令。
        SW_RESTORE,
        // 导入前景激活请求。
        SetForegroundWindow,
        // 导入异步窗口恢复调用。
        ShowWindowAsync,
    },
};

// 引入当前 Component 私有封闭错误类型。
#[path = "foreground_input_windows_error.rs"]
mod error_code;

// 导入当前 Component 私有错误集合。
use error_code::ForegroundInputWindowsErrorCode;

// 导入私有窗口事实与统一结果。
use crate::{
    // 导入 Windows backend 私有窗口记录。
    adapters::windows::WindowRecord,
    // 导入统一结果类型。
    domain::AppResult,
};

// 把私有整数句柄恢复为 Win32 类型。
fn native_window(window: &WindowRecord) -> HWND {
    // 句柄只在 Windows Component 内存在。
    HWND(window.hwnd as *mut std::ffi::c_void)
}

// 验证精确窗口仍存在并返回私有句柄。
pub(crate) fn ensure_window_exists(window: &WindowRecord) -> AppResult<HWND> {
    // 恢复私有窗口句柄。
    let native = native_window(window);
    // 使用时必须重新核对窗口存在。
    if !unsafe { IsWindow(Some(native)) }.as_bool() {
        // 返回 canonical 目标过期。
        return Err(ForegroundInputWindowsErrorCode::StaleSession
            .error("The exact foreground input target is no longer a window."));
    }
    // 返回认证句柄。
    Ok(native)
}

// 如有必要恢复当前精确窗口。
pub(crate) fn restore_window(window: &WindowRecord) -> AppResult<bool> {
    // 核对句柄仍存在。
    let native = ensure_window_exists(window)?;
    // 读取当前最小化或不可见状态。
    let needs_restore = unsafe { IsIconic(native) }.as_bool()
        // 不可见目标同样需要显式恢复。
        || !unsafe { IsWindowVisible(native) }.as_bool();
    // 已可交互时无需产生额外影响。
    if !needs_restore {
        // 返回未恢复证据。
        return Ok(false);
    }
    // 仅由已确认且已同意前景影响的 Module 调用。
    let _ = unsafe { ShowWindowAsync(native, SW_RESTORE) };
    // 返回已请求恢复证据。
    Ok(true)
}

// 尝试把精确窗口置为前景并立即核对结果。
pub(crate) fn try_activate_window(window: &WindowRecord) -> AppResult<bool> {
    // 核对句柄仍存在。
    let native = ensure_window_exists(window)?;
    // 已是前景时直接成功。
    if foreground_matches(window) {
        // 返回成功。
        return Ok(true);
    }
    // 请求 Windows 激活目标。
    let _requested = unsafe { SetForegroundWindow(native) }.as_bool();
    // 以实际前景事实而非 API 返回值为准。
    Ok(foreground_matches(window))
}

// 判断当前前景窗口是否仍是精确目标。
pub(crate) fn foreground_matches(window: &WindowRecord) -> bool {
    // 只比较 Adapter 私有句柄事实。
    crate::adapters::windows::foreground_hwnd() == window.hwnd
}
