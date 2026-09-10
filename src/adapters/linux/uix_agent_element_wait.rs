//! UIX Agent 同连接语义元素 revision wait 的私有 Adapter。

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::components::{
    opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    uix_element_location_contract::UixElementSelector,
    uix_element_wait_contract::{UixElementWaitCondition, UixElementWaitInput},
    window_revision_wait_contract::WindowRevisionWaitCondition,
};

use super::{
    AgentClient, Failure, MAX_NODES, NodeRecord, Snapshot, WindowRecord, WireSnapshot, rect_record,
    resolve_until, truncate_utf8,
};

/// 元素语义歧义独立于窗口 target 歧义，避免错误文案混淆两种边界。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ElementWaitFailure {
    Transport(Failure),
    Ambiguous { match_count: usize },
}

/// 条件满足时公开给 Module 的完整快照与中立节点投影源。
#[derive(Debug)]
pub(crate) struct ElementWaitOutcome {
    pub(crate) snapshot: Snapshot,
    pub(crate) element: Option<NodeRecord>,
    pub(crate) match_count: usize,
    pub(crate) sample_count: u32,
    pub(crate) wait_count: u32,
}

/// 在总 deadline 内重新解析一次；随后用同一认证操作连接等待语义 revision。
pub(crate) fn perform_element_wait(
    target: &str,
    input: &UixElementWaitInput,
) -> Result<ElementWaitOutcome, ElementWaitFailure> {
    if OpaqueTargetId::parse(target).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Window) {
        return Err(ElementWaitFailure::Transport(Failure::Stale));
    }
    let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms()));
    let window = resolve_until(target, deadline).map_err(ElementWaitFailure::Transport)?;
    if window.session_id != target {
        return Err(ElementWaitFailure::Transport(Failure::Stale));
    }
    let mut client = AgentClient::connect_until(&window.endpoint, deadline)
        .map_err(ElementWaitFailure::Transport)?;
    execute_wait(&mut client, &window, target, input)
}

fn execute_wait(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    input: &UixElementWaitInput,
) -> Result<ElementWaitOutcome, ElementWaitFailure> {
    if !client.request_types.contains("snapshot") {
        return Err(ElementWaitFailure::Transport(Failure::Unavailable));
    }

    execute_wait_from_baseline(
        client,
        window,
        target,
        input.selector(),
        input.condition(),
        window.revision,
        window.presented_revision,
    )
}

/// 从调用方给定的可信修订基线开始，在同一连接等待元素条件。
pub(super) fn execute_wait_from_baseline(
    client: &mut AgentClient,
    window: &WindowRecord,
    target: &str,
    selector: &UixElementSelector,
    condition: UixElementWaitCondition,
    mut baseline_revision: u64,
    mut baseline_presented_revision: u64,
) -> Result<ElementWaitOutcome, ElementWaitFailure> {
    let mut sample_count = 0_u32;
    let mut wait_count = 0_u32;
    loop {
        let wire = client
            .snapshot(window.window_id, window.generation)
            .map_err(ElementWaitFailure::Transport)?;
        sample_count = sample_count.saturating_add(1);
        let snapshot = snapshot_from_wire(
            wire,
            window,
            target,
            baseline_revision,
            baseline_presented_revision,
        )
        .map_err(ElementWaitFailure::Transport)?;
        let (match_count, element) = classify_matches(&snapshot, selector)?;
        if condition_satisfied(condition, match_count) {
            return Ok(ElementWaitOutcome {
                snapshot,
                element,
                match_count,
                sample_count,
                wait_count,
            });
        }

        let waited = client
            .wait_revision(
                window.window_id,
                window.generation,
                snapshot.revision,
                snapshot.presented_revision,
                WindowRevisionWaitCondition::RevisionAfter {
                    revision: snapshot.revision,
                },
            )
            .map_err(ElementWaitFailure::Transport)?;
        wait_count = wait_count.saturating_add(1);
        match waited.kind {
            super::RevisionWaitOutcomeKind::Changed => {
                baseline_revision = waited.revision;
                baseline_presented_revision = waited.presented_revision;
            }
            super::RevisionWaitOutcomeKind::Closed => {
                return Err(ElementWaitFailure::Transport(Failure::Stale));
            }
            super::RevisionWaitOutcomeKind::Presented => {
                return Err(ElementWaitFailure::Transport(Failure::Protocol));
            }
        }
    }
}

