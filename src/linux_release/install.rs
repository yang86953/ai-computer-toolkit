//! Linux Release System 的预检、可恢复安装事务与精确卸载 Module。

mod transaction;

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Component, Path, PathBuf},
};

use serde_json::json;

use self::transaction::{Journal, Operation, State};
use super::{
    ArchiveManifest, BufferedEntry, CONTRACT_VERSION, INSTALL_MARKER, INSTALL_SCRIPT, LICENSE_FILE,
    MAIN_BINARY, MANIFEST_FILE, MAXIMUM_ARCHIVE_BYTES, NOTICE_FILE, Options, PACKAGE_NAME,
    ReleaseResult, SBOM_FILE, SiblingPolicy, archive, bounded_read, bounded_read_single_link, elf,
    ensure_host_target, reject_unused_options, require_absolute, require_trusted_directory, sbom,
    sha256, verify_checksum,
};

const MARKER_FILE: &str = ".act-linux-install-v1";
const LOCK_FILE: &str = ".ai-computer-toolkit.install.lock";
const JOURNAL_FILE: &str = ".ai-computer-toolkit.transaction-v1.json";
const JOURNAL_STAGING_FILE: &str = ".ai-computer-toolkit.transaction-v1.next";
const TOMBSTONE: &str = ".ai-computer-toolkit.uninstalling";
const LAUNCHER_BACKUP: &str = ".ai-computer-toolkit.uninstall-launcher";

#[derive(Clone)]
pub(super) struct InstallLayout {
    pub(super) data_home: PathBuf,
    pub(super) root: PathBuf,
    pub(super) entry: PathBuf,
    pub(super) lock: PathBuf,
    pub(super) journal: PathBuf,
    pub(super) journal_staging: PathBuf,
    pub(super) tombstone: PathBuf,
    pub(super) launcher_backup: PathBuf,
}

struct InstallSnapshot {
    current: Option<String>,
    rollback: Option<String>,
    releases: BTreeMap<String, ArchiveManifest>,
    launcher_present: bool,
}

/// 安装 archive 前完成 archive、现有根、入口和 pending journal 的全部预检。
pub(super) fn install_archive(options: Options) -> ReleaseResult<serde_json::Value> {
    if options.output_dir.is_some()
        || options.target_dir.is_some()
        || options.target.is_some()
        || options.source_date_epoch.is_some()
    {
        return Err("install received an unsupported option".to_owned());
    }
    let archive_path = require_absolute(options.archive, "--archive")?;
    let checksum_path = require_absolute(options.checksum, "--checksum")?;
    let layout = install_layout()?;
    let _lock = transaction::acquire(&layout)?;
    recover_pending(&layout)?;
    reject_unmanaged_recovery_paths(&layout)?;
    let archive_bytes = bounded_read_single_link(&archive_path, MAXIMUM_ARCHIVE_BYTES)?;
    let archive_digest = verify_checksum(&archive_path, &checksum_path, &archive_bytes)?;
    let (manifest, entries) = archive::read_and_validate_archive(&archive_bytes)?;
    ensure_host_target(&manifest.target)?;
    validate_archive_payload_semantics(&manifest, &entries)?;
    let snapshot = inspect_install(&layout, true)?;
    let release_id = format!("{}-{}", manifest.version, &archive_digest[..12]);
    if let Some(existing) = snapshot.releases.get(&release_id)
        && existing != &manifest
    {
        return Err("release digest identifier collides with different content".to_owned());
    }
    let new_target = format!("releases/{release_id}");
    if snapshot.current.as_deref() == Some(new_target.as_str()) {
        return Ok(json!({
            "ok": true,
            "operation": "install",
            "version": manifest.version,
            "releaseId": release_id,
            "current": layout.entry,
            "rollbackAvailable": snapshot.rollback.is_some(),
            "alreadyCurrent": true,
        }));
    }
    let journal = Journal::install(
        snapshot.current.clone(),
        snapshot.rollback.clone(),
        release_id.clone(),
        archive_digest,
        snapshot.launcher_present,
    );
    transaction::write_journal(&layout, &journal)?;
    apply_install(&layout, &journal, &manifest, &entries)?;
    Ok(json!({
        "ok": true,
        "operation": "install",
        "version": manifest.version,
        "releaseId": release_id,
        "current": layout.entry,
        "rollbackAvailable": journal.new_rollback.is_some(),
    }))
}

/// 回滚只在完整预检后以 journal 驱动两个 link 的可恢复切换。
pub(super) fn rollback(options: Options) -> ReleaseResult<serde_json::Value> {
    reject_unused_options(&options)?;
    let layout = install_layout()?;
    let _lock = transaction::acquire(&layout)?;
    if recover_pending(&layout)? == Some(Operation::Rollback) {
        return Ok(json!({
            "ok": true,
            "operation": "rollback",
            "current": layout.entry,
            "recovered": true,
        }));
    }
    reject_unmanaged_recovery_paths(&layout)?;
    let snapshot = inspect_install(&layout, false)?;
    let current = snapshot
        .current
        .ok_or_else(|| "current release link is missing".to_owned())?;
    let rollback = snapshot
        .rollback
        .ok_or_else(|| "no rollback release is available".to_owned())?;
    let journal = Journal::rollback(current, rollback);
    transaction::write_journal(&layout, &journal)?;
    apply_link_transaction(&layout, journal)?;
    Ok(json!({
        "ok": true,
        "operation": "rollback",
        "current": layout.entry,
    }))
}

