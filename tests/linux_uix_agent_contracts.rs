#![cfg(target_os = "linux")]
#![recursion_limit = "256"]

//! UIX Agent v3 生产路由、公开契约与隐私边界回归。

use ai_computer_toolkit::cli;
use serde_json::{Value, json};

fn schema(source: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(source)?)
}

fn assert_valid(schema: &Value, instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    jsonschema::draft202012::validate(schema, instance)
        .map_err(|error| format!("JSON Schema validation failed: {error}").into())
}

#[test]
fn status_is_side_effect_free_and_does_not_claim_global_desktop_control()
-> Result<(), Box<dyn std::error::Error>> {
    let window = cli::run(vec!["status".to_owned(), "window".to_owned()])?.json;
    assert_eq!(window["provider"], "uix-agent-v1");
    assert_eq!(window["connected"], false);
    assert_eq!(window["metadataRead"], false);
    assert_eq!(window["globalWindowDirectory"], false);
    assert_eq!(window["arbitraryThirdPartyApplications"], false);

    let accessibility = cli::run(vec!["status".to_owned(), "accessibility".to_owned()])?.json;
    assert_eq!(accessibility["connected"], false);
    assert_eq!(accessibility["boundsExposed"], false);
    assert_eq!(accessibility["sensitiveSnapshotRead"], false);
    Ok(())
}

#[test]
fn v3_schemas_accept_minimal_opaque_instances() -> Result<(), Box<dyn std::error::Error>> {
    let safety = json!({
        "foregroundClaimed": false,
        "globalWindowDirectoryClaimed": false,
        "arbitraryThirdPartyAppsClaimed": false,
        "nativeIdentityExposed": false,
        "transportIdentityExposed": false,
        "x11Used": false,
        "fallback": "none"
    });
    let discovery = json!({
        "ok": true,
        "contractVersion": "act/window-observation/v3",
        "capability": "window.discover@3",
        "coverage": "opt-in-uix-agent-applications",
        "executionDomain": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "count": 1,
        "total": 1,
        "truncated": false,
        "complete": true,
        "warnings": [],
        "windows": [{
            "sessionId": "s2:w:0000000000000000",
            "targetKind": "uix-agent-window",
            "title": "fixture",
            "visible": true,
            "presentable": true,
            "focused": false,
            "revision": 1,
            "presentedRevision": 1,
            "targetIdentity": {
                "contractVersion": "act/window-target-identity/v3",
                "provider": "uix-agent-v1",
                "coverage": "opt-in-uix-agent-applications",
                "freshness": "agent-process-token-and-window-generation",
                "mutationAllowed": false,
                "generationOwner": "uix-application",
                "nativeIdentityExposed": false
            },
            "capabilities": [
                "window.metadata.read@3",
                "accessibility.tree.read@3",
                "ui.element.locate@2",
                "ui.element.wait@2",
                "ui.element.action@2",
                "ui.element.transition@1",
                "ui.input.key@2",
                "ui.input.key.sequence@1",
                "ui.input.key.transition@1",
                "ui.input.sequence.transition@1",
                "ui.input.pointer@2",
                "ui.input.pointer.click.sequence@1",
                "ui.input.pointer.click.transition@1",
                "ui.input.pointer.drag.transition@1",
                "ui.input.pointer.move.transition@1",
                "ui.input.pointer.move.sequence@1",
                "ui.input.pointer.sequence@1",
                "ui.input.pointer.sequence.transition@1",
                "window.close@2",
                "window.close.transition@1",
                "window.closed.wait@2",
                "window.revision.wait@1",
                "window.lifecycle@2",
                "window.lifecycle.sequence@1",
                "window.lifecycle.transition@1",
                "window.state.read@1",
                "window.state.wait@1",
                "window.activate.transition@1"
            ]
        }],
        "safety": safety
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v3/window-observation.schema.json"
        ))?,
        &discovery,
    )?;

    let tree = json!({
        "ok": true,
        "contractVersion": "act/accessibility-tree/v3",
        "capability": "accessibility.tree.read@3",
        "coverage": "opt-in-uix-agent-applications",
        "executionDomain": "same-session-no-focus",
        "readOnly": true,
        "mutationAllowed": false,
        "sessionId": "s2:w:0000000000000000",
        "snapshotId": "as3:0000000000000000",
        "revision": 0,
        "presentedRevision": 0,
        "maximumDepth": 4,
        "maximumItems": 50,
        "visited": 0,
        "truncated": false,
        "truncationReasons": [],
        "nodes": [],
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
            "mutationInterfacesCalled": false
        }
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v3/accessibility-tree.schema.json"
        ))?,
        &tree,
    )?;
    Ok(())
}

