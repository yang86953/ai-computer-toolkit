//! 冻结 Linux 私有 MPRIS observation worker 的无 I/O 输入契约。

use serde::Deserialize;
use serde_json::Value;

use super::linux_media_worker_contract_common as common;

/// worker 与父 Module 之间固定的内部协议版本。
pub(crate) const CONTRACT_VERSION: &str = "act/internal/linux-mpris-observation/v1";
/// 保留旧 worker 的 crate 内部导出，同时让上限的单一来源位于 common Component。
pub(crate) use common::{MAXIMUM_INPUT_BYTES, MAXIMUM_OUTPUT_BYTES};

#[cfg(test)]
const DEFAULT_MAXIMUM_ITEMS: u32 = common::DEFAULT_MAXIMUM_ITEMS;
#[cfg(test)]
const MAXIMUM_TIMEOUT_MS: u32 = common::MAXIMUM_TIMEOUT_MS;

const fn default_maximum_items() -> u32 {
    common::default_maximum_items()
}

const fn default_timeout_ms() -> u32 {
    common::default_timeout_ms()
}

/// 表示 worker 只读协议允许的两种 operation。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LinuxMediaObservationOperation {
    /// 有界发现当前总线上的 MPRIS 会话。
    Discover,
    /// 对一个 canonical media target 做唯一重新解析与状态读取。
    State,
}

impl LinuxMediaObservationOperation {
    /// 返回协议中的稳定 operation 文本。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Discover => "discover",
            Self::State => "state",
        }
    }
}

/// 保存严格验证后的私有 MPRIS worker 请求。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LinuxMediaObservationWorkerInput {
    /// 保存固定协议版本；parse 后一定等于 CONTRACT_VERSION。
    protocol_version: String,
    /// 保存封闭的只读 operation。
    operation: String,
    /// 保存仅允许 unix:path= 形式的总线地址。
    session_bus_address: String,
    /// 保存只允许 ASCII 的 broker 代际。
    broker_epoch: String,
    /// 保存 runtime resolver 交付的规范化总线 GUID；旧 raw fixture 可省略。
    #[serde(default)]
    expected_bus_guid: Option<String>,
    /// 保存 discovery 与 state 重新枚举共同使用的数量上限。
    #[serde(default = "default_maximum_items")]
    maximum_items: u32,
    /// 保存覆盖单次请求的总 deadline。
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
    /// discover 不允许目标；state 必须保存 canonical media target。
    #[serde(default)]
    target_id: Option<String>,
}

impl LinuxMediaObservationWorkerInput {
    /// 严格解析请求，拒绝未知字段、所有层级的 null 与非法原始内容。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        if value.as_object().is_none() || common::contains_null(value) {
            return Err(invalid_input_message());
        }
        let input =
            serde_json::from_value::<Self>(value.clone()).map_err(|_| invalid_input_message())?;
        input.validate()
    }

    /// 校验版本、地址、代际、边界与 operation/target 互斥关系。
    fn validate(self) -> Result<Self, &'static str> {
        if !common::validate_common_fields(common::CommonValidationInput {
            protocol_version: &self.protocol_version,
            expected_protocol_version: CONTRACT_VERSION,
            session_bus_address: &self.session_bus_address,
            broker_epoch: &self.broker_epoch,
            target_id: self.target_id.as_deref(),
            target_requirement: common::TargetRequirement::Optional,
            maximum_items: self.maximum_items,
            timeout_ms: self.timeout_ms,
        }) || !self
            .expected_bus_guid
            .as_deref()
            .is_none_or(common::valid_bus_guid)
        {
            return Err(invalid_input_message());
        }

        match self.operation.as_str() {
            "discover" if self.target_id.is_none() => Ok(self),
            "state" if self.target_id.is_some() => Ok(self),
            _ => Err(invalid_input_message()),
        }
    }

    /// 返回已验证的固定协议版本。
    pub(crate) fn protocol_version(&self) -> &str {
        &self.protocol_version
    }

    /// 返回封闭 operation。
    pub(crate) fn operation(&self) -> LinuxMediaObservationOperation {
        match self.operation.as_str() {
            "discover" => LinuxMediaObservationOperation::Discover,
            // validate 已保证这里只可能是 state。
            "state" => LinuxMediaObservationOperation::State,
            _ => unreachable!("validated MPRIS operation must be discover or state"),
        }
    }

    /// 返回供 Config 借用或复制的已验证总线地址。
    pub(crate) fn session_bus_address(&self) -> &str {
        &self.session_bus_address
    }

    /// 返回供 Config 借用或复制的已验证 broker 代际。
    pub(crate) fn broker_epoch(&self) -> &str {
        &self.broker_epoch
    }

    /// 返回可选的跨进程总线 GUID 复核值。
    pub(crate) fn expected_bus_guid(&self) -> Option<&str> {
        self.expected_bus_guid.as_deref()
    }

    /// 返回有界发现/重新枚举数量上限。
    pub(crate) const fn maximum_items(&self) -> u32 {
        self.maximum_items
    }

    /// 返回覆盖整个 worker 请求的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }

    /// 返回 state 使用的已验证 opaque target；discover 恒为 None。
    pub(crate) fn target_id(&self) -> Option<&str> {
        self.target_id.as_deref()
    }
}

