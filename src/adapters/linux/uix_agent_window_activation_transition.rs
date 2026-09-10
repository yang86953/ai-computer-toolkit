//! UIX Agent 窗口激活与精确 generation 焦点观察的同连接 Adapter。

use std::{
    collections::BTreeSet,
    thread,
    time::{Duration, Instant},
};

use serde_json::json;

use crate::components::{
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    uix_window_activation_transition_contract::UixWindowActivationTransitionInput,
};

use super::{
    ActionOutcome, AgentClient, Failure, MAX_WINDOWS, WindowActionFailure, WindowRecord,
    WireWindow, resolve_until, window_session_id,
};

const TRANSITION_REQUEST_ID: &str = "act-activation-transition";

/// 激活提交后一次精确窗口焦点观察。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowFocusObservation {
    pub(crate) focused: bool,
    pub(crate) revision: u64,
    pub(crate) presented_revision: u64,
    pub(crate) poll_count: u32,
}

/// 激活动作响应与提交后焦点命中的中立事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowActivationTransitionOutcome {
    pub(crate) action: ActionOutcome,
    pub(crate) observation: WindowFocusObservation,
}

/// transition 失败时公开所需的最小动作与观察事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowActivationTransitionFailure {
    pub(crate) source: WindowActionFailure,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) action_accepted: bool,
    pub(crate) action_revision: Option<u64>,
    pub(crate) action_presented_revision: Option<u64>,
    pub(crate) action_settled: Option<bool>,
    pub(crate) poll_count: u32,
}

impl WindowActivationTransitionFailure {
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

    fn action(source: WindowActionFailure) -> Self {
        Self {
            source,
            accepted_may_have_occurred: dispatch_may_have_occurred(source),
            action_accepted: false,
            action_revision: None,
            action_presented_revision: None,
            action_settled: None,
            poll_count: 0,
        }
    }

    fn after_dispatch(source: Failure, action: ActionOutcome, poll_count: u32) -> Self {
        Self {
            source: WindowActionFailure::Transport(source),
            accepted_may_have_occurred: true,
            action_accepted: true,
            action_revision: Some(action.revision),
            action_presented_revision: Some(action.presented_revision),
            action_settled: Some(action.settled),
            poll_count,
        }
    }
}

