//! 协调确认、精确语义动作与提交后元素条件同步。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, ElementTransitionFailure, ElementTransitionFailureSource, ElementTransitionOutcome,
        ElementWaitFailure,
    },
    capabilities,
    components::uix_element_transition_contract::UixElementTransitionInput,
    domain::{AppControlError, AppResult},
    modules::{uix_action, uix_element_wait, uix_window},
};

const CAPABILITY: &str = capabilities::UI_ELEMENT_TRANSITION;

trait UixElementTransitionPort {
    fn perform(
        &self,
        session_id: &str,
        input: &UixElementTransitionInput,
    ) -> Result<ElementTransitionOutcome, ElementTransitionFailure>;
}

struct SystemUixElementTransition;

impl UixElementTransitionPort for SystemUixElementTransition {
    fn perform(
        &self,
        session_id: &str,
        input: &UixElementTransitionInput,
    ) -> Result<ElementTransitionOutcome, ElementTransitionFailure> {
        uix_agent::perform_element_transition(session_id, input)
    }
}

/// 对 snapshot-scoped 精确元素执行已确认动作并等待提交后语义条件。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    perform_with(&SystemUixElementTransition, session_id, confirmed, value)
}

fn perform_with(
    port: &impl UixElementTransitionPort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 主人确认必须先于 input、target 和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX element transition requires explicit confirmation.",
        ));
    }
    let input = UixElementTransitionInput::parse(value)
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
        "provider": "uix-agent-v1",
        "providerCache": "disabled",
        "sameAuthenticatedConnectionUsed": true,
        "windowGenerationFixed": true,
        "revisionWaitUsed": revision_wait_used,
        "providerPolling": false,
        "stabilitySemantics": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "confirmIdentityExposed": false,
        "sensitiveSnapshotPublished": false,
        "hostCoordinateMappingPublished": false,
        "clickPointInferred": false,
        "foregroundActivationRequested": false,
        "desktopInputInjected": false,
        "desktopPointerMoved": false,
        "x11Used": false,
        "fallback": "none",
    });
    let Value::Object(mut result) = json!({
        "ok": true,
        "contractVersion": "act/ui-element-transition/v1",
        "capability": CAPABILITY,
        "targetId": session_id,
        "sourceSnapshotId": input.snapshot_id(),
        "sourceElementId": input.element_id(),
        "action": input.action().as_str(),
        "actionAccepted": true,
        "actionRevision": outcome.action.revision,
        "actionPresentedRevision": outcome.action.presented_revision,
        "actionSettled": outcome.action.settled,
        "applicationConfirmation": if outcome.action.application_confirmation_performed {
            "approved"
        } else {
            "not-required"
        },
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
        unreachable!("元素 transition 事实投影固定为 JSON 对象");
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
        "effectObservation": "semantic-postcondition-after-dispatch-no-causal-claim",
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout_ms(),
        "executionRealm": "same-session-no-focus",
        "readOnly": false,
        "mutationAllowed": true,
        "safety": safety,
    }) else {
        unreachable!("元素 transition 执行投影固定为 JSON 对象");
    };
    result.extend(execution);
    Ok(Value::Object(result))
}

fn transition_error(
    failure: ElementTransitionFailure,
    input: &UixElementTransitionInput,
) -> AppControlError {
    if let ElementTransitionFailureSource::Postcondition(ElementWaitFailure::Ambiguous {
        match_count,
    }) = failure.source
    {
        return AppControlError::with_details(
            "AMBIGUOUS_TARGET",
            "The UIX semantic postcondition matched more than one element after dispatch.",
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
            "The UIX element transition lost a trusted final after dispatch may have begun; automatic retry is prohibited.",
            failure_details(&failure, input, "trusted-final-lost", true, None),
        );
    }
    match failure.source {
        ElementTransitionFailureSource::Transport(source) => {
            let mut error = uix_window::public_error(source);
            merge_failure_details(
                &mut error.details,
                failure_details(&failure, input, "transport-before-dispatch", false, None),
            );
            error
        }
        ElementTransitionFailureSource::NegotiationUnavailable => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The authenticated UIX Agent does not publish the complete element transition contract.",
            failure_details(&failure, input, "agent-preflight-missing", false, None),
        ),
        ElementTransitionFailureSource::StaleElement => {
            uix_action::stale_element("The UIX semantic snapshot or element is no longer current.")
        }
        ElementTransitionFailureSource::AmbiguousSource => AppControlError::with_details(
            "AMBIGUOUS_TARGET",
            "The opaque UIX source element matched more than one current node.",
            failure_details(&failure, input, "source-element-ambiguous", false, None),
        ),
        ElementTransitionFailureSource::ElementDisabled => AppControlError::new(
            "ELEMENT_NOT_ENABLED",
            "The exact UIX semantic source element is disabled.",
        ),
        ElementTransitionFailureSource::NodeActionUnsupported => AppControlError::new(
            "ACTION_UNSUPPORTED",
            "The exact UIX semantic source element does not publish the requested action.",
        ),
        ElementTransitionFailureSource::Action(source) => {
            uix_action::action_error(source, input.action().as_str())
        }
        ElementTransitionFailureSource::Postcondition(ElementWaitFailure::Transport(_))
        | ElementTransitionFailureSource::Postcondition(ElementWaitFailure::Ambiguous { .. }) => {
            AppControlError::with_details(
                "OUTCOME_UNKNOWN",
                "The UIX element transition lost its postcondition observation after dispatch.",
                failure_details(&failure, input, "postcondition-untrusted", true, None),
            )
        }
    }
}

