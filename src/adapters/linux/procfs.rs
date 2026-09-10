//! `/proc` 只读进程快照 Adapter，不承担公共 JSON 语义。

use std::{collections::BTreeSet, fs, io, os::unix::fs::MetadataExt, path::Path};

use crate::components::opaque_id::{OpaqueTargetId, OpaqueTargetKind};

/// 供所属 System 私有 Module 使用的平台中立进程事实。
#[derive(Clone, Debug)]
pub(crate) struct ProcessRecord {
    pub(crate) session_id: String,
    pub(crate) process_name: String,
    pub(crate) identity_reliable: bool,
    pub(crate) metadata_access: &'static str,
    pub(crate) native_pid: u32,
    state: char,
    pub(crate) parent_pid: u32,
    pub(crate) start_ticks: u64,
    pub(crate) owner_uids: [u32; 4],
    pub(crate) namespace_pids: Vec<u32>,
    /// boot_id 与 PID namespace 只作为不公开的 procfs owner epoch 参与代际绑定。
    pub(crate) owner_epoch: String,
}

/// 当前调用进程的 Linux credential 事实；只在 Adapter 内用于保护判定。
pub(crate) struct CurrentCredentialFacts {
    pub(crate) uids: [u32; 4],
    pub(crate) effective_capabilities: u64,
}

struct StatusIdentity {
    uids: [u32; 4],
    namespace_pids: Vec<u32>,
    effective_capabilities: Option<u64>,
}

/// 一次有界快照及其覆盖完整性。
pub(crate) struct ProcessInventory {
    pub(crate) records: Vec<ProcessRecord>,
    pub(crate) complete: bool,
}

fn process_identity(
    owner_epoch: &str,
    native_pid: u32,
    start_ticks: u64,
    process_name: &str,
) -> String {
    format!("linux:{owner_epoch}:{native_pid}:{start_ticks}:{process_name}")
}

pub(crate) fn opaque_process_session_id(record: &ProcessRecord) -> String {
    OpaqueTargetId::new(
        OpaqueTargetKind::Process,
        &process_identity(
            &record.owner_epoch,
            record.native_pid,
            record.start_ticks,
            &record.process_name,
        ),
    )
    .to_string()
}

fn parse_stat_identity(stat: &str) -> Option<(char, u32, u64)> {
    let command_end = stat.rfind(')')?;
    let after_command = stat.get(command_end + 1..)?.trim_start();
    let mut fields = after_command.split_whitespace();
    let mut state_chars = fields.next()?.chars();
    let state = state_chars.next()?;
    if state_chars.next().is_some() {
        return None;
    }
    let parent_pid = fields.next()?.parse().ok()?;
    let start_ticks = fields.nth(17)?.parse().ok()?;
    Some((state, parent_pid, start_ticks))
}

fn parse_status_identity(status: &str) -> Option<StatusIdentity> {
    let mut uids = None;
    let mut namespace_pids = None;
    let mut effective_capabilities = None;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("Uid:") {
            let values = value
                .split_whitespace()
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            let values: [u32; 4] = values.try_into().ok()?;
            uids = Some(values);
        } else if let Some(value) = line.strip_prefix("NSpid:") {
            let values = value
                .split_whitespace()
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if values.is_empty() {
                return None;
            }
            namespace_pids = Some(values);
        } else if let Some(value) = line.strip_prefix("CapEff:") {
            effective_capabilities = u64::from_str_radix(value.trim(), 16).ok();
        }
    }
    Some(StatusIdentity {
        uids: uids?,
        namespace_pids: namespace_pids?,
        effective_capabilities,
    })
}

fn read_record(
    entry: &fs::DirEntry,
    native_pid: u32,
    owner_epoch: &str,
) -> io::Result<ProcessRecord> {
    read_record_from_directory(&entry.path(), native_pid, owner_epoch)
}

/// 从已知 `/proc/<pid>` 目录读取同一进程代际事实，供其他 Linux Adapter 建立中立关联。
fn read_record_from_directory(
    directory: &Path,
    native_pid: u32,
    owner_epoch: &str,
) -> io::Result<ProcessRecord> {
    let process_name = fs::read_to_string(directory.join("comm"))?
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    let stat = fs::read_to_string(directory.join("stat"))?;
    let (state, parent_pid, start_ticks) = parse_stat_identity(&stat)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid proc stat"))?;
    // status 的 Uid/NSpid 是进程 credential 与命名空间事实；目录 owner 可能受 dumpable 规则改写。
    let status = fs::read_to_string(directory.join("status"))?;
    let status = parse_status_identity(&status)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid proc status"))?;
    let mut record = ProcessRecord {
        session_id: String::new(),
        process_name,
        identity_reliable: true,
        metadata_access: "available",
        native_pid,
        state,
        parent_pid,
        start_ticks,
        owner_uids: status.uids,
        namespace_pids: status.namespace_pids,
        owner_epoch: owner_epoch.to_owned(),
    };
    record.session_id = opaque_process_session_id(&record);
    Ok(record)
}

/// 把经认证本机 peer 的 PID 重新解析为当前进程代际 opaque ID；失败时不猜测关联。
pub(crate) fn session_id_for_pid(native_pid: u32) -> Option<String> {
    record_for_pid(native_pid)
        .ok()
        .filter(|record| !is_terminal_state(record.state))
        .map(|record| record.session_id)
}

fn is_terminal_state(state: char) -> bool {
    matches!(state, 'Z' | 'X' | 'x')
}

