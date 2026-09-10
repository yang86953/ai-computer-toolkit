//! 协调确认、精确快照目标与 UIX Agent 语义 mutation。

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{self, ActionFailure, NodeRecord},
    components::uix_semantic_action_contract::UixSemanticActionInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

/// 对当前协作式窗口执行一次已确认精确元素动作。
pub(crate) fn perform(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    // 确认必须先于 input、target、发现、快照和任何 provider I/O。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX semantic element action requires explicit confirmation.",
        ));
    }
    let input = UixSemanticActionInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let snapshot = uix_agent::snapshot(session_id).map_err(uix_window::public_error)?;
    let current_snapshot_id = uix_window::snapshot_id(
        &snapshot.session_id,
        snapshot.revision,
        snapshot.presented_revision,
    );
    if input.snapshot_id != current_snapshot_id {
        return Err(stale_element(
            "The UIX semantic snapshot is no longer current.",
        ));
    }
    let matches = snapshot
        .nodes
        .iter()
        .filter(|node| {
            uix_window::public_node_id(&current_snapshot_id, &node.native_id) == input.element_id
        })
        .collect::<Vec<_>>();
    let node = match matches.as_slice() {
        [] => {
            return Err(stale_element(
                "The UIX semantic element is no longer current.",
            ));
        }
        [node] => *node,
        _ => {
            return Err(AppControlError::new(
                "AMBIGUOUS_TARGET",
                "The opaque UIX element matched more than one current node.",
            ));
        }
    };
    validate_node(node, &input)?;
    let outcome = uix_agent::perform(
        session_id,
        &node.native_id,
        snapshot.revision,
        &input.action,
        input.timeout_ms,
    )
    .map_err(|failure| action_error(failure, input.action.as_str()))?;
    Ok(json!({
        "action": input.action.as_str(),
        "outcome": "completed",
        "dispatchState": "completed",
        "acceptedMayHaveOccurred": true,
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "windowReResolved": true,
        "snapshotReResolved": true,
        "elementReResolved": true,
        "snapshotId": current_snapshot_id,
        "elementId": input.element_id,
        "revision": outcome.revision,
        "presentedRevision": outcome.presented_revision,
        "settled": outcome.settled,
        "applicationConfirmation": if outcome.application_confirmation_performed {
            "approved"
        } else {
            "not-required"
        },
        "confirmationEvaluatedBeforeDiscovery": true,
        "pointerFallbackUsed": false,
        "writePerformed": true,
        "executionRealm": "same-session-no-focus",
        "readOnly": false,
        "timeoutMs": input.timeout_ms,
        "safety": {
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "confirmIdentityExposed": false,
            "sensitiveSnapshotPublished": false,
            "hostForegroundActivationRequested": false,
            "desktopInputInjected": false,
            "fallback": "none",
        },
    }))
}

fn validate_node(node: &NodeRecord, input: &UixSemanticActionInput) -> AppResult<()> {
    if !node.enabled {
        return Err(AppControlError::new(
            "ELEMENT_NOT_ENABLED",
            "The exact UIX semantic element is disabled.",
        ));
    }
    if !node
        .actions
        .iter()
        .any(|action| action == input.action.provider_action())
    {
        return Err(AppControlError::new(
            "ACTION_UNSUPPORTED",
            "The exact UIX semantic element does not publish the requested action.",
        ));
    }
    Ok(())
}

pub(crate) fn stale_element(message: &'static str) -> AppControlError {
    AppControlError::with_details(
        "STALE_ELEMENT",
        message,
        json!({
            "provider": "uix-agent-v1",
            "retrySafe": true,
            "requiredNextStep": "read-a-new-accessibility-tree",
        }),
    )
}

pub(crate) fn action_error(failure: ActionFailure, action: &str) -> AppControlError {
    let ordinary = match failure {
        ActionFailure::Transport(failure) => return uix_window::public_error(failure),
        ActionFailure::StaleRevision | ActionFailure::ElementNotFound => {
            return stale_element("The UIX semantic element changed before dispatch.");
        }
        ActionFailure::AmbiguousTarget => (
            "AMBIGUOUS_TARGET",
            "The UIX provider could not resolve a unique semantic element.",
        ),
        ActionFailure::UnsupportedAction => (
            "ACTION_UNSUPPORTED",
            "The UIX provider does not support the requested semantic action.",
        ),
        ActionFailure::Forbidden => (
            "PERMISSION_DENIED",
            "The UIX application policy forbids the requested semantic action.",
        ),
        ActionFailure::ConfirmationRejected => (
            "CONFIRMATION_REJECTED",
            "The UIX application user rejected the semantic action.",
        ),
        ActionFailure::ConfirmationNotFound => (
            "CONFIRMATION_EXPIRED",
            "The UIX application confirmation expired or became unavailable.",
        ),
        ActionFailure::InvalidValue => (
            "INVALID_ARGUMENT",
            "The UIX application rejected the semantic action value.",
        ),
        ActionFailure::NotInteractable => (
            "ELEMENT_NOT_INTERACTABLE",
            "The exact UIX semantic element is not interactable.",
        ),
        ActionFailure::Blocked => (
            "ELEMENT_BLOCKED",
            "The exact UIX semantic element is blocked by application UI.",
        ),
        ActionFailure::DidNotSettle => {
            return AppControlError::with_details(
                "UI_DID_NOT_SETTLE",
                "The UIX action completed but the application did not settle.",
                terminal_details(action, "completed-unsettled"),
            );
        }
        ActionFailure::NotPresentable => (
            "WINDOW_NOT_PRESENTABLE",
            "The UIX application window is not presentable.",
        ),
        ActionFailure::OutcomeUnknown => {
            return AppControlError::with_details(
                "OUTCOME_UNKNOWN",
                "The UIX semantic action lost its trusted final after dispatch may have begun.",
                terminal_details(action, "unknown"),
            );
        }
    };
    AppControlError::new(ordinary.0, ordinary.1)
}

fn terminal_details(action: &str, outcome: &str) -> Value {
    json!({
        "action": action,
        "outcome": outcome,
        "dispatchState": "accepted-may-have-occurred",
        "acceptedMayHaveOccurred": true,
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "pointerFallbackUsed": false,
        "requiredNextStep": "read-current-accessibility-state",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::linux::uix_agent::Failure;

    #[test]
    fn confirmation_precedes_input_and_provider_parsing() {
        let error = perform("not-a-target", false, &Value::Null)
            .expect_err("unconfirmed mutation must fail first");
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn outcome_unknown_is_non_retryable() {
        let error = action_error(ActionFailure::OutcomeUnknown, "invoke");
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        assert_eq!(error.details["automaticRetryProhibited"], true);
        assert_eq!(error.details["retrySafe"], false);
    }

    #[test]
    fn transport_failure_before_action_uses_read_transport_projection() {
        let error = action_error(ActionFailure::Transport(Failure::Timeout), "invoke");
        assert_eq!(error.code, "TIMEOUT");
    }
}
