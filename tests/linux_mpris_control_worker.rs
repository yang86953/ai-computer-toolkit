#![cfg(all(target_os = "linux", feature = "linux-mpris-candidate"))]
#![recursion_limit = "256"]

//! Linux MPRIS control worker 的私有总线、accepted 与终态边界集成契约。

#[allow(dead_code)]
#[path = "support/mpris_private_bus.rs"]
mod mpris_private_bus;

use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ai_computer_toolkit::{
    AppControlError, AppResult, mpris_candidate, mpris_control_candidate_client,
};
use jsonschema::draft202012;
use mpris_private_bus::Fixture;
use serde_json::{Value, json};

const TOOLKIT: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit");
const BROKER_EPOCH: &str = "control-integration-epoch";
const INPUT_SCHEMA: &str = include_str!("../contracts/internal/linux-mpris-control-v1.schema.json");
const RESULT_SCHEMA: &str = include_str!("../contracts/v2/media-playback-control.schema.json");
const FIXTURE_TARGET: &str = "s2:m:0000000000000000";

fn validate(schema_text: &str, value: &Value) -> Result<(), Box<dyn Error>> {
    let schema = serde_json::from_str::<Value>(schema_text)?;
    draft202012::validate(&schema, value).map_err(|error| error.to_string())?;
    Ok(())
}

fn control_request(
    address: &str,
    target: &str,
    operation: &str,
    confirmed: bool,
    timeout_ms: u32,
) -> Value {
    json!({
        "protocolVersion": "act/internal/linux-mpris-control/v1",
        "sessionBusAddress": address,
        "brokerEpoch": BROKER_EPOCH,
        "targetId": target,
        "operation": operation,
        "confirmed": confirmed,
        "timeoutMs": timeout_ms,
    })
}

// 从私有 fixture 地址提取真实 broker GUID，避免测试依赖固定或伪造身份值。
fn fixture_bus_guid(address: &str) -> Result<String, Box<dyn Error>> {
    let guid = address
        .split(',')
        .find_map(|part| part.strip_prefix("guid="))
        .ok_or_else(|| io::Error::other("private fixture address has no D-Bus GUID"))?;
    if guid.len() != 32
        || !guid
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(io::Error::other("private fixture address has an invalid D-Bus GUID").into());
    }
    Ok(guid.to_owned())
}

// 确定性翻转首个十六进制位，生成同长度、同格式但必然错配的 GUID。
fn mismatched_bus_guid(address: &str) -> Result<String, Box<dyn Error>> {
    let mut guid = fixture_bus_guid(address)?.into_bytes();
    guid[0] = if guid[0] == b'0' { b'1' } else { b'0' };
    String::from_utf8(guid).map_err(|error| io::Error::other(error.to_string()).into())
}

fn run_worker(
    request: &Value,
    timeout: Duration,
    cancelled: impl Fn() -> bool,
) -> AppResult<Value> {
    mpris_control_candidate_client::run_fixed_image_process(
        Path::new(TOOLKIT),
        request,
        timeout,
        cancelled,
    )
}

fn discovered_target(fixture: &Fixture) -> Result<String, Box<dyn Error>> {
    let discovery = mpris_candidate::discover_private(&fixture.bus.address, BROKER_EPOCH, 3000)
        .map_err(io::Error::other)?;
    discovery["data"]["sessions"]
        .as_array()
        .and_then(|sessions| sessions.first())
        .and_then(|session| session["sessionId"].as_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::other("private fixture returned no media target").into())
}

async fn delayed_target(fixture: &Fixture) -> Result<String, Box<dyn Error>> {
    let discovery = mpris_candidate::discover_private(&fixture.bus.address, BROKER_EPOCH, 3000)
        .map_err(io::Error::other)?;
    let Some(sessions) = discovery["data"]["sessions"].as_array() else {
        return Err(io::Error::other("private fixture returned no session list").into());
    };
    for session in sessions {
        let Some(target) = session["sessionId"].as_str() else {
            continue;
        };
        let state =
            mpris_candidate::state_private(&fixture.bus.address, BROKER_EPOCH, target, 3000)
                .map_err(io::Error::other)?;
        if state["data"]["availableControls"]["skipPrevious"] == true {
            return Ok(target.to_owned());
        }
    }
    Err(io::Error::other("delayed private fixture target was not found").into())
}

