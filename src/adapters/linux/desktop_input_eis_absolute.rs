//! 将已观察图像的坐标映射到同一 Portal 授权区域的 EIS 绝对指针。

use super::*;
use crate::components::desktop_interaction::{FrameMapping, FramePoint, normalized_point};

impl DesktopEisInput {
    pub(crate) fn set_capture_mapping(&mut self, mapping: Option<String>) {
        self.capture_mapping_id = mapping;
    }

    fn mapped_device(&self) -> Option<(Device, usize)> {
        let id = self.capture_mapping_id.as_deref()?;
        let mut matches = self.absolute_devices.iter().flat_map(|device| {
            device
                .regions()
                .iter()
                .enumerate()
                .filter(move |(_, region)| {
                    region.mapping_id.as_deref() == Some(id)
                        && region.width > 0
                        && region.height > 0
                })
                .map(move |(index, _)| (device.clone(), index))
        });
        let found = matches.next()?;
        if matches.next().is_some() {
            None
        } else {
            Some(found)
        }
    }

    pub(crate) fn frame_mapping(
        &mut self,
        liveness: &mut dyn DesktopSessionInputLiveness,
    ) -> Result<Option<FrameMapping>, DesktopSessionInputFailure> {
        let token = DesktopInputCancellation::new();
        let mut guard = InputExecutionGuard::new(&token, liveness);
        self.refresh_events(&mut guard)
            .map_err(before_dispatch_failure)?;
        Ok(self.mapped_device().map(|(device, index)| {
            let region = &device.regions()[index];
            FrameMapping {
                generation: self.absolute_generation,
                width: region.width,
                height: region.height,
            }
        }))
    }

    pub(crate) fn send_frame_point(
        &mut self,
        point: &FramePoint,
        timeout_ms: u32,
        cancellation: &DesktopInputCancellation,
        liveness: &mut dyn DesktopSessionInputLiveness,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
        let mut guard = InputExecutionGuard::new(cancellation, liveness);
        self.refresh_events(&mut guard)
            .map_err(before_dispatch_failure)?;
        let (device, index) = self.mapped_device().ok_or_else(|| {
            DesktopSessionInputFailure::before_dispatch("INPUT_MAPPING_UNAVAILABLE", "frame-point")
        })?;
        let region = &device.regions()[index];
        let mapping = FrameMapping {
            generation: self.absolute_generation,
            width: region.width,
            height: region.height,
        };
        if mapping != point.mapping {
            // 维度或代际与观察时不一致：坐标不再可信，必须重新观察。
            return Err(DesktopSessionInputFailure::before_dispatch(
                "STALE_OBSERVATION",
                "frame-point-mapping",
            ));
        }
        let (x, y) = normalized_point(point).map_err(|_| {
            DesktopSessionInputFailure::before_dispatch("INVALID_ARGUMENT", "frame-point")
        })?;
        let (x, y) = (
            (f64::from(region.x) + x * f64::from(region.width)) as f32,
            (f64::from(region.y) + y * f64::from(region.height)) as f32,
        );
        let pointer = device.interface::<ei::PointerAbsolute>().ok_or_else(|| {
            DesktopSessionInputFailure::before_dispatch("INPUT_MAPPING_UNAVAILABLE", "frame-point")
        })?;
        let button = device.interface::<ei::Button>().ok_or_else(|| {
            DesktopSessionInputFailure::before_dispatch("INPUT_MAPPING_UNAVAILABLE", "frame-point")
        })?;
        let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
        self.ensure_absolute_live(&device, point.mapping.generation, &mut guard, deadline)
            .map_err(before_dispatch_failure)?;
        let mut sent = 0usize;
        let mut held = None;
        let result = (|| {
            // emulation 会话覆盖整段点派发：已开着就只发帧，不再每个点开关一次。
            if !self.absolute_emulating {
                device
                    .device()
                    .start_emulating(self.connection.serial(), self.sequence);
                self.advance_sequence();
                self.flush_bounded(deadline).map_err(|_| RuntimeFailure {
                    code: "OUTCOME_UNKNOWN",
                    stage: "start-emulating",
                })?;
                self.absolute_emulating = true;
            }
            self.ensure_absolute_live(&device, point.mapping.generation, &mut guard, deadline)?;
            pointer.motion_absolute(x, y);
            sent += 1;
            self.flush_pointer_frame(&device, "absolute-motion", deadline)?;
            if let Some(public) = point.button {
                let code = match public {
                    1 => 0x110,
                    2 => 0x111,
                    3 => 0x112,
                    _ => {
                        return Err(RuntimeFailure {
                            code: "INVALID_ARGUMENT",
                            stage: "frame-point",
                        });
                    }
                };
                self.ensure_absolute_live(&device, point.mapping.generation, &mut guard, deadline)?;
                button.button(code, ButtonState::Press);
                held = Some(code);
                sent += 1;
                self.flush_pointer_frame(&device, "absolute-button-down", deadline)?;
                self.ensure_absolute_live(&device, point.mapping.generation, &mut guard, deadline)?;
                button.button(code, ButtonState::Released);
                sent += 1;
                self.flush_pointer_frame(&device, "absolute-button-up", deadline)?;
                held = None;
            }
            self.ensure_absolute_live(&device, point.mapping.generation, &mut guard, deadline)?;
            guard.check_with_deadline(deadline, "absolute-complete")
        })();
        if let Err(failure) = result {
            let live = self.absolute_devices.contains(&device) && device.device().is_alive();
            let released = if live {
                if let Some(code) = held {
                    button.button(code, ButtonState::Released);
                    device
                        .device()
                        .frame(self.connection.serial(), monotonic_microseconds());
                }
                device.device().stop_emulating(self.connection.serial());
                self.absolute_emulating = false;
                self.flush_bounded(Instant::now() + RELEASE_GRACE).is_ok()
            } else {
                // 设备已不在，本客户端不可能仍在仿真它。
                self.absolute_emulating = false;
                true
            };
            if failure.code == "CANCELLED" {
                return Err(DesktopSessionInputFailure::cancelled(released, 0, sent));
            }
            return Err(DesktopSessionInputFailure::after_dispatch(
                failure.code,
                failure.stage,
                released,
                0,
                sent,
            ));
        }
        Ok(DesktopPointerDispatchFacts::new(1, sent))
    }

