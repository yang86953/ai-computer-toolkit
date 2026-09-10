//! MCP 工具到桌面会话的映射层。
//!
//! 本层持有本客户端独占的会话状态（broker、sessionId、最新 frameId、临时图像目录），
//! 并把工具调用翻译为 broker 操作。授权布尔值只表达用户已有授权：这里不创造授权，
//! 也不提供后台隔离路线。

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use serde_json::{Value, json};

use super::broker::{Broker, BrokerFailure};

/// PNG 文件头；只接受真实 PNG，拒绝被替换的路径内容。
const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// 内联图像的大小上限，避免把超大截图塞进协议帧。
const MAXIMUM_PNG_BYTES: usize = 32 * 1024 * 1024;

/// 默认返回图最长边。
const DEFAULT_MAX_DIMENSION: u64 = 1280;

/// 工具执行结果：MCP `content` 数组与是否错误。
pub struct ToolOutcome {
    pub content: Vec<Value>,
    pub is_error: bool,
}

impl ToolOutcome {
    /// 纯文本结果。
    fn text(value: &Value) -> Self {
        Self {
            content: vec![json!({ "type": "text", "text": value.to_string() })],
            is_error: false,
        }
    }

    /// 文本 + PNG 图像结果。
    fn image(text: &Value, png: &[u8]) -> Self {
        Self {
            content: vec![
                json!({ "type": "text", "text": text.to_string() }),
                json!({
                    "type": "image",
                    "mimeType": "image/png",
                    "data": BASE64_STANDARD.encode(png),
                }),
            ],
            is_error: false,
        }
    }

    /// 失败结果：仍是成功的 JSON-RPC 响应，错误在工具层表达。
    fn failed(failure: &BrokerFailure) -> Self {
        Self {
            content: vec![json!({ "type": "text", "text": failure.payload().to_string() })],
            is_error: true,
        }
    }
}

/// 一个 MCP 客户端独占的桌面会话。
pub struct Desktop {
    broker: Option<Broker>,
    session: Option<String>,
    frame: Option<String>,
    directory: PathBuf,
    cancellation: crate::components::desktop_session_input_cancellation::DesktopInputCancellation,
}

impl Desktop {
    /// 建立会话容器；此时不启动 broker、不打开 Portal。
    pub fn new() -> Result<Self, BrokerFailure> {
        let name = format!(
            "computer-control-mcp-{}-{}",
            std::process::id(),
            unique_name()
        );
        #[cfg(target_os = "windows")]
        let directory = crate::components::owner_only_directory_windows::ensure_owner_only_child(
            &std::env::temp_dir(),
            &name,
        )
        .map_err(|_| {
            BrokerFailure::failed(
                "IMAGE_DELIVERY_FAILED",
                "Cannot create a private capture directory.",
            )
        })?;
        #[cfg(not(target_os = "windows"))]
        let directory = {
            let path = std::env::temp_dir().join(name);
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .map_err(|error| {
                    BrokerFailure::failed("IMAGE_DELIVERY_FAILED", error.to_string())
                })?;
            path
        };
        Ok(Self {
            broker: None,
            session: None,
            frame: None,
            directory,
            cancellation: Default::default(),
        })
    }

    pub(crate) fn set_cancellation(
        &mut self,
        token: crate::components::desktop_session_input_cancellation::DesktopInputCancellation,
    ) {
        self.cancellation = token.clone();
        if let Some(broker) = self.broker.as_mut() {
            broker.set_cancellation(token);
        }
    }

    /// 执行一次工具调用。
    pub fn call(&mut self, name: &str, arguments: &Value) -> ToolOutcome {
        match self.dispatch(name, arguments) {
            Ok(outcome) => outcome,
            Err(failure) => ToolOutcome::failed(&failure),
        }
    }

