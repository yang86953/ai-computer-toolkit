//! UIX Agent 窗口关闭请求与精确 generation 终态同步的同连接 Adapter。

use std::time::{Duration, Instant};

use crate::components::{
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    uix_window_close_transition_contract::UixWindowCloseTransitionInput,
};

use super::{
    ActionOutcome, AgentClient, Failure, RevisionWaitOutcome, RevisionWaitOutcomeKind,
    WindowActionFailure, WindowRecord, resolve_until,
};

/// 区分 mutation 前失败、关闭动作失败与动作后的终态观测失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowCloseTransitionFailureSource {
    TransportBeforeDispatch(Failure),
    NegotiationUnavailable,
    Action(WindowActionFailure),
    ClosedObservation(Failure),
}

/// 关闭 transition 失败时允许 Module 公开的最小中立事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowCloseTransitionFailure {
    pub(crate) source: WindowCloseTransitionFailureSource,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) action_accepted: bool,
    pub(crate) action_revision: Option<u64>,
    pub(crate) action_presented_revision: Option<u64>,
    pub(crate) action_settled: Option<bool>,
}

impl WindowCloseTransitionFailure {
    fn before_dispatch(source: WindowCloseTransitionFailureSource) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            action_accepted: false,
            action_revision: None,
            action_presented_revision: None,
            action_settled: None,
        }
    }

    fn action(source: WindowActionFailure) -> Self {
        Self {
            source: WindowCloseTransitionFailureSource::Action(source),
            accepted_may_have_occurred: close_may_have_occurred(source),
            action_accepted: false,
            action_revision: None,
            action_presented_revision: None,
            action_settled: None,
        }
    }

    fn after_dispatch(source: Failure, action: ActionOutcome) -> Self {
        Self {
            source: WindowCloseTransitionFailureSource::ClosedObservation(source),
            accepted_may_have_occurred: true,
            action_accepted: true,
            action_revision: Some(action.revision),
            action_presented_revision: Some(action.presented_revision),
            action_settled: Some(action.settled),
        }
    }
}

/// 同一认证操作连接上的关闭动作响应与显式 closed 终态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowCloseTransitionOutcome {
    pub(crate) action: ActionOutcome,
    pub(crate) closed: RevisionWaitOutcome,
}

