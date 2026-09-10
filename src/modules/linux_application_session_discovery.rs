//! Linux Application Session Discovery System 的版本化只读聚合 Module。

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::{
    adapters::linux::{
        desktop_entries, procfs,
        uix_agent::{self, Failure as UixFailure},
    },
    capabilities,
    components::{
        linux_host_identity,
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
    },
    domain::{AppControlError, AppResult},
};

const MAXIMUM_DISCOVERY_ITEMS: usize = 4_096;
const MAXIMUM_PUBLIC_NAME_CHARS: usize = 4_096;

/// Module 私有的 provider-neutral 应用观察事实。
#[derive(Clone, Debug)]
struct ApplicationObservation {
    session_id: String,
    display_name: String,
    launchable: bool,
}

/// Module 私有的 provider-neutral 进程观察事实。
#[derive(Clone, Debug)]
struct ProcessObservation {
    session_id: String,
    process_name: String,
    identity_reliable: bool,
    metadata_access: &'static str,
}

/// XDG 快照端口只暴露中立记录、可用性和完整性。
#[derive(Clone, Debug)]
struct ApplicationSnapshot {
    records: Vec<ApplicationObservation>,
    available: bool,
    complete: bool,
    total: Option<usize>,
    source_truncated: bool,
    warnings: Vec<&'static str>,
}

/// procfs 快照端口只暴露中立记录和完整性。
#[derive(Clone, Debug)]
struct ProcessSnapshot {
    records: Vec<ProcessObservation>,
    available: bool,
    complete: bool,
    total: Option<usize>,
    source_truncated: bool,
}

/// UIX 窗口只以 opaque 身份和经认证 peer 对应的进程代际进入聚合。
#[derive(Clone, Debug)]
struct WindowObservation {
    session_id: String,
    owner_process_session_id: Option<String>,
    title: String,
    visible: bool,
    presentable: bool,
    revision: u64,
    presented_revision: u64,
}

/// UIX 窗口快照显式保存可用性、总量和部分失败，不冒充全局桌面目录。
#[derive(Clone, Debug)]
struct WindowSnapshot {
    records: Vec<WindowObservation>,
    available: bool,
    complete: bool,
    total: Option<usize>,
    truncated: Option<bool>,
    warnings: Vec<&'static str>,
}

/// UIX-aware 聚合的封闭版本事实，避免调用点分别传递可漂移字段。
#[derive(Clone, Copy)]
struct UixAggregateContract {
    capability: &'static str,
    application_contract: &'static str,
    publish_launch: bool,
}

/// Application Session Discovery Module 消费的两个只读快照端口。
trait SnapshotPort {
    fn applications(&self) -> ApplicationSnapshot;
    fn processes(&self) -> Result<ProcessSnapshot, std::io::Error>;
}

/// UIX 窗口来源保持独立端口，使冻结的版本二聚合不会隐式连接 Agent。
trait WindowSnapshotPort {
    fn windows(&self, maximum_items: usize) -> WindowSnapshot;
}

/// 生产 Adapter 在边界内终止 Desktop Entry、procfs 与原生错误类型。
struct LinuxSnapshotAdapter;

/// 生产 UIX Adapter 只投影认证后中立记录，并把失败降级成显式 coverage。
struct LinuxUixWindowSnapshotAdapter;

impl SnapshotPort for LinuxSnapshotAdapter {
    fn applications(&self) -> ApplicationSnapshot {
        let inventory = desktop_entries::enumerate(usize::MAX);
        ApplicationSnapshot {
            records: inventory
                .records
                .into_iter()
                .map(|record| ApplicationObservation {
                    session_id: record.session_id,
                    display_name: record.display_name,
                    launchable: record.launch_route.is_some(),
                })
                .collect(),
            available: inventory.available,
            complete: inventory.complete,
            total: inventory.total_eligible_applications,
            source_truncated: inventory.applications_truncated,
            warnings: inventory.warnings,
        }
    }

    fn processes(&self) -> Result<ProcessSnapshot, std::io::Error> {
        let inventory = procfs::snapshot(usize::MAX)?;
        let complete = inventory.complete;
        let total = complete.then_some(inventory.records.len());
        Ok(ProcessSnapshot {
            records: inventory
                .records
                .into_iter()
                .map(|record| ProcessObservation {
                    session_id: record.session_id,
                    process_name: record.process_name,
                    identity_reliable: record.identity_reliable,
                    metadata_access: record.metadata_access,
                })
                .collect(),
            available: true,
            complete,
            total,
            source_truncated: false,
        })
    }
}

