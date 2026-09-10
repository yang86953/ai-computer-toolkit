//! 在固定 Linux 自进程 worker 中执行单个 Workflow step 并保留双阶段事实。

use std::{
    io::{Read, Write},
    os::unix::process::CommandExt,
    process::{Child, ChildStderr, ChildStdout, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    components::sequence_step_protocol::{
        MAXIMUM_OUTPUT_BYTES, SequenceStepWorkerControl, SequenceStepWorkerRequest,
        frames::{SequenceStepFrameObservation, parse_frame_log},
    },
    domain::{AppControlError, AppResult},
};

const WORKER_ARGUMENT: &str = "__sequence-step-worker-v1";
const MAXIMUM_STDERR_BYTES: usize = 64 * 1024;
const WAIT_SLICE: Duration = Duration::from_millis(5);
const COOPERATIVE_STOP_WINDOW: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SequenceStepRunnerStop {
    Completed,
    Cancelled,
    Deadline,
}

pub(crate) struct SequenceStepRunnerOutput {
    observation: SequenceStepFrameObservation,
    stop: SequenceStepRunnerStop,
    forced_reap: bool,
}

impl SequenceStepRunnerOutput {
    pub(crate) const fn observation(&self) -> &SequenceStepFrameObservation {
        &self.observation
    }

    pub(crate) const fn stop(&self) -> SequenceStepRunnerStop {
        self.stop
    }

    pub(crate) const fn forced_reap(&self) -> bool {
        self.forced_reap
    }
}

#[derive(Clone, Copy)]
enum WorkerErrorCode {
    OutputTooLarge,
    ProtocolFailed,
    StartFailed,
    WaitFailed,
}

impl WorkerErrorCode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::OutputTooLarge => "WORKER_OUTPUT_TOO_LARGE",
            Self::ProtocolFailed => "WORKER_PROTOCOL_FAILED",
            Self::StartFailed => "WORKER_START_FAILED",
            Self::WaitFailed => "WORKER_WAIT_FAILED",
        }
    }

    fn error(self, message: impl Into<String>) -> AppControlError {
        AppControlError::new(self.as_str(), message)
    }
}

