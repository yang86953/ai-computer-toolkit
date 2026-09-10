//! 实现 sequence Workflow 私有的有界跨步骤输入绑定。

// 导入公开序列化契约派生。
use serde::{Deserialize, Serialize};
// 导入 JSON 值与证据构造宏。
use serde_json::{Value, json};

// 导入 JSON Pointer 语法 Component。
use crate::components::json_postcondition::is_valid_json_pointer;
// 导入公开请求、错误、动词与 provider-neutral JSON Map。
use crate::domain::{AppControlError, AppResult, CommandRequest, JsonMap, Verb};
// 导入 Workflow 公开硬边界。
use super::sequence::{
    MAX_SEQUENCE_BINDING_FIELD_BYTES, MAX_SEQUENCE_BINDING_POINTER_BYTES, MAX_SEQUENCE_BINDINGS,
    MAX_SEQUENCE_BOUND_VALUE_BYTES,
};
// 导入稳定绑定失败定位。
use super::sequence_binding_errors::SequenceBindingFailure;
// 导入结构化模板类型、验证与渲染结果。
use super::sequence_templates::{
    SequenceTemplateFailure, SequenceTemplateSegment, render_sequence_template,
    validate_sequence_template,
};

// 表示绑定只允许写入的两个 provider-neutral 请求区域。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
// 使用封闭小写文本。
#[serde(rename_all = "lowercase")]
pub enum SequenceBindingDestination {
    // 替换调用方预先声明的 target 顶层字段。
    Target,
    // 替换调用方预先声明的 args 顶层字段。
    Args,
}

// 为绑定目标提供稳定公开文本。
impl SequenceBindingDestination {
    // 返回 schema 使用的封闭目标名称。
    const fn as_str(self) -> &'static str {
        // 映射全部封闭目标。
        match self {
            // target 保持小写文本。
            Self::Target => "target",
            // args 保持小写文本。
            Self::Args => "args",
        }
    }
}

// 表示一个封闭且只复制有界 JSON 值的跨步骤绑定。
#[derive(Debug, Deserialize, Serialize)]
// 使用 camelCase 并拒绝未知字段。
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SequenceBinding {
    // 保存直接绑定的一基来源索引。
    #[serde(skip_serializing_if = "Option::is_none")]
    source_step: Option<usize>,
    // 保存直接绑定的 RFC 6901 来源 Pointer。
    #[serde(skip_serializing_if = "Option::is_none")]
    source_pointer: Option<String>,
    // 保存与直接来源恰好二选一的结构化模板段。
    #[serde(skip_serializing_if = "Option::is_none")]
    template: Option<Vec<SequenceTemplateSegment>>,
    // 保存 target 或 args 封闭目标区域。
    destination: SequenceBindingDestination,
    // 保存旧契约中必须预先声明的顶层目标字段。
    #[serde(skip_serializing_if = "Option::is_none")]
    destination_field: Option<String>,
    // 保存允许创建叶字段的非根嵌套目标 Pointer。
    #[serde(skip_serializing_if = "Option::is_none")]
    destination_pointer: Option<String>,
}

// 表示一个已经通过二选一判断的绑定来源形状。
#[derive(Clone, Copy)]
enum SequenceBindingSource<'a> {
    // 保留复制单个完整 JSON 值的旧来源。
    Direct {
        // 保存一基来源步骤。
        source_step: usize,
        // 借用来源 Pointer。
        source_pointer: &'a str,
    },
    // 表示确定性渲染为 JSON string 的结构化模板。
    Template(&'a [SequenceTemplateSegment]),
}

