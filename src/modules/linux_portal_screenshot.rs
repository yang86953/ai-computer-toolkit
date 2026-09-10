//! Linux 交互式 Desktop Portal Screenshot Module。

use std::{
    fs::{self, File},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::Duration,
};

use image::GenericImageView;
use serde_json::{Value, json};
use url::Url;

use crate::{
    adapters::linux::wayland_portal::{self, PortalScreenshotArtifact, PortalScreenshotFailure},
    capabilities,
    components::{
        atomic_file::{AtomicFileError, StagedFile},
        linux_host_identity,
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
        output_guard::{OutputGuardError, guard_file_output},
    },
    domain::{AppControlError, AppResult},
};

const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MAXIMUM_PNG_BYTES: u64 = 64 * 1024 * 1024;
const MAXIMUM_DIMENSION: u32 = 16_384;

#[derive(Clone, Debug, Eq, PartialEq)]
struct ScreenshotInput {
    path: String,
    timeout_ms: u32,
    overwrite: bool,
}

trait ScreenshotPortalPort {
    fn capture(
        &self,
        timeout: Duration,
    ) -> Result<PortalScreenshotArtifact, PortalScreenshotFailure>;
}

struct SystemScreenshotPortal;

impl ScreenshotPortalPort for SystemScreenshotPortal {
    fn capture(
        &self,
        timeout: Duration,
    ) -> Result<PortalScreenshotArtifact, PortalScreenshotFailure> {
        wayland_portal::screenshot(timeout)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ValidatedPng {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    digest: String,
}

/// 执行确认与前景同意门禁后的交互式 Portal 截图。
pub(crate) fn screenshot(
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    input: &Value,
) -> AppResult<Value> {
    screenshot_with(
        &SystemScreenshotPortal,
        session_id,
        confirmed,
        foreground_consent,
        input,
    )
}

fn screenshot_with(
    portal: &impl ScreenshotPortalPort,
    session_id: &str,
    confirmed: bool,
    foreground_consent: bool,
    input: &Value,
) -> AppResult<Value> {
    // 敏感屏幕读取必须在解析目标、路径或触碰文件系统之前获得逐操作确认。
    if !confirmed {
        return Err(AppControlError::new(
            "CONFIRMATION_REQUIRED",
            "Interactive desktop screenshot requires explicit confirmation.",
        ));
    }
    // Portal 会显示系统选择界面，因此必须独立取得可见前景影响同意。
    if !foreground_consent {
        return Err(AppControlError::with_details(
            "FOREGROUND_CONSENT_REQUIRED",
            "Interactive desktop screenshot requires consent for a visible Portal dialog.",
            json!({
                "capability": capabilities::DESKTOP_SCREENSHOT_INTERACTIVE,
                "executionRealm": "host-foreground",
                "visibleEffect": "system-portal-selection-dialog",
                "portalRequestIssued": false,
                "fallback": "none",
            }),
        ));
    }
    validate_target(session_id)?;
    let input = parse_input(input)?;
    let destination = PathBuf::from(&input.path);
    guard_file_output(&destination, input.overwrite).map_err(output_guard_error)?;
    let mut staged = StagedFile::reserve(&destination).map_err(atomic_file_error)?;
    let portal_artifact = portal
        .capture(Duration::from_millis(u64::from(input.timeout_ms)))
        .map_err(portal_error)?;
    let png = read_portal_png(&portal_artifact.uri)?;
    staged.write_all(&png.bytes).map_err(atomic_file_error)?;
    let commit = staged.commit(input.overwrite).map_err(atomic_file_error)?;
    Ok(json!({
        "ok": true,
        "app": "desktop",
        "operation": "screenshot-interactive",
        "capability": capabilities::DESKTOP_SCREENSHOT_INTERACTIVE,
        "targetId": session_id,
        "targetKind": "host",
        "executionDomain": "host-foreground",
        "confirmationRequired": true,
        "confirmationSatisfied": true,
        "foregroundConsentRequired": true,
        "foregroundConsentSatisfied": true,
        "foregroundImpact": "system-portal-selection-dialog-visible",
        "path": input.path,
        "bytes": png.bytes.len(),
        "width": png.width,
        "height": png.height,
        "pixelDigest": png.digest,
        "atomicOutput": true,
        "overwriteConfirmed": input.overwrite,
        "replacedExisting": commit.replaced_existing,
        "cursorCapture": "portal-defined",
        "oneShot": true,
        "coordinateMapping": "none",
        "controllableSurfaceIdentity": "none",
        "systemCaptureIndicatorMayAppear": true,
        "portal": {
            "interface": "org.freedesktop.portal.Screenshot",
            "version": portal_artifact.interface_version,
            "interactive": true,
            "requestState": "completed",
            "sourceUriExposed": false,
        },
        "fallback": "none",
    }))
}

fn validate_target(session_id: &str) -> AppResult<()> {
    if OpaqueTargetId::parse(session_id).map(OpaqueTargetId::kind) != Some(OpaqueTargetKind::Host) {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "Interactive desktop screenshot requires a canonical s2:h target.",
        ));
    }
    if session_id != linux_host_identity::current_host_target() {
        return Err(AppControlError::new(
            "STALE_SESSION",
            "The Linux host screenshot target is not current.",
        ));
    }
    Ok(())
}

