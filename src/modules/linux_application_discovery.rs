//! Linux 应用关系发现 Module 的首个只读纵切。

use serde_json::{Value, json};

use crate::{
    adapters::linux::{desktop_entries, procfs},
    capabilities,
    components::linux_host_identity,
    domain::{AppControlError, AppResult},
};

const MAXIMUM_DISCOVERY_ITEMS: usize = 4_096;

fn snapshot_error(_: std::io::Error) -> AppControlError {
    AppControlError::with_details(
        "PROCESS_SNAPSHOT_FAILED",
        "The Linux process snapshot could not be read.",
        json!({
            "platform": "linux",
            "provider": "procfs",
            "providerState": "unavailable",
            "executionRealm": "none",
            "fallback": "none",
        }),
    )
}

fn validate_limit(value: usize, name: &str) -> AppResult<()> {
    if (1..=MAXIMUM_DISCOVERY_ITEMS).contains(&value) {
        Ok(())
    } else {
        Err(AppControlError::new(
            "INVALID_ARGUMENT",
            format!("{name} must be from 1 through {MAXIMUM_DISCOVERY_ITEMS}."),
        ))
    }
}

/// 返回相互独立的 XDG 应用清单与 procfs 进程清单，不猜测二者关系。
pub(crate) fn discover(
    maximum_applications: usize,
    maximum_processes: usize,
    maximum_windows: usize,
) -> AppResult<Value> {
    discover_with(
        capabilities::APPLICATION_DISCOVER_V2,
        false,
        maximum_applications,
        maximum_processes,
        maximum_windows,
    )
}

/// 返回与 application.open@2 同步发布认证启动状态的版本三清单。
pub(crate) fn discover_v3(
    maximum_applications: usize,
    maximum_processes: usize,
    maximum_windows: usize,
) -> AppResult<Value> {
    discover_with(
        capabilities::APPLICATION_DISCOVER_V3,
        true,
        maximum_applications,
        maximum_processes,
        maximum_windows,
    )
}

fn discover_with(
    capability: &'static str,
    publish_launch: bool,
    maximum_applications: usize,
    maximum_processes: usize,
    maximum_windows: usize,
) -> AppResult<Value> {
    validate_limit(maximum_applications, "max-applications")?;
    validate_limit(maximum_processes, "max-processes")?;
    validate_limit(maximum_windows, "max-windows")?;

    let application_inventory = desktop_entries::enumerate(maximum_applications);
    let applications = application_inventory
        .records
        .iter()
        .map(|application| {
            json!({
                "sessionId": application.session_id,
                "targetKind": "installed-application",
                "displayName": application.display_name,
                "version": "",
                "publisher": "",
                "state": "installed",
                "runningProcessSessionIds": [],
                "discoverySources": [desktop_entries::DISCOVERY_SOURCE],
                "launchCapability": if publish_launch && application.launch_route.is_some() {
                    "available-confirmed"
                } else {
                    "unavailable"
                },
                "relationshipEvidence": "none",
                "targetIdentity": {
                    "contractVersion": "act/application-target-identity/v1",
                    "providerDomain": "linux-xdg-desktop-entry",
                    "generationBound": true,
                    "freshness": "discovery-snapshot",
                    "desktopFileIdExposed": false,
                    "nativePathExposed": false,
                    "contentDigestExposed": false,
                },
            })
        })
        .collect::<Vec<_>>();
    let inventory = procfs::snapshot(maximum_processes).map_err(snapshot_error)?;
    let processes = inventory
        .records
        .iter()
        .map(|process| {
            json!({
                "sessionId": process.session_id,
                "targetKind": "running-process",
                "processName": process.process_name,
                "state": "running",
                "identityFreshness": if process.identity_reliable { "process-lifetime" } else { "best-effort-current-snapshot" },
                "metadataAccess": process.metadata_access,
                "integrityRelation": "unknown",
                "hasVisibleWindow": false,
                "windowVisibility": "no-visible-titled-window",
                "foregroundRequiredForObservation": false,
                "windowSessionIds": [],
                "relatedApplicationIds": [],
                "relationshipStatus": "unassociated",
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "ok": true,
        "contractVersion": "act/control/v1",
        "implementation": "rust",
        "capability": capability,
        "data": {
            "hostTargetId": linux_host_identity::current_host_target(),
            "readOnly": true,
            "foregroundUnchanged": true,
            "productPromise": "broad-general-control-with-capability-degradation",
            "coverage": {
                "runningProcesses": "available-linux-procfs",
                "visibleTitledWindows": "unavailable-no-linux-desktop-provider",
                "desktopEntryApplications": if application_inventory.available {
                    "available-xdg-desktop-entry"
                } else {
                    "unavailable-no-xdg-data-roots"
                },
                "traditionalInstalledApplications": "not-applicable-on-linux",
                "shellApplications": "unavailable-no-linux-provider",
                "startMenuApplications": "unavailable-no-linux-provider",
                "storeAndUwpPackages": "not-applicable-on-linux",
                "noWindowProcessesIncluded": true,
                "relationshipPolicy": "none-linux-independent-inventories",
            },
            "complete": {
                "applications": application_inventory.complete,
                "processes": inventory.complete,
                "windows": false,
            },
            "counts": {
                "installedApplications": applications.len(),
                "runningInstalledApplications": 0,
                "runningProcesses": processes.len(),
                "noWindowProcesses": processes.len(),
                "unassociatedProcesses": processes.len(),
                "permissionBlockedProcesses": 0,
                "higherIntegrityProcesses": 0,
                "visibleWindows": 0,
            },
            "applicationsReturned": application_inventory.applications_returned,
            "applicationsTruncated": application_inventory.applications_truncated,
            "totalEligibleApplications": application_inventory.total_eligible_applications,
            "warnings": application_inventory.warnings,
            "applications": applications,
            "processes": processes,
            "windows": [],
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::snapshot_error;
    use crate::domain::error_json;

    #[test]
    fn discovery_snapshot_failure_matches_public_error_contract() {
        let instance = error_json(&snapshot_error(std::io::Error::other("fixture")));
        let schema = serde_json::from_str(include_str!(
            "../../contracts/v1/error-envelope.schema.json"
        ))
        .expect("schema must parse");
        jsonschema::draft202012::validate(&schema, &instance)
            .expect("discovery snapshot error must validate");
        assert_eq!(instance["error"]["code"], "PROCESS_SNAPSHOT_FAILED");
    }
}
