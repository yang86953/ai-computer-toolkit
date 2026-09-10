//! 实现 ComputerControlSystem 的版本化 sequence Workflow。
//
// 导入公开序列化契约派生。
use serde::{Deserialize, Serialize};
// 导入 JSON 值与构造宏。
use serde_json::{Value, json};
//
// 导入父模块拥有的 System 类型。
use super::AppControlService;
// 导入 Workflow 私有的跨步骤输入绑定。
use super::sequence_bindings::{
    SequenceBinding, apply_sequence_bindings, validate_sequence_bindings,
};
// 导入 sequence 绑定终止错误的稳定公开投影。
use super::sequence_binding_errors::sequence_binding_error;
// 导入 Workflow 私有的跨步骤前置条件判定。
use super::sequence_preconditions::{
    SequencePrecondition, evaluate_sequence_preconditions, sequence_precondition_error,
};
// 导入 Workflow 私有的本步骤后置条件判定。
use super::sequence_postconditions::{
    SequencePostcondition, evaluate_sequence_postconditions, insert_postcondition_error,
};
// 导入 Workflow 使用的窄 Component 和公共领域契约。
use crate::{
    // 导入 Workflow 使用的预算与结果 Components。
    components::{
        // 导入两级单调执行预算。
        sequence_execution_budget::{
            DEFAULT_SEQUENCE_STEP_TIMEOUT_MS as COMPONENT_DEFAULT_SEQUENCE_STEP_TIMEOUT_MS,
            DEFAULT_SEQUENCE_TOTAL_TIMEOUT_MS as COMPONENT_DEFAULT_SEQUENCE_TOTAL_TIMEOUT_MS,
            MAXIMUM_SEQUENCE_STEP_TIMEOUT_MS as COMPONENT_MAX_SEQUENCE_STEP_TIMEOUT_MS,
            MAXIMUM_SEQUENCE_TOTAL_TIMEOUT_MS as COMPONENT_MAX_SEQUENCE_TOTAL_TIMEOUT_MS,
            MINIMUM_SEQUENCE_STEP_TIMEOUT_MS as COMPONENT_MIN_SEQUENCE_STEP_TIMEOUT_MS,
            MINIMUM_SEQUENCE_TOTAL_TIMEOUT_MS as COMPONENT_MIN_SEQUENCE_TOTAL_TIMEOUT_MS,
            SequenceWorkflowBudget,
        },
        // 导入有界结果负载预算 Component。
        sequence_result_budget::{SequenceBudgetExceeded, SequenceResultBudget},
    },
    // 导入公开请求、错误、动词与 provider-neutral JSON Map。
    domain::{AppControlError, AppResult, CommandRequest, JsonMap, Verb},
};
//
// 扩展 ComputerControlSystem 的显式 sequence Workflow。
impl AppControlService {
    // 编排固定顺序、显式停止条件且不宣称事务性的公开工作流。
    pub fn sequence(&self, sequence: SequenceInput) -> AppResult<Value> {
        // 在任何 provider 调用前验证步骤数与结果预算边界。
        sequence.validate()?;
        // 保存调用方请求的总步骤数。
        let total = sequence.steps.len();
        // 冻结是否允许普通执行错误后继续的策略。
        let continue_on_error = sequence.continue_on_error;
        // 创建只统计本次工作流 provider 负载的预算状态。
        let mut budget = SequenceResultBudget::new(sequence.max_result_bytes);
        // 创建不因步骤切换而重置的单调总执行预算。
        let workflow_budget = SequenceWorkflowBudget::new(sequence.total_timeout_ms)
            // validate 已保证边界，理论漂移仍失败闭合。
            .ok_or_else(|| invalid_sequence_timeout("totalTimeoutMs"))?;
        // 预留受硬上限约束的结果条目空间。
        let mut results = Vec::with_capacity(total);
        // 初始尚未发生工作流级预算错误。
        let mut budget_exhausted = false;
        // 初始尚未发生条件或结果预算工作流错误。
        let mut workflow_error_count = 0usize;
        // 初始没有发生阻止当前步骤启动的前置条件错误。
        let mut terminal_workflow_error = None;
        // 按输入顺序逐步执行，System 保持流程决定权。
        for (index, step) in sequence.steps.into_iter().enumerate() {
            // 保存可选步骤名称用于公开证据。
            let name = step.name.clone();
            // 在当前 provider 启动前读取已建立的更早步骤结果。
            let precondition_evaluation =
                evaluate_sequence_preconditions(&results, &step.preconditions);
            // 前置条件失败是不可由 continueOnError 放宽的工作流错误。
            if let Some(failure) = precondition_evaluation.failure {
                // 构造不伪装成 provider 失败的顶层终止错误。
                terminal_workflow_error = Some(sequence_precondition_error(
                    // 使用一基当前步骤索引。
                    index + 1,
                    // 保留调用方可选名称。
                    name,
                    // 传递首个失败条件定位。
                    failure,
                    // 传递不含实际值或期望值的判定证据。
                    precondition_evaluation.evidence,
                ));
                // 工作流级错误只计入一次。
                workflow_error_count += 1;
                // 当前步骤没有启动，后续步骤必须全部停止。
                break;
            }
            // 保存通过的可选前置条件证据用于当前步骤记录。
            let precondition_evidence = precondition_evaluation.evidence;
            // 保存当前步骤 deadline，随后才消费公开步骤对象。
            let step_timeout_ms = step.timeout_ms;
            // 分离统一请求和仅由 Workflow 消费的绑定与后置条件。
            let (mut request, bindings, postconditions) = step.into_request_and_workflow_fields();
            // 把更早成功结果的有界值写入最终 provider-neutral 请求。
            let binding_evaluation = apply_sequence_bindings(&results, &mut request, &bindings);
            // 绑定失败是不可由 continueOnError 放宽的工作流错误。
            if let Some(failure) = binding_evaluation.failure {
                // 构造不伪装成 provider 失败的顶层终止错误。
                terminal_workflow_error = Some(sequence_binding_error(
                    // 使用一基当前步骤索引。
                    index + 1,
                    // 保留调用方可选名称。
                    name,
                    // 传递首个失败绑定定位。
                    failure,
                ));
                // 工作流级错误只计入一次。
                workflow_error_count += 1;
                // 当前步骤没有启动，后续步骤必须全部停止。
                break;
            }
            // 保存成功应用的可选绑定证据用于当前步骤记录。
            let binding_evidence = binding_evaluation.evidence;
            // 从总预算派生当前步骤自己的 deadline。
            let step_budget = workflow_budget
                // 使用调用方步骤预算。
                .begin_step(step_timeout_ms)
                // validate 已保证边界，理论漂移仍失败闭合。
                .ok_or_else(|| invalid_sequence_timeout("steps[].timeoutMs"))?;
            // 执行固定 Job-bound worker 并投影最后可靠观察。
            let executed = execute_worker_step(request, &step_budget)?;
            // 拆分 worker 业务结果、生命周期证据与硬停止事实。
            let SequenceExecutedStep {
                // 取得确定或保守结果。
                result: executed_result,
                // 取得有界生命周期证据。
                evidence: worker_evidence,
                // 取得不可放宽停止事实。
                hard_stop,
            } = executed;
            // 执行仍经过统一策略评估的固定 worker 结果。
            match executed_result {
                // 处理已经完成并成功的 provider 结果。
                Ok(result) => {
                    // provider 成功后立即对完整内存结果执行有界断言。
                    let postcondition_evaluation =
                        evaluate_sequence_postconditions(&result, &postconditions);
                    // 只在完整结果负载能纳入预算时公开它。
                    match budget.reserve(&result) {
                        // 预算内结果可以同时返回真实结果和断言证据。
                        Ok(_) => {
                            // 先构造保持 provider 成功事实的步骤记录。
                            let mut record = json!({
                                // 使用一基步骤索引。
                                "index": index + 1,
                                // 保留调用方可选名称。
                                "name": name,
                                // 后置断言不得改写 provider 成功事实。
                                "ok": true,
                                // 返回完整且未截断的 provider 结果。
                                "result": result,
                            });
                            // 附加双阶段 worker 的可靠生命周期证据。
                            insert_worker_evidence(&mut record, &worker_evidence);
                            // 仅在调用方声明断言时附加评估证据。
                            insert_postcondition_evidence(
                                // 修改刚构造的步骤记录。
                                &mut record,
                                // 借用不含实际值或期望值的证据。
                                postcondition_evaluation.evidence.as_ref(),
                            );
                            // 仅在调用方声明前置条件时附加通过证据。
                            insert_condition_evidence(
                                // 修改同一个步骤记录。
                                &mut record,
                                // 使用公开前置条件字段名。
                                "preconditions",
                                // 借用有界通过证据。
                                precondition_evidence.as_ref(),
                            );
                            // 仅在调用方声明绑定时附加应用证据。
                            insert_condition_evidence(
                                // 修改同一个步骤记录。
                                &mut record,
                                // 使用公开绑定字段名。
                                "bindings",
                                // 借用有界应用证据。
                                binding_evidence.as_ref(),
                            );
                            // 断言失败是不可由 continueOnError 放宽的工作流错误。
                            if let Some(failure) = postcondition_evaluation.failure {
                                // 附加稳定错误码和最小失败定位信息。
                                insert_postcondition_error(&mut record, failure);
                                // 工作流级错误只计入条件或预算，不计 provider 失败。
                                workflow_error_count += 1;
                                // 保存已成功完成的步骤事实。
                                results.push(record);
                                // 后置断言失败必须停止所有后续步骤。
                                break;
                            }
                            // 全部后置断言通过时保存成功步骤。
                            results.push(record);
                            // 极窄 final/停止竞争仍必须计为工作流生命周期错误。
                            if hard_stop {
                                // 生命周期停止只计一次并覆盖继续策略。
                                workflow_error_count += 1;
                                // 不执行任何后续步骤。
                                break;
                            }
                        }
                        // 超预算时保留“步骤成功、结果省略”的真实语义。
                        Err(exceeded) => {
                            // 添加不计入结果预算的紧凑工作流错误记录。
                            results.push(sequence_budget_record(
                                // 使用一基步骤索引。
                                index + 1,
                                // 保留调用方可选名称。
                                name,
                                // provider 步骤已经成功。
                                true,
                                // 报告结果负载被省略。
                                "resultOmitted",
                                // 传递窄 Component 的预算证据。
                                exceeded,
                                // 聚合不计入 provider 负载预算的 Workflow 证据。
                                SequenceStepEvidence {
                                    // 保留 worker 双阶段生命周期证据。
                                    worker: Some(worker_evidence.clone()),
                                    // 保留 provider 启动前已通过的前置条件证据。
                                    preconditions: precondition_evidence,
                                    // 保留 provider 启动前已应用的绑定证据。
                                    bindings: binding_evidence,
                                    // 保留先于预算判定完成的有界断言证据。
                                    postconditions: postcondition_evaluation.evidence,
                                },
                            ));
                            // 标记整体工作流未能完整收集结果。
                            budget_exhausted = true;
                            // 预算错误优先对外报告并只计一次工作流错误。
                            workflow_error_count += 1;
                            // 资源预算是硬停止条件，不受 continueOnError 放宽。
                            break;
                        }
                    }
                }
                // 处理已经完成但失败的 provider 步骤。
                Err(error_payload) => {
                    // 错误负载与成功结果共享同一个总预算。
                    match budget.reserve(&error_payload) {
                        // 预算内错误保持现有步骤失败语义。
                        Ok(_) => {
                            // 构造保持 provider 失败事实的步骤记录。
                            let mut record = json!({
                                // 使用一基步骤索引。
                                "index": index + 1,
                                // 保留调用方可选名称。
                                "name": name,
                                // 报告 provider 步骤实际失败。
                                "ok": false,
                                // 返回完整公开错误负载。
                                "error": error_payload,
                            });
                            // 附加双阶段 worker 的可靠生命周期证据。
                            insert_worker_evidence(&mut record, &worker_evidence);
                            // 失败发生在 provider 内，因此前置条件已经通过。
                            insert_condition_evidence(
                                // 修改刚构造的失败步骤记录。
                                &mut record,
                                // 使用公开前置条件字段名。
                                "preconditions",
                                // 借用有界通过证据。
                                precondition_evidence.as_ref(),
                            );
                            // 失败发生在 provider 内，因此绑定已经成功应用。
                            insert_condition_evidence(
                                // 修改刚构造的失败步骤记录。
                                &mut record,
                                // 使用公开绑定字段名。
                                "bindings",
                                // 借用有界应用证据。
                                binding_evidence.as_ref(),
                            );
                            // 保存完整 provider 失败事实。
                            results.push(record);
                        }
                        // 超预算时保留“步骤失败、错误省略”的真实语义。
                        Err(exceeded) => {
                            // 添加不计入结果预算的紧凑工作流错误记录。
                            results.push(sequence_budget_record(
                                // 使用一基步骤索引。
                                index + 1,
                                // 保留调用方可选名称。
                                name,
                                // provider 步骤已经失败。
                                false,
                                // 报告错误负载被省略。
                                "errorOmitted",
                                // 传递窄 Component 的预算证据。
                                exceeded,
                                // 聚合不计入 provider 错误预算的 Workflow 证据。
                                SequenceStepEvidence {
                                    // 保留 worker 双阶段生命周期证据。
                                    worker: Some(worker_evidence.clone()),
                                    // 保留 provider 启动前已通过的前置条件证据。
                                    preconditions: precondition_evidence,
                                    // 保留 provider 启动前已应用的绑定证据。
                                    bindings: binding_evidence,
                                    // provider 失败时不存在可评估的后置条件。
                                    postconditions: None,
                                },
                            ));
                            // 标记整体工作流未能完整收集错误。
                            budget_exhausted = true;
                            // 预算错误优先对外报告并只计一次工作流错误。
                            workflow_error_count += 1;
                            // 资源预算是硬停止条件，不受 continueOnError 放宽。
                            break;
                        }
                    }
                    // 取消、deadline 或未知终态必须停止全部后续步骤。
                    if hard_stop {
                        // 生命周期停止在 provider 失败之外单独计入工作流错误。
                        workflow_error_count += 1;
                        // 不允许 continueOnError 放宽生命周期停止。
                        break;
                    }
                    // 普通步骤失败只按调用方显式策略决定是否继续。
                    if !continue_on_error {
                        // 停止执行剩余步骤且不宣称回滚。
                        break;
                    }
                }
            }
        }
        // 统计真实 provider 步骤失败，不把成功后结果省略算作执行失败。
        let failed_count = results
            // 遍历每个已执行步骤记录。
            .iter()
            // 只保留明确报告 ok=false 的步骤。
            .filter(|item| item.get("ok") == Some(&Value::Bool(false)))
            // 取得 provider 失败数量。
            .count();
        // 构造包含版本、部分执行事实和资源证据的工作流结果。
        let mut response = json!({
            // 只有 provider 和工作流级断言/预算都无错误时整体成功。
            "ok": failed_count == 0 && workflow_error_count == 0,
            // 固定公开工作流契约版本。
            "contractVersion": "act/sequence-workflow/v1",
            // 保留现有后台优先策略声明。
            "policy": "background-preferred",
            // 报告实际完成的步骤数量。
            "count": results.len(),
            // 报告调用方请求的步骤总数。
            "total": total,
            // 报告真实 provider 失败数量。
            "failedCount": failed_count,
            // 工作流级条件或预算错误只可能出现一次并立即停止。
            "workflowErrorCount": workflow_error_count,
            // 公开机器可判定的资源预算证据。
            "budget": {
                // 固定公开步骤数硬上限。
                "maxSteps": MAX_SEQUENCE_STEPS,
                // 回显本次工作流采用的最大结果字节数。
                "maxResultBytes": sequence.max_result_bytes,
                // 报告实际接纳的 provider 负载字节数。
                "resultBytes": budget.used_bytes(),
                // 报告是否因下一份完整负载无法接纳而停止。
                "exhausted": budget_exhausted,
            },
            // 返回每个已经执行步骤的有序事实。
            "results": results,
        });
        // 只有前置条件或绑定阻止步骤启动时才增加顶层终止错误。
        if let (Some(object), Some(error)) = (response.as_object_mut(), terminal_workflow_error) {
            // 顶层错误不得伪装成任何 provider 步骤结果。
            object.insert("workflowError".to_owned(), error);
        }
        // 返回最终工作流事实。
        Ok(response)
    }
}