/// 卸载先认证整个 root，再以 journal 和固定 tombstone 完成前滚恢复。
pub(super) fn uninstall(options: Options) -> ReleaseResult<serde_json::Value> {
    reject_unused_options(&options)?;
    let layout = install_layout()?;
    let _lock = transaction::acquire(&layout)?;
    if recover_pending(&layout)? == Some(Operation::Uninstall) {
        return Ok(json!({
            "ok": true,
            "operation": "uninstall",
            "removed": true,
            "launcher": layout.entry,
            "recovered": true,
        }));
    }
    reject_unmanaged_recovery_paths(&layout)?;
    let snapshot = inspect_install(&layout, false)?;
    let current = snapshot
        .current
        .ok_or_else(|| "current release link is missing".to_owned())?;
    let journal = Journal::uninstall(current, snapshot.rollback);
    transaction::write_journal(&layout, &journal)?;
    let mut committing = journal;
    committing.state = State::Committing;
    transaction::write_journal(&layout, &committing)?;
    complete_uninstall(&layout)?;
    committing.state = State::Committed;
    transaction::write_journal(&layout, &committing)?;
    transaction::remove_journal(&layout)?;
    Ok(json!({
        "ok": true,
        "operation": "uninstall",
        "removed": true,
        "launcher": layout.entry,
    }))
}

fn install_layout() -> ReleaseResult<InstallLayout> {
    let data_home = super::absolute_environment_path("XDG_DATA_HOME")?;
    let home = super::absolute_environment_path("HOME")?;
    require_trusted_directory(&data_home, "XDG data home")?;
    require_trusted_directory(&home, "home directory")?;
    let local = home.join(".local");
    validate_optional_user_directory(&local, "user local directory")?;
    let bin = local.join("bin");
    validate_optional_user_directory(&bin, "user launcher directory")?;
    Ok(InstallLayout {
        root: data_home.join(PACKAGE_NAME),
        entry: bin.join(PACKAGE_NAME),
        lock: data_home.join(LOCK_FILE),
        journal: data_home.join(JOURNAL_FILE),
        journal_staging: data_home.join(JOURNAL_STAGING_FILE),
        tombstone: data_home.join(TOMBSTONE),
        launcher_backup: bin.join(LAUNCHER_BACKUP),
        data_home,
    })
}

fn validate_optional_user_directory(path: &Path, label: &str) -> ReleaseResult<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(format!("{label} metadata is unavailable")),
        Ok(metadata)
            if !metadata.file_type().is_symlink()
                && metadata.is_dir()
                && metadata.uid() == current_uid()
                && metadata.permissions().mode() & 0o022 == 0 =>
        {
            Ok(())
        }
        Ok(_) => Err(format!("{label} ownership or mode is invalid")),
    }
}

fn reject_unmanaged_recovery_paths(layout: &InstallLayout) -> ReleaseResult<()> {
    for (path, label) in [
        (&layout.tombstone, "uninstall tombstone"),
        (&layout.launcher_backup, "uninstall launcher backup"),
    ] {
        if fs::symlink_metadata(path).is_ok() {
            return Err(format!("{label} exists without an active transaction"));
        }
    }
    Ok(())
}

