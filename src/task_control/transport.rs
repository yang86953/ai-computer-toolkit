//! 授权文件与 JSON Lines 传输；读取、取消和总截止时间不等待 provider 返回。

use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{self, BufRead, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError, TrySendError},
    },
    thread,
    time::Duration,
};

use serde_json::Value;

use crate::{components::cancellation, domain::AppResult};

use super::{
    policy,
    protocol::{
        CONTRACT, MAX_FRAME_BYTES, MAX_PENDING, MAX_REQUESTS, Operation, Request, TaskGrant,
        valid_request_id,
    },
    session::{TaskSession, failure},
};

pub(super) fn reject_startup() -> i32 {
    let _ = emit(&failure(
        None,
        policy::error(
            "INVALID_ARGUMENT",
            "Use serve --stdio --grant-file=<trusted-task-grant.json>.",
        ),
        "not-dispatched",
    ));
    2
}

fn load_grant(path: &str) -> AppResult<TaskGrant> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // 只打开目录项本身；随后拒绝 reparse point，不跟随到另一个授权来源。
        options.custom_flags(0x0020_0000);
    }
    let mut file = options.open(path).map_err(|_| {
        policy::error(
            "TASK_GRANT_INVALID",
            "The trusted startup grant cannot be opened.",
        )
    })?;
    let metadata = file.metadata().map_err(|_| {
        policy::error(
            "TASK_GRANT_INVALID",
            "The startup grant metadata is unavailable.",
        )
    })?;
    if !metadata.is_file() || metadata.len() > MAX_FRAME_BYTES as u64 {
        return Err(policy::error(
            "TASK_GRANT_INVALID",
            "The startup grant must be a bounded regular file.",
        ));
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid 不接收指针，仅返回本进程的有效用户标识。
        let owner = unsafe { libc::geteuid() };
        if metadata.uid() != owner || metadata.mode() & 0o022 != 0 {
            return Err(policy::error(
                "TASK_GRANT_INVALID",
                "The task grant must be owned by the caller and not writable by other users.",
            ));
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x0000_0400 != 0 {
            return Err(policy::error(
                "TASK_GRANT_INVALID",
                "Reparse points cannot provide task authorization.",
            ));
        }
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_FRAME_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| policy::error("TASK_GRANT_INVALID", "The startup grant could not be read."))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(policy::error(
            "TASK_GRANT_INVALID",
            "The startup grant exceeds its byte limit.",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        policy::error(
            "TASK_GRANT_INVALID",
            "The startup grant does not match its closed JSON contract.",
        )
    })
}

enum Input {
    Request(Request),
    Rejected(Request, &'static str),
    Invalid,
}

/// 累计到硬上限即拒绝，不使用会为任意长行分配内存的 read_line。
fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Ok(Some(bytes))
            };
        }
        let length = chunk
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(chunk.len(), |index| index + 1);
        if bytes.len().saturating_add(length) > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "task frame limit exceeded",
            ));
        }
        let complete = chunk[length - 1] == b'\n';
        bytes.extend_from_slice(&chunk[..length]);
        reader.consume(length);
        if complete {
            return Ok(Some(bytes));
        }
    }
}

/// 输入身份校验先于取消信号；冲突请求不能先取消任务再收到拒绝。
fn admit_request(
    request: Request,
    task_id: &str,
    identities: &mut HashMap<String, Vec<u8>>,
) -> Input {
    if request.contract_version != CONTRACT
        || request.task_id != task_id
        || !valid_request_id(&request.request_id)
    {
        return Input::Request(request);
    }
    let canonical = match serde_json::to_vec(&request) {
        Ok(canonical) => canonical,
        Err(_) => return Input::Invalid,
    };
    if let Some(previous) = identities.get(&request.request_id) {
        if previous != &canonical {
            return Input::Rejected(request, "REQUEST_ID_CONFLICT");
        }
    } else if identities.len() < MAX_REQUESTS {
        identities.insert(request.request_id.clone(), canonical);
    } else if !matches!(
        request.operation,
        Operation::Status | Operation::Cancel | Operation::Close
    ) {
        return Input::Rejected(request, "TASK_CAPACITY_EXHAUSTED");
    }
    if matches!(request.operation, Operation::Cancel) {
        cancellation::request_cancellation();
    }
    Input::Request(request)
}

fn pump_input(
    mut input: impl BufRead,
    task_id: &str,
    sender: mpsc::SyncSender<Input>,
    overflow: &AtomicBool,
) {
    let mut identities = HashMap::new();
    loop {
        let item = match read_frame(&mut input) {
            Ok(Some(bytes)) => match serde_json::from_slice::<Request>(&bytes) {
                Ok(request) => admit_request(request, task_id, &mut identities),
                Err(_) => Input::Invalid,
            },
            // EOF 通过 sender 析构表达，不能占用一个队列槽并把恰好满队列误判成溢出。
            Ok(None) => return,
            Err(_) => Input::Invalid,
        };
        let terminal = matches!(item, Input::Invalid);
        match sender.try_send(item) {
            Ok(()) => {}
            Err(TrySendError::Disconnected(_)) => return,
            Err(TrySendError::Full(_)) => {
                overflow.store(true, Ordering::Release);
                cancellation::request_cancellation();
                return;
            }
        }
        if terminal {
            return;
        }
    }
}