// 导入固定 worker 执行与生命周期投影子模块。
#[path = "sequence_execution.rs"]
mod sequence_execution;
// 导入父 Workflow 需要的最小执行投影。
use sequence_execution::{
    SequenceExecutedStep, execute_worker_step, insert_worker_evidence, invalid_sequence_timeout,
};

// 固定一个 sequence 可以请求的最大步骤数。
pub const MAX_SEQUENCE_STEPS: usize = 64;
// 固定 sequence 总执行预算的公开最小毫秒数。
pub const MIN_SEQUENCE_TOTAL_TIMEOUT_MS: u32 = COMPONENT_MIN_SEQUENCE_TOTAL_TIMEOUT_MS;
// 固定 sequence 总执行预算的公开兼容默认值。
pub const DEFAULT_SEQUENCE_TOTAL_TIMEOUT_MS: u32 = COMPONENT_DEFAULT_SEQUENCE_TOTAL_TIMEOUT_MS;
// 固定 sequence 总执行预算的公开最大毫秒数。
pub const MAX_SEQUENCE_TOTAL_TIMEOUT_MS: u32 = COMPONENT_MAX_SEQUENCE_TOTAL_TIMEOUT_MS;
// 固定单步骤执行预算的公开最小毫秒数。
pub const MIN_SEQUENCE_STEP_TIMEOUT_MS: u32 = COMPONENT_MIN_SEQUENCE_STEP_TIMEOUT_MS;
// 固定单步骤执行预算的公开兼容默认值。
pub const DEFAULT_SEQUENCE_STEP_TIMEOUT_MS: u32 = COMPONENT_DEFAULT_SEQUENCE_STEP_TIMEOUT_MS;
// 固定单步骤执行预算的公开最大毫秒数。
pub const MAX_SEQUENCE_STEP_TIMEOUT_MS: u32 = COMPONENT_MAX_SEQUENCE_STEP_TIMEOUT_MS;
// 固定结果负载预算的最小可配置字节数。
pub const MIN_SEQUENCE_RESULT_BYTES: usize = 256;
// 固定默认结果负载预算为一 MiB。
pub const DEFAULT_SEQUENCE_RESULT_BYTES: usize = 1_048_576;
// 固定结果负载预算的最大可配置字节数为十六 MiB。
pub const MAX_SEQUENCE_RESULT_BYTES: usize = 16_777_216;
// 固定每个步骤可以声明的最大后置断言数。
pub const MAX_SEQUENCE_POSTCONDITIONS: usize = 16;
// 固定每个步骤可以声明的最大前置条件数。
pub const MAX_SEQUENCE_PRECONDITIONS: usize = 16;
// 固定每个步骤可以声明的最大输入绑定数。
pub const MAX_SEQUENCE_BINDINGS: usize = 16;
// 固定 JSON Pointer 的最大 UTF-8 字节数。
pub const MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES: usize = 256;
// 固定 binding 来源 JSON Pointer 的最大 UTF-8 字节数。
pub const MAX_SEQUENCE_BINDING_POINTER_BYTES: usize = 256;
// 固定 binding 目标顶层字段名的最大 UTF-8 字节数。
pub const MAX_SEQUENCE_BINDING_FIELD_BYTES: usize = 128;
// 固定 equals 期望 JSON 的最大紧凑序列化字节数。
pub const MAX_SEQUENCE_POSTCONDITION_EXPECTED_BYTES: usize = 4_096;
// 固定单个跨步骤绑定值的最大紧凑 JSON UTF-8 字节数。
pub const MAX_SEQUENCE_BOUND_VALUE_BYTES: usize = 4_096;

