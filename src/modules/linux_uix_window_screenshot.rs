//! 协调 UIX 精确窗口截图、代次复核与原子 PNG 输出。

use std::{path::PathBuf, time::Duration};

use serde_json::{Value, json};

use crate::{
    adapters::linux::uix_agent,
    capabilities,
    components::{
        atomic_file::{AtomicFileError, StagedFile},
        output_guard::{OutputGuardError, guard_file_output},
        uix_window_screenshot_contract::UixWindowScreenshotInput,
    },
    domain::{AppControlError, AppResult},
    modules::uix_window,
};

trait UixScreenshotPort {
    fn capture(
        &self,
        session_id: &str,
        timeout: Duration,
    ) -> Result<uix_agent::ScreenshotRecord, uix_agent::Failure>;
}

struct SystemUixScreenshot;

impl UixScreenshotPort for SystemUixScreenshot {
    fn capture(
        &self,
        session_id: &str,
        timeout: Duration,
    ) -> Result<uix_agent::ScreenshotRecord, uix_agent::Failure> {
        uix_agent::capture_screenshot(session_id, timeout)
    }
}

/// 确认后截图精确协作式窗口，并只提交复核过代次的 PNG。
pub(crate) fn screenshot(session_id: &str, confirmed: bool, value: &Value) -> AppResult<Value> {
    screenshot_with(&SystemUixScreenshot, session_id, confirmed, value)
}

fn screenshot_with(
    port: &impl UixScreenshotPort,
    session_id: &str,
    confirmed: bool,
    value: &Value,
) -> AppResult<Value> {
    // 敏感像素读取与文件创建都必须晚于逐操作确认。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "UIX window screenshot requires explicit confirmation.",
        ));
    }
    let input = UixWindowScreenshotInput::parse(value)?;
    let destination = PathBuf::from(input.path());
    guard_file_output(&destination, input.overwrite()).map_err(output_guard_error)?;
    let mut staged = StagedFile::reserve(&destination).map_err(atomic_file_error)?;
    let capture = port
        .capture(session_id, input.timeout())
        .map_err(screenshot_error)?;
    staged.write_all(&capture.png).map_err(atomic_file_error)?;
    let commit = staged
        .commit(input.overwrite())
        .map_err(atomic_file_error)?;
    Ok(json!({
        "capability": capabilities::WINDOW_SCREENSHOT_V2,
        "targetId": session_id,
        "format": "png",
        "path": input.path(),
        "bytes": capture.png.len(),
        "width": capture.width,
        "height": capture.height,
        "pngDigest": capture.png_digest,
        "outcome": "completed",
        "captureCompleted": true,
        "artifactCommitted": true,
        "windowReResolved": true,
        "generationRevalidatedAfterCapture": true,
        "atomicOutput": true,
        "overwriteConfirmed": input.overwrite(),
        "replacedExisting": commit.replaced_existing,
        "confirmationEvaluatedBeforeInputAndDiscovery": true,
        "foregroundConsentRequired": false,
        "foregroundActivationRequested": false,
        "desktopInputInjected": false,
        "cursorCaptured": false,
        "presentedSurfaceRequired": true,
        "systemCaptureIndicatorRequested": false,
        "automaticRetryProhibited": true,
        "retrySafe": false,
        "timeoutMs": input.timeout().as_millis(),
        "executionRealm": "same-session-no-focus",
        "safety": {
            "sourceScope": "uix-application-surface",
            "providerCache": "disabled",
            "base64Exposed": false,
            "nativeIdentityExposed": false,
            "transportIdentityExposed": false,
            "globalWindowCaptureClaimed": false,
            "desktopPixelsCaptured": false,
            "x11Used": false,
            "fallback": "none",
        },
    }))
}

fn screenshot_error(failure: uix_agent::Failure) -> AppControlError {
    let mut error = match failure {
        uix_agent::Failure::Unavailable => AppControlError::with_details(
            "CAPABILITY_UNAVAILABLE",
            "The target UIX Agent did not negotiate bounded application-surface screenshots.",
            json!({
                "requiredNegotiation": [
                    "hello.capabilities.screenshot",
                    "hello.limits.max_screenshot_bytes",
                    "hello.limits.max_response_bytes"
                ],
                "executionRealm": "none",
                "retrySafe": false,
                "automaticRetryProhibited": true,
                "partialArtifactCommitted": false,
                "fallback": "none",
            }),
        ),
        other => uix_window::public_error(other),
    };
    if let Some(details) = error.details.as_object_mut() {
        details.insert("retrySafe".to_owned(), Value::Bool(false));
        details.insert("automaticRetryProhibited".to_owned(), Value::Bool(true));
        details.insert("partialArtifactCommitted".to_owned(), Value::Bool(false));
    }
    error
}

