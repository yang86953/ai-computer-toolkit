use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    sync::atomic::{AtomicU64, Ordering},
    thread,
};

use serde_json::{Value, json};

use super::*;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "act-uix-agent-test-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("fixture directory must be created");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .expect("fixture directory must be private");
        Self(path)
    }
}

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn authenticated_fixture_lists_and_snapshots_without_exposing_transport_identity() {
    let directory = FixtureDirectory::new();
    let process_id = std::process::id();
    let token = "1".repeat(64);
    let socket_path = directory
        .0
        .join(format!("uix-{process_id}-{}.sock", &token[..24]));
    let listener = UnixListener::bind(&socket_path).expect("fixture socket must bind");
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
        .expect("fixture socket must be private");
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("fixture must accept client");
        let mut reader = BufReader::new(stream);
        for expected in ["hello", "list_windows", "snapshot"] {
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .expect("fixture request must read");
            let request = serde_json::from_str::<Value>(&line).expect("request must be JSON");
            assert_eq!(request["type"], expected);
            let payload = match expected {
                "hello" => json!({
                    "process_id": process_id,
                    "capabilities": {
                        "request_types": ["hello", "list_windows", "snapshot"],
                        "window_state_fields": [
                            "logical_width", "logical_height", "maximized", "minimized", "fullscreen"
                        ]
                    }
                }),
                "list_windows" => json!({
                    "windows": [{
                        "window_id": 7, "generation": 3, "title": "UIX fixture",
                        "visible": true, "presentable": true,
                        "logical_width": 1000, "logical_height": 700,
                        "maximized": true, "minimized": false, "fullscreen": false,
                        "revision": 9,
                        "presented_revision": 9, "closed": false
                    }]
                }),
                _ => json!({
                    "snapshot": {
                        "window_id": 7, "generation": 3, "revision": 9,
                        "presented_revision": 9, "closed": false,
                        "nodes": [{
                            "node_id": "1:1", "automation_id": "root", "parent": null,
                            "frame": {"x": 0, "y": 0, "w": 10, "h": 10},
                            "visible_bounds": null, "focused": false, "role": "window",
                            "name": null,
                            "state": {"disabled": false}, "selection": null,
                            "actions": ["focus"]
                        }]
                    }
                }),
            };
            let mut reply = json!({
                "schema": PROTOCOL_SCHEMA,
                "request_id": request["request_id"],
                "ok": true,
                "type": expected,
            });
            reply
                .as_object_mut()
                .unwrap()
                .extend(payload.as_object().unwrap().clone());
            writeln!(reader.get_mut(), "{reply}").expect("fixture reply must write");
        }
    });
    let descriptor = EndpointDescriptor {
        schema: PROTOCOL_SCHEMA.to_owned(),
        process_id,
        endpoint: socket_path.to_string_lossy().into_owned(),
        token,
        state: "ready".to_owned(),
    };
    let mut client = AgentClient::connect(&descriptor).expect("fixture must authenticate");
    assert!(client.supports_window_state());
    let windows = client.list_windows().expect("fixture windows must parse");
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].logical_width, Some(1000));
    assert_eq!(windows[0].logical_height, Some(700));
    assert_eq!(windows[0].maximized, Some(true));
    assert_eq!(
        window_state_record(&windows[0], true),
        Ok(Some(WindowStateRecord {
            logical_width: 1000,
            logical_height: 700,
            maximized: true,
            minimized: false,
            fullscreen: false,
        }))
    );
    let snapshot = client.snapshot(7, 3).expect("fixture snapshot must parse");
    assert_eq!(snapshot.nodes.len(), 1);
    assert_eq!(snapshot.nodes[0].name, None);
    assert_eq!(snapshot.nodes[0].frame.x, 0.0);
    assert_eq!(snapshot.nodes[0].frame.y, 0.0);
    assert_eq!(snapshot.nodes[0].frame.w, 10.0);
    assert_eq!(snapshot.nodes[0].frame.h, 10.0);
    assert!(snapshot.nodes[0].visible_bounds.is_none());
    server.join().expect("fixture server must stop");
}

