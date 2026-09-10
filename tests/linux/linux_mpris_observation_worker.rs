#![cfg(all(target_os = "linux", feature = "linux-mpris-candidate"))]

//! Linux MPRIS observation worker 的私有总线、边界和回收集成契约。

#[allow(dead_code)]
#[path = "../support/mpris_private_bus.rs"]
mod mpris_private_bus;

use std::{
    error::Error,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ai_computer_toolkit::{AppResult, mpris_candidate_client};
use jsonschema::draft202012;
use mpris_private_bus::{Fixture, OwnerChurn};
use serde_json::{Value, json};

const TOOLKIT: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit");
const REQUEST_SCHEMA: &str =
    include_str!("../../contracts/internal/linux-mpris-observation-worker-v1.schema.json");
const DISCOVER_SCHEMA: &str = include_str!("../../contracts/v2/media-session-observation.schema.json");
const STATE_SCHEMA: &str = include_str!("../../contracts/v2/media-playback-state.schema.json");

fn worker_request(
    address: &str,
    operation: &str,
    maximum_items: Option<u32>,
    timeout_ms: Option<u32>,
    target_id: Option<&str>,
) -> Value {
    let mut request = json!({
        "protocolVersion": "act/internal/linux-mpris-observation/v1",
        "operation": operation,
        "sessionBusAddress": address,
        "brokerEpoch": "integration-epoch",
    });
    if let Some(maximum_items) = maximum_items {
        request["maximumItems"] = json!(maximum_items);
    }
    if let Some(timeout_ms) = timeout_ms {
        request["timeoutMs"] = json!(timeout_ms);
    }
    if let Some(target_id) = target_id {
        request["targetId"] = json!(target_id);
    }
    request
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
    mpris_candidate_client::run_fixed_image_process(Path::new(TOOLKIT), request, timeout, cancelled)
}

fn validate(schema_text: &str, instance: &Value) -> Result<(), Box<dyn Error>> {
    let schema = serde_json::from_str::<Value>(schema_text)?;
    draft202012::validate(&schema, instance).map_err(|error| error.to_string())?;
    Ok(())
}

fn assert_private_output(value: &Value, address: &str) -> Result<(), Box<dyn Error>> {
    let serialized = serde_json::to_string(value)?;
    for forbidden in [
        address,
        "org.mpris",
        "\"owner\"",
        "\"path\"",
        "\"pid\"",
        "\"native\"",
        "\"transport\"",
        "\"busAddress\"",
        "\"sessionBusAddress\"",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "worker output leaked forbidden provider material: {forbidden}"
        );
    }
    Ok(())
}

fn required_target(value: &Value) -> Result<String, Box<dyn Error>> {
    value["data"]["sessions"]
        .as_array()
        .and_then(|sessions| sessions.first())
        .and_then(|session| session["sessionId"].as_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::other("worker discovery returned no opaque target").into())
}

#[test]
fn worker_discover_and_state_validate_v2_and_keep_provider_private() -> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let discovery_request = worker_request(&fixture.bus.address, "discover", None, None, None);
        validate(REQUEST_SCHEMA, &discovery_request)?;
        let discovery = run_worker(&discovery_request, Duration::from_secs(3), || false)?;
        validate(DISCOVER_SCHEMA, &discovery)?;
        assert_eq!(discovery["data"]["count"], 1);
        assert_eq!(discovery["data"]["total"], 1);
        assert_eq!(discovery["data"]["truncated"], false);
        assert_private_output(&discovery, &fixture.bus.address)?;
        assert_eq!(fixture.metrics.properties.load(Ordering::Relaxed), 0);
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 0);

        let target_id = required_target(&discovery)?;
        let state_request = worker_request(
            &fixture.bus.address,
            "state",
            Some(128),
            Some(3000),
            Some(&target_id),
        );
        validate(REQUEST_SCHEMA, &state_request)?;
        let state = run_worker(&state_request, Duration::from_secs(3), || false)?;
        validate(STATE_SCHEMA, &state)?;
        assert_eq!(state["data"]["sessionId"], target_id);
        assert_eq!(state["data"]["playbackStatus"], "playing");
        assert_private_output(&state, &fixture.bus.address)?;
        assert_eq!(fixture.metrics.properties.load(Ordering::Relaxed), 6);
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 0);
        Ok::<(), Box<dyn Error>>(())
    })
}