// 为绑定声明解析恰好一个来源形状。
impl SequenceBinding {
    // 返回完整直接来源或模板，否则由静态验证拒绝。
    fn source(&self) -> Option<SequenceBindingSource<'_>> {
        // 同时匹配直接来源对和模板。
        match (
            // 复制可选一基索引。
            self.source_step,
            // 借用可选 Pointer。
            self.source_pointer.as_deref(),
            // 借用可选模板段。
            self.template.as_deref(),
        ) {
            // 完整直接来源与模板互斥。
            (Some(source_step), Some(source_pointer), None) => {
                // 返回旧直接来源形状。
                Some(SequenceBindingSource::Direct {
                    // 保存来源步骤。
                    source_step,
                    // 保存 Pointer 借用。
                    source_pointer,
                })
            }
            // 模板不能同时携带顶层直接来源字段。
            (None, None, Some(template)) => Some(SequenceBindingSource::Template(template)),
            // 部分直接来源、两种同时声明或全部缺失均无效。
            _ => None,
        }
    }
}

// 表示一个已经通过二选一判断的绑定目标路径声明。
#[derive(Clone, Copy)]
enum SequenceBindingPath<'a> {
    // 保留旧顶层字段替换语义。
    Field(&'a str),
    // 提供受限嵌套对象叶字段写入语义。
    Pointer(&'a str),
}

// 为绑定目标路径提供统一验证和写入信息。
impl SequenceBindingPath<'_> {
    // 返回公开输入中承载本路径的字段名。
    const fn input_field(self) -> &'static str {
        // 映射两个向后兼容形状。
        match self {
            // 旧形状使用 destinationField。
            Self::Field(_) => "destinationField",
            // 新形状使用 destinationPointer。
            Self::Pointer(_) => "destinationPointer",
        }
    }

    // 把两种形状规范化为解码后的对象字段路径。
    fn segments(self) -> Vec<String> {
        // 旧顶层字段天然只有一个路径段。
        match self {
            // 复制旧字段名形成规范路径。
            Self::Field(field) => vec![field.to_owned()],
            // 解码已经过语法验证的 RFC 6901 Pointer。
            Self::Pointer(pointer) => decode_json_pointer(pointer),
        }
    }

    // 判断是否允许创建最终叶字段。
    const fn permits_leaf_creation(self) -> bool {
        // 只有新 Pointer 形状允许创建叶字段。
        matches!(self, Self::Pointer(_))
    }

    // 返回防御性运行时目标漂移原因。
    const fn unavailable_reason(self) -> &'static str {
        // 保持旧错误文本并为新路径提供独立原因。
        match self {
            // 旧顶层字段继续使用既有原因。
            Self::Field(_) => "destination-field-missing",
            // 新嵌套路径使用不会泄漏路径内容的原因。
            Self::Pointer(_) => "destination-path-unavailable",
        }
    }
}

// 为绑定声明解析恰好一个目标路径形状。
impl SequenceBinding {
    // 返回二选一成立的路径，否则留给结构验证失败闭合。
    fn destination_path(&self) -> Option<SequenceBindingPath<'_>> {
        // 同时匹配两个可选公开字段。
        match (
            // 借用旧顶层字段。
            self.destination_field.as_deref(),
            // 借用新嵌套 Pointer。
            self.destination_pointer.as_deref(),
        ) {
            // 只声明旧字段时保留原行为。
            (Some(field), None) => Some(SequenceBindingPath::Field(field)),
            // 只声明新 Pointer 时启用受限叶创建。
            (None, Some(pointer)) => Some(SequenceBindingPath::Pointer(pointer)),
            // 两者同时存在或同时缺失都不形成目标。
            _ => None,
        }
    }
}

// 解码已经通过 RFC 6901 语法验证的非根 Pointer。
fn decode_json_pointer(pointer: &str) -> Vec<String> {
    // 跳过非根 Pointer 开头的空片段并逐段解码。
    pointer
        // 按斜杠分隔对象字段。
        .split('/')
        // 丢弃开头空片段。
        .skip(1)
        // 依 RFC 6901 顺序还原转义。
        .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
        // 收集拥有型规范路径。
        .collect()
}

