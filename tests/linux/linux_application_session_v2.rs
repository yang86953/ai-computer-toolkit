#![cfg(target_os = "linux")]

//! Linux application.session.discover@2 的真实主机隐私与契约验收。

use std::collections::BTreeSet;

use ai_computer_toolkit::cli;
use jsonschema::{Draft, Registry, Resource};
use serde_json::Value;

fn schema(source: &str) -> Value {
    serde_json::from_str(source).expect("contract schema must parse")
}

fn require_error(
    result: ai_computer_toolkit::domain::AppResult<cli::CliOutput>,
) -> ai_computer_toolkit::domain::AppControlError {
    match result {
        Ok(_) => panic!("fixture request must fail"),
        Err(error) => error,
    }
}

fn assert_no_private_keys(value: &Value) {
    const FORBIDDEN: &[&str] = &[
        "pid",
        "processId",
        "nativePid",
        "uid",
        "path",
        "exec",
        "tryExec",
        "desktopFileId",
        "busAddress",
        "busName",
        "objectPath",
        "atspiObjectPath",
        "applicationId",
        "windowTitle",
    ];
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                assert!(!FORBIDDEN.contains(&key.as_str()));
                assert_no_private_keys(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                assert_no_private_keys(item);
            }
        }
        _ => {}
    }
}

fn assert_dimension(data: &Value, name: &str, maximum: usize) {
    let records = data[name]
        .as_array()
        .expect("session dimension must be an array");
    assert!(records.len() <= maximum);
    assert_eq!(data["count"][name].as_u64(), Some(records.len() as u64));
    match (
        data["total"][name].as_u64(),
        data["truncated"][name].as_bool(),
    ) {
        (Some(total), Some(false)) => assert_eq!(total, records.len() as u64),
        (Some(total), Some(true)) => assert!(total > records.len() as u64),
        (None, Some(true) | None) => {}
        _ => panic!("total and truncated facts must remain closed"),
    }
}

