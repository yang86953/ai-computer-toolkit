//! Linux AT-SPI 中立树投影 Module。

use serde_json::{Value, json};

use crate::{
    adapters::linux::atspi::{self, ConnectionConfig},
    domain::AppResult,
    modules::window_observation::public_error,
};

/// 执行一次总 deadline 内的 BFS，并仅在完整终态投影 JSON。
pub(crate) async fn read_tree(
    config: &ConnectionConfig,
    target: &str,
    maximum_depth: u8,
    maximum_items: usize,
) -> AppResult<Value> {
    let snapshot = atspi::read_tree(config, target, maximum_depth, maximum_items)
        .await
        .map_err(public_error)?;
    let nodes = snapshot
        .nodes
        .iter()
        .map(|node| {
            json!({
                "snapshotId": node.snapshot_id,
                "nodeId": node.node_id,
                "parentNodeId": node.parent_node_id,
                "depth": node.depth,
                "role": node.role,
                "name": node.name,
                "enabled": node.enabled,
                "childCount": node.child_count,
                "propertyReadComplete": node.property_read_complete,
            })
        })
        .collect::<Vec<_>>();
    let reasons = snapshot
        .reasons
        .iter()
        .map(|reason| reason.as_str())
        .collect::<Vec<_>>();
    let forbidden = &snapshot.audit;
    debug_assert_eq!(forbidden.get_all, 0);
    debug_assert_eq!(forbidden.get_children, 0);
    debug_assert_eq!(forbidden.cache_get_items, 0);
    debug_assert_eq!(forbidden.action, 0);
    debug_assert_eq!(forbidden.editable_text, 0);
    debug_assert_eq!(forbidden.selection, 0);
    debug_assert_eq!(forbidden.value, 0);
    debug_assert_eq!(forbidden.component_grab_focus, 0);
    debug_assert_eq!(forbidden.get_application_bus_address, 0);
    Ok(json!({
        "ok": true,
        "contractVersion": "act/accessibility-tree/v2",
        "capability": "accessibility.tree.read@2",
        "coverage": "partial-accessibility-exporters",
        "readOnly": true,
        "mutationAllowed": false,
        "sessionId": target,
        "snapshotId": snapshot.snapshot_id,
        "maximumDepth": maximum_depth,
        "maximumItems": maximum_items,
        "visited": nodes.len(),
        "truncated": !reasons.is_empty(),
        "truncationReasons": reasons,
        "nodes": nodes,
        "safety": {
            "publicationPolicy": "terminal-only-no-partial-results",
            "providerCache": "disabled",
            "nativeIdentityExposed": false,
            "textContentRead": false,
            "valueContentRead": false,
            "boundsExposed": false,
            "mutationInterfacesCalled": false,
            "forbiddenApiCalls": {
                "getAll": forbidden.get_all,
                "getChildren": forbidden.get_children,
                "cacheGetItems": forbidden.cache_get_items,
                "action": forbidden.action,
                "editableText": forbidden.editable_text,
                "selection": forbidden.selection,
                "value": forbidden.value,
                "componentGrabFocus": forbidden.component_grab_focus,
                "getApplicationBusAddress": forbidden.get_application_bus_address,
            },
        },
    }))
}