// 表示 sequence Workflow 公开协议允许的封闭终止错误码。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(super) enum SequenceWorkflowErrorCode {
    // 表示完整 provider 负载无法纳入总结果预算。
    #[serde(rename = "SEQUENCE_RESULT_BUDGET_EXCEEDED")]
    // 使用无冗余前缀的内部变体名。
    ResultBudgetExceeded,
    // 表示当前步骤启动前的跨步骤条件失败。
    #[serde(rename = "SEQUENCE_PRECONDITION_FAILED")]
    // 使用无冗余前缀的内部变体名。
    PreconditionFailed,
    // 表示当前步骤启动前的跨步骤输入绑定失败。
    #[serde(rename = "SEQUENCE_BINDING_FAILED")]
    // 使用无冗余前缀的内部变体名。
    BindingFailed,
    // 表示 provider 成功后的本步骤断言失败。
    #[serde(rename = "SEQUENCE_POSTCONDITION_FAILED")]
    // 使用无冗余前缀的内部变体名。
    PostconditionFailed,
}

// 表示版本化 sequence 工作流的严格公开输入。
#[derive(Debug, Deserialize)]
// 使用 camelCase 并拒绝未声明字段，防止调用方误以为未知控制项已生效。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SequenceInput {
    // 保存按顺序执行且受硬上限约束的步骤。
    pub steps: Vec<SequenceStep>,
    // 普通 provider 错误默认停止后续步骤。
    #[serde(default)]
    pub continue_on_error: bool,
    // 缺省使用一 MiB 结果负载预算。
    #[serde(default = "default_sequence_result_bytes")]
    pub max_result_bytes: usize,
    // 缺省使用五分钟且不因步骤切换重置的总执行预算。
    #[serde(default = "default_sequence_total_timeout_ms")]
    pub total_timeout_ms: u32,
}

