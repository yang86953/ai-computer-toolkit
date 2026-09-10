//! UIX Agent 悬停—点击序列与提交后语义条件同步的同连接 Adapter。

use std::time::{Duration, Instant};

use crate::components::{
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    uix_pointer_sequence_transition_contract::UixPointerSequenceTransitionInput,
};

use super::{
    AgentClient, ElementWaitFailure, ElementWaitOutcome, Failure, PointerSequenceFailure,
    PointerSequenceOutcome, WindowRecord, resolve_until,
};

/// 区分 mutation 前失败、混合指针序列失败与序列后的条件观测失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PointerSequenceTransitionFailureSource {
    Transport(Failure),
    NegotiationUnavailable,
    Sequence(PointerSequenceFailure),
    Postcondition(ElementWaitFailure),
}

/// transition 失败时允许 Module 公开的最小中立事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PointerSequenceTransitionFailure {
    pub(crate) source: PointerSequenceTransitionFailureSource,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) sequence_completed: bool,
    pub(crate) sequence_revision: Option<u64>,
    pub(crate) sequence_presented_revision: Option<u64>,
    pub(crate) sequence_settled: Option<bool>,
    pub(crate) steps_accepted: u16,
    pub(crate) moves_accepted: u16,
    pub(crate) clicks_accepted: u16,
}

impl PointerSequenceTransitionFailure {
    fn before_dispatch(source: PointerSequenceTransitionFailureSource) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            steps_accepted: 0,
            moves_accepted: 0,
            clicks_accepted: 0,
        }
    }

    fn sequence(failure: PointerSequenceFailure) -> Self {
        Self {
            source: PointerSequenceTransitionFailureSource::Sequence(failure),
            accepted_may_have_occurred: failure.accepted_may_have_occurred
                || failure.steps_accepted > 0,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            steps_accepted: failure.steps_accepted,
            moves_accepted: failure.moves_accepted,
            clicks_accepted: failure.clicks_accepted,
        }
    }

    fn after_sequence(source: ElementWaitFailure, sequence: PointerSequenceOutcome) -> Self {
        Self {
            source: PointerSequenceTransitionFailureSource::Postcondition(source),
            accepted_may_have_occurred: true,
            sequence_completed: true,
            sequence_revision: Some(sequence.revision),
            sequence_presented_revision: Some(sequence.presented_revision),
            sequence_settled: Some(sequence.settled),
            steps_accepted: sequence.steps_accepted,
            moves_accepted: sequence.moves_accepted,
            clicks_accepted: sequence.clicks_accepted,
        }
    }
}

/// 完整悬停—点击序列与提交后语义条件均可信时的中立事实。
#[derive(Debug)]
pub(crate) struct PointerSequenceTransitionOutcome {
    pub(crate) sequence: PointerSequenceOutcome,
    pub(crate) postcondition: ElementWaitOutcome,
}

/// 在一个总 deadline 与认证连接内执行完整序列，随后等待语义后置条件。
pub(crate) fn perform_pointer_sequence_transition(
    target: &str,
    input: &UixPointerSequenceTransitionInput,
) -> Result<PointerSequenceTransitionOutcome, PointerSequenceTransitionFailure> {
    if OpaqueTargetId::parse(target).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        return Err(PointerSequenceTransitionFailure::before_dispatch(
            PointerSequenceTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|source| {
        PointerSequenceTransitionFailure::before_dispatch(
            PointerSequenceTransitionFailureSource::Transport(source),
        )
    })?;
    if window.session_id != target {
        return Err(PointerSequenceTransitionFailure::before_dispatch(
            PointerSequenceTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|source| {
        PointerSequenceTransitionFailure::before_dispatch(
            PointerSequenceTransitionFailureSource::Transport(source),
        )
    })?;
    execute_transition(&mut client, &window, target, input)
}

fn execute_transition(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixPointerSequenceTransitionInput,
) -> Result<PointerSequenceTransitionOutcome, PointerSequenceTransitionFailure> {
    // 首个步骤前一次性预检序列与语义修订同步所需的完整协议面。
    if !["snapshot", "perform", "wait"]
        .iter()
        .all(|request| client.request_types.contains(*request))
        || !["pointer_move", "click_at"]
            .iter()
            .all(|action| client.window_actions.contains(*action))
    {
        return Err(PointerSequenceTransitionFailure::before_dispatch(
            PointerSequenceTransitionFailureSource::NegotiationUnavailable,
        ));
    }
    if super::remaining(client.deadline).is_err() {
        return Err(PointerSequenceTransitionFailure::before_dispatch(
            PointerSequenceTransitionFailureSource::Transport(Failure::Timeout),
        ));
    }

    let sequence =
        super::uix_agent_pointer_sequence::execute_sequence(client, window, input.sequence())
            .map_err(PointerSequenceTransitionFailure::sequence)?;
    // 只有全部步骤完成并返回最终修订后，才允许进入语义条件等待。
    let postcondition = super::uix_agent_element_wait::execute_wait_from_baseline(
        client,
        window,
        target,
        input.postcondition().selector(),
        input.postcondition().condition(),
        sequence.revision,
        sequence.presented_revision,
    )
    .map_err(|source| PointerSequenceTransitionFailure::after_sequence(source, sequence))?;
    Ok(PointerSequenceTransitionOutcome {
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

    fn input() -> UixPointerSequenceTransitionInput {
        let Ok(input) = UixPointerSequenceTransitionInput::parse(&json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "move", "x": 10.0, "y": 20.0 },
                { "type": "click", "x": 30.0, "y": 40.0 }
            ],
            "intervalMs": 0,
            "timeoutMs": 1000,
            "postcondition": {
                "selector": { "automationId": "menu-result" },
                "condition": "unique"
            }
        })) else {
            panic!("测试 pointer sequence transition 输入必须有效");
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
            window_actions: BTreeSet::from(["pointer_move".to_owned(), "click_at".to_owned()]),
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
    fn missing_wait_or_sequence_action_fails_before_first_dispatch() {
        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.request_types.remove("wait");
        let Err(failure) = execute_transition(
            &mut client,
            &fixture_window(),
            "s2:w:0123456789abcdef",
            &input(),
        ) else {
            panic!("缺失 wait 必须在首个 sequence 步骤前失败");
        };
        assert_eq!(
            failure.source,
            PointerSequenceTransitionFailureSource::NegotiationUnavailable
        );
        assert!(!failure.accepted_may_have_occurred);
        assert_eq!(failure.steps_accepted, 0);
    }

    #[test]
    fn complete_sequence_and_postcondition_share_one_connection() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            for (index, kind) in ["pointer_move", "click_at"].into_iter().enumerate() {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                if request["type"] != "perform" || request["action"]["kind"] != kind {
                    return Err("混合指针 transition 步骤顺序不匹配".to_owned());
                }
                if request["expected_revision"] != 5 + index as u64 {
                    return Err("混合指针 transition 修订链接不匹配".to_owned());
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
                    "settled": index == 0,
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
                    "revision": 7,
                    "presented_revision": 7,
                    "closed": false,
                    "nodes": [{
                        "node_id": "7:1",
                        "automation_id": "menu-result",
                        "parent": null,
                        "focused": false,
                        "role": "status",
                        "name": "菜单结果",
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
            panic!("同连接完整序列与语义条件必须成功");
        };
        assert_eq!(outcome.sequence.steps_accepted, 2);
        assert_eq!(outcome.sequence.revision, 7);
        assert!(!outcome.sequence.settled);
        assert_eq!(outcome.postcondition.match_count, 1);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }
}
