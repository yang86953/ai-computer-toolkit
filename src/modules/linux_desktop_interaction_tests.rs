//! 验证真正执行的交互入口在预检、观察身份、部分失败和取消上的行为。
use super::*;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Trace {
    calls: usize,
    fail_at: Option<usize>,
    closes: usize,
    points: Vec<FramePoint>,
}
struct Port(Arc<Mutex<Trace>>);
struct Lease(Arc<Mutex<Trace>>);
impl DesktopSessionPort for Port {
    fn open(
        &self,
        _: Duration,
    ) -> Result<(Box<dyn DesktopSessionLease>, DesktopSessionFacts), DesktopSessionPortFailure>
    {
        Ok((
            Box::new(Lease(Arc::clone(&self.0))),
            DesktopSessionFacts::new(2, 5, vec!["keyboard", "pointer"], 1, 1),
        ))
    }
}
impl DesktopSessionLease for Lease {
    fn send_keyboard(
        &mut self,
        input: &crate::components::keyboard_input_contract::KeyboardInput,
        _: &DesktopInputCancellation,
    ) -> Result<DesktopKeyboardDispatchFacts, DesktopSessionInputFailure> {
        let mut t = self.0.lock().unwrap();
        t.calls += 1;
        if t.fail_at == Some(t.calls) {
            return Err(DesktopSessionInputFailure::before_dispatch(
                "EIS_DEVICE_UNAVAILABLE",
                "fixture",
            ));
        }
        Ok(DesktopKeyboardDispatchFacts::new(
            input.steps.len(),
            input.steps.len() * 2,
        ))
    }
    fn send_pointer(
        &mut self,
        _: &crate::components::desktop_session_pointer_input::DesktopPointerInput,
        _: &DesktopInputCancellation,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
        unreachable!()
    }
    fn frame_mapping(&mut self) -> Result<Option<FrameMapping>, DesktopSessionInputFailure> {
        Ok(Some(FrameMapping {
            generation: 1,
            width: 1920,
            height: 1080,
        }))
    }
    fn send_frame_point(
        &mut self,
        p: &FramePoint,
        _: u32,
        _: &DesktopInputCancellation,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
        let mut t = self.0.lock().unwrap();
        t.calls += 1;
        t.points.push(*p);
        Ok(DesktopPointerDispatchFacts::new(1, 3))
    }
    fn close(self: Box<Self>) -> Result<(), DesktopSessionPortFailure> {
        self.0.lock().unwrap().closes += 1;
        Ok(())
    }
}
fn fixture() -> (DesktopSessionModule<Port>, String, Arc<Mutex<Trace>>) {
    let trace = Arc::new(Mutex::new(Trace::default()));
    let mut module = DesktopSessionModule::new(Port(Arc::clone(&trace)));
    let id = module
        .open(
            true,
            true,
            IsolationRequirement::Standard,
            Duration::from_secs(10),
        )
        .unwrap()
        .session_id()
        .to_owned();
    module.sessions.get_mut(&id).unwrap().observation = Some(Observation {
        id: "a".repeat(32),
        baseline: None,
        width: 1280,
        height: 720,
        mapping: Some(FrameMapping {
            generation: 1,
            width: 1920,
            height: 1080,
        }),
    });
    (module, id, trace)
}
fn run(
    module: &mut DesktopSessionModule<Port>,
    id: &str,
    value: Value,
) -> AppResult<InteractionReport> {
    module.interact(
        id,
        true,
        true,
        IsolationRequirement::Standard,
        &value,
        &DesktopInputCancellation::new(),
    )
}

#[test]
fn invalid_tail_or_point_dispatches_nothing() {
    let (mut m, id, t) = fixture();
    assert!(
        run(
            &mut m,
            &id,
            json!({"steps":[{"type":"text","text":"first"},{"type":"key","keys":["bogus"]}]})
        )
        .is_err()
    );
    assert!(run(&mut m,&id,json!({"frameId":"a".repeat(32),"steps":[{"type":"text","text":"first"},{"type":"click","x":1280,"y":10}]})).is_err());
    assert_eq!(t.lock().unwrap().calls, 0);
}

#[test]
fn confirmation_precedes_input_parsing_and_observation_lookup() {
    let (mut m, id, t) = fixture();
    let e = m
        .interact(
            &id,
            false,
            false,
            IsolationRequirement::Strict,
            &json!({"bad":true}),
            &DesktopInputCancellation::new(),
        )
        .err()
        .unwrap();
    assert_eq!(e.code, "CONFIRMATION_REQUIRED");
    assert_eq!(t.lock().unwrap().calls, 0);
}

#[test]
fn stale_observation_or_mapping_does_not_click() {
    let (mut m, id, t) = fixture();
    let p = json!({"frameId":"b".repeat(32),"steps":[{"type":"click","x":500,"y":200}]});
    assert_eq!(run(&mut m, &id, p).err().unwrap().code, "STALE_OBSERVATION");
    m.sessions
        .get_mut(&id)
        .unwrap()
        .observation
        .as_mut()
        .unwrap()
        .mapping
        .as_mut()
        .unwrap()
        .generation = 2;
    assert_eq!(
        run(
            &mut m,
            &id,
            json!({"frameId":"a".repeat(32),"steps":[{"type":"click","x":500,"y":200}]})
        )
        .err()
        .unwrap()
        .code,
        "STALE_OBSERVATION"
    );
    assert_eq!(t.lock().unwrap().calls, 0);
}

#[test]
fn mixed_input_uses_same_lease_and_preserves_observation_coordinates() {
    let (mut m, id, t) = fixture();
    let r=run(&mut m,&id,json!({"frameId":"a".repeat(32),"steps":[{"type":"click","x":640,"y":360},{"type":"text","text":"x".repeat(160)},{"type":"key","keys":["enter"]}]})).unwrap();
    assert_eq!(r.completed_steps, 3);
    assert_eq!(t.lock().unwrap().calls, 4);
    assert_eq!(t.lock().unwrap().points[0].image_width, 1280);
}

#[test]
fn failure_after_prior_dispatch_is_unknown_and_invalidates_once() {
    let (mut m, id, t) = fixture();
    t.lock().unwrap().fail_at = Some(2);
    let e = run(
        &mut m,
        &id,
        json!({"steps":[{"type":"text","text":"first"},{"type":"key","keys":["enter"]}]}),
    )
    .err()
    .unwrap();
    assert_eq!(e.details["outcome"], "unknown");
    assert_eq!(e.details["acceptedMayHaveOccurred"], true);
    assert_eq!(e.details["completedInteractionSteps"], 1);
    assert_eq!(e.details["inputEventsSent"], 10);
    assert!(m.inspect(&id).is_err());
    assert_eq!(t.lock().unwrap().closes, 1);
}

#[test]
fn cancellation_during_wait_does_not_send_later_input() {
    let (mut m, id, t) = fixture();
    let cancel = DesktopInputCancellation::new();
    let other = cancel.clone();
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(15));
        other.cancel();
    });
    let e = m
        .interact(
            &id,
            true,
            true,
            IsolationRequirement::Standard,
            &json!({"steps":[{"type":"wait","ms":300},{"type":"text","text":"never"}]}),
            &cancel,
        )
        .err()
        .unwrap();
    worker.join().unwrap();
    assert_eq!(e.code, "CANCELLED");
    assert_eq!(t.lock().unwrap().calls, 0);
    assert_eq!(t.lock().unwrap().closes, 1);
}
