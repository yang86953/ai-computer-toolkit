//! 协作式 UIX Agent 窗口与语义树 Module。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{
        self, Failure, NodeRecord, RevisionWaitOutcomeKind, WindowRecord,
    },
    capabilities,
    components::{uix_semantic_identity, window_revision_wait_contract::WindowRevisionWaitInput},
    domain::{AppControlError, AppResult},
};

const MAXIMUM_DEPTH: usize = 20;
const MAXIMUM_ITEMS: usize = 4096;

/// 枚举显式启用 UIX Agent 的应用窗口。
pub(crate) fn discover(maximum_items: usize) -> AppResult<Value> {
    validate_limits(0, maximum_items)?;
    let inventory = uix_agent::discover(maximum_items).map_err(public_error)?;
    let windows = inventory
        .windows
        .iter()
        .map(public_window)
        .collect::<Vec<_>>();
    let truncated = inventory.warnings.iter().any(|warning| {
        matches!(
            *warning,
            "deadline" | "endpoint-limit" | "item-limit" | "provider-item-limit"
        )
    });
    Ok(json!({
        "ok": true,
        "contractVersion": "act/window-observation/v3",
        "capability": capabilities::WINDOW_DISCOVER_V3,
        "coverage": "opt-in-uix-agent-applications",
        "executionDomain": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "count": windows.len(),
        "total": inventory.total,
        "truncated": truncated,
        "complete": inventory.complete,
        "warnings": inventory.warnings,
        "windows": windows,
        "safety": public_safety(),
    }))
}

/// 使用当前端点和窗口代际重新解析目标并读取元数据。
pub(crate) fn metadata(target: &str) -> AppResult<Value> {
    let window = uix_agent::resolve(target).map_err(public_error)?;
    Ok(json!({
        "ok": true,
        "contractVersion": "act/window-metadata/v3",
        "capability": capabilities::WINDOW_METADATA_READ_V3,
        "coverage": "opt-in-uix-agent-applications",
        "executionDomain": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "window": public_window(&window),
        "safety": public_safety(),
    }))
}

