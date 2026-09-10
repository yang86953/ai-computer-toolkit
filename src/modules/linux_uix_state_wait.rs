//! 协调 UIX framework-current 状态条件等待与只读事实投影。

use serde_json::json;

use crate::{
    adapters::linux::uix_agent::{self, Failure, WindowStateWaitObservation},
    capabilities,
    components::uix_window_state_wait_contract::UixWindowStateWaitInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

trait UixWindowStateWaitPort {
    fn wait(
        &self,
        session_id: &str,
        input: &UixWindowStateWaitInput,
    ) -> Result<WindowStateWaitObservation, Failure>;
}

struct SystemUixWindowStateWait;

impl UixWindowStateWaitPort for SystemUixWindowStateWait {
    fn wait(
        &self,
        session_id: &str,
        input: &UixWindowStateWaitInput,
    ) -> Result<WindowStateWaitObservation, Failure> {
        uix_agent::perform_window_state_wait(session_id, input)
    }
}

/// 在同一认证连接轮询精确窗口的框架当前状态，且不请求确认或前景影响。
pub(crate) fn wait(session_id: &str, value: &serde_json::Value) -> AppResult<serde_json::Value> {
    wait_with(&SystemUixWindowStateWait, session_id, value)
}

fn wait_with(
    port: &impl UixWindowStateWaitPort,
    session_id: &str,
    value: &serde_json::Value,
) -> AppResult<serde_json::Value> {
    let input = UixWindowStateWaitInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let observation = port.wait(session_id, &input).map_err(wait_error)?;
    Ok(json!({
        "ok": true,
        "contractVersion": "act/window-state-wait/v1",
        "capability": capabilities::WINDOW_STATE_WAIT,
        "targetId": session_id,
        "observationSource": "uix-framework-current",
        "condition": input.public_condition(),
        "coordinateSpace": "client-logical-px",
        "visible": observation.visible,
        "presentable": observation.presentable,
        "focused": observation.focused,
        "clientSize": {
            "width": observation.logical_width,
            "height": observation.logical_height,
        },
        "windowState": {
            "maximized": observation.maximized,
            "minimized": observation.minimized,
            "fullscreen": observation.fullscreen,
        },
        "revision": observation.revision,
        "presentedRevision": observation.presented_revision,
        "pollCount": observation.poll_count,
        "frameworkConditionMatched": true,
        "compositorFinalStateConfirmed": false,
        "targetReResolved": true,
        "sameAuthenticatedConnectionUsed": true,
        "providerPolling": true,
        "waitProtocolUsed": false,
        "pollIntervalMs": input.poll_interval_ms(),
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "retrySafe": true,
        "safety": {
            "provider": "uix-agent-v1",
            "providerCache": "disabled",
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "foregroundActivationRequested": false,
            "desktopInputInjected": false,
            "desktopPointerMoved": false,
            "globalWindowStateClaimed": false,
            "sameAuthenticatedConnectionUsed": true,
            "providerPolling": true,
            "waitProtocolUsed": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

fn wait_error(failure: Failure) -> AppControlError {
    match failure {
        Failure::Unavailable => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The target UIX Agent did not negotiate complete window state wait fields.",
            json!({
                "provider": "uix-agent-v1",
                "requiredNegotiation": [
                    "logical_width",
                    "logical_height",
                    "maximized",
                    "minimized",
                    "fullscreen",
                    "focused"
                ],
                "executionRealm": "none",
                "retrySafe": true,
                "fallback": "none",
            }),
        ),
        other => uix_window::public_error(other),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    struct FixturePort {
        result: Result<WindowStateWaitObservation, Failure>,
    }

    impl UixWindowStateWaitPort for FixturePort {
        fn wait(
            &self,
            _: &str,
            _: &UixWindowStateWaitInput,
        ) -> Result<WindowStateWaitObservation, Failure> {
            self.result
        }
    }

    fn input() -> Value {
        json!({
            "condition": { "type": "client-size", "width": 800, "height": 600 },
            "pollIntervalMs": 20,
            "timeoutMs": 500,
        })
    }

    fn observation() -> WindowStateWaitObservation {
        WindowStateWaitObservation {
            visible: true,
            presentable: true,
            focused: false,
            logical_width: 800,
            logical_height: 600,
            maximized: false,
            minimized: false,
            fullscreen: false,
            revision: 7,
            presented_revision: 7,
            poll_count: 2,
        }
    }

    #[test]
    fn state_wait_projects_complete_framework_current_read_only_facts() {
        let port = FixturePort {
            result: Ok(observation()),
        };
        let Ok(result) = wait_with(&port, "s2:w:0123456789abcdef", &input()) else {
            panic!("状态等待投影必须成功");
        };
        assert_eq!(result["capability"], capabilities::WINDOW_STATE_WAIT);
        assert_eq!(result["observationSource"], "uix-framework-current");
        assert_eq!(result["condition"]["type"], "client-size");
        assert_eq!(result["clientSize"]["width"], 800);
        assert_eq!(result["windowState"]["minimized"], false);
        assert_eq!(result["revision"], 7);
        assert_eq!(result["pollCount"], 2);
        assert_eq!(result["frameworkConditionMatched"], true);
        assert_eq!(result["compositorFinalStateConfirmed"], false);
        assert_eq!(result["sameAuthenticatedConnectionUsed"], true);
        assert_eq!(result["providerPolling"], true);
        assert_eq!(result["waitProtocolUsed"], false);
        assert_eq!(result["retrySafe"], true);
        assert_eq!(result["safety"]["desktopInputInjected"], false);
        assert_eq!(result["safety"]["fallback"], "none");

        let envelope = json!({
            "ok": true,
            "app": "app",
            "verb": "read",
            "capability": capabilities::WINDOW_STATE_WAIT,
            "targetId": "s2:w:0123456789abcdef",
            "executionRealm": "same-session-no-focus",
            "requiredExecutionRealm": "same-session-no-focus",
            "executionRealmCertified": true,
            "isolationRequirement": "standard",
            "hostImpactPolicy": "background-preferred",
            "data": result,
            "meta": {
                "foreground": { "activationRequested": false },
                "targeting": "opaque exact window generation"
            }
        });
        let Ok(schema) = serde_json::from_str::<Value>(include_str!(
            "../../contracts/v1/window-state-wait.schema.json"
        )) else {
            panic!("状态等待结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "测试状态等待结果必须匹配公开 schema"
        );
    }

    #[test]
    fn state_wait_maps_timeout_without_inventing_observation() {
        let port = FixturePort {
            result: Err(Failure::Timeout),
        };
        let Err(error) = wait_with(&port, "s2:w:0123456789abcdef", &input()) else {
            panic!("状态等待超时必须失败");
        };
        assert_eq!(error.code, "TIMEOUT");
        assert_eq!(error.details["partialResultPublished"], false);
    }

    #[test]
    fn state_wait_reports_missing_negotiation_as_capability_gap() {
        let port = FixturePort {
            result: Err(Failure::Unavailable),
        };
        let Err(error) = wait_with(&port, "s2:w:0123456789abcdef", &input()) else {
            panic!("缺失状态协商必须失败");
        };
        assert_eq!(error.code, "CAPABILITY_UNAVAILABLE");
        assert_eq!(error.details["fallback"], "none");
        assert_eq!(error.details["retrySafe"], true);
    }
}
