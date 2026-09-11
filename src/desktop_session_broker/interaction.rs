//! 将紧凑桌面观察与输入接入原 broker 的确认、取消和重放边界。
use super::*;

fn observed_value(
    frame: &crate::modules::desktop_session::DesktopFrameCaptureReport,
    frame_id: &str,
    point_available: bool,
    session_id: &str,
) -> Value {
    json!({"sessionId":session_id,"capability":"desktop.observe@1","frameId":frame_id,"pointAvailable":point_available,"coordinateSpace":"observation-px","path":frame.path(),"width":frame.width(),"height":frame.height(),"bytes":frame.bytes(),"pixelDigest":frame.pixel_digest(),"atomicOutput":true,"replacedExisting":frame.replaced_existing(),"oneShot":true})
}

impl<P: DesktopSessionPort> Broker<P> {
    pub(super) fn execute_interaction(
        &mut self,
        request: &BrokerRequest,
    ) -> Result<Value, AppControlError> {
        match request {
            BrokerRequest::Observe {
                session_id,
                confirmed,
                strict_isolation,
                input,
                change_detection,
                ..
            } => {
                let consent = super::capture_consent(*confirmed, *strict_isolation);
                let observed = match change_detection {
                    Some(options) => {
                        self.module
                            .observe_with_changes(session_id, consent, input, Some(options))
                    }
                    None => self.module.observe(session_id, consent, input),
                }?;
                let mut data = observed_value(
                    &observed.frame,
                    &observed.frame_id,
                    observed.point_available,
                    session_id,
                );
                if let Some(changes) = observed.changes {
                    data["changes"] = changes;
                }
                Ok(data)
            }
            BrokerRequest::Interact {
                session_id,
                confirmed,
                foreground_consent,
                strict_isolation,
                input,
                observation,
                ..
            } => {
                let consent =
                    super::input_consent(*confirmed, *foreground_consent, *strict_isolation);
                if let Some(value) = observation {
                    self.module
                        .preflight_observation(session_id, consent, value)
                        .map_err(|mut error| {
                            if !error.details.is_object() {
                                error.details = json!({});
                            }
                            error.details["failedStage"] = json!("observation-preflight");
                            error.details["interactionAttempted"] = json!(false);
                            error
                        })?;
                }
                let cancellation = self.cancellations.prepare_input(request.request_nonce());
                let report = self
                    .module
                    .interact(session_id, consent, input, &cancellation)
                    .map_err(|mut error| {
                        if observation.is_some() {
                            if !error.details.is_object() {
                                error.details = json!({});
                            }
                            error.details["failedStage"] = json!("interaction");
                            error.details["observationAttempted"] = json!(false);
                        }
                        error
                    })?;
                let mut data = json!({"sessionId":session_id,"capability":"desktop.interaction@1","completedSteps":report.completed_steps,"inputEventsSent":report.input_events_sent,"effectConfirmed":false,"keysHeldAfterReturn":0,"buttonsHeldAfterReturn":0});
                if let Some(value) = observation {
                    let cancelled = cancellation.is_cancelled();
                    let observed = if cancelled {
                        Err(AppControlError::new(
                            "CANCELLED",
                            "Cancelled before post-input observation.",
                        ))
                    } else {
                        self.module.observe(session_id, consent, value)
                    }
                    .map_err(|mut error| {
                        if !error.details.is_object() {
                            error.details = json!({});
                        }
                        error.details["failedStage"] = json!("observation");
                        error.details["interaction"] = data.clone();
                        error.details["interactionCompleted"] = json!(true);
                        error.details["acceptedMayHaveOccurred"] = json!(true);
                        error.details["retrySafe"] = json!(false);
                        error.details["automaticRetryProhibited"] = json!(true);
                        error.details["observationAttempted"] = json!(!cancelled);
                        error.details["outcome"] =
                            json!(if cancelled { "cancelled" } else { "failed" });
                        error
                    })?;
                    data["observation"] = observed_value(
                        &observed.frame,
                        &observed.frame_id,
                        observed.point_available,
                        session_id,
                    );
                }
                Ok(data)
            }
            _ => Err(AppControlError::new(
                "INTERNAL_PROTOCOL_ERROR",
                "Unexpected interaction route.",
            )),
        }
    }
}
