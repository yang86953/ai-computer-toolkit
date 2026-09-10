//! 在单线程 Portal lease 内持有并驱动 EIS 键盘与相对指针 sender。

use std::{
    net::Shutdown,
    os::unix::net::UnixStream,
    pin::Pin,
    sync::mpsc,
    task::{Context as TaskContext, Poll},
    thread,
    time::{Duration, Instant},
};

use futures_lite::{Stream, StreamExt, future};
use reis::{
    PendingRequestResult,
    ei::{self, button::ButtonState, keyboard::KeyState},
    event::{Device, DeviceCapability, EiEvent, EiEventConverter},
    handshake::HandshakeResp,
};
use rustix::time::{ClockId, clock_gettime};

use crate::{
    components::keyboard_input_contract::{
        KeyboardInput, KeyboardKey, KeyboardKeyPhase, KeyboardStep,
    },
    components::{
        desktop_session_input_cancellation::DesktopInputCancellation,
        desktop_session_input_liveness::DesktopSessionInputLiveness,
        desktop_session_pointer_input::{
            DesktopPointerDelta, DesktopPointerInput, DesktopPointerStep,
        },
        pointer_input_contract::{PointerButton, PointerButtonPhase, PointerScrollAxis},
    },
    modules::desktop_session::{
        DesktopKeyboardDispatchFacts, DesktopPointerDispatchFacts, DesktopSessionInputFailure,
    },
};

const BUTTON_LEFT: u32 = 272;
const BUTTON_RIGHT: u32 = 273;
const BUTTON_MIDDLE: u32 = 274;
const DISCRETE_SCROLL_UNIT: i32 = 120;
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 保存初始化阶段的稳定错误，不穿透 reis 错误文本。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopEisInitializationFailure {
    code: &'static str,
    stage: &'static str,
}

impl DesktopEisInitializationFailure {
    const fn new(code: &'static str, stage: &'static str) -> Self {
        Self { code, stage }
    }

    pub(crate) const fn code(self) -> &'static str {
        self.code
    }

    pub(crate) const fn stage(self) -> &'static str {
        self.stage
    }
}
enum BlockingHandshake {
    Ready(ei::Context, HandshakeResp),
    Failed,
}

struct DesktopEisEventStream {
    inner: reis::async_io::EiEventStream,
    converter: EiEventConverter,
}

impl Stream for DesktopEisEventStream {
    type Item = Result<EiEvent, reis::Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.as_mut().get_mut();
        if let Some(event) = this.converter.next_event() {
            return Poll::Ready(Some(Ok(event)));
        }
        while let Poll::Ready(result) = Pin::new(&mut this.inner).poll_next(context) {
            match result {
                Some(Ok(PendingRequestResult::Request(event))) => {
                    if let Err(error) = this.converter.handle_event(event) {
                        return Poll::Ready(Some(Err(error.into())));
                    }
                    if let Some(event) = this.converter.next_event() {
                        return Poll::Ready(Some(Ok(event)));
                    }
                }
                Some(Ok(PendingRequestResult::ParseError(error))) => {
                    return Poll::Ready(Some(Err(error.into())));
                }
                Some(Ok(PendingRequestResult::InvalidObject(_))) => {}
                Some(Err(error)) => return Poll::Ready(Some(Err(error.into()))),
                None => return Poll::Ready(None),
            }
        }
        Poll::Pending
    }
}

fn blocking_handshake(
    socket: UnixStream,
    timeout: Duration,
) -> Result<(ei::Context, HandshakeResp), DesktopEisInitializationFailure> {
    let interrupt = socket.try_clone().map_err(|_| {
        DesktopEisInitializationFailure::new("EIS_HANDSHAKE_FAILED", "eis-handshake")
    })?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("desktop-eis-handshake".to_owned())
        .spawn(move || {
            let result = ei::Context::new(socket)
                .map_err(|_| ())
                .and_then(|context| {
                    reis::handshake::ei_handshake_blocking(
                        &context,
                        "ai-computer-toolkit",
                        ei::handshake::ContextType::Sender,
                    )
                    .map(|response| BlockingHandshake::Ready(context, response))
                    .map_err(|_| ())
                })
                .unwrap_or(BlockingHandshake::Failed);
            let _ = sender.send(result);
        })
        .map_err(|_| {
            DesktopEisInitializationFailure::new("EIS_HANDSHAKE_FAILED", "eis-handshake")
        })?;
    let result = match receiver.recv_timeout(timeout) {
        Ok(BlockingHandshake::Ready(context, response)) => Ok((context, response)),
        Ok(BlockingHandshake::Failed) | Err(mpsc::RecvTimeoutError::Disconnected) => Err(
            DesktopEisInitializationFailure::new("EIS_HANDSHAKE_FAILED", "eis-handshake"),
        ),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = interrupt.shutdown(Shutdown::Both);
            Err(DesktopEisInitializationFailure::new(
                "TIMEOUT",
                "eis-handshake",
            ))
        }
    };
    let _ = worker.join();
    result
}

/// 固定收尾停机允许的宽限：设备已不再持有按键时，重试刷写不值得占满整段请求预算。
const RELEASE_GRACE: Duration = Duration::from_millis(1_000);

/// 同一 broker 线程唯一持有的 EIS 键盘、相对指针设备与事件流。
pub(crate) struct DesktopEisInput {
    connection: reis::event::Connection,
    events: DesktopEisEventStream,
    keyboard_device: Option<Device>,
    pointer_device: Option<Device>,
    absolute_devices: Vec<Device>,
    absolute_generation: u64,
    capture_mapping_id: Option<String>,
    sequence: u32,
    /// 绝对指针是否正处于本客户端的 emulation 会话中。
    ///
    /// 一次 emulation 会话覆盖整段点派发：逐点 start/stop 会让 compositor 在约 90 次
    /// 开关后断开 EIS 连接（实测 `stage=stop-emulating`、会话作废）。会话由
    /// `finish_frame_points` 在小批次结束时关闭。
    absolute_emulating: bool,
}

