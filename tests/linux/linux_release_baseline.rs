#![cfg(all(target_os = "linux", feature = "linux-release-tools"))]

//! Linux Release System 的 hermetic archive / 安装生命周期回归。

use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tar::{Archive, EntryType};
use zstd::stream::read::Decoder;

const RELEASE_TOOL: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-linux-release");
const ARCHIVE_ROOT: &str = "ai-computer-toolkit-0.0.2-x86_64-unknown-linux-gnu";

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock must be after epoch")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("act-linux-release-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).expect("fixture root must be created");
        Self { root }
    }

    fn directory(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::create_dir_all(&path).expect("fixture directory must be created");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run(command: &mut Command) -> Output {
    command.output().expect("command must start")
}

fn success_json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout must be JSON")
}

fn build_archive(fixture: &Fixture, name: &str, epoch: u64) -> (PathBuf, PathBuf, Value) {
    let output = fixture.directory(&format!("{name}-out"));
    let target_name = if name == "c" {
        "a-target".to_owned()
    } else {
        format!("{name}-target")
    };
    let target = fixture.directory(&target_name);
    let mut command = Command::new(RELEASE_TOOL);
    command
        .args([
            "build",
            "--output-dir",
            output.to_str().unwrap(),
            "--target-dir",
            target.to_str().unwrap(),
            "--target",
            "x86_64-unknown-linux-gnu",
            "--source-date-epoch",
            &epoch.to_string(),
        ])
        .env("CARGO_NET_OFFLINE", "true");
    let result = success_json(run(&mut command));
    let archive = output.join(format!("{ARCHIVE_ROOT}.tar.zst"));
    let checksum = output.join(format!("{ARCHIVE_ROOT}.tar.zst.sha256"));
    (archive, checksum, result)
}

fn isolated(command: &mut Command, fixture: &Fixture) {
    let home = fixture.directory("home");
    let data = fixture.directory("xdg-data");
    let empty_data = fixture.directory("empty-xdg-data");
    let runtime = fixture.directory("empty-runtime");
    command
        .env_clear()
        .env("HOME", home)
        .env("XDG_DATA_HOME", data)
        .env("XDG_DATA_DIRS", empty_data)
        .env("XDG_RUNTIME_DIR", runtime)
        .env("XDG_CURRENT_DESKTOP", "KDE")
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8");
}

fn installed_command(fixture: &Fixture, arguments: &[&str]) -> Output {
    let executable = fixture.root.join("home/.local/bin/ai-computer-toolkit");
    let outside = fixture.directory("outside-repository");
    let mut command = Command::new(executable);
    command.current_dir(outside).args(arguments);
    isolated(&mut command, fixture);
    run(&mut command)
}

