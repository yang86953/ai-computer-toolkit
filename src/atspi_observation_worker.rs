//! 非默认 Linux AT-SPI 候选 worker 的一次性 JSON stdio 组合根。

use std::io::{Read, Write};

use serde::Deserialize;
use serde_json::Value;

use crate::{
    adapters::linux::atspi::ConnectionConfig,
    domain::{AppControlError, error_json},
    modules::{accessibility, window_observation},
};

const MAXIMUM_REQUEST_BYTES: u64 = 65_536;

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Operation {
    Discover,
    Metadata,
    Tree,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkerRequest {
    protocol_version: String,
    operation: Operation,
    session_bus_address: String,
    expected_accessibility_bus_address: String,
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default = "default_maximum_items")]
    maximum_items: usize,
    #[serde(default)]
    maximum_depth: u8,
    timeout_ms: u32,
}

const fn default_maximum_items() -> usize {
    256
}

/// 读取一个请求、计算完整终态并只写一次 JSON；不产生诊断日志。
pub fn run_stdio() -> i32 {
    let result = read_request().and_then(run_request);
    let output = match result {
        Ok(value) => value,
        Err(error) => error_json(&error),
    };
    let serialized = match serde_json::to_vec(&output) {
        Ok(value) => value,
        Err(_) => br#"{"ok":false,"error":{"code":"SERIALIZATION_FAILED","message":"The worker result could not be serialized."}}"#.to_vec(),
    };
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(&serialized)
        .and_then(|_| stdout.write_all(b"\n"))
        .is_err()
    {
        return 3;
    }
    if output["ok"] == true { 0 } else { 2 }
}

fn read_request() -> Result<WorkerRequest, AppControlError> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAXIMUM_REQUEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid("The worker request could not be read."))?;
    if bytes.is_empty() || bytes.len() as u64 > MAXIMUM_REQUEST_BYTES {
        return Err(invalid("The worker request size is invalid."));
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid("The worker request is not valid JSON."))
}

fn run_request(request: WorkerRequest) -> Result<Value, AppControlError> {
    if request.protocol_version != "act/internal/linux-atspi-observation/v1"
        || !(1..=4096).contains(&request.maximum_items)
        || request.maximum_depth > 20
        || !(1..=30_000).contains(&request.timeout_ms)
        || !explicit_private_address(&request.session_bus_address)
        || !explicit_private_address(&request.expected_accessibility_bus_address)
    {
        return Err(invalid(
            "The worker request violates the private protocol contract.",
        ));
    }
    let config = ConnectionConfig {
        session_bus_address: request.session_bus_address,
        expected_accessibility_bus_address: request.expected_accessibility_bus_address,
        timeout_ms: request.timeout_ms,
    };
    async_io::block_on(async move {
        match request.operation {
            Operation::Discover => {
                window_observation::discover(&config, request.maximum_items).await
            }
            Operation::Metadata => {
                let target = required_target(request.target_id.as_deref())?;
                window_observation::metadata(&config, target, request.maximum_items).await
            }
            Operation::Tree => {
                let target = required_target(request.target_id.as_deref())?;
                accessibility::read_tree(
                    &config,
                    target,
                    request.maximum_depth,
                    request.maximum_items,
                )
                .await
            }
        }
    })
}

fn explicit_private_address(value: &str) -> bool {
    value.starts_with("unix:path=") && value.len() <= 4096 && !value.contains(['\n', '\r', '\0'])
}

fn required_target(value: Option<&str>) -> Result<&str, AppControlError> {
    let target = value.ok_or_else(|| invalid("The worker request requires an opaque target."))?;
    if target.len() != 21 || !target.starts_with("s2:w:") {
        return Err(invalid(
            "The worker target is not a canonical window identity.",
        ));
    }
    Ok(target)
}

fn invalid(message: &'static str) -> AppControlError {
    AppControlError::with_details(
        "WORKER_PROTOCOL_ERROR",
        message,
        serde_json::json!({
            "platform": "linux",
            "provider": "at-spi2",
            "executionRealm": "isolated-worker",
            "fallback": "none",
            "partialResultPublished": false,
        }),
    )
}
