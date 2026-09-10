//! 协调确认、普通左键 click 与提交后语义条件同步。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, ElementWaitFailure, PointerClickTransitionFailure,
        PointerClickTransitionFailureSource, PointerClickTransitionOutcome, WindowActionFailure,
    },
    capabilities,
    components::uix_pointer_click_transition_contract::UixPointerClickTransitionInput,
    domain::{AppControlError, AppResult},
    modules::{uix_element_wait, uix_window},
};

const CAPABILITY: &str = capabilities::UI_INPUT_POINTER_CLICK_TRANSITION;
const PROVIDER: &str = "uix-agent-v1";

trait UixPointerClickTransitionPort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixPointerClickTransitionInput,
    ) -> Result<PointerClickTransitionOutcome, PointerClickTransitionFailure>;
}

struct SystemUixPointerClickTransition;

impl UixPointerClickTransitionPort for SystemUixPointerClickTransition {
    fn perform(
        &self,
        session_id: &str,
        input: &UixPointerClickTransitionInput,
    ) -> Result<PointerClickTransitionOutcome, PointerClickTransitionFailure> {
        uix_agent::perform_pointer_click_transition(session_id, input)
    }
}

/// 对精确协作式窗口执行一次已确认的普通左键 click，并等待语义后置条件。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    perform_with(
        &SystemUixPointerClickTransition,
        session_id,
        confirmed,
        value,
    )
}

fn perform_with(
    port: &impl UixPointerClickTransitionPort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX pointer click transition requires explicit confirmation.",
        ));
    }
    let input = UixPointerClickTransitionInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = port
        .perform(session_id, &input)
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
        "requestScopedBalancedClicks": true,
        "independentButtonOwnership": false,
        "doubleClickSupported": false,
        "clickCountSupported": false,
        "x11Used": false,
        "fallback": "none",
    });
    let Value::Object(mut result) = json!({
        "ok": true,
        "contractVersion": "act/ui-pointer-click-transition/v1",
        "capability": CAPABILITY,
        "targetId": session_id,
        "action": "click",
        "coordinateSpace": input.coordinate_space().as_str(),
        "x": input.x(),
        "y": input.y(),
        "actionAccepted": true,
        "actionRevision": outcome.action.revision,
        "actionPresentedRevision": outcome.action.presented_revision,
        "actionSettled": outcome.action.settled,
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
        unreachable!("pointer click transition 观察投影固定为 JSON 对象");
    };
    let Value::Object(execution) = json!({
        "outcome": "completed",
        "dispatchState": "completed",
        "accepted": true,
        "sameAuthenticatedConnectionUsed": true,
        "effectConfirmed": false,
        "causalityConfirmed": false,
        "finalStateReached": false,
        "applicationPolicyEvaluated": true,
        "confirmationEvaluatedBeforeDiscovery": true,
        "hostForegroundActivationRequested": false,
        "effectObservation": "semantic-postcondition-after-click-dispatch-no-causal-claim",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "same-session-no-focus",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety,
    }) else {
        unreachable!("pointer click transition 执行投影固定为 JSON 对象");
    };
    result.extend(execution);
    Ok(Value::Object(result))
}

