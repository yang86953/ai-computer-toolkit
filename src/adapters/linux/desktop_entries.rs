//! XDG Desktop Entry 已安装应用私有 Adapter。

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    env,
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::Read,
    os::fd::OwnedFd,
    os::unix::ffi::OsStringExt,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
};

use rustix::{
    fs::{self as unix_fs, AtFlags, Dir, FileType, Mode, OFlags, ResolveFlags},
    io::Errno,
};

use crate::components::opaque_id::{OpaqueTargetId, OpaqueTargetKind};

use super::desktop_entry_launch::{self, DesktopEntryLaunchRoute};

pub(crate) const DISCOVERY_SOURCE: &str = "xdg-desktop-entry";
const MAXIMUM_ROOTS: usize = 64;
const MAXIMUM_PATH_DIRECTORIES: usize = 256;
const MAXIMUM_DIRECTORY_DEPTH: usize = 16;
const MAXIMUM_SCANNED_DIRECTORY_ENTRIES: usize = 32_768;
const MAXIMUM_FILE_BYTES: u64 = 1_048_576;
const MAXIMUM_LINES: usize = 8_192;
const MAXIMUM_LINE_BYTES: usize = 65_536;
const MAXIMUM_NAME_BYTES: usize = 4_096;
const WARNING_INVALID_ENTRY: &str = "desktop-entry-invalid-skipped";
const WARNING_READ_INCOMPLETE: &str = "desktop-entry-read-incomplete";
const WARNING_ID_CONFLICT: &str = "desktop-file-id-conflict";
const WARNING_SCAN_LIMIT: &str = "desktop-entry-scan-limit-reached";
const WARNING_UNSAFE_ENTRY: &str = "unsafe-filesystem-entry-skipped";
const WARNING_ROOT_UNREADABLE: &str = "xdg-application-root-unreadable";
const WARNING_ROOTS_MISSING: &str = "xdg-application-roots-missing";

/// 只向上层返回 provider-neutral 的应用身份与显示名。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopEntryRecord {
    pub(crate) session_id: String,
    pub(crate) display_name: String,
    pub(crate) launch_route: Option<DesktopEntryLaunchRoute>,
}

/// 保存有界清单和来源完整性，不泄漏路径或 desktop-file 内容。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopEntryInventory {
    pub(crate) records: Vec<DesktopEntryRecord>,
    pub(crate) available: bool,
    pub(crate) complete: bool,
    pub(crate) applications_returned: usize,
    pub(crate) applications_truncated: bool,
    pub(crate) total_eligible_applications: Option<usize>,
    pub(crate) warnings: Vec<&'static str>,
}

trait EnvironmentSource {
    fn variable(&self, name: &str) -> Option<OsString>;
}

struct SystemEnvironment;

impl EnvironmentSource for SystemEnvironment {
    fn variable(&self, name: &str) -> Option<OsString> {
        env::var_os(name)
    }
}

#[derive(Clone, Debug)]
struct ScanConfiguration {
    roots: Vec<PathBuf>,
    current_desktops: Vec<String>,
    locale: String,
    path_directories: Vec<PathBuf>,
    complete: bool,
}

