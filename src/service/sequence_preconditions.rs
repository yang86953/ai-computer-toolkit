//! 实现 sequence Workflow 私有的跨步骤前置条件契约与判定。

// 导入公开序列化契约派生。
use serde::{Deserialize, Serialize};
// 导入 JSON 值与证据构造宏。
use serde_json::{Value, json};

// 导入 provider-neutral JSON 条件 Component。
use crate::components::json_postcondition::{
    JsonCondition, JsonConditionFailure, evaluate as evaluate_json_condition, is_valid_json_pointer,
};
// 导入公开错误契约和 workflow 边界常量。
use crate::{
    // 导入统一公开错误结果。
    domain::{AppControlError, AppResult},
    // 导入与后置断言一致的 Pointer 和 expected 硬边界。
    service::sequence::{
        MAX_SEQUENCE_POSTCONDITION_EXPECTED_BYTES, MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES,
        SequenceWorkflowErrorCode,
    },
};

// 表示一个封闭且只读取更早步骤成功结果的前置条件。
#[derive(Debug, Deserialize, Serialize)]
// 使用 operator 作为内部标签并拒绝每个变体的未知字段。
#[serde(tag = "operator", rename_all = "camelCase", deny_unknown_fields)]
pub enum SequencePrecondition {
    // 要求来源步骤结果中的 JSON Pointer 指向任意现存值。
    #[serde(rename = "exists")]
    Exists {
        // 保存一基且必须严格早于当前步骤的来源索引。
        #[serde(rename = "sourceStep")]
        source_step: usize,
        // 保存 RFC 6901 JSON Pointer。
        pointer: String,
    },
    // 要求来源步骤结果中的 JSON Pointer 与期望 JSON 完全相等。
    #[serde(rename = "equals")]
    Equals {
        // 保存一基且必须严格早于当前步骤的来源索引。
        #[serde(rename = "sourceStep")]
        source_step: usize,
        // 保存 RFC 6901 JSON Pointer。
        pointer: String,
        // 保存受紧凑序列化字节上限约束的期望 JSON。
        expected: Value,
    },
}

// 保存一次不泄漏实际值或期望值的前置条件失败定位。
pub(super) struct SequencePreconditionFailure {
    // 保存一基条件索引。
    pub(super) condition_index: usize,
    // 保存一基来源步骤索引。
    pub(super) source_step: usize,
    // 保存封闭 operator 名称。
    pub(super) operator: &'static str,
    // 保存已经过边界验证的 JSON Pointer。
    pub(super) pointer: String,
    // 保存封闭失败原因。
    pub(super) reason: &'static str,
}

// 保存一次当前步骤前置条件组的判定结果。
pub(super) struct SequencePreconditionEvaluation {
    // 保存只在声明前置条件时出现的有界通过证据。
    pub(super) evidence: Option<Value>,
    // 保存首个失败条件并触发 Workflow 硬停止。
    pub(super) failure: Option<SequencePreconditionFailure>,
}

