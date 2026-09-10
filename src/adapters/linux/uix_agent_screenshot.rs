//! UIX Agent 应用表面截图私有 Adapter。

use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::components::uix_screenshot_payload::{UixScreenshotPayloadError, UixScreenshotPng};

use super::{
    AgentClient, Failure, RequestFailure, WireWindow, remaining, resolve_until, window_session_id,
};

pub(super) const MAXIMUM_SCREENSHOT_PNG_BYTES: usize = 32 * 1024 * 1024;
const MAXIMUM_SCREENSHOT_RESPONSE_BYTES: usize =
    MAXIMUM_SCREENSHOT_PNG_BYTES.div_ceil(3) * 4 + 4 * 1024;

/// hello 协商后允许用于单次截图的内存边界。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ScreenshotLimits {
    pub(super) png_bytes: usize,
    pub(super) response_bytes: usize,
}

/// 不泄露 base64、端点或原生窗口身份的截图结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScreenshotRecord {
    pub(crate) png: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) png_digest: String,
}

#[derive(Debug, Deserialize)]
struct WireScreenshot {
    window_id: u64,
    width: u32,
    height: u32,
    format: String,
    data_base64: String,
}

/// 只在 Agent 同时声明截图类型、布尔能力与一致上限时启用。
pub(super) fn negotiated_limits(
    hello: &Value,
    request_types: &std::collections::BTreeSet<String>,
) -> Result<Option<ScreenshotLimits>, Failure> {
    let Some(flag) = hello.pointer("/capabilities/screenshot") else {
        return Ok(None);
    };
    let supported = flag.as_bool().ok_or(Failure::Protocol)?;
    if !supported {
        return Ok(None);
    }
    if !request_types.contains("screenshot") {
        return Err(Failure::Protocol);
    }
    let png_bytes = hello
        .pointer("/limits/max_screenshot_bytes")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| (24..=MAXIMUM_SCREENSHOT_PNG_BYTES).contains(value))
        .ok_or(Failure::Protocol)?;
    let response_bytes = hello
        .pointer("/limits/max_response_bytes")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value <= MAXIMUM_SCREENSHOT_RESPONSE_BYTES)
        .ok_or(Failure::Protocol)?;
    let required_response = png_bytes
        .checked_add(2)
        .and_then(|value| value.checked_div(3))
        .and_then(|value| value.checked_mul(4))
        .and_then(|value| value.checked_add(4 * 1024))
        .ok_or(Failure::Protocol)?;
    if response_bytes < required_response {
        return Err(Failure::Protocol);
    }
    Ok(Some(ScreenshotLimits {
        png_bytes,
        response_bytes,
    }))
}

/// 在总 deadline 内截图，并在同一连接上复核窗口 generation 后才返回 PNG。
pub(crate) fn capture(target: &str, timeout: Duration) -> Result<ScreenshotRecord, Failure> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(Failure::Timeout)?;
    let window = resolve_until(target, deadline)?;
    capture_resolved(target, window, deadline)
}

fn capture_resolved(
    target: &str,
    window: super::WindowRecord,
    deadline: Instant,
) -> Result<ScreenshotRecord, Failure> {
    if !window.screenshot_supported {
        return Err(Failure::Unavailable);
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline)?;
    let limits = client.screenshot_limits.ok_or(Failure::Unavailable)?;
    let reply = client
        .request_with_response_limit(
            "act-screenshot",
            "screenshot",
            json!({ "window_id": window.window_id }),
            limits.response_bytes,
        )
        .map_err(screenshot_request_failure)?;
    let object = reply.as_object().ok_or(Failure::Protocol)?;
    let allowed_fields = [
        "schema",
        "request_id",
        "ok",
        "type",
        "window_id",
        "width",
        "height",
        "format",
        "data_base64",
    ];
    if object.len() != allowed_fields.len()
        || object
            .keys()
            .any(|name| !allowed_fields.contains(&name.as_str()))
    {
        return Err(Failure::Protocol);
    }
    let wire = serde_json::from_value::<WireScreenshot>(reply).map_err(|_| Failure::Protocol)?;
    if wire.window_id != window.window_id || wire.format != "png" {
        return Err(Failure::Protocol);
    }
    let png =
        UixScreenshotPng::decode(&wire.data_base64, wire.width, wire.height, limits.png_bytes)
            .map_err(payload_failure)?;
    remaining(deadline)?;
    let current = client.list_windows()?;
    let mut matching_id = current
        .iter()
        .filter(|candidate| candidate.window_id == window.window_id);
    let Some(candidate) = matching_id.next() else {
        return Err(Failure::Stale);
    };
    if matching_id.next().is_some() {
        return Err(Failure::Protocol);
    }
    validate_post_capture_target(target, &window, candidate)?;
    Ok(ScreenshotRecord {
        png: png.bytes().to_vec(),
        width: png.width(),
        height: png.height(),
        png_digest: png.digest().to_owned(),
    })
}

fn validate_post_capture_target(
    target: &str,
    window: &super::WindowRecord,
    candidate: &WireWindow,
) -> Result<(), Failure> {
    if candidate.closed
        || candidate.generation != window.generation
        || candidate.revision < window.revision
        || candidate.presented_revision < window.presented_revision
        || candidate.presented_revision > candidate.revision
        || window_session_id(&window.endpoint, candidate) != target
    {
        return Err(Failure::Stale);
    }
    Ok(())
}

fn payload_failure(error: UixScreenshotPayloadError) -> Failure {
    match error {
        UixScreenshotPayloadError::InvalidPayload
        | UixScreenshotPayloadError::ResourceExhausted => Failure::Protocol,
    }
}

