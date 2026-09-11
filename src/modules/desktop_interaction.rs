//! 在单一桌面 lease 中执行紧凑交互；整批预检、统一期限、失败后不重放。

use super::*;
use crate::components::desktop_frame_changes::{ChangeOptions, MAX_TRACKED_PIXELS};
use crate::components::desktop_interaction::{self, FrameMapping, FramePoint, InteractionStep};
use std::{thread, time::Instant};

pub(crate) struct ChangeBaseline {
    rgba: Vec<u8>,
    source_size: (u32, u32),
}

pub(crate) struct Observation {
    id: String,
    width: u32,
    height: u32,
    mapping: Option<FrameMapping>,
    baseline: Option<ChangeBaseline>,
}

pub(crate) struct ObservedFrame {
    pub frame: DesktopFrameCaptureReport,
    pub frame_id: String,
    pub point_available: bool,
    pub changes: Option<Value>,
}

pub(crate) struct InteractionReport {
    pub completed_steps: usize,
    pub input_events_sent: usize,
}

impl<P: DesktopSessionPort> DesktopSessionModule<P> {
    fn checked_mapping(&mut self, session_id: &str) -> AppResult<Option<FrameMapping>> {
        let result = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(stale_session_error)?
            .lease
            .frame_mapping();
        match result {
            Ok(mapping) => Ok(mapping),
            Err(failure) => {
                let entry = self
                    .sessions
                    .remove(session_id)
                    .ok_or_else(stale_session_error)?;
                Err(input_port_error(failure, entry.lease.close().is_ok()))
            }
        }
    }

    /// 在组合调用发送输入前校验截图参数和输出位置；实际提交仍再次检查。
    pub(crate) fn preflight_observation(
        &self,
        session_id: &str,
        consent: DesktopConsent,
        value: &Value,
    ) -> AppResult<()> {
        self.resolve_capture_consent(session_id, consent)?;
        let input = desktop_session_frame_capture::parse_input(value)?;
        validate_session_target(session_id)?;
        if !self.sessions.contains_key(session_id) {
            return Err(stale_session_error());
        }
        let destination = PathBuf::from(input.path());
        guard_file_output(&destination, input.overwrite()).map_err(capture_output_guard_error)?;
        // Reserve/drop a staging file to catch unwritable parents before input.
        let _staged = StagedFile::reserve(&destination).map_err(capture_atomic_file_error)?;
        Ok(())
    }

    /// 截图前后保持同一输入映射，成功后替换本会话的最近观察身份。
    pub(crate) fn observe(
        &mut self,
        session_id: &str,
        consent: DesktopConsent,
        value: &Value,
    ) -> AppResult<ObservedFrame> {
        self.observe_with_changes(session_id, consent, value, None)
    }

    /// 可选差分只比较调用方明确引用的最近基准，失败后不沿用旧定位。
    pub(crate) fn observe_with_changes(
        &mut self,
        session_id: &str,
        consent: DesktopConsent,
        value: &Value,
        change_detection: Option<&Value>,
    ) -> AppResult<ObservedFrame> {
        self.resolve_capture_consent(session_id, consent)?;
        let options = change_detection.map(ChangeOptions::parse).transpose()?;
        desktop_session_frame_capture::parse_input(value)?;
        validate_session_target(session_id)?;
        let frame_id = desktop_session_identity::random_nonce()?;
        let entry = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(stale_session_error)?;
        if let Some(id) = options.as_ref().and_then(|o| o.baseline_frame_id.as_ref()) {
            let previous = entry.observation.as_ref()
                .filter(|o| &o.id == id && o.baseline.is_some())
                .ok_or_else(|| AppControlError::new("STALE_OBSERVATION", "Use the latest tracked frame from this session, or omit baselineFrameId to reset."))?;
            if let Some(options) = &options {
                options.checked_region(previous.width, previous.height)?;
            }
        }
        let previous = entry.observation.take().filter(|o| {
            options
                .as_ref()
                .and_then(|options| options.baseline_frame_id.as_ref())
                == Some(&o.id)
        });
        let before = self.checked_mapping(session_id)?;
        let mut frame = self.capture_frame(session_id, consent, value)?;
        let after = match self.checked_mapping(session_id) {
            Ok(mapping) => mapping,
            Err(mut error) => {
                error.details["capturedPath"] = json!(frame.path());
                error.details["pixelsMayHaveBeenConsumed"] = json!(true);
                return Err(error);
            }
        };
        let mapping = before.filter(|m| Some(*m) == after);
        let changes = options
            .as_ref()
            .map(|options| observation_changes(options, previous.as_ref(), &frame, before, after))
            .transpose()
            .map_err(|mut error| {
                error.details["capturedPath"] = json!(frame.path());
                error.details["pixelsMayHaveBeenConsumed"] = json!(true);
                error
            })?;
        let baseline = (options.is_some()
            && u64::from(frame.width()) * u64::from(frame.height()) <= MAX_TRACKED_PIXELS)
            .then(|| ChangeBaseline {
                rgba: std::mem::take(&mut frame.rgba),
                source_size: frame.source_size(),
            });
        let entry = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(stale_session_error)?;
        entry.observation = Some(Observation {
            id: frame_id.clone(),
            width: frame.width(),
            height: frame.height(),
            mapping,
            baseline,
        });
        Ok(ObservedFrame {
            frame,
            frame_id,
            point_available: mapping.is_some(),
            changes,
        })
    }

