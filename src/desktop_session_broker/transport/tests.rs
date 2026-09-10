//! 验证单次 socket 调用的结果绑定、重复投递与在途取消，不访问真实桌面。

use std::{sync::atomic::AtomicUsize, time::Instant};

use crate::{
    components::{
        desktop_session_input_cancellation::DesktopInputCancellation,
        keyboard_input_contract::KeyboardInput,
    },
    modules::desktop_session::{
        DesktopKeyboardDispatchFacts, DesktopSessionFacts, DesktopSessionInputFailure,
        DesktopSessionLease, DesktopSessionPortFailure,
    },
};

use super::*;

const EPOCH: &str = "12345678901234567890123456789012";
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type Running = (
    Endpoint,
    thread::JoinHandle<i32>,
    Arc<AtomicUsize>,
    Arc<AtomicBool>,
);

#[derive(Default)]
struct RecordingWriter {
    bytes: Vec<u8>,
    writes: usize,
    flushes: usize,
    chunk_limit: Option<usize>,
    interrupt_once: bool,
    fail_flush: bool,
}

impl Write for RecordingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        if std::mem::take(&mut self.interrupt_once) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let count = self.chunk_limit.unwrap_or(bytes.len()).min(bytes.len());
        self.bytes.extend_from_slice(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        if self.fail_flush {
            Err(io::ErrorKind::BrokenPipe.into())
        } else {
            Ok(())
        }
    }
}

#[test]
fn json_line_is_coalesced_without_changing_wire_bytes() -> TestResult {
    let value = json!({"messageType":"response", "completed":true,
        "data":{"text":"中文\n\"quoted\"", "steps":[1,2,3], "empty":null}});
    let mut legacy = RecordingWriter::default();
    serde_json::to_writer(&mut legacy, &value)?;
    legacy.write_all(b"\n")?;
    legacy.flush()?;
    let mut writer = RecordingWriter::default();
    write_json_line(&mut writer, &value)?;
    assert_eq!(writer.bytes, legacy.bytes);
    assert!(legacy.writes > 20);
    assert_eq!(
        writer.writes, 1,
        "one frame should not generate per-token writes"
    );
    assert_eq!(writer.flushes, 1);
    Ok(())
}

