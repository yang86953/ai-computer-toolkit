//! Linux MPRIS observation/control worker 共用的无 I/O 输入边界。
//!
//! 该 Component 只保存两个 worker 的稳定尺寸与字段校验原语；operation、确认语义和
//! 输出时序分别归属于各自的协议 Component/worker，不在这里组合。

use serde_json::Value;

/// 单次 worker 请求允许的最大 UTF-8 输入字节数。
pub(crate) const MAXIMUM_INPUT_BYTES: usize = 64 * 1024;
/// 单次 worker stdout 协议输出允许的最大 UTF-8 总字节数。
pub(crate) const MAXIMUM_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

/// 有界重新枚举的默认数量。
pub(crate) const DEFAULT_MAXIMUM_ITEMS: u32 = 128;
/// 有界重新枚举的最小数量。
pub(crate) const MINIMUM_MAXIMUM_ITEMS: u32 = 1;
/// 有界重新枚举的最大数量。
pub(crate) const MAXIMUM_MAXIMUM_ITEMS: u32 = 128;
/// worker 总 deadline 的最小毫秒数。
pub(crate) const MINIMUM_TIMEOUT_MS: u32 = 1;
/// worker 总 deadline 的最大毫秒数。
pub(crate) const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

const SESSION_BUS_ADDRESS_PREFIX: &str = "unix:path=";
const MEDIA_TARGET_PREFIX: &str = "s2:m:";

/// 表示共享校验对 media target 的要求。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TargetRequirement {
    /// 允许省略 target；若提供则仍必须是 canonical target。
    Optional,
    /// 必须提供 canonical target。
    Required,
}

/// 共享输入校验的结构化参数，避免 observation/control 调用点重复长参数列表。
pub(crate) struct CommonValidationInput<'a> {
    pub(crate) protocol_version: &'a str,
    pub(crate) expected_protocol_version: &'a str,
    pub(crate) session_bus_address: &'a str,
    pub(crate) broker_epoch: &'a str,
    pub(crate) target_id: Option<&'a str>,
    pub(crate) target_requirement: TargetRequirement,
    pub(crate) maximum_items: u32,
    pub(crate) timeout_ms: u32,
}

/// serde `default` 使用的共享数量默认值。
pub(crate) const fn default_maximum_items() -> u32 {
    DEFAULT_MAXIMUM_ITEMS
}

/// serde `default` 使用的共享 timeout 默认值。
pub(crate) const fn default_timeout_ms() -> u32 {
    MAXIMUM_TIMEOUT_MS
}

/// 递归拒绝任意位置的显式 null，避免嵌套值绕过闭合协议。
pub(crate) fn contains_null(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(values) => values.iter().any(contains_null),
        Value::Object(fields) => fields.values().any(contains_null),
        Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

/// 校验 observation/control 共用的版本、连接地址、代际、target 与数值边界。
pub(crate) fn validate_common_fields(input: CommonValidationInput<'_>) -> bool {
    input.protocol_version == input.expected_protocol_version
        && valid_session_bus_address(input.session_bus_address)
        && valid_broker_epoch(input.broker_epoch)
        && valid_target(input.target_id, input.target_requirement)
        && (MINIMUM_MAXIMUM_ITEMS..=MAXIMUM_MAXIMUM_ITEMS).contains(&input.maximum_items)
        && (MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&input.timeout_ms)
}

fn valid_target(target_id: Option<&str>, requirement: TargetRequirement) -> bool {
    match (requirement, target_id) {
        (TargetRequirement::Required, Some(target))
        | (TargetRequirement::Optional, Some(target)) => valid_media_target_id(target),
        (TargetRequirement::Required, None) => false,
        (TargetRequirement::Optional, None) => true,
    }
}

fn valid_session_bus_address(value: &str) -> bool {
    value.len() <= 4096
        && value.starts_with(SESSION_BUS_ADDRESS_PREFIX)
        && value.len() > SESSION_BUS_ADDRESS_PREFIX.len()
        // 只接受单一 unix 地址；`;` 不能引入备用非 unix transport。
        && value
            .bytes()
            .all(|byte| byte != b'\0' && byte != b'\n' && byte != b'\r' && byte != b';')
}

fn valid_broker_epoch(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

/// 校验跨进程复核使用的规范化小写 D-Bus GUID。
pub(crate) fn valid_bus_guid(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_media_target_id(value: &str) -> bool {
    value.len() == MEDIA_TARGET_PREFIX.len() + 16
        && value.starts_with(MEDIA_TARGET_PREFIX)
        && value.as_bytes()[MEDIA_TARGET_PREFIX.len()..]
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