fn inspect_install(layout: &InstallLayout, allow_absent: bool) -> ReleaseResult<InstallSnapshot> {
    match fs::symlink_metadata(&layout.root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_absent => {
            ensure_launcher(layout, false)?;
            return Ok(InstallSnapshot {
                current: None,
                rollback: None,
                releases: BTreeMap::new(),
                launcher_present: false,
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err("install prefix is missing".to_owned());
        }
        Err(_) => return Err("install prefix metadata is unavailable".to_owned()),
        Ok(metadata) => validate_owned_directory(&metadata, 0o700, "install prefix")?,
    }
    require_marker(&layout.root)?;
    validate_install_top_level(&layout.root)?;
    let releases_path = layout.root.join("releases");
    validate_owned_directory(
        &fs::symlink_metadata(&releases_path)
            .map_err(|_| "release directory is unavailable".to_owned())?,
        0o700,
        "release directory",
    )?;
    let mut releases = BTreeMap::new();
    for entry in fs::read_dir(&releases_path)
        .map_err(|_| "release directory could not be read".to_owned())?
    {
        let entry = entry.map_err(|_| "release directory entry could not be read".to_owned())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "release identifier must be UTF-8".to_owned())?;
        if !valid_release_id(&name) {
            return Err("release directory contains an invalid identifier".to_owned());
        }
        let manifest = validate_installed_release(&entry.path())?;
        if releases.insert(name, manifest).is_some() {
            return Err("release directory contains duplicate identifiers".to_owned());
        }
    }
    let current = read_release_link(&layout.root.join("current"))?;
    let rollback = read_release_link(&layout.root.join("rollback"))?;
    let current = current.ok_or_else(|| "current release link is missing".to_owned())?;
    for target in [Some(&current), rollback.as_ref()].into_iter().flatten() {
        let identifier = target
            .strip_prefix("releases/")
            .ok_or_else(|| "release link target is invalid".to_owned())?;
        if !releases.contains_key(identifier) {
            return Err("release link references an unknown release".to_owned());
        }
    }
    ensure_launcher(layout, true)?;
    Ok(InstallSnapshot {
        current: Some(current),
        rollback,
        releases,
        launcher_present: true,
    })
}

fn validate_install_top_level(prefix: &Path) -> ReleaseResult<()> {
    let allowed = BTreeSet::from([
        MARKER_FILE.to_owned(),
        "current".to_owned(),
        "rollback".to_owned(),
        "releases".to_owned(),
    ]);
    let mut found = BTreeSet::new();
    for entry in fs::read_dir(prefix).map_err(|_| "install prefix could not be read".to_owned())? {
        let name = entry
            .map_err(|_| "install prefix entry could not be read".to_owned())?
            .file_name()
            .into_string()
            .map_err(|_| "install prefix contains a non-UTF-8 entry".to_owned())?;
        if !allowed.contains(&name) || !found.insert(name) {
            return Err("install prefix contains an unmanaged entry".to_owned());
        }
    }
    if !found.contains(MARKER_FILE) || !found.contains("releases") {
        return Err("install prefix is incomplete".to_owned());
    }
    Ok(())
}

fn apply_install(
    layout: &InstallLayout,
    journal: &Journal,
    manifest: &ArchiveManifest,
    entries: &[BufferedEntry],
) -> ReleaseResult<()> {
    prepare_install_directories(layout)?;
    let release_id = journal
        .release_id
        .as_deref()
        .ok_or_else(|| "install transaction release is missing".to_owned())?;
    let destination = layout.root.join("releases").join(release_id);
    if fs::symlink_metadata(&destination).is_err() {
        let staging = layout.root.join("releases").join(
            journal
                .staging_name
                .as_deref()
                .ok_or_else(|| "install transaction staging is missing".to_owned())?,
        );
        create_managed_directory(&staging, 0o700, "install staging")?;
        create_managed_directory(&staging.join("bin"), 0o755, "install staging bin")?;
        extract_entries(&staging, manifest, entries)?;
        validate_installed_release(&staging)?;
        transaction::checkpoint("release:before-rename")?;
        fs::rename(&staging, &destination)
            .map_err(|_| "installed release commit failed".to_owned())?;
        transaction::checkpoint("release:after-rename")?;
        transaction::sync_directory_labeled(
            &layout.root.join("releases"),
            "releases-after-release-rename",
        )?;
    } else if &validate_installed_release(&destination)? != manifest {
        return Err("installed release content does not match the archive".to_owned());
    }
    let mut committing = journal.clone();
    committing.state = State::Committing;
    transaction::write_journal(layout, &committing)?;
    apply_link_transaction(layout, committing)
}

fn apply_link_transaction(layout: &InstallLayout, mut journal: Journal) -> ReleaseResult<()> {
    for target in [journal.new_current.as_ref(), journal.new_rollback.as_ref()]
        .into_iter()
        .flatten()
    {
        validate_installed_release(&layout.root.join(target))?;
    }
    replace_release_link(&layout.root, "rollback", journal.new_rollback.as_deref())?;
    replace_release_link(&layout.root, "current", journal.new_current.as_deref())?;
    ensure_launcher_parent(layout)?;
    create_or_validate_launcher(layout)?;
    journal.state = State::Committed;
    transaction::write_journal(layout, &journal)?;
    transaction::remove_journal(layout)
}

fn prepare_install_directories(layout: &InstallLayout) -> ReleaseResult<()> {
    if fs::symlink_metadata(&layout.root).is_err() {
        create_managed_directory(&layout.root, 0o700, "install prefix")?;
        let marker = layout.root.join(MARKER_FILE);
        transaction::durable_atomic_write(
            &marker,
            &layout.root.join(format!(".{MARKER_FILE}.next")),
            INSTALL_MARKER.as_bytes(),
            0o600,
            "install-marker",
        )?;
        create_managed_directory(&layout.root.join("releases"), 0o700, "release directory")?;
    }
    require_marker(&layout.root)
}

fn create_managed_directory(path: &Path, mode: u32, label: &str) -> ReleaseResult<()> {
    transaction::checkpoint(&format!("{label}:before-mkdir"))?;
    fs::create_dir(path).map_err(|_| format!("{label} could not be created"))?;
    transaction::checkpoint(&format!("{label}:after-mkdir"))?;
    transaction::checkpoint(&format!("{label}:before-chmod"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|_| format!("{label} mode could not be set"))?;
    transaction::checkpoint(&format!("{label}:after-chmod"))?;
    transaction::sync_directory_labeled(path, &format!("{label}-self"))?;
    transaction::sync_directory_labeled(
        path.parent()
            .ok_or_else(|| format!("{label} parent is invalid"))?,
        &format!("{label}-parent"),
    )
}

fn extract_entries(
    staging: &Path,
    manifest: &ArchiveManifest,
    entries: &[BufferedEntry],
) -> ReleaseResult<()> {
    let prefix = format!("{}/", manifest.archive_root);
    for entry in entries {
        let relative = entry
            .path
            .strip_prefix(&prefix)
            .ok_or_else(|| "archive entry is outside the declared root".to_owned())?;
        let path = staging.join(relative);
        let file_name = path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| "installed file name is invalid".to_owned())?;
        let next = path.with_file_name(format!(".{file_name}.next"));
        transaction::durable_atomic_write(
            &path,
            &next,
            &entry.bytes,
            entry.mode,
            &format!("payload-{file_name}"),
        )?;
    }
    transaction::sync_directory_labeled(staging, "staging-final")?;
    transaction::sync_directory_labeled(&staging.join("bin"), "staging-bin-final")
}

fn validate_archive_payload_semantics(
    manifest: &ArchiveManifest,
    entries: &[BufferedEntry],
) -> ReleaseResult<()> {
    let prefix = format!("{}/", manifest.archive_root);
    let find = |name: &str| {
        entries
            .iter()
            .find(|entry| entry.path == format!("{prefix}{name}"))
            .map(|entry| entry.bytes.as_slice())
            .ok_or_else(|| "archive semantic payload is missing".to_owned())
    };
    let binary = find(MAIN_BINARY)?;
    let evidence = elf::inspect(binary, &manifest.target)?;
    if evidence != manifest.elf_policy {
        return Err("archive ELF evidence does not match the binary".to_owned());
    }
    sbom::validate(
        find(SBOM_FILE)?,
        &manifest.target,
        manifest.sbom_package_count,
    )
}

fn validate_installed_release(path: &Path) -> ReleaseResult<ArchiveManifest> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "installed release is missing".to_owned())?;
    validate_owned_directory(&metadata, 0o700, "installed release")?;
    let manifest_path = path.join(MANIFEST_FILE);
    transaction::validate_owned_path(&manifest_path, 0o644, "installed manifest")?;
    let manifest_bytes = bounded_read_single_link(&manifest_path, 1024 * 1024)?;
    let manifest: ArchiveManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| "installed manifest is invalid".to_owned())?;
    validate_installed_manifest_policy(&manifest)?;
    let mut expected = BTreeSet::from([MANIFEST_FILE.to_owned()]);
    for file in &manifest.files {
        let file_path = path.join(&file.path);
        transaction::validate_owned_path(&file_path, file.mode, "installed payload")?;
        if sha256(&bounded_read(&file_path, MAXIMUM_ARCHIVE_BYTES)?) != file.sha256 {
            return Err("installed payload verification failed".to_owned());
        }
        expected.insert(file.path.clone());
    }
    if installed_files(path)? != expected {
        return Err("installed release contains an unmanaged file".to_owned());
    }
    let binary = bounded_read(&path.join(MAIN_BINARY), MAXIMUM_ARCHIVE_BYTES)?;
    if elf::inspect(&binary, &manifest.target)? != manifest.elf_policy {
        return Err("installed ELF evidence does not match the manifest".to_owned());
    }
    sbom::validate(
        &bounded_read(&path.join(SBOM_FILE), 16 * 1024 * 1024)?,
        &manifest.target,
        manifest.sbom_package_count,
    )?;
    Ok(manifest)
}

