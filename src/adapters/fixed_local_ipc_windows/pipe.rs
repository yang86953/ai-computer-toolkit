//! 固定本机 broker 共用的 Windows local-only named-pipe Component。

// 将固定 endpoint 身份与帧预算隔离到窄 Component，避免 pipe 生命周期文件越界。
#[path = "pipe_endpoint.rs"]
mod endpoint;
// 向同一 adapter 内调用方重新导出冻结 endpoint 类型与预算常量。
pub(crate) use endpoint::{
    // 导出封闭 endpoint 类别。
    FixedLocalEndpointKind,
    // 保持既有交互 broker 帧预算入口。
    MAXIMUM_FRAME_BYTES,
};
// 将 owner-preserving listener outcomes 隔离为窄生命周期类型。
#[path = "pipe_lifecycle.rs"]
mod lifecycle;
// 向 browser broker 组合根导出封闭生命周期结果。
pub(crate) use lifecycle::{PersistentServerAccept, PersistentServerRelisten};
// 将 server instance 创建与 browser 首实例 authority 隔离到窄 Component。
#[path = "pipe_instance.rs"]
mod instance;
// 向 browser broker 宿主导出固定首实例 owner 与它派生的 secondary factory。
pub(crate) use instance::{
    // 导出进程代际唯一的首实例 owner。
    BrowserSessionPipeOwner,
    // 导出不延长 owner 生命周期的弱 factory。
    BrowserSessionSecondaryFactory,
};
// 仅在当前 pipe 生命周期中复用同源 server handle 创建。
use instance::{
    // 保存 browser secondary 的 owner lease 类型。
    BrowserSessionFirstAuthority,
    // 读取 owner 关闭事实。
    browser_authority_live,
    // 创建同源 server instance。
    create_server_handle,
    // 在协议 I/O 前强制 owner live。
    require_browser_authority_live,
};
// 将单帧 JSON/text 写入隔离到窄 Component。
#[path = "pipe_write.rs"]
mod write;
// 向 browser session adapter 导出单次非阻塞写入的封闭投递结果。
pub(crate) use write::BoundedTextWriteAttempt;
// 将有界 message 读取与 wait-mode 分流隔离到窄 Component。
#[path = "pipe_read.rs"]
mod read;
// 将 client 连接与创建期写入模式安装隔离到窄 Component。
#[path = "pipe_connect.rs"]
mod connect;

// 导入结构尺寸、轮询线程与单调 deadline。
use std::{
    // 保存 browser secondary 与首实例关闭事实的共享 lease。
    sync::Arc,
    // 在无消息时执行短间隔等待。
    thread,
    // 使用单调时钟约束连接与读取生命周期。
    time::{Duration, Instant},
};

// 导入 Windows pipe、安全描述符与同步文件 API。
use windows::{
    // 只在当前 Component 内使用原生类型。
    Win32::{
        // 导入 handle、访问掩码和稳定 Win32 错误。
        Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, HANDLE},
        // 导入 named-pipe 生命周期与 peer PID 查询。
        System::Pipes::{
            ConnectNamedPipe, DisconnectNamedPipe, GetNamedPipeClientProcessId,
            GetNamedPipeServerProcessId, NAMED_PIPE_MODE, PIPE_NOWAIT, PIPE_READMODE_MESSAGE,
            PIPE_WAIT, SetNamedPipeHandleState,
        },
    },
};

// 导入统一错误类型与当前用户 SID 来源。
use crate::domain::{AppControlError, AppResult};

// 使用 8 KiB 块读取 message-mode frame。
const READ_CHUNK_BYTES: usize = 8 * 1024;
// 使用短轮询片约束取消和单调 deadline 的观察延迟。
const WAIT_SLICE_MS: u32 = 10;

// 构造不泄漏 pipe、PID 或 session 的稳定 endpoint 错误。
fn endpoint_error(message: &'static str) -> AppControlError {
    // 使用统一 endpoint 不可用错误码。
    AppControlError::new("ISOLATED_WORKER_UNAVAILABLE", message)
}