// 判断目标路径的全部父段是否已存在且保持对象类型。
fn destination_parents_are_objects(destination: &JsonMap, segments: &[String]) -> bool {
    // 非根目标至少必须包含一个叶字段段。
    if segments.is_empty() {
        // 空路径不能形成受限叶字段写入。
        return false;
    }
    // 单段 Pointer 的父对象就是静态 target 或 args 根对象。
    if segments.len() == 1 {
        // 根对象由 CommandRequest 类型保证存在。
        return true;
    }
    // 取得首个父字段，禁止隐式创建父对象。
    let Some(mut current) = destination.get(&segments[0]) else {
        // 缺失首个父字段时失败闭合。
        return false;
    };
    // 逐层读取叶字段之前的剩余父段。
    for segment in &segments[1..segments.len() - 1] {
        // 当前父值必须保持对象类型而不是数组或标量。
        let Some(object) = current.as_object() else {
            // 类型不匹配时拒绝穿透。
            return false;
        };
        // 下一个父字段同样必须已经静态存在。
        let Some(next) = object.get(segment) else {
            // 不递归创建缺失父对象。
            return false;
        };
        // 推进到下一个父值。
        current = next;
    }
    // 最终叶字段的直接父值必须是对象。
    current.is_object()
}

// 判断两个规范对象路径是否重复或存在祖先/后代覆盖。
fn destination_paths_overlap(left: &[String], right: &[String]) -> bool {
    // 取较短长度用于公共前缀比较。
    let common_length = left.len().min(right.len());
    // 较短路径的全部段相等就表示两个写入范围重叠。
    left[..common_length] == right[..common_length]
}

// 把有界拥有型值写入已经验证的目标对象叶字段。
fn write_destination_value(
    // 接收唯一允许修改的 target 或 args 根对象。
    destination: &mut JsonMap,
    // 借用解码后的非根规范路径。
    segments: &[String],
    // 指示是否允许创建最终叶字段。
    permits_leaf_creation: bool,
    // 接收已经完成字节预算验证的拥有型值。
    value: Value,
) -> bool {
    // 拆分最终叶字段与全部父段。
    let Some((leaf, parents)) = segments.split_last() else {
        // 防御性拒绝空路径。
        return false;
    };
    // 单段路径直接写根对象字段。
    if parents.is_empty() {
        // 旧字段形状不得在运行时创建缺失字段。
        if !permits_leaf_creation && !destination.contains_key(leaf) {
            // 验证与应用间漂移时失败闭合。
            return false;
        }
        // 新形状可创建叶字段，旧形状只替换既有字段。
        destination.insert(leaf.clone(), value);
        // 报告写入完成。
        return true;
    }
    // 取得静态存在的首个父字段。
    let Some(mut current) = destination.get_mut(&parents[0]) else {
        // 防御性拒绝父路径漂移。
        return false;
    };
    // 逐层取得后续静态父对象字段。
    for segment in &parents[1..] {
        // 当前父值必须仍为对象。
        let Some(object) = current.as_object_mut() else {
            // 较早绑定若破坏父对象则失败闭合。
            return false;
        };
        // 下一个父字段必须继续存在。
        let Some(next) = object.get_mut(segment) else {
            // 不在运行时递归创建父对象。
            return false;
        };
        // 推进到下一个父值。
        current = next;
    }
    // 叶字段的直接父值必须仍为对象。
    let Some(object) = current.as_object_mut() else {
        // 数组或标量父值不能接受对象字段写入。
        return false;
    };
    // 旧字段形状不得创建嵌套叶字段。
    if !permits_leaf_creation && !object.contains_key(leaf) {
        // 防御性保持旧行为。
        return false;
    }
    // 插入或替换最终叶字段。
    object.insert(leaf.clone(), value);
    // 报告写入完成。
    true
}

// 保存当前步骤绑定组的应用结果。
pub(super) struct SequenceBindingEvaluation {
    // 保存只在声明绑定时出现的有界成功证据。
    pub(super) evidence: Option<Value>,
    // 保存首个失败绑定并触发 Workflow 硬停止。
    pub(super) failure: Option<SequenceBindingFailure>,
}