fn validate_installed_manifest_policy(manifest: &ArchiveManifest) -> ReleaseResult<()> {
    elf::validate_manifest_evidence(
        &manifest.elf_policy,
        &manifest.target,
        &manifest.elf_needed,
        &manifest.maximum_glibc,
        manifest.ubuntu_2204_compatible,
    )?;
    if manifest.contract_version != CONTRACT_VERSION
        || manifest.package_name != PACKAGE_NAME
        || manifest.version != env!("CARGO_PKG_VERSION")
        || manifest.archive_root
            != format!(
                "{PACKAGE_NAME}-{}-{}",
                env!("CARGO_PKG_VERSION"),
                manifest.target
            )
        || manifest.layout != "versioned-releases-with-recoverable-link-transaction"
        || manifest.binaries != [MAIN_BINARY]
        || !manifest.companion_binaries.is_empty()
        || manifest.sibling_policy
            != (SiblingPolicy {
                resolution: "same-release-bin-only".to_owned(),
                path_fallback: false,
                executable_override: false,
            })
        || manifest.release_environment_verified
        || manifest.sbom_package_count == 0
    {
        return Err("installed manifest policy is invalid".to_owned());
    }
    let expected = BTreeMap::from([
        (MAIN_BINARY, ("main-cli", 0o755)),
        (INSTALL_SCRIPT, ("managed-installer", 0o755)),
        (LICENSE_FILE, ("license", 0o644)),
        (NOTICE_FILE, ("third-party-notices", 0o644)),
        (SBOM_FILE, ("spdx-sbom", 0o644)),
    ]);
    if manifest.files.len() != expected.len() {
        return Err("installed manifest payload is invalid".to_owned());
    }
    let mut found = BTreeSet::new();
    for file in &manifest.files {
        let Some((role, mode)) = expected.get(file.path.as_str()) else {
            return Err("installed manifest payload is invalid".to_owned());
        };
        if !found.insert(file.path.as_str())
            || file.role != *role
            || file.mode != *mode
            || file.sha256.len() != 64
            || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("installed manifest payload policy is invalid".to_owned());
        }
    }
    Ok(())
}

fn installed_files(root: &Path) -> ReleaseResult<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    for entry in fs::read_dir(root).map_err(|_| "installed release could not be read".to_owned())? {
        let entry = entry.map_err(|_| "installed release entry could not be read".to_owned())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "installed release has a non-UTF-8 entry".to_owned())?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| "installed release entry metadata is unavailable".to_owned())?;
        if metadata.file_type().is_symlink() {
            return Err("installed release contains a symlink".to_owned());
        }
        if metadata.is_file() {
            found.insert(name);
        } else if metadata.is_dir() && name == "bin" {
            validate_owned_directory(&metadata, 0o755, "installed bin")?;
            for child in fs::read_dir(entry.path())
                .map_err(|_| "installed bin could not be read".to_owned())?
            {
                let child =
                    child.map_err(|_| "installed bin entry could not be read".to_owned())?;
                let child_metadata = fs::symlink_metadata(child.path())
                    .map_err(|_| "installed bin entry metadata is unavailable".to_owned())?;
                if child_metadata.file_type().is_symlink() || !child_metadata.is_file() {
                    return Err("installed bin contains a non-regular entry".to_owned());
                }
                let child_name = child
                    .file_name()
                    .into_string()
                    .map_err(|_| "installed bin has a non-UTF-8 entry".to_owned())?;
                found.insert(format!("bin/{child_name}"));
            }
        } else {
            return Err("installed release contains an unmanaged directory".to_owned());
        }
    }
    Ok(found)
}

