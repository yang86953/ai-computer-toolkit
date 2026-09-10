//! Linux 可复现 archive 与用户级版本化安装 Release System。

mod archive;
mod elf;
mod install;
mod sbom;

use std::{
    ffi::OsStr,
    fs::{self, File},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

#[cfg(feature = "linux-release-tools")]
use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

pub(super) const CONTRACT_VERSION: &str = "act/linux-release-archive/v1";
pub(super) const INSTALL_MARKER: &str = "act/linux-user-install/v1\n";
pub(super) const PACKAGE_NAME: &str = "ai-computer-toolkit";
pub(super) const MAIN_BINARY: &str = "bin/ai-computer-toolkit";
pub(super) const INSTALL_SCRIPT: &str = "install.sh";
pub(super) const LICENSE_FILE: &str = "LICENSE";
pub(super) const NOTICE_FILE: &str = "THIRD_PARTY_NOTICES.txt";
pub(super) const SBOM_FILE: &str = "SBOM.spdx.json";
pub(super) const MANIFEST_FILE: &str = "manifest.json";
pub(super) const MAXIMUM_ARCHIVE_ENTRIES: usize = 16;
pub(super) const MAXIMUM_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;

pub(super) type ReleaseResult<T> = Result<T, String>;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PayloadFile {
    pub(super) path: String,
    pub(super) role: String,
    pub(super) mode: u32,
    pub(super) sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SiblingPolicy {
    pub(super) resolution: String,
    pub(super) path_fallback: bool,
    pub(super) executable_override: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ArchiveManifest {
    pub(super) contract_version: String,
    pub(super) package_name: String,
    pub(super) version: String,
    pub(super) target: String,
    pub(super) source_date_epoch: u64,
    pub(super) archive_root: String,
    pub(super) layout: String,
    pub(super) binaries: Vec<String>,
    pub(super) companion_binaries: Vec<String>,
    pub(super) sibling_policy: SiblingPolicy,
    pub(super) elf_needed: Vec<String>,
    pub(super) maximum_glibc: String,
    pub(super) ubuntu_2204_compatible: bool,
    pub(super) release_environment_verified: bool,
    pub(super) elf_policy: elf::ElfPolicyEvidence,
    pub(super) sbom_package_count: usize,
    pub(super) files: Vec<PayloadFile>,
}

pub(super) struct BufferedEntry {
    pub(super) path: String,
    pub(super) mode: u32,
    pub(super) uid: u64,
    pub(super) gid: u64,
    pub(super) modified: u64,
    pub(super) bytes: Vec<u8>,
}

#[derive(Default)]
pub(super) struct Options {
    pub(super) archive: Option<PathBuf>,
    pub(super) checksum: Option<PathBuf>,
    pub(super) output_dir: Option<PathBuf>,
    pub(super) target_dir: Option<PathBuf>,
    pub(super) target: Option<String>,
    pub(super) source_date_epoch: Option<u64>,
}

/// 执行严格 release 子命令并只输出结构化结果。
pub fn run() -> i32 {
    run_arguments(std::env::args().skip(1).collect())
}

/// 供 archive 内固定主程序入口复用同一安装事务。
pub fn run_arguments(arguments: Vec<String>) -> i32 {
    match execute(arguments) {
        Ok(value) => {
            println!("{value}");
            0
        }
        Err(message) => {
            eprintln!(
                "{}",
                json!({
                    "ok": false,
                    "error": { "code": "RELEASE_FAILED", "message": message }
                })
            );
            2
        }
    }
}

fn execute(arguments: Vec<String>) -> ReleaseResult<serde_json::Value> {
    let (command, options) = parse(arguments)?;
    match command.as_str() {
        "build" => build_archive(options),
        "install" => install::install_archive(options),
        "rollback" => install::rollback(options),
        "uninstall" => install::uninstall(options),
        _ => Err("unknown Linux release command".to_owned()),
    }
}

fn parse(arguments: Vec<String>) -> ReleaseResult<(String, Options)> {
    let Some(command) = arguments.first().cloned() else {
        return Err("a Linux release command is required".to_owned());
    };
    let mut options = Options::default();
    let mut index = 1;
    while index < arguments.len() {
        let flag = arguments[index].as_str();
        index += 1;
        let value = arguments
            .get(index)
            .ok_or_else(|| "release option requires a value".to_owned())?;
        match flag {
            "--archive" => options.archive = Some(PathBuf::from(value)),
            "--checksum" => options.checksum = Some(PathBuf::from(value)),
            "--output-dir" => options.output_dir = Some(PathBuf::from(value)),
            "--target-dir" => options.target_dir = Some(PathBuf::from(value)),
            "--target" => options.target = Some(value.clone()),
            "--source-date-epoch" => {
                options.source_date_epoch = Some(
                    value
                        .parse()
                        .map_err(|_| "source date epoch must be an integer".to_owned())?,
                );
            }
            _ => return Err("unknown Linux release option".to_owned()),
        }
        index += 1;
    }
    Ok((command, options))
}

pub(super) fn require_absolute(value: Option<PathBuf>, name: &str) -> ReleaseResult<PathBuf> {
    let value = value.ok_or_else(|| format!("{name} is required"))?;
    if !value.is_absolute() || value.components().any(|part| part == Component::ParentDir) {
        return Err(format!("{name} must be an absolute normalized path"));
    }
    Ok(value)
}

#[cfg(feature = "linux-release-tools")]
fn supported_target(target: Option<String>) -> ReleaseResult<String> {
    let target = target.ok_or_else(|| "--target is required".to_owned())?;
    if matches!(
        target.as_str(),
        "x86_64-unknown-linux-gnu" | "aarch64-unknown-linux-gnu"
    ) {
        Ok(target)
    } else {
        Err("the Linux release target is not allowlisted".to_owned())
    }
}

#[cfg(feature = "linux-release-tools")]
fn build_archive(options: Options) -> ReleaseResult<serde_json::Value> {
    let output_dir = require_absolute(options.output_dir, "--output-dir")?;
    let target_dir = require_absolute(options.target_dir, "--target-dir")?;
    let target = supported_target(options.target)?;
    let modified = options
        .source_date_epoch
        .ok_or_else(|| "--source-date-epoch is required".to_owned())?;
    fs::create_dir_all(&output_dir).map_err(|_| "release output directory is unavailable")?;
    fs::create_dir_all(&target_dir).map_err(|_| "release target directory is unavailable")?;
    require_trusted_directory(&output_dir, "release output directory")?;
    require_trusted_directory(&target_dir, "release target directory")?;
    let encoded_rustflags = release_rustflags()?;
    let status = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "build",
            "--frozen",
            "--release",
            "--no-default-features",
            "--bin",
            PACKAGE_NAME,
            "--target",
            &target,
            "--target-dir",
        ])
        .arg(&target_dir)
        .env("SOURCE_DATE_EPOCH", modified.to_string())
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("RUSTFLAGS")
        .env("CARGO_ENCODED_RUSTFLAGS", encoded_rustflags)
        .status()
        .map_err(|_| "cargo release build could not start".to_owned())?;
    if !status.success() {
        return Err("cargo release build failed".to_owned());
    }
    let binary = target_dir.join(&target).join("release").join(PACKAGE_NAME);
    let binary_bytes = bounded_read(&binary, MAXIMUM_ARCHIVE_BYTES)?;
    if fs::metadata(&binary)
        .map_err(|_| "release binary metadata is unavailable".to_owned())?
        .permissions()
        .mode()
        & 0o111
        == 0
    {
        return Err("release binary is not executable".to_owned());
    }
    let license = bounded_read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(LICENSE_FILE),
        256 * 1024,
    )?;
    let dependencies = sbom::normal_dependencies(&target)?;
    let notices = sbom::notices(&target, &dependencies);
    let install_script = install_script();
    let sbom = sbom::build(&target, &dependencies, &binary_bytes)?;
    let archive_root = format!("{PACKAGE_NAME}-{}-{target}", env!("CARGO_PKG_VERSION"));
    let files = vec![
        payload(MAIN_BINARY, "main-cli", 0o755, &binary_bytes),
        payload(INSTALL_SCRIPT, "managed-installer", 0o755, &install_script),
        payload(LICENSE_FILE, "license", 0o644, &license),
        payload(NOTICE_FILE, "third-party-notices", 0o644, &notices),
        payload(SBOM_FILE, "spdx-sbom", 0o644, &sbom),
    ];
    let elf_policy = elf::inspect(&binary_bytes, &target)?;
    let maximum_glibc = elf_policy.maximum_glibc.clone();
    let ubuntu_2204_compatible = glibc_at_most(&maximum_glibc, 2, 35)?;
    let manifest = ArchiveManifest {
        contract_version: CONTRACT_VERSION.to_owned(),
        package_name: PACKAGE_NAME.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        target: target.clone(),
        source_date_epoch: modified,
        archive_root: archive_root.clone(),
        layout: "versioned-releases-with-recoverable-link-transaction".to_owned(),
        binaries: vec![MAIN_BINARY.to_owned()],
        companion_binaries: Vec::new(),
        sibling_policy: SiblingPolicy {
            resolution: "same-release-bin-only".to_owned(),
            path_fallback: false,
            executable_override: false,
        },
        elf_needed: elf_policy.needed.clone(),
        maximum_glibc,
        ubuntu_2204_compatible,
        release_environment_verified: false,
        elf_policy,
        sbom_package_count: dependencies.len() + 1,
        files,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|_| "release manifest serialization failed".to_owned())?;
    let archive_name = format!("{archive_root}.tar.zst");
    let archive_path = output_dir.join(&archive_name);
    let checksum_path = output_dir.join(format!("{archive_name}.sha256"));
    if archive_path.exists() || checksum_path.exists() {
        return Err("release output already exists".to_owned());
    }
    archive::write_archive(
        &archive_path,
        &archive_root,
        modified,
        [
            (MAIN_BINARY, 0o755, binary_bytes.as_slice()),
            (INSTALL_SCRIPT, 0o755, install_script.as_slice()),
            (LICENSE_FILE, 0o644, license.as_slice()),
            (NOTICE_FILE, 0o644, notices.as_slice()),
            (SBOM_FILE, 0o644, sbom.as_slice()),
            (MANIFEST_FILE, 0o644, manifest_bytes.as_slice()),
        ],
    )?;
    let archive_digest = sha256(&bounded_read(&archive_path, MAXIMUM_ARCHIVE_BYTES)?);
    atomic_write(
        &checksum_path,
        format!("{archive_digest}  {archive_name}\n").as_bytes(),
        0o644,
    )?;
    Ok(json!({
        "ok": true,
        "operation": "build",
        "archive": archive_path,
        "checksum": checksum_path,
        "sha256": archive_digest,
        "binaryRoles": ["main-cli"],
        "companionBinaries": [],
        "publishable": false,
        "publishabilityReason": "digest-locked-ubuntu-22.04-builder-not-verified",
        "ubuntu2204Compatible": ubuntu_2204_compatible,
        "releaseEnvironmentVerified": false,
    }))
}