impl WindowSnapshotPort for LinuxUixWindowSnapshotAdapter {
    fn windows(&self, maximum_items: usize) -> WindowSnapshot {
        match uix_agent::discover(maximum_items) {
            Ok(inventory) => {
                let truncated = inventory.warnings.iter().any(|warning| {
                    matches!(
                        *warning,
                        "deadline" | "endpoint-limit" | "item-limit" | "provider-item-limit"
                    )
                });
                WindowSnapshot {
                    records: inventory
                        .windows
                        .into_iter()
                        .map(|window| WindowObservation {
                            session_id: window.session_id,
                            owner_process_session_id: window.owner_process_session_id,
                            title: window.title,
                            visible: window.visible,
                            presentable: window.presentable,
                            revision: window.revision,
                            presented_revision: window.presented_revision,
                        })
                        .collect(),
                    available: true,
                    complete: inventory.complete && !truncated,
                    total: inventory.total,
                    truncated: if truncated {
                        Some(true)
                    } else if inventory.total.is_some() {
                        Some(false)
                    } else {
                        None
                    },
                    warnings: inventory
                        .warnings
                        .into_iter()
                        .map(map_uix_warning)
                        .collect(),
                }
            }
            Err(failure) => WindowSnapshot {
                records: Vec::new(),
                available: false,
                complete: false,
                total: None,
                truncated: None,
                warnings: vec![uix_failure_warning(failure)],
            },
        }
    }
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

fn invalid_snapshot(message: &'static str) -> AppControlError {
    AppControlError::new("OPERATION_FAILED", message)
}

fn validate_application_snapshot(snapshot: &ApplicationSnapshot) -> AppResult<()> {
    if (!snapshot.available && !snapshot.records.is_empty())
        || snapshot
            .total
            .is_some_and(|total| total < snapshot.records.len())
    {
        return Err(invalid_snapshot(
            "The application inventory returned inconsistent completeness facts.",
        ));
    }
    let mut identifiers = BTreeSet::new();
    for record in &snapshot.records {
        let canonical = OpaqueTargetId::parse(&record.session_id)
            .is_some_and(|target| target.kind() == OpaqueTargetKind::Application);
        if !canonical
            || record.display_name.is_empty()
            || record.display_name.chars().count() > MAXIMUM_PUBLIC_NAME_CHARS
            || !identifiers.insert(record.session_id.as_str())
        {
            return Err(invalid_snapshot(
                "The application inventory returned an invalid public record.",
            ));
        }
    }
    Ok(())
}

fn validate_process_snapshot(snapshot: &ProcessSnapshot) -> AppResult<()> {
    if (!snapshot.available && !snapshot.records.is_empty())
        || snapshot
            .total
            .is_some_and(|total| total < snapshot.records.len())
    {
        return Err(invalid_snapshot(
            "The process inventory returned inconsistent completeness facts.",
        ));
    }
    let mut identifiers = BTreeSet::new();
    for record in &snapshot.records {
        let canonical = OpaqueTargetId::parse(&record.session_id)
            .is_some_and(|target| target.kind() == OpaqueTargetKind::Process);
        if !canonical
            || record.process_name.is_empty()
            || record.process_name.chars().count() > MAXIMUM_PUBLIC_NAME_CHARS
            || !matches!(
                record.metadata_access,
                "available" | "permission-blocked" | "unavailable"
            )
            || !identifiers.insert(record.session_id.as_str())
        {
            return Err(invalid_snapshot(
                "The process inventory returned an invalid public record.",
            ));
        }
    }
    Ok(())
}

fn validate_window_snapshot(snapshot: &WindowSnapshot) -> AppResult<()> {
    if (!snapshot.available && !snapshot.records.is_empty())
        || snapshot
            .total
            .is_some_and(|total| total < snapshot.records.len())
        || (snapshot.complete
            && (snapshot.total != Some(snapshot.records.len())
                || snapshot.truncated != Some(false)))
    {
        return Err(invalid_snapshot(
            "The UIX window inventory returned inconsistent completeness facts.",
        ));
    }
    let mut identifiers = BTreeSet::new();
    for record in &snapshot.records {
        let canonical = OpaqueTargetId::parse(&record.session_id)
            .is_some_and(|target| target.kind() == OpaqueTargetKind::Window);
        let owner_canonical = record
            .owner_process_session_id
            .as_deref()
            .is_none_or(|owner| {
                OpaqueTargetId::parse(owner)
                    .is_some_and(|target| target.kind() == OpaqueTargetKind::Process)
            });
        if !canonical
            || !owner_canonical
            || record.title.chars().count() > 256
            || record.presented_revision > record.revision
            || !identifiers.insert(record.session_id.as_str())
        {
            return Err(invalid_snapshot(
                "The UIX window inventory returned an invalid public record.",
            ));
        }
    }
    Ok(())
}

fn map_uix_warning(code: &str) -> &'static str {
    match code {
        "authentication-rejected" => "uix-window-authentication-rejected",
        "deadline" | "endpoint-timeout" => "uix-window-endpoint-timeout",
        "endpoint-limit" | "item-limit" | "provider-item-limit" => "uix-window-inventory-truncated",
        _ => "uix-window-inventory-incomplete",
    }
}

fn uix_failure_warning(failure: UixFailure) -> &'static str {
    match failure {
        UixFailure::PermissionDenied => "uix-window-authentication-rejected",
        UixFailure::Timeout => "uix-window-endpoint-timeout",
        UixFailure::Protocol | UixFailure::Ambiguous | UixFailure::Stale => {
            "uix-window-inventory-incomplete"
        }
        UixFailure::Unavailable => "uix-window-provider-unavailable",
    }
}

fn warning_message(code: &str) -> &'static str {
    match code {
        "desktop-entry-invalid-skipped" => "One or more invalid desktop entries were skipped.",
        "desktop-entry-read-incomplete" => {
            "One or more desktop entries changed or could not be read safely."
        }
        "desktop-file-id-conflict" => "One or more desktop file identities were ambiguous.",
        "desktop-entry-scan-limit-reached" => "The bounded desktop entry scan limit was reached.",
        "unsafe-filesystem-entry-skipped" => "One or more unsafe filesystem entries were skipped.",
        "xdg-application-root-unreadable" => {
            "One or more configured application roots were unreadable."
        }
        "xdg-application-roots-missing" => "No readable XDG application root was available.",
        "application-source-incomplete" => "The application inventory is incomplete.",
        "process-snapshot-incomplete" => "The process inventory is incomplete.",
        "window-provider-unavailable" => {
            "No certified Linux window inventory provider is available."
        }
        "uix-window-provider-unavailable" => {
            "The cooperative UIX window inventory provider is unavailable."
        }
        "uix-window-authentication-rejected" => {
            "One or more UIX Agent endpoints failed authentication."
        }
        "uix-window-endpoint-timeout" => {
            "One or more UIX Agent endpoints exceeded the bounded deadline."
        }
        "uix-window-inventory-truncated" => {
            "The cooperative UIX window inventory was truncated by a bounded limit."
        }
        "uix-window-inventory-incomplete" => {
            "One or more UIX Agent inventory entries were unavailable or invalid."
        }
        _ => "The application inventory is incomplete.",
    }
}

