//! 原 broker 公开入口的订阅回归：异步 producer、补丁拼接、重放、期限与收尾配额。
use super::*;
use crate::components::desktop_session_input_cancellation::DesktopInputCancellation;
use crate::{
    components::{
        desktop_frame_changes::Region,
        desktop_frame_stream::{FrameMailbox, FrameUpdate, MappedFrame},
        desktop_session_frame_capture::DesktopPackedPixelFormat,
    },
    modules::desktop_session::{
        DesktopKeyboardDispatchFacts, DesktopPointerDispatchFacts, DesktopSessionFacts,
        DesktopSessionFrameFailure, DesktopSessionInputFailure, DesktopSessionLease,
        DesktopSessionPortFailure,
    },
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Trace {
    active: Option<(String, Arc<FrameMailbox>)>,
    starts: usize,
    nexts: usize,
    stops: usize,
    closes: usize,
    retired: (u64, u64),
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
            Box::new(Lease(self.0.clone())),
            DesktopSessionFacts::new(2, 5, vec!["keyboard", "pointer"], 1, 1),
        ))
    }
}
fn failure(code: &'static str) -> DesktopSessionFrameFailure {
    DesktopSessionFrameFailure::new(code, "fixture-subscription", false, false)
}
impl DesktopSessionLease for Lease {
    fn subscription_stats(&self) -> (u64, u64) {
        let t = self.0.lock().unwrap();
        let active = t.active.as_ref().map(|(_, m)| m.stats()).unwrap_or((0, 0));
        (t.retired.0 + active.0, t.retired.1 + active.1)
    }
    fn send_keyboard(
        &mut self,
        _: &crate::components::keyboard_input_contract::KeyboardInput,
        _: &DesktopInputCancellation,
    ) -> Result<DesktopKeyboardDispatchFacts, DesktopSessionInputFailure> {
        unreachable!()
    }
    fn send_pointer(
        &mut self,
        _: &crate::components::desktop_session_pointer_input::DesktopPointerInput,
        _: &DesktopInputCancellation,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
        unreachable!()
    }
    fn subscribe_frames(
        &mut self,
        id: &str,
        _: Duration,
    ) -> Result<(), DesktopSessionFrameFailure> {
        let mut t = self.0.lock().unwrap();
        if t.active.is_some() {
            return Err(failure("SUBSCRIPTION_ALREADY_ACTIVE"));
        }
        t.starts += 1;
        t.active = Some((id.to_owned(), Arc::new(FrameMailbox::default())));
        Ok(())
    }
    fn next_frame_update(
        &mut self,
        id: &str,
        after: u64,
        wait: Duration,
    ) -> Result<Option<FrameUpdate>, DesktopSessionFrameFailure> {
        let mut t = self.0.lock().unwrap();
        let m = t
            .active
            .as_ref()
            .filter(|(key, _)| key == id)
            .ok_or_else(|| failure("STALE_SUBSCRIPTION"))?
            .1
            .clone();
        t.nexts += 1;
        drop(t);
        m.next(after, wait).map_err(failure)
    }
    fn unsubscribe_frames(&mut self, id: &str) -> Result<(), DesktopSessionFrameFailure> {
        let mut t = self.0.lock().unwrap();
        if !t.active.as_ref().is_some_and(|(key, _)| key == id) {
            return Err(failure("STALE_SUBSCRIPTION"));
        }
        let m = t.active.take().unwrap().1;
        m.finish("SUBSCRIPTION_CLOSED");
        let stats = m.stats();
        t.retired.0 += stats.0;
        t.retired.1 += stats.1;
        t.stops += 1;
        Ok(())
    }
    fn close(self: Box<Self>) -> Result<(), DesktopSessionPortFailure> {
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut t = self.0.lock().unwrap();
        if let Some((_, m)) = t.active.take() {
            m.finish("SUBSCRIPTION_CLOSED");
        }
        t.closes += 1;
    }
}
struct Dir(std::path::PathBuf);
impl Dir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "frame-subscription-test-{}",
            crate::components::desktop_session_identity::random_nonce().unwrap()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
