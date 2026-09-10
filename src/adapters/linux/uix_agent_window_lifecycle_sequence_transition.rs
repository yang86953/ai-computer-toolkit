//! UIX Agent 窗口生命周期序列与框架当前条件同步的同连接 Adapter。

use std::time::{Duration, Instant};

use crate::components::{
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    uix_window_lifecycle_sequence_transition_contract::UixWindowLifecycleSequenceTransitionInput,
    uix_window_state_wait_contract::UixWindowStateFacts,
};

use super::{
    AgentClient, Failure, WindowLifecycleSequenceFailure, WindowLifecycleSequenceOutcome,
    WindowRecord, WindowStateWaitObservation, resolve_until,
};

/// 区分 mutation 前失败、生命周期序列失败与提交后的框架状态观察失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowLifecycleSequenceTransitionFailureSource {
    Transport(Failure),
    NegotiationUnavailable,
    Sequence(WindowLifecycleSequenceFailure),
    Observation(Failure),
}

/// transition 失败时保留动作进度、最终修订和不可重试边界。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowLifecycleSequenceTransitionFailure {
    pub(crate) source: WindowLifecycleSequenceTransitionFailureSource,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) sequence_completed: bool,
    pub(crate) sequence_revision: Option<u64>,
    pub(crate) sequence_presented_revision: Option<u64>,
    pub(crate) sequence_settled: Option<bool>,
    pub(crate) actions_accepted: u16,
    pub(crate) poll_count: u32,
}

impl WindowLifecycleSequenceTransitionFailure {
    fn before_dispatch(source: WindowLifecycleSequenceTransitionFailureSource) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            actions_accepted: 0,
            poll_count: 0,
        }
    }

    fn sequence(failure: WindowLifecycleSequenceFailure) -> Self {
        Self {
            source: WindowLifecycleSequenceTransitionFailureSource::Sequence(failure),
            accepted_may_have_occurred: failure.accepted_may_have_occurred
                || failure.actions_accepted > 0,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            actions_accepted: failure.actions_accepted,
            poll_count: 0,
        }
    }

    fn after_sequence(
        source: Failure,
        sequence: WindowLifecycleSequenceOutcome,
        poll_count: u32,
    ) -> Self {
        Self {
            source: WindowLifecycleSequenceTransitionFailureSource::Observation(source),
            accepted_may_have_occurred: true,
            sequence_completed: true,
            sequence_revision: Some(sequence.revision),
            sequence_presented_revision: Some(sequence.presented_revision),
            sequence_settled: Some(sequence.settled),
            actions_accepted: sequence.actions_accepted,
            poll_count,
        }
    }
}

/// 完整生命周期序列与提交后框架当前条件均可信时的中立事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowLifecycleSequenceTransitionOutcome {
    pub(crate) sequence: WindowLifecycleSequenceOutcome,
    pub(crate) observation: WindowStateWaitObservation,
}

