//! Linux 私有 MPRIS observation self-worker 的有界 launcher。
//!
//! 生产路径只执行 `/proc/self/exe` 的固定隐藏 argv；显式映像参数仅供集成测试指向同一
//! 主 CLI 产物。不执行 PATH、同目录或 sibling 搜索，也不进入公开 App 路由。

use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, ExitStatus},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::{
    components::linux_media_worker_process,
    domain::{AppControlError, AppResult},
};

const MAXIMUM_INPUT_BYTES: usize = 64 * 1024;
const MAXIMUM_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAXIMUM_TIMEOUT: Duration = Duration::from_secs(35);
const MINIMUM_TIMEOUT: Duration = Duration::from_millis(1);

/// 以已知主 CLI 映像的固定隐藏入口运行 observation worker。
pub fn run_fixed_image_process(
    executable: &Path,
    request: &Value,
    timeout: Duration,
    cancelled: impl Fn() -> bool,
) -> AppResult<Value> {
    run_process(
        executable,
        crate::linux_media_observation_worker::HIDDEN_ARGUMENT,
        request,
        timeout,
        cancelled,
    )
}

/// 在当前生产映像中运行固定 observation self-worker，不搜索 sibling。
pub fn run_current_image_process(
    request: &Value,
    timeout: Duration,
    cancelled: impl Fn() -> bool,
) -> AppResult<Value> {
    run_fixed_image_process(Path::new("/proc/self/exe"), request, timeout, cancelled)
}

fn run_process(
    executable: &Path,
    hidden_argument: &'static str,
    request: &Value,
    timeout: Duration,
    cancelled: impl Fn() -> bool,
) -> AppResult<Value> {
    if !executable.is_absolute() || !(MINIMUM_TIMEOUT..=MAXIMUM_TIMEOUT).contains(&timeout) {
        return Err(protocol(
            "The fixture worker launch parameters are invalid.",
        ));
    }
    let request = serde_json::to_vec(request)
        .map_err(|_| protocol("The fixture worker request could not be serialized."))?;
    if request.is_empty() || request.len() > MAXIMUM_INPUT_BYTES {
        return Err(protocol("The fixture worker request exceeds its bound."));
    }
    let mut child = linux_media_worker_process::spawn(executable, hidden_argument)
        .map_err(|_| unavailable("The fixture worker could not be started."))?;
    let mut exit_notification = linux_media_worker_process::ExitNotification::new(&child);

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            linux_media_worker_process::kill_and_wait(&mut child);
            return Err(unavailable("The fixture worker stdout is unavailable."));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            linux_media_worker_process::kill_and_wait(&mut child);
            return Err(unavailable("The fixture worker stderr is unavailable."));
        }
    };
    let stdout_reader = thread::spawn(move || bounded_read(stdout));
    let stderr_reader = thread::spawn(move || bounded_read(stderr));
    let Some(stdin) = child.stdin.take() else {
        abort_and_join(&mut child, None, stdout_reader, stderr_reader);
        return Err(unavailable("The fixture worker stdin is unavailable."));
    };
    let mut writer = Some(thread::spawn(move || {
        let mut stdin = stdin;
        stdin
            .write_all(&request)
            .map_err(|_| unavailable("The fixture worker request could not be written."))
    }));

    let started = Instant::now();
    let terminal = loop {
        if cancelled() {
            abort_and_join(&mut child, writer.take(), stdout_reader, stderr_reader);
            return Err(cancelled_error());
        }
        if started.elapsed() >= timeout {
            abort_and_join(&mut child, writer.take(), stdout_reader, stderr_reader);
            return Err(timeout_error());
        }
        if let Some(handle) = take_finished_writer(&mut writer) {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    abort_and_join(&mut child, None, stdout_reader, stderr_reader);
                    return Err(error);
                }
                Err(_) => {
                    abort_and_join(&mut child, None, stdout_reader, stderr_reader);
                    return Err(protocol("The fixture worker writer failed."));
                }
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                exit_notification.wait_slice(timeout.saturating_sub(started.elapsed()));
            }
            Err(_) => {
                abort_and_join(&mut child, writer.take(), stdout_reader, stderr_reader);
                return Err(unavailable(
                    "The fixture worker state could not be observed.",
                ));
            }
        }
    };

    let stdout = join_reader(stdout_reader)?;
    let stderr = join_reader(stderr_reader)?;
    let status = terminal?;
    if let Some(handle) = writer {
        join_writer(handle)?;
    }
    if !stderr.is_empty() || stdout.is_empty() {
        return Err(protocol(
            "The fixture worker did not publish one clean terminal JSON result.",
        ));
    }
    let value: Value = serde_json::from_slice(&stdout)
        .map_err(|_| protocol("The fixture worker terminal result is invalid."))?;
    validate_terminal(value, status)
}