fn screenshot_request_failure(error: RequestFailure) -> Failure {
    match error {
        RequestFailure::Transport(failure) => failure,
        RequestFailure::Remote { code, .. } => match code.as_str() {
            "unauthorized" | "forbidden" => Failure::PermissionDenied,
            "timeout" => Failure::Timeout,
            "window_not_found" | "stale_window" | "app_closed" => Failure::Stale,
            "unsupported_action" => Failure::Unavailable,
            _ => Failure::Protocol,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        fs,
        io::{BufRead, BufReader, Write},
        os::unix::{fs::PermissionsExt, net::UnixListener},
        sync::atomic::{AtomicU64, Ordering},
        thread,
    };

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
    use serde_json::json;

    use super::*;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn screenshot_negotiation_requires_consistent_bounded_limits() {
        let requests = BTreeSet::from(["screenshot".to_owned()]);
        let hello = json!({
            "capabilities": { "screenshot": true },
            "limits": {
                "max_screenshot_bytes": MAXIMUM_SCREENSHOT_PNG_BYTES,
                "max_response_bytes": MAXIMUM_SCREENSHOT_RESPONSE_BYTES
            }
        });
        let Ok(limits) = negotiated_limits(&hello, &requests) else {
            panic!("有效截图协商必须成功");
        };
        assert!(limits.is_some());
        let mut invalid = hello;
        invalid["limits"]["max_response_bytes"] = json!(1024);
        assert_eq!(
            negotiated_limits(&invalid, &requests),
            Err(Failure::Protocol)
        );
    }

    #[test]
    fn same_connection_generation_recheck_accepts_only_the_original_window() {
        assert!(fixture_capture(3).is_ok());
        assert_eq!(fixture_capture(4), Err(Failure::Stale));
    }

    fn fixture_capture(post_capture_generation: u64) -> Result<ScreenshotRecord, Failure> {
        let directory = std::env::temp_dir().join(format!(
            "act-uix-screenshot-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).map_err(|_| Failure::Protocol)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| Failure::Protocol)?;
        let socket_path = directory.join("agent.sock");
        let listener = UnixListener::bind(&socket_path).map_err(|_| Failure::Protocol)?;
        let process_id = std::process::id();
        let server = thread::spawn(move || -> Result<(), String> {
            let (stream, _) = listener.accept().map_err(|error| error.to_string())?;
            let mut reader = BufReader::new(stream);
            for expected in ["hello", "screenshot", "list_windows"] {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                assert_eq!(request["type"], expected);
                let payload = match expected {
                    "hello" => json!({
                        "process_id": process_id,
                        "capabilities": {
                            "request_types": ["hello", "list_windows", "snapshot", "screenshot"],
                            "screenshot": true
                        },
                        "limits": {
                            "max_screenshot_bytes": 1024,
                            "max_response_bytes": 5464
                        }
                    }),
                    "screenshot" => {
                        assert_eq!(request["window_id"], 7);
                        let mut png = Vec::new();
                        PngEncoder::new(&mut png)
                            .write_image(&[0_u8; 4], 1, 1, ExtendedColorType::Rgba8)
                            .map_err(|error| error.to_string())?;
                        json!({
                            "window_id": 7,
                            "width": 1,
                            "height": 1,
                            "format": "png",
                            "data_base64": STANDARD.encode(png)
                        })
                    }
                    _ => json!({
                        "windows": [{
                            "window_id": 7,
                            "generation": post_capture_generation,
                            "title": "fixture",
                            "visible": true,
                            "presentable": true,
                            "revision": 5,
                            "presented_revision": 5,
                            "closed": false
                        }]
                    }),
                };
                let mut reply = json!({
                    "schema": super::super::PROTOCOL_SCHEMA,
                    "request_id": request["request_id"],
                    "ok": true,
                    "type": expected
                });
                let reply_object = reply
                    .as_object_mut()
                    .ok_or_else(|| "测试响应必须是对象".to_owned())?;
                let payload_object = payload
                    .as_object()
                    .ok_or_else(|| "测试负载必须是对象".to_owned())?;
                reply_object.extend(payload_object.clone());
                writeln!(reader.get_mut(), "{reply}").map_err(|error| error.to_string())?;
            }
            Ok(())
        });
        let descriptor = super::super::EndpointDescriptor {
            schema: super::super::PROTOCOL_SCHEMA.to_owned(),
            process_id,
            endpoint: socket_path.to_string_lossy().into_owned(),
            token: "7".repeat(64),
            state: "ready".to_owned(),
        };
        let original = WireWindow {
            window_id: 7,
            generation: 3,
            title: "fixture".to_owned(),
            visible: true,
            presentable: true,
            logical_width: None,
            logical_height: None,
            maximized: None,
            minimized: None,
            fullscreen: None,
            focused: None,
            revision: 5,
            presented_revision: 5,
            closed: false,
        };
        let target = window_session_id(&descriptor, &original);
        let window = super::super::WindowRecord {
            session_id: target.clone(),
            owner_process_session_id: None,
            title: "fixture".to_owned(),
            visible: true,
            presentable: true,
            focused: None,
            revision: 5,
            presented_revision: 5,
            state: None,
            screenshot_supported: true,
            activation_supported: false,
            pointer_drag_supported: false,
            endpoint: descriptor,
            window_id: 7,
            generation: 3,
        };
        let result = capture_resolved(&target, window, Instant::now() + Duration::from_secs(2));
        let server_result = server.join().map_err(|_| Failure::Protocol)?;
        server_result.map_err(|_| Failure::Protocol)?;
        fs::remove_file(socket_path).map_err(|_| Failure::Protocol)?;
        fs::remove_dir(directory).map_err(|_| Failure::Protocol)?;
        result
    }
}
