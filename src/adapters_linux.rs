//! Linux 平台 Adapter 组合边界。

use std::{collections::HashMap, sync::Arc};

use serde_json::Value;

use crate::domain::{AppControlError, AppResult, CommandRequest};

#[path = "adapters/linux/mod.rs"]
pub(crate) mod linux;

#[path = "adapters/linux/uix_app_descriptor.rs"]
pub(crate) mod uix_app_descriptor;

pub use linux::{
    AccessibilityAdapter, AppFacadeAdapter, DesktopAdapter, ProcessAdapter, WindowAdapter,
};

/// System 面向所有平台 provider 的中立调用契约。
pub trait AppAdapter: Send + Sync {
    fn app_id(&self) -> &'static str;
    fn status(&self) -> AppResult<Value>;
    fn sessions(&self, request: &CommandRequest) -> AppResult<Value>;
    fn inspect(&self, request: &CommandRequest) -> AppResult<Value>;
    fn run(&self, request: &CommandRequest) -> AppResult<Value>;
}

/// 当前平台没有 provider 时使用的显式失败 Adapter。
struct UnavailableAdapter {
    app_id: &'static str,
}

impl UnavailableAdapter {
    fn error(&self) -> AppControlError {
        AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The requested surface has no certified Linux provider.",
            serde_json::json!({
                "platform": "linux",
                "surface": self.app_id,
                "executionRealm": "none",
                "fallback": "none",
                "foregroundActivationAllowed": false,
                "inputAllowed": false,
            }),
        )
    }
}

impl AppAdapter for UnavailableAdapter {
    fn app_id(&self) -> &'static str {
        self.app_id
    }

    fn status(&self) -> AppResult<Value> {
        Err(self.error())
    }

    fn sessions(&self, _: &CommandRequest) -> AppResult<Value> {
        Err(self.error())
    }

    fn inspect(&self, _: &CommandRequest) -> AppResult<Value> {
        Err(self.error())
    }

    fn run(&self, _: &CommandRequest) -> AppResult<Value> {
        Err(self.error())
    }
}

/// System 唯一持有的 provider 注册表。
pub struct AdapterRegistry {
    adapters: HashMap<&'static str, Arc<dyn AppAdapter>>,
}

impl AdapterRegistry {
    pub fn adaptive() -> Self {
        let mut adapters: Vec<Arc<dyn AppAdapter>> = vec![
            Arc::new(AppFacadeAdapter),
            Arc::new(ProcessAdapter),
            Arc::new(DesktopAdapter),
            Arc::new(WindowAdapter),
            Arc::new(AccessibilityAdapter),
        ];
        for app_id in [
            "uia",
            "browser",
            "notepad",
            "win32-control",
            "media",
            "media-session",
        ] {
            adapters.push(Arc::new(UnavailableAdapter { app_id }));
        }
        Self {
            adapters: adapters
                .into_iter()
                .map(|adapter| (adapter.app_id(), adapter))
                .collect(),
        }
    }

    pub fn get(&self, app_id: &str) -> AppResult<&Arc<dyn AppAdapter>> {
        self.adapters.get(app_id).ok_or_else(|| {
            AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The requested surface has no certified Linux provider.",
                serde_json::json!({
                    "platform": "linux",
                    "surface": app_id,
                    "executionRealm": "none",
                    "fallback": "none",
                }),
            )
        })
    }
}
