#![cfg(target_os = "linux")]
use ai_computer_toolkit::cli;

#[test]
fn default_production_publishes_v2_candidate_without_execution_realm()
-> Result<(), Box<dyn std::error::Error>> {
    let value = cli::run(vec!["capabilities".into()])?.json;
    for id in [
        "media.session.discover@2",
        "media.playback.state.read@2",
        "media.playback.control@2",
    ] {
        let item = value["data"]["capabilities"]
            .as_array()
            .ok_or("capabilities")?
            .iter()
            .find(|v| v["id"] == id)
            .ok_or("missing")?;
        assert_eq!(item["linuxClassification"], "candidate");
        assert!(item.get("executionDomain").is_none());
    }
    Ok(())
}

#[test]
fn production_assessment_is_closed_without_any_bus_route() -> Result<(), Box<dyn std::error::Error>>
{
    let host = cli::run(vec![
        "discover".into(),
        "app".into(),
        "--max-applications".into(),
        "1".into(),
        "--max-processes".into(),
        "1".into(),
    ])?
    .json["data"]["hostTargetId"]
        .as_str()
        .ok_or("host")?
        .to_owned();
    let discover = cli::run(vec![
        "assess".into(),
        "app".into(),
        "--capability".into(),
        "media.session.discover@2".into(),
        "--target".into(),
        format!("sessionId={host}"),
    ])?
    .json;
    assert_eq!(discover["decision"], "unavailable");
    assert_eq!(discover["executionRealm"], "none");
    let control = cli::run(vec![
        "assess".into(),
        "app".into(),
        "--capability".into(),
        "media.playback.control@2".into(),
        "--target".into(),
        "sessionId=s2:m:0000000000000000".into(),
    ])?
    .json;
    assert_eq!(control["decision"], "unavailable");
    assert_eq!(control["requiresConfirmation"], true);
    assert_eq!(
        control["evidence"]["implementationState"],
        "candidate-no-production-dispatch"
    );
    Ok(())
}

#[test]
fn candidate_adapter_has_no_forbidden_calls_or_environment_lookup()
-> Result<(), Box<dyn std::error::Error>> {
    let source = include_str!("../src/adapters/linux/mpris.rs");
    let runtime = include_str!("../src/adapters/linux/mpris_runtime.rs");
    let endpoint = include_str!("../src/components/linux_user_bus_endpoint.rs");
    let candidate = include_str!("../src/mpris_candidate.rs");
    for forbidden in [
        "Connection::session",
        "DBUS_SESSION_BUS_ADDRESS",
        "ListActivatableNames",
        "StartServiceByName",
        "GetAll",
        "Metadata",
        "Identity",
        "DesktopEntry",
        "PropertiesChanged",
    ] {
        assert!(!source.contains(forbidden), "forbidden call: {forbidden}");
    }
    assert!(source.contains("cache_properties(CacheProperties::No)"));
    let subscription = source
        .find("owner_change_stream(conn, deadline).await?")
        .ok_or_else(|| std::io::Error::other("MPRIS owner subscription is missing"))?;
    let list_names = source
        .find("call(\"ListNames\"")
        .ok_or_else(|| std::io::Error::other("MPRIS ListNames call is missing"))?;
    assert!(subscription < list_names);
    assert!(source.contains("for (expected_owner, names) in &by_owner"));
    assert!(source.contains("MessageStream::for_match_rule"));
    assert!(source.contains("owner_changed_during_enumeration(&mut owner_changes).await?"));
    assert!(runtime.contains("MethodFlags::NoAutoStart"));
    assert!(runtime.contains("resolve_current_endpoint()"));
    assert!(endpoint.contains("/run/user/{effective_uid}"));
    assert!(candidate.contains("resolved_deadline(timeout_ms)"));
    assert!(candidate.contains("remaining_ms(&deadline)"));
    let resolved_control = candidate
        .split("pub fn control_private_with_resolved_epoch(")
        .nth(1)
        .ok_or_else(|| std::io::Error::other("resolved control candidate is missing"))?;
    let confirmation = resolved_control
        .find("if !confirmed")
        .ok_or_else(|| std::io::Error::other("resolved control confirmation gate is missing"))?;
    let runtime_resolution = resolved_control
        .find("mpris_runtime::resolve_private")
        .ok_or_else(|| std::io::Error::other("resolved control runtime lookup is missing"))?;
    assert!(confirmation < runtime_resolution);
    for forbidden in [
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
        "std::env::var",
        "std::env::var_os",
    ] {
        assert!(!runtime.contains(forbidden));
        assert!(!endpoint.contains(forbidden));
    }
    Ok(())
}

#[test]
fn windows_v1_media_contracts_remain_byte_frozen() {
    use sha2::{Digest, Sha256};
    let files = [
        include_str!("../contracts/v1/media-session-observation.schema.json"),
        include_str!("../contracts/v1/media-playback-state.schema.json"),
    ];
    let hashes = files.map(|v| format!("{:x}", Sha256::digest(v.as_bytes())));
    assert_eq!(
        hashes,
        [
            "616bbb6b6d1896251bc1c332ef48314a1d9948455e2e86dd20541869801faa9f",
            "de1501fab4da721b7c0ccc3c485c9588ec6b339bb60f0a36214f981422d2a6d0"
        ]
    );
}
