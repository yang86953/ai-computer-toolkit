//! 提供 Desktop Session 长输入使用的窄协作取消令牌。

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// 只表达取消是否已经请求，不携带 broker、线程或平台身份。
#[derive(Clone, Debug, Default)]
pub(crate) struct DesktopInputCancellation {
    requested: Arc<AtomicBool>,
}

impl DesktopInputCancellation {
    /// 创建尚未取消的请求局部令牌。
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 单调请求取消；重复调用保持幂等。
    pub(crate) fn cancel(&self) {
        self.requested.store(true, Ordering::Release);
    }

    /// 在输入 owner 线程读取当前取消事实。
    pub(crate) fn is_cancelled(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CancellationFinalState {
    Active,
    Completed,
    Cancelled,
}

struct CancellationEntry {
    token: DesktopInputCancellation,
    input_seen: bool,
    final_state: CancellationFinalState,
}

/// 表示 cancel 控制请求可公开的封闭状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopInputCancelStatus {
    UnknownRequest,
    CancellationRequested,
    TooLate,
    Cancelled,
}

impl DesktopInputCancelStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownRequest => "unknown-request",
            Self::CancellationRequested => "cancellation-requested",
            Self::TooLate => "too-late",
            Self::Cancelled => "cancelled",
        }
    }
}

/// 在 reader 与唯一输入 owner 间共享有界取消事实，不持有领域或平台对象。
pub(crate) struct DesktopInputCancellationRegistry {
    entries: Mutex<BTreeMap<String, CancellationEntry>>,
    maximum_entries: usize,
}

impl DesktopInputCancellationRegistry {
    pub(crate) fn new(maximum_entries: usize) -> Self {
        Self {
            entries: Mutex::new(BTreeMap::new()),
            maximum_entries,
        }
    }

    pub(crate) fn prepare_input(&self, request_nonce: &str) -> DesktopInputCancellation {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = entries.get_mut(request_nonce) {
            entry.input_seen = true;
            return entry.token.clone();
        }
        let token = DesktopInputCancellation::new();
        if entries.len() < self.maximum_entries {
            entries.insert(
                request_nonce.to_owned(),
                CancellationEntry {
                    token: token.clone(),
                    input_seen: true,
                    final_state: CancellationFinalState::Active,
                },
            );
        }
        token
    }

    pub(crate) fn request_cancel(&self, target_request_nonce: &str) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = entries.get_mut(target_request_nonce) {
            if entry.final_state == CancellationFinalState::Active {
                entry.token.cancel();
            }
            return;
        }
        if entries.len() >= self.maximum_entries {
            return;
        }
        let token = DesktopInputCancellation::new();
        token.cancel();
        entries.insert(
            target_request_nonce.to_owned(),
            CancellationEntry {
                token,
                input_seen: false,
                final_state: CancellationFinalState::Active,
            },
        );
    }

    pub(crate) fn finish(&self, request_nonce: &str, cancelled: bool) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = entries.get_mut(request_nonce) {
            entry.final_state = if cancelled {
                CancellationFinalState::Cancelled
            } else {
                CancellationFinalState::Completed
            };
        }
    }

    pub(crate) fn status(&self, target_request_nonce: &str) -> DesktopInputCancelStatus {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(entry) = entries.get(target_request_nonce) else {
            return DesktopInputCancelStatus::UnknownRequest;
        };
        if !entry.input_seen {
            return DesktopInputCancelStatus::UnknownRequest;
        }
        match entry.final_state {
            CancellationFinalState::Active if entry.token.is_cancelled() => {
                DesktopInputCancelStatus::CancellationRequested
            }
            CancellationFinalState::Active | CancellationFinalState::Completed => {
                DesktopInputCancelStatus::TooLate
            }
            CancellationFinalState::Cancelled => DesktopInputCancelStatus::Cancelled,
        }
    }

    pub(crate) fn cancel_all_active(&self) {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for entry in entries.values() {
            if entry.input_seen && entry.final_state == CancellationFinalState::Active {
                entry.token.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_a_monotonic_cancellation_fact() {
        let token = DesktopInputCancellation::new();
        let observer = token.clone();
        assert!(!observer.is_cancelled());
        token.cancel();
        assert!(observer.is_cancelled());
        observer.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn registry_preserves_pre_cancel_and_terminal_status() {
        let registry = DesktopInputCancellationRegistry::new(4);
        registry.request_cancel("target");
        assert_eq!(
            registry.status("target"),
            DesktopInputCancelStatus::UnknownRequest
        );
        let token = registry.prepare_input("target");
        assert!(token.is_cancelled());
        assert_eq!(
            registry.status("target"),
            DesktopInputCancelStatus::CancellationRequested
        );
        registry.finish("target", true);
        assert_eq!(
            registry.status("target"),
            DesktopInputCancelStatus::Cancelled
        );
    }
}
