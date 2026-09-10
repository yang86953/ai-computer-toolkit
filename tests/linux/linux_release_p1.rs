#![cfg(all(target_os = "linux", feature = "linux-release-tools"))]

//! Linux Release System 的 P1 对抗、事务恢复与互斥回归。

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use tar::Archive;

const RELEASE_TOOL: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-linux-release");
const ARCHIVE_NAME: &str = "ai-computer-toolkit-0.0.2-x86_64-unknown-linux-gnu.tar.zst";
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
        let root = std::env::temp_dir().join(format!(
            "act-linux-release-p1-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("fixture root must be created");
        Self { root }
    }

    fn directory(&self, relative: impl AsRef<Path>) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(&path).expect("fixture directory must be created");
        path
    }

    fn environment(&self, name: &str) -> Layout {
        Layout {
            home: self.directory(format!("layouts/{name}/home")),
            data: self.directory(format!("layouts/{name}/data")),
            empty_data: self.directory(format!("layouts/{name}/empty-data")),
            runtime: self.directory(format!("layouts/{name}/runtime")),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Clone)]
struct Layout {
    home: PathBuf,
    data: PathBuf,
    empty_data: PathBuf,
    runtime: PathBuf,
}

impl Layout {
    fn isolate(&self, command: &mut Command) {
        command
            .env_clear()
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", &self.data)
            .env("XDG_DATA_DIRS", &self.empty_data)
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("XDG_CURRENT_DESKTOP", "KDE")
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8");
    }

    fn prefix(&self) -> PathBuf {
        self.data.join("ai-computer-toolkit")
    }

    fn launcher(&self) -> PathBuf {
        self.home.join(".local/bin/ai-computer-toolkit")
    }
}

fn run(command: &mut Command) -> Output {
    command.output().expect("release command must start")
}

fn build_archive(fixture: &Fixture) -> (PathBuf, PathBuf, PathBuf) {
    let output = fixture.directory("release/output");
    let target = fixture.directory("release/target");
    let result = run(Command::new(RELEASE_TOOL).args([
        "build",
        "--output-dir",
        output.to_str().unwrap(),
        "--target-dir",
        target.to_str().unwrap(),
        "--target",
        "x86_64-unknown-linux-gnu",
        "--source-date-epoch",
        "1700000000",
    ]));
    assert!(
        result.status.success(),
        "archive build failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let archive = output.join(ARCHIVE_NAME);
    let checksum = output.join(format!("{ARCHIVE_NAME}.sha256"));
    let extracted = fixture.directory("release/extracted");
    let bytes = fs::read(&archive).expect("archive must be readable");
    Archive::new(zstd::stream::read::Decoder::new(bytes.as_slice()).unwrap())
        .unpack(&extracted)
        .expect("archive fixture must extract");
    let installer = extracted.join(ARCHIVE_ROOT).join("install.sh");
    (archive, checksum, installer)
}

fn checksum_for(path: &Path, bytes: &[u8]) -> PathBuf {
    fs::write(path, bytes).expect("attack archive must be written");
    let checksum = path.with_extension("sha256");
    fs::write(
        &checksum,
        format!(
            "{:x}  {}\n",
            Sha256::digest(bytes),
            path.file_name().unwrap().to_string_lossy()
        ),
    )
    .expect("attack checksum must be written");
    checksum
}

fn install_command(installer: &Path, archive: &Path, checksum: &Path, layout: &Layout) -> Command {
    let mut command = Command::new(installer);
    command.args([
        "install",
        "--archive",
        archive.to_str().unwrap(),
        "--checksum",
        checksum.to_str().unwrap(),
    ]);
    layout.isolate(&mut command);
    command
}

fn assert_rejected_without_install_root(output: &Output, layout: &Layout, fixture: &Fixture) {
    assert!(!output.status.success());
    assert!(
        !layout.prefix().exists(),
        "failed input mutated install root"
    );
    assert!(!layout.launcher().exists(), "failed input created launcher");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains(fixture.root.to_str().unwrap()));
}