/// 在同一总 deadline、认证连接和窗口 generation 内执行完整序列并观察最终条件。
pub(crate) fn perform_window_lifecycle_sequence_transition(
    target: &str,
    input: &UixWindowLifecycleSequenceTransitionInput,
) -> Result<WindowLifecycleSequenceTransitionOutcome, WindowLifecycleSequenceTransitionFailure> {
    if OpaqueTargetId::parse(target).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        return Err(WindowLifecycleSequenceTransitionFailure::before_dispatch(
            WindowLifecycleSequenceTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|source| {
        WindowLifecycleSequenceTransitionFailure::before_dispatch(
            WindowLifecycleSequenceTransitionFailureSource::Transport(source),
        )
    })?;
    if window.session_id != target {
        return Err(WindowLifecycleSequenceTransitionFailure::before_dispatch(
            WindowLifecycleSequenceTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|source| {
        WindowLifecycleSequenceTransitionFailure::before_dispatch(
            WindowLifecycleSequenceTransitionFailureSource::Transport(source),
        )
    })?;
    execute_transition(&mut client, &window, target, input)
}

fn execute_transition(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixWindowLifecycleSequenceTransitionInput,
) -> Result<WindowLifecycleSequenceTransitionOutcome, WindowLifecycleSequenceTransitionFailure> {
    // 首个 lifecycle action 前一次性预检 perform、全部动作、状态清单请求与六个状态字段。
    if !["perform", concat!("list_", "windows")]
        .iter()
        .all(|request| client.request_types.contains(*request))
        || input
            .actions()
            .iter()
            .any(|action| !client.window_actions.contains(action.provider_action()))
        || !super::uix_agent_window_state_wait::REQUIRED_WINDOW_STATE_FIELDS
            .iter()
            .all(|field| client.window_state_fields.contains(*field))
    {
        return Err(WindowLifecycleSequenceTransitionFailure::before_dispatch(
            WindowLifecycleSequenceTransitionFailureSource::NegotiationUnavailable,
        ));
    }
    if super::remaining(client.deadline).is_err() {
        return Err(WindowLifecycleSequenceTransitionFailure::before_dispatch(
            WindowLifecycleSequenceTransitionFailureSource::Transport(Failure::Timeout),
        ));
    }

    // 复用的 sequence routine 会在此处以 perform_targetless_with_request_id 连续 dispatch。
    let sequence = super::uix_agent_window_lifecycle_sequence::execute_sequence(
        client,
        window,
        input.sequence(),
    )
    .map_err(WindowLifecycleSequenceTransitionFailure::sequence)?;

    // 只有完整序列取得最终 revision 后，才允许在同一连接轮询 framework-current。
    let mut baseline_revision = sequence.revision;
    let mut baseline_presented_revision = sequence.presented_revision;
    let mut poll_count = 0_u32;
    loop {
        let windows = client.list_windows().map_err(|source| {
            WindowLifecycleSequenceTransitionFailure::after_sequence(source, sequence, poll_count)
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
            WindowLifecycleSequenceTransitionFailure::after_sequence(source, sequence, poll_count)
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
            return Ok(WindowLifecycleSequenceTransitionOutcome {
                sequence,
                observation,
            });
        }

        baseline_revision = observation.revision;
        baseline_presented_revision = observation.presented_revision;
        let remaining = super::remaining(client.deadline).map_err(|source| {
            WindowLifecycleSequenceTransitionFailure::after_sequence(source, sequence, poll_count)
        })?;
        let pause = remaining.min(Duration::from_millis(u64::from(input.poll_interval_ms())));
        if pause.is_zero() {
            return Err(WindowLifecycleSequenceTransitionFailure::after_sequence(
                Failure::Timeout,
                sequence,
                poll_count,
            ));
        }
        std::thread::sleep(pause);
    }
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

    use super::super::WindowActionFailure;
    use super::super::WireWindow;
    use super::*;

    fn input() -> UixWindowLifecycleSequenceTransitionInput {
        let Ok(input) = UixWindowLifecycleSequenceTransitionInput::parse(&json!({
            "actions": [
                { "type": "restore" },
                { "type": "resize", "coordinateSpace": "client-logical-px", "width": 800, "height": 600 },
                { "type": "maximize" }
            ],
            "condition": { "type": "window-flags", "maximized": true },
            "pollIntervalMs": 20,
            "timeoutMs": 1000
        })) else {
            panic!("测试生命周期序列 transition 输入必须有效");
        };
        input
    }

    fn fixture_client(stream: UnixStream) -> AgentClient {
        AgentClient {
            reader: BufReader::new(stream),
            deadline: Instant::now() + Duration::from_secs(2),
            request_types: BTreeSet::from(["perform".to_owned(), "list_windows".to_owned()]),
            semantic_actions: BTreeSet::new(),
            window_actions: BTreeSet::from([
                "restore_window".to_owned(),
                "resize_window".to_owned(),
                "maximize_window".to_owned(),
            ]),
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

    fn wire_window(generation: u64, revision: u64, maximized: bool) -> Value {
        json!({
            "window_id": 7,
            "generation": generation,
            "title": "fixture",
            "visible": true,
            "presentable": true,
            "logical_width": 800,
            "logical_height": 600,
            "maximized": maximized,
            "minimized": false,
            "fullscreen": false,
            "focused": false,
            "revision": revision,
            "presented_revision": revision,
            "closed": false
        })
    }

    fn fixture_window() -> WindowRecord {
        let endpoint = super::super::EndpointDescriptor {
            schema: super::super::PROTOCOL_SCHEMA.to_owned(),
            process_id: std::process::id(),
            endpoint: "/fixture/agent.sock".to_owned(),
            token: "7".repeat(64),
            state: "ready".to_owned(),
        };
        let Ok(wire) = serde_json::from_value::<WireWindow>(wire_window(3, 5, false)) else {
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

    #[test]
    fn missing_state_field_or_action_fails_before_first_dispatch() {
        let window = fixture_window();
        let target = window.session_id.clone();
        for missing in ["focused", "resize_window"] {
            let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
                panic!("测试 Unix 连接必须创建成功");
            };
            let mut client = fixture_client(client_stream);
            if missing == "focused" {
                client.window_state_fields.remove(missing);
            } else {
                client.window_actions.remove(missing);
            }
            let Err(failure) = execute_transition(&mut client, &window, &target, &input()) else {
                panic!("缺失 {missing} 必须在首个动作前失败");
            };
            assert_eq!(
                failure.source,
                WindowLifecycleSequenceTransitionFailureSource::NegotiationUnavailable
            );
            assert!(!failure.accepted_may_have_occurred);
            assert_eq!(failure.actions_accepted, 0);
        }
    }

    #[test]
    fn complete_sequence_polls_same_connection_and_accepts_unsettled_sequence() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            for (index, (request_id, kind)) in [
                ("act-lifecycle-sequence-restore", "restore_window"),
                ("act-lifecycle-sequence-resize", "resize_window"),
                ("act-lifecycle-sequence-maximize", "maximize_window"),
            ]
            .into_iter()
            .enumerate()
            {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                if request["request_id"] != request_id
                    || request["action"]["kind"] != kind
                    || request["expected_revision"] != 5 + index as u64
                {
                    return Err("生命周期序列动作或修订链不匹配".to_owned());
                }
                writeln!(
                    reader.get_mut(),
                    "{}",
                    json!({
                        "schema": super::super::PROTOCOL_SCHEMA,
                        "request_id": request_id,
                        "ok": true,
                        "type": "perform",
                        "window_id": 7,
                        "generation": 3,
                        "revision": 6 + index as u64,
                        "presented_revision": 6 + index as u64,
                        "settled": index != 2
                    })
                )
                .map_err(|error| error.to_string())?;
            }
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            let request =
                serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
            if request["request_id"] != "act-list-windows" || request["type"] != "list_windows" {
                return Err("最终状态必须在同一连接 list_windows".to_owned());
            }
            writeln!(
                reader.get_mut(),
                "{}",
                json!({
                    "schema": super::super::PROTOCOL_SCHEMA,
                    "request_id": "act-list-windows",
                    "ok": true,
                    "type": "list_windows",
                    "windows": [wire_window(3, 8, true)]
                })
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        });

        let mut client = fixture_client(client_stream);
        let window = fixture_window();
        let target = window.session_id.clone();
        let Ok(outcome) = execute_transition(&mut client, &window, &target, &input()) else {
            panic!("完整生命周期序列及最终框架条件必须成功");
        };
        assert_eq!(outcome.sequence.actions_accepted, 3);
        assert_eq!(outcome.sequence.revision, 8);
        assert!(!outcome.sequence.settled);
        assert_eq!(outcome.observation.revision, 8);
        assert_eq!(outcome.observation.poll_count, 1);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }

    #[test]
    fn lost_first_response_is_unknown_and_not_retryable() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || {
            let mut reader = BufReader::new(server_stream);
            let mut line = String::new();
            reader.read_line(&mut line)
        });
        let mut client = fixture_client(client_stream);
        let window = fixture_window();
        let target = window.session_id.clone();
        let Err(failure) = execute_transition(&mut client, &window, &target, &input()) else {
            panic!("首个动作响应丢失不得报告成功");
        };
        assert!(matches!(
            failure.source,
            WindowLifecycleSequenceTransitionFailureSource::Sequence(
                WindowLifecycleSequenceFailure {
                    source: WindowActionFailure::OutcomeUnknown,
                    ..
                }
            )
        ));
        assert!(failure.accepted_may_have_occurred);
        assert!(!failure.sequence_completed);
        assert_eq!(failure.actions_accepted, 0);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器必须读到首个请求");
    }
}
