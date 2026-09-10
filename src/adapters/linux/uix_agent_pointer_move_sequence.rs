//! UIX Agent 应用内多点 pointer_move 序列的私有 Adapter。

use std::{
    thread,
    time::{Duration, Instant},
};

use crate::components::uix_pointer_move_sequence_contract::UixPointerMoveSequenceInput;

use super::{AgentClient, Failure, WindowActionFailure, WindowRecord, resolve_until};

const EXECUTION_RESERVE: Duration = Duration::from_millis(100);

/// 全部 pointer_move 完成后可公开投影的最小事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PointerMoveSequenceOutcome {
    pub(crate) revision: u64,
    pub(crate) presented_revision: u64,
    pub(crate) settled: bool,
    pub(crate) moves_accepted: u16,
}

/// pointer_move 序列失败与可能部分执行的保守事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PointerMoveSequenceFailure {
    pub(crate) source: WindowActionFailure,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) moves_accepted: u16,
}

impl PointerMoveSequenceFailure {
    fn before_dispatch(source: WindowActionFailure) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            moves_accepted: 0,
        }
    }
}

/// 重新解析窗口后，在同一认证连接内执行有界 pointer_move 序列。
pub(crate) fn perform_pointer_move_sequence(
    target: &str,
    input: &UixPointerMoveSequenceInput,
) -> Result<PointerMoveSequenceOutcome, PointerMoveSequenceFailure> {
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|failure| {
        PointerMoveSequenceFailure::before_dispatch(WindowActionFailure::Transport(failure))
    })?;
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|failure| {
        PointerMoveSequenceFailure::before_dispatch(WindowActionFailure::Transport(failure))
    })?;
    execute_sequence(&mut client, &window, input)
}

pub(super) fn execute_sequence(
    client: &mut AgentClient,
    window: &WindowRecord,
    input: &UixPointerMoveSequenceInput,
) -> Result<PointerMoveSequenceOutcome, PointerMoveSequenceFailure> {
    // perform 与 pointer_move 必须在首个移动前一次性预检。
    if !client.request_types.contains("perform") || !client.window_actions.contains("pointer_move")
    {
        return Err(PointerMoveSequenceFailure::before_dispatch(
            WindowActionFailure::UnsupportedAction,
        ));
    }
    let required =
        Duration::from_millis(u64::from(input.planned_duration_ms())) + EXECUTION_RESERVE;
    if client.deadline.saturating_duration_since(Instant::now()) < required {
        return Err(PointerMoveSequenceFailure::before_dispatch(
            WindowActionFailure::Transport(Failure::Timeout),
        ));
    }

    let interval = Duration::from_millis(u64::from(input.interval_ms()));
    let mut target_time = Instant::now();
    let mut latest_revision = window.revision;
    let mut latest_presented_revision = window.presented_revision;
    let mut latest_settled = true;
    let mut moves_accepted = 0_u16;
    for (index, point) in input.moves().iter().enumerate() {
        if index > 0 {
            target_time += interval;
            if target_time + EXECUTION_RESERVE > client.deadline {
                return Err(PointerMoveSequenceFailure {
                    source: WindowActionFailure::Transport(Failure::Timeout),
                    accepted_may_have_occurred: moves_accepted > 0,
                    moves_accepted,
                });
            }
            if let Some(wait) = target_time.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
        match client.perform_targetless_with_request_id(
            "act-pointer-move-sequence",
            window.window_id,
            window.generation,
            latest_revision,
            point.provider_move_value(),
        ) {
            Ok(outcome) => {
                latest_revision = outcome.revision;
                latest_presented_revision = outcome.presented_revision;
                latest_settled = outcome.settled;
                moves_accepted += 1;
            }
            Err(source) => {
                return Err(PointerMoveSequenceFailure {
                    source,
                    accepted_may_have_occurred: moves_accepted > 0
                        || dispatch_may_have_occurred(source),
                    moves_accepted,
                });
            }
        }
    }
    Ok(PointerMoveSequenceOutcome {
        revision: latest_revision,
        presented_revision: latest_presented_revision,
        settled: latest_settled,
        moves_accepted,
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

    fn input() -> UixPointerMoveSequenceInput {
        let Ok(input) = UixPointerMoveSequenceInput::parse(&json!({
            "coordinateSpace": "client-logical-px",
            "moves": [
                { "x": 10.0, "y": 20.0 },
                { "x": 30.0, "y": 40.0 },
                { "x": 50.0, "y": 60.0 }
            ],
            "timeoutMs": 1000
        })) else {
            panic!("测试移动序列必须有效");
        };
        input
    }

    fn fixture_client(stream: UnixStream) -> AgentClient {
        AgentClient {
            reader: BufReader::new(stream),
            deadline: Instant::now() + Duration::from_secs(2),
            request_types: BTreeSet::from(["perform".to_owned()]),
            semantic_actions: BTreeSet::new(),
            window_actions: BTreeSet::from(["pointer_move".to_owned()]),
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
            for index in 0..3 {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                if request["request_id"] != "act-pointer-move-sequence"
                    || request["action"]["kind"] != "pointer_move"
                    || request["expected_revision"] != 5 + index as u64
                {
                    return Err("移动动作或修订链不匹配".to_owned());
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
            panic!("测试移动序列必须成功");
        };
        assert_eq!(outcome.moves_accepted, 3);
        assert_eq!(outcome.revision, 8);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }

    #[test]
    fn missing_pointer_move_fails_before_first_dispatch() {
        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.window_actions.remove("pointer_move");
        let Err(failure) = execute_sequence(&mut client, &fixture_window(), &input()) else {
            panic!("缺失 pointer_move 必须在首个 dispatch 前失败");
        };
        assert_eq!(failure.source, WindowActionFailure::UnsupportedAction);
        assert!(!failure.accepted_may_have_occurred);
        assert_eq!(failure.moves_accepted, 0);
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
            panic!("首个响应丢失不得报告移动序列成功");
        };
        assert_eq!(failure.source, WindowActionFailure::OutcomeUnknown);
        assert!(failure.accepted_may_have_occurred);
        assert_eq!(failure.moves_accepted, 0);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器必须读到首个请求");
    }
}