#[test]
fn archive_exhaustion_tamper_lock_and_crash_recovery_are_closed() {
    let fixture = Fixture::new();
    let (archive, checksum, installer) = build_archive(&fixture);
    let valid = fs::read(&archive).expect("valid archive must be readable");

    let bad_lock_layout = fixture.environment("bad-lock");
    let bad_lock = bad_lock_layout
        .data
        .join(".ai-computer-toolkit.install.lock");
    fs::write(&bad_lock, b"lock").unwrap();
    fs::set_permissions(&bad_lock, fs::Permissions::from_mode(0o644)).unwrap();
    assert_rejected_without_install_root(
        &run(&mut install_command(
            &installer,
            &archive,
            &checksum,
            &bad_lock_layout,
        )),
        &bad_lock_layout,
        &fixture,
    );
    let link_lock_layout = fixture.environment("link-lock");
    symlink(
        fixture.directory("link-lock/outside"),
        link_lock_layout
            .data
            .join(".ai-computer-toolkit.install.lock"),
    )
    .unwrap();
    assert_rejected_without_install_root(
        &run(&mut install_command(
            &installer,
            &archive,
            &checksum,
            &link_lock_layout,
        )),
        &link_lock_layout,
        &fixture,
    );
    let bad_journal_layout = fixture.environment("bad-journal");
    let bad_journal = bad_journal_layout
        .data
        .join(".ai-computer-toolkit.transaction-v1.json");
    fs::write(&bad_journal, br#"{"state":"unknown"}"#).unwrap();
    fs::set_permissions(&bad_journal, fs::Permissions::from_mode(0o600)).unwrap();
    assert_rejected_without_install_root(
        &run(&mut install_command(
            &installer,
            &archive,
            &checksum,
            &bad_journal_layout,
        )),
        &bad_journal_layout,
        &fixture,
    );

    let mut trailing = valid.clone();
    trailing.extend_from_slice(b"audit-tail-16byt");
    let trailing_path = fixture.root.join("trailing.tar.zst");
    let trailing_checksum = checksum_for(&trailing_path, &trailing);
    let trailing_layout = fixture.environment("trailing");
    assert_rejected_without_install_root(
        &run(&mut install_command(
            &installer,
            &trailing_path,
            &trailing_checksum,
            &trailing_layout,
        )),
        &trailing_layout,
        &fixture,
    );

    let mut concatenated = valid.clone();
    concatenated.extend_from_slice(&valid);
    let concatenated_path = fixture.root.join("concatenated.tar.zst");
    let concatenated_checksum = checksum_for(&concatenated_path, &concatenated);
    let concatenated_layout = fixture.environment("concatenated");
    assert_rejected_without_install_root(
        &run(&mut install_command(
            &installer,
            &concatenated_path,
            &concatenated_checksum,
            &concatenated_layout,
        )),
        &concatenated_layout,
        &fixture,
    );

    let mut tar_tail = zstd::stream::decode_all(valid.as_slice()).expect("tar must decode");
    tar_tail.extend_from_slice(&[0_u8; 512]);
    let tar_tail = zstd::stream::encode_all(tar_tail.as_slice(), 19).expect("tar must encode");
    let tar_tail_path = fixture.root.join("tar-tail.tar.zst");
    let tar_tail_checksum = checksum_for(&tar_tail_path, &tar_tail);
    let tar_tail_layout = fixture.environment("tar-tail");
    assert_rejected_without_install_root(
        &run(&mut install_command(
            &installer,
            &tar_tail_path,
            &tar_tail_checksum,
            &tar_tail_layout,
        )),
        &tar_tail_layout,
        &fixture,
    );

    let tamper_layout = fixture.environment("tamper");
    assert!(
        run(&mut install_command(
            &installer,
            &archive,
            &checksum,
            &tamper_layout,
        ))
        .status
        .success()
    );
    let current = tamper_layout.prefix().join("current");
    let original_target = fs::read_link(&current).expect("current must be a symlink");
    let physical_installer = tamper_layout
        .prefix()
        .join(&original_target)
        .join("install.sh");
    let release = tamper_layout.prefix().join(&original_target);
    fs::remove_file(&current).expect("current fixture must be replaceable");
    let outside = fixture.directory("tamper/outside");
    symlink(&outside, &current).expect("absolute tamper link must be created");
    let mut uninstall = Command::new(&physical_installer);
    uninstall.arg("uninstall");
    tamper_layout.isolate(&mut uninstall);
    assert!(!run(&mut uninstall).status.success());
    assert_eq!(fs::read_link(&current).unwrap(), outside);
    assert!(release.exists(), "tampered uninstall removed a release");
    assert!(
        fs::symlink_metadata(tamper_layout.launcher()).is_ok(),
        "tampered uninstall removed launcher"
    );
    fs::remove_file(&current).unwrap();
    symlink(&original_target, &current).unwrap();

    fs::remove_file(&current).unwrap();
    symlink("releases/unknown-release", &current).unwrap();
    let mut unknown_release = Command::new(&physical_installer);
    unknown_release.arg("uninstall");
    tamper_layout.isolate(&mut unknown_release);
    assert!(!run(&mut unknown_release).status.success());
    assert!(release.exists());
    assert_eq!(
        fs::read_link(&current).unwrap(),
        PathBuf::from("releases/unknown-release")
    );
    fs::remove_file(&current).unwrap();
    symlink(&original_target, &current).unwrap();

    let rollback_link = tamper_layout.prefix().join("rollback");
    symlink(&outside, &rollback_link).unwrap();
    let mut rollback_tamper = Command::new(&physical_installer);
    rollback_tamper.arg("uninstall");
    tamper_layout.isolate(&mut rollback_tamper);
    assert!(!run(&mut rollback_tamper).status.success());
    assert!(release.exists());
    assert_eq!(fs::read_link(&rollback_link).unwrap(), outside);
    fs::remove_file(&rollback_link).unwrap();

    fs::remove_file(tamper_layout.launcher()).unwrap();
    fs::write(tamper_layout.launcher(), b"owner launcher").unwrap();
    let mut launcher_tamper = Command::new(&physical_installer);
    launcher_tamper.arg("uninstall");
    tamper_layout.isolate(&mut launcher_tamper);
    assert!(!run(&mut launcher_tamper).status.success());
    assert_eq!(
        fs::read(tamper_layout.launcher()).unwrap(),
        b"owner launcher"
    );
    assert!(release.exists());
    fs::remove_file(tamper_layout.launcher()).unwrap();
    symlink(
        tamper_layout
            .prefix()
            .join("current/bin/ai-computer-toolkit"),
        tamper_layout.launcher(),
    )
    .unwrap();

    let unknown = release.join("unknown-file");
    fs::write(&unknown, b"unknown").unwrap();
    let mut release_tamper = Command::new(&physical_installer);
    release_tamper.arg("uninstall");
    tamper_layout.isolate(&mut release_tamper);
    assert!(!run(&mut release_tamper).status.success());
    assert!(unknown.exists());
    fs::remove_file(&unknown).unwrap();

    let binary = release.join("bin/ai-computer-toolkit");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    let mut mode_tamper = Command::new(&physical_installer);
    mode_tamper.arg("uninstall");
    tamper_layout.isolate(&mut mode_tamper);
    assert!(!run(&mut mode_tamper).status.success());
    assert!(release.exists());
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();

    let license = release.join("LICENSE");
    let hardlink = release.join("LICENSE.hardlink");
    fs::hard_link(&license, &hardlink).unwrap();
    let mut hardlink_tamper = Command::new(&physical_installer);
    hardlink_tamper.arg("uninstall");
    tamper_layout.isolate(&mut hardlink_tamper);
    assert!(!run(&mut hardlink_tamper).status.success());
    assert!(hardlink.exists());
    fs::remove_file(&hardlink).unwrap();

    let manifest_path = release.join("manifest.json");
    let manifest_bytes = fs::read(&manifest_path).unwrap();
    fs::write(&manifest_path, b"{}").unwrap();
    let mut manifest_tamper = Command::new(&physical_installer);
    manifest_tamper.arg("uninstall");
    tamper_layout.isolate(&mut manifest_tamper);
    assert!(!run(&mut manifest_tamper).status.success());
    assert!(release.exists());
    fs::write(&manifest_path, manifest_bytes).unwrap();

    let mut first = install_command(&physical_installer, &archive, &checksum, &tamper_layout);
    first.env("ACT_RELEASE_TEST_HOLD_LOCK_MS", "750");
    let mut first = first
        .spawn()
        .expect("first concurrent installer must start");
    thread::sleep(Duration::from_millis(100));
    let second = run(&mut install_command(
        &physical_installer,
        &archive,
        &checksum,
        &tamper_layout,
    ));
    assert!(
        !second.status.success(),
        "concurrent operation was not rejected"
    );
    assert!(first.wait().expect("first installer must finish").success());

    let crash_points = [
        "journal:before-write",
        "journal:after-write",
        "journal:before-chmod",
        "journal:after-chmod",
        "journal:before-file-fsync",
        "journal:after-file-fsync",
        "journal:before-rename",
        "journal:after-rename",
        "journal-parent:before-dir-fsync",
        "journal-parent:after-dir-fsync",
        "install prefix:before-mkdir",
        "install prefix:after-mkdir",
        "install prefix:before-chmod",
        "install prefix:after-chmod",
        "install prefix-self:before-dir-fsync",
        "install prefix-self:after-dir-fsync",
        "install prefix-parent:before-dir-fsync",
        "install prefix-parent:after-dir-fsync",
        "install-marker:before-write",
        "install-marker:after-write",
        "install staging:before-mkdir",
        "install staging:after-mkdir",
        "install staging:before-chmod",
        "install staging:after-chmod",
        "install staging-self:before-dir-fsync",
        "install staging-self:after-dir-fsync",
        "install staging-parent:before-dir-fsync",
        "install staging-parent:after-dir-fsync",
        "install staging bin:before-mkdir",
        "install staging bin:after-mkdir",
        "install staging bin:before-chmod",
        "install staging bin:after-chmod",
        "install staging bin-self:before-dir-fsync",
        "install staging bin-self:after-dir-fsync",
        "install staging bin-parent:before-dir-fsync",
        "install staging bin-parent:after-dir-fsync",
        "payload-ai-computer-toolkit:before-write",
        "payload-ai-computer-toolkit:after-write",
        "payload-ai-computer-toolkit:before-chmod",
        "payload-ai-computer-toolkit:after-chmod",
        "payload-ai-computer-toolkit:before-file-fsync",
        "payload-ai-computer-toolkit:after-file-fsync",
        "payload-ai-computer-toolkit:before-rename",
        "payload-ai-computer-toolkit:after-rename",
        "payload-ai-computer-toolkit-parent:before-dir-fsync",
        "payload-ai-computer-toolkit-parent:after-dir-fsync",
        "release:before-rename",
        "release:after-rename",
        "releases-after-release-rename:before-dir-fsync",
        "releases-after-release-rename:after-dir-fsync",
        "current:before-symlink",
        "current:after-symlink",
        "current:before-rename",
        "current:after-rename",
        "current-parent:before-dir-fsync",
        "current-parent:after-dir-fsync",
        "launcher:before-symlink",
        "launcher:after-symlink",
        "launcher:before-rename",
        "launcher:after-rename",
        "launcher-parent:before-dir-fsync",
        "launcher-parent:after-dir-fsync",
        "journal:before-remove",
        "journal:after-remove",
    ];
    for (index, point) in crash_points.iter().enumerate() {
        let layout = fixture.environment(&format!("crash-{index}"));
        let mut crash = install_command(&installer, &archive, &checksum, &layout);
        crash.env("ACT_RELEASE_TEST_CRASH_AT", point);
        let crashed = run(&mut crash);
        assert_eq!(
            crashed.status.code(),
            Some(86),
            "checkpoint was not hit: {point}"
        );
        let recovered = run(&mut install_command(
            &installer, &archive, &checksum, &layout,
        ));
        assert!(
            recovered.status.success(),
            "recovery failed at {point}: {}",
            String::from_utf8_lossy(&recovered.stderr)
        );
        assert!(layout.prefix().join("current").exists());
        assert!(layout.launcher().exists());
        assert!(
            !layout
                .data
                .join(".ai-computer-toolkit.transaction-v1.json")
                .exists()
        );
    }

    let uninstall_crash_points = [
        "uninstall-launcher:before-rename",
        "uninstall-launcher:after-rename",
        "uninstall-launcher-parent:before-dir-fsync",
        "uninstall-launcher-parent:after-dir-fsync",
        "uninstall-root:before-rename",
        "uninstall-root:after-rename",
        "uninstall-root-parent:before-dir-fsync",
        "uninstall-root-parent:after-dir-fsync",
        "uninstall-delete:before-release",
        "uninstall-delete:after-release",
        "uninstall-delete-parent:before-dir-fsync",
        "uninstall-delete-parent:after-dir-fsync",
        "uninstall-launcher-delete-parent:before-dir-fsync",
        "uninstall-launcher-delete-parent:after-dir-fsync",
    ];
    for (index, point) in uninstall_crash_points.iter().enumerate() {
        let layout = fixture.environment(&format!("uninstall-crash-{index}"));
        assert!(
            run(&mut install_command(
                &installer, &archive, &checksum, &layout
            ))
            .status
            .success()
        );
        let mut crash = Command::new(&installer);
        crash.arg("uninstall");
        layout.isolate(&mut crash);
        crash.env("ACT_RELEASE_TEST_CRASH_AT", point);
        let crashed = run(&mut crash);
        assert_eq!(
            crashed.status.code(),
            Some(86),
            "uninstall checkpoint was not hit: {point}"
        );
        let mut recover = Command::new(&installer);
        recover.arg("uninstall");
        layout.isolate(&mut recover);
        let recovered = run(&mut recover);
        assert!(
            recovered.status.success(),
            "uninstall recovery failed at {point}: {}",
            String::from_utf8_lossy(&recovered.stderr)
        );
        assert!(!layout.prefix().exists());
        assert!(fs::symlink_metadata(layout.launcher()).is_err());
        assert!(
            !layout
                .data
                .join(".ai-computer-toolkit.transaction-v1.json")
                .exists()
        );
    }
}
