//! 协调确认、完整按键序列与提交后的语义条件同步。

use serde_json::{Map, Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, ElementWaitFailure, KeySequenceFailure, KeySequenceTransitionFailure,
        KeySequenceTransitionFailureSource, KeySequenceTransitionOutcome, WindowActionFailure,
    },
    capabilities,
    components::uix_key_sequence_transition_contract::UixKeySequenceTransitionInput,
    domain::{AppControlError, AppResult},
    modules::{uix_element_wait, uix_window},
};

const CAPABILITY: &str = capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION;
const PROVIDER: &str = "uix-agent-v1";

/// Module 与系统 Adapter 之间只传递已验证的按键序列和中立结果。
trait UixKeySequenceTransitionPort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixKeySequenceTransitionInput,
    ) -> Result<KeySequenceTransitionOutcome, KeySequenceTransitionFailure>;
}

struct SystemUixKeySequenceTransition;

impl UixKeySequenceTransitionPort for SystemUixKeySequenceTransition {
    fn perform(
        &self,
        session_id: &str,
        input: &UixKeySequenceTransitionInput,
    ) -> Result<KeySequenceTransitionOutcome, KeySequenceTransitionFailure> {
        uix_agent::perform_key_sequence_transition(session_id, input)
    }
}

/// 对精确协作式窗口执行一次已确认的完整按键序列，并等待语义后置条件。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    perform_with(
        &SystemUixKeySequenceTransition,
        session_id,
        confirmed,
        value,
    )
}

fn perform_with(
    port: &impl UixKeySequenceTransitionPort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX key sequence transition requires explicit confirmation.",
        ));
    }
    let input = UixKeySequenceTransitionInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .perform(session_id, &input)
        .map_err(|failure| transition_error(failure, &input))?;
    debug_assert_eq!(
        outcome.sequence.presses_accepted as usize,
        input.presses_requested()
    );

    let presses = input
        .presses()
        .iter()
        .map(|press| {
            json!({
                "key": press.key(),
                "modifiers": press.modifiers(),
            })
        })
        .collect::<Vec<_>>();
    let KeySequenceTransitionOutcome {
        sequence,
        postcondition: observation,
    } = outcome;
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
        "applicationInternalEventsDispatched": true,
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
        "requestScopedBalancedPresses": true,
        "independentKeyOwnership": false,
        "textInputSupported": false,
        "keyHoldSupported": false,
        "keyRepeatSupported": false,
        "dragSupported": false,
        "doubleClickSupported": false,
        "clickCountSupported": false,
        "scrollSupported": false,
        "x11Used": false,
        "fallback": "none",
    });
    let Value::Object(mut result) = json!({
        "ok": true,
        "contractVersion": "act/ui-key-sequence-transition/v1",
        "capability": CAPABILITY,
        "targetId": session_id,
        "action": input.action(),
        "presses": presses,
        "pressesRequested": input.presses_requested(),
        "pressesAccepted": sequence.presses_accepted,
        "intervalMs": input.interval_ms(),
        "plannedDurationMs": input.planned_duration_ms(),
        "allPressesBalanced": true,
        "transactionSemantics": false,
        "rollbackSemantics": false,
        "sequenceRevision": sequence.revision,
        "sequencePresentedRevision": sequence.presented_revision,
        "sequenceSettled": sequence.settled,
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
        unreachable!("key sequence transition 观察投影固定为 JSON 对象");
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
        "effectObservation": "semantic-postcondition-after-complete-key-sequence-no-causal-claim",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "same-session-no-focus",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety,
    }) else {
        unreachable!("key sequence transition 执行投影固定为 JSON 对象");
    };
    result.extend(execution);
    Ok(Value::Object(result))
}

