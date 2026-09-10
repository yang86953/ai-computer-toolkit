//! 实现 sequence Workflow 私有的本步骤后置条件契约与判定。

// 导入公开序列化契约派生。
use serde::{Deserialize, Serialize};
// 导入 JSON 值与证据构造宏。
use serde_json::{Value, json};

// 导入 provider-neutral JSON 条件 Component。
use crate::components::json_postcondition::{
    JsonCondition, JsonConditionFailure, evaluate as evaluate_json_condition, is_valid_json_pointer,
};
// 导入公开错误结果。
use crate::domain::{AppControlError, AppResult};
// 导入 Workflow 公开的条件硬边界。
use super::sequence::{
    MAX_SEQUENCE_POSTCONDITION_EXPECTED_BYTES, MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES,
    SequenceWorkflowErrorCode,
};

// 表示一个封闭且可在完整 JSON 结果上确定性执行的后置条件。
#[derive(Clone, Debug, Deserialize, Serialize)]
// 使用 operator 作为内部标签并拒绝每个变体的未知字段。
#[serde(tag = "operator", rename_all = "camelCase", deny_unknown_fields)]
pub enum SequencePostcondition {
    // 要求 JSON Pointer 指向任意现存值，包括 null。
    #[serde(rename = "exists")]
    Exists {
        // 保存 RFC 6901 JSON Pointer。
        pointer: String,
    },
    // 要求 JSON Pointer 指向与期望 JSON 完全相等的值。
    #[serde(rename = "equals")]
    Equals {
        // 保存 RFC 6901 JSON Pointer。
        pointer: String,
        // 保存受序列化字节上限约束的期望 JSON。
        expected: Value,
    },
}

// 保存一次不泄漏实际值或期望值的后置条件失败定位。
pub(super) struct SequencePostconditionFailure {
    // 保存一基条件索引。
    condition_index: usize,
    // 保存封闭 operator 名称。
    operator: &'static str,
    // 保存调用方提供且已经过边界验证的 JSON Pointer。
    pointer: String,
    // 保存封闭失败原因。
    reason: &'static str,
}

// 保存一次步骤后置条件判定的公开证据和可选失败。
pub(super) struct SequencePostconditionEvaluation {
    // 保存仅在声明过条件时出现的有界证据。
    pub(super) evidence: Option<Value>,
    // 保存首个失败条件并触发硬停止。
    pub(super) failure: Option<SequencePostconditionFailure>,
}

// 对完整 provider 结果按声明顺序执行后置条件。
pub(super) fn evaluate_sequence_postconditions(
    // 借用已经成功建立的完整 provider 结果。
    result: &Value,
    // 借用已经通过全部输入边界验证的条件。
    postconditions: &[SequencePostcondition],
) -> SequencePostconditionEvaluation {
    // 未声明条件时保持旧步骤结果形状。
    if postconditions.is_empty() {
        // 返回无证据且无失败的空判定。
        return SequencePostconditionEvaluation {
            // 不增加公开字段。
            evidence: None,
            // 不产生工作流错误。
            failure: None,
        };
    }
    // 按调用方声明顺序执行并在首个失败处停止。
    for (index, condition) in postconditions.iter().enumerate() {
        // 把 System 公共类型映射为 Component 的窄输入。
        let decision = match condition {
            // exists 只借用已验证 Pointer。
            SequencePostcondition::Exists { pointer } => evaluate_json_condition(
                // 传递完整结果。
                result,
                // 构造不拥有工作流语义的窄条件。
                JsonCondition::Exists { pointer },
            ),
            // equals 只借用已验证 Pointer 和有界期望值。
            SequencePostcondition::Equals { pointer, expected } => {
                // 委托无状态 Component 执行精确 JSON 比较。
                evaluate_json_condition(
                    // 传递完整结果。
                    result,
                    // 构造不包含步骤或终止策略的窄条件。
                    JsonCondition::Equals { pointer, expected },
                )
            }
        };
        // 首个失败必须产生确定证据并停止继续判定。
        if let Err(reason) = decision {
            // 使用一基索引对齐公开步骤证据。
            let condition_index = index + 1;
            // 返回失败证据和硬停止原因。
            return SequencePostconditionEvaluation {
                // 不公开实际值或期望值。
                evidence: Some(json!({
                    // 报告实际执行到首个失败条件的数量。
                    "checked": condition_index,
                    // 报告整组条件未通过。
                    "passed": false,
                    // 精确定位首个失败条件。
                    "failedCondition": condition_index,
                })),
                // 保存用于稳定工作流错误的最小定位。
                failure: Some(SequencePostconditionFailure {
                    // 保存一基条件索引。
                    condition_index,
                    // 映射为封闭 operator 文本。
                    operator: condition.operator(),
                    // 复制受 256 字节上限约束的 Pointer。
                    pointer: condition.pointer().to_owned(),
                    // 映射为封闭失败原因文本。
                    reason: match reason {
                        // 目标缺失保持独立原因。
                        JsonConditionFailure::Missing => "missing",
                        // 值不相等保持独立原因。
                        JsonConditionFailure::NotEqual => "not-equal",
                    },
                }),
            };
        }
    }
    // 全部条件通过时返回完整已检查数量。
    SequencePostconditionEvaluation {
        // 证据不包含 provider 值或调用方期望值。
        evidence: Some(json!({
            // 全部声明条件均已检查。
            "checked": postconditions.len(),
            // 报告整组条件通过。
            "passed": true,
        })),
        // 不产生工作流错误。
        failure: None,
    }
}

