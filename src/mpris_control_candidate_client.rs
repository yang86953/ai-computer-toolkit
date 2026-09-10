//! Linux 私有 MPRIS control self-worker 的两阶段 launcher。
//!
//! accepted 帧一旦出现，任何终态缺失、超时、取消或协议异常都必须保守映射为
//! `OUTCOME_UNKNOWN`，不能自动重试可能已经执行的媒体命令。生产只执行 `/proc/self/exe`
//! 的固定隐藏 argv；显式映像参数仅供集成测试指向同一主 CLI 产物。

use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, ExitStatus},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::{Map, Value};

use crate::{
    components::linux_media_control_worker_contract::CONTRACT_VERSION,
    components::linux_media_worker_contract_common::{MAXIMUM_INPUT_BYTES, MAXIMUM_OUTPUT_BYTES},
    components::linux_media_worker_process,
    domain::{AppControlError, AppResult},
};

const MAXIMUM_TIMEOUT: Duration = Duration::from_secs(35);
const MINIMUM_TIMEOUT: Duration = Duration::from_millis(1);

/// 以已知主 CLI 映像的固定隐藏入口运行 control worker。
pub fn run_fixed_image_process(
    executable: &Path,
    request: &Value,
    timeout: Duration,
    cancelled: impl Fn() -> bool,
) -> AppResult<Value> {
    run_process(
        executable,
        crate::linux_media_control_worker::HIDDEN_ARGUMENT,
        request,
        timeout,
        cancelled,
    )
}

/// 在当前生产映像中运行固定 control self-worker，不搜索 sibling。
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
            "The fixture control worker launch parameters are invalid.",
        ));
    }
    let expected = ExpectedRequest::from_value(request);
    let request = serde_json::to_vec(request)
        .map_err(|_| protocol("The fixture control request could not be serialized."))?;
    if request.is_empty() || request.len() > MAXIMUM_INPUT_BYTES {
        return Err(protocol("The fixture control request exceeds its bound."));
    }
    let mut child = linux_media_worker_process::spawn(executable, hidden_argument)
        .map_err(|_| unavailable("The fixture control worker could not be started."))?;
    let mut exit_notification = linux_media_worker_process::ExitNotification::new(&child);

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            linux_media_worker_process::kill_and_wait(&mut child);
            return Err(unavailable(
                "The fixture control worker stdout is unavailable.",
            ));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            linux_media_worker_process::kill_and_wait(&mut child);
            return Err(unavailable(
                "The fixture control worker stderr is unavailable.",
            ));
        }
    };
    let stdout_reader = thread::spawn(move || bounded_read(stdout));
    let stderr_reader = thread::spawn(move || bounded_read(stderr));
    let Some(stdin) = child.stdin.take() else {
        let collected = abort_and_collect(&mut child, None, stdout_reader, stderr_reader);
        drop(collected);
        return Err(unavailable(
            "The fixture control worker stdin is unavailable.",
        ));
    };
    let mut writer = Some(thread::spawn(move || {
        let mut stdin = stdin;
        stdin
            .write_all(&request)
            .map_err(|_| unavailable("The fixture control request could not be written."))
    }));

    let started = Instant::now();
    let status = loop {
        if cancelled() {
            return stop_process(
                &mut child,
                writer.take(),
                stdout_reader,
                stderr_reader,
                &expected,
                StopReason::Cancelled,
            );
        }
        if started.elapsed() >= timeout {
            return stop_process(
                &mut child,
                writer.take(),
                stdout_reader,
                stderr_reader,
                &expected,
                StopReason::Timeout,
            );
        }
        if let Some(handle) = take_finished_writer(&mut writer)
            && let Err(error) = join_writer(handle)
        {
            let collected = abort_and_collect(&mut child, None, stdout_reader, stderr_reader);
            return if accepted_frame_present(&collected.stdout) {
                Err(outcome_unknown("request-writer-failed-after-accepted"))
            } else {
                Err(error)
            };
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                exit_notification.wait_slice(timeout.saturating_sub(started.elapsed()));
            }
            Err(_) => {
                let collected =
                    abort_and_collect(&mut child, writer.take(), stdout_reader, stderr_reader);
                return classify_process_failure(
                    collected,
                    &expected,
                    "The fixture control worker state could not be observed.",
                );
            }
        }
    };

    let writer_result = match writer {
        Some(handle) => join_writer(handle),
        None => Ok(()),
    };
    let stdout = join_reader(stdout_reader);
    let stderr = join_reader(stderr_reader);
    evaluate_terminal(writer_result, stdout, stderr, status, &expected)
}

