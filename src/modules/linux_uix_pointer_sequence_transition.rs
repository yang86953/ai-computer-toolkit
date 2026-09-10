//! 协调确认、完整悬停—点击序列与提交后的语义条件同步。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, ElementWaitFailure, PointerSequenceFailure, PointerSequenceTransitionFailure,
        PointerSequenceTransitionFailureSource, PointerSequenceTransitionOutcome,
        WindowActionFailure,
    },
    capabilities,
    components::uix_pointer_sequence_transition_contract::UixPointerSequenceTransitionInput,
    domain::{AppControlError, AppResult},
    modules::{uix_element_wait, uix_window},
};

const CAPABILITY: &str = capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION;
const PROVIDER: &str = "uix-agent-v1";

trait UixPointerSequenceTransitionPort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixPointerSequenceTransitionInput,
    ) -> Result<PointerSequenceTransitionOutcome, PointerSequenceTransitionFailure>;
}

struct SystemUixPointerSequenceTransition;

impl UixPointerSequenceTransitionPort for SystemUixPointerSequenceTransition {
    fn perform(
        &self,
        session_id: &str,
        input: &UixPointerSequenceTransitionInput,
    ) -> Result<PointerSequenceTransitionOutcome, PointerSequenceTransitionFailure> {
        uix_agent::perform_pointer_sequence_transition(session_id, input)
    }
}

/// 对精确协作式窗口执行已确认的悬停—点击序列，并等待语义后置条件。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    perform_with(
        &SystemUixPointerSequenceTransition,
        session_id,
        confirmed,
        value,
    )
}

fn perform_with(
    port: &impl UixPointerSequenceTransitionPort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX pointer sequence transition requires explicit confirmation.",
        ));
    }
    let input = UixPointerSequenceTransitionInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .perform(session_id, &input)
        .map_err(|failure| transition_error(failure, &input))?;
    debug_assert_eq!(
        usize::from(outcome.sequence.moves_accepted),
        input.moves_requested()
    );
    debug_assert_eq!(
        usize::from(outcome.sequence.clicks_accepted),
        input.clicks_requested()
    );
    let steps = input
        .steps()
        .iter()
        .map(|step| {
            json!({
                "type": step.as_str(),
                "x": step.x(),
                "y": step.y(),
            })
        })
        .collect::<Vec<_>>();
    let observation = outcome.postcondition;
    let snapshot_id = uix_window::snapshot_id(
        &observation.snapshot.session_id,
        observation.snapshot.revision,
        observation.snapshot.presented_revision,
    );
    let element = observation
        .element
        .as_ref()
        .map(|node| uix_element_wait::public_element(node, &snapshot_id));
    let match_state = if element.is_some() {
        "unique"
    } else {
        "missing"
    };
    let revision_wait_used = observation.wait_count > 0;
    let safety = json!({
        "provider": PROVIDER,
        "providerCache": "disabled",
        "sameAuthenticatedConnectionUsed": true,
        "windowGenerationFixed": true,
        "revisionChainUsed": true,
        "semanticRevisionWaitUsed": revision_wait_used,
        "providerPolling": false,
        "stabilitySemantics": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "sensitiveSnapshotPublished": false,
        "hostCoordinateMappingPublished": false,
        "foregroundActivationRequested": false,
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "requestScopedBalancedClicks": true,
        "independentButtonOwnership": false,
        "dragSupported": false,
        "doubleClickSupported": false,
        "clickCountSupported": false,
        "scrollSupported": false,
        "x11Used": false,
        "fallback": "none",
    });
    let Value::Object(mut result) = json!({
        "ok": true,
        "contractVersion": "act/ui-pointer-sequence-transition/v1",
        "capability": CAPABILITY,
        "targetId": session_id,
        "action": "pointer-sequence",
        "coordinateSpace": input.coordinate_space().as_str(),
        "steps": steps,
        "stepsRequested": input.steps_requested(),
        "stepsAccepted": outcome.sequence.steps_accepted,
        "movesAccepted": outcome.sequence.moves_accepted,
        "clicksAccepted": outcome.sequence.clicks_accepted,
        "intervalMs": input.interval_ms(),
        "plannedDurationMs": input.planned_duration_ms(),
        "allClicksBalanced": true,
        "sequenceRevision": outcome.sequence.revision,
        "sequencePresentedRevision": outcome.sequence.presented_revision,
        "sequenceSettled": outcome.sequence.settled,
        "postcondition": input.postcondition().condition().as_str(),
        "selectorSemantics": "exact-and",
        "postconditionMatchedAfterDispatch": true,
        "snapshotId": snapshot_id,
        "matchState": match_state,
        "matchCount": observation.match_count,
        "sampleCount": observation.sample_count,
        "waitCount": observation.wait_count,
        "revision": observation.snapshot.revision,
        "presentedRevision": observation.snapshot.presented_revision,
        "revisionWaitUsed": revision_wait_used,
        "element": element,
    }) else {
        unreachable!("pointer sequence transition 观察投影固定为 JSON 对象");
    };
    let Value::Object(execution) = json!({
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "sameAuthenticatedConnectionUsed": true,
        "effectConfirmed": false,
        "applicationConsumptionConfirmed": false,
        "causalityConfirmed": false,
        "finalStateReached": false,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "hostForegroundActivationRequested": false,
        "effectObservation": "semantic-postcondition-after-complete-pointer-sequence-no-causal-claim",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "same-session-no-focus",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety,
    }) else {
        unreachable!("pointer sequence transition 执行投影固定为 JSON 对象");
    };
    result.extend(execution);
    Ok(Value::Object(result))
}

