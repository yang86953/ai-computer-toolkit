//! MPRIS App v3 的纯输入边界；确认、封闭字段与预算先于 provider I/O。

use serde::Deserialize;

use crate::{
    capabilities,
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    domain::{AppControlError, AppResult, CommandRequest, Verb},
};

pub(crate) const MAXIMUM_ITEMS: u32 = 128;
pub(crate) const DEFAULT_TIMEOUT_MS: u32 = 5_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Control {
    Play,
    Pause,
    Stop,
}

impl Control {
    pub(crate) const fn operation(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Stop => "stop",
        }
    }

    pub(crate) const fn expected_status(self) -> &'static str {
        match self {
            Self::Play => "playing",
            Self::Pause => "paused",
            Self::Stop => "stopped",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Discover,
    State,
    Control(Control),
}

impl Action {
    pub(crate) const fn capability(self) -> &'static str {
        match self {
            Self::Discover => capabilities::MEDIA_SESSION_DISCOVER_V3,
            Self::State => capabilities::MEDIA_PLAYBACK_STATE_READ_V3,
            Self::Control(_) => capabilities::MEDIA_PLAYBACK_CONTROL_V3,
        }
    }
}

pub(crate) struct Plan<'a> {
    pub action: Action,
    pub session_id: Option<&'a str>,
    pub maximum_items: u32,
    pub timeout_ms: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DiscoveryInput {
    #[serde(default = "maximum_items")]
    maximum_items: u32,
    #[serde(default = "default_timeout")]
    timeout_ms: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StateInput {
    #[serde(default = "default_timeout")]
    timeout_ms: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ControlInput {
    operation: Control,
    #[serde(default = "default_timeout")]
    timeout_ms: u32,
}

const fn maximum_items() -> u32 {
    MAXIMUM_ITEMS
}

const fn default_timeout() -> u32 {
    DEFAULT_TIMEOUT_MS
}

pub(crate) fn is_capability(capability: &str) -> bool {
    matches!(
        capability,
        capabilities::MEDIA_SESSION_DISCOVER_V3
            | capabilities::MEDIA_PLAYBACK_STATE_READ_V3
            | capabilities::MEDIA_PLAYBACK_CONTROL_V3
    )
}

pub(crate) fn parse(request: &CommandRequest) -> AppResult<Plan<'_>> {
    let capability = request
        .args
        .get("capability")
        .and_then(serde_json::Value::as_str);
    // 即使其余字段完全无效，已识别的写 capability 仍必须先检查确认。
    if capability == Some(capabilities::MEDIA_PLAYBACK_CONTROL_V3) && !request.confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "MPRIS playback control requires task authorization or explicit confirmation.",
        ));
    }
    if request.app != "app"
        || request.verb != Verb::Run
        || request.args.len() != 2
        || request.foreground_consent
    {
        return Err(invalid());
    }
    let input = request
        .args
        .get("input")
        .filter(|input| input.is_object())
        .ok_or_else(invalid)?;
    let (action, maximum_items, timeout_ms, verb) = match capability {
        Some(capabilities::MEDIA_SESSION_DISCOVER_V3) => {
            let input: DiscoveryInput =
                serde_json::from_value(input.clone()).map_err(|_| invalid())?;
            (
                Action::Discover,
                input.maximum_items,
                input.timeout_ms,
                "discover",
            )
        }
        Some(capabilities::MEDIA_PLAYBACK_STATE_READ_V3) => {
            let input: StateInput = serde_json::from_value(input.clone()).map_err(|_| invalid())?;
            (Action::State, MAXIMUM_ITEMS, input.timeout_ms, "read")
        }
        Some(capabilities::MEDIA_PLAYBACK_CONTROL_V3) => {
            let input: ControlInput =
                serde_json::from_value(input.clone()).map_err(|_| invalid())?;
            (
                Action::Control(input.operation),
                MAXIMUM_ITEMS,
                input.timeout_ms,
                "apply",
            )
        }
        _ => return Err(invalid()),
    };
    if request.operation.as_deref() != Some(verb)
        || !(1..=MAXIMUM_ITEMS).contains(&maximum_items)
        || !(1..=30_000).contains(&timeout_ms)
    {
        return Err(invalid());
    }
    let session_id = if action == Action::Discover {
        if !request.target.is_empty() {
            return Err(invalid());
        }
        None
    } else {
        let target = request
            .target
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .filter(|target| valid_target(target))
            .ok_or_else(invalid)?;
        if request.target.len() != 1 {
            return Err(invalid());
        }
        Some(target)
    };
    Ok(Plan {
        action,
        session_id,
        maximum_items,
        timeout_ms,
    })
}