#[test]
fn element_location_v2_schemas_cover_input_boundaries_and_output_states()
-> Result<(), Box<dyn std::error::Error>> {
    let input_schema = schema(include_str!(
        "../contracts/v2/ui-element-locate-input.schema.json"
    ))?;
    let input = json!({
        "selector": {
            "automationId": "save",
            "role": "button",
            "enabled": true,
            "action": "invoke"
        }
    });
    assert_valid(&input_schema, &input)?;

    // 输入契约必须拒绝 selector 显式 null、空对象和未公开字段。
    for invalid_input in [
        json!({ "selector": { "automationId": null } }),
        json!({ "selector": {} }),
        json!({ "selector": { "xpath": "//*" } }),
    ] {
        assert!(
            assert_valid(&input_schema, &invalid_input).is_err(),
            "无效 selector 不应通过 input schema: {invalid_input}"
        );
    }

    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "read",
        "capability": "ui.element.locate@2",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "ok": true,
            "contractVersion": "act/ui-element-location/v2",
            "capability": "ui.element.locate@2",
            "targetId": "s2:w:0000000000000000",
            "snapshotId": "as3:0000000000000000",
            "revision": 7,
            "presentedRevision": 6,
            "selectorSemantics": "exact-and",
            "matchState": "unique",
            "matchCount": 1,
            "visited": 43,
            "complete": true,
            "element": {
                "elementId": "s2:e:0000000000000000",
                "identityFreshness": "snapshot-revision",
                "automationId": "save",
                "role": "button",
                "name": "保存",
                "focused": false,
                "enabled": true,
                "actions": ["focus", "invoke"],
                "geometry": {
                    "coordinateSpace": "application-client",
                    "unit": "logical-px",
                    "frame": { "x": 10.0, "y": 20.0, "width": 80.0, "height": 30.0 },
                    "visibleBounds": { "x": 12.0, "y": 20.0, "width": 78.0, "height": 30.0 }
                }
            },
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
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": { "activationRequested": false },
            "targeting": "opaque exact window generation"
        }
    });
    let output_schema = schema(include_str!(
        "../contracts/v2/ui-element-location.schema.json"
    ))?;
    assert_valid(&output_schema, &result)?;

    // 缺失结果仍须是完整快照结果，并显式保持零匹配与 null 元素。
    let mut missing_result = result;
    missing_result["data"]["matchState"] = json!("missing");
    missing_result["data"]["matchCount"] = json!(0);
    missing_result["data"]["element"] = Value::Null;
    assert_valid(&output_schema, &missing_result)?;
    Ok(())
}

#[test]
fn action_v2_schemas_accept_exact_snapshot_element_result() -> Result<(), Box<dyn std::error::Error>>
{
    let input = json!({
        "snapshotId": "as3:0000000000000000",
        "elementId": "s2:e:0000000000000000",
        "action": { "type": "invoke" }
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v2/ui-element-action-input.schema.json"
        ))?,
        &input,
    )?;
    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.element.action@2",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "action": "invoke",
            "outcome": "completed",
            "dispatchState": "completed",
            "acceptedMayHaveOccurred": true,
            "automaticRetryProhibited": true,
            "retrySafe": false,
            "windowReResolved": true,
            "snapshotReResolved": true,
            "elementReResolved": true,
            "snapshotId": "as3:0000000000000000",
            "elementId": "s2:e:0000000000000000",
            "revision": 2,
            "presentedRevision": 2,
            "settled": true,
            "applicationConfirmation": "not-required",
            "confirmationEvaluatedBeforeDiscovery": true,
            "pointerFallbackUsed": false,
            "writePerformed": true,
            "executionRealm": "same-session-no-focus",
            "readOnly": false,
            "timeoutMs": 30000,
            "safety": {
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "confirmIdentityExposed": false,
                "sensitiveSnapshotPublished": false,
                "hostForegroundActivationRequested": false,
                "desktopInputInjected": false,
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": { "activationRequested": false },
            "targeting": "opaque exact snapshot element"
        }
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v2/ui-element-action.schema.json"
        ))?,
        &result,
    )?;
    Ok(())
}