fn transition_error(
    failure: PointerSequenceTransitionFailure,
    input: &UixPointerSequenceTransitionInput,
) -> AppControlError {
    if let PointerSequenceTransitionFailureSource::Postcondition(ElementWaitFailure::Ambiguous {
        match_count,
    }) = failure.source
    {
        return AppControlError::with_details(
            "AMBIGUOUS_TARGET",
            "The UIX pointer sequence transition postcondition matched multiple elements after the complete sequence; automatic retry is prohibited.",
            failure_details(
                &failure,
                input,
                "postcondition-ambiguous-after-complete-sequence",
                true,
                Some(match_count),
            ),
        );
    }
    if failure.accepted_may_have_occurred {
        let reason = if failure.sequence_completed {
            "trusted-postcondition-lost-after-complete-sequence"
        } else {
            "partial-pointer-sequence-or-final-state-indeterminate"
        };
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer sequence transition may have executed without a trusted semantic postcondition; automatic retry is prohibited.",
            failure_details(&failure, input, reason, true, None),
        );
    }
    match failure.source {
        PointerSequenceTransitionFailureSource::Transport(source) => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(&failure, input, "transport-before-dispatch", false, None),
            );
            error
        }
        PointerSequenceTransitionFailureSource::NegotiationUnavailable => {
            AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The authenticated UIX Agent does not publish the complete pointer sequence and semantic revision synchronization surface.",
                failure_details(&failure, input, "agent-preflight-missing", false, None),
            )
        }
        PointerSequenceTransitionFailureSource::Sequence(sequence) => {
            safe_sequence_error(sequence, &failure, input)
        }
        PointerSequenceTransitionFailureSource::Postcondition(_) => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The complete UIX pointer sequence lost its trusted semantic postcondition; automatic retry is prohibited.",
            failure_details(&failure, input, "postcondition-untrusted", true, None),
        ),
    }
}

fn safe_sequence_error(
    sequence: PointerSequenceFailure,
    failure: &PointerSequenceTransitionFailure,
    input: &UixPointerSequenceTransitionInput,
) -> AppControlError {
    match sequence.source {
        WindowActionFailure::Transport(source) if !failure.accepted_may_have_occurred => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(failure, input, "transport-before-dispatch", false, None),
            );
            error
        }
        WindowActionFailure::UnsupportedAction => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish both pointer_move and click_at.",
            failure_details(
                failure,
                input,
                "agent-move-or-click-not-published",
                false,
                None,
            ),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before pointer sequence transition dispatch.",
            failure_details(failure, input, "window-revision-changed", false, None),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids the pointer sequence.",
            failure_details(failure, input, "application-policy-forbidden", false, None),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for pointer sequence input.",
            failure_details(failure, input, "window-not-presentable", false, None),
        ),
        WindowActionFailure::Transport(_)
        | WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer sequence transition may have partially executed; automatic retry is prohibited.",
            failure_details(
                failure,
                input,
                "unexpected-unsafe-sequence-failure",
                true,
                None,
            ),
        ),
    }
}

fn merge_failure_details(target: &mut Value, extra: Value) {
    let (Some(target), Some(extra)) = (target.as_object_mut(), extra.as_object()) else {
        return;
    };
    target.extend(extra.clone());
}