// 表示一个不含 provider 私有类型的公开工作流步骤。
#[derive(Debug, Deserialize, Serialize)]
// 使用 camelCase 并拒绝未声明字段，保持输入 schema 封闭。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SequenceStep {
    // 保存仅用于公开结果关联的可选名称。
    pub name: Option<String>,
    // 保存统一控制动词。
    pub verb: Verb,
    // 保存公开 app 或 surface ID。
    pub app: String,
    // 保存 run 动词需要的可选 operation ID。
    pub operation: Option<String>,
    // 保存 provider-neutral 精确目标字段。
    #[serde(default)]
    pub target: JsonMap,
    // 保存 provider-neutral 操作参数。
    #[serde(default)]
    pub args: JsonMap,
    // 保存读取结果的调用方数量边界。
    #[serde(default = "default_max_items")]
    pub max_items: usize,
    // 保存层级读取的调用方深度边界。
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
    // 保存写操作的显式确认事实。
    #[serde(default)]
    pub confirmed: bool,
    // 保存允许受控前台影响的显式同意。
    #[serde(default)]
    pub foreground_consent: bool,
    // 保存 sequence step 的隔离要求，默认兼容标准模式。
    #[serde(default)]
    pub isolation_requirement: crate::domain::IsolationRequirement,
    // 保存当前步骤自己的 deadline，默认三十秒。
    #[serde(default = "default_sequence_step_timeout_ms")]
    pub timeout_ms: u32,
    // 保存只针对本步骤成功结果执行的有界后置断言。
    #[serde(default)]
    pub postconditions: Vec<SequencePostcondition>,
    // 保存只读取严格更早步骤成功结果的有界前置条件。
    #[serde(default)]
    pub preconditions: Vec<SequencePrecondition>,
    // 保存只替换当前 target/args 已声明顶层字段的有界绑定。
    #[serde(default)]
    pub bindings: Vec<SequenceBinding>,
}

