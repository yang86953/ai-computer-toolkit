//! Linux capability assessment Module。

use serde_json::{Value, json};

use crate::{
    adapters::linux::{procfs, uix_agent, wayland_portal},
    capabilities,
    components::{
        linux_host_identity,
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    },
    domain::{AppControlError, AppResult},
    modules::{application_launch, process_termination, uix_window},
};

fn snapshot_error(_: std::io::Error) -> AppControlError {
    AppControlError::with_details(
        "PROCESS_SNAPSHOT_FAILED",
        "The Linux process snapshot could not be read.",
        json!({
            "platform": "linux",
            "provider": "procfs",
            "providerState": "unavailable",
            "executionRealm": "none",
            "fallback": "none"
        }),
    )
}

/// 对当前 Linux provider 重新发现精确目标并给出封闭决策。
pub(crate) fn assess(capability: &str, session_id: &str) -> AppResult<Value> {
    if capabilities::definition(capability).is_none() {
        return Err(AppControlError::with_details(
            "INVALID_ARGUMENT",
            "The capability identifier is not registered.",
            json!({
                "argument": "capability",
                "reason": "unknown-capability-id"
            }),
        ));
    }
    let parsed = OpaqueTargetId::parse(session_id).ok_or_else(|| {
        AppControlError::new(
            "STALE_SESSION",
            "The assessment target is not a current canonical opaque session.",
        )
    })?;
    if parsed.kind() == OpaqueTargetKind::Host {
        return assess_host(capability, session_id);
    }
    if parsed.kind() == OpaqueTargetKind::InteractiveSession
        && matches!(
            capability,
            capabilities::DESKTOP_SESSION_CLOSE
                | capabilities::SCREEN_CAPTURE
                | capabilities::UI_INPUT_KEY_V3
                | capabilities::UI_INPUT_POINTER_V3
        )
    {
        let confirmation_required = matches!(
            capability,
            capabilities::SCREEN_CAPTURE
                | capabilities::UI_INPUT_KEY_V3
                | capabilities::UI_INPUT_POINTER_V3
        );
        let foreground_consent_required = matches!(
            capability,
            capabilities::UI_INPUT_KEY_V3 | capabilities::UI_INPUT_POINTER_V3
        );
        return Ok(json!({
            "ok": true,
            "contractVersion": "act/control/v1",
            "capability": capability,
            "targetId": session_id,
            "decision": "unavailable",
            "executionRealm": "none",
            "requiresConfirmation": confirmation_required,
            "requiresForegroundConsent": foreground_consent_required,
            "reasons": ["live-desktop-session-must-be-resolved-by-owning-broker-epoch"],
            "constraints": {
                "scope": "exact-live-desktop-session-owner-generation",
                "assessmentRoute": if capability == capabilities::UI_INPUT_KEY_V3 {
                    "session-host-desktop-input-key"
                } else if capability == capabilities::UI_INPUT_POINTER_V3 {
                    "session-host-desktop-input-pointer"
                } else if capability == capabilities::SCREEN_CAPTURE {
                    "session-host-desktop-capture-frame"
                } else {
                    "session-host-desktop-inspect"
                },
                "noFallback": true,
            },
            "evidence": {
                "targetKind": "desktop-session",
                "platform": "linux",
                "implementationState": "persistent-broker-context-required",
                "nativeIdentityExposed": false,
                "automaticRetryAllowed": false,
            },
        }));
    }
    if parsed.kind() == OpaqueTargetKind::Window
        && matches!(
            capability,
            capabilities::WINDOW_METADATA_READ_V3
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
        )
    {
        let window = uix_agent::resolve(session_id).map_err(uix_window::public_error)?;
        if capability == capabilities::WINDOW_STATE_READ && window.state.is_none() {
            return Err(uix_window::window_state_unavailable());
        }
        if matches!(
            capability,
            capabilities::WINDOW_STATE_WAIT | capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION
        ) && (window.state.is_none() || window.focused.is_none())
        {
            return Err(AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The target UIX Agent did not negotiate the complete window state wait fields.",
                json!({
                    "requiredNegotiation": ["logical_width", "logical_height", "maximized", "minimized", "fullscreen", "focused"],
                    "executionRealm": "none",
                    "fallback": "none",
                }),
            ));
        }
        if capability == capabilities::WINDOW_SCREENSHOT_V2 && !window.screenshot_supported {
            return Err(AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The target UIX Agent did not negotiate bounded application-surface screenshots.",
                json!({
                    "requiredNegotiation": ["screenshot", "max_screenshot_bytes", "max_response_bytes"],
                    "executionRealm": "none",
                    "fallback": "none",
                }),
            ));
        }
        if capability == capabilities::WINDOW_ACTIVATE && !window.activation_supported {
            return Err(AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The target UIX Agent did not publish window activation.",
                json!({
                    "requiredWindowAction": "activate_window",
                    "executionRealm": "none",
                    "fallback": "none",
                }),
            ));
        }
        if capability == capabilities::WINDOW_ACTIVATE_TRANSITION
            && (!window.activation_supported || window.focused.is_none())
        {
            return Err(AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The target UIX Agent did not publish activate_window and focused for an activation transition.",
                json!({
                    "requiredWindowAction": "activate_window",
                    "requiredNegotiation": ["focused"],
                    "executionRealm": "none",
                    "fallback": "none",
                }),
            ));
        }
        if matches!(
            capability,
            capabilities::UI_INPUT_POINTER_DRAG | capabilities::UI_INPUT_POINTER_DRAG_TRANSITION
        ) && !window.pointer_drag_supported
        {
            return Err(AppControlError::with_details(
                "CAPABILITY_UNAVAILABLE",
                "The target UIX Agent did not publish the complete drag action set.",
                json!({
                    "requiredWindowActions": ["pointer_down", "pointer_move", "pointer_up"],
                    "executionRealm": "none",
                    "fallback": "none",
                }),
            ));
        }
        let semantic_mutation = capability == capabilities::UI_ELEMENT_ACTION_V2;
        let element_location_read = capability == capabilities::UI_ELEMENT_LOCATE_V2;
        let element_wait = capability == capabilities::UI_ELEMENT_WAIT_V2;
        let semantic_transition = capability == capabilities::UI_ELEMENT_TRANSITION;
        let key_mutation = capability == capabilities::UI_INPUT_KEY_V2;
        let key_sequence = capability == capabilities::UI_INPUT_KEY_SEQUENCE;
        let key_sequence_transition = capability == capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION;
        let key_transition = capability == capabilities::UI_INPUT_KEY_TRANSITION;
        let input_sequence = capability == capabilities::UI_INPUT_SEQUENCE;
        let input_sequence_transition = capability == capabilities::UI_INPUT_SEQUENCE_TRANSITION;
        let pointer_mutation = capability == capabilities::UI_INPUT_POINTER_V2;
        let pointer_click_sequence = capability == capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE;
        let pointer_click_sequence_transition =
            capability == capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION;
        let pointer_click_transition =
            capability == capabilities::UI_INPUT_POINTER_CLICK_TRANSITION;
        let pointer_move_transition = capability == capabilities::UI_INPUT_POINTER_MOVE_TRANSITION;
        let pointer_move_sequence = capability == capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE;
        let pointer_move_sequence_transition =
            capability == capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION;
        let pointer_sequence = capability == capabilities::UI_INPUT_POINTER_SEQUENCE;
        let pointer_sequence_transition =
            capability == capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION;
        let pointer_drag = capability == capabilities::UI_INPUT_POINTER_DRAG;
        let pointer_drag_transition = capability == capabilities::UI_INPUT_POINTER_DRAG_TRANSITION;
        let lifecycle_mutation = capability == capabilities::WINDOW_LIFECYCLE_V2;
        let lifecycle_sequence = capability == capabilities::WINDOW_LIFECYCLE_SEQUENCE;
        let lifecycle_sequence_transition =
            capability == capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION;
        let lifecycle_transition = capability == capabilities::WINDOW_LIFECYCLE_TRANSITION;
        let close_mutation = capability == capabilities::WINDOW_CLOSE_V2;
        let close_transition = capability == capabilities::WINDOW_CLOSE_TRANSITION;
        let state_read = capability == capabilities::WINDOW_STATE_READ;
        let state_wait = capability == capabilities::WINDOW_STATE_WAIT;
        let screenshot = capability == capabilities::WINDOW_SCREENSHOT_V2;
        let activation = capability == capabilities::WINDOW_ACTIVATE;
        let activation_transition = capability == capabilities::WINDOW_ACTIVATE_TRANSITION;
        // 将新增 close transition 约束移出总 assessment JSON，避免宏展开递归过深。
        let supported_window_close_transition = if close_transition {
            json!({
                "target": "exact-window-generation",
                "windowAction": "close_window",
                "sameAuthenticatedConnectionUsed": true,
                "waitProtocolUsed": true,
                "providerPolling": false,
                "terminalReplyRequired": true,
                "connectionCloseAcceptedAsProof": false,
                "causalityConfirmed": false,
                "applicationClosedConfirmed": false,
                "foregroundActivationRequested": false,
                "desktopInputInjected": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "fallback": "none"
            })
        } else {
            Value::Null
        };
        // 将激活 transition 约束单独建值，避免把聚合 assessment 宏展开成递归表达式。
        let supported_window_activation_transition = if activation_transition {
            json!({
                "target": "exact-window-generation",
                "windowAction": "activate_window",
                "requiredNegotiation": ["focused"],
                "sameAuthenticatedConnectionUsed": true,
                "providerPolling": true,
                "waitProtocolUsed": false,
                "requestAcceptedDoesNotGuaranteePersistentFocus": true,
                "causalityConfirmed": false,
                "finalFocusStateGuaranteed": false,
                "foregroundActivationRequested": true,
                "desktopInputInjected": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "fallback": "none"
            })
        } else {
            Value::Null
        };
        // 将按键 transition 约束单独建值，明确完整 press 与提交后语义观察边界。
        let supported_key_transition = if key_transition {
            json!({
                "sourceScope": "single-complete-key-press",
                "windowAction": "press_key",
                "completePress": true,
                "selectorSemantics": "exact-and",
                "conditionKinds": ["unique", "missing"],
                "sameAuthenticatedConnectionUsed": true,
                "semanticRevisionWaitUsed": true,
                "providerPolling": false,
                "causalityConfirmed": false,
                "finalUIStateConfirmed": false,
                "finalStateReached": false,
                "foregroundActivationRequested": false,
                "desktopInputInjected": false,
                "independentKeyOwnershipPublished": false,
                "textInputPublished": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "automaticRetryProhibited": true,
                "retrySafe": false,
                "fallback": "none"
            })
        } else {
            Value::Null
        };
        // 将完整按键序列 transition 约束单独建值，冻结序列完成后的语义观察边界。
        let supported_key_sequence_transition = if key_sequence_transition {
            json!({
                "sourceScope": "complete-request-scoped-key-press-sequence",
                "windowAction": "press_key",
                "maximumPresses": 64,
                "maximumPlannedDurationMs": 5000,
                "completeSequenceRequiredBeforePostcondition": true,
                "selectorSemantics": "exact-and",
                "conditionKinds": ["unique", "missing"],
                "sameAuthenticatedConnectionUsed": true,
                "sameConnectionRevisionChain": true,
                "semanticRevisionWaitUsed": true,
                "providerPolling": false,
                "waitProtocolUsed": true,
                "requestScopedBalancedPresses": true,
                "transactionSemantics": false,
                "rollbackSemantics": false,
                "causalityConfirmed": false,
                "applicationConsumptionConfirmed": false,
                "finalUIStateConfirmed": false,
                "finalStateReached": false,
                "foregroundActivationRequested": false,
                "desktopInputInjected": false,
                "independentKeyOwnershipPublished": false,
                "keyDownUpPublished": false,
                "keyHoldPublished": false,
                "keyRepeatPublished": false,
                "textInputPublished": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "automaticRetryProhibited": true,
                "retrySafe": false,
                "fallback": "none"
            })
        } else {
            Value::Null
        };
        // 将普通左键 click transition 约束单独建值，冻结同连接条件观察边界。
        let supported_pointer_click_transition = if pointer_click_transition {
            json!({
                "sourceScope": "single-ordinary-left-click",
                "windowAction": "click_at",
                "ordinaryLeftClick": true,
                "coordinateSpace": "client-logical-px",
                "selectorSemantics": "exact-and",
                "conditionKinds": ["unique", "missing"],
                "sameAuthenticatedConnectionUsed": true,
                "semanticRevisionWaitUsed": true,
                "providerPolling": false,
                "waitProtocolUsed": true,
                "causalityConfirmed": false,
                "finalUIStateConfirmed": false,
                "finalStateReached": false,
                "foregroundActivationRequested": false,
                "desktopInputInjected": false,
                "desktopPointerMoved": false,
                "independentButtonOwnershipPublished": false,
                "doubleClickSemantics": false,
                "clickCountSemantics": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "automaticRetryProhibited": true,
                "retrySafe": false,
                "fallback": "none"
            })
        } else {
            Value::Null
        };
        // 将普通左键点击序列 transition 约束单独建值，冻结完整序列后的语义观察边界。
        let supported_pointer_click_sequence_transition = if pointer_click_sequence_transition {
            json!({
                "sourceScope": "complete-request-scoped-ordinary-left-click-sequence",
                "windowAction": "click_at",
                "ordinaryLeftClicksOnly": true,
                "coordinateSpace": "client-logical-px",
                "maximumClicks": 64,
                "maximumPlannedDurationMs": 5000,
                "completeSequenceRequiredBeforePostcondition": true,
                "selectorSemantics": "exact-and",
                "conditionKinds": ["unique", "missing"],
                "sameAuthenticatedConnectionUsed": true,
                "sameConnectionRevisionChain": true,
                "semanticRevisionWaitUsed": true,
                "providerPolling": false,
                "waitProtocolUsed": true,
                "requestScopedBalancedClicks": true,
                "transactionSemantics": false,
                "rollbackSemantics": false,
                "causalityConfirmed": false,
                "applicationConsumptionConfirmed": false,
                "finalUIStateConfirmed": false,
                "finalStateReached": false,
                "foregroundActivationRequested": false,
                "desktopInputInjected": false,
                "desktopPointerMoved": false,
                "independentButtonOwnershipPublished": false,
                "dragSemantics": false,
                "doubleClickSemantics": false,
                "clickCountSemantics": false,
                "scrollSemantics": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "automaticRetryProhibited": true,
                "retrySafe": false,
                "fallback": "none"
            })
        } else {
            Value::Null
        };
        // 将应用内 pointer_move transition 约束单独建值，冻结 hover 状态观察边界。
        let supported_pointer_move_transition = if pointer_move_transition {
            json!({
                "sourceScope": "single-application-internal-pointer-move",
                "windowAction": "pointer_move",
                "coordinateSpace": "client-logical-px",
                "selectorSemantics": "exact-and",
                "conditionKinds": ["unique", "missing"],
                "sameAuthenticatedConnectionUsed": true,
                "semanticRevisionWaitUsed": true,
                "providerPolling": false,
                "waitProtocolUsed": true,
                "causalityConfirmed": false,
                "applicationConsumptionConfirmed": false,
                "finalUIStateConfirmed": false,
                "finalStateReached": false,
                "foregroundActivationRequested": false,
                "desktopInputInjected": false,
                "desktopPointerMoved": false,
                "clickSemantics": false,
                "independentButtonOwnershipPublished": false,
                "interpolationSemantics": false,
                "dragSemantics": false,
                "scrollSemantics": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "automaticRetryProhibited": true,
                "retrySafe": false,
                "fallback": "none"
            })
        } else {
            Value::Null
        };
        // 将纯移动点序列 transition 单独建值，冻结完整路径后的语义观察边界。
        let supported_pointer_move_sequence_transition = if pointer_move_sequence_transition {
            json!({
                "sourceScope": "complete-request-scoped-pointer-move-sequence",
                "windowAction": "pointer_move",
                "coordinateSpace": "client-logical-px",
                "minimumMoves": 2,
                "maximumMoves": 64,
                "maximumPlannedDurationMs": 5000,
                "completeSequenceRequiredBeforePostcondition": true,
                "selectorSemantics": "exact-and",
                "conditionKinds": ["unique", "missing"],
                "sameAuthenticatedConnectionUsed": true,
                "sameConnectionRevisionChain": true,
                "semanticRevisionWaitUsed": true,
                "providerPolling": false,
                "waitProtocolUsed": true,
                "transactionSemantics": false,
                "rollbackSemantics": false,
                "causalityConfirmed": false,
                "applicationConsumptionConfirmed": false,
                "finalUIStateConfirmed": false,
                "finalStateReached": false,
                "foregroundActivationRequested": false,
                "desktopInputInjected": false,
                "desktopPointerMoved": false,
                "clickSemantics": false,
                "keyPressSemantics": false,
                "interpolationSemantics": false,
                "dragSemantics": false,
                "scrollSemantics": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "automaticRetryProhibited": true,
                "retrySafe": false,
                "fallback": "none"
            })
        } else {
            Value::Null
        };
        // 将配平拖拽 transition 约束单独建值，冻结释放确认后的语义观察边界。
        let supported_pointer_drag_transition = if pointer_drag_transition {
            json!({
                "sourceScope": "request-scoped-balanced-left-drag",
                "windowActions": ["pointer_down", "pointer_move", "pointer_up"],
                "coordinateSpace": "client-logical-px",
                "postconditionRequiresConfirmedRelease": true,
                "selectorSemantics": "exact-and",
                "conditionKinds": ["unique", "missing"],
                "sameAuthenticatedConnectionUsed": true,
                "semanticRevisionWaitUsed": true,
                "providerPolling": false,
                "waitProtocolUsed": true,
                "requestScopedBalancedButtons": true,
                "bestEffortReleaseAfterDownFailure": true,
                "causalityConfirmed": false,
                "applicationConsumptionConfirmed": false,
                "finalUIStateConfirmed": false,
                "finalStateReached": false,
                "foregroundActivationRequested": false,
                "desktopInputInjected": false,
                "desktopPointerMoved": false,
                "independentButtonOwnershipPublished": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "automaticRetryProhibited": true,
                "retrySafe": false,
                "fallback": "none"
            })
        } else {
            Value::Null
        };
        // 将混合指针序列 transition 约束单独建值，冻结完整序列后的语义观察边界。
        let supported_pointer_sequence_transition = if pointer_sequence_transition {
            json!({
                "sourceScope": "complete-request-scoped-hover-and-ordinary-left-click-sequence",
                "windowActions": ["pointer_move", "click_at"],
                "stepKinds": ["move", "click"],
                "requiresMoveAndClick": true,
                "completeSequenceRequiredBeforePostcondition": true,
                "selectorSemantics": "exact-and",
                "conditionKinds": ["unique", "missing"],
                "sameAuthenticatedConnectionUsed": true,
                "sameConnectionRevisionChain": true,
                "semanticRevisionWaitUsed": true,
                "providerPolling": false,
                "waitProtocolUsed": true,
                "requestScopedBalancedClicks": true,
                "causalityConfirmed": false,
                "applicationConsumptionConfirmed": false,
                "finalUIStateConfirmed": false,
                "finalStateReached": false,
                "foregroundActivationRequested": false,
                "desktopInputInjected": false,
                "desktopPointerMoved": false,
                "independentButtonOwnershipPublished": false,
                "dragSemantics": false,
                "doubleClickSemantics": false,
                "clickCountSemantics": false,
                "scrollSemantics": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "automaticRetryProhibited": true,
                "retrySafe": false,
                "fallback": "none"
            })
        } else {
            Value::Null
        };
        // 将跨模态序列 transition 约束单独建值，避免聚合证据巨型宏超过递归限制。
        let supported_input_sequence_transition = if input_sequence_transition {
            json!({
                "sourceScope": "complete-request-scoped-key-and-pointer-sequence",
                "stepKinds": ["press", "move", "click"],
                "requiresKeyAndPointer": true,
                "maximumSteps": 64,
                "maximumPlannedDurationMs": 5000,
                "completeSequenceRequiredBeforePostcondition": true,
                "selectorSemantics": "exact-and",
                "conditionKinds": ["unique", "missing"],
                "sameAuthenticatedConnectionUsed": true,
                "sameConnectionRevisionChain": true,
                "semanticRevisionWaitUsed": true,
                "providerPolling": false,
                "waitProtocolUsed": true,
                "requestScopedBalancedPresses": true,
                "requestScopedBalancedClicks": true,
                "transactionSemantics": false,
                "rollbackSemantics": false,
                "causalityConfirmed": false,
                "applicationConsumptionConfirmed": false,
                "finalUIStateConfirmed": false,
                "finalStateReached": false,
                "foregroundActivationRequested": false,
                "desktopInputInjected": false,
                "desktopPointerMoved": false,
                "independentKeyOwnershipPublished": false,
                "independentButtonOwnershipPublished": false,
                "dragSemantics": false,
                "doubleClickSemantics": false,
                "clickCountSemantics": false,
                "scrollSemantics": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "automaticRetryProhibited": true,
                "retrySafe": false,
                "fallback": "none"
            })
        } else {
            Value::Null
        };
        let supported_state_wait = if state_wait {
            json!({
                "conditionKinds": ["visibility", "focus", "client-size", "window-flags"],
                "providerPolling": "list_windows",
                "waitProtocolUsed": false,
                "sameAuthenticatedConnectionUsed": true
            })
        } else {
            Value::Null
        };
        let supported_lifecycle_sequence_transition = if lifecycle_sequence_transition {
            json!({
                "actionKinds": ["restore", "minimize", "maximize", "resize"],
                "conditionKinds": ["visibility", "focus", "client-size", "window-flags"],
                "requiredDistinctActionKinds": 2,
                "maximumActions": 16,
                "maximumPlannedDurationMs": 5000,
                "completeSequenceRequiredBeforeCondition": true,
                "sameAuthenticatedConnectionUsed": true,
                "sameConnectionRevisionChain": true,
                "providerPolling": true,
                "waitProtocolUsed": false,
                "arbitraryWindowMoveSupported": false,
                "transactionSemantics": false,
                "rollbackSemantics": false,
                "causalityConfirmed": false,
                "compositorFinalStateConfirmed": false,
                "desktopInputInjected": false,
                "nativeIdentityExposed": false,
                "transportIdentityExposed": false,
                "x11Used": false,
                "fallback": "none"
            })
        } else {
            Value::Null
        };
        let mutation = semantic_mutation
            || semantic_transition
            || key_mutation
            || key_sequence
            || key_sequence_transition
            || key_transition
            || input_sequence
            || input_sequence_transition
            || pointer_mutation
            || pointer_click_sequence
            || pointer_click_sequence_transition
            || pointer_click_transition
            || pointer_move_transition
            || pointer_move_sequence
            || pointer_move_sequence_transition
            || pointer_sequence
            || pointer_sequence_transition
            || pointer_drag
            || pointer_drag_transition
            || lifecycle_mutation
            || lifecycle_sequence
            || lifecycle_sequence_transition
            || lifecycle_transition
            || close_mutation
            || close_transition
            || screenshot
            || activation
            || activation_transition;
        let mut evidence = json!({
            "targetKind": "uix-agent-window",
            "platform": "linux",
            "implementationState": if semantic_transition || key_sequence_transition || key_transition || input_sequence_transition || pointer_click_sequence_transition || pointer_click_transition || pointer_move_transition || pointer_move_sequence_transition || pointer_sequence_transition || pointer_drag_transition || lifecycle_sequence_transition || close_transition || activation_transition {
                "production-route-protocol-fixture-verified-owner-interaction-and-observation-acceptance-deferred"
            } else if state_wait || element_wait || lifecycle_transition {
                "production-route-protocol-fixture-verified-owner-observation-acceptance-deferred"
            } else if pointer_drag
                || key_sequence
                || input_sequence
                || pointer_click_sequence
                || pointer_move_sequence
                || pointer_sequence
                || lifecycle_sequence
            {
                "production-route-protocol-fixture-verified-owner-interaction-acceptance-deferred"
            } else if screenshot || activation {
                "production-route-protocol-fixture-verified-owner-visual-acceptance-deferred"
            } else {
                "production-route-wayland-live-verified"
            },
            "foregroundActivationAllowed": activation || activation_transition,
            "inputAllowed": false,
            "semanticMutationAllowed": semantic_mutation || semantic_transition,
            "semanticTransitionAllowed": semantic_transition,
            "elementLocationReadAllowed": element_location_read,
            "elementWaitAllowed": element_wait,
            "applicationInternalKeyMutationAllowed": key_mutation,
            "applicationInternalKeySequenceAllowed": key_sequence,
            "applicationInternalKeySequenceTransitionAllowed": key_sequence_transition,
            "applicationInternalKeyTransitionAllowed": key_transition,
            "applicationInternalInputSequenceAllowed": input_sequence,
            "applicationInternalInputSequenceTransitionAllowed": input_sequence_transition,
        });
        if let (Some(evidence), Value::Object(additional_evidence)) = (
            evidence.as_object_mut(),
            json!({
            "applicationInternalPointerMutationAllowed": pointer_mutation,
            "applicationInternalPointerClickSequenceAllowed": pointer_click_sequence,
            "applicationInternalPointerClickSequenceTransitionAllowed": pointer_click_sequence_transition,
            "applicationInternalPointerClickTransitionAllowed": pointer_click_transition,
            "applicationInternalPointerMoveTransitionAllowed": pointer_move_transition,
            "applicationInternalPointerMoveSequenceAllowed": pointer_move_sequence,
            "applicationInternalPointerMoveSequenceTransitionAllowed": pointer_move_sequence_transition,
            "applicationInternalPointerSequenceAllowed": pointer_sequence,
            "applicationInternalPointerSequenceTransitionAllowed": pointer_sequence_transition,
            "applicationInternalPointerDragAllowed": pointer_drag,
            "applicationInternalPointerDragTransitionAllowed": pointer_drag_transition,
            "windowLifecycleMutationAllowed": lifecycle_mutation,
            "windowLifecycleSequenceAllowed": lifecycle_sequence,
            "windowLifecycleSequenceTransitionAllowed": lifecycle_sequence_transition,
            "windowLifecycleTransitionAllowed": lifecycle_transition,
            "windowCloseRequestAllowed": close_mutation || close_transition,
            "windowCloseTransitionAllowed": close_transition,
            "terminalClosedObservationObservable": close_transition,
            "windowStateReadAllowed": state_read,
            "windowStateWaitAllowed": state_wait,
            "applicationSurfaceScreenshotAllowed": screenshot,
            "windowActivationRequestAllowed": activation || activation_transition,
            "windowActivationTransitionAllowed": activation_transition,
            "finalLifecycleStateObservable": false,
            "frameworkStateConditionObservable": state_wait || lifecycle_sequence_transition || lifecycle_transition,
            "semanticPostconditionObservable": semantic_transition || key_sequence_transition || key_transition || input_sequence_transition || pointer_click_sequence_transition || pointer_click_transition || pointer_move_transition || pointer_move_sequence_transition || pointer_sequence_transition || pointer_drag_transition,
            "compositorFinalLifecycleStateObservable": false,
            }),
        ) {
            evidence.extend(additional_evidence);
        }
        let same_connection_focus_observation = if activation || activation_transition {
            Value::Bool(true)
        } else {
            Value::Null
        };
        let mut constraints = json!({
            "scope": "exact-cooperative-uix-window",
            "readOnly": !mutation,
            "noFallback": true,
            "supportedElementWait": if element_wait {
                json!({
                    "selectorSemantics": "exact-and",
                    "conditionKinds": ["unique", "missing"],
                    "sameAuthenticatedConnectionUsed": true,
                    "semanticRevisionWaitUsed": true,
                    "providerPolling": false,
                    "stabilitySemantics": false
                })
            } else {
                Value::Null
            },
            "supportedElementTransition": if semantic_transition {
                json!({
                    "sourceScope": "snapshot-scoped-element",
                    "selectorSemantics": "exact-and",
                    "conditionKinds": ["unique", "missing"],
                    "sameAuthenticatedConnectionUsed": true,
                    "semanticRevisionWaitUsed": true,
                    "providerPolling": false,
                    "causalityConfirmed": false,
                    "finalUIStateConfirmed": false,
                    "desktopInputInjected": false,
                    "foregroundActivationRequested": false,
                    "nativeIdentityExposed": false,
                    "transportIdentityExposed": false,
                    "x11Used": false,
                    "fallback": "none"
                })
            } else {
                Value::Null
            },
            "supportedLifecycleTransition": if lifecycle_transition {
                json!({
                    "actions": ["restore", "minimize", "maximize", "resize"],
                    "conditionKinds": ["visibility", "focus", "client-size", "window-flags"],
                    "sameAuthenticatedConnectionUsed": true,
                    "providerPolling": true,
                    "waitProtocolUsed": false,
                    "compositorFinalStateConfirmed": false
                })
            } else {
                Value::Null
            },
            "supportedLifecycleActions": if lifecycle_mutation {
                json!(["restore", "minimize", "maximize", "resize"])
            } else {
                Value::Null
            },
            "supportedWindowCloseTransition": supported_window_close_transition,
            "supportedWindowActivationTransition": supported_window_activation_transition,
            "unavailableLifecycleActions": if lifecycle_mutation {
                json!(["move"])
            } else {
                Value::Null
            },
            "supportedLifecycleSequence": if lifecycle_sequence {
                json!({
                    "actionKinds": ["restore", "minimize", "maximize", "resize"],
                    "requiredDistinctActionKinds": 2,
                    "maximumActions": 16,
                    "maximumPlannedDurationMs": 5000,
                    "sameConnectionRevisionChain": true,
                    "arbitraryWindowMoveSupported": false,
                    "transactionSemantics": false,
                    "rollbackSemantics": false,
                    "compositorFinalStateConfirmed": false
                })
            } else {
                Value::Null
            },
            "supportedLifecycleSequenceTransition": supported_lifecycle_sequence_transition,
            "supportedKeyPhases": if key_mutation {
                json!(["press"])
            } else {
                Value::Null
            },
            "unavailableKeyFeatures": if key_mutation {
                json!(["down", "up", "hold", "repeat", "text"])
            } else {
                Value::Null
            },
            "supportedKeySequence": if key_sequence {
                json!({
                    "completePressesOnly": true,
                    "maximumPresses": 64,
                    "maximumPlannedDurationMs": 5000,
                    "requestScopedBalancedPresses": true
                })
            } else {
                Value::Null
            },
            "supportedKeySequenceTransition": supported_key_sequence_transition,
            "supportedKeyTransition": supported_key_transition,
            "independentKeyOwnership": if key_sequence || key_sequence_transition || key_transition {
                json!(false)
            } else {
                Value::Null
            },
            "supportedInputSequence": if input_sequence {
                json!({
                    "stepKinds": ["press", "move", "click"],
                    "requiresKeyAndPointer": true,
                    "maximumSteps": 64,
                    "maximumPlannedDurationMs": 5000,
                    "sameConnectionRevisionChain": true,
                    "transactionSemantics": false,
                    "rollbackSemantics": false
                })
            } else {
                Value::Null
            },
            "supportedInputSequenceTransition": supported_input_sequence_transition,
        });
        if let (Some(constraints), Value::Object(additional_constraints)) = (
            constraints.as_object_mut(),
            json!({
            "supportedPointerActions": if pointer_mutation {
                json!(["move", "click"])
            } else {
                Value::Null
            },
            "supportedPointerClickSequence": if pointer_click_sequence {
                json!({
                    "ordinaryLeftClicksOnly": true,
                    "maximumClicks": 64,
                    "maximumPlannedDurationMs": 5000,
                    "sameConnectionRevisionChain": true,
                    "doubleClickSemantics": false,
                    "clickCountSemantics": false
                })
            } else {
                Value::Null
            },
            "supportedPointerClickSequenceTransition": supported_pointer_click_sequence_transition,
            "supportedPointerClickTransition": supported_pointer_click_transition,
            "supportedPointerMoveTransition": supported_pointer_move_transition,
            "supportedPointerMoveSequenceTransition": supported_pointer_move_sequence_transition,
            "supportedPointerSequenceTransition": supported_pointer_sequence_transition,
            "supportedPointerDragTransition": supported_pointer_drag_transition,
            "supportedPointerMoveSequence": if pointer_move_sequence {
                json!({
                    "minimumMoves": 2,
                    "maximumMoves": 64,
                    "maximumPlannedDurationMs": 5000,
                    "sameConnectionRevisionChain": true,
                    "interpolationSemantics": false,
                    "desktopPointerMoved": false
                })
            } else {
                Value::Null
            },
            "supportedPointerSequence": if pointer_sequence {
                json!({
                    "stepKinds": ["move", "click"],
                    "requiresMoveAndClick": true,
                    "ordinaryLeftClicksOnly": true,
                    "maximumSteps": 64,
                    "maximumPlannedDurationMs": 5000,
                    "sameConnectionRevisionChain": true,
                    "doubleClickSemantics": false,
                    "independentButtonOwnership": false
                })
            } else {
                Value::Null
            },
            "unavailablePointerFeatures": if pointer_mutation {
                json!(["pointer-down", "pointer-up", "drag", "scroll", "double-click", "right-click", "middle-click"])
            } else {
                Value::Null
            },
            "supportedPointerDrag": if pointer_drag {
                json!({
                    "button": "left",
                    "coordinateSpace": "client-logical-px",
                    "requestScopedBalancedButtons": true,
                    "maximumSamples": 64
                })
            } else {
                Value::Null
            },
            "independentPointerButtonOwnership": if pointer_drag {
                json!(false)
            } else {
                Value::Null
            },
            "closeFinalStateCapability": if close_mutation {
                json!(capabilities::WINDOW_CLOSED_WAIT_V2)
            } else if close_transition {
                json!(capabilities::WINDOW_CLOSE_TRANSITION)
            } else {
                Value::Null
            },
            "closeFinalStateClaimed": if close_mutation {
                json!(false)
            } else if close_transition {
                json!(true)
            } else {
                Value::Null
            },
            "stateObservationSource": if state_read {
                json!("uix-framework-current")
            } else {
                Value::Null
            },
            "supportedStateWait": supported_state_wait,
            "compositorFinalStateConfirmed": if state_read || state_wait {
                json!(false)
            } else {
                Value::Null
            },
            "captureSource": if screenshot {
                json!("uix-application-surface")
            } else {
                Value::Null
            },
            "generationRevalidatedAfterCapture": if screenshot {
                json!(true)
            } else {
                Value::Null
            },
            "activationRequestDoesNotGuaranteeFocus": if activation || activation_transition {
                Value::Bool(true)
            } else {
                Value::Null
            },
            "sameConnectionFocusObservation": if activation || activation_transition {
                same_connection_focus_observation
            } else {
                Value::Null
            },
            }),
        ) {
            constraints.extend(additional_constraints);
        }
        return Ok(json!({
            "ok": true,
            "contractVersion": "act/control/v1",
            "capability": capability,
            "targetId": session_id,
            "decision": if mutation { "confirmation-required" } else { "executable-background" },
            "executionRealm": if lifecycle_mutation || lifecycle_sequence || lifecycle_sequence_transition || lifecycle_transition || activation || activation_transition { "host-foreground" } else { "same-session-no-focus" },
            "requiresConfirmation": mutation,
            "requiresForegroundConsent": lifecycle_mutation || lifecycle_sequence || lifecycle_sequence_transition || lifecycle_transition || activation || activation_transition,
            "reasons": ["authenticated-opt-in-uix-agent-window-is-current"],
            "constraints": constraints,
            "evidence": evidence,
        }));
    }
    if parsed.kind() == OpaqueTargetKind::Window
        && matches!(
            capability,
            capabilities::WINDOW_DISCOVER_V2
                | capabilities::WINDOW_METADATA_READ_V2
                | capabilities::ACCESSIBILITY_TREE_READ_V2
        )
    {
        return Ok(json!({
            "ok": true,
            "contractVersion": "act/control/v1",
            "capability": capability,
            "targetId": session_id,
            "decision": "unavailable",
            "executionRealm": "none",
            "requiresConfirmation": false,
            "requiresForegroundConsent": false,
            "reasons": ["atspi-v2-private-fixture-verified-live-host-not-authorized"],
            "constraints": {
                "scope": "partial-accessibility-exporters-read-only",
                "readOnly": true,
                "noFallback": true,
            },
            "evidence": {
                "targetKind": "accessibility-exporter-window",
                "platform": "linux",
                "implementationState": "candidate-no-production-dispatch",
                "foregroundActivationAllowed": false,
                "inputAllowed": false,
            },
        }));
    }
    if parsed.kind() == OpaqueTargetKind::Media
        && matches!(
            capability,
            capabilities::MEDIA_PLAYBACK_STATE_READ_V2 | capabilities::MEDIA_PLAYBACK_CONTROL_V2
        )
    {
        return Ok(media_candidate_assessment(capability, session_id));
    }
    if matches!(
        capability,
        capabilities::MEDIA_PLAYBACK_STATE_READ_V3 | capabilities::MEDIA_PLAYBACK_CONTROL_V3
    ) {
        return crate::modules::media_playback::assess(capability, session_id);
    }
    if parsed.kind() == OpaqueTargetKind::Application
        && capability == capabilities::APPLICATION_OPEN_V2
    {
        return application_launch::assess(session_id);
    }
    if parsed.kind() != OpaqueTargetKind::Process {
        return Ok(assessment(
            capability,
            session_id,
            "unsupported",
            "none",
            "target-kind-has-no-linux-provider",
            "unavailable-no-linux-provider",
        ));
    }
    if capability == capabilities::PROCESS_TERMINATE_GRACEFUL_V2 {
        return process_termination::assess(session_id);
    }
    if capability == capabilities::PROCESS_TERMINATE_FORCE_V2 {
        return process_termination::assess_force(session_id);
    }
    let inventory = procfs::snapshot(usize::MAX).map_err(snapshot_error)?;
    let matches = inventory
        .records
        .iter()
        .filter(|process| process.session_id == session_id)
        .count();
    if matches == 0 {
        return Err(AppControlError::new(
            "STALE_SESSION",
            "The running process target no longer exists.",
        ));
    }
    if matches > 1 {
        return Err(AppControlError::new(
            "AMBIGUOUS_TARGET",
            "The opaque process session matched more than one process.",
        ));
    }
    if capability == capabilities::PROCESS_METADATA_READ {
        return Ok(assessment(
            capability,
            session_id,
            "executable-background",
            "host-headless",
            "read-only-certified-linux-provider-available",
            "linux-procfs-available",
        ));
    }
    Ok(assessment(
        capability,
        session_id,
        "unavailable",
        "none",
        "exact-target-has-no-certified-linux-route",
        "unavailable-no-linux-provider",
    ))
}