#[test]
fn key_v2_schemas_accept_confirmed_application_internal_press()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "key": "page-down",
        "modifiers": ["control", "shift"],
        "timeoutMs": 5000
    });
    assert_valid(
        &schema(include_str!("../contracts/v2/key-input.schema.json"))?,
        &input,
    )?;
    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.key@2",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "capability": "ui.input.key@2",
            "targetId": "s2:w:0000000000000000",
            "key": "page-down",
            "modifiers": ["control", "shift"],
            "phase": "press",
            "outcome": "completed",
            "dispatchState": "completed",
            "accepted": true,
            "keyDownHandled": true,
            "keyUpHandled": true,
            "effectConfirmed": false,
            "finalStateReached": false,
            "effectObservation": "not-exposed-by-uix-agent-v1",
            "revision": 5,
            "presentedRevision": 5,
            "settled": true,
            "windowReResolved": true,
            "applicationPolicyEvaluated": true,
            "confirmationEvaluatedBeforeDiscovery": true,
            "foregroundConsentRequired": false,
            "hostForegroundActivationRequested": false,
            "automaticRetryProhibited": true,
            "retrySafe": false,
            "timeoutMs": 5000,
            "executionRealm": "same-session-no-focus",
            "safety": {
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "desktopInputInjected": false,
                "applicationInternalEventsDispatched": true,
                "requestScopedBalancedPress": true,
                "textInputSupported": false,
                "keyHoldSupported": false,
                "x11Used": false,
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": { "activationRequested": false },
            "targeting": "opaque exact window generation"
        }
    });
    assert_valid(
        &schema(include_str!("../contracts/v2/key-input-result.schema.json"))?,
        &result,
    )?;
    Ok(())
}

#[test]
fn key_sequence_v1_schemas_accept_confirmed_balanced_press_sequence()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "presses": [
            { "key": "a", "modifiers": ["control"] },
            { "key": "enter" }
        ],
        "intervalMs": 20,
        "timeoutMs": 5000
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/uix-key-sequence-input.schema.json"
        ))?,
        &input,
    )?;
    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.key.sequence@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "capability": "ui.input.key.sequence@1",
            "targetId": "s2:w:0000000000000000",
            "action": "key-sequence",
            "presses": [
                { "key": "a", "modifiers": ["control"] },
                { "key": "enter", "modifiers": [] }
            ],
            "pressesRequested": 2,
            "pressesAccepted": 2,
            "intervalMs": 20,
            "plannedDurationMs": 20,
            "outcome": "completed",
            "dispatchState": "completed",
            "accepted": true,
            "allPressesBalanced": true,
            "revision": 8,
            "presentedRevision": 8,
            "settled": true,
            "effectConfirmed": false,
            "finalStateReached": false,
            "windowReResolved": true,
            "applicationPolicyEvaluated": true,
            "confirmationEvaluatedBeforeDiscovery": true,
            "foregroundConsentRequired": false,
            "hostForegroundActivationRequested": false,
            "automaticRetryProhibited": true,
            "retrySafe": false,
            "timeoutMs": 5000,
            "executionRealm": "same-session-no-focus",
            "safety": {
                "provider": "uix-agent-v1",
                "applicationInternalEventsDispatched": true,
                "desktopInputInjected": false,
                "requestScopedBalancedPresses": true,
                "textInputSupported": false,
                "keyHoldSupported": false,
                "keyRepeatSupported": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": { "activationRequested": false },
            "targeting": "opaque exact window generation"
        }
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/uix-key-sequence-result.schema.json"
        ))?,
        &result,
    )?;
    Ok(())
}