#[derive(Clone, Debug)]
struct ExpectedRequest {
    confirmed: bool,
    target_id: Option<String>,
    operation: Option<String>,
}

impl ExpectedRequest {
    fn from_value(value: &Value) -> Self {
        Self {
            confirmed: value
                .get("confirmed")
                .and_then(Value::as_bool)
                .is_some_and(|confirmed| confirmed),
            target_id: value
                .get("targetId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            operation: value
                .get("operation")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        }
    }
}

enum StopReason {
    Cancelled,
    Timeout,
}

struct CollectedOutput {
    stdout: AppResult<Vec<u8>>,
}

fn stop_process(
    child: &mut Child,
    writer: Option<JoinHandle<AppResult<()>>>,
    stdout_reader: JoinHandle<AppResult<Vec<u8>>>,
    stderr_reader: JoinHandle<AppResult<Vec<u8>>>,
    expected: &ExpectedRequest,
    reason: StopReason,
) -> AppResult<Value> {
    let collected = abort_and_collect(child, writer, stdout_reader, stderr_reader);
    if accepted_frame_present(&collected.stdout)
        || (expected.confirmed && collected.stdout.is_err())
    {
        return Err(outcome_unknown("interrupted-after-accepted"));
    }
    match reason {
        StopReason::Cancelled => Err(cancelled_error()),
        StopReason::Timeout => Err(timeout_error()),
    }
}

fn classify_process_failure(
    collected: CollectedOutput,
    expected: &ExpectedRequest,
    message: &'static str,
) -> AppResult<Value> {
    if accepted_frame_present(&collected.stdout)
        || (expected.confirmed && collected.stdout.is_err())
    {
        Err(outcome_unknown("worker-failed-after-accepted"))
    } else {
        Err(unavailable(message))
    }
}

fn evaluate_terminal(
    writer: AppResult<()>,
    stdout: AppResult<Vec<u8>>,
    stderr: AppResult<Vec<u8>>,
    status: ExitStatus,
    expected: &ExpectedRequest,
) -> AppResult<Value> {
    let stdout = match stdout {
        Ok(stdout) => stdout,
        Err(_) if expected.confirmed => return Err(outcome_unknown("output-lost")),
        Err(error) => return Err(error),
    };
    let accepted = accepted_frame_present(&Ok(stdout.clone()));
    if writer.is_err() {
        return if accepted {
            Err(outcome_unknown("request-writer-failed-after-accepted"))
        } else {
            Err(protocol("The fixture control request writer failed."))
        };
    }
    let stderr = match stderr {
        Ok(stderr) => stderr,
        Err(_) if accepted => return Err(outcome_unknown("stderr-lost-after-accepted")),
        Err(error) => return Err(error),
    };
    if !stderr.is_empty() || stdout.is_empty() {
        return if accepted || expected.confirmed {
            Err(outcome_unknown("unclean-terminal"))
        } else {
            Err(protocol(
                "The fixture control worker did not publish clean JSONL.",
            ))
        };
    }
    let frames = match parse_frames(&stdout) {
        Ok(frames) => frames,
        Err(_) if accepted || expected.confirmed => {
            return Err(outcome_unknown("invalid-terminal-after-confirmation"));
        }
        Err(error) => return Err(error),
    };
    match frames.as_slice() {
        [final_frame] => validate_pre_dispatch_final(final_frame.clone(), status, expected),
        [accepted_frame, final_frame]
            if accepted_matches_request(accepted_frame, expected) && expected.confirmed =>
        {
            validate_post_dispatch_final(final_frame.clone(), status, expected)
        }
        _ if accepted || expected.confirmed => Err(outcome_unknown("frame-sequence-uncertain")),
        _ => Err(protocol(
            "The fixture control worker frame sequence is invalid.",
        )),
    }
}

fn validate_pre_dispatch_final(
    value: Value,
    status: ExitStatus,
    expected: &ExpectedRequest,
) -> AppResult<Value> {
    let Some(ok) = value.get("ok").and_then(Value::as_bool) else {
        return Err(protocol("The fixture control final has no ok flag."));
    };
    if !ok {
        if status.success() {
            return Err(protocol(
                "The fixture control exit status and final error disagree.",
            ));
        }
        return structured_failure(&value);
    }
    if !status.success() || !matches_rejected_final(&value, expected) {
        return if expected.confirmed {
            Err(outcome_unknown("untrusted-pre-dispatch-final"))
        } else {
            Err(protocol(
                "The fixture control pre-dispatch final is inconsistent.",
            ))
        };
    }
    Ok(value)
}

fn validate_post_dispatch_final(
    value: Value,
    status: ExitStatus,
    expected: &ExpectedRequest,
) -> AppResult<Value> {
    if !status.success() || !matches_dispatched_final(&value, expected) {
        return Err(outcome_unknown("untrusted-post-dispatch-final"));
    }
    Ok(value)
}

fn matches_rejected_final(value: &Value, expected: &ExpectedRequest) -> bool {
    common_success_envelope(value)
        && final_data(value).is_some_and(|data| {
            exact_control_data(data)
                && matches_identity(data, expected)
                && boolean(data, "accepted") == Some(false)
                && boolean(data, "finalStateReached") == Some(false)
                && string(data, "dispatchOutcome") == Some("pre-dispatch-rejected")
                && boolean(data, "effectConfirmed") == Some(false)
                && boolean(data, "retrySafe") == Some(true)
                && boolean(data, "targetMayHaveMutated") == Some(false)
                && boolean(data, "automaticRetryProhibited") == Some(false)
        })
}

fn matches_dispatched_final(value: &Value, expected: &ExpectedRequest) -> bool {
    common_success_envelope(value)
        && final_data(value).is_some_and(|data| {
            let outcome = string(data, "dispatchOutcome");
            let final_state = boolean(data, "finalStateReached");
            let consistent_outcome = matches!(
                (outcome, final_state),
                (Some("replied" | "provider-error"), Some(true))
                    | (Some("outcome-unknown"), Some(false))
            );
            exact_control_data(data)
                && matches_identity(data, expected)
                && boolean(data, "accepted") == Some(true)
                && consistent_outcome
                && boolean(data, "effectConfirmed") == Some(false)
                && boolean(data, "retrySafe") == Some(false)
                && boolean(data, "targetMayHaveMutated") == Some(true)
                && boolean(data, "automaticRetryProhibited") == Some(true)
        })
}

fn common_success_envelope(value: &Value) -> bool {
    value.get("ok").and_then(Value::as_bool) == Some(true)
        && value.get("contractVersion").and_then(Value::as_str) == Some("act/control/v2")
        && value.get("capability").and_then(Value::as_str) == Some("media.playback.control@2")
}

fn final_data(value: &Value) -> Option<&Map<String, Value>> {
    value.get("data").and_then(Value::as_object)
}

fn exact_control_data(data: &Map<String, Value>) -> bool {
    const KEYS: [&str; 10] = [
        "sessionId",
        "targetKind",
        "operation",
        "accepted",
        "finalStateReached",
        "dispatchOutcome",
        "effectConfirmed",
        "retrySafe",
        "targetMayHaveMutated",
        "automaticRetryProhibited",
    ];
    data.len() == KEYS.len() && data.keys().all(|key| KEYS.contains(&key.as_str()))
}

fn matches_identity(data: &Map<String, Value>, expected: &ExpectedRequest) -> bool {
    string(data, "targetKind") == Some("media-session")
        && expected.target_id.as_deref() == string(data, "sessionId")
        && expected.operation.as_deref() == string(data, "operation")
}

fn string<'a>(data: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    data.get(key).and_then(Value::as_str)
}

