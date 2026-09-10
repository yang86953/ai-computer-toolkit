//! Windows 桌面租约只做平台适配；会话、契约、输入执行及图像编码由共享实现拥有。
use crate::{
    adapters::{
        pointer_input_windows::PointerDpiGuard,
        security_context_windows::WindowsSecurityContextProbe,
        window_capture::capture_monitor_frame,
    },
    components::{
        desktop_interaction::{FrameMapping, FramePoint, normalized_point},
        desktop_session_frame_capture::{
            DesktopCapturedFrame, DesktopPackedPixelFormat, encode_mapped_frame,
        },
        desktop_session_input_cancellation::DesktopInputCancellation,
        desktop_session_pointer_input::{
            DesktopPointerDelta, DesktopPointerInput, DesktopPointerStep,
        },
        keyboard_input_contract::KeyboardInput,
        pointer_input_contract::{
            PointerButton, PointerCoordinateSpace, PointerInput, PointerPoint, PointerStep,
        },
    },
    domain::{AppControlError, AppResult},
    modules::{
        desktop_session::*,
        keyboard_input,
        permission_boundary::{self, TargetAccessKind},
        pointer_input,
    },
};
use image::{RgbaImage, imageops};
use serde_json::Value;
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};
use uix::platform::{Platform, windowing::desktop_cursor};
use windows::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint},
    UI::Input::KeyboardAndMouse::GetAsyncKeyState,
};

#[derive(Default)]
pub(crate) struct SystemDesktopSessionPort {
    platform: RefCell<Option<Rc<Platform>>>,
}
#[derive(Clone, Debug, PartialEq)]
struct Monitor {
    handle: isize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale: f32,
}
#[derive(Clone, Debug, PartialEq)]
struct Layout {
    monitors: Vec<Monitor>,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}
struct Lease {
    platform: Rc<Platform>,
    layout: Layout,
    generation: u64,
}

