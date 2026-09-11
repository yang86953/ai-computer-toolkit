#![cfg(target_os = "linux")]

//! Linux Desktop Session Broker 的公开 schema 与真实 launcher 回归。

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use serde_json::{Value, json};

const CONTRACT: &str = "act/linux-desktop-session-broker/v1";
const EPOCH_PATTERN_NONCE: &str = "22222222222222222222222222222222";

fn schema(source: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(source)?)
}

fn assert_valid(schema: &Value, instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    jsonschema::draft202012::validate(schema, instance)
        .map_err(|error| format!("JSON Schema validation failed: {error}").into())
}

#[test]
fn lifecycle_input_schemas_freeze_timeout_and_empty_close_payload()
-> Result<(), Box<dyn std::error::Error>> {
    let open = schema(include_str!(
        "../../contracts/v1/desktop-session-open.schema.json"
    ))?;
    let close = schema(include_str!(
        "../../contracts/v1/desktop-session-close.schema.json"
    ))?;
    let capture = schema(include_str!("../../contracts/v1/screen-capture.schema.json"))?;
    assert_valid(&open, &json!({"timeoutMs": 120000}))?;
    assert!(assert_valid(&open, &json!({"timeoutMs": 9999})).is_err());
    assert!(
        assert_valid(
            &open,
            &json!({"timeoutMs": 120000, "nativePath": "private"})
        )
        .is_err()
    );
    assert_valid(&close, &json!({}))?;
    assert!(assert_valid(&close, &json!({"force": true})).is_err());
    assert_valid(
        &capture,
        &json!({"path": "/tmp/frame.png", "timeoutMs": 5000}),
    )?;
    assert!(assert_valid(&capture, &json!({"path": "/tmp/frame.jpg"})).is_err());
    assert_valid(
        &capture,
        &json!({"path": "/tmp/preview.png", "maxDimension": 1280}),
    )?;
    for invalid in [
        json!(null),
        json!(255),
        json!(16385),
        json!(1280.5),
        json!("1280"),
    ] {
        assert!(
            assert_valid(
                &capture,
                &json!({"path": "/tmp/preview.png", "maxDimension": invalid})
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn broker_schema_accepts_strict_public_frames_and_rejects_native_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let broker = schema(include_str!(
        "../../contracts/v1/linux-desktop-session-broker-v1.schema.json"
    ))?;
    let open = json!({
        "contractVersion": CONTRACT,
        "brokerEpoch": EPOCH_PATTERN_NONCE,
        "requestNonce": "33333333333333333333333333333333",
        "operation": "open",
        "confirmed": true,
        "foregroundConsent": true,
        "strictIsolation": false,
        "timeoutMs": 120000
    });
    assert_valid(&broker, &open)?;
    // 一次授权会话模式与记住授权意图是合法请求形态。
    let mut scoped = open.clone();
    scoped["authorizationScope"] = json!("session");
    scoped["rememberAuthorization"] = json!(true);
    assert_valid(&broker, &scoped)?;
    let mut bad_scope = scoped.clone();
    bad_scope["authorizationScope"] = json!("forever");
    assert!(assert_valid(&broker, &bad_scope).is_err());
    let mut leaked = open;
    leaked["portalSessionPath"] = json!("/org/freedesktop/private");
    assert!(assert_valid(&broker, &leaked).is_err());
    let mut host_identity = leaked;
    host_identity
        .as_object_mut()
        .expect("object")
        .remove("portalSessionPath");
    host_identity["hostSessionPath"] = json!("/org/freedesktop/login1/session/_private");
    host_identity["hostProcessId"] = json!(42);
    assert!(assert_valid(&broker, &host_identity).is_err());
    let key = json!({
        "contractVersion": CONTRACT,
        "brokerEpoch": EPOCH_PATTERN_NONCE,
        "requestNonce": "77777777777777777777777777777777",
        "operation": "input-key",
        "sessionId": "s2:i:0123456789abcdef",
        "confirmed": true,
        "foregroundConsent": true,
        "strictIsolation": false,
        "input": {
            "steps": [
                {"type": "key", "key": "left-control", "phase": "down"},
                {"type": "key", "key": "a"},
                {"type": "key", "key": "left-control", "phase": "up"}
            ]
        }
    });
    assert_valid(&broker, &key)?;
    // session 授权会话可以省略确认字段。
    let mut omitted = key.clone();
    omitted.as_object_mut().expect("object").remove("confirmed");
    omitted
        .as_object_mut()
        .expect("object")
        .remove("foregroundConsent");
    omitted
        .as_object_mut()
        .expect("object")
        .remove("strictIsolation");
    assert_valid(&broker, &omitted)?;
    let mut native = key.clone();
    native["input"]["steps"][1]["linuxKeyCode"] = json!(30);
    assert!(assert_valid(&broker, &native).is_err());
    let mut text = key;
    text["input"]["steps"] = json!([{"type": "text", "text": "blocked"}]);
    assert!(assert_valid(&broker, &text).is_err());
    let pointer = json!({
        "contractVersion": CONTRACT,
        "brokerEpoch": EPOCH_PATTERN_NONCE,
        "requestNonce": "88888888888888888888888888888888",
        "operation": "input-pointer",
        "sessionId": "s2:i:0123456789abcdef",
        "confirmed": true,
        "foregroundConsent": true,
        "strictIsolation": false,
        "input": {
            "coordinateSpace": "relative-logical-px",
            "steps": [
                {"type": "move", "delta": {"x": 24, "y": -12}},
                {"type": "click", "button": "left", "count": 2},
                {"type": "scroll", "axis": "vertical", "ticks": -3},
                {"type": "drag", "button": "middle", "delta": {"x": 30, "y": 15}}
            ]
        }
    });
    assert_valid(&broker, &pointer)?;
    let mut native_pointer = pointer;
    native_pointer["input"]["steps"][1]["linuxButtonCode"] = json!(272);
    assert!(assert_valid(&broker, &native_pointer).is_err());
    let cancel = json!({
        "contractVersion": CONTRACT,
        "brokerEpoch": EPOCH_PATTERN_NONCE,
        "requestNonce": "99999999999999999999999999999999",
        "operation": "input-cancel",
        "targetRequestNonce": "77777777777777777777777777777777"
    });
    assert_valid(&broker, &cancel)?;
    let mut native_cancel = cancel;
    native_cancel["sessionId"] = json!("s2:i:0123456789abcdef");
    assert!(assert_valid(&broker, &native_cancel).is_err());
    let capture = json!({
        "contractVersion": CONTRACT,
        "brokerEpoch": EPOCH_PATTERN_NONCE,
        "requestNonce": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "operation": "capture-frame",
        "sessionId": "s2:i:0123456789abcdef",
        "confirmed": true,
        "strictIsolation": false,
        "input": {"path": "/tmp/frame.png", "timeoutMs": 5000, "overwrite": false}
    });
    assert_valid(&broker, &capture)?;
    let mut preview = capture.clone();
    preview["input"]["maxDimension"] = json!(1280);
    assert_valid(&broker, &preview)?;
    preview["input"]["maxDimension"] = json!(255);
    assert!(assert_valid(&broker, &preview).is_err());
    let mut native_capture = capture;
    native_capture["pipeWireNodeId"] = json!(42);
    assert!(assert_valid(&broker, &native_capture).is_err());
    // 授权状态与撤销入口也是契约内请求；不得携带多余字段。
    for operation in ["authorization-status", "forget-authorization"] {
        let status = json!({
            "contractVersion": CONTRACT,
            "brokerEpoch": EPOCH_PATTERN_NONCE,
            "requestNonce": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "operation": operation,
        });
        assert_valid(&broker, &status)?;
        let mut native_status = status;
        native_status["restoreToken"] = json!("secret-material");
        assert!(assert_valid(&broker, &native_status).is_err());
    }
    // session 投影必须携带脱敏授权事实；token 值不属于公开形状。
    let response_with_session = |data: Value| {
        json!({
            "contractVersion": CONTRACT,
            "messageType": "response",
            "brokerEpoch": EPOCH_PATTERN_NONCE,
            "requestNonce": "51515151515151515151515151515151",
            "operation": "inspect",
            "transportAccepted": true,
            "businessAccepted": true,
            "outcome": "completed",
            "completed": true,
            "retrySafe": true,
            "automaticRetryProhibited": false,
            "acceptedMayHaveOccurred": true,
            "data": data,
            "error": null,
        })
    };
    let session_with_persistence = json!({
        "sessionId": "s2:i:0123456789abcdef",
        "targetKind": "desktop-session",
        "live": true,
        "authorizedDeviceClasses": ["keyboard", "pointer"],
        "streamMetadata": {"streamCount": 1, "mappingIdCount": 1},
        "providerVersions": {"remoteDesktop": 2, "screenCast": 5},
        "eisHandshakeComplete": true,
        "pipeWireRemoteObtained": true,
        "authorization": {
            "mode": "session",
            "persistence": {
                "requested": true,
                "restoredFromSaved": true,
                "restoreTokenRetained": true,
            },
        },
        "restoreTokenRetained": true,
        "inputEventsSent": 0,
        "framesCaptured": 0,
        "pixelsConsumed": 0
    });
    assert_valid(
        &broker,
        &response_with_session(session_with_persistence.clone()),
    )?;
    let mut leaking = session_with_persistence;
    leaking["authorization"]["persistence"]["restoreToken"] = json!("secret");
    assert!(assert_valid(&broker, &response_with_session(leaking)).is_err());
    Ok(())
}

#[test]
fn portal_key_v3_schema_accepts_named_keys_and_rejects_text()
-> Result<(), Box<dyn std::error::Error>> {
    let key = schema(include_str!("../../contracts/v3/key-input.schema.json"))?;
    assert_valid(
        &key,
        &json!({
            "timeoutMs": 5000,
            "steps": [
                {"type": "key", "key": "f24"},
                {"type": "chord", "keys": ["left-alt", "f4"]}
            ]
        }),
    )?;
    assert!(assert_valid(&key, &json!({"steps": [{"type": "text", "text": "x"}]})).is_err());
    assert!(
        assert_valid(
            &key,
            &json!({"steps": [{"type": "key", "key": "a", "phase": "down", "holdMs": 1}]})
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn portal_pointer_v3_schema_accepts_relative_actions_and_rejects_absolute_coordinates()
-> Result<(), Box<dyn std::error::Error>> {
    let pointer = schema(include_str!("../../contracts/v3/pointer-input.schema.json"))?;
    assert_valid(
        &pointer,
        &json!({
            "coordinateSpace": "relative-logical-px",
            "timeoutMs": 5000,
            "steps": [
                {"type": "move", "delta": {"x": 9, "y": -4}},
                {"type": "button", "button": "right", "phase": "down"},
                {"type": "move", "delta": {"x": 2, "y": 1}},
                {"type": "button", "button": "right", "phase": "up"}
            ]
        }),
    )?;
    assert!(
        assert_valid(
            &pointer,
            &json!({
                "coordinateSpace": "screen-physical-px",
                "steps": [{"type": "move", "point": {"x": 10, "y": 20}}]
            })
        )
        .is_err()
    );
    assert!(
        assert_valid(
            &pointer,
            &json!({
                "coordinateSpace": "relative-logical-px",
                "steps": [{"type": "move", "delta": {"x": 0, "y": 0}}]
            })
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn l2_liveness_is_standard_fail_closed_and_keeps_native_identity_private() {
    let host = include_str!("../../src/adapters/linux/desktop_session_host_activity_logind.rs");
    let portal = include_str!("../../src/adapters/linux/desktop_session_portal.rs");
    let eis = include_str!("../../src/adapters/linux/desktop_input_eis.rs");
    for marker in [
        "\"GetSession\"",
        "\"auto\"",
        "Active",
        "LockedHint",
        "CanLock",
        "NameOwnerChanged",
        "SessionRemoved",
        "PropertiesChanged",
        "MethodFlags::NoAutoStart",
    ] {
        assert!(
            host.contains(marker),
            "missing host liveness marker: {marker}"
        );
    }
    for marker in [
        "PORTAL_SESSION_CLOSED",
        "PORTAL_OWNER_DISCONNECTED",
        "PORTAL_SESSION_STATE_UNAVAILABLE",
        "closed_confirmed",
    ] {
        assert!(
            portal.contains(marker),
            "missing Portal liveness marker: {marker}"
        );
    }
    assert!(eis.contains("InputExecutionGuard"));
    assert!(eis.contains("Duration::from_millis(10)"));
    // 长期 Markdown 已归智协工作区，不再是编译输入；本测试核对当前实现与 JSON 契约。
    for forbidden in ["XTest", "xdotool", "XWayland", "org.kde.KWin"] {
        assert!(!host.contains(forbidden));
        assert!(!portal.contains(forbidden));
        assert!(!eis.contains(forbidden));
    }
}

#[test]
fn real_launcher_stays_live_and_gates_open_before_portal_access()
-> Result<(), Box<dyn std::error::Error>> {
    let broker_schema = schema(include_str!(
        "../../contracts/v1/linux-desktop-session-broker-v1.schema.json"
    ))?;
    // 隔离状态目录：授权查询不得读写测试机用户的真实保存凭据。
    let state_root = std::env::temp_dir().join(format!(
        "act-launcher-state-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&state_root)?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_ai-computer-toolkit"))
        .args(["session-host", "desktop"])
        .env("XDG_STATE_HOME", &state_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("broker stdout must be piped")?;
    let mut output = BufReader::new(stdout);
    let mut input = child.stdin.take().ok_or("broker stdin must be piped")?;

    let ready = read_frame(&mut output)?;
    assert_valid(&broker_schema, &ready)?;
    assert_eq!(ready["messageType"], "broker-ready");
    assert_eq!(ready["persistentAuthorizationSupported"], true);
    assert_eq!(ready["authorizationModes"], json!(["operation", "session"]));
    let epoch = ready["brokerEpoch"]
        .as_str()
        .ok_or("ready frame must include brokerEpoch")?;

    write_frame(
        &mut input,
        &json!({
            "contractVersion": CONTRACT,
            "brokerEpoch": epoch,
            "requestNonce": "44444444444444444444444444444444",
            "operation": "open",
            "confirmed": false,
            "foregroundConsent": true,
            "strictIsolation": false,
            "timeoutMs": 120000
        }),
    )?;
    let gated = read_frame(&mut output)?;
    assert_valid(&broker_schema, &gated)?;
    assert_eq!(gated["error"]["code"], "CONFIRMATION_REQUIRED");
    assert_eq!(gated["error"]["details"]["portalRequestIssued"], false);

    // session 模式不是隐式授权：确认不足同样在 Portal 之前闭合。
    write_frame(
        &mut input,
        &json!({
            "contractVersion": CONTRACT,
            "brokerEpoch": epoch,
            "requestNonce": "45454545454545454545454545454545",
            "operation": "open",
            "confirmed": false,
            "foregroundConsent": true,
            "strictIsolation": false,
            "timeoutMs": 120000,
            "authorizationScope": "session"
        }),
    )?;
    let scoped_gated = read_frame(&mut output)?;
    assert_valid(&broker_schema, &scoped_gated)?;
    assert_eq!(scoped_gated["error"]["code"], "CONFIRMATION_REQUIRED");
    assert_eq!(
        scoped_gated["error"]["details"]["portalRequestIssued"],
        false
    );

    write_frame(
        &mut input,
        &json!({
            "contractVersion": CONTRACT,
            "brokerEpoch": epoch,
            "requestNonce": "77777777777777777777777777777777",
            "operation": "input-key",
            "sessionId": "s2:i:0123456789abcdef",
            "confirmed": false,
            "foregroundConsent": true,
            "strictIsolation": false,
            "input": {"steps": [{"type": "key", "key": "enter"}]}
        }),
    )?;
    let input_gated = read_frame(&mut output)?;
    assert_valid(&broker_schema, &input_gated)?;
    assert_eq!(input_gated["error"]["code"], "CONFIRMATION_REQUIRED");
    assert_eq!(input_gated["error"]["details"]["inputAttempted"], false);

    // 无 live 会话时省略确认字段：无法继承，报告 STALE_SESSION 而非放行。
    write_frame(
        &mut input,
        &json!({
            "contractVersion": CONTRACT,
            "brokerEpoch": epoch,
            "requestNonce": "78787878787878787878787878787878",
            "operation": "input-key",
            "sessionId": "s2:i:0123456789abcdef",
            "input": {"steps": [{"type": "key", "key": "enter"}]}
        }),
    )?;
    let omitted_gated = read_frame(&mut output)?;
    assert_valid(&broker_schema, &omitted_gated)?;
    assert_eq!(omitted_gated["error"]["code"], "STALE_SESSION");
    assert_eq!(omitted_gated["error"]["details"]["targetInvalidated"], true);

    write_frame(
        &mut input,
        &json!({
            "contractVersion": CONTRACT,
            "brokerEpoch": epoch,
            "requestNonce": "88888888888888888888888888888888",
            "operation": "input-pointer",
            "sessionId": "s2:i:0123456789abcdef",
            "confirmed": false,
            "foregroundConsent": true,
            "strictIsolation": false,
            "input": {
                "coordinateSpace": "relative-logical-px",
                "steps": [{"type": "move", "delta": {"x": 1, "y": 1}}]
            }
        }),
    )?;
    let pointer_gated = read_frame(&mut output)?;
    assert_valid(&broker_schema, &pointer_gated)?;
    assert_eq!(pointer_gated["error"]["code"], "CONFIRMATION_REQUIRED");
    assert_eq!(pointer_gated["error"]["details"]["inputAttempted"], false);

    write_frame(
        &mut input,
        &json!({
            "contractVersion": CONTRACT,
            "brokerEpoch": epoch,
            "requestNonce": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "operation": "capture-frame",
            "sessionId": "s2:i:0123456789abcdef",
            "confirmed": false,
            "strictIsolation": false,
            "input": {"path": "/tmp/act-gated-frame.png"}
        }),
    )?;
    let capture_gated = read_frame(&mut output)?;
    assert_valid(&broker_schema, &capture_gated)?;
    assert_eq!(capture_gated["error"]["code"], "CONFIRMATION_REQUIRED");
    assert_eq!(capture_gated["error"]["details"]["pixelsConsumed"], false);
    assert_eq!(capture_gated["error"]["details"]["outputTouched"], false);

    // 授权状态查询：脱敏事实，隔离目录下没有已保存授权。
    write_frame(
        &mut input,
        &json!({
            "contractVersion": CONTRACT,
            "brokerEpoch": epoch,
            "requestNonce": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "operation": "authorization-status"
        }),
    )?;
    let status = read_frame(&mut output)?;
    assert_valid(&broker_schema, &status)?;
    assert_eq!(status["data"]["persistentAuthorizationSupported"], true);
    assert_eq!(status["data"]["savedAuthorizationState"], "absent");
    // 契约已拒绝任何携带凭据值的字段；这里再核对状态事实只有脱敏枚举与后端名。
    assert_eq!(
        status["data"]["savedAuthorizationBackend"],
        "xdg-portal-restore-token"
    );

    // 撤销入口：无凭据可清时同样成功收尾，并如实声明不撤销系统 Portal 记录。
    write_frame(
        &mut input,
        &json!({
            "contractVersion": CONTRACT,
            "brokerEpoch": epoch,
            "requestNonce": "cccccccccccccccccccccccccccccccc",
            "operation": "forget-authorization"
        }),
    )?;
    let forgotten = read_frame(&mut output)?;
    assert_valid(&broker_schema, &forgotten)?;
    assert_eq!(forgotten["data"]["savedAuthorizationCleared"], true);
    assert_eq!(forgotten["data"]["hadSavedAuthorization"], false);
    assert_eq!(forgotten["data"]["liveSessionsClosed"], 0);
    assert_eq!(forgotten["data"]["revokesSystemPortalRecords"], false);

    write_frame(
        &mut input,
        &json!({
            "contractVersion": CONTRACT,
            "brokerEpoch": epoch,
            "requestNonce": "99999999999999999999999999999999",
            "operation": "input-cancel",
            "targetRequestNonce": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }),
    )?;
    let cancel = read_frame(&mut output)?;
    assert_valid(&broker_schema, &cancel)?;
    assert_eq!(cancel["data"]["status"], "unknown-request");
    assert_eq!(cancel["retrySafe"], true);

    write_frame(
        &mut input,
        &json!({
            "contractVersion": CONTRACT,
            "brokerEpoch": epoch,
            "requestNonce": "55555555555555555555555555555555",
            "operation": "sessions"
        }),
    )?;
    let sessions = read_frame(&mut output)?;
    assert_valid(&broker_schema, &sessions)?;
    assert_eq!(sessions["data"]["sessions"], json!([]));

    write_frame(
        &mut input,
        &json!({
            "contractVersion": CONTRACT,
            "brokerEpoch": epoch,
            "requestNonce": "66666666666666666666666666666666",
            "operation": "shutdown"
        }),
    )?;
    let shutdown = read_frame(&mut output)?;
    assert_valid(&broker_schema, &shutdown)?;
    assert_eq!(shutdown["data"]["shutdownAccepted"], true);
    drop(input);
    assert!(child.wait()?.success());
    let _ = std::fs::remove_dir_all(&state_root);
    Ok(())
}

#[test]
fn real_launcher_can_finish_after_saturated_readonly_ledger()
-> Result<(), Box<dyn std::error::Error>> {
    struct OwnedBroker(std::process::Child);
    impl Drop for OwnedBroker {
        fn drop(&mut self) {
            if !matches!(self.0.try_wait(), Ok(Some(_))) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
    let mut broker = OwnedBroker(
        Command::new(env!("CARGO_BIN_EXE_ai-computer-toolkit"))
            .args(["session-host", "desktop"])
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env_remove("WAYLAND_DISPLAY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let mut input = broker.0.stdin.take().ok_or("missing input")?;
    let mut output = BufReader::new(broker.0.stdout.take().ok_or("missing output")?);
    let ready = read_frame(&mut output)?;
    let epoch = ready["brokerEpoch"].as_str().ok_or("missing epoch")?;
    for nonce in 0..1033 {
        write_frame(
            &mut input,
            &json!({"contractVersion":CONTRACT,
            "brokerEpoch":epoch,"requestNonce":format!("{nonce:032x}"),"operation":"sessions"}),
        )?;
        let response = read_frame(&mut output)?;
        assert_eq!(response["requestNonce"], format!("{nonce:032x}"));
        if nonce < 1032 {
            assert_eq!(response["completed"], true);
            assert_eq!(response["data"]["sessions"], json!([]));
        } else {
            assert_eq!(response["error"]["code"], "BROKER_REQUEST_LEDGER_FULL");
            assert_eq!(response["acceptedMayHaveOccurred"], false);
        }
    }
    write_frame(
        &mut input,
        &json!({"contractVersion":CONTRACT,
        "brokerEpoch":epoch,"requestNonce":format!("{:032x}",1033),"operation":"shutdown"}),
    )?;
    let shutdown = read_frame(&mut output)?;
    assert_eq!(shutdown["completed"], true);
    assert_eq!(shutdown["data"]["liveSessionsBeforeShutdown"], 0);
    drop(input);
    assert!(broker.0.wait()?.success());
    Ok(())
}

fn write_frame(writer: &mut impl Write, frame: &Value) -> Result<(), Box<dyn std::error::Error>> {
    serde_json::to_writer(&mut *writer, frame)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Value, Box<dyn std::error::Error>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err("broker closed before returning a frame".into());
    }
    Ok(serde_json::from_str(line.trim_end())?)
}