#[test]
fn json_line_handles_short_writes_and_surfaces_output_failures() -> TestResult {
    let value = json!({"completed":true});
    let mut writer = RecordingWriter {
        chunk_limit: Some(2),
        interrupt_once: true,
        ..RecordingWriter::default()
    };
    write_json_line(&mut writer, &value)?;
    assert_eq!(writer.bytes, format!("{value}\n").as_bytes());
    assert_eq!(writer.flushes, 1);

    let mut blocked = RecordingWriter {
        chunk_limit: Some(0),
        ..RecordingWriter::default()
    };
    assert_eq!(
        write_json_line(&mut blocked, &value).unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );
    assert_eq!(blocked.flushes, 0);
    let mut broken = RecordingWriter {
        fail_flush: true,
        ..RecordingWriter::default()
    };
    assert_eq!(
        write_json_line(&mut broken, &value).unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(broken.bytes, format!("{value}\n").as_bytes());
    assert_eq!(broken.flushes, 1);
    Ok(())
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> TestResult<Self> {
        let path = std::env::temp_dir().join(format!(
            "act-socket-test-{}",
            desktop_session_identity::random_nonce().map_err(io::Error::other)?
        ));
        DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct TestPort {
    opens: Arc<AtomicUsize>,
    entered: Arc<AtomicBool>,
}

struct TestLease(Arc<AtomicBool>);

impl DesktopSessionPort for TestPort {
    fn open(
        &self,
        _: Duration,
    ) -> Result<(Box<dyn DesktopSessionLease>, DesktopSessionFacts), DesktopSessionPortFailure>
    {
        self.opens.fetch_add(1, Ordering::AcqRel);
        Ok((
            Box::new(TestLease(Arc::clone(&self.entered))),
            DesktopSessionFacts::new(2, 5, vec!["keyboard", "pointer"], 1, 1),
        ))
    }
}

impl DesktopSessionLease for TestLease {
    fn send_keyboard(
        &mut self,
        _: &KeyboardInput,
        cancellation: &DesktopInputCancellation,
    ) -> Result<DesktopKeyboardDispatchFacts, DesktopSessionInputFailure> {
        self.0.store(true, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !cancellation.is_cancelled() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        if cancellation.is_cancelled() {
            Err(DesktopSessionInputFailure::cancelled(true, 0, 0))
        } else {
            Err(DesktopSessionInputFailure::before_dispatch(
                "TIMEOUT",
                "test-wait",
            ))
        }
    }

    fn send_pointer(
        &mut self,
        _: &crate::components::desktop_session_pointer_input::DesktopPointerInput,
        _: &DesktopInputCancellation,
    ) -> Result<
        crate::modules::desktop_session::DesktopPointerDispatchFacts,
        DesktopSessionInputFailure,
    > {
        Err(DesktopSessionInputFailure::before_dispatch(
            "TEST_UNUSED",
            "test-pointer",
        ))
    }

    fn capture_frame(
        &mut self,
        _: Duration,
        _: Option<u32>,
    ) -> Result<
        crate::components::desktop_session_frame_capture::DesktopCapturedFrame,
        crate::modules::desktop_session::DesktopSessionFrameFailure,
    > {
        Err(
            crate::modules::desktop_session::DesktopSessionFrameFailure::new(
                "TEST_UNUSED",
                "test-capture",
                false,
                false,
            ),
        )
    }

    fn close(self: Box<Self>) -> Result<(), DesktopSessionPortFailure> {
        Ok(())
    }
}

fn request(operation: &str, nonce: u32) -> Value {
    json!({"contractVersion":CONTRACT_VERSION,"brokerEpoch":EPOCH,
        "requestNonce":format!("{nonce:032x}"),"operation":operation})
}

fn call(directory: &Path, request: Value) -> TestResult<Value> {
    request_response(directory, &serde_json::from_value(request)?)
        .map_err(|error| format!("{error:?}").into())
}

fn start(directory: &Path) -> TestResult<Running> {
    let (endpoint, listener) = Endpoint::bind(directory, EPOCH)?;
    let opens = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(AtomicBool::new(false));
    let port = TestPort {
        opens: Arc::clone(&opens),
        entered: Arc::clone(&entered),
    };
    let worker = thread::spawn(move || {
        serve_socket(
            listener,
            Broker::new(EPOCH.to_owned(), DesktopSessionModule::new(port)),
            &mut Vec::new(),
        )
    });
    Ok((endpoint, worker, opens, entered))
}

fn open(directory: &Path, nonce: u32) -> TestResult<Value> {
    let mut input = request("open", nonce);
    input["confirmed"] = json!(true);
    input["foregroundConsent"] = json!(true);
    input["strictIsolation"] = json!(false);
    input["timeoutMs"] = json!(10000);
    call(directory, input)
}

#[test]
fn socket_round_trip_and_shutdown_preserve_request_identity() -> TestResult {
    let directory = Directory::new()?;
    let (endpoint, worker, opens, _) = start(&directory.0)?;
    let response = call(&directory.0, request("sessions", 1))?;
    assert_eq!(response["completed"], true);
    assert_eq!(response["data"]["sessions"], json!([]));
    assert_eq!(response["requestNonce"], format!("{:032x}", 1));
    assert_eq!(opens.load(Ordering::Acquire), 0);
    assert_eq!(
        call(&directory.0, request("shutdown", 2))?["completed"],
        true
    );
    assert_eq!(worker.join().map_err(|_| "server panicked")?, 0);
    let path = endpoint.path.clone();
    drop(endpoint);
    assert!(!path.exists());
    Ok(())
}

#[test]
fn retransmission_replays_original_result_without_reopening() -> TestResult {
    let directory = Directory::new()?;
    let (_endpoint, worker, opens, _) = start(&directory.0)?;
    let first = open(&directory.0, 1)?;
    assert_eq!(first["completed"], true);
    assert_eq!(open(&directory.0, 1)?, first);
    assert_eq!(opens.load(Ordering::Acquire), 1);
    let mut conflicting = request("inspect", 1);
    conflicting["sessionId"] = first["data"]["sessionId"].clone();
    assert_eq!(
        call(&directory.0, conflicting)?["error"]["code"],
        "NONCE_SEMANTIC_CONFLICT"
    );
    call(&directory.0, request("shutdown", 3))?;
    assert_eq!(worker.join().map_err(|_| "server panicked")?, 0);
    Ok(())
}

#[test]
fn a_separate_connection_cancels_input_while_owner_is_busy() -> TestResult {
    let directory = Directory::new()?;
    let (_endpoint, worker, _, entered) = start(&directory.0)?;
    let session = open(&directory.0, 1)?["data"]["sessionId"].clone();
    let mut input = request("input-key", 2);
    input["sessionId"] = session;
    input["confirmed"] = json!(true);
    input["foregroundConsent"] = json!(true);
    input["strictIsolation"] = json!(false);
    input["input"] = json!({"key":"a"});
    let path = directory.0.clone();
    let input_worker = thread::spawn(move || call(&path, input));
    let deadline = Instant::now() + Duration::from_secs(2);
    while !entered.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert!(entered.load(Ordering::Acquire));
    let mut cancel = request("input-cancel", 3);
    cancel["targetRequestNonce"] = json!(format!("{:032x}", 2));
    assert_eq!(call(&directory.0, cancel)?["data"]["status"], "cancelled");
    let response = input_worker.join().map_err(|_| "input panicked")??;
    assert_eq!(response["error"]["code"], "CANCELLED");
    assert_eq!(response["automaticRetryProhibited"], true);
    call(&directory.0, request("shutdown", 4))?;
    assert_eq!(worker.join().map_err(|_| "server panicked")?, 0);
    Ok(())
}

#[test]
fn foreign_response_identity_and_extra_frames_are_rejected() -> TestResult {
    let directory = Directory::new()?;
    let (endpoint, listener) = Endpoint::bind(&directory.0, EPOCH)?;
    let server = thread::spawn(move || -> TestResult {
        let (mut stream, _) = listener.accept()?;
        let mut input = String::new();
        stream.read_to_string(&mut input)?;
        write_json_line(
            &mut stream,
            &json!({"messageType":"response","requestNonce":"other"}),
        )?;
        Ok(())
    });
    let parsed = serde_json::from_value(request("sessions", 1))?;
    let failure = request_response(&directory.0, &parsed)
        .err()
        .ok_or("accepted foreign response")?;
    let report = failure.response(&parsed);
    assert_eq!(report["outcome"], "unknown");
    assert_eq!(report["acceptedMayHaveOccurred"], true);
    assert_eq!(report["automaticRetryProhibited"], true);
    assert_eq!(report["requestNonce"], parsed.request_nonce());
    server.join().map_err(|_| "server panicked")??;
    drop(endpoint);

    let (endpoint, worker, opens, _) = start(&directory.0)?;
    let mut stream = UnixStream::connect(&endpoint.path)?;
    stream.write_all(
        format!("{}\n{}\n", request("sessions", 2), request("shutdown", 3)).as_bytes(),
    )?;
    stream.shutdown(Shutdown::Write)?;
    let mut output = String::new();
    stream.read_to_string(&mut output)?;
    assert_eq!(
        serde_json::from_str::<Value>(&output)?["code"],
        "BROKER_PROTOCOL_FAILED"
    );
    assert_eq!(opens.load(Ordering::Acquire), 0);
    call(&directory.0, request("shutdown", 4))?;
    assert_eq!(worker.join().map_err(|_| "server panicked")?, 0);
    Ok(())
}

#[test]
fn non_private_directories_and_symlink_endpoints_are_rejected() -> TestResult {
    let directory = Directory::new()?;
    let uid = unsafe { libc::geteuid() };
    validate_directory(&directory.0, uid)?;
    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o755))?;
    assert!(validate_directory(&directory.0, uid).is_err());
    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700))?;
    let path = directory.0.join(format!("{EPOCH}.sock"));
    std::os::unix::fs::symlink("/nonexistent", path)?;
    let parsed = serde_json::from_value(request("sessions", 1))?;
    let failure = request_response(&directory.0, &parsed)
        .err()
        .ok_or("accepted symlink")?;
    assert_eq!(failure.response(&parsed)["outcome"], "failed");
    assert_eq!(failure.response(&parsed)["acceptedMayHaveOccurred"], false);
    Ok(())
}

