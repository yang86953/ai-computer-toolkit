//! 键鼠输入共用的单调期限和协作取消等待。
use crate::domain::{AppControlError, AppResult};
use std::{
    thread,
    time::{Duration, Instant},
};
pub(crate) struct InterruptDeadline<'a> {
    deadline: Instant,
    cancelled: Box<dyn Fn() -> bool + 'a>,
}
impl<'a> InterruptDeadline<'a> {
    pub(crate) fn new(
        started: Instant,
        timeout_ms: u32,
        cancelled: impl Fn() -> bool + 'a,
    ) -> Self {
        Self {
            deadline: started
                .checked_add(Duration::from_millis(u64::from(timeout_ms)))
                .unwrap_or(started),
            cancelled: Box::new(cancelled),
        }
    }
    pub(crate) fn check(&self) -> AppResult<()> {
        if (self.cancelled)() {
            return Err(AppControlError::new(
                "CANCELLED",
                "Input was cancelled before the next platform effect.",
            ));
        }
        if Instant::now() >= self.deadline {
            return Err(AppControlError::new(
                "TIMEOUT",
                "Input exceeded its synchronous deadline.",
            ));
        }
        Ok(())
    }
    pub(crate) fn pause(&self, duration: Duration) -> AppResult<()> {
        let until = Instant::now()
            .checked_add(duration)
            .unwrap_or(self.deadline);
        loop {
            self.check()?;
            let now = Instant::now();
            if now >= until {
                return Ok(());
            }
            thread::sleep(
                until
                    .saturating_duration_since(now)
                    .min(Duration::from_millis(5)),
            );
        }
    }
}
