//! Linux 私有 MPRIS control worker 的 accepted→final JSONL 组合根。
//!
//! 该入口只在显式 `linux-mpris-candidate` feature 下构建。它不属于公开
//! capability；写入前必须先把 accepted 帧完整刷新到父进程。

use std::io::{Read, Write};

use serde_json::Value;

use crate::{
    adapters::linux::mpris::{Config, Failure},
    components::linux_media_control_worker_contract::{
        CONTRACT_VERSION, LinuxMediaControlWorkerInput,
    },
    components::linux_media_worker_contract_common::{MAXIMUM_INPUT_BYTES, MAXIMUM_OUTPUT_BYTES},
    domain::{AppControlError, error_json},
    modules::media_session,
};

/// 主 CLI 映像承载 control worker 的唯一隐藏 argv。
pub const HIDDEN_ARGUMENT: &str = "__linux-mpris-control-worker-v1";

/// 读取一个控制请求，在 mutation 前发布 accepted，并只再发布一个 final。
pub fn run_stdio() -> i32 {
    let stdout = std::io::stdout();
    let mut writer = FrameWriter::new(stdout.lock());
    let result = read_request().and_then(|value| run_request(value, &mut writer));
    write_terminal(&mut writer, result)
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

fn run_request<W: Write>(
    value: Value,
    writer: &mut FrameWriter<W>,
) -> Result<Value, AppControlError> {
    let input = LinuxMediaControlWorkerInput::parse(&value).map_err(protocol_error)?;
    debug_assert_eq!(input.protocol_version(), CONTRACT_VERSION);
    let config = Config {
        address: input.session_bus_address().to_owned(),
        broker_epoch: input.broker_epoch().to_owned(),
        expected_bus_guid: input.expected_bus_guid().map(ToOwned::to_owned),
        timeout_ms: input.timeout_ms(),
        maximum_items: input.maximum_items() as usize,
        // control worker 只消费真实 broker 代际，不伪造公开 target 指纹。
        fixture_public_id: None,
    };
    let accepted = serde_json::json!({
        "protocolVersion": CONTRACT_VERSION,
        "phase": "accepted",
        "targetId": input.target_id(),
        "operation": input.operation(),
    });
    media_session::control_with_acceptance(
        &config,
        input.target_id(),
        input.operation(),
        input.confirmed(),
        || {
            // 只有完整写入并 flush accepted 后，Adapter 才会调用 MPRIS method。
            writer.write_value(&accepted).map_err(|_| Failure::Protocol)
        },
    )
    .map_err(map_failure)
}

fn write_terminal<W: Write>(
    writer: &mut FrameWriter<W>,
    result: Result<Value, AppControlError>,
) -> i32 {
    let output = match result {
        Ok(value) => value,
        Err(error) => error_json(&error),
    };
    let succeeded = output
        .get("ok")
        .and_then(Value::as_bool)
        .is_some_and(|value| value);
    let fallback = serde_json::json!({
        "ok": false,
        "error": {
            "code": "WORKER_PROTOCOL_ERROR",
            "message": "The worker terminal result exceeded its bound.",
        }
    });
    let terminal = match serde_json::to_vec(&output) {
        Ok(bytes) if writer.can_write(bytes.len().saturating_add(1)) => (bytes, succeeded),
        _ => match serde_json::to_vec(&fallback) {
            Ok(bytes) if writer.can_write(bytes.len().saturating_add(1)) => (bytes, false),
            _ => return 3,
        },
    };
    if writer.write_serialized(&terminal.0).is_err() {
        return 3;
    }
    if terminal.1 { 0 } else { 2 }
}

struct FrameWriter<W> {
    inner: W,
    written: usize,
}

impl<W: Write> FrameWriter<W> {
    fn new(inner: W) -> Self {
        Self { inner, written: 0 }
    }

    fn can_write(&self, bytes: usize) -> bool {
        self.written.saturating_add(bytes) <= MAXIMUM_OUTPUT_BYTES
    }

    fn write_value(&mut self, value: &Value) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
        if !self.can_write(bytes.len().saturating_add(1)) {
            return Err(std::io::Error::other("worker output bound exceeded"));
        }
        self.write_serialized(&bytes)
    }

    fn write_serialized(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let frame_bytes = bytes.len().saturating_add(1);
        if !self.can_write(frame_bytes) {
            return Err(std::io::Error::other("worker output bound exceeded"));
        }
        self.inner.write_all(bytes)?;
        self.inner.write_all(b"\n")?;
        self.inner.flush()?;
        self.written = self.written.saturating_add(frame_bytes);
        Ok(())
    }
}

