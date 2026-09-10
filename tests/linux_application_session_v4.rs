#![cfg(target_os = "linux")]

//! Linux application.session.discover@4 的启动状态聚合与兼容回归。

use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::Value;

const TOOLKIT: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit");
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    root: PathBuf,
    user: PathBuf,
    system: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "ai-computer-toolkit-application-session-v4-{}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let user = root.join("user");
        let system = root.join("system");
        fs::create_dir_all(user.join("applications")).unwrap_or_else(|error| {
            panic!("建立用户应用目录失败：{error}");
        });
        fs::create_dir_all(&system).unwrap_or_else(|error| {
            panic!("建立系统数据目录失败：{error}");
        });
        fs::write(
            user.join("applications/toolkit-fixture.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName=Toolkit fixture\nExec=\"{TOOLKIT}\" __application-launch-fixture-v1\n"
            ),
        )
        .unwrap_or_else(|error| panic!("写入 toolkit Desktop Entry 失败：{error}"));
        fs::write(
            user.join("applications/ordinary.desktop"),
            "[Desktop Entry]\nType=Application\nName=Ordinary application\nExec=/bin/true\n",
        )
        .unwrap_or_else(|error| panic!("写入普通 Desktop Entry 失败：{error}"));
        Self { root, user, system }
    }

    fn run(&self, arguments: &[&str]) -> (i32, Value) {
        let output = Command::new(TOOLKIT)
            .args(arguments)
            .env_clear()
            .env("XDG_DATA_HOME", &self.user)
            .env("XDG_DATA_DIRS", &self.system)
            .env("XDG_CURRENT_DESKTOP", "KDE")
            .env("LANG", "C")
            .env("PATH", "/usr/bin")
            .output()
            .unwrap_or_else(|error| panic!("运行 toolkit fixture CLI 失败：{error}"));
        let value = serde_json::from_slice::<Value>(&output.stdout)
            .unwrap_or_else(|error| panic!("CLI stdout 必须是单一 JSON：{error}"));
        (output.status.code().unwrap_or(-1), value)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn validate(schema: &str, value: &Value) {
    let schema = serde_json::from_str::<Value>(schema)
        .unwrap_or_else(|error| panic!("schema 必须解析：{error}"));
    jsonschema::draft202012::validate(&schema, value)
        .unwrap_or_else(|error| panic!("结果必须匹配 schema：{error}"));
}

fn application<'a>(value: &'a Value, name: &str) -> &'a Value {
    value["data"]["applications"]
        .as_array()
        .and_then(|applications| {
            applications
                .iter()
                .find(|application| application["displayName"] == name)
        })
        .unwrap_or_else(|| panic!("必须发现应用 {name}"))
}

#[test]
fn v4_atomically_publishes_launch_status_and_keeps_relationships_closed() {
    let fixture = Fixture::new();
    let (code, v4) = fixture.run(&[
        "discover",
        "app",
        "--capability",
        "application.session.discover@4",
        "--max-applications",
        "8",
        "--max-processes",
        "1",
        "--max-windows",
        "1",
    ]);
    assert_eq!(code, 0);
    validate(
        include_str!("../contracts/v4/application-session-discovery.schema.json"),
        &v4,
    );
    assert_eq!(v4["data"]["applicationContract"], "application.discover@3");
    assert_eq!(
        application(&v4, "Toolkit fixture")["launchCapability"],
        "available-confirmed"
    );
    assert_eq!(
        application(&v4, "Ordinary application")["launchCapability"],
        "unavailable"
    );
    assert!(v4["data"]["applications"].as_array().is_some_and(|items| {
        items.iter().all(|item| {
            item["runningProcessSessionIds"] == serde_json::json!([])
                && item["relationshipEvidence"] == "none"
        })
    }));

    let (code, v3) = fixture.run(&[
        "discover",
        "app",
        "--capability",
        "application.session.discover@3",
        "--max-applications",
        "8",
        "--max-processes",
        "1",
        "--max-windows",
        "1",
    ]);
    assert_eq!(code, 0);
    validate(
        include_str!("../contracts/v3/application-session-discovery.schema.json"),
        &v3,
    );
    assert!(v3["data"]["applications"].as_array().is_some_and(|items| {
        items
            .iter()
            .all(|item| item["launchCapability"] == "unavailable")
    }));
}

#[test]
fn v4_metadata_and_assessment_are_explicit_and_read_only() {
    let fixture = Fixture::new();
    let (code, surface) = fixture.run(&["capabilities"]);
    assert_eq!(code, 0);
    let entry = surface["data"]["capabilities"]
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["id"] == "application.session.discover@4")
        })
        .unwrap_or_else(|| panic!("版本四能力元数据必须存在"));
    assert_eq!(
        entry["status"],
        "available-verified-read-only-linux-launch-aware-uix-aggregation"
    );
    assert_eq!(entry["executionDomain"], "same-session-no-focus");

    let (code, discovery) = fixture.run(&[
        "discover",
        "app",
        "--capability",
        "application.session.discover@4",
        "--max-applications",
        "2",
        "--max-processes",
        "1",
        "--max-windows",
        "1",
    ]);
    assert_eq!(code, 0);
    let host = discovery["data"]["hostTargetId"]
        .as_str()
        .unwrap_or_else(|| panic!("host target 必须存在"));
    let (code, assessment) = fixture.run(&[
        "assess",
        "app",
        "--capability",
        "application.session.discover@4",
        "--target",
        &format!("sessionId={host}"),
    ]);
    assert_eq!(code, 0);
    assert_eq!(assessment["decision"], "executable-background");
    assert_eq!(assessment["executionRealm"], "same-session-no-focus");
    assert_eq!(assessment["requiresConfirmation"], false);
    assert_eq!(assessment["requiresForegroundConsent"], false);
    assert_eq!(assessment["constraints"]["noFallback"], true);
}
