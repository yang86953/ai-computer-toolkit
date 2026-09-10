//! UIX Agent 窗口 lifecycle transition 的同连接私有 Adapter。

use std::{
    thread,
    time::{Duration, Instant},
};

use crate::components::{
    uix_window_lifecycle_transition_contract::UixWindowLifecycleTransitionInput,
    uix_window_state_wait_contract::UixWindowStateFacts,
};

use super::{
    AgentClient, Failure, WindowActionFailure, WindowRecord, WindowStateWaitObservation,
    resolve_until,
};

const TRANSITION_REQUEST_ID: &str = "act-lifecycle-transition";

/// action response 与最终 framework-current 观察的中立事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowLifecycleTransitionOutcome {
    pub(crate) action_revision: u64,
    pub(crate) action_presented_revision: u64,
    pub(crate) action_settled: bool,
    pub(crate) observation: WindowStateWaitObservation,
}

/// transition 失败的安全投影，保留 dispatch 后不确定性与已接受事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowLifecycleTransitionFailure {
    pub(crate) source: WindowActionFailure,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) action_accepted: bool,
    pub(crate) action_revision: Option<u64>,
    pub(crate) action_presented_revision: Option<u64>,
    pub(crate) action_settled: Option<bool>,
    pub(crate) poll_count: u32,
}

impl WindowLifecycleTransitionFailure {
    fn before_dispatch(source: WindowActionFailure) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            action_accepted: false,
            action_revision: None,
            action_presented_revision: None,
            action_settled: None,
            poll_count: 0,
        }
    }

    fn after_dispatch(
        source: WindowActionFailure,
        action_revision: u64,
        action_presented_revision: u64,
        action_settled: bool,
        poll_count: u32,
    ) -> Self {
        Self {
            source,
            accepted_may_have_occurred: true,
            action_accepted: true,
            action_revision: Some(action_revision),
            action_presented_revision: Some(action_presented_revision),
            action_settled: Some(action_settled),
            poll_count,
        }
    }
}

/// 在总 deadline 内只 resolve 一次、认证一次并执行完整 transition。
pub(crate) fn perform_window_lifecycle_transition(
    target: &str,
    input: &UixWindowLifecycleTransitionInput,
) -> Result<WindowLifecycleTransitionOutcome, WindowLifecycleTransitionFailure> {
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|failure| {
        WindowLifecycleTransitionFailure::before_dispatch(WindowActionFailure::Transport(failure))
    })?;
    if window.session_id != target {
        return Err(WindowLifecycleTransitionFailure::before_dispatch(
            WindowActionFailure::Transport(Failure::Stale),
        ));
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|failure| {
        WindowLifecycleTransitionFailure::before_dispatch(WindowActionFailure::Transport(failure))
    })?;
    execute_transition(&mut client, &window, target, input)
}

fn execute_transition(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixWindowLifecycleTransitionInput,
) -> Result<WindowLifecycleTransitionOutcome, WindowLifecycleTransitionFailure> {
    // mutation 前一次性验证 perform、具体动作和六个 framework state 字段。
    if !client.request_types.contains("perform")
        || !client
            .window_actions
            .contains(input.action().provider_action())
        || !super::uix_agent_window_state_wait::REQUIRED_WINDOW_STATE_FIELDS
            .iter()
            .all(|field| client.window_state_fields.contains(*field))
    {
        return Err(WindowLifecycleTransitionFailure::before_dispatch(
            WindowActionFailure::UnsupportedAction,
        ));
    }
    if super::remaining(client.deadline).is_err() {
        return Err(WindowLifecycleTransitionFailure::before_dispatch(
            WindowActionFailure::Transport(Failure::Timeout),
        ));
    }

    let action = match client.perform_targetless_with_request_id(
        TRANSITION_REQUEST_ID,
        window.window_id,
        window.generation,
        window.revision,
        input.action().provider_value(),
    ) {
        Ok(action) => action,
        Err(source) => {
            let accepted_may_have_occurred = dispatch_may_have_occurred(source);
            return Err(WindowLifecycleTransitionFailure {
                source,
                accepted_may_have_occurred,
                action_accepted: false,
                action_revision: None,
                action_presented_revision: None,
                action_settled: None,
                poll_count: 0,
            });
        }
    };

    let mut baseline_revision = action.revision;
    let mut baseline_presented_revision = action.presented_revision;
    let mut poll_count = 0_u32;
    loop {
        let windows = client.list_windows().map_err(|source| {
            WindowLifecycleTransitionFailure::after_dispatch(
                WindowActionFailure::Transport(source),
                action.revision,
                action.presented_revision,
                action.settled,
                poll_count,
            )
        })?;
        poll_count = poll_count.saturating_add(1);
        let mut observation = super::uix_agent_window_state_wait::matching_observation(
            windows,
            window,
            target,
            baseline_revision,
            baseline_presented_revision,
        )
        .map_err(|source| {
            WindowLifecycleTransitionFailure::after_dispatch(
                WindowActionFailure::Transport(source),
                action.revision,
                action.presented_revision,
                action.settled,
                poll_count,
            )
        })?;
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
            return Ok(WindowLifecycleTransitionOutcome {
                action_revision: action.revision,
                action_presented_revision: action.presented_revision,
                action_settled: action.settled,
                observation,
            });
        }

        baseline_revision = observation.revision;
        baseline_presented_revision = observation.presented_revision;
        let remaining = super::remaining(client.deadline).map_err(|source| {
            WindowLifecycleTransitionFailure::after_dispatch(
                WindowActionFailure::Transport(source),
                action.revision,
                action.presented_revision,
                action.settled,
                poll_count,
            )
        })?;
        let pause = remaining.min(Duration::from_millis(u64::from(input.poll_interval_ms())));
        if pause.is_zero() {
            return Err(WindowLifecycleTransitionFailure::after_dispatch(
                WindowActionFailure::Transport(Failure::Timeout),
                action.revision,
                action.presented_revision,
                action.settled,
                poll_count,
            ));
        }
        thread::sleep(pause);
    }
}

