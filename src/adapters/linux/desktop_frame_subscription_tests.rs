//! Native probe uses a private PipeWire daemon and generated test pixels, never a desktop source.
use super::*;
use std::{
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
};

const CONFIG: &str = r#"
context.properties = { core.daemon = true core.name = pipewire-0 default.video.width = 64 default.video.height = 48 }
context.spa-libs = { support.* = support/libspa-support video.convert.* = videoconvert/libspa-videoconvert }
context.modules = [
 { name = libpipewire-module-protocol-native }
 { name = libpipewire-module-spa-node-factory }
 { name = libpipewire-module-client-node }
 { name = libpipewire-module-adapter }
 { name = libpipewire-module-link-factory }
 { name = libpipewire-module-metadata }
 { name = libpipewire-module-access }
]
context.objects = [
 { factory = spa-node-factory args = { factory.name = support.node.driver node.name = Dummy-Driver priority.driver = 20000 } }
]
"#;
struct Daemon {
    path: PathBuf,
    child: Child,
}
impl Daemon {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "act-pipewire-native-{}",
            crate::components::desktop_session_identity::random_nonce().unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(path.join("fixture.conf"), CONFIG).unwrap();
        let log = std::fs::File::create(path.join("daemon.log")).unwrap();
        let child = Command::new("pipewire")
            .arg("-c")
            .arg(path.join("fixture.conf"))
            .env("PIPEWIRE_RUNTIME_DIR", &path)
            .env("XDG_RUNTIME_DIR", &path)
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        let mut daemon = Self { path, child };
        for _ in 0..100 {
            if daemon.path.join("pipewire-0").exists() {
                return daemon;
            }
            assert!(daemon.child.try_wait().unwrap().is_none());
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("fixture daemon did not publish its socket");
    }
    fn command(&self, program: &str, args: &[&str]) -> String {
        let output = Command::new("timeout")
            .arg("5s")
            .arg(program)
            .args(args)
            .env("PIPEWIRE_RUNTIME_DIR", &self.path)
            .env("XDG_RUNTIME_DIR", &self.path)
            .env("PIPEWIRE_REMOTE", "pipewire-0")
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.stderr.is_empty() {
            eprintln!(
                "fixture {program}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        String::from_utf8(output.stdout).unwrap()
    }
    fn nodes(&self) -> Vec<serde_json::Value> {
        serde_json::from_str(&self.command("pw-dump", &[])).unwrap()
    }
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn metadata_parameters_round_trip_and_drop_joins_the_worker() {
    for (kind, size) in [
        (
            spa::sys::SPA_META_Header,
            std::mem::size_of::<spa::sys::spa_meta_header>() as i32,
        ),
        (
            spa::sys::SPA_META_VideoDamage,
            (256 * std::mem::size_of::<spa::sys::spa_meta_region>()) as i32,
        ),
    ] {
        let bytes = meta_parameter(kind, size).unwrap();
        let (_, value) =
            spa::pod::deserialize::PodDeserializer::deserialize_any_from(&bytes).unwrap();
        let Value::Object(object) = value else {
            panic!("metadata must be an object");
        };
        assert_eq!(object.type_, spa::sys::SPA_TYPE_OBJECT_ParamMeta);
        assert_eq!(object.properties[0].value, Value::Id(spa::utils::Id(kind)));
        assert_eq!(object.properties[1].value, Value::Int(size));
    }
    let mailbox = Arc::new(FrameMailbox::default());
    let producer = mailbox.clone();
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = done.clone();
    let worker = std::thread::spawn(move || {
        assert_eq!(
            producer.next(0, Duration::from_secs(5)).unwrap_err(),
            "SUBSCRIPTION_CLOSED"
        );
        flag.store(true, std::sync::atomic::Ordering::Release);
    });
    let subscription = PipeWireSubscription {
        id: "a".repeat(32),
        mailbox,
        worker: Some(worker),
    };
    drop(subscription);
    assert!(done.load(std::sync::atomic::Ordering::Acquire));
}

#[test]
#[ignore = "requires pipewire, pw-cli, pw-dump and timeout; creates an isolated daemon and owned RGBA source"]
fn native_pipewire_subscription_fixture() {
    let daemon = Daemon::new();
    let source_stream = fixture_source::Source::start(
        UnixStream::connect(daemon.path.join("pipewire-0"))
            .unwrap()
            .into(),
    );
    let source = source_stream.id;
    let remote: OwnedFd = UnixStream::connect(daemon.path.join("pipewire-0"))
        .unwrap()
        .into();
    let mailbox = Arc::new(FrameMailbox::default());
    let producer = mailbox.clone();
    let worker = std::thread::spawn(move || {
        let result = run_stream(
            remote,
            PipeWireStreamTarget::NodeId(source),
            Duration::from_secs(3),
            &producer,
            || Ok(()),
        );
        producer.finish(result.err().unwrap_or("SUBSCRIPTION_CLOSED"));
    });
    let subscription = PipeWireSubscription {
        id: "a".repeat(32),
        mailbox,
        worker: Some(worker),
    };
    let mut input = None;
    for _ in 0..50 {
        input = daemon
            .nodes()
            .into_iter()
            .find(|n| {
                n["type"] == "PipeWire:Interface:Node"
                    && n["info"]["props"]["media.name"] == "ai-computer-toolkit-frame-subscription"
            })
            .and_then(|n| n["id"].as_u64());
        if input.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let input = input.unwrap_or_else(|| panic!("consumer node missing: {:?}", daemon.nodes()));
    daemon.command(
        "pw-cli",
        &[
            "--",
            "create-link",
            &source.to_string(),
            "0",
            &input.to_string(),
            "0",
            "{ object.linger=true }",
        ],
    );
    let first = subscription
        .next(&subscription.id, 0, Duration::from_secs(2))
        .unwrap_or_else(|e| {
            panic!(
                "{e}: {}",
                std::fs::read_to_string(daemon.path.join("daemon.log")).unwrap()
            )
        })
        .unwrap_or_else(|| {
            panic!(
                "fixture must deliver a keyframe: log={} nodes={:?}",
                std::fs::read_to_string(daemon.path.join("daemon.log")).unwrap(),
                daemon.nodes()
            )
        });
    assert!(first.keyframe);
    assert_eq!(
        first.rgba.len(),
        first.width as usize * first.height as usize * 4
    );
    std::thread::sleep(Duration::from_millis(150));
    let latest = subscription
        .next(&subscription.id, 0, Duration::ZERO)
        .unwrap()
        .unwrap();
    assert!(
        latest.sequence > first.sequence,
        "worker must consume frames without next calls"
    );
    eprintln!(
        "native subscription: {}x{}, sequence {} -> {}, method {}, pixelsRead {}",
        first.width,
        first.height,
        first.sequence,
        latest.sequence,
        latest.method,
        latest.pixels_read
    );
    let end = Instant::now() + Duration::from_secs(4);
    loop {
        match subscription.next(
            &subscription.id,
            latest.sequence,
            Duration::from_millis(100),
        ) {
            Err("SUBSCRIPTION_EXPIRED") => break,
            Err(e) => panic!("unexpected terminal: {e}"),
            _ => assert!(
                Instant::now() < end,
                "subscription TTL did not terminate worker"
            ),
        }
    }
    drop(subscription);
}
