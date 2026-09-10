//! 把固定 sequence step worker 结果投影为 Workflow 可聚合的生命周期事实。

// 导入 provider-neutral JSON 值与构造器。
use serde_json::{Value, json};

// 导入执行投影依赖的窄 Components 与统一领域类型。
use crate::{
    // 导入取消、预算、协议、nonce 与 Job runner Components。
    components::{
        // 导入进程级取消观察。
        cancellation,
        // 导入当前步骤两级预算与停止事实。
        sequence_execution_budget::{
            SequenceDispatchPhase, SequenceStepBudget, SequenceStopReason,
        },
        // 导入严格 worker 请求与 final outcome。
        sequence_step_protocol::{
            SequenceStepWorkerCommand, SequenceStepWorkerRequest, frames::SequenceStepWorkerOutcome,
        },
    },
    // 导入统一请求、错误、结果与错误投影。
    domain::{AppControlError, AppResult, CommandRequest, error_json},
};

// Windows 由固定 sibling + Job 执行步骤。
#[cfg(target_os = "windows")]
use crate::components::{
    secure_nonce_windows::random_nonce,
    worker_process::sequence::{SequenceStepRunnerStop, run as run_sequence_step},
};

// Linux 由当前映像的固定隐藏入口和独立 process group 执行步骤。
#[cfg(target_os = "linux")]
use crate::components::{
    linux_sequence_worker::{SequenceStepRunnerStop, run as run_sequence_step},
    secure_nonce_linux::random_nonce,
};
// 保存一次固定 worker 步骤的业务结果与生命周期证据。
pub(super) struct SequenceExecutedStep {
    // 保存成功结果或完整公开错误对象。
    pub(super) result: Result<Value, Value>,
    // 保存不计入 provider 负载预算的双阶段证据。
    pub(super) evidence: Value,
    // 标记取消、deadline 或未知结果必须停止后续步骤。
    pub(super) hard_stop: bool,
}