#[test]
fn oversized_requests_are_rejected_before_business_dispatch() -> TestResult {
    let directory = Directory::new()?;
    let (endpoint, worker, opens, _) = start(&directory.0)?;
    let mut stream = UnixStream::connect(&endpoint.path)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(&vec![b'x'; MAXIMUM_FRAME_BYTES + 1])?;
    stream.shutdown(Shutdown::Write)?;
    let mut output = String::new();
    BufReader::new(stream).read_line(&mut output)?;
    assert_eq!(
        serde_json::from_str::<Value>(&output)?["code"],
        "BROKER_PROTOCOL_FAILED"
    );
    assert_eq!(opens.load(Ordering::Acquire), 0);
    call(&directory.0, request("shutdown", 1))?;
    assert_eq!(worker.join().map_err(|_| "server panicked")?, 0);
    Ok(())
}

#[test]
fn response_deadline_does_not_wait_for_peer_disconnect() -> TestResult {
    let (mut client, mut server) = UnixStream::pair()?;
    server.write_all(b"{")?;
    let started = Instant::now();
    let failure = read_response(&mut client, Duration::from_millis(20))
        .err()
        .ok_or("incomplete response accepted")?;
    assert!(matches!(
        failure.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ));
    assert!(started.elapsed() < Duration::from_secs(1));
    Ok(())
}

#[test]
fn socket_ready_and_client_failures_match_public_schema() -> TestResult {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../contracts/v1/linux-desktop-session-broker-v1.schema.json"
    ))?;
    let port = TestPort {
        opens: Arc::new(AtomicUsize::new(0)),
        entered: Arc::new(AtomicBool::new(false)),
    };
    let mut ready = Broker::new(EPOCH.to_owned(), DesktopSessionModule::new(port)).ready();
    ready["transport"] = json!("json-lines-unix-socket");
    let parsed = serde_json::from_value(request("sessions", 1))?;
    for report in [
        ready,
        ClientFailure { dispatched: false }.response(&parsed),
        ClientFailure { dispatched: true }.response(&parsed),
    ] {
        jsonschema::draft202012::validate(&schema, &report)
            .map_err(|error| format!("invalid transport envelope: {error}"))?;
    }
    Ok(())
}