// 对已经完成的更早步骤结果按声明顺序执行前置条件。
pub(super) fn evaluate_sequence_preconditions(
    // 借用当前 Workflow 已经建立的步骤事实。
    results: &[Value],
    // 借用已经通过全部输入边界验证的前置条件。
    preconditions: &[SequencePrecondition],
) -> SequencePreconditionEvaluation {
    // 未声明前置条件时保持旧步骤结果形状。
    if preconditions.is_empty() {
        // 返回无证据且无失败的空判定。
        return SequencePreconditionEvaluation {
            // 不增加公开字段。
            evidence: None,
            // 不产生工作流错误。
            failure: None,
        };
    }
    // 按调用方声明顺序执行并在首个失败处停止。
    for (index, condition) in preconditions.iter().enumerate() {
        // 取得已经验证为非零的一基来源索引。
        let source_step = condition.source_step();
        // 防御性取得来源步骤记录，不把索引错误转化为 panic。
        let Some(source_record) = source_step
            // 把一基索引转换为零基索引。
            .checked_sub(1)
            // 从已建立结果中取得来源记录。
            .and_then(|source_index| results.get(source_index))
        else {
            // 输入预验证与运行记录若不一致，报告来源结果不可用。
            return failed_evaluation(index, condition, "source-result-unavailable");
        };
        // provider 失败的来源步骤没有成功结果可供前置条件读取。
        if source_record.get("ok") != Some(&Value::Bool(true)) {
            // 以封闭原因停止当前步骤，保留来源失败事实。
            return failed_evaluation(index, condition, "source-step-failed");
        }
        // 只允许读取已完整纳入结果预算的成功结果。
        let Some(source_result) = source_record.get("result") else {
            // 省略或缺失的结果不能被静默当成空 JSON。
            return failed_evaluation(index, condition, "source-result-unavailable");
        };
        // 把 System 私有条件映射为窄 Component 输入。
        let decision = match condition {
            // exists 只借用已验证 Pointer。
            SequencePrecondition::Exists { pointer, .. } => evaluate_json_condition(
                // 传递完整来源结果。
                source_result,
                // 构造无步骤流程语义的窄条件。
                JsonCondition::Exists { pointer },
            ),
            // equals 借用已验证 Pointer 和有界期望值。
            SequencePrecondition::Equals {
                pointer, expected, ..
            } => evaluate_json_condition(
                // 传递完整来源结果。
                source_result,
                // 构造无步骤流程语义的窄条件。
                JsonCondition::Equals { pointer, expected },
            ),
        };
        // 首个 JSON 判定失败必须产生确定证据并停止。
        if let Err(reason) = decision {
            // 把 Component 原因映射为 Workflow 封闭原因。
            let reason = match reason {
                // 来源结果中目标缺失。
                JsonConditionFailure::Missing => "missing",
                // 来源结果中的值不相等。
                JsonConditionFailure::NotEqual => "not-equal",
            };
            // 返回首个失败定位。
            return failed_evaluation(index, condition, reason);
        }
    }
    // 全部前置条件通过时返回有界判定证据。
    SequencePreconditionEvaluation {
        // 只报告检查数量和通过状态。
        evidence: Some(json!({
            // 全部声明条件均已检查。
            "checked": preconditions.len(),
            // 报告整组前置条件通过。
            "passed": true,
        })),
        // 不产生工作流错误。
        failure: None,
    }
}

// 构造阻止当前步骤启动的稳定前置条件错误。
pub(super) fn sequence_precondition_error(
    // 接收一基当前步骤索引。
    step_index: usize,
    // 接收调用方可选步骤名称。
    step_name: Option<String>,
    // 接收首个失败条件的最小定位。
    failure: SequencePreconditionFailure,
    // 接收不含实际值或期望值的条件判定证据。
    evidence: Option<Value>,
) -> Value {
    // 返回顶层工作流错误，不创建伪 provider 步骤记录。
    json!({
        // 使用前置条件专用稳定错误码。
        "code": SequenceWorkflowErrorCode::PreconditionFailed,
        // 说明当前步骤没有启动且后续步骤已停止。
        "message": "sequence 前置条件失败；当前步骤未启动，已停止后续步骤。",
        // 返回不包含 provider 私有数据的完整定位证据。
        "details": {
            // 精确定位被阻止的一基步骤索引。
            "stepIndex": step_index,
            // 保留调用方可选名称用于关联。
            "stepName": step_name,
            // provider 尚未启动。
            "stepStarted": false,
            // provider 因此前置条件失败而未完成。
            "stepCompleted": false,
            // 报告首个失败条件的一基索引。
            "conditionIndex": failure.condition_index,
            // 报告被读取的一基来源步骤。
            "sourceStep": failure.source_step,
            // 报告封闭 operator 名称。
            "operator": failure.operator,
            // 回显受硬上限约束的调用方 Pointer。
            "pointer": failure.pointer,
            // 报告封闭失败原因。
            "reason": failure.reason,
            // 报告整组前置条件已经检查到的位置。
            "preconditions": evidence,
        },
    })
}

// 构造首个前置条件失败的稳定判定结果。
fn failed_evaluation(
    // 接收零基条件索引。
    index: usize,
    // 借用失败条件。
    condition: &SequencePrecondition,
    // 接收封闭失败原因。
    reason: &'static str,
) -> SequencePreconditionEvaluation {
    // 使用一基索引对齐公开证据。
    let condition_index = index + 1;
    // 返回不泄漏实际值或期望值的失败证据。
    SequencePreconditionEvaluation {
        // 报告执行到首个失败条件的数量。
        evidence: Some(json!({
            // 首个失败条件也已经完成判定。
            "checked": condition_index,
            // 报告整组条件未通过。
            "passed": false,
            // 精确定位首个失败条件。
            "failedCondition": condition_index,
        })),
        // 保存 Workflow 构造终止错误所需的最小定位。
        failure: Some(SequencePreconditionFailure {
            // 保存一基条件索引。
            condition_index,
            // 保存一基来源步骤索引。
            source_step: condition.source_step(),
            // 保存封闭 operator 名称。
            operator: condition.operator(),
            // 复制受硬上限约束的 Pointer。
            pointer: condition.pointer().to_owned(),
            // 保存封闭失败原因。
            reason,
        }),
    }
}