    /// 分派单个工具；未知工具在此闭合。
    fn dispatch(&mut self, name: &str, arguments: &Value) -> Result<ToolOutcome, BrokerFailure> {
        if self.cancellation.is_cancelled() {
            return Err(BrokerFailure::failed(
                "CANCELLED",
                "Request cancelled before dispatch.",
            ));
        }
        let arguments = arguments.as_object().cloned().ok_or_else(|| {
            BrokerFailure::failed("INVALID_ARGUMENT", "Tool arguments must be an object.")
        })?;
        if let Some(schema) = super::tools::input_schema(name) {
            let properties = &schema["properties"];
            for (key, value) in &arguments {
                let Some(spec) = properties.get(key) else {
                    return Err(BrokerFailure::failed(
                        "INVALID_ARGUMENT",
                        "Unknown tool argument.",
                    ));
                };
                let valid = match spec["type"].as_str() {
                    Some("integer") => value.as_u64().is_some_and(|n| {
                        n >= spec["minimum"].as_u64().unwrap_or(0)
                            && n <= spec["maximum"].as_u64().unwrap_or(u64::MAX)
                    }),
                    Some("boolean") => value.is_boolean(),
                    Some("string") => value.is_string(),
                    Some("array") => value.is_array(),
                    _ => true,
                };
                if !valid {
                    return Err(BrokerFailure::failed(
                        "INVALID_ARGUMENT",
                        "Tool argument has an invalid type or range.",
                    ));
                }
            }
        }
        let flag = |key: &str| arguments.get(key).and_then(Value::as_bool) == Some(true);
        match name {
            "computer_status" => {
                let data = match self.broker.as_mut() {
                    Some(broker) => broker.call("sessions", json!({}), Duration::from_secs(70))?,
                    None => json!({ "sessions": [] }),
                };
                let data = data.get("data").cloned().unwrap_or(data);
                Ok(ToolOutcome::text(&data))
            }
            "computer_connect" => {
                self.require_authorization(&arguments, true)?;
                self.connect(
                    &arguments,
                    flag("confirmed"),
                    flag("foregroundConsent"),
                    flag("strictIsolation"),
                )
            }
            "computer_disconnect" => self.disconnect(&arguments),
            "computer_observe" => {
                self.require_authorization(&arguments, false)?;
                self.require_session(&arguments)?;
                self.observe(&arguments)
            }
            "computer_interact" | "computer_keys" | "computer_pointer" => {
                self.require_authorization(&arguments, true)?;
                self.require_session(&arguments)?;
                self.input(name, &arguments)
            }
            _ => Err(BrokerFailure::failed(
                "INVALID_ARGUMENT",
                "Unknown tool; call tools/list for the closed catalog.",
            )),
        }
    }

    /// 采集/输入共用的授权检查；授权不足时不触达桌面。
    fn require_authorization(
        &self,
        arguments: &serde_json::Map<String, Value>,
        write: bool,
    ) -> Result<(), BrokerFailure> {
        let confirmed = arguments.get("confirmed").and_then(Value::as_bool) == Some(true);
        let isolated = arguments.get("strictIsolation").and_then(Value::as_bool) != Some(false);
        let foreground = arguments.get("foregroundConsent").and_then(Value::as_bool) == Some(true);
        if !confirmed || isolated || (write && !foreground) {
            return Err(BrokerFailure::failed(
                "CONSENT_REQUIRED",
                "Desktop foreground requires existing user authorization with strictIsolation=false.",
            ));
        }
        Ok(())
    }

    /// 校验调用方只使用本客户端返回的会话标识。
    fn require_session(
        &self,
        arguments: &serde_json::Map<String, Value>,
    ) -> Result<(), BrokerFailure> {
        let requested = arguments.get("sessionId").and_then(Value::as_str);
        match (requested, self.session.as_deref()) {
            (Some(requested), Some(current)) if requested == current && self.broker.is_some() => {
                Ok(())
            }
            _ => Err(BrokerFailure::failed(
                "STALE_SESSION",
                "Connect and use this client's returned sessionId.",
            )),
        }
    }

    /// 输入类工具必须引用最新帧；旧帧一律失效。
    fn require_frame(
        &self,
        arguments: &serde_json::Map<String, Value>,
    ) -> Result<(), BrokerFailure> {
        let requested = arguments.get("frameId").and_then(Value::as_str);
        match (requested, self.frame.as_deref()) {
            (Some(requested), Some(current)) if requested == current => Ok(()),
            _ => Err(BrokerFailure::failed(
                "STALE_FRAME",
                "Observe again and use the latest frameId; inputs are never replayed.",
            )),
        }
    }