// 向已成功步骤插入稳定的后置条件工作流错误。
pub(super) fn insert_postcondition_error(
    // 接收已经包含完整 provider 结果的步骤记录。
    record: &mut Value,
    // 接收不包含实际值或期望值的失败定位。
    failure: SequencePostconditionFailure,
) {
    // 取得步骤对象并保持内部构造失败闭合。
    if let Some(object) = record.as_object_mut() {
        // 插入不伪装成 provider 错误的工作流错误。
        object.insert(
            // 使用与预算错误一致的工作流错误字段。
            "workflowError".to_owned(),
            // 构造版本化契约内的稳定错误对象。
            json!({
                // 使用后置条件专用稳定错误码。
                "code": SequenceWorkflowErrorCode::PostconditionFailed,
                // 说明步骤已成功但条件阻止后续步骤。
                "message": "sequence 步骤已成功，但后置断言失败；已停止后续步骤。",
                // 返回不包含 provider 私有数据的定位证据。
                "details": {
                    // 当前步骤已经执行结束。
                    "stepCompleted": true,
                    // provider 成功事实不得被条件改写。
                    "stepSucceeded": true,
                    // 报告一基条件索引。
                    "conditionIndex": failure.condition_index,
                    // 报告封闭 operator 名称。
                    "operator": failure.operator,
                    // 回显受硬上限约束的调用方 Pointer。
                    "pointer": failure.pointer,
                    // 报告 missing 或 not-equal 封闭原因。
                    "reason": failure.reason,
                },
            }),
        );
    }
}

// 实现公开后置条件的边界验证和窄字段访问。
impl SequencePostcondition {
    // 在任何 provider 调用前验证 Pointer 和期望值边界。
    pub(super) fn validate(&self, step_index: usize, condition_index: usize) -> AppResult<()> {
        // 构造当前条件的稳定零基字段路径前缀。
        let field_prefix = format!("steps[{step_index}].postconditions[{condition_index}]");
        // 借用两个 operator 共用的 Pointer。
        let pointer = self.pointer();
        // Pointer 采用 UTF-8 字节硬上限而不是字符近似。
        if pointer.len() > MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES {
            // 返回稳定参数错误和真实字节数。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明 Pointer 字节上限。
                format!(
                    "postcondition pointer 最多允许 {MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES} 个 UTF-8 字节。"
                ),
                // 返回精确字段路径和字节证据。
                json!({
                    // 定位 Pointer 字段。
                    "field": format!("{field_prefix}.pointer"),
                    // 报告真实 UTF-8 字节数。
                    "actualBytes": pointer.len(),
                    // 报告公开硬上限。
                    "maximumBytes": MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES,
                }),
            ));
        }
        // Pointer 必须是空根指针或以斜杠开头且只使用标准转义。
        if !is_valid_json_pointer(pointer) {
            // 返回不依赖 provider 的稳定参数错误。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明采用 RFC 6901 Pointer 语法。
                "postcondition pointer 必须是有效的 RFC 6901 JSON Pointer。",
                // 只返回字段路径，不复制可能敏感的 Pointer 内容。
                json!({ "field": format!("{field_prefix}.pointer") }),
            ));
        }
        // 只有 equals 携带期望 JSON 负载。
        if let SequencePostcondition::Equals { expected, .. } = self {
            // 以 stdout 同源紧凑 JSON 序列化测量真实 UTF-8 字节数。
            let expected_bytes = expected.to_string().len();
            // 期望负载必须满足公开硬上限。
            if expected_bytes > MAX_SEQUENCE_POSTCONDITION_EXPECTED_BYTES {
                // 返回稳定参数错误且不回显期望值。
                return Err(AppControlError::with_details(
                    // 使用现有公开参数错误码。
                    "INVALID_ARGUMENT",
                    // 说明 equals 期望值字节上限。
                    format!(
                        "equals expected 最多允许 {MAX_SEQUENCE_POSTCONDITION_EXPECTED_BYTES} 个紧凑 JSON UTF-8 字节。"
                    ),
                    // 返回精确字段路径和字节证据。
                    json!({
                        // 定位 expected 字段。
                        "field": format!("{field_prefix}.expected"),
                        // 报告真实紧凑 JSON 字节数。
                        "actualBytes": expected_bytes,
                        // 报告公开硬上限。
                        "maximumBytes": MAX_SEQUENCE_POSTCONDITION_EXPECTED_BYTES,
                    }),
                ));
            }
        }
        // 当前条件边界全部成立。
        Ok(())
    }

    // 返回两个 operator 共用的 Pointer。
    fn pointer(&self) -> &str {
        // 从封闭变体取得 Pointer 借用。
        match self {
            // exists 直接返回 Pointer。
            Self::Exists { pointer } => pointer,
            // equals 直接返回 Pointer。
            Self::Equals { pointer, .. } => pointer,
        }
    }

    // 返回公开 schema 使用的封闭 operator 名称。
    const fn operator(&self) -> &'static str {
        // 映射为稳定小写文本。
        match self {
            // exists 变体使用 exists。
            Self::Exists { .. } => "exists",
            // equals 变体使用 equals。
            Self::Equals { .. } => "equals",
        }
    }
}