// 在首个 provider 调用前验证一个步骤的全部绑定结构边界。
pub(super) fn validate_sequence_bindings(
    // 接收当前步骤的零基索引。
    step_index: usize,
    // 接收当前步骤的静态动词以限制模板首批范围。
    verb: Verb,
    // 借用调用方预先声明的 target 字段。
    target: &JsonMap,
    // 借用调用方预先声明的 args 字段。
    args: &JsonMap,
    // 借用当前步骤全部绑定。
    bindings: &[SequenceBinding],
) -> AppResult<()> {
    // 单步骤绑定数量必须满足公开硬上限。
    if bindings.len() > MAX_SEQUENCE_BINDINGS {
        // 返回稳定参数错误和精确字段路径。
        return Err(AppControlError::with_details(
            // 使用现有公开参数错误码。
            "INVALID_ARGUMENT",
            // 说明单步骤绑定上限。
            format!("每个 sequence step 最多允许 {MAX_SEQUENCE_BINDINGS} 个 binding。"),
            // 返回零基字段路径与实际数量。
            json!({
                // 精确定位失败步骤。
                "field": format!("steps[{step_index}].bindings"),
                // 报告调用方实际数量。
                "actual": bindings.len(),
                // 报告公开硬上限。
                "maximum": MAX_SEQUENCE_BINDINGS,
            }),
        ));
    }
    // 创建仅服务本步骤的规范目标路径集合。
    let mut destinations: Vec<(&'static str, Vec<String>)> =
        // 按单步骤公开上限预分配规范路径记录。
        Vec::with_capacity(bindings.len());
    // 逐项验证来源、Pointer、字段与重复目标。
    for (binding_index, binding) in bindings.iter().enumerate() {
        // 构造当前绑定的稳定字段路径前缀。
        let field_prefix = format!("steps[{step_index}].bindings[{binding_index}]");
        // 直接来源对与模板必须恰好二选一。
        let Some(binding_source) = binding.source() else {
            // 拒绝部分来源、双重来源和空来源。
            return Err(AppControlError::with_details(
                // 使用统一参数错误码。
                "INVALID_ARGUMENT",
                // 说明两个来源形状的互斥规则。
                "binding 必须恰好声明完整 sourceStep/sourcePointer 对或 template 之一。",
                // 返回当前绑定位置和封闭形状。
                json!({
                    // 定位绑定对象。
                    "field": field_prefix,
                    // 报告两个合法形状。
                    "requiredExactlyOne": ["sourceStep+sourcePointer", "template"],
                }),
            ));
        };
        // 按来源形状执行专属静态验证。
        if let SequenceBindingSource::Template(template) = binding_source {
            // 委托模板 Module 验证只读范围、段和来源边界。
            validate_sequence_template(step_index, binding_index, verb, template)?;
        }
        // 直接来源必须是一基且严格早于当前步骤。
        if let SequenceBindingSource::Direct {
            // 复制一基来源索引。
            source_step,
            // 本判断不需要读取已经借用的来源 Pointer。
            source_pointer: _,
        } = binding_source
            && (source_step == 0 || source_step > step_index)
        {
            // 拒绝第一步来源、当前步骤来源和未来步骤来源。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明来源必须严格更早。
                "binding sourceStep 必须引用严格更早的 sequence step。",
                // 返回字段位置与允许边界。
                json!({
                    // 精确定位来源字段。
                    "field": format!("{field_prefix}.sourceStep"),
                    // 报告实际一基来源索引。
                    "actual": source_step,
                    // 报告最小合法索引。
                    "minimum": 1,
                    // 当前零基索引等于最大合法一基来源索引。
                    "maximum": step_index,
                }),
            ));
        }
        // 直接来源 Pointer 采用 UTF-8 字节硬上限。
        if let SequenceBindingSource::Direct { source_pointer, .. } = binding_source
            && source_pointer.len() > MAX_SEQUENCE_BINDING_POINTER_BYTES
        {
            // 返回稳定参数错误和真实字节数。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明来源 Pointer 字节上限。
                format!(
                    "binding sourcePointer 最多允许 {MAX_SEQUENCE_BINDING_POINTER_BYTES} 个 UTF-8 字节。"
                ),
                // 返回精确字段路径和字节证据。
                json!({
                    // 定位来源 Pointer 字段。
                    "field": format!("{field_prefix}.sourcePointer"),
                    // 报告真实 UTF-8 字节数。
                    "actualBytes": source_pointer.len(),
                    // 报告公开硬上限。
                    "maximumBytes": MAX_SEQUENCE_BINDING_POINTER_BYTES,
                }),
            ));
        }
        // 直接来源 Pointer 必须满足 RFC 6901 语法。
        if let SequenceBindingSource::Direct { source_pointer, .. } = binding_source
            && !is_valid_json_pointer(source_pointer)
        {
            // 返回不依赖 provider 的稳定参数错误。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明采用 RFC 6901 Pointer 语法。
                "binding sourcePointer 必须是有效的 RFC 6901 JSON Pointer。",
                // 只返回字段路径，不复制可能敏感的 Pointer 内容。
                json!({ "field": format!("{field_prefix}.sourcePointer") }),
            ));
        }
        // 两个向后兼容目标形状必须恰好声明一个。
        let Some(destination_path) = binding.destination_path() else {
            // 同时声明或同时缺失时拒绝整个 Workflow。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明两个形状必须二选一。
                "binding 必须恰好声明 destinationField 或 destinationPointer 之一。",
                // 返回绑定位置与封闭候选字段。
                json!({
                    // 定位当前绑定对象。
                    "field": field_prefix,
                    // 报告恰好二选一要求。
                    "requiredExactlyOne": ["destinationField", "destinationPointer"],
                }),
            ));
        };
        // 旧目标字段必须非空且满足 UTF-8 字节硬上限。
        if let SequenceBindingPath::Field(destination_field) = destination_path
            && (destination_field.is_empty()
                || destination_field.len() > MAX_SEQUENCE_BINDING_FIELD_BYTES)
        {
            // 返回稳定参数错误和字段名字节证据。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明旧目标字段边界。
                format!(
                    "binding destinationField 必须为 1..={MAX_SEQUENCE_BINDING_FIELD_BYTES} 个 UTF-8 字节。"
                ),
                // 返回精确字段路径和边界证据。
                json!({
                    // 定位目标字段名称。
                    "field": format!("{field_prefix}.destinationField"),
                    // 报告真实 UTF-8 字节数。
                    "actualBytes": destination_field.len(),
                    // 报告最小字节数。
                    "minimumBytes": 1,
                    // 报告最大字节数。
                    "maximumBytes": MAX_SEQUENCE_BINDING_FIELD_BYTES,
                }),
            ));
        }
        // 新目标 Pointer 必须是有界非根 RFC 6901 Pointer。
        if let SequenceBindingPath::Pointer(destination_pointer) = destination_path
            && (destination_pointer.is_empty()
                || destination_pointer.len() > MAX_SEQUENCE_BINDING_POINTER_BYTES
                || !is_valid_json_pointer(destination_pointer))
        {
            // 返回不回显 Pointer 内容的稳定参数错误。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明新目标 Pointer 的完整边界。
                format!(
                    "binding destinationPointer 必须是 1..={MAX_SEQUENCE_BINDING_POINTER_BYTES} 个 UTF-8 字节的非根 RFC 6901 JSON Pointer。"
                ),
                // 返回字段位置与字节边界。
                json!({
                    // 定位目标 Pointer。
                    "field": format!("{field_prefix}.destinationPointer"),
                    // 报告真实 UTF-8 字节数。
                    "actualBytes": destination_pointer.len(),
                    // 报告最小字节数。
                    "minimumBytes": 1,
                    // 报告最大字节数。
                    "maximumBytes": MAX_SEQUENCE_BINDING_POINTER_BYTES,
                }),
            ));
        }
        // 把目标规范化为解码后的对象字段路径。
        let destination_segments = destination_path.segments();
        // 选择调用方静态声明的目标区域。
        let destination_map = match binding.destination {
            // target 绑定只能修改 target 对象。
            SequenceBindingDestination::Target => target,
            // args 绑定只能修改 args 对象。
            SequenceBindingDestination::Args => args,
        };
        // 旧形状继续要求调用方预先声明顶层字段。
        if let SequenceBindingPath::Field(destination_field) = destination_path
            && !destination_map.contains_key(destination_field)
        {
            // 返回稳定参数错误且不回显目标字段内容。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明目标必须由调用方预先声明。
                "binding destinationField 必须已存在于当前 step 的目标对象中。",
                // 返回目标区域和字段位置。
                json!({
                    // 精确定位目标字段声明。
                    "field": format!("{field_prefix}.destinationField"),
                    // 报告 target 或 args 区域。
                    "destination": binding.destination.as_str(),
                }),
            ));
        }
        // 新形状要求全部父路径静态存在且均为对象。
        if matches!(destination_path, SequenceBindingPath::Pointer(_))
            && !destination_parents_are_objects(destination_map, &destination_segments)
        {
            // 拒绝递归创建父对象、数组写入和标量穿透。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明只允许在静态对象父路径下创建或替换叶字段。
                "binding destinationPointer 的全部父路径必须已存在且均为对象。",
                // 返回精确字段位置和目标区域。
                json!({
                    // 定位目标 Pointer。
                    "field": format!("{field_prefix}.destinationPointer"),
                    // 报告 target 或 args 区域。
                    "destination": binding.destination.as_str(),
                }),
            ));
        }
        // 相同区域内相同或祖先/后代目标都会造成声明顺序依赖。
        if destinations.iter().any(|(destination, segments)| {
            // 只比较同一 target 或 args 区域中的路径。
            *destination == binding.destination.as_str()
                // 拒绝任一方向的路径前缀重叠。
                && destination_paths_overlap(segments, &destination_segments)
        }) {
            // 拒绝规范别名、重复叶和祖先/后代覆盖。
            return Err(AppControlError::with_details(
                // 使用现有公开参数错误码。
                "INVALID_ARGUMENT",
                // 说明重叠目标不允许。
                "同一 sequence step 不允许多个 binding 写入重复或重叠的目标路径。",
                // 返回后出现绑定的精确字段位置与区域。
                json!({
                    // 定位当前目标形状字段。
                    "field": format!("{field_prefix}.{}", destination_path.input_field()),
                    // 报告 target 或 args 区域。
                    "destination": binding.destination.as_str(),
                }),
            ));
        }
        // 保存不包含任何目标值的规范路径身份。
        destinations.push((binding.destination.as_str(), destination_segments));
    }
    // 当前步骤全部绑定结构边界成立。
    Ok(())
}