fn failure_details(
    failure: &PointerSequenceTransitionFailure,
    input: &UixPointerSequenceTransitionInput,
    reason: &'static str,
    accepted_may_have_occurred: bool,
    match_count: Option<usize>,
) -> Value {
    json!({
        "action": "pointer-sequence",
        "coordinateSpace": input.coordinate_space().as_str(),
        "stepsRequested": input.steps_requested(),
        "stepsAccepted": failure.steps_accepted,
        "movesAccepted": failure.moves_accepted,
        "clicksAccepted": failure.clicks_accepted,
        "intervalMs": input.interval_ms(),
        "plannedDurationMs": input.planned_duration_ms(),
        "postcondition": input.postcondition().condition().as_str(),
        "selectorSemantics": "exact-and",
        "reason": reason,
        "sequenceCompleted": failure.sequence_completed,
        "sequenceRevision": failure.sequence_revision,
        "sequencePresentedRevision": failure.sequence_presented_revision,
        "sequenceSettled": failure.sequence_settled,
        "allClicksBalanced": failure.sequence_completed,
        "matchCount": match_count,
        "semanticPostconditionMatched": false,
        "applicationConsumptionConfirmed": false,
        "causalityConfirmed": false,
        "finalStateReached": false,
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
        "automaticRetryProhibited": accepted_may_have_occurred,
        "retrySafe": false,
        "requiredNextStep": if accepted_may_have_occurred {
            "read-current-semantic-snapshot-before-any-new-pointer-input"
        } else {
            "refresh-target-and-reassess"
        },
        "timeoutMs": input.timeout_ms(),
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "fallback": "none",
    })
}

#[cfg(test)]
mod tests {
    use crate::adapters::linux::uix_agent::{
        ElementWaitOutcome, Failure, NodeRecord, PointerSequenceOutcome, RectRecord, Snapshot,
    };

    use super::*;

    struct FixturePort {
        result: Result<PointerSequenceTransitionOutcome, PointerSequenceTransitionFailure>,
    }

