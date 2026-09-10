//! Linux 当前主机 opaque 身份 Component。

use std::fs;

use super::opaque_id::{OpaqueTargetId, OpaqueTargetKind};

/// 从当前主机与用户的稳定私有材料生成 provider-neutral `s2:h`。
pub(crate) fn current_host_target() -> String {
    let machine = fs::read_to_string("/etc/machine-id")
        .unwrap_or_else(|_| "machine-id-unavailable".to_owned());
    let user = fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Uid:").map(str::trim))
                .and_then(|uids| uids.split_whitespace().next())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "uid-unavailable".to_owned());
    OpaqueTargetId::new(
        OpaqueTargetKind::Host,
        &format!("linux:{}:{user}", machine.trim()),
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::current_host_target;

    #[test]
    fn linux_host_identity_is_canonical_and_stable_in_process() {
        let first = current_host_target();
        assert!(first.starts_with("s2:h:"));
        assert_eq!(first, current_host_target());
    }
}