fn dispatch_may_have_occurred(source: WindowActionFailure) -> bool {
    matches!(
        source,
        WindowActionFailure::Transport(
            Failure::Protocol | Failure::Unavailable | Failure::Ambiguous
        ) | WindowActionFailure::NotInteractable
            | WindowActionFailure::WindowOperationFailed
            | WindowActionFailure::DidNotSettle
            | WindowActionFailure::OutcomeUnknown
    )
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

    use super::super::WireWindow;
    use super::*;

    fn input() -> UixWindowLifecycleTransitionInput {
        let Ok(input) = UixWindowLifecycleTransitionInput::parse(&json!({
            "action": { "type": "maximize" },
            "condition": { "type": "focus", "focused": true },
            "pollIntervalMs": 20,
            "timeoutMs": 1000
        })) else {
            panic!("测试 transition 输入必须有效");
        };
        input
    }

    fn fixture_client(stream: UnixStream) -> AgentClient {
        AgentClient {
            reader: BufReader::new(stream),
            deadline: Instant::now() + Duration::from_secs(2),
            request_types: BTreeSet::from(["perform".to_owned(), "list_windows".to_owned()]),
            semantic_actions: BTreeSet::new(),
            window_actions: BTreeSet::from(["maximize_window".to_owned()]),
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
            session_id: super::super::window_session_id(&endpoint, &wire),
            owner_process_session_id: None,
            title: "fixture".to_owned(),
            visible: true,
            presentable: true,
            focused: Some(false),
            revision: 5,
            presented_revision: 5,
            state: None,
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
            "maximized": true,
            "minimized": false,
            "fullscreen": false,
            "focused": focused,
            "revision": revision,
            "presented_revision": revision,
            "closed": false,
        })
    }

    #[test]
    fn missing_state_field_or_action_fails_before_first_dispatch() {
        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.window_state_fields.remove("focused");
        let window = fixture_window();
        let target = window.session_id.clone();
        let Err(failure) = execute_transition(&mut client, &window, &target, &input()) else {
            panic!("缺失 state 字段必须在 dispatch 前失败");
        };
        assert_eq!(failure.source, WindowActionFailure::UnsupportedAction);
        assert!(!failure.accepted_may_have_occurred);
        assert!(!failure.action_accepted);

        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.window_actions.remove("maximize_window");
        let Err(failure) = execute_transition(&mut client, &window, &target, &input()) else {
            panic!("缺失具体动作必须在 dispatch 前失败");
        };
        assert_eq!(failure.source, WindowActionFailure::UnsupportedAction);
        assert_eq!(failure.poll_count, 0);
    }

    #[test]
    fn action_then_poll_uses_one_connection_and_accepts_unsettled_action() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            let request =
                serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
            if request["request_id"] != TRANSITION_REQUEST_ID
                || request["type"] != "perform"
                || request["expected_revision"] != 5
                || request["action"]["kind"] != "maximize_window"
            {
                return Err("transition 首个请求不匹配".to_owned());
            }
            writeln!(
                reader.get_mut(),
                "{}",
                json!({
                    "schema": super::super::PROTOCOL_SCHEMA,
                    "request_id": TRANSITION_REQUEST_ID,
                    "ok": true,
                    "type": "perform",
                    "window_id": 7,
                    "generation": 3,
                    "revision": 6,
                    "presented_revision": 6,
                    "settled": false
                })
            )
            .map_err(|error| error.to_string())?;

            line.clear();
            reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            let poll_request =
                serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
            if poll_request["request_id"] != "act-list-windows"
                || poll_request["type"] != "list_windows"
            {
                return Err("transition 必须在同一连接 list_windows".to_owned());
            }
            writeln!(
                reader.get_mut(),
                "{}",
                json!({
                    "schema": super::super::PROTOCOL_SCHEMA,
                    "request_id": "act-list-windows",
                    "ok": true,
                    "type": "list_windows",
                    "windows": [wire_window(true, 3, 6)]
                })
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        });

        let mut client = fixture_client(client_stream);
        let window = fixture_window();
        let target = window.session_id.clone();
        let Ok(outcome) = execute_transition(&mut client, &window, &target, &input()) else {
            panic!("action 后 framework condition 命中必须成功");
        };
        assert_eq!(outcome.action_revision, 6);
        assert!(!outcome.action_settled);
        assert_eq!(outcome.observation.revision, 6);
        assert_eq!(outcome.observation.poll_count, 1);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }

    #[test]
    fn post_dispatch_poll_timeout_is_unknown_and_non_retryable() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            writeln!(
                reader.get_mut(),
                "{}",
                json!({
                    "schema": super::super::PROTOCOL_SCHEMA,
                    "request_id": TRANSITION_REQUEST_ID,
                    "ok": true,
                    "type": "perform",
                    "window_id": 7,
                    "generation": 3,
                    "revision": 6,
                    "presented_revision": 6,
                    "settled": true
                })
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        });
        let mut client = fixture_client(client_stream);
        let window = fixture_window();
        let target = window.session_id.clone();
        let Err(failure) = execute_transition(&mut client, &window, &target, &input()) else {
            panic!("dispatch 后 polling 连接丢失不得伪报成功");
        };
        assert!(failure.accepted_may_have_occurred);
        assert!(failure.action_accepted);
        assert_eq!(failure.action_revision, Some(6));
        assert_eq!(failure.poll_count, 0);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器必须正常停止");
    }
}
