//! Linux 协作式 UIX 窗口与可访问性 surface Adapter。

use serde_json::{Value, json};

use crate::{
    adapters::AppAdapter,
    capabilities,
    domain::{AppControlError, AppResult, CommandRequest, IsolationRequirement},
    modules::uix_window,
};

/// 只拥有 UIX 协作式窗口发现和元数据读取路由。
pub struct WindowAdapter;

/// 只拥有 UIX 协作式语义树读取路由。
pub struct AccessibilityAdapter;

fn unavailable(surface: &'static str) -> AppControlError {
    AppControlError::with_details(
        "CAPABILITY_UNAVAILABLE",
        "The requested UIX Agent surface operation is unavailable.",
        json!({
            "platform": "linux",
            "surface": surface,
            "provider": "uix-agent-v1",
            "executionRealm": "none",
            "fallback": "none",
            "foregroundActivationAllowed": false,
            "inputAllowed": false,
        }),
    )
}

fn required_session_id(request: &CommandRequest) -> AppResult<&str> {
    request
        .target
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppControlError::new("INVALID_ARGUMENT", "target.sessionId is required."))
}

fn reject_strict_isolation(request: &CommandRequest) -> AppResult<()> {
    if request.isolation_requirement == IsolationRequirement::Strict {
        return Err(AppControlError::with_details(
            "ISOLATION_REQUIRED",
            "UIX Agent requests execute in the same session and cannot satisfy strict isolation.",
            json!({
                "platform": "linux",
                "provider": "uix-agent-v1",
                "executionRealm": "same-session-no-focus",
                "fallback": "none",
            }),
        ));
    }
    Ok(())
}

impl AppAdapter for WindowAdapter {
    fn app_id(&self) -> &'static str {
        "window"
    }

    fn status(&self) -> AppResult<Value> {
        Ok(json!({
            "ok": true,
            "app": self.app_id(),
            "platform": "linux",
            "provider": "uix-agent-v1",
            "providerState": "available-for-opt-in-uix-applications-wayland-verified",
            "executionDomain": "same-session-no-focus",
            "readOnly": true,
            "connected": false,
            "metadataRead": false,
            "globalWindowDirectory": false,
            "arbitraryThirdPartyApplications": false,
            "inputAllowed": false,
            "fallback": "none",
            "capabilities": [
                capabilities::WINDOW_DISCOVER_V3,
                capabilities::WINDOW_METADATA_READ_V3,
            ],
        }))
    }

    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        reject_strict_isolation(request)?;
        if !request.target.is_empty() || !request.args.is_empty() {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "UIX window discovery accepts only --max-items and --pretty.",
            ));
        }
        uix_window::discover(request.max_items)
    }

    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        reject_strict_isolation(request)?;
        if request.target.len() != 1 || !request.args.is_empty() {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "UIX window metadata accepts only target.sessionId and --pretty.",
            ));
        }
        uix_window::metadata(required_session_id(request)?)
    }

    fn run(&self, _: &CommandRequest) -> AppResult<Value> {
        Err(unavailable(self.app_id()))
    }
}

impl AppAdapter for AccessibilityAdapter {
    fn app_id(&self) -> &'static str {
        "accessibility"
    }

    fn status(&self) -> AppResult<Value> {
        Ok(json!({
            "ok": true,
            "app": self.app_id(),
            "platform": "linux",
            "provider": "uix-agent-v1",
            "providerState": "available-for-opt-in-uix-applications-wayland-verified",
            "executionDomain": "same-session-no-focus",
            "readOnly": true,
            "connected": false,
            "metadataRead": false,
            "sensitiveSnapshotRead": false,
            "boundsExposed": false,
            "inputAllowed": false,
            "fallback": "none",
            "capabilities": [capabilities::ACCESSIBILITY_TREE_READ_V3],
        }))
    }

    fn sessions(&self, _: &CommandRequest) -> AppResult<Value> {
        Err(unavailable(self.app_id()))
    }

    fn inspect(&self, _: &CommandRequest) -> AppResult<Value> {
        Err(unavailable(self.app_id()))
    }

    fn run(&self, request: &CommandRequest) -> AppResult<Value> {
        reject_strict_isolation(request)?;
        if request.operation.as_deref() != Some("inspect-tree")
            || request.target.len() != 1
            || !request.args.is_empty()
        {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "UIX accessibility only accepts inspect-tree with target.sessionId and bounds.",
            ));
        }
        uix_window::read_tree(
            required_session_id(request)?,
            request.max_depth,
            request.max_items,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_does_not_scan_or_connect_to_application_endpoints() {
        let status = WindowAdapter
            .status()
            .expect("status must be local metadata");
        assert_eq!(status["connected"], false);
        assert_eq!(status["metadataRead"], false);
        assert_eq!(status["globalWindowDirectory"], false);
    }

    #[test]
    fn accessibility_rejects_non_tree_operations() {
        let request = CommandRequest::read(crate::domain::Verb::Run, "accessibility");
        let error = AccessibilityAdapter
            .run(&request)
            .expect_err("unknown operation must fail closed");
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }
}