/// 重新认证目标并读取 UIX 已协商的窗口框架当前状态。
pub(crate) fn state(target: &str, value: &Value) -> AppResult<Value> {
    if value.as_object().is_none_or(|object| !object.is_empty()) {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "UIX window state read input must be an empty object.",
        ));
    }
    let window = uix_agent::resolve(target).map_err(public_error)?;
    let state = window.state.ok_or_else(window_state_unavailable)?;
    Ok(json!({
        "ok": true,
        "contractVersion": "act/window-state-read/v1",
        "capability": capabilities::WINDOW_STATE_READ,
        "targetId": window.session_id,
        "observationSource": "uix-framework-current",
        "coordinateSpace": "client-logical-px",
        "visible": window.visible,
        "presentable": window.presentable,
        "clientSize": {
            "width": state.logical_width,
            "height": state.logical_height,
        },
        "windowState": {
            "maximized": state.maximized,
            "minimized": state.minimized,
            "fullscreen": state.fullscreen,
        },
        "compositorFinalStateConfirmed": false,
        "targetReResolved": true,
        "executionRealm": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "safety": {
            "providerCache": "disabled",
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "foregroundActivationRequested": false,
            "desktopInputInjected": false,
            "globalWindowStateClaimed": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

/// 将未协商新字段的旧 Agent 投影为明确 capability 缺口。
pub(crate) fn window_state_unavailable() -> AppControlError {
    AppControlError::with_details(
        "CAPABILITY_UNAVAILABLE",
        "The target UIX Agent did not negotiate window state fields.",
        json!({
            "platform": "linux",
            "provider": "uix-agent-v1",
            "requiredNegotiation": "hello.capabilities.window_state_fields",
            "executionRealm": "none",
            "fallback": "none",
        }),
    )
}

/// 在总 deadline 内等待精确窗口代际满足修订条件或关闭。
pub(crate) fn wait_revision(target: &str, value: &Value) -> AppResult<Value> {
    let input = WindowRevisionWaitInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let outcome = uix_agent::wait_revision(target, input.condition, input.timeout_ms)
        .map_err(public_error)?;
    let outcome_name = match outcome.kind {
        RevisionWaitOutcomeKind::Changed => "changed",
        RevisionWaitOutcomeKind::Presented => "presented",
        RevisionWaitOutcomeKind::Closed => "closed",
    };
    Ok(json!({
        "ok": true,
        "contractVersion": "act/window-revision-wait/v1",
        "capability": capabilities::WINDOW_REVISION_WAIT,
        "targetId": target,
        "condition": input.condition.as_str(),
        "thresholdRevision": input.condition.revision(),
        "outcome": outcome_name,
        "revision": outcome.revision,
        "presentedRevision": outcome.presented_revision,
        "closed": outcome.closed,
        "targetReResolved": true,
        "timeoutMs": input.timeout_ms,
        "executionRealm": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "safety": {
            "providerCache": "disabled",
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "foregroundActivationRequested": false,
            "desktopInputInjected": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

/// 读取 UIX 完整协议快照后，在 Module 内投影有界中立树。
pub(crate) fn read_tree(
    target: &str,
    maximum_depth: usize,
    maximum_items: usize,
) -> AppResult<Value> {
    validate_limits(maximum_depth, maximum_items)?;
    let snapshot = uix_agent::snapshot(target).map_err(public_error)?;
    let snapshot_id = snapshot_id(
        &snapshot.session_id,
        snapshot.revision,
        snapshot.presented_revision,
    );
    let projected = project_nodes(&snapshot.nodes, &snapshot_id, maximum_depth, maximum_items)?;
    Ok(json!({
        "ok": true,
        "contractVersion": "act/accessibility-tree/v3",
        "capability": capabilities::ACCESSIBILITY_TREE_READ_V3,
        "coverage": "opt-in-uix-agent-applications",
        "executionDomain": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "sessionId": snapshot.session_id,
        "snapshotId": snapshot_id,
        "revision": snapshot.revision,
        "presentedRevision": snapshot.presented_revision,
        "maximumDepth": maximum_depth,
        "maximumItems": maximum_items,
        "visited": projected.nodes.len(),
        "truncated": !projected.reasons.is_empty(),
        "truncationReasons": projected.reasons,
        "nodes": projected.nodes,
        "safety": {
            "publicationPolicy": "terminal-only-no-partial-results",
            "providerCache": "disabled",
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "protocolSnapshotContainsSensitiveFields": true,
            "boundsExposed": false,
            "textInterfaceContentExposed": false,
            "valueContentExposed": false,
            "selectionContentExposed": false,
            "mutationInterfacesCalled": false,
        },
    }))
}

fn validate_limits(maximum_depth: usize, maximum_items: usize) -> AppResult<()> {
    if maximum_depth > MAXIMUM_DEPTH || !(1..=MAXIMUM_ITEMS).contains(&maximum_items) {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "UIX inspection requires maxDepth 0..20 and maxItems 1..4096.",
        ));
    }
    Ok(())
}

fn public_window(window: &WindowRecord) -> Value {
    let mut window_capabilities = vec![
        capabilities::WINDOW_METADATA_READ_V3,
        capabilities::ACCESSIBILITY_TREE_READ_V3,
        capabilities::UI_ELEMENT_LOCATE_V2,
        capabilities::UI_ELEMENT_WAIT_V2,
        capabilities::UI_ELEMENT_ACTION_V2,
        capabilities::UI_ELEMENT_TRANSITION,
        capabilities::UI_INPUT_KEY_V2,
        capabilities::UI_INPUT_KEY_SEQUENCE,
        capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION,
        capabilities::UI_INPUT_KEY_TRANSITION,
        capabilities::UI_INPUT_SEQUENCE,
        capabilities::UI_INPUT_SEQUENCE_TRANSITION,
        capabilities::UI_INPUT_POINTER_V2,
        capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE,
        capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION,
        capabilities::UI_INPUT_POINTER_CLICK_TRANSITION,
        capabilities::UI_INPUT_POINTER_MOVE_TRANSITION,
        capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE,
        capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION,
        capabilities::UI_INPUT_POINTER_SEQUENCE,
        capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION,
        capabilities::WINDOW_REVISION_WAIT,
        capabilities::WINDOW_CLOSED_WAIT_V2,
        capabilities::WINDOW_CLOSE_V2,
        capabilities::WINDOW_CLOSE_TRANSITION,
        capabilities::WINDOW_LIFECYCLE_V2,
        capabilities::WINDOW_LIFECYCLE_SEQUENCE,
    ];
    if window.state.is_some() {
        window_capabilities.push(capabilities::WINDOW_STATE_READ);
    }
    if window.state.is_some() && window.focused.is_some() {
        window_capabilities.push(capabilities::WINDOW_STATE_WAIT);
        window_capabilities.push(capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION);
        window_capabilities.push(capabilities::WINDOW_LIFECYCLE_TRANSITION);
    }
    if window.screenshot_supported {
        window_capabilities.push(capabilities::WINDOW_SCREENSHOT_V2);
    }
    if window.activation_supported {
        window_capabilities.push(capabilities::WINDOW_ACTIVATE);
        if window.focused.is_some() {
            window_capabilities.push(capabilities::WINDOW_ACTIVATE_TRANSITION);
        }
    }
    if window.pointer_drag_supported {
        window_capabilities.push(capabilities::UI_INPUT_POINTER_DRAG);
        window_capabilities.push(capabilities::UI_INPUT_POINTER_DRAG_TRANSITION);
    }
    json!({
        "sessionId": window.session_id,
        "targetKind": "uix-agent-window",
        "title": window.title,
        "visible": window.visible,
        "presentable": window.presentable,
        "focused": window.focused,
        "revision": window.revision,
        "presentedRevision": window.presented_revision,
        "targetIdentity": {
            "contractVersion": "act/window-target-identity/v3",
            "provider": "uix-agent-v1",
            "coverage": "opt-in-uix-agent-applications",
            "freshness": "agent-process-token-and-window-generation",
            "mutationAllowed": false,
            "generationOwner": "uix-application",
            "nativeIdentityExposed": false,
        },
        "capabilities": window_capabilities,
    })
}

fn public_safety() -> Value {
    json!({
        "foregroundClaimed": false,
        "globalWindowDirectoryClaimed": false,
        "arbitraryThirdPartyAppsClaimed": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "x11Used": false,
        "fallback": "none",
    })
}

#[derive(Debug)]
struct ProjectedTree {
    nodes: Vec<Value>,
    reasons: Vec<&'static str>,
}

fn project_nodes(
    nodes: &[NodeRecord],
    snapshot_id: &str,
    maximum_depth: usize,
    maximum_items: usize,
) -> AppResult<ProjectedTree> {
    let mut by_id = BTreeMap::new();
    for node in nodes {
        if by_id.insert(node.native_id.as_str(), node).is_some() {
            return Err(public_error(Failure::Protocol));
        }
    }
    let mut children = BTreeMap::<&str, Vec<&str>>::new();
    let mut roots = Vec::new();
    for node in nodes {
        match node.parent_native_id.as_deref() {
            Some(parent) if by_id.contains_key(parent) => {
                children.entry(parent).or_default().push(&node.native_id);
            }
            Some(_) => return Err(public_error(Failure::Protocol)),
            None => roots.push(node.native_id.as_str()),
        }
    }
    if !nodes.is_empty() && roots.is_empty() {
        return Err(public_error(Failure::Protocol));
    }
    roots.sort_unstable();
    for values in children.values_mut() {
        values.sort_unstable();
    }

    let mut queue = roots
        .into_iter()
        .map(|node_id| (node_id, 0_usize))
        .collect::<VecDeque<_>>();
    let mut visited = BTreeSet::new();
    let mut output = Vec::new();
    let mut reasons = BTreeSet::new();
    while let Some((native_id, depth)) = queue.pop_front() {
        if !visited.insert(native_id) {
            return Err(public_error(Failure::Protocol));
        }
        if depth > maximum_depth {
            reasons.insert("depth-limit");
            continue;
        }
        if output.len() == maximum_items {
            reasons.insert("item-limit");
            break;
        }
        let node = by_id
            .get(native_id)
            .copied()
            .ok_or_else(|| public_error(Failure::Protocol))?;
        let node_id = public_node_id(snapshot_id, native_id);
        let parent_node_id = node
            .parent_native_id
            .as_deref()
            .map(|parent| public_node_id(snapshot_id, parent));
        output.push(json!({
            "snapshotId": snapshot_id,
            "nodeId": node_id,
            "parentNodeId": parent_node_id,
            "automationId": node.automation_id,
            "depth": depth,
            "role": node.role,
            "name": node.name,
            "focused": node.focused,
            "enabled": node.enabled,
            "actions": node.actions,
            "propertyReadComplete": true,
        }));
        if let Some(descendants) = children.get(native_id) {
            if depth == maximum_depth && !descendants.is_empty() {
                reasons.insert("depth-limit");
            } else {
                queue.extend(descendants.iter().map(|child| (*child, depth + 1)));
            }
        }
    }
    if reasons.is_empty() && visited.len() != nodes.len() {
        return Err(public_error(Failure::Protocol));
    }
    Ok(ProjectedTree {
        nodes: output,
        reasons: reasons.into_iter().collect(),
    })
}

pub(crate) fn snapshot_id(session_id: &str, revision: u64, presented_revision: u64) -> String {
    uix_semantic_identity::snapshot_id(session_id, revision, presented_revision)
}

pub(crate) fn public_node_id(snapshot_id: &str, native_id: &str) -> String {
    uix_semantic_identity::element_id(snapshot_id, native_id)
}

pub(crate) fn public_error(failure: Failure) -> AppControlError {
    let (code, message, state) = match failure {
        Failure::Unavailable => (
            "AUTHORIZED_ENDPOINT_UNAVAILABLE",
            "The authorized UIX Agent endpoint is unavailable.",
            "unavailable",
        ),
        Failure::PermissionDenied => (
            "ENDPOINT_AUTHENTICATION_FAILED",
            "The UIX Agent endpoint identity or authentication was rejected.",
            "authentication-rejected",
        ),
        Failure::Timeout => (
            "TIMEOUT",
            "The bounded UIX Agent request timed out.",
            "timeout",
        ),
        Failure::Protocol => (
            "WORKER_PROTOCOL_ERROR",
            "The UIX Agent protocol response was invalid.",
            "protocol-error",
        ),
        Failure::Stale => (
            "STALE_SESSION",
            "The UIX Agent window target is no longer current.",
            "stale",
        ),
        Failure::Ambiguous => (
            "AMBIGUOUS_TARGET",
            "The opaque UIX Agent window matched more than one current target.",
            "ambiguous",
        ),
    };
    AppControlError::with_details(
        code,
        message,
        json!({
            "platform": "linux",
            "provider": "uix-agent-v1",
            "providerState": state,
            "executionRealm": "same-session-no-focus",
            "fallback": "none",
            "foregroundActivationAllowed": false,
            "inputAllowed": false,
            "transportIdentityExposed": false,
            "partialResultPublished": false,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, parent: Option<&str>) -> NodeRecord {
        NodeRecord {
            native_id: id.to_owned(),
            parent_native_id: parent.map(str::to_owned),
            automation_id: Some(format!("automation-{id}")),
            focused: false,
            role: "button".to_owned(),
            name: format!("node-{id}"),
            enabled: true,
            actions: vec!["invoke".to_owned()],
            frame: crate::adapters::linux::uix_agent::RectRecord {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            visible_bounds: None,
        }
    }

    #[test]
    fn tree_projection_is_parent_first_bounded_and_opaque() {
        let nodes = vec![node("child", Some("root")), node("root", None)];
        let projected = project_nodes(&nodes, "as3:0123456789abcdef", 0, 10)
            .expect("fixture tree must project");
        assert_eq!(projected.nodes.len(), 1);
        assert_eq!(projected.reasons, ["depth-limit"]);
        assert_eq!(projected.nodes[0]["depth"], 0);
        assert!(
            projected.nodes[0]["nodeId"]
                .as_str()
                .unwrap()
                .starts_with("s2:e:")
        );
        assert_ne!(projected.nodes[0]["nodeId"], "root");
    }

    #[test]
    fn tree_projection_rejects_missing_parent() {
        let error = project_nodes(
            &[node("child", Some("missing"))],
            "as3:0123456789abcdef",
            20,
            10,
        )
        .expect_err("missing parent must fail closed");
        assert_eq!(error.code, "WORKER_PROTOCOL_ERROR");
    }
}
