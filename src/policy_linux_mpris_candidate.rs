//! Linux MPRIS crate-private 候选路由的纯 Policy 边界。
//!
//! 本模块只验证公共请求形状并证明固定隔离执行域，不注册 Adapter、不连接 D-Bus，
//! 也不改变默认 capability registry 的 `execution:none`。

use serde_json::{Value, json};

use crate::{
    capabilities,
    components::linux_media_worker_contract_common,
    domain::{
        AppControlError, AppResult, CommandRequest, ExecutionRealm, HostImpactPolicy,
        IsolationRequirement, Verb,
    },
};

const MINIMUM_MAXIMUM_ITEMS: usize = 1;
const MAXIMUM_MAXIMUM_ITEMS: usize = 128;
// Candidate App 路由没有公开 timeout 字段，显式冻结为现有 worker v1 的总期限上限。
const CANDIDATE_TIMEOUT_MS: u32 = linux_media_worker_contract_common::MAXIMUM_TIMEOUT_MS;
const MEDIA_SESSION_APP: &str = "media-session";
const MEDIA_TARGET_PREFIX: &str = "s2:m:";

/// 保存已由 Policy 封闭的三类 MPRIS 候选动作。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CandidateAction<'a> {
    /// 有界发现当前用户总线上的媒体会话。
    Discover,
    /// 读取一个 canonical opaque 媒体目标。
    State { session_id: &'a str },
    /// 对一个 canonical opaque 媒体目标提交一次确认式控制。
    Control {
        session_id: &'a str,
        operation: &'a str,
    },
}

impl CandidateAction<'_> {
    /// 返回该动作唯一允许的版本化 capability。
    pub(crate) const fn capability(self) -> &'static str {
        match self {
            Self::Discover => capabilities::MEDIA_SESSION_DISCOVER_V2,
            Self::State { .. } => capabilities::MEDIA_PLAYBACK_STATE_READ_V2,
            Self::Control { .. } => capabilities::MEDIA_PLAYBACK_CONTROL_V2,
        }
    }
}

/// 保存候选请求经纯 Policy 冻结后的不可变执行事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionPlan<'a> {
    action: CandidateAction<'a>,
    maximum_items: u32,
    timeout_ms: u32,
    isolation_requirement: IsolationRequirement,
    host_impact_policy: HostImpactPolicy,
}

impl<'a> ExecutionPlan<'a> {
    /// 返回后续 candidate Adapter 唯一允许执行的动作。
    pub(crate) const fn action(self) -> CandidateAction<'a> {
        self.action
    }

    /// 返回已限制在 worker 契约范围内的目录上限。
    pub(crate) const fn maximum_items(self) -> u32 {
        self.maximum_items
    }

    /// 返回由 Policy 显式冻结、不得由 Adapter 重置的总期限。
    pub(crate) const fn timeout_ms(self) -> u32 {
        self.timeout_ms
    }

    /// 验证 provider-neutral 成功结果并附加不可伪造的执行证明。
    pub(crate) fn attest_result(self, result: &mut Value) -> AppResult<()> {
        let object = result.as_object_mut().ok_or_else(|| {
            AppControlError::new(
                "OPERATION_FAILED",
                "The MPRIS candidate returned a non-object result.",
            )
        })?;
        if !exact_top_level_result(object)
            || object.get("ok") != Some(&Value::Bool(true))
            || object.get("contractVersion").and_then(Value::as_str) != Some("act/control/v2")
            || object.get("capability").and_then(Value::as_str) != Some(self.action.capability())
            || !valid_result_binding(self.action, self.maximum_items, object.get("data"))
        {
            return Err(AppControlError::new(
                "OPERATION_FAILED",
                "The MPRIS candidate result conflicts with the frozen Policy plan.",
            ));
        }
        insert_attested(
            object,
            "executionRealm",
            json!(ExecutionRealm::IsolatedWorker),
        )?;
        insert_attested(
            object,
            "requiredExecutionRealm",
            json!(ExecutionRealm::IsolatedWorker),
        )?;
        insert_attested(object, "executionRealmCertified", Value::Bool(true))?;
        insert_attested(
            object,
            "isolationRequirement",
            json!(self.isolation_requirement),
        )?;
        insert_attested(object, "hostImpactPolicy", json!(self.host_impact_policy))
    }
}

