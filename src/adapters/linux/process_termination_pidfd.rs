//! Linux pidfd 终止 Adapter；原生 PID、UID、fd 与实际 signal 不跨平台边界。

use std::{os::fd::OwnedFd, time::Duration};

use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    io::Errno,
    process::{Pid, PidfdFlags, Signal, pidfd_open, pidfd_send_signal},
};

use crate::{
    components::opaque_id::{OpaqueTargetMatch, match_opaque_target},
    modules::process_termination::{
        LinuxProcessProtection, LinuxProcessTerminationFailure, LinuxProcessTerminationLease,
        LinuxProcessTerminationPort,
    },
};

use super::procfs;

const MAXIMUM_PROCESS_INVENTORY: usize = 65_536;
const CAP_KILL_BIT: u64 = 1 << 5;

/// 系统 pidfd 端口不保存状态，每个请求只持有一个精确 lease。
pub(crate) struct SystemPidfdTerminationPort;

struct PidfdTerminationLease {
    pidfd: OwnedFd,
}

impl LinuxProcessTerminationPort for SystemPidfdTerminationPort {
    fn prepare(
        &self,
        target_id: &str,
    ) -> Result<Box<dyn LinuxProcessTerminationLease>, LinuxProcessTerminationFailure> {
        let credentials = procfs::current_credential_facts().map_err(|_| {
            LinuxProcessTerminationFailure::Protected(
                LinuxProcessProtection::ProtectionStateUnknown,
            )
        })?;
        if let Some(protection) = current_execution_protection(&credentials) {
            return Err(LinuxProcessTerminationFailure::Protected(protection));
        }
        let inventory = procfs::snapshot(MAXIMUM_PROCESS_INVENTORY)
            .map_err(|_| LinuxProcessTerminationFailure::InventoryIncomplete)?;
        let target = match match_opaque_target(target_id, &inventory.records, |record| {
            Some(procfs::opaque_process_session_id(record))
        }) {
            OpaqueTargetMatch::Unique(target) => target.clone(),
            OpaqueTargetMatch::Missing if inventory.complete => {
                return Err(LinuxProcessTerminationFailure::Stale);
            }
            OpaqueTargetMatch::Missing => {
                return Err(LinuxProcessTerminationFailure::InventoryIncomplete);
            }
            OpaqueTargetMatch::Ambiguous => {
                return Err(LinuxProcessTerminationFailure::Ambiguous);
            }
        };
        let ancestors = procfs::current_ancestor_pids().map_err(|_| {
            LinuxProcessTerminationFailure::Protected(
                LinuxProcessProtection::ProtectionStateUnknown,
            )
        })?;
        if let Some(protection) = target_protection(
            target.native_pid,
            &target.namespace_pids,
            &target.owner_uids,
            credentials.uids[0],
            &ancestors,
        ) {
            return Err(LinuxProcessTerminationFailure::Protected(protection));
        }
        let pid = i32::try_from(target.native_pid)
            .ok()
            .and_then(Pid::from_raw)
            .ok_or(LinuxProcessTerminationFailure::Stale)?;
        let pidfd = pidfd_open(pid, PidfdFlags::empty()).map_err(map_open_error)?;
        let current = procfs::record_for_pid(target.native_pid)
            .map_err(|_| LinuxProcessTerminationFailure::Stale)?;
        if current.session_id != target.session_id
            || current.owner_epoch != target.owner_epoch
            || current.start_ticks != target.start_ticks
            || current.owner_uids != target.owner_uids
            || current.namespace_pids != target.namespace_pids
        {
            return Err(LinuxProcessTerminationFailure::Stale);
        }
        let lease = PidfdTerminationLease { pidfd };
        if lease.wait_exited(Duration::ZERO)? {
            return Err(LinuxProcessTerminationFailure::Stale);
        }
        Ok(Box::new(lease))
    }
}

fn current_execution_protection(
    credentials: &procfs::CurrentCredentialFacts,
) -> Option<LinuxProcessProtection> {
    if credentials.uids.contains(&0) {
        return Some(LinuxProcessProtection::RootExecution);
    }
    if credentials
        .uids
        .iter()
        .any(|uid| *uid != credentials.uids[0])
        || credentials.effective_capabilities & CAP_KILL_BIT != 0
    {
        return Some(LinuxProcessProtection::PrivilegedExecution);
    }
    None
}

