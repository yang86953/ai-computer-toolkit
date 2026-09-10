#![cfg(target_os = "linux")]

use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{Value, json};

const TOOLKIT: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit");
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    root: PathBuf,
    user: PathBuf,
    system: PathBuf,
    input: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "ai-computer-toolkit-application-launch-{}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let user = root.join("user");
        let system = root.join("system");
        fs::create_dir_all(user.join("applications")).expect("建立用户应用目录");
        fs::create_dir_all(&system).expect("建立系统数据目录");
        let input = root.join("input.json");
        fs::write(&input, b"{}").expect("写入空输入");
        let fixture = Self {
            root,
            user,
            system,
            input,
        };
        fixture.write_entry("Owned launch fixture");
        fixture
    }

    fn write_entry(&self, name: &str) {
        fs::write(
            self.user.join("applications/toolkit-owned.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nName={name}\nExec=\"{TOOLKIT}\" __application-launch-fixture-v1\n"
            ),
        )
        .expect("写入 Desktop Entry");
    }

    fn write_untrusted_entry(&self) {
        fs::write(
            self.user.join("applications/ordinary.desktop"),
            "[Desktop Entry]\nType=Application\nName=Ordinary application\nExec=/bin/true\n",
        )
        .expect("写入普通应用 Desktop Entry");
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
            .expect("运行 toolkit fixture CLI");
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
    let schema = serde_json::from_str::<Value>(schema).expect("schema 必须解析");
    jsonschema::draft202012::validate(&schema, value).expect("结果必须匹配 schema");
}

#[test]
fn fixture_discovery_assessment_launch_and_stale_route_are_end_to_end() {
    let fixture = Fixture::new();
    let (code, discovery) = fixture.run(&[
        "discover",
        "app",
        "--capability",
        "application.discover@3",
        "--max-applications",
        "8",
        "--max-processes",
        "1",
        "--max-windows",
        "1",
    ]);
    assert_eq!(code, 0);
    validate(
        include_str!("../../contracts/v3/application-inventory.schema.json"),
        &discovery,
    );
    assert_eq!(discovery["data"]["applicationsReturned"], 1);
    assert_eq!(
        discovery["data"]["applications"][0]["launchCapability"],
        "available-confirmed"
    );
    let session_id = discovery["data"]["applications"][0]["sessionId"]
        .as_str()
        .expect("fixture 必须发布 opaque 目标")
        .to_owned();

    let (code, assessment) = fixture.run(&[
        "assess",
        "app",
        "--capability",
        "application.open@2",
        "--target",
        &format!("sessionId={session_id}"),
    ]);
    assert_eq!(code, 0);
    assert_eq!(assessment["decision"], "confirmation-required");
    assert_eq!(assessment["executionRealm"], "host-foreground");
    assert_eq!(assessment["requiresForegroundConsent"], true);

    let (code, result) = fixture.run(&[
        "run",
        "app",
        "create",
        "--capability",
        "application.open@2",
        "--target",
        &format!("sessionId={session_id}"),
        "--input",
        fixture.input.to_str().expect("输入路径必须是 UTF-8"),
        "--confirm",
        "--allow-foreground",
    ]);
    assert_eq!(code, 0);
    validate(
        include_str!("../../contracts/v2/application-open-result.schema.json"),
        &result,
    );
    assert_eq!(result["shellUsed"], false);
    assert_eq!(result["argumentsCallerControlled"], false);

    fixture.write_entry("Replacement generation");
    let (code, stale) = fixture.run(&[
        "run",
        "app",
        "create",
        "--capability",
        "application.open@2",
        "--target",
        &format!("sessionId={session_id}"),
        "--input",
        fixture.input.to_str().expect("输入路径必须是 UTF-8"),
        "--confirm",
        "--allow-foreground",
    ]);
    assert_eq!(code, 2);
    assert_eq!(stale["error"]["code"], "STALE_SESSION");
}