#[cfg(feature = "linux-release-tools")]
fn release_rustflags() -> ReleaseResult<String> {
    let home = std::env::var("HOME")
        .map_err(|_| "HOME is required for reproducible path remapping".to_owned())?;
    if home.contains('\u{1f}') || env!("CARGO_MANIFEST_DIR").contains('\u{1f}') {
        return Err("release build path contains an unsupported separator".to_owned());
    }
    let mut flags = vec![
        "-C".to_owned(),
        "link-arg=-Wl,-z,relro,-z,now".to_owned(),
        "-C".to_owned(),
        "link-arg=-Wl,--build-id=none".to_owned(),
        format!("--remap-path-prefix={home}=/build-user"),
        format!(
            "--remap-path-prefix={}=/workspace",
            env!("CARGO_MANIFEST_DIR")
        ),
    ];
    for (name, replacement) in [("CARGO_HOME", "/cargo"), ("RUSTUP_HOME", "/rustup")] {
        if let Some(value) = std::env::var_os(name) {
            let value = value
                .to_str()
                .ok_or_else(|| format!("{name} must be UTF-8"))?;
            if value.contains('\u{1f}') {
                return Err(format!("{name} contains an unsupported separator"));
            }
            flags.push(format!("--remap-path-prefix={value}={replacement}"));
        }
    }
    Ok(flags.join("\u{1f}"))
}

