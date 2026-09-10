#![cfg(all(target_os = "linux", feature = "linux-mpris-candidate"))]
#[allow(dead_code)]
#[path = "../support/mpris_private_bus.rs"]
mod mpris_private_bus;
use ai_computer_toolkit::mpris_candidate;
use jsonschema::Draft;
use mpris_private_bus::Fixture;
use serde_json::Value;
use std::sync::atomic::Ordering;

fn validate(schema: &str, value: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let schema: Value = serde_json::from_str(schema)?;
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .build(&schema)?
        .validate(value)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn private_bus_discover_state_and_control_preserve_privacy()
-> Result<(), Box<dyn std::error::Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let discovery = mpris_candidate::discover_private(&fixture.bus.address, "epoch-a", 3000)
            .map_err(|e| e.to_owned())?;
        validate(
            include_str!("../../contracts/v2/media-session-observation.schema.json"),
            &discovery,
        )?;
        assert_eq!(
            fixture.metrics.properties.load(Ordering::Relaxed),
            0,
            "discover must not read Player properties"
        );
        let id = discovery["data"]["sessions"][0]["sessionId"]
            .as_str()
            .ok_or("id")?;
        let state = mpris_candidate::state_private(&fixture.bus.address, "epoch-a", id, 3000)
            .map_err(|e| e.to_owned())?;
        validate(
            include_str!("../../contracts/v2/media-playback-state.schema.json"),
            &state,
        )?;
        assert_eq!(
            fixture.metrics.properties.load(Ordering::Relaxed),
            6,
            "exact property whitelist"
        );
        assert!(state.to_string().find("title").is_none());
        let result = mpris_candidate::control_private(
            &fixture.bus.address,
            "epoch-a",
            id,
            "play",
            true,
            3000,
        );
        validate(
            include_str!("../../contracts/v2/media-playback-control.schema.json"),
            &result,
        )?;
        assert_eq!(result["data"]["effectConfirmed"], false);
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 1);
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

