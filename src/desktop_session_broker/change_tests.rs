//! 通过 broker 请求/响应验证 opt-in 差分、重放与失效，不依赖真实桌面权限。
use super::*;
use crate::{
    components::{
        desktop_interaction::FrameMapping,
        desktop_session_frame_capture::{self, DesktopPackedPixelFormat},
    },
    modules::desktop_session::{
        DesktopKeyboardDispatchFacts, DesktopPointerDispatchFacts, DesktopSessionFacts,
        DesktopSessionFrameFailure, DesktopSessionInputFailure, DesktopSessionLease,
        DesktopSessionPortFailure,
    },
};
use std::sync::{Arc, Mutex};

struct TestDir(std::path::PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "desktop-changes-{}",
            crate::components::desktop_session_identity::random_nonce().unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct State {
    captures: usize,
    color: u8,
    width: u32,
    height: u32,
    generation: u64,
    fail: bool,
}
struct Port(Arc<Mutex<State>>);
struct Lease(Arc<Mutex<State>>);
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
impl DesktopSessionLease for Lease {
    fn send_keyboard(
        &mut self,
        _: &crate::components::keyboard_input_contract::KeyboardInput,
        _: &crate::components::desktop_session_input_cancellation::DesktopInputCancellation,
    ) -> Result<DesktopKeyboardDispatchFacts, DesktopSessionInputFailure> {
        unreachable!()
    }
    fn send_pointer(
        &mut self,
        _: &crate::components::desktop_session_pointer_input::DesktopPointerInput,
        _: &crate::components::desktop_session_input_cancellation::DesktopInputCancellation,
    ) -> Result<DesktopPointerDispatchFacts, DesktopSessionInputFailure> {
        unreachable!()
    }
    fn frame_mapping(&mut self) -> Result<Option<FrameMapping>, DesktopSessionInputFailure> {
        let s = self.0.lock().unwrap();
        Ok(Some(FrameMapping {
            generation: s.generation,
            width: 1920,
            height: 1080,
        }))
    }
    fn capture_frame(
        &mut self,
        _: Duration,
        _: Option<u32>,
    ) -> Result<desktop_session_frame_capture::DesktopCapturedFrame, DesktopSessionFrameFailure>
    {
        let mut s = self.0.lock().unwrap();
        s.captures += 1;
        if s.fail {
            return Err(DesktopSessionFrameFailure::new(
                "CAPTURE_READBACK_FAILED",
                "fixture",
                false,
                false,
            ));
        }
        let mut rgba = vec![0; s.width as usize * s.height as usize * 4];
        rgba[0] = s.color;
        desktop_session_frame_capture::encode_mapped_frame(
            &rgba,
            0,
            rgba.len() as u32,
            (s.width * 4) as i32,
            (s.width, s.height),
            DesktopPackedPixelFormat::Rgba,
            None,
        )
        .map_err(|_| {
            DesktopSessionFrameFailure::new("CAPTURE_READBACK_FAILED", "fixture", false, false)
        })
    }
    fn close(self: Box<Self>) -> Result<(), DesktopSessionPortFailure> {
        Ok(())
    }
}
const EPOCH: &str = "11111111111111111111111111111111";
fn request(operation: &str, nonce: usize) -> Value {
    json!({"contractVersion":CONTRACT_VERSION,"brokerEpoch":EPOCH,"requestNonce":format!("{nonce:032x}"),"operation":operation})
}
fn schema_valid(value: &Value) {
    let schema: Value = serde_json::from_str(include_str!(
        "../../contracts/v1/linux-desktop-session-broker-v1.schema.json"
    ))
    .unwrap();
    jsonschema::draft202012::validate(&schema, value).unwrap_or_else(|e| panic!("{e}: {value}"));
}
fn call(b: &mut Broker<Port>, value: Value) -> Value {
    schema_valid(&value);
    let response = b.handle(serde_json::from_value(value).unwrap()).0;
    schema_valid(&response);
    response
}
fn setup() -> (Broker<Port>, Arc<Mutex<State>>, String, TestDir) {
    let state = Arc::new(Mutex::new(State {
        width: 2,
        height: 1,
        generation: 1,
        ..State::default()
    }));
    let mut b = Broker::new(
        EPOCH.to_owned(),
        DesktopSessionModule::new(Port(state.clone())),
    );
    schema_valid(&b.ready());
    assert_eq!(
        b.ready()["observationModes"],
        json!(["snapshot", "frame-diff", "region-diff"])
    );
    let mut open = request("open", 1);
    open.as_object_mut().unwrap().extend(json!({"confirmed":true,"foregroundConsent":true,"strictIsolation":false,"timeoutMs":10000}).as_object().unwrap().clone());
    let response = call(&mut b, open);
    let id = response["data"]["sessionId"].as_str().unwrap().to_owned();
    (b, state, id, TestDir::new())
}
fn observe(id: &str, dir: &TestDir, nonce: usize, options: Option<Value>) -> Value {
    let mut value = request("observe", nonce);
    value.as_object_mut().unwrap().extend(
        json!({"sessionId":id,"confirmed":true,"strictIsolation":false,
        "input":{"path":dir.path().join(format!("{nonce}.png")),"timeoutMs":1000}})
        .as_object()
        .unwrap()
        .clone(),
    );
    if let Some(options) = options {
        value["changeDetection"] = options;
    }
    value
}

#[test]
fn tracked_frames_report_changes_and_replay_does_not_capture_again() {
    let (mut b, state, id, dir) = setup();
    let first = call(&mut b, observe(&id, &dir, 2, Some(json!({}))));
    assert_eq!(first["data"]["changes"]["status"], "baseline-reset");
    assert!(first["data"]["changes"]["changed"].is_null());
    let next = observe(
        &id,
        &dir,
        3,
        Some(json!({"baselineFrameId":first["data"]["frameId"]})),
    );
    let same = call(&mut b, next.clone());
    assert_eq!(same["data"]["changes"]["changed"], false);
    state.lock().unwrap().color = 10;
    let replay = call(&mut b, next);
    assert_eq!(replay["data"], same["data"]);
    assert_eq!(state.lock().unwrap().captures, 2);
    let changed = call(
        &mut b,
        observe(
            &id,
            &dir,
            4,
            Some(json!({"baselineFrameId":same["data"]["frameId"]})),
        ),
    );
    assert_eq!(changed["data"]["changes"]["changedPixels"], 1);
    assert_eq!(
        changed["data"]["changes"]["changedBounds"],
        json!({"x":0,"y":0,"width":1,"height":1})
    );
    assert_eq!(changed["data"]["changes"]["effectConfirmed"], false);
    let stale = call(
        &mut b,
        observe(
            &id,
            &dir,
            5,
            Some(json!({"baselineFrameId":first["data"]["frameId"]})),
        ),
    );
    assert_eq!(stale["error"]["code"], "STALE_OBSERVATION");
    assert_eq!(state.lock().unwrap().captures, 3);
}

#[test]
fn plain_observe_releases_baseline_and_region_filters_noise() {
    let (mut b, state, id, dir) = setup();
    let first = call(&mut b, observe(&id, &dir, 2, Some(json!({}))));
    state.lock().unwrap().color = 10;
    let local = call(
        &mut b,
        observe(
            &id,
            &dir,
            3,
            Some(json!({"baselineFrameId":first["data"]["frameId"],
        "region":{"x":1,"y":0,"width":1,"height":1}})),
        ),
    );
    assert_eq!(local["data"]["changes"]["changed"], false);
    let plain = call(&mut b, observe(&id, &dir, 4, None));
    assert!(plain["data"].get("changes").is_none());
    let stale = call(
        &mut b,
        observe(
            &id,
            &dir,
            5,
            Some(json!({"baselineFrameId":plain["data"]["frameId"]})),
        ),
    );
    assert_eq!(stale["error"]["code"], "STALE_OBSERVATION");
    assert_eq!(state.lock().unwrap().captures, 3);
}

#[test]
fn resize_and_mapping_changes_reset_without_claiming_unchanged() {
    let (mut b, state, id, dir) = setup();
    let first = call(&mut b, observe(&id, &dir, 2, Some(json!({}))));
    state.lock().unwrap().width = 3;
    let resize = call(
        &mut b,
        observe(
            &id,
            &dir,
            3,
            Some(json!({"baselineFrameId":first["data"]["frameId"]})),
        ),
    );
    assert_eq!(resize["data"]["changes"]["reason"], "dimensions-changed");
    assert!(resize["data"]["changes"]["changed"].is_null());
    state.lock().unwrap().generation = 2;
    let mapping = call(
        &mut b,
        observe(
            &id,
            &dir,
            4,
            Some(json!({"baselineFrameId":resize["data"]["frameId"]})),
        ),
    );
    assert_eq!(mapping["data"]["changes"]["reason"], "mapping-changed");
    let same = call(
        &mut b,
        observe(
            &id,
            &dir,
            5,
            Some(json!({"baselineFrameId":mapping["data"]["frameId"]})),
        ),
    );
    assert_eq!(same["data"]["changes"]["changed"], false);
}

#[test]
fn failed_capture_invalidates_baseline_and_unconfirmed_capture_does_not_run() {
    let (mut b, state, id, dir) = setup();
    let first = call(&mut b, observe(&id, &dir, 2, Some(json!({}))));
    let options = json!({"baselineFrameId":first["data"]["frameId"]});
    let mut denied = observe(&id, &dir, 3, Some(options.clone()));
    denied["confirmed"] = json!(false);
    assert_eq!(
        call(&mut b, denied)["error"]["code"],
        "CONFIRMATION_REQUIRED"
    );
    assert_eq!(state.lock().unwrap().captures, 1);
    state.lock().unwrap().fail = true;
    assert_eq!(
        call(&mut b, observe(&id, &dir, 4, Some(options.clone())))["error"]["code"],
        "CAPTURE_READBACK_FAILED"
    );
    state.lock().unwrap().fail = false;
    assert_eq!(
        call(&mut b, observe(&id, &dir, 5, Some(options)))["error"]["code"],
        "STALE_OBSERVATION"
    );
    assert_eq!(state.lock().unwrap().captures, 2);
    assert_eq!(
        call(&mut b, observe(&id, &dir, 6, Some(json!({}))))["data"]["changes"]["status"],
        "baseline-reset"
    );
}

#[test]
fn out_of_bounds_region_is_rejected_without_consuming_another_frame() {
    let (mut b, state, id, dir) = setup();
    let first = call(&mut b, observe(&id, &dir, 2, Some(json!({}))));
    let bad = observe(
        &id,
        &dir,
        3,
        Some(json!({"baselineFrameId":first["data"]["frameId"],
        "region":{"x":2,"y":0,"width":1,"height":1}})),
    );
    assert_eq!(call(&mut b, bad)["error"]["code"], "INVALID_ARGUMENT");
    assert_eq!(state.lock().unwrap().captures, 1);
    assert!(!dir.path().join("3.png").exists());
}

#[test]
fn region_invalidated_by_resize_reports_committed_path_and_requires_reset() {
    let (mut b, state, id, dir) = setup();
    let first = call(&mut b, observe(&id, &dir, 2, Some(json!({}))));
    state.lock().unwrap().width = 1;
    let options = json!({"baselineFrameId":first["data"]["frameId"], "region":{"x":1,"y":0,"width":1,"height":1}});
    let response = call(&mut b, observe(&id, &dir, 3, Some(options.clone())));
    assert_eq!(response["error"]["code"], "INVALID_ARGUMENT");
    assert_eq!(
        response["error"]["details"]["pixelsMayHaveBeenConsumed"],
        true
    );
    assert_eq!(
        response["error"]["details"]["capturedPath"],
        json!(dir.path().join("3.png"))
    );
    assert!(dir.path().join("3.png").exists());
    assert_eq!(
        call(&mut b, observe(&id, &dir, 4, Some(options)))["error"]["code"],
        "STALE_OBSERVATION"
    );
}

#[test]
fn tracking_limit_falls_back_to_snapshot_without_retaining_oversized_baseline() {
    let (mut b, state, id, dir) = setup();
    {
        let mut s = state.lock().unwrap();
        s.width = 4096;
        s.height = 2049;
    }
    let large = call(&mut b, observe(&id, &dir, 2, Some(json!({}))));
    assert_eq!(large["data"]["changes"]["status"], "unavailable");
    assert_eq!(large["data"]["changes"]["reason"], "tracking-limit");
    assert!(large["data"]["changes"]["changed"].is_null());
    assert!(dir.path().join("2.png").exists());
    assert_eq!(
        call(
            &mut b,
            observe(
                &id,
                &dir,
                3,
                Some(json!({"baselineFrameId":large["data"]["frameId"]}))
            )
        )["error"]["code"],
        "STALE_OBSERVATION"
    );
    assert_eq!(state.lock().unwrap().captures, 1);
    {
        let mut s = state.lock().unwrap();
        s.width = 2;
        s.height = 1;
    }
    assert_eq!(
        call(&mut b, observe(&id, &dir, 4, Some(json!({}))))["data"]["changes"]["reason"],
        "baseline-created"
    );
}

#[test]
fn cross_session_baseline_and_nonce_option_changes_are_rejected() {
    let (mut b, state, id, dir) = setup();
    let original = observe(&id, &dir, 2, Some(json!({})));
    let first = call(&mut b, original.clone());
    let mut changed = original;
    changed["changeDetection"]["pixelThreshold"] = json!(3);
    assert_eq!(
        call(&mut b, changed)["error"]["code"],
        "NONCE_SEMANTIC_CONFLICT"
    );
    let mut open = request("open", 3);
    open.as_object_mut().unwrap().extend(json!({"confirmed":true,"foregroundConsent":true,"strictIsolation":false,"timeoutMs":10000}).as_object().unwrap().clone());
    let other = call(&mut b, open);
    let other_id = other["data"]["sessionId"].as_str().unwrap();
    assert_eq!(
        call(
            &mut b,
            observe(
                other_id,
                &dir,
                4,
                Some(json!({"baselineFrameId":first["data"]["frameId"]}))
            )
        )["error"]["code"],
        "STALE_OBSERVATION"
    );
    assert_eq!(state.lock().unwrap().captures, 1);
}