#[cfg(not(feature = "linux-release-tools"))]
fn build_archive(_options: Options) -> ReleaseResult<serde_json::Value> {
    Err("archive building is unavailable in the installed main CLI".to_owned())
}

#[cfg(feature = "linux-release-tools")]
fn payload(path: &str, role: &str, mode: u32, bytes: &[u8]) -> PayloadFile {
    PayloadFile {
        path: path.to_owned(),
        role: role.to_owned(),
        mode,
        sha256: sha256(bytes),
    }
}

#[cfg(feature = "linux-release-tools")]
fn install_script() -> Vec<u8> {
    b"#!/bin/sh\nset -eu\nscript_dir=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd -P)\nexec \"$script_dir/bin/ai-computer-toolkit\" __linux-release \"$@\"\n"
        .to_vec()
}

pub(super) fn glibc_at_most(
    version: &str,
    allowed_major: u32,
    allowed_minor: u32,
) -> ReleaseResult<bool> {
    let parts = version.split('.').collect::<Vec<_>>();
    if !matches!(parts.len(), 2 | 3)
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err("GLIBC version is malformed".to_owned());
    }
    let parsed = (
        parts[0]
            .parse::<u32>()
            .map_err(|_| "GLIBC version is malformed".to_owned())?,
        parts[1]
            .parse::<u32>()
            .map_err(|_| "GLIBC version is malformed".to_owned())?,
        parts
            .get(2)
            .map_or(Ok(0), |patch| patch.parse())
            .map_err(|_| "GLIBC version is malformed".to_owned())?,
    );
    Ok(parsed <= (allowed_major, allowed_minor, 0))
}