fn target_protection(
    native_pid: u32,
    namespace_pids: &[u32],
    owner_uids: &[u32; 4],
    current_uid: u32,
    ancestors: &std::collections::BTreeSet<u32>,
) -> Option<LinuxProcessProtection> {
    if native_pid == 1 || namespace_pids.contains(&1) {
        return Some(LinuxProcessProtection::InitProcess);
    }
    if ancestors.contains(&native_pid) {
        return Some(LinuxProcessProtection::CurrentToolOrAncestor);
    }
    if owner_uids.iter().any(|uid| *uid != current_uid) {
        return Some(LinuxProcessProtection::DifferentUser);
    }
    None
}

impl LinuxProcessTerminationLease for PidfdTerminationLease {
    fn dispatch_graceful(&self) -> Result<(), LinuxProcessTerminationFailure> {
        pidfd_send_signal(&self.pidfd, Signal::TERM).map_err(map_signal_error)
    }

    fn dispatch_force(&self) -> Result<(), LinuxProcessTerminationFailure> {
        pidfd_send_signal(&self.pidfd, Signal::KILL).map_err(map_signal_error)
    }

    fn wait_exited(&self, timeout: Duration) -> Result<bool, LinuxProcessTerminationFailure> {
        let timeout =
            Timespec::try_from(timeout).map_err(|_| LinuxProcessTerminationFailure::WaitFailed)?;
        let mut descriptors = [PollFd::new(&self.pidfd, PollFlags::IN | PollFlags::HUP)];
        let ready = poll(&mut descriptors, Some(&timeout))
            .map_err(|_| LinuxProcessTerminationFailure::WaitFailed)?;
        if ready == 0 {
            return Ok(false);
        }
        let events = descriptors[0].revents();
        if events.intersects(PollFlags::ERR | PollFlags::NVAL) {
            return Err(LinuxProcessTerminationFailure::WaitFailed);
        }
        if events.intersects(PollFlags::IN | PollFlags::HUP) {
            Ok(true)
        } else {
            Err(LinuxProcessTerminationFailure::WaitFailed)
        }
    }
}

fn map_open_error(error: Errno) -> LinuxProcessTerminationFailure {
    match error {
        Errno::SRCH => LinuxProcessTerminationFailure::Stale,
        Errno::PERM => LinuxProcessTerminationFailure::PermissionDenied,
        Errno::NOSYS => LinuxProcessTerminationFailure::ProviderUnavailable,
        _ => LinuxProcessTerminationFailure::ProviderUnavailable,
    }
}

fn map_signal_error(error: Errno) -> LinuxProcessTerminationFailure {
    match error {
        Errno::SRCH => LinuxProcessTerminationFailure::Stale,
        Errno::PERM => LinuxProcessTerminationFailure::PermissionDenied,
        Errno::NOSYS => LinuxProcessTerminationFailure::ProviderUnavailable,
        _ => LinuxProcessTerminationFailure::DispatchFailed,
    }
}