#[test]
fn reproducible_archive_and_managed_install_lifecycle_are_closed() {
    let fixture = Fixture::new();
    let (archive_a, checksum_a, build_a) = build_archive(&fixture, "a", 1_700_000_000);
    let (archive_b, _, build_b) = build_archive(&fixture, "b", 1_700_000_000);
    let bytes_a = fs::read(&archive_a).unwrap();
    let bytes_b = fs::read(&archive_b).unwrap();
    assert_eq!(
        bytes_a, bytes_b,
        "same inputs must produce identical archive bytes"
    );
    let digest = format!("{:x}", Sha256::digest(&bytes_a));
    assert_eq!(
        fs::read_to_string(&checksum_a).unwrap(),
        format!(
            "{digest}  {}\n",
            archive_a.file_name().unwrap().to_string_lossy()
        )
    );
    assert_eq!(build_a["publishable"], false);
    assert_eq!(build_b["publishable"], false);

    let mut archive = Archive::new(Decoder::new(bytes_a.as_slice()).unwrap());
    let mut names = BTreeSet::new();
    let mut manifest = None;
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        assert_eq!(entry.header().entry_type(), EntryType::Regular);
        assert_eq!(entry.header().uid().unwrap(), 0);
        assert_eq!(entry.header().gid().unwrap(), 0);
        let name = entry.path().unwrap().to_string_lossy().into_owned();
        if name.ends_with("/manifest.json") {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            manifest = Some(serde_json::from_slice::<Value>(&bytes).unwrap());
        }
        names.insert(name);
    }
    assert_eq!(
        names,
        BTreeSet::from([
            format!("{ARCHIVE_ROOT}/bin/ai-computer-toolkit"),
            format!("{ARCHIVE_ROOT}/manifest.json"),
            format!("{ARCHIVE_ROOT}/install.sh"),
            format!("{ARCHIVE_ROOT}/LICENSE"),
            format!("{ARCHIVE_ROOT}/THIRD_PARTY_NOTICES.txt"),
            format!("{ARCHIVE_ROOT}/SBOM.spdx.json"),
        ])
    );
    let manifest = manifest.expect("manifest must be present");
    let schema: Value = serde_json::from_str(include_str!(
        "../../contracts/v1/linux-release-archive.schema.json"
    ))
    .unwrap();
    jsonschema::draft202012::validate(&schema, &manifest).unwrap();
    assert_eq!(
        manifest["binaries"],
        serde_json::json!(["bin/ai-computer-toolkit"])
    );
    assert_eq!(manifest["companionBinaries"], serde_json::json!([]));
    assert_eq!(manifest["releaseEnvironmentVerified"], false);
    assert_eq!(manifest["maximumGlibc"], "2.39");
    assert_eq!(manifest["ubuntu2204Compatible"], false);
    assert_eq!(manifest["elfPolicy"]["positionIndependentExecutable"], true);
    assert_eq!(manifest["elfPolicy"]["nonExecutableStack"], true);
    assert_eq!(manifest["elfPolicy"]["relro"], true);
    assert_eq!(manifest["elfPolicy"]["bindNow"], true);
    assert_eq!(manifest["elfPolicy"]["maximumGlibc"], "2.39");
    assert!(manifest["sbomPackageCount"].as_u64().unwrap() > 1);

    let extracted = fixture.directory("extracted-archive");
    Archive::new(Decoder::new(bytes_a.as_slice()).unwrap())
        .unpack(&extracted)
        .unwrap();
    let archive_installer = extracted.join(ARCHIVE_ROOT).join("install.sh");
    assert_eq!(
        fs::metadata(&archive_installer)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    let sbom: Value = serde_json::from_slice(
        &fs::read(extracted.join(ARCHIVE_ROOT).join("SBOM.spdx.json")).unwrap(),
    )
    .unwrap();
    let sbom_schema: Value = serde_json::from_str(include_str!(
        "../../contracts/release/spdx-2.3-release-subset.schema.json"
    ))
    .unwrap();
    jsonschema::draft202012::validate(&sbom_schema, &sbom).unwrap();
    assert_eq!(sbom["spdxVersion"], "SPDX-2.3");
    assert_eq!(sbom["dataLicense"], "CC0-1.0");
    assert_eq!(
        sbom["packages"].as_array().unwrap().len() as u64,
        manifest["sbomPackageCount"].as_u64().unwrap()
    );
    for package in sbom["packages"].as_array().unwrap() {
        assert!(
            !package["versionInfo"]
                .as_str()
                .unwrap()
                .contains("(proc-macro)")
        );
        assert_eq!(package["checksums"][0]["algorithm"], "SHA256");
        let license = package["licenseDeclared"].as_str().unwrap();
        assert!(license == "NOASSERTION" || spdx::Expression::parse(license).is_ok());
    }
    let notices =
        fs::read_to_string(extracted.join(ARCHIVE_ROOT).join("THIRD_PARTY_NOTICES.txt")).unwrap();
    assert!(notices.contains("zstd "));
    assert!(!notices.contains("(proc-macro)"));

    let mut install = Command::new(&archive_installer);
    install.args([
        "install",
        "--archive",
        archive_a.to_str().unwrap(),
        "--checksum",
        checksum_a.to_str().unwrap(),
    ]);
    isolated(&mut install, &fixture);
    let installed = success_json(run(&mut install));
    assert_eq!(installed["rollbackAvailable"], false);
    let launcher = fixture.root.join("home/.local/bin/ai-computer-toolkit");
    assert!(
        fs::symlink_metadata(&launcher)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_link(&launcher).unwrap(),
        fixture
            .root
            .join("xdg-data/ai-computer-toolkit/current/bin/ai-computer-toolkit")
    );

    for arguments in [
        vec!["version", "--pretty"],
        vec!["build-info", "--pretty"],
        vec!["status", "process", "--pretty"],
        vec!["capabilities", "--pretty"],
        vec![
            "discover",
            "app",
            "--max-applications",
            "1",
            "--max-processes",
            "1",
            "--max-windows",
            "1",
            "--pretty",
        ],
    ] {
        let value = success_json(installed_command(&fixture, &arguments));
        assert_eq!(value["ok"], true);
    }
    let build_info = success_json(installed_command(&fixture, &["build-info", "--pretty"]));
    let build_info_text = serde_json::to_string(&build_info).unwrap();
    assert!(!build_info_text.contains(env!("CARGO_MANIFEST_DIR")));
    assert!(!build_info_text.contains(fixture.root.to_str().unwrap()));
    let sessions = success_json(installed_command(
        &fixture,
        &["sessions", "process", "--max-items", "16", "--pretty"],
    ));
    let process_id = sessions["sessions"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["sessionId"].as_str())
        .expect("at least the installed CLI process must be observed")
        .to_owned();
    assert_eq!(
        success_json(installed_command(
            &fixture,
            &[
                "inspect",
                "process",
                "--target",
                &format!("sessionId={process_id}"),
                "--pretty",
            ],
        ))["capability"],
        "process.metadata.read@1"
    );

    let discovery = success_json(installed_command(
        &fixture,
        &[
            "discover",
            "app",
            "--max-applications",
            "1",
            "--max-processes",
            "1",
            "--max-windows",
            "1",
        ],
    ));
    let host = discovery["data"]["hostTargetId"].as_str().unwrap();
    for capability in ["accessibility.tree.read@1", "browser.screenshot@1"] {
        let assessment = success_json(installed_command(
            &fixture,
            &[
                "assess",
                "app",
                "--capability",
                capability,
                "--target",
                &format!("sessionId={host}"),
            ],
        ));
        assert_eq!(assessment["decision"], "unavailable");
        assert_eq!(assessment["executionRealm"], "none");
        assert_eq!(assessment["constraints"]["noFallback"], true);
    }
    let screenshot = success_json(installed_command(
        &fixture,
        &[
            "assess",
            "app",
            "--capability",
            "desktop.screenshot.interactive@1",
            "--target",
            &format!("sessionId={host}"),
        ],
    ));
    match screenshot["decision"].as_str() {
        Some("unavailable") => assert_eq!(screenshot["executionRealm"], "none"),
        Some("foreground-consent-required") => {
            assert_eq!(screenshot["executionRealm"], "host-foreground");
            assert_eq!(screenshot["requiresConfirmation"], true);
            assert_eq!(screenshot["requiresForegroundConsent"], true);
        }
        other => panic!("unexpected screenshot assessment: {other:?}"),
    }
    let unknown = installed_command(
        &fixture,
        &[
            "assess",
            "app",
            "--capability",
            "unknown.future@99",
            "--target",
            "sessionId=invalid",
        ],
    );
    assert!(!unknown.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&unknown.stdout).unwrap()["error"]["code"],
        "INVALID_ARGUMENT"
    );

    let release_bin = fixture
        .root
        .join("xdg-data/ai-computer-toolkit/current/bin");
    assert_eq!(
        fs::read_dir(&release_bin).unwrap().count(),
        1,
        "no worker or fixture may be installed"
    );
    assert_eq!(
        fs::metadata(release_bin.join("ai-computer-toolkit"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    let binary = release_bin.join("ai-computer-toolkit");
    let elf_header = run(Command::new("/usr/bin/readelf").args(["-h", binary.to_str().unwrap()]));
    assert!(elf_header.status.success());
    assert!(
        String::from_utf8_lossy(&elf_header.stdout)
            .contains("DYN (Position-Independent Executable")
    );
    let elf_program =
        run(Command::new("/usr/bin/readelf").args(["-W", "-l", binary.to_str().unwrap()]));
    let program_text = String::from_utf8_lossy(&elf_program.stdout);
    assert!(program_text.contains("GNU_RELRO"));
    let stack = program_text
        .lines()
        .find(|line| line.contains("GNU_STACK"))
        .expect("GNU_STACK header must exist");
    assert!(!stack.contains("RWE"), "GNU stack must not be executable");
    let elf_dynamic = run(Command::new("/usr/bin/readelf").args(["-d", binary.to_str().unwrap()]));
    let dynamic_text = String::from_utf8_lossy(&elf_dynamic.stdout);
    assert!(dynamic_text.contains("BIND_NOW"));
    assert!(dynamic_text.contains("Flags: NOW PIE"));
    let needed = dynamic_text
        .lines()
        .filter(|line| line.contains("(NEEDED)"))
        .map(|line| line.split('[').nth(1).unwrap().split(']').next().unwrap())
        .collect::<BTreeSet<_>>();
    assert!(
        needed.is_subset(&BTreeSet::from([
            "libgcc_s.so.1",
            "libc.so.6",
            "ld-linux-x86-64.so.2",
        ])),
        "DT_NEEDED contains an unreviewed library: {needed:?}"
    );
    let strings = run(Command::new("/usr/bin/strings").arg(&binary));
    let strings = String::from_utf8_lossy(&strings.stdout);
    assert!(!strings.contains(env!("CARGO_MANIFEST_DIR")));
    assert!(!strings.contains("target/debug"));
    assert!(!strings.contains("target/release"));

    let bad_checksum = fixture.root.join("bad.sha256");
    fs::write(
        &bad_checksum,
        format!(
            "{}  {}\n",
            "0".repeat(64),
            archive_a.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();
    let before = fs::read_link(fixture.root.join("xdg-data/ai-computer-toolkit/current")).unwrap();
    let installed_installer = fixture
        .root
        .join("xdg-data/ai-computer-toolkit/current/install.sh");
    let mut failed_upgrade = Command::new(&installed_installer);
    failed_upgrade.args([
        "install",
        "--archive",
        archive_a.to_str().unwrap(),
        "--checksum",
        bad_checksum.to_str().unwrap(),
    ]);
    isolated(&mut failed_upgrade, &fixture);
    assert!(!run(&mut failed_upgrade).status.success());
    assert_eq!(
        fs::read_link(fixture.root.join("xdg-data/ai-computer-toolkit/current")).unwrap(),
        before,
        "failed upgrade must preserve current"
    );

    let (archive_c, checksum_c, _) = build_archive(&fixture, "c", 1_700_000_001);
    let mut upgrade = Command::new(&installed_installer);
    upgrade.args([
        "install",
        "--archive",
        archive_c.to_str().unwrap(),
        "--checksum",
        checksum_c.to_str().unwrap(),
    ]);
    isolated(&mut upgrade, &fixture);
    assert_eq!(success_json(run(&mut upgrade))["rollbackAvailable"], true);
    let mut rollback = Command::new(&installed_installer);
    rollback.arg("rollback");
    isolated(&mut rollback, &fixture);
    assert_eq!(success_json(run(&mut rollback))["operation"], "rollback");

    let mut uninstall = Command::new(&installed_installer);
    uninstall.arg("uninstall");
    isolated(&mut uninstall, &fixture);
    assert_eq!(success_json(run(&mut uninstall))["removed"], true);
    assert!(!launcher.exists());
    assert!(!fixture.root.join("xdg-data/ai-computer-toolkit").exists());

    let mut relative = Command::new(&archive_installer);
    relative.args([
        "install",
        "--archive",
        archive_a.to_str().unwrap(),
        "--checksum",
        checksum_a.to_str().unwrap(),
    ]);
    relative
        .env("HOME", fixture.directory("relative-home"))
        .env("XDG_DATA_HOME", "relative-data");
    assert!(!run(&mut relative).status.success());

    let home = fixture.directory("collision-home");
    fs::create_dir(home.join(".local")).unwrap();
    fs::create_dir(home.join(".local/bin")).unwrap();
    fs::write(home.join(".local/bin/ai-computer-toolkit"), b"owner-data").unwrap();
    let data = fixture.directory("collision-data");
    let mut collision = Command::new(&archive_installer);
    collision
        .args([
            "install",
            "--archive",
            archive_a.to_str().unwrap(),
            "--checksum",
            checksum_a.to_str().unwrap(),
        ])
        .env("HOME", &home)
        .env("XDG_DATA_HOME", &data);
    assert!(!run(&mut collision).status.success());
    assert_eq!(
        fs::read(home.join(".local/bin/ai-computer-toolkit")).unwrap(),
        b"owner-data"
    );
    assert!(!data.join("ai-computer-toolkit/current").exists());
}
