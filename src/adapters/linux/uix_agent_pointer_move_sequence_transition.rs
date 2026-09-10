//! UIX Agent 完整 pointer_move 序列及提交后语义条件同步的同连接 Adapter。

use std::time::{Duration, Instant};

use crate::components::{
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    uix_pointer_move_sequence_transition_contract::UixPointerMoveSequenceTransitionInput,
};

use super::{
    AgentClient, ElementWaitFailure, ElementWaitOutcome, Failure, PointerMoveSequenceFailure,
    PointerMoveSequenceOutcome, WindowRecord, resolve_until,
};

const EXECUTION_RESERVE: Duration = Duration::from_millis(100);

/// 区分 dispatch 前失败、完整移动序列失败与动作后条件观测失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PointerMoveSequenceTransitionFailureSource {
    Transport(Failure),
    NegotiationUnavailable,
    Sequence(PointerMoveSequenceFailure),
    Postcondition(ElementWaitFailure),
}

/// transition 失败时允许 Module 公开的最小中立事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PointerMoveSequenceTransitionFailure {
    pub(crate) source: PointerMoveSequenceTransitionFailureSource,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) sequence_completed: bool,
    pub(crate) sequence_revision: Option<u64>,
    pub(crate) sequence_presented_revision: Option<u64>,
    pub(crate) sequence_settled: Option<bool>,
    pub(crate) moves_accepted: u16,
}

impl PointerMoveSequenceTransitionFailure {
    fn before_dispatch(source: PointerMoveSequenceTransitionFailureSource) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            moves_accepted: 0,
        }
    }

    fn sequence(failure: PointerMoveSequenceFailure) -> Self {
        Self {
            source: PointerMoveSequenceTransitionFailureSource::Sequence(failure),
            accepted_may_have_occurred: failure.accepted_may_have_occurred
                || failure.moves_accepted > 0,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            moves_accepted: failure.moves_accepted,
        }
    }

    fn after_sequence(source: ElementWaitFailure, sequence: PointerMoveSequenceOutcome) -> Self {
        Self {
            source: PointerMoveSequenceTransitionFailureSource::Postcondition(source),
            accepted_may_have_occurred: true,
            sequence_completed: true,
            sequence_revision: Some(sequence.revision),
            sequence_presented_revision: Some(sequence.presented_revision),
            sequence_settled: Some(sequence.settled),
            moves_accepted: sequence.moves_accepted,
        }
    }
}

/// 完整移动序列与提交后语义条件均可信时的中立事实。
#[derive(Debug)]
pub(crate) struct PointerMoveSequenceTransitionOutcome {
    pub(crate) sequence: PointerMoveSequenceOutcome,
    pub(crate) postcondition: ElementWaitOutcome,
}

