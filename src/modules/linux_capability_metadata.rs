//! Linux provider 可用性投影 Module。

use serde_json::{Value, json};

use crate::{
    adapters::linux::{process_termination_pidfd, wayland_portal},
    capabilities,
};

/// 返回不复制公开 capability 定义的 Linux 运行时矩阵。
pub(crate) fn surface() -> Value {
    let portal = wayland_portal::probe();
    surface_for(&portal)
}

fn surface_for(portal: &wayland_portal::WaylandPortalFacts) -> Value {
    let process_termination_ready = process_termination_pidfd::runtime_supported();
    let portal_screenshot_ready = portal.wayland_socket_count > 0
        && portal.user_bus_socket_present
        && portal.user_bus_reachable
        && portal.portal_service_registered
        && portal
            .screenshot_version
            .is_some_and(|version| version >= 2);
    let portal_session_ready = portal.wayland_socket_count > 0
        && portal.user_bus_socket_present
        && portal.user_bus_reachable
        && portal.portal_service_registered
        && portal
            .remote_desktop_version
            .is_some_and(|version| version >= 2)
        && portal
            .screen_cast_version
            .is_some_and(|version| version >= 5);
    let entries = capabilities::ALL
        .iter()
        .map(|definition| {
            let procfs_available = matches!(
                definition.id,
                capabilities::APPLICATION_DISCOVER_V2
                    | capabilities::APPLICATION_DISCOVER_V3
                    | capabilities::APPLICATION_SESSION_DISCOVER_V2
                    | capabilities::APPLICATION_SESSION_DISCOVER_V3
                    | capabilities::APPLICATION_SESSION_DISCOVER_V4
                    | capabilities::PROCESS_DISCOVER
                    | capabilities::PROCESS_METADATA_READ
            );
            let uix_available = matches!(
                definition.id,
                capabilities::APPLICATION_SESSION_DISCOVER_V3
                    | capabilities::APPLICATION_SESSION_DISCOVER_V4
                    | capabilities::WINDOW_DISCOVER_V3
                    | capabilities::WINDOW_METADATA_READ_V3
                    | capabilities::ACCESSIBILITY_TREE_READ_V3
                    | capabilities::UI_ELEMENT_LOCATE_V2
                    | capabilities::UI_ELEMENT_WAIT_V2
                    | capabilities::UI_ELEMENT_ACTION_V2
                    | capabilities::UI_ELEMENT_TRANSITION
                    | capabilities::UI_INPUT_KEY_V2
                    | capabilities::UI_INPUT_KEY_SEQUENCE
                    | capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION
                    | capabilities::UI_INPUT_KEY_TRANSITION
                    | capabilities::UI_INPUT_SEQUENCE
                    | capabilities::UI_INPUT_SEQUENCE_TRANSITION
                    | capabilities::UI_INPUT_POINTER_V2
                    | capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE
                    | capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION
                    | capabilities::UI_INPUT_POINTER_CLICK_TRANSITION
                    | capabilities::UI_INPUT_POINTER_MOVE_TRANSITION
                    | capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE
                    | capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION
                    | capabilities::UI_INPUT_POINTER_SEQUENCE
                    | capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION
                    | capabilities::UI_INPUT_POINTER_DRAG
                    | capabilities::UI_INPUT_POINTER_DRAG_TRANSITION
                    | capabilities::WINDOW_REVISION_WAIT
                    | capabilities::WINDOW_CLOSED_WAIT_V2
                    | capabilities::WINDOW_CLOSE_V2
                    | capabilities::WINDOW_CLOSE_TRANSITION
                    | capabilities::WINDOW_LIFECYCLE_V2
                    | capabilities::WINDOW_LIFECYCLE_SEQUENCE
                    | capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION
                    | capabilities::WINDOW_LIFECYCLE_TRANSITION
                    | capabilities::WINDOW_STATE_READ
                    | capabilities::WINDOW_STATE_WAIT
                    | capabilities::WINDOW_SCREENSHOT_V2
                    | capabilities::WINDOW_ACTIVATE
                    | capabilities::WINDOW_ACTIVATE_TRANSITION
            );
            let mpris_available = crate::modules::media_playback::is_capability(definition.id);
            let available = procfs_available
                || mpris_available
                || uix_available
                || definition.id == capabilities::APPLICATION_OPEN_V2
                || (matches!(
                    definition.id,
                    capabilities::PROCESS_TERMINATE_GRACEFUL_V2
                        | capabilities::PROCESS_TERMINATE_FORCE_V2
                )
                    && process_termination_ready)
                || (definition.id == capabilities::DESKTOP_SCREENSHOT_INTERACTIVE
                    && portal_screenshot_ready);
            let status = match definition.id {
                capabilities::MEDIA_SESSION_DISCOVER_V3
                | capabilities::MEDIA_PLAYBACK_STATE_READ_V3
                | capabilities::MEDIA_PLAYBACK_CONTROL_V3 => "production-route-current-target-assessment-required",
                capabilities::APPLICATION_DISCOVER_V2 => {
                    "available-partial-linux-xdg-desktop-entry-and-procfs"
                }
                capabilities::APPLICATION_DISCOVER_V3 => {
                    "available-verified-linux-xdg-procfs-with-toolkit-fixture-launch-status"
                }
                capabilities::APPLICATION_OPEN_V2 => {
                    "available-route-toolkit-self-executable-fixture-only"
                }
                capabilities::APPLICATION_SESSION_DISCOVER_V2 => {
                    "available-verified-read-only-linux-xdg-procfs-aggregation"
                }
                capabilities::APPLICATION_SESSION_DISCOVER_V3 => {
                    "available-verified-read-only-linux-uix-aware-aggregation"
                }
                capabilities::APPLICATION_SESSION_DISCOVER_V4 => {
                    "available-verified-read-only-linux-launch-aware-uix-aggregation"
                }
                capabilities::PROCESS_DISCOVER | capabilities::PROCESS_METADATA_READ => "available",
                capabilities::PROCESS_TERMINATE_GRACEFUL_V2
                | capabilities::PROCESS_TERMINATE_FORCE_V2
                    if process_termination_ready =>
                {
                    "available-verified-procfs-owner-generation-bound-same-non-root-uid-pidfd"
                }
                capabilities::WINDOW_SCREENSHOT_V2 => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-visual-acceptance-deferred"
                }
                capabilities::WINDOW_ACTIVATE => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-foreground-acceptance-deferred"
                }
                capabilities::WINDOW_ACTIVATE_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::UI_INPUT_POINTER_DRAG => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-acceptance-deferred"
                }
                capabilities::UI_INPUT_POINTER_DRAG_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::UI_INPUT_KEY_SEQUENCE => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-acceptance-deferred"
                }
                capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::UI_INPUT_KEY_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::UI_INPUT_SEQUENCE => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-acceptance-deferred"
                }
                capabilities::UI_INPUT_SEQUENCE_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-acceptance-deferred"
                }
                capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::UI_INPUT_POINTER_CLICK_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::UI_INPUT_POINTER_MOVE_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-acceptance-deferred"
                }
                capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::UI_INPUT_POINTER_SEQUENCE => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-acceptance-deferred"
                }
                capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::WINDOW_LIFECYCLE_SEQUENCE => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-acceptance-deferred"
                }
                capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::WINDOW_STATE_WAIT => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-observation-acceptance-deferred"
                }
                capabilities::WINDOW_LIFECYCLE_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-observation-acceptance-deferred"
                }
                capabilities::UI_ELEMENT_WAIT_V2 => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-observation-acceptance-deferred"
                }
                capabilities::UI_ELEMENT_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::WINDOW_CLOSE_TRANSITION => {
                    "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
                }
                capabilities::WINDOW_DISCOVER_V3
                | capabilities::WINDOW_METADATA_READ_V3
                | capabilities::ACCESSIBILITY_TREE_READ_V3
                | capabilities::UI_ELEMENT_LOCATE_V2
                | capabilities::UI_ELEMENT_ACTION_V2
                | capabilities::UI_INPUT_KEY_V2
                | capabilities::UI_INPUT_POINTER_V2
                | capabilities::WINDOW_REVISION_WAIT
                | capabilities::WINDOW_CLOSED_WAIT_V2
                | capabilities::WINDOW_CLOSE_V2
                | capabilities::WINDOW_LIFECYCLE_V2
                | capabilities::WINDOW_STATE_READ => {
                    "available-verified-cooperative-uix-agent-v1-wayland"
                }
                capabilities::DESKTOP_SCREENSHOT_INTERACTIVE if portal_screenshot_ready => {
                    "live-acceptance-pending-confirmation-and-foreground-consent"
                }
                capabilities::DESKTOP_SESSION_OPEN
                | capabilities::DESKTOP_SESSION_CLOSE
                | capabilities::SCREEN_CAPTURE
                | capabilities::DESKTOP_OBSERVE
                    if portal_session_ready =>
                {
                    "production-route-live-acceptance-pending"
                }
                capabilities::UI_INPUT_KEY_V3 | capabilities::UI_INPUT_POINTER_V3 | capabilities::DESKTOP_INTERACTION
                    if portal_session_ready =>
                {
                    "production-route-live-acceptance-pending-not-advertised"
                }
                capabilities::WINDOW_DISCOVER_V2
                | capabilities::WINDOW_METADATA_READ_V2
                | capabilities::ACCESSIBILITY_TREE_READ_V2 => {
                    "candidate-private-dbus-fixture-only-live-host-unavailable"
                }
                capabilities::MEDIA_SESSION_DISCOVER_V2
                | capabilities::MEDIA_PLAYBACK_STATE_READ_V2
                | capabilities::MEDIA_PLAYBACK_CONTROL_V2 => {
                    "candidate-private-mpris-bus-fixture-only-live-host-unavailable"
                }
                _ => "unavailable-no-linux-provider",
            };
            let risk = if definition.action.mutates() {
                "mutation"
            } else if uix_available {
                "read-sensitive"
            } else {
                "read"
            };
            let mut entry = json!({
                "id": definition.id,
                "status": status,
                "risk": risk,
                "linuxClassification": classification(definition.id),
                "constraint": if definition.id == capabilities::APPLICATION_DISCOVER_V2 {
                    "linux-xdg-desktop-entry-and-procfs-independent-read-only"
                } else if definition.id == capabilities::APPLICATION_DISCOVER_V3 {
                    "linux-xdg-procfs-read-only-with-toolkit-fixture-launch-status"
                } else if definition.id == capabilities::APPLICATION_OPEN_V2 {
                    "toolkit-owned-self-executable-fixture-fixed-argv-empty-env-no-shell-no-user-apps"
                } else if definition.id == capabilities::APPLICATION_SESSION_DISCOVER_V2 {
                    "linux-xdg-procfs-read-only-no-relationships-no-window-provider"
                } else if definition.id == capabilities::APPLICATION_SESSION_DISCOVER_V3 {
                    "linux-xdg-procfs-opt-in-uix-window-process-relations-no-global-window-claim"
                } else if definition.id == capabilities::APPLICATION_SESSION_DISCOVER_V4 {
                    "linux-launch-aware-xdg-procfs-opt-in-uix-window-process-relations-no-application-inference"
                } else if definition.id == capabilities::WINDOW_LIFECYCLE_V2 {
                    "opt-in-uix-agent-window-logical-resize-and-state-no-move-no-final-state-claim"
                } else if definition.id == capabilities::WINDOW_LIFECYCLE_SEQUENCE {
                    "opt-in-uix-agent-request-scoped-lifecycle-sequence-same-connection-no-transaction-or-final-state-claim"
                } else if definition.id == capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION {
                    "opt-in-uix-agent-complete-lifecycle-sequence-same-authenticated-connection-framework-current-condition-poll-no-causality-no-transaction-no-rollback-no-compositor-final-state-no-arbitrary-wayland-move-no-desktop-input-no-native-identity-no-transport-identity-no-x11-no-fallback"
                } else if definition.id == capabilities::WINDOW_LIFECYCLE_TRANSITION {
                    "opt-in-uix-agent-lifecycle-dispatch-and-framework-state-condition-same-connection-bounded-poll-no-compositor-final-state-claim"
                } else if definition.id == capabilities::WINDOW_STATE_READ {
                    "opt-in-uix-agent-framework-current-logical-state-no-compositor-final-state-claim"
                } else if definition.id == capabilities::WINDOW_STATE_WAIT {
                    "opt-in-uix-agent-framework-current-state-condition-same-connection-bounded-poll-no-compositor-final-state-claim"
                } else if definition.id == capabilities::WINDOW_SCREENSHOT_V2 {
                    "opt-in-uix-agent-application-surface-bounded-png-same-connection-generation-recheck-atomic-output"
                } else if definition.id == capabilities::WINDOW_ACTIVATE {
                    "opt-in-uix-agent-explicit-foreground-activation-request-same-connection-focus-observation-no-focus-guarantee"
                } else if definition.id == capabilities::WINDOW_ACTIVATE_TRANSITION {
                    "opt-in-uix-agent-exact-window-generation-activate-window-same-authenticated-connection-focused-poll-no-wait-protocol-no-persistent-focus-or-causality-no-desktop-input-no-native-identity-no-x11-no-fallback"
                } else if definition.id == capabilities::UI_ELEMENT_LOCATE_V2 {
                    "opt-in-uix-agent-exact-semantic-location-client-logical-geometry-no-host-hit-point"
                } else if definition.id == capabilities::UI_ELEMENT_WAIT_V2 {
                    "opt-in-uix-agent-semantic-revision-wait-exact-selector-unique-or-missing-no-polling"
                } else if definition.id == capabilities::UI_ELEMENT_TRANSITION {
                    "opt-in-uix-agent-snapshot-scoped-element-transition-same-connection-exact-and-unique-or-missing-no-desktop-input-no-foreground-no-native-identity-no-fallback"
                } else if definition.id == capabilities::WINDOW_CLOSE_TRANSITION {
                    "opt-in-uix-agent-exact-window-generation-close-window-same-authenticated-connection-wait-protocol-terminal-reply-required-no-provider-polling-no-foreground-no-desktop-input-no-native-identity-no-x11-no-fallback"
                } else if definition.id == capabilities::UI_INPUT_KEY_V2 {
                    "opt-in-uix-agent-application-internal-press-only-no-desktop-input-no-effect-claim"
                } else if definition.id == capabilities::UI_INPUT_KEY_SEQUENCE {
                    "opt-in-uix-agent-request-scoped-complete-press-sequence-no-independent-key-ownership-no-text"
                } else if definition.id == capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION {
                    "opt-in-uix-agent-complete-press-sequence-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-independent-key-ownership-no-down-up-no-hold-no-repeat-no-text-no-transaction-no-rollback-no-native-identity-no-transport-identity-no-x11-no-fallback"
                } else if definition.id == capabilities::UI_INPUT_KEY_TRANSITION {
                    "opt-in-uix-agent-complete-press-same-authenticated-connection-semantic-revision-wait-exact-and-no-foreground-no-desktop-input-no-independent-key-ownership-no-text-no-native-identity-no-transport-identity-no-x11-no-fallback"
                } else if definition.id == capabilities::UI_INPUT_SEQUENCE {
                    "opt-in-uix-agent-request-scoped-key-pointer-sequence-same-connection-no-transaction-or-rollback"
                } else if definition.id == capabilities::UI_INPUT_SEQUENCE_TRANSITION {
                    "opt-in-uix-agent-complete-key-pointer-sequence-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-desktop-pointer-no-independent-key-or-button-ownership-no-transaction-no-rollback-no-drag-no-double-click-no-click-count-no-scroll-no-native-identity-no-transport-identity-no-x11-no-fallback"
                } else if definition.id == capabilities::UI_INPUT_POINTER_V2 {
                    "opt-in-uix-agent-application-internal-logical-move-click-no-desktop-pointer-no-effect-claim"
                } else if definition.id == capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE {
                    "opt-in-uix-agent-request-scoped-ordinary-left-click-sequence-no-double-click-semantics"
                } else if definition.id == capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION {
                    "opt-in-uix-agent-complete-ordinary-left-click-sequence-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-desktop-pointer-no-independent-button-ownership-no-transaction-no-rollback-no-drag-no-double-click-no-click-count-no-scroll-no-native-identity-no-transport-identity-no-x11-no-fallback"
                } else if definition.id == capabilities::UI_INPUT_POINTER_CLICK_TRANSITION {
                    "opt-in-uix-agent-ordinary-left-click-same-authenticated-connection-semantic-revision-wait-exact-and-no-foreground-no-desktop-input-no-double-click-no-click-count-no-native-identity-no-transport-identity-no-x11-no-fallback"
                } else if definition.id == capabilities::UI_INPUT_POINTER_MOVE_TRANSITION {
                    "opt-in-uix-agent-application-internal-pointer-move-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-desktop-pointer-no-click-no-interpolation-no-drag-no-scroll-no-native-identity-no-transport-identity-no-x11-no-fallback"
                } else if definition.id == capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE {
                    "opt-in-uix-agent-request-scoped-pointer-move-sequence-no-interpolation-or-desktop-pointer"
                } else if definition.id == capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION {
                    "opt-in-uix-agent-complete-pointer-move-sequence-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-desktop-pointer-no-click-no-key-press-no-interpolation-no-transaction-no-rollback-no-drag-no-scroll-no-native-identity-no-transport-identity-no-x11-no-fallback"
                } else if definition.id == capabilities::UI_INPUT_POINTER_SEQUENCE {
                    "opt-in-uix-agent-request-scoped-hover-and-ordinary-left-click-sequence-no-desktop-pointer"
                } else if definition.id == capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION {
                    "opt-in-uix-agent-complete-hover-click-sequence-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-desktop-pointer-no-independent-button-ownership-no-drag-no-double-click-no-click-count-no-scroll-no-native-identity-no-transport-identity-no-x11-no-fallback"
                } else if definition.id == capabilities::UI_INPUT_POINTER_DRAG {
                    "opt-in-uix-agent-request-scoped-left-drag-balanced-release-no-independent-button-ownership"
                } else if definition.id == capabilities::UI_INPUT_POINTER_DRAG_TRANSITION {
                    "opt-in-uix-agent-request-scoped-left-drag-confirmed-release-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-desktop-pointer-no-independent-button-ownership-no-native-identity-no-transport-identity-no-x11-no-fallback"
                } else if definition.id == capabilities::DESKTOP_INTERACTION {
                    "portal-broker-bounded-ascii-keys-observation-pixel-clicks-no-global-focus-binding"
                } else if definition.id == capabilities::UI_INPUT_KEY_V3 {
                    "wayland-portal-session-bound-eis-named-key-input-no-text-no-effect-claim-no-fallback"
                } else if definition.id == capabilities::UI_INPUT_POINTER_V3 {
                    "wayland-portal-session-bound-eis-relative-logical-pointer-no-final-position-or-effect-claim-no-fallback"
                } else if definition.id == capabilities::WINDOW_CLOSE_V2 {
                    "opt-in-uix-agent-close-request-only-final-state-via-window-closed-wait-v2"
                } else if definition.id == capabilities::PROCESS_TERMINATE_GRACEFUL_V2 {
                    "procfs-owner-generation-bound-same-non-root-uid-exact-process-generation-pidfd-sigterm-no-force-fallback"
                } else if definition.id == capabilities::PROCESS_TERMINATE_FORCE_V2 {
                    "explicit-critical-procfs-owner-generation-bound-same-non-root-uid-pidfd-force-no-graceful-fallback"
                } else if matches!(
                    definition.id,
                    capabilities::DESKTOP_SESSION_OPEN
                        | capabilities::DESKTOP_SESSION_CLOSE
                        | capabilities::SCREEN_CAPTURE
                | capabilities::DESKTOP_OBSERVE
                ) {
                    "wayland-portal-live-session-single-thread-jsonl-owner-no-restore-token-no-fallback"
                } else if uix_available {
                    "opt-in-uix-agent-applications-only-no-global-window-claim-no-fallback"
                } else if mpris_available {
                    "current-user-mpris-window-independent-fixed-worker-playback-state-only-no-autoactivation-no-fallback"
                } else if procfs_available {
                    "linux-procfs-read-only-no-native-id-or-path"
                } else if available {
                    "wayland-portal-interactive-host-target-atomic-png"
                } else if matches!(definition.id,
                    capabilities::WINDOW_DISCOVER_V2
                    | capabilities::WINDOW_METADATA_READ_V2
                    | capabilities::ACCESSIBILITY_TREE_READ_V2
                ) {
                    "partial-accessibility-exporters-read-only-private-fixture-no-live-route"
                } else if matches!(definition.id,
                    capabilities::MEDIA_SESSION_DISCOVER_V2
                    | capabilities::MEDIA_PLAYBACK_STATE_READ_V2
                    | capabilities::MEDIA_PLAYBACK_CONTROL_V2
                ) {
                    "mpris-v2-private-explicit-bus-no-production-route-no-autoactivation"
                } else {
                    "no-certified-linux-provider-no-fallback"
                },
            });
            if available {
                entry["executionDomain"] = json!(if matches!(definition.id, capabilities::APPLICATION_SESSION_DISCOVER_V3 | capabilities::APPLICATION_SESSION_DISCOVER_V4) {
                    "same-session-no-focus"
                } else if definition.id == capabilities::APPLICATION_OPEN_V2 {
                    "host-foreground"
                } else if matches!(
                    definition.id,
                    capabilities::PROCESS_TERMINATE_GRACEFUL_V2
                        | capabilities::PROCESS_TERMINATE_FORCE_V2
                ) {
                    "host-background"
                } else if procfs_available {
                    "host-headless"
                } else if mpris_available {
                    "isolated-worker"
                } else if definition.id == capabilities::DESKTOP_SESSION_CLOSE {
                    "host-background"
                } else if matches!(
                    definition.id,
                    capabilities::WINDOW_LIFECYCLE_V2
                        | capabilities::WINDOW_LIFECYCLE_SEQUENCE
                        | capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION
                        | capabilities::WINDOW_LIFECYCLE_TRANSITION
                        | capabilities::WINDOW_ACTIVATE
                        | capabilities::WINDOW_ACTIVATE_TRANSITION
                ) {
                    "host-foreground"
                } else if uix_available {
                    "same-session-no-focus"
                } else {
                    "host-foreground"
                });
            }
            entry
        })
        .collect::<Vec<_>>();

    json!({
        "ok": true,
        "contractVersion": "act/control/v1",
        "implementation": "rust",
        "data": {
            "surface": "app",
            "productPromise": "broad-general-control-with-capability-degradation",
            "supportLevel": "L2-confirmed-background",
            "platformPolicy": {
                "desktopProtocol": "wayland-only",
                "x11Supported": false,
                "xwaylandFallback": false,
                "fallback": "none",
                "desktopStatus": if portal_screenshot_ready {
                    "portal-interactive-screenshot-ready"
                } else {
                    "no-certified-desktop-provider"
                },
            },
            "capabilities": entries,
        },
    })
}

