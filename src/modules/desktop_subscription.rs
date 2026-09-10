//! 桌面订阅的确认、参数、输出与失败边界；原生 worker 始终由 lease 拥有。
use super::*;
use serde::{Deserialize, de::DeserializeOwned};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubscribeInput {
    #[serde(default = "default_duration")]
    duration_ms: u64,
}
fn default_duration() -> u64 {
    60_000
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NextInput {
    subscription_id: String,
    after_sequence: u64,
    path: String,
    #[serde(default)]
    wait_ms: u64,
    #[serde(default)]
    overwrite: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StopInput {
    subscription_id: String,
}
fn parse<T: DeserializeOwned>(value: &Value) -> AppResult<T> {
    serde_json::from_value(value.clone()).map_err(|_| {
        AppControlError::new("INVALID_ARGUMENT", "Invalid desktop subscription input.")
    })
}
fn check_id(id: &str) -> AppResult<()> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(AppControlError::new(
            "INVALID_ARGUMENT",
            "subscriptionId must be a canonical opaque subscription identity.",
        ));
    }
    Ok(())
}

impl<P: DesktopSessionPort> DesktopSessionModule<P> {
    fn subscription_error(
        &mut self,
        session_id: &str,
        failure: DesktopSessionFrameFailure,
    ) -> AppControlError {
        let cleanup = if failure.invalidates_session() {
            self.sessions
                .remove(session_id)
                .map(|entry| entry.lease.close().is_ok())
        } else {
            None
        };
        frame_port_error(failure, cleanup)
    }
    pub(crate) fn subscribe_observation(
        &mut self,
        session_id: &str,
        confirmed: bool,
        isolation: IsolationRequirement,
        value: &Value,
    ) -> AppResult<Value> {
        validate_capture_permissions(confirmed, isolation)?;
        let input: SubscribeInput = parse(value)?;
        if !(1000..=300_000).contains(&input.duration_ms) {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "durationMs must be 1000..300000.",
            ));
        }
        validate_session_target(session_id)?;
        let id = desktop_session_identity::random_nonce()?;
        self.sessions
            .get_mut(session_id)
            .ok_or_else(stale_session_error)?
            .lease
            .subscribe_frames(&id, Duration::from_millis(input.duration_ms))
            .map_err(|e| self.subscription_error(session_id, e))?;
        Ok(
            json!({"subscriptionId":id,"status":"starting","durationMs":input.duration_ms,
            "delivery":"latest-pull","maxTrackedPixels":crate::components::desktop_frame_changes::MAX_TRACKED_PIXELS,"effectConfirmed":false}),
        )
    }
    pub(crate) fn next_observation(
        &mut self,
        session_id: &str,
        confirmed: bool,
        isolation: IsolationRequirement,
        value: &Value,
    ) -> AppResult<Value> {
        validate_capture_permissions(confirmed, isolation)?;
        let input: NextInput = parse(value)?;
        check_id(&input.subscription_id)?;
        if input.wait_ms > 1000 || input.after_sequence > 9_007_199_254_740_991 {
            return Err(AppControlError::new(
                "INVALID_ARGUMENT",
                "waitMs must be 0..1000 and afterSequence a bounded nonnegative integer.",
            ));
        }
        desktop_session_frame_capture::parse_input(
            &json!({"path":input.path,"overwrite":input.overwrite}),
        )?;
        validate_session_target(session_id)?;
        if !self.sessions.contains_key(session_id) {
            return Err(stale_session_error());
        }
        let destination = PathBuf::from(&input.path);
        guard_file_output(&destination, input.overwrite).map_err(capture_output_guard_error)?;
        let mut staged = StagedFile::reserve(&destination).map_err(capture_atomic_file_error)?;
        let update = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(stale_session_error)?
            .lease
            .next_frame_update(
                &input.subscription_id,
                input.after_sequence,
                Duration::from_millis(input.wait_ms),
            )
            .map_err(|e| self.subscription_error(session_id, e))?;
        let Some(update) = update else {
            return Ok(
                json!({"subscriptionId":input.subscription_id,"status":"idle","effectConfirmed":false}),
            );
        };
        let result = (|| {
            let png = update.png().map_err(|_| {
                AppControlError::new(
                    "CAPTURE_ENCODING_FAILED",
                    "Could not encode subscription update.",
                )
            })?;
            staged.write_all(&png).map_err(capture_atomic_file_error)?;
            let commit = staged
                .commit(input.overwrite)
                .map_err(capture_atomic_file_error)?;
            Ok(
                json!({"subscriptionId":input.subscription_id,"status":"update",
                "sequence":update.sequence,"baseSequence":update.base_sequence,"generation":update.generation,
                "kind":if update.keyframe {"keyframe"} else {"patch"},"coordinateSpace":"source-px",
                "width":update.width,"height":update.height,"region":update.region,"path":input.path,"bytes":png.len(),
                "pixelDigest":crate::components::byte_digest::digest(&update.rgba),
                "captureMethod":update.method,"framesReceived":update.frames_received,"pixelsRead":update.pixels_read,
                "coalescedFrames":update.coalesced_frames,"ageMs":update.age_ms,"atomicOutput":true,"replacedExisting":commit.replaced_existing,"effectConfirmed":false}),
            )
        })();
        result.map_err(|mut error: AppControlError| {
            error.details["requiresKeyframe"] = json!(true);
            error.details["subscriptionId"] = json!(input.subscription_id);
            error.details["pixelsMayHaveBeenConsumed"] = json!(true);
            error
        })
    }
    pub(crate) fn unsubscribe_observation(
        &mut self,
        session_id: &str,
        value: &Value,
    ) -> AppResult<Value> {
        let input: StopInput = parse(value)?;
        check_id(&input.subscription_id)?;
        validate_session_target(session_id)?;
        self.sessions
            .get_mut(session_id)
            .ok_or_else(stale_session_error)?
            .lease
            .unsubscribe_frames(&input.subscription_id)
            .map_err(|e| self.subscription_error(session_id, e))?;
        Ok(json!({"subscriptionId":input.subscription_id,"status":"closed","workerJoined":true}))
    }
}