/// 在任何 candidate Adapter/provider 调用前冻结请求与隔离执行计划。
pub(crate) fn authorize(request: &CommandRequest) -> AppResult<ExecutionPlan<'_>> {
    if request.app != MEDIA_SESSION_APP {
        return Err(AppControlError::new(
            "CAPABILITY_UNSUPPORTED",
            "The MPRIS candidate Policy only accepts the media-session surface.",
        ));
    }
    // mutation 确认必须先于 operation、target、args、数量和任何 provider 解析。
    if request.verb == Verb::Run && !request.confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "MPRIS playback control requires explicit confirmation.",
        ));
    }
    if request.foreground_consent {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "The isolated MPRIS candidate does not accept foreground consent.",
        ));
    }
    if !(MINIMUM_MAXIMUM_ITEMS..=MAXIMUM_MAXIMUM_ITEMS).contains(&request.max_items) {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "maxItems must be between 1 and 128 for the MPRIS candidate.",
        ));
    }
    let action = match request.verb {
        Verb::Status => {
            return Err(AppControlError::new(
                "CAPABILITY_UNAVAILABLE",
                "The MPRIS v2 candidate does not publish a status capability.",
            ));
        }
        Verb::Sessions => {
            require_no_operation(request)?;
            require_empty(&request.target, "target")?;
            require_empty(&request.args, "args")?;
            CandidateAction::Discover
        }
        Verb::Inspect => {
            require_no_operation(request)?;
            require_empty(&request.args, "args")?;
            CandidateAction::State {
                session_id: canonical_target(request)?,
            }
        }
        Verb::Run => {
            require_empty(&request.args, "args")?;
            CandidateAction::Control {
                session_id: canonical_target(request)?,
                operation: control_operation(request)?,
            }
        }
    };
    Ok(ExecutionPlan {
        action,
        maximum_items: request.max_items as u32,
        timeout_ms: CANDIDATE_TIMEOUT_MS,
        isolation_requirement: request.isolation_requirement,
        host_impact_policy: request.isolation_requirement.into(),
    })
}

fn exact_top_level_result(object: &serde_json::Map<String, Value>) -> bool {
    const KEYS: [&str; 9] = [
        "ok",
        "contractVersion",
        "capability",
        "data",
        "executionRealm",
        "requiredExecutionRealm",
        "executionRealmCertified",
        "isolationRequirement",
        "hostImpactPolicy",
    ];
    (4..=KEYS.len()).contains(&object.len())
        && object.keys().all(|key| KEYS.contains(&key.as_str()))
}

fn valid_result_binding(
    action: CandidateAction<'_>,
    maximum_items: u32,
    data: Option<&Value>,
) -> bool {
    let Some(data) = data.and_then(Value::as_object) else {
        return false;
    };
    match action {
        CandidateAction::Discover => valid_discovery_data(data, maximum_items),
        CandidateAction::State { session_id } => valid_state_data(data, session_id),
        CandidateAction::Control {
            session_id,
            operation,
        } => valid_control_data(data, session_id, operation),
    }
}