fn output_guard_error(error: OutputGuardError) -> AppControlError {
    match error {
        OutputGuardError::ConfirmationRequired => AppControlError::new(
            "OVERWRITE_CONFIRMATION_REQUIRED",
            "The UIX screenshot output exists; set overwrite=true after confirmation.",
        ),
        OutputGuardError::InspectionFailed | OutputGuardError::InvalidTargetType => {
            AppControlError::new(
                "INVALID_OUTPUT_PATH",
                "The UIX screenshot output is not a trusted regular file destination.",
            )
        }
    }
}

fn atomic_file_error(error: AtomicFileError) -> AppControlError {
    match error {
        AtomicFileError::TargetExists => AppControlError::new(
            "OVERWRITE_CONFIRMATION_REQUIRED",
            "The UIX screenshot output appeared before commit; retry with overwrite=true.",
        ),
        AtomicFileError::InvalidDestination => AppControlError::new(
            "INVALID_OUTPUT_PATH",
            "The UIX screenshot output path is not a trusted regular file destination.",
        ),
        AtomicFileError::StagingCreationFailed
        | AtomicFileError::WriteFailed
        | AtomicFileError::SyncFailed
        | AtomicFileError::CommitFailed => AppControlError::new(
            "SCREENSHOT_WRITE_FAILED",
            "The UIX screenshot PNG could not be atomically written.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use serde_json::json;

    use super::*;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);
    const FIXTURE_PNG: &[u8] = b"fixture-png-with-at-least-24-bytes";

    struct FixturePort;

    impl UixScreenshotPort for FixturePort {
        fn capture(
            &self,
            _: &str,
            _: Duration,
        ) -> Result<uix_agent::ScreenshotRecord, uix_agent::Failure> {
            Ok(uix_agent::ScreenshotRecord {
                png: FIXTURE_PNG.to_vec(),
                width: 2,
                height: 3,
                png_digest: "0123456789abcdef".to_owned(),
            })
        }
    }

    #[test]
    fn confirmation_precedes_input_target_and_output() {
        let Err(error) = screenshot("not-a-target", false, &Value::Null) else {
            panic!("未确认截图必须优先失败");
        };
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }

    #[test]
    fn verified_capture_is_atomically_committed_without_transport_identity() {
        let directory = std::env::temp_dir().join(format!(
            "act-uix-screenshot-module-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(fs::create_dir(&directory).is_ok(), "测试目录必须创建成功");
        let output = directory.join("result.png");
        let target = "s2:w:0123456789abcdef";
        let Ok(result) = screenshot_with(
            &FixturePort,
            target,
            true,
            &json!({ "path": output.to_string_lossy() }),
        ) else {
            panic!("测试截图必须成功完成");
        };
        assert_eq!(result["targetId"], target);
        assert_eq!(result["generationRevalidatedAfterCapture"], true);
        assert_eq!(result["safety"]["base64Exposed"], false);
        let envelope = json!({
            "ok": true,
            "app": "app",
            "verb": "screenshot",
            "capability": capabilities::WINDOW_SCREENSHOT_V2,
            "targetId": target,
            "executionRealm": "same-session-no-focus",
            "requiredExecutionRealm": "same-session-no-focus",
            "executionRealmCertified": true,
            "isolationRequirement": "standard",
            "hostImpactPolicy": "background-preferred",
            "data": result,
            "meta": {
                "foreground": {
                    "activationRequested": false,
                    "captureSource": "application-surface"
                },
                "targeting": "opaque exact window generation"
            }
        });
        let Ok(schema) = serde_json::from_str::<Value>(include_str!(
            "../../contracts/v2/window-screenshot-result.schema.json"
        )) else {
            panic!("结果 schema 必须成功解析");
        };
        assert!(
            jsonschema::draft202012::validate(&schema, &envelope).is_ok(),
            "测试结果必须匹配公开 schema"
        );
        let Ok(committed) = fs::read(&output) else {
            panic!("已提交输出必须可读");
        };
        assert_eq!(committed, FIXTURE_PNG);
        assert!(fs::remove_file(output).is_ok(), "测试输出必须删除成功");
        assert!(fs::remove_dir(directory).is_ok(), "测试目录必须删除成功");
    }
}
