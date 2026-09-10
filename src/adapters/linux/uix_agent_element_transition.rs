//! UIX Agent 语义元素动作与提交后条件同步的同连接私有 Adapter。

use std::time::{Duration, Instant};

use crate::components::{
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    uix_element_transition_contract::UixElementTransitionInput,
    uix_semantic_identity,
};

use super::{
    ActionFailure, ActionOutcome, AgentClient, ElementWaitFailure, ElementWaitOutcome, Failure,
    NodeRecord, WindowRecord, resolve_until,
};

/// 区分 mutation 前失败、动作失败和提交后条件观测失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ElementTransitionFailureSource {
    Transport(Failure),
    NegotiationUnavailable,
    StaleElement,
    AmbiguousSource,
    ElementDisabled,
    NodeActionUnsupported,
    Action(ActionFailure),
    Postcondition(ElementWaitFailure),
}

/// 语义 transition 失败的最小中立事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ElementTransitionFailure {
    pub(crate) source: ElementTransitionFailureSource,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) action_accepted: bool,
    pub(crate) action_revision: Option<u64>,
    pub(crate) action_presented_revision: Option<u64>,
    pub(crate) action_settled: Option<bool>,
    pub(crate) application_confirmation_performed: Option<bool>,
    pub(crate) sample_count: Option<u32>,
    pub(crate) wait_count: Option<u32>,
}

impl ElementTransitionFailure {
    fn before_dispatch(source: ElementTransitionFailureSource) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            action_accepted: false,
            action_revision: None,
            action_presented_revision: None,
            action_settled: None,
            application_confirmation_performed: None,
            sample_count: None,
            wait_count: None,
        }
    }

    fn dispatch(source: ActionFailure) -> Self {
        Self {
            source: ElementTransitionFailureSource::Action(source),
            accepted_may_have_occurred: action_may_have_occurred(source),
            action_accepted: false,
            action_revision: None,
            action_presented_revision: None,
            action_settled: None,
            application_confirmation_performed: None,
            sample_count: None,
            wait_count: None,
        }
    }

    fn after_dispatch(source: ElementWaitFailure, action: ActionOutcome) -> Self {
        Self {
            source: ElementTransitionFailureSource::Postcondition(source),
            accepted_may_have_occurred: true,
            action_accepted: true,
            action_revision: Some(action.revision),
            action_presented_revision: Some(action.presented_revision),
            action_settled: Some(action.settled),
            application_confirmation_performed: Some(action.application_confirmation_performed),
            sample_count: None,
            wait_count: None,
        }
    }
}

/// 动作响应与 postcondition 观察均可信时公开的中立事实。
#[derive(Debug)]
pub(crate) struct ElementTransitionOutcome {
    pub(crate) action: ActionOutcome,
    pub(crate) postcondition: ElementWaitOutcome,
}

