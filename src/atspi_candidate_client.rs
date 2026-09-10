//! AT-SPI 私有 fixture worker 的有界 launcher；生产路由不得调用。

use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::domain::{AppControlError, AppResult};

const MAXIMUM_OUTPUT_BYTES: u64 = 2 * 1024 * 1024;

/// 仅供 hermetic fixture 显式注入已构建 worker；不执行 PATH 或 sibling 搜索。
pub fn run_fixture_process(
    executable: &Path,
    request: &Value,
    timeout: Duration,
    cancelled: impl Fn() -> bool,
) -> AppResult<Value> {
    if !executable.is_absolute() || timeout.is_zero() || timeout > Duration::from_secs(35) {
        return Err(protocol(
            "The fixture worker launch parameters are invalid.",
        ));
    }
    let mut child = Command::new(executable)
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/fixture-poison-session",
        )
        .env(
            "AT_SPI_BUS_ADDRESS",
            "unix:path=/fixture-poison-accessibility",
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| unavailable("The fixture worker could not be started."))?;
    let request = serde_json::to_vec(request)
        .map_err(|_| protocol("The fixture worker request could not be serialized."))?;
    child
        .stdin
        .take()
        .ok_or_else(|| unavailable("The fixture worker stdin is unavailable."))?
        .write_all(&request)
        .map_err(|_| unavailable("The fixture worker request could not be written."))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| unavailable("The fixture worker stdout is unavailable."))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| unavailable("The fixture worker stderr is unavailable."))?;
    let stdout_reader = thread::spawn(move || bounded_read(stdout));
    let stderr_reader = thread::spawn(move || bounded_read(stderr));
    let started = Instant::now();
    let terminal = loop {
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            break Err(cancelled_error());
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            break Err(timeout_error());
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(2)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(unavailable(
                    "The fixture worker state could not be observed.",
                ));
            }
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| protocol("The fixture worker stdout reader failed."))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| protocol("The fixture worker stderr reader failed."))??;
    let status = terminal?;
    if !stderr.is_empty() || stdout.is_empty() {
        return Err(protocol(
            "The fixture worker did not publish one clean terminal JSON result.",
        ));
    }
    let value: Value = serde_json::from_slice(&stdout)
        .map_err(|_| protocol("The fixture worker terminal result is invalid."))?;
    if value["ok"] == true && status.success() {
        return Ok(value);
    }
    if value["ok"] == false && !status.success() {
        let code = match value["error"]["code"].as_str() {
            Some("ACCESSIBILITY_UNAVAILABLE") => "ACCESSIBILITY_UNAVAILABLE",
            Some("AMBIGUOUS_TARGET") => "AMBIGUOUS_TARGET",
            Some("CANCELLED") => "CANCELLED",
            Some("PERMISSION_DENIED") => "PERMISSION_DENIED",
            Some("STALE_SESSION") => "STALE_SESSION",
            Some("TIMEOUT") => "TIMEOUT",
            Some("WORKER_PROTOCOL_ERROR") => "WORKER_PROTOCOL_ERROR",
            _ => "WORKER_PROTOCOL_ERROR",
        };
        return Err(AppControlError::with_details(
            code,
            "The fixture worker returned a structured terminal failure.",
            details(if code == "CANCELLED" {
                "cancelled"
            } else {
                "failed"
            }),
        ));
    }
    Err(protocol(
        "The fixture worker exit status and terminal result disagree.",
    ))
}

fn bounded_read(reader: impl Read) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(MAXIMUM_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| protocol("The fixture worker output could not be read."))?;
    if bytes.len() as u64 > MAXIMUM_OUTPUT_BYTES {
        return Err(protocol("The fixture worker output exceeded its bound."));
    }
    Ok(bytes)
}

fn details(state: &str) -> Value {
    serde_json::json!({
        "platform": "linux",
        "provider": "at-spi2",
        "providerState": state,
        "executionRealm": "isolated-worker",
        "fallback": "none",
        "partialResultPublished": false,
        "workerReaped": true,
    })
}

fn cancelled_error() -> AppControlError {
    AppControlError::with_details(
        "CANCELLED",
        "The bounded accessibility observation was cancelled.",
        details("cancelled"),
    )
}

fn timeout_error() -> AppControlError {
    AppControlError::with_details(
        "TIMEOUT",
        "The bounded accessibility observation worker timed out.",
        details("timeout"),
    )
}

fn unavailable(message: &'static str) -> AppControlError {
    AppControlError::with_details(
        "ISOLATED_WORKER_UNAVAILABLE",
        message,
        details("unavailable"),
    )
}

fn protocol(message: &'static str) -> AppControlError {
    AppControlError::with_details("WORKER_PROTOCOL_ERROR", message, details("protocol-error"))
}