/// 使用当前 procfs 重新读取一个精确 PID 的代际与所有者事实。
pub(crate) fn record_for_pid(native_pid: u32) -> io::Result<ProcessRecord> {
    let directory = Path::new("/proc").join(native_pid.to_string());
    let owner_epoch = current_owner_epoch()?;
    read_record_from_directory(&directory, native_pid, &owner_epoch)
}

/// 组合 canonical boot_id 与 PID namespace 身份，形成跨 CLI 共享的私有 procfs owner epoch。
pub(crate) fn current_owner_epoch() -> io::Result<String> {
    let value = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    let boot_id = canonical_boot_id(&value)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid Linux boot id"))?;
    let pid_namespace = fs::metadata("/proc/self/ns/pid")?;
    if pid_namespace.ino() == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid Linux pid namespace identity",
        ));
    }
    Ok(format!(
        "{boot_id}:{:016x}:{:016x}",
        pid_namespace.dev(),
        pid_namespace.ino()
    ))
}

fn canonical_boot_id(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.len() != 36 {
        return None;
    }
    for (index, byte) in value.bytes().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return None;
            }
        } else if !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte) {
            return None;
        }
    }
    Some(value)
}

/// 读取当前调用进程的完整 UID 四元组与有效 capability mask。
pub(crate) fn current_credential_facts() -> io::Result<CurrentCredentialFacts> {
    let status = fs::read_to_string("/proc/self/status")?;
    let status = parse_status_identity(&status)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid self proc status"))?;
    Ok(CurrentCredentialFacts {
        uids: status.uids,
        effective_capabilities: status.effective_capabilities.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing effective capabilities")
        })?,
    })
}

/// 读取当前工具及其祖先 PID；任一缺口都失败闭合，不猜测保护域。
pub(crate) fn current_ancestor_pids() -> io::Result<BTreeSet<u32>> {
    const MAXIMUM_ANCESTORS: usize = 128;
    let mut protected = BTreeSet::new();
    let mut current = std::process::id();
    for _ in 0..MAXIMUM_ANCESTORS {
        if !protected.insert(current) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "procfs ancestor cycle",
            ));
        }
        let record = record_for_pid(current)?;
        if record.parent_pid == 0 {
            return Ok(protected);
        }
        current = record.parent_pid;
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "procfs ancestor limit exceeded",
    ))
}

/// 枚举 Linux procfs；单个短命或无权限条目只降低完整性，不使全部快照失效。
pub(crate) fn snapshot(maximum_items: usize) -> io::Result<ProcessInventory> {
    let owner_epoch = current_owner_epoch()?;
    let mut entries = fs::read_dir(Path::new("/proc"))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let native_pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
            Some((native_pid, entry))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|(native_pid, _)| *native_pid);

    let mut records = Vec::new();
    let mut complete = true;
    for (native_pid, entry) in entries {
        match read_record(&entry, native_pid, &owner_epoch) {
            Ok(record) if !is_terminal_state(record.state) => records.push(record),
            Ok(_) => {}
            Err(_) => complete = false,
        }
        if records.len() == maximum_items {
            complete = false;
            break;
        }
    }
    Ok(ProcessInventory { records, complete })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_parser_handles_spaces_and_parentheses_in_command() {
        let stat = format!(
            "42 (fixture ) name) S 7 {} 98765 0",
            vec!["0"; 17].join(" ")
        );
        assert_eq!(parse_stat_identity(&stat), Some(('S', 7, 98_765)));
    }

    #[test]
    fn current_process_record_matches_current_generation() {
        let record = record_for_pid(std::process::id())
            .unwrap_or_else(|error| panic!("当前进程应能从 procfs 读取：{error}"));
        assert_eq!(record.native_pid, std::process::id());
        assert!(record.start_ticks > 0);
        assert!(record.session_id.starts_with("s2:p:"));
        assert!(!record.namespace_pids.is_empty());
        let owner_epoch = current_owner_epoch()
            .unwrap_or_else(|error| panic!("当前 procfs owner epoch 应可读取：{error}"));
        assert_eq!(record.owner_epoch, owner_epoch);
    }

    #[test]
    fn owner_epoch_uses_canonical_boot_and_pid_namespace_identity() {
        let epoch = "01234567-89ab-cdef-0123-456789abcdef";
        assert_eq!(canonical_boot_id(epoch), Some(epoch));
        assert!(canonical_boot_id("0123456789abcdef").is_none());
        assert!(canonical_boot_id("01234567-89AB-cdef-0123-456789abcdef").is_none());
        assert!(canonical_boot_id("01234567-89ab-cdef-0123-456789abcdeg").is_none());
        let owner_epoch = current_owner_epoch()
            .unwrap_or_else(|error| panic!("当前 procfs owner epoch 应可读取：{error}"));
        let fields = owner_epoch.split(':').collect::<Vec<_>>();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].len(), 36);
        assert!(fields[1..].iter().all(|field| field.len() == 16));
    }

    #[test]
    fn status_parser_reads_uid_namespace_and_capabilities() {
        let status = "Name:\tfixture\nUid:\t1000\t1000\t1000\t1000\nNSpid:\t42000\t7\nCapEff:\t0000000000000020\n";
        let parsed =
            parse_status_identity(status).unwrap_or_else(|| panic!("完整 status fixture 应可解析"));
        assert_eq!(parsed.uids, [1000; 4]);
        assert_eq!(parsed.namespace_pids, [42_000, 7]);
        assert_eq!(parsed.effective_capabilities, Some(1 << 5));
    }
}
