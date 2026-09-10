//! Linux 认证 Desktop Entry 应用启动 Module。

use serde_json::{Value, json};

use crate::{
    adapters::linux::{desktop_entries, desktop_entry_launch},
    capabilities,
    components::{
        linux_application_open,
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    },
    domain::{AppControlError, AppResult},
};

const MAXIMUM_APPLICATIONS: usize = 4_096;

enum ApplicationMatch {
    Missing,
    Unique(desktop_entries::DesktopEntryRecord),
}

fn resolve(session_id: &str) -> AppResult<ApplicationMatch> {
    if OpaqueTargetId::parse(session_id)
        .is_none_or(|target| target.kind() != OpaqueTargetKind::Application)
    {
        return Ok(ApplicationMatch::Missing);
    }
    let inventory = desktop_entries::enumerate(MAXIMUM_APPLICATIONS);
    let mut matches = inventory
        .records
        .into_iter()
        .filter(|application| application.session_id == session_id)
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err(AppControlError::new(
            "AMBIGUOUS_TARGET",
            "The opaque application target resolves to multiple current Desktop Entry records.",
        ));
    }
    if let Some(application) = matches.pop() {
        return Ok(ApplicationMatch::Unique(application));
    }
    if !inventory.complete {
        return Err(AppControlError::new(
            "BACKGROUND_OPERATION_UNAVAILABLE",
            "The current Desktop Entry inventory is incomplete.",
        ));
    }
    Ok(ApplicationMatch::Missing)
}

fn current(session_id: &str) -> AppResult<desktop_entries::DesktopEntryRecord> {
    match resolve(session_id)? {
        ApplicationMatch::Unique(application) => Ok(application),
        ApplicationMatch::Missing => Err(AppControlError::new(
            "STALE_SESSION",
            "The opaque application target no longer resolves.",
        )),
    }
}

/// 返回当前精确应用的公开启动状态，不泄漏 Desktop File ID、Exec 或路径。
pub(crate) fn inspect(session_id: &str) -> AppResult<Value> {
    let application = current(session_id)?;
    Ok(json!({
        "ok": true,
        "readOnly": true,
        "application": {
            "sessionId": application.session_id,
            "targetKind": "installed-application",
            "displayName": application.display_name,
            "launchCapability": if application.launch_route.is_some() {
                "available-confirmed"
            } else {
                "unavailable"
            },
            "nativeIdentityExposed": false,
            "runtimePathExposed": false,
        },
    }))
}

/// assessment 只重解析目标和认证路线，不启动子进程。
pub(crate) fn assess(session_id: &str) -> AppResult<Value> {
    let application = current(session_id)?;
    let launchable = application.launch_route.is_some();
    Ok(json!({
        "ok": true,
        "contractVersion": "act/control/v1",
        "capability": capabilities::APPLICATION_OPEN_V2,
        "targetId": session_id,
        "decision": if launchable { "confirmation-required" } else { "unavailable" },
        "executionRealm": if launchable { "host-foreground" } else { "none" },
        "requiresConfirmation": true,
        "requiresForegroundConsent": true,
        "reasons": [if launchable {
            "authenticated-toolkit-self-executable-fixture-is-current"
        } else {
            "desktop-entry-is-not-in-the-toolkit-self-executable-fixture-set"
        }],
        "constraints": {
            "scope": "toolkit-owned-self-executable-fixture-only",
            "fixedArguments": true,
            "callerArgumentsAccepted": false,
            "shellUsed": false,
            "noFallback": true,
        },
        "evidence": {
            "targetKind": "installed-application",
            "platform": "linux",
            "implementationState": "fixture-route-verified",
            "nativeIdentityExposed": false,
            "runtimePathExposed": false,
        },
    }))
}

/// 按确认、前景同意、输入、目标和 provider 的顺序执行固定夹具启动。
pub(crate) fn perform(
    session_id: Option<&str>,
    confirmed: bool,
    foreground_consent: bool,
    input: Option<&Value>,
) -> AppResult<Value> {
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "Linux application launch requires explicit confirmation.",
        ));
    }
    if !foreground_consent {
        return Err(AppControlError::new(
            "FOREGROUND_CONSENT_REQUIRED",
            "Linux application launch requires upfront foreground-impact consent.",
        ));
    }
    linux_application_open::validate(input)?;
    let session_id = session_id
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppControlError::new("INVALID_ARGUMENT", "target.sessionId is required."))?;
    let application = current(session_id)?;
    let route = application.launch_route.ok_or_else(|| {
        AppControlError::new(
            "CAPABILITY_UNAVAILABLE",
            "The exact Desktop Entry is outside the toolkit-owned launch fixture set.",
        )
    })?;
    let evidence = desktop_entry_launch::dispatch(route)?;
    Ok(json!({
        "ok": true,
        "contractVersion": "act/control/v2",
        "capability": capabilities::APPLICATION_OPEN_V2,
        "targetId": application.session_id,
        "targetKind": "installed-application",
        "launchDispatched": evidence.dispatched,
        "processObserved": evidence.process_observed,
        "processExitObserved": evidence.process_exit_observed,
        "fixtureExitVerified": evidence.fixture_exit_verified,
        "foregroundConsentObserved": true,
        "foregroundMayChange": true,
        "foregroundActivationRequested": false,
        "foregroundStateObserved": false,
        "shellUsed": false,
        "argumentsCallerControlled": false,
        "nativeIdentifiersExposed": false,
        "runtimePathExposed": false,
        "fixtureScope": "toolkit-owned-self-executable-v1",
        "retrySafe": false,
        "automaticRetryProhibited": true,
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::perform;

    #[test]
    fn confirmation_and_foreground_consent_precede_input_and_target() {
        let confirmation = perform(None, false, false, None)
            .err()
            .unwrap_or_else(|| panic!("未确认必须失败"));
        assert_eq!(confirmation.code, "CONFIRMATION_REQUIRED");
        let foreground = perform(None, true, false, None)
            .err()
            .unwrap_or_else(|| panic!("缺少前景同意必须失败"));
        assert_eq!(foreground.code, "FOREGROUND_CONSENT_REQUIRED");
        let input = perform(None, true, true, Some(&json!({"path": "/bin/true"})))
            .err()
            .unwrap_or_else(|| panic!("非法输入必须失败"));
        assert_eq!(input.code, "INVALID_ARGUMENT");
    }
}
