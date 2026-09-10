//! Linux MPRIS crate-private candidate App Adapter。
//!
//! Adapter 只消费纯 Policy 冻结后的动作，调用 current-user runtime client，并在返回前
//! 强制执行 attestation。它尚未加入 Adaptive registry、Service 或 CLI。

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{
    adapters::AppAdapter,
    domain::{AppControlError, AppResult, CommandRequest},
    mpris_runtime_client,
    policy::mpris_candidate::{self, CandidateAction},
};

/// 表示尚未注册的 Linux MPRIS candidate provider。
pub(crate) struct MediaSessionCandidateAdapter;

/// 为下一批显式 candidate registry 接线保留唯一构造点；当前不会进入 Adaptive registry。
#[allow(dead_code)]
pub(crate) fn boxed() -> Arc<dyn AppAdapter> {
    Arc::new(MediaSessionCandidateAdapter)
}

impl AppAdapter for MediaSessionCandidateAdapter {
    fn app_id(&self) -> &'static str {
        "media-session"
    }

    fn status(&self) -> AppResult<Value> {
        Err(status_unavailable())
    }

    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        execute(request)
    }

    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        execute(request)
    }

    fn run(&self, request: &CommandRequest) -> AppResult<Value> {
        execute(request)
    }
}

fn execute(request: &CommandRequest) -> AppResult<Value> {
    execute_with(request, run_runtime)
}

