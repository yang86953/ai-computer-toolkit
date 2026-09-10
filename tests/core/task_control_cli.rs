//! 原样生产二进制的任务授权、UIX CLI、无显示服务和封闭传输回归。

use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

const TASK: &str = "t2:0123456789abcdef0123456789abcdef";
static NEXT: AtomicU64 = AtomicU64::new(0);
type TestResult = Result<(), Box<dyn std::error::Error>>;

struct GrantFile {
    directory: PathBuf,
    file: PathBuf,
}

impl GrantFile {
    fn new(value: &Value) -> Result<Self, Box<dyn std::error::Error>> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "act-task-contract-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory)?;
        let file = directory.join("grant.json");
        fs::write(&file, serde_json::to_vec(value)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o600))?;
        }
        Ok(Self { directory, file })
    }
}

impl Drop for GrantFile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn grant() -> Value {
    json!({
        "contractVersion": "act/task-grant/v1", "taskId": TASK,
        "authorizationSource": "user-task", "totalTimeoutMs": 5000,
        "maxExecutions": 8, "allowDiscovery": true,
        "permissions": [{ "capability": "ui.element.action@2", "targetId": "s2:w:0123456789abcdef", "requiredInput": { "action": "invoke" } }]
    })
}

fn request(id: &str, operation: Value) -> Value {
    json!({ "contractVersion": "act/control/v2", "requestId": id, "taskId": TASK, "operation": operation })
}