fn parse_input(input: &Value) -> AppResult<ScreenshotInput> {
    let object = input.as_object().ok_or_else(|| {
        AppControlError::new(
            "INVALID_ARGUMENT",
            "Interactive desktop screenshot input must be an object.",
        )
    })?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "path" | "timeoutMs" | "overwrite"))
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "Interactive desktop screenshot accepts path, timeoutMs and overwrite only.",
        ));
    }
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| (5..=32_767).contains(&value.len()))
        .ok_or_else(|| {
            AppControlError::new(
                "INVALID_OUTPUT_PATH",
                "Interactive desktop screenshot requires a bounded UTF-8 .png path.",
            )
        })?;
    if Path::new(path).extension().and_then(|value| value.to_str()) != Some("png") {
        return Err(AppControlError::new(
            "INVALID_OUTPUT_PATH",
            "Interactive desktop screenshot output must use the .png extension.",
        ));
    }
    let timeout_ms = match object.get("timeoutMs") {
        None => DEFAULT_TIMEOUT_MS,
        Some(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                AppControlError::new(
                    "INVALID_ARGUMENT",
                    "Interactive desktop screenshot timeoutMs must be an integer.",
                )
            })?,
    };
    if !(1_000..=30_000).contains(&timeout_ms) {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "Interactive desktop screenshot timeoutMs must be 1000..30000ms.",
        ));
    }
    let overwrite = match object.get("overwrite") {
        None => false,
        Some(value) => value.as_bool().ok_or_else(|| {
            AppControlError::new(
                "INVALID_ARGUMENT",
                "Interactive desktop screenshot overwrite must be a boolean.",
            )
        })?,
    };
    Ok(ScreenshotInput {
        path: path.to_owned(),
        timeout_ms,
        overwrite,
    })
}

fn read_portal_png(uri: &str) -> AppResult<ValidatedPng> {
    let url = Url::parse(uri).map_err(|_| portal_protocol_error())?;
    if url.scheme() != "file" || url.host_str().is_some_and(|host| !host.is_empty()) {
        return Err(portal_protocol_error());
    }
    let path = url.to_file_path().map_err(|_| portal_protocol_error())?;
    let before = fs::symlink_metadata(&path).map_err(|_| screenshot_read_error())?;
    if before.file_type().is_symlink() || !before.is_file() || before.len() > MAXIMUM_PNG_BYTES {
        return Err(screenshot_read_error());
    }
    let file = File::open(&path).map_err(|_| screenshot_read_error())?;
    let opened = file.metadata().map_err(|_| screenshot_read_error())?;
    if !opened.is_file() || before.dev() != opened.dev() || before.ino() != opened.ino() {
        return Err(screenshot_read_error());
    }
    let mut bytes = Vec::new();
    file.take(MAXIMUM_PNG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| screenshot_read_error())?;
    if bytes.is_empty() || bytes.len() as u64 > MAXIMUM_PNG_BYTES {
        return Err(screenshot_read_error());
    }
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|_| screenshot_read_error())?;
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || width > MAXIMUM_DIMENSION || height > MAXIMUM_DIMENSION {
        return Err(screenshot_read_error());
    }
    let digest = fnv1a_digest(&bytes);
    Ok(ValidatedPng {
        bytes,
        width,
        height,
        digest,
    })
}

fn fnv1a_digest(bytes: &[u8]) -> String {
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:016x}")
}

