//! Linux Release System 的结构化 ELF 策略 Component。

use std::collections::BTreeSet;

use goblin::elf::{
    Elf,
    dynamic::{DF_1_NOW, DF_1_PIE, DF_BIND_NOW, DT_BIND_NOW},
    header::{EM_AARCH64, EM_X86_64, ET_DYN},
    program_header::{PF_X, PT_GNU_RELRO, PT_GNU_STACK},
};
use serde::{Deserialize, Serialize};

use super::{ReleaseResult, glibc_at_most};

/// 冻结在 manifest 中并由安装路径重新计算的 ELF 事实。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ElfPolicyEvidence {
    pub(super) architecture: String,
    pub(super) interpreter: String,
    pub(super) position_independent_executable: bool,
    pub(super) non_executable_stack: bool,
    pub(super) relro: bool,
    pub(super) bind_now: bool,
    pub(super) needed: Vec<String>,
    pub(super) maximum_glibc: String,
}

/// 从 ELF 二进制本身解析全部发布门禁，任何缺字段或未知格式均失败闭合。
pub(super) fn inspect(bytes: &[u8], target: &str) -> ReleaseResult<ElfPolicyEvidence> {
    let elf = Elf::parse(bytes).map_err(|_| "ELF policy inspection failed".to_owned())?;
    let (machine, interpreter, needed_allowlist) = match target {
        "x86_64-unknown-linux-gnu" => (
            EM_X86_64,
            "/lib64/ld-linux-x86-64.so.2",
            BTreeSet::from(["ld-linux-x86-64.so.2", "libc.so.6", "libgcc_s.so.1"]),
        ),
        "aarch64-unknown-linux-gnu" => (
            EM_AARCH64,
            "/lib/ld-linux-aarch64.so.1",
            BTreeSet::from(["ld-linux-aarch64.so.1", "libc.so.6", "libgcc_s.so.1"]),
        ),
        _ => return Err("ELF target policy is unavailable".to_owned()),
    };
    if !elf.is_64 || !elf.little_endian || elf.header.e_machine != machine {
        return Err("ELF architecture does not match the release target".to_owned());
    }
    let actual_interpreter = elf
        .interpreter
        .ok_or_else(|| "ELF interpreter is missing".to_owned())?;
    if actual_interpreter != interpreter {
        return Err("ELF interpreter is not allowlisted".to_owned());
    }
    if elf.header.e_type != ET_DYN {
        return Err("ELF is not a position-independent executable".to_owned());
    }
    if !elf.rpaths.is_empty() || !elf.runpaths.is_empty() {
        return Err("ELF runtime search paths are forbidden".to_owned());
    }
    let dynamic = elf
        .dynamic
        .as_ref()
        .ok_or_else(|| "ELF dynamic policy is missing".to_owned())?;
    let pie = dynamic.info.flags_1 & DF_1_PIE != 0;
    let bind_now = dynamic.info.flags & DF_BIND_NOW != 0
        || dynamic.info.flags_1 & DF_1_NOW != 0
        || dynamic
            .dyns
            .iter()
            .any(|entry| u64::from(entry.d_tag) == DT_BIND_NOW);
    if !pie || !bind_now {
        return Err("ELF PIE or BIND_NOW policy is incomplete".to_owned());
    }
    let stack_headers = elf
        .program_headers
        .iter()
        .filter(|header| header.p_type == PT_GNU_STACK)
        .collect::<Vec<_>>();
    if stack_headers.len() != 1 || stack_headers[0].p_flags & PF_X != 0 {
        return Err("ELF non-executable stack evidence is invalid".to_owned());
    }
    if !elf
        .program_headers
        .iter()
        .any(|header| header.p_type == PT_GNU_RELRO)
    {
        return Err("ELF RELRO evidence is missing".to_owned());
    }
    let needed = elf
        .libraries
        .iter()
        .map(|library| (*library).to_owned())
        .collect::<BTreeSet<_>>();
    if needed.is_empty()
        || needed
            .iter()
            .any(|library| !needed_allowlist.contains(library.as_str()))
    {
        return Err("ELF DT_NEEDED contains an unreviewed library".to_owned());
    }
    let maximum_glibc = maximum_glibc(&elf)?;
    Ok(ElfPolicyEvidence {
        architecture: target.to_owned(),
        interpreter: interpreter.to_owned(),
        position_independent_executable: true,
        non_executable_stack: true,
        relro: true,
        bind_now: true,
        needed: needed.into_iter().collect(),
        maximum_glibc,
    })
}