#[test]
fn pointer_v2_schemas_accept_confirmed_application_internal_click()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "action": "click",
        "coordinateSpace": "client-logical-px",
        "x": 20.5,
        "y": 30.25,
        "timeoutMs": 5000
    });
    assert_valid(
        &schema(include_str!("../contracts/v2/pointer-input.schema.json"))?,
        &input,
    )?;
    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.pointer@2",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "capability": "ui.input.pointer@2",
            "targetId": "s2:w:0000000000000000",
            "action": "click",
            "coordinateSpace": "client-logical-px",
            "x": 20.5,
            "y": 30.25,
            "handledEvents": ["pointer-down", "pointer-up"],
            "outcome": "completed",
            "dispatchState": "completed",
            "accepted": true,
            "effectConfirmed": false,
            "finalStateReached": false,
            "effectObservation": "not-exposed-by-uix-agent-v1",
            "revision": 6,
            "presentedRevision": 6,
            "settled": true,
            "windowReResolved": true,
            "applicationPolicyEvaluated": true,
            "confirmationEvaluatedBeforeDiscovery": true,
            "foregroundConsentRequired": false,
            "hostForegroundActivationRequested": false,
            "automaticRetryProhibited": true,
            "retrySafe": false,
            "timeoutMs": 5000,
            "executionRealm": "same-session-no-focus",
            "safety": {
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "desktopInputInjected": false,
                "desktopPointerMoved": false,
                "applicationInternalEventsDispatched": true,
                "requestScopedBalancedButtons": true,
                "x11Used": false,
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": { "activationRequested": false },
            "targeting": "opaque exact window generation"
        }
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v2/pointer-input-result.schema.json"
        ))?,
        &result,
    )?;
    Ok(())
}

#[test]
fn pointer_drag_v1_schemas_accept_balanced_application_internal_sequence()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "coordinateSpace": "client-logical-px",
        "start": { "x": 20.5, "y": 30.25 },
        "end": { "x": 120.5, "y": 230.25 },
        "samples": 12,
        "durationMs": 250,
        "timeoutMs": 5000
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/uix-pointer-drag-input.schema.json"
        ))?,
        &input,
    )?;
    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "ui.input.pointer.drag@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "capability": "ui.input.pointer.drag@1",
            "targetId": "s2:w:0000000000000000",
            "action": "drag",
            "button": "left",
            "coordinateSpace": "client-logical-px",
            "start": { "x": 20.5, "y": 30.25 },
            "end": { "x": 120.5, "y": 230.25 },
            "outcome": "completed",
            "dispatchState": "completed",
            "accepted": true,
            "pointerDownAccepted": true,
            "pointerUpAccepted": true,
            "buttonReleaseConfirmed": true,
            "requestScopedBalancedButtons": true,
            "samplesRequested": 12,
            "moveSamplesAccepted": 12,
            "durationMs": 250,
            "revision": 20,
            "presentedRevision": 20,
            "settled": true,
            "effectConfirmed": false,
            "finalStateReached": false,
            "windowReResolved": true,
            "applicationPolicyEvaluated": true,
            "confirmationEvaluatedBeforeDiscovery": true,
            "foregroundConsentRequired": false,
            "hostForegroundActivationRequested": false,
            "automaticRetryProhibited": true,
            "retrySafe": false,
            "timeoutMs": 5000,
            "executionRealm": "same-session-no-focus",
            "safety": {
                "provider": "uix-agent-v1",
                "applicationInternalEventsDispatched": true,
                "desktopInputInjected": false,
                "desktopPointerMoved": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": { "activationRequested": false },
            "targeting": "opaque exact window generation"
        }
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/uix-pointer-drag-result.schema.json"
        ))?,
        &result,
    )?;
    Ok(())
}

