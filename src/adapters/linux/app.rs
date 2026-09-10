//! Linux UIX 协作式应用 facade Adapter。

use serde_json::{Value, json};

use crate::{
    adapters::AppAdapter,
    adapters::uix_app_descriptor::descriptor,
    capabilities,
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    domain::{AppControlError, AppResult, CommandRequest, IsolationRequirement},
    modules::{
        application_launch, uix_action, uix_close, uix_closed_wait, uix_element_location,
        uix_element_transition, uix_element_wait, uix_input_sequence,
        uix_input_sequence_transition, uix_key_input, uix_key_sequence,
        uix_key_sequence_transition, uix_key_transition, uix_lifecycle, uix_lifecycle_sequence,
        uix_lifecycle_sequence_transition, uix_lifecycle_transition, uix_pointer_click_sequence,
        uix_pointer_click_sequence_transition, uix_pointer_click_transition, uix_pointer_drag,
        uix_pointer_drag_transition, uix_pointer_input, uix_pointer_move_sequence,
        uix_pointer_move_sequence_transition, uix_pointer_move_transition, uix_pointer_sequence,
        uix_pointer_sequence_transition, uix_state_wait, uix_window, uix_window_activation,
        uix_window_activation_transition, uix_window_close_transition, uix_window_screenshot,
    },
};

use super::uix_agent;

/// 只路由已认证 UIX Agent 精确窗口 capability 的应用 facade。
pub struct AppFacadeAdapter;

fn required_session_id(request: &CommandRequest) -> AppResult<&str> {
    request
        .target
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppControlError::new("INVALID_ARGUMENT", "target.sessionId is required."))
}

fn reject_strict_isolation(request: &CommandRequest) -> AppResult<()> {
    if request.isolation_requirement == IsolationRequirement::Strict {
        return Err(AppControlError::with_details(
            "ISOLATION_REQUIRED",
            "UIX Agent requests execute in the application session.",
            json!({
                "platform": "linux",
                "provider": "uix-agent-v1",
                "executionRealm": "same-session-no-focus",
                "fallback": "none",
            }),
        ));
    }
    Ok(())
}

