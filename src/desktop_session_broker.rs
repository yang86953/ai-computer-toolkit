//! 跨平台桌面会话的单线程长期 JSONL stdio 宿主。

use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    components::{
        desktop_session_identity,
        desktop_session_input_cancellation::DesktopInputCancellationRegistry,
    },
    domain::AppControlError,
    modules::desktop_session::{
        DesktopAuthorizationScope, DesktopConsent, DesktopSessionAuthorization,
        DesktopSessionModule, DesktopSessionPort, DesktopSessionView,
    },
};

#[cfg(test)]
use crate::modules::desktop_session::DesktopAuthorizationPersistence;

#[cfg(target_os = "windows")]
use crate::adapters::desktop_session_windows::SystemDesktopSessionPort;
#[cfg(target_os = "linux")]
use crate::adapters::linux::desktop_session_portal::SystemDesktopSessionPortal as SystemDesktopSessionPort;

const CONTRACT_VERSION: &str = "act/linux-desktop-session-broker/v1";
const MAXIMUM_FRAME_BYTES: usize = 64 * 1024;
const MAXIMUM_LEDGER_ENTRIES: usize = 1024;
const MAXIMUM_PENDING_FRAMES: usize = 16;

#[cfg(target_os = "linux")]
mod transport;
#[cfg(target_os = "linux")]
pub use transport::{run_socket_client, run_socket_server};

mod request;
use request::BrokerRequest;

mod interaction;

struct LedgerRecord {
    fingerprint: String,
    response: Value,
}

/// 普通账本满后保留独立收尾配额；不清空旧 nonce，也不让查询/取消挤占关闭和退出。
#[derive(Default)]
struct LedgerReserve {
    cancellations: usize,
    closes: usize,
    queries: usize,
    shutdowns: usize,
    subscription_stops: usize,
    authorization_forgets: usize,
}

impl LedgerReserve {
    fn admit(&mut self, request: &BrokerRequest, recorded: usize) -> bool {
        if recorded < MAXIMUM_LEDGER_ENTRIES {
            return true;
        }
        let (used, limit) = match request {
            BrokerRequest::InputCancel { .. } => (&mut self.cancellations, MAXIMUM_PENDING_FRAMES),
            BrokerRequest::Close { .. } => (
                &mut self.closes,
                crate::modules::desktop_session::MAXIMUM_LIVE_SESSIONS,
            ),
            BrokerRequest::Sessions { .. } | BrokerRequest::AuthorizationStatus { .. } => (
                &mut self.queries,
                crate::modules::desktop_session::MAXIMUM_LIVE_SESSIONS,
            ),
            BrokerRequest::Shutdown { .. } => (&mut self.shutdowns, 1),
            BrokerRequest::ObserveUnsubscribe { .. } => (
                &mut self.subscription_stops,
                crate::modules::desktop_session::MAXIMUM_LIVE_SESSIONS,
            ),
            BrokerRequest::ForgetAuthorization { .. } => (
                &mut self.authorization_forgets,
                crate::modules::desktop_session::MAXIMUM_LIVE_SESSIONS,
            ),
            _ => return false,
        };
        if *used >= limit {
            return false;
        }
        *used += 1;
        true
    }
}

#[derive(Default)]
struct RequestSemantics {
    fingerprints: BTreeMap<String, String>,
    reserve: LedgerReserve,
}

/// broker 与 Module 同线程，确保非 Send 的 EIS event stream 不跨线程移动。
struct Broker<P: DesktopSessionPort> {
    epoch: String,
    module: DesktopSessionModule<P>,
    ledger: BTreeMap<String, LedgerRecord>,
    ledger_reserve: LedgerReserve,
    cancellations: Arc<DesktopInputCancellationRegistry>,
}

impl<P: DesktopSessionPort> Broker<P> {
    fn new(epoch: String, module: DesktopSessionModule<P>) -> Self {
        Self {
            epoch,
            module,
            ledger: BTreeMap::new(),
            ledger_reserve: LedgerReserve::default(),
            cancellations: Arc::new(DesktopInputCancellationRegistry::new(
                MAXIMUM_LEDGER_ENTRIES,
            )),
        }
    }

    fn cancellation_registry(&self) -> Arc<DesktopInputCancellationRegistry> {
        Arc::clone(&self.cancellations)
    }

    fn ready(&self) -> Value {
        json!({
            "contractVersion": CONTRACT_VERSION,
            "messageType": "broker-ready",
            "brokerEpoch": self.epoch,
            "ownerState": "live-single-thread",
            "observationModes": ["snapshot", "frame-diff", "region-diff"],
            "frameSubscriptions": cfg!(target_os = "linux"),
            "postInputObservation": true,
            "subscriptionDelivery": "latest-pull",
            "maxTrackedPixels": crate::components::desktop_frame_changes::MAX_TRACKED_PIXELS,
            "transport": "json-lines-stdio",
            "authorizationModes": ["operation", "session"],
            "persistentAuthorizationSupported": P::PERSISTENT_AUTHORIZATION_SUPPORTED,
            "restoreTokenRetained": false,
            "inputEventsSent": 0,
            "pixelsConsumed": 0,
        })
    }

    fn handle(&mut self, request: BrokerRequest) -> (Value, bool) {
        let operation = request.operation();
        let request_nonce = request.request_nonce().to_owned();
        if let Err(error) = request.validate(&self.epoch) {
            return (failure_response(&self.epoch, &request, error), false);
        }
        let fingerprint = match request.fingerprint() {
            Ok(fingerprint) => fingerprint,
            Err(error) => return (failure_response(&self.epoch, &request, error), false),
        };
        if let Some(record) = self.ledger.get(&request_nonce) {
            if record.fingerprint == fingerprint {
                return (record.response.clone(), operation == "shutdown");
            }
            return (
                failure_response(
                    &self.epoch,
                    &request,
                    AppControlError::with_details(
                        "NONCE_SEMANTIC_CONFLICT",
                        "The request nonce was already bound to different semantics.",
                        json!({
                            "outcome": "failed",
                            "acceptedMayHaveOccurred": false,
                            "retrySafe": false,
                            "automaticRetryProhibited": true,
                        }),
                    ),
                ),
                false,
            );
        }
        if !self.ledger_reserve.admit(&request, self.ledger.len()) {
            return (
                failure_response(
                    &self.epoch,
                    &request,
                    AppControlError::with_details(
                        "BROKER_REQUEST_LEDGER_FULL",
                        "The desktop session broker request ledger is full.",
                        json!({
                            "outcome": "failed",
                            "acceptedMayHaveOccurred": false,
                            "retrySafe": false,
                            "automaticRetryProhibited": true,
                        }),
                    ),
                ),
                false,
            );
        }
        if let Some(target_request_nonce) = request.cancellation_target() {
            self.cancellations.request_cancel(target_request_nonce);
        }
        let shutdown = matches!(&request, BrokerRequest::Shutdown { .. });
        let (response, cancelled) = match self.execute(&request) {
            Ok(data) => (success_response(&self.epoch, &request, data), false),
            Err(error) => {
                let cancelled = error.code == "CANCELLED";
                (failure_response(&self.epoch, &request, error), cancelled)
            }
        };
        if request.is_input() {
            self.cancellations.finish(&request_nonce, cancelled);
        }
        self.ledger.insert(
            request_nonce,
            LedgerRecord {
                fingerprint,
                response: response.clone(),
            },
        );
        (response, shutdown)
    }