fn merge_failure_details(target: &mut Value, extra: Value) {
    let (Some(target), Some(extra)) = (target.as_object_mut(), extra.as_object()) else {
        return;
    };
    target.extend(extra.clone());
}

fn failure_details(
    failure: &ElementTransitionFailure,
    input: &UixElementTransitionInput,
    reason: &'static str,
    accepted_may_have_occurred: bool,
    match_count: Option<usize>,
) -> Value {
    json!({
        "action": input.action().as_str(),
        "sourceSnapshotId": input.snapshot_id(),
        "sourceElementId": input.element_id(),
        "postcondition": input.postcondition().condition().as_str(),
        "selectorPublished": false,
        "reason": reason,
        "actionAccepted": failure.action_accepted,
        "actionRevision": failure.action_revision,
        "actionPresentedRevision": failure.action_presented_revision,
        "actionSettled": failure.action_settled,
        "applicationConfirmationPerformed": failure.application_confirmation_performed,
        "sampleCount": failure.sample_count,
        "waitCount": failure.wait_count,
        "matchCount": match_count,
        "acceptedMayHaveOccurred": accepted_may_have_occurred,
        "automaticRetryProhibited": accepted_may_have_occurred,
        "retrySafe": false,
        "requiredNextStep": "read-current-uix-semantic-state-before-any-new-action",
    })
}

#[cfg(test)]
mod tests {
    use crate::adapters::linux::uix_agent::{
        ActionOutcome, ElementWaitOutcome, NodeRecord, RectRecord, Snapshot,
    };

    use super::*;

    struct FixturePort {
        result: Result<ElementTransitionOutcome, ElementTransitionFailure>,
    }

    impl UixElementTransitionPort for FixturePort {
        fn perform(
            &self,
            _: &str,
            _: &UixElementTransitionInput,
        ) -> Result<ElementTransitionOutcome, ElementTransitionFailure> {
            match &self.result {
                Ok(outcome) => Ok(outcome_fixture(
                    outcome.action.settled,
                    outcome.postcondition.wait_count,
                )),
                Err(failure) => Err(*failure),
            }
        }
    }

    fn input() -> Value {
        json!({
            "snapshotId": "as3:0123456789abcdef",
            "elementId": "s2:e:0123456789abcdef",
            "action": { "type": "invoke" },
            "postcondition": {
                "selector": { "automationId": "saved" },
                "condition": "unique"
            },
            "timeoutMs": 1000
        })
    }

    fn node() -> NodeRecord {
        NodeRecord {
            native_id: "7:2".to_owned(),
            parent_native_id: None,
            automation_id: Some("saved".to_owned()),
            focused: false,
            role: "status".to_owned(),
            name: "已保存".to_owned(),
            enabled: true,
            actions: Vec::new(),
            frame: RectRecord {
                x: 1.0,
                y: 2.0,
                width: 20.0,
                height: 10.0,
            },
            visible_bounds: None,
        }
    }

    fn outcome_fixture(settled: bool, wait_count: u32) -> ElementTransitionOutcome {
        ElementTransitionOutcome {
            action: ActionOutcome {
                revision: 6,
                presented_revision: 6,
                settled,
                application_confirmation_performed: false,
            },
            postcondition: ElementWaitOutcome {
                snapshot: Snapshot {
                    session_id: "s2:w:0123456789abcdef".to_owned(),
                    revision: 7,
                    presented_revision: 7,
                    nodes: Vec::new(),
                },
                element: Some(node()),
                match_count: 1,
                sample_count: 2,
                wait_count,
            },
        }
    }

    #[test]
    fn confirmation_precedes_input_target_and_provider() {
        let Err(error) = perform("not-a-target", false, &Value::Null) else {
            panic!("未确认元素 transition 必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn success_publishes_action_and_postcondition_without_causal_claim() {
        let port = FixturePort {
            result: Ok(outcome_fixture(false, 1)),
        };
        let Ok(result) = perform_with(&port, "s2:w:0123456789abcdef", true, &input()) else {
            panic!("元素 transition 成功事实必须投影");
        };
        assert_eq!(result["capability"], CAPABILITY);
        assert_eq!(result["action"], "invoke");
        assert_eq!(result["actionSettled"], false);
        assert_eq!(result["postcondition"], "unique");
        assert_eq!(result["postconditionMatchedAfterDispatch"], true);
        assert_eq!(result["matchCount"], 1);
        assert_eq!(result["revisionWaitUsed"], true);
        assert_eq!(result["causalityConfirmed"], false);
        assert_eq!(result["sameAuthenticatedConnectionUsed"], true);
        assert_eq!(result["safety"]["desktopInputInjected"], false);
    }

    #[test]
    fn postcondition_ambiguity_preserves_specific_error_and_prohibits_retry() {
        let failure = ElementTransitionFailure {
            source: ElementTransitionFailureSource::Postcondition(ElementWaitFailure::Ambiguous {
                match_count: 2,
            }),
            accepted_may_have_occurred: true,
            action_accepted: true,
            action_revision: Some(6),
            action_presented_revision: Some(6),
            action_settled: Some(true),
            application_confirmation_performed: Some(false),
            sample_count: None,
            wait_count: None,
        };
        let port = FixturePort {
            result: Err(failure),
        };
        let Err(error) = perform_with(&port, "s2:w:0123456789abcdef", true, &input()) else {
            panic!("动作后 postcondition 歧义必须失败");
        };
        assert_eq!(error.code, "AMBIGUOUS_TARGET");
        assert_eq!(error.details["acceptedMayHaveOccurred"], true);
        assert_eq!(error.details["automaticRetryProhibited"], true);
        assert_eq!(error.details["matchCount"], 2);
    }
}
