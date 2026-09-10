#![cfg(target_os = "linux")]

//! Linux procfs 纵切的真实契约回归。

use ai_computer_toolkit::{AppControlService, cli};
use serde_json::Value;

fn linux_process_session() -> Result<Value, Box<dyn std::error::Error>> {
    let service = AppControlService::new();
    let mut request = ai_computer_toolkit::domain::CommandRequest::read(
        ai_computer_toolkit::domain::Verb::Sessions,
        "process",
    );
    request.max_items = 4_096;
    let result = service.execute(request)?;
    let current_name = std::fs::read_to_string("/proc/self/comm")?
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    result["sessions"]
        .as_array()
        .and_then(|sessions| {
            sessions
                .iter()
                .find(|session| session["processName"] == current_name)
        })
        .cloned()
        .ok_or_else(|| "current Linux test process must be observable".into())
}

#[test]
fn linux_basic_cli_entries_and_procfs_status_are_runnable() -> Result<(), Box<dyn std::error::Error>>
{
    for argv in [
        vec!["version".to_owned()],
        vec!["build-info".to_owned()],
        vec!["status".to_owned(), "process".to_owned()],
    ] {
        let output = cli::run(argv)?;
        assert_eq!(output.json["ok"], true);
    }
    let status = cli::run(vec!["status".to_owned(), "process".to_owned()])?.json;
    assert_eq!(status["backend"], "Linux procfs process snapshot");
    assert_eq!(status["backgroundPolicy"], "guaranteed");
    let termination_available = status["capabilities"]
        .as_array()
        .is_some_and(|capabilities| {
            capabilities
                .iter()
                .any(|capability| capability == "process.terminate.graceful@2")
        });
    let force_available = status["capabilities"]
        .as_array()
        .is_some_and(|capabilities| {
            capabilities
                .iter()
                .any(|capability| capability == "process.terminate.force@2")
        });
    assert_eq!(force_available, termination_available);
    assert_eq!(status["readOnly"], !termination_available);
    assert_eq!(
        status["terminationProviderState"],
        if termination_available {
            "available-procfs-owner-generation-bound-same-non-root-uid-pidfd"
        } else {
            "unavailable"
        }
    );
    Ok(())
}

#[test]
fn linux_process_sessions_inspect_and_assessment_form_one_real_slice()
-> Result<(), Box<dyn std::error::Error>> {
    let session = linux_process_session()?;
    let session_id = session["sessionId"]
        .as_str()
        .ok_or("sessionId must be a string")?;
    assert!(session_id.starts_with("s2:p:"));
    for forbidden in ["pid", "processId", "path", "executablePath", "token", "sid"] {
        assert!(session.get(forbidden).is_none());
    }

    let inspect = cli::run(vec![
        "inspect".to_owned(),
        "process".to_owned(),
        "--target".to_owned(),
        format!("sessionId={session_id}"),
    ])?
    .json;
    assert_eq!(inspect["capability"], "process.metadata.read@1");
    assert_eq!(inspect["process"]["sessionId"], session_id);
    assert_eq!(inspect["executionDomain"], "host-headless");

    let assessment = cli::run(vec![
        "assess".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "process.metadata.read@1".to_owned(),
        "--target".to_owned(),
        format!("sessionId={session_id}"),
    ])?
    .json;
    assert_eq!(assessment["decision"], "executable-background");
    assert_eq!(assessment["executionRealm"], "host-headless");
    assert_eq!(assessment["constraints"]["noFallback"], true);
    Ok(())
}

