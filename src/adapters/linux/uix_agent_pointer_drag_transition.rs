//! UIX Agent 请求内配平拖拽与提交后语义条件同步的同连接 Adapter。

use std::time::{Duration, Instant};

use crate::components::{
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    uix_pointer_drag_transition_contract::UixPointerDragTransitionInput,
};

use super::{
    AgentClient, ElementWaitFailure, ElementWaitOutcome, Failure, PointerDragFailure,
    PointerDragOutcome, WindowRecord, resolve_until,
};

/// 区分 mutation 前失败、拖拽失败与完整 release 后的条件观测失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PointerDragTransitionFailureSource {
    Transport(Failure),
    NegotiationUnavailable,
    Drag(PointerDragFailure),
    Postcondition(ElementWaitFailure),
}

/// transition 失败时允许 Module 公开的最小中立事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PointerDragTransitionFailure {
    pub(crate) source: PointerDragTransitionFailureSource,
    pub(crate) accepted_may_have_occurred: bool,
    pub(crate) drag_completed: bool,
    pub(crate) drag_revision: Option<u64>,
    pub(crate) drag_presented_revision: Option<u64>,
    pub(crate) drag_settled: Option<bool>,
    pub(crate) pointer_down_may_have_occurred: bool,
    pub(crate) pointer_down_accepted: bool,
    pub(crate) move_samples_accepted: u16,
    pub(crate) button_release_attempted: bool,
    pub(crate) button_release_confirmed: bool,
}

impl PointerDragTransitionFailure {
    fn before_dispatch(source: PointerDragTransitionFailureSource) -> Self {
        Self {
            source,
            accepted_may_have_occurred: false,
            drag_completed: false,
            drag_revision: None,
            drag_presented_revision: None,
            drag_settled: None,
            pointer_down_may_have_occurred: false,
            pointer_down_accepted: false,
            move_samples_accepted: 0,
            button_release_attempted: false,
            button_release_confirmed: false,
        }
    }

    fn drag(failure: PointerDragFailure) -> Self {
        let accepted_may_have_occurred = failure.pointer_down_may_have_occurred
            || failure.pointer_down_accepted
            || failure.move_samples_accepted > 0
            || failure.button_release_attempted;
        Self {
            source: PointerDragTransitionFailureSource::Drag(failure),
            accepted_may_have_occurred,
            drag_completed: false,
            drag_revision: None,
            drag_presented_revision: None,
            drag_settled: None,
            pointer_down_may_have_occurred: failure.pointer_down_may_have_occurred,
            pointer_down_accepted: failure.pointer_down_accepted,
            move_samples_accepted: failure.move_samples_accepted,
            button_release_attempted: failure.button_release_attempted,
            button_release_confirmed: failure.button_release_confirmed,
        }
    }

    fn after_drag(source: ElementWaitFailure, drag: PointerDragOutcome) -> Self {
        Self {
            source: PointerDragTransitionFailureSource::Postcondition(source),
            accepted_may_have_occurred: true,
            drag_completed: true,
            drag_revision: Some(drag.revision),
            drag_presented_revision: Some(drag.presented_revision),
            drag_settled: Some(drag.settled),
            pointer_down_may_have_occurred: true,
            pointer_down_accepted: true,
            move_samples_accepted: drag.move_samples_accepted,
            button_release_attempted: true,
            button_release_confirmed: true,
        }
    }
}

/// 完整拖拽 release 与提交后语义条件均可信时的中立事实。
#[derive(Debug)]
pub(crate) struct PointerDragTransitionOutcome {
    pub(crate) drag: PointerDragOutcome,
    pub(crate) postcondition: ElementWaitOutcome,
}

