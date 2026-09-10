//! 媒体任务资格评估消费公开 facade 观察；不直接访问 MPRIS 或绕过任务期限。

use std::time::Duration;

use serde_json::{Value, json};

use crate::{AppControlService, capabilities, domain::AppResult};

use super::{execution, policy, protocol::TaskGrant};

pub(super) fn assess(
    service: &AppControlService,
    grant: &TaskGrant,
    capability: &str,
    target: &str,
    remaining: Duration,
    result_bytes: usize,
) -> AppResult<Value> {
    policy::check_scope(grant, capability, target, None)?;
    let request = policy::read_request(capabilities::MEDIA_PLAYBACK_STATE_READ_V3, Some(target))?;
    let observation = execution::execute(service, request, &[], remaining, result_bytes)?;
    qualify(grant, capability, target, observation)
}

fn qualify(
    grant: &TaskGrant,
    capability: &str,
    target: &str,
    observation: Value,
) -> AppResult<Value> {
    let result = &observation["workflow"]["results"][0]["result"];
    if result["ok"] != true
        || result["capability"] != capabilities::MEDIA_PLAYBACK_STATE_READ_V3
        || result["data"]["sessionId"] != target
        || !result["data"]["availableControls"].is_object()
    {
        return Err(policy::error(
            "OPERATION_FAILED",
            "Media assessment has no matching current-target observation.",
        ));
    }
    let permission = grant
        .permissions
        .iter()
        .find(|permission| permission.capability == capability && permission.target_id == target)
        .ok_or_else(|| {
            policy::error(
                "TASK_SCOPE_VIOLATION",
                "Media assessment is outside this task grant.",
            )
        })?;
    let operations = ["play", "pause", "stop"]
        .into_iter()
        .filter(|operation| result["data"]["availableControls"][*operation] == true)
        .filter(|operation| {
            permission
                .required_input
                .get("operation")
                .is_none_or(|required| required == *operation)
        })
        .collect::<Vec<_>>();
    let eligible = capability == capabilities::MEDIA_PLAYBACK_STATE_READ_V3
        || (capability == capabilities::MEDIA_PLAYBACK_CONTROL_V3 && !operations.is_empty());
    let mut guarantees = policy::guarantees(policy::definition(capability)?);
    guarantees["eligible"] = json!(eligible);
    guarantees["assessmentRequired"] = json!(false);
    Ok(json!({
        "capability": capability, "targetId": target,
        "decision": if eligible { "executable-background" } else { "unavailable" },
        "authorization": {"source": "task-preauthorization", "perActionPrompt": false},
        "guarantees": guarantees,
        "assessment": {
            "qualification": "target-capability-current-provider-state", "availableOperations": operations,
            "materializedInputValidated": false, "revalidateAtDispatch": true,
            "evidence": result["data"],
        },
        "observation": observation,
    }))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    const TARGET: &str = "s2:m:0123456789abcdef";

    fn grant() -> TaskGrant {
        serde_json::from_value(json!({
            "contractVersion": "act/task-grant/v1", "taskId": "t2:0123456789abcdef0123456789abcdef",
            "authorizationSource": "user-task", "totalTimeoutMs": 5000, "maxExecutions": 8,
            "permissions": [{"capability": capabilities::MEDIA_PLAYBACK_CONTROL_V3, "targetId": TARGET,
                "requiredInput": {"operation": "pause"}}],
        })).unwrap()
    }

    fn observed(pause: bool) -> Value {
        json!({"workflow": {"results": [{"result": {
            "ok": true, "capability": capabilities::MEDIA_PLAYBACK_STATE_READ_V3,
            "data": {"sessionId": TARGET, "playbackStatus": "playing", "availableControls": {"play": true, "pause": pause, "stop": true}},
        }}]}})
    }

    #[test]
    fn route_availability_is_not_target_eligibility() {
        let definition = policy::definition(capabilities::MEDIA_PLAYBACK_CONTROL_V3).unwrap();
        let route = policy::guarantees(definition);
        assert_eq!(route["routeEligible"], true);
        assert_eq!(route["eligible"], false);
        assert_eq!(route["assessmentRequired"], true);
    }

    #[test]
    fn qualified_control_must_match_both_current_can_property_and_task_operation() {
        for (can_pause, eligible) in [(false, false), (true, true)] {
            let result = qualify(
                &grant(),
                capabilities::MEDIA_PLAYBACK_CONTROL_V3,
                TARGET,
                observed(can_pause),
            )
            .unwrap();
            assert_eq!(result["guarantees"]["eligible"], eligible);
            assert_eq!(
                result["assessment"]["availableOperations"],
                if eligible {
                    json!(["pause"])
                } else {
                    json!([])
                }
            );
            assert_eq!(result["authorization"]["perActionPrompt"], false);
        }
    }

    #[test]
    fn unrelated_or_missing_observation_never_certifies_target() {
        let mut observation = observed(true);
        observation["workflow"]["results"][0]["result"]["data"]["sessionId"] =
            json!("s2:m:fedcba9876543210");
        assert!(
            qualify(
                &grant(),
                capabilities::MEDIA_PLAYBACK_CONTROL_V3,
                TARGET,
                observation
            )
            .is_err()
        );
        assert!(
            qualify(
                &grant(),
                capabilities::MEDIA_PLAYBACK_CONTROL_V3,
                TARGET,
                json!({})
            )
            .is_err()
        );
    }
}