#[derive(Clone, Debug)]
struct Candidate {
    desktop_file_id: String,
    relative_path: PathBuf,
    observed: FileSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileSnapshot {
    device: u64,
    inode: u64,
    link_count: u64,
    mode: u32,
    size: i64,
    modified_seconds: i64,
    modified_nanoseconds: u64,
    changed_seconds: i64,
    changed_nanoseconds: u64,
}

#[derive(Clone, Debug)]
struct ParsedEntry {
    fields: HashMap<String, String>,
    localized_names: HashMap<String, String>,
    generation: FileGeneration,
}

#[derive(Clone, Debug)]
struct FileGeneration {
    device: u64,
    inode: u64,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: u64,
    changed_seconds: i64,
    changed_nanoseconds: u64,
    content_digest: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadFailure {
    Invalid,
    Incomplete,
}

fn bounded_absolute_paths(raw: &OsStr, maximum: usize, complete: &mut bool) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for path in env::split_paths(raw) {
        if path.as_os_str().is_empty() || !path.is_absolute() {
            *complete = false;
            continue;
        }
        if paths.len() >= maximum {
            *complete = false;
            break;
        }
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

fn configuration(source: &impl EnvironmentSource) -> ScanConfiguration {
    let mut complete = true;
    let mut roots = Vec::new();
    match source
        .variable("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
    {
        Some(value) => {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                roots.push(path);
            } else {
                complete = false;
            }
        }
        None => match source.variable("HOME").filter(|value| !value.is_empty()) {
            Some(value) => {
                let path = PathBuf::from(value);
                if path.is_absolute() {
                    roots.push(path.join(".local/share"));
                } else {
                    complete = false;
                }
            }
            None => complete = false,
        },
    }
    let data_directories = source
        .variable("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .map_or_else(
            || {
                vec![
                    PathBuf::from("/usr/local/share"),
                    PathBuf::from("/usr/share"),
                ]
            },
            |value| bounded_absolute_paths(&value, MAXIMUM_ROOTS, &mut complete),
        );
    for path in data_directories {
        if roots.len() >= MAXIMUM_ROOTS {
            complete = false;
            break;
        }
        if !roots.contains(&path) {
            roots.push(path);
        }
    }

    let current_desktops = source
        .variable("XDG_CURRENT_DESKTOP")
        .and_then(|value| value.into_string().ok())
        .map(|value| {
            value
                .split(':')
                .filter(|entry| !entry.is_empty() && entry.len() <= 128)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| {
            source
                .variable(name)
                .filter(|value| !value.is_empty())
                .and_then(|value| value.into_string().ok())
        })
        .unwrap_or_else(|| "C".to_owned());
    let path_directories = source.variable("PATH").map_or_else(Vec::new, |value| {
        bounded_absolute_paths(&value, MAXIMUM_PATH_DIRECTORIES, &mut complete)
    });
    ScanConfiguration {
        roots,
        current_desktops,
        locale,
        path_directories,
        complete,
    }
}

fn desktop_file_id(relative: &Path) -> Option<String> {
    if relative.extension().and_then(OsStr::to_str) != Some("desktop") {
        return None;
    }
    let parts = relative
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    (!parts.is_empty()).then(|| parts.join("-"))
}

#[derive(Debug)]
enum ApplicationRoot {
    Missing,
    Ready(OwnedFd),
    Unsafe,
    Unreadable,
}

fn directory_open_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC
}

fn open_applications_root(root: &Path) -> ApplicationRoot {
    let path = root.join("applications");
    if !path.is_absolute() {
        return ApplicationRoot::Unsafe;
    }
    let mut directory = match unix_fs::open("/", directory_open_flags(), Mode::empty()) {
        Ok(directory) => directory,
        Err(_) => return ApplicationRoot::Unreadable,
    };
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(name) => {
                directory = match unix_fs::openat(
                    &directory,
                    name,
                    directory_open_flags(),
                    Mode::empty(),
                ) {
                    Ok(next) => next,
                    Err(Errno::NOENT) => return ApplicationRoot::Missing,
                    Err(Errno::LOOP | Errno::NOTDIR) => return ApplicationRoot::Unsafe,
                    Err(_) => return ApplicationRoot::Unreadable,
                };
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return ApplicationRoot::Unsafe;
            }
        }
    }
    ApplicationRoot::Ready(directory)
}

fn snapshot(stat: &unix_fs::Stat) -> FileSnapshot {
    FileSnapshot {
        device: stat.st_dev,
        inode: stat.st_ino,
        link_count: stat.st_nlink,
        mode: stat.st_mode,
        size: stat.st_size,
        modified_seconds: stat.st_mtime,
        modified_nanoseconds: stat.st_mtime_nsec,
        changed_seconds: stat.st_ctime,
        changed_nanoseconds: stat.st_ctime_nsec,
    }
}

fn collect_directory(
    directory: &OwnedFd,
    relative_directory: &Path,
    depth: usize,
    candidates: &mut Vec<Candidate>,
    remaining_directory_entries: &mut usize,
    complete: &mut bool,
    warnings: &mut BTreeSet<&'static str>,
) {
    let before = match unix_fs::fstat(directory).map(|stat| snapshot(&stat)) {
        Ok(before) if FileType::from_raw_mode(before.mode) == FileType::Directory => before,
        _ => {
            *complete = false;
            warnings.insert(WARNING_READ_INCOMPLETE);
            return;
        }
    };
    let mut stream = match Dir::read_from(directory) {
        Ok(stream) => stream,
        Err(_) => {
            *complete = false;
            warnings.insert(WARNING_READ_INCOMPLETE);
            return;
        }
    };
    let mut names = Vec::new();
    let mut budget_exhausted = false;
    while let Some(entry) = stream.read() {
        match entry {
            Ok(entry) => {
                let bytes = entry.file_name().to_bytes();
                if bytes != b"." && bytes != b".." {
                    if *remaining_directory_entries == 0 {
                        budget_exhausted = true;
                        break;
                    }
                    *remaining_directory_entries -= 1;
                    names.push(OsString::from_vec(bytes.to_vec()));
                }
            }
            Err(_) => {
                *complete = false;
                warnings.insert(WARNING_READ_INCOMPLETE);
            }
        }
    }
    if unix_fs::fstat(directory)
        .map(|stat| snapshot(&stat))
        .as_ref()
        != Ok(&before)
    {
        *complete = false;
        warnings.insert(WARNING_READ_INCOMPLETE);
        return;
    }
    if budget_exhausted {
        *complete = false;
        warnings.insert(WARNING_SCAN_LIMIT);
        return;
    }
    names.sort();
    for name in names {
        let relative = relative_directory.join(&name);
        let observed = match unix_fs::statat(directory, &name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => snapshot(&stat),
            Err(_) => {
                *complete = false;
                warnings.insert(WARNING_READ_INCOMPLETE);
                continue;
            }
        };
        match FileType::from_raw_mode(observed.mode) {
            FileType::Directory => {
                if depth >= MAXIMUM_DIRECTORY_DEPTH {
                    *complete = false;
                    warnings.insert(WARNING_SCAN_LIMIT);
                    continue;
                }
                let child = match unix_fs::openat(
                    directory,
                    &name,
                    directory_open_flags(),
                    Mode::empty(),
                ) {
                    Ok(child) => child,
                    Err(_) => {
                        *complete = false;
                        warnings.insert(WARNING_UNSAFE_ENTRY);
                        continue;
                    }
                };
                let verified = unix_fs::fstat(&child).map(|stat| snapshot(&stat));
                if verified.as_ref() != Ok(&observed) {
                    *complete = false;
                    warnings.insert(WARNING_UNSAFE_ENTRY);
                    continue;
                }
                collect_directory(
                    &child,
                    &relative,
                    depth.saturating_add(1),
                    candidates,
                    remaining_directory_entries,
                    complete,
                    warnings,
                );
            }
            FileType::RegularFile => {
                let Some(id) = desktop_file_id(&relative) else {
                    continue;
                };
                candidates.push(Candidate {
                    desktop_file_id: id,
                    relative_path: relative,
                    observed,
                });
            }
            _ => {
                if desktop_file_id(&relative).is_some() {
                    *complete = false;
                    warnings.insert(WARNING_UNSAFE_ENTRY);
                }
            }
        }
    }
}

fn collect_candidates(
    root: &OwnedFd,
    remaining_directory_entries: &mut usize,
    complete: &mut bool,
    warnings: &mut BTreeSet<&'static str>,
) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    collect_directory(
        root,
        Path::new(""),
        0,
        &mut candidates,
        remaining_directory_entries,
        complete,
        warnings,
    );
    candidates
}