fn boolean(data: &Map<String, Value>, key: &str) -> Option<bool> {
    data.get(key).and_then(Value::as_bool)
}

fn parse_frames(stdout: &[u8]) -> AppResult<Vec<Value>> {
    let text = std::str::from_utf8(stdout)
        .map_err(|_| protocol("The fixture control worker output is not UTF-8."))?;
    let Some(body) = text.strip_suffix('\n') else {
        return Err(protocol(
            "The fixture control worker did not terminate its final frame.",
        ));
    };
    if body.is_empty() || body.contains('\r') {
        return Err(protocol(
            "The fixture control worker output contains an invalid frame.",
        ));
    }
    let lines = body.split('\n').collect::<Vec<_>>();
    if !(1..=2).contains(&lines.len()) || lines.iter().any(|line| line.is_empty()) {
        return Err(protocol(
            "The fixture control worker published an invalid frame count.",
        ));
    }
    lines
        .into_iter()
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .map_err(|_| protocol("The fixture control worker frame is invalid JSON."))
        })
        .collect()
}

fn accepted_frame_present(stdout: &AppResult<Vec<u8>>) -> bool {
    let Ok(stdout) = stdout else {
        return false;
    };
    let Some(end) = stdout.iter().position(|byte| *byte == b'\n') else {
        return false;
    };
    // 即使版本或关联字段漂移，只要对端声明 accepted，就不能把后续中断降级为可重试。
    serde_json::from_slice::<Value>(&stdout[..end])
        .is_ok_and(|value| value.get("phase").and_then(Value::as_str) == Some("accepted"))
}