/// 在一个总 deadline 内只解析一次目标并等待 Agent 显式证明精确 generation 已关闭。
pub(crate) fn perform_window_close_transition(
    target: &str,
    input: &UixWindowCloseTransitionInput,
) -> Result<WindowCloseTransitionOutcome, WindowCloseTransitionFailure> {
    if OpaqueTargetId::parse(target).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        return Err(WindowCloseTransitionFailure::before_dispatch(
            WindowCloseTransitionFailureSource::TransportBeforeDispatch(Failure::Stale),
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|source| {
        WindowCloseTransitionFailure::before_dispatch(
            WindowCloseTransitionFailureSource::TransportBeforeDispatch(source),
        )
    })?;
    if window.session_id != target {
        return Err(WindowCloseTransitionFailure::before_dispatch(
            WindowCloseTransitionFailureSource::TransportBeforeDispatch(Failure::Stale),
        ));
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|source| {
        WindowCloseTransitionFailure::before_dispatch(
            WindowCloseTransitionFailureSource::TransportBeforeDispatch(source),
        )
    })?;
    execute_close_transition(&mut client, &window)
}

fn execute_close_transition(
    client: &mut AgentClient,
    window: &WindowRecord,
) -> Result<WindowCloseTransitionOutcome, WindowCloseTransitionFailure> {
    // 首个 mutation 前一次性预检关闭动作与终态等待协议面。
    if !client.request_types.contains("perform")
        || !client.request_types.contains("wait")
        || !client.window_actions.contains("close_window")
    {
        return Err(WindowCloseTransitionFailure::before_dispatch(
            WindowCloseTransitionFailureSource::NegotiationUnavailable,
        ));
    }

    let action = client
        .perform_close(window.window_id, window.generation, window.revision)
        .map_err(WindowCloseTransitionFailure::action)?;
    let closed = client
        .wait_closed(
            window.window_id,
            window.generation,
            action.revision,
            action.presented_revision,
        )
        .map_err(|source| WindowCloseTransitionFailure::after_dispatch(source, action))?;
    if closed.kind != RevisionWaitOutcomeKind::Closed || !closed.closed {
        return Err(WindowCloseTransitionFailure::after_dispatch(
            Failure::Protocol,
            action,
        ));
    }
    Ok(WindowCloseTransitionOutcome { action, closed })
}

fn close_may_have_occurred(source: WindowActionFailure) -> bool {
    matches!(
        source,
        // transport 分类只会在 perform 帧开始发送后从窗口动作客户端返回。
        WindowActionFailure::Transport(_)
            | WindowActionFailure::NotInteractable
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
            window_actions: BTreeSet::from(["close_window".to_owned()]),
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
    fn missing_wait_negotiation_fails_before_provider_request() {
        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.request_types.remove("wait");
        let Err(failure) = execute_close_transition(&mut client, &fixture_window()) else {
            panic!("缺失 wait 必须在关闭请求前失败");
        };
        assert_eq!(
            failure.source,
            WindowCloseTransitionFailureSource::NegotiationUnavailable
        );
        assert!(!failure.accepted_may_have_occurred);
    }

    #[test]
    fn close_and_closed_wait_share_one_connection() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            for step in 0..2 {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                let reply = if step == 0 {
                    if request["type"] != "perform" || request["action"]["kind"] != "close_window" {
                        return Err("首个请求必须是 close_window perform".to_owned());
                    }
                    json!({
                        "schema": super::super::PROTOCOL_SCHEMA,
                        "request_id": request["request_id"],
                        "ok": true,
                        "type": "perform",
                        "window_id": 7,
                        "generation": 3,
                        "revision": 6,
                        "presented_revision": 6,
                        "settled": false
                    })
                } else {
                    if request["type"] != "wait" || request["after_revision"] != 6 {
                        return Err("第二个请求必须从动作 revision 等待关闭".to_owned());
                    }
                    json!({
                        "schema": super::super::PROTOCOL_SCHEMA,
                        "request_id": request["request_id"],
                        "ok": true,
                        "type": "wait",
                        "outcome": "closed",
                        "window": {
                            "window_id": 7,
                            "generation": 3,
                            "title": "fixture",
                            "visible": false,
                            "presentable": false,
                            "focused": false,
                            "logical_width": 800,
                            "logical_height": 600,
                            "maximized": false,
                            "minimized": false,
                            "fullscreen": false,
                            "revision": 7,
                            "presented_revision": 6,
                            "closed": true,
                            "screenshot_supported": false,
                            "activation_supported": false,
                            "pointer_drag_supported": false
                        }
                    })
                };
                writeln!(reader.get_mut(), "{reply}").map_err(|error| error.to_string())?;
            }
            Ok(())
        });

        let mut client = fixture_client(client_stream);
        let Ok(outcome) = execute_close_transition(&mut client, &fixture_window()) else {
            panic!("同连接显式关闭终态必须成功");
        };
        assert!(!outcome.action.settled);
        assert!(outcome.closed.closed);
        assert_eq!(outcome.closed.revision, 7);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }

    #[test]
    fn connection_loss_after_close_acceptance_is_unknown() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            let request =
                serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
            writeln!(
                reader.get_mut(),
                "{}",
                json!({
                    "schema": super::super::PROTOCOL_SCHEMA,
                    "request_id": request["request_id"],
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
        let Err(failure) = execute_close_transition(&mut client, &fixture_window()) else {
            panic!("动作后断连不得冒充 closed 终态");
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
    fn every_action_transport_failure_is_treated_as_possible_close() {
        for source in [
            Failure::Unavailable,
            Failure::PermissionDenied,
            Failure::Timeout,
            Failure::Protocol,
            Failure::Stale,
            Failure::Ambiguous,
        ] {
            assert!(close_may_have_occurred(WindowActionFailure::Transport(
                source
            )));
        }
        assert!(!close_may_have_occurred(WindowActionFailure::StaleRevision));
        assert!(!close_may_have_occurred(WindowActionFailure::Forbidden));
    }
}