fn valid_base_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 256
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_locale(locale: &str) -> bool {
    !locale.is_empty()
        && locale.len() <= 128
        && locale
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'@'))
}

fn read_entry(root: &OwnedFd, candidate: &Candidate) -> Result<ParsedEntry, ReadFailure> {
    let before = unix_fs::statat(root, &candidate.relative_path, AtFlags::SYMLINK_NOFOLLOW)
        .map(|stat| snapshot(&stat))
        .map_err(|_| ReadFailure::Incomplete)?;
    if before != candidate.observed || FileType::from_raw_mode(before.mode) != FileType::RegularFile
    {
        return Err(ReadFailure::Incomplete);
    }
    let descriptor = unix_fs::openat2(
        root,
        &candidate.relative_path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )
    .map_err(|_| ReadFailure::Incomplete)?;
    let opened = unix_fs::fstat(&descriptor)
        .map(|stat| snapshot(&stat))
        .map_err(|_| ReadFailure::Incomplete)?;
    if opened != before
        || FileType::from_raw_mode(opened.mode) != FileType::RegularFile
        || opened.size < 0
        || u64::try_from(opened.size).unwrap_or(u64::MAX) > MAXIMUM_FILE_BYTES
    {
        return Err(ReadFailure::Incomplete);
    }
    let mut file = File::from(descriptor);
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAXIMUM_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ReadFailure::Incomplete)?;
    let after = unix_fs::fstat(&file)
        .map(|stat| snapshot(&stat))
        .map_err(|_| ReadFailure::Incomplete)?;
    if after != opened
        || usize::try_from(opened.size).ok() != Some(bytes.len())
        || u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_FILE_BYTES
    {
        return Err(ReadFailure::Incomplete);
    }
    let content_digest = fnv1a_digest(&bytes);
    let text = String::from_utf8(bytes).map_err(|_| ReadFailure::Invalid)?;
    let mut parsed = ParsedEntry {
        fields: HashMap::new(),
        localized_names: HashMap::new(),
        generation: FileGeneration {
            device: opened.device,
            inode: opened.inode,
            size: u64::try_from(opened.size).map_err(|_| ReadFailure::Incomplete)?,
            modified_seconds: opened.modified_seconds,
            modified_nanoseconds: opened.modified_nanoseconds,
            changed_seconds: opened.changed_seconds,
            changed_nanoseconds: opened.changed_nanoseconds,
            content_digest,
        },
    };
    let mut in_desktop_group = false;
    let mut desktop_group_seen = false;
    let mut seen_keys = HashSet::new();
    for (index, raw_line) in text.lines().enumerate() {
        if index >= MAXIMUM_LINES || raw_line.len() > MAXIMUM_LINE_BYTES {
            return Err(ReadFailure::Incomplete);
        }
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if line == "[Desktop Entry]" {
                if desktop_group_seen {
                    return Err(ReadFailure::Invalid);
                }
                desktop_group_seen = true;
                in_desktop_group = true;
            } else {
                in_desktop_group = false;
            }
            continue;
        }
        if !in_desktop_group {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or(ReadFailure::Invalid)?;
        if !seen_keys.insert(key.to_owned()) {
            return Err(ReadFailure::Invalid);
        }
        if let Some((base, locale)) = key
            .split_once('[')
            .and_then(|(base, locale)| locale.strip_suffix(']').map(|locale| (base, locale)))
        {
            if !valid_base_key(base) || !valid_locale(locale) || locale.contains(['[', ']']) {
                return Err(ReadFailure::Invalid);
            }
            if base == "Name" {
                parsed
                    .localized_names
                    .insert(locale.to_owned(), value.to_owned());
            }
        } else if !valid_base_key(key)
            || parsed
                .fields
                .insert(key.to_owned(), value.to_owned())
                .is_some()
        {
            return Err(ReadFailure::Invalid);
        }
    }
    if !desktop_group_seen {
        return Err(ReadFailure::Invalid);
    }
    Ok(parsed)
}

fn fnv1a_digest(bytes: &[u8]) -> u64 {
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
}

fn unescape(value: &str) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        output.push(match characters.next()? {
            's' => ' ',
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '\\' => '\\',
            _ => return None,
        });
    }
    Some(output)
}

fn parse_boolean(value: Option<&String>) -> Option<bool> {
    match value.map(String::as_str) {
        None | Some("false") => Some(false),
        Some("true") => Some(true),
        _ => None,
    }
}

fn parse_list(value: &str) -> Option<Vec<String>> {
    let mut values = Vec::new();
    let mut item = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        match character {
            ';' => {
                if !item.is_empty() {
                    values.push(std::mem::take(&mut item));
                }
            }
            '\\' => item.push(match characters.next()? {
                's' => ' ',
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '\\' => '\\',
                ';' => ';',
                _ => return None,
            }),
            other => item.push(other),
        }
    }
    if !item.is_empty() {
        values.push(item);
    }
    Some(values)
}