fn ensure_launcher(layout: &InstallLayout, required: bool) -> ReleaseResult<()> {
    let expected = layout.root.join("current/bin").join(PACKAGE_NAME);
    match fs::symlink_metadata(&layout.entry) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err("managed launcher is missing".to_owned())
        }
        Err(_) => Err("managed launcher metadata is unavailable".to_owned()),
        Ok(metadata)
            if metadata.file_type().is_symlink()
                && metadata.uid() == current_uid()
                && fs::read_link(&layout.entry)
                    .map_err(|_| "managed launcher target could not be read".to_owned())?
                    == expected =>
        {
            Ok(())
        }
        Ok(_) => Err("managed launcher destination is occupied or tampered".to_owned()),
    }
}

fn ensure_launcher_parent(layout: &InstallLayout) -> ReleaseResult<()> {
    let home = layout
        .entry
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| "managed launcher home is invalid".to_owned())?;
    let local = home.join(".local");
    if fs::symlink_metadata(&local).is_err() {
        create_managed_directory(&local, 0o700, "user local directory")?;
    }
    let bin = local.join("bin");
    if fs::symlink_metadata(&bin).is_err() {
        create_managed_directory(&bin, 0o700, "user launcher directory")?;
    }
    validate_optional_user_directory(&bin, "user launcher directory")
}

fn create_or_validate_launcher(layout: &InstallLayout) -> ReleaseResult<()> {
    if fs::symlink_metadata(&layout.entry).is_ok() {
        return ensure_launcher(layout, true);
    }
    let expected = layout.root.join("current/bin").join(PACKAGE_NAME);
    let temporary = layout.entry.with_file_name(format!(".{PACKAGE_NAME}.next"));
    if fs::symlink_metadata(&temporary).is_ok() {
        validate_managed_symlink(&temporary, &expected, "managed launcher staging")?;
        fs::remove_file(&temporary)
            .map_err(|_| "managed launcher staging could not be recovered".to_owned())?;
    }
    transaction::checkpoint("launcher:before-symlink")?;
    symlink(&expected, &temporary).map_err(|_| "managed launcher staging failed".to_owned())?;
    transaction::checkpoint("launcher:after-symlink")?;
    transaction::checkpoint("launcher:before-rename")?;
    fs::rename(&temporary, &layout.entry)
        .map_err(|_| "managed launcher commit failed".to_owned())?;
    transaction::checkpoint("launcher:after-rename")?;
    transaction::sync_directory_labeled(
        layout
            .entry
            .parent()
            .ok_or_else(|| "managed launcher parent is invalid".to_owned())?,
        "launcher-parent",
    )
}

fn replace_release_link(root: &Path, name: &str, target: Option<&str>) -> ReleaseResult<()> {
    let path = root.join(name);
    let temporary = root.join(format!(".{name}.next"));
    if fs::symlink_metadata(&temporary).is_ok() {
        let expected = target.ok_or_else(|| "release link staging is unexpected".to_owned())?;
        validate_managed_symlink(&temporary, Path::new(expected), "release link staging")?;
        fs::remove_file(&temporary)
            .map_err(|_| "release link staging could not be recovered".to_owned())?;
    }
    if let Some(target) = target {
        if read_release_link(&path)?.as_deref() == Some(target) {
            return Ok(());
        }
        transaction::checkpoint(&format!("{name}:before-symlink"))?;
        symlink(target, &temporary).map_err(|_| "release link staging failed".to_owned())?;
        transaction::checkpoint(&format!("{name}:after-symlink"))?;
        transaction::checkpoint(&format!("{name}:before-rename"))?;
        fs::rename(&temporary, &path).map_err(|_| "release link commit failed".to_owned())?;
        transaction::checkpoint(&format!("{name}:after-rename"))?;
    } else if fs::symlink_metadata(&path).is_ok() {
        read_release_link(&path)?;
        transaction::checkpoint(&format!("{name}:before-remove"))?;
        fs::remove_file(&path).map_err(|_| "release link could not be removed".to_owned())?;
        transaction::checkpoint(&format!("{name}:after-remove"))?;
    }
    transaction::sync_directory_labeled(root, &format!("{name}-parent"))
}

fn read_release_link(path: &Path) -> ReleaseResult<Option<String>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("release link metadata is unavailable".to_owned()),
        Ok(metadata) if metadata.file_type().is_symlink() && metadata.uid() == current_uid() => {
            let target =
                fs::read_link(path).map_err(|_| "release link could not be read".to_owned())?;
            let parts = target.components().collect::<Vec<_>>();
            if parts.len() != 2
                || parts[0] != Component::Normal(OsStr::new("releases"))
                || !matches!(parts[1], Component::Normal(_))
            {
                return Err("release link target is outside the install root".to_owned());
            }
            let target = target
                .to_str()
                .ok_or_else(|| "release link target must be UTF-8".to_owned())?
                .to_owned();
            if !target
                .strip_prefix("releases/")
                .is_some_and(valid_release_id)
            {
                return Err("release link target is invalid".to_owned());
            }
            Ok(Some(target))
        }
        Ok(_) => Err("release link is not a managed symlink".to_owned()),
    }
}

fn recover_pending(layout: &InstallLayout) -> ReleaseResult<Option<Operation>> {
    let Some(mut journal) = transaction::read_journal(layout)? else {
        return Ok(None);
    };
    let operation = journal.operation.clone();
    validate_transition_links(layout, &journal)?;
    match journal.operation {
        Operation::Install => recover_install(layout, journal),
        Operation::Rollback => {
            journal.state = State::Committing;
            apply_link_transaction(layout, journal)
        }
        Operation::Uninstall if journal.state == State::Prepared => {
            inspect_install(layout, false)?;
            transaction::remove_journal(layout)
        }
        Operation::Uninstall => {
            complete_uninstall(layout)?;
            journal.state = State::Committed;
            transaction::write_journal(layout, &journal)?;
            transaction::remove_journal(layout)
        }
    }?;
    Ok(Some(operation))
}