// 实现公开输入的工作流级前置验证。
impl SequenceInput {
    // 在任何步骤执行前验证全部总量边界。
    fn validate(&self) -> AppResult<()> {
        // 空工作流没有可执行语义。
        if self.steps.is_empty() {
            // 返回稳定参数错误和机器可判定边界。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明最小步骤数要求。
                "sequence 至少需要一个 step。",
                // 返回当前值和允许范围。
                json!({ "field": "steps", "actual": 0, "minimum": 1, "maximum": MAX_SEQUENCE_STEPS }),
            ));
        }
        // 超过硬上限时必须在首个 provider 调用前失败。
        if self.steps.len() > MAX_SEQUENCE_STEPS {
            // 返回稳定参数错误和机器可判定边界。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明最大步骤数要求。
                format!("sequence 最多允许 {MAX_SEQUENCE_STEPS} 个 step。"),
                // 返回当前值和允许范围。
                json!({ "field": "steps", "actual": self.steps.len(), "minimum": 1, "maximum": MAX_SEQUENCE_STEPS }),
            ));
        }
        // 结果预算必须同时满足公开上下界。
        if !(MIN_SEQUENCE_RESULT_BYTES..=MAX_SEQUENCE_RESULT_BYTES)
            // 检查调用方提供的字节数。
            .contains(&self.max_result_bytes)
        {
            // 返回稳定参数错误和机器可判定边界。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明结果预算范围。
                format!(
                    // 保持消息与常量同源。
                    "maxResultBytes 必须在 {MIN_SEQUENCE_RESULT_BYTES}..={MAX_SEQUENCE_RESULT_BYTES}。"
                ),
                // 返回当前值和允许范围。
                json!({
                    // 标识错误字段。
                    "field": "maxResultBytes",
                    // 回显实际值。
                    "actual": self.max_result_bytes,
                    // 报告最小允许值。
                    "minimum": MIN_SEQUENCE_RESULT_BYTES,
                    // 报告最大允许值。
                    "maximum": MAX_SEQUENCE_RESULT_BYTES,
                }),
            ));
        }
        // 总执行预算必须满足冻结的公开边界。
        if !(MIN_SEQUENCE_TOTAL_TIMEOUT_MS..=MAX_SEQUENCE_TOTAL_TIMEOUT_MS)
            // 核对调用方总预算。
            .contains(&self.total_timeout_ms)
        {
            // 返回稳定字段边界错误。
            return Err(AppControlError::with_details(
                // 使用既有参数错误码。
                "INVALID_ARGUMENT",
                // 说明总预算范围。
                format!(
                    "totalTimeoutMs 必须在 {MIN_SEQUENCE_TOTAL_TIMEOUT_MS}..={MAX_SEQUENCE_TOTAL_TIMEOUT_MS}。"
                ),
                // 返回精确字段和上下界。
                json!({
                    "field": "totalTimeoutMs",
                    "actual": self.total_timeout_ms,
                    "minimum": MIN_SEQUENCE_TOTAL_TIMEOUT_MS,
                    "maximum": MAX_SEQUENCE_TOTAL_TIMEOUT_MS,
                }),
            ));
        }
        // 所有步骤的 Workflow 字段必须在任何 provider 调用前完成边界验证。
        for (step_index, step) in self.steps.iter().enumerate() {
            // 当前步骤 deadline 必须在任何 worker 启动前验证。
            if !(MIN_SEQUENCE_STEP_TIMEOUT_MS..=MAX_SEQUENCE_STEP_TIMEOUT_MS)
                // 核对步骤预算。
                .contains(&step.timeout_ms)
            {
                // 返回稳定字段边界错误。
                return Err(AppControlError::with_details(
                    // 使用既有参数错误码。
                    "INVALID_ARGUMENT",
                    // 说明步骤预算范围。
                    format!(
                        "每个 sequence step 的 timeoutMs 必须在 {MIN_SEQUENCE_STEP_TIMEOUT_MS}..={MAX_SEQUENCE_STEP_TIMEOUT_MS}。"
                    ),
                    // 返回零基字段路径和上下界。
                    json!({
                        "field": format!("steps[{step_index}].timeoutMs"),
                        "actual": step.timeout_ms,
                        "minimum": MIN_SEQUENCE_STEP_TIMEOUT_MS,
                        "maximum": MAX_SEQUENCE_STEP_TIMEOUT_MS,
                    }),
                ));
            }
            // 绑定结构、来源方向与目标字段必须在任何 provider 前验证。
            validate_sequence_bindings(
                // 传递当前零基步骤索引。
                step_index,
                // 传递静态动词以限制模板首批只读范围。
                step.verb,
                // 借用当前步骤 target 模板。
                &step.target,
                // 借用当前步骤 args 模板。
                &step.args,
                // 借用全部绑定声明。
                &step.bindings,
            )?;
            // 单步骤前置条件数量必须满足公开硬上限。
            if step.preconditions.len() > MAX_SEQUENCE_PRECONDITIONS {
                // 返回稳定参数错误和精确字段路径。
                return Err(AppControlError::with_details(
                    // 使用现有公开参数错误码。
                    "INVALID_ARGUMENT",
                    // 说明单步骤前置条件上限。
                    format!(
                        "每个 sequence step 最多允许 {MAX_SEQUENCE_PRECONDITIONS} 个 precondition。"
                    ),
                    // 返回零基字段路径与实际数量。
                    json!({
                        // 精确定位失败步骤。
                        "field": format!("steps[{step_index}].preconditions"),
                        // 报告调用方实际数量。
                        "actual": step.preconditions.len(),
                        // 报告公开硬上限。
                        "maximum": MAX_SEQUENCE_PRECONDITIONS,
                    }),
                ));
            }
            // 逐项验证来源方向、Pointer 语法和期望值字节边界。
            for (condition_index, condition) in step.preconditions.iter().enumerate() {
                // 委托前置条件自身验证公开字段边界。
                condition.validate(step_index, condition_index)?;
            }
            // 单步骤断言数量必须满足公开硬上限。
            if step.postconditions.len() > MAX_SEQUENCE_POSTCONDITIONS {
                // 返回稳定参数错误和精确字段路径。
                return Err(AppControlError::with_details(
                    // 使用现有公开参数错误码。
                    "INVALID_ARGUMENT",
                    // 说明单步骤后置断言上限。
                    format!(
                        // 保持消息与常量同源。
                        "每个 sequence step 最多允许 {MAX_SEQUENCE_POSTCONDITIONS} 个 postcondition。"
                    ),
                    // 返回零基字段路径与实际数量。
                    json!({
                        // 精确定位失败步骤。
                        "field": format!("steps[{step_index}].postconditions"),
                        // 报告调用方实际数量。
                        "actual": step.postconditions.len(),
                        // 报告公开硬上限。
                        "maximum": MAX_SEQUENCE_POSTCONDITIONS,
                    }),
                ));
            }
            // 逐项验证 Pointer 语法和期望值字节边界。
            for (condition_index, condition) in step.postconditions.iter().enumerate() {
                // 委托断言输入自身验证公开字段边界。
                condition.validate(step_index, condition_index)?;
            }
        }
        // 全部工作流前置条件成立。
        Ok(())
    }
}

