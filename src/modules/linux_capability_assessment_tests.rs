use super::{assess, snapshot_error};
use crate::capabilities;
use crate::domain::error_json;

#[test]
fn assessment_snapshot_failure_matches_public_error_contract() {
    let instance = error_json(&snapshot_error(std::io::Error::other("fixture")));
    let Ok(schema) = serde_json::from_str(include_str!(
        "../../contracts/v1/error-envelope.schema.json"
    )) else {
        panic!("schema must parse");
    };
    if let Err(error) = jsonschema::draft202012::validate(&schema, &instance) {
        panic!("assessment snapshot error must validate: {error}");
    }
    assert_eq!(instance["error"]["code"], "PROCESS_SNAPSHOT_FAILED");
}

#[test]
fn unknown_capability_is_rejected_before_target_routing() {
    let Err(error) = assess("unknown.future@99", "not-a-target") else {
        panic!("unknown capability must not degrade to ordinary unavailable");
    };
    assert_eq!(error.code, "INVALID_ARGUMENT");
    assert_eq!(error.details["reason"], "unknown-capability-id");
}

#[test]
fn portal_keyboard_assessment_requires_the_owning_broker() {
    let assessment = assess(capabilities::UI_INPUT_KEY_V3, "s2:i:0123456789abcdef")
        .unwrap_or_else(|error| panic!("Portal keyboard assessment failed: {error}"));
    assert_eq!(assessment["decision"], "unavailable");
    assert_eq!(assessment["requiresConfirmation"], true);
    assert_eq!(assessment["requiresForegroundConsent"], true);
    assert_eq!(
        assessment["constraints"]["assessmentRoute"],
        "session-host-desktop-input-key"
    );
    assert_eq!(assessment["constraints"]["noFallback"], true);
    assert_eq!(assessment["evidence"]["nativeIdentityExposed"], false);
}

#[test]
fn portal_pointer_assessment_requires_the_owning_broker() {
    let assessment = assess(capabilities::UI_INPUT_POINTER_V3, "s2:i:0123456789abcdef")
        .unwrap_or_else(|error| panic!("Portal pointer assessment failed: {error}"));
    assert_eq!(assessment["decision"], "unavailable");
    assert_eq!(assessment["requiresConfirmation"], true);
    assert_eq!(assessment["requiresForegroundConsent"], true);
    assert_eq!(
        assessment["constraints"]["assessmentRoute"],
        "session-host-desktop-input-pointer"
    );
    assert_eq!(assessment["constraints"]["noFallback"], true);
    assert_eq!(assessment["evidence"]["nativeIdentityExposed"], false);
}

#[test]
fn portal_frame_capture_assessment_requires_the_owning_broker() {
    let assessment = assess(capabilities::SCREEN_CAPTURE, "s2:i:0123456789abcdef")
        .unwrap_or_else(|error| panic!("Portal frame assessment failed: {error}"));
    assert_eq!(assessment["decision"], "unavailable");
    assert_eq!(assessment["requiresConfirmation"], true);
    assert_eq!(assessment["requiresForegroundConsent"], false);
    assert_eq!(
        assessment["constraints"]["assessmentRoute"],
        "session-host-desktop-capture-frame"
    );
    assert_eq!(assessment["constraints"]["noFallback"], true);
    assert_eq!(assessment["evidence"]["nativeIdentityExposed"], false);
    assert_eq!(assessment["evidence"]["automaticRetryAllowed"], false);
}