fn accepted_matches_request(value: &Value, expected: &ExpectedRequest) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    const KEYS: [&str; 4] = ["protocolVersion", "phase", "targetId", "operation"];
    object.len() == KEYS.len()
        && object.keys().all(|key| KEYS.contains(&key.as_str()))
        && value.get("protocolVersion").and_then(Value::as_str) == Some(CONTRACT_VERSION)
        && value.get("phase").and_then(Value::as_str) == Some("accepted")
        && expected.target_id.as_deref() == value.get("targetId").and_then(Value::as_str)
        && expected.operation.as_deref() == value.get("operation").and_then(Value::as_str)
}

fn structured_failure(value: &Value) -> AppResult<Value> {
    let Some(code) = value
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
    else {
        return Err(protocol("The fixture control final error has no code."));
    };
    if !is_allowed_error_code(code) {
        return Err(protocol(
            "The fixture control worker returned an unstable error code.",
        ));
    }
    Err(AppControlError::with_details(
        allowed_error_code(code),
        "The fixture control worker returned a structured pre-dispatch failure.",
        details("failed-before-accepted", false, false),
    ))
}

fn is_allowed_error_code(code: &str) -> bool {
    matches!(
        code,
        "WORKER_PROTOCOL_ERROR"
            | "BACKGROUND_OPERATION_UNAVAILABLE"
            | "STALE_SESSION"
            | "AMBIGUOUS_TARGET"
            | "TIMEOUT"
            | "PROCESS_SNAPSHOT_FAILED"
    )
}

fn allowed_error_code(code: &str) -> &'static str {
    match code {
        "BACKGROUND_OPERATION_UNAVAILABLE" => "BACKGROUND_OPERATION_UNAVAILABLE",
        "STALE_SESSION" => "STALE_SESSION",
        "AMBIGUOUS_TARGET" => "AMBIGUOUS_TARGET",
        "TIMEOUT" => "TIMEOUT",
        "PROCESS_SNAPSHOT_FAILED" => "PROCESS_SNAPSHOT_FAILED",
        _ => "WORKER_PROTOCOL_ERROR",
    }
}

fn bounded_read(reader: impl Read) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((MAXIMUM_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| protocol("The fixture control worker output could not be read."))?;
    if bytes.len() > MAXIMUM_OUTPUT_BYTES {
        return Err(protocol(
            "The fixture control worker output exceeded its bound.",
        ));
    }
    Ok(bytes)
}

fn join_reader(handle: JoinHandle<AppResult<Vec<u8>>>) -> AppResult<Vec<u8>> {
    handle
        .join()
        .map_err(|_| protocol("The fixture control worker output reader failed."))?
}

