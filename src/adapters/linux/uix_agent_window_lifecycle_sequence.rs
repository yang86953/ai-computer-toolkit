//! UIX Agent 窗口生命周期序列的私有 Adapter。

use std::time::{Duration, Instant};

use crate::components::uix_window_lifecycle_contract::UixWindowLifecycleAction;
use crate::components::uix_window_lifecycle_sequence_contract::UixWindowLifecycleSequenceInput;

use super::{AgentClient, Failure, WindowActionFailure, WindowRecord, resolve_until};

const EXECUTION_RESERVE: Duration = Duration::from_millis(100);

/// 全部生命周期动作完成后可公开投影的最小事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowLifecycleSequenceOutcome {
    pub(crate) revision: u64,
    pub(crate) presented_revision: u64,
    pub(crate) settled: bool,
    pub(crate) actions_accepted: u16,
}

/// 生命周期序列失败与可能部分执行的保守事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowLifecycleSequenceFailure {
    pub(crate) source: WindowActionFailure,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) actions_accepted: u16,
}

impl WindowLifecycleSequenceFailure {
    fn before_dispatch(source: WindowActionFailure) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            actions_accepted: 0,
        }
    }
}

/// 重新解析窗口后，在同一认证连接内执行有界生命周期序列。
pub(crate) fn perform_window_lifecycle_sequence(
    target: &str,
    input: &UixWindowLifecycleSequenceInput,
) -> Result<WindowLifecycleSequenceOutcome, WindowLifecycleSequenceFailure> {
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|failure| {
        WindowLifecycleSequenceFailure::before_dispatch(WindowActionFailure::Transport(failure))
    })?;
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|failure| {
        WindowLifecycleSequenceFailure::before_dispatch(WindowActionFailure::Transport(failure))
    })?;
    execute_sequence(&mut client, &window, input)
}