/// 在一个总 deadline 内只解析一次目标，并在同一认证连接等待焦点命中。
pub(crate) fn perform_window_activation_transition(
    target: &str,
    input: &UixWindowActivationTransitionInput,
) -> Result<WindowActivationTransitionOutcome, WindowActivationTransitionFailure> {
    if OpaqueTargetId::parse(target).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        return Err(WindowActivationTransitionFailure::before_dispatch(
            WindowActionFailure::Transport(Failure::Stale),
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|source| {
        WindowActivationTransitionFailure::before_dispatch(WindowActionFailure::Transport(source))
    })?;
    if window.session_id != target {
        return Err(WindowActivationTransitionFailure::before_dispatch(
            WindowActionFailure::Transport(Failure::Stale),
        ));
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|source| {
        WindowActivationTransitionFailure::before_dispatch(WindowActionFailure::Transport(source))
    })?;
    execute_transition(&mut client, &window, target, input)
}

fn execute_transition(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixWindowActivationTransitionInput,
) -> Result<WindowActivationTransitionOutcome, WindowActivationTransitionFailure> {
    // mutation 前一次性验证动作、清单读取与唯一需要的 focused 字段。
    if !client.request_types.contains("perform")
        || !client.request_types.contains("list_windows")
        || !client.window_actions.contains("activate_window")
        || !client.window_state_fields.contains("focused")
    {
        return Err(WindowActivationTransitionFailure::before_dispatch(
            WindowActionFailure::UnsupportedAction,
        ));
    }
    if super::remaining(client.deadline).is_err() {
        return Err(WindowActivationTransitionFailure::before_dispatch(
            WindowActionFailure::Transport(Failure::Timeout),
        ));
    }

    let action = client
        .perform_targetless_with_request_id(
            TRANSITION_REQUEST_ID,
            window.window_id,
            window.generation,
            window.revision,
            json!({ "kind": "activate_window" }),
        )
        .map_err(WindowActivationTransitionFailure::action)?;

    let mut baseline_revision = action.revision;
    let mut baseline_presented_revision = action.presented_revision;
    let mut poll_count = 0_u32;
    loop {
        let windows = client.list_windows().map_err(|source| {
            WindowActivationTransitionFailure::after_dispatch(source, action, poll_count)
        })?;
        poll_count = poll_count.saturating_add(1);
        let mut observation = matching_focus_observation(
            windows,
            window,
            target,
            baseline_revision,
            baseline_presented_revision,
        )
        .map_err(|source| {
            WindowActivationTransitionFailure::after_dispatch(source, action, poll_count)
        })?;
        observation.poll_count = poll_count;
        if observation.focused {
            return Ok(WindowActivationTransitionOutcome {
                action,
                observation,
            });
        }

        baseline_revision = observation.revision;
        baseline_presented_revision = observation.presented_revision;
        let remaining = super::remaining(client.deadline).map_err(|source| {
            WindowActivationTransitionFailure::after_dispatch(source, action, poll_count)
        })?;
        let pause = remaining.min(Duration::from_millis(u64::from(input.poll_interval_ms())));
        if pause.is_zero() {
            return Err(WindowActivationTransitionFailure::after_dispatch(
                Failure::Timeout,
                action,
                poll_count,
            ));
        }
        thread::sleep(pause);
    }
}

fn matching_focus_observation(
    windows: Vec<WireWindow>,
    fixed_window: &WindowRecord,
    target: &str,
    baseline_revision: u64,
    baseline_presented_revision: u64,
) -> Result<WindowFocusObservation, Failure> {
    if fixed_window.session_id != target || windows.len() > MAX_WINDOWS {
        return Err(if fixed_window.session_id == target {
            Failure::Protocol
        } else {
            Failure::Stale
        });
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
        if wire.generation != fixed_window.generation
            || wire.closed
            || window_session_id(&fixed_window.endpoint, &wire) != target
        {
            return Err(Failure::Stale);
        }
        if wire.revision < baseline_revision
            || wire.presented_revision < baseline_presented_revision
            || wire.presented_revision > wire.revision
        {
            return Err(Failure::Protocol);
        }
        let Some(focused) = wire.focused else {
            return Err(Failure::Protocol);
        };
        let observation = WindowFocusObservation {
            focused,
            revision: wire.revision,
            presented_revision: wire.presented_revision,
            poll_count: 0,
        };
        if candidate.replace(observation).is_some() {
            return Err(Failure::Protocol);
        }
    }
    candidate.ok_or(Failure::Stale)
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
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixStream,
    };

    use serde_json::Value;

    use super::*;

    fn input() -> UixWindowActivationTransitionInput {
        let Ok(input) = UixWindowActivationTransitionInput::parse(&json!({
            "pollIntervalMs": 20,
            "timeoutMs": 500,
        })) else {
            panic!("测试激活 transition 输入必须有效");
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
                "perform".to_owned(),
            ]),
            semantic_actions: BTreeSet::new(),
            window_actions: BTreeSet::from(["activate_window".to_owned()]),
            window_state_fields: BTreeSet::from(["focused".to_owned()]),
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
        let wire_window = super::super::WireWindow {
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
            focused: Some(false),
            revision: 5,
            presented_revision: 5,
            closed: false,
        };
        WindowRecord {
            session_id: window_session_id(&endpoint, &wire_window),
            owner_process_session_id: None,
            title: "fixture".to_owned(),
            visible: true,
            presentable: true,
            focused: Some(false),
            revision: 5,
            presented_revision: 5,
            state: None,
            screenshot_supported: false,
            activation_supported: true,
            pointer_drag_supported: false,
            endpoint,
            window_id: 7,
            generation: 3,
        }
    }

    fn fixture_target() -> String {
        fixture_window().session_id
    }

    fn performed() -> Value {
        json!({
            "schema": super::super::PROTOCOL_SCHEMA,
            "ok": true,
            "type": "perform",
            "request_id": TRANSITION_REQUEST_ID,
            "window_id": 7,
            "generation": 3,
            "revision": 6,
            "presented_revision": 6,
            "settled": true,
        })
    }

    fn windows(focused: bool) -> Value {
        json!({
            "schema": super::super::PROTOCOL_SCHEMA,
            "ok": true,
            "type": "list_windows",
            "request_id": "act-list-windows",
            "windows": [{
                "window_id": 7,
                "generation": 3,
                "title": "fixture",
                "visible": true,
                "presentable": true,
                "focused": focused,
                "revision": 6,
                "presented_revision": 6,
                "closed": false,
            }],
        })
    }

    #[test]
    fn missing_focused_negotiation_fails_before_activation_request() {
        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.window_state_fields.clear();
        let Err(failure) =
            execute_transition(&mut client, &fixture_window(), &fixture_target(), &input())
        else {
            panic!("缺失 focused 必须在激活前失败");
        };
        assert_eq!(failure.source, WindowActionFailure::UnsupportedAction);
        assert!(!failure.accepted_may_have_occurred);
    }

    #[test]
    fn activation_and_focus_polling_share_one_connection() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || {
            let mut reader = BufReader::new(server_stream);
            for reply in [performed(), windows(false), windows(true)] {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).is_ok());
                let Ok(request) = serde_json::from_str::<Value>(&line) else {
                    panic!("测试请求必须为 JSON");
                };
                if reply["type"] == "perform" {
                    assert_eq!(request["type"], "perform");
                    assert_eq!(request["action"]["kind"], "activate_window");
                } else {
                    assert_eq!(request["type"], "list_windows");
                }
                assert!(writeln!(reader.get_mut(), "{reply}").is_ok());
            }
        });
        let mut client = fixture_client(client_stream);
        let Ok(outcome) =
            execute_transition(&mut client, &fixture_window(), &fixture_target(), &input())
        else {
            panic!("同连接激活后焦点命中必须成功");
        };
        assert_eq!(outcome.action.revision, 6);
        assert!(outcome.observation.focused);
        assert_eq!(outcome.observation.poll_count, 2);
        assert!(server.join().is_ok());
    }

    #[test]
    fn post_activation_connection_loss_is_unknown_and_non_retryable() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || {
            let mut reader = BufReader::new(server_stream);
            let mut line = String::new();
            assert!(reader.read_line(&mut line).is_ok());
            assert!(writeln!(reader.get_mut(), "{}", performed()).is_ok());
        });
        let mut client = fixture_client(client_stream);
        let Err(failure) =
            execute_transition(&mut client, &fixture_window(), &fixture_target(), &input())
        else {
            panic!("激活后断连必须失去可信焦点终态");
        };
        assert!(failure.accepted_may_have_occurred);
        assert!(failure.action_accepted);
        assert_eq!(failure.action_revision, Some(6));
        assert!(server.join().is_ok());
    }
}