#[test]
fn logical_rectangles_reject_non_finite_negative_or_unbounded_dimensions() {
    for rect in [
        WireRect {
            x: f64::NAN,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        },
        WireRect {
            x: 0.0,
            y: 0.0,
            w: -1.0,
            h: 1.0,
        },
        WireRect {
            x: 0.0,
            y: 0.0,
            w: 1_000_000_001.0,
            h: 1.0,
        },
    ] {
        assert_eq!(rect_record(rect), Err(Failure::Protocol));
    }
}

#[test]
fn descriptor_symlink_is_rejected_without_following_it() {
    use std::os::unix::fs::symlink;

    let directory = FixtureDirectory::new();
    let outside = directory.0.with_extension("outside");
    fs::write(&outside, b"{}").expect("outside fixture must exist");
    let link = directory.0.join(format!("uix-{}.json", std::process::id()));
    symlink(&outside, &link).expect("fixture symlink must exist");
    assert!(read_descriptor(&directory.0, &link, geteuid().as_raw()).is_none());
    fs::remove_file(outside).expect("outside fixture must be removed");
}

#[test]
fn private_descriptor_discovery_returns_only_opaque_window_identity() {
    let directory = FixtureDirectory::new();
    let process_id = std::process::id();
    let token = "2".repeat(64);
    let socket_path = directory
        .0
        .join(format!("uix-{process_id}-{}.sock", &token[..24]));
    let listener = UnixListener::bind(&socket_path).expect("fixture socket must bind");
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
        .expect("fixture socket must be private");
    let descriptor_path = directory.0.join(format!("uix-{process_id}.json"));
    fs::write(
        &descriptor_path,
        serde_json::to_vec(&json!({
            "schema": PROTOCOL_SCHEMA,
            "process_id": process_id,
            "endpoint": socket_path,
            "token": token,
            "state": "ready"
        }))
        .unwrap(),
    )
    .expect("fixture descriptor must write");
    fs::set_permissions(&descriptor_path, fs::Permissions::from_mode(0o600))
        .expect("fixture descriptor must be private");
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("fixture must accept client");
        let mut reader = BufReader::new(stream);
        for expected in ["hello", "list_windows"] {
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .expect("fixture request must read");
            let request = serde_json::from_str::<Value>(&line).expect("request must be JSON");
            let payload = if expected == "hello" {
                json!({
                    "process_id": process_id,
                    "capabilities": { "request_types": ["hello", "list_windows", "snapshot"] }
                })
            } else {
                json!({
                    "windows": [{
                        "window_id": 91, "generation": 4, "title": "Discovered",
                        "visible": true, "presentable": true, "revision": 6,
                        "presented_revision": 6, "closed": false
                    }]
                })
            };
            let mut reply = json!({
                "schema": PROTOCOL_SCHEMA,
                "request_id": request["request_id"],
                "ok": true,
                "type": expected,
            });
            reply
                .as_object_mut()
                .unwrap()
                .extend(payload.as_object().unwrap().clone());
            writeln!(reader.get_mut(), "{reply}").expect("fixture reply must write");
        }
    });
    let inventory = discover_from(&directory.0, 8).expect("fixture discovery must succeed");
    assert!(inventory.complete);
    assert_eq!(inventory.total, Some(1));
    assert_eq!(inventory.windows.len(), 1);
    assert_eq!(inventory.windows[0].state, None);
    assert!(inventory.windows[0].session_id.starts_with("s2:w:"));
    assert!(
        !inventory.windows[0]
            .session_id
            .contains(&process_id.to_string())
    );
    assert!(!inventory.windows[0].session_id.contains(&"2".repeat(24)));
    server.join().expect("fixture server must stop");
}