// 构造 deadline 到达且未读取任何新消息的稳定错误。
fn timeout_error() -> AppControlError {
    // 使用公共 timeout 码且不公开 pipe 状态。
    AppControlError::new(
        // 保持跨 worker 的统一错误码。
        "TIMEOUT",
        // 明确本机 endpoint 没有在预算内响应。
        "The interactive endpoint did not respond before the request deadline.",
    )
}

// 构造调用方取消等待的稳定错误。
fn cancelled_error() -> AppControlError {
    // 取消发生时 ConnectedPipe 的 RAII drop 会关闭当前连接。
    AppControlError::new(
        // 使用统一取消错误码。
        "CANCELLED",
        // 不公开当前协议阶段。
        "The interactive endpoint request was cancelled.",
    )
}

// 返回不越过单调 deadline 的下一轮毫秒等待片。
fn wait_slice_ms(deadline: Instant) -> AppResult<u32> {
    // 计算当前剩余单调预算。
    let remaining = deadline
        // deadline 已到时没有可安全继续的等待。
        .checked_duration_since(Instant::now())
        // 映射为统一 timeout。
        .ok_or_else(timeout_error)?;
    // 零预算不能进入平台等待。
    if remaining.is_zero() {
        // 返回稳定 timeout。
        return Err(timeout_error());
    }
    // 将不足一毫秒的尾部预算向上取整为一次最短轮询。
    let remaining_ms = remaining.as_millis().max(1);
    // 限制取消观察延迟为固定短片。
    let slice = remaining_ms.min(u128::from(WAIT_SLICE_MS));
    // 固定上限保证转换成功，理论漂移仍失败闭合。
    u32::try_from(slice).map_err(|_| endpoint_error("The endpoint wait boundary overflowed."))
}

// 让 named-pipe handle 在全部返回路径确定性关闭。
struct OwnedPipeHandle {
    // 保存不跨出当前 Component 的原生 handle。
    handle: HANDLE,
}

// 为 pipe handle 提供安全构造与借用。
impl OwnedPipeHandle {
    // 接管 Windows API 返回的 pipe handle。
    fn new(handle: HANDLE) -> AppResult<Self> {
        // 拒绝空值与 INVALID_HANDLE_VALUE。
        if handle.is_invalid() {
            // 不公开 handle 或 pipe 名称。
            return Err(endpoint_error(
                "The fixed interactive endpoint could not be opened.",
            ));
        }
        // 返回唯一所有者。
        Ok(Self { handle })
    }

    // 返回仅供当前 Component 调用使用的复制 handle。
    const fn raw(&self) -> HANDLE {
        // 原生值不跨出文件。
        self.handle
    }
}

