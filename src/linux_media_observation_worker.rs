//! Linux 私有 MPRIS observation worker 的一次性 JSON stdio 组合根。
//!
//! 该入口只在显式 `linux-mpris-candidate` feature 下构建。它不属于公开
//! capability，只执行一次受限的只读观察。

use std::io::{Read, Write};

use serde_json::Value;

use crate::{
    adapters::linux::mpris::{Config, Failure},
    components::linux_media_observation_worker_contract::{
        CONTRACT_VERSION, LinuxMediaObservationOperation, LinuxMediaObservationWorkerInput,
        MAXIMUM_INPUT_BYTES, MAXIMUM_OUTPUT_BYTES,
    },
    domain::{AppControlError, error_json},
    modules::media_session,
};

/// 主 CLI 映像承载 observation worker 的唯一隐藏 argv。
pub const HIDDEN_ARGUMENT: &str = "__linux-mpris-observation-worker-v1";

/// 读取一个请求、执行一次只读观察并只发布一个终态 JSON。
pub fn run_stdio() -> i32 {
    let result = read_request().and_then(run_request);
    let output = match result {
        Ok(value) => value,
        Err(error) => error_json(&error),
    };
    let (mut serialized, succeeded) = serialize_output(&output);
    serialized.push(b'\n');
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(&serialized).is_err() {
        return 3;
    }
    if succeeded { 0 } else { 2 }
}

fn read_request() -> Result<Value, AppControlError> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take((MAXIMUM_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| protocol_error("The worker request could not be read."))?;
    if bytes.is_empty() || bytes.len() > MAXIMUM_INPUT_BYTES {
        return Err(protocol_error("The worker request size is invalid."));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| protocol_error("The worker request is not valid JSON."))
}

fn run_request(value: Value) -> Result<Value, AppControlError> {
    let input = LinuxMediaObservationWorkerInput::parse(&value).map_err(protocol_error)?;
    debug_assert_eq!(input.protocol_version(), CONTRACT_VERSION);
    let operation = input.operation();
    debug_assert!(matches!(operation.as_str(), "discover" | "state"));
    let config = Config {
        address: input.session_bus_address().to_owned(),
        broker_epoch: input.broker_epoch().to_owned(),
        expected_bus_guid: input.expected_bus_guid().map(ToOwned::to_owned),
        timeout_ms: input.timeout_ms(),
        maximum_items: input.maximum_items() as usize,
        // worker 只消费真实 broker 代际，不伪造公开 target 指纹。
        fixture_public_id: None,
    };
    match operation {
        LinuxMediaObservationOperation::Discover => {
            media_session::discover(&config).map_err(map_failure)
        }
        LinuxMediaObservationOperation::State => {
            let Some(target_id) = input.target_id() else {
                return Err(protocol_error("The state operation requires a target."));
            };
            media_session::state(&config, target_id).map_err(map_failure)
        }
    }
}

fn serialize_output(output: &Value) -> (Vec<u8>, bool) {
    let succeeded = output
        .get("ok")
        .and_then(Value::as_bool)
        .is_some_and(|value| value);
    match serde_json::to_vec(output) {
        Ok(bytes) if bytes.len().saturating_add(1) <= MAXIMUM_OUTPUT_BYTES => (bytes, succeeded),
        _ => (
            br#"{"ok":false,"error":{"code":"WORKER_PROTOCOL_ERROR","message":"The worker terminal result exceeded its bound."}}"#.to_vec(),
            false,
        ),
    }
}

fn map_failure(failure: Failure) -> AppControlError {
    let (code, message, state) = match failure {
        Failure::Stale | Failure::BusEpochStale => (
            "STALE_SESSION",
            "The MPRIS target is stale for this broker generation.",
            "stale",
        ),
        Failure::Ambiguous => (
            "AMBIGUOUS_TARGET",
            "The MPRIS target is not unique for this broker generation.",
            "ambiguous",
        ),
        Failure::Timeout => (
            "TIMEOUT",
            "The MPRIS observation exceeded its total deadline.",
            "timeout",
        ),
        Failure::Protocol => (
            "WORKER_PROTOCOL_ERROR",
            "The MPRIS provider violated the worker protocol.",
            "protocol-error",
        ),
        Failure::Unavailable => (
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The private MPRIS observation provider is unavailable.",
            "unavailable",
        ),
        Failure::ProviderError => (
            "PROCESS_SNAPSHOT_FAILED",
            "The MPRIS observation provider failed to produce a snapshot.",
            "provider-error",
        ),
        Failure::OutcomeUnknown => (
            "OUTCOME_UNKNOWN",
            "The MPRIS observation outcome is unknown and is not retryable.",
            "outcome-unknown",
        ),
    };
    AppControlError::with_details(code, message, details(state))
}

fn protocol_error(message: &'static str) -> AppControlError {
    AppControlError::with_details("WORKER_PROTOCOL_ERROR", message, details("protocol-error"))
}

fn details(state: &'static str) -> Value {
    serde_json::json!({
        "platform": "linux",
        "provider": "mpris-v2",
        "providerState": state,
        "executionRealm": "isolated-worker",
        "fallback": "none",
        "partialResultPublished": false,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn oversized_success_result_is_replaced_by_protocol_error() {
        // 仅构造内存 fixture，证明序列化边界不会发布超限成功结果。
        let output = json!({
            "ok": true,
            "data": { "padding": "x".repeat(MAXIMUM_OUTPUT_BYTES) },
        });
        let (serialized, succeeded) = serialize_output(&output);
        assert!(!succeeded);
        let Ok(fallback) = serde_json::from_slice::<Value>(&serialized) else {
            panic!("超限结果必须降级为合法 JSON");
        };
        assert_eq!(fallback["ok"], false);
        assert_eq!(fallback["error"]["code"], "WORKER_PROTOCOL_ERROR");
    }

    #[test]
    fn bounded_success_result_preserves_success_flag_and_payload() {
        // 小型成功 fixture 必须保留 ok=true 与原始 provider-neutral payload。
        let output = json!({
            "ok": true,
            "data": { "state": "complete" },
        });
        let (serialized, succeeded) = serialize_output(&output);
        assert!(succeeded);
        let Ok(round_tripped) = serde_json::from_slice::<Value>(&serialized) else {
            panic!("正常结果必须保持合法 JSON");
        };
        assert_eq!(round_tripped, output);
    }
}
