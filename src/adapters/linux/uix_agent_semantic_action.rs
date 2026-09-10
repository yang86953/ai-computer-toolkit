//! UIX Agent 精确语义元素动作的私有 wire Adapter。

use serde_json::{Value, json};

use crate::components::uix_semantic_action_contract::UixSemanticAction;

use super::{ActionFailure, ActionOutcome, AgentClient, Failure, RequestFailure, WirePerformed};

impl AgentClient {
    /// 在同一认证连接内执行语义动作及可选应用确认。
    pub(super) fn perform(
        &mut self,
        window_id: u64,
        generation: u64,
        expected_revision: u64,
        native_node_id: &str,
        action: &UixSemanticAction,
    ) -> Result<ActionOutcome, ActionFailure> {
        if !self.request_types.contains("perform")
            || !self.request_types.contains("confirm")
            || !self.semantic_actions.contains(action.provider_action())
        {
            return Err(ActionFailure::UnsupportedAction);
        }
        let payload = json!({
            "window_id": window_id,
            "generation": generation,
            "expected_revision": expected_revision,
            "target": { "node_id": native_node_id },
            "action": action.provider_value(),
        });
        match self.request("act-perform", "perform", payload) {
            Ok(reply) => performed_outcome(reply, window_id, generation, expected_revision, false),
            Err(RequestFailure::Remote { code, confirm_id }) if code == "requires_confirmation" => {
                let confirm_id = confirm_id.ok_or(ActionFailure::Transport(Failure::Protocol))?;
                let reply = self
                    .request(
                        "act-confirm",
                        "confirm",
                        json!({ "window_id": window_id, "confirm_id": confirm_id }),
                    )
                    .map_err(action_request_failure)?;
                performed_outcome(reply, window_id, generation, expected_revision, true)
            }
            Err(error) => Err(action_request_failure(error)),
        }
    }
}

fn performed_outcome(
    reply: Value,
    window_id: u64,
    generation: u64,
    expected_revision: u64,
    application_confirmation_performed: bool,
) -> Result<ActionOutcome, ActionFailure> {
    let performed = serde_json::from_value::<WirePerformed>(reply)
        .map_err(|_| ActionFailure::Transport(Failure::Protocol))?;
    if performed.window_id != window_id
        || performed.generation != generation
        || performed.revision < expected_revision
        || performed.presented_revision > performed.revision
    {
        return Err(ActionFailure::Transport(Failure::Protocol));
    }
    Ok(ActionOutcome {
        revision: performed.revision,
        presented_revision: performed.presented_revision,
        settled: performed.settled,
        application_confirmation_performed,
    })
}

pub(super) fn action_request_failure(error: RequestFailure) -> ActionFailure {
    match error {
        // perform/confirm 帧开始发送后失去可信 final 时一律禁止自动重试。
        RequestFailure::Transport(_) => ActionFailure::OutcomeUnknown,
        RequestFailure::Remote { code, .. } => match code.as_str() {
            "unauthorized" => ActionFailure::Transport(Failure::PermissionDenied),
            "timeout" => ActionFailure::Transport(Failure::Timeout),
            "window_not_found" | "stale_window" => ActionFailure::Transport(Failure::Stale),
            // 请求发出后应用关闭不能证明动作未发生，禁止按 stale 自动重试。
            "app_closed" => ActionFailure::OutcomeUnknown,
            "stale_revision" => ActionFailure::StaleRevision,
            "node_not_found" => ActionFailure::ElementNotFound,
            "ambiguous_target" => ActionFailure::AmbiguousTarget,
            "unsupported_action" => ActionFailure::UnsupportedAction,
            "forbidden" => ActionFailure::Forbidden,
            "confirmation_rejected" => ActionFailure::ConfirmationRejected,
            "confirmation_not_found" | "requires_confirmation" => {
                ActionFailure::ConfirmationNotFound
            }
            "invalid_value" => ActionFailure::InvalidValue,
            "not_interactable" => ActionFailure::NotInteractable,
            "blocked" => ActionFailure::Blocked,
            "did_not_settle" => ActionFailure::DidNotSettle,
            "not_presentable" => ActionFailure::NotPresentable,
            "outcome_unknown" => ActionFailure::OutcomeUnknown,
            _ => ActionFailure::Transport(Failure::Protocol),
        },
    }
}