// 把更早成功结果的有界 JSON 值应用到最终统一请求。
pub(super) fn apply_sequence_bindings(
    // 借用当前 Workflow 已经建立的步骤事实。
    results: &[Value],
    // 只允许修改最终请求的 target 与 args。
    request: &mut CommandRequest,
    // 借用已经通过全部结构边界验证的绑定。
    bindings: &[SequenceBinding],
) -> SequenceBindingEvaluation {
    // 未声明绑定时保持旧步骤结果形状。
    if bindings.is_empty() {
        // 返回无证据且无失败的空应用。
        return SequenceBindingEvaluation {
            // 不增加公开字段。
            evidence: None,
            // 不产生工作流错误。
            failure: None,
        };
    }
    // 按声明顺序物化直接值或模板字符串并写入互不重叠的目标。
    for (index, binding) in bindings.iter().enumerate() {
        // 防御性重建已经通过静态验证的来源形状。
        let Some(binding_source) = binding.source() else {
            // 验证与应用不一致时失败闭合。
            return failed_evaluation(index, binding, "source-result-unavailable", None);
        };
        // 按直接复制或结构化模板渲染产生拥有型值。
        let bound_value = match binding_source {
            // 旧来源复制完整 JSON 值。
            SequenceBindingSource::Direct {
                // 复制一基来源步骤。
                source_step,
                // 借用来源 Pointer。
                source_pointer,
            } => {
                // 防御性取得来源步骤记录。
                let Some(source_record) = source_step
                    // 把一基索引转换为零基索引。
                    .checked_sub(1)
                    // 从已建立结果中取得来源记录。
                    .and_then(|source_index| results.get(source_index))
                else {
                    // 输入预验证与运行记录不一致时停止。
                    return failed_evaluation(
                        // 传递绑定索引。
                        index,
                        // 传递失败绑定。
                        binding,
                        // 使用来源结果不可用原因。
                        "source-result-unavailable",
                        // 不报告字节证据。
                        None,
                    );
                };
                // provider 失败来源没有可绑定成功结果。
                if source_record.get("ok") != Some(&Value::Bool(true)) {
                    // 保留来源失败事实并停止当前步骤。
                    return failed_evaluation(index, binding, "source-step-failed", None);
                }
                // 只读取完整纳入结果预算的来源结果。
                let Some(source_result) = source_record.get("result") else {
                    // 省略结果不能静默视为空 JSON。
                    return failed_evaluation(
                        // 传递绑定索引。
                        index,
                        // 传递失败绑定。
                        binding,
                        // 使用来源结果不可用原因。
                        "source-result-unavailable",
                        // 不报告字节证据。
                        None,
                    );
                };
                // 使用已经验证的 Pointer 读取来源值。
                let Some(source_value) = source_result.pointer(source_pointer) else {
                    // 缺失 Pointer 在当前 provider 前失败。
                    return failed_evaluation(
                        // 传递绑定索引。
                        index,
                        // 传递失败绑定。
                        binding,
                        // 使用来源 Pointer 缺失原因。
                        "source-pointer-missing",
                        // 不报告字节证据。
                        None,
                    );
                };
                // 以紧凑 JSON 测量直接绑定值字节数。
                let bound_value_bytes = source_value.to_string().len();
                // 单个直接绑定值不得突破公开上限。
                if bound_value_bytes > MAX_SEQUENCE_BOUND_VALUE_BYTES {
                    // 返回字节证据但不回显来源值。
                    return failed_evaluation(
                        // 传递绑定索引。
                        index,
                        // 传递失败绑定。
                        binding,
                        // 使用既有超限原因。
                        "bound-value-too-large",
                        // 报告实际紧凑 JSON 字节数。
                        Some(bound_value_bytes),
                    );
                }
                // 复制拥有型直接值。
                source_value.clone()
            }
            // 新模板只产生有界 JSON string。
            SequenceBindingSource::Template(template) => {
                // 委托模板 Module 读取来源和确定性渲染。
                match render_sequence_template(results, template) {
                    // 返回拥有型模板字符串。
                    Ok(value) => value,
                    // 把模板失败映射为统一绑定终止证据。
                    Err(failure) => {
                        // 当前步骤硬停止且不写任何 provider 结果。
                        return template_failed_evaluation(index, binding, failure);
                    }
                }
            }
        };
        // 防御性重建已经通过结构验证的目标路径。
        let Some(destination_path) = binding.destination_path() else {
            // 验证与应用契约不一致时停止当前步骤。
            return failed_evaluation(index, binding, "destination-path-unavailable", None);
        };
        // 规范化目标路径以支持嵌套对象叶字段。
        let destination_segments = destination_path.segments();
        // 选择唯一允许修改的 provider-neutral 请求区域。
        let destination_map = match binding.destination {
            // target 绑定不能影响其他控制字段。
            SequenceBindingDestination::Target => &mut request.target,
            // args 绑定不能影响其他控制字段。
            SequenceBindingDestination::Args => &mut request.args,
        };
        // 写入静态父对象下的叶字段，不保留来源结果借用。
        if !write_destination_value(
            // 传递唯一可变请求区域。
            destination_map,
            // 传递规范对象路径。
            &destination_segments,
            // 新 Pointer 可创建叶字段，旧字段保持替换语义。
            destination_path.permits_leaf_creation(),
            // 移动有界拥有型 JSON 值。
            bound_value,
        ) {
            // 目标漂移时失败闭合且不得启动 provider。
            return failed_evaluation(
                // 传递当前零基绑定索引。
                index,
                // 传递失败绑定。
                binding,
                // 按公开形状选择稳定失败原因。
                destination_path.unavailable_reason(),
                // 目标失败没有值字节证据。
                None,
            );
        }
    }
    // 全部绑定应用成功时返回有界证据。
    SequenceBindingEvaluation {
        // 只报告应用数量，不回显来源或最终值。
        evidence: Some(json!({
            // 报告已经应用的绑定总数。
            "applied": bindings.len(),
        })),
        // 不产生工作流错误。
        failure: None,
    }
}