fn error(code: &'static str) -> AppControlError {
    AppControlError::new(
        code,
        "The Windows desktop platform could not complete this operation.",
    )
}
fn access(write: bool) -> AppResult<()> {
    permission_boundary::authorize(
        &WindowsSecurityContextProbe,
        if write {
            TargetAccessKind::Mutation
        } else {
            TargetAccessKind::Read
        },
    )
    .map(|_| ())
}
fn layout(platform: &Platform) -> AppResult<Layout> {
    let _dpi = PointerDpiGuard::enter()?;
    let displays = platform
        .displays()
        .map_err(|_| error("DISPLAY_UNAVAILABLE"))?;
    if displays.is_empty() || displays.len() > 32 {
        return Err(error("DISPLAY_UNAVAILABLE"));
    }
    let mut monitors = Vec::with_capacity(displays.len());
    for display in &displays {
        let r = display.bounds();
        let scale = display.scale();
        let values = [r.x, r.y, r.w, r.h].map(|v| (f64::from(v) * f64::from(scale)).round());
        if !scale.is_finite()
            || scale <= 0.0
            || values
                .iter()
                .any(|v| !v.is_finite() || *v < -1_000_000.0 || *v > 1_000_000.0)
            || !(1.0..=16384.0).contains(&values[2])
            || !(1.0..=16384.0).contains(&values[3])
        {
            return Err(error("DISPLAY_UNAVAILABLE"));
        }
        let (x, y, width, height) = (
            values[0] as i32,
            values[1] as i32,
            values[2] as u32,
            values[3] as u32,
        );
        let handle = unsafe {
            MonitorFromPoint(
                POINT {
                    x: x + width as i32 / 2,
                    y: y + height as i32 / 2,
                },
                MONITOR_DEFAULTTONULL,
            )
        };
        if handle.0.is_null() {
            return Err(error("DISPLAY_UNAVAILABLE"));
        }
        monitors.push(Monitor {
            handle: handle.0 as isize,
            x,
            y,
            width,
            height,
            scale,
        });
    }
    monitors.sort_by_key(|m| (m.x, m.y, m.handle));
    let x = monitors.iter().map(|m| m.x).min().unwrap();
    let y = monitors.iter().map(|m| m.y).min().unwrap();
    let width = monitors
        .iter()
        .map(|m| m.x + i32::try_from(m.width).unwrap())
        .max()
        .unwrap()
        - x;
    let height = monitors
        .iter()
        .map(|m| m.y + i32::try_from(m.height).unwrap())
        .max()
        .unwrap()
        - y;
    if !(1..=16384).contains(&width)
        || !(1..=16384).contains(&height)
        || i64::from(width) * i64::from(height) > 67_108_864
    {
        return Err(error("RESOURCE_EXHAUSTED"));
    }
    Ok(Layout {
        monitors,
        x,
        y,
        width: width as u32,
        height: height as u32,
    })
}
fn contains(m: &Monitor, point: PointerPoint) -> bool {
    point.x >= m.x
        && point.y >= m.y
        && i64::from(point.x) < i64::from(m.x) + i64::from(m.width)
        && i64::from(point.y) < i64::from(m.y) + i64::from(m.height)
}
fn input_failure(e: AppControlError) -> DesktopSessionInputFailure {
    let d = &e.details;
    let code = if e.code == "CANCELLED"
        || d.get("causeCode").and_then(Value::as_str) == Some("CANCELLED")
    {
        "CANCELLED"
    } else if d.get("outcome").and_then(Value::as_str) == Some("unknown") {
        "OUTCOME_UNKNOWN"
    } else {
        "INPUT_UNAVAILABLE"
    };
    if d.get("outcome").and_then(Value::as_str) == Some("unknown") {
        DesktopSessionInputFailure::after_dispatch(
            code,
            "windows-input",
            d.get("safeReleaseSucceeded")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            d.get("completedStepCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize,
            events(d),
        )
    } else {
        DesktopSessionInputFailure::before_dispatch(code, "windows-input")
    }
}
fn events(value: &Value) -> usize {
    value
        .get("inputEventsSent")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize
}
fn frame_failure(_: AppControlError) -> DesktopSessionFrameFailure {
    DesktopSessionFrameFailure::new("CAPTURE_UNAVAILABLE", "windows-wgc", true, true)
}
impl DesktopSessionPort for SystemDesktopSessionPort {
    fn open(
        &self,
        _: Duration,
    ) -> Result<(Box<dyn DesktopSessionLease>, DesktopSessionFacts), DesktopSessionPortFailure>
    {
        let fail = |_: AppControlError| {
            DesktopSessionPortFailure::before_session("DESKTOP_SESSION_UNAVAILABLE", "windows-open")
        };
        access(false).map_err(fail)?;
        let platform = if let Some(p) = self.platform.borrow().as_ref() {
            p.clone()
        } else {
            let p = Rc::new(Platform::new().map_err(|_| {
                DesktopSessionPortFailure::before_session("PLATFORM_UNAVAILABLE", "uix-platform")
            })?);
            // 借用在分支后写回，避免 RefCell 重入。
            p
        };
        *self.platform.borrow_mut() = Some(platform.clone());
        let layout = layout(&platform).map_err(fail)?;
        let facts = DesktopSessionFacts::windows(layout.monitors.len());
        Ok((
            Box::new(Lease {
                platform,
                layout,
                generation: 1,
            }),
            facts,
        ))
    }
}
impl Lease {
    fn check(&self, point: Option<PointerPoint>) -> AppResult<()> {
        access(true)?;
        if layout(&self.platform)? != self.layout {
            return Err(error("STALE_FRAME"));
        }
        if point.is_some_and(|p| !self.layout.monitors.iter().any(|m| contains(m, p))) {
            return Err(error("INPUT_MAPPING_UNAVAILABLE"));
        }
        Ok(())
    }
    fn ready(&self) -> AppResult<()> {
        self.check(None)?;
        if (1..=254).any(|key| unsafe { GetAsyncKeyState(key) } < 0) {
            return Err(error("INPUT_BUSY"));
        }
        Ok(())
    }
    fn pointer(
        &self,
        steps: Vec<PointerStep>,
        timeout_ms: u32,
        cancellation: &DesktopInputCancellation,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
        self.ready().map_err(input_failure)?;
        let input = PointerInput {
            coordinate_space: PointerCoordinateSpace::ScreenPhysicalPx,
            steps,
            timeout_ms,
            legacy_click_compatibility: false,
        };
        let result = pointer_input::perform_desktop(&input, cancellation, &|p| self.check(p))
            .map_err(input_failure)?;
        Ok(DesktopPointerDispatchFacts::new(
            input.steps.len(),
            events(&result),
        ))
    }
    fn mapping(&self) -> FrameMapping {
        FrameMapping {
            generation: self.generation,
            width: self.layout.width,
            height: self.layout.height,
        }
    }
    fn relative(&self, point: PointerPoint, delta: DesktopPointerDelta) -> AppResult<PointerPoint> {
        let scale = self
            .layout
            .monitors
            .iter()
            .find(|m| contains(m, point))
            .ok_or_else(|| error("INPUT_MAPPING_UNAVAILABLE"))?
            .scale;
        let x = i64::from(point.x) + (f64::from(delta.x) * f64::from(scale)).round() as i64;
        let y = i64::from(point.y) + (f64::from(delta.y) * f64::from(scale)).round() as i64;
        let point = PointerPoint {
            x: i32::try_from(x).map_err(|_| error("INVALID_ARGUMENT"))?,
            y: i32::try_from(y).map_err(|_| error("INVALID_ARGUMENT"))?,
        };
        self.check(Some(point))?;
        Ok(point)
    }
}
impl DesktopSessionLease for Lease {
    fn frame_mapping(&mut self) -> Result<Option<FrameMapping>, DesktopSessionInputFailure> {
        self.check(None).map_err(input_failure)?;
        Ok(Some(self.mapping()))
    }
    fn send_frame_point(
        &mut self,
        p: &FramePoint,
        timeout_ms: u32,
        c: &DesktopInputCancellation,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
        if p.mapping != self.mapping() {
            return Err(DesktopSessionInputFailure::before_dispatch(
                "STALE_FRAME",
                "frame-point",
            ));
        }
        let (x, y) = normalized_point(p).map_err(input_failure)?;
        let point = PointerPoint {
            x: self.layout.x + (x * f64::from(self.layout.width)).floor() as i32,
            y: self.layout.y + (y * f64::from(self.layout.height)).floor() as i32,
        };
        let step = if let Some(button) = p.button {
            PointerStep::Click {
                point,
                button: match button {
                    1 => PointerButton::Left,
                    2 => PointerButton::Right,
                    3 => PointerButton::Middle,
                    _ => {
                        return Err(DesktopSessionInputFailure::before_dispatch(
                            "INVALID_ARGUMENT",
                            "frame-point",
                        ));
                    }
                },
                count: 1,
                interval_ms: 0,
            }
        } else {
            PointerStep::Move { point }
        };
        self.pointer(vec![step], timeout_ms, c)
    }
    fn send_keyboard(
        &mut self,
        input: &KeyboardInput,
        c: &DesktopInputCancellation,
    ) -> Result<DesktopKeyboardDispatchFacts, DesktopSessionInputFailure> {
        self.ready().map_err(input_failure)?;
        let result = keyboard_input::perform_desktop(input, c, &|| self.check(None))
            .map_err(input_failure)?;
        Ok(DesktopKeyboardDispatchFacts::new(
            input.steps.len(),
            events(&result),
        ))
    }
    fn send_pointer(
        &mut self,
        input: &DesktopPointerInput,
        c: &DesktopInputCancellation,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
        let _dpi = PointerDpiGuard::enter().map_err(input_failure)?;
        self.ready().map_err(input_failure)?;
        let current = desktop_cursor::cursor_position()
            .map_err(|_| input_failure(error("POINTER_UNAVAILABLE")))?;
        let mut point = PointerPoint {
            x: current.x as i32,
            y: current.y as i32,
        };
        let mut steps = Vec::with_capacity(input.steps.len());
        for step in &input.steps {
            steps.push(match *step {
                DesktopPointerStep::Move { delta } => {
                    point = self.relative(point, delta).map_err(input_failure)?;
                    PointerStep::Move { point }
                }
                DesktopPointerStep::Button { button, phase } => PointerStep::Button {
                    button,
                    phase,
                    point,
                },
                DesktopPointerStep::Click {
                    button,
                    count,
                    interval_ms,
                } => PointerStep::Click {
                    button,
                    count,
                    interval_ms,
                    point,
                },
                DesktopPointerStep::Scroll { axis, ticks } => {
                    PointerStep::Scroll { axis, ticks, point }
                }
                DesktopPointerStep::Drag {
                    button,
                    delta,
                    duration_ms,
                    samples,
                } => {
                    let start = point;
                    point = self.relative(point, delta).map_err(input_failure)?;
                    PointerStep::Drag {
                        button,
                        start,
                        end: point,
                        duration_ms,
                        samples,
                    }
                }
            });
        }
        self.pointer(steps, input.timeout_ms, c)
    }
    fn capture_frame(
        &mut self,
        timeout: Duration,
        max_dimension: Option<u32>,
    ) -> Result<DesktopCapturedFrame, DesktopSessionFrameFailure> {
        access(false).map_err(frame_failure)?;
        let current = layout(&self.platform).map_err(frame_failure)?;
        if current != self.layout {
            self.generation = self
                .generation
                .checked_add(1)
                .ok_or_else(|| frame_failure(error("RESOURCE_EXHAUSTED")))?;
            self.layout = current;
        }
        let l = &self.layout;
        let mut canvas = RgbaImage::new(l.width, l.height);
        let deadline = Instant::now() + timeout;
        for monitor in &l.monitors {
            access(false).map_err(frame_failure)?;
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| frame_failure(error("TIMEOUT")))?;
            let frame = capture_monitor_frame(monitor.handle, remaining).map_err(frame_failure)?;
            if frame.width != monitor.width || frame.height != monitor.height {
                return Err(frame_failure(error("STALE_FRAME")));
            }
            let rgba = RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
                .ok_or_else(|| frame_failure(error("INVALID_FRAME")))?;
            imageops::replace(
                &mut canvas,
                &rgba,
                i64::from(monitor.x) - i64::from(l.x),
                i64::from(monitor.y) - i64::from(l.y),
            );
        }
        if layout(&self.platform).map_err(frame_failure)? != self.layout {
            return Err(frame_failure(error("STALE_FRAME")));
        }
        access(false).map_err(frame_failure)?;
        encode_mapped_frame(
            canvas.as_raw(),
            0,
            canvas.as_raw().len() as u32,
            (l.width * 4) as i32,
            (l.width, l.height),
            DesktopPackedPixelFormat::Rgba,
            max_dimension,
        )
        .map_err(|_| frame_failure(error("INVALID_FRAME")))
    }
    fn close(self: Box<Self>) -> Result<(), DesktopSessionPortFailure> {
        Ok(())
    }
}