fn join_writer(handle: JoinHandle<AppResult<()>>) -> AppResult<()> {
    handle
        .join()
        .map_err(|_| protocol("The fixture control worker writer failed."))?
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

fn abort_and_collect(
    child: &mut Child,
    writer: Option<JoinHandle<AppResult<()>>>,
    stdout_reader: JoinHandle<AppResult<Vec<u8>>>,
    stderr_reader: JoinHandle<AppResult<Vec<u8>>>,
) -> CollectedOutput {
    linux_media_worker_process::kill_and_wait(child);
    if let Some(handle) = writer {
        let _ = handle.join();
    }
    let stdout = join_reader(stdout_reader);
    let _ = join_reader(stderr_reader);
    CollectedOutput { stdout }
}

fn details(state: &'static str, accepted: bool, target_may_have_mutated: bool) -> Value {
    serde_json::json!({
        "platform": "linux",
        "provider": "mpris-v2",
        "providerState": state,
        "executionRealm": "isolated-worker",
        "fallback": "none",
        "partialResultPublished": false,
        "workerReaped": true,
        "accepted": accepted,
        "retrySafe": !accepted,
        "targetMayHaveMutated": target_may_have_mutated,
        "automaticRetryProhibited": accepted,
    })
}

fn outcome_unknown(state: &'static str) -> AppControlError {
    AppControlError::with_details(
        "OUTCOME_UNKNOWN",
        "The MPRIS control worker crossed accepted but no trustworthy final result remained.",
        details(state, true, true),
    )
}

fn cancelled_error() -> AppControlError {
    AppControlError::with_details(
        "CANCELLED",
        "The MPRIS control worker was cancelled before accepted.",
        details("cancelled-before-accepted", false, false),
    )
}

fn timeout_error() -> AppControlError {
    AppControlError::with_details(
        "TIMEOUT",
        "The MPRIS control worker timed out before accepted.",
        details("timeout-before-accepted", false, false),
    )
}

fn unavailable(message: &'static str) -> AppControlError {
    AppControlError::with_details(
        "ISOLATED_WORKER_UNAVAILABLE",
        message,
        details("unavailable-before-accepted", false, false),
    )
}

fn protocol(message: &'static str) -> AppControlError {
    AppControlError::with_details(
        "WORKER_PROTOCOL_ERROR",
        message,
        details("protocol-error-before-accepted", false, false),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn expected_request(confirmed: bool) -> ExpectedRequest {
        ExpectedRequest {
            confirmed,
            target_id: Some("s2:m:0123456789abcdef".to_owned()),
            operation: Some("play".to_owned()),
        }
    }

    fn accepted_frame() -> Value {
        json!({
            "protocolVersion": CONTRACT_VERSION,
            "phase": "accepted",
            "targetId": "s2:m:0123456789abcdef",
            "operation": "play",
        })
    }

    fn rejected_final() -> Value {
        json!({
            "ok": true,
            "contractVersion": "act/control/v2",
            "capability": "media.playback.control@2",
            "data": {
                "sessionId": "s2:m:0123456789abcdef",
                "targetKind": "media-session",
                "operation": "play",
                "accepted": false,
                "finalStateReached": false,
                "dispatchOutcome": "pre-dispatch-rejected",
                "effectConfirmed": false,
                "retrySafe": true,
                "targetMayHaveMutated": false,
                "automaticRetryProhibited": false,
            }
        })
    }

    fn dispatched_final(outcome: &str, final_state_reached: bool) -> Value {
        json!({
            "ok": true,
            "contractVersion": "act/control/v2",
            "capability": "media.playback.control@2",
            "data": {
                "sessionId": "s2:m:0123456789abcdef",
                "targetKind": "media-session",
                "operation": "play",
                "accepted": true,
                "finalStateReached": final_state_reached,
                "dispatchOutcome": outcome,
                "effectConfirmed": false,
                "retrySafe": false,
                "targetMayHaveMutated": true,
                "automaticRetryProhibited": true,
            }
        })
    }

    #[test]
    fn parse_frames_requires_one_or_two_json_lines_with_terminal_newline() {
        let Ok(one) = parse_frames(
            br#"{"ok":true}
"#,
        ) else {
            panic!("一个合法 JSONL frame 必须解析");
        };
        assert_eq!(one.len(), 1);

        let Ok(two) = parse_frames(
            br#"{"phase":"accepted"}
{"ok":true}
"#,
        ) else {
            panic!("两个合法 JSONL frame 必须解析");
        };
        assert_eq!(two.len(), 2);

        for invalid in [
            br#"{"ok":true}"#.as_slice(),
            b"",
            br#"{"ok":true}

"#,
            br#"{}
{}
{}
"#,
            b"{}\r\n",
            br#"{"#,
        ] {
            assert!(parse_frames(invalid).is_err());
        }
    }

    #[test]
    fn accepted_matching_requires_exact_frame_and_request_identity() {
        let expected = expected_request(true);
        let valid = accepted_frame();
        assert!(accepted_matches_request(&valid, &expected));

        for (field, replacement) in [
            ("protocolVersion", json!("act/internal/other/v1")),
            ("phase", json!("ready")),
            ("targetId", json!("s2:m:fedcba9876543210")),
            ("operation", json!("pause")),
        ] {
            let mut changed = valid.clone();
            changed[field] = replacement;
            assert!(!accepted_matches_request(&changed, &expected));
        }

        let mut extra = valid.clone();
        extra["extra"] = json!(false);
        assert!(!accepted_matches_request(&extra, &expected));

        let mut missing = valid;
        if let Some(object) = missing.as_object_mut() {
            object.remove("operation");
        }
        assert!(!accepted_matches_request(&missing, &expected));
    }

    #[test]
    fn accepted_frame_presence_is_conservative_about_version_drift() {
        let drifted = json!({
            "protocolVersion": "act/internal/other/v1",
            "phase": "accepted",
            "targetId": "not-replayed",
            "operation": "unknown",
        });
        let Ok(serialized) = serde_json::to_vec(&drifted) else {
            panic!("accepted fixture 必须可序列化");
        };
        let mut terminated = serialized;
        terminated.push(b'\n');
        assert!(accepted_frame_present(&Ok(terminated)));

        let mut non_accepted = drifted;
        non_accepted["phase"] = json!("final");
        let Ok(serialized) = serde_json::to_vec(&non_accepted) else {
            panic!("非 accepted fixture 必须可序列化");
        };
        let mut terminated = serialized;
        terminated.push(b'\n');
        assert!(!accepted_frame_present(&Ok(terminated)));

        assert!(!accepted_frame_present(&Ok(b"{}".to_vec())));
    }

    #[test]
    fn rejected_final_requires_pre_dispatch_safe_flags() {
        let expected = expected_request(false);
        let valid = rejected_final();
        assert!(matches_rejected_final(&valid, &expected));

        for (field, replacement) in [
            ("accepted", json!(true)),
            ("finalStateReached", json!(true)),
            ("dispatchOutcome", json!("replied")),
            ("retrySafe", json!(false)),
            ("targetMayHaveMutated", json!(true)),
            ("automaticRetryProhibited", json!(true)),
        ] {
            let mut changed = valid.clone();
            changed["data"][field] = replacement;
            assert!(!matches_rejected_final(&changed, &expected));
        }

        let mut extra = valid;
        extra["data"]["providerIdentity"] = Value::String("secret".to_owned());
        assert!(!matches_rejected_final(&extra, &expected));
    }

    #[test]
    fn dispatched_final_accepts_replied_provider_error_and_unknown_shapes() {
        let expected = expected_request(true);
        for (outcome, final_state_reached) in [
            ("replied", true),
            ("provider-error", true),
            ("outcome-unknown", false),
        ] {
            let value = dispatched_final(outcome, final_state_reached);
            assert!(matches_dispatched_final(&value, &expected));
        }

        for (field, replacement) in [
            ("retrySafe", json!(true)),
            ("targetMayHaveMutated", json!(false)),
            ("automaticRetryProhibited", json!(false)),
            ("accepted", json!(false)),
        ] {
            let mut changed = dispatched_final("replied", true);
            changed["data"][field] = replacement;
            assert!(!matches_dispatched_final(&changed, &expected));
        }

        assert!(!matches_dispatched_final(
            &dispatched_final("outcome-unknown", true),
            &expected
        ));
        assert!(!matches_dispatched_final(
            &dispatched_final("provider-error", false),
            &expected
        ));
    }
}