#[test]
fn live_linux_v2_is_schema_valid_private_bounded_and_relationship_free()
-> Result<(), Box<dyn std::error::Error>> {
    let result = cli::run(vec![
        "discover".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "application.session.discover@2".to_owned(),
        "--max-applications".to_owned(),
        "2".to_owned(),
        "--max-processes".to_owned(),
        "3".to_owned(),
    ])?
    .json;
    let v2 = schema(include_str!(
        "../../contracts/v2/application-session-discovery.schema.json"
    ));
    jsonschema::draft202012::validate(&v2, &result)
        .map_err(|_| std::io::Error::other("live v2 instance must validate"))?;
    assert_eq!(result["capability"], "application.session.discover@2");
    let data = &result["data"];
    assert_eq!(data["applicationContract"], "application.discover@2");
    assert_eq!(data["processContract"], "process.discover@1");
    assert_eq!(data["readOnly"], true);
    assert_eq!(data["foregroundUnchanged"], true);
    assert_dimension(data, "applications", 2);
    assert_dimension(data, "processes", 3);
    assert_eq!(data["windows"], serde_json::json!([]));
    assert_eq!(data["total"]["windows"], Value::Null);
    assert_eq!(data["truncated"]["windows"], Value::Null);
    assert_eq!(data["complete"]["windows"], false);
    assert!(data["applications"].as_array().is_some_and(|applications| {
        applications.iter().all(|application| {
            application["runningProcessSessionIds"] == serde_json::json!([])
                && application["relationshipEvidence"] == "none"
                && application["launchCapability"] == "unavailable"
        })
    }));
    assert!(data["processes"].as_array().is_some_and(|processes| {
        processes.iter().all(|process| {
            process["relatedApplicationIds"] == serde_json::json!([])
                && process["relationshipStatus"] == "unassociated"
                && process["windowSessionIds"] == serde_json::json!([])
        })
    }));
    let warning_codes = data["warnings"]
        .as_array()
        .expect("warnings must be an array")
        .iter()
        .map(|warning| warning["code"].as_str().expect("warning code must be text"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        warning_codes.len(),
        data["warnings"].as_array().unwrap().len()
    );
    assert!(warning_codes.contains("window-provider-unavailable"));
    assert_no_private_keys(&result);

    let v1 = schema(include_str!(
        "../../contracts/v1/application-session-discovery.schema.json"
    ));
    let identity = schema(include_str!(
        "../../contracts/v1/window-target-identity.schema.json"
    ));
    let registry = Registry::new()
        .draft(Draft::Draft202012)
        .add(
            "schema://application/session-discover/window-target-identity.schema.json",
            Resource::from_contents(identity),
        )?
        .prepare()?;
    let validator = jsonschema::draft202012::options()
        .with_registry(&registry)
        .build(&v1)?;
    assert!(!validator.is_valid(&result));
    Ok(())
}

#[test]
fn explicit_v2_route_preserves_linux_defaults_v1_gap_and_option_closure()
-> Result<(), Box<dyn std::error::Error>> {
    let default = cli::run(vec!["discover".to_owned(), "app".to_owned()])?.json;
    assert_eq!(default["capability"], "application.discover@2");

    // sessions app 现在只枚举 opt-in UIX 窗口，不得冒充冻结的应用关系聚合 v1。
    let uix_sessions = cli::run(vec!["sessions".to_owned(), "app".to_owned()])?.json;
    assert_eq!(uix_sessions["provider"], "uix-agent-v1");
    assert_eq!(uix_sessions["coverage"], "opt-in-uix-agent-applications");
    assert!(uix_sessions.get("capability").is_none());

    let surface = cli::run(vec!["capabilities".to_owned()])?.json;
    let entries = surface["data"]["capabilities"]
        .as_array()
        .ok_or("capability entries must be an array")?;
    let v1 = entries
        .iter()
        .find(|entry| entry["id"] == "application.session.discover@1")
        .ok_or("application session v1 metadata must exist")?;
    let v2 = entries
        .iter()
        .find(|entry| entry["id"] == "application.session.discover@2")
        .ok_or("application session v2 metadata must exist")?;
    assert_eq!(v1["status"], "unavailable-no-linux-provider");
    assert_eq!(
        v1["linuxClassification"],
        "conditional-or-permanent-unavailable"
    );
    assert_eq!(
        v2["status"],
        "available-verified-read-only-linux-xdg-procfs-aggregation"
    );
    assert_eq!(v2["linuxClassification"], "verified");

    let unused = require_error(cli::run(vec![
        "discover".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "application.session.discover@2".to_owned(),
        "--max-windows".to_owned(),
        "1".to_owned(),
    ]));
    assert_eq!(unused.code, "INVALID_ARGUMENT");

    let unknown = require_error(cli::run(vec![
        "discover".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "application.session.discover@99".to_owned(),
    ]));
    assert_eq!(unknown.code, "INVALID_ARGUMENT");
    Ok(())
}

#[test]
fn v2_assessment_accepts_only_the_current_host_target() -> Result<(), Box<dyn std::error::Error>> {
    let discovery = cli::run(vec![
        "discover".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "application.session.discover@2".to_owned(),
        "--max-applications".to_owned(),
        "1".to_owned(),
        "--max-processes".to_owned(),
        "1".to_owned(),
    ])?
    .json;
    let host = discovery["data"]["hostTargetId"]
        .as_str()
        .ok_or("host target must be text")?;
    let assessment = cli::run(vec![
        "assess".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "application.session.discover@2".to_owned(),
        "--target".to_owned(),
        format!("sessionId={host}"),
    ])?
    .json;
    assert_eq!(assessment["decision"], "executable-background");
    assert_eq!(assessment["executionRealm"], "host-headless");
    assert_eq!(assessment["constraints"]["readOnly"], true);
    assert_eq!(assessment["constraints"]["noFallback"], true);

    let stale = require_error(cli::run(vec![
        "assess".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "application.session.discover@2".to_owned(),
        "--target".to_owned(),
        "sessionId=s2:h:0000000000000000".to_owned(),
    ]));
    assert_eq!(stale.code, "STALE_SESSION");
    Ok(())
}