fn invalid_input_message() -> &'static str {
    "Linux MPRIS observation worker input violates protocol v1."
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_discover_with_defaults_and_borrowed_fields() {
        let Ok(input) = LinuxMediaObservationWorkerInput::parse(&json!({
            "protocolVersion": CONTRACT_VERSION,
            "operation": "discover",
            "sessionBusAddress": "unix:path=/run/user/1000/bus",
            "brokerEpoch": "fixture-epoch_1",
        })) else {
            panic!("合法 discover 请求必须解析");
        };
        assert_eq!(input.protocol_version(), CONTRACT_VERSION);
        assert_eq!(input.operation(), LinuxMediaObservationOperation::Discover);
        assert_eq!(input.operation().as_str(), "discover");
        assert_eq!(input.session_bus_address(), "unix:path=/run/user/1000/bus");
        assert_eq!(input.broker_epoch(), "fixture-epoch_1");
        assert_eq!(input.maximum_items(), DEFAULT_MAXIMUM_ITEMS);
        assert_eq!(input.timeout_ms(), MAXIMUM_TIMEOUT_MS);
        assert_eq!(input.target_id(), None);
    }

    #[test]
    fn parses_state_and_keeps_maximum_items_for_bounded_reenumeration() {
        let Ok(input) = LinuxMediaObservationWorkerInput::parse(&json!({
            "protocolVersion": CONTRACT_VERSION,
            "operation": "state",
            "sessionBusAddress": "unix:path=/tmp/mpris-bus",
            "brokerEpoch": "epoch:42",
            "maximumItems": 1,
            "timeoutMs": 1,
            "targetId": "s2:m:0123456789abcdef",
        })) else {
            panic!("合法 state 请求必须解析");
        };
        assert_eq!(input.operation(), LinuxMediaObservationOperation::State);
        assert_eq!(input.maximum_items(), 1);
        assert_eq!(input.timeout_ms(), 1);
        assert_eq!(input.target_id(), Some("s2:m:0123456789abcdef"));
    }

    #[test]
    fn rejects_unknown_null_and_operation_target_mixing() {
        assert!(
            LinuxMediaObservationWorkerInput::parse(&json!({
                "protocolVersion": CONTRACT_VERSION,
                "operation": "discover",
                "sessionBusAddress": "unix:path=/tmp/bus",
                "brokerEpoch": "epoch",
                "extra": false,
            }))
            .is_err()
        );
        assert!(
            LinuxMediaObservationWorkerInput::parse(&json!({
                "protocolVersion": CONTRACT_VERSION,
                "operation": "discover",
                "sessionBusAddress": "unix:path=/tmp/bus",
                "brokerEpoch": "epoch",
                "targetId": null,
            }))
            .is_err()
        );
        assert!(
            LinuxMediaObservationWorkerInput::parse(&json!({
                "protocolVersion": CONTRACT_VERSION,
                "operation": "discover",
                "sessionBusAddress": "unix:path=/tmp/bus",
                "brokerEpoch": "epoch",
                "targetId": "s2:m:0123456789abcdef",
            }))
            .is_err()
        );
        assert!(
            LinuxMediaObservationWorkerInput::parse(&json!({
                "protocolVersion": CONTRACT_VERSION,
                "operation": "state",
                "sessionBusAddress": "unix:path=/tmp/bus",
                "brokerEpoch": "epoch",
            }))
            .is_err()
        );
        assert!(
            LinuxMediaObservationWorkerInput::parse(&json!({
                "protocolVersion": CONTRACT_VERSION,
                "operation": "state",
                "sessionBusAddress": "unix:path=/tmp/bus",
                "brokerEpoch": { "nested": null },
                "targetId": "s2:m:0123456789abcdef",
            }))
            .is_err()
        );
    }

    #[test]
    fn enforces_address_and_epoch_boundaries() {
        let base = json!({
            "protocolVersion": CONTRACT_VERSION,
            "operation": "discover",
            "sessionBusAddress": "unix:path=/tmp/bus",
            "brokerEpoch": "epoch",
        });
        assert!(LinuxMediaObservationWorkerInput::parse(&base).is_ok());

        for address in [
            "/tmp/bus",
            "unix:abstract=mpris",
            "unix:path=",
            "unix:path=/tmp/bus;tcp:host=...",
            "unix:path=/tmp\nbus",
        ] {
            let mut value = base.clone();
            value["sessionBusAddress"] = json!(address);
            assert!(LinuxMediaObservationWorkerInput::parse(&value).is_err());
        }
        let mut guid_address = base.clone();
        guid_address["sessionBusAddress"] = json!("unix:path=/tmp/bus,guid=0123456789abcdef");
        assert!(LinuxMediaObservationWorkerInput::parse(&guid_address).is_ok());

        let mut nul_address = base.clone();
        nul_address["sessionBusAddress"] = json!("unix:path=/tmp\u{0}bus");
        assert!(LinuxMediaObservationWorkerInput::parse(&nul_address).is_err());

        for epoch in ["", "epoch value", "époch", "epoch/next"] {
            let mut value = base.clone();
            value["brokerEpoch"] = json!(epoch);
            assert!(LinuxMediaObservationWorkerInput::parse(&value).is_err());
        }
        let mut long_epoch = base.clone();
        long_epoch["brokerEpoch"] = json!("a".repeat(129));
        assert!(LinuxMediaObservationWorkerInput::parse(&long_epoch).is_err());
        let mut long_address = base;
        long_address["sessionBusAddress"] = json!(format!("unix:path={}", "a".repeat(4087)));
        assert!(LinuxMediaObservationWorkerInput::parse(&long_address).is_err());
    }

    #[test]
    fn enforces_target_and_numeric_bounds() {
        let mut state = json!({
            "protocolVersion": CONTRACT_VERSION,
            "operation": "state",
            "sessionBusAddress": "unix:path=/tmp/bus",
            "brokerEpoch": "epoch",
            "targetId": "s2:m:0123456789abcdef",
        });
        assert!(LinuxMediaObservationWorkerInput::parse(&state).is_ok());
        for target in [
            "s2:m:0123456789ABCDEf",
            "s2:m:0123456789abcde",
            "s2:w:0123456789abcdef",
        ] {
            state["targetId"] = json!(target);
            assert!(LinuxMediaObservationWorkerInput::parse(&state).is_err());
        }

        let mut discover = json!({
            "protocolVersion": CONTRACT_VERSION,
            "operation": "discover",
            "sessionBusAddress": "unix:path=/tmp/bus",
            "brokerEpoch": "epoch",
        });
        for maximum_items in [0, 129] {
            discover["maximumItems"] = json!(maximum_items);
            assert!(LinuxMediaObservationWorkerInput::parse(&discover).is_err());
        }
        for timeout_ms in [0, 30_001] {
            discover["maximumItems"] = json!(DEFAULT_MAXIMUM_ITEMS);
            discover["timeoutMs"] = json!(timeout_ms);
            assert!(LinuxMediaObservationWorkerInput::parse(&discover).is_err());
        }
    }

    #[test]
    fn validates_optional_expected_bus_guid_shape() {
        let mut value = json!({
            "protocolVersion": CONTRACT_VERSION,
            "operation": "discover",
            "sessionBusAddress": "unix:path=/tmp/bus",
            "brokerEpoch": "epoch",
        });
        value["expectedBusGuid"] = json!("0123456789ABCDEF0123456789abcdef");
        assert!(LinuxMediaObservationWorkerInput::parse(&value).is_err());
        value["expectedBusGuid"] = json!("0123456789abcdef0123456789abcde");
        assert!(LinuxMediaObservationWorkerInput::parse(&value).is_err());
        value["expectedBusGuid"] = json!("0123456789abcdef0123456789abcdef");
        assert!(LinuxMediaObservationWorkerInput::parse(&value).is_ok());
    }
}