/// 从结构化 GNU version-need 表读取最大 GLIBC 版本。
fn maximum_glibc(elf: &Elf<'_>) -> ReleaseResult<String> {
    let section = elf
        .verneed
        .as_ref()
        .ok_or_else(|| "ELF GLIBC version requirements are missing".to_owned())?;
    let mut maximum: Option<(u32, u32, u32)> = None;
    for need in section {
        let mut observed = 0_u16;
        for auxiliary in &need {
            observed = observed.saturating_add(1);
            let name = elf
                .dynstrtab
                .get_at(auxiliary.vna_name)
                .ok_or_else(|| "ELF version requirement name is invalid".to_owned())?;
            let Some(version) = name.strip_prefix("GLIBC_") else {
                continue;
            };
            let parsed = parse_glibc(version)?;
            maximum = Some(maximum.map_or(parsed, |current| current.max(parsed)));
        }
        if observed != need.vn_cnt {
            return Err("ELF version requirement table is incomplete".to_owned());
        }
    }
    let (major, minor, patch) =
        maximum.ok_or_else(|| "ELF has no recognized GLIBC requirement".to_owned())?;
    if patch == 0 {
        Ok(format!("{major}.{minor}"))
    } else {
        Ok(format!("{major}.{minor}.{patch}"))
    }
}

/// 只接受精确的 major.minor 或 major.minor.patch GLIBC 标识。
fn parse_glibc(value: &str) -> ReleaseResult<(u32, u32, u32)> {
    let parts = value.split('.').collect::<Vec<_>>();
    if !matches!(parts.len(), 2 | 3)
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err("ELF GLIBC version is malformed".to_owned());
    }
    Ok((
        parts[0]
            .parse()
            .map_err(|_| "ELF GLIBC version is malformed".to_owned())?,
        parts[1]
            .parse()
            .map_err(|_| "ELF GLIBC version is malformed".to_owned())?,
        parts
            .get(2)
            .map_or(Ok(0), |patch| patch.parse())
            .map_err(|_| "ELF GLIBC version is malformed".to_owned())?,
    ))
}

/// 确认 manifest 的派生兼容真值与 ELF 事实一致。
pub(super) fn validate_manifest_evidence(
    evidence: &ElfPolicyEvidence,
    target: &str,
    needed: &[String],
    maximum_glibc: &str,
    ubuntu_2204_compatible: bool,
) -> ReleaseResult<()> {
    if evidence.architecture != target
        || evidence.needed != needed
        || evidence.maximum_glibc != maximum_glibc
        || !evidence.position_independent_executable
        || !evidence.non_executable_stack
        || !evidence.relro
        || !evidence.bind_now
        || ubuntu_2204_compatible != glibc_at_most(maximum_glibc, 2, 35)?
    {
        return Err("ELF manifest evidence is inconsistent".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_glibc;

    #[test]
    fn glibc_version_parser_rejects_unknown_formats() {
        assert_eq!(parse_glibc("2.35").unwrap(), (2, 35, 0));
        assert_eq!(parse_glibc("2.2.5").unwrap(), (2, 2, 5));
        for value in ["2", "2.35.1.4", "2.x", "", ".35"] {
            assert!(parse_glibc(value).is_err(), "accepted {value}");
        }
    }
}