fn valid_discovery_data(data: &serde_json::Map<String, Value>, maximum_items: u32) -> bool {
    const KEYS: [&str; 7] = [
        "count",
        "total",
        "truncated",
        "coverage",
        "complete",
        "warnings",
        "sessions",
    ];
    let Some(sessions) = data.get("sessions").and_then(Value::as_array) else {
        return false;
    };
    let Some(count) = data.get("count").and_then(Value::as_u64) else {
        return false;
    };
    let Some(truncated) = data.get("truncated").and_then(Value::as_bool) else {
        return false;
    };
    let Some(complete) = data.get("complete").and_then(Value::as_bool) else {
        return false;
    };
    let Some(warnings) = data.get("warnings").and_then(Value::as_array) else {
        return false;
    };
    let valid_total = match data.get("total") {
        Some(Value::Null) => true,
        Some(value) => value.as_u64().is_some_and(|total| total >= count),
        None => false,
    };
    exact_keys(data, &KEYS)
        && count == sessions.len() as u64
        && count <= u64::from(maximum_items)
        && data.get("coverage").and_then(Value::as_str)
            == Some("mpris-owned-names-private-broker-generation")
        && valid_total
        && complete == (!truncated && warnings.is_empty())
        && valid_warnings(warnings)
        && sessions.iter().all(valid_session_entry)
}

fn valid_warnings(warnings: &[Value]) -> bool {
    let mut seen = Vec::with_capacity(warnings.len());
    warnings.iter().all(|warning| {
        warning.as_str().is_some_and(|value| {
            matches!(
                value,
                "ambiguous-owner-alias" | "ambiguous-public-id" | "owner-changed"
            ) && !seen.contains(&value)
                && {
                    seen.push(value);
                    true
                }
        })
    })
}

fn valid_session_entry(value: &Value) -> bool {
    const KEYS: [&str; 2] = ["sessionId", "targetKind"];
    value.as_object().is_some_and(|session| {
        exact_keys(session, &KEYS)
            && session
                .get("sessionId")
                .and_then(Value::as_str)
                .is_some_and(valid_media_target)
            && session.get("targetKind").and_then(Value::as_str) == Some("media-session")
    })
}

fn valid_state_data(data: &serde_json::Map<String, Value>, session_id: &str) -> bool {
    const KEYS: [&str; 4] = [
        "sessionId",
        "targetKind",
        "playbackStatus",
        "availableControls",
    ];
    const CONTROL_KEYS: [&str; 6] = [
        "play",
        "pause",
        "togglePlayPause",
        "stop",
        "skipNext",
        "skipPrevious",
    ];
    exact_keys(data, &KEYS)
        && data.get("sessionId").and_then(Value::as_str) == Some(session_id)
        && data.get("targetKind").and_then(Value::as_str) == Some("media-session")
        && data
            .get("playbackStatus")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "playing" | "paused" | "stopped" | "unknown"))
        && data
            .get("availableControls")
            .and_then(Value::as_object)
            .is_some_and(|controls| {
                exact_keys(controls, &CONTROL_KEYS) && controls.values().all(Value::is_boolean)
            })
}

fn valid_control_data(
    data: &serde_json::Map<String, Value>,
    session_id: &str,
    operation: &str,
) -> bool {
    const KEYS: [&str; 10] = [
        "sessionId",
        "targetKind",
        "operation",
        "accepted",
        "finalStateReached",
        "dispatchOutcome",
        "effectConfirmed",
        "retrySafe",
        "targetMayHaveMutated",
        "automaticRetryProhibited",
    ];
    if !exact_keys(data, &KEYS)
        || data.get("sessionId").and_then(Value::as_str) != Some(session_id)
        || data.get("targetKind").and_then(Value::as_str) != Some("media-session")
        || data.get("operation").and_then(Value::as_str) != Some(operation)
        || data.get("effectConfirmed").and_then(Value::as_bool) != Some(false)
    {
        return false;
    }
    let facts = (
        data.get("accepted").and_then(Value::as_bool),
        data.get("finalStateReached").and_then(Value::as_bool),
        data.get("retrySafe").and_then(Value::as_bool),
        data.get("targetMayHaveMutated").and_then(Value::as_bool),
        data.get("automaticRetryProhibited")
            .and_then(Value::as_bool),
    );
    match data.get("dispatchOutcome").and_then(Value::as_str) {
        Some("replied" | "provider-error") => {
            facts == (Some(true), Some(true), Some(false), Some(true), Some(true))
        }
        Some("outcome-unknown") => {
            facts == (Some(true), Some(false), Some(false), Some(true), Some(true))
        }
        Some("pre-dispatch-rejected") => {
            facts
                == (
                    Some(false),
                    Some(false),
                    Some(true),
                    Some(false),
                    Some(false),
                )
        }
        _ => false,
    }
}