fn emit(value: &Value) -> io::Result<()> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, value)?;
    output.write_all(b"\n")?;
    output.flush()
}

pub(super) fn run(grant_file: &str) -> i32 {
    let mut session = match load_grant(grant_file).and_then(TaskSession::new) {
        Ok(session) => session,
        Err(error) => {
            let _ = emit(&failure(None, error, "not-dispatched"));
            return 2;
        }
    };
    let task_id = session.task_id().to_owned();
    let (sender, receiver) = mpsc::sync_channel(MAX_PENDING);
    let overflow = Arc::new(AtomicBool::new(false));
    let reader_overflow = Arc::clone(&overflow);
    // 读取线程只拥有进程级 stdin；关闭入口后随宿主进程退出，不借用领域对象。
    let reader = thread::Builder::new()
        .name("task-stdin".into())
        .spawn(move || {
            pump_input(io::stdin().lock(), &task_id, sender, &reader_overflow);
        });
    if reader.is_err() {
        let _ = emit(&failure(
            None,
            policy::error("OPERATION_FAILED", "The task input owner could not start."),
            "not-dispatched",
        ));
        return 2;
    }
    let (stop_deadline, deadline_stopped) = mpsc::sync_channel::<()>(1);
    let timeout = session.remaining();
    let deadline_thread = thread::Builder::new()
        .name("task-deadline".into())
        .spawn(move || {
            if matches!(
                deadline_stopped.recv_timeout(timeout),
                Err(RecvTimeoutError::Timeout)
            ) {
                cancellation::request_cancellation();
            }
        });
    if deadline_thread.is_err() {
        cancellation::request_cancellation();
        let _ = emit(&failure(
            None,
            policy::error(
                "OPERATION_FAILED",
                "The task deadline owner could not start.",
            ),
            "not-dispatched",
        ));
        return 2;
    }
    let exit_code = loop {
        if overflow.load(Ordering::Acquire) {
            let _ = emit(&failure(
                None,
                policy::error(
                    "TASK_CAPACITY_EXHAUSTED",
                    "The bounded task input queue overflowed; pending work is cancelled.",
                ),
                "not-dispatched",
            ));
            break 2;
        }
        if session.remaining().is_zero() {
            let _ = emit(&failure(
                None,
                policy::error("TASK_EXPIRED", "The task's original deadline has expired."),
                "not-dispatched",
            ));
            break 2;
        }
        match receiver.recv_timeout(session.remaining().min(Duration::from_millis(100))) {
            Ok(Input::Request(request)) => {
                if emit(&session.handle(request)).is_err() {
                    cancellation::request_cancellation();
                    break 2;
                }
                if session.is_closed() {
                    break 0;
                }
            }
            Ok(Input::Rejected(request, code)) => {
                if emit(&failure(
                    Some(&request),
                    policy::error(
                        code,
                        "The input request identity is conflicting or its ledger is full.",
                    ),
                    "not-dispatched",
                ))
                .is_err()
                {
                    cancellation::request_cancellation();
                    break 2;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break 0,
            Ok(Input::Invalid) => {
                cancellation::request_cancellation();
                let _ = emit(&failure(
                    None,
                    policy::error(
                        "INVALID_ARGUMENT",
                        "The task input is not a bounded, valid request frame.",
                    ),
                    "not-dispatched",
                ));
                break 2;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    };
    // 截止时间所有者必须有界退出；stdin 线程由独立 CLI 进程的退出统一回收。
    let _ = stop_deadline.send(());
    if let Ok(thread) = deadline_thread {
        let _ = thread.join();
    }
    exit_code
}

#[cfg(test)]
mod tests {
    use super::{Input, MAX_PENDING, pump_input};
    use serde_json::json;
    use std::{
        io::Cursor,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
    };

    #[test]
    fn eof_after_exact_queue_capacity_does_not_cancel_pending_requests() {
        let task_id = "t2:0123456789abcdef0123456789abcdef";
        let source = (0..MAX_PENDING).map(|index| format!("{}\n", json!({
            "contractVersion": "act/control/v2", "requestId": format!("r-{index}"), "taskId": task_id,
            "operation": { "type": "task.status" }
        }))).collect::<String>();
        let (sender, receiver) = mpsc::sync_channel(MAX_PENDING);
        let overflow = AtomicBool::new(false);
        // 消费者尚未取走任何条目，确定性覆盖队列满时的 EOF。
        pump_input(Cursor::new(source), task_id, sender, &overflow);
        assert!(!overflow.load(Ordering::Acquire));
        for _ in 0..MAX_PENDING {
            assert!(matches!(receiver.try_recv(), Ok(Input::Request(_))));
        }
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}
