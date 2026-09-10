//! UIX Agent 框架当前窗口状态条件等待的私有 Adapter。

use std::{
    collections::BTreeSet,
    thread,
    time::{Duration, Instant},
};

use crate::components::uix_window_state_wait_contract::{
    UixWindowStateFacts, UixWindowStateWaitInput,
};

use super::{
    AgentClient, Failure, MAX_WINDOWS, WindowRecord, WireWindow, resolve_until, window_session_id,
};

pub(super) const REQUIRED_WINDOW_STATE_FIELDS: [&str; 6] = [
    "logical_width",
    "logical_height",
    "maximized",
    "minimized",
    "fullscreen",
    "focused",
];

/// 一次条件满足时可公开的完整 framework-current 状态观察。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowStateWaitObservation {
    pub(crate) visible: bool,
    pub(crate) presentable: bool,
    pub(crate) focused: bool,
    pub(crate) logical_width: u32,
    pub(crate) logical_height: u32,
    pub(crate) maximized: bool,
    pub(crate) minimized: bool,
    pub(crate) fullscreen: bool,
    pub(crate) revision: u64,
    pub(crate) presented_revision: u64,
    pub(crate) poll_count: u32,
}

/// 在总 deadline 内 resolve 一次并在同一认证连接轮询窗口状态。
pub(crate) fn perform_window_state_wait(
    target: &str,
    input: &UixWindowStateWaitInput,
) -> Result<WindowStateWaitObservation, Failure> {
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline)?;
    let mut client = AgentClient::connect_until(&window.endpoint, deadline)?;
    execute_wait(&mut client, &window, target, input)
}

fn execute_wait(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixWindowStateWaitInput,
) -> Result<WindowStateWaitObservation, Failure> {
    if !REQUIRED_WINDOW_STATE_FIELDS
        .iter()
        .all(|field| client.window_state_fields.contains(*field))
    {
        return Err(Failure::Unavailable);
    }

    let mut baseline_revision = window.revision;
    let mut baseline_presented_revision = window.presented_revision;
    let mut poll_count = 0_u32;
    loop {
        let windows = client.list_windows()?;
        poll_count = poll_count.saturating_add(1);
        let mut observation = matching_observation(
            windows,
            window,
            target,
            baseline_revision,
            baseline_presented_revision,
        )?;
        observation.poll_count = poll_count;
        if input.condition().matches(UixWindowStateFacts {
            visible: observation.visible,
            presentable: observation.presentable,
            focused: observation.focused,
            width: observation.logical_width,
            height: observation.logical_height,
            maximized: observation.maximized,
            minimized: observation.minimized,
            fullscreen: observation.fullscreen,
        }) {
            return Ok(observation);
        }

        baseline_revision = observation.revision;
        baseline_presented_revision = observation.presented_revision;
        let remaining = super::remaining(client.deadline)?;
        let pause = remaining.min(Duration::from_millis(u64::from(input.poll_interval_ms())));
        if pause.is_zero() {
            return Err(Failure::Timeout);
        }
        thread::sleep(pause);
    }
}

pub(super) fn matching_observation(
    windows: Vec<WireWindow>,
    fixed_window: &WindowRecord,
    target: &str,
    baseline_revision: u64,
    baseline_presented_revision: u64,
) -> Result<WindowStateWaitObservation, Failure> {
    if fixed_window.session_id != target {
        return Err(Failure::Stale);
    }
    if windows.len() > MAX_WINDOWS {
        return Err(Failure::Protocol);
    }
    let mut window_ids = BTreeSet::new();
    let mut candidate = None;
    for wire in windows {
        if !window_ids.insert(wire.window_id) {
            return Err(Failure::Protocol);
        }
        if wire.window_id != fixed_window.window_id {
            continue;
        }
        if wire.generation != fixed_window.generation || wire.closed {
            return Err(Failure::Stale);
        }
        if window_session_id(&fixed_window.endpoint, &wire) != target {
            return Err(Failure::Stale);
        }
        let observation =
            observation_from_wire(&wire, baseline_revision, baseline_presented_revision)?;
        if candidate.replace(observation).is_some() {
            return Err(Failure::Protocol);
        }
    }
    candidate.ok_or(Failure::Stale)
}