fn exact_keys<const N: usize>(object: &serde_json::Map<String, Value>, keys: &[&str; N]) -> bool {
    object.len() == keys.len() && object.keys().all(|key| keys.contains(&key.as_str()))
}

fn insert_attested(
    object: &mut serde_json::Map<String, Value>,
    field: &'static str,
    expected: Value,
) -> AppResult<()> {
    if object.get(field).is_some_and(|value| value != &expected) {
        return Err(AppControlError::new(
            "OPERATION_FAILED",
            "The MPRIS candidate self-reported an execution fact that conflicts with Policy.",
        ));
    }
    object.insert(field.to_owned(), expected);
    Ok(())
}

fn require_no_operation(request: &CommandRequest) -> AppResult<()> {
    if request.operation.is_none() {
        Ok(())
    } else {
        Err(invalid_request(
            "Read-only MPRIS candidate requests must not include an operation.",
        ))
    }
}

fn require_empty(values: &serde_json::Map<String, Value>, group: &str) -> AppResult<()> {
    if values.is_empty() {
        Ok(())
    } else {
        Err(AppControlError::new(
            "INVALID_ARGUMENT",
            format!("MPRIS candidate {group} contains unsupported fields."),
        ))
    }
}

fn canonical_target(request: &CommandRequest) -> AppResult<&str> {
    if request.target.len() != 1 {
        return Err(invalid_request(
            "MPRIS state/control requires exactly one target.sessionId.",
        ));
    }
    let session_id = request
        .target
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| valid_media_target(value))
        .ok_or_else(|| {
            invalid_request("target.sessionId must be a canonical s2:m opaque target.")
        })?;
    Ok(session_id)
}

fn control_operation(request: &CommandRequest) -> AppResult<&str> {
    request
        .operation
        .as_deref()
        .filter(|operation| {
            matches!(
                *operation,
                "play" | "pause" | "toggle-play-pause" | "stop" | "skip-next" | "skip-previous"
            )
        })
        .ok_or_else(|| invalid_request("The MPRIS control operation is not supported."))
}