/// 使用当前已执行映像的固定隐藏入口运行唯一 step。
pub(crate) fn run(
    request: &SequenceStepWorkerRequest,
    cancelled: impl Fn() -> bool,
) -> AppResult<SequenceStepRunnerOutput> {
    let started = Instant::now();
    let request_nonce = request.request_nonce().to_owned();
    let timeout = Duration::from_millis(u64::from(request.timeout_ms()));
    let request_line = request
        .to_line()
        .map_err(|failure| WorkerErrorCode::ProtocolFailed.error(failure.code().as_str()))?;
    let cancel_line = SequenceStepWorkerControl::cancel(&request_nonce)
        .and_then(|control| control.to_line())
        .map_err(|failure| WorkerErrorCode::ProtocolFailed.error(failure.code().as_str()))?;

    let mut child = spawn_worker()?;
    let process_group = match i32::try_from(child.id()) {
        Ok(process_group) => process_group,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(WorkerErrorCode::StartFailed
                .error("The fixed Workflow worker process id overflowed."));
        }
    };
    let Some(mut stdin) = child.stdin.take() else {
        reap_process_group(&mut child, process_group);
        return Err(
            WorkerErrorCode::StartFailed.error("The fixed Workflow worker stdin was unavailable.")
        );
    };
    let Some(stdout) = child.stdout.take() else {
        reap_process_group(&mut child, process_group);
        return Err(
            WorkerErrorCode::StartFailed.error("The fixed Workflow worker stdout was unavailable.")
        );
    };
    let Some(stderr) = child.stderr.take() else {
        reap_process_group(&mut child, process_group);
        return Err(
            WorkerErrorCode::StartFailed.error("The fixed Workflow worker stderr was unavailable.")
        );
    };

    let accepted = Arc::new(AtomicBool::new(false));
    let reader_failed = Arc::new(AtomicBool::new(false));
    let stdout_reader = match spawn_stdout_reader(
        stdout,
        request_nonce.clone(),
        Arc::clone(&accepted),
        Arc::clone(&reader_failed),
    ) {
        Ok(reader) => reader,
        Err(error) => {
            reap_process_group(&mut child, process_group);
            return Err(error);
        }
    };
    let stderr_reader = match spawn_stderr_reader(stderr) {
        Ok(reader) => reader,
        Err(error) => {
            reap_process_group(&mut child, process_group);
            let _ = join_reader(stdout_reader);
            return Err(error);
        }
    };

    if stdin
        .write_all(&request_line)
        .and_then(|_| stdin.flush())
        .is_err()
    {
        reap_process_group(&mut child, process_group);
        let _ = join_reader(stdout_reader);
        let _ = join_reader(stderr_reader);
        return Err(WorkerErrorCode::StartFailed
            .error("The fixed Workflow worker request could not be delivered."));
    }

    let mut stop = SequenceStepRunnerStop::Completed;
    let mut stop_requested_at = None;
    let mut control_sent = false;
    let mut forced_reap = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => {
                reap_process_group(&mut child, process_group);
                let _ = join_reader(stdout_reader);
                let _ = join_reader(stderr_reader);
                return Err(WorkerErrorCode::WaitFailed
                    .error("Waiting for the fixed Workflow worker failed."));
            }
        }
        if reader_failed.load(Ordering::Acquire) {
            reap_process_group(&mut child, process_group);
            forced_reap = true;
            break;
        }
        if !control_sent {
            let elapsed = started.elapsed();
            let deadline_control_at = timeout.saturating_sub(COOPERATIVE_STOP_WINDOW);
            if cancelled() {
                stop = SequenceStepRunnerStop::Cancelled;
                send_control(&mut stdin, &cancel_line);
                control_sent = true;
                stop_requested_at = Some(Instant::now());
            } else if elapsed >= deadline_control_at {
                stop = SequenceStepRunnerStop::Deadline;
                send_control(&mut stdin, &cancel_line);
                control_sent = true;
                stop_requested_at = Some(Instant::now());
            }
        }
        if stop == SequenceStepRunnerStop::Cancelled
            && stop_requested_at.is_some_and(|instant| instant.elapsed() >= COOPERATIVE_STOP_WINDOW)
        {
            reap_process_group(&mut child, process_group);
            forced_reap = true;
            break;
        }
        if started.elapsed() >= timeout {
            if stop == SequenceStepRunnerStop::Completed {
                stop = SequenceStepRunnerStop::Deadline;
            }
            reap_process_group(&mut child, process_group);
            forced_reap = true;
            break;
        }
        thread::sleep(WAIT_SLICE);
    }
    drop(stdin);

    let stdout = join_reader(stdout_reader)?;
    let stderr = join_reader(stderr_reader)?;
    if !stderr.is_empty() {
        return Err(WorkerErrorCode::ProtocolFailed
            .error("The fixed Workflow worker wrote diagnostics to stderr."));
    }
    let text = std::str::from_utf8(&stdout).map_err(|_| {
        WorkerErrorCode::ProtocolFailed.error("Workflow worker stdout is not UTF-8.")
    })?;
    let observation = parse_frame_log(text, &request_nonce, forced_reap).map_err(|failure| {
        WorkerErrorCode::ProtocolFailed.error(format!(
            "Workflow worker output failed with {}.",
            failure.code().as_str()
        ))
    })?;
    Ok(SequenceStepRunnerOutput {
        observation,
        stop,
        forced_reap,
    })
}

