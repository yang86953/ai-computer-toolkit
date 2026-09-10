//! Linux direct surface 的统一 Policy；只冻结公开 catalog、确认顺序与执行域。

use serde_json::{Value, json};

use crate::{
    capabilities::{self, CapabilitySurface},
    catalog::{self, FieldDescriptor},
    domain::{
        AppControlError, AppResult, CommandRequest, ExecutionRealm, HostImpactPolicy,
        IsolationRequirement, Verb,
    },
};

// MPRIS 候选 Policy 只冻结 feature 内部契约，不进入默认 Service 或 AdapterRegistry。
#[cfg(feature = "linux-mpris-candidate")]
pub(crate) mod mpris_candidate;

/// Linux System 在 provider 前冻结的执行计划。
#[derive(Clone, Copy, Debug)]
pub(crate) struct ExecutionPlan {
    required_realm: ExecutionRealm,
    runtime_realm: ExecutionRealm,
    host_impact_policy: HostImpactPolicy,
    isolation_requirement: IsolationRequirement,
}

impl ExecutionPlan {
    /// 附加不可由 provider 放宽的执行证明，并拒绝自报域冲突。
    pub(crate) fn attest_result(self, result: &mut Value) -> AppResult<()> {
        let object = result.as_object_mut().ok_or_else(|| {
            AppControlError::new(
                "OPERATION_FAILED",
                "The provider returned a non-object execution result.",
            )
        })?;
        let runtime_realm = json!(self.runtime_realm);
        if object
            .get("executionRealm")
            .is_some_and(|value| value != &runtime_realm)
        {
            return Err(AppControlError::new(
                "OPERATION_FAILED",
                "The provider execution realm conflicts with the frozen policy plan.",
            ));
        }
        object.insert("executionRealm".to_owned(), runtime_realm);
        object.insert(
            "requiredExecutionRealm".to_owned(),
            json!(self.required_realm),
        );
        object.insert(
            "isolationRequirement".to_owned(),
            json!(self.isolation_requirement),
        );
        object.insert(
            "hostImpactPolicy".to_owned(),
            json!(self.host_impact_policy),
        );
        object.insert(
            "executionRealmCertified".to_owned(),
            Value::Bool(self.required_realm == self.runtime_realm),
        );
        Ok(())
    }
}

/// 在确认、target、input 和 provider 访问之间执行 direct surface 门禁。
pub fn validate(request: &CommandRequest) -> AppResult<()> {
    let descriptor = catalog::app(&request.app).ok_or_else(|| {
        AppControlError::new(
            "CAPABILITY_GAP",
            "The requested surface is not in the public capability catalog.",
        )
    })?;
    if !descriptor.public_verbs.contains(&request.verb) {
        return Err(AppControlError::new(
            "CAPABILITY_GAP",
            "The requested verb is not public for this surface.",
        ));
    }
    if request.verb != Verb::Run {
        return Ok(());
    }
    let operation_id = request
        .operation
        .as_deref()
        .ok_or_else(|| AppControlError::new("INVALID_ARGUMENT", "run requires an operation."))?;
    let operation = catalog::operation(&request.app, operation_id).ok_or_else(|| {
        AppControlError::new(
            "CAPABILITY_GAP",
            "The requested operation is not in the public capability catalog.",
        )
    })?;
    // mutation 确认先于 target 与 input 字段检查。
    if operation.requires_confirmation && !request.confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "State-changing operations require explicit confirmation.",
        ));
    }
    if !matches!(
        operation.background_policy,
        "guaranteed" | "best-effort" | "prefer-background-then-consent"
    ) {
        return Err(AppControlError::new(
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The Linux direct operation has no certified background policy.",
        ));
    }
    let capability_id = capability_id_for_operation(request, operation)?;
    let definition =
        capabilities::definition_for_surface(surface_for_app(&request.app)?, capability_id)
            .ok_or_else(|| {
                AppControlError::new(
                    "CAPABILITY_UNSUPPORTED",
                    "The catalog operation has no matching versioned capability definition.",
                )
            })?;
    let action_conflicts =
        request.app == "app" && definition.action.as_str() != operation.operation;
    if action_conflicts || definition.action.mutates() != operation.mutates {
        return Err(AppControlError::new(
            "OPERATION_FAILED",
            "The capability registry conflicts with the public operation catalog.",
        ));
    }
    if definition.requires_upfront_foreground_consent && !request.foreground_consent {
        return Err(AppControlError::new(
            "FOREGROUND_CONSENT_REQUIRED",
            "The selected Linux operation requires upfront foreground-impact consent.",
        ));
    }
    if !definition.requires_foreground_consent && request.foreground_consent {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "This Linux background operation does not accept foreground consent.",
        ));
    }
    if request.isolation_requirement == IsolationRequirement::Strict
        && !definition.execution_realm.permits_strict_isolation()
    {
        return Err(AppControlError::with_details(
            "ISOLATION_REQUIRED",
            "Strict isolation permits only certified host-headless execution on this Linux route.",
            json!({
                "requiredExecutionRealm": definition.execution_realm,
                "foregroundConsentIgnored": request.foreground_consent,
            }),
        ));
    }
    if request.app == "app" && request.target.contains_key("interactiveSessionId") {
        return Err(AppControlError::with_details(
            "ISOLATION_REQUIRED",
            "Linux App capabilities do not accept an independent interactive-session target.",
            json!({
                "platform": "linux",
                "executionRealm": "none",
                "fallback": "none",
            }),
        ));
    }
    if request.app != "app" && definition.execution_realm != operation.execution_realm {
        return Err(AppControlError::new(
            "OPERATION_FAILED",
            "The capability registry conflicts with the public operation catalog.",
        ));
    }
    validate_fields(&request.target, operation.target_fields, "target")?;
    validate_fields(&request.args, operation.argument_fields, "args")?;
    if request.app == "app"
        && crate::components::media_playback_contract::is_capability(capability_id)
    {
        // 封闭媒体 input 与精确目标也在 System 的 dispatch hook 之前验证。
        crate::components::media_playback_contract::parse(request)?;
    }
    Ok(())
}

