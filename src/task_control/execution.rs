//! 复用已有固定 worker Workflow 的取消、硬截止时间、收集预算和双阶段证据。

use std::time::Duration;

use serde_json::{Value, json};

use crate::{
    AppControlService,
    domain::{AppControlError, AppResult, CommandRequest},
    service::{SequenceInput, SequencePostcondition, SequenceStep},
};

use super::policy;

pub(super) fn execute(
    service: &AppControlService,
    request: CommandRequest,
    postconditions: &[SequencePostcondition],
    remaining: Duration,
    result_bytes: usize,
) -> AppResult<Value> {
    let timeout_ms = remaining.as_millis().min(30_000) as u32;
    if timeout_ms == 0 {
        return Err(policy::error(
            "TASK_EXPIRED",
            "The task expired before its worker could start.",
        ));
    }
    if result_bytes < 256 {
        return Err(policy::error(
            "TASK_RESULT_BUDGET_EXCEEDED",
            "There is no result budget for another task execution.",
        ));
    }
    let workflow = service.sequence(SequenceInput {
        steps: vec![SequenceStep {
            name: Some("task-operation".into()),
            verb: request.verb,
            app: request.app,
            operation: request.operation,
            target: request.target,
            args: request.args,
            max_items: request.max_items,
            max_depth: request.max_depth,
            confirmed: request.confirmed,
            foreground_consent: false,
            isolation_requirement: request.isolation_requirement,
            timeout_ms,
            postconditions: postconditions.to_vec(),
            preconditions: Vec::new(),
            bindings: Vec::new(),
        }],
        continue_on_error: false,
        max_result_bytes: result_bytes,
        total_timeout_ms: timeout_ms,
    })?;
    project(workflow, !postconditions.is_empty())
}

fn project(workflow: Value, assertions_requested: bool) -> AppResult<Value> {
    let record = &workflow["results"][0];
    let outcome = match record["execution"]["outcome"].as_str() {
        Some("not-dispatched") => "not-dispatched",
        Some("completed") => "completed",
        Some("failed") => "failed",
        _ => "unknown",
    };
    if workflow["ok"] == true && outcome == "completed" {
        return Ok(json!({
            "authorization": { "source": "task-preauthorization", "perActionPrompt": false },
            "execution": { "outcome": outcome, "automaticRetryProhibited": true },
            "verification": { "level": if assertions_requested { "provider-result-assertions" } else { "provider-result" }, "domainCommitVerified": false },
            "workflow": workflow,
        }));
    }
    let code = if outcome == "unknown" {
        "OUTCOME_UNKNOWN"
    } else if record["postconditions"]["passed"] == false
        || record["error"]["code"] == "TASK_VERIFICATION_FAILED"
    {
        "TASK_VERIFICATION_FAILED"
    } else if workflow["budget"]["exhausted"] == true {
        "TASK_RESULT_BUDGET_EXCEEDED"
    } else if record["error"]["code"] == "CANCELLED" {
        "CANCELLED"
    } else {
        "TASK_EXECUTION_FAILED"
    };
    Err(AppControlError::with_details(
        code,
        "The task operation did not satisfy its execution and verification contract.",
        json!({ "outcome": outcome, "automaticRetryProhibited": outcome != "not-dispatched", "workflow": workflow }),
    ))
}

#[cfg(test)]
mod tests {
    use super::project;
    use serde_json::json;

    #[test]
    fn completed_provider_with_failed_assertions_is_not_not_dispatched() {
        let failure = project(json!({ "ok": false, "results": [{ "ok": true, "execution": { "outcome": "completed" }, "postconditions": { "passed": false } }] }), true).expect_err("assertions failed");
        assert_eq!(failure.code, "TASK_VERIFICATION_FAILED");
        assert_eq!(failure.details["outcome"], "completed");
        assert_eq!(failure.details["automaticRetryProhibited"], true);
    }

    #[test]
    fn provider_state_verification_failure_retains_execution_evidence() {
        let failure = project(json!({"ok": false, "results": [{"ok": false,
            "execution": {"outcome": "failed"}, "error": {"code": "TASK_VERIFICATION_FAILED", "details": {"providerCompleted": true}}}]}), false).expect_err("状态验证未完成");
        assert_eq!(failure.code, "TASK_VERIFICATION_FAILED");
        assert_eq!(failure.details["outcome"], "failed");
        assert_eq!(
            failure.details["workflow"]["results"][0]["error"]["details"]["providerCompleted"],
            true
        );
    }

    #[test]
    fn accepted_without_a_final_receipt_preserves_unknown_outcome() {
        let failure = project(json!({ "ok": false, "results": [{ "ok": false, "execution": { "outcome": "unknown", "acceptedMayHaveOccurred": true }, "error": { "code": "OUTCOME_UNKNOWN" } }] }), false).expect_err("outcome is unknown");
        assert_eq!(failure.code, "OUTCOME_UNKNOWN");
        assert_eq!(failure.details["outcome"], "unknown");
    }

    #[test]
    fn successful_assertions_do_not_claim_business_commit() {
        let result = project(json!({ "ok": true, "results": [{ "ok": true, "execution": { "outcome": "completed" }, "postconditions": { "passed": true } }] }), true).expect("workflow succeeded");
        assert_eq!(
            result["verification"]["level"],
            "provider-result-assertions"
        );
        assert_eq!(result["verification"]["domainCommitVerified"], false);
    }
}
