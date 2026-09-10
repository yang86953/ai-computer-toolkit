//! 任务授权与严格非图像后台准入；不执行 provider，也不从请求帧建立授权。

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use crate::{
    capabilities::{self, CapabilityDefinition, CapabilitySurface},
    domain::{AppControlError, AppResult, CommandRequest, IsolationRequirement, Verb},
};

use super::protocol::{GRANT_CONTRACT, TaskGrant, valid_task_id};

pub(super) fn validate_grant(grant: &TaskGrant) -> AppResult<()> {
    if grant.contract_version != GRANT_CONTRACT
        || !valid_task_id(&grant.task_id)
        || !(1..=1_920_000).contains(&grant.total_timeout_ms)
        || !(1..=64).contains(&grant.max_executions)
        || grant.permissions.len() > 64
    {
        return Err(error(
            "TASK_GRANT_INVALID",
            "The startup task grant violates its bounded contract.",
        ));
    }
    let mut targets = HashSet::new();
    for permission in &grant.permissions {
        let definition = definition(&permission.capability)?;
        if definition.action == capabilities::CapabilityAction::Discover {
            return Err(error(
                "TASK_GRANT_INVALID",
                "Discovery uses allowDiscovery, not an exact-target permission.",
            ));
        }
        if !permission.target_id.starts_with("s2:")
            || permission.target_id.len() > 128
            || !permission
                .target_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b':')
            || !targets.insert((&permission.capability, &permission.target_id))
        {
            return Err(error(
                "TASK_GRANT_INVALID",
                "Task permissions require unique exact opaque targets.",
            ));
        }
    }
    // 枚举值只能从受信任的启动输入取得；普通 operation 没有对应字段。
    match grant.authorization_source {
        super::protocol::AuthorizationSource::UserTask => Ok(()),
    }
}

pub(super) fn definition(capability: &str) -> AppResult<&'static CapabilityDefinition> {
    capabilities::definition_for_surface(CapabilitySurface::App, capability).ok_or_else(|| {
        error(
            "INVALID_ARGUMENT",
            "The capability is not registered on the application facade.",
        )
    })
}

pub(super) fn check_scope(
    grant: &TaskGrant,
    capability: &str,
    target_id: &str,
    input: Option<&Map<String, Value>>,
) -> AppResult<()> {
    let permission = grant
        .permissions
        .iter()
        .find(|permission| permission.capability == capability && permission.target_id == target_id)
        .ok_or_else(|| {
            error(
                "TASK_SCOPE_VIOLATION",
                "The exact capability and target are outside this task grant.",
            )
        })?;
    if let Some(input) = input
        && !permission.required_input.iter().all(|(key, expected)| {
            input
                .get(key)
                .is_some_and(|actual| contains_required(actual, expected))
        })
    {
        return Err(error(
            "TASK_SCOPE_VIOLATION",
            "Materialized input does not satisfy the task constraints.",
        ));
    }
    Ok(())
}

fn contains_required(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => expected.iter().all(|(key, value)| {
            actual
                .get(key)
                .is_some_and(|actual| contains_required(actual, value))
        }),
        _ => actual == expected,
    }
}

pub(super) fn requirements() -> Value {
    json!({
        "background": "required",
        "minimizedOperation": "required-or-window-independent",
        "imageDependency": "forbidden",
        "hostInput": "forbidden",
        "hostClipboardWrite": "forbidden",
        "foregroundActivation": "forbidden",
        "foregroundFallback": "forbidden",
        "perActionHumanInteraction": "forbidden"
    })
}

/// 未认证的条件必须保持 unknown，不能从执行域名称推断最小化支持。
pub(super) fn guarantees(definition: &CapabilityDefinition) -> Value {
    let supported = certified_route(definition);
    json!({
        "background": if supported { "supported" } else { "unknown" },
        "minimizedOperation": if supported { "window-independent" } else { "unknown" },
        "imageIndependent": if supported { "supported" } else { "unknown" },
        "noHostInput": if supported { "supported" } else { "unknown" },
        "noHostClipboardWrite": if supported { "supported" } else { "unknown" },
        "noForegroundActivation": if supported { "supported" } else { "unknown" },
        "noPerActionHumanInteraction": if supported { "supported" } else { "unknown" },
        "targetScope": if supported && cfg!(target_os = "linux") { "current-user-mpris-media-sessions" }
            else if supported { "tool-owned-isolated-browser-session" } else { "not-certified" },
        "routeEligible": supported,
        "assessmentRequired": true,
        "eligible": false
    })
}

fn certified_route(definition: &CapabilityDefinition) -> bool {
    if cfg!(target_os = "linux")
        && matches!(
            definition.id,
            capabilities::MEDIA_SESSION_DISCOVER_V3
                | capabilities::MEDIA_PLAYBACK_STATE_READ_V3
                | capabilities::MEDIA_PLAYBACK_CONTROL_V3
        )
    {
        return true;
    }
    // 该集合来自现有浏览器公开契约，不涵盖用户现有窗口，也不放行页面截图 Query。
    cfg!(target_os = "windows")
        && matches!(
            definition.id,
            capabilities::BROWSER_PAGE_NAVIGATE
                | capabilities::BROWSER_PAGE_WAIT
                | capabilities::BROWSER_PAGE_QUERY
                | capabilities::BROWSER_ELEMENT_CLICK
                | capabilities::BROWSER_ELEMENT_TYPE
        )
        && !definition.requires_foreground_consent
        && !definition.requires_upfront_foreground_consent
}

pub(super) fn authorize(
    grant: &TaskGrant,
    capability: &str,
    target_id: &str,
    input: &Map<String, Value>,
) -> AppResult<CommandRequest> {
    check_scope(grant, capability, target_id, Some(input))?;
    let definition = definition(capability)?;
    if !certified_route(definition) {
        return Err(AppControlError::with_details(
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "This capability has no certified image-independent minimized-safe route for the task host.",
            json!({ "requirements": requirements(), "guarantees": guarantees(definition), "fallback": "none" }),
        ));
    }
    // 确认布尔值仅为旧 facade 的内部兼容表示；它来源于完整任务授权和准入评估。
    let mut request = CommandRequest::read(Verb::Run, "app");
    request.operation = Some(definition.action.as_str().to_owned());
    request
        .target
        .insert("sessionId".to_owned(), json!(target_id));
    request
        .args
        .insert("capability".to_owned(), json!(capability));
    request
        .args
        .insert("input".to_owned(), Value::Object(input.clone()));
    request.confirmed = definition.action.mutates();
    request.foreground_consent = false;
    request.isolation_requirement = IsolationRequirement::Strict;
    Ok(request)
}

/// 只构造固定只读 capability；调用方必须先验证 discovery 或原目标 assessment 授权。
pub(super) fn read_request(capability: &str, target: Option<&str>) -> AppResult<CommandRequest> {
    let definition = definition(capability)?;
    if definition.action.mutates() || !certified_route(definition) {
        return Err(error(
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "This task host has no certified read-only route.",
        ));
    }
    let mut request = CommandRequest::read(Verb::Run, "app");
    request.operation = Some(definition.action.as_str().into());
    if let Some(target) = target {
        request.target.insert("sessionId".into(), json!(target));
    }
    request.args.insert("capability".into(), json!(capability));
    request.args.insert("input".into(), json!({}));
    request.isolation_requirement = IsolationRequirement::Strict;
    Ok(request)
}

pub(super) fn error(code: &'static str, message: &'static str) -> AppControlError {
    AppControlError::new(code, message)
}