#[test]
fn lifecycle_v2_schemas_accept_confirmed_targetless_window_action()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "action": { "type": "maximize" },
        "timeoutMs": 5000
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v2/window-lifecycle-input.schema.json"
        ))?,
        &input,
    )?;
    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "apply",
        "capability": "window.lifecycle@2",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "host-foreground",
        "requiredExecutionRealm": "host-foreground",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "capability": "window.lifecycle@2",
            "targetId": "s2:w:0000000000000000",
            "action": "maximize",
            "outcome": "completed",
            "dispatchState": "completed",
            "accepted": true,
            "effectConfirmed": false,
            "finalStateReached": false,
            "effectObservation": "not-exposed-by-uix-agent-v1",
            "revision": 4,
            "presentedRevision": 4,
            "settled": true,
            "coordinateSpace": null,
            "requestedClientSize": null,
            "windowReResolved": true,
            "applicationPolicyEvaluated": true,
            "confirmationEvaluatedBeforeDiscovery": true,
            "foregroundConsentEvaluatedBeforeDiscovery": true,
            "foregroundImpactAuthorized": true,
            "automaticRetryProhibited": true,
            "retrySafe": false,
            "timeoutMs": 5000,
            "executionRealm": "host-foreground",
            "safety": {
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "hostForegroundActivationRequested": false,
                "desktopInputInjected": false,
                "moveSupportedOnLinux": false,
                "x11Used": false,
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": {
                "activationRequested": false,
                "visibleMutationAuthorized": true
            },
            "targeting": "opaque exact window generation"
        }
    });
    assert_valid(
        &schema(include_str!("../contracts/v2/window-lifecycle.schema.json"))?,
        &result,
    )?;
    Ok(())
}

#[test]
fn state_read_v1_schemas_accept_framework_current_logical_state()
-> Result<(), Box<dyn std::error::Error>> {
    let input = json!({});
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/window-state-read-input.schema.json"
        ))?,
        &input,
    )?;
    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "read",
        "capability": "window.state.read@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "ok": true,
            "contractVersion": "act/window-state-read/v1",
            "capability": "window.state.read@1",
            "targetId": "s2:w:0000000000000000",
            "observationSource": "uix-framework-current",
            "coordinateSpace": "client-logical-px",
            "visible": true,
            "presentable": true,
            "clientSize": { "width": 1000, "height": 700 },
            "windowState": {
                "maximized": false,
                "minimized": false,
                "fullscreen": false
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
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": { "activationRequested": false },
            "targeting": "opaque exact window generation"
        }
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/window-state-read.schema.json"
        ))?,
        &result,
    )?;
    Ok(())
}

#[test]
fn revision_wait_schemas_accept_changed_result() -> Result<(), Box<dyn std::error::Error>> {
    let input = json!({
        "condition": { "type": "revision-after", "revision": 9 },
        "timeoutMs": 5000
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/window-revision-wait-input.schema.json"
        ))?,
        &input,
    )?;
    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "read",
        "capability": "window.revision.wait@1",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "ok": true,
            "contractVersion": "act/window-revision-wait/v1",
            "capability": "window.revision.wait@1",
            "targetId": "s2:w:0000000000000000",
            "condition": "revision-after",
            "thresholdRevision": 9,
            "outcome": "changed",
            "revision": 10,
            "presentedRevision": 9,
            "closed": false,
            "targetReResolved": true,
            "timeoutMs": 5000,
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
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": { "activationRequested": false },
            "targeting": "opaque exact window generation"
        }
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v1/window-revision-wait.schema.json"
        ))?,
        &result,
    )?;

    let closed_input = json!({ "timeoutMs": 5000 });
    assert_valid(
        &schema(include_str!(
            "../contracts/v2/window-closed-wait-input.schema.json"
        ))?,
        &closed_input,
    )?;
    let closed_result = json!({
        "ok": true,
        "app": "app",
        "verb": "read",
        "capability": "window.closed.wait@2",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "ok": true,
            "contractVersion": "act/window-closed-wait/v2",
            "capability": "window.closed.wait@2",
            "targetId": "s2:w:0000000000000000",
            "outcome": "closed",
            "revision": 11,
            "presentedRevision": 10,
            "closed": true,
            "exactGenerationConfirmed": true,
            "targetReResolved": true,
            "timeoutMs": 5000,
            "executionRealm": "same-session-no-focus",
            "readOnly": true,
            "mutationAllowed": false,
            "safety": {
                "providerCache": "disabled",
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "foregroundActivationRequested": false,
                "desktopInputInjected": false,
                "windowInventoryPolled": false,
                "x11Used": false,
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": { "activationRequested": false },
            "targeting": "opaque exact window generation"
        }
    });
    assert_valid(
        &schema(include_str!(
            "../contracts/v2/window-closed-wait.schema.json"
        ))?,
        &closed_result,
    )?;
    Ok(())
}

