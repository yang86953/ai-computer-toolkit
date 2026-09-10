//! 投影 sequence 绑定失败的稳定公开终止证据。

// 导入 JSON 值和构造宏。
use serde_json::{Value, json};

// 导入绑定值公开字节上限与错误码。
use super::sequence::{MAX_SEQUENCE_BOUND_VALUE_BYTES, SequenceWorkflowErrorCode};

// 保存一次不泄漏来源值的绑定失败定位。
pub(super) struct SequenceBindingFailure {
    // 保存一基绑定索引。
    pub(super) binding_index: usize,
    // 保存直接绑定或模板段的一基来源步骤索引。
    pub(super) source_step: Option<usize>,
    // 保存直接绑定或模板段的有界来源 Pointer。
    pub(super) source_pointer: Option<String>,
    // 保存只在模板失败时出现的一基段索引。
    pub(super) segment_index: Option<usize>,
    // 保存 target 或 args 封闭目标名称。
    pub(super) destination: &'static str,
    // 保存旧形状中有界的目标字段名。
    pub(super) destination_field: Option<String>,
    // 保存新形状中有界的目标 Pointer。
    pub(super) destination_pointer: Option<String>,
    // 保存封闭失败原因。
    pub(super) reason: &'static str,
    // 仅在值超限时保存实际 UTF-8 字节数。
    pub(super) attempted_bytes: Option<usize>,
}

// 构造阻止当前步骤启动的稳定绑定错误。
pub(super) fn sequence_binding_error(
    // 接收一基当前步骤索引。
    step_index: usize,
    // 接收调用方可选步骤名称。
    step_name: Option<String>,
    // 接收首个失败绑定的最小定位。
    failure: SequenceBindingFailure,
) -> Value {
    // 构造不含可选来源和目标定位的基础错误。
    let mut error = json!({
        // 使用绑定专用稳定错误码。
        "code": SequenceWorkflowErrorCode::BindingFailed,
        // 说明当前步骤没有启动且后续步骤已停止。
        "message": "sequence 输入绑定失败；当前步骤未启动，已停止后续步骤。",
        // 返回不包含来源值或 provider 私有数据的定位证据。
        "details": {
            // 精确定位被阻止的一基步骤索引。
            "stepIndex": step_index,
            // 保留调用方可选名称用于关联。
            "stepName": step_name,
            // provider 尚未启动。
            "stepStarted": false,
            // provider 因绑定失败而未完成。
            "stepCompleted": false,
            // 报告首个失败绑定的一基索引。
            "bindingIndex": failure.binding_index,
            // 报告 target 或 args 封闭目标区域。
            "destination": failure.destination,
            // 报告封闭失败原因。
            "reason": failure.reason,
        },
    });
    // 取得稳定 details 对象并加入全部可选定位。
    if let Some(details) = error.get_mut("details")
        // 只允许修改对象形状。
        && let Some(object) = details.as_object_mut()
    {
        // 直接绑定或失败模板 source 段报告来源步骤。
        if let Some(source_step) = failure.source_step {
            // 插入一基来源索引。
            object.insert("sourceStep".to_owned(), json!(source_step));
        }
        // 直接绑定或失败模板 source 段报告来源 Pointer。
        if let Some(source_pointer) = failure.source_pointer {
            // 插入受硬上限保护的 Pointer。
            object.insert("sourcePointer".to_owned(), json!(source_pointer));
        }
        // 模板失败增加一基段索引。
        if let Some(segment_index) = failure.segment_index {
            // 插入失败段位置。
            object.insert("segmentIndex".to_owned(), json!(segment_index));
        }
        // 旧目标形状继续报告 destinationField。
        if let Some(destination_field) = failure.destination_field {
            // 插入受硬上限保护的字段名。
            object.insert("destinationField".to_owned(), json!(destination_field));
        }
        // 新目标形状报告 destinationPointer。
        if let Some(destination_pointer) = failure.destination_pointer {
            // 插入受硬上限保护的目标 Pointer。
            object.insert("destinationPointer".to_owned(), json!(destination_pointer));
        }
        // 只有值或模板输出超限才增加字节证据。
        if let Some(attempted_bytes) = failure.attempted_bytes {
            // 报告实际 UTF-8 或紧凑 JSON 字节数。
            object.insert("attemptedBytes".to_owned(), json!(attempted_bytes));
            // 报告绑定值统一最大字节数。
            object.insert(
                // 使用稳定字段名。
                "maximumBytes".to_owned(),
                // 使用单一来源常量。
                json!(MAX_SEQUENCE_BOUND_VALUE_BYTES),
            );
        }
    }
    // 返回顶层工作流错误，不创建伪 provider 步骤记录。
    error
}