fn assess_host(capability: &str, session_id: &str) -> AppResult<Value> {
    if session_id != linux_host_identity::current_host_target() {
        return Err(AppControlError::new(
            "STALE_SESSION",
            "The Linux host target is not current.",
        ));
    }
    if matches!(
        capability,
        capabilities::APPLICATION_DISCOVER_V2 | capabilities::APPLICATION_DISCOVER_V3
    ) {
        let launch_status_published = capability == capabilities::APPLICATION_DISCOVER_V3;
        return Ok(json!({
            "ok": true,
            "contractVersion": "act/control/v1",
            "capability": capability,
            "targetId": session_id,
            "decision": "executable-background",
            "executionRealm": "host-headless",
            "requiresConfirmation": false,
            "requiresForegroundConsent": false,
            "reasons": [if launch_status_published {
                "read-only-certified-linux-xdg-procfs-and-toolkit-fixture-launch-status"
            } else {
                "read-only-certified-linux-xdg-and-procfs-providers-available"
            }],
            "constraints": {
                "scope": "exact-current-host",
                "readOnly": true,
                "launchStatusPublished": launch_status_published,
                "noFallback": true,
            },
            "evidence": {
                "targetKind": "host",
                "platform": "linux",
                "implementationState": if launch_status_published {
                    "linux-xdg-procfs-toolkit-fixture-launch-status-available"
                } else {
                    "linux-xdg-desktop-entry-procfs-available"
                },
                "foregroundActivationAllowed": false,
                "inputAllowed": false,
            },
        }));
    }
    if capability == capabilities::APPLICATION_SESSION_DISCOVER_V2 {
        return Ok(json!({
            "ok": true,
            "contractVersion": "act/control/v1",
            "capability": capability,
            "targetId": session_id,
            "decision": "executable-background",
            "executionRealm": "host-headless",
            "requiresConfirmation": false,
            "requiresForegroundConsent": false,
            "reasons": ["read-only-certified-linux-xdg-and-procfs-session-aggregation"],
            "constraints": {
                "scope": "exact-current-host",
                "readOnly": true,
                "noFallback": true,
            },
            "evidence": {
                "targetKind": "host",
                "platform": "linux",
                "implementationState": "linux-xdg-procfs-unrelated-session-aggregation-available",
                "foregroundActivationAllowed": false,
                "inputAllowed": false,
            },
        }));
    }
    if matches!(
        capability,
        capabilities::APPLICATION_SESSION_DISCOVER_V3
            | capabilities::APPLICATION_SESSION_DISCOVER_V4
    ) {
        let launch_status_published = capability == capabilities::APPLICATION_SESSION_DISCOVER_V4;
        return Ok(json!({
            "ok": true,
            "contractVersion": "act/control/v1",
            "capability": capability,
            "targetId": session_id,
            "decision": "executable-background",
            "executionRealm": "same-session-no-focus",
            "requiresConfirmation": false,
            "requiresForegroundConsent": false,
            "reasons": [if launch_status_published {
                "read-only-certified-linux-launch-aware-xdg-procfs-and-uix-session-aggregation"
            } else {
                "read-only-certified-linux-xdg-procfs-and-uix-session-aggregation"
            }],
            "constraints": {
                "scope": if launch_status_published {
                    "exact-current-host-toolkit-self-executable-fixture-and-opt-in-uix-agent-applications"
                } else {
                    "exact-current-host-and-opt-in-uix-agent-applications"
                },
                "readOnly": true,
                "globalWindowDirectoryClaimed": false,
                "noFallback": true,
            },
            "evidence": {
                "targetKind": "host",
                "platform": "linux",
                "implementationState": if launch_status_published {
                    "linux-launch-aware-uix-session-aggregation-available"
                } else {
                    "linux-uix-aware-session-aggregation-available"
                },
                "foregroundActivationAllowed": false,
                "inputAllowed": false,
                "x11Used": false,
            },
        }));
    }
    if capability == capabilities::WINDOW_DISCOVER_V2 {
        return Ok(json!({
            "ok": true,
            "contractVersion": "act/control/v1",
            "capability": capability,
            "targetId": session_id,
            "decision": "unavailable",
            "executionRealm": "none",
            "requiresConfirmation": false,
            "requiresForegroundConsent": false,
            "reasons": ["atspi-v2-private-fixture-verified-live-host-not-authorized"],
            "constraints": {
                "scope": "partial-accessibility-exporters-read-only",
                "readOnly": true,
                "noFallback": true,
            },
            "evidence": {
                "targetKind": "host",
                "platform": "linux",
                "implementationState": "candidate-no-production-dispatch",
                "foregroundActivationAllowed": false,
                "inputAllowed": false,
            },
        }));
    }
    if capability == capabilities::WINDOW_DISCOVER_V3 {
        return Ok(json!({
            "ok": true,
            "contractVersion": "act/control/v1",
            "capability": capability,
            "targetId": session_id,
            "decision": "executable-background",
            "executionRealm": "same-session-no-focus",
            "requiresConfirmation": false,
            "requiresForegroundConsent": false,
            "reasons": ["cooperative-uix-agent-discovery-provider-available"],
            "constraints": {
                "scope": "opt-in-uix-agent-applications-only",
                "readOnly": true,
                "noFallback": true,
            },
            "evidence": {
                "targetKind": "host",
                "platform": "linux",
                "implementationState": "production-route-wayland-live-verified",
                "foregroundActivationAllowed": false,
                "inputAllowed": false,
            },
        }));
    }
    if capability == capabilities::MEDIA_SESSION_DISCOVER_V2 {
        return Ok(media_candidate_assessment(capability, session_id));
    }
    let portal = wayland_portal::probe();
    let screenshot_ready = portal.wayland_socket_count > 0
        && portal.user_bus_socket_present
        && portal.user_bus_reachable
        && portal.portal_service_registered
        && portal
            .screenshot_version
            .is_some_and(|version| version >= 2);
    let desktop_session_ready = portal.wayland_socket_count > 0
        && portal.user_bus_socket_present
        && portal.user_bus_reachable
        && portal.portal_service_registered
        && portal
            .remote_desktop_version
            .is_some_and(|version| version >= 2)
        && portal
            .screen_cast_version
            .is_some_and(|version| version >= 5);
    if capability == capabilities::DESKTOP_SESSION_OPEN && desktop_session_ready {
        return Ok(json!({
            "ok": true,
            "contractVersion": "act/control/v1",
            "capability": capability,
            "targetId": session_id,
            "decision": "unavailable",
            "executionRealm": "none",
            "requiresConfirmation": true,
            "requiresForegroundConsent": true,
            "reasons": ["production-desktop-session-live-acceptance-pending"],
            "constraints": {
                "scope": "exact-current-host",
                "launcher": "ai-computer-toolkit session-host desktop",
                "transport": "json-lines-stdio",
                "readOnly": false,
                "strictIsolationSupported": false,
                "noFallback": true,
            },
            "evidence": {
                "targetKind": "host",
                "platform": "linux",
                "implementationState": "production-route-live-acceptance-pending-not-advertised",
                "remoteDesktopVersion": portal.remote_desktop_version,
                "screenCastVersion": portal.screen_cast_version,
                "foregroundActivationAllowed": false,
                "inputAllowed": false,
                "restoreTokenRetained": false,
            },
        }));
    }
    if capability == capabilities::DESKTOP_SCREENSHOT_INTERACTIVE && screenshot_ready {
        return Ok(json!({
            "ok": true,
            "contractVersion": "act/control/v1",
            "capability": capability,
            "targetId": session_id,
            "decision": "foreground-consent-required",
            "executionRealm": "host-foreground",
            "requiresConfirmation": true,
            "requiresForegroundConsent": true,
            "reasons": ["visible-system-portal-selection-and-screen-read"],
            "constraints": {
                "scope": "exact-current-host",
                "readOnly": false,
                "noFallback": true,
            },
            "evidence": {
                "targetKind": "host",
                "platform": "linux",
                "implementationState": "ready-awaiting-owner-authorization",
                "portalInterface": "org.freedesktop.portal.Screenshot",
                "portalVersion": portal.screenshot_version,
                "foregroundActivationAllowed": true,
                "inputAllowed": false,
            },
        }));
    }
    Ok(json!({
        "ok": true,
        "contractVersion": "act/control/v1",
        "capability": capability,
        "targetId": session_id,
        "decision": "unavailable",
        "executionRealm": "none",
        "requiresConfirmation": matches!(capability, capabilities::DESKTOP_SCREENSHOT_INTERACTIVE | capabilities::DESKTOP_SESSION_OPEN),
        "requiresForegroundConsent": matches!(capability, capabilities::DESKTOP_SCREENSHOT_INTERACTIVE | capabilities::DESKTOP_SESSION_OPEN),
        "reasons": [if capability == capabilities::DESKTOP_SCREENSHOT_INTERACTIVE {
            "wayland-screenshot-portal-not-ready"
        } else if capability == capabilities::DESKTOP_SESSION_OPEN {
            "wayland-remote-desktop-or-screen-cast-portal-not-ready"
        } else {
            "exact-host-target-has-no-certified-linux-route"
        }],
        "constraints": { "scope": "exact-current-host", "readOnly": false, "noFallback": true },
        "evidence": {
            "targetKind": "host",
            "platform": "linux",
            "implementationState": "unavailable-no-linux-provider",
            "foregroundActivationAllowed": false,
            "inputAllowed": false,
        },
    }))
}

