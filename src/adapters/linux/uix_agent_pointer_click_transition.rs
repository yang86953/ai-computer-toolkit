//! UIX Agent 普通左键 click 与提交后语义条件同步的同连接 Adapter。

use std::time::{Duration, Instant};

use crate::components::{
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    uix_pointer_click_transition_contract::UixPointerClickTransitionInput,
};

use super::{
    ActionOutcome, AgentClient, ElementWaitFailure, ElementWaitOutcome, Failure,
    WindowActionFailure, WindowRecord, resolve_until,
};

const TRANSITION_REQUEST_ID: &str = "act-pointer-click-transition";

/// 区分 mutation 前失败、普通左键 click 失败与动作后条件观测失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PointerClickTransitionFailureSource {
    Transport(Failure),
    NegotiationUnavailable,
    Action(WindowActionFailure),
    Postcondition(ElementWaitFailure),
}

/// transition 失败时允许 Module 公开的最小中立事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PointerClickTransitionFailure {
    pub(crate) source: PointerClickTransitionFailureSource,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) action_accepted: bool,
    pub(crate) action_revision: Option<u64>,
    pub(crate) action_presented_revision: Option<u64>,
    pub(crate) action_settled: Option<bool>,
    pub(crate) sample_count: Option<u32>,
    pub(crate) wait_count: Option<u32>,
}

impl PointerClickTransitionFailure {
    fn before_dispatch(source: PointerClickTransitionFailureSource) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            action_accepted: false,
            action_revision: None,
            action_presented_revision: None,
            action_settled: None,
            sample_count: None,
            wait_count: None,
        }
    }

    fn action(source: WindowActionFailure) -> Self {
        Self {
            source: PointerClickTransitionFailureSource::Action(source),
            accepted_may_have_occurred: action_may_have_occurred(source),
            action_accepted: false,
            action_revision: None,
            action_presented_revision: None,
            action_settled: None,
            sample_count: None,
            wait_count: None,
        }
    }

    fn after_dispatch(source: ElementWaitFailure, action: ActionOutcome) -> Self {
        Self {
            source: PointerClickTransitionFailureSource::Postcondition(source),
            accepted_may_have_occurred: true,
            action_accepted: true,
            action_revision: Some(action.revision),
            action_presented_revision: Some(action.presented_revision),
            action_settled: Some(action.settled),
            sample_count: None,
            wait_count: None,
        }
    }
}

/// click 响应与提交后语义条件观察均可信时的中立事实。
#[derive(Debug)]
pub(crate) struct PointerClickTransitionOutcome {
    pub(crate) action: ActionOutcome,
    pub(crate) postcondition: ElementWaitOutcome,
}

