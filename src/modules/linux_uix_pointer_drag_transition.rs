//! 协调确认、请求内配平拖拽与完整 release 后的语义条件同步。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, ElementWaitFailure, PointerDragFailure, PointerDragTransitionFailure,
        PointerDragTransitionFailureSource, PointerDragTransitionOutcome, WindowActionFailure,
    },
    capabilities,
    components::uix_pointer_drag_transition_contract::UixPointerDragTransitionInput,
    domain::{AppControlError, AppResult},
    modules::{uix_element_wait, uix_window},
};

const CAPABILITY: &str = capabilities::UI_INPUT_POINTER_DRAG_TRANSITION;
const PROVIDER: &str = "uix-agent-v1";

trait UixPointerDragTransitionPort {
    fn drag(
        &self,
        session_id: &str,
        input: &UixPointerDragTransitionInput,
    ) -> Result<PointerDragTransitionOutcome, PointerDragTransitionFailure>;
}

struct SystemUixPointerDragTransition;

impl UixPointerDragTransitionPort for SystemUixPointerDragTransition {
    fn drag(
        &self,
        session_id: &str,
        input: &UixPointerDragTransitionInput,
    ) -> Result<PointerDragTransitionOutcome, PointerDragTransitionFailure> {
        uix_agent::perform_pointer_drag_transition(session_id, input)
    }
}

/// 对精确协作式窗口执行已确认的配平拖拽，并等待语义后置条件。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    perform_with(
        &SystemUixPointerDragTransition,
        session_id,
        confirmed,
        value,
    )
}

fn perform_with(
    port: &impl UixPointerDragTransitionPort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX pointer drag transition requires explicit confirmation.",
        ));
    }
    let input = UixPointerDragTransitionInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .drag(session_id, &input)
        .map_err(|failure| transition_error(failure, &input))?;
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
        "requestScopedBalancedButtons": true,
        "buttonReleaseConfirmed": true,
        "independentButtonOwnership": false,
        "x11Used": false,
        "fallback": "none",
    });
    let Value::Object(mut result) = json!({
        "ok": true,
        "contractVersion": "act/ui-pointer-drag-transition/v1",
        "capability": CAPABILITY,
        "targetId": session_id,
        "action": "drag",
        "button": "left",
        "coordinateSpace": input.coordinate_space().as_str(),
        "start": { "x": input.start().x(), "y": input.start().y() },
        "end": { "x": input.end().x(), "y": input.end().y() },
        "samplesRequested": input.samples(),
        "moveSamplesAccepted": outcome.drag.move_samples_accepted,
        "durationMs": input.duration_ms(),
        "pointerDownAccepted": true,
        "pointerUpAccepted": true,
        "buttonReleaseConfirmed": true,
        "requestScopedBalancedButtons": true,
        "dragRevision": outcome.drag.revision,
        "dragPresentedRevision": outcome.drag.presented_revision,
        "dragSettled": outcome.drag.settled,
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
        unreachable!("pointer drag transition 观察投影固定为 JSON 对象");
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
        "effectObservation": "semantic-postcondition-after-balanced-drag-release-no-causal-claim",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "same-session-no-focus",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety,
    }) else {
        unreachable!("pointer drag transition 执行投影固定为 JSON 对象");
    };
    result.extend(execution);
    Ok(Value::Object(result))
}

fn transition_error(
    failure: PointerDragTransitionFailure,
    input: &UixPointerDragTransitionInput,
) -> AppControlError {
    if let PointerDragTransitionFailureSource::Postcondition(ElementWaitFailure::Ambiguous {
        match_count,
    }) = failure.source
    {
        return AppControlError::with_details(
            "AMBIGUOUS_TARGET",
            "The UIX pointer drag transition postcondition matched multiple elements after a balanced drag completed; automatic retry is prohibited.",
            failure_details(
                &failure,
                input,
                "postcondition-ambiguous-after-balanced-drag",
                true,
                Some(match_count),
            ),
        );
    }
    if failure.accepted_may_have_occurred {
        let reason = if failure.drag_completed {
            "trusted-postcondition-lost-after-balanced-drag"
        } else {
            "partial-drag-or-release-state-indeterminate"
        };
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer drag transition may have executed without a trusted semantic postcondition; automatic retry is prohibited.",
            failure_details(&failure, input, reason, true, None),
        );
    }
    match failure.source {
        PointerDragTransitionFailureSource::Transport(source) => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(&failure, input, "transport-before-dispatch", false, None),
            );
            error
        }
        PointerDragTransitionFailureSource::NegotiationUnavailable => {
            AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The authenticated UIX Agent does not publish the complete drag and semantic revision synchronization surface.",
                failure_details(&failure, input, "agent-preflight-missing", false, None),
            )
        }
        PointerDragTransitionFailureSource::Drag(drag) => safe_drag_error(drag, &failure, input),
        PointerDragTransitionFailureSource::Postcondition(_) => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The balanced UIX pointer drag completed without a trusted semantic postcondition; automatic retry is prohibited.",
            failure_details(&failure, input, "postcondition-untrusted", true, None),
        ),
    }
}