    fn execute(&mut self, request: &BrokerRequest) -> Result<Value, AppControlError> {
        match request {
            BrokerRequest::Observe { .. } | BrokerRequest::Interact { .. } => {
                self.execute_interaction(request)
            }
            BrokerRequest::ObserveSubscribe {
                session_id,
                confirmed,
                strict_isolation,
                input,
                ..
            } => self.module.subscribe_observation(
                session_id,
                capture_consent(*confirmed, *strict_isolation),
                input,
            ),
            BrokerRequest::ObserveNext {
                session_id,
                confirmed,
                strict_isolation,
                input,
                ..
            } => self.module.next_observation(
                session_id,
                capture_consent(*confirmed, *strict_isolation),
                input,
            ),
            BrokerRequest::ObserveUnsubscribe {
                session_id, input, ..
            } => self.module.unsubscribe_observation(session_id, input),
            BrokerRequest::Open {
                confirmed,
                foreground_consent,
                strict_isolation,
                timeout_ms,
                authorization_scope,
                remember_authorization,
                ..
            } => {
                let authorization = DesktopSessionAuthorization {
                    scope: match authorization_scope {
                        Some(request::AuthorizationScopeDto::Session) => {
                            DesktopAuthorizationScope::Session
                        }
                        _ => DesktopAuthorizationScope::PerOperation,
                    },
                    remember: remember_authorization.unwrap_or(false),
                };
                let view = self.module.open(
                    *confirmed,
                    *foreground_consent,
                    if *strict_isolation {
                        crate::domain::IsolationRequirement::Strict
                    } else {
                        crate::domain::IsolationRequirement::Standard
                    },
                    Duration::from_millis(u64::from(*timeout_ms)),
                    authorization,
                )?;
                Ok(session_view(&view))
            }
            BrokerRequest::Sessions { .. } => Ok(json!({
                "sessions": self
                    .module
                    .sessions()
                    .iter()
                    .map(session_view)
                    .collect::<Vec<_>>(),
            })),
            BrokerRequest::AuthorizationStatus { .. } => {
                let saved = self.module.saved_authorization();
                Ok(json!({
                    "persistentAuthorizationSupported": P::PERSISTENT_AUTHORIZATION_SUPPORTED,
                    "savedAuthorizationState": saved.state().as_str(),
                    "savedAuthorizationBackend": saved.backend(),
                }))
            }
            BrokerRequest::ForgetAuthorization { .. } => {
                let report = self.module.forget_authorization()?;
                Ok(json!({
                    "hadSavedAuthorization": report.had_saved(),
                    "savedAuthorizationCleared": report.cleared(),
                    "liveSessionsClosed": report.sessions_closed(),
                    "liveSessionsFailedToClose": report.close_failures(),
                    // 本地忘记只清除本工具保存的凭据；系统 Portal 的授权
                    // 记录仍由桌面环境权限管理持有，不谎称全系统已撤销。
                    "revokesSystemPortalRecords": false,
                }))
            }
            BrokerRequest::Inspect { session_id, .. } => {
                Ok(session_view(&self.module.inspect(session_id)?))
            }
            BrokerRequest::Close { session_id, .. } => {
                let report = self.module.close(session_id)?;
                Ok(json!({
                    "sessionId": report.session_id(),
                    "closed": report.completed(),
                    "targetInvalidated": true,
                }))
            }
            BrokerRequest::InputKey {
                session_id,
                confirmed,
                foreground_consent,
                strict_isolation,
                input,
                ..
            } => {
                let cancellation = self.cancellations.prepare_input(request.request_nonce());
                let report = self.module.send_keyboard(
                    session_id,
                    input_consent(*confirmed, *foreground_consent, *strict_isolation),
                    input,
                    &cancellation,
                )?;
                Ok(json!({
                    "sessionId": report.session_id(),
                    "capability": "ui.input.key@3",
                    "completedSteps": report.completed_steps(),
                    "inputEventsSent": report.input_events_sent(),
                    "effectConfirmed": report.effect_confirmed(),
                    "keysHeldAfterReturn": 0,
                }))
            }
            BrokerRequest::InputPointer {
                session_id,
                confirmed,
                foreground_consent,
                strict_isolation,
                input,
                ..
            } => {
                let cancellation = self.cancellations.prepare_input(request.request_nonce());
                let report = self.module.send_pointer(
                    session_id,
                    input_consent(*confirmed, *foreground_consent, *strict_isolation),
                    input,
                    &cancellation,
                )?;
                Ok(json!({
                    "sessionId": report.session_id(),
                    "capability": "ui.input.pointer@3",
                    "coordinateSpace": report.coordinate_space(),
                    "completedSteps": report.completed_steps(),
                    "inputEventsSent": report.input_events_sent(),
                    "effectConfirmed": report.effect_confirmed(),
                    "finalPointerPositionConfirmed": report.final_position_confirmed(),
                    "buttonsHeldAfterReturn": 0,
                }))
            }
            BrokerRequest::CaptureFrame {
                session_id,
                confirmed,
                strict_isolation,
                input,
                ..
            } => {
                let report = self.module.capture_frame(
                    session_id,
                    capture_consent(*confirmed, *strict_isolation),
                    input,
                )?;
                let mut result = json!({
                    "sessionId": report.session_id(),
                    "capability": "screen.capture@1",
                    "path": report.path(),
                    "bytes": report.bytes(),
                    "width": report.width(),
                    "height": report.height(),
                    "pixelDigest": report.pixel_digest(),
                    "atomicOutput": true,
                    "replacedExisting": report.replaced_existing(),
                    "oneShot": true,
                    "systemCaptureIndicatorMayAppear": true,
                });
                // 原有调用保持原结果形状，只有显式图像预算请求才附带采样元数据。
                if input.get("maxDimension").is_some() {
                    result["sourceWidth"] = json!(report.source_size().0);
                    result["sourceHeight"] = json!(report.source_size().1);
                    result["sampling"] = json!(if (report.width(), report.height())
                        == report.source_size()
                    {
                        "identity"
                    } else {
                        "nearest-preview"
                    });
                }
                Ok(result)
            }
            BrokerRequest::InputCancel {
                target_request_nonce,
                ..
            } => Ok(json!({
                "targetRequestNonce": target_request_nonce,
                "status": self.cancellations.status(target_request_nonce).as_str(),
                "terminalForCancelRequest": true,
            })),
            BrokerRequest::Shutdown { .. } => Ok(json!({
                "shutdownAccepted": true,
                "liveSessionsBeforeShutdown": self.module.sessions().len(),
                "cleanupMode": "module-drop-before-process-exit",
            })),
        }
    }
}

/// 输入类请求的确认三元组；`None` 字段由模块按 session 授权继承解析。
fn input_consent(
    confirmed: Option<bool>,
    foreground_consent: Option<bool>,
    strict_isolation: Option<bool>,
) -> DesktopConsent {
    DesktopConsent {
        confirmed,
        foreground_consent,
        strict_isolation,
    }
}

/// 截图/观察类请求的确认字段。
fn capture_consent(confirmed: Option<bool>, strict_isolation: Option<bool>) -> DesktopConsent {
    DesktopConsent {
        confirmed,
        foreground_consent: None,
        strict_isolation,
    }
}

fn session_view(view: &DesktopSessionView) -> Value {
    let facts = view.facts();
    let persistence = facts.authorization_persistence();
    let mut data = json!({
        "sessionId": view.session_id(),
        "targetKind": "desktop-session",
        "live": true,
        "authorizedDeviceClasses": facts.authorized_device_classes(),
        "streamMetadata": {
            "streamCount": facts.stream_count(),
            "mappingIdCount": facts.mapping_id_count(),
        },
        "providerVersions": {
            "remoteDesktop": facts.remote_desktop_version(),
            "screenCast": facts.screen_cast_version(),
        },
        "eisHandshakeComplete": true,
        "pipeWireRemoteObtained": true,
        "authorization": {
            "mode": view.authorization().scope.as_str(),
            "persistence": {
                "requested": persistence.requested,
                "restoreAttempted": persistence.restore_attempted,
                "restoreTokenRetained": persistence.token_retained,
            },
        },
        "restoreTokenRetained": persistence.token_retained,
        "inputEventsSent": view.input_events_sent(),
        "framesCaptured": view.frames_captured(),
        "pixelsConsumed": view.pixels_consumed(),
    });
    data["backend"] = json!(facts.backend());
    if let Some(note) = persistence.note {
        data["authorization"]["persistence"]["note"] = json!(note);
    }
    if facts.backend() != "portal-eis" {
        let object = data.as_object_mut().unwrap();
        object.remove("providerVersions");
        object.remove("eisHandshakeComplete");
        object.remove("pipeWireRemoteObtained");
    }
    data
}

fn success_response(epoch: &str, request: &BrokerRequest, data: Value) -> Value {
    let retry_safe = request.retry_safe_on_success();
    json!({
        "contractVersion": CONTRACT_VERSION,
        "messageType": "response",
        "brokerEpoch": epoch,
        "requestNonce": request.request_nonce(),
        "operation": request.operation(),
        "transportAccepted": true,
        "businessAccepted": true,
        "outcome": "completed",
        "completed": true,
        "retrySafe": retry_safe,
        "automaticRetryProhibited": !retry_safe,
        "acceptedMayHaveOccurred": true,
        "data": data,
        "error": Value::Null,
    })
}