/// 在一个总 deadline 内只解析一次窗口并完成一次普通左键 click 与条件同步。
pub(crate) fn perform_pointer_click_transition(
    target: &str,
    input: &UixPointerClickTransitionInput,
) -> Result<PointerClickTransitionOutcome, PointerClickTransitionFailure> {
    if OpaqueTargetId::parse(target).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        return Err(PointerClickTransitionFailure::before_dispatch(
            PointerClickTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|source| {
        PointerClickTransitionFailure::before_dispatch(
            PointerClickTransitionFailureSource::Transport(source),
        )
    })?;
    if window.session_id != target {
        return Err(PointerClickTransitionFailure::before_dispatch(
            PointerClickTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|source| {
        PointerClickTransitionFailure::before_dispatch(
            PointerClickTransitionFailureSource::Transport(source),
        )
    })?;
    execute_transition(&mut client, &window, target, input)
}

fn execute_transition(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixPointerClickTransitionInput,
) -> Result<PointerClickTransitionOutcome, PointerClickTransitionFailure> {
    // mutation 前一次性预检语义条件同步与普通左键 click 所需的完整协议面。
    if !["snapshot", "perform", "wait"]
        .iter()
        .all(|request| client.request_types.contains(*request))
        || !client.window_actions.contains("click_at")
    {
        return Err(PointerClickTransitionFailure::before_dispatch(
            PointerClickTransitionFailureSource::NegotiationUnavailable,
        ));
    }
    if super::remaining(client.deadline).is_err() {
        return Err(PointerClickTransitionFailure::before_dispatch(
            PointerClickTransitionFailureSource::Transport(Failure::Timeout),
        ));
    }

    let action = client
        .perform_targetless_with_request_id(
            TRANSITION_REQUEST_ID,
            window.window_id,
            window.generation,
            window.revision,
            input.provider_value(),
        )
        .map_err(PointerClickTransitionFailure::action)?;
    let postcondition = super::uix_agent_element_wait::execute_wait_from_baseline(
        client,
        window,
        target,
        input.postcondition().selector(),
        input.postcondition().condition(),
        action.revision,
        action.presented_revision,
    )
    .map_err(|source| PointerClickTransitionFailure::after_dispatch(source, action))?;
    Ok(PointerClickTransitionOutcome {
        action,
        postcondition,
    })
}

fn action_may_have_occurred(source: WindowActionFailure) -> bool {
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
        time::{Duration, Instant},
    };

    use serde_json::{Value, json};

    use super::*;

    fn input() -> UixPointerClickTransitionInput {
        let Ok(input) = UixPointerClickTransitionInput::parse(&json!({
            "coordinateSpace": "client-logical-px",
            "x": 10.0,
            "y": 20.0,
            "postcondition": {
                "selector": { "automationId": "saved" },
                "condition": "unique"
            },
            "timeoutMs": 1000
        })) else {
            panic!("测试 pointer click transition 输入必须有效");
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
            window_actions: BTreeSet::from(["click_at".to_owned()]),
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
    fn missing_wait_or_click_action_fails_before_first_dispatch() {
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
            panic!("缺失 wait 必须在 click dispatch 前失败");
        };
        assert_eq!(
            failure.source,
            PointerClickTransitionFailureSource::NegotiationUnavailable
        );
        assert!(!failure.accepted_may_have_occurred);
    }

    #[test]
    fn click_and_postcondition_share_one_connection_and_keep_unsettled_fact() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || {
            let mut reader = BufReader::new(server_stream);
            let mut perform_line = String::new();
            assert!(reader.read_line(&mut perform_line).is_ok());
            let Ok(perform) = serde_json::from_str::<Value>(&perform_line) else {
                panic!("测试 perform 请求必须为 JSON");
            };
            assert_eq!(perform["type"], "perform");
            assert_eq!(perform["request_id"], TRANSITION_REQUEST_ID);
            assert_eq!(perform["action"]["kind"], "click_at");
            assert_eq!(perform["action"]["x"], 10.0);
            assert_eq!(perform["action"]["y"], 20.0);
            assert!(
                writeln!(
                    reader.get_mut(),
                    "{}",
                    json!({
                        "schema": super::super::PROTOCOL_SCHEMA,
                        "type": "perform",
                        "request_id": TRANSITION_REQUEST_ID,
                        "ok": true,
                        "window_id": 7,
                        "generation": 3,
                        "revision": 6,
                        "presented_revision": 6,
                        "settled": false,
                    })
                )
                .is_ok()
            );

            let mut snapshot_line = String::new();
            assert!(reader.read_line(&mut snapshot_line).is_ok());
            let Ok(snapshot) = serde_json::from_str::<Value>(&snapshot_line) else {
                panic!("测试 snapshot 请求必须为 JSON");
            };
            assert_eq!(snapshot["type"], "snapshot");
            assert!(
                writeln!(
                    reader.get_mut(),
                    "{}",
                    json!({
                        "schema": super::super::PROTOCOL_SCHEMA,
                        "type": "snapshot",
                        "request_id": "act-snapshot",
                        "ok": true,
                        "snapshot": {
                            "window_id": 7,
                            "generation": 3,
                            "revision": 6,
                            "presented_revision": 6,
                            "closed": false,
                            "nodes": [{
                                "node_id": "7:1",
                                "automation_id": "saved",
                                "parent": null,
                                "focused": false,
                                "role": "status",
                                "name": "saved",
                                "frame": { "x": 0.0, "y": 0.0, "w": 20.0, "h": 20.0 },
                                "visible_bounds": null,
                                "state": { "disabled": false },
                                "actions": []
                            }]
                        }
                    })
                )
                .is_ok()
            );
        });

        let mut client = fixture_client(client_stream);
        let Ok(outcome) = execute_transition(
            &mut client,
            &fixture_window(),
            "s2:w:0123456789abcdef",
            &input(),
        ) else {
            panic!("同连接 click 与语义条件必须成功");
        };
        assert!(!outcome.action.settled);
        assert_eq!(outcome.postcondition.match_count, 1);
        assert_eq!(outcome.postcondition.sample_count, 1);
        assert_eq!(outcome.postcondition.wait_count, 0);
        assert!(server.join().is_ok());
    }
}