pub(crate) fn valid_target(target: &str) -> bool {
    OpaqueTargetId::parse(target).is_some_and(|target| target.kind() == OpaqueTargetKind::Media)
}

fn invalid() -> AppControlError {
    AppControlError::new(
        "INVALID_ARGUMENT",
        "The request violates the closed MPRIS App v3 contract.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(capability: &str, verb: &str) -> CommandRequest {
        let mut request = CommandRequest::read(Verb::Run, "app");
        request.operation = Some(verb.into());
        request.args.insert("capability".into(), json!(capability));
        request.args.insert("input".into(), json!({}));
        if verb != "discover" {
            request
                .target
                .insert("sessionId".into(), json!("s2:m:0123456789abcdef"));
        }
        request
    }

    #[test]
    fn confirmation_precedes_malformed_control_fields() {
        let mut request = request(capabilities::MEDIA_PLAYBACK_CONTROL_V3, "bad");
        request.target.clear();
        request
            .args
            .insert("input".into(), json!({"method": "Raise", "timeoutMs": 0}));
        assert_eq!(parse(&request).err().unwrap().code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn canonical_discovery_and_state_are_read_only() {
        let discovery = request(capabilities::MEDIA_SESSION_DISCOVER_V3, "discover");
        let plan = parse(&discovery).unwrap();
        assert_eq!(plan.action, Action::Discover);
        assert_eq!(plan.maximum_items, MAXIMUM_ITEMS);
        assert_eq!(plan.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert!(plan.session_id.is_none());
        let state = request(capabilities::MEDIA_PLAYBACK_STATE_READ_V3, "read");
        assert_eq!(parse(&state).unwrap().action, Action::State);
    }

    #[test]
    fn control_is_closed_to_three_state_verifiable_operations() {
        for (operation, expected) in [
            ("play", "playing"),
            ("pause", "paused"),
            ("stop", "stopped"),
        ] {
            let mut request = request(capabilities::MEDIA_PLAYBACK_CONTROL_V3, "apply");
            request.confirmed = true;
            request
                .args
                .insert("input".into(), json!({"operation": operation}));
            let Action::Control(control) = parse(&request).unwrap().action else {
                panic!("控制动作");
            };
            assert_eq!(control.operation(), operation);
            assert_eq!(control.expected_status(), expected);
        }
        for operation in ["togglePlayPause", "skipNext", "Raise", "OpenUri"] {
            let mut request = request(capabilities::MEDIA_PLAYBACK_CONTROL_V3, "apply");
            request.confirmed = true;
            request
                .args
                .insert("input".into(), json!({"operation": operation}));
            assert_eq!(parse(&request).err().unwrap().code, "INVALID_ARGUMENT");
        }
    }

    #[test]
    fn native_identity_foreground_and_wrong_targets_are_rejected() {
        for field in [
            "address",
            "sessionBusAddress",
            "method",
            "script",
            "confirmed",
            "foregroundConsent",
        ] {
            let mut request = request(capabilities::MEDIA_PLAYBACK_STATE_READ_V3, "read");
            request.args["input"][field] = json!("injected");
            assert!(parse(&request).is_err());
        }
        let mut request = request(capabilities::MEDIA_PLAYBACK_STATE_READ_V3, "read");
        request.foreground_consent = true;
        assert!(parse(&request).is_err());
        request.foreground_consent = false;
        for target in [
            "s2:w:0123456789abcdef",
            "s2:m:0123456789ABCDEF",
            "org.mpris.MediaPlayer2.player",
        ] {
            request.target.insert("sessionId".into(), json!(target));
            assert!(parse(&request).is_err());
        }
    }

    #[test]
    fn input_limits_and_types_are_enforced_before_io() {
        for input in [
            json!({"maximumItems": 0}),
            json!({"maximumItems": 129}),
            json!({"timeoutMs": 0}),
            json!({"timeoutMs": 30001}),
            json!({"timeoutMs": 1.5}),
            json!({"timeoutMs": "5000"}),
            json!([]),
        ] {
            let mut request = request(capabilities::MEDIA_SESSION_DISCOVER_V3, "discover");
            request.args.insert("input".into(), input);
            assert!(parse(&request).is_err());
        }
    }
}