/// 在一个总 deadline 内只解析一次目标、认证一次并完成完整语义 transition。
pub(crate) fn perform_element_transition(
    target: &str,
    input: &UixElementTransitionInput,
) -> Result<ElementTransitionOutcome, ElementTransitionFailure> {
    if OpaqueTargetId::parse(target).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        return Err(ElementTransitionFailure::before_dispatch(
            ElementTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|source| {
        ElementTransitionFailure::before_dispatch(ElementTransitionFailureSource::Transport(source))
    })?;
    if window.session_id != target {
        return Err(ElementTransitionFailure::before_dispatch(
            ElementTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|source| {
        ElementTransitionFailure::before_dispatch(ElementTransitionFailureSource::Transport(source))
    })?;
    execute_transition(&mut client, &window, target, input)
}

fn execute_transition(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixElementTransitionInput,
) -> Result<ElementTransitionOutcome, ElementTransitionFailure> {
    // mutation 前一次性预检动作、应用确认和 revision wait 全部协议面。
    if !["snapshot", "perform", "confirm", "wait"]
        .iter()
        .all(|request| client.request_types.contains(*request))
        || !client
            .semantic_actions
            .contains(input.action().provider_action())
    {
        return Err(ElementTransitionFailure::before_dispatch(
            ElementTransitionFailureSource::NegotiationUnavailable,
        ));
    }

    let wire = client
        .snapshot(window.window_id, window.generation)
        .map_err(|source| {
            ElementTransitionFailure::before_dispatch(ElementTransitionFailureSource::Transport(
                source,
            ))
        })?;
    let snapshot = super::uix_agent_element_wait::snapshot_from_wire(
        wire,
        window,
        target,
        window.revision,
        window.presented_revision,
    )
    .map_err(|source| {
        ElementTransitionFailure::before_dispatch(ElementTransitionFailureSource::Transport(source))
    })?;
    let current_snapshot_id = uix_semantic_identity::snapshot_id(
        &snapshot.session_id,
        snapshot.revision,
        snapshot.presented_revision,
    );
    if current_snapshot_id != input.snapshot_id() {
        return Err(ElementTransitionFailure::before_dispatch(
            ElementTransitionFailureSource::StaleElement,
        ));
    }
    let node = exact_source_node(&snapshot.nodes, &current_snapshot_id, input.element_id())?;
    if !node.enabled {
        return Err(ElementTransitionFailure::before_dispatch(
            ElementTransitionFailureSource::ElementDisabled,
        ));
    }
    if !node
        .actions
        .iter()
        .any(|action| action == input.action().provider_action())
    {
        return Err(ElementTransitionFailure::before_dispatch(
            ElementTransitionFailureSource::NodeActionUnsupported,
        ));
    }

    let action = client
        .perform(
            window.window_id,
            window.generation,
            snapshot.revision,
            &node.native_id,
            input.action(),
        )
        .map_err(ElementTransitionFailure::dispatch)?;
    let postcondition = super::uix_agent_element_wait::execute_wait_from_baseline(
        client,
        window,
        target,
        input.postcondition().selector(),
        input.postcondition().condition(),
        action.revision,
        action.presented_revision,
    )
    .map_err(|source| ElementTransitionFailure::after_dispatch(source, action))?;
    Ok(ElementTransitionOutcome {
        action,
        postcondition,
    })
}

fn exact_source_node(
    nodes: &[NodeRecord],
    snapshot_id: &str,
    element_id: &str,
) -> Result<NodeRecord, ElementTransitionFailure> {
    let mut matched = None;
    for node in nodes {
        if uix_semantic_identity::element_id(snapshot_id, &node.native_id) != element_id {
            continue;
        }
        if matched.replace(node.clone()).is_some() {
            return Err(ElementTransitionFailure::before_dispatch(
                ElementTransitionFailureSource::AmbiguousSource,
            ));
        }
    }
    matched.ok_or_else(|| {
        ElementTransitionFailure::before_dispatch(ElementTransitionFailureSource::StaleElement)
    })
}

fn action_may_have_occurred(source: ActionFailure) -> bool {
    matches!(
        source,
        ActionFailure::OutcomeUnknown
            | ActionFailure::DidNotSettle
            // 这里的 transport 失败均发生在 perform/confirm 请求开始发送之后；即使远端回报
            // stale 或 timeout，也不能据此证明 mutation 未发生。
            | ActionFailure::Transport(_)
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

    fn fixture_client(stream: UnixStream) -> AgentClient {
        AgentClient {
            reader: BufReader::new(stream),
            deadline: Instant::now() + Duration::from_secs(2),
            request_types: BTreeSet::from([
                "hello".to_owned(),
                "list_windows".to_owned(),
                "snapshot".to_owned(),
                "perform".to_owned(),
                "confirm".to_owned(),
                "wait".to_owned(),
            ]),
            semantic_actions: BTreeSet::from(["invoke".to_owned()]),
            window_actions: BTreeSet::new(),
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

    fn input() -> UixElementTransitionInput {
        let target = "s2:w:0123456789abcdef";
        let snapshot_id = uix_semantic_identity::snapshot_id(target, 5, 5);
        let element_id = uix_semantic_identity::element_id(&snapshot_id, "7:1");
        let Ok(input) = UixElementTransitionInput::parse(&json!({
            "snapshotId": snapshot_id,
            "elementId": element_id,
            "action": { "type": "invoke" },
            "postcondition": {
                "selector": { "automationId": "saved" },
                "condition": "unique"
            },
            "timeoutMs": 1000
        })) else {
            panic!("测试语义 transition 输入必须有效");
        };
        input
    }

    fn node(id: &str, automation_id: &str) -> Value {
        json!({
            "node_id": id,
            "automation_id": automation_id,
            "parent": null,
            "focused": false,
            "role": "button",
            "name": "按钮",
            "frame": { "x": 1.0, "y": 2.0, "w": 20.0, "h": 10.0 },
            "visible_bounds": null,
            "state": { "disabled": false },
            "actions": ["invoke"]
        })
    }

    fn snapshot_reply(revision: u64, nodes: Vec<Value>) -> Value {
        json!({
            "schema": super::super::PROTOCOL_SCHEMA,
            "request_id": "act-snapshot",
            "ok": true,
            "type": "snapshot",
            "snapshot": {
                "window_id": 7,
                "generation": 3,
                "revision": revision,
                "presented_revision": revision,
                "closed": false,
                "nodes": nodes
            }
        })
    }

    #[test]
    fn missing_wait_negotiation_fails_before_any_provider_request() {
        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.request_types.remove("wait");
        let window = fixture_window();
        let Err(failure) = execute_transition(&mut client, &window, &window.session_id, &input())
        else {
            panic!("缺失 wait 必须在 provider 请求前失败");
        };
        assert_eq!(
            failure.source,
            ElementTransitionFailureSource::NegotiationUnavailable
        );
        assert!(!failure.accepted_may_have_occurred);
    }

    #[test]
    fn snapshot_perform_and_post_snapshot_share_one_connection() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            for step in 0..3 {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                let reply = match step {
                    0 => {
                        if request["type"] != "snapshot" {
                            return Err("首个请求必须是 snapshot".to_owned());
                        }
                        snapshot_reply(5, vec![node("7:1", "save")])
                    }
                    1 => {
                        if request["type"] != "perform"
                            || request["target"]["node_id"] != "7:1"
                            || request["expected_revision"] != 5
                        {
                            return Err("第二个请求必须是精确 perform".to_owned());
                        }
                        json!({
                            "schema": super::super::PROTOCOL_SCHEMA,
                            "request_id": "act-perform",
                            "ok": true,
                            "type": "perform",
                            "window_id": 7,
                            "generation": 3,
                            "revision": 6,
                            "presented_revision": 6,
                            "settled": false
                        })
                    }
                    _ => {
                        if request["type"] != "snapshot" {
                            return Err("动作后请求必须是 snapshot".to_owned());
                        }
                        snapshot_reply(6, vec![node("7:2", "saved")])
                    }
                };
                writeln!(reader.get_mut(), "{reply}").map_err(|error| error.to_string())?;
            }
            Ok(())
        });

        let mut client = fixture_client(client_stream);
        let window = fixture_window();
        let Ok(outcome) = execute_transition(&mut client, &window, &window.session_id, &input())
        else {
            panic!("同连接动作后条件命中必须成功");
        };
        assert_eq!(outcome.action.revision, 6);
        assert!(!outcome.action.settled);
        assert_eq!(outcome.postcondition.match_count, 1);
        assert_eq!(outcome.postcondition.sample_count, 1);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }

    #[test]
    fn post_dispatch_connection_loss_is_unknown_and_non_retryable() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            writeln!(
                reader.get_mut(),
                "{}",
                snapshot_reply(5, vec![node("7:1", "save")])
            )
            .map_err(|error| error.to_string())?;
            line.clear();
            reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            writeln!(
                reader.get_mut(),
                "{}",
                json!({
                    "schema": super::super::PROTOCOL_SCHEMA,
                    "request_id": "act-perform",
                    "ok": true,
                    "type": "perform",
                    "window_id": 7,
                    "generation": 3,
                    "revision": 6,
                    "presented_revision": 6,
                    "settled": true
                })
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        });

        let mut client = fixture_client(client_stream);
        let window = fixture_window();
        let Err(failure) = execute_transition(&mut client, &window, &window.session_id, &input())
        else {
            panic!("动作后连接丢失不得伪报成功");
        };
        assert!(failure.accepted_may_have_occurred);
        assert!(failure.action_accepted);
        assert_eq!(failure.action_revision, Some(6));
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器必须正常停止");
    }

    #[test]
    fn every_transport_failure_after_perform_is_treated_as_outcome_unknown() {
        for source in [
            Failure::Unavailable,
            Failure::PermissionDenied,
            Failure::Timeout,
            Failure::Protocol,
            Failure::Stale,
            Failure::Ambiguous,
        ] {
            assert!(action_may_have_occurred(ActionFailure::Transport(source)));
        }
        assert!(!action_may_have_occurred(ActionFailure::StaleRevision));
        assert!(!action_may_have_occurred(ActionFailure::InvalidValue));
    }
}