fn media_candidate_assessment(capability: &str, session_id: &str) -> Value {
    json!({
        "ok": true,
        "contractVersion": "act/control/v1",
        "capability": capability,
        "targetId": session_id,
        "decision": "unavailable",
        "executionRealm": "none",
        "requiresConfirmation": capability == capabilities::MEDIA_PLAYBACK_CONTROL_V2,
        "requiresForegroundConsent": false,
        "reasons": ["mpris-v2-private-fixture-verified-live-session-not-authorized"],
        "constraints": {"scope":"exact-current-private-broker-generation", "noFallback":true},
        "evidence": {
            "targetKind": if capability == capabilities::MEDIA_SESSION_DISCOVER_V2 {"host"} else {"media-session"},
            "platform":"linux",
            "implementationState":"candidate-no-production-dispatch",
            "automaticRetryAllowed":false,
        },
    })
}

fn assessment(
    capability: &str,
    session_id: &str,
    decision: &str,
    execution_realm: &str,
    reason: &str,
    implementation_state: &str,
) -> Value {
    json!({
        "ok": true,
        "contractVersion": "act/control/v1",
        "capability": capability,
        "targetId": session_id,
        "decision": decision,
        "executionRealm": execution_realm,
        "requiresConfirmation": false,
        "requiresForegroundConsent": false,
        "reasons": [reason],
        "constraints": {
            "scope": "exact-running-process",
            "readOnly": true,
            "noFallback": true,
        },
        "evidence": {
            "targetKind": "running-process",
            "platform": "linux",
            "implementationState": implementation_state,
            "foregroundActivationAllowed": false,
            "inputAllowed": false,
        },
    })
}

#[cfg(test)]
#[path = "linux_capability_assessment_tests.rs"]
mod tests;
