//! UIX Agent 应用内键盘与指针混合序列的私有 Adapter。

use std::{
    thread,
    time::{Duration, Instant},
};

use crate::components::uix_input_sequence_contract::{
    UixInputSequenceInput, UixInputSequenceStepKind,
};

use super::{AgentClient, Failure, WindowActionFailure, WindowRecord, resolve_until};

const EXECUTION_RESERVE: Duration = Duration::from_millis(100);

/// 全部混合步骤完成后可公开投影的最小事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InputSequenceOutcome {
    pub(crate) revision: u64,
    pub(crate) presented_revision: u64,
    pub(crate) settled: bool,
    pub(crate) steps_accepted: u16,
    pub(crate) presses_accepted: u16,
    pub(crate) moves_accepted: u16,
    pub(crate) clicks_accepted: u16,
}

/// 混合序列失败与可能部分执行的保守事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InputSequenceFailure {
    pub(crate) source: WindowActionFailure,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) steps_accepted: u16,
    pub(crate) presses_accepted: u16,
    pub(crate) moves_accepted: u16,
    pub(crate) clicks_accepted: u16,
}

impl InputSequenceFailure {
    fn before_dispatch(source: WindowActionFailure) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            steps_accepted: 0,
            presses_accepted: 0,
            moves_accepted: 0,
            clicks_accepted: 0,
        }
    }
}

/// 重新解析窗口后，在同一认证连接内执行有界交错输入序列。
pub(crate) fn perform_input_sequence(
    target: &str,
    input: &UixInputSequenceInput,
) -> Result<InputSequenceOutcome, InputSequenceFailure> {
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|failure| {
        InputSequenceFailure::before_dispatch(WindowActionFailure::Transport(failure))
    })?;
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|failure| {
        InputSequenceFailure::before_dispatch(WindowActionFailure::Transport(failure))
    })?;
    execute_sequence(&mut client, &window, input)
}