    impl UixPointerSequenceTransitionPort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixPointerSequenceTransitionInput,
        ) -> Result<PointerSequenceTransitionOutcome, PointerSequenceTransitionFailure> {
            match &self.result {
                Ok(outcome) => Ok(PointerSequenceTransitionOutcome {
                    sequence: outcome.sequence,
                    postcondition: ElementWaitOutcome {
                        snapshot: Snapshot {
                            session_id: outcome.postcondition.snapshot.session_id.clone(),
                            revision: outcome.postcondition.snapshot.revision,
                            presented_revision: outcome.postcondition.snapshot.presented_revision,
                            nodes: outcome.postcondition.snapshot.nodes.clone(),
                        },
                        element: outcome.postcondition.element.clone(),
                        match_count: outcome.postcondition.match_count,
                        sample_count: outcome.postcondition.sample_count,
                        wait_count: outcome.postcondition.wait_count,
                    },
                }),
                Err(failure) => Err(*failure),
            }
        }
    }

    fn input() -> Value {
        json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "move", "x": 10.0, "y": 20.0 },
                { "type": "click", "x": 30.0, "y": 40.0 }
            ],
            "intervalMs": 0,
            "timeoutMs": 1000,
            "postcondition": {
                "selector": { "automationId": "menu-result" },
                "condition": "unique"
            }
        })
    }

    fn success(settled: bool) -> PointerSequenceTransitionOutcome {
        let node = NodeRecord {
            native_id: "7:1".to_owned(),
            parent_native_id: None,
            automation_id: Some("menu-result".to_owned()),
            focused: false,
            role: "status".to_owned(),
            name: "菜单结果".to_owned(),
            enabled: true,
            actions: Vec::new(),
            frame: RectRecord {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            visible_bounds: None,
        };
        PointerSequenceTransitionOutcome {
            sequence: PointerSequenceOutcome {
                revision: 7,
                presented_revision: 7,
                settled,
                steps_accepted: 2,
                moves_accepted: 1,
                clicks_accepted: 1,
            },
            postcondition: ElementWaitOutcome {
                snapshot: Snapshot {
                    session_id: "s2:w:0123456789abcdef".to_owned(),
                    revision: 7,
                    presented_revision: 7,
                    nodes: vec![node.clone()],
                },
                element: Some(node),
                match_count: 1,
                sample_count: 1,
                wait_count: 0,
            },
        }
    }

    fn pre_dispatch_failure(source: WindowActionFailure) -> PointerSequenceTransitionFailure {
        PointerSequenceTransitionFailure {
            source: PointerSequenceTransitionFailureSource::Sequence(PointerSequenceFailure {
                source,
                accepted_may_have_occurred: false,
                steps_accepted: 0,
                moves_accepted: 0,
                clicks_accepted: 0,
            }),
            accepted_may_have_occurred: false,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            steps_accepted: 0,
            moves_accepted: 0,
            clicks_accepted: 0,
        }
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let port = FixturePort {
            result: Ok(success(true)),
        };
        let Err(error) = perform_with(&port, "not-a-target", false, &Value::Null) else {
            panic!("未确认 pointer sequence transition 必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn pre_dispatch_remote_timeout_preserves_specific_error() {
        let port = FixturePort {
            result: Err(pre_dispatch_failure(WindowActionFailure::Transport(
                Failure::Timeout,
            ))),
        };
        let Err(error) = perform_with(&port, "s2:w:0123456789abcdef", true, &input()) else {
            panic!("dispatch 前的远端 timeout 必须失败");
        };
        assert_eq!(error.code, "TIMEOUT");
        assert_eq!(error.details["acceptedMayHaveOccurred"], false);
        assert_eq!(error.details["automaticRetryProhibited"], false);
    }

    #[test]
    fn unsettled_complete_sequence_can_succeed_with_trusted_postcondition() {
        let target = "s2:w:0123456789abcdef";
        let port = FixturePort {
            result: Ok(success(false)),
        };
        let Ok(result) = perform_with(&port, target, true, &input()) else {
            panic!("可信后置条件必须允许 settled=false 的完整序列成功");
        };
        assert_eq!(result["sequenceSettled"], false);
        assert_eq!(result["stepsAccepted"], 2);
        assert_eq!(result["allClicksBalanced"], true);
        assert_eq!(result["postconditionMatchedAfterDispatch"], true);
        assert_eq!(result["applicationConsumptionConfirmed"], false);
        assert_eq!(result["causalityConfirmed"], false);
        assert_eq!(result["safety"]["desktopPointerMoved"], false);
        let envelope = json!({
            "ok": true,
            "app": "app",
            "verb": "apply",
            "capability": CAPABILITY,
            "targetId": target,
            "executionRealm": "same-session-no-focus",
            "requiredExecutionRealm": "same-session-no-focus",
            "executionRealmCertified": true,
            "isolationRequirement": "standard",
            "hostImpactPolicy": "background-preferred",
            "data": result,
            "meta": {
                "foreground": {
                    "activationRequested": false,
                    "semanticPostconditionObservationRequested": true
                },
                "targeting": "opaque exact window generation"
            }
        });
        let Ok(schema) = serde_json::from_str::<Value>(include_str!(
            "../../contracts/v1/uix-pointer-sequence-transition-result.schema.json"
        )) else {
            panic!("指针序列 transition 结果 schema 必须解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "生产结果必须匹配指针序列 transition schema"
        );
    }

    #[test]
    fn partial_sequence_is_non_retryable_unknown() {
        let failure = PointerSequenceTransitionFailure {
            source: PointerSequenceTransitionFailureSource::Sequence(PointerSequenceFailure {
                source: WindowActionFailure::OutcomeUnknown,
                accepted_may_have_occurred: true,
                steps_accepted: 1,
                moves_accepted: 1,
                clicks_accepted: 0,
            }),
            accepted_may_have_occurred: true,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            steps_accepted: 1,
            moves_accepted: 1,
            clicks_accepted: 0,
        };
        let port = FixturePort {
            result: Err(failure),
        };
        let Err(error) = perform_with(&port, "s2:w:0123456789abcdef", true, &input()) else {
            panic!("部分序列必须失败");
        };
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["stepsAccepted"], 1);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }

    #[test]
    fn postcondition_ambiguity_remains_specific_and_non_retryable() {
        let failure = PointerSequenceTransitionFailure {
            source: PointerSequenceTransitionFailureSource::Postcondition(
                ElementWaitFailure::Ambiguous { match_count: 2 },
            ),
            accepted_may_have_occurred: true,
            sequence_completed: true,
            sequence_revision: Some(7),
            sequence_presented_revision: Some(7),
            sequence_settled: Some(false),
            steps_accepted: 2,
            moves_accepted: 1,
            clicks_accepted: 1,
        };
        let port = FixturePort {
            result: Err(failure),
        };
        let Err(error) = perform_with(&port, "s2:w:0123456789abcdef", true, &input()) else {
            panic!("动作后歧义必须失败");
        };
        assert_eq!(error.code, "AMBIGUOUS_TARGET");
        assert_eq!(error.details["matchCount"], 2);
        assert_eq!(error.details["allClicksBalanced"], true);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
