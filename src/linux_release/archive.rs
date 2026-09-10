//! Linux Release System 的严格 archive 生成与验证 Component。

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
    path::{Component, Path},
};

#[cfg(feature = "linux-release-tools")]
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::PermissionsExt,
};

use tar::{Archive, EntryType};
#[cfg(feature = "linux-release-tools")]
use tar::{Builder, Header, HeaderMode};
#[cfg(feature = "linux-release-tools")]
use zstd::stream::write::Encoder;

use super::{
    ArchiveManifest, BufferedEntry, CONTRACT_VERSION, INSTALL_SCRIPT, LICENSE_FILE, MAIN_BINARY,
    MANIFEST_FILE, MAXIMUM_ARCHIVE_BYTES, MAXIMUM_ARCHIVE_ENTRIES, NOTICE_FILE, PACKAGE_NAME,
    ReleaseResult, SBOM_FILE, SiblingPolicy, elf, sha256,
};
#[cfg(feature = "linux-release-tools")]
use super::{sync_directory, temporary_sibling};

/// 以确定性元数据写入单一 zstd frame 与精确 tar 结束块。
#[cfg(feature = "linux-release-tools")]
pub(super) fn write_archive<'a>(
    path: &Path,
    root: &str,
    modified: u64,
    entries: impl IntoIterator<Item = (&'a str, u32, &'a [u8])>,
) -> ReleaseResult<()> {
    let temporary = temporary_sibling(path, "archive")?;
    let result = (|| {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "release archive staging file could not be created".to_owned())?;
        let mut encoder = Encoder::new(file, 19)
            .map_err(|_| "release zstd encoder could not be created".to_owned())?;
        encoder
            .include_checksum(true)
            .map_err(|_| "release zstd checksum could not be enabled".to_owned())?;
        let mut builder = Builder::new(encoder);
        builder.mode(HeaderMode::Deterministic);
        for (relative, mode, bytes) in entries {
            let mut header = Header::new_ustar();
            header.set_entry_type(EntryType::Regular);
            header.set_size(bytes.len() as u64);
            header.set_mode(mode);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(modified);
            header.set_cksum();
            builder
                .append_data(&mut header, format!("{root}/{relative}"), bytes)
                .map_err(|_| "release archive entry could not be written".to_owned())?;
        }
        let encoder = builder
            .into_inner()
            .map_err(|_| "release archive finalization failed".to_owned())?;
        let file = encoder
            .finish()
            .map_err(|_| "release zstd finalization failed".to_owned())?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o644))
            .map_err(|_| "release archive mode could not be set".to_owned())?;
        file.sync_all()
            .map_err(|_| "release archive synchronization failed".to_owned())?;
        fs::rename(&temporary, path)
            .map_err(|_| "release archive atomic commit failed".to_owned())?;
        sync_directory(
            path.parent()
                .ok_or_else(|| "release archive parent is invalid".to_owned())?,
        )
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// 验证只有一个 zstd frame，并严格消费唯一 tar 物理输入。
pub(super) fn read_and_validate_archive(
    bytes: &[u8],
) -> ReleaseResult<(ArchiveManifest, Vec<BufferedEntry>)> {
    let frame_size = zstd::zstd_safe::find_frame_compressed_size(bytes)
        .map_err(|_| "zstd archive frame is invalid".to_owned())?;
    if frame_size != bytes.len() {
        return Err("zstd archive contains trailing data or concatenated frames".to_owned());
    }
    let mut decoder = zstd::stream::read::Decoder::new(Cursor::new(bytes))
        .map_err(|_| "zstd archive decoder could not be created".to_owned())?
        .single_frame();
    let mut tar_bytes = Vec::new();
    decoder
        .by_ref()
        .take(MAXIMUM_ARCHIVE_BYTES + 1)
        .read_to_end(&mut tar_bytes)
        .map_err(|_| "zstd archive decoding failed".to_owned())?;
    if tar_bytes.len() as u64 > MAXIMUM_ARCHIVE_BYTES {
        return Err("archive payload exceeds the size limit".to_owned());
    }
    validate_tar_physical_input(&tar_bytes)?;
    let mut archive = Archive::new(tar_bytes.as_slice());
    let mut entries = Vec::new();
    let mut total = 0_u64;
    for entry in archive
        .entries()
        .map_err(|_| "archive entries could not be read".to_owned())?
    {
        if entries.len() >= MAXIMUM_ARCHIVE_ENTRIES {
            return Err("archive contains too many entries".to_owned());
        }
        let mut entry = entry.map_err(|_| "archive entry could not be read".to_owned())?;
        if entry.header().entry_type() != EntryType::Regular {
            return Err("archive contains a non-regular entry".to_owned());
        }
        let path = safe_archive_path(&entry.path().map_err(|_| "archive path is invalid")?)?;
        let size = entry
            .header()
            .size()
            .map_err(|_| "archive size is invalid".to_owned())?;
        total = total
            .checked_add(size)
            .ok_or_else(|| "archive payload size overflowed".to_owned())?;
        if total > MAXIMUM_ARCHIVE_BYTES {
            return Err("archive payload exceeds the size limit".to_owned());
        }
        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .map_err(|_| "archive entry content could not be read".to_owned())?;
        if data.len() as u64 != size {
            return Err("archive entry content is truncated".to_owned());
        }
        entries.push(BufferedEntry {
            path,
            mode: entry
                .header()
                .mode()
                .map_err(|_| "archive mode is invalid".to_owned())?
                & 0o777,
            uid: entry
                .header()
                .uid()
                .map_err(|_| "archive uid is invalid".to_owned())?,
            gid: entry
                .header()
                .gid()
                .map_err(|_| "archive gid is invalid".to_owned())?,
            modified: entry
                .header()
                .mtime()
                .map_err(|_| "archive timestamp is invalid".to_owned())?,
            bytes: data,
        });
    }
    let manifest_entry = entries
        .iter()
        .find(|entry| entry.path.ends_with(&format!("/{MANIFEST_FILE}")))
        .ok_or_else(|| "archive manifest is missing".to_owned())?;
    let manifest: ArchiveManifest = serde_json::from_slice(&manifest_entry.bytes)
        .map_err(|_| "archive manifest is invalid".to_owned())?;
    validate_manifest(&manifest, &entries)?;
    Ok((manifest, entries))
}