fn recover_install(layout: &InstallLayout, mut journal: Journal) -> ReleaseResult<()> {
    let release_id = journal
        .release_id
        .as_deref()
        .ok_or_else(|| "install recovery release is missing".to_owned())?;
    let destination = layout.root.join("releases").join(release_id);
    if fs::symlink_metadata(&destination).is_ok() {
        validate_installed_release(&destination)?;
        journal.state = State::Committing;
        apply_link_transaction(layout, journal)
    } else {
        cleanup_staging(layout, journal.staging_name.as_deref())?;
        if journal.old_current.is_none() && fs::symlink_metadata(&layout.root).is_ok() {
            cleanup_empty_new_root(layout)?;
        }
        ensure_link_matches(&layout.root.join("current"), journal.old_current.as_deref())?;
        ensure_link_matches(
            &layout.root.join("rollback"),
            journal.old_rollback.as_deref(),
        )?;
        ensure_launcher(layout, journal.old_launcher_present)?;
        transaction::remove_journal(layout)
    }
}

fn validate_transition_links(layout: &InstallLayout, journal: &Journal) -> ReleaseResult<()> {
    for (name, old, new) in [
        (
            "current",
            journal.old_current.as_deref(),
            journal.new_current.as_deref(),
        ),
        (
            "rollback",
            journal.old_rollback.as_deref(),
            journal.new_rollback.as_deref(),
        ),
    ] {
        let observed = read_release_link(&layout.root.join(name))?;
        if observed.as_deref() != old && observed.as_deref() != new {
            return Err("install transaction observed an unknown link state".to_owned());
        }
    }
    Ok(())
}

fn ensure_link_matches(path: &Path, expected: Option<&str>) -> ReleaseResult<()> {
    if read_release_link(path)?.as_deref() == expected {
        Ok(())
    } else {
        Err("install transaction could not restore the prior link state".to_owned())
    }
}

fn cleanup_staging(layout: &InstallLayout, staging_name: Option<&str>) -> ReleaseResult<()> {
    let Some(staging_name) = staging_name else {
        return Ok(());
    };
    let staging = layout.root.join("releases").join(staging_name);
    if fs::symlink_metadata(&staging).is_err() {
        return Ok(());
    }
    remove_release_tree(&staging, false)
}

fn cleanup_empty_new_root(layout: &InstallLayout) -> ReleaseResult<()> {
    validate_recoverable_directory(
        &fs::symlink_metadata(&layout.root)
            .map_err(|_| "install prefix could not be inspected".to_owned())?,
        "install prefix",
    )?;
    let releases = layout.root.join("releases");
    if fs::symlink_metadata(&releases).is_ok() {
        validate_recoverable_directory(
            &fs::symlink_metadata(&releases)
                .map_err(|_| "release directory could not be inspected".to_owned())?,
            "release directory",
        )?;
        if fs::read_dir(&releases)
            .map_err(|_| "release directory could not be read".to_owned())?
            .next()
            .is_some()
        {
            return Err("new install recovery found an unexpected release".to_owned());
        }
        fs::remove_dir(&releases)
            .map_err(|_| "empty release directory could not be removed".to_owned())?;
    }
    let marker_staging = format!(".{MARKER_FILE}.next");
    for name in [MARKER_FILE, marker_staging.as_str()] {
        let path = layout.root.join(name);
        if fs::symlink_metadata(&path).is_ok() {
            remove_owned_regular(&path)?;
        }
    }
    if fs::read_dir(&layout.root)
        .map_err(|_| "install prefix could not be read".to_owned())?
        .next()
        .is_some()
    {
        return Err("new install recovery found an unmanaged entry".to_owned());
    }
    fs::remove_dir(&layout.root)
        .map_err(|_| "empty install prefix could not be removed".to_owned())?;
    transaction::sync_directory_labeled(&layout.data_home, "data-home-after-recovery")
}

fn complete_uninstall(layout: &InstallLayout) -> ReleaseResult<()> {
    if fs::symlink_metadata(&layout.launcher_backup).is_err()
        && fs::symlink_metadata(&layout.entry).is_ok()
    {
        ensure_launcher(layout, true)?;
        transaction::checkpoint("uninstall-launcher:before-rename")?;
        fs::rename(&layout.entry, &layout.launcher_backup)
            .map_err(|_| "managed launcher could not enter uninstall transaction".to_owned())?;
        transaction::checkpoint("uninstall-launcher:after-rename")?;
        transaction::sync_directory_labeled(
            layout
                .entry
                .parent()
                .ok_or_else(|| "managed launcher parent is invalid".to_owned())?,
            "uninstall-launcher-parent",
        )?;
    }
    if fs::symlink_metadata(&layout.tombstone).is_err()
        && fs::symlink_metadata(&layout.root).is_ok()
    {
        validate_prefix_at(&layout.root)?;
        transaction::checkpoint("uninstall-root:before-rename")?;
        fs::rename(&layout.root, &layout.tombstone)
            .map_err(|_| "install prefix could not enter uninstall transaction".to_owned())?;
        transaction::checkpoint("uninstall-root:after-rename")?;
        transaction::sync_directory_labeled(&layout.data_home, "uninstall-root-parent")?;
    }
    if fs::symlink_metadata(&layout.tombstone).is_ok() {
        validate_recoverable_prefix(&layout.tombstone)?;
        remove_authenticated_prefix(&layout.tombstone)?;
        transaction::sync_directory_labeled(&layout.data_home, "uninstall-delete-parent")?;
    }
    if fs::symlink_metadata(&layout.launcher_backup).is_ok() {
        let expected = layout.root.join("current/bin").join(PACKAGE_NAME);
        validate_managed_symlink(
            &layout.launcher_backup,
            &expected,
            "uninstall launcher backup",
        )?;
        fs::remove_file(&layout.launcher_backup)
            .map_err(|_| "uninstall launcher backup could not be removed".to_owned())?;
        transaction::sync_directory_labeled(
            layout
                .launcher_backup
                .parent()
                .ok_or_else(|| "uninstall launcher parent is invalid".to_owned())?,
            "uninstall-launcher-delete-parent",
        )?;
    }
    Ok(())
}