fn assert_worker_error(
    error: &AppControlError,
    code: &str,
    accepted: bool,
    retry_safe: bool,
    target_may_have_mutated: bool,
) {
    assert_eq!(error.code, code);
    assert_eq!(error.details["workerReaped"], true);
    assert_eq!(error.details["accepted"], accepted);
    assert_eq!(error.details["retrySafe"], retry_safe);
    assert_eq!(
        error.details["targetMayHaveMutated"],
        target_may_have_mutated
    );
    assert_eq!(
        error.details["automaticRetryProhibited"], accepted,
        "accepted control failures must prohibit automatic retry"
    );
}

#[test]
fn unconfirmed_control_returns_pre_dispatch_rejection_without_provider_io()
-> Result<(), Box<dyn Error>> {
    let request = control_request(
        "unix:path=/fixture-poison-control",
        FIXTURE_TARGET,
        "play",
        false,
        3000,
    );
    validate(INPUT_SCHEMA, &request)?;
    let result = run_worker(&request, Duration::from_secs(3), || false)?;
    validate(RESULT_SCHEMA, &result)?;
    assert_eq!(result["data"]["accepted"], false);
    assert_eq!(result["data"]["finalStateReached"], false);
    assert_eq!(result["data"]["dispatchOutcome"], "pre-dispatch-rejected");
    assert_eq!(result["data"]["retrySafe"], true);
    assert_eq!(result["data"]["targetMayHaveMutated"], false);
    assert_eq!(result["data"]["automaticRetryProhibited"], false);
    Ok(())
}

#[test]
fn confirmed_play_on_private_fixture_publishes_replied_without_effect_claim()
-> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let target = discovered_target(&fixture)?;
        let request = control_request(&fixture.bus.address, &target, "play", true, 3000);
        validate(INPUT_SCHEMA, &request)?;
        let result = run_worker(&request, Duration::from_secs(3), || false)?;
        validate(RESULT_SCHEMA, &result)?;
        assert_eq!(result["data"]["sessionId"], target);
        assert_eq!(result["data"]["operation"], "play");
        assert_eq!(result["data"]["accepted"], true);
        assert_eq!(result["data"]["finalStateReached"], true);
        assert_eq!(result["data"]["dispatchOutcome"], "replied");
        assert_eq!(result["data"]["effectConfirmed"], false);
        assert_eq!(result["data"]["retrySafe"], false);
        assert_eq!(result["data"]["targetMayHaveMutated"], true);
        assert_eq!(result["data"]["automaticRetryProhibited"], true);
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 1);
        Ok::<(), Box<dyn Error>>(())
    })
}

#[test]
fn mismatched_expected_bus_guid_is_rejected_before_accepted_and_provider_io()
-> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let mismatch = mismatched_bus_guid(&fixture.bus.address)?;
        let mut request = control_request(&fixture.bus.address, FIXTURE_TARGET, "play", true, 3000);
        request["expectedBusGuid"] = json!(mismatch);
        validate(INPUT_SCHEMA, &request)?;

        let error = match run_worker(&request, Duration::from_secs(3), || false) {
            Ok(_) => {
                return Err(io::Error::other(
                    "mismatched expected bus GUID unexpectedly succeeded",
                )
                .into());
            }
            Err(error) => error,
        };
        assert_worker_error(&error, "STALE_SESSION", false, true, false);
        let serialized = serde_json::json!({
            "code": error.code,
            "message": error.message,
            "details": error.details,
        })
        .to_string();
        assert!(!serialized.contains(&fixture.bus.address));
        assert!(!serialized.contains(&mismatch));
        assert!(!serialized.contains("expectedBusGuid"));
        assert_eq!(fixture.metrics.properties.load(Ordering::Relaxed), 0);
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 0);
        Ok::<(), Box<dyn Error>>(())
    })
}

