//! Linux 私有 MPRIS control worker 的无 I/O 输入契约。
//!
//! 该 Component 只解析并验证请求。总线连接、能力读取、accepted frame 的 flush、MPRIS
//! method 调用与终态投影由 feature-gated worker/Adapter/Module 分层承担。

use serde::Deserialize;
use serde_json::Value;

use super::linux_media_worker_contract_common as common;

/// worker 与父 Module 之间固定的内部控制协议版本。
pub(crate) const CONTRACT_VERSION: &str = "act/internal/linux-mpris-control/v1";

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

fn invalid_input_message() -> &'static str {
    "Linux MPRIS control worker input violates protocol v1."
}

/// 保存严格验证后的单个 MPRIS 控制请求。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LinuxMediaControlWorkerInput {
    /// 保存固定协议版本；parse 后一定等于 CONTRACT_VERSION。
    protocol_version: String,
    /// 保存仅允许 unix:path= 形式的单一总线地址。
    session_bus_address: String,
    /// 保存只允许 ASCII 的 broker 代际。
    broker_epoch: String,
    /// 保存 runtime resolver 交付的规范化总线 GUID；旧 raw fixture 可省略。
    #[serde(default)]
    expected_bus_guid: Option<String>,
    /// 保存每次控制前重新解析的 canonical media target。
    target_id: String,
    /// 保存封闭的 MPRIS 控制 operation。
    operation: String,
    /// 保存调用方确认；false 仍可解析，但 worker 必须零 provider I/O。
    confirmed: bool,
    /// 保存有界重新枚举的数量上限。
    #[serde(default = "default_maximum_items")]
    maximum_items: u32,
    /// 保存覆盖整个控制请求的总 deadline。
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u32,
}

impl LinuxMediaControlWorkerInput {
    /// 严格解析请求，拒绝未知字段、所有层级的 null 与非法共享字段。
    pub(crate) fn parse(value: &Value) -> Result<Self, &'static str> {
        if value.as_object().is_none() || common::contains_null(value) {
            return Err(invalid_input_message());
        }
        let input =
            serde_json::from_value::<Self>(value.clone()).map_err(|_| invalid_input_message())?;
        input.validate()
    }

    /// 校验共享边界与 control 专属 operation；不执行任何 provider I/O。
    fn validate(self) -> Result<Self, &'static str> {
        if !common::validate_common_fields(common::CommonValidationInput {
            protocol_version: &self.protocol_version,
            expected_protocol_version: CONTRACT_VERSION,
            session_bus_address: &self.session_bus_address,
            broker_epoch: &self.broker_epoch,
            target_id: Some(&self.target_id),
            target_requirement: common::TargetRequirement::Required,
            maximum_items: self.maximum_items,
            timeout_ms: self.timeout_ms,
        }) || !self
            .expected_bus_guid
            .as_deref()
            .is_none_or(common::valid_bus_guid)
            || !valid_operation(&self.operation)
        {
            return Err(invalid_input_message());
        }
        Ok(self)
    }

    /// 返回已验证的固定协议版本。
    pub(crate) fn protocol_version(&self) -> &str {
        &self.protocol_version
    }

    /// 返回供 Config 借用或复制的已验证单一总线地址。
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

    /// 返回每次控制前重新解析的 canonical media target。
    pub(crate) fn target_id(&self) -> &str {
        &self.target_id
    }

    /// 返回可直接传给 media_session 的封闭 operation 文本。
    pub(crate) fn operation(&self) -> &str {
        &self.operation
    }

    /// 返回调用方确认事实；false 不代表已执行控制。
    pub(crate) const fn confirmed(&self) -> bool {
        self.confirmed
    }

    /// 返回有界重新枚举的数量上限。
    pub(crate) const fn maximum_items(&self) -> u32 {
        self.maximum_items
    }

    /// 返回覆盖整个 worker 请求的总 deadline。
    pub(crate) const fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }
}