#[path = "desktop_input_eis_absolute.rs"]
mod absolute;
#[path = "desktop_input_eis_devices.rs"]
mod devices;

impl DesktopEisInput {
    pub(crate) async fn connect(
        socket: UnixStream,
        timeout: Duration,
    ) -> Result<Self, DesktopEisInitializationFailure> {
        let deadline = Instant::now() + timeout;
        let (context, response) = blocking_handshake(socket, timeout)?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|value| !value.is_zero())
            .ok_or_else(|| DesktopEisInitializationFailure::new("TIMEOUT", "eis-input-ready"))?;
        let events = reis::async_io::EiEventStream::new(context.clone()).map_err(|_| {
            DesktopEisInitializationFailure::new("EIS_HANDSHAKE_FAILED", "eis-handshake")
        })?;
        let converter = EiEventConverter::new(&context, response);
        let connection = converter.connection().clone();
        Self::initialize(
            connection,
            DesktopEisEventStream {
                inner: events,
                converter,
            },
            remaining,
        )
        .await
    }

    /// 绑定键盘与相对指针能力，并等待两类 ready 后 resumed 的发送设备。
    async fn initialize(
        connection: reis::event::Connection,
        events: DesktopEisEventStream,
        timeout: Duration,
    ) -> Result<Self, DesktopEisInitializationFailure> {
        let mut input = Self {
            connection,
            events,
            keyboard_device: None,
            pointer_device: None,
            absolute_devices: Vec::new(),
            absolute_generation: 1,
            capture_mapping_id: None,
            sequence: 1,
            absolute_emulating: false,
        };
        let deadline = Instant::now() + timeout;
        while input.keyboard_device.is_none() || input.pointer_device.is_none() {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|value| !value.is_zero())
                .ok_or_else(|| {
                    DesktopEisInitializationFailure::new("TIMEOUT", "eis-input-ready")
                })?;
            enum InitializationWait {
                Event(Option<Result<EiEvent, reis::Error>>),
                Timeout,
            }
            let next = future::race(
                async { InitializationWait::Event(input.events.next().await) },
                async {
                    async_io::Timer::after(remaining).await;
                    InitializationWait::Timeout
                },
            )
            .await;
            let next = match next {
                InitializationWait::Event(Some(Ok(event))) => event,
                InitializationWait::Event(_) => {
                    return Err(DesktopEisInitializationFailure::new(
                        "EIS_DEVICE_UNAVAILABLE",
                        "eis-input-ready",
                    ));
                }
                InitializationWait::Timeout => {
                    return Err(DesktopEisInitializationFailure::new(
                        "TIMEOUT",
                        "eis-input-ready",
                    ));
                }
            };
            input.apply_event(next).map_err(|failure| {
                DesktopEisInitializationFailure::new(failure.code, failure.stage)
            })?;
        }
        Ok(input)
    }

    /// 发送已完整验证的请求内配平序列。
    pub(crate) fn send_keyboard(
        &mut self,
        input: &KeyboardInput,
        cancellation: &DesktopInputCancellation,
        liveness: &mut dyn DesktopSessionInputLiveness,
    ) -> Result<DesktopKeyboardDispatchFacts, DesktopSessionInputFailure> {
        let mut guard = InputExecutionGuard::new(cancellation, liveness);
        self.refresh_events(&mut guard)
            .map_err(before_dispatch_failure)?;
        let device = self.keyboard_device.clone().ok_or_else(|| {
            DesktopSessionInputFailure::before_dispatch(
                "EIS_DEVICE_UNAVAILABLE",
                "keyboard-preflight",
            )
        })?;
        let keyboard = device.interface::<ei::Keyboard>().ok_or_else(|| {
            DesktopSessionInputFailure::before_dispatch(
                "EIS_DEVICE_UNAVAILABLE",
                "keyboard-preflight",
            )
        })?;
        if !device.device().is_alive() || !keyboard.is_alive() {
            return Err(DesktopSessionInputFailure::before_dispatch(
                "EIS_DEVICE_UNAVAILABLE",
                "keyboard-preflight",
            ));
        }
        validate_key_mapping(input)?;
        let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms));
        let serial = self.connection.serial();
        device.device().start_emulating(serial, self.sequence);
        self.advance_sequence();
        if self.flush_bounded(deadline).is_err() {
            return Err(DesktopSessionInputFailure::after_dispatch(
                "OUTCOME_UNKNOWN",
                "start-emulating",
                false,
                0,
                0,
            ));
        }

        let mut progress = DispatchProgress::default();
        let dispatched = self.dispatch_steps(
            input,
            &device,
            &keyboard,
            &mut guard,
            deadline,
            &mut progress,
        );
        if let Err(failure) =
            dispatched.and_then(|()| guard.check_with_deadline(deadline, "keyboard-dispatch"))
        {
            let releases_confirmed = self.best_effort_stop(&device, &keyboard, &progress.held);
            if failure.code == "CANCELLED" {
                return Err(DesktopSessionInputFailure::cancelled(
                    releases_confirmed,
                    progress.completed_steps,
                    progress.sent,
                ));
            }
            return Err(DesktopSessionInputFailure::after_dispatch(
                failure.code,
                failure.stage,
                releases_confirmed,
                progress.completed_steps,
                progress.sent,
            ));
        }
        device.device().stop_emulating(self.connection.serial());
        if self.flush_bounded(deadline).is_err() {
            return Err(DesktopSessionInputFailure::after_dispatch(
                "OUTCOME_UNKNOWN",
                "stop-emulating",
                true,
                progress.completed_steps,
                progress.sent,
            ));
        }
        if let Err(failure) = guard.check_with_deadline(deadline, "stop-emulating") {
            if failure.code == "CANCELLED" {
                return Err(DesktopSessionInputFailure::cancelled(
                    true,
                    progress.completed_steps,
                    progress.sent,
                ));
            }
            return Err(DesktopSessionInputFailure::after_dispatch(
                failure.code,
                failure.stage,
                true,
                progress.completed_steps,
                progress.sent,
            ));
        }
        Ok(DesktopKeyboardDispatchFacts::new(
            progress.completed_steps,
            progress.sent,
        ))
    }

    /// 发送已完整验证的请求内配平相对指针序列。
    pub(crate) fn send_pointer(
        &mut self,
        input: &DesktopPointerInput,
        cancellation: &DesktopInputCancellation,
        liveness: &mut dyn DesktopSessionInputLiveness,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
        let mut guard = InputExecutionGuard::new(cancellation, liveness);
        self.refresh_events(&mut guard)
            .map_err(before_dispatch_failure)?;
        let device = self.pointer_device.clone().ok_or_else(|| {
            DesktopSessionInputFailure::before_dispatch(
                "EIS_DEVICE_UNAVAILABLE",
                "pointer-preflight",
            )
        })?;
        let pointer = device.interface::<ei::Pointer>().ok_or_else(|| {
            DesktopSessionInputFailure::before_dispatch(
                "EIS_DEVICE_UNAVAILABLE",
                "pointer-preflight",
            )
        })?;
        let button = device.interface::<ei::Button>().ok_or_else(|| {
            DesktopSessionInputFailure::before_dispatch(
                "EIS_DEVICE_UNAVAILABLE",
                "pointer-preflight",
            )
        })?;
        let scroll = device.interface::<ei::Scroll>().ok_or_else(|| {
            DesktopSessionInputFailure::before_dispatch(
                "EIS_DEVICE_UNAVAILABLE",
                "pointer-preflight",
            )
        })?;
        if !device.device().is_alive()
            || !pointer.is_alive()
            || !button.is_alive()
            || !scroll.is_alive()
        {
            return Err(DesktopSessionInputFailure::before_dispatch(
                "EIS_DEVICE_UNAVAILABLE",
                "pointer-preflight",
            ));
        }
        let deadline = Instant::now() + Duration::from_millis(u64::from(input.timeout_ms));
        let serial = self.connection.serial();
        device.device().start_emulating(serial, self.sequence);
        self.advance_sequence();
        if self.flush_bounded(deadline).is_err() {
            return Err(DesktopSessionInputFailure::after_dispatch(
                "OUTCOME_UNKNOWN",
                "start-emulating",
                false,
                0,
                0,
            ));
        }

        let mut progress = DispatchProgress::default();
        let dispatched = self.dispatch_pointer_steps(
            input,
            &device,
            (&pointer, &button, &scroll),
            &mut guard,
            deadline,
            &mut progress,
        );
        if let Err(failure) =
            dispatched.and_then(|()| guard.check_with_deadline(deadline, "pointer-dispatch"))
        {
            let releases_confirmed =
                self.best_effort_pointer_stop(&device, &button, &progress.held);
            if failure.code == "CANCELLED" {
                return Err(DesktopSessionInputFailure::cancelled(
                    releases_confirmed,
                    progress.completed_steps,
                    progress.sent,
                ));
            }
            return Err(DesktopSessionInputFailure::after_dispatch(
                failure.code,
                failure.stage,
                releases_confirmed,
                progress.completed_steps,
                progress.sent,
            ));
        }
        device.device().stop_emulating(self.connection.serial());
        if self.flush_bounded(deadline).is_err() {
            return Err(DesktopSessionInputFailure::after_dispatch(
                "OUTCOME_UNKNOWN",
                "stop-emulating",
                true,
                progress.completed_steps,
                progress.sent,
            ));
        }
        if let Err(failure) = guard.check_with_deadline(deadline, "stop-emulating") {
            if failure.code == "CANCELLED" {
                return Err(DesktopSessionInputFailure::cancelled(
                    true,
                    progress.completed_steps,
                    progress.sent,
                ));
            }
            return Err(DesktopSessionInputFailure::after_dispatch(
                failure.code,
                failure.stage,
                true,
                progress.completed_steps,
                progress.sent,
            ));
        }
        Ok(DesktopPointerDispatchFacts::new(
            progress.completed_steps,
            progress.sent,
        ))
    }

    fn dispatch_pointer_steps(
        &mut self,
        input: &DesktopPointerInput,
        device: &Device,
        interfaces: (&ei::Pointer, &ei::Button, &ei::Scroll),
        guard: &mut InputExecutionGuard<'_>,
        deadline: Instant,
        progress: &mut DispatchProgress,
    ) -> Result<(), RuntimeFailure> {
        let (pointer, button, scroll) = interfaces;
        for step in &input.steps {
            match step {
                DesktopPointerStep::Move { delta } => {
                    self.emit_pointer_motion(device, pointer, *delta, guard, deadline)?;
                    progress.sent += 1;
                }
                DesktopPointerStep::Button {
                    button: public_button,
                    phase,
                } => {
                    let code = pointer_button_code(*public_button);
                    let state = match phase {
                        PointerButtonPhase::Down => ButtonState::Press,
                        PointerButtonPhase::Up => ButtonState::Released,
                    };
                    self.emit_pointer_button(device, button, code, state, guard, deadline)?;
                    match phase {
                        PointerButtonPhase::Down => progress.held.push(code),
                        PointerButtonPhase::Up => {
                            progress.held.retain(|held_code| *held_code != code)
                        }
                    }
                    progress.sent += 1;
                }
                DesktopPointerStep::Click {
                    button: public_button,
                    count,
                    interval_ms,
                } => {
                    let code = pointer_button_code(*public_button);
                    for index in 0..*count {
                        self.emit_pointer_button(
                            device,
                            button,
                            code,
                            ButtonState::Press,
                            guard,
                            deadline,
                        )?;
                        progress.held.push(code);
                        progress.sent += 1;
                        self.emit_pointer_button(
                            device,
                            button,
                            code,
                            ButtonState::Released,
                            guard,
                            deadline,
                        )?;
                        progress.held.pop();
                        progress.sent += 1;
                        if index + 1 < *count {
                            self.wait_pointer(
                                device,
                                Duration::from_millis(u64::from(*interval_ms)),
                                guard,
                                deadline,
                            )?;
                        }
                    }
                }
                DesktopPointerStep::Scroll { axis, ticks } => {
                    let units = ticks.saturating_mul(DISCRETE_SCROLL_UNIT);
                    let (x, y) = match axis {
                        PointerScrollAxis::Horizontal => (units, 0),
                        PointerScrollAxis::Vertical => (0, units),
                    };
                    self.emit_pointer_scroll(device, scroll, x, y, guard, deadline)?;
                    progress.sent += 1;
                }
                DesktopPointerStep::Drag {
                    button: public_button,
                    delta,
                    duration_ms,
                    samples,
                } => {
                    let code = pointer_button_code(*public_button);
                    self.emit_pointer_button(
                        device,
                        button,
                        code,
                        ButtonState::Press,
                        guard,
                        deadline,
                    )?;
                    progress.held.push(code);
                    progress.sent += 1;
                    let interval = Duration::from_nanos(
                        u64::from(*duration_ms)
                            .saturating_mul(1_000_000)
                            .checked_div(u64::from(*samples))
                            .unwrap_or_default(),
                    );
                    for motion in relative_motion_samples(*delta, *samples) {
                        self.wait_pointer(device, interval, guard, deadline)?;
                        if motion.x != 0 || motion.y != 0 {
                            self.emit_pointer_motion(device, pointer, motion, guard, deadline)?;
                            progress.sent += 1;
                        }
                    }
                    self.emit_pointer_button(
                        device,
                        button,
                        code,
                        ButtonState::Released,
                        guard,
                        deadline,
                    )?;
                    progress.held.pop();
                    progress.sent += 1;
                }
            }
            progress.completed_steps += 1;
        }
        Ok(())
    }

    fn dispatch_steps(
        &mut self,
        input: &KeyboardInput,
        device: &Device,
        keyboard: &ei::Keyboard,
        guard: &mut InputExecutionGuard<'_>,
        deadline: Instant,
        progress: &mut DispatchProgress,
    ) -> Result<(), RuntimeFailure> {
        for step in &input.steps {
            match step {
                KeyboardStep::Key {
                    key,
                    phase,
                    hold_ms,
                    repeat,
                    interval_ms,
                } => {
                    let code = key_code(key).ok_or_else(mapping_failure)?;
                    match phase {
                        KeyboardKeyPhase::Down => {
                            self.emit(
                                device,
                                keyboard,
                                &[(code, KeyState::Press)],
                                guard,
                                deadline,
                            )?;
                            progress.held.push(code);
                            progress.sent += 1;
                        }
                        KeyboardKeyPhase::Up => {
                            self.emit(
                                device,
                                keyboard,
                                &[(code, KeyState::Released)],
                                guard,
                                deadline,
                            )?;
                            progress.held.retain(|held_code| *held_code != code);
                            progress.sent += 1;
                        }
                        KeyboardKeyPhase::Press => {
                            for index in 0..*repeat {
                                self.emit(
                                    device,
                                    keyboard,
                                    &[(code, KeyState::Press)],
                                    guard,
                                    deadline,
                                )?;
                                progress.held.push(code);
                                progress.sent += 1;
                                self.wait(
                                    device,
                                    Duration::from_millis(u64::from(*hold_ms)),
                                    guard,
                                    deadline,
                                )?;
                                self.emit(
                                    device,
                                    keyboard,
                                    &[(code, KeyState::Released)],
                                    guard,
                                    deadline,
                                )?;
                                progress.held.pop();
                                progress.sent += 1;
                                if index + 1 < *repeat {
                                    self.wait(
                                        device,
                                        Duration::from_millis(u64::from(*interval_ms)),
                                        guard,
                                        deadline,
                                    )?;
                                }
                            }
                        }
                    }
                }
                KeyboardStep::Chord {
                    keys,
                    hold_ms,
                    repeat,
                    interval_ms,
                } => {
                    let codes = keys
                        .iter()
                        .map(|key| key_code(key).ok_or_else(mapping_failure))
                        .collect::<Result<Vec<_>, _>>()?;
                    for index in 0..*repeat {
                        let presses = codes
                            .iter()
                            .map(|code| (*code, KeyState::Press))
                            .collect::<Vec<_>>();
                        self.emit(device, keyboard, &presses, guard, deadline)?;
                        progress.held.extend(codes.iter().copied());
                        progress.sent += codes.len();
                        self.wait(
                            device,
                            Duration::from_millis(u64::from(*hold_ms)),
                            guard,
                            deadline,
                        )?;
                        let releases = codes
                            .iter()
                            .rev()
                            .map(|code| (*code, KeyState::Released))
                            .collect::<Vec<_>>();
                        self.emit(device, keyboard, &releases, guard, deadline)?;
                        progress
                            .held
                            .truncate(progress.held.len().saturating_sub(codes.len()));
                        progress.sent += codes.len();
                        if index + 1 < *repeat {
                            self.wait(
                                device,
                                Duration::from_millis(u64::from(*interval_ms)),
                                guard,
                                deadline,
                            )?;
                        }
                    }
                }
                KeyboardStep::Text { .. } => return Err(mapping_failure()),
            }
            progress.completed_steps += 1;
        }
        Ok(())
    }

    fn emit(
        &mut self,
        device: &Device,
        keyboard: &ei::Keyboard,
        events: &[(u32, KeyState)],
        guard: &mut InputExecutionGuard<'_>,
        deadline: Instant,
    ) -> Result<(), RuntimeFailure> {
        self.ensure_live(device, guard, deadline, "keyboard-dispatch")?;
        for (code, state) in events {
            keyboard.key(*code, *state);
        }
        device
            .device()
            .frame(self.connection.serial(), monotonic_microseconds());
        self.flush_bounded(deadline).map_err(|_| RuntimeFailure {
            code: "OUTCOME_UNKNOWN",
            stage: "keyboard-dispatch",
        })
    }

    fn emit_pointer_motion(
        &mut self,
        device: &Device,
        pointer: &ei::Pointer,
        delta: DesktopPointerDelta,
        guard: &mut InputExecutionGuard<'_>,
        deadline: Instant,
    ) -> Result<(), RuntimeFailure> {
        self.ensure_pointer_live(device, guard, deadline, "pointer-motion")?;
        pointer.motion_relative(delta.x as f32, delta.y as f32);
        self.flush_pointer_frame(device, "pointer-motion", deadline)
    }

    fn emit_pointer_button(
        &mut self,
        device: &Device,
        button: &ei::Button,
        code: u32,
        state: ButtonState,
        guard: &mut InputExecutionGuard<'_>,
        deadline: Instant,
    ) -> Result<(), RuntimeFailure> {
        self.ensure_pointer_live(device, guard, deadline, "pointer-button")?;
        button.button(code, state);
        self.flush_pointer_frame(device, "pointer-button", deadline)
    }

    fn emit_pointer_scroll(
        &mut self,
        device: &Device,
        scroll: &ei::Scroll,
        x: i32,
        y: i32,
        guard: &mut InputExecutionGuard<'_>,
        deadline: Instant,
    ) -> Result<(), RuntimeFailure> {
        self.ensure_pointer_live(device, guard, deadline, "pointer-scroll")?;
        scroll.scroll_discrete(x, y);
        self.flush_pointer_frame(device, "pointer-scroll", deadline)
    }

    fn flush_pointer_frame(
        &self,
        device: &Device,
        stage: &'static str,
        deadline: Instant,
    ) -> Result<(), RuntimeFailure> {
        device
            .device()
            .frame(self.connection.serial(), monotonic_microseconds());
        self.flush_bounded(deadline).map_err(|_| RuntimeFailure {
            code: "OUTCOME_UNKNOWN",
            stage,
        })
    }

    fn advance_sequence(&mut self) {
        self.sequence = self.sequence.wrapping_add(1);
        if self.sequence == 0 {
            self.sequence = 1;
        }
    }

    /// 刷写连接；对端一时读不过来（`EAGAIN`）时在 deadline 内重试。
    ///
    /// reis 的 `flush` 会把写缓冲里的每条消息推给 socket，socket 暂满就**直接返回
    /// `EAGAIN`**。长批次里这是正常背压：等对端读走再写即可。此前把任何写错误都当成
    /// `OUTCOME_UNKNOWN`，于是单请求写到第 278 次刷写就"连接断开"并作废会话——实测
    /// 649 步批次死在 `stage=absolute-motion`、已完成 277 步。只有超出本请求 deadline
    /// 或真正的写错误才算失败。
    fn flush_bounded(&self, deadline: Instant) -> Result<(), rustix::io::Errno> {
        loop {
            match self.connection.flush() {
                Ok(()) => return Ok(()),
                Err(rustix::io::Errno::AGAIN) => {
                    if Instant::now() >= deadline {
                        return Err(rustix::io::Errno::AGAIN);
                    }
                    // 让出 CPU 给对端读走数据，避免纯自旋。
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn wait(
        &mut self,
        device: &Device,
        duration: Duration,
        guard: &mut InputExecutionGuard<'_>,
        deadline: Instant,
    ) -> Result<(), RuntimeFailure> {
        if duration.is_zero() {
            return self.ensure_live(device, guard, deadline, "keyboard-wait");
        }
        let wait_until = Instant::now() + duration;
        loop {
            guard.check_with_deadline(deadline, "keyboard-wait")?;
            let target = wait_until.min(deadline);
            let remaining = target
                .checked_duration_since(Instant::now())
                .filter(|value| !value.is_zero());
            let Some(remaining) = remaining else {
                if Instant::now() >= deadline {
                    return Err(RuntimeFailure {
                        code: "TIMEOUT",
                        stage: "keyboard-wait",
                    });
                }
                return Ok(());
            };
            enum WaitResult {
                Event(Option<Result<EiEvent, reis::Error>>),
                Timer,
            }
            let result = async_io::block_on(future::race(
                async { WaitResult::Event(self.events.next().await) },
                async {
                    async_io::Timer::after(remaining.min(CANCELLATION_POLL_INTERVAL)).await;
                    WaitResult::Timer
                },
            ));
            match result {
                WaitResult::Event(Some(Ok(event))) => self.apply_event(event)?,
                WaitResult::Event(_) => {
                    return Err(RuntimeFailure {
                        code: "EIS_DEVICE_UNAVAILABLE",
                        stage: "keyboard-wait",
                    });
                }
                WaitResult::Timer if Instant::now() >= deadline => {
                    return Err(RuntimeFailure {
                        code: "TIMEOUT",
                        stage: "keyboard-wait",
                    });
                }
                WaitResult::Timer if Instant::now() >= wait_until => return Ok(()),
                WaitResult::Timer => continue,
            }
            if self.keyboard_device.as_ref() != Some(device) {
                return Err(RuntimeFailure {
                    code: "EIS_DEVICE_UNAVAILABLE",
                    stage: "keyboard-wait",
                });
            }
        }
    }

    fn wait_pointer(
        &mut self,
        device: &Device,
        duration: Duration,
        guard: &mut InputExecutionGuard<'_>,
        deadline: Instant,
    ) -> Result<(), RuntimeFailure> {
        if duration.is_zero() {
            return self.ensure_pointer_live(device, guard, deadline, "pointer-wait");
        }
        let wait_until = Instant::now() + duration;
        loop {
            guard.check_with_deadline(deadline, "pointer-wait")?;
            let target = wait_until.min(deadline);
            let remaining = target
                .checked_duration_since(Instant::now())
                .filter(|value| !value.is_zero());
            let Some(remaining) = remaining else {
                if Instant::now() >= deadline {
                    return Err(RuntimeFailure {
                        code: "TIMEOUT",
                        stage: "pointer-wait",
                    });
                }
                return Ok(());
            };
            enum WaitResult {
                Event(Option<Result<EiEvent, reis::Error>>),
                Timer,
            }
            let result = async_io::block_on(future::race(
                async { WaitResult::Event(self.events.next().await) },
                async {
                    async_io::Timer::after(remaining.min(CANCELLATION_POLL_INTERVAL)).await;
                    WaitResult::Timer
                },
            ));
            match result {
                WaitResult::Event(Some(Ok(event))) => self.apply_event(event)?,
                WaitResult::Event(_) => {
                    return Err(RuntimeFailure {
                        code: "EIS_DEVICE_UNAVAILABLE",
                        stage: "pointer-wait",
                    });
                }
                WaitResult::Timer if Instant::now() >= deadline => {
                    return Err(RuntimeFailure {
                        code: "TIMEOUT",
                        stage: "pointer-wait",
                    });
                }
                WaitResult::Timer if Instant::now() >= wait_until => return Ok(()),
                WaitResult::Timer => continue,
            }
            if self.pointer_device.as_ref() != Some(device) {
                return Err(RuntimeFailure {
                    code: "EIS_DEVICE_UNAVAILABLE",
                    stage: "pointer-wait",
                });
            }
        }
    }

    fn ensure_live(
        &mut self,
        device: &Device,
        guard: &mut InputExecutionGuard<'_>,
        deadline: Instant,
        stage: &'static str,
    ) -> Result<(), RuntimeFailure> {
        guard.check_with_deadline(deadline, stage)?;
        self.refresh_events(guard)?;
        if self.keyboard_device.as_ref() != Some(device) || !device.device().is_alive() {
            return Err(RuntimeFailure {
                code: "EIS_DEVICE_UNAVAILABLE",
                stage,
            });
        }
        Ok(())
    }

    fn ensure_pointer_live(
        &mut self,
        device: &Device,
        guard: &mut InputExecutionGuard<'_>,
        deadline: Instant,
        stage: &'static str,
    ) -> Result<(), RuntimeFailure> {
        guard.check_with_deadline(deadline, stage)?;
        self.refresh_events(guard)?;
        if self.pointer_device.as_ref() != Some(device) || !device.device().is_alive() {
            return Err(RuntimeFailure {
                code: "EIS_DEVICE_UNAVAILABLE",
                stage,
            });
        }
        Ok(())
    }

    fn refresh_events(
        &mut self,
        guard: &mut InputExecutionGuard<'_>,
    ) -> Result<(), RuntimeFailure> {
        loop {
            guard.check()?;
            match async_io::block_on(future::poll_once(self.events.next())) {
                None => return Ok(()),
                Some(Some(Ok(event))) => self.apply_event(event)?,
                Some(_) => {
                    return Err(RuntimeFailure {
                        code: "EIS_DEVICE_UNAVAILABLE",
                        stage: "eis-event-stream",
                    });
                }
            }
        }
    }

    fn best_effort_stop(&self, device: &Device, keyboard: &ei::Keyboard, held: &[u32]) -> bool {
        if self.keyboard_device.as_ref() != Some(device)
            || !device.device().is_alive()
            || !keyboard.is_alive()
        {
            // pause/remove/disconnect 会使 EIS 设备回到中立状态，禁止再向旧设备发送。
            return true;
        }
        for code in held.iter().rev() {
            keyboard.key(*code, KeyState::Released);
        }
        if !held.is_empty() {
            device
                .device()
                .frame(self.connection.serial(), monotonic_microseconds());
        }
        device.device().stop_emulating(self.connection.serial());
        self.flush_bounded(Instant::now() + RELEASE_GRACE).is_ok()
    }

    fn best_effort_pointer_stop(&self, device: &Device, button: &ei::Button, held: &[u32]) -> bool {
        if self.pointer_device.as_ref() != Some(device)
            || !device.device().is_alive()
            || !button.is_alive()
        {
            // pause/remove/disconnect 会使 EIS 设备回到中立状态，禁止再向旧设备发送。
            return true;
        }
        for code in held.iter().rev() {
            button.button(*code, ButtonState::Released);
        }
        if !held.is_empty() {
            device
                .device()
                .frame(self.connection.serial(), monotonic_microseconds());
        }
        device.device().stop_emulating(self.connection.serial());
        self.flush_bounded(Instant::now() + RELEASE_GRACE).is_ok()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeFailure {
    code: &'static str,
    stage: &'static str,
}

/// 聚合同一请求的工具持有状态与公开计数，避免调度函数参数漂移。
#[derive(Default)]
struct DispatchProgress {
    held: Vec<u32>,
    completed_steps: usize,
    sent: usize,
}

/// 固定取消优先级，并把平台存活失败收敛成 EIS 私有运行时失败。
struct InputExecutionGuard<'a> {
    cancellation: &'a DesktopInputCancellation,
    liveness: &'a mut dyn DesktopSessionInputLiveness,
}

impl<'a> InputExecutionGuard<'a> {
    fn new(
        cancellation: &'a DesktopInputCancellation,
        liveness: &'a mut dyn DesktopSessionInputLiveness,
    ) -> Self {
        Self {
            cancellation,
            liveness,
        }
    }

    fn check(&mut self) -> Result<(), RuntimeFailure> {
        cancellation_failure(self.cancellation)?;
        self.liveness
            .poll_input_allowed()
            .map_err(|failure| RuntimeFailure {
                code: failure.code(),
                stage: failure.stage(),
            })
    }

    fn check_with_deadline(
        &mut self,
        deadline: Instant,
        stage: &'static str,
    ) -> Result<(), RuntimeFailure> {
        cancellation_failure(self.cancellation)?;
        if Instant::now() >= deadline {
            return Err(RuntimeFailure {
                code: "TIMEOUT",
                stage,
            });
        }
        self.liveness
            .poll_input_allowed()
            .map_err(|failure| RuntimeFailure {
                code: failure.code(),
                stage: failure.stage(),
            })
    }
}

fn before_dispatch_failure(failure: RuntimeFailure) -> DesktopSessionInputFailure {
    if failure.code == "CANCELLED" {
        DesktopSessionInputFailure::cancelled(true, 0, 0)
    } else {
        DesktopSessionInputFailure::before_dispatch(failure.code, failure.stage)
    }
}

const fn mapping_failure() -> RuntimeFailure {
    RuntimeFailure {
        code: "INPUT_MAPPING_UNAVAILABLE",
        stage: "keyboard-preflight",
    }
}

fn cancellation_failure(cancellation: &DesktopInputCancellation) -> Result<(), RuntimeFailure> {
    if cancellation.is_cancelled() {
        Err(RuntimeFailure {
            code: "CANCELLED",
            stage: "input-cancel",
        })
    } else {
        Ok(())
    }
}

fn device_supports_pointer(device: &Device) -> bool {
    device.has_capability(DeviceCapability::Pointer)
        && device.has_capability(DeviceCapability::Button)
        && device.has_capability(DeviceCapability::Scroll)
}

const fn pointer_button_code(button: PointerButton) -> u32 {
    match button {
        PointerButton::Left => BUTTON_LEFT,
        PointerButton::Right => BUTTON_RIGHT,
        PointerButton::Middle => BUTTON_MIDDLE,
    }
}

fn relative_motion_samples(delta: DesktopPointerDelta, samples: u16) -> Vec<DesktopPointerDelta> {
    let mut result = Vec::with_capacity(usize::from(samples));
    let mut previous_x = 0_i32;
    let mut previous_y = 0_i32;
    for index in 1..=samples {
        let target_x = i64::from(delta.x) * i64::from(index) / i64::from(samples);
        let target_y = i64::from(delta.y) * i64::from(index) / i64::from(samples);
        let target_x = target_x as i32;
        let target_y = target_y as i32;
        let motion = DesktopPointerDelta {
            x: target_x - previous_x,
            y: target_y - previous_y,
        };
        result.push(motion);
        previous_x = target_x;
        previous_y = target_y;
    }
    result
}

fn validate_key_mapping(input: &KeyboardInput) -> Result<(), DesktopSessionInputFailure> {
    let mapped = input.steps.iter().all(|step| match step {
        KeyboardStep::Key { key, .. } => key_code(key).is_some(),
        KeyboardStep::Chord { keys, .. } => keys.iter().all(|key| key_code(key).is_some()),
        KeyboardStep::Text { .. } => false,
    });
    if mapped {
        Ok(())
    } else {
        Err(DesktopSessionInputFailure::before_dispatch(
            "INPUT_MAPPING_UNAVAILABLE",
            "keyboard-preflight",
        ))
    }
}

fn monotonic_microseconds() -> u64 {
    let time = clock_gettime(ClockId::Monotonic);
    u64::try_from(time.tv_sec)
        .unwrap_or_default()
        .saturating_mul(1_000_000)
        .saturating_add(u64::try_from(time.tv_nsec).unwrap_or_default() / 1_000)
}

fn key_code(key: &KeyboardKey) -> Option<u32> {
    Some(match key.as_str() {
        "a" => 30,
        "b" => 48,
        "c" => 46,
        "d" => 32,
        "e" => 18,
        "f" => 33,
        "g" => 34,
        "h" => 35,
        "i" => 23,
        "j" => 36,
        "k" => 37,
        "l" => 38,
        "m" => 50,
        "n" => 49,
        "o" => 24,
        "p" => 25,
        "q" => 16,
        "r" => 19,
        "s" => 31,
        "t" => 20,
        "u" => 22,
        "v" => 47,
        "w" => 17,
        "x" => 45,
        "y" => 21,
        "z" => 44,
        "1" => 2,
        "2" => 3,
        "3" => 4,
        "4" => 5,
        "5" => 6,
        "6" => 7,
        "7" => 8,
        "8" => 9,
        "9" => 10,
        "0" => 11,
        "f1" => 59,
        "f2" => 60,
        "f3" => 61,
        "f4" => 62,
        "f5" => 63,
        "f6" => 64,
        "f7" => 65,
        "f8" => 66,
        "f9" => 67,
        "f10" => 68,
        "f11" => 87,
        "f12" => 88,
        "f13" => 183,
        "f14" => 184,
        "f15" => 185,
        "f16" => 186,
        "f17" => 187,
        "f18" => 188,
        "f19" => 189,
        "f20" => 190,
        "f21" => 191,
        "f22" => 192,
        "f23" => 193,
        "f24" => 194,
        "numpad-0" => 82,
        "numpad-1" => 79,
        "numpad-2" => 80,
        "numpad-3" => 81,
        "numpad-4" => 75,
        "numpad-5" => 76,
        "numpad-6" => 77,
        "numpad-7" => 71,
        "numpad-8" => 72,
        "numpad-9" => 73,
        "enter" => 28,
        "tab" => 15,
        "escape" => 1,
        "backspace" => 14,
        "delete" => 111,
        "insert" => 110,
        "space" => 57,
        "left" => 105,
        "up" => 103,
        "right" => 106,
        "down" => 108,
        "home" => 102,
        "end" => 107,
        "page-up" => 104,
        "page-down" => 109,
        "left-control" => 29,
        "right-control" => 97,
        "left-alt" => 56,
        "right-alt" => 100,
        "left-shift" => 42,
        "right-shift" => 54,
        "left-win" => 125,
        "right-win" => 126,
        "numpad-add" => 78,
        "numpad-subtract" => 74,
        "numpad-multiply" => 55,
        "numpad-divide" => 98,
        "numpad-decimal" => 83,
        "numpad-enter" => 96,
        "minus" => 12,
        "equals" => 13,
        "left-bracket" => 26,
        "right-bracket" => 27,
        "backslash" => 43,
        "semicolon" => 39,
        "apostrophe" => 40,
        "grave" => 41,
        "comma" => 51,
        "period" => 52,
        "slash" => 53,
        "caps-lock" => 58,
        "num-lock" => 69,
        "scroll-lock" => 70,
        "print-screen" => 99,
        "pause" => 119,
        "menu" => 139,
        "volume-mute" => 113,
        "volume-down" => 114,
        "volume-up" => 115,
        "media-next" => 163,
        "media-previous" => 165,
        "media-stop" => 166,
        "media-play-pause" => 164,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::components::desktop_session_input_liveness::DesktopSessionInputLivenessFailure;
    use crate::components::keyboard_input_contract::{NAMED_KEY_NAMES, parse_keyboard_key};

    use super::*;

    #[test]
    fn blocking_handshake_respects_the_open_deadline() {
        let Ok((server, client)) = UnixStream::pair() else {
            panic!("socket pair unavailable");
        };
        let started = Instant::now();

        let failure = match blocking_handshake(client, Duration::from_millis(20)) {
            Ok(_) => panic!("silent EIS peer unexpectedly completed"),
            Err(failure) => failure,
        };

        assert_eq!(failure.code(), "TIMEOUT");
        assert_eq!(failure.stage(), "eis-handshake");
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(server);
    }

    #[test]
    fn every_public_named_key_has_a_linux_input_event_code() {
        for name in NAMED_KEY_NAMES {
            let key = parse_keyboard_key(name)
                .unwrap_or_else(|error| panic!("public key must parse: {name}: {error}"));
            assert!(key_code(&key).is_some(), "missing Linux mapping for {name}");
        }
        for name in ('a'..='z')
            .map(|value| value.to_string())
            .chain(('0'..='9').map(|value| value.to_string()))
            .chain((1..=24).map(|number| format!("f{number}")))
            .chain((0..=9).map(|number| format!("numpad-{number}")))
        {
            let key = parse_keyboard_key(&name)
                .unwrap_or_else(|error| panic!("public key must parse: {name}: {error}"));
            assert!(key_code(&key).is_some(), "missing Linux mapping for {name}");
        }
    }

    #[test]
    fn public_pointer_buttons_map_only_inside_the_adapter() {
        assert_eq!(pointer_button_code(PointerButton::Left), 272);
        assert_eq!(pointer_button_code(PointerButton::Right), 273);
        assert_eq!(pointer_button_code(PointerButton::Middle), 274);
    }

    #[test]
    fn drag_samples_preserve_the_exact_relative_total() {
        for (delta, samples) in [
            (DesktopPointerDelta { x: 40, y: 20 }, 8),
            (DesktopPointerDelta { x: -7, y: 3 }, 12),
            (DesktopPointerDelta { x: 1, y: -1 }, 240),
        ] {
            let motions = relative_motion_samples(delta, samples);
            assert_eq!(motions.len(), usize::from(samples));
            assert_eq!(motions.iter().map(|motion| motion.x).sum::<i32>(), delta.x);
            assert_eq!(motions.iter().map(|motion| motion.y).sum::<i32>(), delta.y);
        }
    }

    struct FakeLiveness {
        failure: Option<DesktopSessionInputLivenessFailure>,
        polls: usize,
    }

    impl DesktopSessionInputLiveness for FakeLiveness {
        fn poll_input_allowed(&mut self) -> Result<(), DesktopSessionInputLivenessFailure> {
            self.polls += 1;
            self.failure.map_or(Ok(()), Err)
        }
    }

    #[test]
    fn execution_guard_keeps_cancellation_ahead_of_host_invalidation() {
        let cancellation = DesktopInputCancellation::new();
        cancellation.cancel();
        let mut liveness = FakeLiveness {
            failure: Some(DesktopSessionInputLivenessFailure::new(
                "HOST_SESSION_LOCKED",
                "host-session-liveness",
            )),
            polls: 0,
        };
        let failure = match InputExecutionGuard::new(&cancellation, &mut liveness).check() {
            Ok(()) => panic!("cancelled guard must fail"),
            Err(failure) => failure,
        };
        assert_eq!(failure.code, "CANCELLED");
        assert_eq!(liveness.polls, 0);
    }

    #[test]
    fn execution_guard_preserves_neutral_liveness_code_and_stage() {
        let cancellation = DesktopInputCancellation::new();
        let mut liveness = FakeLiveness {
            failure: Some(DesktopSessionInputLivenessFailure::new(
                "PORTAL_SESSION_CLOSED",
                "portal-session-liveness",
            )),
            polls: 0,
        };
        let failure = match InputExecutionGuard::new(&cancellation, &mut liveness).check() {
            Ok(()) => panic!("closed Portal session must fail"),
            Err(failure) => failure,
        };
        assert_eq!(
            failure,
            RuntimeFailure {
                code: "PORTAL_SESSION_CLOSED",
                stage: "portal-session-liveness",
            }
        );
        assert_eq!(liveness.polls, 1);
    }
}