#[test]
fn perform_keeps_application_confirmation_on_the_authenticated_connection() {
    let directory = FixtureDirectory::new();
    let process_id = std::process::id();
    let token = "3".repeat(64);
    let socket_path = directory
        .0
        .join(format!("uix-{process_id}-{}.sock", &token[..24]));
    let listener = UnixListener::bind(&socket_path).expect("fixture socket must bind");
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
        .expect("fixture socket must be private");
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("fixture must accept client");
        let mut reader = BufReader::new(stream);
        for expected in ["hello", "perform", "confirm"] {
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .expect("fixture request must read");
            let request = serde_json::from_str::<Value>(&line).expect("request must be JSON");
            assert_eq!(request["type"], expected);
            let reply = match expected {
                "hello" => json!({
                    "schema": PROTOCOL_SCHEMA,
                    "request_id": request["request_id"],
                    "ok": true,
                    "type": "hello",
                    "process_id": process_id,
                    "capabilities": {
                        "request_types": ["hello", "list_windows", "snapshot", "perform", "confirm", "wait"],
                        "semantic_actions": ["invoke"]
                    }
                }),
                "perform" => {
                    assert_eq!(request["target"]["node_id"], "7:2");
                    assert_eq!(request["expected_revision"], 11);
                    json!({
                        "schema": PROTOCOL_SCHEMA,
                        "request_id": request["request_id"],
                        "ok": false,
                        "error": {
                            "code": "requires_confirmation",
                            "message": "confirmation required",
                            "confirm_id": 99,
                            "target": "private",
                            "action": "invoke"
                        }
                    })
                }
                _ => {
                    assert_eq!(request["confirm_id"], 99);
                    json!({
                        "schema": PROTOCOL_SCHEMA,
                        "request_id": request["request_id"],
                        "ok": true,
                        "type": "confirm",
                        "window_id": 7,
                        "generation": 2,
                        "revision": 12,
                        "presented_revision": 12,
                        "settled": true
                    })
                }
            };
            writeln!(reader.get_mut(), "{reply}").expect("fixture reply must write");
        }
    });
    let descriptor = EndpointDescriptor {
        schema: PROTOCOL_SCHEMA.to_owned(),
        process_id,
        endpoint: socket_path.to_string_lossy().into_owned(),
        token,
        state: "ready".to_owned(),
    };
    let mut client =
        AgentClient::connect_until(&descriptor, Instant::now() + Duration::from_secs(2))
            .expect("fixture must authenticate");
    let outcome = client
        .perform(7, 2, 11, "7:2", &UixSemanticAction::Invoke {})
        .expect("confirmed action must finish");
    assert!(outcome.application_confirmation_performed);
    assert_eq!(outcome.revision, 12);
    server.join().expect("fixture server must stop");
}

