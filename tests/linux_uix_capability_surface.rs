#![cfg(target_os = "linux")]

//! UIX 协作式 capability surface 的逐项风险、状态、执行域与限制回归。

use ai_computer_toolkit::cli;

#[test]
fn capability_surface_publishes_thirty_seven_cooperative_uix_routes()
-> Result<(), Box<dyn std::error::Error>> {
    let surface = cli::run(vec!["capabilities".to_owned()])?.json;
    let entries = surface["data"]["capabilities"]
        .as_array()
        .ok_or("capability entries missing")?;
    for id in [
        "window.discover@3",
        "window.metadata.read@3",
        "accessibility.tree.read@3",
        "ui.element.locate@2",
        "ui.element.wait@2",
        "ui.element.action@2",
        "ui.element.transition@1",
        "ui.input.key@2",
        "ui.input.key.sequence@1",
        "ui.input.key.sequence.transition@1",
        "ui.input.key.transition@1",
        "ui.input.sequence@1",
        "ui.input.sequence.transition@1",
        "ui.input.pointer@2",
        "ui.input.pointer.click.sequence@1",
        "ui.input.pointer.click.sequence.transition@1",
        "ui.input.pointer.click.transition@1",
        "ui.input.pointer.move.transition@1",
        "ui.input.pointer.move.sequence.transition@1",
        "ui.input.pointer.drag@1",
        "ui.input.pointer.drag.transition@1",
        "ui.input.pointer.move.sequence@1",
        "ui.input.pointer.sequence@1",
        "ui.input.pointer.sequence.transition@1",
        "window.close@2",
        "window.close.transition@1",
        "window.closed.wait@2",
        "window.revision.wait@1",
        "window.lifecycle@2",
        "window.lifecycle.sequence@1",
        "window.lifecycle.sequence.transition@1",
        "window.lifecycle.transition@1",
        "window.state.read@1",
        "window.screenshot@2",
        "window.activate@1",
        "window.activate.transition@1",
        "window.state.wait@1",
    ] {
        let entry = entries
            .iter()
            .find(|entry| entry["id"] == id)
            .ok_or("UIX capability missing")?;
        assert_eq!(entry["linuxClassification"], "verified");
        assert_eq!(
            entry["risk"],
            if matches!(
                id,
                "ui.element.action@2"
                    | "ui.element.transition@1"
                    | "ui.input.key@2"
                    | "ui.input.key.sequence@1"
                    | "ui.input.key.sequence.transition@1"
                    | "ui.input.key.transition@1"
                    | "ui.input.sequence@1"
                    | "ui.input.sequence.transition@1"
                    | "ui.input.pointer@2"
                    | "ui.input.pointer.click.sequence@1"
                    | "ui.input.pointer.click.sequence.transition@1"
                    | "ui.input.pointer.click.transition@1"
                    | "ui.input.pointer.move.transition@1"
                    | "ui.input.pointer.move.sequence.transition@1"
                    | "ui.input.pointer.drag@1"
                    | "ui.input.pointer.drag.transition@1"
                    | "ui.input.pointer.move.sequence@1"
                    | "ui.input.pointer.sequence@1"
                    | "ui.input.pointer.sequence.transition@1"
                    | "window.close@2"
                    | "window.close.transition@1"
                    | "window.lifecycle@2"
                    | "window.lifecycle.sequence@1"
                    | "window.lifecycle.sequence.transition@1"
                    | "window.lifecycle.transition@1"
                    | "window.screenshot@2"
                    | "window.activate@1"
                    | "window.activate.transition@1"
            ) {
                "mutation"
            } else {
                "read-sensitive"
            }
        );
        assert_eq!(
            entry["status"],
            if id == "window.screenshot@2" {
                "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-visual-acceptance-deferred"
            } else if id == "window.activate@1" {
                "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-foreground-acceptance-deferred"
            } else if matches!(
                id,
                "window.state.wait@1" | "ui.element.wait@2" | "window.lifecycle.transition@1"
            ) {
                "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-observation-acceptance-deferred"
            } else if matches!(
                id,
                "ui.element.transition@1"
                    | "ui.input.key.sequence.transition@1"
                    | "ui.input.key.transition@1"
                    | "ui.input.sequence.transition@1"
                    | "ui.input.pointer.click.sequence.transition@1"
                    | "ui.input.pointer.click.transition@1"
                    | "ui.input.pointer.move.transition@1"
                    | "ui.input.pointer.move.sequence.transition@1"
                    | "ui.input.pointer.sequence.transition@1"
                    | "ui.input.pointer.drag.transition@1"
                    | "window.lifecycle.sequence.transition@1"
                    | "window.close.transition@1"
                    | "window.activate.transition@1"
            ) {
                "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-and-observation-acceptance-deferred"
            } else if matches!(
                id,
                "ui.input.pointer.drag@1"
                    | "ui.input.key.sequence@1"
                    | "ui.input.sequence@1"
                    | "ui.input.pointer.click.sequence@1"
                    | "ui.input.pointer.move.sequence@1"
                    | "ui.input.pointer.sequence@1"
                    | "window.lifecycle.sequence@1"
            ) {
                "available-protocol-fixture-verified-cooperative-uix-agent-v1-owner-interaction-acceptance-deferred"
            } else {
                "available-verified-cooperative-uix-agent-v1-wayland"
            }
        );
        assert_eq!(
            entry["executionDomain"],
            if matches!(
                id,
                "window.lifecycle@2"
                    | "window.lifecycle.sequence@1"
                    | "window.lifecycle.sequence.transition@1"
                    | "window.lifecycle.transition@1"
                    | "window.activate@1"
                    | "window.activate.transition@1"
            ) {
                "host-foreground"
            } else {
                "same-session-no-focus"
            }
        );
        assert_eq!(
            entry["constraint"],
            if id == "window.lifecycle@2" {
                "opt-in-uix-agent-window-logical-resize-and-state-no-move-no-final-state-claim"
            } else if id == "ui.input.key@2" {
                "opt-in-uix-agent-application-internal-press-only-no-desktop-input-no-effect-claim"
            } else if id == "ui.input.key.sequence@1" {
                "opt-in-uix-agent-request-scoped-complete-press-sequence-no-independent-key-ownership-no-text"
            } else if id == "ui.input.key.sequence.transition@1" {
                "opt-in-uix-agent-complete-press-sequence-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-independent-key-ownership-no-down-up-no-hold-no-repeat-no-text-no-transaction-no-rollback-no-native-identity-no-transport-identity-no-x11-no-fallback"
            } else if id == "ui.input.key.transition@1" {
                "opt-in-uix-agent-complete-press-same-authenticated-connection-semantic-revision-wait-exact-and-no-foreground-no-desktop-input-no-independent-key-ownership-no-text-no-native-identity-no-transport-identity-no-x11-no-fallback"
            } else if id == "ui.input.sequence@1" {
                "opt-in-uix-agent-request-scoped-key-pointer-sequence-same-connection-no-transaction-or-rollback"
            } else if id == "ui.input.sequence.transition@1" {
                "opt-in-uix-agent-complete-key-pointer-sequence-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-desktop-pointer-no-independent-key-or-button-ownership-no-transaction-no-rollback-no-drag-no-double-click-no-click-count-no-scroll-no-native-identity-no-transport-identity-no-x11-no-fallback"
            } else if id == "ui.input.pointer@2" {
                "opt-in-uix-agent-application-internal-logical-move-click-no-desktop-pointer-no-effect-claim"
            } else if id == "ui.input.pointer.click.sequence@1" {
                "opt-in-uix-agent-request-scoped-ordinary-left-click-sequence-no-double-click-semantics"
            } else if id == "ui.input.pointer.click.sequence.transition@1" {
                "opt-in-uix-agent-complete-ordinary-left-click-sequence-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-desktop-pointer-no-independent-button-ownership-no-transaction-no-rollback-no-drag-no-double-click-no-click-count-no-scroll-no-native-identity-no-transport-identity-no-x11-no-fallback"
            } else if id == "ui.input.pointer.click.transition@1" {
                "opt-in-uix-agent-ordinary-left-click-same-authenticated-connection-semantic-revision-wait-exact-and-no-foreground-no-desktop-input-no-double-click-no-click-count-no-native-identity-no-transport-identity-no-x11-no-fallback"
            } else if id == "ui.input.pointer.move.transition@1" {
                "opt-in-uix-agent-application-internal-pointer-move-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-desktop-pointer-no-click-no-interpolation-no-drag-no-scroll-no-native-identity-no-transport-identity-no-x11-no-fallback"
            } else if id == "ui.input.pointer.move.sequence@1" {
                "opt-in-uix-agent-request-scoped-pointer-move-sequence-no-interpolation-or-desktop-pointer"
            } else if id == "ui.input.pointer.move.sequence.transition@1" {
                "opt-in-uix-agent-complete-pointer-move-sequence-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-desktop-pointer-no-click-no-key-press-no-interpolation-no-transaction-no-rollback-no-drag-no-scroll-no-native-identity-no-transport-identity-no-x11-no-fallback"
            } else if id == "ui.input.pointer.sequence@1" {
                "opt-in-uix-agent-request-scoped-hover-and-ordinary-left-click-sequence-no-desktop-pointer"
            } else if id == "ui.input.pointer.sequence.transition@1" {
                "opt-in-uix-agent-complete-hover-click-sequence-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-desktop-pointer-no-independent-button-ownership-no-drag-no-double-click-no-click-count-no-scroll-no-native-identity-no-transport-identity-no-x11-no-fallback"
            } else if id == "ui.input.pointer.drag@1" {
                "opt-in-uix-agent-request-scoped-left-drag-balanced-release-no-independent-button-ownership"
            } else if id == "ui.input.pointer.drag.transition@1" {
                "opt-in-uix-agent-request-scoped-left-drag-confirmed-release-same-authenticated-connection-semantic-revision-wait-exact-and-no-causal-or-consumption-claim-no-foreground-no-desktop-input-no-desktop-pointer-no-independent-button-ownership-no-native-identity-no-transport-identity-no-x11-no-fallback"
            } else if id == "window.close@2" {
                "opt-in-uix-agent-close-request-only-final-state-via-window-closed-wait-v2"
            } else if id == "window.close.transition@1" {
                "opt-in-uix-agent-exact-window-generation-close-window-same-authenticated-connection-wait-protocol-terminal-reply-required-no-provider-polling-no-foreground-no-desktop-input-no-native-identity-no-x11-no-fallback"
            } else if id == "window.lifecycle.sequence@1" {
                "opt-in-uix-agent-request-scoped-lifecycle-sequence-same-connection-no-transaction-or-final-state-claim"
            } else if id == "window.lifecycle.sequence.transition@1" {
                "opt-in-uix-agent-complete-lifecycle-sequence-same-authenticated-connection-framework-current-condition-poll-no-causality-no-transaction-no-rollback-no-compositor-final-state-no-arbitrary-wayland-move-no-desktop-input-no-native-identity-no-transport-identity-no-x11-no-fallback"
            } else if id == "window.lifecycle.transition@1" {
                "opt-in-uix-agent-lifecycle-dispatch-and-framework-state-condition-same-connection-bounded-poll-no-compositor-final-state-claim"
            } else if id == "window.state.read@1" {
                "opt-in-uix-agent-framework-current-logical-state-no-compositor-final-state-claim"
            } else if id == "window.state.wait@1" {
                "opt-in-uix-agent-framework-current-state-condition-same-connection-bounded-poll-no-compositor-final-state-claim"
            } else if id == "window.screenshot@2" {
                "opt-in-uix-agent-application-surface-bounded-png-same-connection-generation-recheck-atomic-output"
            } else if id == "window.activate@1" {
                "opt-in-uix-agent-explicit-foreground-activation-request-same-connection-focus-observation-no-focus-guarantee"
            } else if id == "window.activate.transition@1" {
                "opt-in-uix-agent-exact-window-generation-activate-window-same-authenticated-connection-focused-poll-no-wait-protocol-no-persistent-focus-or-causality-no-desktop-input-no-native-identity-no-x11-no-fallback"
            } else if id == "ui.element.locate@2" {
                "opt-in-uix-agent-exact-semantic-location-client-logical-geometry-no-host-hit-point"
            } else if id == "ui.element.wait@2" {
                "opt-in-uix-agent-semantic-revision-wait-exact-selector-unique-or-missing-no-polling"
            } else if id == "ui.element.transition@1" {
                "opt-in-uix-agent-snapshot-scoped-element-transition-same-connection-exact-and-unique-or-missing-no-desktop-input-no-foreground-no-native-identity-no-fallback"
            } else {
                "opt-in-uix-agent-applications-only-no-global-window-claim-no-fallback"
            }
        );
    }
    Ok(())
}
