//! 封装窗口生命周期同步 Command 的单调 deadline 与可注入取消轮询。

// 导入短轮询等待与单调时钟。
use std::{
    // 导入线程短等待。
    thread,
    // 导入单调时间类型。
    time::{Duration, Instant},
};

// 固定取消轮询最大间隔。
const INTERRUPT_POLL_INTERVAL: Duration = Duration::from_millis(10);

// 表示同步 Command 当前中断原因。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowLifecycleInterrupt {
    // 表示调用方请求取消。
    Cancelled,
    // 表示全局 deadline 已到达。
    TimedOut,
}

// 封装一次同步窗口生命周期 Command 的中断生命周期。
pub(super) struct InterruptDeadline {
    // 保存整个 Command 的单调截止时刻。
    deadline: Instant,
    // 保存生产取消信号或测试专用确定性探针。
    cancelled: fn() -> bool,
}

// 提供中断检查与受约束短等待。
impl InterruptDeadline {
    // 从调用起点和已验证毫秒上限构造控制器。
    pub(super) fn new(started: Instant, timeout_ms: u32, cancelled: fn() -> bool) -> Self {
        // 计算单调 deadline。
        let deadline = started
            // 使用已验证毫秒值。
            .checked_add(Duration::from_millis(u64::from(timeout_ms)))
            // 理论溢出时立即到期。
            .unwrap_or(started);
        // 返回只读生命周期控制器。
        Self {
            // 保存单调 deadline。
            deadline,
            // 保存无状态取消探针。
            cancelled,
        }
    }

    // 检查取消与 deadline，且让用户取消优先。
    pub(super) fn check(&self) -> Result<(), WindowLifecycleInterrupt> {
        // 调用封闭无状态取消探针。
        if (self.cancelled)() {
            // 返回取消原因。
            return Err(WindowLifecycleInterrupt::Cancelled);
        }
        // 到达 deadline 时禁止下一项平台动作。
        if Instant::now() >= self.deadline {
            // 返回超时原因。
            return Err(WindowLifecycleInterrupt::TimedOut);
        }
        // 允许继续。
        Ok(())
    }

    // 执行可取消且受全局 deadline 约束的短等待。
    pub(super) fn pause(&self, duration: Duration) -> Result<(), WindowLifecycleInterrupt> {
        // 零等待仍检查一次取消与 deadline。
        if duration.is_zero() {
            // 返回即时检查结果。
            return self.check();
        }
        // 计算本次等待目标时刻。
        let target = Instant::now()
            // 加上调用者请求的有界等待。
            .checked_add(duration)
            // 理论溢出时收敛到全局 deadline。
            .unwrap_or(self.deadline);
        // 轮询直到等待完成。
        loop {
            // 每个轮询先检查取消和全局 deadline。
            self.check()?;
            // 读取当前单调时刻。
            let now = Instant::now();
            // 达到本次等待目标即完成。
            if now >= target {
                // 返回成功。
                return Ok(());
            }
            // 计算不超过本次目标的剩余时间。
            let until_target = target.saturating_duration_since(now);
            // 计算不超过全局 deadline 的剩余时间。
            let until_deadline = self.deadline.saturating_duration_since(now);
            // 选择短轮询切片。
            let slice = INTERRUPT_POLL_INTERVAL
                // 不超过本次等待目标。
                .min(until_target)
                // 不超过全局 deadline。
                .min(until_deadline);
            // 零切片由下一轮统一报告 timeout。
            if slice.is_zero() {
                // 继续进入检查。
                continue;
            }
            // 仅阻塞一个短轮询切片。
            thread::sleep(slice);
        }
    }
}
