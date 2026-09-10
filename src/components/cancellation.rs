//! 把控制台取消事件收敛为进程内只读信号。

// 导入无锁取消状态。
use std::sync::atomic::{AtomicBool, Ordering};

// 导入 Windows 控制台事件注册接口。
use windows::{
    // 导入 Win32 控制台事件常量与注册函数。
    Win32::System::Console::{
        // 导入可取消事件常量。
        CTRL_BREAK_EVENT,
        CTRL_C_EVENT,
        // 导入控制台事件处理器安装函数。
        SetConsoleCtrlHandler,
    },
    // 导入回调返回布尔类型。
    core::BOOL,
};

// 保存当前 CLI 调用是否收到取消事件。
static CANCELLED: AtomicBool = AtomicBool::new(false);

// 接收 Windows 控制台事件并只更新原子状态。
unsafe extern "system" fn console_handler(event: u32) -> BOOL {
    // 只消费用户明确的 Ctrl+C 与 Ctrl+Break。
    if matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
        // 通过唯一写入口发布取消请求。
        request_cancellation();
        // 告知系统该事件已由本进程处理。
        return BOOL::from(true);
    }
    // 其他关闭类事件交回默认处理器。
    BOOL::from(false)
}

// 为已认证的进程内控制通道发布取消请求。
pub(crate) fn request_cancellation() {
    // 以 Release 顺序发布取消事实。
    CANCELLED.store(true, Ordering::Release);
}

// 为当前 CLI 进程安装一次轻量取消处理器。
pub(crate) fn install_console_handler() {
    // 新一次 CLI 调用从未取消状态开始。
    CANCELLED.store(false, Ordering::Release);
    // 安装失败不影响普通命令；worker deadline 仍保持强制回收。
    let _ = unsafe { SetConsoleCtrlHandler(Some(console_handler), true) };
}

// 返回当前调用是否已收到取消请求。
pub(crate) fn is_cancelled() -> bool {
    // 以 Acquire 顺序观察控制台线程发布的状态。
    CANCELLED.load(Ordering::Acquire)
}

// 仅供 Component 测试注入确定性取消状态。
#[cfg(test)]
pub(crate) fn set_cancelled_for_test(value: bool) {
    // 测试与生产路径共享同一原子边界。
    CANCELLED.store(value, Ordering::Release);
}

// 验证取消 Component 不需要锁或 provider 状态。
#[cfg(test)]
mod tests {
    // 导入待测状态接口。
    use super::*;

    // 验证测试注入能够被轮询读取。
    #[test]
    fn cancellation_state_round_trips() {
        // 设置取消状态。
        set_cancelled_for_test(true);
        // 断言读取到已取消。
        assert!(is_cancelled());
        // 恢复状态，避免污染同进程其他测试。
        set_cancelled_for_test(false);
        // 断言状态已经清除。
        assert!(!is_cancelled());
    }
}
