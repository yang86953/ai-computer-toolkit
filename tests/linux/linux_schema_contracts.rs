#![cfg(target_os = "linux")]

//! 真实 Linux CLI 实例的 Draft 2020-12 公共契约回归。

use ai_computer_toolkit::{cli, domain::error_json};
use serde_json::Value;

fn schema(source: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(source)?)
}

fn assert_valid(schema: &Value, instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    jsonschema::draft202012::validate(schema, instance)
        .map_err(|error| format!("JSON Schema validation failed: {error}").into())
}

#[test]
fn real_linux_capability_surface_validates_against_public_schema()
-> Result<(), Box<dyn std::error::Error>> {
    let instance = cli::run(vec!["capabilities".to_owned()])?.json;
    let schema = schema(include_str!("../../contracts/v1/capabilities.schema.json"))?;
    assert_eq!(
        instance["data"]["platformPolicy"]["desktopProtocol"],
        "wayland-only"
    );
    // 真实主机可能没有 Wayland/Portal；host-foreground 正例由私有 hermetic fixture 覆盖。
    assert_valid(&schema, &instance)
}

#[test]
fn real_linux_unavailable_error_validates_against_public_schema()
-> Result<(), Box<dyn std::error::Error>> {
    let error = cli::run(vec!["status".to_owned(), "uia".to_owned()])
        .err()
        .ok_or("unsupported Linux UIA status must fail")?;
    assert_eq!(error.code, "CAPABILITY_UNAVAILABLE");
    let instance = error_json(&error);
    let schema = schema(include_str!("../../contracts/v1/error-envelope.schema.json"))?;
    assert_valid(&schema, &instance)
}

#[test]
fn linux_production_sources_do_not_emit_undeclared_provider_unavailable_code() {
    for source in [
        include_str!("../../src/adapters/linux/process.rs"),
        include_str!("../../src/modules/linux_application_discovery.rs"),
        include_str!("../../src/modules/linux_capability_assessment.rs"),
    ] {
        assert!(!source.contains("PROVIDER_UNAVAILABLE"));
    }
}

#[test]
fn undeclared_provider_unavailable_is_rejected_by_error_schema()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = schema(include_str!("../../contracts/v1/error-envelope.schema.json"))?;
    let instance = serde_json::json!({
        "ok": false,
        "error": {
            "code": "PROVIDER_UNAVAILABLE",
            "message": "must remain undeclared"
        }
    });
    assert!(jsonschema::draft202012::validate(&schema, &instance).is_err());
    Ok(())
}

#[test]
fn real_linux_application_discovery_validates_against_v2_contracts()
-> Result<(), Box<dyn std::error::Error>> {
    let instance = cli::run(vec![
        "discover".to_owned(),
        "app".to_owned(),
        "--max-applications".to_owned(),
        "64".to_owned(),
        "--max-processes".to_owned(),
        "64".to_owned(),
    ])?
    .json;
    let inventory_schema = schema(include_str!(
        "../../contracts/v2/application-inventory.schema.json"
    ))?;
    let identity_schema = schema(include_str!(
        "../../contracts/v1/application-target-identity.schema.json"
    ))?;
    assert_eq!(instance["capability"], "application.discover@2");
    let applications = instance["data"]["applications"]
        .as_array()
        .ok_or("applications must be an array")?;
    assert_valid(&inventory_schema, &instance)?;
    assert_eq!(instance["data"]["applicationsReturned"], applications.len());
    if let Some(total) = instance["data"]["totalEligibleApplications"].as_u64() {
        assert_eq!(
            instance["data"]["applicationsTruncated"],
            total > applications.len() as u64
        );
    }
    assert!(
        instance["data"]["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings.iter().all(|warning| warning
                .as_str()
                .is_some_and(|warning| !warning.contains('/'))))
    );
    for application in applications {
        assert_valid(&identity_schema, &application["targetIdentity"])?;
        assert_eq!(
            application["runningProcessSessionIds"],
            serde_json::json!([])
        );
        assert_eq!(application["relationshipEvidence"], "none");
        assert_eq!(application["launchCapability"], "unavailable");
    }
    let serialized = serde_json::to_string(&instance)?;
    for forbidden in ["Exec=", "TryExec=", "/applications/"] {
        assert!(
            !serialized.contains(forbidden),
            "public inventory leaked {forbidden}"
        );
    }
    for application in applications {
        for forbidden in ["desktopFileId", "path", "exec", "tryExec", "contentDigest"] {
            assert!(application.get(forbidden).is_none());
        }
    }
    Ok(())
}
