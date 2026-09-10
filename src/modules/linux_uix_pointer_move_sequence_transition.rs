//! 协调确认、完整 pointer_move 序列与提交后的语义条件同步。

use serde_json::{Map, Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, ElementWaitFailure, PointerMoveSequenceFailure, PointerMoveSequenceTransitionFailure,
        PointerMoveSequenceTransitionFailureSource, PointerMoveSequenceTransitionOutcome,
        WindowActionFailure,
    },
    capabilities,
    components::uix_pointer_move_sequence_transition_contract::UixPointerMoveSequenceTransitionInput,
    domain::{AppControlError, AppResult},
    modules::{uix_element_wait, uix_window},
};

const CAPABILITY: &str = capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION;
const PROVIDER: &str = "uix-agent-v1";

/// Module 与系统 Adapter 之间只传递已验证的移动序列和中立结果。
trait UixPointerMoveSequenceTransitionPort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixPointerMoveSequenceTransitionInput,
    ) -> Result<PointerMoveSequenceTransitionOutcome, PointerMoveSequenceTransitionFailure>;
}

struct SystemUixPointerMoveSequenceTransition;

impl UixPointerMoveSequenceTransitionPort for SystemUixPointerMoveSequenceTransition {
    fn perform(
        &self,
        session_id: &str,
        input: &UixPointerMoveSequenceTransitionInput,
    ) -> Result<PointerMoveSequenceTransitionOutcome, PointerMoveSequenceTransitionFailure> {
        uix_agent::perform_pointer_move_sequence_transition(session_id, input)
    }
}

/// 对精确协作式窗口执行一次已确认的完整应用内 pointer_move 序列，并等待语义后置条件。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    perform_with(
        &SystemUixPointerMoveSequenceTransition,
        session_id,
        confirmed,
        value,
    )
}

fn perform_with(
    port: &impl UixPointerMoveSequenceTransitionPort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX pointer move sequence transition requires explicit confirmation.",
        ));
    }
    let input = UixPointerMoveSequenceTransitionInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .perform(session_id, &input)
        .map_err(|failure| transition_error(failure, &input))?;
    debug_assert_eq!(
        usize::from(outcome.sequence.moves_accepted),
        input.moves_requested()
    );

    let moves = input
        .moves()
        .iter()
        .map(|point| json!({ "x": point.x(), "y": point.y() }))
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
        "interpolationSemantics": false,
        "smoothingSemantics": false,
        "clickSupported": false,
        "keyPressSupported": false,
        "independentButtonOwnership": false,
        "dragSupported": false,
        "scrollSupported": false,
        "x11Used": false,
        "fallback": "none",
    });
    let Value::Object(mut result) = json!({
        "ok": true,
        "contractVersion": "act/ui-pointer-move-sequence-transition/v1",
        "capability": CAPABILITY,
        "targetId": session_id,
        "action": input.action(),
        "coordinateSpace": input.coordinate_space().as_str(),
        "moves": moves,
        "movesRequested": input.moves_requested(),
        "movesAccepted": outcome.sequence.moves_accepted,
        "intervalMs": input.interval_ms(),
        "plannedDurationMs": input.planned_duration_ms(),
        "transactionSemantics": false,
        "rollbackSemantics": false,
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
        unreachable!("pointer move sequence transition 观察投影固定为 JSON 对象");
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
        "effectObservation": "semantic-postcondition-after-complete-pointer-move-sequence-no-causal-claim",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "same-session-no-focus",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety,
    }) else {
        unreachable!("pointer move sequence transition 执行投影固定为 JSON 对象");
    };
    result.extend(execution);
    Ok(Value::Object(result))
}

fn transition_error(
    failure: PointerMoveSequenceTransitionFailure,
    input: &UixPointerMoveSequenceTransitionInput,
) -> AppControlError {
    if let PointerMoveSequenceTransitionFailureSource::Postcondition(
        ElementWaitFailure::Ambiguous { match_count },
    ) = failure.source
    {
        return AppControlError::with_details(
            "AMBIGUOUS_TARGET",
            "The UIX pointer move sequence transition postcondition matched multiple elements after the complete sequence; automatic retry is prohibited.",
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
            "partial-pointer-move-sequence-or-final-state-indeterminate"
        };
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer move sequence transition may have executed without a trusted semantic postcondition; automatic retry is prohibited.",
            failure_details(&failure, input, reason, true, None),
        );
    }

    match failure.source {
        PointerMoveSequenceTransitionFailureSource::Transport(source) => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(&failure, input, "transport-before-dispatch", false, None),
            );
            error
        }
        PointerMoveSequenceTransitionFailureSource::NegotiationUnavailable => {
            AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The authenticated UIX Agent does not publish pointer_move plus semantic revision synchronization for a pointer move sequence transition.",
                failure_details(&failure, input, "agent-preflight-missing", false, None),
            )
        }
        PointerMoveSequenceTransitionFailureSource::Sequence(sequence) => {
            safe_sequence_error(sequence, &failure, input)
        }
        PointerMoveSequenceTransitionFailureSource::Postcondition(_) => {
            AppControlError::with_details(
                "OUTCOME_UNKNOWN",
                "The complete UIX pointer move sequence lost its trusted semantic postcondition; automatic retry is prohibited.",
                failure_details(&failure, input, "postcondition-untrusted", true, None),
            )
        }
    }
}