// 实现公开前置条件的边界验证与窄字段访问。
impl SequencePrecondition {
    // 在任何 provider 调用前验证来源方向和 JSON 条件边界。
    pub(super) fn validate(
        // 借用当前步骤的零基索引。
        &self,
        // 接收当前步骤的零基索引。
        step_index: usize,
        // 接收当前条件的零基索引。
        condition_index: usize,
    ) -> AppResult<()> {
        // 构造当前条件的稳定字段路径前缀。
        let field_prefix = format!("steps[{step_index}].preconditions[{condition_index}]");
        // 取得调用方的一基来源索引。
        let source_step = self.source_step();
        // 当前零基索引恰好等于允许引用的最大一基来源索引。
        if source_step == 0 || source_step > step_index {
            // 拒绝第一步引用、当前步骤引用和未来步骤引用。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明来源必须严格更早。
                "precondition sourceStep 必须引用严格更早的 sequence step。",
                // 返回字段位置与允许边界，不回显 provider 数据。
                json!({
                    // 精确定位来源字段。
                    "field": format!("{field_prefix}.sourceStep"),
                    // 报告实际一基来源索引。
                    "actual": source_step,
                    // 报告最小合法索引。
                    "minimum": 1,
                    // 报告当前步骤允许的最大来源索引。
                    "maximum": step_index,
                }),
            ));
        }
        // Pointer 采用 UTF-8 字节硬上限。
        if self.pointer().len() > MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES {
            // 返回稳定参数错误和真实字节数。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明 Pointer 字节上限。
                format!(
                    "precondition pointer 最多允许 {MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES} 个 UTF-8 字节。"
                ),
                // 返回精确字段路径和字节证据。
                json!({
                    // 定位 Pointer 字段。
                    "field": format!("{field_prefix}.pointer"),
                    // 报告真实 UTF-8 字节数。
                    "actualBytes": self.pointer().len(),
                    // 报告公开硬上限。
                    "maximumBytes": MAX_SEQUENCE_POSTCONDITION_POINTER_BYTES,
                }),
            ));
        }
        // Pointer 必须满足 RFC 6901 根、前导斜杠和转义语法。
        if !is_valid_json_pointer(self.pointer()) {
            // 返回不依赖 provider 的稳定参数错误。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明采用 RFC 6901 Pointer 语法。
                "precondition pointer 必须是有效的 RFC 6901 JSON Pointer。",
                // 只返回字段路径，不复制可能敏感的 Pointer 内容。
                json!({ "field": format!("{field_prefix}.pointer") }),
            ));
        }
        // 只有 equals 携带期望 JSON 负载。
        if let Some(expected) = self.expected() {
            // 使用紧凑 JSON 表示测量真实 UTF-8 字节数。
            let expected_bytes = expected.to_string().len();
            // 期望负载必须满足公开硬上限。
            if expected_bytes > MAX_SEQUENCE_POSTCONDITION_EXPECTED_BYTES {
                // 返回稳定参数错误且不回显期望值。
                return Err(AppControlError::with_details(
                    // 使用现有公开参数错误码。
                    "INVALID_ARGUMENT",
                    // 说明 equals 期望值字节上限。
                    format!(
                        "precondition equals expected 最多允许 {MAX_SEQUENCE_POSTCONDITION_EXPECTED_BYTES} 个紧凑 JSON UTF-8 字节。"
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
        // 当前前置条件边界全部成立。
        Ok(())
    }

    // 返回两个 operator 共用的一基来源步骤索引。
    const fn source_step(&self) -> usize {
        // 从封闭变体取得来源索引。
        match self {
            // exists 直接返回来源索引。
            Self::Exists { source_step, .. } => *source_step,
            // equals 直接返回来源索引。
            Self::Equals { source_step, .. } => *source_step,
        }
    }

    // 返回两个 operator 共用的 JSON Pointer。
    fn pointer(&self) -> &str {
        // 从封闭变体取得 Pointer 借用。
        match self {
            // exists 直接返回 Pointer。
            Self::Exists { pointer, .. } => pointer,
            // equals 直接返回 Pointer。
            Self::Equals { pointer, .. } => pointer,
        }
    }

    // 返回 equals 的可选期望 JSON。
    fn expected(&self) -> Option<&Value> {
        // 仅 equals 变体携带期望值。
        match self {
            // exists 没有期望值。
            Self::Exists { .. } => None,
            // equals 借用期望 JSON。
            Self::Equals { expected, .. } => Some(expected),
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