fn valid_media_target(value: &str) -> bool {
    value.len() == MEDIA_TARGET_PREFIX.len() + 16
        && value.starts_with(MEDIA_TARGET_PREFIX)
        && value.as_bytes()[MEDIA_TARGET_PREFIX.len()..]
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn invalid_request(message: &'static str) -> AppControlError {
    AppControlError::new("INVALID_ARGUMENT", message)
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use super::*;

    fn request(verb: Verb) -> CommandRequest {
        CommandRequest::read(verb, MEDIA_SESSION_APP)
    }

    fn target() -> serde_json::Map<String, Value> {
        Map::from_iter([("sessionId".to_owned(), json!("s2:m:0123456789abcdef"))])
    }

    #[test]
    fn maps_three_verbs_to_exact_v2_capabilities() {
        let discover_request = request(Verb::Sessions);
        let Ok(discover) = authorize(&discover_request) else {
            panic!("发现请求必须通过");
        };
        assert_eq!(discover.action(), CandidateAction::Discover);
        assert_eq!(
            discover.action().capability(),
            capabilities::MEDIA_SESSION_DISCOVER_V2
        );

        let mut state = request(Verb::Inspect);
        state.target = target();
        let Ok(state) = authorize(&state) else {
            panic!("状态请求必须通过");
        };
        assert_eq!(
            state.action(),
            CandidateAction::State {
                session_id: "s2:m:0123456789abcdef"
            }
        );
        assert_eq!(
            state.action().capability(),
            capabilities::MEDIA_PLAYBACK_STATE_READ_V2
        );

        let mut control = request(Verb::Run);
        control.confirmed = true;
        control.operation = Some("play".to_owned());
        control.target = target();
        let Ok(control) = authorize(&control) else {
            panic!("控制请求必须通过");
        };
        assert_eq!(
            control.action(),
            CandidateAction::Control {
                session_id: "s2:m:0123456789abcdef",
                operation: "play"
            }
        );
        assert_eq!(
            control.action().capability(),
            capabilities::MEDIA_PLAYBACK_CONTROL_V2
        );
        assert_eq!(control.maximum_items(), 50);
        assert_eq!(control.timeout_ms(), 30_000);
    }

    #[test]
    fn confirmation_precedes_poisoned_control_fields() {
        let mut poisoned = request(Verb::Run);
        poisoned.foreground_consent = true;
        poisoned.max_items = 0;
        poisoned.operation = Some("forbidden".to_owned());
        poisoned.target.insert("native".to_owned(), Value::Null);
        poisoned.args.insert("provider".to_owned(), Value::Null);
        let Err(error) = authorize(&poisoned) else {
            panic!("未确认控制必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn rejects_noncanonical_or_extra_read_and_control_fields() {
        let mut status = request(Verb::Status);
        let Err(error) = authorize(&status) else {
            panic!("status 未发布");
        };
        assert_eq!(error.code, "CAPABILITY_UNAVAILABLE");
        status.verb = Verb::Inspect;
        status
            .target
            .insert("sessionId".to_owned(), json!("mpris:raw"));
        let Err(error) = authorize(&status) else {
            panic!("原生目标必须拒绝");
        };
        assert_eq!(error.code, "INVALID_ARGUMENT");
        let mut discover = request(Verb::Sessions);
        discover.args.insert("timeout".to_owned(), json!(1));
        let Err(error) = authorize(&discover) else {
            panic!("额外字段必须拒绝");
        };
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }

    #[test]
    fn attestation_rejects_capability_or_realm_conflicts() {
        let discover_request = request(Verb::Sessions);
        let Ok(plan) = authorize(&discover_request) else {
            panic!("发现计划必须通过");
        };
        let mut wrong_capability = json!({
            "ok": true,
            "contractVersion": "act/control/v2",
            "capability": capabilities::MEDIA_PLAYBACK_STATE_READ_V2,
            "data": {},
        });
        let Err(error) = plan.attest_result(&mut wrong_capability) else {
            panic!("错误 capability 必须拒绝");
        };
        assert_eq!(error.code, "OPERATION_FAILED");

        let mut wrong_realm = json!({
            "ok": true,
            "contractVersion": "act/control/v2",
            "capability": capabilities::MEDIA_SESSION_DISCOVER_V2,
            "executionRealm": "host-headless",
            "data": {},
        });
        let Err(error) = plan.attest_result(&mut wrong_realm) else {
            panic!("自报域冲突必须拒绝");
        };
        assert_eq!(error.code, "OPERATION_FAILED");
    }

    #[test]
    fn attestation_binds_state_target_control_operation_and_closed_fields() {
        let mut state_request = request(Verb::Inspect);
        state_request.target = target();
        let Ok(state_plan) = authorize(&state_request) else {
            panic!("状态计划必须通过");
        };
        let mut wrong_state = json!({
            "ok": true,
            "contractVersion": "act/control/v2",
            "capability": capabilities::MEDIA_PLAYBACK_STATE_READ_V2,
            "data": {
                "sessionId": "s2:m:ffffffffffffffff",
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
        });
        let Err(error) = state_plan.attest_result(&mut wrong_state) else {
            panic!("状态结果必须绑定请求目标");
        };
        assert_eq!(error.code, "OPERATION_FAILED");

        let mut control_request = request(Verb::Run);
        control_request.confirmed = true;
        control_request.operation = Some("play".to_owned());
        control_request.target = target();
        let Ok(control_plan) = authorize(&control_request) else {
            panic!("控制计划必须通过");
        };
        let mut wrong_control = json!({
            "ok": true,
            "contractVersion": "act/control/v2",
            "capability": capabilities::MEDIA_PLAYBACK_CONTROL_V2,
            "data": {
                "sessionId": "s2:m:0123456789abcdef",
                "targetKind": "media-session",
                "operation": "pause",
                "accepted": true,
                "finalStateReached": true,
                "dispatchOutcome": "replied",
                "effectConfirmed": false,
                "retrySafe": false,
                "targetMayHaveMutated": true,
                "automaticRetryProhibited": true,
            },
        });
        let Err(error) = control_plan.attest_result(&mut wrong_control) else {
            panic!("控制结果必须绑定 operation");
        };
        assert_eq!(error.code, "OPERATION_FAILED");

        wrong_control["data"]["operation"] = json!("play");
        wrong_control["providerIdentity"] = json!("forbidden");
        let Err(error) = control_plan.attest_result(&mut wrong_control) else {
            panic!("控制结果必须拒绝额外身份字段");
        };
        assert_eq!(error.code, "OPERATION_FAILED");
    }

    #[test]
    fn strict_plan_attests_isolated_worker_without_provider_identity() {
        let mut strict = request(Verb::Sessions);
        strict.isolation_requirement = IsolationRequirement::Strict;
        let Ok(plan) = authorize(&strict) else {
            panic!("严格隔离 worker 请求必须通过");
        };
        let mut result = json!({
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
        });
        let attestation = plan.attest_result(&mut result);
        assert!(attestation.is_ok(), "执行证明必须成功");
        assert_eq!(result["executionRealm"], "isolated-worker");
        assert_eq!(result["requiredExecutionRealm"], "isolated-worker");
        assert_eq!(result["executionRealmCertified"], true);
        assert_eq!(result["isolationRequirement"], "strict");
        assert_eq!(result["hostImpactPolicy"], "strict-no-interference");
        let serialized = result.to_string();
        for forbidden in ["unix:path=", "guid", "owner", "pid", "providerIdentity"] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn attested_results_match_all_three_v2_schemas() {
        let discover_request = request(Verb::Sessions);
        let Ok(discover_plan) = authorize(&discover_request) else {
            panic!("发现计划必须通过");
        };
        let mut discover = json!({
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
        });
        let attestation = discover_plan.attest_result(&mut discover);
        assert!(attestation.is_ok(), "发现证明必须成功");
        assert_schema(
            include_str!("../contracts/v2/media-session-observation.schema.json"),
            &discover,
        );

        let mut state_request = request(Verb::Inspect);
        state_request.target = target();
        let Ok(state_plan) = authorize(&state_request) else {
            panic!("状态计划必须通过");
        };
        let mut state = json!({
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
        });
        let attestation = state_plan.attest_result(&mut state);
        assert!(attestation.is_ok(), "状态证明必须成功");
        assert_schema(
            include_str!("../contracts/v2/media-playback-state.schema.json"),
            &state,
        );

        let mut control_request = request(Verb::Run);
        control_request.confirmed = true;
        control_request.operation = Some("play".to_owned());
        control_request.target = target();
        let Ok(control_plan) = authorize(&control_request) else {
            panic!("控制计划必须通过");
        };
        let mut control = json!({
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
        });
        let attestation = control_plan.attest_result(&mut control);
        assert!(attestation.is_ok(), "控制证明必须成功");
        assert_schema(
            include_str!("../contracts/v2/media-playback-control.schema.json"),
            &control,
        );
    }

    fn assert_schema(schema: &str, instance: &Value) {
        let Ok(schema) = serde_json::from_str::<Value>(schema) else {
            panic!("v2 schema 必须可解析");
        };
        let validation = jsonschema::draft202012::validate(&schema, instance);
        assert!(
            validation.is_ok(),
            "attested v2 结果必须匹配 schema: {validation:?}"
        );
    }
}