fn locale_candidates(locale: &str) -> Vec<String> {
    let locale = locale.trim();
    if locale.is_empty() || locale == "C" || locale == "POSIX" || locale.starts_with("C.") {
        return Vec::new();
    }
    let (base_with_encoding, modifier) = locale
        .split_once('@')
        .map_or((locale, None), |(base, modifier)| (base, Some(modifier)));
    let base = base_with_encoding
        .split_once('.')
        .map_or(base_with_encoding, |(base, _)| base);
    let (language, country) = base
        .split_once('_')
        .map_or((base, None), |(language, country)| {
            (language, Some(country))
        });
    let mut candidates = Vec::new();
    if let Some(country) = country {
        if let Some(modifier) = modifier {
            candidates.push(format!("{language}_{country}@{modifier}"));
        }
        candidates.push(format!("{language}_{country}"));
    }
    if let Some(modifier) = modifier {
        candidates.push(format!("{language}@{modifier}"));
    }
    candidates.push(language.to_owned());
    candidates
}

fn selected_name(entry: &ParsedEntry, locale: &str) -> Option<String> {
    let base = entry.fields.get("Name")?;
    let base = unescape(base)?;
    if base.trim().is_empty() || base.len() > MAXIMUM_NAME_BYTES {
        return None;
    }
    let value = locale_candidates(locale)
        .into_iter()
        .find_map(|candidate| entry.localized_names.get(&candidate))
        .or_else(|| entry.fields.get("Name"))?;
    let name = unescape(value)?;
    let name = name.trim();
    (!name.is_empty() && name.len() <= MAXIMUM_NAME_BYTES).then(|| name.to_owned())
}

fn visible_in_desktop(entry: &ParsedEntry, desktops: &[String]) -> Option<bool> {
    let only = match entry.fields.get("OnlyShowIn") {
        Some(value) => Some(parse_list(value)?),
        None => None,
    };
    let not = match entry.fields.get("NotShowIn") {
        Some(value) => Some(parse_list(value)?),
        None => None,
    };
    if only.as_ref().is_some_and(|only| {
        not.as_ref()
            .is_some_and(|not| only.iter().any(|desktop| not.contains(desktop)))
    }) {
        return None;
    }
    for desktop in desktops {
        if only
            .as_ref()
            .is_some_and(|entries| entries.contains(desktop))
        {
            return Some(true);
        }
        if not
            .as_ref()
            .is_some_and(|entries| entries.contains(desktop))
        {
            return Some(false);
        }
    }
    Some(only.is_none())
}

fn try_exec_available(value: &str, path_directories: &[PathBuf]) -> bool {
    let Some(value) = unescape(value) else {
        return false;
    };
    let candidate = Path::new(&value);
    let candidates = if candidate.is_absolute() {
        vec![candidate.to_path_buf()]
    } else if candidate.components().count() == 1 {
        path_directories
            .iter()
            .map(|directory| directory.join(candidate))
            .collect()
    } else {
        Vec::new()
    };
    candidates.into_iter().any(|path| {
        fs::metadata(path).ok().is_some_and(|metadata| {
            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
        })
    })
}

enum Projection {
    Eligible(DesktopEntryRecord),
    Filtered,
    Invalid,
}

fn project_entry(
    desktop_file_id: &str,
    root_priority: usize,
    winner_relative_path: &Path,
    entry: &ParsedEntry,
    configuration: &ScanConfiguration,
) -> Projection {
    if entry.fields.get("Type").map(String::as_str) != Some("Application") {
        return Projection::Filtered;
    }
    let Some(hidden) = parse_boolean(entry.fields.get("Hidden")) else {
        return Projection::Invalid;
    };
    let Some(_no_display) = parse_boolean(entry.fields.get("NoDisplay")) else {
        return Projection::Invalid;
    };
    let Some(dbus_activatable) = parse_boolean(entry.fields.get("DBusActivatable")) else {
        return Projection::Invalid;
    };
    let Some(visible) = visible_in_desktop(entry, &configuration.current_desktops) else {
        return Projection::Invalid;
    };
    if hidden || !visible {
        return Projection::Filtered;
    }
    if !dbus_activatable
        && !entry
            .fields
            .get("Exec")
            .and_then(|value| unescape(value))
            .is_some_and(|value| !value.trim().is_empty())
    {
        return Projection::Invalid;
    }
    if entry
        .fields
        .get("TryExec")
        .is_some_and(|value| !try_exec_available(value, &configuration.path_directories))
    {
        return Projection::Filtered;
    }
    let Some(display_name) = selected_name(entry, &configuration.locale) else {
        return Projection::Invalid;
    };
    // 私有 identity 同时绑定 provider、规范 ID、优先级胜出项、路径与文件/内容代际。
    let identity = format!(
        "linux-xdg-desktop-entry-v2\n{desktop_file_id}\n{root_priority}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{:016x}",
        winner_relative_path.to_string_lossy(),
        entry.generation.device,
        entry.generation.inode,
        entry.generation.size,
        entry.generation.modified_seconds,
        entry.generation.modified_nanoseconds,
        entry.generation.changed_seconds,
        entry.generation.changed_nanoseconds,
        entry.generation.content_digest,
    );
    Projection::Eligible(DesktopEntryRecord {
        session_id: OpaqueTargetId::new(OpaqueTargetKind::Application, &identity).to_string(),
        display_name,
        launch_route: desktop_entry_launch::classify(
            entry.fields.get("Exec").map(String::as_str),
            dbus_activatable,
        ),
    })
}

