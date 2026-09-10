//! 提供 Desktop Session 输入执行期间的中立会话存活门禁。

/// 输入 Adapter 可消费的窄存活失败，不携带 D-Bus 或平台身份。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSessionInputLivenessFailure {
    code: &'static str,
    stage: &'static str,
}

impl DesktopSessionInputLivenessFailure {
    pub(crate) const fn new(code: &'static str, stage: &'static str) -> Self {
        Self { code, stage }
    }

    pub(crate) const fn code(self) -> &'static str {
        self.code
    }

    pub(crate) const fn stage(self) -> &'static str {
        self.stage
    }
}

/// 由 lease 私有实现、并在每个输入 effect 前轮询的窄端口。
pub(crate) trait DesktopSessionInputLiveness {
    fn poll_input_allowed(&mut self) -> Result<(), DesktopSessionInputLivenessFailure>;
}

/// 保存公开登录会话接口投影出的活动、锁定提示与可读取事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopLoginSessionActivity {
    active: bool,
    locked: bool,
    lock_state_available: bool,
}

impl DesktopLoginSessionActivity {
    pub(crate) const fn new(active: bool, locked: bool, lock_state_available: bool) -> Self {
        Self {
            active,
            locked,
            lock_state_available,
        }
    }

    pub(crate) fn update_active(&mut self, active: bool) {
        self.active = active;
    }

    pub(crate) fn update_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    pub(crate) fn update_lock_state_available(&mut self, available: bool) {
        self.lock_state_available = available;
    }

    pub(crate) const fn ensure_input_allowed(
        self,
    ) -> Result<(), DesktopSessionInputLivenessFailure> {
        if !self.lock_state_available {
            return Err(DesktopSessionInputLivenessFailure::new(
                "HOST_SESSION_STATE_UNAVAILABLE",
                "host-session-liveness",
            ));
        }
        if self.locked {
            return Err(DesktopSessionInputLivenessFailure::new(
                "HOST_SESSION_LOCKED",
                "host-session-liveness",
            ));
        }
        if !self.active {
            return Err(DesktopSessionInputLivenessFailure::new(
                "HOST_SESSION_INACTIVE",
                "host-session-liveness",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_active_unlocked_observable_session_allows_input() {
        assert!(
            DesktopLoginSessionActivity::new(true, false, true)
                .ensure_input_allowed()
                .is_ok()
        );
        assert_eq!(
            DesktopLoginSessionActivity::new(false, false, true).ensure_input_allowed(),
            Err(DesktopSessionInputLivenessFailure::new(
                "HOST_SESSION_INACTIVE",
                "host-session-liveness",
            ))
        );
        assert_eq!(
            DesktopLoginSessionActivity::new(true, true, true).ensure_input_allowed(),
            Err(DesktopSessionInputLivenessFailure::new(
                "HOST_SESSION_LOCKED",
                "host-session-liveness",
            ))
        );
        assert_eq!(
            DesktopLoginSessionActivity::new(true, false, false).ensure_input_allowed(),
            Err(DesktopSessionInputLivenessFailure::new(
                "HOST_SESSION_STATE_UNAVAILABLE",
                "host-session-liveness",
            ))
        );
    }

    #[test]
    fn activity_updates_are_monotonic_observations_not_hidden_retries() {
        let mut activity = DesktopLoginSessionActivity::new(true, false, true);
        activity.update_locked(true);
        assert_eq!(
            activity
                .ensure_input_allowed()
                .map_err(|failure| failure.code()),
            Err("HOST_SESSION_LOCKED")
        );
        activity.update_locked(false);
        activity.update_active(false);
        assert_eq!(
            activity
                .ensure_input_allowed()
                .map_err(|failure| failure.code()),
            Err("HOST_SESSION_INACTIVE")
        );
        activity.update_active(true);
        activity.update_lock_state_available(false);
        assert_eq!(
            activity
                .ensure_input_allowed()
                .map_err(|failure| failure.code()),
            Err("HOST_SESSION_STATE_UNAVAILABLE")
        );
    }
}