fn portal_error(error: PortalScreenshotFailure) -> AppControlError {
    let (code, message, stage, retry_safe) = match error {
        PortalScreenshotFailure::Unavailable => (
            "CAPABILITY_UNAVAILABLE",
            "The Wayland Screenshot Portal is unavailable.",
            "portal-unavailable",
            true,
        ),
        PortalScreenshotFailure::ProtocolViolation => (
            "OPERATION_FAILED",
            "The Screenshot Portal returned an invalid protocol result.",
            "portal-protocol",
            false,
        ),
        PortalScreenshotFailure::OwnerDisconnected => (
            "OPERATION_FAILED",
            "The Screenshot Portal owner disconnected before a response.",
            "portal-owner-disconnected",
            true,
        ),
        PortalScreenshotFailure::Cancelled => (
            "CANCELLED",
            "The user cancelled the Screenshot Portal request.",
            "portal-user-cancelled",
            true,
        ),
        PortalScreenshotFailure::Dismissed => (
            "OPERATION_FAILED",
            "The Screenshot Portal interaction ended without a screenshot.",
            "portal-dismissed",
            true,
        ),
        PortalScreenshotFailure::Timeout => (
            "TIMEOUT",
            "The Screenshot Portal request exceeded its deadline and was closed.",
            "portal-timeout-closed",
            true,
        ),
    };
    AppControlError::with_details(
        code,
        message,
        json!({
            "platform": "linux",
            "provider": "xdg-desktop-portal-screenshot",
            "stage": stage,
            "retrySafe": retry_safe,
            "targetMayHaveMutated": false,
            "fallback": "none",
        }),
    )
}

fn output_guard_error(error: OutputGuardError) -> AppControlError {
    match error {
        OutputGuardError::ConfirmationRequired => AppControlError::new(
            "OVERWRITE_CONFIRMATION_REQUIRED",
            "The screenshot output already exists; set overwrite=true after confirmation.",
        ),
        OutputGuardError::InspectionFailed | OutputGuardError::InvalidTargetType => {
            AppControlError::new(
                "INVALID_OUTPUT_PATH",
                "The screenshot output target is not a trusted regular file path.",
            )
        }
    }
}

fn atomic_file_error(error: AtomicFileError) -> AppControlError {
    match error {
        AtomicFileError::TargetExists => AppControlError::new(
            "OVERWRITE_CONFIRMATION_REQUIRED",
            "The screenshot output appeared before atomic commit; retry with overwrite=true.",
        ),
        AtomicFileError::InvalidDestination => AppControlError::new(
            "INVALID_OUTPUT_PATH",
            "The screenshot output path is not a trusted regular file destination.",
        ),
        AtomicFileError::StagingCreationFailed
        | AtomicFileError::WriteFailed
        | AtomicFileError::SyncFailed
        | AtomicFileError::CommitFailed => AppControlError::new(
            "SCREENSHOT_WRITE_FAILED",
            "The screenshot could not be atomically written.",
        ),
    }
}

fn portal_protocol_error() -> AppControlError {
    portal_error(PortalScreenshotFailure::ProtocolViolation)
}