fn transition_error(
    failure: KeySequenceTransitionFailure,
    input: &UixKeySequenceTransitionInput,
) -> AppControlError {
    if let KeySequenceTransitionFailureSource::Postcondition(ElementWaitFailure::Ambiguous {
        match_count,
    }) = failure.source
    {
        let mut details = uix_agent::ambiguous_details(match_count);
        merge_failure_details(
            &mut details,
            failure_details(
                &failure,
                input,
                "postcondition-ambiguous-after-complete-sequence",
                true,
                Some(match_count),
            ),
        );
        return AppControlError::with_details(
            "AMBIGUOUS_TARGET",
            "The UIX key sequence transition postcondition matched multiple elements after the complete sequence; automatic retry is prohibited.",
            details,
        );
    }
    if failure.accepted_may_have_occurred {
        let reason = if failure.sequence_completed {
            "trusted-postcondition-lost-after-complete-sequence"
        } else {
            "partial-key-sequence-or-final-state-indeterminate"
        };
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX key sequence transition may have executed without a trusted semantic postcondition; automatic retry is prohibited.",
            failure_details(&failure, input, reason, true, None),
        );
    }

    match failure.source {
        KeySequenceTransitionFailureSource::Transport(source) => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(&failure, input, "transport-before-dispatch", false, None),
            );
            error
        }
        KeySequenceTransitionFailureSource::NegotiationUnavailable => {
            AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The authenticated UIX Agent does not publish the complete key sequence and semantic revision synchronization surface.",
                failure_details(&failure, input, "agent-preflight-missing", false, None),
            )
        }
        KeySequenceTransitionFailureSource::Sequence(sequence) => {
            safe_sequence_error(sequence, &failure, input)
        }
        KeySequenceTransitionFailureSource::Postcondition(_) => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The complete UIX key sequence lost its trusted semantic postcondition; automatic retry is prohibited.",
            failure_details(&failure, input, "postcondition-untrusted", true, None),
        ),
    }
}