// 构造首个绑定失败的稳定应用结果。
fn failed_evaluation(
    // 接收零基绑定索引。
    index: usize,
    // 借用失败绑定。
    binding: &SequenceBinding,
    // 接收封闭失败原因。
    reason: &'static str,
    // 接收可选字节证据。
    attempted_bytes: Option<usize>,
) -> SequenceBindingEvaluation {
    // 返回不泄漏来源值的失败定位。
    SequenceBindingEvaluation {
        // 失败时不产生成功应用证据。
        evidence: None,
        // 保存 Workflow 构造终止错误所需的最小定位。
        failure: Some(SequenceBindingFailure {
            // 使用一基绑定索引。
            binding_index: index + 1,
            // 保存一基来源步骤索引。
            source_step: binding.source_step,
            // 复制受硬上限约束的来源 Pointer。
            source_pointer: binding.source_pointer.clone(),
            // 直接绑定失败没有模板段索引。
            segment_index: None,
            // 保存封闭目标区域。
            destination: binding.destination.as_str(),
            // 复制受硬上限约束的旧目标字段名。
            destination_field: binding.destination_field.clone(),
            // 复制受硬上限约束的新目标 Pointer。
            destination_pointer: binding.destination_pointer.clone(),
            // 保存封闭失败原因。
            reason,
            // 保存可选字节证据。
            attempted_bytes,
        }),
    }
}