#[test]
fn worker_rejects_mismatched_expected_bus_guid_before_player_io() -> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let mismatch = mismatched_bus_guid(&fixture.bus.address)?;
        let mut request = worker_request(&fixture.bus.address, "discover", None, None, None);
        request["expectedBusGuid"] = json!(mismatch);
        validate(REQUEST_SCHEMA, &request)?;

        let error = match run_worker(&request, Duration::from_secs(3), || false) {
            Ok(_) => {
                return Err(io::Error::other(
                    "mismatched expected bus GUID unexpectedly succeeded",
                )
                .into());
            }
            Err(error) => error,
        };
        assert_eq!(error.code, "STALE_SESSION");
        assert_eq!(error.details["workerReaped"], true);
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
fn worker_rejects_owner_churn_during_discovery_before_player_io() -> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        fixture.add_mpris_aliases(64).await?;
        let mut churn = OwnerChurn::start(&fixture.bus.address)?;
        let request = worker_request(&fixture.bus.address, "discover", None, None, None);
        validate(REQUEST_SCHEMA, &request)?;

        let result = run_worker(&request, Duration::from_secs(3), || false);
        let cycles = churn.cycles();
        churn.stop_and_join()?;
        let stale = match result {
            Ok(_) => {
                return Err(io::Error::other(format!(
                    "owner churn unexpectedly allowed discovery to succeed (cycles={cycles})"
                ))
                .into());
            }
            Err(error) => error,
        };
        assert!(cycles >= 1, "owner churn did not complete its ready cycle");
        assert_eq!(stale.code, "STALE_SESSION");
        assert_eq!(stale.details["workerReaped"], true);
        assert_eq!(fixture.metrics.properties.load(Ordering::Relaxed), 0);
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 0);
        Ok::<(), Box<dyn Error>>(())
    })
}

#[test]
fn worker_bounds_discovery_and_rejects_old_target_on_new_private_bus() -> Result<(), Box<dyn Error>>
{
    async_io::block_on(async {
        let mut fixture = Fixture::new().await?;
        fixture
            .add_player("org.mpris.MediaPlayer2.second", "Paused", true, 0)
            .await?;

        let full_request = worker_request(
            &fixture.bus.address,
            "discover",
            Some(128),
            Some(3000),
            None,
        );
        validate(REQUEST_SCHEMA, &full_request)?;
        let full = run_worker(&full_request, Duration::from_secs(3), || false)?;
        validate(DISCOVER_SCHEMA, &full)?;
        let full_sessions = full["data"]["sessions"]
            .as_array()
            .ok_or("full discovery sessions missing")?;
        let old_target = full_sessions
            .first()
            .and_then(|session| session["sessionId"].as_str())
            .map(ToOwned::to_owned)
            .ok_or("first full discovery target missing")?;
        let excluded_target = full_sessions
            .get(1)
            .and_then(|session| session["sessionId"].as_str())
            .map(ToOwned::to_owned)
            .ok_or("second full discovery target missing")?;

        let limited_request =
            worker_request(&fixture.bus.address, "discover", Some(1), Some(3000), None);
        validate(REQUEST_SCHEMA, &limited_request)?;
        let limited = run_worker(&limited_request, Duration::from_secs(3), || false)?;
        validate(DISCOVER_SCHEMA, &limited)?;
        assert_eq!(limited["data"]["count"], 1);
        assert_eq!(limited["data"]["total"], 2);
        assert_eq!(limited["data"]["truncated"], true);
        assert_eq!(limited["data"]["complete"], false);

        let excluded_request = worker_request(
            &fixture.bus.address,
            "state",
            Some(1),
            Some(3000),
            Some(&excluded_target),
        );
        validate(REQUEST_SCHEMA, &excluded_request)?;
        let excluded = match run_worker(&excluded_request, Duration::from_secs(3), || false) {
            Ok(_) => {
                return Err(io::Error::other(
                    "a target excluded by a truncated directory unexpectedly resolved",
                )
                .into());
            }
            Err(error) => error,
        };
        assert_eq!(excluded.code, "BACKGROUND_OPERATION_UNAVAILABLE");
        assert_eq!(excluded.details["workerReaped"], true);

        fixture.stop().await;
        drop(fixture);
        let replacement = Fixture::new().await?;
        let stale_request = worker_request(
            &replacement.bus.address,
            "state",
            Some(128),
            Some(3000),
            Some(&old_target),
        );
        validate(REQUEST_SCHEMA, &stale_request)?;
        let stale = match run_worker(&stale_request, Duration::from_secs(3), || false) {
            Ok(_) => {
                return Err(io::Error::other(
                    "old target unexpectedly resolved on a new private bus",
                )
                .into());
            }
            Err(error) => error,
        };
        assert_eq!(stale.code, "STALE_SESSION");
        assert_eq!(stale.details["workerReaped"], true);
        Ok::<(), Box<dyn Error>>(())
    })
}

