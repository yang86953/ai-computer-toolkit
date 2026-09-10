//! MCP 与 CLI 共用一个跨平台 stdio broker，使用有界帧和可执行的超时。

pub use super::failure::BrokerFailure;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    thread::JoinHandle,
    time::{Duration, Instant},
};

// 保留已发布协议标识；名称中的 linux 是历史标识，不再限定后端。
const CONTRACT_VERSION: &str = "act/linux-desktop-session-broker/v1";
const MAXIMUM_LINE_BYTES: usize = 64 * 1024;
const READY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Broker {
    child: Child,
    cancellation: crate::components::desktop_session_input_cancellation::DesktopInputCancellation,
    requests: Option<SyncSender<String>>,
    responses: Receiver<Result<Value, String>>,
    readers: Vec<JoinHandle<()>>,
    epoch: String,
    closed: bool,
}

impl Broker {
    pub fn start(program: &str) -> Result<Self, BrokerFailure> {
        let mut child = Command::new(program)
            .args(["session-host", "desktop"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| BrokerFailure::failed("BROKER_START_FAILED", e.to_string()))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (requests, queue) = mpsc::sync_channel::<String>(1);
        let (events, responses) = mpsc::sync_channel(2);
        let writer_events = events.clone();
        let writer = std::thread::spawn(move || {
            while let Ok(payload) = queue.recv() {
                if let Err(error) = stdin
                    .write_all(payload.as_bytes())
                    .and_then(|_| stdin.flush())
                {
                    let _ = writer_events.try_send(Err(error.to_string()));
                    break;
                }
            }
        });
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut bytes = Vec::new();
                let result = reader
                    .by_ref()
                    .take((MAXIMUM_LINE_BYTES + 1) as u64)
                    .read_until(b'\n', &mut bytes);
                match result {
                    Ok(0) => break,
                    Ok(_) if bytes.len() <= MAXIMUM_LINE_BYTES && bytes.ends_with(b"\n") => {
                        let value = serde_json::from_slice(&bytes).map_err(|e| e.to_string());
                        if events.try_send(value).is_err() {
                            break;
                        }
                    }
                    _ => {
                        let _ =
                            events.try_send(Err("Invalid or oversized broker frame.".to_owned()));
                        break;
                    }
                }
            }
        });
        let mut broker = Self {
            cancellation: Default::default(),
            child,
            requests: Some(requests),
            responses,
            readers: vec![writer, reader],
            epoch: String::new(),
            closed: false,
        };
        let ready = broker
            .read(READY_TIMEOUT)
            .map_err(|e| BrokerFailure::failed("BROKER_START_FAILED", e.message))?;
        let epoch = ready["brokerEpoch"].as_str().unwrap_or_default();
        if ready["messageType"] != "broker-ready"
            || ready["contractVersion"] != CONTRACT_VERSION
            || ready["transport"] != "json-lines-stdio"
            || epoch.len() != 32
            || !epoch.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(BrokerFailure::failed(
                "BROKER_START_FAILED",
                "Invalid broker-ready frame.",
            ));
        }
        broker.epoch = epoch.to_owned();
        if ready["postInputObservation"] != true {
            broker.stop();
            return Err(BrokerFailure::failed(
                "BROKER_FEATURE_UNAVAILABLE",
                "The broker lacks postInputObservation.",
            ));
        }
        Ok(broker)
    }

    fn read(&self, timeout: Duration) -> Result<Value, BrokerFailure> {
        self.responses
            .recv_timeout(timeout)
            .map_err(|e| BrokerFailure::unknown(format!("Broker response unavailable: {e}")))?
            .map_err(BrokerFailure::unknown)
    }

    pub(crate) fn set_cancellation(
        &mut self,
        token: crate::components::desktop_session_input_cancellation::DesktopInputCancellation,
    ) {
        self.cancellation = token;
    }

    pub fn call(
        &mut self,
        operation: &str,
        fields: Value,
        timeout: Duration,
    ) -> Result<Value, BrokerFailure> {
        if self.closed {
            return Err(BrokerFailure::failed(
                "BROKER_CLOSED",
                "Explicitly reconnect; input is never replayed.",
            ));
        }
        if self.cancellation.is_cancelled()
            && !matches!(operation, "shutdown" | "close" | "sessions")
        {
            return Err(BrokerFailure::failed(
                "CANCELLED",
                "Request cancelled before dispatch.",
            ));
        }
        let nonce = crate::components::desktop_session_identity::random_nonce()
            .map_err(|e| BrokerFailure::failed(e.code, e.message))?;
        let mut request = json!({"contractVersion":CONTRACT_VERSION,"brokerEpoch":self.epoch,"requestNonce":nonce,"operation":operation});
        if let Some(fields) = fields.as_object() {
            for (key, value) in fields {
                if request.get(key).is_some() {
                    return Err(BrokerFailure::failed(
                        "INVALID_ARGUMENT",
                        "Cannot replace broker identity fields.",
                    ));
                }
                request[key] = value.clone();
            }
        }
        let payload = format!("{request}\n");
        if payload.len() > MAXIMUM_LINE_BYTES {
            return Err(BrokerFailure::failed(
                "REQUEST_TOO_LARGE",
                "Broker request exceeds 64 KiB.",
            ));
        }
        let result = (|| {
            self.requests
                .as_ref()
                .ok_or_else(|| BrokerFailure::unknown("Broker input closed."))?
                .try_send(payload)
                .map_err(|e| BrokerFailure::unknown(e.to_string()))?;
            let deadline = Instant::now() + timeout;
            let mut original = None;
            let mut cancel_nonce = None;
            let mut cancel_ack = false;
            loop {
                if cancel_nonce.is_none()
                    && matches!(operation, "input-key" | "input-pointer" | "interact")
                    && (self.cancellation.is_cancelled()
                        || crate::components::cancellation::is_cancelled())
                {
                    let id = crate::components::desktop_session_identity::random_nonce()
                        .map_err(|_| BrokerFailure::unknown("Cannot correlate cancellation."))?;
                    let cancel = json!({"contractVersion":CONTRACT_VERSION,"brokerEpoch":self.epoch,"requestNonce":id,"operation":"input-cancel","targetRequestNonce":nonce});
                    self.requests
                        .as_ref()
                        .ok_or_else(|| BrokerFailure::unknown("Broker input closed."))?
                        .try_send(format!("{cancel}\n"))
                        .map_err(|e| BrokerFailure::unknown(e.to_string()))?;
                    cancel_nonce = Some(id);
                }
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or_else(|| BrokerFailure::unknown("Broker response deadline exceeded."))?;
                let response = match self
                    .responses
                    .recv_timeout(remaining.min(Duration::from_millis(20)))
                {
                    Ok(Ok(value)) => value,
                    Ok(Err(message)) => return Err(BrokerFailure::unknown(message)),
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(error) => return Err(BrokerFailure::unknown(error.to_string())),
                };
                if cancel_nonce.as_deref() == response.get("requestNonce").and_then(Value::as_str) {
                    if cancel_ack {
                        return Err(BrokerFailure::unknown(
                            "Duplicate cancellation acknowledgement.",
                        ));
                    }
                    validate_response(
                        &response,
                        cancel_nonce.as_deref().unwrap(),
                        "input-cancel",
                        &self.epoch,
                    )
                    .map_err(|_| BrokerFailure::unknown("Invalid cancellation acknowledgement."))?;
                    cancel_ack = true;
                } else {
                    validate_correlation(&response, &nonce, operation, &self.epoch)?;
                    if original.replace(response).is_some() {
                        return Err(BrokerFailure::unknown("Duplicate operation response."));
                    }
                }
                if original.is_some() && (cancel_nonce.is_none() || cancel_ack) {
                    return Ok(original.take().unwrap());
                }
            }
        })();
        // 只有传输损坏会杀死 broker；业务失败已由共享会话模块执行收尾。
        if result.is_err() {
            self.terminate();
        }
        let response = result?;
        validate_response(&response, &nonce, operation, &self.epoch)?;
        Ok(response)
    }

    fn terminate(&mut self) {
        self.closed = true;
        self.requests.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        for thread in self.readers.drain(..) {
            let _ = thread.join();
        }
    }
    pub fn stop(&mut self) {
        if !self.closed {
            let _ = self.call("shutdown", json!({}), Duration::from_secs(5));
            let until = Instant::now() + Duration::from_secs(5);
            while self.child.try_wait().ok().flatten().is_none() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        self.terminate();
    }
}
impl Drop for Broker {
    fn drop(&mut self) {
        self.terminate();
    }
}
/// 校验响应与本请求的相关性；不相关的响应一律拒绝。
fn validate_response(
    response: &Value,
    nonce: &str,
    operation: &str,
    epoch: &str,
) -> Result<(), BrokerFailure> {
    validate_correlation(response, nonce, operation, epoch)?;
    let completed = response.get("completed") == Some(&Value::Bool(true))
        && response.get("outcome").and_then(Value::as_str) == Some("completed")
        && response.get("businessAccepted") == Some(&Value::Bool(true));
    if !completed {
        // 业务拒绝原样上抛 broker 的错误码与细节。
        let code = response
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("BROKER_REJECTED");
        let message = response
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("The broker rejected the operation.");
        let details = &response["error"]["details"];
        let mut failure = BrokerFailure::failed(code, message);
        failure.outcome_unknown =
            response["outcome"] == "unknown" || details["outcome"] == "unknown";
        failure.accepted_may_have_occurred = details["acceptedMayHaveOccurred"]
            .as_bool()
            .unwrap_or(failure.outcome_unknown);
        // 细节原样保留给调用方：诊断输入失败靠的就是 stage 与已完成步数。
        if details.is_object() {
            failure.details = details.clone();
        }
        return Err(failure);
    }
    Ok(())
}

fn validate_correlation(
    response: &Value,
    nonce: &str,
    operation: &str,
    epoch: &str,
) -> Result<(), BrokerFailure> {
    let correlated = response.get("contractVersion").and_then(Value::as_str)
        == Some(CONTRACT_VERSION)
        && response.get("brokerEpoch").and_then(Value::as_str) == Some(epoch)
        && response.get("requestNonce").and_then(Value::as_str) == Some(nonce)
        && response.get("operation").and_then(Value::as_str) == Some(operation)
        && response.get("messageType").and_then(Value::as_str) == Some("response");
    if !correlated {
        return Err(BrokerFailure::unknown(
            "Uncorrelated broker response rejected.",
        ));
    }
    let terminal = response["completed"] == true
        && response["outcome"] == "completed"
        && response["businessAccepted"] == true
        || response["completed"] == false
            && response["businessAccepted"].is_boolean()
            && matches!(
                response["outcome"].as_str(),
                Some("failed" | "cancelled" | "unknown")
            )
            && response["error"]["code"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
            && response["error"]["message"].is_string();
    if !terminal {
        return Err(BrokerFailure::unknown(
            "Malformed broker terminal response.",
        ));
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
#[path = "broker_tests.rs"]
mod tests;