pub(super) fn condition_satisfied(condition: UixElementWaitCondition, match_count: usize) -> bool {
    match condition {
        UixElementWaitCondition::Unique => match_count == 1,
        UixElementWaitCondition::Missing => match_count == 0,
    }
}

pub(super) fn classify_matches(
    snapshot: &Snapshot,
    selector: &UixElementSelector,
) -> Result<(usize, Option<NodeRecord>), ElementWaitFailure> {
    let mut first_match = None;
    let mut match_count = 0_usize;
    for node in &snapshot.nodes {
        if selector.matches(
            node.automation_id.as_deref(),
            &node.role,
            &node.name,
            node.focused,
            node.enabled,
            &node.actions,
        ) {
            match_count = match_count.saturating_add(1);
            if first_match.is_none() {
                first_match = Some(node.clone());
            }
            if match_count > 1 {
                return Err(ElementWaitFailure::Ambiguous { match_count });
            }
        }
    }
    Ok((match_count, first_match))
}

pub(super) fn snapshot_from_wire(
    wire: WireSnapshot,
    fixed_window: &WindowRecord,
    target: &str,
    baseline_revision: u64,
    baseline_presented_revision: u64,
) -> Result<Snapshot, Failure> {
    if fixed_window.session_id != target {
        return Err(Failure::Stale);
    }
    if wire.window_id != fixed_window.window_id || wire.generation != fixed_window.generation {
        return Err(Failure::Stale);
    }
    if wire.closed {
        return Err(Failure::Stale);
    }
    if wire.revision < baseline_revision
        || wire.presented_revision < baseline_presented_revision
        || wire.presented_revision > wire.revision
    {
        return Err(Failure::Protocol);
    }
    if wire.nodes.len() > MAX_NODES {
        return Err(Failure::Protocol);
    }

    let mut native_ids = std::collections::BTreeSet::new();
    let mut nodes = Vec::with_capacity(wire.nodes.len());
    for node in wire.nodes {
        if node.node_id.is_empty()
            || node.node_id.len() > 128
            || !native_ids.insert(node.node_id.clone())
            || node.parent.as_ref().is_some_and(|value| value.len() > 128)
            || node
                .automation_id
                .as_ref()
                .is_some_and(|value| value.len() > 512)
            || node.role.is_empty()
            || node.role.len() > 64
            || node.actions.len() > 32
            || node
                .actions
                .iter()
                .any(|action| action.is_empty() || action.len() > 64)
            || node
                .actions
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != node.actions.len()
        {
            return Err(Failure::Protocol);
        }
        nodes.push(NodeRecord {
            native_id: node.node_id,
            parent_native_id: node.parent,
            automation_id: node.automation_id,
            focused: node.focused,
            role: node.role,
            name: node
                .name
                .as_deref()
                .map_or_else(String::new, |name| truncate_utf8(name, 256)),
            enabled: !node.state.disabled,
            actions: node.actions,
            frame: rect_record(node.frame)?,
            visible_bounds: node.visible_bounds.map(rect_record).transpose()?,
        });
    }

    // 完整 snapshot 的父边界也必须封闭，避免把孤立或自引用节点误当作当前元素。
    if nodes.iter().any(|node| {
        node.parent_native_id
            .as_deref()
            .is_some_and(|parent| parent == node.native_id || !native_ids.contains(parent))
    }) {
        return Err(Failure::Protocol);
    }
    if !nodes.is_empty() && nodes.iter().all(|node| node.parent_native_id.is_some()) {
        return Err(Failure::Protocol);
    }

    Ok(Snapshot {
        session_id: fixed_window.session_id.clone(),
        revision: wire.revision,
        presented_revision: wire.presented_revision,
        nodes,
    })
}