/// 只读探测当前内核/权限是否具备 pidfd；不会发送信号。
pub(crate) fn runtime_supported() -> bool {
    if procfs::current_owner_epoch().is_err() {
        return false;
    }
    let Ok(credentials) = procfs::current_credential_facts() else {
        return false;
    };
    if credentials.uids.contains(&0)
        || credentials
            .uids
            .iter()
            .any(|uid| *uid != credentials.uids[0])
        || credentials.effective_capabilities & CAP_KILL_BIT != 0
        || procfs::current_ancestor_pids().is_err()
    {
        return false;
    }
    pidfd_open(rustix::process::getpid(), PidfdFlags::empty()).is_ok()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        io::{BufRead, BufReader, Write},
        process::{Child, Command, Stdio},
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    use serde_json::json;

    use super::*;

    #[test]
    fn privilege_and_target_protection_fixtures_fail_closed() {
        assert_eq!(
            current_execution_protection(&procfs::CurrentCredentialFacts {
                uids: [0; 4],
                effective_capabilities: 0,
            }),
            Some(LinuxProcessProtection::RootExecution)
        );
        assert_eq!(
            current_execution_protection(&procfs::CurrentCredentialFacts {
                uids: [1000; 4],
                effective_capabilities: CAP_KILL_BIT,
            }),
            Some(LinuxProcessProtection::PrivilegedExecution)
        );
        let ancestors = BTreeSet::from([41]);
        assert_eq!(
            target_protection(42, &[42, 1], &[1000; 4], 1000, &ancestors),
            Some(LinuxProcessProtection::InitProcess)
        );
        assert_eq!(
            target_protection(41, &[41], &[1000; 4], 1000, &ancestors),
            Some(LinuxProcessProtection::CurrentToolOrAncestor)
        );
        assert_eq!(
            target_protection(42, &[42], &[1001; 4], 1000, &ancestors),
            Some(LinuxProcessProtection::DifferentUser)
        );
        assert_eq!(
            target_protection(42, &[42], &[1000; 4], 1000, &ancestors),
            None
        );
    }

    struct OwnedChild(Child);

    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn pidfd_fixture_child() {
        if std::env::var_os("ACT_PIDFD_TERMINATION_FIXTURE").is_none() {
            return;
        }
        println!("ACT_PIDFD_TERMINATION_FIXTURE_READY");
        std::io::stdout()
            .flush()
            .unwrap_or_else(|error| panic!("fixture ready 应可刷新：{error}"));
        loop {
            thread::park_timeout(Duration::from_secs(60));
        }
    }

    #[test]
    fn pidfd_force_fixture_child() {
        if std::env::var_os("ACT_PIDFD_FORCE_TERMINATION_FIXTURE").is_none() {
            return;
        }
        // SAFETY: 项目自有测试子进程只把 SIGTERM 固定为忽略，用于证明 force 不误走温和路线。
        unsafe {
            let _ = libc::signal(libc::SIGTERM, libc::SIG_IGN);
        }
        println!("ACT_PIDFD_FORCE_TERMINATION_FIXTURE_READY");
        std::io::stdout()
            .flush()
            .unwrap_or_else(|error| panic!("force fixture ready 应可刷新：{error}"));
        loop {
            thread::park_timeout(Duration::from_secs(60));
        }
    }

    #[test]
    fn project_owned_child_is_terminated_and_result_validates() {
        if !runtime_supported() {
            return;
        }
        let executable =
            std::env::current_exe().unwrap_or_else(|error| panic!("应能定位当前测试程序：{error}"));
        let child = Command::new(executable)
            .args([
                "--exact",
                "adapters::linux::process_termination_pidfd::tests::pidfd_fixture_child",
                "--nocapture",
            ])
            .env("ACT_PIDFD_TERMINATION_FIXTURE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("应能启动项目自有 fixture：{error}"));
        let mut child = child;
        let stdout = child
            .stdout
            .take()
            .unwrap_or_else(|| panic!("fixture stdout pipe 必须存在"));
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let reader = thread::spawn(move || {
            let ready = BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
                .any(|line| line.contains("ACT_PIDFD_TERMINATION_FIXTURE_READY"));
            let _ = ready_sender.send(ready);
        });
        let mut child = OwnedChild(child);
        assert_eq!(
            ready_receiver.recv_timeout(Duration::from_secs(2)),
            Ok(true),
            "fixture 必须在目标解析前确认驻留"
        );
        let native_pid = child.0.id();
        let deadline = Instant::now() + Duration::from_secs(2);
        let target_id = loop {
            if let Some(target_id) = procfs::session_id_for_pid(native_pid) {
                break target_id;
            }
            assert!(Instant::now() < deadline, "fixture 未形成可解析进程代际");
            thread::sleep(Duration::from_millis(10));
        };
        let assessment = crate::modules::process_termination::assess(&target_id)
            .unwrap_or_else(|error| panic!("项目自有 fixture 应通过只读预检：{error:?}"));
        assert_eq!(assessment["decision"], "confirmation-required");
        assert_eq!(assessment["evidence"]["signalAttempted"], false);
        let mut request = crate::domain::CommandRequest::read(crate::domain::Verb::Run, "process");
        request.operation = Some("terminate-graceful".to_owned());
        request
            .target
            .insert("sessionId".to_owned(), json!(target_id));
        request
            .args
            .insert("input".to_owned(), json!({"timeoutMs": 5000}));
        request.confirmed = true;
        let result = crate::service::AppControlService::new()
            .execute(request)
            .unwrap_or_else(|error| panic!("项目自有 fixture 应被温和终止：{error:?}"));
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/v2/process-termination-graceful-result.schema.json"
        ))
        .unwrap_or_else(|error| panic!("结果 schema 应有效：{error}"));
        if let Err(error) = jsonschema::draft202012::validate(&schema, &result) {
            panic!("终止结果必须匹配契约：{error}");
        }
        assert!(procfs::session_id_for_pid(native_pid).is_none());
        let _ = child
            .0
            .wait()
            .unwrap_or_else(|error| panic!("应能回收 fixture：{error}"));
        reader
            .join()
            .unwrap_or_else(|_| panic!("fixture stdout reader 不得 panic"));
    }

    #[test]
    fn sigterm_ignoring_project_child_is_force_terminated_and_result_validates() {
        if !runtime_supported() {
            return;
        }
        let executable =
            std::env::current_exe().unwrap_or_else(|error| panic!("应能定位当前测试程序：{error}"));
        let child = Command::new(executable)
            .args([
                "--exact",
                "adapters::linux::process_termination_pidfd::tests::pidfd_force_fixture_child",
                "--nocapture",
            ])
            .env("ACT_PIDFD_FORCE_TERMINATION_FIXTURE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("应能启动项目自有 force fixture：{error}"));
        let mut child = child;
        let stdout = child
            .stdout
            .take()
            .unwrap_or_else(|| panic!("force fixture stdout pipe 必须存在"));
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let reader = thread::spawn(move || {
            let ready = BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
                .any(|line| line.contains("ACT_PIDFD_FORCE_TERMINATION_FIXTURE_READY"));
            let _ = ready_sender.send(ready);
        });
        let mut child = OwnedChild(child);
        assert_eq!(
            ready_receiver.recv_timeout(Duration::from_secs(2)),
            Ok(true),
            "force fixture 必须在目标解析前确认已忽略 SIGTERM"
        );
        let native_pid = child.0.id();
        let deadline = Instant::now() + Duration::from_secs(2);
        let target_id = loop {
            if let Some(target_id) = procfs::session_id_for_pid(native_pid) {
                break target_id;
            }
            assert!(
                Instant::now() < deadline,
                "force fixture 未形成可解析进程代际"
            );
            thread::sleep(Duration::from_millis(10));
        };
        let assessment = crate::modules::process_termination::assess_force(&target_id)
            .unwrap_or_else(|error| panic!("项目自有 force fixture 应通过只读预检：{error:?}"));
        assert_eq!(assessment["decision"], "confirmation-required");
        assert_eq!(assessment["constraints"]["riskLevel"], "critical");
        assert_eq!(assessment["evidence"]["processOwnerGenerationBound"], true);
        assert_eq!(assessment["evidence"]["signalAttempted"], false);
        let mut request = crate::domain::CommandRequest::read(crate::domain::Verb::Run, "process");
        request.operation = Some("terminate-force".to_owned());
        request
            .target
            .insert("sessionId".to_owned(), json!(target_id));
        request
            .args
            .insert("input".to_owned(), json!({"timeoutMs": 5000}));
        request.confirmed = true;
        let result = crate::service::AppControlService::new()
            .execute(request)
            .unwrap_or_else(|error| panic!("项目自有 force fixture 应被强制终止：{error:?}"));
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/v2/process-termination-force-result.schema.json"
        ))
        .unwrap_or_else(|error| panic!("force 结果 schema 应有效：{error}"));
        if let Err(error) = jsonschema::draft202012::validate(&schema, &result) {
            panic!("强制终止结果必须匹配契约：{error}");
        }
        assert_eq!(result["processOwnerGenerationBound"], true);
        assert_eq!(result["gracefulAttempted"], false);
        assert!(procfs::session_id_for_pid(native_pid).is_none());
        let _ = child
            .0
            .wait()
            .unwrap_or_else(|error| panic!("应能回收 force fixture：{error}"));
        reader
            .join()
            .unwrap_or_else(|_| panic!("force fixture stdout reader 不得 panic"));
    }
}