#[test]
fn closed_wait_cli_reaches_strict_input_contract_before_provider_io()
-> Result<(), Box<dyn std::error::Error>> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let input = std::env::temp_dir().join(format!(
        "ai-computer-toolkit-closed-wait-{}-{nonce}.json",
        std::process::id()
    ));
    std::fs::write(&input, r#"{"timeoutMs":99}"#)?;
    let result = cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "read".to_owned(),
        "--capability".to_owned(),
        "window.closed.wait@2".to_owned(),
        "--target".to_owned(),
        "sessionId=s2:w:0000000000000000".to_owned(),
        "--input".to_owned(),
        input.to_string_lossy().into_owned(),
    ]);
    std::fs::remove_file(&input)?;
    let error = match result {
        Ok(_) => panic!("out-of-range timeout must fail before endpoint discovery"),
        Err(error) => error,
    };
    assert_eq!(error.code, "INVALID_ARGUMENT");
    assert!(error.message.contains("bounded contract"));
    Ok(())
}

#[test]
fn close_v2_schemas_accept_confirmed_request_only_result() -> Result<(), Box<dyn std::error::Error>>
{
    let input = json!({ "timeoutMs": 5000 });
    assert_valid(
        &schema(include_str!(
            "../contracts/v2/window-close-input.schema.json"
        ))?,
        &input,
    )?;
    let result = json!({
        "ok": true,
        "app": "app",
        "verb": "close",
        "capability": "window.close@2",
        "targetId": "s2:w:0000000000000000",
        "executionRealm": "same-session-no-focus",
        "requiredExecutionRealm": "same-session-no-focus",
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": {
            "ok": true,
            "contractVersion": "act/window-close/v2",
            "capability": "window.close@2",
            "targetId": "s2:w:0000000000000000",
            "outcome": "accepted",
            "dispatchState": "completed",
            "accepted": true,
            "closeConfirmed": false,
            "finalStateReached": false,
            "effectObservation": "separate-window.closed.wait@2",
            "followUpCapability": "window.closed.wait@2",
            "revision": 12,
            "presentedRevision": 11,
            "settled": true,
            "windowReResolved": true,
            "applicationPolicyEvaluated": true,
            "confirmationEvaluatedBeforeDiscovery": true,
            "automaticRetryProhibited": true,
            "retrySafe": false,
            "timeoutMs": 5000,
            "executionRealm": "same-session-no-focus",
            "safety": {
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "hostForegroundActivationRequested": false,
                "desktopInputInjected": false,
                "windowInventoryPolled": false,
                "x11Used": false,
                "fallback": "none"
            }
        },
        "meta": {
            "foreground": { "activationRequested": false },
            "targeting": "opaque exact window generation"
        }
    });
    assert_valid(
        &schema(include_str!("../contracts/v2/window-close.schema.json"))?,
        &result,
    )?;
    Ok(())
}