#[test]
fn unavailable_skip_previous_is_rejected_before_accepted_and_method_call()
-> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let target = discovered_target(&fixture)?;
        let request = control_request(&fixture.bus.address, &target, "skip-previous", true, 3000);
        validate(INPUT_SCHEMA, &request)?;
        let result = run_worker(&request, Duration::from_secs(3), || false)?;
        validate(RESULT_SCHEMA, &result)?;
        assert_eq!(result["data"]["accepted"], false);
        assert_eq!(result["data"]["finalStateReached"], false);
        assert_eq!(result["data"]["dispatchOutcome"], "pre-dispatch-rejected");
        assert_eq!(result["data"]["retrySafe"], true);
        assert_eq!(result["data"]["targetMayHaveMutated"], false);
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 0);
        Ok::<(), Box<dyn Error>>(())
    })
}

#[test]
fn delayed_method_times_out_inside_worker_after_accepted() -> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let mut fixture = Fixture::new().await?;
        fixture
            .add_player("org.mpris.MediaPlayer2.delayed", "Playing", true, 800)
            .await?;
        let target = delayed_target(&fixture).await?;
        let request = control_request(&fixture.bus.address, &target, "play", true, 300);
        validate(INPUT_SCHEMA, &request)?;
        let result = run_worker(&request, Duration::from_secs(3), || false)?;
        validate(RESULT_SCHEMA, &result)?;
        assert_eq!(result["data"]["accepted"], true);
        assert_eq!(result["data"]["finalStateReached"], false);
        assert_eq!(result["data"]["dispatchOutcome"], "outcome-unknown");
        assert_eq!(result["data"]["effectConfirmed"], false);
        assert_eq!(result["data"]["retrySafe"], false);
        assert_eq!(result["data"]["targetMayHaveMutated"], true);
        assert_eq!(result["data"]["automaticRetryProhibited"], true);
        Ok::<(), Box<dyn Error>>(())
    })
}

#[test]
fn outer_timeout_and_cancel_after_accepted_are_outcome_unknown() -> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let mut fixture = Fixture::new().await?;
        fixture
            .add_player("org.mpris.MediaPlayer2.long", "Playing", true, 2000)
            .await?;
        let target = delayed_target(&fixture).await?;
        let request = control_request(&fixture.bus.address, &target, "play", true, 30_000);
        validate(INPUT_SCHEMA, &request)?;

        let outer_timeout = match run_worker(&request, Duration::from_millis(500), || false) {
            Ok(_) => {
                return Err(
                    io::Error::other("outer timeout unexpectedly returned a result").into(),
                );
            }
            Err(error) => error,
        };
        assert_worker_error(&outer_timeout, "OUTCOME_UNKNOWN", true, false, true);

        let cancelled = Arc::new(AtomicBool::new(false));
        let trigger = Arc::clone(&cancelled);
        let trigger_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            trigger.store(true, Ordering::Release);
        });
        let cancellation = run_worker(&request, Duration::from_secs(5), || {
            cancelled.load(Ordering::Acquire)
        });
        if trigger_thread.join().is_err() {
            return Err(io::Error::other("cancellation trigger thread failed").into());
        }
        let cancellation = match cancellation {
            Ok(_) => {
                return Err(io::Error::other("cancellation unexpectedly returned a result").into());
            }
            Err(error) => error,
        };
        assert_worker_error(&cancellation, "OUTCOME_UNKNOWN", true, false, true);
        Ok::<(), Box<dyn Error>>(())
    })
}