// 执行一个已完成 Workflow 绑定的统一请求。
pub(super) fn execute_worker_step(
    // 取得最终 provider-neutral 请求。
    request: CommandRequest,
    // 借用当前步骤两级预算。
    budget: &SequenceStepBudget,
) -> AppResult<SequenceExecutedStep> {
    // 在任何 worker 资源创建前观察取消与 deadline。
    if let Some(stop) = budget.observe(
        // 使用当前单调时间。
        std::time::Instant::now(),
        // 读取进程级取消事实。
        cancellation::is_cancelled(),
        // worker 尚未 dispatch。
        SequenceDispatchPhase::BeforeDispatch,
    ) {
        // 返回确定未 dispatch 的硬停止结果。
        return Ok(stopped_step(stop.reason, false));
    }
    // 冻结紧邻 worker 创建前的第二次单调观察。
    let timeout_observed_at = std::time::Instant::now();
    // 计算总预算与步骤预算的较早剩余毫秒数。
    let Some(timeout_ms) = budget.remaining_timeout_ms(timeout_observed_at) else {
        // 极窄耗尽竞争必须投影为生命周期停止而不是参数错误。
        let reason = budget
            // 保留取消优先于同次 deadline 的契约。
            .observe(
                // 复用同一个单调观察点。
                timeout_observed_at,
                // 再次读取进程级取消事实。
                cancellation::is_cancelled(),
                // worker 尚未创建或 dispatch。
                SequenceDispatchPhase::BeforeDispatch,
            )
            // 取得已验证存在的停止原因。
            .map(|stop| stop.reason)
            // 理论平台时间漂移失败闭合为步骤 deadline。
            .unwrap_or(SequenceStopReason::StepDeadlineExceeded);
        // 返回确定未 dispatch 的硬停止结果。
        return Ok(stopped_step(reason, false));
    };
    // 把统一请求转换为严格 worker 命令。
    let command = SequenceStepWorkerCommand::from_request(request).map_err(|failure| {
        // 协议边界失败必须在 worker 前暴露稳定错误。
        AppControlError::new(
            // 使用协议封闭文本。
            failure.code().as_str(),
            // 不回显目标或参数。
            "The sequence step request exceeded its worker protocol boundary.",
        )
    })?;
    // 生成不包含目标、时间或身份事实的一次性 nonce。
    let request_nonce = random_nonce()?;
    // 构造严格 worker request。
    let worker_request = SequenceStepWorkerRequest::new(request_nonce, timeout_ms, command)
        // 理论构造漂移失败闭合。
        .map_err(|failure| {
            AppControlError::new(
                // 使用协议封闭文本。
                failure.code().as_str(),
                // 不回显请求。
                "The sequence step worker request could not be constructed.",
            )
        })?;
    // 运行固定 Job-bound worker 并实时保留 accepted 观察。
    let output = match run_sequence_step(&worker_request, cancellation::is_cancelled) {
        // 保存完成或强制回收后的观察。
        Ok(output) => output,
        // transport 失败按可证明阶段投影。
        Err(error) => return Ok(runner_failure_step(error)),
    };
    // 借用最后可靠帧观察。
    let observation = output.observation();
    // 正常 final 直接保留 worker 已验证的跨字段事实。
    if let Some(final_observation) = observation.final_observation() {
        // 构造 final 生命周期证据。
        let evidence = json!({
            // 使用稳定结果文本。
            "outcome": outcome_text(final_observation.outcome()),
            // 保留确定完成事实。
            "completed": final_observation.completed(),
            // 保留重试安全事实。
            "retrySafe": final_observation.retry_safe(),
            // 保留可能接受事实。
            "acceptedMayHaveOccurred": final_observation.accepted_may_have_occurred(),
            // final 是最后可靠观察。
            "lastReliableObservation": "final",
            // 正常或协作退出没有 transport 停止原因。
            "stopReason": stop_reason_text(output.stop()),
            // 公开是否由 parent 强制回收。
            "forcedReap": output.forced_reap(),
        });
        // 成功 final 必须携带结果。
        let result = match final_observation.outcome() {
            // 确定成功复制完整结果。
            SequenceStepWorkerOutcome::Completed => final_observation
                // 借用结果。
                .result()
                // 复制 provider-neutral JSON。
                .cloned()
                // 缺失结果使用稳定协议错误。
                .ok_or_else(protocol_result_missing),
            // 其他 final 必须携带错误。
            SequenceStepWorkerOutcome::NotDispatched
            | SequenceStepWorkerOutcome::Failed
            | SequenceStepWorkerOutcome::Unknown => Err(final_observation
                // 借用错误。
                .error()
                // 复制完整公开错误。
                .cloned()
                // 缺失错误使用稳定协议对象。
                .unwrap_or_else(protocol_error_payload)),
        };
        // unknown 或外部停止不得继续后续步骤。
        let hard_stop = final_observation.outcome() == SequenceStepWorkerOutcome::Unknown
            // 取消或 deadline 也必须停止。
            || output.stop() != SequenceStepRunnerStop::Completed;
        // 返回完整 final 投影。
        return Ok(SequenceExecutedStep {
            // 保存成功或错误对象。
            result,
            // 保存生命周期证据。
            evidence,
            // 保存硬停止事实。
            hard_stop,
        });
    }
    // 部分输出只可能来自取消或 deadline 强制回收。
    let phase = if observation.dispatch_accepted() {
        // accepted-only 证明 provider 可能已接受。
        SequenceDispatchPhase::AfterDispatch
    } else {
        // 零帧只能证明尚未 accepted。
        SequenceDispatchPhase::BeforeDispatch
    };
    // 重新观察两级预算以区分总 deadline 与步骤 deadline。
    let stop = budget
        // 使用回收后的单调时间。
        .observe(
            // 观察当前时刻。
            std::time::Instant::now(),
            // runner 明确选择取消时保持最高优先级。
            output.stop() == SequenceStepRunnerStop::Cancelled,
            // 保留最后可靠阶段。
            phase,
        )
        // runner deadline 极窄提前量内仍映射为步骤 deadline。
        .map(|observation| observation.reason)
        // 理论缺失使用 runner 原因补全。
        .unwrap_or_else(|| match output.stop() {
            // 取消保持最高优先级。
            SequenceStepRunnerStop::Cancelled => SequenceStopReason::Cancelled,
            // runner 只知道有效较早 deadline，缺省按步骤 deadline。
            SequenceStepRunnerStop::Deadline | SequenceStepRunnerStop::Completed => {
                SequenceStopReason::StepDeadlineExceeded
            }
        });
    // 返回零帧或 accepted-only 停止投影。
    Ok(stopped_step(stop, observation.dispatch_accepted()))
}