fn execute_with<'a, F>(request: &'a CommandRequest, runtime: F) -> AppResult<Value>
where
    F: FnOnce(CandidateAction<'a>, u32, u32) -> AppResult<Value>,
{
    // Policy 必须先于 endpoint、GUID、worker 映像和任何 provider I/O。
    let plan = mpris_candidate::authorize(request)?;
    let mut result = runtime(plan.action(), plan.maximum_items(), plan.timeout_ms())?;
    // optional schema 字段不构成证明；Adapter 返回前必须实际执行 attestation。
    plan.attest_result(&mut result)?;
    Ok(result)
}

fn run_runtime(
    action: CandidateAction<'_>,
    maximum_items: u32,
    timeout_ms: u32,
) -> AppResult<Value> {
    match action {
        CandidateAction::Discover => {
            mpris_runtime_client::discover_current(maximum_items, timeout_ms)
        }
        CandidateAction::State { session_id } => {
            mpris_runtime_client::state_current(session_id, maximum_items, timeout_ms)
        }
        CandidateAction::Control {
            session_id,
            operation,
        } => mpris_runtime_client::control_current(
            session_id,
            operation,
            true,
            maximum_items,
            timeout_ms,
        ),
    }
}

fn status_unavailable() -> AppControlError {
    AppControlError::with_details(
        "CAPABILITY_UNAVAILABLE",
        "The MPRIS v2 candidate does not publish a status capability.",
        json!({
            "platform": "linux",
            "provider": "mpris-v2",
            "providerState": "candidate-status-unpublished",
            "executionRealm": "none",
            "fallback": "none",
        }),
    )
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use serde_json::{Map, json};

    use super::*;
    use crate::{
        capabilities,
        domain::{IsolationRequirement, Verb},
    };

    fn request(verb: Verb) -> CommandRequest {
        CommandRequest::read(verb, "media-session")
    }

    fn target() -> Map<String, Value> {
        Map::from_iter([("sessionId".to_owned(), json!("s2:m:0123456789abcdef"))])
    }

    fn discovery_result() -> Value {
        json!({
            "ok": true,
            "contractVersion": "act/control/v2",
            "capability": capabilities::MEDIA_SESSION_DISCOVER_V2,
            "data": {
                "count": 0,
                "total": 0,
                "truncated": false,
                "coverage": "mpris-owned-names-private-broker-generation",
                "complete": true,
                "warnings": [],
                "sessions": [],
            },
        })
    }

    fn state_result() -> Value {
        json!({
            "ok": true,
            "contractVersion": "act/control/v2",
            "capability": capabilities::MEDIA_PLAYBACK_STATE_READ_V2,
            "data": {
                "sessionId": "s2:m:0123456789abcdef",
                "targetKind": "media-session",
                "playbackStatus": "playing",
                "availableControls": {
                    "play": true,
                    "pause": true,
                    "togglePlayPause": true,
                    "stop": true,
                    "skipNext": true,
                    "skipPrevious": true,
                },
            },
        })
    }

    fn control_result() -> Value {
        json!({
            "ok": true,
            "contractVersion": "act/control/v2",
            "capability": capabilities::MEDIA_PLAYBACK_CONTROL_V2,
            "data": {
                "sessionId": "s2:m:0123456789abcdef",
                "targetKind": "media-session",
                "operation": "play",
                "accepted": true,
                "finalStateReached": true,
                "dispatchOutcome": "replied",
                "effectConfirmed": false,
                "retrySafe": false,
                "targetMayHaveMutated": true,
                "automaticRetryProhibited": true,
            },
        })
    }

    #[test]
    fn unconfirmed_control_precedes_poisoned_fields_and_runtime() {
        let mut poisoned = request(Verb::Run);
        poisoned.foreground_consent = true;
        poisoned.max_items = 0;
        poisoned.operation = Some("forbidden".to_owned());
        poisoned.target.insert("native".to_owned(), Value::Null);
        poisoned.args.insert("provider".to_owned(), Value::Null);
        let called = Cell::new(false);
        let result = execute_with(&poisoned, |_, _, _| {
            called.set(true);
            Ok(Value::Null)
        });
        let Err(error) = result else {
            panic!("未确认控制必须失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
        assert!(!called.get());
    }

    #[test]
    fn three_routes_use_frozen_action_limit_timeout_and_attestation() {
        let discover_request = request(Verb::Sessions);
        let discover = execute_with(&discover_request, |action, maximum_items, timeout_ms| {
            assert_eq!(action, CandidateAction::Discover);
            assert_eq!(maximum_items, 50);
            assert_eq!(timeout_ms, 30_000);
            Ok(discovery_result())
        });
        let Ok(discover) = discover else {
            panic!("发现候选必须成功");
        };
        assert_eq!(discover["executionRealmCertified"], true);

        let mut state_request = request(Verb::Inspect);
        state_request.target = target();
        state_request.isolation_requirement = IsolationRequirement::Strict;
        let state = execute_with(&state_request, |action, maximum_items, timeout_ms| {
            assert_eq!(
                action,
                CandidateAction::State {
                    session_id: "s2:m:0123456789abcdef"
                }
            );
            assert_eq!(maximum_items, 50);
            assert_eq!(timeout_ms, 30_000);
            Ok(state_result())
        });
        let Ok(state) = state else {
            panic!("状态候选必须成功");
        };
        assert_eq!(state["isolationRequirement"], "strict");
        assert_eq!(state["hostImpactPolicy"], "strict-no-interference");

        let mut control_request = request(Verb::Run);
        control_request.confirmed = true;
        control_request.operation = Some("play".to_owned());
        control_request.target = target();
        let control = execute_with(&control_request, |action, maximum_items, timeout_ms| {
            assert_eq!(
                action,
                CandidateAction::Control {
                    session_id: "s2:m:0123456789abcdef",
                    operation: "play"
                }
            );
            assert_eq!(maximum_items, 50);
            assert_eq!(timeout_ms, 30_000);
            Ok(control_result())
        });
        let Ok(control) = control else {
            panic!("控制候选必须成功");
        };
        assert_eq!(control["executionRealm"], "isolated-worker");
    }

    #[test]
    fn provider_error_propagates_and_mismatched_success_is_rejected() {
        let request = request(Verb::Sessions);
        let failure = execute_with(&request, |_, _, _| {
            Err(AppControlError::new("TIMEOUT", "fixture timeout"))
        });
        let Err(failure) = failure else {
            panic!("provider error 必须透传");
        };
        assert_eq!(failure.code, "TIMEOUT");

        let mismatch = execute_with(&request, |_, _, _| Ok(state_result()));
        let Err(mismatch) = mismatch else {
            panic!("错配结果必须被 attestation 拒绝");
        };
        assert_eq!(mismatch.code, "OPERATION_FAILED");
    }

    #[test]
    fn candidate_status_remains_unpublished() {
        let adapter = MediaSessionCandidateAdapter;
        assert_eq!(adapter.app_id(), "media-session");
        let Err(error) = adapter.status() else {
            panic!("candidate status 必须保持未发布");
        };
        assert_eq!(error.code, "CAPABILITY_UNAVAILABLE");
        assert_eq!(error.details["executionRealm"], "none");
    }
}