fn validate_prefix_at(root: &Path) -> ReleaseResult<()> {
    let metadata =
        fs::symlink_metadata(root).map_err(|_| "uninstall tombstone is missing".to_owned())?;
    validate_owned_directory(&metadata, 0o700, "uninstall tombstone")?;
    require_marker(root)?;
    validate_install_top_level(root)?;
    let releases = root.join("releases");
    validate_owned_directory(
        &fs::symlink_metadata(&releases)
            .map_err(|_| "uninstall releases directory is missing".to_owned())?,
        0o700,
        "uninstall releases directory",
    )?;
    let mut identifiers = BTreeSet::new();
    for entry in fs::read_dir(&releases)
        .map_err(|_| "uninstall releases directory could not be read".to_owned())?
    {
        let entry = entry.map_err(|_| "uninstall release entry could not be read".to_owned())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "uninstall release identifier must be UTF-8".to_owned())?;
        if !valid_release_id(&name) || !identifiers.insert(name) {
            return Err("uninstall release identifier is invalid".to_owned());
        }
        validate_installed_release(&entry.path())?;
    }
    for link in ["current", "rollback"] {
        if let Some(target) = read_release_link(&root.join(link))? {
            let identifier = target
                .strip_prefix("releases/")
                .ok_or_else(|| "uninstall release link is invalid".to_owned())?;
            if !identifiers.contains(identifier) {
                return Err("uninstall release link is stale".to_owned());
            }
        }
    }
    Ok(())
}

/// journal 已证明 tombstone 来源后，只允许精确受管子集以支持中断删除恢复。
fn validate_recoverable_prefix(root: &Path) -> ReleaseResult<()> {
    validate_recoverable_directory(
        &fs::symlink_metadata(root).map_err(|_| "uninstall tombstone is missing".to_owned())?,
        "uninstall tombstone",
    )?;
    let allowed = BTreeSet::from([MARKER_FILE, "current", "rollback", "releases"]);
    for entry in
        fs::read_dir(root).map_err(|_| "uninstall tombstone could not be read".to_owned())?
    {
        let entry = entry.map_err(|_| "uninstall tombstone entry could not be read".to_owned())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "uninstall tombstone entry must be UTF-8".to_owned())?;
        if !allowed.contains(name.as_str()) {
            return Err("uninstall tombstone contains an unmanaged entry".to_owned());
        }
        match name.as_str() {
            MARKER_FILE => {
                transaction::validate_owned_path(&entry.path(), 0o600, "uninstall marker")?;
            }
            "current" | "rollback" => {
                read_release_link(&entry.path())?;
            }
            "releases" => {
                validate_recoverable_directory(
                    &fs::symlink_metadata(entry.path())
                        .map_err(|_| "uninstall releases metadata is unavailable".to_owned())?,
                    "uninstall releases",
                )?;
                for release in fs::read_dir(entry.path())
                    .map_err(|_| "uninstall releases could not be read".to_owned())?
                {
                    let release = release
                        .map_err(|_| "uninstall release entry could not be read".to_owned())?;
                    let identifier = release
                        .file_name()
                        .into_string()
                        .map_err(|_| "uninstall release identifier must be UTF-8".to_owned())?;
                    if !valid_release_id(&identifier) {
                        return Err("uninstall release identifier is invalid".to_owned());
                    }
                    validate_recoverable_directory(
                        &fs::symlink_metadata(release.path())
                            .map_err(|_| "uninstall release metadata is unavailable".to_owned())?,
                        "uninstall release",
                    )?;
                    validate_recoverable_release_entries(&release.path())?;
                }
            }
            _ => return Err("uninstall tombstone entry is invalid".to_owned()),
        }
    }
    Ok(())
}

fn validate_recoverable_release_entries(path: &Path) -> ReleaseResult<()> {
    let allowed_root = BTreeSet::from([
        MANIFEST_FILE,
        INSTALL_SCRIPT,
        LICENSE_FILE,
        NOTICE_FILE,
        SBOM_FILE,
    ]);
    for entry in
        fs::read_dir(path).map_err(|_| "recoverable release could not be read".to_owned())?
    {
        let entry = entry.map_err(|_| "recoverable release entry could not be read".to_owned())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "recoverable release entry must be UTF-8".to_owned())?;
        if name == "bin" {
            validate_recoverable_directory(
                &fs::symlink_metadata(entry.path())
                    .map_err(|_| "recoverable bin metadata is unavailable".to_owned())?,
                "recoverable bin",
            )?;
            for child in fs::read_dir(entry.path())
                .map_err(|_| "recoverable bin could not be read".to_owned())?
            {
                let child =
                    child.map_err(|_| "recoverable bin entry could not be read".to_owned())?;
                if child.file_name() != OsStr::new(PACKAGE_NAME) {
                    return Err("recoverable bin contains an unmanaged entry".to_owned());
                }
                let metadata = fs::symlink_metadata(child.path())
                    .map_err(|_| "recoverable bin entry metadata is unavailable".to_owned())?;
                if metadata.file_type().is_symlink()
                    || !metadata.is_file()
                    || metadata.nlink() != 1
                    || metadata.uid() != current_uid()
                {
                    return Err("recoverable bin entry is unsafe".to_owned());
                }
            }
        } else if allowed_root.contains(name.as_str()) {
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| "recoverable release file metadata is unavailable".to_owned())?;
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.nlink() != 1
                || metadata.uid() != current_uid()
            {
                return Err("recoverable release file is unsafe".to_owned());
            }
        } else {
            return Err("recoverable release contains an unmanaged entry".to_owned());
        }
    }
    Ok(())
}

