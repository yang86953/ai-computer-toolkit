//! 给常驻桌面 broker 提供收到终态即返回的本机单次调用入口。

use std::{
    fs::{self, DirBuilder},
    io::{BufReader, Read},
    net::Shutdown,
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{Mutex, atomic::AtomicBool},
    thread,
    time::Instant,
};

use super::*;

const CONNECTION_LIMIT: usize = 32;
const CONNECTION_IO_TIMEOUT: Duration = Duration::from_secs(2);
const CLIENT_RESPONSE_TIMEOUT: Duration = Duration::from_secs(35);
const RESPONSE_LIMIT: u64 = 1024 * 1024;

/// endpoint 的清理由创建者负责，不能删除后来替换的路径。
struct Endpoint {
    path: PathBuf,
    inode: u64,
    device: u64,
}

impl Endpoint {
    fn bind(directory: &Path, epoch: &str) -> io::Result<(Self, UnixListener)> {
        let path = directory.join(format!("{epoch}.sock"));
        let listener = UnixListener::bind(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        let endpoint = Self {
            path,
            inode: metadata.ino(),
            device: metadata.dev(),
        };
        fs::set_permissions(&endpoint.path, fs::Permissions::from_mode(0o600))?;
        Ok((endpoint, listener))
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
            metadata.file_type().is_socket()
                && metadata.ino() == self.inode
                && metadata.dev() == self.device
        }) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// 只从内核 UID 解析既有私有运行目录，不接受调用方指定 socket 路径。
fn runtime_directory(create: bool) -> io::Result<PathBuf> {
    // geteuid 不读取环境变量，也不改变任何权限。
    let uid = unsafe { libc::geteuid() };
    let runtime = PathBuf::from(format!("/run/user/{uid}"));
    validate_directory(&runtime, uid)?;
    let directory = runtime.join("ai-computer-toolkit-desktop");
    if create {
        match DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    validate_directory(&directory, uid)?;
    Ok(directory)
}

fn validate_directory(path: &Path, uid: u32) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private runtime directory required",
        ));
    }
    Ok(())
}

/// 除文件权限外再验证对端真实 UID，拒绝来自其他用户的连接。
fn same_user(stream: &UnixStream) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let mut credentials = std::mem::MaybeUninit::<libc::ucred>::uninit();
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // getsockopt 只写入固定大小的 ucred，成功及长度复核后才读取。
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            credentials.as_mut_ptr().cast(),
            &raw mut size,
        )
    };
    if result != 0 || size as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::last_os_error());
    }
    let credentials = unsafe { credentials.assume_init() };
    if credentials.uid != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "peer rejected",
        ));
    }
    Ok(())
}

struct SocketRequest {
    request: BrokerRequest,
    response: UnixStream,
}

/// reader 只解析传输及置位取消，EIS 和全部业务调用继续属于原 owner 线程。
fn receive_request(
    mut stream: UnixStream,
    epoch: &str,
    sender: &mpsc::SyncSender<SocketRequest>,
    semantics: &Mutex<RequestSemantics>,
    cancellations: &DesktopInputCancellationRegistry,
) {
    let result = (|| -> io::Result<BrokerRequest> {
        same_user(&stream)?;
        stream.set_read_timeout(Some(CONNECTION_IO_TIMEOUT))?;
        stream.set_write_timeout(Some(CONNECTION_IO_TIMEOUT))?;
        let mut reader = BufReader::new(&mut stream);
        let line = read_bounded_line(&mut reader)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "request missing"))?;
        // 单次调用必须提交一帧后关闭写半边，禁止额外命令偷渡。
        let mut trailing = [0_u8; 1];
        if reader.read(&mut trailing)? != 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "extra request"));
        }
        serde_json::from_str(&line).map_err(io::Error::other)
    })();
    let request = match result {
        Ok(request) => request,
        Err(_) => {
            let _ = write_json_line(&mut stream, &terminal_error("BROKER_PROTOCOL_FAILED"));
            return;
        }
    };
    let mut semantics = semantics
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    prepare_control_request(&request, epoch, &mut semantics, cancellations);
    if let Err(error) = sender.try_send(SocketRequest {
        request,
        response: stream,
    }) {
        let mut pending = match error {
            mpsc::TrySendError::Full(pending) | mpsc::TrySendError::Disconnected(pending) => {
                pending
            }
        };
        let _ = write_json_line(&mut pending.response, &terminal_error("BROKER_QUEUE_FULL"));
    }
}

