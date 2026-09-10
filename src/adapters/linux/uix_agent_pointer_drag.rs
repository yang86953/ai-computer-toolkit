//! UIX Agent 固定左键原子拖拽的私有 Adapter。

use std::{
    thread,
    time::{Duration, Instant},
};

use serde_json::json;

use crate::components::uix_pointer_drag_contract::UixPointerDragInput;

use super::{
    AgentClient, Failure, WindowActionFailure, WindowRecord, resolve_until, window_session_id,
};

const RELEASE_RESERVE: Duration = Duration::from_millis(100);
const RELEASE_REVISION_PROBE_BUDGET: Duration = Duration::from_millis(50);
const RELEASE_SEND_RESERVE: Duration = Duration::from_millis(50);

/// 原子拖拽完成后可公开投影的最小事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PointerDragOutcome {
    pub(crate) revision: u64,
    pub(crate) presented_revision: u64,
    pub(crate) settled: bool,
    pub(crate) move_samples_accepted: u16,
}

/// 拖拽失败以及请求内左键释放的保守事实。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PointerDragFailure {
    pub(crate) source: WindowActionFailure,
    pub(crate) pointer_down_may_have_occurred: bool,
    pub(crate) pointer_down_accepted: bool,
    pub(crate) move_samples_accepted: u16,
    pub(crate) button_release_attempted: bool,
    pub(crate) button_release_confirmed: bool,
}

impl PointerDragFailure {
    fn before_dispatch(source: WindowActionFailure) -> Self {
        Self {
            source,
            pointer_down_may_have_occurred: false,
            pointer_down_accepted: false,
            move_samples_accepted: 0,
            button_release_attempted: false,
            button_release_confirmed: false,
        }
    }

    fn after_down_attempt(source: WindowActionFailure, may_have_occurred: bool) -> Self {
        Self {
            source,
            pointer_down_may_have_occurred: may_have_occurred,
            pointer_down_accepted: false,
            move_samples_accepted: 0,
            button_release_attempted: false,
            button_release_confirmed: false,
        }
    }
}

/// 重新解析窗口后，在同一认证连接内执行固定左键拖拽并配平释放。
pub(crate) fn perform_pointer_drag(
    target: &str,
    input: &UixPointerDragInput,
) -> Result<PointerDragOutcome, PointerDragFailure> {
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(|failure| {
        PointerDragFailure::before_dispatch(WindowActionFailure::Transport(failure))
    })?;
    let mut client = AgentClient::connect_until(&window.endpoint, deadline).map_err(|failure| {
        PointerDragFailure::before_dispatch(WindowActionFailure::Transport(failure))
    })?;
    execute_drag(&mut client, &window, target, input)
}