fn error_details(match_count: usize) -> Value {
    json!({
        "platform": "linux",
        "provider": "uix-agent-v1",
        "targetKind": "element",
        "matchCount": match_count,
        "matchCountSemantics": "two-or-more",
        "selectorPublished": false,
        "executionRealm": "same-session-no-focus",
        "fallback": "none",
    })
}

/// 仅供 Module 使用的元素歧义细节，保持窗口 target 错误与元素错误分离。
pub(crate) fn ambiguous_details(match_count: usize) -> Value {
    error_details(match_count)
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

    fn fixture_client(stream: UnixStream) -> AgentClient {
        AgentClient {
            reader: BufReader::new(stream),
            deadline: Instant::now() + Duration::from_secs(2),
            request_types: BTreeSet::from([
                "hello".to_owned(),
                "list_windows".to_owned(),
                "snapshot".to_owned(),
                "wait".to_owned(),
            ]),
            semantic_actions: BTreeSet::new(),
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

    fn input(condition: &str) -> UixElementWaitInput {
        let Ok(input) = UixElementWaitInput::parse(&json!({
            "selector": { "automationId": "save" },
            "condition": condition,
            "timeoutMs": 1000
        })) else {
            panic!("元素等待 fixture 输入必须有效");
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

    fn wire_snapshot(revision: u64, nodes: Vec<Value>) -> Value {
        json!({
            "window_id": 7,
            "generation": 3,
            "revision": revision,
            "presented_revision": revision,
            "closed": false,
            "nodes": nodes
        })
    }

    fn snapshot_with_nodes(nodes: Vec<NodeRecord>) -> Snapshot {
        Snapshot {
            session_id: "s2:w:0123456789abcdef".to_owned(),
            revision: 5,
            presented_revision: 5,
            nodes,
        }
    }

    fn node_record(id: &str, automation_id: Option<&str>) -> NodeRecord {
        NodeRecord {
            native_id: id.to_owned(),
            parent_native_id: None,
            automation_id: automation_id.map(str::to_owned),
            focused: false,
            role: "button".to_owned(),
            name: "按钮".to_owned(),
            enabled: true,
            actions: vec!["invoke".to_owned()],
            frame: super::super::RectRecord {
                x: 1.0,
                y: 2.0,
                width: 20.0,
                height: 10.0,
            },
            visible_bounds: None,
        }
    }

    #[test]
    fn match_classification_keeps_unique_missing_and_ambiguous_distinct() {
        let Ok(unique_input) = UixElementWaitInput::parse(&json!({
            "selector": { "automationId": "save" },
            "condition": "unique"
        })) else {
            panic!("唯一 fixture 输入必须有效");
        };
        let snapshot = snapshot_with_nodes(vec![node_record("7:1", Some("save"))]);
        let Ok((count, element)) = classify_matches(&snapshot, unique_input.selector()) else {
            panic!("唯一匹配必须分类成功");
        };
        assert_eq!(count, 1);
        assert!(element.is_some());
        assert!(condition_satisfied(unique_input.condition(), count));

        let Ok(missing_input) = UixElementWaitInput::parse(&json!({
            "selector": { "automationId": "missing" },
            "condition": "missing"
        })) else {
            panic!("缺失 fixture 输入必须有效");
        };
        let Ok((count, element)) = classify_matches(&snapshot, missing_input.selector()) else {
            panic!("缺失匹配必须分类成功");
        };
        assert_eq!(count, 0);
        assert!(element.is_none());
        assert!(condition_satisfied(missing_input.condition(), count));

        let Ok(ambiguous_input) = UixElementWaitInput::parse(&json!({
            "selector": { "role": "button" },
            "condition": "unique"
        })) else {
            panic!("歧义 fixture 输入必须有效");
        };
        let Err(ElementWaitFailure::Ambiguous { match_count }) = classify_matches(
            &snapshot_with_nodes(vec![
                node_record("7:1", Some("save")),
                node_record("7:2", Some("open")),
            ]),
            ambiguous_input.selector(),
        ) else {
            panic!("多匹配必须是元素语义歧义");
        };
        assert_eq!(match_count, 2);
    }

    #[test]
    fn snapshot_validation_rejects_duplicate_or_unbounded_node_boundaries() {
        let window = fixture_window();
        let Ok(wire) = serde_json::from_value::<WireSnapshot>(wire_snapshot(
            5,
            vec![node("7:1", "save"), node("7:1", "duplicate")],
        )) else {
            panic!("snapshot fixture wire 必须有效");
        };
        let Err(error) = snapshot_from_wire(wire, &window, &window.session_id, 5, 5) else {
            panic!("重复节点必须拒绝");
        };
        assert_eq!(error, Failure::Protocol);

        let Ok(wire) =
            serde_json::from_value::<WireSnapshot>(wire_snapshot(4, vec![node("7:1", "save")]))
        else {
            panic!("revision fixture wire 必须有效");
        };
        let Err(error) = snapshot_from_wire(wire, &window, &window.session_id, 5, 5) else {
            panic!("倒退 revision 必须拒绝");
        };
        assert_eq!(error, Failure::Protocol);
    }

    #[test]
    fn unsatisfied_snapshot_uses_one_connection_and_revision_after_wait() {
        let Ok((client_stream, server_stream)) = UnixStream::pair() else {
            panic!("测试 Unix 连接必须创建成功");
        };
        let server = thread::spawn(move || -> Result<(), String> {
            let mut reader = BufReader::new(server_stream);

            let mut line = String::new();
            reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            let snapshot_request =
                serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
            if snapshot_request["request_id"] != "act-snapshot"
                || snapshot_request["type"] != "snapshot"
            {
                return Err("首个请求必须是 snapshot".to_owned());
            }
            writeln!(
                reader.get_mut(),
                "{}",
                json!({
                    "schema": super::super::PROTOCOL_SCHEMA,
                    "request_id": "act-snapshot",
                    "ok": true,
                    "type": "snapshot",
                    "snapshot": wire_snapshot(5, vec![node("7:1", "open")])
                })
            )
            .map_err(|error| error.to_string())?;

            line.clear();
            reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            let wait_request =
                serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
            if wait_request["request_id"] != "act-wait"
                || wait_request["type"] != "wait"
                || wait_request["after_revision"] != 5
            {
                return Err("第二个请求必须是 revision-after wait".to_owned());
            }
            writeln!(
                reader.get_mut(),
                "{}",
                json!({
                    "schema": super::super::PROTOCOL_SCHEMA,
                    "request_id": "act-wait",
                    "ok": true,
                    "type": "wait",
                    "outcome": "changed",
                    "window": {
                        "window_id": 7,
                        "generation": 3,
                        "title": "fixture",
                        "visible": true,
                        "presentable": true,
                        "logical_width": null,
                        "logical_height": null,
                        "maximized": null,
                        "minimized": null,
                        "fullscreen": null,
                        "focused": null,
                        "revision": 6,
                        "presented_revision": 6,
                        "closed": false
                    }
                })
            )
            .map_err(|error| error.to_string())?;

            line.clear();
            reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            let final_request =
                serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
            if final_request["request_id"] != "act-snapshot" || final_request["type"] != "snapshot"
            {
                return Err("第三个请求必须复用 snapshot".to_owned());
            }
            writeln!(
                reader.get_mut(),
                "{}",
                json!({
                    "schema": super::super::PROTOCOL_SCHEMA,
                    "request_id": "act-snapshot",
                    "ok": true,
                    "type": "snapshot",
                    "snapshot": wire_snapshot(6, vec![node("7:1", "save")])
                })
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        });

        let mut client = fixture_client(client_stream);
        let window = fixture_window();
        let target = window.session_id.clone();
        let Ok(outcome) = execute_wait(&mut client, &window, &target, &input("unique")) else {
            panic!("revision-after 元素等待必须成功");
        };
        assert_eq!(outcome.match_count, 1);
        assert_eq!(outcome.sample_count, 2);
        assert_eq!(outcome.wait_count, 1);
        assert_eq!(outcome.snapshot.revision, 6);
        assert!(outcome.element.is_some());
        let Ok(server_result) = server.join() else {
            panic!("测试服务器必须正常停止");
        };
        assert!(server_result.is_ok(), "测试服务器协议必须成功");
    }
}