fn remove_authenticated_prefix(root: &Path) -> ReleaseResult<()> {
    let releases = root.join("releases");
    if fs::symlink_metadata(&releases).is_ok() {
        let paths = fs::read_dir(&releases)
            .map_err(|_| "uninstall release directory could not be read".to_owned())?
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|_| "uninstall release entry could not be read".to_owned())
            })
            .collect::<ReleaseResult<Vec<_>>>()?;
        for path in paths {
            transaction::checkpoint("uninstall-delete:before-release")?;
            remove_release_tree(&path, false)?;
            transaction::checkpoint("uninstall-delete:after-release")?;
        }
        fs::remove_dir(&releases)
            .map_err(|_| "uninstall releases directory was not empty".to_owned())?;
    }
    for link in ["current", "rollback"] {
        let path = root.join(link);
        if fs::symlink_metadata(&path).is_ok() {
            read_release_link(&path)?;
            fs::remove_file(&path)
                .map_err(|_| "uninstall release link could not be removed".to_owned())?;
        }
    }
    let marker = root.join(MARKER_FILE);
    if fs::symlink_metadata(&marker).is_ok() {
        transaction::validate_owned_path(&marker, 0o600, "uninstall marker")?;
        fs::remove_file(&marker).map_err(|_| "uninstall marker could not be removed".to_owned())?;
    }
    fs::remove_dir(root).map_err(|_| "uninstall root was not empty".to_owned())
}

fn remove_release_tree(path: &Path, complete: bool) -> ReleaseResult<()> {
    if complete {
        validate_installed_release(path)?;
    } else {
        let metadata =
            fs::symlink_metadata(path).map_err(|_| "install staging is unavailable".to_owned())?;
        validate_recoverable_directory(&metadata, "install staging")?;
    }
    let allowed_root = BTreeSet::from([
        MANIFEST_FILE,
        INSTALL_SCRIPT,
        LICENSE_FILE,
        NOTICE_FILE,
        SBOM_FILE,
        ".manifest.json.next",
        ".install.sh.next",
        ".LICENSE.next",
        ".THIRD_PARTY_NOTICES.txt.next",
        ".SBOM.spdx.json.next",
    ]);
    for entry in fs::read_dir(path).map_err(|_| "release tree could not be read".to_owned())? {
        let entry = entry.map_err(|_| "release tree entry could not be read".to_owned())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "release tree entry must be UTF-8".to_owned())?;
        if name == "bin" {
            validate_recoverable_directory(
                &fs::symlink_metadata(entry.path())
                    .map_err(|_| "release bin metadata is unavailable".to_owned())?,
                "release bin",
            )?;
            for child in fs::read_dir(entry.path())
                .map_err(|_| "release bin could not be read".to_owned())?
            {
                let child = child.map_err(|_| "release bin entry could not be read".to_owned())?;
                let child_name = child
                    .file_name()
                    .into_string()
                    .map_err(|_| "release bin entry must be UTF-8".to_owned())?;
                if !matches!(
                    child_name.as_str(),
                    PACKAGE_NAME | ".ai-computer-toolkit.next"
                ) {
                    return Err("release bin contains an unmanaged entry".to_owned());
                }
                remove_owned_regular(&child.path())?;
            }
            fs::remove_dir(entry.path()).map_err(|_| "release bin was not empty".to_owned())?;
        } else if allowed_root.contains(name.as_str()) {
            remove_owned_regular(&entry.path())?;
        } else {
            return Err("release tree contains an unmanaged entry".to_owned());
        }
    }
    fs::remove_dir(path).map_err(|_| "release tree was not empty".to_owned())
}

fn remove_owned_regular(path: &Path) -> ReleaseResult<()> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "release file is unavailable".to_owned())?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != current_uid()
    {
        return Err("release file cannot be safely removed".to_owned());
    }
    fs::remove_file(path).map_err(|_| "release file could not be removed".to_owned())
}

fn require_marker(root: &Path) -> ReleaseResult<()> {
    let marker = root.join(MARKER_FILE);
    transaction::validate_owned_path(&marker, 0o600, "install marker")?;
    if bounded_read_single_link(&marker, 128)? != INSTALL_MARKER.as_bytes() {
        return Err("install prefix marker is invalid".to_owned());
    }
    Ok(())
}

fn validate_owned_directory(metadata: &fs::Metadata, mode: u32, label: &str) -> ReleaseResult<()> {
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o777 != mode
    {
        return Err(format!("{label} ownership or mode is invalid"));
    }
    Ok(())
}

fn validate_recoverable_directory(metadata: &fs::Metadata, label: &str) -> ReleaseResult<()> {
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err(format!("{label} cannot be safely recovered"));
    }
    Ok(())
}

fn validate_managed_symlink(path: &Path, expected: &Path, label: &str) -> ReleaseResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| format!("{label} is unavailable"))?;
    if !metadata.file_type().is_symlink()
        || metadata.uid() != current_uid()
        || fs::read_link(path).map_err(|_| format!("{label} target could not be read"))? != expected
    {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

fn valid_release_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}