fn safe_sequence_error(
    sequence: PointerMoveSequenceFailure,
    failure: &PointerMoveSequenceTransitionFailure,
    input: &UixPointerMoveSequenceTransitionInput,
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
            "The authenticated UIX Agent does not publish pointer_move.",
            failure_details(
                failure,
                input,
                "agent-pointer-move-not-published",
                false,
                None,
            ),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before pointer move sequence transition dispatch.",
            failure_details(failure, input, "window-revision-changed", false, None),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids the pointer move sequence.",
            failure_details(failure, input, "application-policy-forbidden", false, None),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for pointer move sequence input.",
            failure_details(failure, input, "window-not-presentable", false, None),
        ),
        WindowActionFailure::Transport(_)
        | WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer move sequence transition may have partially executed; automatic retry is prohibited.",
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
    failure: &PointerMoveSequenceTransitionFailure,
    input: &UixPointerMoveSequenceTransitionInput,
    reason: &'static str,
    accepted_may_have_occurred: bool,
    match_count: Option<usize>,
) -> Value {
    let mut details = Map::new();
    details.insert(
        "action".to_owned(),
        Value::String(input.action().to_owned()),
    );
    details.insert(
        "coordinateSpace".to_owned(),
        Value::String(input.coordinate_space().as_str().to_owned()),
    );
    details.insert(
        "movesRequested".to_owned(),
        Value::from(input.moves_requested()),
    );
    details.insert(
        "movesAccepted".to_owned(),
        Value::from(failure.moves_accepted),
    );
    details.insert("intervalMs".to_owned(), Value::from(input.interval_ms()));
    details.insert(
        "plannedDurationMs".to_owned(),
        Value::from(input.planned_duration_ms()),
    );
    details.insert("transactionSemantics".to_owned(), Value::Bool(false));
    details.insert("rollbackSemantics".to_owned(), Value::Bool(false));
    details.insert(
        "postcondition".to_owned(),
        Value::String(input.postcondition().condition().as_str().to_owned()),
    );
    details.insert(
        "selectorSemantics".to_owned(),
        Value::String("exact-and".to_owned()),
    );
    details.insert("reason".to_owned(), Value::String(reason.to_owned()));
    details.insert(
        "sequenceCompleted".to_owned(),
        Value::Bool(failure.sequence_completed),
    );
    details.insert(
        "sequenceRevision".to_owned(),
        failure.sequence_revision.map_or(Value::Null, Value::from),
    );
    details.insert(
        "sequencePresentedRevision".to_owned(),
        failure
            .sequence_presented_revision
            .map_or(Value::Null, Value::from),
    );
    details.insert(
        "sequenceSettled".to_owned(),
        failure.sequence_settled.map_or(Value::Null, Value::Bool),
    );
    details.insert(
        "matchCount".to_owned(),
        match_count.map_or(Value::Null, Value::from),
    );
    details.insert(
        "postconditionMatchedAfterDispatch".to_owned(),
        Value::Bool(false),
    );
    details.insert(
        "applicationConsumptionConfirmed".to_owned(),
        Value::Bool(false),
    );
    details.insert("causalityConfirmed".to_owned(), Value::Bool(false));
    details.insert("finalStateReached".to_owned(), Value::Bool(false));
    details.insert(
        "acceptedMayHaveOccurred".to_owned(),
        Value::Bool(accepted_may_have_occurred),
    );
    details.insert(
        "automaticRetryProhibited".to_owned(),
        Value::Bool(accepted_may_have_occurred),
    );
    details.insert("retrySafe".to_owned(), Value::Bool(false));
    details.insert(
        "requiredNextStep".to_owned(),
        Value::String(
            if accepted_may_have_occurred {
                "read-current-semantic-snapshot-before-any-new-pointer-move"
            } else {
                "refresh-target-and-reassess"
            }
            .to_owned(),
        ),
    );
    details.insert("timeoutMs".to_owned(), Value::from(input.timeout_ms()));
    details.insert("desktopInputInjected".to_owned(), Value::Bool(false));
    details.insert("desktopPointerMoved".to_owned(), Value::Bool(false));
    details.insert("fallback".to_owned(), Value::String("none".to_owned()));
    Value::Object(details)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::adapters::linux::uix_agent::{
        ElementWaitOutcome, Failure, NodeRecord, PointerMoveSequenceOutcome, RectRecord, Snapshot,
    };

    use super::*;

    struct FixturePort {
        result: Result<PointerMoveSequenceTransitionOutcome, PointerMoveSequenceTransitionFailure>,
    }

    impl UixPointerMoveSequenceTransitionPort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixPointerMoveSequenceTransitionInput,
        ) -> Result<PointerMoveSequenceTransitionOutcome, PointerMoveSequenceTransitionFailure>
        {
            match &self.result {
                Ok(outcome) => Ok(PointerMoveSequenceTransitionOutcome {
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
            "moves": [
                { "x": 10.0, "y": 20.0 },
                { "x": 30.0, "y": 40.0 }
            ],
            "intervalMs": 20,
            "timeoutMs": 1000,
            "postcondition": {
                "selector": { "automationId": "saved" },
                "condition": "unique"
            }
        })
    }

    fn success(settled: bool) -> PointerMoveSequenceTransitionOutcome {
        let node = NodeRecord {
            native_id: "7:1".to_owned(),
            parent_native_id: None,
            automation_id: Some("saved".to_owned()),
            focused: false,
            role: "status".to_owned(),
            name: "移动序列结果".to_owned(),
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
        PointerMoveSequenceTransitionOutcome {
            sequence: PointerMoveSequenceOutcome {
                revision: 7,
                presented_revision: 7,
                settled,
                moves_accepted: 2,
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
                sample_count: 2,
                wait_count: 1,
            },
        }
    }

    fn pre_dispatch_failure(source: WindowActionFailure) -> PointerMoveSequenceTransitionFailure {
        PointerMoveSequenceTransitionFailure {
            source: PointerMoveSequenceTransitionFailureSource::Sequence(
                PointerMoveSequenceFailure {
                    source,
                    accepted_may_have_occurred: false,
                    moves_accepted: 0,
                },
            ),
            accepted_may_have_occurred: false,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            moves_accepted: 0,
        }
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let port = FixturePort {
            result: Ok(success(true)),
        };
        let Err(error) = perform_with(&port, "not-a-target", false, &Value::Null) else {
            panic!("未确认 pointer move sequence transition 必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn pre_dispatch_timeout_preserves_specific_error() {
        let port = FixturePort {
            result: Err(pre_dispatch_failure(WindowActionFailure::Transport(
                Failure::Timeout,
            ))),
        };
        let Err(error) = perform_with(&port, "s2:w:0123456789abcdef", true, &input()) else {
            panic!("dispatch 前 timeout 必须失败");
        };
        assert_eq!(error.code, "TIMEOUT");
        assert_eq!(error.details["acceptedMayHaveOccurred"], false);
        assert_eq!(error.details["automaticRetryProhibited"], false);
    }

    #[test]
    fn complete_sequence_can_succeed_with_trusted_unsettled_postcondition() {
        let target = "s2:w:0123456789abcdef";
        let port = FixturePort {
            result: Ok(success(false)),
        };
        let Ok(result) = perform_with(&port, target, true, &input()) else {
            panic!("可信后置条件必须允许 settled=false 的完整移动序列成功");
        };
        assert_eq!(result["sequenceSettled"], false);
        assert_eq!(result["movesAccepted"], 2);
        assert_eq!(result["postconditionMatchedAfterDispatch"], true);
        assert_eq!(result["applicationConsumptionConfirmed"], false);
        assert_eq!(result["causalityConfirmed"], false);
        assert_eq!(result["safety"]["desktopPointerMoved"], false);
        assert_eq!(result["safety"]["clickSupported"], false);
    }

    #[test]
    fn partial_sequence_is_unknown_and_non_retryable() {
        let Ok(parsed) =
            crate::components::uix_pointer_move_sequence_transition_contract::UixPointerMoveSequenceTransitionInput::parse(&input())
        else {
            panic!("测试输入必须有效");
        };
        let failure = PointerMoveSequenceTransitionFailure {
            source: PointerMoveSequenceTransitionFailureSource::Sequence(
                PointerMoveSequenceFailure {
                    source: WindowActionFailure::OutcomeUnknown,
                    accepted_may_have_occurred: true,
                    moves_accepted: 1,
                },
            ),
            accepted_may_have_occurred: true,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            moves_accepted: 1,
        };
        let error = transition_error(failure, &parsed);
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["movesAccepted"], 1);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }

    #[test]
    fn ambiguous_postcondition_is_specific_and_non_retryable() {
        let Ok(parsed) =
            crate::components::uix_pointer_move_sequence_transition_contract::UixPointerMoveSequenceTransitionInput::parse(&input())
        else {
            panic!("测试输入必须有效");
        };
        let failure = PointerMoveSequenceTransitionFailure {
            source: PointerMoveSequenceTransitionFailureSource::Postcondition(
                ElementWaitFailure::Ambiguous { match_count: 2 },
            ),
            accepted_may_have_occurred: true,
            sequence_completed: true,
            sequence_revision: Some(7),
            sequence_presented_revision: Some(7),
            sequence_settled: Some(false),
            moves_accepted: 2,
        };
        let error = transition_error(failure, &parsed);
        assert_eq!(error.code, "AMBIGUOUS_TARGET");
        assert_eq!(error.details["matchCount"], 2);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