struct SilentUnixSocket {
    path: PathBuf,
    address: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SilentUnixSocket {
    fn start() -> Result<Self, Box<dyn Error>> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ai-computer-toolkit-mpris-control-{}-{stamp}.sock",
            std::process::id()
        ));
        if let Err(error) = fs::remove_file(&path)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(error.into());
        }
        let listener = std::os::unix::net::UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let listener_thread = thread::spawn(move || {
            loop {
                if thread_stop.load(Ordering::Acquire) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        while !thread_stop.load(Ordering::Acquire) {
                            thread::sleep(Duration::from_millis(1));
                        }
                        drop(stream);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            address: format!("unix:path={}", path.display()),
            path,
            stop,
            thread: Some(listener_thread),
        })
    }
}

impl Drop for SilentUnixSocket {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(listener_thread) = self.thread.take() {
            let _ = listener_thread.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

#[test]
fn silent_socket_outer_timeout_is_before_accepted_and_retry_safe() -> Result<(), Box<dyn Error>> {
    let socket = SilentUnixSocket::start()?;
    let request = control_request(&socket.address, FIXTURE_TARGET, "play", true, 30_000);
    validate(INPUT_SCHEMA, &request)?;
    let error = match run_worker(&request, Duration::from_millis(80), || false) {
        Ok(_) => {
            return Err(io::Error::other("silent socket unexpectedly returned a result").into());
        }
        Err(error) => error,
    };
    assert_worker_error(&error, "TIMEOUT", false, true, false);
    Ok(())
}

#[test]
fn old_target_is_pre_dispatch_rejected_on_replacement_private_bus() -> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let mut first = Fixture::new().await?;
        let old_target = discovered_target(&first)?;
        first.stop().await;
        drop(first);

        let replacement = Fixture::new().await?;
        let request = control_request(&replacement.bus.address, &old_target, "play", true, 3000);
        validate(INPUT_SCHEMA, &request)?;
        let result = run_worker(&request, Duration::from_secs(3), || false)?;
        validate(RESULT_SCHEMA, &result)?;
        assert_eq!(result["data"]["accepted"], false);
        assert_eq!(result["data"]["finalStateReached"], false);
        assert_eq!(result["data"]["dispatchOutcome"], "pre-dispatch-rejected");
        assert_eq!(result["data"]["retrySafe"], true);
        assert_eq!(result["data"]["targetMayHaveMutated"], false);
        assert_eq!(replacement.metrics.methods.load(Ordering::Relaxed), 0);
        Ok::<(), Box<dyn Error>>(())
    })
}

#[test]
fn invalid_requests_fail_before_provider_io_with_structured_protocol_error()
-> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let base = control_request(&fixture.bus.address, FIXTURE_TARGET, "play", true, 3000);
        let mut unknown = base.clone();
        unknown["unexpected"] = json!(true);
        let mut null_target = base.clone();
        null_target["targetId"] = Value::Null;
        let mut missing_confirmation = base.clone();
        if let Some(fields) = missing_confirmation.as_object_mut() {
            fields.remove("confirmed");
        }
        let mut null_operation = base;
        null_operation["operation"] = Value::Null;
        for request in [unknown, null_target, missing_confirmation, null_operation] {
            assert!(validate(INPUT_SCHEMA, &request).is_err());
            let error = match run_worker(&request, Duration::from_secs(3), || false) {
                Ok(_) => {
                    return Err(io::Error::other("invalid request unexpectedly succeeded").into());
                }
                Err(error) => error,
            };
            assert_worker_error(&error, "WORKER_PROTOCOL_ERROR", false, true, false);
        }
        assert_eq!(fixture.metrics.properties.load(Ordering::Relaxed), 0);
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 0);
        Ok::<(), Box<dyn Error>>(())
    })
}

