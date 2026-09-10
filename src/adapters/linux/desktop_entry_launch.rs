//! Linux Desktop Entry 私有认证与无 shell 启动 Adapter。

use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::json;

use crate::domain::{AppControlError, AppResult};

/// 工具自有无窗口启动夹具的固定私有参数。
pub(crate) const TOOLKIT_FIXTURE_ARGUMENT: &str = "__application-launch-fixture-v1";

const FIXTURE_EXIT_TIMEOUT: Duration = Duration::from_secs(2);
const FIXTURE_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Desktop Entry 使用时重解析后允许交给启动边界的封闭路线。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopEntryLaunchRoute {
    /// 只启动当前正在运行的 toolkit 二进制及固定无副作用子命令。
    ToolkitFixtureV1,
}

/// 不携带 PID、路径或 argv 的最小启动证据。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LaunchEvidence {
    pub(crate) dispatched: bool,
    pub(crate) process_observed: bool,
    pub(crate) process_exit_observed: bool,
    pub(crate) fixture_exit_verified: bool,
}

/// 只解析足够表达 toolkit 自身映像夹具的 Desktop Entry Exec 严格子集。
fn parse_strict_exec(value: &str) -> Option<Vec<String>> {
    let mut arguments = Vec::new();
    let mut argument = String::new();
    let mut quoted = false;
    let mut started = false;
    for character in value.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            character if character.is_whitespace() && !quoted => {
                if started {
                    arguments.push(std::mem::take(&mut argument));
                    started = false;
                }
            }
            '\\' | '%' | '$' | '`' | ';' | '&' | '|' | '<' | '>' | '\'' => return None,
            character if character.is_control() => return None,
            character => {
                argument.push(character);
                started = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if started {
        arguments.push(argument);
    }
    (!arguments.is_empty() && arguments.iter().all(|argument| !argument.is_empty()))
        .then_some(arguments)
}

/// 核对 Exec 的绝对路径仍指向当前进程实际映像，而不是同名文件。
fn is_current_executable(path: &Path) -> bool {
    if !path.is_absolute() || std::env::current_exe().ok().as_deref() != Some(path) {
        return false;
    }
    let Some((candidate, running)) = fs::metadata(path)
        .ok()
        .zip(fs::metadata("/proc/self/exe").ok())
    else {
        return false;
    };
    candidate.is_file()
        && candidate.permissions().mode() & 0o111 != 0
        && candidate.dev() == running.dev()
        && candidate.ino() == running.ino()
}

/// 只认证精确 toolkit 自身映像夹具；不声明 Desktop Entry 文件归工具所有。
pub(crate) fn classify(
    exec: Option<&str>,
    dbus_activatable: bool,
) -> Option<DesktopEntryLaunchRoute> {
    if dbus_activatable {
        return None;
    }
    let arguments = parse_strict_exec(exec?)?;
    if arguments.len() != 2 || arguments[1] != TOOLKIT_FIXTURE_ARGUMENT {
        return None;
    }
    is_current_executable(Path::new(&arguments[0]))
        .then_some(DesktopEntryLaunchRoute::ToolkitFixtureV1)
}

fn pre_dispatch_error() -> AppControlError {
    AppControlError::with_details(
        "APPLICATION_START_FAILED",
        "The authenticated toolkit fixture process could not be started.",
        json!({
            "accepted": false,
            "retrySafe": true,
            "automaticRetryProhibited": false,
            "nativeIdentityExposed": false,
        }),
    )
}

fn accepted_error(code: &'static str, message: &'static str) -> AppControlError {
    AppControlError::with_details(
        code,
        message,
        json!({
            "accepted": true,
            "acceptedMayHaveOccurred": true,
            "retrySafe": false,
            "automaticRetryProhibited": true,
            "nativeIdentityExposed": false,
        }),
    )
}

/// 通过 `/proc/self/exe` 固定当前映像、固定 argv 与空环境直接启动，不调用 shell。
pub(crate) fn dispatch(route: DesktopEntryLaunchRoute) -> AppResult<LaunchEvidence> {
    match route {
        DesktopEntryLaunchRoute::ToolkitFixtureV1 => {}
    }
    let mut child = Command::new("/proc/self/exe")
        .arg(TOOLKIT_FIXTURE_ARGUMENT)
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| pre_dispatch_error())?;
    let deadline = Instant::now() + FIXTURE_EXIT_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                return Ok(LaunchEvidence {
                    dispatched: true,
                    process_observed: true,
                    process_exit_observed: true,
                    fixture_exit_verified: true,
                });
            }
            Ok(Some(_)) => {
                return Err(accepted_error(
                    "APPLICATION_START_FAILED",
                    "The authenticated toolkit fixture process exited unsuccessfully.",
                ));
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(FIXTURE_POLL_INTERVAL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(accepted_error(
                    "OUTCOME_UNKNOWN",
                    "The authenticated toolkit fixture launch did not reach a trustworthy final state.",
                ));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(accepted_error(
                    "OUTCOME_UNKNOWN",
                    "The authenticated toolkit fixture launch final state could not be observed.",
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_exec_accepts_only_fixed_current_fixture_shape() {
        let Ok(executable) = std::env::current_exe() else {
            panic!("测试映像必须可定位");
        };
        let raw = format!("\"{}\" {TOOLKIT_FIXTURE_ARGUMENT}", executable.display());
        assert_eq!(
            classify(Some(&raw), false),
            Some(DesktopEntryLaunchRoute::ToolkitFixtureV1)
        );
        assert_eq!(classify(Some(&raw), true), None);
        assert_eq!(classify(Some(&format!("{raw} extra")), false), None);
        assert_eq!(classify(Some(&format!("{raw}; touch canary")), false), None);
        assert_eq!(classify(Some("sh -c true"), false), None);
        assert_eq!(classify(Some("tool %f"), false), None);
    }
}