/// 逐 header 计算 tar 物理终点，拒绝结束块之后的任何字节。
fn validate_tar_physical_input(bytes: &[u8]) -> ReleaseResult<()> {
    const BLOCK: usize = 512;
    if bytes.len() < BLOCK * 2 || bytes.len() % BLOCK != 0 {
        return Err("tar physical input length is invalid".to_owned());
    }
    let mut offset = 0_usize;
    let mut entries = 0_usize;
    loop {
        let header = bytes
            .get(offset..offset + BLOCK)
            .ok_or_else(|| "tar physical input is truncated".to_owned())?;
        if header.iter().all(|byte| *byte == 0) {
            let second = bytes
                .get(offset + BLOCK..offset + BLOCK * 2)
                .ok_or_else(|| "tar end marker is truncated".to_owned())?;
            if second.iter().any(|byte| *byte != 0) || offset + BLOCK * 2 != bytes.len() {
                return Err("tar contains trailing physical input".to_owned());
            }
            return Ok(());
        }
        entries = entries.saturating_add(1);
        if entries > MAXIMUM_ARCHIVE_ENTRIES {
            return Err("archive contains too many physical entries".to_owned());
        }
        let size = parse_tar_octal(&header[124..136])?;
        let padded = size
            .checked_add((BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64)
            .ok_or_else(|| "tar entry size overflowed".to_owned())?;
        offset = offset
            .checked_add(BLOCK)
            .and_then(|value| value.checked_add(usize::try_from(padded).ok()?))
            .ok_or_else(|| "tar entry offset overflowed".to_owned())?;
        if offset > bytes.len().saturating_sub(BLOCK * 2) {
            return Err("tar physical entry exceeds the end marker".to_owned());
        }
    }
}

/// 仅接受 ustar header 的空格/NUL 结尾八进制 size。
fn parse_tar_octal(field: &[u8]) -> ReleaseResult<u64> {
    let text = field
        .iter()
        .copied()
        .skip_while(|byte| matches!(byte, b' ' | 0))
        .take_while(|byte| !matches!(byte, b' ' | 0))
        .collect::<Vec<_>>();
    if text.is_empty() || !text.iter().all(|byte| matches!(byte, b'0'..=b'7')) {
        return Err("tar entry size field is malformed".to_owned());
    }
    let text = std::str::from_utf8(&text).map_err(|_| "tar entry size is invalid".to_owned())?;
    u64::from_str_radix(text, 8).map_err(|_| "tar entry size is invalid".to_owned())
}

/// 验证 manifest 精确策略和六项 archive 内容。
pub(super) fn validate_manifest(
    manifest: &ArchiveManifest,
    entries: &[BufferedEntry],
) -> ReleaseResult<()> {
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
    {
        return Err("archive manifest policy is invalid".to_owned());
    }
    validate_manifest_files(manifest, entries)
}

fn validate_manifest_files(
    manifest: &ArchiveManifest,
    entries: &[BufferedEntry],
) -> ReleaseResult<()> {
    let mut expected = BTreeMap::new();
    for file in &manifest.files {
        if expected.insert(file.path.as_str(), file).is_some() {
            return Err("archive manifest has duplicate files".to_owned());
        }
    }
    if expected.keys().copied().collect::<BTreeSet<_>>()
        != BTreeSet::from([
            MAIN_BINARY,
            INSTALL_SCRIPT,
            LICENSE_FILE,
            NOTICE_FILE,
            SBOM_FILE,
        ])
    {
        return Err("archive manifest payload is not allowlisted".to_owned());
    }
    let expected_policy = BTreeMap::from([
        (MAIN_BINARY, ("main-cli", 0o755)),
        (INSTALL_SCRIPT, ("managed-installer", 0o755)),
        (LICENSE_FILE, ("license", 0o644)),
        (NOTICE_FILE, ("third-party-notices", 0o644)),
        (SBOM_FILE, ("spdx-sbom", 0o644)),
    ]);
    for (path, file) in &expected {
        let Some((role, mode)) = expected_policy.get(path) else {
            return Err("archive manifest payload role is invalid".to_owned());
        };
        if file.role != *role
            || file.mode != *mode
            || file.sha256.len() != 64
            || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("archive manifest payload policy is invalid".to_owned());
        }
    }
    let mut found = BTreeSet::new();
    for entry in entries {
        let prefix = format!("{}/", manifest.archive_root);
        let relative = entry
            .path
            .strip_prefix(&prefix)
            .ok_or_else(|| "archive entry is outside the declared root".to_owned())?;
        if entry.uid != 0 || entry.gid != 0 || !found.insert(relative.to_owned()) {
            return Err("archive entry ownership or uniqueness is invalid".to_owned());
        }
        if relative == MANIFEST_FILE {
            if entry.mode != 0o644 || entry.modified != manifest.source_date_epoch {
                return Err("archive manifest metadata is invalid".to_owned());
            }
            continue;
        }
        let file = expected
            .get(relative)
            .ok_or_else(|| "archive contains an undeclared file".to_owned())?;
        if entry.mode != file.mode
            || entry.modified != manifest.source_date_epoch
            || sha256(&entry.bytes) != file.sha256
        {
            return Err("archive payload verification failed".to_owned());
        }
    }
    let expected_found = BTreeSet::from([
        MAIN_BINARY.to_owned(),
        INSTALL_SCRIPT.to_owned(),
        LICENSE_FILE.to_owned(),
        NOTICE_FILE.to_owned(),
        SBOM_FILE.to_owned(),
        MANIFEST_FILE.to_owned(),
    ]);
    if found != expected_found {
        return Err("archive payload is incomplete".to_owned());
    }
    Ok(())
}