fn safe_drag_error(
    drag: PointerDragFailure,
    failure: &PointerDragTransitionFailure,
    input: &UixPointerDragTransitionInput,
) -> AppControlError {
    match drag.source {
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
            "The authenticated UIX Agent does not publish the complete pointer drag action set.",
            failure_details(
                failure,
                input,
                "agent-drag-actions-not-published",
                false,
                None,
            ),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before pointer drag transition dispatch.",
            failure_details(failure, input, "window-revision-changed", false, None),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids application-internal pointer drag.",
            failure_details(failure, input, "application-policy-forbidden", false, None),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for pointer drag.",
            failure_details(failure, input, "window-not-presentable", false, None),
        ),
        WindowActionFailure::Transport(_)
        | WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer drag transition may have partially executed; automatic retry is prohibited.",
            failure_details(failure, input, "unexpected-unsafe-drag-failure", true, None),
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
    failure: &PointerDragTransitionFailure,
    input: &UixPointerDragTransitionInput,
    reason: &'static str,
    accepted_may_have_occurred: bool,
    match_count: Option<usize>,
) -> Value {
    json!({
        "action": "drag",
        "button": "left",
        "coordinateSpace": input.coordinate_space().as_str(),
        "start": { "x": input.start().x(), "y": input.start().y() },
        "end": { "x": input.end().x(), "y": input.end().y() },
        "samplesRequested": input.samples(),
        "durationMs": input.duration_ms(),
        "postcondition": input.postcondition().condition().as_str(),
        "selectorSemantics": "exact-and",
        "reason": reason,
        "dragCompleted": failure.drag_completed,
        "dragRevision": failure.drag_revision,
        "dragPresentedRevision": failure.drag_presented_revision,
        "dragSettled": failure.drag_settled,
        "pointerDownMayHaveOccurred": failure.pointer_down_may_have_occurred,
        "pointerDownAccepted": failure.pointer_down_accepted,
        "moveSamplesAccepted": failure.move_samples_accepted,
        "buttonReleaseAttempted": failure.button_release_attempted,
        "buttonReleaseConfirmed": failure.button_release_confirmed,
        "requestScopedBalancedButtons": failure.button_release_confirmed,
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
        ElementWaitOutcome, Failure, NodeRecord, PointerDragOutcome, RectRecord, Snapshot,
    };

    use super::*;

    struct FixturePort {
        result: Result<PointerDragTransitionOutcome, PointerDragTransitionFailure>,
    }

    impl UixPointerDragTransitionPort for FixturePort {
        fn drag(
            &self,
            _: &str,
            _: &UixPointerDragTransitionInput,
        ) -> Result<PointerDragTransitionOutcome, PointerDragTransitionFailure> {
            match &self.result {
                Ok(outcome) => Ok(PointerDragTransitionOutcome {
                    drag: outcome.drag,
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
            "start": { "x": 10.0, "y": 20.0 },
            "end": { "x": 30.0, "y": 40.0 },
            "samples": 2,
            "durationMs": 0,
            "timeoutMs": 1000,
            "postcondition": {
                "selector": { "automationId": "drag-target" },
                "condition": "unique"
            }
        })
    }

    fn success(settled: bool) -> PointerDragTransitionOutcome {
        let node = NodeRecord {
            native_id: "7:1".to_owned(),
            parent_native_id: None,
            automation_id: Some("drag-target".to_owned()),
            focused: false,
            role: "status".to_owned(),
            name: "拖拽目标".to_owned(),
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
        PointerDragTransitionOutcome {
            drag: PointerDragOutcome {
                revision: 8,
                presented_revision: 8,
                settled,
                move_samples_accepted: 2,
            },
            postcondition: ElementWaitOutcome {
                snapshot: Snapshot {
                    session_id: "s2:w:0123456789abcdef".to_owned(),
                    revision: 8,
                    presented_revision: 8,
                    nodes: vec![node.clone()],
                },
                element: Some(node),
                match_count: 1,
                sample_count: 1,
                wait_count: 0,
            },
        }
    }

    fn pre_dispatch_failure(source: WindowActionFailure) -> PointerDragTransitionFailure {
        PointerDragTransitionFailure {
            source: PointerDragTransitionFailureSource::Drag(PointerDragFailure {
                source,
                pointer_down_may_have_occurred: false,
                pointer_down_accepted: false,
                move_samples_accepted: 0,
                button_release_attempted: false,
                button_release_confirmed: false,
            }),
            accepted_may_have_occurred: false,
            drag_completed: false,
            drag_revision: None,
            drag_presented_revision: None,
            drag_settled: None,
            pointer_down_may_have_occurred: false,
            pointer_down_accepted: false,
            move_samples_accepted: 0,
            button_release_attempted: false,
            button_release_confirmed: false,
        }
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let port = FixturePort {
            result: Ok(success(true)),
        };
        let Err(error) = perform_with(&port, "not-a-target", false, &Value::Null) else {
            panic!("未确认 pointer drag transition 必须优先失败");
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
    fn unsettled_balanced_drag_can_succeed_with_trusted_postcondition() {
        let target = "s2:w:0123456789abcdef";
        let port = FixturePort {
            result: Ok(success(false)),
        };
        let Ok(result) = perform_with(&port, target, true, &input()) else {
            panic!("可信后置条件必须允许 settled=false 的配平拖拽成功");
        };
        assert_eq!(result["dragSettled"], false);
        assert_eq!(result["buttonReleaseConfirmed"], true);
        assert_eq!(result["postconditionMatchedAfterDispatch"], true);
        assert_eq!(result["applicationConsumptionConfirmed"], false);
        assert_eq!(result["causalityConfirmed"], false);
        assert_eq!(result["safety"]["independentButtonOwnership"], false);
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
            "../../contracts/v1/uix-pointer-drag-transition-result.schema.json"
        )) else {
            panic!("拖拽 transition 结果 schema 必须解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "生产结果必须匹配拖拽 transition schema"
        );
    }

    #[test]
    fn partial_drag_or_uncertain_release_is_non_retryable_unknown() {
        let failure = PointerDragTransitionFailure {
            source: PointerDragTransitionFailureSource::Drag(PointerDragFailure {
                source: WindowActionFailure::OutcomeUnknown,
                pointer_down_may_have_occurred: true,
                pointer_down_accepted: true,
                move_samples_accepted: 1,
                button_release_attempted: true,
                button_release_confirmed: false,
            }),
            accepted_may_have_occurred: true,
            drag_completed: false,
            drag_revision: None,
            drag_presented_revision: None,
            drag_settled: None,
            pointer_down_may_have_occurred: true,
            pointer_down_accepted: true,
            move_samples_accepted: 1,
            button_release_attempted: true,
            button_release_confirmed: false,
        };
        let port = FixturePort {
            result: Err(failure),
        };
        let Err(error) = perform_with(&port, "s2:w:0123456789abcdef", true, &input()) else {
            panic!("不确定 release 必须失败");
        };
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["buttonReleaseConfirmed"], false);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }

    #[test]
    fn postcondition_ambiguity_remains_specific_and_non_retryable() {
        let failure = PointerDragTransitionFailure {
            source: PointerDragTransitionFailureSource::Postcondition(
                ElementWaitFailure::Ambiguous { match_count: 2 },
            ),
            accepted_may_have_occurred: true,
            drag_completed: true,
            drag_revision: Some(8),
            drag_presented_revision: Some(8),
            drag_settled: Some(false),
            pointer_down_may_have_occurred: true,
            pointer_down_accepted: true,
            move_samples_accepted: 2,
            button_release_attempted: true,
            button_release_confirmed: true,
        };
        let port = FixturePort {
            result: Err(failure),
        };
        let Err(error) = perform_with(&port, "s2:w:0123456789abcdef", true, &input()) else {
            panic!("动作后歧义必须失败");
        };
        assert_eq!(error.code, "AMBIGUOUS_TARGET");
        assert_eq!(error.details["matchCount"], 2);
        assert_eq!(error.details["buttonReleaseConfirmed"], true);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