// 构造取消或 deadline 停止的步骤投影。
fn stopped_step(reason: SequenceStopReason, accepted: bool) -> SequenceExecutedStep {
    // dispatch 后停止必须保守报告 OutcomeUnknown。
    let outcome = if accepted {
        "unknown"
    } else {
        "not-dispatched"
    };
    // dispatch 后错误固定为 OutcomeUnknown。
    let error = if accepted {
        // 构造未知终态错误。
        json!({
            // 固定未知结果码。
            "code": "OUTCOME_UNKNOWN",
            // 不声称 provider 是否完成。
            "message": "The sequence step stopped after dispatch acceptance; its final outcome is unknown."
        })
    } else {
        // 构造确定未 dispatch 的停止错误。
        json!({
            // 按取消或两级 deadline 选择稳定错误码。
            "code": stop_error_code(reason),
            // 使用不含 provider 事实的固定消息。
            "message": "The sequence step stopped before dispatch acceptance."
        })
    };
    // 返回硬停止结果。
    SequenceExecutedStep {
        // 停止总是步骤失败。
        result: Err(error),
        // 保存机器可判定生命周期证据。
        evidence: json!({
            // 区分未 dispatch 与未知。
            "outcome": outcome,
            // transport 停止没有确定完成。
            "completed": false,
            // 仅确定未 dispatch 才允许修正后重试。
            "retrySafe": !accepted,
            // accepted 后可能已被 provider 接受。
            "acceptedMayHaveOccurred": accepted,
            // 区分零帧和 accepted-only。
            "lastReliableObservation": if accepted { "dispatch-accepted" } else { "before-dispatch" },
            // 公开取消或两级 deadline。
            "stopReason": stop_reason(reason),
            // 停止路径由 Job runner 负责回收。
            "forcedReap": true,
        }),
        // 取消和 deadline 永远停止后续步骤。
        hard_stop: true,
    }
}

// 把 runner 自身失败投影为保守步骤结果。
fn runner_failure_step(error: AppControlError) -> SequenceExecutedStep {
    // 只有明确启动前错误可证明未 dispatch。
    let before_dispatch = matches!(
        // 核对稳定错误码。
        error.code,
        // 固定 sibling 缺失。
        "COMPANION_WORKER_UNAVAILABLE"
            // 参数或序列化失败。
            | "INVALID_ARGUMENT"
            | "SERIALIZATION_FAILED"
            // 进程创建前后未产生 accepted 的启动失败。
            | "WORKER_START_FAILED"
    );
    // 取得完整统一错误对象。
    let mut payload = error_json(&error)["error"].clone();
    // 无法证明 dispatch 前时必须升级为 OutcomeUnknown。
    if !before_dispatch {
        // 覆盖为稳定未知结果对象。
        payload = json!({
            // 固定未知终态码。
            "code": "OUTCOME_UNKNOWN",
            // 不泄漏 worker transport 细节为业务终态。
            "message": "The sequence step worker failed after dispatch may have occurred."
        });
    }
    // 返回保守硬停止。
    SequenceExecutedStep {
        // 保存公开错误对象。
        result: Err(payload),
        // 保存最小 transport 证据。
        evidence: json!({
            // 区分可证明未 dispatch 与未知。
            "outcome": if before_dispatch { "not-dispatched" } else { "unknown" },
            // 没有可信 final。
            "completed": false,
            // 仅 dispatch 前失败允许修正后重试。
            "retrySafe": before_dispatch,
            // 非启动失败可能已被 provider 接受。
            "acceptedMayHaveOccurred": !before_dispatch,
            // 保守记录最后可靠观察。
            "lastReliableObservation": if before_dispatch { "before-dispatch" } else { "transport-unknown" },
            // 使用稳定 transport 停止原因。
            "stopReason": "worker-failure",
            // 启动前失败没有可回收进程，其余 transport 失败保守报告回收。
            "forcedReap": !before_dispatch,
        }),
        // transport 漂移不得继续执行后续步骤。
        hard_stop: true,
    }
}