#[test]
fn production_sources_keep_transport_secrets_private_and_forbid_x11_fallback() {
    let transport = include_str!("../src/adapters/linux/uix_agent.rs");
    let client = include_str!("../src/adapters/linux/uix_agent_client.rs");
    let module = include_str!("../src/modules/linux_uix_window.rs");
    let element_location = include_str!("../src/modules/linux_uix_element_location.rs");
    let closed_wait = include_str!("../src/modules/linux_uix_closed_wait.rs");
    let close_module = include_str!("../src/modules/linux_uix_close.rs");
    let drag_adapter = include_str!("../src/adapters/linux/uix_agent_pointer_drag.rs");
    let drag_module = include_str!("../src/modules/linux_uix_pointer_drag.rs");
    let drag_transition_adapter =
        include_str!("../src/adapters/linux/uix_agent_pointer_drag_transition.rs");
    let drag_transition_module =
        include_str!("../src/modules/linux_uix_pointer_drag_transition.rs");
    let key_sequence_adapter = include_str!("../src/adapters/linux/uix_agent_key_sequence.rs");
    let key_sequence_module = include_str!("../src/modules/linux_uix_key_sequence.rs");
    let input_sequence_adapter = include_str!("../src/adapters/linux/uix_agent_input_sequence.rs");
    let input_sequence_module = include_str!("../src/modules/linux_uix_input_sequence.rs");
    let input_sequence_transition_adapter =
        include_str!("../src/adapters/linux/uix_agent_input_sequence_transition.rs");
    let input_sequence_transition_module =
        include_str!("../src/modules/linux_uix_input_sequence_transition.rs");
    let click_sequence_adapter =
        include_str!("../src/adapters/linux/uix_agent_pointer_click_sequence.rs");
    let click_sequence_module = include_str!("../src/modules/linux_uix_pointer_click_sequence.rs");
    let click_transition_adapter =
        include_str!("../src/adapters/linux/uix_agent_pointer_click_transition.rs");
    let click_transition_module =
        include_str!("../src/modules/linux_uix_pointer_click_transition.rs");
    let move_transition_adapter =
        include_str!("../src/adapters/linux/uix_agent_pointer_move_transition.rs");
    let move_transition_module =
        include_str!("../src/modules/linux_uix_pointer_move_transition.rs");
    let pointer_sequence_adapter =
        include_str!("../src/adapters/linux/uix_agent_pointer_sequence.rs");
    let pointer_sequence_module = include_str!("../src/modules/linux_uix_pointer_sequence.rs");
    let pointer_sequence_transition_adapter =
        include_str!("../src/adapters/linux/uix_agent_pointer_sequence_transition.rs");
    assert!(transport.contains("socket_peercred"));
    assert!(transport.contains("OFlags::NOFOLLOW"));
    assert!(!module.contains("process_id"));
    assert!(!module.contains("descriptor.token"));
    assert!(!closed_wait.contains("process_id"));
    assert!(!closed_wait.contains("descriptor.token"));
    assert!(!close_module.contains("process_id"));
    assert!(!close_module.contains("descriptor.token"));
    assert!(!drag_module.contains("process_id"));
    assert!(!drag_module.contains("descriptor.token"));
    assert!(!drag_transition_module.contains("process_id"));
    assert!(!drag_transition_module.contains("descriptor.token"));
    assert!(!key_sequence_module.contains("process_id"));
    assert!(!key_sequence_module.contains("descriptor.token"));
    assert!(!input_sequence_module.contains("process_id"));
    assert!(!input_sequence_module.contains("descriptor.token"));
    assert!(!input_sequence_transition_module.contains("process_id"));
    assert!(!input_sequence_transition_module.contains("descriptor.token"));
    assert!(!click_sequence_module.contains("process_id"));
    assert!(!click_sequence_module.contains("descriptor.token"));
    assert!(!click_transition_module.contains("process_id"));
    assert!(!click_transition_module.contains("descriptor.token"));
    assert!(!move_transition_module.contains("process_id"));
    assert!(!move_transition_module.contains("descriptor.token"));
    assert!(!pointer_sequence_module.contains("process_id"));
    assert!(!pointer_sequence_module.contains("descriptor.token"));
    for forbidden in ["XOpenDisplay", "XTest", "xdotool", "xwayland"] {
        for source in [
            transport,
            client,
            drag_adapter,
            drag_transition_adapter,
            key_sequence_adapter,
            input_sequence_adapter,
            input_sequence_transition_adapter,
            click_sequence_adapter,
            click_transition_adapter,
            move_transition_adapter,
            pointer_sequence_adapter,
            pointer_sequence_transition_adapter,
        ] {
            assert!(
                !source
                    .to_ascii_lowercase()
                    .contains(&forbidden.to_ascii_lowercase())
            );
        }
        assert!(
            !module
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
        assert!(
            !element_location
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
        assert!(
            !closed_wait
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
        assert!(
            !close_module
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
        assert!(
            !drag_module
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
        assert!(
            !drag_transition_module
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
        assert!(
            !key_sequence_module
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
        assert!(
            !click_sequence_module
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
        assert!(
            !click_transition_module
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
        assert!(
            !move_transition_module
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
        assert!(
            !pointer_sequence_module
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase())
        );
    }
}

#[test]
fn tree_cli_rejects_missing_target_before_endpoint_discovery() {
    let error = match cli::run(vec!["inspect-tree".to_owned(), "accessibility".to_owned()]) {
        Ok(_) => panic!("missing target must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.code, "INVALID_ARGUMENT");
}

#[test]
fn strict_isolation_is_rejected_before_uix_endpoint_discovery() {
    let error = match cli::run(vec![
        "sessions".to_owned(),
        "window".to_owned(),
        "--strict-isolation".to_owned(),
    ]) {
        Ok(_) => panic!("same-session provider must reject strict isolation"),
        Err(error) => error,
    };
    assert_eq!(error.code, "ISOLATION_REQUIRED");
    assert_eq!(error.details["executionRealm"], "same-session-no-focus");
    assert_eq!(error.details["fallback"], "none");
}

#[test]
fn unconfirmed_uix_action_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.element.action@2".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed mutation must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_uix_key_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.key@2".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed key mutation must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_uix_key_sequence_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.key.sequence@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed key sequence must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_uix_input_sequence_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.sequence@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed input sequence must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_uix_pointer_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.pointer@2".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed pointer mutation must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_uix_pointer_click_sequence_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.pointer.click.sequence@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed click sequence must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_uix_pointer_click_transition_fails_before_target_input_or_endpoint_discovery() {
    // transition 未确认时必须在读取目标与输入文件前封闭失败。
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.pointer.click.transition@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed click transition must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_uix_pointer_move_transition_fails_before_target_input_or_endpoint_discovery() {
    // transition 未确认时必须在读取目标与输入文件前封闭失败。
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.pointer.move.transition@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed pointer move transition must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_uix_pointer_sequence_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.pointer.sequence@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed mixed pointer sequence must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_uix_pointer_drag_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.pointer.drag@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed pointer drag must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_uix_pointer_drag_transition_fails_before_target_input_or_endpoint_discovery() {
    // transition 未确认时必须在读取目标与输入文件前封闭失败。
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "ui.input.pointer.drag.transition@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed pointer drag transition must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_uix_close_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "close".to_owned(),
        "--capability".to_owned(),
        "window.close@2".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed close must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_uix_screenshot_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "screenshot".to_owned(),
        "--capability".to_owned(),
        "window.screenshot@2".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed screenshot must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn unconfirmed_lifecycle_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "window.lifecycle@2".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed mutation must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn lifecycle_requires_foreground_consent_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "window.lifecycle@2".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-foreground-consent.json".to_owned(),
        "--confirm".to_owned(),
    ]) {
        Ok(_) => panic!("foreground consent must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "FOREGROUND_CONSENT_REQUIRED");
}

#[test]
fn unconfirmed_activation_fails_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "window.activate@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-confirmation.json".to_owned(),
    ]) {
        Ok(_) => panic!("unconfirmed activation must fail before other fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn activation_requires_foreground_consent_before_target_input_or_endpoint_discovery() {
    let error = match cli::run(vec![
        "run".to_owned(),
        "app".to_owned(),
        "apply".to_owned(),
        "--capability".to_owned(),
        "window.activate@1".to_owned(),
        "--target".to_owned(),
        "sessionId=not-a-target".to_owned(),
        "--input".to_owned(),
        "/definitely/not/read-before-foreground-consent.json".to_owned(),
        "--confirm".to_owned(),
    ]) {
        Ok(_) => panic!("foreground consent must fail before activation fields"),
        Err(error) => error,
    };
    assert_eq!(error.code, "FOREGROUND_CONSENT_REQUIRED");
}
