#![cfg(target_os = "linux")]

//! Linux application.session.discover@3 的真实主机 UIX 聚合与契约验收。

use std::collections::{BTreeMap, BTreeSet};

use ai_computer_toolkit::cli;
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
        "endpoint",
        "token",
        "windowId",
        "generation",
        "desktopFileId",
        "busAddress",
        "busName",
        "objectPath",
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

#[test]
fn live_linux_v3_is_schema_valid_bounded_private_and_relation_closed()
-> Result<(), Box<dyn std::error::Error>> {
    let result = cli::run(vec![
        "discover".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "application.session.discover@3".to_owned(),
        "--max-applications".to_owned(),
        "2".to_owned(),
        "--max-processes".to_owned(),
        "4096".to_owned(),
        "--max-windows".to_owned(),
        "4".to_owned(),
    ])?
    .json;
    let v3 = schema(include_str!(
        "../contracts/v3/application-session-discovery.schema.json"
    ));
    jsonschema::draft202012::validate(&v3, &result)
        .map_err(|_| std::io::Error::other("live v3 instance must validate"))?;
    assert_eq!(result["capability"], "application.session.discover@3");
    let data = &result["data"];
    assert_eq!(data["windowContract"], "window.discover@3");
    assert_eq!(data["coverage"]["windows"]["provider"], "uix-agent-v1");
    assert_eq!(data["foregroundUnchanged"], true);
    assert!(
        data["windows"]
            .as_array()
            .is_some_and(|items| items.len() <= 4)
    );
    assert!(data["applications"].as_array().is_some_and(|applications| {
        applications.iter().all(|application| {
            application["runningProcessSessionIds"] == serde_json::json!([])
                && application["relationshipEvidence"] == "none"
        })
    }));

    let windows = data["windows"]
        .as_array()
        .ok_or("windows must be an array")?
        .iter()
        .map(|window| {
            (
                window["sessionId"]
                    .as_str()
                    .expect("window ID must be text"),
                window["ownerProcessSessionId"].as_str(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for process in data["processes"]
        .as_array()
        .ok_or("processes must be an array")?
    {
        let process_id = process["sessionId"]
            .as_str()
            .ok_or("process ID must be text")?;
        let mut unique = BTreeSet::new();
        for window_id in process["windowSessionIds"]
            .as_array()
            .ok_or("windowSessionIds must be an array")?
        {
            let window_id = window_id.as_str().ok_or("window ID must be text")?;
            assert!(unique.insert(window_id));
            assert_eq!(windows.get(window_id), Some(&Some(process_id)));
        }
    }
    assert_no_private_keys(&result);
    Ok(())
}

#[test]
fn v3_route_metadata_assessment_and_option_closure_are_explicit()
-> Result<(), Box<dyn std::error::Error>> {
    let surface = cli::run(vec!["capabilities".to_owned()])?.json;
    let entry = surface["data"]["capabilities"]
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["id"] == "application.session.discover@3")
        })
        .ok_or("application session v3 metadata must exist")?;
    assert_eq!(
        entry["status"],
        "available-verified-read-only-linux-uix-aware-aggregation"
    );
    assert_eq!(entry["linuxClassification"], "verified");
    assert_eq!(entry["executionDomain"], "same-session-no-focus");

    let discovery = cli::run(vec![
        "discover".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "application.session.discover@3".to_owned(),
        "--max-windows".to_owned(),
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
        "application.session.discover@3".to_owned(),
        "--target".to_owned(),
        format!("sessionId={host}"),
    ])?
    .json;
    assert_eq!(assessment["decision"], "executable-background");
    assert_eq!(assessment["executionRealm"], "same-session-no-focus");
    assert_eq!(
        assessment["constraints"]["globalWindowDirectoryClaimed"],
        false
    );
    assert_eq!(assessment["constraints"]["noFallback"], true);

    let unused = require_error(cli::run(vec![
        "discover".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "application.session.discover@3".to_owned(),
        "--max-items".to_owned(),
        "1".to_owned(),
    ]));
    assert_eq!(unused.code, "INVALID_ARGUMENT");
    Ok(())
}