// 聚合一个已启动步骤产生的有界 Workflow 证据。
struct SequenceStepEvidence {
    // 保存固定 worker 的有界生命周期证据。
    worker: Option<Value>,
    // 保存 provider 启动前的可选前置条件证据。
    preconditions: Option<Value>,
    // 保存 provider 启动前的可选绑定证据。
    bindings: Option<Value>,
    // 保存 provider 成功后的可选后置条件证据。
    postconditions: Option<Value>,
}

// 构造不计入 provider 结果预算的紧凑工作流错误记录。
fn sequence_budget_record(
    // 接收一基步骤索引。
    index: usize,
    // 接收调用方可选步骤名称。
    name: Option<String>,
    // 接收 provider 的真实步骤结果。
    step_succeeded: bool,
    // 接收 resultOmitted 或 errorOmitted 封闭字段名。
    omitted_field: &'static str,
    // 接收 Component 产生的预算证据。
    exceeded: SequenceBudgetExceeded,
    // 接收不计入 provider 负载预算的有界 Workflow 证据。
    evidence: SequenceStepEvidence,
) -> Value {
    // 创建基础记录以便插入封闭的动态省略字段。
    let mut record = json!({
        // 使用一基步骤索引。
        "index": index,
        // 保留调用方可选名称。
        "name": name,
        // provider 步骤的真实成功状态不得被预算错误改写。
        "ok": step_succeeded,
        // 返回工作流级错误而不是伪造 provider 错误。
        "workflowError": {
            // 使用稳定工作流错误码。
            "code": SequenceWorkflowErrorCode::ResultBudgetExceeded,
            // 说明完整负载被省略且后续步骤未执行。
            "message": "sequence 结果负载预算不足；已省略本步骤负载并停止后续步骤。",
            // 返回不包含 provider 私有数据的完整证据。
            "details": {
                // 当前步骤已经执行结束。
                "stepCompleted": true,
                // 区分成功结果省略与失败错误省略。
                "stepSucceeded": step_succeeded,
                // 报告被省略负载的序列化字节数。
                "attemptedBytes": exceeded.attempted_bytes,
                // 报告尝试前剩余预算。
                "remainingBytes": exceeded.remaining_bytes,
            },
        },
    });
    // 取得刚构造的对象以插入封闭省略标记。
    if let Some(object) = record.as_object_mut() {
        // 标记 result 或 error 已省略。
        object.insert(omitted_field.to_owned(), Value::Bool(true));
        // 只在成功结果确实声明过断言时返回判定证据。
        if let Some(evidence) = evidence.postconditions {
            // 预算错误优先但不得抹去已完成的断言判定事实。
            object.insert("postconditions".to_owned(), evidence);
        }
        // 只在当前步骤确实声明过前置条件时返回通过证据。
        if let Some(evidence) = evidence.preconditions {
            // 前置条件已在 provider 启动前完成且不受预算错误改写。
            object.insert("preconditions".to_owned(), evidence);
        }
        // 只在当前步骤确实声明过绑定时返回应用证据。
        if let Some(evidence) = evidence.bindings {
            // 绑定已在统一 Policy 评估前完成且不受预算错误改写。
            object.insert("bindings".to_owned(), evidence);
        }
        // worker 生命周期证据不计入 provider 负载预算且永不省略。
        if let Some(evidence) = evidence.worker {
            // 保留 dispatch、终态与停止原因。
            object.insert("execution".to_owned(), evidence);
        }
    }
    // 返回紧凑且可机器判定的步骤事实。
    record
}

