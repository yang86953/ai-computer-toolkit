//! 单个任务的状态、请求去重与结果责任；所有外部调用仍经统一 System。

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::{
    AppControlService, capabilities,
    components::cancellation,
    domain::{AppControlError, AppResult},
    service::SequencePostcondition,
};

use super::{
    execution, policy,
    protocol::{
        CONTRACT, MAX_REQUESTS, MAX_RESULT_BYTES, Operation, Request, TaskGrant, valid_request_id,
        valid_task_id,
    },
};

struct Receipt {
    request: Vec<u8>,
    response: Value,
}

pub(super) struct TaskSession {
    grant: TaskGrant,
    service: AppControlService,
    deadline: Instant,
    state: &'static str,
    executions: usize,
    result_bytes: usize,
    receipts: HashMap<String, Receipt>,
    close_requested: bool,
}

impl TaskSession {
    pub(super) fn new(grant: TaskGrant) -> AppResult<Self> {
        policy::validate_grant(&grant)?;
        Ok(Self {
            deadline: Instant::now() + Duration::from_millis(grant.total_timeout_ms),
            grant,
            service: AppControlService::new(),
            state: "ready",
            executions: 0,
            result_bytes: 0,
            receipts: HashMap::new(),
            close_requested: false,
        })
    }

    pub(super) fn task_id(&self) -> &str {
        &self.grant.task_id
    }
    pub(super) fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
    pub(super) fn is_closed(&self) -> bool {
        self.close_requested
    }

    pub(super) fn handle(&mut self, request: Request) -> Value {
        if request.contract_version != CONTRACT
            || !valid_request_id(&request.request_id)
            || request.task_id != self.grant.task_id
        {
            return failure(
                Some(&request),
                policy::error(
                    "INVALID_ARGUMENT",
                    "The request contract, identity or task binding is invalid.",
                ),
                "not-dispatched",
            );
        }
        let canonical = match serde_json::to_vec(&request) {
            Ok(value) => value,
            Err(_) => {
                return failure(
                    Some(&request),
                    policy::error(
                        "INVALID_ARGUMENT",
                        "The request cannot be represented as canonical JSON.",
                    ),
                    "not-dispatched",
                );
            }
        };
        if let Some(receipt) = self.receipts.get(&request.request_id) {
            return if receipt.request == canonical {
                receipt.response.clone()
            } else {
                failure(
                    Some(&request),
                    policy::error(
                        "REQUEST_ID_CONFLICT",
                        "A request identity cannot represent a different operation.",
                    ),
                    "not-dispatched",
                )
            };
        }
        let management = matches!(
            request.operation,
            Operation::Status | Operation::Cancel | Operation::Close
        );
        if self.receipts.len() >= MAX_REQUESTS && !management {
            return failure(
                Some(&request),
                policy::error(
                    "TASK_CAPACITY_EXHAUSTED",
                    "The task request ledger is full; existing receipts are retained.",
                ),
                "not-dispatched",
            );
        }
        let result = self.dispatch(&request.operation);
        let mut response = match result {
            Ok(data) => json!({
                "contractVersion": CONTRACT, "requestId": request.request_id, "taskId": request.task_id,
                "ok": true, "data": data,
            }),
            Err(error) => {
                let outcome = match error.details["outcome"].as_str() {
                    Some("completed") => "completed",
                    Some("failed") => "failed",
                    Some("unknown") => "unknown",
                    _ if error.code == "OUTCOME_UNKNOWN" => "unknown",
                    _ => "not-dispatched",
                };
                failure(Some(&request), error, outcome)
            }
        };
        let bytes = serde_json::to_vec(&response).map_or(MAX_RESULT_BYTES + 1, |bytes| bytes.len());
        if self.result_bytes.saturating_add(bytes) > MAX_RESULT_BYTES && !management {
            // 结果收集失败不把已执行的操作改成未派发，也不释放重复保护记录。
            response = json!({
                "contractVersion": CONTRACT, "requestId": request.request_id, "taskId": request.task_id,
                "ok": false, "error": { "code": "TASK_RESULT_BUDGET_EXCEEDED", "message": "The task result budget is exhausted.",
                    "details": { "resultOmitted": true, "requestSucceeded": response["ok"], "automaticRetryProhibited": true,
                        "outcome": response["data"]["execution"]["outcome"].as_str().or_else(|| response["error"]["details"]["outcome"].as_str()).unwrap_or("not-applicable") } }
            });
            if self.state != "outcome-unknown" {
                self.state = "result-budget-exhausted";
            }
        } else {
            self.result_bytes = self.result_bytes.saturating_add(bytes);
        }
        if self.receipts.len() < MAX_REQUESTS {
            self.receipts.insert(
                request.request_id.clone(),
                Receipt {
                    request: canonical,
                    response: response.clone(),
                },
            );
        }
        response
    }

