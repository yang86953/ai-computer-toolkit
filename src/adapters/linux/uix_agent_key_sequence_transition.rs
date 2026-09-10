//! UIX Agent 完整按键序列及提交后语义条件同步的同连接 Adapter。

use std::time::{Duration, Instant};

use crate::components::{
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    uix_key_sequence_transition_contract::UixKeySequenceTransitionInput,
};

use super::{
    AgentClient, ElementWaitFailure, ElementWaitOutcome, Failure, KeySequenceFailure,
    KeySequenceOutcome, WindowRecord, resolve_until,
};

const EXECUTION_RESERVE: Duration = Duration::from_millis(100);

/// 区分 mutation 前失败、按键序列失败与序列后的条件观测失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KeySequenceTransitionFailureSource {
    Transport(Failure),
    NegotiationUnavailable,
    Sequence(KeySequenceFailure),
    Postcondition(ElementWaitFailure),
}

/// transition 失败时允许 Module 公开的最小中立事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KeySequenceTransitionFailure {
    pub(crate) source: KeySequenceTransitionFailureSource,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) sequence_completed: bool,
    pub(crate) sequence_revision: Option<u64>,
    pub(crate) sequence_presented_revision: Option<u64>,
    pub(crate) sequence_settled: Option<bool>,
    pub(crate) presses_accepted: u16,
}

impl KeySequenceTransitionFailure {
    fn before_dispatch(source: KeySequenceTransitionFailureSource) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            presses_accepted: 0,
        }
    }

    fn sequence(failure: KeySequenceFailure) -> Self {
        Self {
            source: KeySequenceTransitionFailureSource::Sequence(failure),
            accepted_may_have_occurred: failure.accepted_may_have_occurred
                || failure.presses_accepted > 0,
            sequence_completed: false,
            sequence_revision: None,
            sequence_presented_revision: None,
            sequence_settled: None,
            presses_accepted: failure.presses_accepted,
        }
    }

    fn after_sequence(source: ElementWaitFailure, sequence: KeySequenceOutcome) -> Self {
        Self {
            source: KeySequenceTransitionFailureSource::Postcondition(source),
            accepted_may_have_occurred: true,
            sequence_completed: true,
            sequence_revision: Some(sequence.revision),
            sequence_presented_revision: Some(sequence.presented_revision),
            sequence_settled: Some(sequence.settled),
            presses_accepted: sequence.presses_accepted,
        }
    }
}

/// 完整按键序列与提交后语义条件均可信时的中立事实。
#[derive(Debug)]
pub(crate) struct KeySequenceTransitionOutcome {
    pub(crate) sequence: KeySequenceOutcome,
    pub(crate) postcondition: ElementWaitOutcome,
}

/// 在一个总 deadline 与认证连接内执行完整按键序列，随后等待语义后置条件。
pub(crate) fn perform_key_sequence_transition(
    target: &str,
    input: &UixKeySequenceTransitionInput,
) -> Result<KeySequenceTransitionOutcome, KeySequenceTransitionFailure> {
    if OpaqueTargetId::parse(target).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        return Err(KeySequenceTransitionFailure::before_dispatch(
            KeySequenceTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|source| {
        KeySequenceTransitionFailure::before_dispatch(
            KeySequenceTransitionFailureSource::Transport(source),
        )
    })?;
    if window.session_id != target {
        return Err(KeySequenceTransitionFailure::before_dispatch(
            KeySequenceTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|source| {
        KeySequenceTransitionFailure::before_dispatch(
            KeySequenceTransitionFailureSource::Transport(source),
        )
    })?;
    execute_transition(&mut client, &window, target, input)
}

fn execute_transition(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixKeySequenceTransitionInput,
) -> Result<KeySequenceTransitionOutcome, KeySequenceTransitionFailure> {
    // 首个 press 前一次性预检语义同步、按键动作和全部键目录。
    if !["snapshot", "perform", "wait"]
        .iter()
        .all(|request| client.request_types.contains(*request))
        || !client.window_actions.contains("press_key")
        || input.presses().iter().any(|press| {
            !client.key_names.contains(press.provider_key())
                || press
                    .provider_modifiers()
                    .iter()
                    .any(|modifier| !client.key_modifiers.contains(*modifier))
        })
    {
        return Err(KeySequenceTransitionFailure::before_dispatch(
            KeySequenceTransitionFailureSource::NegotiationUnavailable,
        ));
    }
    let required =
        Duration::from_millis(u64::from(input.planned_duration_ms())) + EXECUTION_RESERVE;
    if client.deadline.saturating_duration_since(Instant::now()) < required {
        return Err(KeySequenceTransitionFailure::before_dispatch(
            KeySequenceTransitionFailureSource::Transport(Failure::Timeout),
        ));
    }

    let sequence =
        super::uix_agent_key_sequence::execute_sequence(client, window, input.sequence())
            .map_err(KeySequenceTransitionFailure::sequence)?;
    // 只有所有 press 完成并返回最终修订后，才允许进入语义条件等待。
    let postcondition = super::uix_agent_element_wait::execute_wait_from_baseline(
        client,
        window,
        target,
        input.postcondition().selector(),
        input.postcondition().condition(),
        sequence.revision,
        sequence.presented_revision,
    )
    .map_err(|source| KeySequenceTransitionFailure::after_sequence(source, sequence))?;
    Ok(KeySequenceTransitionOutcome {
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

    fn input() -> UixKeySequenceTransitionInput {
        let Ok(input) = UixKeySequenceTransitionInput::parse(&json!({
            "presses": [
                { "key": "a", "modifiers": ["control"] },
                { "key": "tab" },
                { "key": "enter" }
            ],
            "intervalMs": 0,
            "timeoutMs": 1000,
            "postcondition": {
                "selector": { "automationId": "sequence-result" },
                "condition": "unique"
            }
        })) else {
            panic!("测试 key sequence transition 输入必须有效");
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
            window_actions: BTreeSet::from(["press_key".to_owned()]),
            window_state_fields: BTreeSet::new(),
            key_names: BTreeSet::from(["a".to_owned(), "tab".to_owned(), "enter".to_owned()]),
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
    fn missing_wait_or_key_catalog_fails_before_first_dispatch() {
        for missing in ["wait", "a"] {
            let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
                panic!("测试 Unix 连接必须创建成功");
            };
            let mut client = fixture_client(client_stream);
            match missing {
                "wait" => {
                    client.request_types.remove("wait");
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
                panic!("缺失 {missing} 必须在首个 press 前失败");
            };
            assert_eq!(
                failure.source,
                KeySequenceTransitionFailureSource::NegotiationUnavailable
            );
            assert!(!failure.accepted_may_have_occurred);
            assert_eq!(failure.presses_accepted, 0);
        }
    }

    #[test]
    fn complete_sequence_and_snapshot_share_one_connection_with_unsettled_press() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            for (index, expected_key) in ["a", "tab", "enter"].into_iter().enumerate() {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                if request["type"] != "perform"
                    || request["request_id"] != "act-key-sequence-press"
                    || request["action"]["kind"] != "press_key"
                    || request["action"]["key"] != expected_key
                    || request["expected_revision"] != 5 + index as u64
                {
                    return Err("按键序列或修订链不匹配".to_owned());
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
            panic!("完整按键序列与语义条件必须成功");
        };
        assert_eq!(outcome.sequence.presses_accepted, 3);
        assert_eq!(outcome.sequence.revision, 8);
        assert!(!outcome.sequence.settled);
        assert_eq!(outcome.postcondition.match_count, 1);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }
}
