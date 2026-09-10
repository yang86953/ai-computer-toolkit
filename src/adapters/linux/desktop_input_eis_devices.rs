//! 管理 EIS 设备代际与已声明的输入能力。

use super::*;

impl DesktopEisInput {
    pub(super) fn apply_event(&mut self, event: EiEvent) -> Result<(), RuntimeFailure> {
        match event {
            EiEvent::SeatAdded(added) => {
                added.seat.bind_capabilities(
                    DeviceCapability::Keyboard
                        | DeviceCapability::PointerAbsolute
                        | DeviceCapability::Pointer
                        | DeviceCapability::Button
                        | DeviceCapability::Scroll,
                );
                self.connection.flush().map_err(|_| RuntimeFailure {
                    code: "EIS_DEVICE_UNAVAILABLE",
                    stage: "eis-bind-input",
                })?;
            }
            EiEvent::DeviceAdded(added) => {
                if added.device.device().version() >= 3 {
                    added.device.device().ready();
                    self.connection.flush().map_err(|_| RuntimeFailure {
                        code: "EIS_DEVICE_UNAVAILABLE",
                        stage: "eis-device-ready",
                    })?;
                }
            }
            EiEvent::DeviceResumed(resumed) => {
                if resumed
                    .device
                    .has_capability(DeviceCapability::PointerAbsolute)
                    && resumed.device.has_capability(DeviceCapability::Button)
                    && !self.absolute_devices.contains(&resumed.device)
                    && self.absolute_devices.len() < 16
                {
                    self.absolute_devices.push(resumed.device.clone());
                    self.absolute_generation = self.absolute_generation.wrapping_add(1);
                }
                if resumed.device.has_capability(DeviceCapability::Keyboard)
                    && (self.keyboard_device.is_none()
                        || self.keyboard_device.as_ref() == Some(&resumed.device))
                {
                    self.keyboard_device = Some(resumed.device.clone());
                }
                if device_supports_pointer(&resumed.device)
                    && (self.pointer_device.is_none()
                        || self.pointer_device.as_ref() == Some(&resumed.device))
                {
                    self.pointer_device = Some(resumed.device);
                }
            }
            EiEvent::DevicePaused(paused) => {
                if self.absolute_devices.contains(&paused.device) {
                    self.absolute_devices
                        .retain(|device| device != &paused.device);
                    self.absolute_generation = self.absolute_generation.wrapping_add(1);
                }
                if self.keyboard_device.as_ref() == Some(&paused.device) {
                    self.keyboard_device = None;
                }
                if self.pointer_device.as_ref() == Some(&paused.device) {
                    self.pointer_device = None;
                }
            }
            EiEvent::DeviceRemoved(removed) => {
                if self.absolute_devices.contains(&removed.device) {
                    self.absolute_devices
                        .retain(|device| device != &removed.device);
                    self.absolute_generation = self.absolute_generation.wrapping_add(1);
                }
                if self.keyboard_device.as_ref() == Some(&removed.device) {
                    self.keyboard_device = None;
                }
                if self.pointer_device.as_ref() == Some(&removed.device) {
                    self.pointer_device = None;
                }
            }
            EiEvent::SeatRemoved(_) => {
                self.absolute_devices.clear();
                self.absolute_generation = self.absolute_generation.wrapping_add(1);
                self.keyboard_device = None;
                self.pointer_device = None;
            }
            EiEvent::Disconnected(_) => {
                self.absolute_devices.clear();
                self.absolute_generation = self.absolute_generation.wrapping_add(1);
                self.keyboard_device = None;
                self.pointer_device = None;
                return Err(RuntimeFailure {
                    code: "EIS_DEVICE_UNAVAILABLE",
                    stage: "eis-disconnected",
                });
            }
            _ => {}
        }
        Ok(())
    }
}