fn protocol_error(message: &'static str) -> AppControlError {
    AppControlError::with_details(
        "WORKER_PROTOCOL_ERROR",
        message,
        serde_json::json!({
            "platform": "linux",
            "provider": "mpris-v2",
            "providerState": "protocol-error",
            "executionRealm": "isolated-worker",
            "fallback": "none",
            "partialResultPublished": false,
            "targetMayHaveMutated": false,
            "automaticRetryProhibited": false,
        }),
    )
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
            "The MPRIS control exceeded its total deadline before accepted.",
            "timeout",
        ),
        Failure::Protocol => (
            "WORKER_PROTOCOL_ERROR",
            "The MPRIS control provider violated the worker protocol.",
            "protocol-error",
        ),
        Failure::Unavailable => (
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The private MPRIS control provider is unavailable.",
            "unavailable",
        ),
        Failure::ProviderError => (
            "PROCESS_SNAPSHOT_FAILED",
            "The MPRIS control provider failed before accepted.",
            "provider-error",
        ),
        Failure::OutcomeUnknown => (
            "OUTCOME_UNKNOWN",
            "The MPRIS control outcome is unknown and is not retryable.",
            "outcome-unknown",
        ),
    };
    let accepted = matches!(failure, Failure::OutcomeUnknown);
    AppControlError::with_details(
        code,
        message,
        serde_json::json!({
            "platform": "linux",
            "provider": "mpris-v2",
            "providerState": state,
            "executionRealm": "isolated-worker",
            "fallback": "none",
            "partialResultPublished": false,
            "accepted": accepted,
            "retrySafe": !accepted,
            "targetMayHaveMutated": accepted,
            "automaticRetryProhibited": accepted,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_writer_enforces_aggregate_output_bound() {
        // accepted 与 final 共用同一个 2 MiB 预算，不能分别各占满预算。
        let mut output = Vec::new();
        let mut writer = FrameWriter::new(&mut output);
        let accepted = serde_json::json!({"phase": "accepted"});
        assert!(writer.write_value(&accepted).is_ok());
        let oversized = serde_json::json!({"padding": "x".repeat(MAXIMUM_OUTPUT_BYTES)});
        assert!(writer.write_value(&oversized).is_err());
        assert!(output.len() < MAXIMUM_OUTPUT_BYTES);
    }

    #[test]
    fn worker_failure_safety_distinguishes_unknown_from_pre_dispatch() {
        let unknown = map_failure(Failure::OutcomeUnknown);
        assert_eq!(unknown.details["accepted"], true);
        assert_eq!(unknown.details["retrySafe"], false);
        assert_eq!(unknown.details["targetMayHaveMutated"], true);
        assert_eq!(unknown.details["automaticRetryProhibited"], true);

        for failure in [
            Failure::Unavailable,
            Failure::Timeout,
            Failure::Stale,
            Failure::BusEpochStale,
            Failure::Ambiguous,
            Failure::Protocol,
            Failure::ProviderError,
        ] {
            let error = map_failure(failure);
            assert_eq!(error.details["accepted"], false);
            assert_eq!(error.details["retrySafe"], true);
            assert_eq!(error.details["targetMayHaveMutated"], false);
            assert_eq!(error.details["automaticRetryProhibited"], false);
        }
    }
}