fn transition_error(
    failure: PointerClickTransitionFailure,
    input: &UixPointerClickTransitionInput,
) -> AppControlError {
    if let PointerClickTransitionFailureSource::Postcondition(ElementWaitFailure::Ambiguous {
        match_count,
    }) = failure.source
    {
        return AppControlError::with_details(
            "AMBIGUOUS_TARGET",
            "The UIX pointer click transition postcondition matched multiple elements after the click may have been dispatched; automatic retry is prohibited.",
            failure_details(
                &failure,
                input,
                "postcondition-ambiguous-after-dispatch",
                true,
                Some(match_count),
            ),
        );
    }
    if failure.accepted_may_have_occurred {
        return AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer click transition lost a trusted semantic postcondition after the click may have been dispatched; automatic retry is prohibited.",
            failure_details(&failure, input, "trusted-postcondition-lost", true, None),
        );
    }
    match failure.source {
        PointerClickTransitionFailureSource::Transport(source) => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(&failure, input, "transport-before-dispatch", false, None),
            );
            error
        }
        PointerClickTransitionFailureSource::NegotiationUnavailable => {
            AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The authenticated UIX Agent does not publish click_at plus semantic revision synchronization for a pointer click transition.",
                failure_details(&failure, input, "agent-preflight-missing", false, None),
            )
        }
        PointerClickTransitionFailureSource::Action(source) => {
            safe_action_error(source, &failure, input)
        }
        PointerClickTransitionFailureSource::Postcondition(_) => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer click may have been dispatched without a trusted postcondition.",
            failure_details(&failure, input, "postcondition-untrusted", true, None),
        ),
    }
}

