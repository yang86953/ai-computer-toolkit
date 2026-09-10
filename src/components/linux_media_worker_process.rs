//! Linux MPRIS candidate worker 的固定进程启动与进程组回收 Component。
//!
//! 该 Component 只提供绝对 executable、唯一编译期固定参数、空环境和独立进程组；
//! worker 的 JSON 协议、超时映射与业务语义仍由各自 candidate client 负责。

use std::{
    os::fd::OwnedFd,
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    process::{Pid, PidfdFlags, pidfd_open},
};

use crate::domain::{AppControlError, AppResult};

/// 启动参数违反固定 worker 边界时返回的内部错误。
const INVALID_EXECUTABLE_MESSAGE: &str = "The worker executable must be an absolute path.";
/// 固定 worker 无法启动时返回的内部错误。
const START_FAILED_MESSAGE: &str = "The fixed Linux worker could not be started.";

/// 仅为自建且尚未回收的 worker 提供退出唤醒；不回收进程，不证明协议已完成。
/// pidfd 不可用时保留原有有界等待，调用方仍检查取消、总期限和 try_wait。
pub(crate) struct ExitNotification {
    descriptor: Option<OwnedFd>,
}

impl ExitNotification {
    pub(crate) fn new(child: &Child) -> Self {
        let descriptor = i32::try_from(child.id())
            .ok()
            .and_then(Pid::from_raw)
            .and_then(|pid| pidfd_open(pid, PidfdFlags::empty()).ok());
        Self { descriptor }
    }

    /// 最多等原有 2 ms 取消检查间隔；退出立即唤醒，不等待完整固定 sleep。
    pub(crate) fn wait_slice(&mut self, remaining: Duration) -> bool {
        let wait = remaining.min(Duration::from_millis(2));
        let started = Instant::now();
        if let Some(descriptor) = &self.descriptor {
            let timeout = Timespec {
                tv_sec: 0,
                tv_nsec: i64::from(wait.subsec_nanos()),
            };
            let mut descriptors = [PollFd::new(descriptor, PollFlags::IN | PollFlags::HUP)];
            match poll(&mut descriptors, Some(&timeout)) {
                Ok(0) => return false,
                Ok(_)
                    if descriptors[0]
                        .revents()
                        .intersects(PollFlags::IN | PollFlags::HUP) =>
                {
                    return true;
                }
                Err(rustix::io::Errno::INTR) => return false,
                _ => self.descriptor = None,
            }
        }
        thread::sleep(wait.saturating_sub(started.elapsed()));
        false
    }
}

/// 启动一个空环境、独立 process group 的固定 worker。
///
/// `hidden_argument` 只能追加一个由调用方编译期固定的 argv；不接受空入口或参数数组，
/// 避免把任意 argv 注入 worker。
pub(crate) fn spawn(executable: &Path, hidden_argument: &'static str) -> AppResult<Child> {
    if !executable.is_absolute() {
        return Err(AppControlError::new(
            "WORKER_START_FAILED",
            INVALID_EXECUTABLE_MESSAGE,
        ));
    }

    let parent_pid = unsafe { libc::getpid() };
    let mut command = Command::new(executable);
    command
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    command.arg(hidden_argument);
    // SAFETY: pre_exec 仅执行 async-signal-safe 的 prctl/getppid 竞态检查。
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent_pid {
                return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
            }
            Ok(())
        });
    }
    command
        .spawn()
        .map_err(|_| AppControlError::new("WORKER_START_FAILED", START_FAILED_MESSAGE))
}

/// 杀死固定 worker 的独立进程组并等待 leader，确保子进程不会残留。
pub(crate) fn kill_and_wait(child: &mut Child) {
    let process_group = match i32::try_from(child.id()) {
        Ok(process_group) => process_group,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
    };
    // SAFETY: spawn 已用 process_group(0) 建立 child-pid 进程组；负值只命中该组。
    unsafe {
        let _ = libc::kill(-process_group, libc::SIGKILL);
    }
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "child-only fixture waits for its parent's stdin EOF"]
    fn exit_notification_fixture() {
        use std::io::Read;
        let mut input = Vec::new();
        std::io::stdin().read_to_end(&mut input).unwrap();
    }

    #[test]
    fn notification_wakes_for_own_child_but_does_not_reap_it() {
        let mut child = Command::new("/proc/self/exe")
            .args([
                "--exact",
                "components::linux_media_worker_process::tests::exit_notification_fixture",
                "--ignored",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut notification = ExitNotification::new(&child);
        let supported = notification.descriptor.is_some();
        assert!(!notification.wait_slice(Duration::ZERO));
        assert!(child.try_wait().unwrap().is_none());
        drop(child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut signalled = false;
        while supported && Instant::now() < deadline {
            if notification.wait_slice(Duration::from_millis(2)) {
                signalled = true;
                break;
            }
        }
        let status = child.wait().unwrap();
        assert!(status.success());
        assert!(
            !supported || signalled,
            "available pidfd must signal exit without reaping"
        );
    }

    #[test]
    fn missing_pidfd_keeps_a_bounded_nonterminal_wait() {
        let mut notification = ExitNotification { descriptor: None };
        assert!(!notification.wait_slice(Duration::ZERO));
        assert!(notification.descriptor.is_none());
    }
}