fn screenshot_read_error() -> AppControlError {
    AppControlError::with_details(
        "CAPTURE_READBACK_FAILED",
        "The Portal screenshot artifact is not a bounded regular PNG file.",
        json!({
            "platform": "linux",
            "provider": "xdg-desktop-portal-screenshot",
            "sourceUriExposed": false,
            "targetMayHaveMutated": false,
            "fallback": "none",
        }),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use image::{ImageFormat, Rgba, RgbaImage};

    use super::*;

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct FixturePortal {
        result: Result<PortalScreenshotArtifact, PortalScreenshotFailure>,
        calls: Cell<usize>,
    }

    impl ScreenshotPortalPort for FixturePortal {
        fn capture(
            &self,
            _: Duration,
        ) -> Result<PortalScreenshotArtifact, PortalScreenshotFailure> {
            self.calls.set(self.calls.get() + 1);
            self.result.clone()
        }
    }

    fn fixture_directory(label: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "ai-computer-toolkit-portal-{label}-{}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&directory) {
            Ok(()) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => directory,
            Err(error) => panic!("建立 Portal fixture 目录失败：{error}"),
        }
    }

    #[test]
    fn confirmation_and_foreground_consent_precede_portal_and_path_access() {
        let portal = FixturePortal {
            result: Err(PortalScreenshotFailure::ProtocolViolation),
            calls: Cell::new(0),
        };
        let error = screenshot_with(&portal, "not-a-target", false, false, &Value::Null)
            .err()
            .unwrap_or_else(|| panic!("未确认 fixture 必须失败"));
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
        assert_eq!(portal.calls.get(), 0);

        let error = screenshot_with(&portal, "not-a-target", true, false, &Value::Null)
            .err()
            .unwrap_or_else(|| panic!("缺前景同意 fixture 必须失败"));
        assert_eq!(error.code, "FOREGROUND_CONSENT_REQUIRED");
        assert_eq!(error.details["portalRequestIssued"], false);
        assert_eq!(portal.calls.get(), 0);
    }

    #[test]
    fn portal_fixture_copies_png_to_atomic_public_artifact() {
        let directory = fixture_directory("success");
        let source = directory.join("portal-source.png");
        let destination = directory.join("result.png");
        RgbaImage::from_pixel(2, 1, Rgba([10, 20, 30, 255]))
            .save_with_format(&source, ImageFormat::Png)
            .unwrap_or_else(|error| panic!("写入 Portal PNG fixture 失败：{error}"));
        let uri = Url::from_file_path(&source)
            .unwrap_or_else(|_| panic!("fixture 路径必须可转换为 file URI"))
            .to_string();
        let portal = FixturePortal {
            result: Ok(PortalScreenshotArtifact {
                uri,
                interface_version: 2,
            }),
            calls: Cell::new(0),
        };
        let result = screenshot_with(
            &portal,
            &linux_host_identity::current_host_target(),
            true,
            true,
            &json!({ "path": destination.to_string_lossy(), "timeoutMs": 1000 }),
        )
        .unwrap_or_else(|error| panic!("Portal 成功 fixture 失败：{error:?}"));
        assert_eq!(
            result["capability"],
            capabilities::DESKTOP_SCREENSHOT_INTERACTIVE
        );
        assert_eq!(result["width"], 2);
        assert_eq!(result["height"], 1);
        assert_eq!(result["atomicOutput"], true);
        assert_eq!(result["portal"]["sourceUriExposed"], false);
        assert_eq!(result["oneShot"], true);
        assert_eq!(result["coordinateMapping"], "none");
        assert_eq!(result["controllableSurfaceIdentity"], "none");
        assert!(result.get("windowTargetId").is_none());
        assert!(result.get("foregroundUnchanged").is_none());
        assert_eq!(portal.calls.get(), 1);
        assert!(destination.is_file());
        fs::remove_file(source).unwrap_or_else(|error| panic!("清理源 fixture 失败：{error}"));
        fs::remove_file(destination)
            .unwrap_or_else(|error| panic!("清理结果 fixture 失败：{error}"));
        fs::remove_dir(directory).unwrap_or_else(|error| panic!("清理 fixture 目录失败：{error}"));
    }

    #[test]
    fn portal_cancel_and_timeout_keep_output_absent() {
        for (label, failure, expected) in [
            ("cancel", PortalScreenshotFailure::Cancelled, "CANCELLED"),
            ("timeout", PortalScreenshotFailure::Timeout, "TIMEOUT"),
        ] {
            let directory = fixture_directory(label);
            let destination = directory.join("result.png");
            let portal = FixturePortal {
                result: Err(failure),
                calls: Cell::new(0),
            };
            let error = screenshot_with(
                &portal,
                &linux_host_identity::current_host_target(),
                true,
                true,
                &json!({ "path": destination.to_string_lossy(), "timeoutMs": 1000 }),
            )
            .err()
            .unwrap_or_else(|| panic!("Portal 失败 fixture 必须失败"));
            assert_eq!(error.code, expected);
            assert_eq!(error.details["targetMayHaveMutated"], false);
            assert!(!destination.exists());
            fs::remove_dir(directory)
                .unwrap_or_else(|cleanup| panic!("清理 {label} fixture 失败：{cleanup}"));
        }
    }

    #[test]
    fn portal_owner_disconnect_is_structured_and_leaves_no_output() {
        let directory = fixture_directory("owner-disconnect");
        let destination = directory.join("result.png");
        let portal = FixturePortal {
            result: Err(PortalScreenshotFailure::OwnerDisconnected),
            calls: Cell::new(0),
        };
        let error = screenshot_with(
            &portal,
            &linux_host_identity::current_host_target(),
            true,
            true,
            &json!({ "path": destination.to_string_lossy(), "timeoutMs": 1000 }),
        )
        .err()
        .unwrap_or_else(|| panic!("Portal owner 断开 fixture 必须失败"));
        assert_eq!(error.code, "OPERATION_FAILED");
        assert_eq!(error.details["stage"], "portal-owner-disconnected");
        assert!(!destination.exists());
        fs::remove_dir(directory)
            .unwrap_or_else(|cleanup| panic!("清理 owner fixture 失败：{cleanup}"));
    }
}
