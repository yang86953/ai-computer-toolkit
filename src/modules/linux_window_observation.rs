//! Linux partial accessibility exporter 窗口观察 Module。

use serde_json::{Value, json};

use crate::{
    adapters::linux::atspi::{self, ConnectionConfig, Failure, WindowRecord},
    domain::{AppControlError, AppResult},
};

/// 通过显式私有 bus 配置执行候选发现。
pub(crate) async fn discover(config: &ConnectionConfig, maximum_items: usize) -> AppResult<Value> {
    let inventory = atspi::discover(config, maximum_items)
        .await
        .map_err(public_error)?;
    let windows = inventory
        .windows
        .iter()
        .map(public_window)
        .collect::<Vec<_>>();
    let reasons = inventory
        .reasons
        .iter()
        .map(|reason| reason.as_str())
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": true,
        "contractVersion": "act/window-observation/v2",
        "capability": "window.discover@2",
        "coverage": "partial-accessibility-exporters",
        "readOnly": true,
        "mutationAllowed": false,
        "visibilityEvidence": "accessibility-visible-and-showing",
        "count": windows.len(),
        "total": windows.len(),
        "truncated": !reasons.is_empty(),
        "truncationReasons": reasons,
        "windows": windows,
        "safety": public_safety(),
    }))
}

/// 使用当前 partial inventory 唯一解析目标并读取中立元数据。
pub(crate) async fn metadata(
    config: &ConnectionConfig,
    target: &str,
    maximum_items: usize,
) -> AppResult<Value> {
    let window = atspi::resolve(config, target, maximum_items)
        .await
        .map_err(public_error)?;
    Ok(json!({
        "ok": true,
        "contractVersion": "act/window-metadata/v2",
        "capability": "window.metadata.read@2",
        "coverage": "partial-accessibility-exporters",
        "readOnly": true,
        "mutationAllowed": false,
        "window": public_window(&window),
        "safety": public_safety(),
    }))
}

fn public_window(window: &WindowRecord) -> Value {
    json!({
        "sessionId": window.session_id,
        "targetKind": "accessibility-exporter-window",
        "role": window.role,
        "title": window.title,
        "visible": true,
        "visibilityEvidence": "accessibility-visible-and-showing",
        "targetIdentity": {
            "contractVersion": "act/window-target-identity/v2",
            "provider": "at-spi2",
            "coverage": "partial-accessibility-exporters",
            "freshness": "read-only-inspection-snapshot",
            "mutationAllowed": false,
            "sameOwnerObjectPathReuse": "not-guaranteed",
            "generationOwner": "none",
            "nativeIdentityExposed": false,
        },
        "capabilities": ["window.metadata.read@2", "accessibility.tree.read@2"],
    })
}

fn public_safety() -> Value {
    json!({
        "foregroundClaimed": false,
        "compositorVisibilityClaimed": false,
        "globalWindowDirectoryClaimed": false,
        "nativeIdentityExposed": false,
    })
}

pub(crate) fn public_error(failure: Failure) -> AppControlError {
    let (code, message, provider_state) = match failure {
        Failure::AccessibilityUnavailable => (
            "ACCESSIBILITY_UNAVAILABLE",
            "The private accessibility provider is unavailable.",
            "unavailable",
        ),
        Failure::PermissionDenied => (
            "PERMISSION_DENIED",
            "The private accessibility provider denied the read.",
            "permission-denied",
        ),
        Failure::Timeout => (
            "TIMEOUT",
            "The bounded accessibility observation timed out.",
            "timeout",
        ),
        Failure::Protocol => (
            "WORKER_PROTOCOL_ERROR",
            "The private accessibility protocol response was invalid.",
            "protocol-error",
        ),
        Failure::Stale => (
            "STALE_SESSION",
            "The accessibility inspection target is no longer current.",
            "stale",
        ),
        Failure::Ambiguous => (
            "AMBIGUOUS_TARGET",
            "The accessibility inspection target is not unique.",
            "ambiguous",
        ),
    };
    AppControlError::with_details(
        code,
        message,
        json!({
            "platform": "linux",
            "provider": "at-spi2",
            "providerState": provider_state,
            "executionRealm": "isolated-worker",
            "fallback": "none",
            "partialResultPublished": false,
        }),
    )
}