// 向步骤记录插入固定 worker 生命周期证据。
pub(super) fn insert_worker_evidence(record: &mut Value, evidence: &Value) {
    // 步骤记录始终是对象。
    if let Some(object) = record.as_object_mut() {
        // 复制有界且不含 provider payload 的证据。
        object.insert("execution".to_owned(), evidence.clone());
    }
}

// 返回 final outcome 稳定文本。
const fn outcome_text(outcome: SequenceStepWorkerOutcome) -> &'static str {
    // 穷举封闭 outcome。
    match outcome {
        // 映射未 dispatch。
        SequenceStepWorkerOutcome::NotDispatched => "not-dispatched",
        // 映射确定成功。
        SequenceStepWorkerOutcome::Completed => "completed",
        // 映射确定失败。
        SequenceStepWorkerOutcome::Failed => "failed",
        // 映射未知终态。
        SequenceStepWorkerOutcome::Unknown => "unknown",
    }
}

// 返回 runner 停止原因的可选公开文本。
const fn stop_reason_text(stop: SequenceStepRunnerStop) -> Option<&'static str> {
    // 正常完成不声明停止。
    match stop {
        // 正常完成没有 stopReason。
        SequenceStepRunnerStop::Completed => None,
        // 映射取消。
        SequenceStepRunnerStop::Cancelled => Some("cancelled"),
        // runner 尚不能独自区分两级 deadline。
        SequenceStepRunnerStop::Deadline => Some("deadline-exceeded"),
    }
}

// 返回两级停止原因公开文本。
const fn stop_reason(reason: SequenceStopReason) -> &'static str {
    // 穷举封闭停止原因。
    match reason {
        // 映射调用方取消。
        SequenceStopReason::Cancelled => "cancelled",
        // 映射 Workflow 总 deadline。
        SequenceStopReason::WorkflowDeadlineExceeded => "workflow-deadline-exceeded",
        // 映射当前步骤 deadline。
        SequenceStopReason::StepDeadlineExceeded => "step-deadline-exceeded",
    }
}

// 返回 dispatch 前停止的稳定错误码。
const fn stop_error_code(reason: SequenceStopReason) -> &'static str {
    // 与 stopReason 一一对应。
    match reason {
        // 映射取消错误。
        SequenceStopReason::Cancelled => "CANCELLED",
        // 映射总 deadline 错误。
        SequenceStopReason::WorkflowDeadlineExceeded => "SEQUENCE_WORKFLOW_DEADLINE_EXCEEDED",
        // 映射步骤 deadline 错误。
        SequenceStopReason::StepDeadlineExceeded => "SEQUENCE_STEP_DEADLINE_EXCEEDED",
    }
}

// 构造缺失 final 负载时的稳定错误对象。
fn protocol_result_missing() -> Value {
    // 返回完整公开错误对象。
    protocol_error_payload()
}

// 构造 worker final 漂移错误对象。
fn protocol_error_payload() -> Value {
    // 不回显 worker 输出。
    json!({
        // 使用稳定协议错误码。
        "code": "WORKER_PROTOCOL_FAILED",
        // 使用固定消息。
        "message": "The sequence step worker final frame omitted its required payload."
    })
}

// 构造理论预算漂移错误。
pub(super) fn invalid_sequence_timeout(field: &'static str) -> AppControlError {
    // 返回稳定参数错误和字段定位。
    AppControlError::with_details(
        // 使用既有参数错误码。
        "INVALID_ARGUMENT",
        // 不回显调用方值。
        "The sequence timeout is outside its frozen boundary.",
        // 返回安全字段定位。
        json!({ "field": field }),
    )
}
