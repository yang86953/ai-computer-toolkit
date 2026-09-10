//! 把 Linux 进程信号收敛为 Workflow 可轮询的取消事实。

use std::{
    mem::MaybeUninit,
    sync::atomic::{AtomicBool, Ordering},
};

// 保存当前 CLI 或固定 worker 是否收到显式取消信号。
static CANCELLED: AtomicBool = AtomicBool::new(false);

// 信号处理器只写无锁原子，不执行分配、I/O 或 provider 调用。
extern "C" fn signal_handler(_: libc::c_int) {
    request_cancellation();
}

/// 为当前进程发布一次取消请求。
pub(crate) fn request_cancellation() {
    CANCELLED.store(true, Ordering::Release);
}

/// 安装 SIGINT/SIGTERM 处理器，使主 Workflow 有机会取消并回收固定 worker。
pub(crate) fn install_console_handler() {
    CANCELLED.store(false, Ordering::Release);
    // sigaction 结构由 libc 初始化；失败时硬 deadline 和父进程死亡信号仍能回收 worker。
    let mut action = MaybeUninit::<libc::sigaction>::zeroed();
    // SAFETY: zeroed sigaction 随后完整初始化 handler、mask 和 flags，再传给 libc。
    let action = unsafe {
        let pointer = action.as_mut_ptr();
        (*pointer).sa_sigaction = signal_handler as *const () as libc::sighandler_t;
        (*pointer).sa_flags = 0;
        if libc::sigemptyset(&mut (*pointer).sa_mask) != 0 {
            return;
        }
        action.assume_init()
    };
    // SAFETY: action 在当前作用域有效，handler 具有 C ABI 且仅执行原子写。
    unsafe {
        let _ = libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
        let _ = libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut());
    }
}

/// 返回当前进程是否已经收到取消请求。
pub(crate) fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::Acquire)
}

#[cfg(test)]
pub(crate) fn set_cancelled_for_test(value: bool) {
    CANCELLED.store(value, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::{is_cancelled, set_cancelled_for_test};

    #[test]
    fn cancellation_state_round_trips() {
        set_cancelled_for_test(true);
        assert!(is_cancelled());
        set_cancelled_for_test(false);
        assert!(!is_cancelled());
    }
}