/// 只接受冻结根深度内的 UTF-8 相对路径。
pub(super) fn safe_archive_path(path: &Path) -> ReleaseResult<String> {
    if path.is_absolute() {
        return Err("archive path must be relative".to_owned());
    }
    let lexical = path
        .to_str()
        .ok_or_else(|| "archive path must be UTF-8".to_owned())?;
    if lexical
        .split('/')
        .any(|part| matches!(part, "." | ".." | ""))
    {
        return Err("archive path traversal was rejected".to_owned());
    }
    let mut parts = Vec::new();
    for part in path.components() {
        match part {
            Component::Normal(value) => parts.push(
                value
                    .to_str()
                    .ok_or_else(|| "archive path must be UTF-8".to_owned())?,
            ),
            _ => return Err("archive path traversal was rejected".to_owned()),
        }
    }
    if !(parts.len() == 2 || (parts.len() == 3 && parts[1] == "bin")) {
        return Err("archive path layout is invalid".to_owned());
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{parse_tar_octal, safe_archive_path, validate_tar_physical_input};

    #[test]
    fn archive_path_policy_rejects_traversal_and_unmanaged_depth() {
        assert!(safe_archive_path(Path::new("root/manifest.json")).is_ok());
        assert!(safe_archive_path(Path::new("root/bin/ai-computer-toolkit")).is_ok());
        for path in [
            "/root/manifest.json",
            "root/../manifest.json",
            "root/./manifest.json",
            "root/bin/nested/unmanaged",
            "manifest.json",
        ] {
            assert!(
                safe_archive_path(Path::new(path)).is_err(),
                "accepted {path}"
            );
        }
    }

    #[test]
    fn tar_physical_gate_requires_exact_two_block_terminator() {
        assert_eq!(parse_tar_octal(b"00000000001\0").unwrap(), 1);
        assert!(validate_tar_physical_input(&[0_u8; 1024]).is_ok());
        assert!(validate_tar_physical_input(&[0_u8; 1536]).is_err());
    }
}