fn safe_action_error(
    source: WindowActionFailure,
    failure: &PointerClickTransitionFailure,
    input: &UixPointerClickTransitionInput,
) -> AppControlError {
    match source {
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
            "The authenticated UIX Agent does not publish ordinary application-internal click_at.",
            failure_details(failure, input, "agent-click-at-not-published", false, None),
        ),
        WindowActionFailure::StaleRevision => AppControlError::with_details(
            "STALE_SESSION",
            "The UIX window changed before pointer click dispatch.",
            failure_details(failure, input, "window-revision-changed", false, None),
        ),
        WindowActionFailure::Forbidden => AppControlError::with_details(
            "PERMISSION_DENIED",
            "The UIX application policy forbids the pointer click.",
            failure_details(failure, input, "application-policy-forbidden", false, None),
        ),
        WindowActionFailure::NotPresentable => AppControlError::with_details(
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable for the pointer click.",
            failure_details(failure, input, "window-not-presentable", false, None),
        ),
        WindowActionFailure::Transport(_)
        | WindowActionFailure::NotInteractable
        | WindowActionFailure::WindowOperationFailed
        | WindowActionFailure::DidNotSettle
        | WindowActionFailure::OutcomeUnknown => AppControlError::with_details(
            "OUTCOME_UNKNOWN",
            "The UIX pointer click may have been dispatched without a trusted postcondition; automatic retry is prohibited.",
            failure_details(
                failure,
                input,
                "unexpected-unsafe-action-failure",
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
    failure: &PointerClickTransitionFailure,
    input: &UixPointerClickTransitionInput,
    reason: &'static str,
    accepted_may_have_occurred: bool,
    match_count: Option<usize>,
) -> Value {
    json!({
        "action": "click",
        "coordinateSpace": input.coordinate_space().as_str(),
        "x": input.x(),
        "y": input.y(),
        "postcondition": input.postcondition().condition().as_str(),
        "selectorSemantics": "exact-and",
        "reason": reason,
        "actionAccepted": failure.action_accepted,
        "actionRevision": failure.action_revision,
        "actionPresentedRevision": failure.action_presented_revision,
        "actionSettled": failure.action_settled,
        "sampleCount": failure.sample_count,
        "waitCount": failure.wait_count,
        "matchCount": match_count,
        "semanticPostconditionMatched": false,
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
        "automaticRetryProhibited": accepted_may_have_occurred,
        "retrySafe": false,
        "requiredNextStep": if accepted_may_have_occurred {
            "read-current-semantic-snapshot-before-any-new-pointer-click"
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
        ActionOutcome, ElementWaitOutcome, Failure, NodeRecord, RectRecord, Snapshot,
    };

    use super::*;

    struct FixturePort {
        result: Result<PointerClickTransitionOutcome, PointerClickTransitionFailure>,
    }

    impl UixPointerClickTransitionPort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixPointerClickTransitionInput,
        ) -> Result<PointerClickTransitionOutcome, PointerClickTransitionFailure> {
            match &self.result {
                Ok(outcome) => Ok(PointerClickTransitionOutcome {
                    action: outcome.action,
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
            "x": 10.0,
            "y": 20.0,
            "postcondition": {
                "selector": { "automationId": "saved" },
                "condition": "unique"
            },
            "timeoutMs": 1000
        })
    }

    fn success(settled: bool) -> PointerClickTransitionOutcome {
        let node = NodeRecord {
            native_id: "7:1".to_owned(),
            parent_native_id: None,
            automation_id: Some("saved".to_owned()),
            focused: false,
            role: "status".to_owned(),
            name: "saved".to_owned(),
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
        PointerClickTransitionOutcome {
            action: ActionOutcome {
                revision: 6,
                presented_revision: 6,
                settled,
                application_confirmation_performed: false,
            },
            postcondition: ElementWaitOutcome {
                snapshot: Snapshot {
                    session_id: "s2:w:0123456789abcdef".to_owned(),
                    revision: 6,
                    presented_revision: 6,
                    nodes: vec![node.clone()],
                },
                element: Some(node),
                match_count: 1,
                sample_count: 1,
                wait_count: 0,
            },
        }
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let port = FixturePort {
            result: Ok(success(true)),
        };
        let Err(error) = perform_with(&port, "not-a-target", false, &Value::Null) else {
            panic!("未确认 pointer click transition 必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn pre_dispatch_remote_timeout_preserves_specific_error() {
        let failure = PointerClickTransitionFailure {
            source: PointerClickTransitionFailureSource::Action(WindowActionFailure::Transport(
                Failure::Timeout,
            )),
            accepted_may_have_occurred: false,
            action_accepted: false,
            action_revision: None,
            action_presented_revision: None,
            action_settled: None,
            sample_count: None,
            wait_count: None,
        };
        let port = FixturePort {
            result: Err(failure),
        };
        let Err(error) = perform_with(&port, "s2:w:0123456789abcdef", true, &input()) else {
            panic!("dispatch 前的远端 timeout 必须失败");
        };
        assert_eq!(error.code, "TIMEOUT");
        assert_eq!(error.details["acceptedMayHaveOccurred"], false);
        assert_eq!(error.details["automaticRetryProhibited"], false);
    }

    #[test]
    fn unsettled_click_can_succeed_when_postcondition_is_trusted() {
        let port = FixturePort {
            result: Ok(success(false)),
        };
        let Ok(result) = perform_with(&port, "s2:w:0123456789abcdef", true, &input()) else {
            panic!("可信后置条件必须允许 settled=false 的 click 成功");
        };
        assert_eq!(result["action"], "click");
        assert_eq!(result["coordinateSpace"], "client-logical-px");
        assert_eq!(result["x"], 10.0);
        assert_eq!(result["y"], 20.0);
        assert_eq!(result["actionSettled"], false);
        assert_eq!(result["postconditionMatchedAfterDispatch"], true);
        assert_eq!(result["causalityConfirmed"], false);
        assert_eq!(result["finalStateReached"], false);
        assert_eq!(result["safety"]["desktopInputInjected"], false);
    }

    #[test]
    fn postcondition_ambiguity_remains_specific_and_non_retryable() {
        let failure = PointerClickTransitionFailure {
            source: PointerClickTransitionFailureSource::Postcondition(
                ElementWaitFailure::Ambiguous { match_count: 2 },
            ),
            accepted_may_have_occurred: true,
            action_accepted: true,
            action_revision: Some(6),
            action_presented_revision: Some(6),
            action_settled: Some(true),
            sample_count: Some(1),
            wait_count: Some(0),
        };
        let port = FixturePort {
            result: Err(failure),
        };
        let Err(error) = perform_with(&port, "s2:w:0123456789abcdef", true, &input()) else {
            panic!("动作后歧义必须失败");
        };
        assert_eq!(error.code, "AMBIGUOUS_TARGET");
        assert_eq!(error.details["matchCount"], 2);
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["automaticRetryProhibited"], true);
    }
}
