#![cfg(target_os = "linux")]

//! 默认 Linux 组合根的 AT-SPI v2 candidate、v1 冻结与零 dispatch 门禁。

use ai_computer_toolkit::cli;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn sha256(source: &str) -> String {
    format!("{:x}", Sha256::digest(source.as_bytes()))
}

#[test]
fn windows_v1_window_and_accessibility_schemas_remain_byte_frozen() {
    assert_eq!(
        sha256(include_str!(
            "../contracts/v1/window-observation.schema.json"
        )),
        "13804dac4e50ad0eba66ab42ec2aecb89a2d95b245ffcea46f1e5b9c755b7aac"
    );
    assert_eq!(
        sha256(include_str!(
            "../contracts/v1/accessibility-tree.schema.json"
        )),
        "512273fe54706b824519d2d15c2d92151761753f8fde81814e036a501532e7f2"
    );
    assert_eq!(
        sha256(include_str!(
            "../contracts/v1/window-target-identity.schema.json"
        )),
        "3652c7ad5bd152053299629d9424a4234a619a91c67c49d43f4bc553636ac899"
    );
}

#[test]
fn default_linux_surface_publishes_atspi_v2_as_candidate_without_realm()
-> Result<(), Box<dyn std::error::Error>> {
    let surface = cli::run(vec!["capabilities".to_owned()])?.json;
    let entries = surface["data"]["capabilities"]
        .as_array()
        .ok_or("entries missing")?;
    for id in [
        "window.discover@2",
        "window.metadata.read@2",
        "accessibility.tree.read@2",
    ] {
        let entry = entries
            .iter()
            .find(|entry| entry["id"] == id)
            .ok_or("candidate missing")?;
        assert_eq!(entry["linuxClassification"], "candidate");
        assert_eq!(
            entry["status"],
            "candidate-private-dbus-fixture-only-live-host-unavailable"
        );
        assert!(entry.get("executionDomain").is_none());
        assert_eq!(
            entry["constraint"],
            "partial-accessibility-exporters-read-only-private-fixture-no-live-route"
        );
    }
    Ok(())
}

#[test]
fn production_atspi_assessment_remains_unavailable_without_bus_access()
-> Result<(), Box<dyn std::error::Error>> {
    let discovery = cli::run(vec![
        "discover".to_owned(),
        "app".to_owned(),
        "--max-applications".to_owned(),
        "1".to_owned(),
        "--max-processes".to_owned(),
        "1".to_owned(),
    ])?
    .json;
    let host = discovery["data"]["hostTargetId"]
        .as_str()
        .ok_or("host missing")?;
    let discover = cli::run(vec![
        "assess".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "window.discover@2".to_owned(),
        "--target".to_owned(),
        format!("sessionId={host}"),
    ])?
    .json;
    assert_candidate_unavailable(&discover);
    for capability in ["window.metadata.read@2", "accessibility.tree.read@2"] {
        let assessment = cli::run(vec![
            "assess".to_owned(),
            "app".to_owned(),
            "--capability".to_owned(),
            capability.to_owned(),
            "--target".to_owned(),
            "sessionId=s2:w:0000000000000000".to_owned(),
        ])?
        .json;
        assert_candidate_unavailable(&assessment);
    }
    // window surface 现由独立 UIX Agent v3 Provider 承接，但不得借此发布 AT-SPI v2。
    let status = cli::run(vec!["status".to_owned(), "window".to_owned()])?.json;
    assert_eq!(status["provider"], "uix-agent-v1");
    let capabilities = status["capabilities"]
        .as_array()
        .ok_or("capabilities missing")?;
    assert!(!capabilities.iter().any(|value| {
        matches!(
            value.as_str(),
            Some("window.discover@2" | "window.metadata.read@2")
        )
    }));
    Ok(())
}

fn assert_candidate_unavailable(value: &Value) {
    assert_eq!(value["decision"], "unavailable");
    assert_eq!(value["executionRealm"], "none");
    assert_eq!(value["requiresConfirmation"], false);
    assert_eq!(value["requiresForegroundConsent"], false);
    assert_eq!(value["constraints"]["readOnly"], true);
    assert_eq!(value["constraints"]["noFallback"], true);
    assert_eq!(
        value["evidence"]["implementationState"],
        "candidate-no-production-dispatch"
    );
    assert_eq!(value["evidence"]["inputAllowed"], false);
}

#[test]
fn private_adapter_has_no_forbidden_atspi_call_sites_or_implicit_bus_lookup() {
    let adapter = include_str!("../src/adapters/linux/atspi.rs");
    for forbidden_call in [
        "call(\"GetAll\"",
        "call(\"GetChildren\"",
        "call(\"GetItems\"",
        "call(\"GetApplicationBusAddress\"",
        "call(\"GrabFocus\"",
        "interface(\"org.a11y.atspi.Action\"",
        "interface(\"org.a11y.atspi.EditableText\"",
        "interface(\"org.a11y.atspi.Selection\"",
        "interface(\"org.a11y.atspi.Value\"",
    ] {
        assert!(
            !adapter.contains(forbidden_call),
            "forbidden AT-SPI call site: {forbidden_call}"
        );
    }
    assert!(adapter.contains("cache_properties(CacheProperties::No)"));
    assert!(!adapter.contains("Connection::session"));
    assert!(!adapter.contains("DBUS_SESSION_BUS_ADDRESS"));
    let linux_modules = include_str!("../src/modules_linux.rs");
    assert!(linux_modules.contains("cfg(feature = \"linux-atspi-candidate\")"));
    let release = include_str!("../src/linux_release.rs");
    assert!(!release.contains("atspi-observation-worker"));
}

#[test]
fn application_discovery_v2_and_application_session_v1_do_not_absorb_atspi() {
    for source in [
        include_str!("../src/modules/linux_application_discovery.rs"),
        include_str!("../contracts/v2/application-inventory.schema.json"),
        include_str!("../contracts/v1/application-session-discovery.schema.json"),
    ] {
        for forbidden in [
            "at-spi",
            "atspi",
            "window.discover@2",
            "accessibility.tree.read@2",
        ] {
            assert!(!source.to_ascii_lowercase().contains(forbidden));
        }
    }
}