#[test]
fn targetless_lifecycle_key_pointer_and_close_use_negotiated_catalogs() {
    let directory = FixtureDirectory::new();
    let process_id = std::process::id();
    let token = "5".repeat(64);
    let socket_path = directory
        .0
        .join(format!("uix-{process_id}-{}.sock", &token[..24]));
    let listener = UnixListener::bind(&socket_path).expect("fixture socket must bind");
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
        .expect("fixture socket must be private");
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("fixture must accept client");
        let mut reader = BufReader::new(stream);
        for index in 0..5 {
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .expect("fixture request must read");
            let request = serde_json::from_str::<Value>(&line).expect("request must be JSON");
            let expected = if index == 0 { "hello" } else { "perform" };
            assert_eq!(request["type"], expected);
            let reply = if expected == "hello" {
                json!({
                    "schema": PROTOCOL_SCHEMA,
                    "request_id": request["request_id"],
                    "ok": true,
                    "type": "hello",
                    "process_id": process_id,
                    "capabilities": {
                        "request_types": ["hello", "list_windows", "snapshot", "perform"],
                        "window_actions": ["maximize_window", "press_key", "click_at", "close_window"],
                        "key_names": ["page_down"],
                        "key_modifiers": ["ctrl", "shift"]
                    }
                })
            } else {
                assert_eq!(request["window_id"], 29);
                assert_eq!(request["generation"], 8);
                assert_eq!(request["expected_revision"], 12 + index);
                if index == 1 {
                    assert_eq!(request["action"]["kind"], "maximize_window");
                } else if index == 2 {
                    assert_eq!(request["action"]["kind"], "press_key");
                    assert_eq!(request["action"]["key"], "page_down");
                    assert_eq!(request["action"]["modifiers"], json!(["ctrl", "shift"]));
                } else if index == 3 {
                    assert_eq!(request["action"]["kind"], "click_at");
                    assert_eq!(request["action"]["x"], 20.5);
                    assert_eq!(request["action"]["y"], 30.25);
                } else {
                    assert_eq!(request["action"]["kind"], "close_window");
                }
                assert!(request.get("target").is_none());
                json!({
                    "schema": PROTOCOL_SCHEMA,
                    "request_id": request["request_id"],
                    "ok": true,
                    "type": "perform",
                    "window_id": 29,
                    "generation": 8,
                    "revision": 13 + index,
                    "presented_revision": 13 + index,
                    "settled": true
                })
            };
            writeln!(reader.get_mut(), "{reply}").expect("fixture reply must write");
        }
    });
    let descriptor = EndpointDescriptor {
        schema: PROTOCOL_SCHEMA.to_owned(),
        process_id,
        endpoint: socket_path.to_string_lossy().into_owned(),
        token,
        state: "ready".to_owned(),
    };
    let mut client =
        AgentClient::connect_until(&descriptor, Instant::now() + Duration::from_secs(2))
            .expect("fixture must authenticate");
    let outcome = client
        .perform_window(
            29,
            8,
            13,
            crate::components::uix_window_lifecycle_contract::UixWindowLifecycleAction::Maximize {},
        )
        .expect("window lifecycle must finish");
    assert_eq!(outcome.revision, 14);
    assert!(outcome.settled);
    let key = crate::components::uix_key_input_contract::UixKeyInput::parse(&json!({
        "key": "page-down",
        "modifiers": ["control", "shift"]
    }))
    .expect("key fixture must parse");
    let key_outcome = client
        .perform_key(29, 8, 14, &key)
        .expect("key input must finish");
    assert_eq!(key_outcome.revision, 15);
    let pointer = crate::components::uix_pointer_input_contract::UixPointerInput::parse(&json!({
        "action": "click",
        "coordinateSpace": "client-logical-px",
        "x": 20.5,
        "y": 30.25
    }))
    .expect("pointer fixture must parse");
    let pointer_outcome = client
        .perform_pointer(29, 8, 15, &pointer)
        .expect("pointer input must finish");
    assert_eq!(pointer_outcome.revision, 16);
    let close_outcome = client
        .perform_close(29, 8, 16)
        .expect("window close request must finish");
    assert_eq!(close_outcome.revision, 17);
    server.join().expect("fixture server must stop");
}