fn failure_response(epoch: &str, request: &BrokerRequest, error: AppControlError) -> Value {
    let accepted = error.details["acceptedMayHaveOccurred"]
        .as_bool()
        .unwrap_or(false);
    let retry_safe = error.details["retrySafe"].as_bool().unwrap_or(false);
    let outcome = error.details["outcome"].as_str().unwrap_or("failed");
    json!({
        "contractVersion": CONTRACT_VERSION,
        "messageType": "response",
        "brokerEpoch": epoch,
        "requestNonce": request.request_nonce(),
        "operation": request.operation(),
        "transportAccepted": true,
        "businessAccepted": accepted,
        "outcome": outcome,
        "completed": false,
        "retrySafe": retry_safe,
        "automaticRetryProhibited": !retry_safe,
        "acceptedMayHaveOccurred": accepted,
        "data": Value::Null,
        "error": {
            "code": error.code,
            "message": error.message,
            "details": error.details,
        },
    })
}

fn protocol_error(reason: &'static str) -> AppControlError {
    AppControlError::with_details(
        "BROKER_PROTOCOL_FAILED",
        "The desktop session broker request violated the protocol.",
        json!({
            "reason": reason,
            "outcome": "failed",
            "acceptedMayHaveOccurred": false,
            "retrySafe": false,
            "automaticRetryProhibited": true,
        }),
    )
}

fn canonical_nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn terminal_error(code: &'static str) -> Value {
    json!({
        "contractVersion": CONTRACT_VERSION,
        "messageType": "broker-error",
        "code": code,
        "terminal": true,
        "retrySafe": false,
        "automaticRetryProhibited": true,
    })
}

fn write_json_line(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    // 先构造完整帧，避免 serde_json 对 socket 的逐 token 小写入；线缆字节与 flush 边界不变。
    let mut frame = serde_json::to_vec(value).map_err(io::Error::other)?;
    frame.push(b'\n');
    writer.write_all(&frame)?;
    writer.flush()
}

/// 逐 chunk 读取单帧，超限时不继续分配并终止整个 broker。
fn read_bounded_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut bytes = Vec::with_capacity(4096);
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            break;
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position.saturating_add(1));
        if bytes.len().saturating_add(take) > MAXIMUM_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "desktop session broker frame exceeds the limit",
            ));
        }
        let complete = available.get(take.saturating_sub(1)) == Some(&b'\n');
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if complete {
            break;
        }
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "broker frame is not UTF-8"))
}

enum InboundFrame {
    Request(BrokerRequest),
    Eof,
}

fn terminal_code(value: u8) -> Option<&'static str> {
    match value {
        1 => Some("BROKER_FRAME_INVALID"),
        2 => Some("BROKER_PROTOCOL_FAILED"),
        3 => Some("BROKER_QUEUE_FULL"),
        _ => None,
    }
}

/// 独立 reader 线程只解析有界帧和置位取消事实，不接触 Module、EIS 或输出。
fn read_inbound_frames(
    reader: &mut impl BufRead,
    sender: &mpsc::SyncSender<InboundFrame>,
    cancellations: &DesktopInputCancellationRegistry,
    terminal: &AtomicU8,
    epoch: &str,
) {
    let mut nonce_semantics = RequestSemantics::default();
    loop {
        let line = match read_bounded_line(reader) {
            Ok(Some(line)) => line,
            Ok(None) => {
                let _ = sender.send(InboundFrame::Eof);
                return;
            }
            Err(_) => {
                terminal.store(1, Ordering::Release);
                cancellations.cancel_all_active();
                return;
            }
        };
        let request = match serde_json::from_str::<BrokerRequest>(&line) {
            Ok(request) => request,
            Err(_) => {
                terminal.store(2, Ordering::Release);
                cancellations.cancel_all_active();
                return;
            }
        };
        prepare_control_request(&request, epoch, &mut nonce_semantics, cancellations);
        match sender.try_send(InboundFrame::Request(request)) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                terminal.store(3, Ordering::Release);
                cancellations.cancel_all_active();
                return;
            }
            Err(mpsc::TrySendError::Disconnected(_)) => return,
        }
    }
}

/// 两种传输共用取消请求的代际、去重与语义预检。
fn prepare_control_request(
    request: &BrokerRequest,
    epoch: &str,
    nonce_semantics: &mut RequestSemantics,
    cancellations: &DesktopInputCancellationRegistry,
) {
    let valid = request.validate(epoch).is_ok()
        && request.fingerprint().is_ok_and(|fingerprint| {
            match nonce_semantics.fingerprints.get(request.request_nonce()) {
                Some(existing) => existing == &fingerprint,
                None if nonce_semantics
                    .reserve
                    .admit(request, nonce_semantics.fingerprints.len()) =>
                {
                    nonce_semantics
                        .fingerprints
                        .insert(request.request_nonce().to_owned(), fingerprint);
                    true
                }
                None => false,
            }
        });
    if valid && let Some(target) = request.cancellation_target() {
        cancellations.request_cancel(target);
    }
}

fn serve_inbound<P: DesktopSessionPort>(
    receiver: &mpsc::Receiver<InboundFrame>,
    writer: &mut impl Write,
    mut broker: Broker<P>,
    terminal: &AtomicU8,
) -> i32 {
    if write_json_line(writer, &broker.ready()).is_err() {
        return 2;
    }
    loop {
        if let Some(code) = terminal_code(terminal.load(Ordering::Acquire)) {
            let _ = write_json_line(writer, &terminal_error(code));
            return 2;
        }
        let request = match receiver.recv() {
            Ok(InboundFrame::Request(request)) => request,
            Ok(InboundFrame::Eof) | Err(_) => return 0,
        };
        if let Some(code) = terminal_code(terminal.load(Ordering::Acquire)) {
            let _ = write_json_line(writer, &terminal_error(code));
            return 2;
        }
        let (response, shutdown) = broker.handle(request);
        if write_json_line(writer, &response).is_err() {
            return 2;
        }
        if shutdown {
            return 0;
        }
    }
}

#[cfg(test)]
fn serve<P: DesktopSessionPort>(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    mut broker: Broker<P>,
) -> i32 {
    if write_json_line(writer, &broker.ready()).is_err() {
        return 2;
    }
    loop {
        let line = match read_bounded_line(reader) {
            Ok(Some(line)) => line,
            Ok(None) => return 0,
            Err(_) => {
                let _ = write_json_line(writer, &terminal_error("BROKER_FRAME_INVALID"));
                return 2;
            }
        };
        let request = match serde_json::from_str::<BrokerRequest>(&line) {
            Ok(request) => request,
            Err(_) => {
                let _ = write_json_line(writer, &terminal_error("BROKER_PROTOCOL_FAILED"));
                return 2;
            }
        };
        let (response, shutdown) = broker.handle(request);
        if write_json_line(writer, &response).is_err() {
            return 2;
        }
        if shutdown {
            return 0;
        }
    }
}

/// 生产 launcher：`ai-computer-toolkit session-host desktop`。
pub fn run_stdio() -> i32 {
    let epoch = match desktop_session_identity::random_nonce() {
        Ok(epoch) => epoch,
        Err(_) => {
            let _ = write_json_line(
                &mut io::stdout().lock(),
                &terminal_error("SESSION_IDENTITY_FAILED"),
            );
            return 2;
        }
    };
    let module = DesktopSessionModule::new(SystemDesktopSessionPort::default());
    let broker = Broker::new(epoch.clone(), module);
    let cancellations = broker.cancellation_registry();
    let terminal = Arc::new(AtomicU8::new(0));
    let reader_terminal = Arc::clone(&terminal);
    let reader_cancellations = Arc::clone(&cancellations);
    let (sender, receiver) = mpsc::sync_channel(MAXIMUM_PENDING_FRAMES);
    std::thread::spawn(move || {
        read_inbound_frames(
            &mut io::stdin().lock(),
            &sender,
            &reader_cancellations,
            &reader_terminal,
            &epoch,
        );
    });
    serve_inbound(&receiver, &mut io::stdout().lock(), broker, &terminal)
}

#[cfg(all(test, target_os = "linux"))]
mod subscription_tests;

