//! 生产 CLI 的媒体 v3 注册、封闭 schema 和零 provider 访问拒绝回归。
#![cfg(target_os = "linux")]

use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn invoke(
    args: &[&str],
    input: Option<Value>,
) -> Result<(Output, Value), Box<dyn std::error::Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ai-computer-toolkit"))
        .args(args)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(input) = input {
        let mut stdin = child.stdin.take().ok_or("stdin")?;
        if let Err(error) = writeln!(stdin, "{input}")
            && error.kind() != std::io::ErrorKind::BrokenPipe
        {
            return Err(error.into());
        }
    }
    drop(child.stdin.take());
    let output = child.wait_with_output()?;
    let value = serde_json::from_slice(&output.stdout)?;
    Ok((output, value))
}

#[test]
fn production_registers_app_v3_but_keeps_media_v2_candidate_closed() -> TestResult {
    let (output, status) = invoke(&["status", "app"], None)?;
    assert!(output.status.success());
    for id in [
        "media.session.discover@3",
        "media.playback.state.read@3",
        "media.playback.control@3",
    ] {
        assert!(
            status["capabilities"]
                .as_array()
                .ok_or("capabilities")?
                .contains(&json!(id))
        );
    }
    let (_, catalog) = invoke(&["capabilities"], None)?;
    for version in [2, 3] {
        for name in [
            "media.session.discover",
            "media.playback.state.read",
            "media.playback.control",
        ] {
            let id = format!("{name}@{version}");
            let item = catalog["data"]["capabilities"]
                .as_array()
                .ok_or("catalog")?
                .iter()
                .find(|item| item["id"] == id)
                .ok_or("registered capability")?;
            if version == 2 {
                assert_eq!(item["linuxClassification"], "candidate");
                assert!(item.get("executionDomain").is_none());
            } else {
                assert_eq!(item["executionDomain"], "isolated-worker");
                assert_eq!(
                    item["linuxClassification"],
                    "production-route-target-assessment-required"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn production_self_workers_reject_invalid_protocol_before_provider_access() -> TestResult {
    for hidden in [
        "__linux-mpris-observation-worker-v1",
        "__linux-mpris-control-worker-v1",
    ] {
        let (output, result) = invoke(&[hidden], Some(json!({})))?;
        assert!(!output.status.success());
        assert!(output.stderr.is_empty());
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"]["code"], "WORKER_PROTOCOL_ERROR");
    }
    Ok(())
}

#[test]
fn confirmation_precedes_even_an_unreadable_input_source() -> TestResult {
    let (output, result) = invoke(
        &[
            "run",
            "app",
            "apply",
            "--capability",
            "media.playback.control@3",
            "--input",
            "/act-missing-media-input/not-present.json",
        ],
        None,
    )?;
    assert!(!output.status.success());
    assert_eq!(result["error"]["code"], "CONFIRMATION_REQUIRED");
    Ok(())
}

#[test]
fn public_control_rejects_unpublished_operations_and_native_arguments() -> TestResult {
    for input in [
        json!({"operation": "Raise"}),
        json!({"operation": "togglePlayPause"}),
        json!({"operation": "pause", "sessionBusAddress": "unix:path=/tmp/other-bus"}),
        json!({"operation": "pause", "script": "document.body.click()"}),
        json!({"operation": "pause", "timeoutMs": 0}),
    ] {
        let (output, result) = invoke(
            &[
                "run",
                "app",
                "apply",
                "--capability",
                "media.playback.control@3",
                "--target",
                "sessionId=s2:m:0123456789abcdef",
                "--confirm",
                "--strict-isolation",
                "--input",
                "-",
            ],
            Some(input),
        )?;
        assert!(!output.status.success());
        assert_eq!(result["error"]["code"], "INVALID_ARGUMENT", "{result}");
    }
    Ok(())
}

#[test]
fn state_rejects_wrong_surface_and_foreground_consent_without_io() -> TestResult {
    for extra in [
        vec!["--target", "sessionId=s2:w:0123456789abcdef"],
        vec![
            "--target",
            "sessionId=s2:m:0123456789abcdef",
            "--allow-foreground",
        ],
    ] {
        let mut args = vec![
            "run",
            "app",
            "read",
            "--capability",
            "media.playback.state.read@3",
            "--strict-isolation",
        ];
        args.extend(extra);
        let (output, result) = invoke(&args, None)?;
        assert!(!output.status.success());
        assert_eq!(result["error"]["code"], "INVALID_ARGUMENT");
    }
    Ok(())
}

#[test]
fn discovery_has_empty_target_and_bounded_closed_input() -> TestResult {
    for (extra, input) in [
        (vec![], json!({"maximumItems": 129})),
        (vec![], json!({"timeoutMs": 30001})),
        (
            vec!["--target", "sessionId=s2:m:0123456789abcdef"],
            json!({}),
        ),
    ] {
        let mut args = vec![
            "run",
            "app",
            "discover",
            "--capability",
            "media.session.discover@3",
            "--input",
            "-",
        ];
        args.extend(extra);
        let (output, result) = invoke(&args, Some(input))?;
        assert!(!output.status.success());
        assert_eq!(result["error"]["code"], "INVALID_ARGUMENT", "{result}");
    }
    Ok(())
}

#[test]
fn input_schemas_accept_supported_shapes_and_reject_escape_fields() -> TestResult {
    let schemas = [
        (
            include_str!("../contracts/v3/media-session-discover.schema.json"),
            json!({"maximumItems": 128, "timeoutMs": 30000}),
        ),
        (
            include_str!("../contracts/v3/media-playback-state.schema.json"),
            json!({"timeoutMs": 1}),
        ),
        (
            include_str!("../contracts/v3/media-playback-control.schema.json"),
            json!({"operation": "pause"}),
        ),
    ];
    for (source, valid) in schemas {
        let schema: Value = serde_json::from_str(source)?;
        let validator = jsonschema::draft202012::new(&schema)?;
        assert!(validator.is_valid(&valid));
        for field in [
            "script",
            "address",
            "method",
            "foregroundConsent",
            "confirmed",
        ] {
            let mut invalid = valid.clone();
            invalid[field] = json!(true);
            assert!(!validator.is_valid(&invalid));
        }
        for timeout in [json!(0), json!(30001), json!(1.5), json!("5000")] {
            let mut invalid = valid.clone();
            invalid["timeoutMs"] = timeout;
            assert!(!validator.is_valid(&invalid));
        }
    }
    Ok(())
}
