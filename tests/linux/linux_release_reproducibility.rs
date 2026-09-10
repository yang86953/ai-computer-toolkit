#![cfg(all(target_os = "linux", feature = "linux-release-tools"))]

//! 两份不同绝对路径、独立 HOME/target 的固定 Git tree 可复现发布回归。

use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use tar::Archive;

const ARCHIVE_NAME: &str = "ai-computer-toolkit-0.0.1-x86_64-unknown-linux-gnu.tar.zst";
const ROOT: &str = "ai-computer-toolkit-0.0.1-x86_64-unknown-linux-gnu";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock must be after epoch")
            .as_nanos();
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/linux-release-repro-fixtures")
            .join(format!("{}-{nonce}", std::process::id()));
        fs::create_dir_all(path.parent().unwrap()).expect("repro fixture parent must be created");
        fs::create_dir(&path).expect("repro fixture must be created");
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Product {
    archive: Vec<u8>,
    checksum: Vec<u8>,
    members: BTreeMap<String, Vec<u8>>,
}

#[test]
fn fixed_tree_two_absolute_roots_and_targets_are_byte_reproducible() {
    let fixture = Fixture::new();
    let revision = fixed_tree_revision();
    let left = build_export(&fixture.0, "left-deep/export", &revision);
    let right = build_export(&fixture.0, "right/another/deeper/export", &revision);
    assert_eq!(left.archive, right.archive, "archive bytes differ");
    assert_eq!(left.checksum, right.checksum, "external SHA differs");
    for member in [
        format!("{ROOT}/bin/ai-computer-toolkit"),
        format!("{ROOT}/manifest.json"),
        format!("{ROOT}/SBOM.spdx.json"),
    ] {
        assert_eq!(
            left.members.get(&member),
            right.members.get(&member),
            "reproducible member differs: {member}"
        );
    }
}

fn fixed_tree_revision() -> String {
    let candidate = Command::new("git")
        .args(["stash", "create", "linux-release-reproducibility-test"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("git stash create must start");
    assert!(candidate.status.success(), "git fixed tree creation failed");
    let candidate = String::from_utf8(candidate.stdout)
        .expect("git revision must be UTF-8")
        .trim()
        .to_owned();
    let revision = if candidate.is_empty() {
        "HEAD".to_owned()
    } else {
        candidate
    };
    let resolved = Command::new("git")
        .args(["rev-parse", &revision])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("git rev-parse must start");
    assert!(resolved.status.success());
    String::from_utf8(resolved.stdout)
        .expect("git fixed revision must be UTF-8")
        .trim()
        .to_owned()
}

fn build_export(fixture: &Path, relative: &str, revision: &str) -> Product {
    let source = fixture.join(relative);
    fs::create_dir_all(&source).expect("source export root must be created");
    let archive = Command::new("git")
        .args(["archive", "--format=tar", revision])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("git archive must start");
    assert!(archive.status.success(), "git archive failed");
    Archive::new(archive.stdout.as_slice())
        .unpack(&source)
        .expect("fixed tree must extract");
    let home = fixture.join(format!("{relative}-home"));
    let tool_target = fixture.join(format!("{relative}-tool-target"));
    let release_target = fixture.join(format!("{relative}-release-target"));
    let output = fixture.join(format!("{relative}-output"));
    for path in [&home, &tool_target, &release_target, &output] {
        fs::create_dir_all(path).expect("independent build path must be created");
    }
    let cargo_home = std::env::var_os("CARGO_HOME").unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap())
            .join(".cargo")
            .into()
    });
    let tool = Command::new(env!("CARGO"))
        .current_dir(&source)
        .args([
            "build",
            "--frozen",
            "--offline",
            "--features",
            "linux-release-tools",
            "--bin",
            "ai-computer-toolkit-linux-release",
            "--target-dir",
            tool_target.to_str().unwrap(),
        ])
        .env("HOME", &home)
        .env("CARGO_HOME", &cargo_home)
        .env("CARGO_INCREMENTAL", "0")
        .output()
        .expect("independent release tool build must start");
    assert!(
        tool.status.success(),
        "release tool build failed: {}",
        String::from_utf8_lossy(&tool.stderr)
    );
    let release_tool = tool_target
        .join("debug")
        .join("ai-computer-toolkit-linux-release");
    let build = Command::new(release_tool)
        .args([
            "build",
            "--output-dir",
            output.to_str().unwrap(),
            "--target-dir",
            release_target.to_str().unwrap(),
            "--target",
            "x86_64-unknown-linux-gnu",
            "--source-date-epoch",
            "1700000000",
        ])
        .env("HOME", &home)
        .env("CARGO_HOME", &cargo_home)
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .expect("independent release build must start");
    assert!(
        build.status.success(),
        "independent release build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let archive_path = output.join(ARCHIVE_NAME);
    let archive = fs::read(&archive_path).expect("release archive must be readable");
    let checksum = fs::read(output.join(format!("{ARCHIVE_NAME}.sha256")))
        .expect("release checksum must be readable");
    let mut members = BTreeMap::new();
    let mut tar = Archive::new(zstd::stream::read::Decoder::new(archive.as_slice()).unwrap());
    for entry in tar.entries().expect("release members must be readable") {
        let mut entry = entry.expect("release member must be readable");
        let name = entry
            .path()
            .expect("release member path must be readable")
            .to_string_lossy()
            .into_owned();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .expect("release member must be readable");
        members.insert(name, bytes);
    }
    Product {
        archive,
        checksum,
        members,
    }
}