pub(super) fn execute_drag(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixPointerDragInput,
) -> Result<PointerDragOutcome, PointerDragFailure> {
    // 三个动作与 perform 请求必须在 down 前一次性预检，禁止部分能力冒充拖拽。
    if !client.request_types.contains("perform")
        || !["pointer_down", "pointer_move", "pointer_up"]
            .iter()
            .all(|action| client.window_actions.contains(*action))
    {
        return Err(PointerDragFailure::before_dispatch(
            WindowActionFailure::UnsupportedAction,
        ));
    }
    let required = Duration::from_millis(u64::from(input.duration_ms())) + RELEASE_RESERVE;
    if client.deadline.saturating_duration_since(Instant::now()) < required {
        return Err(PointerDragFailure::before_dispatch(
            WindowActionFailure::Transport(Failure::Timeout),
        ));
    }

    let down = perform_before_release_deadline(
        client,
        "act-pointer-drag-down",
        window.window_id,
        window.generation,
        window.revision,
        pointer_action("pointer_down", input.start().x(), input.start().y()),
    );
    let down = match down {
        Ok(outcome) => outcome,
        Err(source) => {
            let may_have_occurred = dispatch_may_have_occurred(source);
            let mut failure = PointerDragFailure::after_down_attempt(source, may_have_occurred);
            if may_have_occurred {
                release_after_failure(
                    client,
                    window,
                    target,
                    (input.start().x(), input.start().y()),
                    window.revision,
                    &mut failure,
                );
            }
            return Err(failure);
        }
    };

    let sequence_started = Instant::now();
    let mut latest = down;
    let mut move_samples_accepted = 0_u16;
    for sample in 1..=input.samples() {
        let ratio = f64::from(sample) / f64::from(input.samples());
        let sample_time = sequence_started
            + Duration::from_secs_f64(
                Duration::from_millis(u64::from(input.duration_ms())).as_secs_f64() * ratio,
            );
        if sample_time + RELEASE_RESERVE > client.deadline {
            let mut failure = partial_failure(
                WindowActionFailure::Transport(Failure::Timeout),
                move_samples_accepted,
            );
            release_after_failure(
                client,
                window,
                target,
                interpolated_point(input, ratio),
                latest.revision,
                &mut failure,
            );
            return Err(failure);
        }
        if let Some(wait) = sample_time.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
        let point = interpolated_point(input, ratio);
        match perform_before_release_deadline(
            client,
            "act-pointer-drag-move",
            window.window_id,
            window.generation,
            latest.revision,
            pointer_action("pointer_move", point.0, point.1),
        ) {
            Ok(outcome) => {
                latest = outcome;
                move_samples_accepted += 1;
            }
            Err(source) => {
                let mut failure = partial_failure(source, move_samples_accepted);
                release_after_failure(client, window, target, point, latest.revision, &mut failure);
                return Err(failure);
            }
        }
    }

    match client.perform_targetless_with_request_id(
        "act-pointer-drag-up",
        window.window_id,
        window.generation,
        latest.revision,
        pointer_action("pointer_up", input.end().x(), input.end().y()),
    ) {
        Ok(outcome) => Ok(PointerDragOutcome {
            revision: outcome.revision,
            presented_revision: outcome.presented_revision,
            settled: outcome.settled,
            move_samples_accepted,
        }),
        Err(source) => Err(PointerDragFailure {
            source,
            pointer_down_may_have_occurred: true,
            pointer_down_accepted: true,
            move_samples_accepted,
            button_release_attempted: true,
            button_release_confirmed: false,
        }),
    }
}

fn partial_failure(source: WindowActionFailure, move_samples_accepted: u16) -> PointerDragFailure {
    PointerDragFailure {
        source,
        pointer_down_may_have_occurred: true,
        pointer_down_accepted: true,
        move_samples_accepted,
        button_release_attempted: false,
        button_release_confirmed: false,
    }
}

fn perform_before_release_deadline(
    client: &mut AgentClient,
    request_id: &'static str,
    window_id: u64,
    generation: u64,
    expected_revision: u64,
    action: serde_json::Value,
) -> Result<super::ActionOutcome, WindowActionFailure> {
    let overall_deadline = client.deadline;
    let Some(dispatch_deadline) = overall_deadline.checked_sub(RELEASE_RESERVE) else {
        return Err(WindowActionFailure::Transport(Failure::Timeout));
    };
    if dispatch_deadline <= Instant::now() {
        return Err(WindowActionFailure::Transport(Failure::Timeout));
    }
    client.deadline = dispatch_deadline;
    let result = client.perform_targetless_with_request_id(
        request_id,
        window_id,
        generation,
        expected_revision,
        action,
    );
    client.deadline = overall_deadline;
    result
}

fn release_after_failure(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    point: (f64, f64),
    last_known_revision: u64,
    failure: &mut PointerDragFailure,
) {
    let revision = current_release_revision(client, window, target, last_known_revision)
        .unwrap_or(last_known_revision);
    failure.button_release_attempted = true;
    failure.button_release_confirmed = client
        .perform_targetless_with_request_id(
            "act-pointer-drag-release",
            window.window_id,
            window.generation,
            revision,
            pointer_action("pointer_up", point.0, point.1),
        )
        .is_ok();
}