#[test]
fn confirmation_first_uses_zero_bus_calls_and_owner_loss_is_stale()
-> Result<(), Box<dyn std::error::Error>> {
    async_io::block_on(async {
        let result = mpris_candidate::control_private(
            "unix:path=/fixture-poison",
            "epoch-a",
            "s2:m:0000000000000000",
            "play",
            false,
            50,
        );
        assert_eq!(result["data"]["accepted"], false);
        assert_eq!(result["data"]["retrySafe"], true);
        let mut fixture = Fixture::new().await?;
        let discovery = mpris_candidate::discover_private(&fixture.bus.address, "epoch-a", 3000)
            .map_err(|e| e.to_owned())?;
        let id = discovery["data"]["sessions"][0]["sessionId"]
            .as_str()
            .ok_or("id")?
            .to_owned();
        fixture.stop().await;
        assert_eq!(
            mpris_candidate::state_private(&fixture.bus.address, "epoch-a", &id, 500).unwrap_err(),
            "STALE_SESSION"
        );
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

#[test]
fn resolved_runtime_epoch_is_stable_and_guid_change_invalidates_target()
-> Result<(), Box<dyn std::error::Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let first =
            mpris_candidate::discover_private_with_resolved_epoch(&fixture.bus.address, 3000)
                .map_err(|error| error.to_owned())?;
        let second =
            mpris_candidate::discover_private_with_resolved_epoch(&fixture.bus.address, 3000)
                .map_err(|error| error.to_owned())?;
        let target = first["data"]["sessions"][0]["sessionId"]
            .as_str()
            .ok_or("resolved target")?
            .to_owned();
        assert_eq!(second["data"]["sessions"][0]["sessionId"], target);
        assert_eq!(fixture.metrics.properties.load(Ordering::Relaxed), 0);

        let state =
            mpris_candidate::state_private_with_resolved_epoch(&fixture.bus.address, &target, 3000)
                .map_err(|error| error.to_owned())?;
        assert_eq!(state["data"]["sessionId"], target);
        assert_eq!(fixture.metrics.properties.load(Ordering::Relaxed), 6);

        let unconfirmed = mpris_candidate::control_private_with_resolved_epoch(
            "unix:path=/fixture-poison-resolved-runtime",
            &target,
            "play",
            false,
            50,
        );
        assert_eq!(unconfirmed["data"]["accepted"], false);
        assert_eq!(unconfirmed["data"]["retrySafe"], true);

        let controlled = mpris_candidate::control_private_with_resolved_epoch(
            &fixture.bus.address,
            &target,
            "play",
            true,
            3000,
        );
        assert_eq!(controlled["data"]["dispatchOutcome"], "replied");
        assert_eq!(controlled["data"]["effectConfirmed"], false);
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 1);

        let replacement = Fixture::new().await?;
        let replacement_discovery =
            mpris_candidate::discover_private_with_resolved_epoch(&replacement.bus.address, 3000)
                .map_err(|error| error.to_owned())?;
        assert_ne!(
            replacement_discovery["data"]["sessions"][0]["sessionId"],
            target
        );
        assert_eq!(
            mpris_candidate::state_private_with_resolved_epoch(
                &replacement.bus.address,
                &target,
                3000,
            )
            .unwrap_err(),
            "STALE_SESSION"
        );
        for forbidden in [&fixture.bus.address, "fixture-user-bus-v1", "guid"] {
            assert!(!first.to_string().contains(forbidden));
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

#[test]
fn empty_bus_and_same_owner_alias_fail_closed_without_player_reads()
-> Result<(), Box<dyn std::error::Error>> {
    async_io::block_on(async {
        let empty = mpris_private_bus::PrivateBus::start()?;
        let result = mpris_candidate::discover_private(&empty.address, "epoch-empty", 3000)
            .map_err(|error| error.to_owned())?;
        assert_eq!(result["data"]["count"], 0);
        assert_eq!(result["data"]["complete"], true);

        let fixture = Fixture::new().await?;
        fixture.add_alias().await?;
        let result = mpris_candidate::discover_private(&fixture.bus.address, "epoch-alias", 3000)
            .map_err(|error| error.to_owned())?;
        assert_eq!(result["data"]["count"], 0);
        assert_eq!(result["data"]["complete"], false);
        assert_eq!(result["data"]["warnings"][0], "ambiguous-owner-alias");
        assert_eq!(fixture.metrics.properties.load(Ordering::Relaxed), 0);
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 0);
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

#[test]
fn multiple_players_collision_unknown_status_gate_and_post_dispatch_timeout_close()
-> Result<(), Box<dyn std::error::Error>> {
    async_io::block_on(async {
        let mut fixture = Fixture::new().await?;
        fixture
            .add_player("org.mpris.MediaPlayer2.second", "FixtureInvalid", true, 0)
            .await?;
        let discovery =
            mpris_candidate::discover_private(&fixture.bus.address, "epoch-multi", 3000)
                .map_err(|error| error.to_owned())?;
        assert_eq!(discovery["data"]["count"], 2);
        let collision = mpris_candidate::discover_private_with_forced_collision(
            &fixture.bus.address,
            "epoch-collision",
            3000,
        )
        .map_err(|error| error.to_owned())?;
        assert_eq!(collision["data"]["count"], 0);
        assert_eq!(collision["data"]["warnings"][0], "ambiguous-public-id");
        let second = discovery["data"]["sessions"]
            .as_array()
            .ok_or("sessions")?
            .iter()
            .find_map(|session| {
                let id = session["sessionId"].as_str()?;
                mpris_candidate::state_private(&fixture.bus.address, "epoch-multi", id, 3000)
                    .ok()
                    .filter(|state| state["data"]["playbackStatus"] == "unknown")
                    .map(|_| id.to_owned())
            })
            .ok_or("unknown status projection")?;
        let rejected = mpris_candidate::control_private(
            &fixture.bus.address,
            "epoch-multi",
            &second,
            "not-an-operation",
            true,
            3000,
        );
        assert_eq!(rejected["data"]["accepted"], false);
        assert_eq!(rejected["data"]["retrySafe"], true);

        let mut delayed = Fixture::new().await?;
        delayed
            .add_player("org.mpris.MediaPlayer2.delayed", "Playing", true, 150)
            .await?;
        let delayed_discovery =
            mpris_candidate::discover_private(&delayed.bus.address, "epoch-delay", 3000)
                .map_err(|error| error.to_owned())?;
        let delayed_id = delayed_discovery["data"]["sessions"]
            .as_array()
            .ok_or("delayed sessions")?
            .iter()
            .find_map(|session| {
                let id = session["sessionId"].as_str()?;
                let state =
                    mpris_candidate::state_private(&delayed.bus.address, "epoch-delay", id, 3000)
                        .ok()?;
                (state["data"]["availableControls"]["skipPrevious"] == true).then(|| id.to_owned())
            })
            .ok_or("delayed id")?;
        let unknown = mpris_candidate::control_private(
            &delayed.bus.address,
            "epoch-delay",
            &delayed_id,
            "play",
            true,
            80,
        );
        assert_eq!(unknown["data"]["accepted"], true);
        assert_eq!(unknown["data"]["dispatchOutcome"], "outcome-unknown");
        assert_eq!(unknown["data"]["automaticRetryProhibited"], true);
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

#[test]
fn schemas_reject_v1_privacy_and_effect_claims() {
    let mut state:Value=serde_json::from_str(r#"{"ok":true,"contractVersion":"act/control/v2","capability":"media.playback.state.read@2","data":{"sessionId":"s2:m:0000000000000000","targetKind":"media-session","playbackStatus":"playing","availableControls":{"play":true,"pause":true,"togglePlayPause":true,"stop":true,"skipNext":true,"skipPrevious":true},"title":"canary"}}"#).unwrap();
    assert!(
        validate(
            include_str!("../../contracts/v2/media-playback-state.schema.json"),
            &state
        )
        .is_err()
    );
    state["data"].as_object_mut().unwrap().remove("title");
    assert!(
        validate(
            include_str!("../../contracts/v2/media-playback-state.schema.json"),
            &state
        )
        .is_ok()
    );
    let v1: Value = serde_json::from_str(include_str!(
        "../../contracts/v1/media-playback-state.schema.json"
    ))
    .unwrap();
    assert!(jsonschema::draft202012::validate(&v1, &state).is_err());

    let false_effect:Value=serde_json::from_str(r#"{"ok":true,"contractVersion":"act/control/v2","capability":"media.playback.control@2","data":{"sessionId":"s2:m:0000000000000000","targetKind":"media-session","operation":"play","accepted":true,"finalStateReached":true,"dispatchOutcome":"replied","effectConfirmed":true,"retrySafe":true,"targetMayHaveMutated":true,"automaticRetryProhibited":false}}"#).unwrap();
    assert!(
        validate(
            include_str!("../../contracts/v2/media-playback-control.schema.json"),
            &false_effect
        )
        .is_err()
    );
}

#[test]
fn every_control_operation_is_gated_and_bus_generation_change_is_stale()
-> Result<(), Box<dyn std::error::Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let discovery = mpris_candidate::discover_private(&fixture.bus.address, "epoch-ops", 3000)
            .map_err(|error| error.to_owned())?;
        let id = discovery["data"]["sessions"][0]["sessionId"]
            .as_str()
            .ok_or("id")?;
        for operation in ["play", "pause", "toggle-play-pause", "stop", "skip-next"] {
            let result = mpris_candidate::control_private(
                &fixture.bus.address,
                "epoch-ops",
                id,
                operation,
                true,
                3000,
            );
            assert_eq!(result["data"]["dispatchOutcome"], "replied");
            assert_eq!(result["data"]["effectConfirmed"], false);
        }
        let previous = mpris_candidate::control_private(
            &fixture.bus.address,
            "epoch-ops",
            id,
            "skip-previous",
            true,
            3000,
        );
        assert_eq!(previous["data"]["accepted"], false);
        assert_eq!(previous["data"]["dispatchOutcome"], "pre-dispatch-rejected");
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 5);

        let replacement = Fixture::new().await?;
        assert_eq!(
            mpris_candidate::state_private(&replacement.bus.address, "epoch-ops", id, 3000)
                .unwrap_err(),
            "STALE_SESSION"
        );
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}