    /// 一次调用可以完成一组已明确的控件操作，不能执行脚本或应用代码。
    pub(crate) fn interact(
        &mut self,
        session_id: &str,
        consent: DesktopConsent,
        value: &Value,
        cancellation: &DesktopInputCancellation,
    ) -> AppResult<InteractionReport> {
        self.resolve_input_consent(session_id, consent)?;
        let plan = desktop_interaction::parse(value)?;
        validate_session_target(session_id)?;
        let current_mapping = if plan
            .steps
            .iter()
            .any(|step| matches!(step, InteractionStep::Point { .. }))
        {
            self.checked_mapping(session_id)?
        } else {
            None
        };
        let entry = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(stale_session_error)?;
        let observation = match &plan.frame_id {
            Some(id) => Some(
                entry
                    .observation
                    .as_ref()
                    .filter(|o| &o.id == id)
                    .ok_or_else(|| {
                        AppControlError::new(
                            "STALE_OBSERVATION",
                            "Use the latest observation from this session.",
                        )
                    })?,
            ),
            None => None,
        };
        let mut points = Vec::with_capacity(plan.steps.len());
        for step in &plan.steps {
            let point = if let InteractionStep::Point { x, y, button } = step {
                let o = observation.ok_or_else(|| {
                    AppControlError::with_details(
                        "STALE_OBSERVATION",
                        "A point requires an observation.",
                        json!({"stage": "observation-missing"}),
                    )
                })?;
                let mapping = o.mapping.ok_or_else(|| {
                    AppControlError::with_details(
                        "INPUT_MAPPING_UNAVAILABLE",
                        "The captured stream has no matching absolute input region.",
                        json!({"stage": "observation-mapping-unavailable"}),
                    )
                })?;
                if current_mapping != Some(mapping) {
                    return Err(AppControlError::with_details(
                        "STALE_OBSERVATION",
                        "The input region changed after observation.",
                        json!({"stage": "observation-mapping"}),
                    ));
                }
                let point = FramePoint {
                    x: *x,
                    y: *y,
                    image_width: o.width,
                    image_height: o.height,
                    mapping,
                    button: *button,
                };
                desktop_interaction::normalized_point(&point)?;
                Some(point)
            } else {
                None
            };
            points.push(point);
        }
        let deadline = Instant::now() + Duration::from_millis(u64::from(plan.timeout_ms));
        let mut completed = 0usize;
        let mut sent = 0usize;
        for (step, point) in plan.steps.iter().zip(points) {
            let result = (|| {
                interaction_checkpoint(cancellation, deadline)?;
                match step {
                    InteractionStep::Keyboard(chunks) => {
                        for input in chunks {
                            let mut input = input.clone();
                            input.timeout_ms = remaining_ms(deadline)?;
                            let facts = entry.lease.send_keyboard(&input, cancellation)?;
                            sent = sent.saturating_add(facts.input_events_sent());
                        }
                    }
                    InteractionStep::Point { .. } => {
                        let facts = entry.lease.send_frame_point(
                            &point.ok_or_else(|| {
                                DesktopSessionInputFailure::before_dispatch(
                                    "INTERNAL_PROTOCOL_ERROR",
                                    "interaction-point",
                                )
                            })?,
                            remaining_ms(deadline)?,
                            cancellation,
                        )?;
                        sent = sent.saturating_add(facts.input_events_sent());
                    }
                    InteractionStep::Wait(ms) => {
                        let until = Instant::now() + Duration::from_millis(u64::from(*ms));
                        while Instant::now() < until {
                            interaction_checkpoint(cancellation, deadline)?;
                            thread::sleep(
                                until
                                    .saturating_duration_since(Instant::now())
                                    .min(Duration::from_millis(10)),
                            );
                        }
                    }
                }
                interaction_checkpoint(cancellation, deadline)
            })();
            if let Err(failure) = result {
                let mut entry = self
                    .sessions
                    .remove(session_id)
                    .ok_or_else(stale_session_error)?;
                // 批内失败同样要收掉后端可能仍开着的注入会话，再关 lease。
                let released = entry.lease.finish_frame_points().is_ok();
                let cleanup = entry.lease.close().is_ok();
                return Err(interaction_input_error(
                    failure, cleanup, sent, completed, released,
                ));
            }
            completed += 1;
        }
        // 批结束：后端可能跨整批维持着一个注入会话（EIS 绝对指针就是如此），这里显式收尾。
        // 收不掉说明连接已经不可信：按批内失败的同一套事实处理，作废会话、不重放。
        if let Err(failure) = entry.lease.finish_frame_points() {
            let entry = self
                .sessions
                .remove(session_id)
                .ok_or_else(stale_session_error)?;
            let cleanup = entry.lease.close().is_ok();
            return Err(interaction_input_error(
                failure, cleanup, sent, completed, false,
            ));
        }
        entry.view.input_events_sent = entry.view.input_events_sent.saturating_add(sent);
        Ok(InteractionReport {
            completed_steps: completed,
            input_events_sent: sent,
        })
    }
}

