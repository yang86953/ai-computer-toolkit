//! UIX 协作式语义元素定位 Module。

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent::{self, Failure, NodeRecord, RectRecord, Snapshot},
    capabilities,
    components::uix_element_location_contract::UixElementLocateInput,
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

/// 重新认证精确窗口并在完整 UIX snapshot 上执行一次只读定位。
pub(crate) fn locate(session_id: &str, value: &Value) -> AppResult<Value> {
    let input = UixElementLocateInput::parse(value)
        .map_err(|message| AppControlError::new("INVALID_ARGUMENT", message))?;
    let snapshot = uix_agent::snapshot(session_id).map_err(uix_window::public_error)?;
    locate_snapshot(snapshot, &input)
}

fn locate_snapshot(snapshot: Snapshot, input: &UixElementLocateInput) -> AppResult<Value> {
    let mut native_ids = BTreeSet::new();
    let mut first_match = None;
    let mut match_count = 0_usize;
    for node in &snapshot.nodes {
        if !native_ids.insert(node.native_id.as_str()) {
            return Err(uix_window::public_error(Failure::Protocol));
        }
        if input.selector().matches(
            node.automation_id.as_deref(),
            &node.role,
            &node.name,
            node.focused,
            node.enabled,
            &node.actions,
        ) {
            match_count = match_count.saturating_add(1);
            if first_match.is_none() {
                first_match = Some(node);
            }
        }
    }
    if match_count > 1 {
        return Err(ambiguous_error());
    }
    let snapshot_id = uix_window::snapshot_id(
        &snapshot.session_id,
        snapshot.revision,
        snapshot.presented_revision,
    );
    let element = first_match.map(|node| public_element(node, &snapshot_id));
    Ok(json!({
        "ok": true,
        "contractVersion": "act/ui-element-location/v2",
        "capability": capabilities::UI_ELEMENT_LOCATE_V2,
        "targetId": snapshot.session_id,
        "snapshotId": snapshot_id,
        "revision": snapshot.revision,
        "presentedRevision": snapshot.presented_revision,
        "selectorSemantics": "exact-and",
        "matchState": if element.is_some() { "unique" } else { "missing" },
        "matchCount": usize::from(element.is_some()),
        "visited": snapshot.nodes.len(),
        "complete": true,
        "element": element,
        "targetReResolved": true,
        "executionRealm": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "safety": {
            "providerCache": "disabled",
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "sensitiveSnapshotPublished": false,
            "hostCoordinateMappingPublished": false,
            "clickPointInferred": false,
            "foregroundActivationRequested": false,
            "desktopInputInjected": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

fn public_element(node: &NodeRecord, snapshot_id: &str) -> Value {
    json!({
        "elementId": uix_window::public_node_id(snapshot_id, &node.native_id),
        "identityFreshness": "snapshot-revision",
        "automationId": node.automation_id,
        "role": node.role,
        "name": node.name,
        "focused": node.focused,
        "enabled": node.enabled,
        "actions": node.actions,
        "geometry": {
            "coordinateSpace": "application-client",
            "unit": "logical-px",
            "frame": public_rect(node.frame),
            "visibleBounds": node.visible_bounds.map(public_rect),
        },
    })
}

fn public_rect(rect: RectRecord) -> Value {
    json!({
        "x": rect.x,
        "y": rect.y,
        "width": rect.width,
        "height": rect.height,
    })
}

fn ambiguous_error() -> AppControlError {
    AppControlError::with_details(
        "AMBIGUOUS_TARGET",
        "The UIX semantic selector matched more than one current element.",
        json!({
            "platform": "linux",
            "provider": "uix-agent-v1",
            "matchCount": 2,
            "matchCountSemantics": "two-or-more",
            "selectorPublished": false,
            "executionRealm": "same-session-no-focus",
            "fallback": "none",
        }),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn node(id: &str, automation_id: Option<&str>) -> NodeRecord {
        NodeRecord {
            native_id: id.to_owned(),
            parent_native_id: None,
            automation_id: automation_id.map(str::to_owned),
            focused: false,
            role: "button".to_owned(),
            name: "保存".to_owned(),
            enabled: true,
            actions: vec!["focus".to_owned(), "invoke".to_owned()],
            frame: RectRecord {
                x: 10.0,
                y: 20.0,
                width: 80.0,
                height: 30.0,
            },
            visible_bounds: Some(RectRecord {
                x: 12.0,
                y: 20.0,
                width: 78.0,
                height: 30.0,
            }),
        }
    }

    fn snapshot(nodes: Vec<NodeRecord>) -> Snapshot {
        Snapshot {
            session_id: "s2:w:0000000000000000".to_owned(),
            revision: 7,
            presented_revision: 6,
            nodes,
        }
    }

    #[test]
    fn exact_selector_returns_snapshot_scoped_logical_geometry() {
        // 测试夹具使用显式分支保留失败语义，不使用 expect。
        let Ok(input) = UixElementLocateInput::parse(&json!({
            "selector": { "automationId": "save", "action": "invoke" }
        })) else {
            panic!("fixture input must parse");
        };
        let Ok(result) = locate_snapshot(snapshot(vec![node("7:1", Some("save"))]), &input) else {
            panic!("unique fixture must locate");
        };
        assert_eq!(result["matchState"], "unique");
        assert_eq!(result["element"]["geometry"]["unit"], "logical-px");
        assert_eq!(result["element"]["geometry"]["frame"]["width"], 80.0);
        assert!(
            result["element"]["elementId"]
                .as_str()
                .is_some_and(|id| id.starts_with("s2:e:"))
        );
    }

    #[test]
    fn missing_is_success_and_multiple_matches_are_ambiguous() {
        let Ok(missing) = UixElementLocateInput::parse(&json!({
            "selector": { "automationId": "missing" }
        })) else {
            panic!("missing selector must parse");
        };
        let Ok(result) = locate_snapshot(snapshot(vec![node("7:1", Some("save"))]), &missing)
        else {
            panic!("complete missing search must succeed");
        };
        assert_eq!(result["matchState"], "missing");
        assert_eq!(result["element"], Value::Null);

        let Ok(ambiguous) = UixElementLocateInput::parse(&json!({
            "selector": { "role": "button" }
        })) else {
            panic!("ambiguous selector must parse");
        };
        let Err(error) = locate_snapshot(
            snapshot(vec![node("7:1", Some("save")), node("7:2", Some("open"))]),
            &ambiguous,
        ) else {
            panic!("multiple matches must fail");
        };
        assert_eq!(error.code, "AMBIGUOUS_TARGET");
        assert_eq!(error.details["matchCountSemantics"], "two-or-more");
    }

    #[test]
    fn complete_scan_rejects_protocol_corruption_after_multiple_matches() {
        let Ok(ambiguous) = UixElementLocateInput::parse(&json!({
            "selector": { "role": "button" }
        })) else {
            panic!("ambiguous selector must parse");
        };
        let Err(error) = locate_snapshot(
            snapshot(vec![
                node("7:1", Some("save")),
                node("7:2", Some("open")),
                node("7:1", Some("duplicate")),
            ]),
            &ambiguous,
        ) else {
            panic!("the complete snapshot must still be validated");
        };
        assert_eq!(error.code, "WORKER_PROTOCOL_ERROR");
    }
}