pub(super) fn execute_sequence(
    client: &mut AgentClient,
    window: &WindowRecord,
    input: &UixInputSequenceInput,
) -> Result<InputSequenceOutcome, InputSequenceFailure> {
    // perform、实际动作和全部键目录必须在首个步骤前一次性预检。
    if !client.request_types.contains("perform")
        || (input.presses_requested() > 0 && !client.window_actions.contains("press_key"))
        || (input.moves_requested() > 0 && !client.window_actions.contains("pointer_move"))
        || (input.clicks_requested() > 0 && !client.window_actions.contains("click_at"))
        || input.steps().iter().any(|step| {
            step.kind() == UixInputSequenceStepKind::Press
                && (step
                    .provider_key()
                    .is_none_or(|key| !client.key_names.contains(key))
                    || step.provider_modifiers().is_none_or(|modifiers| {
                        modifiers
                            .iter()
                            .any(|modifier| !client.key_modifiers.contains(*modifier))
                    }))
        })
    {
        return Err(InputSequenceFailure::before_dispatch(
            WindowActionFailure::UnsupportedAction,
        ));
    }
    let required =
        Duration::from_millis(u64::from(input.planned_duration_ms())) + EXECUTION_RESERVE;
    if client.deadline.saturating_duration_since(Instant::now()) < required {
        return Err(InputSequenceFailure::before_dispatch(
            WindowActionFailure::Transport(Failure::Timeout),
        ));
    }

    let sequence_started = Instant::now();
    let mut latest_revision = window.revision;
    let mut latest_presented_revision = window.presented_revision;
    let mut latest_settled = true;
    let mut steps_accepted = 0_u16;
    let mut presses_accepted = 0_u16;
    let mut moves_accepted = 0_u16;
    let mut clicks_accepted = 0_u16;
    for (index, step) in input.steps().iter().enumerate() {
        if index > 0 {
            let planned_elapsed =
                Duration::from_millis(u64::from(input.interval_ms()).saturating_mul(index as u64));
            let target_time = sequence_started + planned_elapsed;
            if target_time + EXECUTION_RESERVE > client.deadline {
                return Err(InputSequenceFailure {
                    source: WindowActionFailure::Transport(Failure::Timeout),
                    accepted_may_have_occurred: steps_accepted > 0,
                    steps_accepted,
                    presses_accepted,
                    moves_accepted,
                    clicks_accepted,
                });
            }
            if let Some(wait) = target_time.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
        let request_id = match step.provider_action() {
            "press_key" => "act-input-sequence-press",
            "pointer_move" => "act-input-sequence-move",
            "click_at" => "act-input-sequence-click",
            _ => {
                return Err(InputSequenceFailure {
                    source: WindowActionFailure::UnsupportedAction,
                    accepted_may_have_occurred: steps_accepted > 0,
                    steps_accepted,
                    presses_accepted,
                    moves_accepted,
                    clicks_accepted,
                });
            }
        };
        match client.perform_targetless_with_request_id(
            request_id,
            window.window_id,
            window.generation,
            latest_revision,
            step.provider_value(),
        ) {
            Ok(outcome) => {
                latest_revision = outcome.revision;
                latest_presented_revision = outcome.presented_revision;
                latest_settled = outcome.settled;
                steps_accepted += 1;
                match step.kind() {
                    UixInputSequenceStepKind::Press => presses_accepted += 1,
                    UixInputSequenceStepKind::Move => moves_accepted += 1,
                    UixInputSequenceStepKind::Click => clicks_accepted += 1,
                }
            }
            Err(source) => {
                return Err(InputSequenceFailure {
                    source,
                    accepted_may_have_occurred: steps_accepted > 0
                        || dispatch_may_have_occurred(source),
                    steps_accepted,
                    presses_accepted,
                    moves_accepted,
                    clicks_accepted,
                });
            }
        }
    }
    Ok(InputSequenceOutcome {
        revision: latest_revision,
        presented_revision: latest_presented_revision,
        settled: latest_settled,
        steps_accepted,
        presses_accepted,
        moves_accepted,
        clicks_accepted,
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

    fn input() -> UixInputSequenceInput {
        let Ok(input) = UixInputSequenceInput::parse(&json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "press", "key": "a", "modifiers": ["control"] },
                { "type": "move", "x": 10.0, "y": 20.0 },
                { "type": "click", "x": 30.0, "y": 40.0 }
            ],
            "timeoutMs": 1000
        })) else {
            panic!("测试混合输入序列必须有效");
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
                "press_key".to_owned(),
                "pointer_move".to_owned(),
                "click_at".to_owned(),
            ]),
            window_state_fields: BTreeSet::new(),
            key_names: BTreeSet::from(["a".to_owned()]),
            key_modifiers: BTreeSet::from(["ctrl".to_owned()]),
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

    fn serve_steps(
        server_stream: UnixStream,
        replies: usize,
    ) -> thread::JoinHandle<Result<(), String>> {
        thread::spawn(move || {
            let mut reader = BufReader::new(server_stream);
            for index in 0..replies {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                let expected_kind = match index {
                    0 => "press_key",
                    1 => "pointer_move",
                    _ => "click_at",
                };
                if request["action"]["kind"] != expected_kind
                    || request["expected_revision"] != 5 + index as u64
                {
                    return Err("混合步骤或连续修订不匹配".to_owned());
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
        })
    }

    #[test]
    fn sequence_uses_one_connection_and_interleaves_revision_chain() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = serve_steps(server_stream, 3);
        let mut client = fixture_client(client_stream);
        let Ok(outcome) = execute_sequence(&mut client, &fixture_window(), &input()) else {
            panic!("测试混合输入序列必须成功");
        };
        assert_eq!(outcome.steps_accepted, 3);
        assert_eq!(outcome.presses_accepted, 1);
        assert_eq!(outcome.moves_accepted, 1);
        assert_eq!(outcome.clicks_accepted, 1);
        assert_eq!(outcome.revision, 8);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }

    #[test]
    fn missing_later_action_or_key_fails_before_first_dispatch() {
        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.window_actions.remove("click_at");
        let Err(failure) = execute_sequence(&mut client, &fixture_window(), &input()) else {
            panic!("缺失后续动作必须在首个 dispatch 前失败");
        };
        assert_eq!(failure.source, WindowActionFailure::UnsupportedAction);
        assert!(!failure.accepted_may_have_occurred);
        assert_eq!(failure.steps_accepted, 0);

        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.key_names.remove("a");
        let Err(failure) = execute_sequence(&mut client, &fixture_window(), &input()) else {
            panic!("缺失后续键目录必须在首个 dispatch 前失败");
        };
        assert_eq!(failure.source, WindowActionFailure::UnsupportedAction);
        assert!(!failure.accepted_may_have_occurred);
        assert_eq!(failure.steps_accepted, 0);
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
            panic!("首个响应丢失不得报告混合序列成功");
        };
        assert_eq!(failure.source, WindowActionFailure::OutcomeUnknown);
        assert!(failure.accepted_may_have_occurred);
        assert_eq!(failure.steps_accepted, 0);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器必须读到首个请求");
    }

    #[test]
    fn later_unknown_preserves_accepted_counts() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || {
            let mut reader = BufReader::new(server_stream);
            let mut line = String::new();
            let first = reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            if first == 0 {
                return Err("服务器未读到首个请求".to_owned());
            }
            let request =
                serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
            let reply = json!({
                "schema": super::super::PROTOCOL_SCHEMA,
                "request_id": request["request_id"],
                "ok": true,
                "type": "perform",
                "window_id": 7,
                "generation": 3,
                "revision": 6,
                "presented_revision": 6,
                "settled": true
            });
            writeln!(reader.get_mut(), "{reply}").map_err(|error| error.to_string())?;
            let mut second = String::new();
            reader
                .read_line(&mut second)
                .map_err(|error| error.to_string())?;
            Ok(())
        });
        let mut client = fixture_client(client_stream);
        let Err(failure) = execute_sequence(&mut client, &fixture_window(), &input()) else {
            panic!("后续响应丢失不得报告混合序列成功");
        };
        assert_eq!(failure.source, WindowActionFailure::OutcomeUnknown);
        assert!(failure.accepted_may_have_occurred);
        assert_eq!(failure.steps_accepted, 1);
        assert_eq!(failure.presses_accepted, 1);
        assert_eq!(failure.moves_accepted, 0);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }
}