fn command(grant: &GrantFile) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ai-computer-toolkit"));
    command
        .args(["serve", "--stdio"])
        .arg(format!("--grant-file={}", grant.file.display()))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn run(
    grant: &Value,
    frames: &[Value],
) -> Result<(Output, Vec<Value>), Box<dyn std::error::Error>> {
    let grant = GrantFile::new(grant)?;
    let mut child = command(&grant).spawn()?;
    let mut input = child.stdin.take().ok_or("stdin is missing")?;
    for frame in frames {
        writeln!(input, "{}", serde_json::to_string(frame)?)?;
    }
    drop(input);
    let output = child.wait_with_output()?;
    let responses = String::from_utf8(output.stdout.clone())?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    #[cfg(target_os = "linux")]
    {
        let schema: Value = serde_json::from_str(include_str!(
            "../../contracts/v2/task-control-response.schema.json"
        ))?;
        for response in &responses {
            jsonschema::draft202012::validate(&schema, response)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok((output, responses))
}

#[test]
fn native_uix_cli_serves_multiple_json_requests_without_a_display() -> TestResult {
    let (output, responses) = run(
        &grant(),
        &[
            request("open", json!({ "type": "task.open" })),
            request("catalog", json!({ "type": "catalog" })),
            request("status", json!({ "type": "task.status" })),
            request("close", json!({ "type": "task.close" })),
        ],
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(responses.len(), 4);
    for response in &responses {
        assert_eq!(response["contractVersion"], "act/control/v2");
        assert_eq!(response["taskId"], TASK);
        assert_eq!(response["ok"], true);
    }
    assert_eq!(responses[0]["data"]["state"], "active");
    assert_eq!(responses[0]["data"]["perActionPrompt"], false);
    assert!(
        responses[1]["data"]["capabilities"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert_eq!(
        responses[2]["data"]["requirements"]["imageDependency"],
        "forbidden"
    );
    assert_eq!(responses[3]["data"]["state"], "closed");
    Ok(())
}

#[test]
fn repeated_request_replays_receipt_but_changed_intent_is_rejected() -> TestResult {
    let open = request("same-id", json!({ "type": "task.open" }));
    let (output, responses) = run(
        &grant(),
        &[
            open.clone(),
            open,
            request("same-id", json!({ "type": "catalog" })),
            request("close", json!({ "type": "task.close" })),
        ],
    )?;
    assert!(output.status.success());
    assert_eq!(responses.len(), 4);
    assert_eq!(responses[0], responses[1]);
    assert_eq!(responses[2]["error"]["code"], "REQUEST_ID_CONFLICT");
    Ok(())
}

#[test]
fn task_scope_precedes_target_resolution_and_input_constraints_are_enforced() -> TestResult {
    let (output, responses) = run(
        &grant(),
        &[
            request("open", json!({ "type": "task.open" })),
            request(
                "wrong-target",
                json!({ "type": "execute", "capability": "ui.element.action@2", "targetId": "s2:w:ffffffffffffffff", "input": { "action": "invoke" } }),
            ),
            request(
                "wrong-input",
                json!({ "type": "execute", "capability": "ui.element.action@2", "targetId": "s2:w:0123456789abcdef", "input": { "action": "toggle" } }),
            ),
            request("close", json!({ "type": "task.close" })),
        ],
    )?;
    assert!(output.status.success());
    assert_eq!(responses[1]["error"]["code"], "TASK_SCOPE_VIOLATION");
    assert_eq!(responses[2]["error"]["code"], "TASK_SCOPE_VIOLATION");
    assert_eq!(
        responses[1]["error"]["details"]["outcome"],
        "not-dispatched"
    );
    assert_eq!(responses[3]["data"]["executions"], 0);
    Ok(())
}

#[test]
fn no_focus_capability_does_not_implicitly_certify_minimized_execution() -> TestResult {
    let (output, responses) = run(
        &grant(),
        &[
            request("open", json!({ "type": "task.open" })),
            request(
                "assess",
                json!({ "type": "assess", "capability": "ui.element.action@2", "targetId": "s2:w:0123456789abcdef" }),
            ),
            request(
                "execute",
                json!({ "type": "execute", "capability": "ui.element.action@2", "targetId": "s2:w:0123456789abcdef", "input": { "action": "invoke" } }),
            ),
            request("close", json!({ "type": "task.close" })),
        ],
    )?;
    assert!(output.status.success());
    assert_eq!(
        responses[1]["data"]["guarantees"]["minimizedOperation"],
        "unknown"
    );
    assert_eq!(responses[1]["data"]["decision"], "unavailable");
    assert_eq!(
        responses[2]["error"]["code"],
        "BACKGROUND_OPERATION_UNAVAILABLE"
    );
    assert_eq!(responses[3]["data"]["executions"], 0);
    Ok(())
}

#[test]
fn ordinary_frames_cannot_inject_authorization_or_foreground_consent() -> TestResult {
    let mut forged = request("forged", json!({ "type": "task.open" }));
    forged["confirmed"] = json!(true);
    forged["foregroundConsent"] = json!(true);
    let (output, responses) = run(&grant(), &[forged])?;
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["error"]["code"], "INVALID_ARGUMENT");
    Ok(())
}

#[test]
fn grant_contract_and_unknown_capabilities_fail_before_task_start() -> TestResult {
    let mut invalid = grant();
    invalid["permissions"][0]["capability"] = json!("not.registered@99");
    let (output, responses) = run(&invalid, &[])?;
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["error"]["code"], "INVALID_ARGUMENT");
    Ok(())
}

#[test]
fn idle_task_expires_without_a_new_request_or_graphical_session() -> TestResult {
    let mut value = grant();
    value["totalTimeoutMs"] = json!(50);
    let grant = GrantFile::new(&value)?;
    let mut child = command(&grant).spawn()?;
    let held_input = child.stdin.take().ok_or("stdin is missing")?;
    let output = child.wait_with_output()?;
    drop(held_input);
    assert_eq!(output.status.code(), Some(2));
    let response: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(response["error"]["code"], "TASK_EXPIRED");
    Ok(())
}

#[test]
fn malformed_startup_never_prints_uix_human_help_into_machine_output() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_ai-computer-toolkit"))
        .args(["serve", "--help"])
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    let response: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(response["error"]["code"], "INVALID_ARGUMENT");
    assert_eq!(response["contractVersion"], "act/control/v2");
    Ok(())
}

#[test]
fn legacy_single_result_cli_remains_compatible() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_ai-computer-toolkit"))
        .arg("version")
        .output()?;
    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(response["name"], "ai-computer-toolkit");
    assert_eq!(response["version"], env!("CARGO_PKG_VERSION"));
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn task_authority_and_request_examples_match_published_schemas() -> TestResult {
    let grant_schema: Value =
        serde_json::from_str(include_str!("../../contracts/v2/task-grant.schema.json"))?;
    let request_schema: Value = serde_json::from_str(include_str!(
        "../../contracts/v2/task-control-request.schema.json"
    ))?;
    jsonschema::draft202012::validate(&grant_schema, &grant())
        .map_err(|error| error.to_string())?;
    let mut frame = request("open", json!({ "type": "task.open" }));
    jsonschema::draft202012::validate(&request_schema, &frame)
        .map_err(|error| error.to_string())?;
    frame["authorizationSource"] = json!("user-task");
    assert!(jsonschema::draft202012::validate(&request_schema, &frame).is_err());
    Ok(())
}

#[test]
fn conflicting_cancel_identity_cannot_cancel_before_it_is_rejected() -> TestResult {
    let (output, responses) = run(
        &grant(),
        &[
            request("open", json!({ "type": "task.open" })),
            request("open", json!({ "type": "task.cancel" })),
            request("catalog", json!({ "type": "catalog" })),
            request("close", json!({ "type": "task.close" })),
        ],
    )?;
    assert!(output.status.success());
    assert_eq!(responses[1]["error"]["code"], "REQUEST_ID_CONFLICT");
    assert_eq!(responses[2]["ok"], true);
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn writable_or_symlinked_authority_file_is_rejected() -> TestResult {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let mut grant_file = GrantFile::new(&grant())?;
    fs::set_permissions(&grant_file.file, fs::Permissions::from_mode(0o666))?;
    let output = command(&grant_file).output()?;
    assert_eq!(output.status.code(), Some(2));
    let response: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(response["error"]["code"], "TASK_GRANT_INVALID");
    fs::set_permissions(&grant_file.file, fs::Permissions::from_mode(0o600))?;
    let link = grant_file.directory.join("grant-link.json");
    symlink(&grant_file.file, &link)?;
    grant_file.file = link;
    let output = command(&grant_file).output()?;
    assert_eq!(output.status.code(), Some(2));
    let response: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(response["error"]["code"], "TASK_GRANT_INVALID");
    Ok(())
}

#[test]
fn invalid_correlation_is_not_echoed_as_a_valid_receipt() -> TestResult {
    let (output, responses) = run(
        &grant(),
        &[
            request("not a valid id", json!({ "type": "task.open" })),
            request("open", json!({ "type": "task.open" })),
            request("close", json!({ "type": "task.close" })),
        ],
    )?;
    assert!(output.status.success());
    assert_eq!(responses[0]["error"]["code"], "INVALID_ARGUMENT");
    assert!(responses[0].get("requestId").is_none());
    assert_eq!(responses[1]["ok"], true);
    Ok(())
}

#[test]
fn task_cancel_stops_future_dispatch_without_claiming_rollback() -> TestResult {
    let (output, responses) = run(
        &grant(),
        &[
            request("cancel", json!({ "type": "task.cancel" })),
            request("open", json!({ "type": "task.open" })),
            request("close", json!({ "type": "task.close" })),
        ],
    )?;
    assert!(output.status.success());
    assert_eq!(responses[0]["data"]["state"], "cancelled");
    assert_eq!(responses[0]["data"]["cancellationRequested"], true);
    assert_eq!(responses[0]["data"]["executions"], 0);
    assert!(responses[0]["data"].get("rolledBack").is_none());
    assert_eq!(responses[1]["error"]["code"], "CANCELLED");
    Ok(())
}

#[test]
fn oversized_frame_is_rejected_without_an_unbounded_line_buffer() -> TestResult {
    let grant_file = GrantFile::new(&grant())?;
    let mut child = command(&grant_file).spawn()?;
    let mut input = child.stdin.take().ok_or("stdin is missing")?;
    if let Err(error) = input.write_all(&vec![b' '; 65537])
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(error.into());
    }
    drop(input);
    let output = child.wait_with_output()?;
    assert_eq!(output.status.code(), Some(2));
    let response: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(response["error"]["code"], "INVALID_ARGUMENT");
    Ok(())
}

#[test]
fn postconditions_cannot_smuggle_an_executable_evaluator() -> TestResult {
    let (output, responses) = run(
        &grant(),
        &[request(
            "script",
            json!({ "type": "execute", "capability": "ui.element.action@2", "targetId": "s2:w:0123456789abcdef", "input": { "action": "invoke" },
            "postconditions": [{ "operator": "script", "source": "forbidden" }] }),
        )],
    )?;
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(responses[0]["error"]["code"], "INVALID_ARGUMENT");
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn media_catalog_distinguishes_route_from_current_target_eligibility() -> TestResult {
    let (_, responses) = run(
        &grant(),
        &[
            request("open", json!({"type": "task.open"})),
            request("catalog", json!({"type": "catalog"})),
        ],
    )?;
    for id in [
        "media.session.discover@3",
        "media.playback.state.read@3",
        "media.playback.control@3",
    ] {
        let item = responses[1]["data"]["capabilities"]
            .as_array()
            .ok_or("catalog")?
            .iter()
            .find(|item| item["id"] == id)
            .ok_or("media capability")?;
        assert_eq!(item["guarantees"]["routeEligible"], true);
        assert_eq!(item["guarantees"]["eligible"], false);
        assert_eq!(item["guarantees"]["assessmentRequired"], true);
        assert_eq!(
            item["guarantees"]["minimizedOperation"],
            "window-independent"
        );
    }
    Ok(())
}

#[test]
fn each_discovery_scope_requires_startup_authorization() -> TestResult {
    let mut value = grant();
    value["allowDiscovery"] = json!(false);
    let (_, responses) = run(
        &value,
        &[
            request("open", json!({"type": "task.open"})),
            request("apps", json!({"type": "discover", "scope": "applications"})),
            request("media", json!({"type": "discover", "scope": "media"})),
        ],
    )?;
    assert_eq!(responses[1]["error"]["code"], "TASK_SCOPE_VIOLATION");
    assert_eq!(responses[2]["error"]["code"], "TASK_SCOPE_VIOLATION");
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn media_task_scope_and_input_rejection_precede_worker_dispatch() -> TestResult {
    let mut value = grant();
    value["permissions"] = json!([{"capability": "media.playback.control@3", "targetId": "s2:m:0123456789abcdef", "requiredInput": {"operation": "pause"}}]);
    let (_, responses) = run(
        &value,
        &[
            request("open", json!({"type": "task.open"})),
            request(
                "wrong-operation",
                json!({"type": "execute", "capability": "media.playback.control@3", "targetId": "s2:m:0123456789abcdef", "input": {"operation": "play"}}),
            ),
            request(
                "bad-input",
                json!({"type": "execute", "capability": "media.playback.control@3", "targetId": "s2:m:0123456789abcdef", "input": {"operation": "pause", "method": "Raise"}}),
            ),
        ],
    )?;
    assert_eq!(responses[1]["error"]["code"], "TASK_SCOPE_VIOLATION");
    assert_eq!(
        responses[2]["error"]["code"], "TASK_EXECUTION_FAILED",
        "{}",
        responses[2]
    );
    assert_eq!(
        responses[2]["error"]["details"]["outcome"],
        "not-dispatched"
    );
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn discovery_capability_cannot_be_granted_against_a_fabricated_exact_target() -> TestResult {
    let mut value = grant();
    value["permissions"] =
        json!([{"capability": "media.session.discover@3", "targetId": "s2:m:0123456789abcdef"}]);
    let (output, responses) = run(&value, &[request("open", json!({"type": "task.open"}))])?;
    assert!(!output.status.success());
    assert!(
        responses
            .iter()
            .any(|response| response["error"]["code"] == "TASK_GRANT_INVALID")
    );
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
fn discovery_request_schema_preserves_default_and_closes_scope() -> TestResult {
    let schema: Value = serde_json::from_str(include_str!(
        "../../contracts/v2/task-control-request.schema.json"
    ))?;
    let validator = jsonschema::draft202012::new(&schema)?;
    for operation in [
        json!({"type": "discover"}),
        json!({"type": "discover", "scope": "applications"}),
        json!({"type": "discover", "scope": "media"}),
    ] {
        assert!(validator.is_valid(&request("discovery", operation)));
    }
    for operation in [
        json!({"type": "discover", "scope": "raw"}),
        json!({"type": "discover", "scope": "media", "address": "unix:path=/tmp/other-bus"}),
    ] {
        assert!(!validator.is_valid(&request("discovery", operation)));
    }
    Ok(())
}