// 向步骤记录插入可选后置断言证据。
fn insert_postcondition_evidence(
    // 接收必须保持对象形状的步骤记录。
    record: &mut Value,
    // 借用不包含实际值或期望值的可选证据。
    evidence: Option<&Value>,
) {
    // 委托通用有界条件证据插入逻辑。
    insert_condition_evidence(record, "postconditions", evidence);
}

// 向步骤记录插入一组可选且有硬上限的条件判定证据。
fn insert_condition_evidence(
    // 接收必须保持对象形状的步骤记录。
    record: &mut Value,
    // 接收由 Workflow 选择的封闭公开字段名。
    field: &'static str,
    // 借用不包含实际值或期望值的可选证据。
    evidence: Option<&Value>,
) {
    // 只有声明过条件时才修改旧结果形状。
    if let (Some(object), Some(evidence)) = (record.as_object_mut(), evidence) {
        // 复制很小且有硬上限的证据对象。
        object.insert(field.to_owned(), evidence.clone());
    }
}

impl SequenceStep {
    // 把公开步骤拆为统一 System 请求和 Workflow 私有控制字段。
    fn into_request_and_workflow_fields(
        // 取得当前公开步骤所有权。
        self,
    ) -> (
        // 返回最终由绑定更新并交给 Policy 的统一请求。
        CommandRequest,
        // 返回只由 Workflow 消费的绑定列表。
        Vec<SequenceBinding>,
        // 返回只由 Workflow 消费的后置条件列表。
        Vec<SequencePostcondition>,
    ) {
        // 构造不包含 Workflow 控制字段的普通请求。
        let request = CommandRequest {
            // 转发统一控制动词。
            verb: self.verb,
            // 转发公开 app 或 surface ID。
            app: self.app,
            // 转发可选 operation ID。
            operation: self.operation,
            // 转发 provider-neutral 精确目标。
            target: self.target,
            // 转发 provider-neutral 操作参数。
            args: self.args,
            // 转发调用方读取数量边界。
            max_items: self.max_items,
            // 转发调用方读取深度边界。
            max_depth: self.max_depth,
            // 转发显式写确认事实。
            confirmed: self.confirmed,
            // 转发显式前台影响同意。
            foreground_consent: self.foreground_consent,
            // 转发不可由 provider 放宽的隔离要求。
            isolation_requirement: self.isolation_requirement,
        };
        // 返回普通请求和只由 sequence 消费的绑定及后置条件列表。
        (request, self.bindings, self.postconditions)
    }
}