fn enumerate_with(source: &impl EnvironmentSource, maximum_items: usize) -> DesktopEntryInventory {
    let configuration = configuration(source);
    let mut claims = HashSet::new();
    let mut records = Vec::new();
    let mut available = false;
    let mut complete = configuration.complete;
    let mut warnings = BTreeSet::new();
    let mut remaining_directory_entries = MAXIMUM_SCANNED_DIRECTORY_ENTRIES;
    for (root_priority, root) in configuration.roots.iter().enumerate() {
        let root = match open_applications_root(root) {
            ApplicationRoot::Missing => continue,
            ApplicationRoot::Ready(root) => {
                available = true;
                root
            }
            ApplicationRoot::Unsafe => {
                complete = false;
                warnings.insert(WARNING_UNSAFE_ENTRY);
                continue;
            }
            ApplicationRoot::Unreadable => {
                complete = false;
                warnings.insert(WARNING_ROOT_UNREADABLE);
                continue;
            }
        };
        let candidates = collect_candidates(
            &root,
            &mut remaining_directory_entries,
            &mut complete,
            &mut warnings,
        );
        let mut by_id = BTreeMap::<String, Vec<Candidate>>::new();
        for candidate in candidates {
            by_id
                .entry(candidate.desktop_file_id.clone())
                .or_default()
                .push(candidate);
        }
        for (desktop_file_id, candidates) in by_id {
            if !claims.insert(desktop_file_id.clone()) {
                continue;
            }
            if candidates.len() != 1 {
                complete = false;
                warnings.insert(WARNING_ID_CONFLICT);
                continue;
            }
            let candidate = &candidates[0];
            let entry = match read_entry(&root, candidate) {
                Ok(entry) => entry,
                Err(ReadFailure::Invalid) => {
                    warnings.insert(WARNING_INVALID_ENTRY);
                    continue;
                }
                Err(ReadFailure::Incomplete) => {
                    complete = false;
                    warnings.insert(WARNING_READ_INCOMPLETE);
                    continue;
                }
            };
            match project_entry(
                &desktop_file_id,
                root_priority,
                &candidate.relative_path,
                &entry,
                &configuration,
            ) {
                Projection::Eligible(record) => records.push(record),
                Projection::Filtered => {}
                Projection::Invalid => {
                    complete = false;
                    warnings.insert(WARNING_INVALID_ENTRY);
                }
            }
        }
    }
    records.sort_by(|left, right| {
        left.display_name
            .cmp(&right.display_name)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    if !available {
        complete = false;
        warnings.insert(WARNING_ROOTS_MISSING);
    }
    let scan_complete = complete;
    let total_eligible_applications = scan_complete.then_some(records.len());
    let applications_truncated = records.len() > maximum_items;
    if applications_truncated {
        records.truncate(maximum_items);
        complete = false;
    }
    DesktopEntryInventory {
        applications_returned: records.len(),
        records,
        available,
        complete,
        applications_truncated,
        total_eligible_applications,
        warnings: warnings.into_iter().collect(),
    }
}

/// 按 XDG 优先级枚举 Desktop Entry；只读文件，不执行任何声明字段。
pub(crate) fn enumerate(maximum_items: usize) -> DesktopEntryInventory {
    enumerate_with(&SystemEnvironment, maximum_items)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        os::unix::net::UnixListener,
        process::{Command, Stdio},
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    use super::*;

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct FixtureEnvironment {
        values: HashMap<String, OsString>,
    }

    impl EnvironmentSource for FixtureEnvironment {
        fn variable(&self, name: &str) -> Option<OsString> {
            self.values.get(name).cloned()
        }
    }

    fn fixture_directory() -> PathBuf {
        let directory = env::temp_dir().join(format!(
            "ai-computer-toolkit-desktop-entry-{}-{}",
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory)
            .unwrap_or_else(|error| panic!("建立 Desktop Entry fixture 失败：{error}"));
        directory
    }

    fn write_entry(root: &Path, relative: &str, body: &str) {
        let path = root.join("applications").join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("建立 fixture 子目录失败：{error}"));
        }
        fs::write(path, body).unwrap_or_else(|error| panic!("写入 fixture 失败：{error}"));
    }

    fn fixture_environment(user: &Path, system: &Path, bin: &Path) -> FixtureEnvironment {
        FixtureEnvironment {
            values: HashMap::from([
                ("XDG_DATA_HOME".to_owned(), user.as_os_str().to_owned()),
                ("XDG_DATA_DIRS".to_owned(), system.as_os_str().to_owned()),
                ("XDG_CURRENT_DESKTOP".to_owned(), OsString::from("KDE")),
                ("LC_MESSAGES".to_owned(), OsString::from("zh_CN.UTF-8")),
                ("PATH".to_owned(), bin.as_os_str().to_owned()),
            ]),
        }
    }

    #[test]
    fn fixture_applies_precedence_visibility_try_exec_and_locale_without_execution() {
        let directory = fixture_directory();
        let user = directory.join("user");
        let system = directory.join("system");
        let bin = directory.join("bin");
        fs::create_dir_all(&bin).unwrap_or_else(|error| panic!("建立 bin fixture 失败：{error}"));
        let executable = bin.join("fixture-tool");
        fs::write(&executable, b"not executed")
            .unwrap_or_else(|error| panic!("写入 TryExec fixture 失败：{error}"));
        let mut permissions = fs::metadata(&executable)
            .unwrap_or_else(|error| panic!("读取权限失败：{error}"))
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions)
            .unwrap_or_else(|error| panic!("设置权限失败：{error}"));

        write_entry(
            &user,
            "visible.desktop",
            "[Desktop Entry]\nType=Application\nName=Visible\nName[zh_CN]=本地化应用\nExec=never-run\nOnlyShowIn=KDE;\nTryExec=fixture-tool\n",
        );
        write_entry(
            &user,
            "hidden.desktop",
            "[Desktop Entry]\nType=Application\nName=Deleted\nExec=never-run\nHidden=true\n",
        );
        write_entry(
            &system,
            "hidden.desktop",
            "[Desktop Entry]\nType=Application\nName=Must Stay Hidden\nExec=never-run\n",
        );
        for (name, extra) in [
            ("nodisplay", "NoDisplay=true"),
            ("wrong-desktop", "OnlyShowIn=GNOME;"),
            ("excluded-desktop", "NotShowIn=KDE;"),
            ("missing-exec", "TryExec=definitely-missing"),
            ("both-allowed", "OnlyShowIn=KDE;\nNotShowIn=GNOME;"),
            ("both-denied", "OnlyShowIn=GNOME;\nNotShowIn=KDE;"),
        ] {
            write_entry(
                &user,
                &format!("{name}.desktop"),
                &format!(
                    "[Desktop Entry]\nType=Application\nName={name}\nExec=never-run\n{extra}\n"
                ),
            );
        }
        let inventory = enumerate_with(&fixture_environment(&user, &system, &bin), 32);
        let names = inventory
            .records
            .iter()
            .map(|record| record.display_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["both-allowed", "nodisplay", "本地化应用"]);
        assert!(
            inventory
                .records
                .iter()
                .all(|record| record.session_id.starts_with("s2:a:"))
        );
        assert!(inventory.available);
        assert!(inventory.complete);
        fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("清理 Desktop Entry fixture 失败：{error}"));
    }

    #[test]
    fn fixture_rejects_ambiguous_ids_invalid_files_and_reports_maximum_truncation() {
        let directory = fixture_directory();
        let user = directory.join("user");
        let system = directory.join("system");
        let bin = directory.join("bin");
        write_entry(
            &user,
            "same-id.desktop",
            "[Desktop Entry]\nType=Application\nName=Ambiguous Flat\nExec=never-run\n",
        );
        write_entry(
            &user,
            "same/id.desktop",
            "[Desktop Entry]\nType=Application\nName=Ambiguous Nested\nExec=never-run\n",
        );
        write_entry(&user, "invalid.desktop", "not a desktop entry\n");
        for name in ["Alpha", "Beta"] {
            write_entry(
                &user,
                &format!("{name}.desktop"),
                &format!("[Desktop Entry]\nType=Application\nName={name}\nExec=never-run\n"),
            );
        }
        let inventory = enumerate_with(&fixture_environment(&user, &system, &bin), 1);
        assert_eq!(inventory.records.len(), 1);
        assert_eq!(inventory.records[0].display_name, "Alpha");
        assert_eq!(inventory.applications_returned, 1);
        assert_eq!(inventory.total_eligible_applications, None);
        assert!(inventory.applications_truncated);
        assert!(inventory.warnings.contains(&WARNING_ID_CONFLICT));
        assert!(!inventory.complete);
        fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("清理 Desktop Entry fixture 失败：{error}"));
    }

    #[test]
    fn fixture_reports_exact_two_to_one_truncation() {
        let directory = fixture_directory();
        let user = directory.join("user");
        let system = directory.join("system");
        let bin = directory.join("bin");
        for name in ["Alpha", "Beta"] {
            write_entry(
                &user,
                &format!("{name}.desktop"),
                &format!("[Desktop Entry]\nType=Application\nName={name}\nExec=never-run\n"),
            );
        }
        let inventory = enumerate_with(&fixture_environment(&user, &system, &bin), 1);
        assert_eq!(inventory.applications_returned, 1);
        assert_eq!(inventory.total_eligible_applications, Some(2));
        assert!(inventory.applications_truncated);
        assert_eq!(inventory.records[0].display_name, "Alpha");
        fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("清理截断 fixture 失败：{error}"));
    }

    #[test]
    fn fixture_filters_type_and_requires_exec_unless_dbus_activatable() {
        let directory = fixture_directory();
        let user = directory.join("user");
        let system = directory.join("system");
        let bin = directory.join("bin");
        for (file, body) in [
            (
                "dbus.desktop",
                "Type=Application\nName=D-Bus Only\nDBusActivatable=true",
            ),
            (
                "link.desktop",
                "Type=Link\nName=Link\nURL=https://invalid.example",
            ),
            (
                "missing-exec.desktop",
                "Type=Application\nName=Missing Exec",
            ),
        ] {
            write_entry(&user, file, &format!("[Desktop Entry]\n{body}\n"));
        }
        let inventory = enumerate_with(&fixture_environment(&user, &system, &bin), 8);
        assert_eq!(inventory.records.len(), 1);
        assert_eq!(inventory.records[0].display_name, "D-Bus Only");
        assert!(!inventory.complete);
        assert!(inventory.warnings.contains(&WARNING_INVALID_ENTRY));
        fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("清理 Type/Exec fixture 失败：{error}"));
    }

    #[test]
    fn fixture_identity_changes_when_content_or_precedence_winner_changes() {
        let directory = fixture_directory();
        let user = directory.join("user");
        let system = directory.join("system");
        let bin = directory.join("bin");
        write_entry(
            &user,
            "identity.desktop",
            "[Desktop Entry]\nType=Application\nName=User Winner\nExec=never-run\n",
        );
        write_entry(
            &system,
            "identity.desktop",
            "[Desktop Entry]\nType=Application\nName=System Winner\nExec=never-run\n",
        );
        let environment = fixture_environment(&user, &system, &bin);
        let first = enumerate_with(&environment, 8).records[0]
            .session_id
            .clone();
        write_entry(
            &user,
            "identity.desktop",
            "[Desktop Entry]\nType=Application\nName=User Replacement\nExec=never-run\n",
        );
        let replacement = enumerate_with(&environment, 8).records[0]
            .session_id
            .clone();
        assert_ne!(first, replacement);
        fs::remove_file(user.join("applications/identity.desktop"))
            .unwrap_or_else(|error| panic!("删除高优先级 fixture 失败：{error}"));
        let lower_winner = enumerate_with(&environment, 8).records[0]
            .session_id
            .clone();
        assert_ne!(replacement, lower_winner);
        fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("清理 Desktop Entry fixture 失败：{error}"));
    }

    #[test]
    fn fixture_applies_only_not_left_to_right_and_rejects_overlap_or_missing_base_name() {
        let directory = fixture_directory();
        let user = directory.join("user");
        let system = directory.join("system");
        let bin = directory.join("bin");
        write_entry(
            &user,
            "ordered.desktop",
            "[Desktop Entry]\nType=Application\nName=Ordered\nExec=never-run\nOnlyShowIn=GNOME;\nNotShowIn=KDE;\n",
        );
        write_entry(
            &user,
            "overlap.desktop",
            "[Desktop Entry]\nType=Application\nName=Overlap\nExec=never-run\nOnlyShowIn=KDE;\nNotShowIn=KDE;\n",
        );
        write_entry(
            &user,
            "localized-only.desktop",
            "[Desktop Entry]\nType=Application\nName[zh_CN]=缺少基础名称\nExec=never-run\n",
        );
        let mut kde_first = fixture_environment(&user, &system, &bin);
        kde_first.values.insert(
            "XDG_CURRENT_DESKTOP".to_owned(),
            OsString::from("KDE:GNOME"),
        );
        let denied = enumerate_with(&kde_first, 32);
        assert!(denied.records.is_empty());
        assert!(!denied.complete);
        assert!(denied.warnings.contains(&WARNING_INVALID_ENTRY));

        let mut gnome_first = fixture_environment(&user, &system, &bin);
        gnome_first.values.insert(
            "XDG_CURRENT_DESKTOP".to_owned(),
            OsString::from("GNOME:KDE"),
        );
        let allowed = enumerate_with(&gnome_first, 32);
        assert_eq!(
            allowed
                .records
                .iter()
                .map(|record| record.display_name.as_str())
                .collect::<Vec<_>>(),
            ["Ordered"]
        );
        assert_eq!(
            parse_list(r"KDE\;Special;GNOME;"),
            Some(vec!["KDE;Special".to_owned(), "GNOME".to_owned()])
        );
        fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("清理 Only/Not fixture 失败：{error}"));
    }

    #[test]
    fn fixture_all_missing_roots_are_unavailable_incomplete_and_path_free() {
        let directory = fixture_directory();
        let user = directory.join("missing-user");
        let system = directory.join("missing-system");
        let bin = directory.join("bin");
        let inventory = enumerate_with(&fixture_environment(&user, &system, &bin), 8);
        assert!(!inventory.available);
        assert!(!inventory.complete);
        assert!(inventory.records.is_empty());
        assert_eq!(inventory.total_eligible_applications, None);
        assert_eq!(inventory.warnings, [WARNING_ROOTS_MISSING]);
        let warnings = inventory.warnings.join(",");
        assert!(!warnings.contains("missing-user"));
        assert!(!warnings.contains("missing-system"));
        let relative = FixtureEnvironment {
            values: HashMap::from([
                ("XDG_DATA_HOME".to_owned(), OsString::from("relative-home")),
                (
                    "XDG_DATA_DIRS".to_owned(),
                    OsString::from("relative-system"),
                ),
                ("PATH".to_owned(), OsString::from("/usr/bin")),
            ]),
        };
        let relative_inventory = enumerate_with(&relative, 8);
        assert!(!relative_inventory.available);
        assert!(!relative_inventory.complete);
        assert!(relative_inventory.records.is_empty());
        fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("清理缺失根 fixture 失败：{error}"));
    }

    #[test]
    fn fixture_rejects_lstat_to_open_replacement_with_symlink_or_fifo() {
        let directory = fixture_directory();
        let user = directory.join("user");
        let outside = directory.join("outside.desktop");
        write_entry(
            &user,
            "swap.desktop",
            "[Desktop Entry]\nType=Application\nName=Safe\nExec=never-run\n",
        );
        fs::write(
            &outside,
            "[Desktop Entry]\nType=Application\nName=TOCTOU-CANARY\nExec=never-run\n",
        )
        .unwrap_or_else(|error| panic!("写入替换 canary 失败：{error}"));
        let root = match open_applications_root(&user) {
            ApplicationRoot::Ready(root) => root,
            other => panic!("fixture root 必须可读：{other:?}"),
        };
        let mut complete = true;
        let mut warnings = BTreeSet::new();
        let mut remaining_directory_entries = MAXIMUM_SCANNED_DIRECTORY_ENTRIES;
        let candidates = collect_candidates(
            &root,
            &mut remaining_directory_entries,
            &mut complete,
            &mut warnings,
        );
        assert_eq!(candidates.len(), 1);
        let target = user.join("applications/swap.desktop");
        fs::remove_file(&target).unwrap_or_else(|error| panic!("删除原文件失败：{error}"));
        symlink(&outside, &target).unwrap_or_else(|error| panic!("建立替换 symlink 失败：{error}"));
        assert!(matches!(
            read_entry(&root, &candidates[0]),
            Err(ReadFailure::Incomplete)
        ));
        fs::remove_file(&target).unwrap_or_else(|error| panic!("删除替换 symlink 失败：{error}"));
        let fifo_created = Command::new("/usr/bin/mkfifo")
            .arg(&target)
            .status()
            .is_ok_and(|status| status.success());
        assert!(fifo_created);
        assert!(matches!(
            read_entry(&root, &candidates[0]),
            Err(ReadFailure::Incomplete)
        ));
        fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("清理 TOCTOU fixture 失败：{error}"));
    }

    #[test]
    fn fixture_rejects_file_and_root_directory_symlinks_without_canary_disclosure() {
        let directory = fixture_directory();
        let user = directory.join("user");
        let system = directory.join("system");
        let bin = directory.join("bin");
        let outside = directory.join("outside");
        write_entry(
            &outside,
            "canary.desktop",
            "[Desktop Entry]\nType=Application\nName=ROOT-OUTSIDE-CANARY\nExec=never-run\n",
        );
        fs::create_dir_all(user.join("applications"))
            .unwrap_or_else(|error| panic!("建立 symlink fixture 目录失败：{error}"));
        write_entry(
            &user,
            "inside/real.desktop",
            "[Desktop Entry]\nType=Application\nName=Safe Inside\nExec=never-run\n",
        );
        symlink(
            outside.join("applications/canary.desktop"),
            user.join("applications/file-link.desktop"),
        )
        .unwrap_or_else(|error| panic!("建立文件 symlink 失败：{error}"));
        symlink(
            user.join("applications/inside/real.desktop"),
            user.join("applications/inside-link.desktop"),
        )
        .unwrap_or_else(|error| panic!("建立根内文件 symlink 失败：{error}"));
        symlink(
            user.join("applications/inside"),
            user.join("applications/directory-link"),
        )
        .unwrap_or_else(|error| panic!("建立根内目录 symlink 失败：{error}"));
        let _socket = UnixListener::bind(user.join("applications/socket.desktop"))
            .unwrap_or_else(|error| panic!("建立 socket fixture 失败：{error}"));
        fs::create_dir_all(&system)
            .unwrap_or_else(|error| panic!("建立 system fixture 失败：{error}"));
        symlink(outside.join("applications"), system.join("applications"))
            .unwrap_or_else(|error| panic!("建立目录 symlink 失败：{error}"));

        let inventory = enumerate_with(&fixture_environment(&user, &system, &bin), 32);
        let disclosed = inventory
            .records
            .iter()
            .any(|record| record.display_name == "ROOT-OUTSIDE-CANARY");
        assert_eq!(
            inventory
                .records
                .iter()
                .filter(|record| record.display_name == "Safe Inside")
                .count(),
            1
        );
        fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("清理 symlink fixture 失败：{error}"));
        assert!(!disclosed, "根外 Desktop Entry 不得经 symlink 发布");
        assert!(inventory.available);
        assert!(!inventory.complete);
        assert_eq!(inventory.total_eligible_applications, None);
        assert!(inventory.warnings.contains(&WARNING_UNSAFE_ENTRY));
    }

    #[test]
    fn fixture_concurrent_regular_symlink_fifo_replacement_never_discloses_or_blocks() {
        let directory = fixture_directory();
        let user = directory.join("user");
        let system = directory.join("system");
        let bin = directory.join("bin");
        let target = user.join("applications/race.desktop");
        let outside = directory.join("outside.desktop");
        write_entry(
            &user,
            "race.desktop",
            "[Desktop Entry]\nType=Application\nName=Safe\nExec=never-run\n",
        );
        fs::write(
            &outside,
            "[Desktop Entry]\nType=Application\nName=RACE-CANARY\nExec=never-run\n",
        )
        .unwrap_or_else(|error| panic!("写入竞态 canary 失败：{error}"));
        let running = Arc::new(AtomicBool::new(true));
        let writer_running = Arc::clone(&running);
        let writer_target = target.clone();
        let writer_outside = outside.clone();
        let writer = thread::spawn(move || {
            while writer_running.load(Ordering::Acquire) {
                let _ = fs::remove_file(&writer_target);
                let _ = symlink(&writer_outside, &writer_target);
                let _ = fs::remove_file(&writer_target);
                let _ = fs::write(
                    &writer_target,
                    "[Desktop Entry]\nType=Application\nName=Safe\nExec=never-run\n",
                );
                let _ = fs::remove_file(&writer_target);
                let _ = Command::new("/usr/bin/mkfifo")
                    .arg(&writer_target)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        });
        for _ in 0..64 {
            let inventory = enumerate_with(&fixture_environment(&user, &system, &bin), 8);
            assert!(
                inventory
                    .records
                    .iter()
                    .all(|record| record.display_name == "Safe")
            );
        }
        running.store(false, Ordering::Release);
        writer
            .join()
            .unwrap_or_else(|_| panic!("竞态 writer 不得 panic"));
        fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("清理竞态 fixture 失败：{error}"));
    }

    #[test]
    #[ignore = "只由有界父测试启动"]
    fn fifo_child_enumerates_fixture() {
        let _ = enumerate(8);
    }

    #[test]
    fn fixture_fifo_never_blocks_inventory() {
        let directory = fixture_directory();
        let user = directory.join("user");
        let system = directory.join("system");
        let applications = user.join("applications");
        fs::create_dir_all(&applications)
            .unwrap_or_else(|error| panic!("建立 FIFO fixture 目录失败：{error}"));
        let fifo = applications.join("blocking.desktop");
        let created = Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        assert!(created, "FIFO fixture 必须建立成功");
        let executable =
            std::env::current_exe().unwrap_or_else(|error| panic!("定位测试进程失败：{error}"));
        let mut child = Command::new(executable)
            .arg("fifo_child_enumerates_fixture")
            .arg("--ignored")
            .env_clear()
            .env("XDG_DATA_HOME", &user)
            .env("XDG_DATA_DIRS", &system)
            .env("XDG_CURRENT_DESKTOP", "KDE")
            .env("LANG", "C")
            .env("PATH", "/usr/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("启动 FIFO 子测试失败：{error}"));
        let deadline = Instant::now() + Duration::from_millis(750);
        let completed = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.success(),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break false;
                }
                Err(_) => break false,
            }
        };
        fs::remove_dir_all(directory)
            .unwrap_or_else(|error| panic!("清理 FIFO fixture 失败：{error}"));
        assert!(completed, "Desktop Entry FIFO 读取不得阻塞 inventory");
    }
}