// 把模板 Module 失败映射为统一绑定终止结果。
fn template_failed_evaluation(
    // 接收零基绑定索引。
    index: usize,
    // 借用失败绑定以取得目标定位。
    binding: &SequenceBinding,
    // 接收模板 Module 的安全失败证据。
    failure: SequenceTemplateFailure,
) -> SequenceBindingEvaluation {
    // 返回不回显模板文本或来源值的失败定位。
    SequenceBindingEvaluation {
        // 模板失败不产生应用成功证据。
        evidence: None,
        // 保存统一 Workflow 错误所需字段。
        failure: Some(SequenceBindingFailure {
            // 使用一基绑定索引。
            binding_index: index + 1,
            // 保存失败 source 段的可选来源步骤。
            source_step: failure.source_step,
            // 保存失败 source 段的可选 Pointer。
            source_pointer: failure.source_pointer,
            // 保存一基失败模板段索引。
            segment_index: Some(failure.segment_index),
            // 保存封闭目标区域。
            destination: binding.destination.as_str(),
            // 复制旧目标字段定位。
            destination_field: binding.destination_field.clone(),
            // 复制新目标 Pointer 定位。
            destination_pointer: binding.destination_pointer.clone(),
            // 保存模板封闭失败原因。
            reason: failure.reason,
            // 保存可选输出字节证据。
            attempted_bytes: failure.attempted_bytes,
        }),
    }
}

// 把绑定私有单元测试拆到独立文件以保持生产代码文件小于 900 行。
#[cfg(test)]
#[path = "sequence_bindings_tests.rs"]
mod tests;