/// listener 与有界 reader 不拥有任何桌面资源，也不执行绘图或应用脚本。
fn accept_connections(
    listener: UnixListener,
    epoch: String,
    sender: mpsc::SyncSender<SocketRequest>,
    cancellations: Arc<DesktopInputCancellationRegistry>,
    stopping: Arc<AtomicBool>,
) {
    let semantics = Arc::new(Mutex::new(RequestSemantics::default()));
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    while !stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                if active.load(Ordering::Acquire) >= CONNECTION_LIMIT {
                    let _ = stream.set_write_timeout(Some(CONNECTION_IO_TIMEOUT));
                    let _ = write_json_line(&mut stream, &terminal_error("BROKER_QUEUE_FULL"));
                    continue;
                }
                active.fetch_add(1, Ordering::AcqRel);
                let (active, sender, semantics, cancellations, epoch) = (
                    Arc::clone(&active),
                    sender.clone(),
                    Arc::clone(&semantics),
                    Arc::clone(&cancellations),
                    epoch.clone(),
                );
                thread::spawn(move || {
                    receive_request(stream, &epoch, &sender, &semantics, &cancellations);
                    active.fetch_sub(1, Ordering::AcqRel);
                });
            }
            Err(_) => break,
        }
    }
}

fn serve_socket<P: DesktopSessionPort>(
    listener: UnixListener,
    mut broker: Broker<P>,
    ready_writer: &mut impl Write,
) -> i32 {
    let (sender, receiver) = mpsc::sync_channel(MAXIMUM_PENDING_FRAMES);
    let stopping = Arc::new(AtomicBool::new(false));
    let reader_stopping = Arc::clone(&stopping);
    let epoch = broker.epoch.clone();
    let cancellations = broker.cancellation_registry();
    let wake_path = listener
        .local_addr()
        .ok()
        .and_then(|address| address.as_pathname().map(Path::to_path_buf));
    let reader = thread::spawn(move || {
        accept_connections(listener, epoch, sender, cancellations, reader_stopping);
    });
    let mut ready = broker.ready();
    ready["transport"] = json!("json-lines-unix-socket");
    let mut exit_code = 0;
    if write_json_line(ready_writer, &ready).is_err() {
        exit_code = 2;
    } else {
        while let Ok(mut pending) = receiver.recv() {
            let (response, shutdown) = broker.handle(pending.request);
            // 业务终态已经保存在原 nonce 台账；客户端断开不允许重做操作。
            let _ = write_json_line(&mut pending.response, &response);
            if shutdown {
                break;
            }
        }
    }
    stopping.store(true, Ordering::Release);
    // 阻塞 accept 只在真实连接或明确关闭时唤醒，空闲时没有轮询。
    if let Some(path) = wake_path {
        let _ = UnixStream::connect(path);
    }
    let _ = reader.join();
    exit_code
}

/// 创建私有 socket，但不自动创建 Portal 会话或发送输入。
pub fn run_socket_server() -> i32 {
    let prepared = (|| {
        let epoch = desktop_session_identity::random_nonce().map_err(io::Error::other)?;
        let directory = runtime_directory(true)?;
        let (endpoint, listener) = Endpoint::bind(&directory, &epoch)?;
        Ok::<_, io::Error>((epoch, endpoint, listener))
    })();
    let (epoch, _endpoint, listener) = match prepared {
        Ok(prepared) => prepared,
        Err(_) => {
            let _ = write_json_line(
                &mut io::stdout().lock(),
                &terminal_error("BROKER_ENDPOINT_UNAVAILABLE"),
            );
            return 2;
        }
    };
    serve_socket(
        listener,
        Broker::new(
            epoch,
            DesktopSessionModule::new(SystemDesktopSessionPort::default()),
        ),
        &mut io::stdout().lock(),
    )
}

#[derive(Debug)]
struct ClientFailure {
    dispatched: bool,
}

impl ClientFailure {
    fn response(&self, request: &BrokerRequest) -> Value {
        json!({
            "contractVersion": CONTRACT_VERSION,
            "messageType": "client-error",
            "brokerEpoch": request.broker_epoch(),
            "requestNonce": request.request_nonce(),
            "operation": request.operation(),
            "code": "BROKER_TRANSPORT_FAILED",
            "terminal": true,
            "outcome": if self.dispatched { "unknown" } else { "failed" },
            "completed": false,
            "acceptedMayHaveOccurred": self.dispatched,
            "retrySafe": false,
            "automaticRetryProhibited": true,
        })
    }
}

