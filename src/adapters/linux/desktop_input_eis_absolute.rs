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
            // 与观察时不一致：坐标不再可信，必须重新观察；分解差异类别
            // 供脱敏诊断（维度与代际可同时不同，维度优先报出）。
            let dimensions_match =
                mapping.width == point.mapping.width && mapping.height == point.mapping.height;
            return Err(
                self.with_event_trail(DesktopSessionInputFailure::before_dispatch(
                    "STALE_OBSERVATION",
                    frame_point_mapping_stage(dimensions_match),
                )),
            );
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
            .map_err(|failure| self.with_event_trail(before_dispatch_failure(failure)))?;
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
        // 同一检查点读取全部脱敏事实后交给纯分类：DevicePaused/Removed/
        // SeatRemoved 会同时改代际并移出列表，条件可共存；stage 按固定
        // 优先级报最先命中的失败事实，不是互斥的事件原因，也不携带任何
        // 原生设备身份。
        let device_registered = self.absolute_devices.contains(device);
        let device_alive = device.device().is_alive();
        if device_registered && device_alive && generation == self.absolute_generation {
            return Ok(());
        }
        Err(RuntimeFailure {
            code: "STALE_OBSERVATION",
            stage: absolute_input_stage(device_registered, device_alive),
        })
    }
}

/// 绝对输入新鲜度失败的脱敏分类：设备事实（列表缺失/失效）优先于代际，
/// 避免 pause/remove 同时改代际+移除列表时被代际分支遮住。
///
/// `device_registered=false` 只说明本检查点设备不在可用列表——pause、
/// remove、seat 撤销都会产生这一事实，区分它们需要真实事件轨迹，
/// 本工具不凭列表缺失断言具体事件。两个设备事实共存时仍按此优先级
/// 报第一个，文档明确 stage 不是唯一原因。
const fn absolute_input_stage(device_registered: bool, device_alive: bool) -> &'static str {
    if !device_registered {
        "absolute-input-device-unregistered"
    } else if !device_alive {
        "absolute-input-device-dead"
    } else {
        "absolute-input-generation"
    }
}

/// frame-point 映射比较失败的脱敏分类：先报维度差异，再报代际差异。
///
/// 调用点已保证 `mapping != point.mapping`；两者可同时不同，stage 只按
/// 固定优先级报最先命中者。
const fn frame_point_mapping_stage(dimensions_match: bool) -> &'static str {
    if dimensions_match {
        "frame-point-mapping-generation"
    } else {
        "frame-point-mapping-dimensions"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 反例回归：DevicePaused/Removed/SeatRemoved 同时改代际并移出列表，
    /// 设备事实必须优先于代际，否则最需要识别的设备暂停/移除全被
    /// generation 分支遮住（主脑源码反例）。
    #[test]
    fn coexisting_device_loss_is_not_masked_by_generation() {
        assert_eq!(
            absolute_input_stage(false, true),
            "absolute-input-device-unregistered"
        );
        // 设备失效与代际漂移共存：设备事实优先。
        assert_eq!(
            absolute_input_stage(false, false),
            "absolute-input-device-unregistered"
        );
        assert_eq!(
            absolute_input_stage(true, false),
            "absolute-input-device-dead"
        );
        // 设备在列表且存活时，失败只能是代际不匹配。
        assert_eq!(
            absolute_input_stage(true, true),
            "absolute-input-generation"
        );
    }

    /// frame-point 映射失败必须指出差异类别；维度与代际可同时不同，
    /// 维度优先报出。
    #[test]
    fn frame_point_mapping_reports_which_fact_differed() {
        assert_eq!(
            frame_point_mapping_stage(false),
            "frame-point-mapping-dimensions"
        );
        assert_eq!(
            frame_point_mapping_stage(true),
            "frame-point-mapping-generation"
        );
    }
}