/// 在一个总 deadline 与认证连接内执行配平拖拽，随后等待语义后置条件。
pub(crate) fn perform_pointer_drag_transition(
    target: &str,
    input: &UixPointerDragTransitionInput,
) -> Result<PointerDragTransitionOutcome, PointerDragTransitionFailure> {
    if OpaqueTargetId::parse(target).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        return Err(PointerDragTransitionFailure::before_dispatch(
            PointerDragTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|source| {
        PointerDragTransitionFailure::before_dispatch(
            PointerDragTransitionFailureSource::Transport(source),
        )
    })?;
    if window.session_id != target {
        return Err(PointerDragTransitionFailure::before_dispatch(
            PointerDragTransitionFailureSource::Transport(Failure::Stale),
        ));
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|source| {
        PointerDragTransitionFailure::before_dispatch(
            PointerDragTransitionFailureSource::Transport(source),
        )
    })?;
    execute_transition(&mut client, &window, target, input)
}

fn execute_transition(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixPointerDragTransitionInput,
) -> Result<PointerDragTransitionOutcome, PointerDragTransitionFailure> {
    // pointer_down 前一次性预检拖拽与语义修订同步所需的完整协议面。
    if !["snapshot", "perform", "wait"]
        .iter()
        .all(|request| client.request_types.contains(*request))
        || !["pointer_down", "pointer_move", "pointer_up"]
            .iter()
            .all(|action| client.window_actions.contains(*action))
    {
        return Err(PointerDragTransitionFailure::before_dispatch(
            PointerDragTransitionFailureSource::NegotiationUnavailable,
        ));
    }
    if super::remaining(client.deadline).is_err() {
        return Err(PointerDragTransitionFailure::before_dispatch(
            PointerDragTransitionFailureSource::Transport(Failure::Timeout),
        ));
    }

    // 既有拖拽 Adapter 保证 pointer_down -> pointer_move -> pointer_up，并在失败时尽力释放。
    let drag = super::uix_agent_pointer_drag::execute_drag(client, window, target, input.drag())
        .map_err(PointerDragTransitionFailure::drag)?;
    // 只有完整 pointer_up 已确认时才允许从最终拖拽修订进入语义条件等待。
    let postcondition = super::uix_agent_element_wait::execute_wait_from_baseline(
        client,
        window,
        target,
        input.postcondition().selector(),
        input.postcondition().condition(),
        drag.revision,
        drag.presented_revision,
    )
    .map_err(|source| PointerDragTransitionFailure::after_drag(source, drag))?;
    Ok(PointerDragTransitionOutcome {
        drag,
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

    fn input() -> UixPointerDragTransitionInput {
        let Ok(input) = UixPointerDragTransitionInput::parse(&json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 10.0, "y": 20.0 },
            "end": { "x": 30.0, "y": 40.0 },
            "samples": 1,
            "durationMs": 0,
            "timeoutMs": 1000,
            "postcondition": {
                "selector": { "automationId": "drag-target" },
                "condition": "unique"
            }
        })) else {
            panic!("测试 pointer drag transition 输入必须有效");
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
                "pointer_down".to_owned(),
                "pointer_move".to_owned(),
                "pointer_up".to_owned(),
            ]),
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
            pointer_drag_supported: true,
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
    fn missing_wait_or_drag_action_fails_before_first_dispatch() {
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
            panic!("缺失 wait 必须在 pointer_down 前失败");
        };
        assert_eq!(
            failure.source,
            PointerDragTransitionFailureSource::NegotiationUnavailable
        );
        assert!(!failure.accepted_may_have_occurred);
        assert!(!failure.button_release_attempted);
    }

    #[test]
    fn balanced_drag_and_postcondition_share_one_connection() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            for (index, kind) in ["pointer_down", "pointer_move", "pointer_up"]
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
                    return Err("拖拽 transition 动作顺序不匹配".to_owned());
                }
                if request["expected_revision"] != 5 + index as u64 {
                    return Err("拖拽 transition 修订链接不匹配".to_owned());
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
                    "settled": index != 2,
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
                return Err("完整 pointer_up 后必须进入同连接 snapshot".to_owned());
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
                        "automation_id": "drag-target",
                        "parent": null,
                        "focused": false,
                        "role": "status",
                        "name": "拖拽目标",
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
            panic!("同连接配平拖拽与语义条件必须成功");
        };
        assert_eq!(outcome.drag.move_samples_accepted, 1);
        assert_eq!(outcome.drag.revision, 8);
        assert!(!outcome.drag.settled);
        assert_eq!(outcome.postcondition.match_count, 1);
        assert_eq!(outcome.postcondition.wait_count, 0);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }
}