fn add_warning(warnings: &mut BTreeMap<&'static str, (&'static str, usize)>, code: &'static str) {
    warnings
        .entry(code)
        .and_modify(|warning| warning.1 += 1)
        .or_insert((warning_message(code), 1));
}

fn add_warning_count(
    warnings: &mut BTreeMap<&'static str, (&'static str, usize)>,
    code: &'static str,
    count: usize,
) {
    warnings
        .entry(code)
        .and_modify(|warning| warning.1 += count)
        .or_insert((warning_message(code), count));
}

fn source_truncated(
    total: Option<usize>,
    observed: usize,
    module_truncated: bool,
    adapter_truncated: bool,
    complete: bool,
) -> Option<bool> {
    if module_truncated || adapter_truncated || total.is_some_and(|total| total > observed) {
        Some(true)
    } else if total.is_some() || complete {
        Some(false)
    } else {
        None
    }
}

fn public_application(record: &ApplicationObservation, publish_launch: bool) -> Value {
    json!({
        "sessionId": record.session_id,
        "targetKind": "installed-application",
        "displayName": record.display_name,
        "version": "",
        "publisher": "",
        "state": "installed",
        "runningProcessSessionIds": [],
        "discoverySources": [desktop_entries::DISCOVERY_SOURCE],
        "launchCapability": if publish_launch && record.launchable {
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
}

fn public_process(record: &ProcessObservation) -> Value {
    json!({
        "sessionId": record.session_id,
        "targetKind": "running-process",
        "processName": record.process_name,
        "state": "running",
        "identityFreshness": if record.identity_reliable {
            "process-lifetime"
        } else {
            "best-effort-current-snapshot"
        },
        "metadataAccess": record.metadata_access,
        "integrityRelation": "unknown",
        "hasVisibleWindow": false,
        "windowVisibility": "no-visible-titled-window",
        "foregroundRequiredForObservation": false,
        "windowSessionIds": [],
        "relatedApplicationIds": [],
        "relationshipStatus": "unassociated",
    })
}

fn public_uix_window(record: &WindowObservation) -> Value {
    json!({
        "sessionId": record.session_id,
        "targetKind": "uix-agent-window",
        "title": record.title,
        "visible": record.visible,
        "presentable": record.presentable,
        "revision": record.revision,
        "presentedRevision": record.presented_revision,
        "ownerProcessSessionId": record.owner_process_session_id,
        "relationshipEvidence": if record.owner_process_session_id.is_some() {
            "authenticated-peer-process-lifetime"
        } else {
            "none"
        },
        "targetIdentity": {
            "contractVersion": "act/window-target-identity/v3",
            "provider": "uix-agent-v1",
            "coverage": "opt-in-uix-agent-applications",
            "freshness": "agent-process-token-and-window-generation",
            "nativeIdentityExposed": false,
        },
    })
}

fn validate_dimension_policy(data: &Value, name: &str) -> AppResult<()> {
    let records = data[name]
        .as_array()
        .ok_or_else(|| invalid_snapshot("The session aggregate collection is invalid."))?;
    let count = data["count"][name]
        .as_u64()
        .ok_or_else(|| invalid_snapshot("The session aggregate count is invalid."))?;
    if count != records.len() as u64 {
        return Err(invalid_snapshot(
            "The session aggregate count does not match its collection.",
        ));
    }
    let total = data["total"][name].as_u64();
    let truncated = data["truncated"][name].as_bool();
    let complete = data["complete"][name]
        .as_bool()
        .ok_or_else(|| invalid_snapshot("The session aggregate completeness is invalid."))?;
    if total.is_some_and(|total| total < count)
        || (truncated == Some(false) && total != Some(count))
        || (truncated == Some(true) && total.is_some_and(|total| total <= count))
        || (complete && (total != Some(count) || truncated != Some(false)))
    {
        return Err(invalid_snapshot(
            "The session aggregate total or truncation facts are inconsistent.",
        ));
    }
    Ok(())
}

/// 公开投影发布前重验跨字段不变量，避免 JSON schema 无法表达的漂移。
fn validate_public_policy(instance: &Value) -> AppResult<()> {
    let data = instance
        .get("data")
        .ok_or_else(|| invalid_snapshot("The session aggregate data is missing."))?;
    if instance["capability"] != capabilities::APPLICATION_SESSION_DISCOVER_V2
        || data["applicationContract"] != capabilities::APPLICATION_DISCOVER_V2
        || data["processContract"] != capabilities::PROCESS_DISCOVER
        || data["coverage"]["applications"]["complete"] != data["complete"]["applications"]
        || data["coverage"]["processes"]["complete"] != data["complete"]["processes"]
        || data["coverage"]["windows"]["complete"] != data["complete"]["windows"]
    {
        return Err(invalid_snapshot(
            "The session aggregate contract or coverage facts are inconsistent.",
        ));
    }
    validate_dimension_policy(data, "applications")?;
    validate_dimension_policy(data, "processes")?;
    let windows = data["windows"]
        .as_array()
        .ok_or_else(|| invalid_snapshot("The window collection is invalid."))?;
    if !windows.is_empty()
        || data["count"]["windows"] != 0
        || data["total"]["windows"] != Value::Null
        || data["truncated"]["windows"] != Value::Null
        || data["complete"]["windows"] != false
    {
        return Err(invalid_snapshot(
            "The unavailable window inventory was presented as observed.",
        ));
    }
    let applications = data["applications"]
        .as_array()
        .ok_or_else(|| invalid_snapshot("The application collection is invalid."))?;
    if applications.iter().any(|application| {
        application["runningProcessSessionIds"] != json!([])
            || application["relationshipEvidence"] != "none"
            || application["launchCapability"] != "unavailable"
    }) {
        return Err(invalid_snapshot(
            "The application aggregate attempted to publish a relationship or launch route.",
        ));
    }
    let processes = data["processes"]
        .as_array()
        .ok_or_else(|| invalid_snapshot("The process collection is invalid."))?;
    if processes.iter().any(|process| {
        process["relatedApplicationIds"] != json!([])
            || process["relationshipStatus"] != "unassociated"
            || process["windowSessionIds"] != json!([])
    }) {
        return Err(invalid_snapshot(
            "The process aggregate attempted to publish an uncertified relationship.",
        ));
    }
    Ok(())
}

fn v2_warning_code(code: &str) -> Option<&'static str> {
    match code {
        "desktop-entry-invalid-skipped" => Some("desktop-entry-invalid-skipped"),
        "desktop-entry-read-incomplete" => Some("desktop-entry-read-incomplete"),
        "desktop-file-id-conflict" => Some("desktop-file-id-conflict"),
        "desktop-entry-scan-limit-reached" => Some("desktop-entry-scan-limit-reached"),
        "unsafe-filesystem-entry-skipped" => Some("unsafe-filesystem-entry-skipped"),
        "xdg-application-root-unreadable" => Some("xdg-application-root-unreadable"),
        "xdg-application-roots-missing" => Some("xdg-application-roots-missing"),
        "application-source-incomplete" => Some("application-source-incomplete"),
        "process-snapshot-incomplete" => Some("process-snapshot-incomplete"),
        "window-provider-unavailable" => Some("window-provider-unavailable"),
        _ => None,
    }
}

/// UIX-aware 版本发布前重验窗口与进程反向关系，拒绝孤立或猜测型关联。
fn validate_uix_public_policy(
    instance: &Value,
    expected_capability: &str,
    expected_application_contract: &str,
    publish_launch: bool,
) -> AppResult<()> {
    let data = instance
        .get("data")
        .ok_or_else(|| invalid_snapshot("The UIX-aware session aggregate data is missing."))?;
    if instance["capability"] != expected_capability
        || data["applicationContract"] != expected_application_contract
        || data["processContract"] != capabilities::PROCESS_DISCOVER
        || data["windowContract"] != capabilities::WINDOW_DISCOVER_V3
        || data["coverage"]["windows"]["provider"] != "uix-agent-v1"
        || data["coverage"]["relationshipPolicy"]
            != "uix-window-to-process-exact-no-application-inference"
        || data["coverage"]["applications"]["complete"] != data["complete"]["applications"]
        || data["coverage"]["processes"]["complete"] != data["complete"]["processes"]
        || data["coverage"]["windows"]["complete"] != data["complete"]["windows"]
    {
        return Err(invalid_snapshot(
            "The UIX-aware session aggregate contract or coverage facts are inconsistent.",
        ));
    }
    for name in ["applications", "processes", "windows"] {
        validate_dimension_policy(data, name)?;
    }
    let applications = data["applications"]
        .as_array()
        .ok_or_else(|| invalid_snapshot("The application collection is invalid."))?;
    if applications.iter().any(|application| {
        let launch_status = application["launchCapability"].as_str();
        let launch_status_valid = if publish_launch {
            matches!(launch_status, Some("unavailable" | "available-confirmed"))
        } else {
            launch_status == Some("unavailable")
        };
        application["runningProcessSessionIds"] != json!([])
            || application["relationshipEvidence"] != "none"
            || !launch_status_valid
    }) {
        return Err(invalid_snapshot(
            "The UIX-aware aggregate attempted to infer an application relationship.",
        ));
    }

    let windows = data["windows"]
        .as_array()
        .ok_or_else(|| invalid_snapshot("The UIX window collection is invalid."))?;
    if data["coverage"]["windows"]["available"] == false && !windows.is_empty() {
        return Err(invalid_snapshot(
            "An unavailable UIX provider published window records.",
        ));
    }
    let mut window_by_id = BTreeMap::new();
    for window in windows {
        let session_id = window["sessionId"]
            .as_str()
            .filter(|value| {
                OpaqueTargetId::parse(value)
                    .is_some_and(|target| target.kind() == OpaqueTargetKind::Window)
            })
            .ok_or_else(|| invalid_snapshot("A UIX window target is not canonical."))?;
        let owner = window["ownerProcessSessionId"].as_str();
        let owner_canonical = owner.is_none_or(|value| {
            OpaqueTargetId::parse(value)
                .is_some_and(|target| target.kind() == OpaqueTargetKind::Process)
        });
        let evidence_matches = match owner {
            Some(_) => window["relationshipEvidence"] == "authenticated-peer-process-lifetime",
            None => window["relationshipEvidence"] == "none",
        };
        if !owner_canonical
            || !evidence_matches
            || window["presentedRevision"].as_u64() > window["revision"].as_u64()
            || window_by_id.insert(session_id, window).is_some()
        {
            return Err(invalid_snapshot(
                "A UIX window relationship or revision fact is invalid.",
            ));
        }
    }

    let processes = data["processes"]
        .as_array()
        .ok_or_else(|| invalid_snapshot("The process collection is invalid."))?;
    for process in processes {
        let process_id = process["sessionId"]
            .as_str()
            .ok_or_else(|| invalid_snapshot("A process target is invalid."))?;
        let linked = process["windowSessionIds"]
            .as_array()
            .ok_or_else(|| invalid_snapshot("A process window relation is invalid."))?;
        let mut unique = BTreeSet::new();
        let mut visible = false;
        for target in linked {
            let target = target
                .as_str()
                .ok_or_else(|| invalid_snapshot("A linked window target is invalid."))?;
            let window = window_by_id
                .get(target)
                .filter(|window| window["ownerProcessSessionId"] == process_id)
                .ok_or_else(|| invalid_snapshot("A process points to an unrelated UIX window."))?;
            if !unique.insert(target) {
                return Err(invalid_snapshot("A process repeats a UIX window relation."));
            }
            visible |= window["visible"] == true
                && window["title"]
                    .as_str()
                    .is_some_and(|title| !title.is_empty());
        }
        if process["hasVisibleWindow"] != visible
            || process["windowVisibility"]
                != if visible {
                    "visible-titled-window"
                } else {
                    "no-visible-titled-window"
                }
            || process["relatedApplicationIds"] != json!([])
            || process["relationshipStatus"] != "unassociated"
        {
            return Err(invalid_snapshot(
                "A process window or application relationship fact is inconsistent.",
            ));
        }
    }
    Ok(())
}

fn discover_base_with(
    port: &impl SnapshotPort,
    maximum_applications: usize,
    maximum_processes: usize,
    capability: &'static str,
    application_contract: &'static str,
    publish_launch: bool,
) -> AppResult<Value> {
    validate_limit(maximum_applications, "max-applications")?;
    validate_limit(maximum_processes, "max-processes")?;

    let mut application_snapshot = port.applications();
    let mut process_snapshot = port.processes().map_err(snapshot_error)?;
    validate_application_snapshot(&application_snapshot)?;
    validate_process_snapshot(&process_snapshot)?;

    application_snapshot.records.sort_by(|left, right| {
        left.display_name
            .cmp(&right.display_name)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    process_snapshot.records.sort_by(|left, right| {
        left.process_name
            .cmp(&right.process_name)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });

    let observed_applications = application_snapshot.records.len();
    let observed_processes = process_snapshot.records.len();
    let application_limit_truncated = observed_applications > maximum_applications;
    let process_limit_truncated = observed_processes > maximum_processes;
    application_snapshot.records.truncate(maximum_applications);
    process_snapshot.records.truncate(maximum_processes);

    let applications_truncated = source_truncated(
        application_snapshot.total,
        observed_applications,
        application_limit_truncated,
        application_snapshot.source_truncated,
        application_snapshot.complete,
    );
    let processes_truncated = source_truncated(
        process_snapshot.total,
        observed_processes,
        process_limit_truncated,
        process_snapshot.source_truncated,
        process_snapshot.complete,
    );
    let applications_complete = application_snapshot.available
        && application_snapshot.complete
        && applications_truncated == Some(false);
    let processes_complete = process_snapshot.available
        && process_snapshot.complete
        && processes_truncated == Some(false);

    let applications = application_snapshot
        .records
        .iter()
        .map(|record| public_application(record, publish_launch))
        .collect::<Vec<_>>();
    let processes = process_snapshot
        .records
        .iter()
        .map(public_process)
        .collect::<Vec<_>>();

    let mut warnings = BTreeMap::new();
    for code in application_snapshot.warnings {
        let code = match code {
            "desktop-entry-invalid-skipped" => "desktop-entry-invalid-skipped",
            "desktop-entry-read-incomplete" => "desktop-entry-read-incomplete",
            "desktop-file-id-conflict" => "desktop-file-id-conflict",
            "desktop-entry-scan-limit-reached" => "desktop-entry-scan-limit-reached",
            "unsafe-filesystem-entry-skipped" => "unsafe-filesystem-entry-skipped",
            "xdg-application-root-unreadable" => "xdg-application-root-unreadable",
            "xdg-application-roots-missing" => "xdg-application-roots-missing",
            _ => "application-source-incomplete",
        };
        add_warning(&mut warnings, code);
    }
    if !application_snapshot.complete && warnings.is_empty() {
        add_warning(&mut warnings, "application-source-incomplete");
    }
    if !process_snapshot.complete {
        add_warning(&mut warnings, "process-snapshot-incomplete");
    }
    add_warning(&mut warnings, "window-provider-unavailable");
    let warnings = warnings
        .into_iter()
        .map(|(code, (message, count))| {
            json!({
                "code": code,
                "message": message,
                "count": count,
            })
        })
        .collect::<Vec<_>>();

    let instance = json!({
        "ok": true,
        "contractVersion": "act/control/v1",
        "implementation": "rust",
        "capability": capability,
        "data": {
            "hostTargetId": linux_host_identity::current_host_target(),
            "applicationContract": application_contract,
            "processContract": capabilities::PROCESS_DISCOVER,
            "readOnly": true,
            "foregroundUnchanged": true,
            "coverage": {
                "applications": {
                    "provider": "xdg-desktop-entry",
                    "available": application_snapshot.available,
                    "complete": applications_complete,
                },
                "processes": {
                    "provider": "linux-procfs",
                    "available": process_snapshot.available,
                    "complete": processes_complete,
                },
                "windows": {
                    "provider": "none",
                    "available": false,
                    "complete": false,
                },
                "relationshipPolicy": "none-unassociated-no-inference",
            },
            "complete": {
                "applications": applications_complete,
                "processes": processes_complete,
                "windows": false,
            },
            "count": {
                "applications": applications.len(),
                "processes": processes.len(),
                "windows": 0,
            },
            "total": {
                "applications": application_snapshot.total,
                "processes": process_snapshot.total,
                "windows": Value::Null,
            },
            "truncated": {
                "applications": applications_truncated,
                "processes": processes_truncated,
                "windows": Value::Null,
            },
            "warnings": warnings,
            "applications": applications,
            "processes": processes,
            "windows": [],
        },
    });
    Ok(instance)
}

fn discover_with(
    port: &impl SnapshotPort,
    maximum_applications: usize,
    maximum_processes: usize,
) -> AppResult<Value> {
    let instance = discover_base_with(
        port,
        maximum_applications,
        maximum_processes,
        capabilities::APPLICATION_SESSION_DISCOVER_V2,
        capabilities::APPLICATION_DISCOVER_V2,
        false,
    )?;
    validate_public_policy(&instance)?;
    Ok(instance)
}

/// 生产入口只创建私有 Adapter 并返回中立聚合结果。
pub(crate) fn discover(maximum_applications: usize, maximum_processes: usize) -> AppResult<Value> {
    discover_with(
        &LinuxSnapshotAdapter,
        maximum_applications,
        maximum_processes,
    )
}

fn discover_uix_with(
    snapshot_port: &impl SnapshotPort,
    window_port: &impl WindowSnapshotPort,
    maximum_applications: usize,
    maximum_processes: usize,
    maximum_windows: usize,
    contract: UixAggregateContract,
) -> AppResult<Value> {
    validate_limit(maximum_windows, "max-windows")?;
    let mut window_snapshot = window_port.windows(maximum_windows);
    validate_window_snapshot(&window_snapshot)?;
    window_snapshot
        .records
        .sort_by(|left, right| left.session_id.cmp(&right.session_id));

    let mut instance = discover_base_with(
        snapshot_port,
        maximum_applications,
        maximum_processes,
        contract.capability,
        contract.application_contract,
        contract.publish_launch,
    )?;
    let data = instance["data"]
        .as_object_mut()
        .ok_or_else(|| invalid_snapshot("The session aggregate data is invalid."))?;

    let mut warnings = BTreeMap::new();
    let existing_warnings = data
        .get("warnings")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_snapshot("The session aggregate warnings are invalid."))?;
    for warning in existing_warnings {
        let code = warning["code"]
            .as_str()
            .and_then(v2_warning_code)
            .ok_or_else(|| invalid_snapshot("The session aggregate warning is invalid."))?;
        if code == "window-provider-unavailable" {
            continue;
        }
        let count = warning["count"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| invalid_snapshot("The session aggregate warning count is invalid."))?;
        add_warning_count(&mut warnings, code, count);
    }
    for code in &window_snapshot.warnings {
        add_warning(&mut warnings, code);
    }
    let warning_values = warnings
        .into_iter()
        .map(|(code, (message, count))| {
            json!({
                "code": code,
                "message": message,
                "count": count,
            })
        })
        .collect::<Vec<_>>();

    let mut windows_by_process = BTreeMap::<&str, Vec<&WindowObservation>>::new();
    for window in &window_snapshot.records {
        if let Some(process) = window.owner_process_session_id.as_deref() {
            windows_by_process.entry(process).or_default().push(window);
        }
    }
    let processes = data
        .get_mut("processes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid_snapshot("The process collection is invalid."))?;
    for process in processes {
        let process_id = process["sessionId"]
            .as_str()
            .ok_or_else(|| invalid_snapshot("A process target is invalid."))?;
        let linked = windows_by_process
            .get(process_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let visible = linked
            .iter()
            .any(|window| window.visible && !window.title.is_empty());
        process["hasVisibleWindow"] = json!(visible);
        process["windowVisibility"] = json!(if visible {
            "visible-titled-window"
        } else {
            "no-visible-titled-window"
        });
        process["windowSessionIds"] = json!(
            linked
                .iter()
                .map(|window| window.session_id.as_str())
                .collect::<Vec<_>>()
        );
    }

    let windows = window_snapshot
        .records
        .iter()
        .map(public_uix_window)
        .collect::<Vec<_>>();
    data.insert(
        "windowContract".to_owned(),
        json!(capabilities::WINDOW_DISCOVER_V3),
    );
    data["coverage"]["windows"] = json!({
        "provider": "uix-agent-v1",
        "coverage": "opt-in-uix-agent-applications",
        "available": window_snapshot.available,
        "complete": window_snapshot.complete,
    });
    data["coverage"]["relationshipPolicy"] =
        json!("uix-window-to-process-exact-no-application-inference");
    data["complete"]["windows"] = json!(window_snapshot.complete);
    data["count"]["windows"] = json!(windows.len());
    data["total"]["windows"] = json!(window_snapshot.total);
    data["truncated"]["windows"] = json!(window_snapshot.truncated);
    data["warnings"] = json!(warning_values);
    data["windows"] = json!(windows);
    validate_uix_public_policy(
        &instance,
        contract.capability,
        contract.application_contract,
        contract.publish_launch,
    )?;
    Ok(instance)
}

fn discover_v3_with(
    snapshot_port: &impl SnapshotPort,
    window_port: &impl WindowSnapshotPort,
    maximum_applications: usize,
    maximum_processes: usize,
    maximum_windows: usize,
) -> AppResult<Value> {
    discover_uix_with(
        snapshot_port,
        window_port,
        maximum_applications,
        maximum_processes,
        maximum_windows,
        UixAggregateContract {
            capability: capabilities::APPLICATION_SESSION_DISCOVER_V3,
            application_contract: capabilities::APPLICATION_DISCOVER_V2,
            publish_launch: false,
        },
    )
}

fn discover_v4_with(
    snapshot_port: &impl SnapshotPort,
    window_port: &impl WindowSnapshotPort,
    maximum_applications: usize,
    maximum_processes: usize,
    maximum_windows: usize,
) -> AppResult<Value> {
    discover_uix_with(
        snapshot_port,
        window_port,
        maximum_applications,
        maximum_processes,
        maximum_windows,
        UixAggregateContract {
            capability: capabilities::APPLICATION_SESSION_DISCOVER_V4,
            application_contract: capabilities::APPLICATION_DISCOVER_V3,
            publish_launch: true,
        },
    )
}

/// 生产版本三聚合显式连接 UIX Agent，并保留 XDG/procfs 的原有独立事实。
pub(crate) fn discover_v3(
    maximum_applications: usize,
    maximum_processes: usize,
    maximum_windows: usize,
) -> AppResult<Value> {
    discover_v3_with(
        &LinuxSnapshotAdapter,
        &LinuxUixWindowSnapshotAdapter,
        maximum_applications,
        maximum_processes,
        maximum_windows,
    )
}

/// 生产版本四把认证启动状态加入同一 XDG/UIX/procfs 只读聚合快照。
pub(crate) fn discover_v4(
    maximum_applications: usize,
    maximum_processes: usize,
    maximum_windows: usize,
) -> AppResult<Value> {
    discover_v4_with(
        &LinuxSnapshotAdapter,
        &LinuxUixWindowSnapshotAdapter,
        maximum_applications,
        maximum_processes,
        maximum_windows,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixturePort {
        applications: ApplicationSnapshot,
        processes: ProcessSnapshot,
    }

    struct FixtureWindowPort {
        snapshot: WindowSnapshot,
    }

    impl SnapshotPort for FixturePort {
        fn applications(&self) -> ApplicationSnapshot {
            self.applications.clone()
        }

        fn processes(&self) -> Result<ProcessSnapshot, std::io::Error> {
            Ok(self.processes.clone())
        }
    }

    impl WindowSnapshotPort for FixtureWindowPort {
        fn windows(&self, maximum_items: usize) -> WindowSnapshot {
            let mut snapshot = self.snapshot.clone();
            if snapshot.records.len() > maximum_items {
                snapshot.records.truncate(maximum_items);
                snapshot.truncated = Some(true);
                snapshot.complete = false;
            }
            snapshot
        }
    }

    fn application(identity: &str, name: &str) -> ApplicationObservation {
        ApplicationObservation {
            session_id: OpaqueTargetId::new(OpaqueTargetKind::Application, identity).to_string(),
            display_name: name.to_owned(),
            launchable: false,
        }
    }

    fn launchable_application(identity: &str, name: &str) -> ApplicationObservation {
        let mut application = application(identity, name);
        application.launchable = true;
        application
    }

    fn process(identity: &str, name: &str) -> ProcessObservation {
        ProcessObservation {
            session_id: OpaqueTargetId::new(OpaqueTargetKind::Process, identity).to_string(),
            process_name: name.to_owned(),
            identity_reliable: true,
            metadata_access: "available",
        }
    }

    fn fixture(
        applications: Vec<ApplicationObservation>,
        processes: Vec<ProcessObservation>,
    ) -> FixturePort {
        FixturePort {
            applications: ApplicationSnapshot {
                total: Some(applications.len()),
                records: applications,
                available: true,
                complete: true,
                source_truncated: false,
                warnings: Vec::new(),
            },
            processes: ProcessSnapshot {
                total: Some(processes.len()),
                records: processes,
                available: true,
                complete: true,
                source_truncated: false,
            },
        }
    }

    fn validate_schema(instance: &Value) {
        let schema = serde_json::from_str(include_str!(
            "../../contracts/v2/application-session-discovery.schema.json"
        ))
        .expect("v2 schema must parse");
        jsonschema::draft202012::validate(&schema, instance)
            .expect("fixture must validate against application session v2");
    }

    fn validate_v3_schema(instance: &Value) {
        let schema = serde_json::from_str(include_str!(
            "../../contracts/v3/application-session-discovery.schema.json"
        ))
        .expect("v3 schema must parse");
        jsonschema::draft202012::validate(&schema, instance)
            .expect("fixture must validate against application session v3");
    }

    fn validate_v4_schema(instance: &Value) {
        let Ok(schema) = serde_json::from_str(include_str!(
            "../../contracts/v4/application-session-discovery.schema.json"
        )) else {
            panic!("v4 schema must parse");
        };
        if let Err(error) = jsonschema::draft202012::validate(&schema, instance) {
            panic!("fixture must validate against application session v4: {error}");
        }
    }

    #[test]
    fn fixture_aggregates_one_host_one_app_one_process_and_no_window() {
        let instance = discover_with(
            &fixture(
                vec![application("desktop-generation", "Fixture App")],
                vec![process("proc-generation", "fixture-process")],
            ),
            16,
            16,
        )
        .expect("fixture aggregation must succeed");
        validate_schema(&instance);
        assert_eq!(
            instance["data"]["count"],
            json!({
                "applications": 1,
                "processes": 1,
                "windows": 0,
            })
        );
        assert_eq!(instance["data"]["windows"], json!([]));
        assert_eq!(
            instance["data"]["applications"][0]["runningProcessSessionIds"],
            json!([])
        );
        assert_eq!(
            instance["data"]["processes"][0]["relatedApplicationIds"],
            json!([])
        );
    }

    #[test]
    fn v3_fixture_links_authenticated_uix_window_to_exact_process_only() {
        let process = process("uix-owner", "uix-demo");
        let process_id = process.session_id.clone();
        let window_id = OpaqueTargetId::new(OpaqueTargetKind::Window, "uix-window").to_string();
        let window_port = FixtureWindowPort {
            snapshot: WindowSnapshot {
                records: vec![WindowObservation {
                    session_id: window_id.clone(),
                    owner_process_session_id: Some(process_id.clone()),
                    title: "UIX Demo".to_owned(),
                    visible: true,
                    presentable: true,
                    revision: 8,
                    presented_revision: 8,
                }],
                available: true,
                complete: true,
                total: Some(1),
                truncated: Some(false),
                warnings: Vec::new(),
            },
        };
        let instance = discover_v3_with(
            &fixture(
                vec![application("desktop-generation", "Fixture App")],
                vec![process],
            ),
            &window_port,
            16,
            16,
            16,
        )
        .expect("UIX-aware fixture aggregation must succeed");
        validate_v3_schema(&instance);
        assert_eq!(
            instance["data"]["processes"][0]["windowSessionIds"],
            json!([window_id])
        );
        assert_eq!(instance["data"]["processes"][0]["hasVisibleWindow"], true);
        assert_eq!(
            instance["data"]["windows"][0]["ownerProcessSessionId"],
            process_id
        );
        assert_eq!(
            instance["data"]["applications"][0]["runningProcessSessionIds"],
            json!([])
        );
    }

    #[test]
    fn v4_fixture_adds_only_authenticated_launch_status() {
        let window_port = FixtureWindowPort {
            snapshot: WindowSnapshot {
                records: Vec::new(),
                available: true,
                complete: true,
                total: Some(0),
                truncated: Some(false),
                warnings: Vec::new(),
            },
        };
        let Ok(instance) = discover_v4_with(
            &fixture(
                vec![
                    launchable_application("launchable", "Launchable"),
                    application("ordinary", "Ordinary"),
                ],
                Vec::new(),
            ),
            &window_port,
            16,
            16,
            16,
        ) else {
            panic!("launch-aware fixture aggregation must succeed");
        };
        validate_v4_schema(&instance);
        assert_eq!(
            instance["data"]["applicationContract"],
            capabilities::APPLICATION_DISCOVER_V3
        );
        assert_eq!(
            instance["data"]["applications"][0]["launchCapability"],
            "available-confirmed"
        );
        assert_eq!(
            instance["data"]["applications"][1]["launchCapability"],
            "unavailable"
        );
        assert!(
            instance["data"]["applications"]
                .as_array()
                .is_some_and(|applications| applications.iter().all(|application| {
                    application["runningProcessSessionIds"] == json!([])
                        && application["relationshipEvidence"] == "none"
                }))
        );
    }

    #[test]
    fn fixture_empty_and_unknown_totals_remain_explicitly_partial() {
        let empty = discover_with(&fixture(Vec::new(), Vec::new()), 1, 1)
            .expect("empty fixture must succeed");
        validate_schema(&empty);
        assert_eq!(empty["data"]["total"]["applications"], 0);
        assert_eq!(empty["data"]["truncated"]["processes"], false);

        let mut partial = fixture(Vec::new(), vec![process("partial", "partial")]);
        partial.applications.available = false;
        partial.applications.complete = false;
        partial.applications.total = None;
        partial
            .applications
            .warnings
            .push("xdg-application-roots-missing");
        partial.processes.complete = false;
        partial.processes.total = None;
        let instance = discover_with(&partial, 4, 4).expect("partial fixture must succeed");
        validate_schema(&instance);
        assert_eq!(instance["data"]["total"]["applications"], Value::Null);
        assert_eq!(instance["data"]["truncated"]["processes"], Value::Null);
        assert_eq!(instance["data"]["complete"]["windows"], false);
        assert!(
            instance["data"]["warnings"]
                .as_array()
                .is_some_and(|warnings| {
                    warnings
                        .iter()
                        .any(|warning| warning["code"] == "process-snapshot-incomplete")
                })
        );
    }

    #[test]
    fn fixture_sorts_before_independent_application_and_process_truncation() {
        let fixture = fixture(
            vec![application("z", "Zulu"), application("a", "Alpha")],
            vec![process("z", "zeta"), process("a", "alpha")],
        );
        let first = discover_with(&fixture, 1, 2).expect("application truncation must succeed");
        let second = discover_with(&fixture, 2, 1).expect("process truncation must succeed");
        validate_schema(&first);
        validate_schema(&second);
        assert_eq!(first["data"]["applications"][0]["displayName"], "Alpha");
        assert_eq!(first["data"]["total"]["applications"], 2);
        assert_eq!(first["data"]["truncated"]["applications"], true);
        assert_eq!(second["data"]["processes"][0]["processName"], "alpha");
        assert_eq!(second["data"]["total"]["processes"], 2);
        assert_eq!(second["data"]["truncated"]["processes"], true);
    }

    #[test]
    fn policy_and_schema_reject_relationship_window_contract_and_count_drift() {
        let valid = discover_with(
            &fixture(
                vec![application("policy-app", "Policy App")],
                vec![process("policy-process", "policy-process")],
            ),
            4,
            4,
        )
        .expect("policy fixture must succeed");
        let schema = serde_json::from_str(include_str!(
            "../../contracts/v2/application-session-discovery.schema.json"
        ))
        .expect("v2 schema must parse");

        let mut mutations = Vec::new();
        let mut complete_window = valid.clone();
        complete_window["data"]["complete"]["windows"] = json!(true);
        mutations.push(complete_window);
        let mut nonempty_window = valid.clone();
        nonempty_window["data"]["windows"] = json!([{"sessionId": "s2:w:0000000000000001"}]);
        mutations.push(nonempty_window);
        let mut application_relation = valid.clone();
        application_relation["data"]["applications"][0]["runningProcessSessionIds"] =
            json!(["s2:p:0000000000000001"]);
        mutations.push(application_relation);
        let mut process_relation = valid.clone();
        process_relation["data"]["processes"][0]["relationshipStatus"] = json!("matched");
        mutations.push(process_relation);
        let mut launch = valid.clone();
        launch["data"]["applications"][0]["launchCapability"] = json!("available");
        mutations.push(launch);
        let mut contract = valid.clone();
        contract["data"]["applicationContract"] = json!("application.discover@3");
        mutations.push(contract);
        let mut atspi = valid.clone();
        atspi["data"]["applications"][0]["atspiObjectPath"] = json!("canary");
        mutations.push(atspi);
        let mut count = valid.clone();
        count["data"]["count"]["applications"] = json!(0);
        mutations.push(count);
        let mut total = valid.clone();
        total["data"]["total"]["applications"] = json!(0);
        mutations.push(total);
        let mut truncation = valid.clone();
        truncation["data"]["truncated"]["processes"] = json!(true);
        mutations.push(truncation);
        let mut warning_canary = valid.clone();
        warning_canary["data"]["warnings"][0]["message"] = json!("/private/canary.desktop");
        mutations.push(warning_canary);

        for mutation in mutations {
            assert!(
                validate_public_policy(&mutation).is_err()
                    || jsonschema::draft202012::validate(&schema, &mutation).is_err()
            );
        }
    }
}
