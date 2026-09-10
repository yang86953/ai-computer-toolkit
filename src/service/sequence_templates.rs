//! 实现 sequence Workflow 私有的无脚本结构化字符串模板。

// 导入严格输入序列化派生。
use serde::{Deserialize, Serialize};
// 导入 JSON 值和错误证据构造宏。
use serde_json::{Value, json};

// 导入 JSON Pointer 语法 Component。
use crate::components::json_postcondition::is_valid_json_pointer;
// 导入公开错误、结果与动词。
use crate::domain::{AppControlError, AppResult, Verb};
// 导入既有绑定 Pointer 与结果字节边界。
use super::sequence::{MAX_SEQUENCE_BINDING_POINTER_BYTES, MAX_SEQUENCE_BOUND_VALUE_BYTES};

// 限制单个模板的总段数。
pub const MAX_SEQUENCE_TEMPLATE_SEGMENTS: usize = 32;
// 限制单个模板的动态来源段数。
pub const MAX_SEQUENCE_TEMPLATE_SOURCES: usize = 16;
// 限制单个 literal 段的 UTF-8 字节数。
pub const MAX_SEQUENCE_TEMPLATE_LITERAL_BYTES: usize = 1_024;
// 限制单个模板全部静态 literal 的 UTF-8 字节数。
pub const MAX_SEQUENCE_TEMPLATE_STATIC_BYTES: usize = 4_096;

// 表示一个封闭、无求值能力的模板段。
#[derive(Debug, Deserialize, Serialize)]
// 使用 kind 标签并拒绝所有未声明字段。
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum SequenceTemplateSegment {
    // 保存逐字拼接的静态文本。
    Literal {
        // 保存不解释转义或表达式的 UTF-8 文本。
        text: String,
    },
    // 保存严格更早成功结果中的字符串来源。
    Source {
        // 使用公开 camelCase 来源索引字段。
        #[serde(rename = "sourceStep")]
        // 保存一基来源步骤索引。
        source_step: usize,
        // 使用公开 camelCase Pointer 字段。
        #[serde(rename = "sourcePointer")]
        // 保存 RFC 6901 来源 Pointer。
        source_pointer: String,
    },
}

// 保存不泄漏来源值的模板渲染失败。
pub(super) struct SequenceTemplateFailure {
    // 保存一基失败段索引。
    pub(super) segment_index: usize,
    // 保存可选一基来源步骤。
    pub(super) source_step: Option<usize>,
    // 保存可选有界来源 Pointer。
    pub(super) source_pointer: Option<String>,
    // 保存封闭失败原因。
    pub(super) reason: &'static str,
    // 保存只用于输出超限的实际 UTF-8 字节数。
    pub(super) attempted_bytes: Option<usize>,
}