fn validate_terminal(value: Value, status: ExitStatus) -> AppResult<Value> {
    let Some(ok) = value.get("ok").and_then(Value::as_bool) else {
        return Err(protocol(
            "The fixture worker terminal result has no ok flag.",
        ));
    };
    if ok && status.success() {
        return Ok(value);
    }
    if ok || status.success() {
        return Err(protocol(
            "The fixture worker exit status and terminal result disagree.",
        ));
    }
    let Some(code) = value
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
    else {
        return Err(protocol("The fixture worker terminal error has no code."));
    };
    if !is_allowed_error_code(code) {
        return Err(protocol(
            "The fixture worker returned an unstable error code.",
        ));
    }
    Err(AppControlError::with_details(
        allowed_error_code(code),
        "The fixture worker returned a structured terminal failure.",
        details("failed"),
    ))
}

fn is_allowed_error_code(code: &str) -> bool {
    matches!(
        code,
        "STALE_SESSION"
            | "AMBIGUOUS_TARGET"
            | "TIMEOUT"
            | "WORKER_PROTOCOL_ERROR"
            | "BACKGROUND_OPERATION_UNAVAILABLE"
            | "PROCESS_SNAPSHOT_FAILED"
            | "OUTCOME_UNKNOWN"
    )
}

fn allowed_error_code(code: &str) -> &'static str {
    match code {
        "STALE_SESSION" => "STALE_SESSION",
        "AMBIGUOUS_TARGET" => "AMBIGUOUS_TARGET",
        "TIMEOUT" => "TIMEOUT",
        "WORKER_PROTOCOL_ERROR" => "WORKER_PROTOCOL_ERROR",
        "BACKGROUND_OPERATION_UNAVAILABLE" => "BACKGROUND_OPERATION_UNAVAILABLE",
        "PROCESS_SNAPSHOT_FAILED" => "PROCESS_SNAPSHOT_FAILED",
        "OUTCOME_UNKNOWN" => "OUTCOME_UNKNOWN",
        _ => "WORKER_PROTOCOL_ERROR",
    }
}

fn bounded_read(reader: impl Read) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((MAXIMUM_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| protocol("The fixture worker output could not be read."))?;
    if bytes.len() > MAXIMUM_OUTPUT_BYTES {
        return Err(protocol("The fixture worker output exceeded its bound."));
    }
    Ok(bytes)
}

fn join_reader(handle: JoinHandle<AppResult<Vec<u8>>>) -> AppResult<Vec<u8>> {
    handle
        .join()
        .map_err(|_| protocol("The fixture worker output reader failed."))?
}

fn join_writer(handle: JoinHandle<AppResult<()>>) -> AppResult<()> {
    handle
        .join()
        .map_err(|_| protocol("The fixture worker writer failed."))?
}

fn take_finished_writer(
    writer: &mut Option<JoinHandle<AppResult<()>>>,
) -> Option<JoinHandle<AppResult<()>>> {
    if writer.as_ref().is_some_and(JoinHandle::is_finished) {
        writer.take()
    } else {
        None
    }
}

fn abort_and_join(
    child: &mut Child,
    writer: Option<JoinHandle<AppResult<()>>>,
    stdout_reader: JoinHandle<AppResult<Vec<u8>>>,
    stderr_reader: JoinHandle<AppResult<Vec<u8>>>,
) {
    linux_media_worker_process::kill_and_wait(child);
    if let Some(handle) = writer {
        let _ = handle.join();
    }
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
}

fn details(state: &'static str) -> Value {
    serde_json::json!({
        "platform": "linux",
        "provider": "mpris-v2",
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
        "The bounded MPRIS observation was cancelled.",
        details("cancelled"),
    )
}

fn timeout_error() -> AppControlError {
    AppControlError::with_details(
        "TIMEOUT",
        "The bounded MPRIS observation worker timed out.",
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