fn current_release_revision(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    minimum_revision: u64,
) -> Option<u64> {
    // 新鲜修订探测最多占用一半最小释放余量，避免 list_windows 吞掉实际 up 的机会。
    let overall_deadline = client.deadline;
    let remaining = overall_deadline.saturating_duration_since(Instant::now());
    let probe_budget = remaining
        .saturating_sub(RELEASE_SEND_RESERVE)
        .min(RELEASE_REVISION_PROBE_BUDGET);
    if probe_budget.is_zero() {
        return None;
    }
    client.deadline = Instant::now() + probe_budget;
    let windows = client.list_windows();
    client.deadline = overall_deadline;
    let windows = windows.ok()?;
    if windows.len() > super::MAX_WINDOWS {
        return None;
    }
    let mut matching = windows
        .iter()
        .filter(|candidate| candidate.window_id == window.window_id);
    let candidate = matching.next()?;
    if matching.next().is_some()
        || candidate.closed
        || candidate.generation != window.generation
        || candidate.revision < minimum_revision
        || candidate.presented_revision < window.presented_revision
        || candidate.presented_revision > candidate.revision
        || window_session_id(&window.endpoint, candidate) != target
    {
        return None;
    }
    Some(candidate.revision)
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

fn interpolated_point(input: &UixPointerDragInput, ratio: f64) -> (f64, f64) {
    let start = input.start();
    let end = input.end();
    (
        start.x() + (end.x() - start.x()) * ratio,
        start.y() + (end.y() - start.y()) * ratio,
    )
}

fn pointer_action(kind: &'static str, x: f64, y: f64) -> serde_json::Value {
    json!({ "kind": kind, "x": x, "y": y })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixStream,
        thread,
    };

    use super::*;

    fn input(samples: u16) -> UixPointerDragInput {
        UixPointerDragInput::parse(&json!({
            "coordinateSpace": "client-logical-px",
            "start": { "x": 10, "y": 20 },
            "end": { "x": 30, "y": 60 },
            "samples": samples,
            "durationMs": 0,
            "timeoutMs": 1000
        }))
        .unwrap_or_else(|_| panic!("测试拖拽输入必须有效"))
    }

    fn fixture_client(stream: UnixStream) -> AgentClient {
        AgentClient {
            reader: BufReader::new(stream),
            deadline: Instant::now() + Duration::from_secs(2),
            request_types: BTreeSet::from(["perform".to_owned()]),
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
        let endpoint = super::super::EndpointDescriptor {
            schema: super::super::PROTOCOL_SCHEMA.to_owned(),
            process_id: std::process::id(),
            endpoint: "/fixture/agent.sock".to_owned(),
            token: "7".repeat(64),
            state: "ready".to_owned(),
        };
        let wire = super::super::WireWindow {
            window_id: 7,
            generation: 3,
            title: "fixture".to_owned(),
            visible: true,
            presentable: true,
            logical_width: None,
            logical_height: None,
            maximized: None,
            minimized: None,
            fullscreen: None,
            focused: None,
            revision: 5,
            presented_revision: 5,
            closed: false,
        };
        WindowRecord {
            session_id: window_session_id(&endpoint, &wire),
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
            endpoint,
            window_id: 7,
            generation: 3,
        }
    }

    #[test]
    fn drag_uses_one_connection_and_balances_down_moves_up() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            for (index, kind) in ["pointer_down", "pointer_move", "pointer_move", "pointer_up"]
                .into_iter()
                .enumerate()
            {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request = serde_json::from_str::<serde_json::Value>(&line)
                    .map_err(|error| error.to_string())?;
                if request["action"]["kind"] != kind {
                    return Err("拖拽动作顺序不匹配".to_owned());
                }
                let expected_request_id = match kind {
                    "pointer_down" => "act-pointer-drag-down",
                    "pointer_move" => "act-pointer-drag-move",
                    "pointer_up" => "act-pointer-drag-up",
                    _ => return Err("未知拖拽动作".to_owned()),
                };
                if request["request_id"] != expected_request_id {
                    return Err("拖拽阶段 request id 不匹配".to_owned());
                }
                if request["expected_revision"] != 5 + index as u64 {
                    return Err("拖拽修订链接不匹配".to_owned());
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
        let window = fixture_window();
        let Ok(outcome) = execute_drag(&mut client, &window, &window.session_id, &input(2)) else {
            panic!("测试拖拽必须成功");
        };
        assert_eq!(outcome.move_samples_accepted, 2);
        assert_eq!(outcome.revision, 9);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }

    #[test]
    fn move_failure_uses_fresh_revision_for_confirmed_release() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);
            for stage in ["down", "move", "list", "release"] {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .map_err(|error| error.to_string())?;
                let request = serde_json::from_str::<serde_json::Value>(&line)
                    .map_err(|error| error.to_string())?;
                let expected_request_id = match stage {
                    "down" => "act-pointer-drag-down",
                    "move" => "act-pointer-drag-move",
                    "list" => "act-list-windows",
                    "release" => "act-pointer-drag-release",
                    _ => return Err("未知测试阶段".to_owned()),
                };
                if request["request_id"] != expected_request_id {
                    return Err("失败释放阶段 request id 不匹配".to_owned());
                }
                let reply = match stage {
                    "down" => json!({
                        "schema": super::super::PROTOCOL_SCHEMA,
                        "request_id": request["request_id"],
                        "ok": true,
                        "type": "perform",
                        "window_id": 7,
                        "generation": 3,
                        "revision": 6,
                        "presented_revision": 6,
                        "settled": true
                    }),
                    "move" => json!({
                        "schema": super::super::PROTOCOL_SCHEMA,
                        "request_id": request["request_id"],
                        "ok": false,
                        "error": { "code": "not_interactable", "message": "fixture" }
                    }),
                    "list" => json!({
                        "schema": super::super::PROTOCOL_SCHEMA,
                        "request_id": request["request_id"],
                        "ok": true,
                        "type": "list_windows",
                        "windows": [{
                            "window_id": 7,
                            "generation": 3,
                            "title": "fixture",
                            "visible": true,
                            "presentable": true,
                            "revision": 6,
                            "presented_revision": 6,
                            "closed": false
                        }]
                    }),
                    "release" => {
                        if request["action"]["kind"] != "pointer_up"
                            || request["expected_revision"] != 6
                        {
                            return Err("安全释放必须使用最新修订与 pointer_up".to_owned());
                        }
                        json!({
                            "schema": super::super::PROTOCOL_SCHEMA,
                            "request_id": request["request_id"],
                            "ok": true,
                            "type": "perform",
                            "window_id": 7,
                            "generation": 3,
                            "revision": 7,
                            "presented_revision": 7,
                            "settled": true
                        })
                    }
                    _ => return Err("未知测试阶段".to_owned()),
                };
                writeln!(reader.get_mut(), "{reply}").map_err(|error| error.to_string())?;
            }
            Ok(())
        });
        let mut client = fixture_client(client_stream);
        let window = fixture_window();
        let Err(failure) = execute_drag(&mut client, &window, &window.session_id, &input(2)) else {
            panic!("移动失败必须结束拖拽");
        };
        assert!(failure.pointer_down_accepted);
        assert!(failure.button_release_attempted);
        assert!(failure.button_release_confirmed);
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试安全释放协议必须成功");
    }

    #[test]
    fn incomplete_action_catalog_fails_before_pointer_down() {
        let Ok((client_stream, _server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let mut client = fixture_client(client_stream);
        client.window_actions.remove("pointer_up");
        let window = fixture_window();
        let Err(failure) = execute_drag(&mut client, &window, &window.session_id, &input(2)) else {
            panic!("缺少 pointer_up 时必须在 down 前失败");
        };
        assert_eq!(failure.source, WindowActionFailure::UnsupportedAction);
        assert!(!failure.pointer_down_may_have_occurred);
        assert!(!failure.button_release_attempted);
    }
}
