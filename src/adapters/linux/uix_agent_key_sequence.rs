//! UIX Agent 请求级成对按键序列的私有 Adapter。

use std::{
    thread,
    time::{Duration, Instant},
};

use crate::components::uix_key_sequence_contract::UixKeySequenceInput;

use super::{AgentClient, Failure, WindowActionFailure, WindowRecord, resolve_until};

const EXECUTION_RESERVE: Duration = Duration::from_millis(100);

/// 全部成对按键完成后可公开投影的最小事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KeySequenceOutcome {
    pub(crate) revision: u64,
    pub(crate) presented_revision: u64,
    pub(crate) settled: bool,
    pub(crate) presses_accepted: u16,
}

/// 按键序列失败与可能部分执行的保守事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct KeySequenceFailure {
    pub(crate) source: WindowActionFailure,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) presses_accepted: u16,
}

impl KeySequenceFailure {
    fn before_dispatch(source: WindowActionFailure) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            presses_accepted: 0,
        }
    }
}

/// 重新解析窗口后，在同一认证连接内执行有界完整 press 序列。
pub(crate) fn perform_key_sequence(
    target: &str,
    input: &UixKeySequenceInput,
) -> Result<KeySequenceOutcome, KeySequenceFailure> {
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|failure| {
        KeySequenceFailure::before_dispatch(WindowActionFailure::Transport(failure))
    })?;
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|failure| {
        KeySequenceFailure::before_dispatch(WindowActionFailure::Transport(failure))
    })?;
    execute_sequence(&mut client, &window, input)
}

pub(super) fn execute_sequence(
    client: &mut AgentClient,
    window: &WindowRecord,
    input: &UixKeySequenceInput,
) -> Result<KeySequenceOutcome, KeySequenceFailure> {
    // perform、动作、键名和修饰键必须在首个 press 前全部预检。
    if !client.request_types.contains("perform")
        || !client.window_actions.contains("press_key")
        || input.presses().iter().any(|press| {
            !client.key_names.contains(press.provider_key())
                || press
                    .provider_modifiers()
                    .iter()
                    .any(|modifier| !client.key_modifiers.contains(*modifier))
        })
    {
        return Err(KeySequenceFailure::before_dispatch(
            WindowActionFailure::UnsupportedAction,
        ));
    }
    let required =
        Duration::from_millis(u64::from(input.planned_duration_ms())) + EXECUTION_RESERVE;
    if client.deadline.saturating_duration_since(Instant::now()) < required {
        return Err(KeySequenceFailure::before_dispatch(
            WindowActionFailure::Transport(Failure::Timeout),
        ));
    }

    let sequence_started = Instant::now();
    let mut latest_revision = window.revision;
    let mut latest_presented_revision = window.presented_revision;
    let mut latest_settled = true;
    let mut presses_accepted = 0_u16;
    for (index, press) in input.presses().iter().enumerate() {
        if index > 0 {
            let planned_elapsed = Duration::from_millis(
                u64::from(input.interval_ms()) * u64::try_from(index).unwrap_or(u64::MAX),
            );
            let target_time = sequence_started + planned_elapsed;
            if target_time + EXECUTION_RESERVE > client.deadline {
                return Err(KeySequenceFailure {
                    source: WindowActionFailure::Transport(Failure::Timeout),
                    accepted_may_have_occurred: presses_accepted > 0,
                    presses_accepted,
                });
            }
            if let Some(wait) = target_time.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
        match client.perform_targetless_with_request_id(
            "act-key-sequence-press",
            window.window_id,
            window.generation,
            latest_revision,
            press.provider_value(),
        ) {
            Ok(outcome) => {
                latest_revision = outcome.revision;
                latest_presented_revision = outcome.presented_revision;
                latest_settled = outcome.settled;
                presses_accepted += 1;
            }
            Err(source) => {
                return Err(KeySequenceFailure {
                    source,
                    accepted_may_have_occurred: presses_accepted > 0
                        || dispatch_may_have_occurred(source),
                    presses_accepted,
                });
            }
        }
    }
    Ok(KeySequenceOutcome {
        revision: latest_revision,
        presented_revision: latest_presented_revision,
        settled: latest_settled,
        presses_accepted,
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

    fn input() -> UixKeySequenceInput {
        let Ok(input) = UixKeySequenceInput::parse(&json!({
            "presses": [
                { "key": "a", "modifiers": ["control"] },
                { "key": "tab" },
                { "key": "enter" }
            ],
            "intervalMs": 0,
            "timeoutMs": 1000
        })) else {
            panic!("测试按键序列必须有效");
        };
        input
    }

    fn fixture_client(stream: UnixStream) -> AgentClient {
        AgentClient {
            reader: BufReader::new(stream),
            deadline: Instant::now() + Duration::from_secs(2),
            request_types: BTreeSet::from(["perform".to_owned()]),
            semantic_actions: BTreeSet::new(),
            window_actions: BTreeSet::from(["press_key".to_owned()]),
            window_state_fields: BTreeSet::new(),
            key_names: BTreeSet::from(["a".to_owned(), "enter".to_owned(), "tab".to_owned()]),
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
    fn sequence_uses_one_connection_and_revision_chain() {
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
                if request["request_id"] != "act-key-sequence-press"
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
                    "settled": true
                });
                writeln!(reader.get_mut(), "{reply}").map_err(|error| error.to_string())?;
            }
            Ok(())
        });
        let mut client = fixture_client(client_stream);
        let Ok(outcome) = execute_sequence(&mut client, &fixture_window(), &input()) else {
            panic!("测试按键序列必须成功");
        };
        assert_eq!(outcome.presses_accepted, 3);
        assert_eq!(outcome.revision, 8);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }

    #[test]
    fn unsupported_later_key_fails_before_first_dispatch() {
        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.key_names.remove("enter");
        let Err(failure) = execute_sequence(&mut client, &fixture_window(), &input()) else {
            panic!("不完整键名目录必须在首个 dispatch 前失败");
        };
        assert_eq!(failure.source, WindowActionFailure::UnsupportedAction);
        assert!(!failure.accepted_may_have_occurred);
        assert_eq!(failure.presses_accepted, 0);
    }

    #[test]
    fn lost_first_press_reply_is_unknown_and_not_safe_to_retry() {
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
            panic!("首项响应丢失不得报告序列成功");
        };
        assert_eq!(failure.source, WindowActionFailure::OutcomeUnknown);
        assert!(failure.accepted_may_have_occurred);
        assert_eq!(failure.presses_accepted, 0);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器必须读到首个请求");
    }
}