#[test]
fn linux_xdg_discovery_and_independent_uix_window_provider_are_honest()
-> Result<(), Box<dyn std::error::Error>> {
    let discovery = cli::run(vec![
        "discover".to_owned(),
        "app".to_owned(),
        "--max-processes".to_owned(),
        "64".to_owned(),
    ])?
    .json;
    assert_eq!(discovery["ok"], true);
    assert_eq!(discovery["capability"], "application.discover@2");
    assert_eq!(discovery["data"]["readOnly"], true);
    assert_eq!(
        discovery["data"]["coverage"]["runningProcesses"],
        "available-linux-procfs"
    );
    assert_eq!(
        discovery["data"]["coverage"]["visibleTitledWindows"],
        "unavailable-no-linux-desktop-provider"
    );
    assert!(matches!(
        discovery["data"]["coverage"]["desktopEntryApplications"].as_str(),
        Some("available-xdg-desktop-entry" | "unavailable-no-xdg-data-roots")
    ));
    let applications = discovery["data"]["applications"]
        .as_array()
        .ok_or("applications must be an array")?;
    assert!(applications.iter().all(|application| {
        application["runningProcessSessionIds"] == serde_json::json!([])
            && application["relationshipEvidence"] == "none"
            && application["launchCapability"] == "unavailable"
    }));
    assert_eq!(
        discovery["data"]["applicationsReturned"],
        applications.len()
    );
    if discovery["data"]["coverage"]["desktopEntryApplications"] == "unavailable-no-xdg-data-roots"
    {
        assert!(applications.is_empty());
        assert_eq!(discovery["data"]["complete"]["applications"], false);
    }
    let host_target = discovery["data"]["hostTargetId"]
        .as_str()
        .ok_or("hostTargetId must be a string")?;
    let assessment = cli::run(vec![
        "assess".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "application.discover@2".to_owned(),
        "--target".to_owned(),
        format!("sessionId={host_target}"),
    ])?
    .json;
    assert_eq!(assessment["decision"], "executable-background");
    assert_eq!(assessment["executionRealm"], "host-headless");
    assert_eq!(assessment["constraints"]["noFallback"], true);
    assert_eq!(discovery["data"]["windows"], serde_json::json!([]));

    let window = cli::run(vec!["status".to_owned(), "window".to_owned()])?.json;
    assert_eq!(window["provider"], "uix-agent-v1");
    assert_eq!(window["globalWindowDirectory"], false);
    assert_eq!(window["arbitraryThirdPartyApplications"], false);
    assert_eq!(window["fallback"], "none");
    assert_eq!(window["inputAllowed"], false);
    Ok(())
}

#[test]
fn linux_capability_matrix_marks_only_certified_slice_available()
-> Result<(), Box<dyn std::error::Error>> {
    let result = cli::run(vec!["capabilities".to_owned()])?.json;
    let capabilities = result["data"]["capabilities"]
        .as_array()
        .ok_or("capabilities must be an array")?;
    let status = |id: &str| {
        capabilities
            .iter()
            .find(|entry| entry["id"] == id)
            .and_then(|entry| entry["status"].as_str())
    };
    assert_eq!(status("process.discover@1"), Some("available"));
    assert_eq!(status("process.metadata.read@1"), Some("available"));
    assert!(matches!(
        status("process.terminate.graceful@2"),
        Some(
            "available-verified-procfs-owner-generation-bound-same-non-root-uid-pidfd"
                | "unavailable-no-linux-provider"
        )
    ));
    assert!(matches!(
        status("process.terminate.force@2"),
        Some(
            "available-verified-procfs-owner-generation-bound-same-non-root-uid-pidfd"
                | "unavailable-no-linux-provider"
        )
    ));
    assert_eq!(
        status("application.discover@1"),
        Some("unavailable-no-linux-provider")
    );
    assert_eq!(
        status("application.discover@2"),
        Some("available-partial-linux-xdg-desktop-entry-and-procfs")
    );
    assert_eq!(
        status("application.discover@3"),
        Some("available-verified-linux-xdg-procfs-with-toolkit-fixture-launch-status")
    );
    assert_eq!(
        status("application.open@2"),
        Some("available-route-toolkit-self-executable-fixture-only")
    );
    assert_eq!(
        status("application.session.discover@4"),
        Some("available-verified-read-only-linux-launch-aware-uix-aggregation")
    );
    assert_eq!(
        status("window.discover@1"),
        Some("unavailable-no-linux-provider")
    );
    assert_eq!(
        status("ui.input.key@1"),
        Some("unavailable-no-linux-provider")
    );
    Ok(())
}