fn safe_sequence_error(
    sequence: KeySequenceFailure,
    failure: &KeySequenceTransitionFailure,
    input: &UixKeySequenceTransitionInput,
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
            "The authenticated UIX Agent does not publish the requested key sequence actions or key catalog.",
            failure_details(
                failure,
                input,
                "agent-action-or-key-not-published",
                false,
                None,
            ),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before key sequence transition dispatch.",
            failure_details(failure, input, "window-revision-changed", false, None),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids the key sequence.",
            failure_details(failure, input, "application-policy-forbidden", false, None),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for key sequence transition dispatch.",
            failure_details(failure, input, "window-not-presentable", false, None),
        ),
        WindowActionFailure::Transport(_)
        | WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX key sequence transition may have partially executed; automatic retry is prohibited.",
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
    failure: &KeySequenceTransitionFailure,
    input: &UixKeySequenceTransitionInput,
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
        "pressesRequested".to_owned(),
        Value::from(input.presses_requested()),
    );
    details.insert(
        "pressesAccepted".to_owned(),
        Value::from(failure.presses_accepted),
    );
    details.insert("intervalMs".to_owned(), Value::from(input.interval_ms()));
    details.insert(
        "plannedDurationMs".to_owned(),
        Value::from(input.planned_duration_ms()),
    );
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
        "allPressesBalanced".to_owned(),
        Value::Bool(failure.sequence_completed),
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
                "read-current-semantic-snapshot-before-any-new-key-input"
            } else {
                "refresh-target-and-reassess"
            }
            .to_owned(),
        ),
    );
    details.insert("timeoutMs".to_owned(), Value::from(input.timeout_ms()));
    details.insert("desktopInputInjected".to_owned(), Value::Bool(false));
    details.insert("fallback".to_owned(), Value::String("none".to_owned()));
    Value::Object(details)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::adapters::linux::uix_agent::{
        ElementWaitOutcome, Failure, KeySequenceOutcome, NodeRecord, RectRecord, Snapshot,
    };

    use super::*;

    struct FixturePort {
        result: Result<KeySequenceTransitionOutcome, KeySequenceTransitionFailure>,
    }

    impl UixKeySequenceTransitionPort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixKeySequenceTransitionInput,
        ) -> Result<KeySequenceTransitionOutcome, KeySequenceTransitionFailure> {
            match &self.result {
                Ok(outcome) => Ok(KeySequenceTransitionOutcome {
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
            "presses": [
                { "key": "control" },
                { "key": "s", "modifiers": ["control"] }
            ],
            "intervalMs": 20,
            "timeoutMs": 1000,
            "postcondition": {
                "selector": { "automationId": "saved" },
                "condition": "unique"
            }
        })
    }

    fn success(settled: bool) -> KeySequenceTransitionOutcome {
        let node = NodeRecord {
            native_id: "7:1".to_owned(),
            parent_native_id: None,
            automation_id: Some("saved".to_owned()),
            focused: false,
            role: "status".to_owned(),
            name: "按键序列结果".to_owned(),
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
        KeySequenceTransitionOutcome {
            sequence: KeySequenceOutcome {
                revision: 7,
                presented_revision: 7,
                settled,
                presses_accepted: 2,
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

    fn pre_dispatch_failure(source: WindowActionFailure) -> KeySequenceTransitionFailure {
        KeySequenceTransitionFailure {
            source: KeySequenceTransitionFailureSource::Sequence(KeySequenceFailure {
                source,
                accepted_may_have_occurred: false,
                presses_accepted: 0,
            }),
            accepted_may_have_occurred: false,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            presses_accepted: 0,
        }
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let port = FixturePort {
            result: Ok(success(true)),
        };
        let Err(error) = perform_with(&port, "not-a-target", false, &Value::Null) else {
            panic!("未确认 key sequence transition 必须优先失败");
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
        assert_eq!(error.details["providerState"], "timeout");
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
            panic!("可信后置条件必须允许 settled=false 的完整按键序列成功");
        };
        assert_eq!(result["sequenceSettled"], false);
        assert_eq!(result["pressesAccepted"], 2);
        assert_eq!(result["allPressesBalanced"], true);
        assert_eq!(result["postconditionMatchedAfterDispatch"], true);
        assert_eq!(result["applicationConsumptionConfirmed"], false);
        assert_eq!(result["causalityConfirmed"], false);
        assert_eq!(
            result["safety"]["applicationInternalEventsDispatched"],
            true
        );
        assert_eq!(result["safety"]["desktopInputInjected"], false);

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
            "../../contracts/v1/uix-key-sequence-transition-result.schema.json"
        )) else {
            panic!("key sequence transition 结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "key sequence transition 结果必须匹配公开 schema"
        );
    }

    #[test]
    fn partial_sequence_is_unknown_and_non_retryable() {
        let Ok(parsed) = UixKeySequenceTransitionInput::parse(&input()) else {
            panic!("测试输入必须有效");
        };
        let failure = KeySequenceTransitionFailure {
            source: KeySequenceTransitionFailureSource::Sequence(KeySequenceFailure {
                source: WindowActionFailure::OutcomeUnknown,
                accepted_may_have_occurred: true,
                presses_accepted: 1,
            }),
            accepted_may_have_occurred: true,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            presses_accepted: 1,
        };
        let error = transition_error(failure, &parsed);
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["pressesAccepted"], 1);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }

    #[test]
    fn ambiguous_postcondition_is_specific_and_non_retryable() {
        let Ok(parsed) = UixKeySequenceTransitionInput::parse(&input()) else {
            panic!("测试输入必须有效");
        };
        let failure = KeySequenceTransitionFailure {
            source: KeySequenceTransitionFailureSource::Postcondition(
                ElementWaitFailure::Ambiguous { match_count: 2 },
            ),
            accepted_may_have_occurred: true,
            sequence_completed: true,
            sequence_revision: Some(7),
            sequence_presented_revision: Some(7),
            sequence_settled: Some(false),
            presses_accepted: 2,
        };
        let error = transition_error(failure, &parsed);
        assert_eq!(error.code, "AMBIGUOUS_TARGET");
        assert_eq!(error.details["matchCount"], 2);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