    fn dispatch(&mut self, operation: &Operation) -> AppResult<Value> {
        match operation {
            Operation::Status => return Ok(self.status()),
            Operation::Cancel => {
                cancellation::request_cancellation();
                if self.state != "outcome-unknown" {
                    self.state = "cancelled";
                }
                return Ok(self.status());
            }
            Operation::Close => {
                cancellation::request_cancellation();
                self.close_requested = true;
                if self.state != "outcome-unknown" {
                    self.state = "closed";
                }
                return Ok(self.status());
            }
            _ => {}
        }
        if self.remaining().is_zero() {
            self.state = "expired";
            return Err(policy::error(
                "TASK_EXPIRED",
                "The task's original monotonic deadline has expired.",
            ));
        }
        if cancellation::is_cancelled() {
            return Err(policy::error(
                "CANCELLED",
                "The task has been cancelled; no further operation will be dispatched.",
            ));
        }
        if matches!(operation, Operation::Open) {
            if self.state != "ready" {
                return Err(policy::error(
                    "TASK_STATE_CONFLICT",
                    "The task was already opened.",
                ));
            }
            self.state = "active";
            return Ok(self.status());
        }
        let observation = matches!(
            operation,
            Operation::Catalog | Operation::Discover { .. } | Operation::Assess { .. }
        ) || matches!(operation, Operation::Execute { capability, .. } if policy::definition(capability).is_ok_and(|definition| !definition.action.mutates()));
        let reconciliation =
            matches!(self.state, "outcome-unknown" | "verification-failed") && observation;
        if self.state != "active" && !reconciliation {
            return Err(policy::error(
                "TASK_STATE_CONFLICT",
                "The task is not active.",
            ));
        }
        match operation {
            Operation::Catalog => Ok(json!({
                "capabilities": capabilities::ALL.iter().filter(|definition| definition.surface == capabilities::CapabilitySurface::App)
                    .map(|definition| json!({ "id": definition.id, "action": definition.action.as_str(),
                        "inputSchema": definition.input_schema, "guarantees": policy::guarantees(definition) })).collect::<Vec<_>>()
            })),
            Operation::Discover { scope } => {
                if !self.grant.allow_discovery {
                    return Err(policy::error(
                        "TASK_SCOPE_VIOLATION",
                        "Inventory discovery is not included in this task grant.",
                    ));
                }
                match scope {
                    super::protocol::DiscoveryScope::Applications => {
                        self.service.discover_app(64, 64, 64)
                    }
                    super::protocol::DiscoveryScope::Media => {
                        let request =
                            policy::read_request(capabilities::MEDIA_SESSION_DISCOVER_V3, None)?;
                        execution::execute(
                            &self.service,
                            request,
                            &[],
                            self.remaining(),
                            MAX_RESULT_BYTES.saturating_sub(self.result_bytes),
                        )
                    }
                }
            }
            Operation::Assess {
                capability,
                target_id,
            } => {
                policy::check_scope(&self.grant, capability, target_id, None)?;
                let definition = policy::definition(capability)?;
                let guarantees = policy::guarantees(definition);
                if guarantees["routeEligible"] != true {
                    return Ok(
                        json!({ "capability": capability, "targetId": target_id, "decision": "unavailable",
                        "guarantees": guarantees, "requirements": policy::requirements(), "fallback": "none" }),
                    );
                }
                if cfg!(target_os = "linux")
                    && matches!(
                        capability.as_str(),
                        capabilities::MEDIA_PLAYBACK_STATE_READ_V3
                            | capabilities::MEDIA_PLAYBACK_CONTROL_V3
                    )
                {
                    return super::media::assess(
                        &self.service,
                        &self.grant,
                        capability,
                        target_id,
                        self.remaining(),
                        MAX_RESULT_BYTES.saturating_sub(self.result_bytes),
                    );
                }
                let assessment = self.service.assess_capability(capability, target_id)?;
                let mut guarantees = guarantees;
                guarantees["eligible"] = json!(matches!(
                    assessment["decision"].as_str(),
                    Some("executable-background" | "confirmation-required")
                ));
                guarantees["assessmentRequired"] = json!(false);
                Ok(json!({ "assessment": assessment, "guarantees": guarantees }))
            }
            Operation::Execute {
                capability,
                target_id,
                input,
                postconditions,
            } => self.execute(capability, target_id, input, postconditions),
            _ => Err(policy::error(
                "TASK_STATE_CONFLICT",
                "The task operation is not valid in its current state.",
            )),
        }
    }

