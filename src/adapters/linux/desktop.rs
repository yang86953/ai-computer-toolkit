//! Linux `desktop` surface 的 Wayland-only 就绪度 Adapter。

use serde_json::{Value, json};

use crate::{
    adapters::{AppAdapter, linux::wayland_portal},
    capabilities,
    domain::{AppControlError, AppResult, CommandRequest, IsolationRequirement},
    modules::portal_screenshot,
};

/// 只发布标准接口就绪度，不拥有窗口、截图或输入能力。
pub struct DesktopAdapter;

fn unavailable() -> AppControlError {
    AppControlError::with_details(
        "CAPABILITY_UNAVAILABLE",
        "The Linux desktop surface has no authorized Wayland operation provider.",
        json!({
            "platform": "linux",
            "surface": "desktop",
            "executionRealm": "none",
            "fallback": "none",
            "desktopProtocol": "wayland-only",
            "foregroundActivationAllowed": false,
            "inputAllowed": false,
            "portalRequestIssued": false,
            "persistentSessionLauncher": "ai-computer-toolkit session-host desktop",
        }),
    )
}

impl AppAdapter for DesktopAdapter {
    fn app_id(&self) -> &'static str {
        "desktop"
    }

    fn status(&self) -> AppResult<Value> {
        let facts = wayland_portal::probe();
        let screenshot_ready = facts.wayland_socket_count > 0
            && facts.user_bus_socket_present
            && facts.user_bus_reachable
            && facts.portal_service_registered
            && facts.screenshot_version.is_some_and(|version| version >= 2);
        let desktop_session_ready = facts.wayland_socket_count > 0
            && facts.user_bus_socket_present
            && facts.user_bus_reachable
            && facts.portal_service_registered
            && facts
                .remote_desktop_version
                .is_some_and(|version| version >= 2)
            && facts
                .screen_cast_version
                .is_some_and(|version| version >= 5);
        let mut capabilities = Vec::new();
        if screenshot_ready {
            capabilities.push(json!({
                "id": capabilities::DESKTOP_SCREENSHOT_INTERACTIVE,
                "availability": "available-confirmation-and-foreground-consent",
                "targetKind": "host",
                "executionRealm": "host-foreground",
                "fallback": "none",
            }));
        }
        Ok(json!({
            "ok": true,
            "app": self.app_id(),
            "platform": "linux",
            "provider": "wayland-portal-readiness-probe",
            "readOnly": true,
            "executionDomain": "host-headless",
            "capabilityState": if screenshot_ready {
                "portal-interactive-screenshot-ready"
            } else {
                "no-desktop-operation-provider"
            },
            "wayland": {
                "runtimeSocketPresent": facts.wayland_socket_count > 0,
                "runtimeSocketCount": facts.wayland_socket_count,
                "connectionAttempted": false,
                "compositorPrivateProtocolUsed": false,
            },
            "portal": {
                "userBusSocketPresent": facts.user_bus_socket_present,
                "userBusReachable": facts.user_bus_reachable,
                "desktopServiceRegistered": facts.portal_service_registered,
                "autoStartAllowed": false,
                "requestIssued": false,
                "permissionPrompted": false,
                "interfaces": {
                    "screenCast": {
                        "advertised": facts.screen_cast_version.is_some(),
                        "version": facts.screen_cast_version,
                    },
                    "remoteDesktop": {
                        "advertised": facts.remote_desktop_version.is_some(),
                        "version": facts.remote_desktop_version,
                    },
                    "screenshot": {
                        "advertised": facts.screenshot_version.is_some(),
                        "version": facts.screenshot_version,
                    },
                },
                "desktopSessionCandidate": {
                    "prerequisitesReady": desktop_session_ready,
                    "productionRouteWired": true,
                    "liveAcceptancePassed": false,
                    "launcher": "ai-computer-toolkit session-host desktop",
                    "transport": "json-lines-stdio",
                    "advertisedAsAvailable": false,
                },
            },
            "policy": {
                "desktopProtocol": "wayland-only",
                "x11Supported": false,
                "xwaylandFallback": false,
                "fallback": "none",
                "foregroundActivationAllowed": false,
                "inputAllowed": false,
            },
            "capabilities": capabilities,
        }))
    }

    fn sessions(&self, _: &CommandRequest) -> AppResult<Value> {
        Err(unavailable())
    }

    fn inspect(&self, _: &CommandRequest) -> AppResult<Value> {
        Err(unavailable())
    }

    fn run(&self, request: &CommandRequest) -> AppResult<Value> {
        if request.operation.as_deref() != Some("screenshot-interactive") {
            return Err(unavailable());
        }
        if request.isolation_requirement == IsolationRequirement::Strict {
            return Err(AppControlError::with_details(
                "ISOLATION_REQUIRED",
                "Interactive Portal screenshot cannot satisfy strict isolation.",
                json!({
                    "platform": "linux",
                    "provider": "xdg-desktop-portal-screenshot",
                    "executionRealm": "host-foreground",
                    "fallback": "none",
                }),
            ));
        }
        let session_id = request
            .target
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppControlError::new(
                    "INVALID_ARGUMENT",
                    "Interactive desktop screenshot requires target sessionId.",
                )
            })?;
        portal_screenshot::screenshot(
            session_id,
            request.confirmed,
            request.foreground_consent,
            &Value::Object(request.args.clone()),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{adapters::AppAdapter, domain::CommandRequest};

    use super::DesktopAdapter;

    #[test]
    fn unsupported_desktop_operations_fail_closed_after_probe_status() {
        let result = DesktopAdapter.sessions(&CommandRequest::read(
            crate::domain::Verb::Sessions,
            "desktop",
        ));
        let error = match result {
            Ok(_) => panic!("fixture operation must remain unavailable"),
            Err(error) => error,
        };
        assert_eq!(error.code, "CAPABILITY_UNAVAILABLE");
        assert_eq!(error.details["executionRealm"], "none");
        assert_eq!(error.details["fallback"], "none");
        assert_eq!(error.details["portalRequestIssued"], false);
    }
}