#[test]
fn negotiated_wait_supports_revision_and_exact_generation_close_loop() {
    let directory = FixtureDirectory::new();
    let process_id = std::process::id();
    let token = "4".repeat(64);
    let socket_path = directory
        .0
        .join(format!("uix-{process_id}-{}.sock", &token[..24]));
    let listener = UnixListener::bind(&socket_path).expect("fixture socket must bind");
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
        .expect("fixture socket must be private");
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("fixture must accept client");
        let mut reader = BufReader::new(stream);
        for (index, expected) in ["hello", "wait", "wait", "wait"].into_iter().enumerate() {
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .expect("fixture request must read");
            let request = serde_json::from_str::<Value>(&line).expect("request must be JSON");
            assert_eq!(request["type"], expected);
            let reply = if expected == "hello" {
                json!({
                    "schema": PROTOCOL_SCHEMA,
                    "request_id": request["request_id"],
                    "ok": true,
                    "type": "hello",
                    "process_id": process_id,
                    "capabilities": {
                        "request_types": ["hello", "list_windows", "snapshot", "wait"]
                    }
                })
            } else {
                assert_eq!(request["window_id"], 17);
                assert_eq!(request["generation"], 5);
                let expected_revision = match index {
                    1 => 9,
                    2 => 10,
                    3 => 11,
                    _ => unreachable!("hello is handled separately"),
                };
                assert_eq!(request["after_revision"], expected_revision);
                assert!(
                    request["timeout_ms"]
                        .as_u64()
                        .is_some_and(|value| value > 0)
                );
                let closed = index == 3;
                let revision = if index == 1 { 10 } else { 11 };
                json!({
                    "schema": PROTOCOL_SCHEMA,
                    "request_id": request["request_id"],
                    "ok": true,
                    "type": "wait",
                    "outcome": if closed { "closed" } else { "changed" },
                    "window": {
                        "window_id": 17,
                        "generation": 5,
                        "title": "fixture",
                        "visible": true,
                        "presentable": true,
                        "revision": revision,
                        "presented_revision": if index == 1 { 9 } else { 10 },
                        "closed": closed
                    }
                })
            };
            writeln!(reader.get_mut(), "{reply}").expect("fixture reply must write");
        }
    });
    let descriptor = EndpointDescriptor {
        schema: PROTOCOL_SCHEMA.to_owned(),
        process_id,
        endpoint: socket_path.to_string_lossy().into_owned(),
        token,
        state: "ready".to_owned(),
    };
    let mut client =
        AgentClient::connect_until(&descriptor, Instant::now() + Duration::from_secs(2))
            .expect("fixture must authenticate");
    let outcome = client
        .wait_revision(
            17,
            5,
            9,
            9,
            WindowRevisionWaitCondition::RevisionAfter { revision: 9 },
        )
        .expect("revision wait must finish");
    assert_eq!(outcome.kind, RevisionWaitOutcomeKind::Changed);
    assert_eq!(outcome.revision, 10);
    assert!(!outcome.closed);
    let closed = client
        .wait_closed(17, 5, outcome.revision, outcome.presented_revision)
        .expect("closed wait must cross ordinary revision changes");
    assert_eq!(closed.kind, RevisionWaitOutcomeKind::Closed);
    assert_eq!(closed.revision, 11);
    assert!(closed.closed);
    server.join().expect("fixture server must stop");
}

#[test]
fn wait_outcome_accepts_presented_and_same_generation_closed_terminals() {
    let reply = json!({
        "outcome": "presented",
        "window": {
            "window_id": 1, "generation": 2, "title": "", "visible": false,
            "presentable": false, "revision": 8, "presented_revision": 8,
            "closed": false
        }
    });
    let presented = wait_outcome(
        reply.clone(),
        1,
        2,
        7,
        7,
        WindowRevisionWaitCondition::PresentedAtLeast { revision: 8 },
    )
    .expect("presented terminal must validate");
    assert_eq!(presented.kind, RevisionWaitOutcomeKind::Presented);

    let mut closed_reply = reply;
    closed_reply["outcome"] = json!("closed");
    closed_reply["window"]["closed"] = json!(true);
    let closed = wait_outcome(
        closed_reply,
        1,
        2,
        7,
        7,
        WindowRevisionWaitCondition::RevisionAfter { revision: 8 },
    )
    .expect("same-generation close must validate");
    assert_eq!(closed.kind, RevisionWaitOutcomeKind::Closed);
    assert!(closed.closed);
}

#[test]
fn remote_outcome_unknown_remains_distinct_from_retryable_timeout() {
    let error = action_request_failure(RequestFailure::Remote {
        code: "outcome_unknown".to_owned(),
        confirm_id: None,
    });
    assert_eq!(error, ActionFailure::OutcomeUnknown);
    let app_closed = action_request_failure(RequestFailure::Remote {
        code: "app_closed".to_owned(),
        confirm_id: None,
    });
    assert_eq!(app_closed, ActionFailure::OutcomeUnknown);
    let timeout = action_request_failure(RequestFailure::Remote {
        code: "timeout".to_owned(),
        confirm_id: None,
    });
    assert_eq!(timeout, ActionFailure::Transport(Failure::Timeout));
}