    fn execute(
        &mut self,
        capability: &str,
        target_id: &str,
        input: &serde_json::Map<String, Value>,
        postconditions: &[SequencePostcondition],
    ) -> AppResult<Value> {
        if self.executions >= self.grant.max_executions {
            return Err(policy::error(
                "TASK_CAPACITY_EXHAUSTED",
                "The task execution limit has been reached.",
            ));
        }
        let request = policy::authorize(&self.grant, capability, target_id, input)?;
        let mutating = policy::definition(capability)?.action.mutates();
        self.executions += 1;
        let result = execution::execute(
            &self.service,
            request,
            postconditions,
            self.remaining(),
            MAX_RESULT_BYTES.saturating_sub(self.result_bytes),
        );
        match result {
            Ok(mut data) => {
                data["capability"] = json!(capability);
                data["targetId"] = json!(target_id);
                Ok(data)
            }
            Err(error) => {
                if error.code == "OUTCOME_UNKNOWN" && mutating {
                    self.state = "outcome-unknown";
                } else if error.code == "TASK_VERIFICATION_FAILED" {
                    self.state = "verification-failed";
                }
                Err(error)
            }
        }
    }

    fn status(&self) -> Value {
        json!({ "state": self.state, "executions": self.executions, "maxExecutions": self.grant.max_executions,
            "remainingMs": self.remaining().as_millis(), "recordedRequests": self.receipts.len(),
            "authorizationSource": "task-preauthorization", "perActionPrompt": false,
            "requirements": policy::requirements(), "durableResumeSupported": false,
            "cancellationRequested": cancellation::is_cancelled(), "closed": self.close_requested,
            "expired": self.remaining().is_zero() })
    }
}

pub(super) fn failure(
    request: Option<&Request>,
    error: AppControlError,
    outcome: &'static str,
) -> Value {
    let mut result = json!({ "contractVersion": CONTRACT, "ok": false, "error": {
        "code": error.code, "message": error.message,
        "details": { "outcome": outcome, "automaticRetryProhibited": outcome != "not-dispatched", "cause": error.details }
    }});
    if let Some(request) = request
        && valid_request_id(&request.request_id)
        && valid_task_id(&request.task_id)
    {
        result["requestId"] = json!(request.request_id);
        result["taskId"] = json!(request.task_id);
    }
    result
}