pub(super) fn execute_sequence(
    client: &mut AgentClient,
    window: &WindowRecord,
    input: &UixWindowLifecycleSequenceInput,
) -> Result<WindowLifecycleSequenceOutcome, WindowLifecycleSequenceFailure> {
    // perform 与所有实际使用的生命周期动作必须在首个 dispatch 前一次性预检。
    if !client.request_types.contains("perform")
        || input
            .actions()
            .iter()
            .any(|action| !client.window_actions.contains(action.provider_action()))
    {
        return Err(WindowLifecycleSequenceFailure::before_dispatch(
            WindowActionFailure::UnsupportedAction,
        ));
    }
    let required =
        Duration::from_millis(u64::from(input.planned_duration_ms())) + EXECUTION_RESERVE;
    if client.deadline.saturating_duration_since(Instant::now()) < required {
        return Err(WindowLifecycleSequenceFailure::before_dispatch(
            WindowActionFailure::Transport(Failure::Timeout),
        ));
    }

    let interval = Duration::from_millis(u64::from(input.interval_ms()));
    let mut target_time = Instant::now();
    let mut latest_revision = window.revision;
    let mut latest_presented_revision = window.presented_revision;
    let mut latest_settled = true;
    let mut actions_accepted = 0_u16;
    for (index, action) in input.actions().iter().enumerate() {
        if index > 0 {
            target_time += interval;
            if target_time + EXECUTION_RESERVE > client.deadline {
                return Err(WindowLifecycleSequenceFailure {
                    source: WindowActionFailure::Transport(Failure::Timeout),
                    accepted_may_have_occurred: actions_accepted > 0,
                    actions_accepted,
                });
            }
            if let Some(wait) = target_time.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
        }
        let request_id = match action {
            UixWindowLifecycleAction::Restore {} => "act-lifecycle-sequence-restore",
            UixWindowLifecycleAction::Minimize {} => "act-lifecycle-sequence-minimize",
            UixWindowLifecycleAction::Maximize {} => "act-lifecycle-sequence-maximize",
            UixWindowLifecycleAction::Resize { .. } => "act-lifecycle-sequence-resize",
        };
        match client.perform_targetless_with_request_id(
            request_id,
            window.window_id,
            window.generation,
            latest_revision,
            action.provider_value(),
        ) {
            Ok(outcome) => {
                latest_revision = outcome.revision;
                latest_presented_revision = outcome.presented_revision;
                latest_settled = outcome.settled;
                actions_accepted += 1;
            }
            Err(source) => {
                return Err(WindowLifecycleSequenceFailure {
                    source,
                    accepted_may_have_occurred: actions_accepted > 0
                        || dispatch_may_have_occurred(source),
                    actions_accepted,
                });
            }
        }
    }
    Ok(WindowLifecycleSequenceOutcome {
        revision: latest_revision,
        presented_revision: latest_presented_revision,
        settled: latest_settled,
        actions_accepted,
    })
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
    };

    use serde_json::{Value, json};

    use super::*;

    fn input() -> UixWindowLifecycleSequenceInput {
        let Ok(input) = UixWindowLifecycleSequenceInput::parse(&json!({
            "actions": [
                { "type": "restore" },
                { "type": "resize", "coordinateSpace": "client-logical-px", "width": 800, "height": 600 },
                { "type": "maximize" }
            ],
            "timeoutMs": 1000
        })) else {
            panic!("测试生命周期序列必须有效");
        };
        input
    }

    fn fixture_client(stream: UnixStream) -> AgentClient {
        AgentClient {
            reader: BufReader::new(stream),
            deadline: Instant::now() + Duration::from_secs(2),
            request_types: BTreeSet::from(["perform".to_owned()]),
            semantic_actions: BTreeSet::new(),
            window_actions: BTreeSet::from([
                "restore_window".to_owned(),
                "resize_window".to_owned(),
                "maximize_window".to_owned(),
            ]),
            window_state_fields: BTreeSet::new(),
            key_names: BTreeSet::new(),
            key_modifiers: BTreeSet::new(),
            screenshot_limits: None,
        }
    }

    fn fixture_window() -> WindowRecord {
        WindowRecord {
            session_id: "s2:w:0123456789abcdef".to_owned(),
            owner_process_session_id: None,
            title: "fixture".to_owned(),
            visible: true,
            presentable: true,
            focused: None,
            revision: 5,
            presented_revision: 5,
            state: None,
            screenshot_supported: false,
            activation_supported: false,
            pointer_drag_supported: false,
            endpoint: super::super::EndpointDescriptor {
                schema: super::super::PROTOCOL_SCHEMA.to_owned(),
                process_id: std::process::id(),
                endpoint: "/fixture/agent.sock".to_owned(),
                token: "7".repeat(64),
                state: "ready".to_owned(),
            },
            window_id: 7,
            generation: 3,
        }
    }

    #[test]
    fn sequence_uses_one_connection_and_continuous_revision_chain() {
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
                    return Err("生命周期动作或修订链不匹配".to_owned());
                }
                let reply = json!({
                    "schema": super::super::PROTOCOL_SCHEMA,
                    "request_id": request["request_id"],
                    "ok": true,
                    "type": "perform",
                    "window_id": 7,
                    "generation": 3,
                    "revision": 6 + index as u64,
                    "presented_revision": 6 + index as u64,
                    "settled": true
                });
                writeln!(reader.get_mut(), "{reply}").map_err(|error| error.to_string())?;
            }
            Ok(())
        });
        let mut client = fixture_client(client_stream);
        let Ok(outcome) = execute_sequence(&mut client, &fixture_window(), &input()) else {
            panic!("测试生命周期序列必须成功");
        };
        assert_eq!(outcome.actions_accepted, 3);
        assert_eq!(outcome.revision, 8);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }

    #[test]
    fn missing_later_action_fails_before_first_dispatch() {
        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.window_actions.remove("resize_window");
        let Err(failure) = execute_sequence(&mut client, &fixture_window(), &input()) else {
            panic!("缺失后续生命周期动作必须在首个 dispatch 前失败");
        };
        assert_eq!(failure.source, WindowActionFailure::UnsupportedAction);
        assert!(!failure.accepted_may_have_occurred);
        assert_eq!(failure.actions_accepted, 0);
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
        let Err(failure) = execute_sequence(&mut client, &fixture_window(), &input()) else {
            panic!("首个响应丢失不得报告生命周期序列成功");
        };
        assert_eq!(failure.source, WindowActionFailure::OutcomeUnknown);
        assert!(failure.accepted_may_have_occurred);
        assert_eq!(failure.actions_accepted, 0);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器必须读到首个请求");
    }
}