const EPOCH: &str = "11111111111111111111111111111111";
fn request(operation: &str, nonce: usize) -> Value {
    json!({"contractVersion":CONTRACT_VERSION,"brokerEpoch":EPOCH,"requestNonce":format!("{nonce:032x}"),"operation":operation})
}
fn valid(value: &Value) {
    let schema: Value = serde_json::from_str(include_str!(
        "../../contracts/v1/linux-desktop-session-broker-v1.schema.json"
    ))
    .unwrap();
    jsonschema::draft202012::validate(&schema, value).unwrap_or_else(|e| panic!("{e}: {value}"));
}
fn call(b: &mut Broker<Port>, v: Value) -> Value {
    valid(&v);
    let r = b.handle(serde_json::from_value(v).unwrap()).0;
    valid(&r);
    r
}
fn setup() -> (Broker<Port>, Arc<Mutex<Trace>>, String, Dir) {
    let trace = Arc::new(Mutex::new(Trace::default()));
    let mut b = Broker::new(EPOCH.into(), DesktopSessionModule::new(Port(trace.clone())));
    valid(&b.ready());
    assert_eq!(b.ready()["frameSubscriptions"], true);
    let mut r = request("open", 1);
    r.as_object_mut().unwrap().extend(json!({"confirmed":true,"foregroundConsent":true,"strictIsolation":false,"timeoutMs":10000}).as_object().unwrap().clone());
    let id = call(&mut b, r)["data"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    (b, trace, id, Dir::new())
}
fn sub_request(op: &str, id: &str, nonce: usize, input: Value) -> Value {
    let mut r = request(op, nonce);
    r["sessionId"] = json!(id);
    r["input"] = input;
    if op != "observe-unsubscribe" {
        r["confirmed"] = json!(true);
        r["strictIsolation"] = json!(false);
    }
    r
}
fn next(id: &str, sub: &Value, nonce: usize, after: &Value, dir: &Dir) -> Value {
    sub_request(
        "observe-next",
        id,
        nonce,
        json!({"subscriptionId":sub,"afterSequence":after,"path":dir.0.join(format!("{nonce}.png"))}),
    )
}
fn feed(trace: &Arc<Mutex<Trace>>, seq: u64, color: u8) {
    let m = trace.lock().unwrap().active.as_ref().unwrap().1.clone();
    let rgba = [0, 0, 0, 255, color, 0, 0, 255];
    m.ingest(
        MappedFrame {
            allocation: &rgba,
            offset: 0,
            chunk_size: 8,
            stride: 8,
            width: 2,
            height: 1,
            format: DesktopPackedPixelFormat::Rgba,
        },
        Some(seq),
        false,
        Some(&[Region {
            x: 1,
            y: 0,
            width: 1,
            height: 1,
        }]),
    )
    .unwrap();
}
#[test]
fn public_subscription_delivers_keyframe_then_cropped_patch_and_replay_is_exact() {
    let (mut b, t, id, dir) = setup();
    let req = sub_request("observe-subscribe", &id, 2, json!({}));
    let started = call(&mut b, req.clone());
    let sub = &started["data"]["subscriptionId"];
    assert_eq!(started["data"]["status"], "starting");
    assert_eq!(call(&mut b, req)["data"], started["data"]);
    assert_eq!(t.lock().unwrap().starts, 1);
    feed(&t, 1, 0);
    assert_eq!(b.module.inspect(&id).unwrap().frames_captured(), 1);
    assert_eq!(b.module.inspect(&id).unwrap().pixels_consumed(), 2);
    let first = call(&mut b, next(&id, sub, 3, &json!(0), &dir));
    assert_eq!(first["data"]["kind"], "keyframe");
    let mut full = image::open(dir.0.join("3.png")).unwrap().to_rgba8();
    assert_eq!(full.dimensions(), (2, 1));
    feed(&t, 2, 10);
    let req = next(&id, sub, 4, &first["data"]["sequence"], &dir);
    let patch = call(&mut b, req.clone());
    assert_eq!(patch["data"]["kind"], "patch");
    assert_eq!(patch["data"]["baseSequence"], first["data"]["sequence"]);
    assert_eq!(patch["data"]["captureMethod"], "pipewire-damage");
    let pixels = image::open(dir.0.join("4.png")).unwrap().to_rgba8();
    assert_eq!(pixels.dimensions(), (1, 1));
    full.put_pixel(1, 0, *pixels.get_pixel(0, 0));
    assert_eq!(full.as_raw(), &[0, 0, 0, 255, 10, 0, 0, 255]);
    feed(&t, 3, 20);
    assert_eq!(call(&mut b, req)["data"], patch["data"]);
    assert_eq!(t.lock().unwrap().nexts, 2);
    let resync = call(&mut b, next(&id, sub, 5, &first["data"]["sequence"], &dir));
    assert_eq!(resync["data"]["kind"], "keyframe");
    let idle = call(&mut b, next(&id, sub, 6, &resync["data"]["sequence"], &dir));
    assert_eq!(idle["data"]["status"], "idle");
    assert!(!dir.0.join("6.png").exists());
    let closed = call(
        &mut b,
        sub_request("observe-unsubscribe", &id, 7, json!({"subscriptionId":sub})),
    );
    assert_eq!(closed["data"]["workerJoined"], true);
    assert!(t.lock().unwrap().active.is_none());
    assert_eq!(b.module.inspect(&id).unwrap().frames_captured(), 3);
    assert_eq!(b.module.inspect(&id).unwrap().pixels_consumed(), 6);
    assert_eq!(
        call(&mut b, next(&id, sub, 8, &json!(0), &dir))["error"]["code"],
        "STALE_SUBSCRIPTION"
    );
}
#[test]
fn async_arrival_wakes_next_without_fixed_sleep_and_errors_preserve_subscription_safety() {
    let (mut b, t, id, dir) = setup();
    let started = call(&mut b, sub_request("observe-subscribe", &id, 2, json!({})));
    let sub = &started["data"]["subscriptionId"];
    let mut denied = next(&id, sub, 3, &json!(0), &dir);
    denied["confirmed"] = json!(false);
    assert_eq!(
        call(&mut b, denied)["error"]["code"],
        "CONFIRMATION_REQUIRED"
    );
    assert_eq!(t.lock().unwrap().nexts, 0);
    assert_eq!(
        call(&mut b, sub_request("observe-subscribe", &id, 4, json!({})))["error"]["code"],
        "SUBSCRIPTION_ALREADY_ACTIVE"
    );
    let producer = t.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        feed(&producer, 1, 10);
    });
    let mut pending = next(&id, sub, 5, &json!(0), &dir);
    pending["input"]["waitMs"] = json!(1000);
    let result = call(&mut b, pending);
    thread.join().unwrap();
    assert_eq!(result["data"]["status"], "update");
    let wrong = next(&id, &json!("a".repeat(32)), 6, &json!(0), &dir);
    assert_eq!(call(&mut b, wrong)["error"]["code"], "STALE_SUBSCRIPTION");
    let m = t.lock().unwrap().active.as_ref().unwrap().1.clone();
    m.finish("SUBSCRIPTION_EXPIRED");
    assert_eq!(
        call(&mut b, next(&id, sub, 7, &result["data"]["sequence"], &dir))["error"]["code"],
        "SUBSCRIPTION_EXPIRED"
    );
    drop(b);
    assert!(t.lock().unwrap().active.is_none());
    assert_eq!(t.lock().unwrap().closes, 1);
}
#[test]
fn output_preflight_and_input_bounds_do_not_consume_updates() {
    let (mut b, t, id, dir) = setup();
    let started = call(&mut b, sub_request("observe-subscribe", &id, 2, json!({})));
    let sub = &started["data"]["subscriptionId"];
    feed(&t, 1, 1);
    std::fs::write(dir.0.join("3.png"), b"do-not-overwrite").unwrap();
    let r = call(&mut b, next(&id, sub, 3, &json!(0), &dir));
    assert_eq!(r["completed"], false);
    assert_eq!(t.lock().unwrap().nexts, 0);
    assert_eq!(
        std::fs::read(dir.0.join("3.png")).unwrap(),
        b"do-not-overwrite"
    );
    for (i, input) in [
        json!({"subscriptionId":sub,"afterSequence":0,"path":dir.0.join("bad.png"),"waitMs":1001}),
        json!({"subscriptionId":sub,"afterSequence":0,"path":dir.0.join("bad.png"),"unknown":true}),
    ]
    .into_iter()
    .enumerate()
    {
        let bad = sub_request("observe-next", &id, 10 + i, input);
        let schema: Value = serde_json::from_str(include_str!(
            "../../contracts/v1/linux-desktop-session-broker-v1.schema.json"
        ))
        .unwrap();
        assert!(jsonschema::draft202012::validate(&schema, &bad).is_err());
        let r = b.handle(serde_json::from_value(bad).unwrap()).0;
        assert_eq!(r["error"]["code"], "INVALID_ARGUMENT");
    }
    assert_eq!(t.lock().unwrap().nexts, 0);
}
#[test]
fn saturated_ledger_keeps_unsubscribe_and_close_reserves_independent() {
    let (mut b, t, id, _) = setup();
    let started = call(&mut b, sub_request("observe-subscribe", &id, 2, json!({})));
    let sub = &started["data"]["subscriptionId"];
    for n in 3..=MAXIMUM_LEDGER_ENTRIES {
        let r = b
            .handle(serde_json::from_value(request("sessions", n)).unwrap())
            .0;
        assert_eq!(r["completed"], true);
    }
    assert_eq!(
        call(
            &mut b,
            sub_request(
                "observe-unsubscribe",
                &id,
                MAXIMUM_LEDGER_ENTRIES + 1,
                json!({"subscriptionId":sub})
            )
        )["data"]["status"],
        "closed"
    );
    let mut close = request("close", MAXIMUM_LEDGER_ENTRIES + 2);
    close["sessionId"] = json!(id);
    assert_eq!(call(&mut b, close)["completed"], true);
    assert_eq!(t.lock().unwrap().closes, 1);
}