// 在首个 provider 调用前验证模板结构和全部静态来源方向。
pub(super) fn validate_sequence_template(
    // 接收当前步骤零基索引。
    step_index: usize,
    // 接收当前绑定零基索引。
    binding_index: usize,
    // 接收当前步骤静态动词。
    verb: Verb,
    // 借用全部模板段。
    segments: &[SequenceTemplateSegment],
) -> AppResult<()> {
    // 构造模板字段的稳定位置。
    let template_field = format!("steps[{step_index}].bindings[{binding_index}].template");
    // 首批只允许静态只读动词使用模板。
    if verb == Verb::Run {
        // mutation 或 run 必须等待稳定 resume 与动态确认。
        return Err(AppControlError::with_details(
            // 使用统一参数错误码阻止首个 provider。
            "INVALID_ARGUMENT",
            // 说明当前发布范围。
            "sequence template 首批只允许 status、sessions 或 inspect 步骤；run 必须等待稳定 resume 与动态物化重新确认。",
            // 返回精确字段和动词。
            json!({
                // 定位模板字段。
                "field": template_field,
                // 报告静态 run 动词。
                "verb": verb.as_str(),
                // 报告后续依赖任务。
                "blockedBy": ["GC-FLOW-003A", "GC-FLOW-003B"],
            }),
        ));
    }
    // 模板必须包含 1..=32 个封闭段。
    if segments.is_empty() || segments.len() > MAX_SEQUENCE_TEMPLATE_SEGMENTS {
        // 返回段数量硬边界。
        return Err(AppControlError::with_details(
            // 使用统一参数错误码。
            "INVALID_ARGUMENT",
            // 说明段数量边界。
            format!("sequence template 必须包含 1..={MAX_SEQUENCE_TEMPLATE_SEGMENTS} 个 segment。"),
            // 报告精确数量证据。
            json!({
                // 定位模板数组。
                "field": template_field,
                // 报告实际段数。
                "actual": segments.len(),
                // 报告最小段数。
                "minimum": 1,
                // 报告最大段数。
                "maximum": MAX_SEQUENCE_TEMPLATE_SEGMENTS,
            }),
        ));
    }
    // 累计动态来源段数量。
    let mut source_count = 0usize;
    // 累计静态 literal UTF-8 字节数。
    let mut static_bytes = 0usize;
    // 逐段验证封闭类型的专属边界。
    for (segment_index, segment) in segments.iter().enumerate() {
        // 构造当前段的稳定字段位置。
        let segment_field = format!("{template_field}[{segment_index}]");
        // 按封闭段类别验证。
        match segment {
            // literal 只贡献静态字节。
            SequenceTemplateSegment::Literal { text } => {
                // 单 literal 段不得突破 1024 字节。
                if text.len() > MAX_SEQUENCE_TEMPLATE_LITERAL_BYTES {
                    // 返回单段字节边界。
                    return Err(AppControlError::with_details(
                        // 使用统一参数错误码。
                        "INVALID_ARGUMENT",
                        // 说明单段上限。
                        format!(
                            "template literal 最多允许 {MAX_SEQUENCE_TEMPLATE_LITERAL_BYTES} 个 UTF-8 字节。"
                        ),
                        // 返回字节证据但不回显文本。
                        json!({
                            // 定位 literal 文本。
                            "field": format!("{segment_field}.text"),
                            // 报告实际字节数。
                            "actualBytes": text.len(),
                            // 报告最大字节数。
                            "maximumBytes": MAX_SEQUENCE_TEMPLATE_LITERAL_BYTES,
                        }),
                    ));
                }
                // 使用饱和加法防御累计溢出。
                static_bytes = static_bytes.saturating_add(text.len());
            }
            // source 必须引用严格更早步骤和合法 Pointer。
            SequenceTemplateSegment::Source {
                // 借用一基来源索引。
                source_step,
                // 借用来源 Pointer。
                source_pointer,
            } => {
                // 累计动态来源段。
                source_count = source_count.saturating_add(1);
                // 来源必须是一基且严格早于当前步骤。
                if *source_step == 0 || *source_step > step_index {
                    // 返回稳定方向错误。
                    return Err(AppControlError::with_details(
                        // 使用统一参数错误码。
                        "INVALID_ARGUMENT",
                        // 说明来源方向。
                        "template sourceStep 必须引用严格更早的 sequence step。",
                        // 返回索引边界。
                        json!({
                            // 定位来源索引。
                            "field": format!("{segment_field}.sourceStep"),
                            // 报告实际一基索引。
                            "actual": source_step,
                            // 报告最小合法索引。
                            "minimum": 1,
                            // 当前零基步骤等于最大合法一基来源。
                            "maximum": step_index,
                        }),
                    ));
                }
                // 来源 Pointer 必须满足字节和语法边界。
                if source_pointer.len() > MAX_SEQUENCE_BINDING_POINTER_BYTES
                    // 同时验证 RFC 6901 语法。
                    || !is_valid_json_pointer(source_pointer)
                {
                    // 返回不回显 Pointer 内容的稳定错误。
                    return Err(AppControlError::with_details(
                        // 使用统一参数错误码。
                        "INVALID_ARGUMENT",
                        // 说明 Pointer 边界。
                        format!(
                            "template sourcePointer 必须是最多 {MAX_SEQUENCE_BINDING_POINTER_BYTES} 个 UTF-8 字节的 RFC 6901 JSON Pointer。"
                        ),
                        // 只报告字段与字节数。
                        json!({
                            // 定位来源 Pointer。
                            "field": format!("{segment_field}.sourcePointer"),
                            // 报告真实字节数。
                            "actualBytes": source_pointer.len(),
                            // 报告最大字节数。
                            "maximumBytes": MAX_SEQUENCE_BINDING_POINTER_BYTES,
                        }),
                    ));
                }
            }
        }
    }
    // 模板至少需要一个动态 source 段。
    if source_count == 0 || source_count > MAX_SEQUENCE_TEMPLATE_SOURCES {
        // 返回来源数量硬边界。
        return Err(AppControlError::with_details(
            // 使用统一参数错误码。
            "INVALID_ARGUMENT",
            // 说明来源数量边界。
            format!(
                "sequence template 必须包含 1..={MAX_SEQUENCE_TEMPLATE_SOURCES} 个 source segment。"
            ),
            // 报告实际来源段数。
            json!({
                // 定位模板数组。
                "field": template_field,
                // 报告实际来源段数。
                "actualSources": source_count,
                // 报告最小来源段数。
                "minimumSources": 1,
                // 报告最大来源段数。
                "maximumSources": MAX_SEQUENCE_TEMPLATE_SOURCES,
            }),
        ));
    }
    // 全部静态文本合计不得突破 4096 字节。
    if static_bytes > MAX_SEQUENCE_TEMPLATE_STATIC_BYTES {
        // 返回累计静态字节边界。
        return Err(AppControlError::with_details(
            // 使用统一参数错误码。
            "INVALID_ARGUMENT",
            // 说明累计字节上限。
            format!(
                "sequence template 的 literal 合计最多允许 {MAX_SEQUENCE_TEMPLATE_STATIC_BYTES} 个 UTF-8 字节。"
            ),
            // 报告累计证据。
            json!({
                // 定位模板数组。
                "field": template_field,
                // 报告实际静态字节数。
                "actualBytes": static_bytes,
                // 报告最大静态字节数。
                "maximumBytes": MAX_SEQUENCE_TEMPLATE_STATIC_BYTES,
            }),
        ));
    }
    // 全部模板结构边界成立。
    Ok(())
}