/// 为已通过门禁的 direct operation 生成不可变执行计划。
pub(crate) fn execution_plan(request: &CommandRequest) -> AppResult<Option<ExecutionPlan>> {
    if request.verb != Verb::Run {
        return Ok(None);
    }
    let operation_id = request
        .operation
        .as_deref()
        .ok_or_else(|| AppControlError::new("INVALID_ARGUMENT", "run requires an operation."))?;
    let operation = catalog::operation(&request.app, operation_id).ok_or_else(|| {
        AppControlError::new(
            "CAPABILITY_GAP",
            "The validated operation is no longer public.",
        )
    })?;
    let capability_id = capability_id_for_operation(request, operation)?;
    let definition =
        capabilities::definition_for_surface(surface_for_app(&request.app)?, capability_id)
            .ok_or_else(|| {
                AppControlError::new(
                    "CAPABILITY_UNSUPPORTED",
                    "The validated operation has no matching capability definition.",
                )
            })?;
    Ok(Some(ExecutionPlan {
        required_realm: definition.execution_realm,
        runtime_realm: definition.execution_realm,
        host_impact_policy: request.isolation_requirement.into(),
        isolation_requirement: request.isolation_requirement,
    }))
}

fn surface_for_app(app: &str) -> AppResult<CapabilitySurface> {
    match app {
        "app" => Ok(CapabilitySurface::App),
        "process" => Ok(CapabilitySurface::Process),
        _ => Err(AppControlError::new(
            "CAPABILITY_UNSUPPORTED",
            "This Linux direct surface has no Policy registry mapping.",
        )),
    }
}

fn capability_id_for_operation<'a>(
    request: &'a CommandRequest,
    operation: &'a catalog::OperationDescriptor,
) -> AppResult<&'a str> {
    if request.app != "app" {
        return Ok(operation.id);
    }
    let capability = request
        .args
        .get("capability")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppControlError::new(
                "INVALID_ARGUMENT",
                "args.capability is required for the app facade.",
            )
        })?;
    if !matches!(
        capability,
        capabilities::APPLICATION_OPEN_V2
            | capabilities::MEDIA_SESSION_DISCOVER_V3
            | capabilities::MEDIA_PLAYBACK_STATE_READ_V3
            | capabilities::MEDIA_PLAYBACK_CONTROL_V3
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
    ) {
        return Err(AppControlError::new(
            "CAPABILITY_UNAVAILABLE",
            "The selected app capability has no certified Linux route.",
        ));
    }
    Ok(capability)
}

fn validate_fields(
    values: &serde_json::Map<String, Value>,
    fields: &[FieldDescriptor],
    group: &str,
) -> AppResult<()> {
    for field in fields {
        let Some(value) = values.get(field.name) else {
            if field.required {
                return Err(AppControlError::new(
                    "INVALID_ARGUMENT",
                    format!("{group}.{} is required.", field.name),
                ));
            }
            continue;
        };
        if field.required && !required_value_present(value) {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                format!("{group}.{} is required.", field.name),
            ));
        }
        if !field.value_type.accepts_type(value) || !field.value_type.satisfies_constraint(value) {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                format!(
                    "{group}.{} must be {}.",
                    field.name,
                    field.value_type.as_str()
                ),
            ));
        }
    }
    if values
        .keys()
        .any(|name| !fields.iter().any(|field| field.name == name))
    {
        return Err(AppControlError::with_details(
            "INVALID_ARGUMENT",
            format!("{group} contains fields outside the public operation catalog."),
            json!({
                "fieldGroup": group,
                "allowedFields": fields.iter().map(|field| field.name).collect::<Vec<_>>(),
            }),
        ));
    }
    Ok(())
}

fn required_value_present(value: &Value) -> bool {
    match value {
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Null => false,
        Value::Bool(_) | Value::Number(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_mutation_confirmation_precedes_target_and_input() {
        let mut request = CommandRequest::read(Verb::Run, "process");
        request.operation = Some("terminate-graceful".to_owned());
        let error = validate(&request)
            .err()
            .unwrap_or_else(|| panic!("未确认 mutation 必须失败"));
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn application_launch_policy_preserves_confirmation_consent_and_isolation_order() {
        let mut request = CommandRequest::read(Verb::Run, "app");
        request.operation = Some("create".to_owned());
        let error = validate(&request)
            .err()
            .unwrap_or_else(|| panic!("未确认应用启动必须失败"));
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");

        request.confirmed = true;
        request.args.insert(
            "capability".to_owned(),
            Value::String(capabilities::APPLICATION_OPEN_V2.to_owned()),
        );
        let error = validate(&request)
            .err()
            .unwrap_or_else(|| panic!("缺少前景同意必须先于 target 与 input 失败"));
        assert_eq!(error.code, "FOREGROUND_CONSENT_REQUIRED");

        request.foreground_consent = true;
        request.isolation_requirement = IsolationRequirement::Strict;
        let error = validate(&request)
            .err()
            .unwrap_or_else(|| panic!("严格隔离必须先于 target 与 input 失败"));
        assert_eq!(error.code, "ISOLATION_REQUIRED");
    }
}
