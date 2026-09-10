//! UIX Agent 键盘与指针混合序列及提交后语义条件同步的同连接 Adapter。

use std::time::{Duration, Instant};

use crate::components::{
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    uix_input_sequence_contract::UixInputSequenceStepKind,
    uix_input_sequence_transition_contract::UixInputSequenceTransitionInput,
};

use super::{
    AgentClient, ElementWaitFailure, ElementWaitOutcome, Failure, InputSequenceFailure,
    InputSequenceOutcome, WindowRecord, resolve_until,
};

const EXECUTION_RESERVE: Duration = Duration::from_millis(100);

/// 区分 mutation 前失败、混合输入序列失败与序列后的条件观测失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InputSequenceTransitionFailureSource {
    Transport(Failure),
    NegotiationUnavailable,
    Sequence(InputSequenceFailure),
    Postcondition(ElementWaitFailure),
}

/// transition 失败时允许 Module 公开的最小中立事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InputSequenceTransitionFailure {
    pub(crate) source: InputSequenceTransitionFailureSource,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) sequence_completed: bool,
    pub(crate) sequence_revision: Option<u64>,
    pub(crate) sequence_presented_revision: Option<u64>,
    pub(crate) sequence_settled: Option<bool>,
    pub(crate) steps_accepted: u16,
    pub(crate) presses_accepted: u16,
    pub(crate) moves_accepted: u16,
    pub(crate) clicks_accepted: u16,
}

impl InputSequenceTransitionFailure {
    fn before_dispatch(source: InputSequenceTransitionFailureSource) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            steps_accepted: 0,
            presses_accepted: 0,
            moves_accepted: 0,
            clicks_accepted: 0,
        }
    }

    fn sequence(failure: InputSequenceFailure) -> Self {
        Self {
            source: InputSequenceTransitionFailureSource::Sequence(failure),
            accepted_may_have_occurred: failure.accepted_may_have_occurred
                || failure.steps_accepted > 0,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            steps_accepted: failure.steps_accepted,
            presses_accepted: failure.presses_accepted,
            moves_accepted: failure.moves_accepted,
            clicks_accepted: failure.clicks_accepted,
        }
    }

    fn after_sequence(source: ElementWaitFailure, sequence: InputSequenceOutcome) -> Self {
        Self {
            source: InputSequenceTransitionFailureSource::Postcondition(source),
            accepted_may_have_occurred: true,
            sequence_completed: true,
            sequence_revision: Some(sequence.revision),
            sequence_presented_revision: Some(sequence.presented_revision),
            sequence_settled: Some(sequence.settled),
            steps_accepted: sequence.steps_accepted,
            presses_accepted: sequence.presses_accepted,
            moves_accepted: sequence.moves_accepted,
            clicks_accepted: sequence.clicks_accepted,
        }
    }
}

/// 完整混合输入序列与提交后语义条件均可信时的中立事实。
#[derive(Debug)]
pub(crate) struct InputSequenceTransitionOutcome {
    pub(crate) sequence: InputSequenceOutcome,
    pub(crate) postcondition: ElementWaitOutcome,
}