/// 在一个总 deadline 与认证连接内执行完整移动序列，随后等待语义后置条件。
pub(crate) fn perform_pointer_move_sequence_transition(
    target: &str,
    input: &UixPointerMoveSequenceTransitionInput,
) -> Result<PointerMoveSequenceTransitionOutcome, PointerMoveSequenceTransitionFailure> {
    if OpaqueTargetId::parse(target).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        return Err(PointerMoveSequenceTransitionFailure::before_dispatch(
            PointerMoveSequenceTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|source| {
        PointerMoveSequenceTransitionFailure::before_dispatch(
            PointerMoveSequenceTransitionFailureSource::Transport(source),
        )
    })?;
    if window.session_id != target {
        return Err(PointerMoveSequenceTransitionFailure::before_dispatch(
            PointerMoveSequenceTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|source| {
        PointerMoveSequenceTransitionFailure::before_dispatch(
            PointerMoveSequenceTransitionFailureSource::Transport(source),
        )
    })?;
    execute_transition(&mut client, &window, target, input)
}

fn execute_transition(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixPointerMoveSequenceTransitionInput,
) -> Result<PointerMoveSequenceTransitionOutcome, PointerMoveSequenceTransitionFailure> {
    // 首个 move 前一次性预检语义同步、perform 与 pointer_move 协议面。
    if !["snapshot", "perform", "wait"]
        .iter()
        .all(|request| client.request_types.contains(*request))
        || !client.window_actions.contains("pointer_move")
    {
        return Err(PointerMoveSequenceTransitionFailure::before_dispatch(
            PointerMoveSequenceTransitionFailureSource::NegotiationUnavailable,
        ));
    }
    let required =
        Duration::from_millis(u64::from(input.planned_duration_ms())) + EXECUTION_RESERVE;
    if client.deadline.saturating_duration_since(Instant::now()) < required {
        return Err(PointerMoveSequenceTransitionFailure::before_dispatch(
            PointerMoveSequenceTransitionFailureSource::Transport(Failure::Timeout),
        ));
    }

    let sequence =
        super::uix_agent_pointer_move_sequence::execute_sequence(client, window, input.sequence())
            .map_err(PointerMoveSequenceTransitionFailure::sequence)?;
    // 只有所有 move 完成并返回最终修订后，才允许进入语义条件等待。
    let postcondition = super::uix_agent_element_wait::execute_wait_from_baseline(
        client,
        window,
        target,
        input.postcondition().selector(),
        input.postcondition().condition(),
        sequence.revision,
        sequence.presented_revision,
    )
    .map_err(|source| PointerMoveSequenceTransitionFailure::after_sequence(source, sequence))?;
    Ok(PointerMoveSequenceTransitionOutcome {
        sequence,
        postcondition,
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

    fn input() -> UixPointerMoveSequenceTransitionInput {
        let Ok(input) = UixPointerMoveSequenceTransitionInput::parse(&json!({
            "coordinateSpace": "client-logical-px",
            "moves": [
                { "x": 10.0, "y": 20.0 },
                { "x": 30.0, "y": 40.0 },
                { "x": 50.0, "y": 60.0 }
            ],
            "intervalMs": 0,
            "timeoutMs": 1000,
            "postcondition": {
                "selector": { "automationId": "sequence-result" },
                "condition": "unique"
            }
        })) else {
            panic!("测试 pointer move sequence transition 输入必须有效");
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
                "wait".to_owned(),
            ]),
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
    fn missing_wait_or_pointer_move_fails_before_first_dispatch() {
        for missing in ["wait", "pointer_move"] {
            let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
                panic!("测试 Unix 连接必须创建成功");
            };
            let mut client = fixture_client(client_stream);
            if missing == "wait" {
                client.request_types.remove("wait");
            } else {
                client.window_actions.remove("pointer_move");
            }
            let Err(failure) = execute_transition(
                &mut client,
                &fixture_window(),
                "s2:w:0123456789abcdef",
                &input(),
            ) else {
                panic!("缺失 {missing} 必须在首个 move 前失败");
            };
            assert_eq!(
                failure.source,
                PointerMoveSequenceTransitionFailureSource::NegotiationUnavailable
            );
            assert!(!failure.accepted_may_have_occurred);
            assert_eq!(failure.moves_accepted, 0);
        }
    }

    #[test]
    fn complete_sequence_and_postcondition_share_one_connection() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            for (index, expected) in [(10.0, 20.0), (30.0, 40.0), (50.0, 60.0)]
                .into_iter()
                .enumerate()
            {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                if request["type"] != "perform"
                    || request["request_id"] != "act-pointer-move-sequence"
                    || request["action"]["kind"] != "pointer_move"
                    || request["action"]["x"] != expected.0
                    || request["action"]["y"] != expected.1
                    || request["expected_revision"] != 5 + index as u64
                {
                    return Err("移动序列或修订链不匹配".to_owned());
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
                    "settled": false
                });
                writeln!(reader.get_mut(), "{reply}").map_err(|error| error.to_string())?;
            }

            let mut line = String::new();
            reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            let request =
                serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
            if request["type"] != "snapshot" {
                return Err("完整序列后必须进入同连接 snapshot".to_owned());
            }
            let reply = json!({
                "schema": super::super::PROTOCOL_SCHEMA,
                "type": "snapshot",
                "request_id": "act-snapshot",
                "ok": true,
                "snapshot": {
                    "window_id": 7,
                    "generation": 3,
                    "revision": 8,
                    "presented_revision": 8,
                    "closed": false,
                    "nodes": [{
                        "node_id": "7:1",
                        "automation_id": "sequence-result",
                        "parent": null,
                        "focused": false,
                        "role": "status",
                        "name": "序列结果",
                        "frame": { "x": 0.0, "y": 0.0, "w": 20.0, "h": 20.0 },
                        "visible_bounds": null,
                        "state": { "disabled": false },
                        "actions": []
                    }]
                }
            });
            writeln!(reader.get_mut(), "{reply}").map_err(|error| error.to_string())?;
            Ok(())
        });

        let mut client = fixture_client(client_stream);
        let Ok(outcome) = execute_transition(
            &mut client,
            &fixture_window(),
            "s2:w:0123456789abcdef",
            &input(),
        ) else {
            panic!("完整移动序列与语义条件必须成功");
        };
        assert_eq!(outcome.sequence.moves_accepted, 3);
        assert_eq!(outcome.sequence.revision, 8);
        assert!(!outcome.sequence.settled);
        assert_eq!(outcome.postcondition.match_count, 1);
        assert_eq!(outcome.postcondition.wait_count, 0);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }

    #[test]
    fn lost_first_move_reply_is_unknown_and_not_safe_to_retry() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || {
            let mut reader = BufReader::new(server_stream);
            let mut line = String::new();
            reader.read_line(&mut line)
        });
        let mut client = fixture_client(client_stream);
        let Err(failure) = execute_transition(
            &mut client,
            &fixture_window(),
            "s2:w:0123456789abcdef",
            &input(),
        ) else {
            panic!("首项响应丢失不得报告移动序列成功");
        };
        let PointerMoveSequenceTransitionFailureSource::Sequence(sequence) = failure.source else {
            panic!("首项响应丢失必须归类为移动序列失败");
        };
        assert_eq!(
            sequence.source,
            super::super::WindowActionFailure::OutcomeUnknown
        );
        assert!(failure.accepted_may_have_occurred);
        assert_eq!(failure.moves_accepted, 0);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器必须读到首个请求");
    }
}