impl AppAdapter for AppFacadeAdapter {
    fn app_id(&self) -> &'static str {
        "app"
    }

    fn status(&self) -> AppResult<Value> {
        Ok(json!({
            "ok": true,
            "app": "app",
            "platform": "linux",
            "provider": "uix-agent-v1-and-xdg-toolkit-fixture-v1-and-mpris-v3",
            "providerState": "available-routes-target-assessment-required",
            "connected": false,
            "metadataRead": false,
            "executionDomain": "same-session-no-focus",
            "executionDomains": ["same-session-no-focus", "host-foreground", "isolated-worker"],
            "readOnly": false,
            "fallback": "none",
            "capabilities": [
                capabilities::MEDIA_SESSION_DISCOVER_V3,
                capabilities::MEDIA_PLAYBACK_STATE_READ_V3,
                capabilities::MEDIA_PLAYBACK_CONTROL_V3,
                capabilities::APPLICATION_OPEN_V2,
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
                capabilities::UI_INPUT_POINTER_DRAG,
                capabilities::UI_INPUT_POINTER_DRAG_TRANSITION,
                capabilities::WINDOW_REVISION_WAIT,
                capabilities::WINDOW_CLOSED_WAIT_V2,
                capabilities::WINDOW_CLOSE_V2,
                capabilities::WINDOW_CLOSE_TRANSITION,
                capabilities::WINDOW_LIFECYCLE_V2,
                capabilities::WINDOW_LIFECYCLE_SEQUENCE,
                capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION,
                capabilities::WINDOW_LIFECYCLE_TRANSITION,
                capabilities::WINDOW_STATE_READ,
                capabilities::WINDOW_STATE_WAIT,
                capabilities::WINDOW_SCREENSHOT_V2,
                capabilities::WINDOW_ACTIVATE,
                capabilities::WINDOW_ACTIVATE_TRANSITION,
            ],
        }))
    }

    fn sessions(&self, request: &CommandRequest) -> AppResult<Value> {
        reject_strict_isolation(request)?;
        if !request.target.is_empty() || !request.args.is_empty() {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "UIX app sessions accept only --max-items and --pretty.",
            ));
        }
        let inventory = uix_agent::discover(request.max_items).map_err(uix_window::public_error)?;
        let base_descriptors = [
            descriptor(capabilities::UI_ELEMENT_LOCATE_V2),
            descriptor(capabilities::UI_ELEMENT_WAIT_V2),
            descriptor(capabilities::UI_ELEMENT_ACTION_V2),
            descriptor(capabilities::UI_ELEMENT_TRANSITION),
            descriptor(capabilities::UI_INPUT_KEY_V2),
            descriptor(capabilities::UI_INPUT_KEY_SEQUENCE),
            descriptor(capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION),
            descriptor(capabilities::UI_INPUT_KEY_TRANSITION),
            descriptor(capabilities::UI_INPUT_SEQUENCE),
            descriptor(capabilities::UI_INPUT_SEQUENCE_TRANSITION),
            descriptor(capabilities::UI_INPUT_POINTER_V2),
            descriptor(capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE),
            descriptor(capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION),
            descriptor(capabilities::UI_INPUT_POINTER_CLICK_TRANSITION),
            descriptor(capabilities::UI_INPUT_POINTER_MOVE_TRANSITION),
            descriptor(capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE),
            descriptor(capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION),
            descriptor(capabilities::UI_INPUT_POINTER_SEQUENCE),
            descriptor(capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION),
            descriptor(capabilities::WINDOW_REVISION_WAIT),
            descriptor(capabilities::WINDOW_CLOSED_WAIT_V2),
            descriptor(capabilities::WINDOW_CLOSE_V2),
            descriptor(capabilities::WINDOW_CLOSE_TRANSITION),
            descriptor(capabilities::WINDOW_LIFECYCLE_V2),
            descriptor(capabilities::WINDOW_LIFECYCLE_SEQUENCE),
        ];
        let sessions = inventory
            .windows
            .iter()
            .map(|window| {
                let mut descriptors = base_descriptors.to_vec();
                if window.state.is_some() {
                    descriptors.push(descriptor(capabilities::WINDOW_STATE_READ));
                }
                if window.state.is_some() && window.focused.is_some() {
                    descriptors.push(descriptor(capabilities::WINDOW_STATE_WAIT));
                    descriptors.push(descriptor(
                        capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION,
                    ));
                    descriptors.push(descriptor(capabilities::WINDOW_LIFECYCLE_TRANSITION));
                }
                if window.screenshot_supported {
                    descriptors.push(descriptor(capabilities::WINDOW_SCREENSHOT_V2));
                }
                if window.activation_supported {
                    descriptors.push(descriptor(capabilities::WINDOW_ACTIVATE));
                    if window.focused.is_some() {
                        descriptors.push(descriptor(capabilities::WINDOW_ACTIVATE_TRANSITION));
                    }
                }
                if window.pointer_drag_supported {
                    descriptors.push(descriptor(capabilities::UI_INPUT_POINTER_DRAG));
                    descriptors.push(descriptor(capabilities::UI_INPUT_POINTER_DRAG_TRANSITION));
                }
                json!({
                    "sessionId": window.session_id,
                    "targetKind": "uix-agent-window",
                    "title": window.title,
                    "visible": window.visible,
                    "presentable": window.presentable,
                    "revision": window.revision,
                    "presentedRevision": window.presented_revision,
                    "capabilities": descriptors,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "ok": true,
            "app": "app",
            "provider": "uix-agent-v1",
            "coverage": "opt-in-uix-agent-applications",
            "count": sessions.len(),
            "total": inventory.total,
            "complete": inventory.complete,
            "warnings": inventory.warnings,
            "sessions": sessions,
        }))
    }

    fn inspect(&self, request: &CommandRequest) -> AppResult<Value> {
        reject_strict_isolation(request)?;
        if request.target.len() != 1 || !request.args.is_empty() {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "Linux app inspect accepts only target.sessionId.",
            ));
        }
        let session_id = required_session_id(request)?;
        match OpaqueTargetId::parse(session_id).map(|target| target.kind()) {
            Some(OpaqueTargetKind::Application) => application_launch::inspect(session_id),
            Some(OpaqueTargetKind::Window) => uix_window::metadata(session_id),
            _ => Err(AppControlError::new(
                "STALE_SESSION",
                "The Linux app facade target is not a current application or UIX window.",
            )),
        }
    }

    fn run(&self, request: &CommandRequest) -> AppResult<Value> {
        if request
            .args
            .get("capability")
            .and_then(Value::as_str)
            .is_some_and(crate::modules::media_playback::is_capability)
        {
            return crate::modules::media_playback::perform(request);
        }
        // mutation 确认必须先于其余请求字段；只读 revision wait 不借用确认。
        if request.operation.as_deref() != Some("read") && !request.confirmed {
            return Err(AppControlError::new(
                "CONFIRMATION_REQUIRED",
                "Linux app mutation requires explicit confirmation.",
            ));
        }
        let capability = request.args.get("capability").and_then(Value::as_str);
        if request.operation.as_deref() == Some("create")
            && capability == Some(capabilities::APPLICATION_OPEN_V2)
            && !request.foreground_consent
        {
            return Err(AppControlError::new(
                "FOREGROUND_CONSENT_REQUIRED",
                "Linux application launch requires upfront foreground-impact consent.",
            ));
        }
        reject_strict_isolation(request)?;
        match request.operation.as_deref() {
            Some("create") => {
                if capability != Some(capabilities::APPLICATION_OPEN_V2)
                    || request.target.len() != 1
                    || request.args.len() != 2
                {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "Linux app create accepts application.open@2, one sessionId, and one empty input object.",
                    ));
                }
                application_launch::perform(
                    request.target.get("sessionId").and_then(Value::as_str),
                    request.confirmed,
                    request.foreground_consent,
                    request.args.get("input"),
                )
            }
            Some("apply") => {
                if matches!(
                    capability,
                    Some(capabilities::WINDOW_LIFECYCLE_V2)
                        | Some(capabilities::WINDOW_LIFECYCLE_SEQUENCE)
                        | Some(capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION)
                        | Some(capabilities::WINDOW_LIFECYCLE_TRANSITION)
                        | Some(capabilities::WINDOW_ACTIVATE)
                        | Some(capabilities::WINDOW_ACTIVATE_TRANSITION)
                ) && !request.foreground_consent
                {
                    return Err(AppControlError::new(
                        "FOREGROUND_CONSENT_REQUIRED",
                        "The selected UIX window operation requires upfront foreground-impact consent.",
                    ));
                }
                if matches!(
                    capability,
                    Some(capabilities::UI_ELEMENT_ACTION_V2)
                        | Some(capabilities::UI_ELEMENT_TRANSITION)
                        | Some(capabilities::UI_INPUT_KEY_V2)
                        | Some(capabilities::UI_INPUT_KEY_SEQUENCE)
                        | Some(capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION)
                        | Some(capabilities::UI_INPUT_KEY_TRANSITION)
                        | Some(capabilities::UI_INPUT_SEQUENCE)
                        | Some(capabilities::UI_INPUT_SEQUENCE_TRANSITION)
                        | Some(capabilities::UI_INPUT_POINTER_V2)
                        | Some(capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE)
                        | Some(capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION)
                        | Some(capabilities::UI_INPUT_POINTER_CLICK_TRANSITION)
                        | Some(capabilities::UI_INPUT_POINTER_MOVE_TRANSITION)
                        | Some(capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE)
                        | Some(capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION)
                        | Some(capabilities::UI_INPUT_POINTER_SEQUENCE)
                        | Some(capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION)
                        | Some(capabilities::UI_INPUT_POINTER_DRAG)
                        | Some(capabilities::UI_INPUT_POINTER_DRAG_TRANSITION)
                ) && request.foreground_consent
                {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "UIX same-session mutations do not accept foreground consent.",
                    ));
                }
                if request.target.len() != 1
                    || request.args.len() != 2
                    || !matches!(
                        capability,
                        Some(capabilities::UI_ELEMENT_ACTION_V2)
                            | Some(capabilities::UI_ELEMENT_TRANSITION)
                            | Some(capabilities::UI_INPUT_KEY_V2)
                            | Some(capabilities::UI_INPUT_KEY_SEQUENCE)
                            | Some(capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION)
                            | Some(capabilities::UI_INPUT_KEY_TRANSITION)
                            | Some(capabilities::UI_INPUT_SEQUENCE)
                            | Some(capabilities::UI_INPUT_SEQUENCE_TRANSITION)
                            | Some(capabilities::UI_INPUT_POINTER_V2)
                            | Some(capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE)
                            | Some(capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION)
                            | Some(capabilities::UI_INPUT_POINTER_CLICK_TRANSITION)
                            | Some(capabilities::UI_INPUT_POINTER_MOVE_TRANSITION)
                            | Some(capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE)
                            | Some(capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION)
                            | Some(capabilities::UI_INPUT_POINTER_SEQUENCE)
                            | Some(capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION)
                            | Some(capabilities::UI_INPUT_POINTER_DRAG)
                            | Some(capabilities::UI_INPUT_POINTER_DRAG_TRANSITION)
                            | Some(capabilities::WINDOW_LIFECYCLE_V2)
                            | Some(capabilities::WINDOW_LIFECYCLE_SEQUENCE)
                            | Some(capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION)
                            | Some(capabilities::WINDOW_LIFECYCLE_TRANSITION)
                            | Some(capabilities::WINDOW_ACTIVATE)
                            | Some(capabilities::WINDOW_ACTIVATE_TRANSITION)
                    )
                {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "Linux UIX app apply accepts ui.element.action@2, ui.element.transition@1, ui.input.key@2, ui.input.key.sequence@1, ui.input.key.sequence.transition@1, ui.input.key.transition@1, ui.input.sequence@1, ui.input.sequence.transition@1, ui.input.pointer@2, ui.input.pointer.click.sequence@1, ui.input.pointer.click.sequence.transition@1, ui.input.pointer.click.transition@1, ui.input.pointer.move.transition@1, ui.input.pointer.move.sequence@1, ui.input.pointer.move.sequence.transition@1, ui.input.pointer.sequence@1, ui.input.pointer.sequence.transition@1, ui.input.pointer.drag@1, ui.input.pointer.drag.transition@1, window.lifecycle@2, window.lifecycle.sequence@1, window.lifecycle.sequence.transition@1, window.lifecycle.transition@1, window.activate@1, or window.activate.transition@1 with one sessionId and one input object.",
                    ));
                }
                let session_id = required_session_id(request)?;
                let input = request
                    .args
                    .get("input")
                    .filter(|value| value.is_object())
                    .ok_or_else(|| {
                        AppControlError::new("INVALID_ARGUMENT", "args.input is required.")
                    })?;
                match capability {
                    Some(capabilities::UI_ELEMENT_ACTION_V2) => {
                        let data = uix_action::perform(session_id, request.confirmed, input)?;
                        Ok(response(
                            "apply",
                            capabilities::UI_ELEMENT_ACTION_V2,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact snapshot element",
                            json!({ "activationRequested": false }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_ELEMENT_TRANSITION) => {
                        let data =
                            uix_element_transition::perform(session_id, request.confirmed, input)?;
                        Ok(response(
                            "apply",
                            capabilities::UI_ELEMENT_TRANSITION,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "semanticPostconditionObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_KEY_V2) => {
                        let data = uix_key_input::perform(session_id, request.confirmed, input)?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_KEY_V2,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({ "activationRequested": false }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_KEY_SEQUENCE) => {
                        let data = uix_key_sequence::perform(session_id, request.confirmed, input)?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_KEY_SEQUENCE,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({ "activationRequested": false }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION) => {
                        let data = uix_key_sequence_transition::perform(
                            session_id,
                            request.confirmed,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "semanticPostconditionObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_KEY_TRANSITION) => {
                        let data =
                            uix_key_transition::perform(session_id, request.confirmed, input)?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_KEY_TRANSITION,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "semanticPostconditionObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_SEQUENCE) => {
                        let data =
                            uix_input_sequence::perform(session_id, request.confirmed, input)?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_SEQUENCE,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({ "activationRequested": false }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_SEQUENCE_TRANSITION) => {
                        let data = uix_input_sequence_transition::perform(
                            session_id,
                            request.confirmed,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_SEQUENCE_TRANSITION,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "semanticPostconditionObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_POINTER_V2) => {
                        let data =
                            uix_pointer_input::perform(session_id, request.confirmed, input)?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_POINTER_V2,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({ "activationRequested": false }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE) => {
                        let data = uix_pointer_click_sequence::perform(
                            session_id,
                            request.confirmed,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({ "activationRequested": false }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION) => {
                        let data = uix_pointer_click_sequence_transition::perform(
                            session_id,
                            request.confirmed,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "semanticPostconditionObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_POINTER_CLICK_TRANSITION) => {
                        let data = uix_pointer_click_transition::perform(
                            session_id,
                            request.confirmed,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_POINTER_CLICK_TRANSITION,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "semanticPostconditionObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_POINTER_MOVE_TRANSITION) => {
                        let data = uix_pointer_move_transition::perform(
                            session_id,
                            request.confirmed,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_POINTER_MOVE_TRANSITION,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "semanticPostconditionObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE) => {
                        let data = uix_pointer_move_sequence::perform(
                            session_id,
                            request.confirmed,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({ "activationRequested": false }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION) => {
                        let data = uix_pointer_move_sequence_transition::perform(
                            session_id,
                            request.confirmed,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE_TRANSITION,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "semanticPostconditionObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_POINTER_SEQUENCE) => {
                        let data =
                            uix_pointer_sequence::perform(session_id, request.confirmed, input)?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_POINTER_SEQUENCE,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({ "activationRequested": false }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION) => {
                        let data = uix_pointer_sequence_transition::perform(
                            session_id,
                            request.confirmed,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "semanticPostconditionObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_POINTER_DRAG) => {
                        let data = uix_pointer_drag::perform(session_id, request.confirmed, input)?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_POINTER_DRAG,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({ "activationRequested": false }),
                            data,
                        ))
                    }
                    Some(capabilities::UI_INPUT_POINTER_DRAG_TRANSITION) => {
                        let data = uix_pointer_drag_transition::perform(
                            session_id,
                            request.confirmed,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::UI_INPUT_POINTER_DRAG_TRANSITION,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "semanticPostconditionObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::WINDOW_LIFECYCLE_V2) => {
                        let data = uix_lifecycle::perform(
                            session_id,
                            request.confirmed,
                            request.foreground_consent,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::WINDOW_LIFECYCLE_V2,
                            session_id,
                            "host-foreground",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "visibleMutationAuthorized": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::WINDOW_LIFECYCLE_SEQUENCE) => {
                        let data = uix_lifecycle_sequence::perform(
                            session_id,
                            request.confirmed,
                            request.foreground_consent,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::WINDOW_LIFECYCLE_SEQUENCE,
                            session_id,
                            "host-foreground",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "visibleMutationAuthorized": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION) => {
                        let data = uix_lifecycle_sequence_transition::perform(
                            session_id,
                            request.confirmed,
                            request.foreground_consent,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::WINDOW_LIFECYCLE_SEQUENCE_TRANSITION,
                            session_id,
                            "host-foreground",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "visibleMutationAuthorized": true,
                                "frameworkConditionObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::WINDOW_LIFECYCLE_TRANSITION) => {
                        let data = uix_lifecycle_transition::perform(
                            session_id,
                            request.confirmed,
                            request.foreground_consent,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::WINDOW_LIFECYCLE_TRANSITION,
                            session_id,
                            "host-foreground",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "visibleMutationAuthorized": true,
                                "frameworkConditionObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::WINDOW_ACTIVATE) => {
                        let data = uix_window_activation::perform(
                            session_id,
                            request.confirmed,
                            request.foreground_consent,
                            input,
                        )?;
                        let focus_confirmed = data["focusConfirmed"].as_bool() == Some(true);
                        Ok(response(
                            "apply",
                            capabilities::WINDOW_ACTIVATE,
                            session_id,
                            "host-foreground",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": true,
                                "focusConfirmed": focus_confirmed,
                            }),
                            data,
                        ))
                    }
                    Some(capabilities::WINDOW_ACTIVATE_TRANSITION) => {
                        let data = uix_window_activation_transition::perform(
                            session_id,
                            request.confirmed,
                            request.foreground_consent,
                            input,
                        )?;
                        Ok(response(
                            "apply",
                            capabilities::WINDOW_ACTIVATE_TRANSITION,
                            session_id,
                            "host-foreground",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": true,
                                "focusObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    _ => Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "The selected UIX mutation received incompatible foreground consent.",
                    )),
                }
            }
            Some("read") => {
                let capability = request.args.get("capability").and_then(Value::as_str);
                if request.confirmed
                    || request.foreground_consent
                    || request.target.len() != 1
                    || request.args.len() != 2
                    || !matches!(
                        capability,
                        Some(capabilities::UI_ELEMENT_LOCATE_V2)
                            | Some(capabilities::UI_ELEMENT_WAIT_V2)
                            | Some(capabilities::WINDOW_REVISION_WAIT)
                            | Some(capabilities::WINDOW_CLOSED_WAIT_V2)
                            | Some(capabilities::WINDOW_STATE_READ)
                            | Some(capabilities::WINDOW_STATE_WAIT)
                    )
                {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "Linux UIX app read accepts ui.element.locate@2, ui.element.wait@2, window.revision.wait@1, window.closed.wait@2, window.state.read@1, or window.state.wait@1, one sessionId, and one input object.",
                    ));
                }
                let session_id = required_session_id(request)?;
                let input = request
                    .args
                    .get("input")
                    .filter(|value| value.is_object())
                    .ok_or_else(|| {
                        AppControlError::new("INVALID_ARGUMENT", "args.input is required.")
                    })?;
                let (capability, data) = match capability {
                    Some(capabilities::UI_ELEMENT_LOCATE_V2) => (
                        capabilities::UI_ELEMENT_LOCATE_V2,
                        uix_element_location::locate(session_id, input)?,
                    ),
                    Some(capabilities::UI_ELEMENT_WAIT_V2) => (
                        capabilities::UI_ELEMENT_WAIT_V2,
                        uix_element_wait::wait(session_id, input)?,
                    ),
                    Some(capabilities::WINDOW_REVISION_WAIT) => (
                        capabilities::WINDOW_REVISION_WAIT,
                        uix_window::wait_revision(session_id, input)?,
                    ),
                    Some(capabilities::WINDOW_CLOSED_WAIT_V2) => (
                        capabilities::WINDOW_CLOSED_WAIT_V2,
                        uix_closed_wait::wait_closed(session_id, input)?,
                    ),
                    Some(capabilities::WINDOW_STATE_READ) => (
                        capabilities::WINDOW_STATE_READ,
                        uix_window::state(session_id, input)?,
                    ),
                    Some(capabilities::WINDOW_STATE_WAIT) => (
                        capabilities::WINDOW_STATE_WAIT,
                        uix_state_wait::wait(session_id, input)?,
                    ),
                    _ => unreachable!("validated capability"),
                };
                Ok(response(
                    "read",
                    capability,
                    session_id,
                    "same-session-no-focus",
                    "opaque exact window generation",
                    json!({ "activationRequested": false }),
                    data,
                ))
            }
            Some("screenshot") => {
                if request.foreground_consent
                    || request.target.len() != 1
                    || request.args.len() != 2
                    || request.args.get("capability").and_then(Value::as_str)
                        != Some(capabilities::WINDOW_SCREENSHOT_V2)
                {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "Linux UIX app screenshot accepts window.screenshot@2, one sessionId, one input object and no foreground consent.",
                    ));
                }
                let session_id = required_session_id(request)?;
                let input = request
                    .args
                    .get("input")
                    .filter(|value| value.is_object())
                    .ok_or_else(|| {
                        AppControlError::new("INVALID_ARGUMENT", "args.input is required.")
                    })?;
                let data = uix_window_screenshot::screenshot(session_id, request.confirmed, input)?;
                Ok(response(
                    "screenshot",
                    capabilities::WINDOW_SCREENSHOT_V2,
                    session_id,
                    "same-session-no-focus",
                    "opaque exact window generation",
                    json!({
                        "activationRequested": false,
                        "captureSource": "application-surface",
                    }),
                    data,
                ))
            }
            Some("close") => {
                if request.foreground_consent
                    || request.target.len() != 1
                    || request.args.len() != 2
                    || !matches!(
                        request.args.get("capability").and_then(Value::as_str),
                        Some(capabilities::WINDOW_CLOSE_V2)
                            | Some(capabilities::WINDOW_CLOSE_TRANSITION)
                    )
                {
                    return Err(AppControlError::new(
                        "INVALID_ARGUMENT",
                        "Linux UIX app close accepts window.close@2 or window.close.transition@1, one sessionId, and one input object without foreground consent.",
                    ));
                }
                let session_id = required_session_id(request)?;
                let input = request
                    .args
                    .get("input")
                    .filter(|value| value.is_object())
                    .ok_or_else(|| {
                        AppControlError::new("INVALID_ARGUMENT", "args.input is required.")
                    })?;
                match request.args.get("capability").and_then(Value::as_str) {
                    Some(capabilities::WINDOW_CLOSE_V2) => {
                        let data = uix_close::close(session_id, request.confirmed, input)?;
                        Ok(response(
                            "close",
                            capabilities::WINDOW_CLOSE_V2,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({ "activationRequested": false }),
                            data,
                        ))
                    }
                    Some(capabilities::WINDOW_CLOSE_TRANSITION) => {
                        let data = uix_window_close_transition::close(
                            session_id,
                            request.confirmed,
                            input,
                        )?;
                        Ok(response(
                            "close",
                            capabilities::WINDOW_CLOSE_TRANSITION,
                            session_id,
                            "same-session-no-focus",
                            "opaque exact window generation",
                            json!({
                                "activationRequested": false,
                                "terminalClosedObservationRequested": true,
                            }),
                            data,
                        ))
                    }
                    _ => unreachable!("validated close capability"),
                }
            }
            _ => Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "Linux app run accepts create, apply, read, screenshot, or close.",
            )),
        }
    }
}

fn response(
    verb: &'static str,
    capability: &'static str,
    session_id: &str,
    execution_realm: &'static str,
    targeting: &'static str,
    foreground: Value,
    data: Value,
) -> Value {
    json!({
        "ok": true,
        "app": "app",
        "verb": verb,
        "capability": capability,
        "targetId": session_id,
        "executionRealm": execution_realm,
        "requiredExecutionRealm": execution_realm,
        "executionRealmCertified": true,
        "isolationRequirement": "standard",
        "hostImpactPolicy": "background-preferred",
        "data": data,
        "meta": {
            "foreground": foreground,
            "targeting": targeting,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Verb;

    #[test]
    fn status_is_local_and_unconfirmed_run_fails_before_fields() {
        let status = AppFacadeAdapter.status().expect("status must be local");
        assert_eq!(status["connected"], false);
        assert_eq!(status["metadataRead"], false);
        let request = CommandRequest::read(Verb::Run, "app");
        let error = AppFacadeAdapter
            .run(&request)
            .expect_err("confirmation must be first");
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn lifecycle_foreground_consent_precedes_target_and_input() {
        let mut request = CommandRequest::read(Verb::Run, "app");
        request.operation = Some("apply".to_owned());
        request.confirmed = true;
        request.args.insert(
            "capability".to_owned(),
            json!(capabilities::WINDOW_LIFECYCLE_V2),
        );
        let error = AppFacadeAdapter
            .run(&request)
            .expect_err("foreground consent must fail before target and input");
        assert_eq!(error.code, "FOREGROUND_CONSENT_REQUIRED");
    }

    #[test]
    fn state_read_descriptor_freezes_framework_current_observation_boundary() {
        let value = descriptor(capabilities::WINDOW_STATE_READ);
        assert_eq!(value["version"], 1);
        assert_eq!(
            value["constraints"]["observationSource"],
            "uix-framework-current"
        );
        assert_eq!(value["constraints"]["compositorFinalStateConfirmed"], false);
        assert_eq!(value["requiresConfirmation"], false);
        assert_eq!(value["requiresForegroundConsent"], false);
    }

    #[test]
    fn state_wait_descriptor_freezes_same_connection_polling_boundary() {
        let value = descriptor(capabilities::WINDOW_STATE_WAIT);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "read");
        assert_eq!(value["constraints"]["providerPolling"], "list_windows");
        assert_eq!(value["constraints"]["waitProtocolUsed"], false);
        assert_eq!(
            value["constraints"]["sameAuthenticatedConnectionUsed"],
            true
        );
        assert_eq!(value["constraints"]["compositorFinalStateConfirmed"], false);
        assert_eq!(value["requiresConfirmation"], false);
        assert_eq!(value["requiresForegroundConsent"], false);
    }

    #[test]
    fn screenshot_descriptor_freezes_negotiated_application_surface_boundary() {
        let value = descriptor(capabilities::WINDOW_SCREENSHOT_V2);
        assert_eq!(value["version"], 2);
        assert_eq!(value["verb"], "screenshot");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(
            value["constraints"]["requiredNegotiation"],
            json!(["screenshot", "max_screenshot_bytes", "max_response_bytes"])
        );
        assert_eq!(
            value["constraints"]["generationRevalidatedAfterCapture"],
            true
        );
        assert_eq!(value["constraints"]["globalWindowCaptureClaimed"], false);
    }

    #[test]
    fn activation_descriptor_freezes_foreground_request_without_focus_guarantee() {
        let value = descriptor(capabilities::WINDOW_ACTIVATE);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "host-foreground");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], true);
        assert_eq!(
            value["constraints"]["requiredWindowAction"],
            "activate_window"
        );
        assert_eq!(
            value["constraints"]["requestAcceptedDoesNotGuaranteeFocus"],
            true
        );
        assert_eq!(value["constraints"]["globalWindowActivationClaimed"], false);
    }

    #[test]
    fn lifecycle_sequence_descriptor_freezes_same_connection_non_transaction_boundary() {
        let value = descriptor(capabilities::WINDOW_LIFECYCLE_SEQUENCE);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "host-foreground");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], true);
        assert_eq!(value["constraints"]["requiredDistinctActionKinds"], 2);
        assert_eq!(value["constraints"]["maximumActions"], 16);
        assert_eq!(value["constraints"]["sameConnectionRevisionChain"], true);
        assert_eq!(value["constraints"]["transactionSemantics"], false);
        assert_eq!(value["constraints"]["rollbackSemantics"], false);
        assert_eq!(value["constraints"]["compositorFinalStateConfirmed"], false);
    }

    #[test]
    fn pointer_drag_descriptor_freezes_request_scoped_balanced_left_button() {
        let value = descriptor(capabilities::UI_INPUT_POINTER_DRAG);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(value["constraints"]["button"], "left");
        assert_eq!(value["constraints"]["requestScopedBalancedButtons"], true);
        assert_eq!(
            value["constraints"]["independentButtonOwnershipPublished"],
            false
        );
    }

    #[test]
    fn pointer_drag_transition_descriptor_requires_release_before_semantic_wait() {
        let value = descriptor(capabilities::UI_INPUT_POINTER_DRAG_TRANSITION);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(
            value["constraints"]["postconditionRequiresConfirmedRelease"],
            true
        );
        assert_eq!(value["constraints"]["requestScopedBalancedButtons"], true);
        assert_eq!(value["constraints"]["semanticRevisionWaitUsed"], true);
        assert_eq!(value["constraints"]["causalityConfirmed"], false);
        assert_eq!(value["constraints"]["desktopPointerMoved"], false);
    }

    #[test]
    fn key_sequence_descriptor_freezes_complete_request_scoped_presses() {
        let value = descriptor(capabilities::UI_INPUT_KEY_SEQUENCE);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(value["constraints"]["maximumPresses"], 64);
        assert_eq!(
            value["constraints"]["operationScope"],
            "request-scoped-complete-press-sequence"
        );
        assert_eq!(
            value["constraints"]["independentKeyOwnershipPublished"],
            false
        );
    }

    #[test]
    fn key_sequence_transition_descriptor_requires_complete_sequence_before_wait() {
        let value = descriptor(capabilities::UI_INPUT_KEY_SEQUENCE_TRANSITION);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(
            value["constraints"]["completeSequenceRequiredBeforePostcondition"],
            true
        );
        assert_eq!(value["constraints"]["semanticRevisionWaitUsed"], true);
        assert_eq!(value["constraints"]["transactionSemantics"], false);
        assert_eq!(value["constraints"]["causalityConfirmed"], false);
        assert_eq!(
            value["constraints"]["independentKeyOwnershipPublished"],
            false
        );
    }

    #[test]
    fn input_sequence_descriptor_requires_cross_modal_same_connection_steps() {
        let value = descriptor(capabilities::UI_INPUT_SEQUENCE);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(value["constraints"]["requiresKeyAndPointer"], true);
        assert_eq!(value["constraints"]["sameConnectionRevisionChain"], true);
        assert_eq!(value["constraints"]["transactionSemantics"], false);
        assert_eq!(value["constraints"]["rollbackSemantics"], false);
    }

    #[test]
    fn input_sequence_transition_descriptor_requires_complete_sequence_before_wait() {
        let value = descriptor(capabilities::UI_INPUT_SEQUENCE_TRANSITION);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(value["constraints"]["requiresKeyAndPointer"], true);
        assert_eq!(
            value["constraints"]["completeSequenceRequiredBeforePostcondition"],
            true
        );
        assert_eq!(value["constraints"]["semanticRevisionWaitUsed"], true);
        assert_eq!(value["constraints"]["transactionSemantics"], false);
        assert_eq!(value["constraints"]["causalityConfirmed"], false);
    }

    #[test]
    fn pointer_click_sequence_descriptor_excludes_double_click_semantics() {
        let value = descriptor(capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(value["constraints"]["requiredWindowAction"], "click_at");
        assert_eq!(value["constraints"]["maximumClicks"], 64);
        assert_eq!(value["constraints"]["doubleClickSemantics"], false);
        assert_eq!(value["constraints"]["clickCountSemantics"], false);
    }

    #[test]
    fn pointer_click_sequence_transition_descriptor_waits_after_complete_sequence() {
        let value = descriptor(capabilities::UI_INPUT_POINTER_CLICK_SEQUENCE_TRANSITION);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(
            value["constraints"]["completeSequenceRequiredBeforePostcondition"],
            true
        );
        assert_eq!(value["constraints"]["semanticRevisionWaitUsed"], true);
        assert_eq!(value["constraints"]["transactionSemantics"], false);
        assert_eq!(value["constraints"]["causalityConfirmed"], false);
        assert_eq!(value["constraints"]["desktopPointerMoved"], false);
        assert_eq!(value["constraints"]["doubleClickSemantics"], false);
    }

    #[test]
    fn pointer_move_transition_descriptor_freezes_hover_observation_without_desktop_pointer() {
        let value = descriptor(capabilities::UI_INPUT_POINTER_MOVE_TRANSITION);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(value["constraints"]["requiredWindowAction"], "pointer_move");
        assert_eq!(value["constraints"]["semanticRevisionWaitUsed"], true);
        assert_eq!(value["constraints"]["causalityConfirmed"], false);
        assert_eq!(value["constraints"]["desktopPointerMoved"], false);
        assert_eq!(value["constraints"]["clickSemantics"], false);
        assert_eq!(value["constraints"]["interpolationSemantics"], false);
    }

    #[test]
    fn pointer_sequence_descriptor_requires_move_and_ordinary_click() {
        let value = descriptor(capabilities::UI_INPUT_POINTER_SEQUENCE);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(value["constraints"]["requiresMoveAndClick"], true);
        assert_eq!(value["constraints"]["maximumSteps"], 64);
        assert_eq!(value["constraints"]["doubleClickSemantics"], false);
        assert_eq!(
            value["constraints"]["independentButtonOwnershipPublished"],
            false
        );
    }

    #[test]
    fn pointer_sequence_transition_descriptor_requires_complete_sequence_before_wait() {
        let value = descriptor(capabilities::UI_INPUT_POINTER_SEQUENCE_TRANSITION);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(
            value["constraints"]["completeSequenceRequiredBeforePostcondition"],
            true
        );
        assert_eq!(value["constraints"]["sameConnectionRevisionChain"], true);
        assert_eq!(value["constraints"]["semanticRevisionWaitUsed"], true);
        assert_eq!(value["constraints"]["causalityConfirmed"], false);
        assert_eq!(value["constraints"]["desktopPointerMoved"], false);
    }

    #[test]
    fn pointer_move_sequence_descriptor_excludes_click_and_interpolation_semantics() {
        let value = descriptor(capabilities::UI_INPUT_POINTER_MOVE_SEQUENCE);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(value["constraints"]["requiredWindowAction"], "pointer_move");
        assert_eq!(value["constraints"]["minimumMoves"], 2);
        assert_eq!(value["constraints"]["maximumMoves"], 64);
        assert_eq!(value["constraints"]["interpolationSemantics"], false);
        assert_eq!(value["constraints"]["desktopPointerMoved"], false);
    }

    #[test]
    fn element_location_descriptor_freezes_logical_geometry_without_host_hit_point() {
        let value = descriptor(capabilities::UI_ELEMENT_LOCATE_V2);
        assert_eq!(value["version"], 2);
        assert_eq!(value["verb"], "read");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(
            value["constraints"]["coordinateSpace"],
            "application-client"
        );
        assert_eq!(value["constraints"]["coordinateUnit"], "logical-px");
        assert_eq!(
            value["constraints"]["hostCoordinateMappingPublished"],
            false
        );
        assert_eq!(value["constraints"]["clickPointPublished"], false);
        assert_eq!(value["requiresConfirmation"], false);
        assert_eq!(value["requiresForegroundConsent"], false);
    }

    #[test]
    fn element_wait_descriptor_freezes_semantic_revision_wait_without_polling() {
        let value = descriptor(capabilities::UI_ELEMENT_WAIT_V2);
        assert_eq!(value["version"], 2);
        assert_eq!(value["verb"], "read");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(
            value["constraints"]["conditionKinds"],
            json!(["unique", "missing"])
        );
        assert_eq!(
            value["constraints"]["sameAuthenticatedConnectionUsed"],
            true
        );
        assert_eq!(value["constraints"]["semanticRevisionWaitUsed"], true);
        assert_eq!(value["constraints"]["providerPolling"], false);
        assert_eq!(value["constraints"]["stabilitySemantics"], false);
        assert_eq!(value["requiresConfirmation"], false);
        assert_eq!(value["requiresForegroundConsent"], false);
    }

    #[test]
    fn element_transition_descriptor_freezes_same_session_postcondition_route() {
        let value = descriptor(capabilities::UI_ELEMENT_TRANSITION);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(
            value["constraints"]["sourceScope"],
            "snapshot-scoped-element"
        );
        assert_eq!(value["constraints"]["selectorSemantics"], "exact-and");
        assert_eq!(
            value["constraints"]["conditionKinds"],
            json!(["unique", "missing"])
        );
        assert_eq!(
            value["constraints"]["sameAuthenticatedConnectionUsed"],
            true
        );
        assert_eq!(value["constraints"]["semanticRevisionWaitUsed"], true);
        assert_eq!(value["constraints"]["providerPolling"], false);
        assert_eq!(value["constraints"]["causalityConfirmed"], false);
        assert_eq!(value["constraints"]["finalUIStateConfirmed"], false);
        assert_eq!(value["constraints"]["desktopInputInjected"], false);
        assert_eq!(value["constraints"]["foregroundActivationRequested"], false);
        assert_eq!(value["constraints"]["nativeIdentityExposed"], false);
        assert_eq!(value["constraints"]["noFallback"], true);
    }

    #[test]
    fn close_transition_descriptor_freezes_same_connection_terminal_observation_route() {
        let value = descriptor(capabilities::WINDOW_CLOSE_TRANSITION);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "close");
        assert_eq!(value["executionRealm"], "same-session-no-focus");
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], false);
        assert_eq!(value["constraints"]["target"], "exact-window-generation");
        assert_eq!(value["constraints"]["requiredWindowAction"], "close_window");
        assert_eq!(
            value["constraints"]["sameAuthenticatedConnectionUsed"],
            true
        );
        assert_eq!(value["constraints"]["waitProtocolUsed"], true);
        assert_eq!(value["constraints"]["providerPolling"], false);
        assert_eq!(value["constraints"]["terminalReplyRequired"], true);
        assert_eq!(
            value["constraints"]["connectionCloseAcceptedAsProof"],
            false
        );
        assert_eq!(value["constraints"]["causalityConfirmed"], false);
        assert_eq!(value["constraints"]["applicationClosedConfirmed"], false);
        assert_eq!(value["constraints"]["foregroundActivationRequested"], false);
        assert_eq!(value["constraints"]["desktopInputInjected"], false);
        assert_eq!(value["constraints"]["nativeIdentityExposed"], false);
        assert_eq!(value["constraints"]["transportIdentityExposed"], false);
        assert_eq!(value["constraints"]["x11Used"], false);
        assert_eq!(value["constraints"]["fallback"], "none");
    }

    #[test]
    fn lifecycle_transition_descriptor_freezes_same_connection_framework_wait() {
        let value = descriptor(capabilities::WINDOW_LIFECYCLE_TRANSITION);
        assert_eq!(value["version"], 1);
        assert_eq!(value["verb"], "apply");
        assert_eq!(value["executionRealm"], "host-foreground");
        assert_eq!(
            value["constraints"]["sameAuthenticatedConnectionUsed"],
            true
        );
        assert_eq!(value["constraints"]["providerPolling"], true);
        assert_eq!(value["constraints"]["waitProtocolUsed"], false);
        assert_eq!(value["constraints"]["compositorFinalStateConfirmed"], false);
        assert_eq!(value["requiresConfirmation"], true);
        assert_eq!(value["requiresForegroundConsent"], true);
    }
}
