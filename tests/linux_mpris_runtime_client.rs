#![cfg(all(target_os = "linux", feature = "linux-mpris-candidate"))]

//! Linux MPRIS runtime client 的私有总线组合、代际和门禁契约。

#[allow(dead_code)]
#[path = "support/mpris_private_bus.rs"]
mod mpris_private_bus;

use std::{
    error::Error,
    io,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};

use ai_computer_toolkit::{AppControlError, mpris_runtime_client};
use jsonschema::draft202012;
use mpris_private_bus::Fixture;
use serde_json::{Value, json};

const TOOLKIT: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit");
const DISCOVER_SCHEMA: &str = include_str!("../contracts/v2/media-session-observation.schema.json");
const STATE_SCHEMA: &str = include_str!("../contracts/v2/media-playback-state.schema.json");
const CONTROL_SCHEMA: &str = include_str!("../contracts/v2/media-playback-control.schema.json");
const FIXTURE_EPOCH_FRAGMENT: &str = "fixture-user-bus-v1";

fn validate(schema_text: &str, value: &Value) -> Result<(), Box<dyn Error>> {
    let schema = serde_json::from_str::<Value>(schema_text)?;
    draft202012::validate(&schema, value).map_err(|error| error.to_string())?;
    Ok(())
}

fn required_target(value: &Value) -> Result<String, Box<dyn Error>> {
    value["data"]["sessions"]
        .as_array()
        .and_then(|sessions| sessions.first())
        .and_then(|session| session["sessionId"].as_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::other("runtime discovery returned no opaque target").into())
}

fn address_guid(address: &str) -> Option<&str> {
    address
        .split(',')
        .find_map(|part| part.strip_prefix("guid="))
        .filter(|guid| !guid.is_empty())
}

fn assert_private_json(
    value: &Value,
    address: &str,
    epoch_fragment: &str,
) -> Result<(), Box<dyn Error>> {
    let serialized = serde_json::to_string(value)?;
    for forbidden in [
        address,
        epoch_fragment,
        "sessionBusAddress",
        "brokerEpoch",
        "org.mpris",
        "\"owner\"",
        "\"path\"",
        "\"pid\"",
        "\"native\"",
        "\"transport\"",
        "\"busAddress\"",
        "guid",
    ] {
        assert!(!serialized.contains(forbidden));
    }
    if let Some(guid) = address_guid(address) {
        assert!(!serialized.contains(guid));
    }
    Ok(())
}

fn assert_private_error(
    error: &AppControlError,
    address: &str,
    epoch_fragment: &str,
) -> Result<(), Box<dyn Error>> {
    let value = json!({
        "code": error.code,
        "message": &error.message,
        "details": &error.details,
    });
    assert_private_json(&value, address, epoch_fragment)
}

#[test]
fn resolved_discover_is_stable_and_state_reads_exact_properties() -> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let executable = Path::new(TOOLKIT);

        let first =
            mpris_runtime_client::discover_private(executable, &fixture.bus.address, 128, 3000)?;
        validate(DISCOVER_SCHEMA, &first)?;
        let second =
            mpris_runtime_client::discover_private(executable, &fixture.bus.address, 128, 3000)?;
        validate(DISCOVER_SCHEMA, &second)?;
        assert_eq!(first["data"]["sessions"], second["data"]["sessions"]);
        assert_eq!(fixture.metrics.properties.load(Ordering::Relaxed), 0);
        assert_private_json(&first, &fixture.bus.address, FIXTURE_EPOCH_FRAGMENT)?;
        assert_private_json(&second, &fixture.bus.address, FIXTURE_EPOCH_FRAGMENT)?;

        let target = required_target(&first)?;
        let state = mpris_runtime_client::state_private(
            executable,
            &fixture.bus.address,
            &target,
            128,
            3000,
        )?;
        validate(STATE_SCHEMA, &state)?;
        assert_eq!(state["data"]["sessionId"], target);
        assert_eq!(fixture.metrics.properties.load(Ordering::Relaxed), 6);
        assert_private_json(&state, &fixture.bus.address, FIXTURE_EPOCH_FRAGMENT)?;
        Ok::<(), Box<dyn Error>>(())
    })
}