#[test]
fn worker_rejects_unknown_null_and_operation_target_mixing_before_provider_reads()
-> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let invalid_requests = [
            json!({
                "protocolVersion": "act/internal/linux-mpris-observation/v1",
                "operation": "discover",
                "sessionBusAddress": fixture.bus.address.as_str(),
                "brokerEpoch": "integration-epoch",
                "extra": true,
            }),
            json!({
                "protocolVersion": "act/internal/linux-mpris-observation/v1",
                "operation": "discover",
                "sessionBusAddress": fixture.bus.address.as_str(),
                "brokerEpoch": "integration-epoch",
                "targetId": null,
            }),
            json!({
                "protocolVersion": "act/internal/linux-mpris-observation/v1",
                "operation": "discover",
                "sessionBusAddress": fixture.bus.address.as_str(),
                "brokerEpoch": "integration-epoch",
                "targetId": "s2:m:0123456789abcdef",
            }),
            json!({
                "protocolVersion": "act/internal/linux-mpris-observation/v1",
                "operation": "state",
                "sessionBusAddress": fixture.bus.address.as_str(),
                "brokerEpoch": "integration-epoch",
            }),
        ];
        for request in invalid_requests {
            assert!(validate(REQUEST_SCHEMA, &request).is_err());
            let error = match run_worker(&request, Duration::from_secs(3), || false) {
                Ok(_) => {
                    return Err(
                        io::Error::other("invalid worker request unexpectedly succeeded").into(),
                    );
                }
                Err(error) => error,
            };
            assert_eq!(error.code, "WORKER_PROTOCOL_ERROR");
        }
        assert_eq!(fixture.metrics.properties.load(Ordering::Relaxed), 0);
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 0);
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
            "ai-computer-toolkit-mpris-observation-{}-{stamp}.sock",
            std::process::id()
        ));
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
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
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nonblocking(true);
                        let mut buffer = [0_u8; 1024];
                        loop {
                            if thread_stop.load(Ordering::Acquire) {
                                break;
                            }
                            match stream.read(&mut buffer) {
                                Ok(0) => break,
                                Ok(_) => {}
                                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                    thread::sleep(Duration::from_millis(1));
                                }
                                Err(_) => break,
                            }
                        }
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
fn fixture_launcher_reports_cancel_and_outer_timeout_after_worker_reap()
-> Result<(), Box<dyn Error>> {
    let cancellation_socket = SilentUnixSocket::start()?;
    let cancellation_request = worker_request(
        &cancellation_socket.address,
        "discover",
        Some(128),
        Some(30_000),
        None,
    );
    let cancellation = Arc::new(AtomicBool::new(false));
    let trigger = Arc::clone(&cancellation);
    let trigger_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        trigger.store(true, Ordering::Release);
    });
    let cancelled = run_worker(&cancellation_request, Duration::from_secs(2), || {
        cancellation.load(Ordering::Acquire)
    });
    if trigger_thread.join().is_err() {
        return Err(io::Error::other("cancellation trigger thread failed").into());
    }
    let cancelled = match cancelled {
        Ok(_) => {
            return Err(io::Error::other("cancelled worker unexpectedly succeeded").into());
        }
        Err(error) => error,
    };
    assert_eq!(cancelled.code, "CANCELLED");
    assert_eq!(cancelled.details["workerReaped"], true);

    let timeout_socket = SilentUnixSocket::start()?;
    let timeout_request = worker_request(
        &timeout_socket.address,
        "discover",
        Some(128),
        Some(30_000),
        None,
    );
    let timed_out = match run_worker(&timeout_request, Duration::from_millis(1), || false) {
        Ok(_) => {
            return Err(io::Error::other("timed-out worker unexpectedly succeeded").into());
        }
        Err(error) => error,
    };
    assert_eq!(timed_out.code, "TIMEOUT");
    assert_eq!(timed_out.details["workerReaped"], true);
    Ok(())
}

#[test]
fn worker_source_is_read_only_and_public_mpris_registry_remains_unavailable()
-> Result<(), Box<dyn Error>> {
    let worker_source = include_str!("../../src/linux_media_observation_worker.rs");
    for forbidden in [
        "mpris::control",
        "player.call",
        ".call(",
        "control(",
        "method(",
        "GetAll",
        "metadata",
        "activation",
        "StartServiceByName",
    ] {
        assert!(
            !worker_source.contains(forbidden),
            "worker source contains forbidden provider operation: {forbidden}"
        );
    }
    assert!(worker_source.contains("LinuxMediaObservationOperation::Discover"));
    assert!(worker_source.contains("LinuxMediaObservationOperation::State"));

    let registry = include_str!("../../src/capabilities/registry.rs");
    let marker = "// 三项 MPRIS v2 作为协调候选登记";
    let start = registry
        .find(marker)
        .ok_or_else(|| io::Error::other("MPRIS registry marker is missing"))?;
    let section = &registry[start..];
    let end = section
        .find("];")
        .ok_or_else(|| io::Error::other("MPRIS registry section is not closed"))?;
    let mpris_section = &section[..end];
    for id in [
        "MEDIA_SESSION_DISCOVER_V2",
        "MEDIA_PLAYBACK_STATE_READ_V2",
        "MEDIA_PLAYBACK_CONTROL_V2",
    ] {
        assert!(mpris_section.contains(&format!("id: {id},")));
    }
    assert_eq!(mpris_section.matches("execution: \"none\"").count(), 3);
    assert_eq!(
        mpris_section
            .matches("execution_realm: ExecutionRealm::None")
            .count(),
        3
    );
    Ok(())
}