#[cfg(all(test, target_os = "linux"))]
mod change_tests;

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::{
        io::BufReader,
        os::unix::net::UnixStream,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Instant,
    };

    use crate::components::desktop_session_input_cancellation::DesktopInputCancellation;
    use crate::modules::desktop_session::{
        DesktopSessionFacts, DesktopSessionLease, DesktopSessionPortFailure,
    };

    use super::*;

    const EPOCH: &str = "11111111111111111111111111111111";

    #[derive(Clone)]
    struct FakePort {
        opens: Arc<AtomicUsize>,
        closes: Arc<AtomicUsize>,
    }

    struct FakeLease(Arc<AtomicUsize>);

    #[derive(Clone)]
    struct BlockingPort {
        entered: Arc<AtomicBool>,
        closes: Arc<AtomicUsize>,
    }

    struct BlockingLease {
        entered: Arc<AtomicBool>,
        closes: Arc<AtomicUsize>,
    }

    fn fixture_frame() -> Result<
        crate::components::desktop_session_frame_capture::DesktopCapturedFrame,
        crate::modules::desktop_session::DesktopSessionFrameFailure,
    > {
        crate::components::desktop_session_frame_capture::encode_mapped_frame(
            &[3, 2, 1, 255],
            0,
            4,
            4,
            (1, 1),
            crate::components::desktop_session_frame_capture::DesktopPackedPixelFormat::Bgra,
            None,
        )
        .map_err(|_| {
            crate::modules::desktop_session::DesktopSessionFrameFailure::new(
                "CAPTURE_READBACK_FAILED",
                "fixture-frame",
                false,
                false,
            )
        })
    }

    impl DesktopSessionLease for FakeLease {
        fn send_keyboard(
            &mut self,
            input: &crate::components::keyboard_input_contract::KeyboardInput,
            cancellation: &DesktopInputCancellation,
        ) -> Result<
            crate::modules::desktop_session::DesktopKeyboardDispatchFacts,
            crate::modules::desktop_session::DesktopSessionInputFailure,
        > {
            if cancellation.is_cancelled() {
                return Err(
                    crate::modules::desktop_session::DesktopSessionInputFailure::cancelled(
                        true, 0, 0,
                    ),
                );
            }
            Ok(
                crate::modules::desktop_session::DesktopKeyboardDispatchFacts::new(
                    input.steps.len(),
                    input.steps.len().saturating_mul(2),
                ),
            )
        }

        fn send_pointer(
            &mut self,
            input: &crate::components::desktop_session_pointer_input::DesktopPointerInput,
            cancellation: &DesktopInputCancellation,
        ) -> Result<
            crate::modules::desktop_session::DesktopPointerDispatchFacts,
            crate::modules::desktop_session::DesktopSessionInputFailure,
        > {
            if cancellation.is_cancelled() {
                return Err(
                    crate::modules::desktop_session::DesktopSessionInputFailure::cancelled(
                        true, 0, 0,
                    ),
                );
            }
            Ok(
                crate::modules::desktop_session::DesktopPointerDispatchFacts::new(
                    input.steps.len(),
                    input.steps.len(),
                ),
            )
        }

        fn capture_frame(
            &mut self,
            _: Duration,
            _: Option<u32>,
        ) -> Result<
            crate::components::desktop_session_frame_capture::DesktopCapturedFrame,
            crate::modules::desktop_session::DesktopSessionFrameFailure,
        > {
            fixture_frame()
        }

        fn close(self: Box<Self>) -> Result<(), DesktopSessionPortFailure> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    impl DesktopSessionPort for FakePort {
        fn open(
            &self,
            _: Duration,
            _: DesktopAuthorizationPersistence,
        ) -> Result<(Box<dyn DesktopSessionLease>, DesktopSessionFacts), DesktopSessionPortFailure>
        {
            self.opens.fetch_add(1, Ordering::Relaxed);
            Ok((
                Box::new(FakeLease(Arc::clone(&self.closes))),
                DesktopSessionFacts::new(2, 5, vec!["keyboard", "pointer"], 1, 1),
            ))
        }
    }

    impl DesktopSessionLease for BlockingLease {
        fn send_keyboard(
            &mut self,
            _: &crate::components::keyboard_input_contract::KeyboardInput,
            cancellation: &DesktopInputCancellation,
        ) -> Result<
            crate::modules::desktop_session::DesktopKeyboardDispatchFacts,
            crate::modules::desktop_session::DesktopSessionInputFailure,
        > {
            self.entered.store(true, Ordering::Release);
            let deadline = Instant::now() + Duration::from_secs(2);
            while !cancellation.is_cancelled() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            if cancellation.is_cancelled() {
                Err(
                    crate::modules::desktop_session::DesktopSessionInputFailure::cancelled(
                        true, 0, 0,
                    ),
                )
            } else {
                Err(
                    crate::modules::desktop_session::DesktopSessionInputFailure::before_dispatch(
                        "TIMEOUT",
                        "fake-input-wait",
                    ),
                )
            }
        }

        fn send_pointer(
            &mut self,
            _: &crate::components::desktop_session_pointer_input::DesktopPointerInput,
            cancellation: &DesktopInputCancellation,
        ) -> Result<
            crate::modules::desktop_session::DesktopPointerDispatchFacts,
            crate::modules::desktop_session::DesktopSessionInputFailure,
        > {
            self.entered.store(true, Ordering::Release);
            while !cancellation.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(crate::modules::desktop_session::DesktopSessionInputFailure::cancelled(true, 0, 0))
        }

        fn capture_frame(
            &mut self,
            _: Duration,
            _: Option<u32>,
        ) -> Result<
            crate::components::desktop_session_frame_capture::DesktopCapturedFrame,
            crate::modules::desktop_session::DesktopSessionFrameFailure,
        > {
            fixture_frame()
        }

        fn close(self: Box<Self>) -> Result<(), DesktopSessionPortFailure> {
            self.closes.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    impl DesktopSessionPort for BlockingPort {
        fn open(
            &self,
            _: Duration,
            _: DesktopAuthorizationPersistence,
        ) -> Result<(Box<dyn DesktopSessionLease>, DesktopSessionFacts), DesktopSessionPortFailure>
        {
            Ok((
                Box::new(BlockingLease {
                    entered: Arc::clone(&self.entered),
                    closes: Arc::clone(&self.closes),
                }),
                DesktopSessionFacts::new(2, 5, vec!["keyboard", "pointer"], 1, 1),
            ))
        }
    }

    fn broker() -> (Broker<FakePort>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let opens = Arc::new(AtomicUsize::new(0));
        let closes = Arc::new(AtomicUsize::new(0));
        let port = FakePort {
            opens: Arc::clone(&opens),
            closes: Arc::clone(&closes),
        };
        (
            Broker::new(EPOCH.to_owned(), DesktopSessionModule::new(port)),
            opens,
            closes,
        )
    }

    fn open_request(nonce: &str) -> BrokerRequest {
        BrokerRequest::Open {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: nonce.to_owned(),
            confirmed: true,
            foreground_consent: true,
            strict_isolation: false,
            timeout_ms: 120_000,
            authorization_scope: None,
            remember_authorization: None,
        }
    }

    fn simple_request(operation: &str, nonce: usize) -> BrokerRequest {
        serde_json::from_value(json!({
            "contractVersion": CONTRACT_VERSION, "brokerEpoch": EPOCH,
            "requestNonce": format!("{nonce:032x}"), "operation": operation,
        }))
        .unwrap_or_else(|error| panic!("fixture request: {error}"))
    }

    #[test]
    fn full_ledger_keeps_close_receipts_and_shutdown_available() {
        let (mut broker, opens, closes) = broker();
        let mut sessions = Vec::new();
        for nonce in 0..8 {
            let (opened, _) = broker.handle(open_request(&format!("{nonce:032x}")));
            sessions.push(opened["data"]["sessionId"].as_str().unwrap().to_owned());
        }
        for nonce in 8..MAXIMUM_LEDGER_ENTRIES {
            assert_eq!(
                broker.handle(simple_request("sessions", nonce)).0["completed"],
                true
            );
        }
        let (rejected, _) = broker.handle(open_request(&format!("{:032x}", 2000)));
        assert_eq!(rejected["error"]["code"], "BROKER_REQUEST_LEDGER_FULL");
        assert_eq!(rejected["acceptedMayHaveOccurred"], false);
        assert_eq!(opens.load(Ordering::Relaxed), 8);
        for (index, session_id) in sessions.into_iter().enumerate() {
            let request = BrokerRequest::Close {
                contract_version: CONTRACT_VERSION.to_owned(),
                broker_epoch: EPOCH.to_owned(),
                request_nonce: format!("{:032x}", 2100 + index),
                session_id,
            };
            let (closed, _) = broker.handle(request.clone());
            assert_eq!(
                closed["completed"], true,
                "close must survive ordinary ledger saturation"
            );
            assert_eq!(closed["data"]["closed"], true);
            assert_eq!(broker.handle(request.clone()).0, closed);
            let conflict = simple_request("shutdown", 2100 + index);
            assert_eq!(conflict.request_nonce(), request.request_nonce());
            let (conflicted, shutdown) = broker.handle(conflict);
            assert!(!shutdown);
            assert_eq!(conflicted["error"]["code"], "NONCE_SEMANTIC_CONFLICT");
        }
        assert_eq!(closes.load(Ordering::Relaxed), 8);
        let (listed, _) = broker.handle(simple_request("sessions", 2200));
        assert_eq!(listed["data"]["sessions"], json!([]));
        assert_eq!(listed["completed"], true);
        let (shutdown, exit) = broker.handle(simple_request("shutdown", 2201));
        assert!(exit);
        assert_eq!(shutdown["completed"], true);
        assert_eq!(shutdown["data"]["liveSessionsBeforeShutdown"], 0);
        drop(broker);
        assert_eq!(closes.load(Ordering::Relaxed), 8);
    }

    #[test]
    fn full_ledger_reserves_are_independent_bounded_and_keep_nonce_conflicts() {
        let (mut broker, _, closes) = broker();
        let (opened, _) = broker.handle(open_request(&format!("{:032x}", 0)));
        let session_id = opened["data"]["sessionId"].as_str().unwrap().to_owned();
        for nonce in 1..MAXIMUM_LEDGER_ENTRIES {
            broker.handle(simple_request("sessions", nonce));
        }
        // Exhausting the observation reserve cannot consume close/cancel/shutdown slots.
        for nonce in 2000..2008 {
            assert_eq!(
                broker.handle(simple_request("sessions", nonce)).0["completed"],
                true
            );
        }
        assert_eq!(
            broker.handle(simple_request("sessions", 2008)).0["error"]["code"],
            "BROKER_REQUEST_LEDGER_FULL"
        );
        for nonce in 2100..2108 {
            let (response, _) = broker.handle(BrokerRequest::Close {
                contract_version: CONTRACT_VERSION.to_owned(),
                broker_epoch: EPOCH.to_owned(),
                request_nonce: format!("{nonce:032x}"),
                session_id: session_id.clone(),
            });
            assert_ne!(response["error"]["code"], "BROKER_REQUEST_LEDGER_FULL");
        }
        assert_eq!(closes.load(Ordering::Relaxed), 1);
        let cancellations = broker.cancellation_registry();
        let first = cancellations.prepare_input(&format!("{:032x}", 9000));
        let second = cancellations.prepare_input(&format!("{:032x}", 9001));
        for nonce in 2200..2216 {
            let (response, _) = broker.handle(BrokerRequest::InputCancel {
                contract_version: CONTRACT_VERSION.to_owned(),
                broker_epoch: EPOCH.to_owned(),
                request_nonce: format!("{nonce:032x}"),
                target_request_nonce: format!("{:032x}", 9000),
            });
            assert_eq!(response["completed"], true);
        }
        assert!(first.is_cancelled());
        for nonce in [2200, 2216] {
            let (response, _) = broker.handle(BrokerRequest::InputCancel {
                contract_version: CONTRACT_VERSION.to_owned(),
                broker_epoch: EPOCH.to_owned(),
                request_nonce: format!("{nonce:032x}"),
                target_request_nonce: format!("{:032x}", 9001),
            });
            assert_eq!(
                response["error"]["code"],
                if nonce == 2200 {
                    "NONCE_SEMANTIC_CONFLICT"
                } else {
                    "BROKER_REQUEST_LEDGER_FULL"
                }
            );
        }
        assert!(!second.is_cancelled());
        let (response, shutdown) = broker.handle(simple_request("shutdown", 2300));
        assert!(shutdown);
        assert_eq!(response["completed"], true);
        assert_eq!(broker.handle(simple_request("shutdown", 2300)).0, response);
        assert_eq!(
            broker.ledger.len(),
            MAXIMUM_LEDGER_ENTRIES + MAXIMUM_PENDING_FRAMES + 8 + 8 + 1
        );
    }

    #[test]
    fn reader_reserves_preserve_in_flight_cancel_and_reject_conflicting_intent() {
        let registry = DesktopInputCancellationRegistry::new(MAXIMUM_LEDGER_ENTRIES);
        let mut semantics = RequestSemantics::default();
        for nonce in 0..MAXIMUM_LEDGER_ENTRIES + 8 {
            prepare_control_request(
                &simple_request("sessions", nonce),
                EPOCH,
                &mut semantics,
                &registry,
            );
        }
        let first = registry.prepare_input(&format!("{:032x}", 9000));
        let second = registry.prepare_input(&format!("{:032x}", 9001));
        let cancel = BrokerRequest::InputCancel {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: format!("{:032x}", 2000),
            target_request_nonce: format!("{:032x}", 9000),
        };
        prepare_control_request(&cancel, EPOCH, &mut semantics, &registry);
        assert!(first.is_cancelled());
        let conflicting = BrokerRequest::InputCancel {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: format!("{:032x}", 2000),
            target_request_nonce: format!("{:032x}", 9001),
        };
        prepare_control_request(&conflicting, EPOCH, &mut semantics, &registry);
        assert!(!second.is_cancelled());
        // Replays do not consume any more of the bounded overflow reserve.
        for _ in 0..100 {
            prepare_control_request(&cancel, EPOCH, &mut semantics, &registry);
        }
        assert_eq!(semantics.reserve.cancellations, 1);
        assert_eq!(semantics.fingerprints.len(), MAXIMUM_LEDGER_ENTRIES + 8 + 1);
    }

    #[test]
    fn duplicate_open_nonce_replays_without_second_portal_dispatch() {
        let (mut broker, opens, closes) = broker();
        let request = open_request("22222222222222222222222222222222");
        let (first, _) = broker.handle(request.clone());
        let (replayed, _) = broker.handle(request);
        assert_eq!(first, replayed);
        assert_eq!(opens.load(Ordering::Relaxed), 1);
        assert!(first["data"]["sessionId"].as_str().is_some());
        drop(broker);
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    fn scoped_open_request(
        nonce: &str,
        scope: Option<&str>,
        remember: Option<bool>,
    ) -> BrokerRequest {
        serde_json::from_value(json!({
            "contractVersion": CONTRACT_VERSION, "brokerEpoch": EPOCH,
            "requestNonce": nonce, "operation": "open",
            "confirmed": true, "foregroundConsent": true, "strictIsolation": false,
            "timeoutMs": 120_000,
            "authorizationScope": scope,
            "rememberAuthorization": remember,
        }))
        .unwrap_or_else(|error| panic!("scoped open fixture: {error}"))
    }

    fn key_request(
        nonce: &str,
        session_id: &str,
        consent: Option<(bool, bool, bool)>,
    ) -> BrokerRequest {
        let mut request = json!({
            "contractVersion": CONTRACT_VERSION, "brokerEpoch": EPOCH,
            "requestNonce": nonce, "operation": "input-key",
            "sessionId": session_id,
            "input": {"steps": [{"type": "key", "key": "enter"}]},
        });
        if let Some((confirmed, foreground, strict)) = consent {
            request["confirmed"] = json!(confirmed);
            request["foregroundConsent"] = json!(foreground);
            request["strictIsolation"] = json!(strict);
        }
        serde_json::from_value(request).unwrap_or_else(|error| panic!("key fixture: {error}"))
    }

    #[test]
    fn session_scope_open_inherits_omitted_consent_for_input() {
        let schema: Value = serde_json::from_str(include_str!(
            "../contracts/v1/linux-desktop-session-broker-v1.schema.json"
        ))
        .unwrap();
        let (mut broker, _, _) = broker();
        let (opened, _) = broker.handle(scoped_open_request(
            "31313131313131313131313131313131",
            Some("session"),
            None,
        ));
        jsonschema::draft202012::validate(&schema, &opened).unwrap();
        let session_id = opened["data"]["sessionId"].as_str().unwrap().to_owned();
        assert_eq!(opened["data"]["authorization"]["mode"], "session");
        assert_eq!(opened["data"]["restoreTokenRetained"], false);
        // 省略全部确认字段：继承 open 处的一次确认。
        let (inherited, _) = broker.handle(key_request(
            "32323232323232323232323232323232",
            &session_id,
            None,
        ));
        jsonschema::draft202012::validate(&schema, &inherited).unwrap();
        assert_eq!(inherited["completed"], true, "{inherited}");
        // 显式拒绝不能被继承覆盖。
        let (refused, _) = broker.handle(key_request(
            "33333333333333333333333333333333",
            &session_id,
            Some((false, true, false)),
        ));
        assert_eq!(refused["error"]["code"], "CONFIRMATION_REQUIRED");
        assert_eq!(refused["error"]["details"]["inputAttempted"], false);
        // 缺省（逐操作）作用域保持旧显式要求。
        let (plain, _) = broker.handle(open_request("34343434343434343434343434343434"));
        let plain_id = plain["data"]["sessionId"].as_str().unwrap().to_owned();
        assert_eq!(plain["data"]["authorization"]["mode"], "operation");
        let (omitted, _) = broker.handle(key_request(
            "35353535353535353535353535353535",
            &plain_id,
            None,
        ));
        assert_eq!(omitted["error"]["code"], "CONFIRMATION_REQUIRED");
        assert_eq!(
            omitted["error"]["details"]["confirmationFieldsOmitted"],
            true
        );
        let (explicit, _) = broker.handle(key_request(
            "36363636363636363636363636363636",
            &plain_id,
            Some((true, true, false)),
        ));
        assert_eq!(explicit["completed"], true, "{explicit}");
    }

    #[test]
    fn remember_on_unsupported_backend_is_refused_before_portal_dispatch() {
        let (mut broker, opens, _) = broker();
        let (refused, _) = broker.handle(scoped_open_request(
            "37373737373737373737373737373737",
            Some("session"),
            Some(true),
        ));
        assert_eq!(
            refused["error"]["code"],
            "DESKTOP_AUTHORIZATION_PERSISTENCE_UNSUPPORTED"
        );
        assert_eq!(refused["error"]["details"]["portalRequestIssued"], false);
        assert_eq!(opens.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn authorization_status_and_forget_round_trip_and_close_live_sessions() {
        let schema: Value = serde_json::from_str(include_str!(
            "../contracts/v1/linux-desktop-session-broker-v1.schema.json"
        ))
        .unwrap();
        let (mut broker, _, closes) = broker();
        let (opened, _) = broker.handle(open_request("38383838383838383838383838383838"));
        let session_id = opened["data"]["sessionId"].as_str().unwrap().to_owned();
        let (status, _) = broker.handle(simple_request("authorization-status", 3900));
        jsonschema::draft202012::validate(&schema, &status).unwrap();
        assert_eq!(status["completed"], true, "{status}");
        assert_eq!(status["data"]["persistentAuthorizationSupported"], false);
        assert_eq!(status["data"]["savedAuthorizationState"], "unsupported");
        assert_eq!(status["retrySafe"], true);
        let (forget, _) = broker.handle(simple_request("forget-authorization", 3910));
        jsonschema::draft202012::validate(&schema, &forget).unwrap();
        assert_eq!(forget["completed"], true, "{forget}");
        assert_eq!(forget["data"]["hadSavedAuthorization"], false);
        assert_eq!(forget["data"]["savedAuthorizationCleared"], true);
        assert_eq!(forget["data"]["liveSessionsClosed"], 1);
        assert_eq!(forget["data"]["liveSessionsFailedToClose"], 0);
        assert_eq!(forget["data"]["revokesSystemPortalRecords"], false);
        // 撤销停止本客户端 live 会话：原会话立即 stale。
        let (stale, _) = broker.handle(BrokerRequest::Inspect {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "39393939393939393939393939393939".to_owned(),
            session_id,
        });
        assert_eq!(stale["error"]["code"], "STALE_SESSION");
        assert_eq!(closes.load(Ordering::Relaxed), 1);
        // 相同 nonce 重放得到原响应；重放不重复收尾。
        let (replayed, _) = broker.handle(simple_request("forget-authorization", 3910));
        assert_eq!(forget, replayed);
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn session_views_keep_restore_token_material_out_of_public_frames() {
        let (mut broker, _, _) = broker();
        let (opened, _) = broker.handle(open_request("40404040404040404040404040404040"));
        let (listed, _) = broker.handle(simple_request("sessions", 4100));
        // 公开帧只包含脱敏布尔与计数；任何 token 形态的字符串都不应出现。
        let serialized = listed.to_string();
        assert_eq!(opened["data"]["restoreTokenRetained"], false);
        assert!(serialized.contains("authorization"));
        for forbidden in ["restoreToken\":", "restore_token", "token\":"] {
            assert!(
                !serialized.contains(forbidden),
                "public frame must not carry token material: {serialized}"
            );
        }
    }

    #[test]
    fn open_inspect_close_uses_same_live_owner_generation() {
        let (mut broker, opens, closes) = broker();
        let (opened, _) = broker.handle(open_request("33333333333333333333333333333333"));
        let session_id = opened["data"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("open response must include opaque session"))
            .to_owned();
        let (inspected, _) = broker.handle(BrokerRequest::Inspect {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "44444444444444444444444444444444".to_owned(),
            session_id: session_id.clone(),
        });
        assert_eq!(inspected["data"]["sessionId"], session_id);
        let input_request = BrokerRequest::InputKey {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            session_id: session_id.clone(),
            confirmed: Some(true),
            foreground_consent: Some(true),
            strict_isolation: Some(false),
            input: json!({"steps": [{"type": "key", "key": "enter"}]}),
        };
        let (input, _) = broker.handle(input_request.clone());
        let (replayed, _) = broker.handle(input_request);
        assert_eq!(input, replayed);
        assert_eq!(input["data"]["capability"], "ui.input.key@3");
        assert_eq!(input["data"]["inputEventsSent"], 2);
        assert_eq!(input["data"]["keysHeldAfterReturn"], 0);
        assert_eq!(input["retrySafe"], false);
        let pointer_request = BrokerRequest::InputPointer {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
            session_id: session_id.clone(),
            confirmed: Some(true),
            foreground_consent: Some(true),
            strict_isolation: Some(false),
            input: json!({
                "coordinateSpace": "relative-logical-px",
                "steps": [{"type": "move", "delta": {"x": 8, "y": -3}}]
            }),
        };
        let (pointer, _) = broker.handle(pointer_request.clone());
        let (pointer_replayed, _) = broker.handle(pointer_request);
        assert_eq!(pointer, pointer_replayed);
        assert_eq!(pointer["data"]["capability"], "ui.input.pointer@3");
        assert_eq!(pointer["data"]["coordinateSpace"], "relative-logical-px");
        assert_eq!(pointer["data"]["inputEventsSent"], 1);
        assert_eq!(pointer["data"]["buttonsHeldAfterReturn"], 0);
        assert_eq!(pointer["data"]["finalPointerPositionConfirmed"], false);
        assert_eq!(pointer["retrySafe"], false);
        let directory = std::env::temp_dir().join(format!(
            "act-broker-frame-{}-{}",
            std::process::id(),
            &session_id[session_id.len().saturating_sub(8)..]
        ));
        std::fs::create_dir(&directory)
            .unwrap_or_else(|error| panic!("建立 broker 单帧目录失败：{error}"));
        let destination = directory.join("frame.png");
        let capture_request = BrokerRequest::CaptureFrame {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "cccccccccccccccccccccccccccccccc".to_owned(),
            session_id: session_id.clone(),
            confirmed: Some(true),
            strict_isolation: Some(false),
            input: json!({"path": destination.to_string_lossy(), "timeoutMs": 1000}),
        };
        let (capture, _) = broker.handle(capture_request.clone());
        let (capture_replayed, _) = broker.handle(capture_request);
        assert_eq!(capture, capture_replayed);
        assert_eq!(capture["data"]["capability"], "screen.capture@1");
        assert_eq!(capture["data"]["oneShot"], true);
        assert_eq!(capture["retrySafe"], false);
        assert!(destination.is_file());
        let (after_capture, _) = broker.handle(BrokerRequest::Inspect {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "dddddddddddddddddddddddddddddddd".to_owned(),
            session_id: session_id.clone(),
        });
        assert_eq!(after_capture["data"]["framesCaptured"], 1);
        assert_eq!(after_capture["data"]["pixelsConsumed"], 1);
        std::fs::remove_file(&destination)
            .unwrap_or_else(|error| panic!("清理 broker 单帧失败：{error}"));
        std::fs::remove_dir(&directory)
            .unwrap_or_else(|error| panic!("清理 broker 单帧目录失败：{error}"));
        let (closed, _) = broker.handle(BrokerRequest::Close {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "55555555555555555555555555555555".to_owned(),
            session_id: session_id.clone(),
        });
        assert_eq!(closed["data"]["closed"], true);
        let (stale, _) = broker.handle(BrokerRequest::Inspect {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "66666666666666666666666666666666".to_owned(),
            session_id,
        });
        assert_eq!(stale["error"]["code"], "STALE_SESSION");
        assert_eq!(opens.load(Ordering::Relaxed), 1);
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cancel_before_target_installs_a_tombstone_and_replays_exactly() {
        let (mut broker, _, closes) = broker();
        let (opened, _) = broker.handle(open_request("10101010101010101010101010101010"));
        let session_id = opened["data"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("open response must include opaque session"))
            .to_owned();
        let cancel = BrokerRequest::InputCancel {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "20202020202020202020202020202020".to_owned(),
            target_request_nonce: "30303030303030303030303030303030".to_owned(),
        };
        let (first_cancel, _) = broker.handle(cancel.clone());
        assert_eq!(first_cancel["data"]["status"], "unknown-request");
        assert_eq!(first_cancel["retrySafe"], true);

        let (cancelled, _) = broker.handle(BrokerRequest::InputKey {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "30303030303030303030303030303030".to_owned(),
            session_id,
            confirmed: Some(true),
            foreground_consent: Some(true),
            strict_isolation: Some(false),
            input: json!({"steps": [{"type": "key", "key": "enter"}]}),
        });
        assert_eq!(cancelled["error"]["code"], "CANCELLED");
        assert_eq!(cancelled["outcome"], "cancelled");
        assert_eq!(cancelled["acceptedMayHaveOccurred"], false);
        assert_eq!(cancelled["error"]["details"]["targetInvalidated"], true);

        let (replayed, _) = broker.handle(cancel);
        assert_eq!(replayed, first_cancel);
        let (final_cancel, _) = broker.handle(BrokerRequest::InputCancel {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "40404040404040404040404040404040".to_owned(),
            target_request_nonce: "30303030303030303030303030303030".to_owned(),
        });
        assert_eq!(final_cancel["data"]["status"], "cancelled");
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cancel_after_input_completion_is_too_late() {
        let (mut broker, _, closes) = broker();
        let (opened, _) = broker.handle(open_request("51515151515151515151515151515151"));
        let session_id = opened["data"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("open response must include opaque session"))
            .to_owned();
        let (input, _) = broker.handle(BrokerRequest::InputPointer {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "52525252525252525252525252525252".to_owned(),
            session_id,
            confirmed: Some(true),
            foreground_consent: Some(true),
            strict_isolation: Some(false),
            input: json!({
                "coordinateSpace": "relative-logical-px",
                "steps": [{"type": "move", "delta": {"x": 1, "y": 1}}]
            }),
        });
        assert_eq!(input["outcome"], "completed");
        let (cancel, _) = broker.handle(BrokerRequest::InputCancel {
            contract_version: CONTRACT_VERSION.to_owned(),
            broker_epoch: EPOCH.to_owned(),
            request_nonce: "53535353535353535353535353535353".to_owned(),
            target_request_nonce: "52525252525252525252525252525252".to_owned(),
        });
        assert_eq!(cancel["data"]["status"], "too-late");
        drop(broker);
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn reader_side_cancel_interrupts_an_inflight_owner_thread_input() {
        let entered = Arc::new(AtomicBool::new(false));
        let closes = Arc::new(AtomicUsize::new(0));
        let port = BlockingPort {
            entered: Arc::clone(&entered),
            closes: Arc::clone(&closes),
        };
        let mut broker = Broker::new(EPOCH.to_owned(), DesktopSessionModule::new(port));
        let (opened, _) = broker.handle(open_request("61616161616161616161616161616161"));
        let session_id = opened["data"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("open response must include opaque session"))
            .to_owned();
        let cancellations = broker.cancellation_registry();
        let terminal = Arc::new(AtomicU8::new(0));
        let (sender, receiver) = mpsc::sync_channel(MAXIMUM_PENDING_FRAMES);
        let (read_end, mut write_end) =
            UnixStream::pair().unwrap_or_else(|error| panic!("Unix stream pair failed: {error}"));
        let reader_terminal = Arc::clone(&terminal);
        let reader_cancellations = Arc::clone(&cancellations);
        let reader = std::thread::spawn(move || {
            read_inbound_frames(
                &mut BufReader::new(read_end),
                &sender,
                &reader_cancellations,
                &reader_terminal,
                EPOCH,
            );
        });
        let writer = std::thread::spawn(move || {
            let input = json!({
                "contractVersion": CONTRACT_VERSION,
                "brokerEpoch": EPOCH,
                "requestNonce": "62626262626262626262626262626262",
                "operation": "input-key",
                "sessionId": session_id,
                "confirmed": true,
                "foregroundConsent": true,
                "strictIsolation": false,
                "input": {"steps": [{"type": "key", "key": "enter", "holdMs": 1000}]}
            });
            write_json_line(&mut write_end, &input)
                .unwrap_or_else(|error| panic!("input frame write failed: {error}"));
            let deadline = Instant::now() + Duration::from_secs(1);
            while !entered.load(Ordering::Acquire) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(entered.load(Ordering::Acquire));
            write_json_line(
                &mut write_end,
                &json!({
                    "contractVersion": CONTRACT_VERSION,
                    "brokerEpoch": EPOCH,
                    "requestNonce": "63636363636363636363636363636363",
                    "operation": "input-cancel",
                    "targetRequestNonce": "62626262626262626262626262626262"
                }),
            )
            .unwrap_or_else(|error| panic!("cancel frame write failed: {error}"));
            write_json_line(
                &mut write_end,
                &json!({
                    "contractVersion": CONTRACT_VERSION,
                    "brokerEpoch": EPOCH,
                    "requestNonce": "64646464646464646464646464646464",
                    "operation": "shutdown"
                }),
            )
            .unwrap_or_else(|error| panic!("shutdown frame write failed: {error}"));
        });
        let mut output = Vec::new();
        let exit = serve_inbound(&receiver, &mut output, broker, &terminal);
        writer
            .join()
            .unwrap_or_else(|_| panic!("writer thread must not panic"));
        reader
            .join()
            .unwrap_or_else(|_| panic!("reader thread must not panic"));
        assert_eq!(exit, 0);
        let frames = String::from_utf8(output)
            .unwrap_or_else(|error| panic!("broker output must be UTF-8: {error}"))
            .lines()
            .map(|line| {
                serde_json::from_str::<Value>(line)
                    .unwrap_or_else(|error| panic!("broker frame must be JSON: {error}"))
            })
            .collect::<Vec<_>>();
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0]["messageType"], "broker-ready");
        assert_eq!(frames[1]["error"]["code"], "CANCELLED");
        assert_eq!(frames[1]["outcome"], "cancelled");
        assert_eq!(frames[2]["data"]["status"], "cancelled");
        assert_eq!(frames[3]["data"]["shutdownAccepted"], true);
        assert_eq!(closes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn strict_unknown_field_and_oversized_frame_fail_closed() {
        let unknown = format!(
            "{{\"operation\":\"sessions\",\"contractVersion\":\"{CONTRACT_VERSION}\",\"brokerEpoch\":\"{EPOCH}\",\"requestNonce\":\"77777777777777777777777777777777\",\"nativePath\":\"private\"}}"
        );
        assert!(serde_json::from_str::<BrokerRequest>(&unknown).is_err());
        let oversized = format!("{}\n", "x".repeat(MAXIMUM_FRAME_BYTES));
        let Err(error) = read_bounded_line(&mut io::Cursor::new(oversized)) else {
            panic!("oversized line including delimiter must fail");
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn compact_interaction_replays_without_resending_and_rejects_nonce_conflict() {
        let (mut broker, _, _) = broker();
        let (opened, _) = broker.handle(open_request("11111111111111111111111111111111"));
        let id = opened["data"]["sessionId"].as_str().unwrap().to_owned();
        let value = json!({"contractVersion":CONTRACT_VERSION,"brokerEpoch":EPOCH,"requestNonce":"22222222222222222222222222222222","operation":"interact","sessionId":id,"confirmed":true,"foregroundConsent":true,"strictIsolation":false,"input":{"steps":[{"type":"text","text":"ab"}]}});
        let request: BrokerRequest = serde_json::from_value(value.clone()).unwrap();
        assert!(request.is_input());
        let (first, _) = broker.handle(request.clone());
        let (second, _) = broker.handle(request);
        assert_eq!(first, second);
        assert_eq!(first["data"]["completedSteps"], 1);
        assert_eq!(broker.module.inspect(&id).unwrap().input_events_sent(), 4);
        let mut conflicting = value;
        conflicting["input"]["steps"][0]["text"] = json!("different");
        let (response, _) = broker.handle(serde_json::from_value(conflicting).unwrap());
        assert_eq!(response["completed"], false);
        assert_eq!(broker.module.inspect(&id).unwrap().input_events_sent(), 4);
    }

    struct FailedCapturePort;
    struct FailedCaptureLease(FakeLease);
    impl DesktopSessionPort for FailedCapturePort {
        fn open(
            &self,
            _: Duration,
            _: DesktopAuthorizationPersistence,
        ) -> Result<(Box<dyn DesktopSessionLease>, DesktopSessionFacts), DesktopSessionPortFailure>
        {
            Ok((
                Box::new(FailedCaptureLease(FakeLease(Arc::new(AtomicUsize::new(0))))),
                DesktopSessionFacts::new(2, 5, vec!["keyboard", "pointer"], 1, 1),
            ))
        }
    }
    impl DesktopSessionLease for FailedCaptureLease {
        fn send_keyboard(
            &mut self,
            input: &crate::components::keyboard_input_contract::KeyboardInput,
            cancellation: &DesktopInputCancellation,
        ) -> Result<
            crate::modules::desktop_session::DesktopKeyboardDispatchFacts,
            crate::modules::desktop_session::DesktopSessionInputFailure,
        > {
            self.0.send_keyboard(input, cancellation)
        }
        fn send_pointer(
            &mut self,
            input: &crate::components::desktop_session_pointer_input::DesktopPointerInput,
            cancellation: &DesktopInputCancellation,
        ) -> Result<
            crate::modules::desktop_session::DesktopPointerDispatchFacts,
            crate::modules::desktop_session::DesktopSessionInputFailure,
        > {
            self.0.send_pointer(input, cancellation)
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
                    "CAPTURE_READBACK_FAILED",
                    "fixture",
                    false,
                    false,
                ),
            )
        }
        fn close(self: Box<Self>) -> Result<(), DesktopSessionPortFailure> {
            Ok(())
        }
    }

    #[test]
    fn combined_capture_failure_retains_completed_action_and_forbids_replay() {
        let mut broker = Broker::new(
            EPOCH.to_owned(),
            DesktopSessionModule::new(FailedCapturePort),
        );
        let (opened, _) = broker.handle(open_request("11111111111111111111111111111111"));
        let id = opened["data"]["sessionId"].as_str().unwrap();
        let path = std::env::temp_dir().join(format!(
            "act-failed-{}.png",
            desktop_session_identity::random_nonce().unwrap()
        ));
        let request = combined_request(id, &path);
        let (result, _) = broker.handle(request.clone());
        assert_eq!(result["completed"], false);
        assert_eq!(result["error"]["details"]["failedStage"], "observation");
        assert_eq!(result["error"]["details"]["interactionCompleted"], true);
        assert_eq!(
            result["error"]["details"]["interaction"]["inputEventsSent"],
            4
        );
        assert_eq!(result["acceptedMayHaveOccurred"], true);
        assert_eq!(result["automaticRetryProhibited"], true);
        assert!(!path.exists());
        assert_eq!(result, broker.handle(request).0);
        assert_eq!(broker.module.inspect(id).unwrap().input_events_sent(), 4);
    }

    fn combined_request(id: &str, path: &std::path::Path) -> BrokerRequest {
        serde_json::from_value(json!({"contractVersion":CONTRACT_VERSION,"brokerEpoch":EPOCH,"requestNonce":"22222222222222222222222222222222","operation":"interact","sessionId":id,"confirmed":true,"foregroundConsent":true,"strictIsolation":false,"input":{"steps":[{"type":"text","text":"ab"}]},"observation":{"path":path}})).unwrap()
    }

    #[test]
    fn combined_observation_replays_without_input_or_recapture() {
        let (mut broker, _, _) = broker();
        let (opened, _) = broker.handle(open_request("11111111111111111111111111111111"));
        let id = opened["data"]["sessionId"].as_str().unwrap();
        let path = std::env::temp_dir().join(format!(
            "act-combined-{}.png",
            desktop_session_identity::random_nonce().unwrap()
        ));
        let request = combined_request(id, &path);
        let schema: Value = serde_json::from_str(include_str!(
            "../contracts/v1/linux-desktop-session-broker-v1.schema.json"
        ))
        .unwrap();
        jsonschema::draft202012::validate(&schema, &serde_json::to_value(&request).unwrap())
            .unwrap();
        let (first, _) = broker.handle(request.clone());
        jsonschema::draft202012::validate(&schema, &first).unwrap();
        assert_eq!(first["completed"], true, "{first}");
        assert_eq!(first["data"]["completedSteps"], 1);
        assert_eq!(first["data"]["effectConfirmed"], false);
        assert_eq!(
            first["data"]["observation"]["coordinateSpace"],
            "observation-px"
        );
        assert_eq!(
            first["data"]["observation"]["frameId"]
                .as_str()
                .unwrap()
                .len(),
            32
        );
        std::fs::remove_file(&path).unwrap();
        let (second, _) = broker.handle(request);
        assert_eq!(first, second);
        assert!(!path.exists(), "replay must not recapture");
        assert_eq!(broker.module.inspect(id).unwrap().input_events_sent(), 4);
    }

    #[test]
    fn combined_invalid_observation_prevents_all_input() {
        let (mut broker, _, _) = broker();
        let (opened, _) = broker.handle(open_request("11111111111111111111111111111111"));
        let id = opened["data"]["sessionId"].as_str().unwrap();
        let request = combined_request(id, std::path::Path::new("/missing-act-dir/frame.png"));
        let (result, _) = broker.handle(request);
        assert_eq!(result["completed"], false);
        assert_eq!(
            result["error"]["details"]["failedStage"],
            "observation-preflight"
        );
        assert_eq!(result["acceptedMayHaveOccurred"], false);
        assert_eq!(broker.module.inspect(id).unwrap().input_events_sent(), 0);
    }

    #[test]
    fn combined_invalid_action_skips_observation() {
        let (mut broker, _, _) = broker();
        let (opened, _) = broker.handle(open_request("11111111111111111111111111111111"));
        let id = opened["data"]["sessionId"].as_str().unwrap();
        let path = std::env::temp_dir().join(format!(
            "act-invalid-{}.png",
            desktop_session_identity::random_nonce().unwrap()
        ));
        let mut value = serde_json::to_value(combined_request(id, &path)).unwrap();
        value["input"]["steps"] = json!([]);
        let (result, _) = broker.handle(serde_json::from_value(value).unwrap());
        assert_eq!(result["error"]["details"]["failedStage"], "interaction");
        assert_eq!(result["error"]["details"]["observationAttempted"], false);
        assert!(!path.exists());
        assert_eq!(broker.module.inspect(id).unwrap().input_events_sent(), 0);
    }

    #[test]
    fn combined_cancelled_input_does_not_observe_or_replay() {
        let (mut broker, _, _) = broker();
        let (opened, _) = broker.handle(open_request("11111111111111111111111111111111"));
        let id = opened["data"]["sessionId"].as_str().unwrap();
        let path = std::env::temp_dir().join(format!(
            "act-cancelled-{}.png",
            desktop_session_identity::random_nonce().unwrap()
        ));
        let request = combined_request(id, &path);
        let cancel: BrokerRequest = serde_json::from_value(json!({"contractVersion":CONTRACT_VERSION,"brokerEpoch":EPOCH,"requestNonce":"33333333333333333333333333333333","operation":"input-cancel","targetRequestNonce":request.request_nonce()})).unwrap();
        broker.handle(cancel);
        let (result, _) = broker.handle(request.clone());
        assert_eq!(result["outcome"], "cancelled", "{result}");
        assert!(!path.exists());
        assert_eq!(result, broker.handle(request).0);
    }

    #[test]
    fn stdio_server_emits_ready_and_clean_shutdown_response() {
        let (broker, _, _) = broker();
        let input = format!(
            "{{\"operation\":\"shutdown\",\"contractVersion\":\"{CONTRACT_VERSION}\",\"brokerEpoch\":\"{EPOCH}\",\"requestNonce\":\"88888888888888888888888888888888\"}}\n"
        );
        let mut output = Vec::new();
        let exit = serve(&mut io::Cursor::new(input), &mut output, broker);
        assert_eq!(exit, 0);
        let frames = String::from_utf8(output)
            .unwrap_or_else(|error| panic!("server output must be UTF-8: {error}"))
            .lines()
            .map(|line| {
                serde_json::from_str::<Value>(line)
                    .unwrap_or_else(|error| panic!("server frame must be JSON: {error}"))
            })
            .collect::<Vec<_>>();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0]["messageType"], "broker-ready");
        assert_eq!(frames[0]["postInputObservation"], true);
        assert_eq!(frames[1]["data"]["shutdownAccepted"], true);
    }
}