fn request_response(directory: &Path, request: &BrokerRequest) -> Result<Value, ClientFailure> {
    let mut dispatched = false;
    exchange(directory, request, &mut dispatched).map_err(|_| ClientFailure { dispatched })
}

fn exchange(directory: &Path, request: &BrokerRequest, dispatched: &mut bool) -> io::Result<Value> {
    if !canonical_nonce(request.broker_epoch()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid epoch"));
    }
    let path = directory.join(format!("{}.sock", request.broker_epoch()));
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "endpoint rejected",
        ));
    }
    let mut stream = UnixStream::connect(path)?;
    same_user(&stream)?;
    stream.set_write_timeout(Some(CONNECTION_IO_TIMEOUT))?;
    let serialized = serde_json::to_value(request).map_err(io::Error::other)?;
    // 一旦尝试写入，即使只写入部分字节，也不能推断 broker 没有接受操作。
    *dispatched = true;
    write_json_line(&mut stream, &serialized)?;
    stream.shutdown(Shutdown::Write)?;
    let timeout = match request {
        BrokerRequest::Open { timeout_ms, .. } => {
            Duration::from_millis(u64::from((*timeout_ms).min(300_000)) + 5_000)
        }
        // A combined request has two independently bounded 30s stages.
        BrokerRequest::Interact {
            observation: Some(_),
            ..
        } => Duration::from_secs(65),
        _ => CLIENT_RESPONSE_TIMEOUT,
    };
    let response = read_response(&mut stream, timeout)?;
    if response["messageType"] != "response"
        || response["contractVersion"] != CONTRACT_VERSION
        || response["brokerEpoch"] != request.broker_epoch()
        || response["requestNonce"] != request.request_nonce()
        || response["operation"] != request.operation()
        || !response["completed"].is_boolean()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "response mismatch",
        ));
    }
    Ok(response)
}

/// 总期限不会被分段到达的字节延长，超限响应也不会继续分配。
fn read_response(stream: &mut UnixStream, timeout: Duration) -> io::Result<Value> {
    let deadline = Instant::now() + timeout;
    let mut response = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "response timeout"))?;
        stream.set_read_timeout(Some(remaining))?;
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if response.len().saturating_add(count) as u64 > RESPONSE_LIMIT {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "response limit"));
        }
        response.extend_from_slice(&chunk[..count]);
    }
    serde_json::from_slice(&response).map_err(io::Error::other)
}

fn read_request(arguments: &[String]) -> io::Result<BrokerRequest> {
    let [flag, path] = arguments else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected input",
        ));
    };
    if flag != "--input" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected input",
        ));
    }
    let mut input = String::new();
    let source: Box<dyn Read> = if path == "-" {
        Box::new(io::stdin())
    } else {
        Box::new(fs::File::open(path)?)
    };
    source
        .take(MAXIMUM_FRAME_BYTES as u64 + 1)
        .read_to_string(&mut input)?;
    if input.len() > MAXIMUM_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "input limit"));
    }
    let request: BrokerRequest = serde_json::from_str(&input).map_err(io::Error::other)?;
    if !canonical_nonce(request.broker_epoch()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid epoch"));
    }
    request
        .validate(request.broker_epoch())
        .map_err(io::Error::other)?;
    Ok(request)
}

/// 单次 CLI 从 stdin 或指定 JSON 文件读取请求，完整响应一到就结束进程。
pub fn run_socket_client(arguments: Vec<String>) -> i32 {
    let request = match read_request(&arguments) {
        Ok(request) => request,
        Err(_) => {
            let _ = write_json_line(
                &mut io::stdout().lock(),
                &terminal_error("BROKER_PROTOCOL_FAILED"),
            );
            return 2;
        }
    };
    let result = runtime_directory(false)
        .map_err(|_| ClientFailure { dispatched: false })
        .and_then(|directory| request_response(&directory, &request));
    match result {
        Ok(response) => {
            let completed = response["completed"] == true;
            if write_json_line(&mut io::stdout().lock(), &response).is_err() {
                return 2;
            }
            if completed { 0 } else { 1 }
        }
        Err(failure) => {
            let _ = write_json_line(&mut io::stdout().lock(), &failure.response(&request));
            2
        }
    }
}

#[cfg(test)]
#[path = "linux_desktop_session_transport_tests.rs"]
mod tests;