fn spawn_worker() -> AppResult<Child> {
    let parent_pid = unsafe { libc::getpid() };
    let mut command = Command::new("/proc/self/exe");
    command
        .arg(WORKER_ARGUMENT)
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // SAFETY: pre_exec 只调用 async-signal-safe 的 prctl/getppid 并返回 errno。
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
    command.spawn().map_err(|_| {
        WorkerErrorCode::StartFailed.error("The fixed Workflow self-worker could not be started.")
    })
}

fn send_control(stdin: &mut impl Write, control: &[u8]) {
    let _ = stdin.write_all(control).and_then(|_| stdin.flush());
}

fn reap_process_group(child: &mut Child, process_group: i32) {
    // SAFETY: child 创建了独立同 PID process group；负值只命中该 worker 组。
    unsafe {
        let _ = libc::kill(-process_group, libc::SIGKILL);
    }
    let _ = child.wait();
}

fn spawn_stdout_reader(
    mut stdout: ChildStdout,
    request_nonce: String,
    accepted: Arc<AtomicBool>,
    reader_failed: Arc<AtomicBool>,
) -> AppResult<JoinHandle<AppResult<Vec<u8>>>> {
    thread::Builder::new()
        .name("act-linux-sequence-stdout".to_owned())
        .spawn(move || {
            let mut output = Vec::with_capacity(8 * 1024);
            let mut buffer = [0_u8; 4 * 1024];
            let mut first_frame_observed = false;
            loop {
                let read = stdout.read(&mut buffer).map_err(|_| {
                    reader_failed.store(true, Ordering::Release);
                    WorkerErrorCode::ProtocolFailed
                        .error("The Workflow worker stdout could not be read.")
                })?;
                if read == 0 {
                    break;
                }
                if output.len().saturating_add(read) > MAXIMUM_OUTPUT_BYTES {
                    reader_failed.store(true, Ordering::Release);
                    return Err(WorkerErrorCode::OutputTooLarge
                        .error("The Workflow worker exceeded its output boundary."));
                }
                output.extend_from_slice(&buffer[..read]);
                if !first_frame_observed
                    && let Some(end) = output.iter().position(|byte| *byte == b'\n')
                {
                    let first = std::str::from_utf8(&output[..=end]).map_err(|_| {
                        reader_failed.store(true, Ordering::Release);
                        WorkerErrorCode::ProtocolFailed
                            .error("The Workflow worker first frame is not UTF-8.")
                    })?;
                    let observation =
                        parse_frame_log(first, &request_nonce, true).map_err(|failure| {
                            reader_failed.store(true, Ordering::Release);
                            WorkerErrorCode::ProtocolFailed.error(failure.code().as_str())
                        })?;
                    accepted.store(observation.dispatch_accepted(), Ordering::Release);
                    first_frame_observed = true;
                }
            }
            Ok(output)
        })
        .map_err(|_| {
            WorkerErrorCode::StartFailed.error("The Workflow stdout reader could not be started.")
        })
}

fn spawn_stderr_reader(mut stderr: ChildStderr) -> AppResult<JoinHandle<AppResult<Vec<u8>>>> {
    thread::Builder::new()
        .name("act-linux-sequence-stderr".to_owned())
        .spawn(move || {
            let mut bytes = Vec::with_capacity(1024);
            stderr
                .by_ref()
                .take((MAXIMUM_STDERR_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| {
                    WorkerErrorCode::ProtocolFailed
                        .error("The Workflow worker stderr could not be read.")
                })?;
            if bytes.len() > MAXIMUM_STDERR_BYTES {
                return Err(WorkerErrorCode::OutputTooLarge
                    .error("The Workflow worker exceeded its stderr boundary."));
            }
            Ok(bytes)
        })
        .map_err(|_| {
            WorkerErrorCode::StartFailed.error("The Workflow stderr reader could not be started.")
        })
}

fn join_reader(reader: JoinHandle<AppResult<Vec<u8>>>) -> AppResult<Vec<u8>> {
    reader.join().map_err(|_| {
        WorkerErrorCode::WaitFailed.error("A Workflow worker reader terminated unexpectedly.")
    })?
}
