//! Linux 精确进程终止的严格输入契约；风险由显式 capability 路由固定。

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::domain::{AppControlError, AppResult};

const DEFAULT_TIMEOUT_MS: u32 = 5_000;
const MINIMUM_TIMEOUT_MS: u32 = 1;
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

/// 通过严格解析的总 deadline。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LinuxProcessTerminationInput {
    timeout: Duration,
}

impl LinuxProcessTerminationInput {
    pub(crate) const fn timeout(self) -> Duration {
        self.timeout
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireInput {
    #[serde(default = "default_timeout_ms", rename = "timeoutMs")]
    timeout_ms: u32,
}

const fn default_timeout_ms() -> u32 {
    DEFAULT_TIMEOUT_MS
}

/// 只接受可选总 deadline；风险与信号均由 capability 固定。
pub(crate) fn parse_graceful(value: &Value) -> AppResult<LinuxProcessTerminationInput> {
    parse(value, "graceful")
}

/// 强制路线复用相同 deadline 形状，但不允许 input 选择或降级风险。
pub(crate) fn parse_force(value: &Value) -> AppResult<LinuxProcessTerminationInput> {
    parse(value, "force")
}

fn parse(value: &Value, route: &'static str) -> AppResult<LinuxProcessTerminationInput> {
    if !value.is_object() {
        return Err(invalid_argument(route));
    }
    let wire: WireInput =
        serde_json::from_value(value.clone()).map_err(|_| invalid_argument(route))?;
    if !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&wire.timeout_ms) {
        return Err(invalid_argument(route));
    }
    Ok(LinuxProcessTerminationInput {
        timeout: Duration::from_millis(u64::from(wire.timeout_ms)),
    })
}

fn invalid_argument(route: &'static str) -> AppControlError {
    AppControlError::new(
        "INVALID_ARGUMENT",
        format!("Linux {route} process termination input accepts timeoutMs=1..30000 only."),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn timeout_is_bounded_and_native_fields_are_rejected() {
        let graceful_schema: Value = serde_json::from_str(include_str!(
            "../../contracts/v2/process-termination-graceful-input.schema.json"
        ))
        .unwrap_or_else(|error| panic!("输入 schema 应有效：{error}"));
        let force_schema: Value = serde_json::from_str(include_str!(
            "../../contracts/v2/process-termination-force-input.schema.json"
        ))
        .unwrap_or_else(|error| panic!("强制输入 schema 应有效：{error}"));
        let default =
            parse_graceful(&json!({})).unwrap_or_else(|error| panic!("默认输入应有效：{error:?}"));
        assert_eq!(
            default.timeout(),
            Duration::from_millis(DEFAULT_TIMEOUT_MS.into())
        );
        let minimum = parse_force(&json!({"timeoutMs": 1}))
            .unwrap_or_else(|error| panic!("最短 deadline 应有效：{error:?}"));
        assert_eq!(minimum.timeout(), Duration::from_millis(1));
        for schema in [&graceful_schema, &force_schema] {
            assert!(jsonschema::draft202012::validate(schema, &json!({})).is_ok());
            assert!(
                jsonschema::draft202012::validate(schema, &json!({"timeoutMs": 30_000})).is_ok()
            );
        }
        for value in [
            json!({"timeoutMs": 0}),
            json!({"timeoutMs": 30_001}),
            json!({"signal": 15}),
            json!({"processId": 42}),
            json!({"forceOnTimeout": true}),
        ] {
            for (parser, schema) in [
                (
                    parse_graceful as fn(&Value) -> AppResult<LinuxProcessTerminationInput>,
                    &graceful_schema,
                ),
                (parse_force, &force_schema),
            ] {
                let Err(error) = parser(&value) else {
                    panic!("越界或原生字段必须失败闭合");
                };
                assert_eq!(error.code, "INVALID_ARGUMENT");
                assert!(jsonschema::draft202012::validate(schema, &value).is_err());
            }
        }
    }
}