fn classification(capability: &str) -> &'static str {
    match capability {
        capabilities::APPLICATION_DISCOVER_V2 => "partial",
        capabilities::APPLICATION_DISCOVER_V3 | capabilities::APPLICATION_OPEN_V2 => "verified",
        capabilities::APPLICATION_SESSION_DISCOVER_V2 => "verified",
        capabilities::APPLICATION_SESSION_DISCOVER_V3 => "verified",
        capabilities::APPLICATION_SESSION_DISCOVER_V4 => "verified",
        capabilities::PROCESS_DISCOVER
        | capabilities::PROCESS_METADATA_READ
        | capabilities::PROCESS_TERMINATE_GRACEFUL_V2
        | capabilities::PROCESS_TERMINATE_FORCE_V2 => "verified",
        capabilities::DESKTOP_SCREENSHOT_INTERACTIVE
        | capabilities::DESKTOP_SESSION_OPEN
        | capabilities::DESKTOP_SESSION_CLOSE
        | capabilities::SCREEN_CAPTURE
        | capabilities::DESKTOP_OBSERVE
        | capabilities::UI_INPUT_KEY_V3
        | capabilities::UI_INPUT_POINTER_V3
        | capabilities::DESKTOP_INTERACTION => "live-acceptance-pending",
        capabilities::WINDOW_DISCOVER_V3
        | capabilities::WINDOW_METADATA_READ_V3
        | capabilities::ACCESSIBILITY_TREE_READ_V3
        | capabilities::UI_ELEMENT_LOCATE_V2
        | capabilities::UI_ELEMENT_WAIT_V2
        | capabilities::UI_ELEMENT_ACTION_V2
        | capabilities::UI_ELEMENT_TRANSITION
        | capabilities::UI_INPUT_KEY_V2
        | capabilities::UI_INPUT_KEY_SEQUENCE
        | capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION
        | capabilities::UI_INPUT_KEY_TRANSITION
        | capabilities::UI_INPUT_SEQUENCE
        | capabilities::UI_INPUT_SEQUENCE_TRANSITION
        | capabilities::UI_INPUT_POINTER_V2
        | capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE
        | capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION
        | capabilities::UI_INPUT_POINTER_CLICK_TRANSITION
        | capabilities::UI_INPUT_POINTER_MOVE_TRANSITION
        | capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE
        | capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION
        | capabilities::UI_INPUT_POINTER_SEQUENCE
        | capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION
        | capabilities::UI_INPUT_POINTER_DRAG
        | capabilities::UI_INPUT_POINTER_DRAG_TRANSITION
        | capabilities::WINDOW_REVISION_WAIT
        | capabilities::WINDOW_CLOSED_WAIT_V2
        | capabilities::WINDOW_CLOSE_V2
        | capabilities::WINDOW_CLOSE_TRANSITION
        | capabilities::WINDOW_LIFECYCLE_V2
        | capabilities::WINDOW_LIFECYCLE_SEQUENCE
        | capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION
        | capabilities::WINDOW_LIFECYCLE_TRANSITION
        | capabilities::WINDOW_STATE_READ
        | capabilities::WINDOW_STATE_WAIT
        | capabilities::WINDOW_SCREENSHOT_V2
        | capabilities::WINDOW_ACTIVATE
        | capabilities::WINDOW_ACTIVATE_TRANSITION => "verified",
        capabilities::WINDOW_DISCOVER_V2
        | capabilities::WINDOW_METADATA_READ_V2
        | capabilities::ACCESSIBILITY_TREE_READ_V2 => "candidate",
        capabilities::MEDIA_SESSION_DISCOVER_V2
        | capabilities::MEDIA_PLAYBACK_STATE_READ_V2
        | capabilities::MEDIA_PLAYBACK_CONTROL_V2 => "candidate",
        capabilities::MEDIA_SESSION_DISCOVER_V3
        | capabilities::MEDIA_PLAYBACK_STATE_READ_V3
        | capabilities::MEDIA_PLAYBACK_CONTROL_V3 => "production-route-target-assessment-required",
        _ => "conditional-or-permanent-unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::surface_for;
    use crate::adapters::linux::wayland_portal::WaylandPortalFacts;

    #[test]
    fn portal_ready_fixture_validates_host_foreground_capability() {
        let instance = surface_for(&WaylandPortalFacts {
            wayland_socket_count: 1,
            user_bus_socket_present: true,
            user_bus_reachable: true,
            portal_service_registered: true,
            screen_cast_version: Some(5),
            remote_desktop_version: Some(2),
            screenshot_version: Some(2),
        });
        let Ok(schema) =
            serde_json::from_str(include_str!("../../contracts/v1/capabilities.schema.json"))
        else {
            panic!("schema must parse");
        };
        if let Err(error) = jsonschema::draft202012::validate(&schema, &instance) {
            panic!("portal-ready capability instance must validate: {error}");
        }
        let Some(screenshot) = instance["data"]["capabilities"]
            .as_array()
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry["id"] == "desktop.screenshot.interactive@1")
            })
        else {
            panic!("screenshot capability must exist");
        };
        assert_eq!(screenshot["executionDomain"], "host-foreground");
        let Some(keyboard) = instance["data"]["capabilities"]
            .as_array()
            .and_then(|entries| entries.iter().find(|entry| entry["id"] == "ui.input.key@3"))
        else {
            panic!("Portal keyboard capability must exist");
        };
        assert_eq!(
            keyboard["status"],
            "production-route-live-acceptance-pending-not-advertised"
        );
        assert_eq!(keyboard["linuxClassification"], "live-acceptance-pending");
        assert!(keyboard.get("executionDomain").is_none());
        let Some(pointer) = instance["data"]["capabilities"]
            .as_array()
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry["id"] == "ui.input.pointer@3")
            })
        else {
            panic!("Portal pointer capability must exist");
        };
        assert_eq!(
            pointer["status"],
            "production-route-live-acceptance-pending-not-advertised"
        );
        assert_eq!(pointer["linuxClassification"], "live-acceptance-pending");
        assert!(pointer.get("executionDomain").is_none());
        let Some(capture) = instance["data"]["capabilities"]
            .as_array()
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry["id"] == "screen.capture@1")
            })
        else {
            panic!("Portal single-frame capability must exist");
        };
        assert_eq!(
            capture["status"],
            "production-route-live-acceptance-pending"
        );
        assert_eq!(capture["linuxClassification"], "live-acceptance-pending");
        assert!(capture.get("executionDomain").is_none());
    }

    #[test]
    fn every_registered_capability_has_exactly_one_linux_classification() {
        let instance = surface_for(&WaylandPortalFacts {
            wayland_socket_count: 0,
            user_bus_socket_present: false,
            user_bus_reachable: false,
            portal_service_registered: false,
            screen_cast_version: None,
            remote_desktop_version: None,
            screenshot_version: None,
        });
        let Some(entries) = instance["data"]["capabilities"].as_array() else {
            panic!("Linux capability entries must be an array");
        };
        assert_eq!(entries.len(), crate::capabilities::ALL.len());
        let mut identifiers = std::collections::BTreeSet::new();
        for entry in entries {
            let Some(identifier) = entry["id"].as_str() else {
                panic!("Linux capability ID must be a string");
            };
            assert!(identifiers.insert(identifier));
            assert!(matches!(
                entry["linuxClassification"].as_str(),
                Some(
                    "partial"
                        | "verified"
                        | "production-route-target-assessment-required"
                        | "live-acceptance-pending"
                        | "candidate"
                        | "conditional-or-permanent-unavailable"
                )
            ));
        }
        let Some(application) = entries
            .iter()
            .find(|entry| entry["id"] == "application.discover@2")
        else {
            panic!("application discover v2 must be classified");
        };
        assert_eq!(
            application["status"],
            "available-partial-linux-xdg-desktop-entry-and-procfs"
        );
        assert_eq!(application["linuxClassification"], "partial");
        let Some(application_v3) = entries
            .iter()
            .find(|entry| entry["id"] == "application.discover@3")
        else {
            panic!("application discover v3 must be classified");
        };
        assert_eq!(
            application_v3["status"],
            "available-verified-linux-xdg-procfs-with-toolkit-fixture-launch-status"
        );
        assert_eq!(application_v3["linuxClassification"], "verified");
        let Some(application_open) = entries
            .iter()
            .find(|entry| entry["id"] == "application.open@2")
        else {
            panic!("application open v2 must be classified");
        };
        assert_eq!(
            application_open["status"],
            "available-route-toolkit-self-executable-fixture-only"
        );
        assert_eq!(application_open["linuxClassification"], "verified");
        assert_eq!(application_open["executionDomain"], "host-foreground");
    }
}