    /// 建立独占会话。
    fn connect(
        &mut self,
        arguments: &serde_json::Map<String, Value>,
        confirmed: bool,
        foreground: bool,
        isolated: bool,
    ) -> Result<ToolOutcome, BrokerFailure> {
        if !confirmed || !foreground || isolated {
            return Err(BrokerFailure::failed(
                "CONSENT_REQUIRED",
                "Desktop foreground requires existing user authorization and strictIsolation=false.",
            ));
        }
        if self.broker.is_some() {
            return Err(BrokerFailure::failed(
                "ALREADY_CONNECTED",
                "Disconnect the current client first; no implicit reconnection.",
            ));
        }
        let program = std::env::current_exe()
            .map_err(|error| BrokerFailure::failed("BROKER_START_FAILED", error.to_string()))?;
        let program = program.to_string_lossy().to_string();
        let timeout_ms = arguments
            .get("timeoutMs")
            .and_then(Value::as_u64)
            .unwrap_or(30_000);
        let mut broker = Broker::start(&program)?;
        broker.set_cancellation(self.cancellation.clone());
        let response = broker.call(
            "open",
            json!({
                "confirmed": true,
                "foregroundConsent": true,
                "strictIsolation": false,
                "timeoutMs": timeout_ms,
            }),
            Duration::from_millis(timeout_ms) + Duration::from_secs(5),
        );
        match response {
            Ok(response) => {
                let data = response.get("data").cloned().unwrap_or(json!({}));
                let session = data
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        BrokerFailure::failed(
                            "BROKER_PROTOCOL_FAILED",
                            "Broker returned no sessionId.",
                        )
                    })?
                    .to_owned();
                if self.cancellation.is_cancelled() {
                    let _ = broker.call(
                        "close",
                        json!({"sessionId":session}),
                        Duration::from_secs(5),
                    );
                    broker.stop();
                    return Err(BrokerFailure::failed(
                        "CANCELLED",
                        "Connection closed after cancellation.",
                    ));
                }
                self.session = Some(session);
                self.broker = Some(broker);
                Ok(ToolOutcome::text(&data))
            }
            Err(failure) => {
                broker.stop();
                Err(failure)
            }
        }
    }

    /// 关闭会话、读回空 sessions 并释放 broker。
    fn disconnect(
        &mut self,
        arguments: &serde_json::Map<String, Value>,
    ) -> Result<ToolOutcome, BrokerFailure> {
        self.require_session(arguments)?;
        let session = self.session.clone().unwrap_or_default();
        let Some(mut broker) = self.broker.take() else {
            return Err(BrokerFailure::failed("BROKER_CLOSED", "No active broker."));
        };
        let result = (|| {
            let closed = broker.call(
                "close",
                json!({ "sessionId": session }),
                Duration::from_secs(70),
            )?;
            let sessions = broker.call("sessions", json!({}), Duration::from_secs(70))?;
            let listed = sessions
                .get("data")
                .and_then(|data| data.get("sessions"))
                .cloned()
                .unwrap_or(json!([]));
            if listed != json!([]) {
                return Err(BrokerFailure::failed(
                    "CLEANUP_FAILED",
                    "The broker still reports open sessions.",
                ));
            }
            let mut data = closed.get("data").cloned().unwrap_or(json!({}));
            if let (Some(target), Some(extra)) = (
                data.as_object_mut(),
                sessions.get("data").and_then(Value::as_object),
            ) {
                for (key, value) in extra {
                    target.insert(key.clone(), value.clone());
                }
            }
            Ok(ToolOutcome::text(&data))
        })();
        self.session = None;
        self.frame = None;
        broker.stop();
        result
    }

    /// 截图并内联返回 PNG。
    fn observe(
        &mut self,
        arguments: &serde_json::Map<String, Value>,
    ) -> Result<ToolOutcome, BrokerFailure> {
        self.frame = None;
        let capture = self.capture_fields(arguments);
        let session = self.session.clone().unwrap_or_default();
        let broker = self
            .broker
            .as_mut()
            .ok_or_else(|| BrokerFailure::failed("BROKER_CLOSED", "No active broker."))?;
        let response = broker.call(
            "observe",
            json!({
                "sessionId": session,
                "confirmed": true,
                "strictIsolation": false,
                "input": capture,
            }),
            Duration::from_secs(70),
        )?;
        self.image_result(&response, &capture)
    }

    /// 发送输入并在同一请求内读回截图。
    fn input(
        &mut self,
        name: &str,
        arguments: &serde_json::Map<String, Value>,
    ) -> Result<ToolOutcome, BrokerFailure> {
        self.require_frame(arguments)?;
        let frame = arguments
            .get("frameId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        // 预检拒绝（acceptedMayHaveOccurred=false）没有投递任何事件，观察仍然成立，
        // 这一帧不该被消耗；只有输入可能已经送出时才作废旧帧。
        let keep_alive = frame.clone();
        self.frame = None;
        let capture = self.capture_fields(arguments);
        let session = self.session.clone().unwrap_or_default();
        let timeout_ms = arguments
            .get("timeoutMs")
            .and_then(Value::as_u64)
            .unwrap_or(3_000);
        let steps = match name {
            "computer_keys" => {
                let keys = arguments.get("keys").cloned().unwrap_or(json!([]));
                json!([{ "type": "key", "keys": keys }])
            }
            _ => arguments.get("steps").cloned().unwrap_or(json!([])),
        };
        let broker = self
            .broker
            .as_mut()
            .ok_or_else(|| BrokerFailure::failed("BROKER_CLOSED", "No active broker."))?;
        if name == "computer_pointer" {
            let interaction = match broker.call(
                "input-pointer",
                json!({
                    "sessionId": session,
                    "confirmed": true,
                    "foregroundConsent": true,
                    "strictIsolation": false,
                    "input": {
                        "coordinateSpace": super::tools::relative_coordinate_space(),
                        "steps": steps,
                        "timeoutMs": timeout_ms,
                    },
                }),
                Duration::from_millis(timeout_ms) + Duration::from_secs(5),
            ) {
                Ok(interaction) => interaction,
                Err(failure) => {
                    restore_undelivered_frame(&mut self.frame, &failure, keep_alive);
                    return Err(failure);
                }
            };
            let interaction = interaction.get("data").cloned().unwrap_or(json!({}));
            let response = match broker.call(
                "observe",
                json!({
                    "sessionId": session,
                    "confirmed": true,
                    "strictIsolation": false,
                    "input": capture,
                }),
                Duration::from_secs(70),
            ) {
                Ok(response) => response,
                Err(error) => {
                    // 输入已投递但观察失败：保留事实，禁止自动重放。
                    return Err(BrokerFailure {
                        code: "OBSERVATION_FAILED_AFTER_INPUT".to_owned(),
                        message: format!(
                            "Input was dispatched but the follow-up observation failed: {}",
                            error.message
                        ),
                        outcome_unknown: true,
                        accepted_may_have_occurred: true,
                    });
                }
            };
            let mut outcome = self.image_result(&response, &capture)?;
            // 附上相对指针交互的回执，供调用方核对。
            if let Some(first) = outcome.content.first_mut() {
                let mut combined = first
                    .get("text")
                    .and_then(Value::as_str)
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                    .unwrap_or(json!({}));
                if let Some(target) = combined.as_object_mut() {
                    target.insert("interaction".to_owned(), interaction);
                }
                *first = json!({ "type": "text", "text": combined.to_string() });
            }
            return Ok(outcome);
        }
        let response = match broker.call(
            "interact",
            json!({
                "sessionId": session,
                "confirmed": true,
                "foregroundConsent": true,
                "strictIsolation": false,
                "input": { "frameId": frame, "steps": steps, "timeoutMs": timeout_ms },
                "observation": capture,
            }),
            Duration::from_millis(timeout_ms) + Duration::from_secs(70),
        ) {
            Ok(response) => response,
            Err(failure) => {
                restore_undelivered_frame(&mut self.frame, &failure, keep_alive);
                return Err(failure);
            }
        };
        self.image_result(&response, &capture)
    }

    /// 构造截图请求字段：私有临时文件 + 有界尺寸。
    fn capture_fields(&self, arguments: &serde_json::Map<String, Value>) -> Value {
        let path = self.directory.join(format!("{}.png", unique_name()));
        json!({
            "path": path.to_string_lossy(),
            "maxDimension": arguments
                .get("maxDimension")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_MAX_DIMENSION),
            "timeoutMs": 5000,
        })
    }

    /// 读取截图为内联 MCP 图像；任何不一致都收敛为投递失败。
    fn image_result(
        &mut self,
        response: &Value,
        capture: &Value,
    ) -> Result<ToolOutcome, BrokerFailure> {
        let data = response.get("data").cloned().unwrap_or(json!({}));
        let observation = data
            .get("observation")
            .cloned()
            .unwrap_or_else(|| data.clone());
        self.frame = observation
            .get("frameId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let path = capture
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let result = read_capture(Path::new(path), &observation, self.session.as_deref());
        // 隐私：无论成功与否都移除这份瞬时文件。
        let _ = fs::remove_file(path);
        match result {
            Ok(raw) => {
                let mut clean = observation.clone();
                if let Some(target) = clean.as_object_mut() {
                    target.remove("path");
                }
                let mut details = data.clone();
                match details.as_object_mut() {
                    Some(target) if target.contains_key("observation") => {
                        target.insert("observation".to_owned(), clean);
                    }
                    _ => details = clean,
                }
                Ok(ToolOutcome::image(&details, &raw))
            }
            Err(failure) => {
                // 帧已不可信；要求重新观察而不是重放输入。
                self.frame = None;
                Err(failure)
            }
        }
    }

    /// 释放本客户端资源；EOF 与信号退出都走这里。
    pub fn dispose(&mut self) {
        self.session = None;
        self.frame = None;
        if let Some(mut broker) = self.broker.take() {
            broker.stop();
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// 预检拒绝时把帧还给会话：broker 明确报告没有投递事件，观察结论仍然成立。
/// 一旦输入可能已经送出（含结果未知），保持失效，绝不自动重放。
fn restore_undelivered_frame(frame: &mut Option<String>, failure: &BrokerFailure, frame_id: String) {
    if !failure.accepted_may_have_occurred {
        *frame = Some(frame_id);
    }
}

/// 校验并读取截图文件；身份不一致、软链接、超大或非 PNG 都拒绝。
fn read_capture(
    path: &Path,
    observation: &Value,
    session: Option<&str>,
) -> Result<Vec<u8>, BrokerFailure> {
    let declared = observation.get("path").and_then(Value::as_str);
    let observed_session = observation.get("sessionId").and_then(Value::as_str);
    if declared != Some(path.to_string_lossy().as_ref()) || observed_session != session {
        return Err(BrokerFailure {
            code: "IMAGE_DELIVERY_FAILED".to_owned(),
            message: "Capture identity mismatch; input may already have completed. Observe, never replay.".to_owned(),
            outcome_unknown: true,
            accepted_may_have_occurred: true,
        });
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| BrokerFailure {
        code: "IMAGE_DELIVERY_FAILED".to_owned(),
        message: format!("Capture is unavailable: {error}"),
        outcome_unknown: true,
        accepted_may_have_occurred: true,
    })?;
    if metadata.file_type().is_symlink() || metadata.len() > MAXIMUM_PNG_BYTES as u64 {
        return Err(BrokerFailure {
            code: "IMAGE_DELIVERY_FAILED".to_owned(),
            message: "Capture is a symlink or exceeds the inline image limit.".to_owned(),
            outcome_unknown: true,
            accepted_may_have_occurred: true,
        });
    }
    let raw = fs::read(path).map_err(|error| BrokerFailure {
        code: "IMAGE_DELIVERY_FAILED".to_owned(),
        message: format!("Capture cannot be read: {error}"),
        outcome_unknown: true,
        accepted_may_have_occurred: true,
    })?;
    if !raw.starts_with(PNG_MAGIC) {
        return Err(BrokerFailure {
            code: "IMAGE_DELIVERY_FAILED".to_owned(),
            message: "Capture is not a PNG image.".to_owned(),
            outcome_unknown: true,
            accepted_may_have_occurred: true,
        });
    }
    Ok(raw)
}

/// 生成进程内唯一的一次性图像名。
fn unique_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{nanos:032x}{sequence:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_dispatch_rejection_keeps_the_observation_alive() {
        let frame = "a".repeat(32);
        // 预检拒绝没有投递任何事件，观察仍然成立：帧还给会话，省掉一次重新观察。
        let mut current = None;
        restore_undelivered_frame(
            &mut current,
            &BrokerFailure::failed("INVALID_ARGUMENT", "rejected before dispatch"),
            frame.clone(),
        );
        assert_eq!(current.as_deref(), Some(frame.as_str()));
        // 输入可能已经送出（含结果未知）：调用前帧已作废，这里必须保持失效，只能重新观察。
        let mut dispatched = None;
        let mut failure = BrokerFailure::failed("OUTCOME_UNKNOWN", "input may have been sent");
        failure.accepted_may_have_occurred = true;
        restore_undelivered_frame(&mut dispatched, &failure, frame);
        assert_eq!(dispatched, None);
    }
}