#[test]
fn confirmation_and_foreground_consent_precede_input_file_access() {
    let fixture = Fixture::new();
    let missing = fixture.root.join("never-read.json");
    let missing = missing.to_str().expect("fixture 路径必须是 UTF-8");
    let (code, confirmation) = fixture.run(&["run", "app", "create", "--input", missing]);
    assert_eq!(code, 2);
    assert_eq!(confirmation["error"]["code"], "CONFIRMATION_REQUIRED");

    let (code, foreground) = fixture.run(&[
        "run",
        "app",
        "create",
        "--capability",
        "application.open@2",
        "--input",
        missing,
        "--confirm",
    ]);
    assert_eq!(code, 4);
    assert_eq!(foreground["error"]["code"], "FOREGROUND_CONSENT_REQUIRED");
}

#[test]
fn contracts_freeze_empty_input_and_fixture_only_security_scope() {
    validate(
        include_str!("../../contracts/v2/application-open-input.schema.json"),
        &json!({}),
    );
    let schema = serde_json::from_str::<Value>(include_str!(
        "../../contracts/v2/application-open-input.schema.json"
    ))
    .expect("输入 schema 必须解析");
    assert!(jsonschema::draft202012::validate(&schema, &json!({"argv": []})).is_err());
    let security = serde_json::from_str::<Value>(include_str!(
        "../contracts/linux-application-launch-security-v2.json"
    ))
    .expect("安全 manifest 必须解析");
    assert_eq!(security["dispatch"]["shell"], false);
    assert_eq!(security["dispatch"]["pathSearch"], false);
    assert_eq!(security["publicInput"]["path"], false);
    assert_eq!(
        security["scope"],
        "toolkit-owned-self-executable-fixture-only"
    );
}

#[test]
fn version_two_discovery_remains_launch_unavailable() {
    let fixture = Fixture::new();
    let (code, discovery) = fixture.run(&[
        "discover",
        "app",
        "--capability",
        "application.discover@2",
        "--max-applications",
        "8",
        "--max-processes",
        "1",
        "--max-windows",
        "1",
    ]);
    assert_eq!(code, 0);
    validate(
        include_str!("../../contracts/v2/application-inventory.schema.json"),
        &discovery,
    );
    assert!(
        discovery["data"]["applications"]
            .as_array()
            .is_some_and(|applications| applications
                .iter()
                .all(|application| application["launchCapability"] == "unavailable"))
    );
}

#[test]
fn ordinary_desktop_entry_remains_unavailable_and_is_never_dispatched() {
    let fixture = Fixture::new();
    fixture.write_untrusted_entry();
    let (code, discovery) = fixture.run(&[
        "discover",
        "app",
        "--capability",
        "application.discover@3",
        "--max-applications",
        "8",
        "--max-processes",
        "1",
        "--max-windows",
        "1",
    ]);
    assert_eq!(code, 0);
    let ordinary = discovery["data"]["applications"]
        .as_array()
        .and_then(|applications| {
            applications
                .iter()
                .find(|application| application["displayName"] == "Ordinary application")
        })
        .unwrap_or_else(|| panic!("普通应用必须出现在只读清单"));
    assert_eq!(ordinary["launchCapability"], "unavailable");
    let session_id = ordinary["sessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("普通应用必须保留 opaque 目标"));

    let (code, assessment) = fixture.run(&[
        "assess",
        "app",
        "--capability",
        "application.open@2",
        "--target",
        &format!("sessionId={session_id}"),
    ]);
    assert_eq!(code, 0);
    assert_eq!(assessment["decision"], "unavailable");

    let (code, rejected) = fixture.run(&[
        "run",
        "app",
        "create",
        "--capability",
        "application.open@2",
        "--target",
        &format!("sessionId={session_id}"),
        "--input",
        fixture.input.to_str().expect("输入路径必须是 UTF-8"),
        "--confirm",
        "--allow-foreground",
    ]);
    assert_eq!(code, 2);
    assert_eq!(rejected["error"]["code"], "CAPABILITY_UNAVAILABLE");
}

#[test]
fn capability_golden_keeps_linux_versions_separate_from_windows_v1() {
    let golden = serde_json::from_str::<Value>(include_str!(
        "../contracts/linux-application-launch-v2-capability-golden.json"
    ))
    .expect("capability golden 必须解析");
    assert_eq!(
        golden["capabilities"],
        json!(["application.discover@3", "application.open@2"])
    );
    assert_eq!(golden["policy"]["windowsV1SemanticsReused"], false);
}