#[test]
fn replacement_private_bus_rejects_old_target_as_stale() -> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let first_fixture = Fixture::new().await?;
        let executable = Path::new(TOOLKIT);
        let discovery = mpris_runtime_client::discover_private(
            executable,
            &first_fixture.bus.address,
            128,
            3000,
        )?;
        let old_target = required_target(&discovery)?;

        let replacement = Fixture::new().await?;
        let error = match mpris_runtime_client::state_private(
            executable,
            &replacement.bus.address,
            &old_target,
            128,
            3000,
        ) {
            Ok(_) => {
                return Err(io::Error::other("replacement bus accepted an old target").into());
            }
            Err(error) => error,
        };
        assert_eq!(error.code, "STALE_SESSION");
        assert_private_error(&error, &replacement.bus.address, FIXTURE_EPOCH_FRAGMENT)?;
        assert_eq!(replacement.metrics.properties.load(Ordering::Relaxed), 0);
        assert_eq!(replacement.metrics.methods.load(Ordering::Relaxed), 0);
        Ok::<(), Box<dyn Error>>(())
    })
}

#[test]
fn unconfirmed_control_precedes_poison_address_executable_and_zero_limits()
-> Result<(), Box<dyn Error>> {
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let poison_address = "unix:path=/fixture-poison-runtime-client";
        let relative_executable = Path::new("relative-mpris-worker");
        let missing_executable = Path::new(
            "/tmp/ai-computer-toolkit-mpris-runtime-client-confirmation-missing-executable",
        );

        for executable in [relative_executable, missing_executable] {
            let result = mpris_runtime_client::control_private(
                executable,
                poison_address,
                "s2:m:0000000000000000",
                "play",
                false,
                0,
                0,
            )?;
            assert_eq!(result["data"]["dispatchOutcome"], "pre-dispatch-rejected");
            assert_eq!(result["data"]["accepted"], false);
            assert_private_json(&result, poison_address, FIXTURE_EPOCH_FRAGMENT)?;
        }
        assert_eq!(fixture.metrics.properties.load(Ordering::Relaxed), 0);
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 0);
        Ok::<(), Box<dyn Error>>(())
    })
}

#[test]
fn executable_errors_are_precise_and_confirmed_play_uses_fixed_image() -> Result<(), Box<dyn Error>>
{
    async_io::block_on(async {
        let fixture = Fixture::new().await?;
        let executable = Path::new(TOOLKIT);
        let discovery =
            mpris_runtime_client::discover_private(executable, &fixture.bus.address, 128, 3000)?;
        let target = required_target(&discovery)?;

        let relative_error = match mpris_runtime_client::control_private(
            Path::new("relative-mpris-worker"),
            &fixture.bus.address,
            &target,
            "play",
            true,
            128,
            3000,
        ) {
            Ok(_) => return Err(io::Error::other("relative executable unexpectedly ran").into()),
            Err(error) => error,
        };
        assert_eq!(relative_error.code, "WORKER_PROTOCOL_ERROR");
        assert_private_error(
            &relative_error,
            &fixture.bus.address,
            FIXTURE_EPOCH_FRAGMENT,
        )?;

        let missing_path = PathBuf::from(
            "/tmp/ai-computer-toolkit-mpris-runtime-client-definitely-missing-executable",
        );
        let missing_error = match mpris_runtime_client::control_private(
            &missing_path,
            &fixture.bus.address,
            &target,
            "play",
            true,
            128,
            3000,
        ) {
            Ok(_) => return Err(io::Error::other("missing executable unexpectedly ran").into()),
            Err(error) => error,
        };
        assert_eq!(missing_error.code, "ISOLATED_WORKER_UNAVAILABLE");
        assert_private_error(&missing_error, &fixture.bus.address, FIXTURE_EPOCH_FRAGMENT)?;

        let result = mpris_runtime_client::control_private(
            executable,
            &fixture.bus.address,
            &target,
            "play",
            true,
            128,
            3000,
        )?;
        validate(CONTROL_SCHEMA, &result)?;
        assert_eq!(result["data"]["accepted"], true);
        assert_eq!(result["data"]["dispatchOutcome"], "replied");
        assert_eq!(result["data"]["finalStateReached"], true);
        assert_eq!(result["data"]["effectConfirmed"], false);
        assert_eq!(result["data"]["retrySafe"], false);
        assert_eq!(result["data"]["automaticRetryProhibited"], true);
        assert_private_json(&result, &fixture.bus.address, FIXTURE_EPOCH_FRAGMENT)?;
        assert_eq!(fixture.metrics.methods.load(Ordering::Relaxed), 1);
        Ok::<(), Box<dyn Error>>(())
    })
}