fn valid_operation(operation: &str) -> bool {
    matches!(
        operation,
        "play" | "pause" | "toggle-play-pause" | "stop" | "skip-next" | "skip-previous"
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn valid_input() -> Value {
        json!({
            "protocolVersion": CONTRACT_VERSION,
            "sessionBusAddress": "unix:path=/tmp/mpris-bus",
            "brokerEpoch": "fixture-epoch-1",
            "targetId": "s2:m:0123456789abcdef",
            "operation": "play",
            "confirmed": true,
        })
    }

    #[test]
    fn parses_confirmed_request_with_shared_defaults() {
        let Ok(input) = LinuxMediaControlWorkerInput::parse(&valid_input()) else {
            panic!("合法 MPRIS control 请求必须解析");
        };
        assert_eq!(input.protocol_version(), CONTRACT_VERSION);
        assert_eq!(input.session_bus_address(), "unix:path=/tmp/mpris-bus");
        assert_eq!(input.broker_epoch(), "fixture-epoch-1");
        assert_eq!(input.target_id(), "s2:m:0123456789abcdef");
        assert_eq!(input.operation(), "play");
        assert!(input.confirmed());
        assert_eq!(input.maximum_items(), DEFAULT_MAXIMUM_ITEMS);
        assert_eq!(input.timeout_ms(), MAXIMUM_TIMEOUT_MS);
    }

    #[test]
    fn accepts_only_the_six_provider_operations() {
        for operation in [
            "play",
            "pause",
            "toggle-play-pause",
            "stop",
            "skip-next",
            "skip-previous",
        ] {
            let mut value = valid_input();
            value["operation"] = json!(operation);
            let Ok(input) = LinuxMediaControlWorkerInput::parse(&value) else {
                panic!("封闭 MPRIS operation 必须解析");
            };
            assert_eq!(input.operation(), operation);
        }
    }

    #[test]
    fn parses_unconfirmed_request_without_changing_provider_boundary() {
        let mut value = valid_input();
        value["confirmed"] = json!(false);
        let Ok(input) = LinuxMediaControlWorkerInput::parse(&value) else {
            panic!("confirmed=false 仍必须可解析");
        };
        // Component 只交付确认事实；worker 必须据此在 provider I/O 前返回拒绝。
        assert!(!input.confirmed());
        assert_eq!(input.target_id(), "s2:m:0123456789abcdef");
    }

    #[test]
    fn rejects_unknown_null_shared_fields_and_invalid_operations() {
        let mut unknown = valid_input();
        unknown["unexpected"] = json!(true);
        assert!(LinuxMediaControlWorkerInput::parse(&unknown).is_err());

        let mut nested_null = valid_input();
        nested_null["brokerEpoch"] = json!({ "value": null });
        assert!(LinuxMediaControlWorkerInput::parse(&nested_null).is_err());

        let mut invalid_operation = valid_input();
        invalid_operation["operation"] = json!("seek");
        assert!(LinuxMediaControlWorkerInput::parse(&invalid_operation).is_err());

        let mut missing_confirmation = valid_input();
        if let Some(object) = missing_confirmation.as_object_mut() {
            object.remove("confirmed");
        }
        assert!(LinuxMediaControlWorkerInput::parse(&missing_confirmation).is_err());

        let mut wrong_target = valid_input();
        wrong_target["targetId"] = json!("s2:w:0123456789abcdef");
        assert!(LinuxMediaControlWorkerInput::parse(&wrong_target).is_err());
    }

    #[test]
    fn rejects_non_unix_address_forbidden_delimiters_and_numeric_edges() {
        for address in [
            "/tmp/mpris-bus",
            "unix:abstract=mpris",
            "unix:path=",
            "unix:path=/tmp/bus;tcp:host=...",
            "unix:path=/tmp\nbus",
            "unix:path=/tmp\rbus",
        ] {
            let mut value = valid_input();
            value["sessionBusAddress"] = json!(address);
            assert!(LinuxMediaControlWorkerInput::parse(&value).is_err());
        }

        let mut guid_address = valid_input();
        guid_address["sessionBusAddress"] = json!("unix:path=/tmp/mpris-bus,guid=0123456789abcdef");
        assert!(LinuxMediaControlWorkerInput::parse(&guid_address).is_ok());

        let mut invalid_maximum = valid_input();
        invalid_maximum["maximumItems"] = json!(0);
        assert!(LinuxMediaControlWorkerInput::parse(&invalid_maximum).is_err());
        invalid_maximum["maximumItems"] = json!(129);
        assert!(LinuxMediaControlWorkerInput::parse(&invalid_maximum).is_err());

        let mut invalid_timeout = valid_input();
        invalid_timeout["timeoutMs"] = json!(0);
        assert!(LinuxMediaControlWorkerInput::parse(&invalid_timeout).is_err());
        invalid_timeout["timeoutMs"] = json!(30_001);
        assert!(LinuxMediaControlWorkerInput::parse(&invalid_timeout).is_err());

        let mut invalid_target = valid_input();
        invalid_target["targetId"] = Value::Null;
        assert!(LinuxMediaControlWorkerInput::parse(&invalid_target).is_err());
    }

    #[test]
    fn validates_optional_expected_bus_guid_shape() {
        let mut value = valid_input();
        value["expectedBusGuid"] = json!("0123456789ABCDEF0123456789abcdef");
        assert!(LinuxMediaControlWorkerInput::parse(&value).is_err());
        value["expectedBusGuid"] = json!("0123456789abcdef0123456789abcde");
        assert!(LinuxMediaControlWorkerInput::parse(&value).is_err());
        value["expectedBusGuid"] = json!("0123456789abcdef0123456789abcdef");
        assert!(LinuxMediaControlWorkerInput::parse(&value).is_ok());
    }
}