    /// 结束本段点派发：关闭仍开着的 emulation 会话。
    ///
    /// 关不掉说明连接已经不可信，按已投递处理；标记一律清零，不留「仍在仿真」的假状态。
    pub(crate) fn finish_frame_points(&mut self) -> Result<(), DesktopSessionInputFailure> {
        if !self.absolute_emulating {
            return Ok(());
        }
        self.absolute_emulating = false;
        let Some((device, _)) = self.mapped_device() else {
            return Ok(());
        };
        if !device.device().is_alive() {
            return Ok(());
        }
        device.device().stop_emulating(self.connection.serial());
        self.flush_bounded(Instant::now() + RELEASE_GRACE).map_err(|_| {
            DesktopSessionInputFailure::after_dispatch("OUTCOME_UNKNOWN", "stop-emulating", false, 0, 0)
        })
    }

    fn ensure_absolute_live(
        &mut self,
        device: &Device,
        generation: u64,
        guard: &mut InputExecutionGuard<'_>,
        deadline: Instant,
    ) -> Result<(), RuntimeFailure> {
        guard.check_with_deadline(deadline, "absolute-input")?;
        self.refresh_events(guard)?;
        // stage 细分是脱敏诊断：generation 只随 EIS 服务端 pause/remove/
        // resume/seat 事件变化（apply_event 是唯一写点）。三种子条件对应
        // 不同的服务端状态变化，公开错误不携带任何原生设备身份。
        if generation != self.absolute_generation {
            return Err(RuntimeFailure {
                code: "STALE_OBSERVATION",
                stage: "absolute-input-generation",
            });
        }
        if !self.absolute_devices.contains(device) {
            return Err(RuntimeFailure {
                code: "STALE_OBSERVATION",
                stage: "absolute-input-device-paused",
            });
        }
        if !device.device().is_alive() {
            return Err(RuntimeFailure {
                code: "STALE_OBSERVATION",
                stage: "absolute-input-device-dead",
            });
        }
        Ok(())
    }
}
