#![cfg(target_os = "linux")]

//! Linux Wayland-only 桌面就绪度纵切契约回归。

use ai_computer_toolkit::cli;

#[test]
fn linux_desktop_status_is_read_only_and_only_claims_ready_portal_capability()
-> Result<(), Box<dyn std::error::Error>> {
    let status = cli::run(vec!["status".to_owned(), "desktop".to_owned()])?.json;
    assert_eq!(status["ok"], true);
    assert_eq!(status["provider"], "wayland-portal-readiness-probe");
    assert_eq!(status["readOnly"], true);
    assert_eq!(status["wayland"]["connectionAttempted"], false);
    assert_eq!(status["portal"]["autoStartAllowed"], false);
    assert_eq!(status["portal"]["requestIssued"], false);
    assert_eq!(status["portal"]["permissionPrompted"], false);
    assert_eq!(status["policy"]["desktopProtocol"], "wayland-only");
    assert_eq!(status["policy"]["x11Supported"], false);
    assert_eq!(status["policy"]["xwaylandFallback"], false);
    assert_eq!(status["policy"]["fallback"], "none");
    let common_ready = status["wayland"]["runtimeSocketPresent"] == true
        && status["portal"]["userBusSocketPresent"] == true
        && status["portal"]["userBusReachable"] == true
        && status["portal"]["desktopServiceRegistered"] == true;
    let screenshot_ready = common_ready
        && status["portal"]["interfaces"]["screenshot"]["version"]
            .as_u64()
            .is_some_and(|version| version >= 2);
    let session_ready = common_ready
        && status["portal"]["interfaces"]["remoteDesktop"]["version"]
            .as_u64()
            .is_some_and(|version| version >= 2)
        && status["portal"]["interfaces"]["screenCast"]["version"]
            .as_u64()
            .is_some_and(|version| version >= 5);
    let identifiers = status["capabilities"]
        .as_array()
        .ok_or("desktop capabilities must be an array")?
        .iter()
        .filter_map(|entry| entry["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if screenshot_ready {
        assert!(identifiers.contains("desktop.screenshot.interactive@1"));
    }
    assert!(!identifiers.contains("desktop.session.open@1"));
    assert!(!identifiers.contains("desktop.session.close@1"));
    assert!(!identifiers.contains("screen.capture@1"));
    assert_eq!(
        status["portal"]["desktopSessionCandidate"]["prerequisitesReady"],
        session_ready
    );
    assert_eq!(
        status["portal"]["desktopSessionCandidate"]["liveAcceptancePassed"],
        false
    );
    assert_eq!(
        status["portal"]["desktopSessionCandidate"]["advertisedAsAvailable"],
        false
    );
    if screenshot_ready {
        assert_eq!(
            status["capabilityState"],
            "portal-interactive-screenshot-ready"
        );
    } else {
        assert_eq!(status["capabilityState"], "no-desktop-operation-provider");
        assert_eq!(status["capabilities"], serde_json::json!([]));
    }
    Ok(())
}

#[test]
fn one_shot_desktop_session_routes_point_to_the_persistent_owner() {
    for argv in [
        vec!["sessions".to_owned(), "desktop".to_owned()],
        vec!["inspect".to_owned(), "desktop".to_owned()],
    ] {
        let error = match cli::run(argv) {
            Ok(_) => panic!("desktop operation must fail closed"),
            Err(error) => error,
        };
        assert_eq!(error.code, "CAPABILITY_UNAVAILABLE");
        assert_eq!(error.details["executionRealm"], "none");
        assert_eq!(error.details["fallback"], "none");
        assert_eq!(error.details["inputAllowed"], false);
        assert_eq!(error.details["portalRequestIssued"], false);
        assert_eq!(
            error.details["persistentSessionLauncher"],
            "ai-computer-toolkit session-host desktop"
        );
    }
}

#[test]
fn linux_capability_surface_freezes_wayland_only_and_keeps_window_semantics_unavailable()
-> Result<(), Box<dyn std::error::Error>> {
    let surface = cli::run(vec!["capabilities".to_owned()])?.json;
    let policy = &surface["data"]["platformPolicy"];
    assert_eq!(policy["desktopProtocol"], "wayland-only");
    assert_eq!(policy["x11Supported"], false);
    assert_eq!(policy["xwaylandFallback"], false);
    assert_eq!(policy["fallback"], "none");

    let capabilities = surface["data"]["capabilities"]
        .as_array()
        .ok_or("capabilities must be an array")?;
    for capability in ["window.discover@1", "window.screenshot@1", "ui.input.key@1"] {
        let status = capabilities
            .iter()
            .find(|entry| entry["id"] == capability)
            .and_then(|entry| entry["status"].as_str());
        assert_eq!(status, Some("unavailable-no-linux-provider"));
    }
    Ok(())
}

#[test]
fn portal_screenshot_gates_precede_target_path_and_request()
-> Result<(), Box<dyn std::error::Error>> {
    let discovery = cli::run(vec!["discover".to_owned(), "app".to_owned()])?.json;
    let host = discovery["data"]["hostTargetId"]
        .as_str()
        .ok_or("discovery must return a host target")?;
    let base = vec![
        "run".to_owned(),
        "desktop".to_owned(),
        "screenshot-interactive".to_owned(),
        "--target".to_owned(),
        format!("sessionId={host}"),
        "--arg".to_owned(),
        "path=/tmp/ai-computer-toolkit-never-requested.png".to_owned(),
    ];
    let error = cli::run(base.clone())
        .err()
        .ok_or("missing confirmation must fail")?;
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");

    let mut confirmed = base;
    confirmed.push("--confirm".to_owned());
    let error = cli::run(confirmed)
        .err()
        .ok_or("missing foreground consent must fail")?;
    assert_eq!(error.code, "FOREGROUND_CONSENT_REQUIRED");
    assert_eq!(error.details["portalRequestIssued"], false);
    assert_eq!(error.details["fallback"], "none");
    Ok(())
}

#[test]
fn portal_screenshot_assessment_is_ready_or_structurally_unavailable()
-> Result<(), Box<dyn std::error::Error>> {
    let discovery = cli::run(vec!["discover".to_owned(), "app".to_owned()])?.json;
    let host = discovery["data"]["hostTargetId"]
        .as_str()
        .ok_or("discovery must return a host target")?;
    let assessment = cli::run(vec![
        "assess".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "desktop.screenshot.interactive@1".to_owned(),
        "--target".to_owned(),
        format!("sessionId={host}"),
    ])?
    .json;
    assert_eq!(assessment["constraints"]["noFallback"], true);
    match assessment["decision"].as_str() {
        Some("foreground-consent-required") => {
            assert_eq!(assessment["executionRealm"], "host-foreground");
            assert_eq!(assessment["requiresConfirmation"], true);
            assert_eq!(assessment["requiresForegroundConsent"], true);
        }
        Some("unavailable") => assert_eq!(assessment["executionRealm"], "none"),
        decision => return Err(format!("unexpected assessment decision: {decision:?}").into()),
    }
    Ok(())
}

#[test]
fn portal_desktop_session_open_assessment_remains_pending_until_live_acceptance()
-> Result<(), Box<dyn std::error::Error>> {
    let discovery = cli::run(vec!["discover".to_owned(), "app".to_owned()])?.json;
    let host = discovery["data"]["hostTargetId"]
        .as_str()
        .ok_or("discovery must return a host target")?;
    let assessment = cli::run(vec![
        "assess".to_owned(),
        "app".to_owned(),
        "--capability".to_owned(),
        "desktop.session.open@1".to_owned(),
        "--target".to_owned(),
        format!("sessionId={host}"),
    ])?
    .json;
    assert_eq!(assessment["constraints"]["noFallback"], true);
    match assessment["decision"].as_str() {
        Some("foreground-consent-required") => {
            return Err("desktop session must not be advertised before live acceptance".into());
        }
        Some("unavailable") if assessment["constraints"]["launcher"].is_string() => {
            assert_eq!(assessment["requiresConfirmation"], true);
            assert_eq!(assessment["requiresForegroundConsent"], true);
            assert_eq!(
                assessment["constraints"]["launcher"],
                "ai-computer-toolkit session-host desktop"
            );
            assert_eq!(assessment["evidence"]["inputAllowed"], false);
            assert_eq!(
                assessment["evidence"]["implementationState"],
                "production-route-live-acceptance-pending-not-advertised"
            );
        }
        Some("unavailable") => assert_eq!(assessment["executionRealm"], "none"),
        decision => return Err(format!("unexpected assessment decision: {decision:?}").into()),
    }
    Ok(())
}