// 在作用域结束时关闭本端 handle 且保留对端尚未读取的缓冲数据。
impl Drop for OwnedPipeHandle {
    // 执行平台配对回收。
    fn drop(&mut self) {
        // 只关闭本端；复用 server handle 的 relisten 路径会显式执行 DisconnectNamedPipe。
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

// 构造固定 pipe 名称的 NUL 结尾 UTF-16。
fn pipe_name(kind: FixedLocalEndpointKind, session_id: u32) -> AppResult<Vec<u16>> {
    // Session 0 不得拥有交互 broker。
    if session_id == 0 {
        // 返回结构化不可用。
        return Err(endpoint_error(
            "The interactive endpoint session is unavailable.",
        ));
    }
    // 将 native session 只用于私有本机 endpoint 路由。
    let name = format!("{}{session_id}", kind.pipe_name_prefix());
    // 编码为 Windows NUL 结尾 UTF-16。
    Ok(name.encode_utf16().chain(Some(0)).collect())
}

// 表示尚未接收客户端的固定 server pipe。
pub(crate) struct ServerPipe {
    // 保存拥有的原生 pipe handle。
    handle: OwnedPipeHandle,
    // 保存 client 到 server 的读取预算。
    request_frame_bytes: usize,
    // 保存 server 到 client 的写入预算。
    response_frame_bytes: usize,
    // 保存连接期是否固定使用非阻塞有界写入模式。
    bounded_write_nonblocking: bool,
    // browser secondary 保存关闭后失败闭合的 owner lease。
    browser_authority: Option<Arc<BrowserSessionFirstAuthority>>,
}

// 为固定 server pipe 提供创建、连接与转换。
impl ServerPipe {
    // 在精确 native session 上创建唯一固定 pipe instance。
    pub(crate) fn create(session_id: u32) -> AppResult<Self> {
        // 保持既有独立交互会话入口语义。
        Self::create_for(FixedLocalEndpointKind::InteractiveSession, session_id)
    }

    // 在精确 native session 上创建指定固定类别的唯一 pipe instance。
    pub(crate) fn create_for(
        // 接收封闭 endpoint 类别而非调用方名称。
        kind: FixedLocalEndpointKind,
        // 接收私有 native session。
        session_id: u32,
    ) -> AppResult<Self> {
        // 构造私有 endpoint 名称。
        let name = pipe_name(kind, session_id)?;
        // 取得双向冻结预算。
        let (request_frame_bytes, response_frame_bytes) = kind.frame_budgets();
        // 使用 endpoint 固定双向预算创建真正首实例。
        Self::create_named_instance(
            // 传入固定名称。
            &name,
            // 传入封闭 endpoint 类别。
            kind,
            // 传入请求预算。
            request_frame_bytes,
            // 传入响应预算。
            response_frame_bytes,
            // 生产组合根只创建真正首实例。
            true,
            // browser 连接期固定使用非阻塞有界写入。
            matches!(kind, FixedLocalEndpointKind::BrowserSession),
        )
    }

    // 使用已经校验来源的 NUL 结尾名称创建唯一 pipe instance。
    #[cfg(test)]
    fn create_named(name: &[u16]) -> AppResult<Self> {
        // 测试命名入口保持独立交互会话预算。
        Self::create_named_with_budgets(name, MAXIMUM_FRAME_BYTES, MAXIMUM_FRAME_BYTES)
    }

    // 使用已经校验来源的名称与固定双向预算创建唯一 pipe instance。
    #[cfg(test)]
    fn create_named_with_budgets(
        // 借用固定 NUL 结尾名称。
        name: &[u16],
        // 接收 client 到 server 的输入预算。
        request_frame_bytes: usize,
        // 接收 server 到 client 的输出预算。
        response_frame_bytes: usize,
    ) -> AppResult<Self> {
        // 测试命名入口保持独立交互会话单实例语义。
        Self::create_named_instance(
            // 传入受控测试名称。
            name,
            // 测试命名入口不扩大 browser 多实例边界。
            FixedLocalEndpointKind::InteractiveSession,
            // 传入请求预算。
            request_frame_bytes,
            // 传入响应预算。
            response_frame_bytes,
            // 仍要求唯一首实例。
            true,
            // 既有测试入口保持阻塞连接期模式。
            false,
        )
    }

    // 只为 live browser 首实例 factory 创建固定 secondary listener。
    fn create_secondary_for(
        // 接收 authority 冻结的 native session。
        session_id: u32,
        // 保存与首实例 owner 关闭线性化的强 lease。
        browser_authority: Arc<BrowserSessionFirstAuthority>,
    ) -> AppResult<Self> {
        // 构造固定 browser endpoint 名称。
        let name = pipe_name(FixedLocalEndpointKind::BrowserSession, session_id)?;
        // 取得 browser 冻结双向预算。
        let (request_frame_bytes, response_frame_bytes) =
            FixedLocalEndpointKind::BrowserSession.frame_budgets();
        // 创建不携带 first-instance 标志的同名 instance。
        let mut listener = Self::create_named_instance(
            // 传入固定名称。
            &name,
            // 固定为 browser-session 类别。
            FixedLocalEndpointKind::BrowserSession,
            // 传入请求预算。
            request_frame_bytes,
            // 传入响应预算。
            response_frame_bytes,
            // secondary 不得伪装首实例。
            false,
            // browser secondary 与首实例使用同一非阻塞写入事实。
            true,
        )?;
        // secondary 创建成功后安装不可由调用方伪造的 owner lease。
        listener.browser_authority = Some(browser_authority);
        // 返回关闭后会失败闭合的 listener。
        Ok(listener)
    }

    // 使用已经校验的名称、类别与预算创建一个 server instance。
    fn create_named_instance(
        // 借用固定 NUL 结尾名称。
        name: &[u16],
        // 接收封闭 endpoint 类别。
        kind: FixedLocalEndpointKind,
        // 接收 client 到 server 的输入预算。
        request_frame_bytes: usize,
        // 接收 server 到 client 的输出预算。
        response_frame_bytes: usize,
        // 区分真正首实例与受 owner 约束的 secondary。
        first_instance: bool,
        // 声明连接期是否固定保持 PIPE_NOWAIT。
        bounded_write_nonblocking: bool,
    ) -> AppResult<Self> {
        // 拒绝空名称或缺少 Windows 终止符的测试输入。
        if name.len() < 2 || name.last().copied() != Some(0) {
            // 保持名称细节不进入公共错误。
            return Err(endpoint_error(
                "The fixed interactive endpoint name was invalid.",
            ));
        }
        // 由同源 instance Component 安装 DACL、模式、预算与有界数量。
        let handle = create_server_handle(
            // 传入固定名称。
            name,
            // 传入封闭类别。
            kind,
            // 传入请求预算。
            request_frame_bytes,
            // 传入响应预算。
            response_frame_bytes,
            // 传入首实例事实。
            first_instance,
        )?;
        // 返回尚未连接的 server。
        Ok(Self {
            // 保存拥有的 handle。
            handle,
            // 保存请求预算。
            request_frame_bytes,
            // 保存响应预算。
            response_frame_bytes,
            // 保存只能在创建期确定的非阻塞写入事实。
            bounded_write_nonblocking,
            // 普通首实例与测试 listener 不持有 browser secondary lease。
            browser_authority: None,
        })
    }

    // 阻塞等待一个本机客户端并转换为已连接 pipe。
    pub(crate) fn accept(self, cancelled: impl Fn() -> bool) -> AppResult<ConnectedPipe> {
        // 保持既有调用方在取消时释放 listener 的行为。
        match self.accept_persistent(cancelled) {
            // 返回已连接 pipe。
            PersistentServerAccept::Connected(pipe) => Ok(pipe),
            // 既有入口把取消映射为稳定错误并释放 listener。
            PersistentServerAccept::Cancelled(_) => Err(cancelled_error()),
            // 既有入口仍返回原错误并按普通临时值顺序释放 owner。
            PersistentServerAccept::Failed(_, error) => Err(error),
        }
    }

    // 等待客户端，同时在取消时把同一首实例 listener 交还所有者。
    pub(crate) fn accept_persistent(
        // 转移 listener 以便成功后把同一 handle 交给连接。
        self,
        // 接收进程关闭观察。
        cancelled: impl Fn() -> bool,
    ) -> PersistentServerAccept {
        // 在非阻塞 pipe 上轮询直到连接或收到取消。
        loop {
            // owner 已进入关闭线性化点时停止接收新 peer。
            if !browser_authority_live(self.browser_authority.as_ref()) {
                // 返还 listener 供组合根按固定次序关闭。
                return PersistentServerAccept::Cancelled(self);
            }
            // 长期 broker 必须能在没有客户端时响应 Ctrl+C。
            if cancelled() {
                // 保留 listener，供所有者先关闭 System 再释放 endpoint。
                return PersistentServerAccept::Cancelled(self);
            }
            // 尝试建立当前单实例连接。
            match unsafe { ConnectNamedPipe(self.handle.raw(), None) } {
                // 非阻塞调用观察到当前客户端。
                Ok(()) => break,
                // 客户端在调用前已连接也属于成功。
                Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => break,
                // 当前仍在监听时继续短轮询。
                Err(error) if error.code() == ERROR_PIPE_LISTENING.to_hresult() => {
                    // 限制空闲 broker 的 CPU 占用。
                    thread::sleep(Duration::from_millis(u64::from(WAIT_SLICE_MS)));
                }
                // 其他状态保持结构化不可用。
                Err(_) => {
                    // 返回不泄漏 endpoint 的错误。
                    return PersistentServerAccept::Failed(
                        // 保留首实例 owner。
                        self,
                        // 返回安全 endpoint 错误。
                        endpoint_error("The interactive endpoint could not accept a local peer."),
                    );
                }
            }
        }
        // 连接建立后再次核对 owner，关闭竞态不得进入认证或业务阶段。
        if !browser_authority_live(self.browser_authority.as_ref()) {
            // 返还已连接 owner；调用方 drop 会断开未经认证的 peer。
            return PersistentServerAccept::Cancelled(self);
        }
        // browser 固定保留非阻塞写入模式，其余 endpoint 保持既有阻塞模式。
        let wait_mode = if self.bounded_write_nonblocking {
            // browser command 需要在同一连接上有界轮询写入。
            PIPE_NOWAIT
        } else {
            // 其余 endpoint 保持既有阻塞 write 语义。
            PIPE_WAIT
        };
        // 安装已在创建期确定的消息读写等待模式。
        let mode = NAMED_PIPE_MODE(PIPE_READMODE_MESSAGE.0 | wait_mode.0);
        // 尝试安装连接期模式。
        if unsafe { SetNamedPipeHandleState(self.handle.raw(), Some(&raw const mode), None, None) }
            .is_err()
        {
            // 模式漂移时仍把首实例 owner 交还调用方。
            return PersistentServerAccept::Failed(
                // 保留首实例 owner。
                self,
                // 返回安全 endpoint 错误。
                endpoint_error("The interactive endpoint wait mode is unavailable."),
            );
        }
        // 转移 handle 所有权并标记 server 角色。
        PersistentServerAccept::Connected(ConnectedPipe {
            // 保存已连接 pipe。
            handle: self.handle,
            // server 必须查询 client PID。
            role: PipeRole::Server,
            // server 按 request 边界读取。
            maximum_read_frame_bytes: self.request_frame_bytes,
            // server 按 response 边界写入。
            maximum_write_frame_bytes: self.response_frame_bytes,
            // 连接继承创建期确定的有界非阻塞写入事实。
            bounded_write_nonblocking: self.bounded_write_nonblocking,
            // 把 browser owner lease 随连接一起移动。
            browser_authority: self.browser_authority,
        })
    }
}

// 表示已连接 pipe 的 peer 查询角色。
#[derive(Clone, Copy)]
enum PipeRole {
    // server 查询 client PID。
    Server,
    // client 查询 server PID。
    Client,
}

// 表示已连接、可收发单条有界 JSON 消息的 pipe。
pub(crate) struct ConnectedPipe {
    // 保存拥有的 pipe handle。
    handle: OwnedPipeHandle,
    // 保存 peer PID 查询方向。
    role: PipeRole,
    // 保存当前方向的读取帧预算。
    maximum_read_frame_bytes: usize,
    // 保存当前方向的写入帧预算。
    maximum_write_frame_bytes: usize,
    // 表示该 browser 连接已在建立期固定安装 PIPE_NOWAIT I/O 模式。
    bounded_write_nonblocking: bool,
    // browser secondary 保存关闭后失败闭合的 owner lease。
    browser_authority: Option<Arc<BrowserSessionFirstAuthority>>,
}

// 为已连接 pipe 提供 bounded message 与 peer 认证事实。
impl ConnectedPipe {
    // 断开 server 连接并把同一 first-instance handle 恢复为 listener。
    // #2059 后续迁移仍保留 owner-preserving 边界，当前生产 dispatcher 使用 secondary factory。
    #[allow(dead_code)]
    pub(crate) fn relisten(self) -> PersistentServerRelisten {
        // 只有 server 角色拥有可复用的 first-instance handle。
        if !matches!(self.role, PipeRole::Server) {
            // client 不得取得 listener 所有权但 owner 仍必须返回。
            return PersistentServerRelisten::Failed(
                // 保留 client owner。
                self,
                // 返回稳定角色错误。
                endpoint_error("The fixed endpoint client cannot become a listener."),
            );
        }
        // owner 关闭后 secondary 不得恢复为可接收新 peer 的 listener。
        if !browser_authority_live(self.browser_authority.as_ref()) {
            // 返还原连接 owner，供组合根按关闭次序释放。
            return PersistentServerRelisten::Failed(
                // 保留当前 handle 与 lease。
                self,
                // 不泄漏关闭竞态。
                endpoint_error("The browser session endpoint owner is unavailable."),
            );
        }
        // 当前连接可能已由 client 关闭；两种状态都允许重听。
        let _ = unsafe { DisconnectNamedPipe(self.handle.raw()) };
        // 恢复 accept 使用的非阻塞消息模式。
        let mode = NAMED_PIPE_MODE(PIPE_READMODE_MESSAGE.0 | PIPE_NOWAIT.0);
        // 尝试安装 listener 模式。
        if unsafe { SetNamedPipeHandleState(self.handle.raw(), Some(&raw const mode), None, None) }
            .is_err()
        {
            // 失败时仍把 first-instance owner 交给组合根。
            return PersistentServerRelisten::Failed(
                // 保留当前 owner。
                self,
                // 返回安全恢复错误。
                endpoint_error("The fixed endpoint could not resume listening."),
            );
        }
        // 从 connected pipe 解构出唯一 handle 与双向预算。
        let Self {
            // 转移 first-instance handle。
            handle,
            // server 角色已经验证。
            role: _,
            // 保存下一连接的读取预算。
            maximum_read_frame_bytes,
            // 保存下一连接的写入预算。
            maximum_write_frame_bytes,
            // 保留下一连接的创建期写入等待模式。
            bounded_write_nonblocking,
            // 保留 browser owner lease。
            browser_authority,
        } = self;
        // 返回继续拥有同一内核 first-instance 的 listener。
        PersistentServerRelisten::Listening(ServerPipe {
            // 转移原 handle。
            handle,
            // server 读取原 request 预算。
            request_frame_bytes: maximum_read_frame_bytes,
            // server 写入原 response 预算。
            response_frame_bytes: maximum_write_frame_bytes,
            // relisten 后继续受同一 owner 关闭事实约束。
            browser_authority,
            // relisten 不得改变已创建 instance 的写入模式事实。
            bounded_write_nonblocking,
        })
    }

    // 查询内核记录的对等进程 PID。
    pub(crate) fn peer_process_id(&self) -> AppResult<u32> {
        // owner 关闭后 secondary 不得继续认证新业务。
        require_browser_authority_live(self.browser_authority.as_ref())?;
        // 初始化不会误报真实 PID 的值。
        let mut process_id = 0_u32;
        // 按 pipe 角色选择唯一对等方向。
        let result = match self.role {
            // server 查询 client。
            PipeRole::Server => unsafe {
                GetNamedPipeClientProcessId(self.handle.raw(), &mut process_id)
            },
            // client 查询 server。
            PipeRole::Client => unsafe {
                GetNamedPipeServerProcessId(self.handle.raw(), &mut process_id)
            },
        };
        // 查询失败保持对等端不可认证。
        result.map_err(|_| endpoint_error("The interactive endpoint peer is unavailable."))?;
        // PID 0 不能成为认证 peer。
        if process_id == 0 {
            // 返回封闭认证失败。
            return Err(endpoint_error("The interactive endpoint peer is invalid."));
        }
        // 返回只供身份 Adapter 使用的 native PID。
        Ok(process_id)
    }
}

// 将 Windows pipe 故障生命周期测试放在独立文件，避免生产 Component 超过行数上限。
#[cfg(test)]
#[path = "pipe_tests.rs"]
mod tests;