#[test]
fn accepted_frame_precedes_mpris_call_and_worker_stays_control_only() -> Result<(), Box<dyn Error>>
{
    let adapter = include_str!("../src/adapters/linux/mpris.rs");
    let accepted = adapter.find("on_accepted()?").ok_or(io::Error::other(
        "MPRIS adapter accepted callback is missing",
    ))?;
    let method_call = adapter
        .find("player.call::<")
        .ok_or(io::Error::other("MPRIS adapter provider call is missing"))?;
    if accepted >= method_call {
        return Err(io::Error::other(
            "MPRIS accepted must be published before the provider method call",
        )
        .into());
    }
    if adapter
        .matches("Builder::address(config.address.as_str())")
        .count()
        != 1
    {
        return Err(
            io::Error::other("MPRIS adapter must keep one shared connection constructor").into(),
        );
    }
    let resolve = adapter
        .split("async fn resolve(")
        .nth(1)
        .and_then(|section| section.split("async fn ensure_owner(").next())
        .ok_or(io::Error::other("MPRIS resolve section is missing"))?;
    for required in [
        "let conn = connect(config, deadline).await?",
        "discover_on_connection(config, &conn, deadline).await?",
    ] {
        if !resolve.contains(required) {
            return Err(io::Error::other(format!(
                "MPRIS resolve does not share its connection: {required}"
            ))
            .into());
        }
    }
    if resolve.contains("discover(config).await?") {
        return Err(io::Error::other(
            "MPRIS resolve must not start an independent discovery deadline",
        )
        .into());
    }
    for required in [
        "MessageStream::for_match_rule",
        "for (expected_owner, names) in &by_owner",
        "owner_changed_during_enumeration(&mut owner_changes).await?",
        "let barrier_guid = normalize_bus_guid(within(deadline, dbus.call(\"GetId\", &())).await?)?",
    ] {
        if !adapter.contains(required) {
            return Err(io::Error::other(format!(
                "MPRIS enumeration identity barrier is missing: {required}"
            ))
            .into());
        }
    }
    if adapter.contains("let _owner_changes") {
        return Err(io::Error::other(
            "MPRIS owner-change subscription must be consumed before publishing a directory",
        )
        .into());
    }

    let worker = include_str!("../src/linux_media_control_worker.rs");
    if !worker.contains("writer.write_value(&accepted)") {
        return Err(io::Error::other("control worker does not write its accepted frame").into());
    }
    for forbidden in ["media_session::discover", "media_session::state"] {
        if worker.contains(forbidden) {
            return Err(io::Error::other(format!(
                "control worker must not call observation path: {forbidden}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn v2_registry_stays_closed_and_main_image_owns_hidden_workers() -> Result<(), Box<dyn Error>> {
    let registry = include_str!("../src/capabilities/registry.rs");
    let marker = "// 三项 MPRIS v2 作为协调候选登记";
    let start = registry
        .find(marker)
        .ok_or_else(|| io::Error::other("MPRIS registry marker is missing"))?;
    let section = &registry[start..];
    let end = section
        .find("];\n")
        .ok_or_else(|| io::Error::other("MPRIS registry section is not closed"))?;
    let mpris_section = &section[..end];
    assert_eq!(mpris_section.matches("execution: \"none\"").count(), 3);
    assert_eq!(
        mpris_section
            .matches("execution_realm: ExecutionRealm::None")
            .count(),
        3
    );

    let cargo = include_str!("../Cargo.toml");
    for binary in [
        "ai-computer-toolkit-linux-media-observation-worker",
        "ai-computer-toolkit-linux-media-control-worker",
    ] {
        assert!(!cargo.contains(&format!("name = \"{binary}\"")));
    }
    let main = include_str!("../src/main.rs");
    // v3 now uses these fixed self-workers in the default production image;
    // the v2 public registry remains closed independently of that entry point.
    assert!(main.contains("linux_media_observation_worker::HIDDEN_ARGUMENT"));
    assert!(main.contains("linux_media_control_worker::HIDDEN_ARGUMENT"));
    let process = include_str!("../src/components/linux_media_worker_process.rs");
    for boundary in [
        ".env_clear()",
        ".current_dir(\"/\")",
        ".process_group(0)",
        "PR_SET_PDEATHSIG",
        "getppid() != parent_pid",
        "libc::kill(-process_group, libc::SIGKILL)",
    ] {
        assert!(
            process.contains(boundary),
            "missing process boundary: {boundary}"
        );
    }
    Ok(())
}