// 从更早成功结果确定性渲染一个有界 JSON string。
pub(super) fn render_sequence_template(
    // 借用 Workflow 已经建立的步骤事实。
    results: &[Value],
    // 借用已经通过静态验证的模板段。
    segments: &[SequenceTemplateSegment],
) -> Result<Value, SequenceTemplateFailure> {
    // 预分配不超过公开输出上限的字符串。
    let mut rendered = String::new();
    // 按声明顺序拼接全部段。
    for (index, segment) in segments.iter().enumerate() {
        // 按封闭段类型追加文本。
        match segment {
            // literal 原样追加。
            SequenceTemplateSegment::Literal { text } => {
                // 在分配前计算追加后的最终字节数。
                let attempted_bytes = rendered.len().saturating_add(text.len());
                // 不允许静态段令已渲染字符串突破最终上限。
                if attempted_bytes > MAX_SEQUENCE_BOUND_VALUE_BYTES {
                    // 返回安全字节证据且不复制 literal。
                    return Err(template_failure(
                        // 传递触发超限的段索引。
                        index,
                        // 输出超限不归因于来源步骤。
                        None,
                        // 不回显 Pointer。
                        None,
                        // 使用封闭输出原因。
                        "template-output-too-large",
                        // 报告不需要真实分配的最终字节数。
                        Some(attempted_bytes),
                    ));
                }
                // 不解释任何表达式或转义。
                rendered.push_str(text);
            }
            // source 只读取 JSON string。
            SequenceTemplateSegment::Source {
                // 借用一基来源索引。
                source_step,
                // 借用来源 Pointer。
                source_pointer,
            } => {
                // 防御性取得来源步骤记录。
                let Some(source_record) = source_step
                    // 把一基索引转换为零基。
                    .checked_sub(1)
                    // 从已建立事实中读取来源。
                    .and_then(|source_index| results.get(source_index))
                else {
                    // 报告来源结果不可用。
                    return Err(template_failure(
                        // 传递当前零基段索引。
                        index,
                        // 保存来源步骤。
                        Some(*source_step),
                        // 保存有界 Pointer。
                        Some(source_pointer.clone()),
                        // 使用封闭原因。
                        "source-result-unavailable",
                        // 不报告字节证据。
                        None,
                    ));
                };
                // provider 失败来源没有可用成功结果。
                if source_record.get("ok") != Some(&Value::Bool(true)) {
                    // 保留来源失败事实并停止当前步骤。
                    return Err(template_failure(
                        // 传递段索引。
                        index,
                        // 保存来源步骤。
                        Some(*source_step),
                        // 保存来源 Pointer。
                        Some(source_pointer.clone()),
                        // 使用封闭原因。
                        "source-step-failed",
                        // 不报告字节证据。
                        None,
                    ));
                }
                // 只读取完整纳入结果预算的来源结果。
                let Some(source_result) = source_record.get("result") else {
                    // 省略结果不能视为空字符串。
                    return Err(template_failure(
                        // 传递段索引。
                        index,
                        // 保存来源步骤。
                        Some(*source_step),
                        // 保存来源 Pointer。
                        Some(source_pointer.clone()),
                        // 使用封闭原因。
                        "source-result-unavailable",
                        // 不报告字节证据。
                        None,
                    ));
                };
                // 使用已经验证的 Pointer 读取来源值。
                let Some(source_value) = source_result.pointer(source_pointer) else {
                    // 缺失来源路径时失败闭合。
                    return Err(template_failure(
                        // 传递段索引。
                        index,
                        // 保存来源步骤。
                        Some(*source_step),
                        // 保存来源 Pointer。
                        Some(source_pointer.clone()),
                        // 使用封闭原因。
                        "source-pointer-missing",
                        // 不报告字节证据。
                        None,
                    ));
                };
                // 模板不做隐式 JSON 类型转换。
                let Some(source_text) = source_value.as_str() else {
                    // 非字符串来源使用独立封闭原因。
                    return Err(template_failure(
                        // 传递段索引。
                        index,
                        // 保存来源步骤。
                        Some(*source_step),
                        // 保存来源 Pointer。
                        Some(source_pointer.clone()),
                        // 使用类型不匹配原因。
                        "template-source-type-mismatch",
                        // 不报告来源值或类型文本。
                        None,
                    ));
                };
                // 在分配前计算追加来源后的最终字节数。
                let attempted_bytes = rendered.len().saturating_add(source_text.len());
                // 大来源字符串不得造成无界临时分配。
                if attempted_bytes > MAX_SEQUENCE_BOUND_VALUE_BYTES {
                    // 返回安全字节证据且不复制来源值。
                    return Err(template_failure(
                        // 传递触发超限的段索引。
                        index,
                        // 输出超限只报告段位置而不归因来源值。
                        None,
                        // 不回显 Pointer。
                        None,
                        // 使用封闭输出原因。
                        "template-output-too-large",
                        // 报告不需要真实分配的最终字节数。
                        Some(attempted_bytes),
                    ));
                }
                // 原样追加来源字符串。
                rendered.push_str(source_text);
            }
        }
    }
    // 返回拥有型 JSON string，不保留任何来源借用。
    Ok(Value::String(rendered))
}

// 构造一个不泄漏模板文本和值的运行时失败。
fn template_failure(
    // 接收零基段索引。
    index: usize,
    // 接收可选来源步骤。
    source_step: Option<usize>,
    // 接收可选来源 Pointer。
    source_pointer: Option<String>,
    // 接收封闭失败原因。
    reason: &'static str,
    // 接收可选字节证据。
    attempted_bytes: Option<usize>,
) -> SequenceTemplateFailure {
    // 返回一基定位和安全证据。
    SequenceTemplateFailure {
        // 转换为一基段索引。
        segment_index: index + 1,
        // 保存可选来源步骤。
        source_step,
        // 保存可选来源 Pointer。
        source_pointer,
        // 保存封闭原因。
        reason,
        // 保存可选字节证据。
        attempted_bytes,
    }
}

// 把模板 Module 私有单元测试拆到独立文件以保持生产文件聚焦。
#[cfg(test)]
#[path = "sequence_templates_tests.rs"]
mod tests;