fn observation_from_wire(
    wire: &WireWindow,
    baseline_revision: u64,
    baseline_presented_revision: u64,
) -> Result<WindowStateWaitObservation, Failure> {
    if wire.revision < baseline_revision
        || wire.presented_revision < baseline_presented_revision
        || wire.presented_revision > wire.revision
    {
        return Err(Failure::Protocol);
    }
    let (
        Some(logical_width),
        Some(logical_height),
        Some(maximized),
        Some(minimized),
        Some(fullscreen),
        Some(focused),
    ) = (
        wire.logical_width,
        wire.logical_height,
        wire.maximized,
        wire.minimized,
        wire.fullscreen,
        wire.focused,
    )
    else {
        return Err(Failure::Protocol);
    };
    if logical_width < 0 || logical_height < 0 {
        return Err(Failure::Protocol);
    }
    Ok(WindowStateWaitObservation {
        visible: wire.visible,
        presentable: wire.presentable,
        focused,
        logical_width: logical_width as u32,
        logical_height: logical_height as u32,
        maximized,
        minimized,
        fullscreen,
        revision: wire.revision,
        presented_revision: wire.presented_revision,
        poll_count: 0,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixStream,
        thread,
        time::{Duration, Instant},
    };

    use serde_json::{Value, json};

    use super::*;

    fn input(condition: Value) -> UixWindowStateWaitInput {
        let Ok(input) = UixWindowStateWaitInput::parse(&json!({
            "condition": condition,
            "pollIntervalMs": 20,
            "timeoutMs": 500,
        })) else {
            panic!("测试窗口状态等待输入必须有效");
        };
        input
    }

    fn fixture_client(stream: UnixStream) -> AgentClient {
        AgentClient {
            reader: BufReader::new(stream),
            deadline: Instant::now() + Duration::from_secs(2),
            request_types: BTreeSet::from([
                "hello".to_owned(),
                "list_windows".to_owned(),
                "snapshot".to_owned(),
            ]),
            semantic_actions: BTreeSet::new(),
            window_actions: BTreeSet::new(),
            window_state_fields: BTreeSet::from([
                "logical_width".to_owned(),
                "logical_height".to_owned(),
                "maximized".to_owned(),
                "minimized".to_owned(),
                "fullscreen".to_owned(),
                "focused".to_owned(),
            ]),
            key_names: BTreeSet::new(),
            key_modifiers: BTreeSet::new(),
            screenshot_limits: None,
        }
    }

    fn fixture_window() -> WindowRecord {
        let endpoint = super::super::EndpointDescriptor {
            schema: super::super::PROTOCOL_SCHEMA.to_owned(),
            process_id: std::process::id(),
            endpoint: "/fixture/agent.sock".to_owned(),
            token: "7".repeat(64),
            state: "ready".to_owned(),
        };
        let Ok(wire) = serde_json::from_value::<WireWindow>(wire_window(false, 3, 5)) else {
            panic!("测试窗口 wire 必须可反序列化");
        };
        WindowRecord {
            session_id: window_session_id(&endpoint, &wire),
            owner_process_session_id: None,
            title: "fixture".to_owned(),
            visible: true,
            presentable: true,
            focused: Some(false),
            revision: 5,
            presented_revision: 5,
            state: Some(super::super::WindowStateRecord {
                logical_width: 800,
                logical_height: 600,
                maximized: false,
                minimized: false,
                fullscreen: false,
            }),
            screenshot_supported: false,
            activation_supported: false,
            pointer_drag_supported: false,
            endpoint,
            window_id: 7,
            generation: 3,
        }
    }

    fn wire_window(focused: bool, generation: u64, revision: u64) -> Value {
        json!({
            "window_id": 7,
            "generation": generation,
            "title": "fixture",
            "visible": true,
            "presentable": true,
            "logical_width": 800,
            "logical_height": 600,
            "maximized": false,
            "minimized": false,
            "fullscreen": false,
            "focused": focused,
            "revision": revision,
            "presented_revision": revision,
            "closed": false,
        })
    }

    #[test]
    fn polling_uses_one_connection_and_accepts_state_update_without_revision_bump() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            for focused in [false, true] {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                if request["type"] != "list_windows" || request["request_id"] != "act-list-windows"
                {
                    return Err("状态等待必须复用同一 list_windows 连接".to_owned());
                }
                let reply = json!({
                    "schema": super::super::PROTOCOL_SCHEMA,
                    "request_id": "act-list-windows",
                    "ok": true,
                    "type": "list_windows",
                    "windows": [wire_window(focused, 3, 5)]
                });
                writeln!(reader.get_mut(), "{reply}").map_err(|error| error.to_string())?;
            }
            Ok(())
        });
        let mut client = fixture_client(client_stream);
        let window = fixture_window();
        let target = window.session_id.clone();
        let Ok(observation) = execute_wait(
            &mut client,
            &window,
            &target,
            &input(json!({ "type": "focus", "focused": true })),
        ) else {
            panic!("状态变化等待必须成功");
        };
        assert!(observation.focused);
        assert_eq!(observation.revision, 5);
        assert_eq!(observation.presented_revision, 5);
        assert_eq!(observation.poll_count, 2);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }

    #[test]
    fn missing_negotiated_state_field_fails_before_polling() {
        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.window_state_fields.remove("focused");
        let window = fixture_window();
        let target = window.session_id.clone();
        let Err(error) = execute_wait(
            &mut client,
            &window,
            &target,
            &input(json!({ "type": "focus", "focused": true })),
        ) else {
            panic!("缺失 focused 协商字段必须在轮询前失败");
        };
        assert_eq!(error, Failure::Unavailable);
    }

    #[test]
    fn changed_generation_is_rejected_and_negative_size_is_protocol_error() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            let reply = json!({
                "schema": super::super::PROTOCOL_SCHEMA,
                "request_id": "act-list-windows",
                "ok": true,
                "type": "list_windows",
                "windows": [wire_window(false, 4, 5)]
            });
            writeln!(reader.get_mut(), "{reply}").map_err(|error| error.to_string())?;
            Ok(())
        });
        let mut client = fixture_client(client_stream);
        let window = fixture_window();
        let target = window.session_id.clone();
        let Err(error) = execute_wait(
            &mut client,
            &window,
            &target,
            &input(json!({ "type": "focus", "focused": true })),
        ) else {
            panic!("generation 改变必须拒绝");
        };
        assert_eq!(error, Failure::Stale);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");

        let mut malformed = wire_window(false, 3, 5);
        malformed["logical_width"] = json!(-1);
        let Ok(wire) = serde_json::from_value::<WireWindow>(malformed) else {
            panic!("负尺寸 wire fixture 必须可反序列化");
        };
        assert_eq!(observation_from_wire(&wire, 5, 5), Err(Failure::Protocol));
    }

    #[test]
    fn every_poll_recomputes_the_exact_opaque_target() {
        let mut window = fixture_window();
        let target = window.session_id.clone();
        window.endpoint.token = "8".repeat(64);
        let Ok(wire) = serde_json::from_value::<WireWindow>(wire_window(false, 3, 5)) else {
            panic!("测试窗口 wire 必须可反序列化");
        };
        assert_eq!(
            matching_observation(vec![wire], &window, &target, 5, 5),
            Err(Failure::Stale)
        );
    }
}