/// 在一个总 deadline 与认证连接内执行完整混合序列，随后等待语义后置条件。
pub(crate) fn perform_input_sequence_transition(
    target: &str,
    input: &UixInputSequenceTransitionInput,
) -> Result<InputSequenceTransitionOutcome, InputSequenceTransitionFailure> {
    if OpaqueTargetId::parse(target).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        return Err(InputSequenceTransitionFailure::before_dispatch(
            InputSequenceTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|source| {
        InputSequenceTransitionFailure::before_dispatch(
            InputSequenceTransitionFailureSource::Transport(source),
        )
    })?;
    if window.session_id != target {
        return Err(InputSequenceTransitionFailure::before_dispatch(
            InputSequenceTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|source| {
        InputSequenceTransitionFailure::before_dispatch(
            InputSequenceTransitionFailureSource::Transport(source),
        )
    })?;
    execute_transition(&mut client, &window, target, input)
}

fn execute_transition(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixInputSequenceTransitionInput,
) -> Result<InputSequenceTransitionOutcome, InputSequenceTransitionFailure> {
    // 首个步骤前一次性预检语义同步与完整混合序列所需的协议面。
    if !["snapshot", "perform", "wait"]
        .iter()
        .all(|request| client.request_types.contains(*request))
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
        return Err(InputSequenceTransitionFailure::before_dispatch(
            InputSequenceTransitionFailureSource::NegotiationUnavailable,
        ));
    }
    let required =
        Duration::from_millis(u64::from(input.planned_duration_ms())) + EXECUTION_RESERVE;
    if client.deadline.saturating_duration_since(Instant::now()) < required {
        return Err(InputSequenceTransitionFailure::before_dispatch(
            InputSequenceTransitionFailureSource::Transport(Failure::Timeout),
        ));
    }

    let sequence =
        super::uix_agent_input_sequence::execute_sequence(client, window, input.sequence())
            .map_err(InputSequenceTransitionFailure::sequence)?;
    // 只有完整序列完成并返回最终修订后，才允许进入语义条件等待。
    let postcondition = super::uix_agent_element_wait::execute_wait_from_baseline(
        client,
        window,
        target,
        input.postcondition().selector(),
        input.postcondition().condition(),
        sequence.revision,
        sequence.presented_revision,
    )
    .map_err(|source| InputSequenceTransitionFailure::after_sequence(source, sequence))?;
    Ok(InputSequenceTransitionOutcome {
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

    fn input() -> UixInputSequenceTransitionInput {
        let Ok(input) = UixInputSequenceTransitionInput::parse(&json!({
            "coordinateSpace": "client-logical-px",
            "steps": [
                { "type": "press", "key": "a", "modifiers": ["control"] },
                { "type": "move", "x": 10.0, "y": 20.0 },
                { "type": "click", "x": 30.0, "y": 40.0 }
            ],
            "intervalMs": 0,
            "timeoutMs": 1000,
            "postcondition": {
                "selector": { "automationId": "sequence-result" },
                "condition": "unique"
            }
        })) else {
            panic!("测试 input sequence transition 输入必须有效");
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

    #[test]
    fn missing_wait_action_or_key_catalog_fails_before_first_dispatch() {
        for missing in ["wait", "click_at", "a"] {
            let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
                panic!("测试 Unix 连接必须创建成功");
            };
            let mut client = fixture_client(client_stream);
            match missing {
                "wait" => {
                    client.request_types.remove("wait");
                }
                "click_at" => {
                    client.window_actions.remove("click_at");
                }
                "a" => {
                    client.key_names.remove("a");
                }
                _ => unreachable!(),
            }
            let Err(failure) = execute_transition(
                &mut client,
                &fixture_window(),
                "s2:w:0123456789abcdef",
                &input(),
            ) else {
                panic!("缺失 {missing} 必须在首个步骤前失败");
            };
            assert_eq!(
                failure.source,
                InputSequenceTransitionFailureSource::NegotiationUnavailable
            );
            assert!(!failure.accepted_may_have_occurred);
            assert_eq!(failure.steps_accepted, 0);
        }
    }

    #[test]
    fn complete_three_step_sequence_and_snapshot_share_one_connection() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            for (index, kind) in ["press_key", "pointer_move", "click_at"]
                .into_iter()
                .enumerate()
            {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                if request["type"] != "perform" || request["action"]["kind"] != kind {
                    return Err("混合输入 transition 步骤顺序不匹配".to_owned());
                }
                if request["expected_revision"] != 5 + index as u64 {
                    return Err("混合输入 transition 修订链接不匹配".to_owned());
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
                    "settled": false,
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
            panic!("完整三步序列与语义条件必须成功");
        };
        assert_eq!(outcome.sequence.steps_accepted, 3);
        assert_eq!(outcome.sequence.presses_accepted, 1);
        assert_eq!(outcome.sequence.moves_accepted, 1);
        assert_eq!(outcome.sequence.clicks_accepted, 1);
        assert_eq!(outcome.sequence.revision, 8);
        assert!(!outcome.sequence.settled);
        assert_eq!(outcome.postcondition.match_count, 1);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }
}
