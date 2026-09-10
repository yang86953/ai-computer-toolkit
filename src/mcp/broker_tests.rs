//! 对真实子进程管道验证 deadline、响应关联和协作取消；不打开桌面。
use super::*;
use std::{fs, os::unix::fs::PermissionsExt};
struct Fixture {
    directory: std::path::PathBuf,
    program: String,
}
impl Fixture {
    fn new(feature: bool) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "act-broker-test-{}",
            crate::components::desktop_session_identity::random_nonce().unwrap()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("broker");
        let script=r##"#!/usr/bin/env python3
import json, sys
from pathlib import Path
E='1'*32
C='act/linux-desktop-session-broker/v1'
def emit(v): print(json.dumps(v),flush=True)
def response(r,**fields):
    v=dict(contractVersion=C,brokerEpoch=E,requestNonce=r['requestNonce'],operation=r['operation'],messageType='response',completed=True,businessAccepted=True,outcome='completed',data={})
    v.update(fields);return v
emit(dict(contractVersion=C,brokerEpoch=E,messageType='broker-ready',transport='json-lines-stdio',postInputObservation=FEATURE))
pending=None
for line in sys.stdin:
    r=json.loads(line)
    if r['operation']=='interact':pending=r
    elif r['operation']=='input-cancel':
        assert r['targetRequestNonce']==pending['requestNonce']
        emit(response(pending,completed=False,businessAccepted=False,outcome='cancelled',error=dict(code='CANCELLED',message='cancelled',details=dict(acceptedMayHaveOccurred=True))))
        emit(response(r))
        pending=None
    elif r['operation']=='inspect':emit(response(r,requestNonce='2'*32))
    elif r['operation']=='shutdown':
        Path(__file__).with_name('shutdown').write_text('closed')
        emit(response(r));break
    else:emit(response(r))
"##.replace("FEATURE",if feature {"True"} else {"False"});
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            program: path.to_string_lossy().into_owned(),
            directory,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
#[test]
fn mismatched_response_closes_transport_without_replay() {
    let fixture = Fixture::new(true);
    let mut broker = Broker::start(&fixture.program).unwrap();
    assert!(
        broker
            .call("inspect", json!({}), Duration::from_secs(1))
            .unwrap_err()
            .outcome_unknown
    );
    assert!(broker.closed);
    assert_eq!(
        broker
            .call("sessions", json!({}), Duration::from_secs(1))
            .unwrap_err()
            .code,
        "BROKER_CLOSED"
    );
}
#[test]
fn timeout_is_bounded_and_never_replays() {
    let fixture = Fixture::new(true);
    let mut broker = Broker::start(&fixture.program).unwrap();
    let started = Instant::now();
    assert!(
        broker
            .call("interact", json!({}), Duration::from_millis(70))
            .unwrap_err()
            .outcome_unknown
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(broker.closed);
}
#[test]
fn cancellation_drains_its_ack_and_preserves_business_failure_facts() {
    let fixture = Fixture::new(true);
    let mut broker = Broker::start(&fixture.program).unwrap();
    let token =
        crate::components::desktop_session_input_cancellation::DesktopInputCancellation::new();
    broker.set_cancellation(token.clone());
    let cancel = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        token.cancel();
    });
    let failure = broker
        .call("interact", json!({}), Duration::from_secs(2))
        .unwrap_err();
    cancel.join().unwrap();
    assert_eq!(failure.code, "CANCELLED");
    assert!(failure.accepted_may_have_occurred);
    assert_eq!(failure.payload()["outcome"], "cancelled");
    assert!(!broker.closed);
    assert!(
        broker
            .call("sessions", json!({}), Duration::from_secs(1))
            .is_ok()
    );
    broker.stop();
}
#[test]
fn incompatible_broker_receives_shutdown_before_termination() {
    let fixture = Fixture::new(false);
    assert!(
        matches!(Broker::start(&fixture.program),Err(f) if f.code=="BROKER_FEATURE_UNAVAILABLE")
    );
    assert!(fixture.directory.join("shutdown").exists());
}