/// 把后端输入失败补齐成对调用方可见的收尾事实。
///
/// 输入失败必须同时说清三件事：有没有可能已经发出事件、完成到第几步、注入会话收没收干净。
/// 批内失败与批末收尾失败两条路径必须同形，所以只在这里定义一次。
fn interaction_input_error(
    failure: DesktopSessionInputFailure,
    session_cleanup_confirmed: bool,
    sent: usize,
    completed: usize,
    released: bool,
) -> AppControlError {
    let accepted = sent > 0 || failure.accepted_may_have_occurred();
    let mut error = input_port_error(failure, session_cleanup_confirmed);
    error.details["acceptedMayHaveOccurred"] = json!(accepted);
    error.details["outcome"] = json!(if failure.code() == "CANCELLED" {
        "cancelled"
    } else if accepted {
        "unknown"
    } else {
        "failed"
    });
    error.details["completedInteractionSteps"] = json!(completed);
    error.details["inputEventsSent"] = json!(sent.saturating_add(failure.input_events_sent()));
    error.details["releasesConfirmed"] = json!(failure.releases_confirmed() && released);
    error
}

fn observation_changes(
    options: &ChangeOptions,
    previous: Option<&Observation>,
    frame: &DesktopFrameCaptureReport,
    before: Option<FrameMapping>,
    after: Option<FrameMapping>,
) -> AppResult<Value> {
    options.checked_region(frame.width(), frame.height())?;
    let previous = previous.filter(|p| options.baseline_frame_id.as_ref() == Some(&p.id));
    let reason = if u64::from(frame.width()) * u64::from(frame.height()) > MAX_TRACKED_PIXELS {
        Some("tracking-limit")
    } else if before != after || previous.is_some_and(|p| p.mapping != before) {
        Some("mapping-changed")
    } else if let Some(p) = previous {
        if p.width != frame.width()
            || p.height != frame.height()
            || p.baseline
                .as_ref()
                .is_none_or(|b| b.source_size != frame.source_size())
        {
            Some("dimensions-changed")
        } else {
            None
        }
    } else {
        Some("baseline-created")
    };
    let mut result = if let Some(reason) = reason {
        json!({"status":if reason == "tracking-limit" {"unavailable"} else {"baseline-reset"},
            "reason":reason,"changed":null})
    } else {
        let baseline = previous.and_then(|p| p.baseline.as_ref()).ok_or_else(|| {
            AppControlError::new("STALE_OBSERVATION", "No tracked baseline is available.")
        })?;
        options.compare(&baseline.rgba, &frame.rgba, frame.width(), frame.height())?
    };
    result["baselineFrameId"] = json!(options.baseline_frame_id);
    result["coordinateSpace"] = json!("observation-px");
    result["method"] = json!("full-frame-rgba-diff");
    result["effectConfirmed"] = json!(false);
    Ok(result)
}

fn remaining_ms(deadline: Instant) -> Result<u32, DesktopSessionInputFailure> {
    let duration = deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| {
            DesktopSessionInputFailure::before_dispatch("TIMEOUT", "interaction-deadline")
        })?;
    Ok(u32::try_from(duration.as_millis().max(1)).unwrap_or(30_000))
}

fn interaction_checkpoint(
    cancellation: &DesktopInputCancellation,
    deadline: Instant,
) -> Result<(), DesktopSessionInputFailure> {
    if cancellation.is_cancelled() {
        return Err(DesktopSessionInputFailure::cancelled(true, 0, 0));
    }
    remaining_ms(deadline).map(|_| ())
}

#[cfg(test)]
#[path = "linux_desktop_interaction_tests.rs"]
mod tests;