// 返回公开契约的默认结果负载预算。
const fn default_sequence_result_bytes() -> usize {
    // 与 schema 的 default 保持一致。
    DEFAULT_SEQUENCE_RESULT_BYTES
}

// 返回 Workflow 总执行预算兼容默认值。
const fn default_sequence_total_timeout_ms() -> u32 {
    // 与公开 schema 和预算 Component 保持一致。
    DEFAULT_SEQUENCE_TOTAL_TIMEOUT_MS
}

// 返回当前步骤 deadline 兼容默认值。
const fn default_sequence_step_timeout_ms() -> u32 {
    // 与公开 schema 和预算 Component 保持一致。
    DEFAULT_SEQUENCE_STEP_TIMEOUT_MS
}

// 返回步骤读取的默认最大条目数。
const fn default_max_items() -> usize {
    // 保留现有兼容默认值。
    50
}

// 返回步骤层级读取的默认深度。
const fn default_max_depth() -> usize {
    // 保留现有兼容默认值。
    4
}

// 覆盖 sequence Workflow 私有协议类型的公开序列化兼容。
#[cfg(test)]
mod tests {
    // 导入父 Workflow 拥有的封闭错误码。
    use super::SequenceWorkflowErrorCode;
    // 导入 JSON 构造器以锁定公开字符串集合和顺序。
    use serde_json::json;

    // 验证四种工作流终止错误只从封闭枚举投影为既有文本。
    #[test]
    fn workflow_error_codes_preserve_public_contract() {
        // 按契约说明顺序序列化全部封闭变体。
        let actual = json!([
            // 结果预算耗尽。
            SequenceWorkflowErrorCode::ResultBudgetExceeded,
            // 前置条件失败。
            SequenceWorkflowErrorCode::PreconditionFailed,
            // 输入绑定失败。
            SequenceWorkflowErrorCode::BindingFailed,
            // 后置条件失败。
            SequenceWorkflowErrorCode::PostconditionFailed,
        ]);
        // 公开文本必须逐字保持版本一契约。
        assert_eq!(
            // 对比实际序列化集合。
            actual,
            // 锁定调用方已经依赖的四个稳定字符串。
            json!([
                "SEQUENCE_RESULT_BUDGET_EXCEEDED",
                "SEQUENCE_PRECONDITION_FAILED",
                "SEQUENCE_BINDING_FAILED",
                "SEQUENCE_POSTCONDITION_FAILED",
            ])
        );
    }
}