pub(super) fn reject_unused_options(options: &Options) -> ReleaseResult<()> {
    if options.archive.is_some()
        || options.checksum.is_some()
        || options.output_dir.is_some()
        || options.target_dir.is_some()
        || options.target.is_some()
        || options.source_date_epoch.is_some()
    {
        return Err("this release operation does not accept options".to_owned());
    }
    Ok(())
}

pub(super) fn absolute_environment_path(name: &str) -> ReleaseResult<PathBuf> {
    let value = std::env::var_os(name).ok_or_else(|| format!("{name} is required"))?;
    let text = value
        .to_str()
        .ok_or_else(|| format!("{name} must be UTF-8"))?;
    let path = PathBuf::from(&value);
    if !path.is_absolute()
        || path == Path::new("/")
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
        || text
            .split('/')
            .enumerate()
            .any(|(index, part)| index > 0 && matches!(part, "" | "." | ".."))
    {
        return Err(format!("{name} must be an absolute normalized path"));
    }
    Ok(path)
}

pub(super) fn require_trusted_directory(path: &Path, label: &str) -> ReleaseResult<()> {
    let mut current = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(part) => current.push(part),
            _ => return Err(format!("{label} is not a normalized directory")),
        }
        let metadata =
            fs::symlink_metadata(&current).map_err(|_| format!("{label} is unavailable"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!("{label} is not a trusted directory"));
        }
    }
    Ok(())
}

pub(super) fn verify_checksum(
    archive: &Path,
    checksum: &Path,
    bytes: &[u8],
) -> ReleaseResult<String> {
    let line = String::from_utf8(bounded_read_single_link(checksum, 1024)?)
        .map_err(|_| "checksum file was not UTF-8".to_owned())?;
    let expected_name = archive
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "archive file name is invalid".to_owned())?;
    let digest = sha256(bytes);
    if line != format!("{digest}  {expected_name}\n") {
        return Err("archive checksum verification failed".to_owned());
    }
    Ok(digest)
}

pub(super) fn ensure_host_target(target: &str) -> ReleaseResult<()> {
    let host = match std::env::consts::ARCH {
        "x86_64" => "x86_64-unknown-linux-gnu",
        "aarch64" => "aarch64-unknown-linux-gnu",
        _ => return Err("the current Linux architecture is unsupported".to_owned()),
    };
    if target == host {
        Ok(())
    } else {
        Err("archive target does not match the current host".to_owned())
    }
}

#[cfg(feature = "linux-release-tools")]
fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> ReleaseResult<()> {
    let temporary = temporary_sibling(path, "file")?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "atomic staging file could not be created".to_owned())?;
        file.write_all(bytes)
            .map_err(|_| "atomic staging file could not be written".to_owned())?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))
            .map_err(|_| "atomic staging file mode could not be set".to_owned())?;
        file.sync_all()
            .map_err(|_| "atomic staging file could not be synchronized".to_owned())?;
        fs::rename(&temporary, path).map_err(|_| "atomic file commit failed".to_owned())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    sync_directory(
        path.parent()
            .ok_or_else(|| "atomic file parent is invalid".to_owned())?,
    )
}

pub(super) fn sync_directory(path: &Path) -> ReleaseResult<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "directory synchronization failed".to_owned())
}

#[cfg(feature = "linux-release-tools")]
pub(super) fn temporary_sibling(path: &Path, label: &str) -> ReleaseResult<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| "output path has no parent".to_owned())?;
    Ok(parent.join(format!(".{label}-{}-{}", std::process::id(), nonce())))
}

pub(super) fn bounded_read(path: &Path, maximum: u64) -> ReleaseResult<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "required file is unavailable")?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > maximum {
        return Err("required file is not a bounded regular file".to_owned());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "required file could not be opened")?
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "required file could not be read")?;
    if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > maximum {
        return Err("required file changed while it was read".to_owned());
    }
    Ok(bytes)
}

pub(super) fn bounded_read_single_link(path: &Path, maximum: u64) -> ReleaseResult<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "required file is unavailable")?;
    if metadata.nlink() != 1 {
        return Err("required file must have one filesystem link".to_owned());
    }
    bounded_read(path, maximum)
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(feature = "linux-release-tools")]
pub(super) fn nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::glibc_at_most;

    #[test]
    fn ubuntu_2204_glibc_gate_is_closed_at_2_35() {
        assert!(glibc_at_most("2.35", 2, 35).unwrap());
        assert!(!glibc_at_most("2.36", 2, 35).unwrap());
        assert!(!glibc_at_most("2.35.1", 2, 35).unwrap());
    }
}
