//! 固定 named-pipe server instance 与 browser 首实例所有权 Component。

// 导入结构尺寸、共享 authority 与短启动预算。
use std::{
    // 读取 Windows 安全属性结构尺寸。
    mem::size_of,
    // 以互斥 live 标记把 secondary 创建绑定到首实例 owner。
    sync::{Arc, Mutex, Weak},
    // 使用单调预算建立不可对外服务的首实例 guard 连接。
    time::{Duration, Instant},
};

// 导入 Windows server pipe、安全描述符与文件模式。
use windows::{
    // 只在当前 Component 内使用原生类型。
    Win32::{
        // 导入 LocalAlloc 描述符释放类型。
        Foundation::{HLOCAL, LocalFree},
        // 导入 SDDL 转换与安全属性。
        Security::{
            // 导入 canonical SDDL 解析入口。
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            // 导入安全描述符与不可继承属性结构。
            PSECURITY_DESCRIPTOR,
            SECURITY_ATTRIBUTES,
        },
        // 导入 server pipe access 与首实例标志。
        Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX},
        // 导入固定 message/local-only server instance 创建入口。
        System::Pipes::{
            CreateNamedPipeW, NAMED_PIPE_MODE, PIPE_NOWAIT, PIPE_READMODE_MESSAGE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE,
        },
    },
    // 导入宽字符串指针。
    core::PCWSTR,
};

// 导入统一结果边界。
use crate::domain::AppResult;
// 从共享 Component 取得 canonical 当前用户 SID 文本。
use crate::components::current_user_sid_windows::current_user_sid_string;
// 导入父 pipe Component 的封闭类型与错误工厂。
use super::{
    // 复用固定双向连接类型保存内部 guard。
    ConnectedPipe,
    // 复用封闭 endpoint 类别。
    FixedLocalEndpointKind,
    // 接管 server handle 的唯一 RAII 所有者。
    OwnedPipeHandle,
    // 创建真正承载外部连接的 secondary listener。
    ServerPipe,
    // 生成不泄漏原生事实的稳定错误。
    endpoint_error,
};

// 固定 browser-session 同名 pipe 的有界 instance 总数。
const BROWSER_SESSION_MAXIMUM_PIPE_INSTANCES: u32 = 16;
// 固定 server instance 的平台默认连接等待时间。
const SERVER_CONNECT_TIMEOUT_MS: u32 = 5_000;
// 限制首实例内部 guard 的本机自连接时间。
const FIRST_INSTANCE_GUARD_TIMEOUT: Duration = Duration::from_secs(1);

// 让 LocalAlloc 安全描述符在创建 pipe 后确定性释放。
struct OwnedSecurityDescriptor(PSECURITY_DESCRIPTOR);

// 在作用域结束时释放安全描述符。
impl Drop for OwnedSecurityDescriptor {
    // 调用 LocalFree 配对释放。
    fn drop(&mut self) {
        // 忽略释放返回值，DACL 已复制到 pipe 对象。
        let _ = unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

// 构造仅 SYSTEM 与当前用户拥有完全访问的显式 pipe DACL。
fn pipe_security_descriptor() -> AppResult<OwnedSecurityDescriptor> {
    // 取得当前用户 canonical SID 文本。
    let current_user = current_user_sid_string().map_err(|_| {
        // 不公开 token 或 SID 失败细节。
        endpoint_error("The interactive endpoint principal could not be encoded.")
    })?;
    // 禁止 SID 文本携带 SDDL 结构字符。
    if current_user.is_empty()
        // SID 不应包含括号或分号。
        || current_user.contains(['(', ')', ';'])
        // SID 必须以 canonical 前缀开始。
        || !current_user.starts_with("S-1-")
    {
        // 返回不泄漏文本的 endpoint 错误。
        return Err(endpoint_error(
            // 固定错误不回显 SID。
            "The interactive endpoint principal could not be restricted.",
        ));
    }
    // 保护 DACL，且只授权 LocalSystem 与当前用户。
    let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;{current_user})");
    // 编码为 NUL 结尾 UTF-16。
    let sddl = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    // 初始化 LocalAlloc 安全描述符指针。
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // 让 Windows 解析 canonical SDDL。
    unsafe {
        // 调用固定版本的 SDDL 转换。
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            // 传入固定结构与私有当前用户 SID。
            PCWSTR(sddl.as_ptr()),
            // 使用固定 SDDL v1。
            SDDL_REVISION_1,
            // 接收 LocalAlloc 描述符。
            &mut descriptor,
            // 不公开描述符长度。
            None,
        )
    }
    // 解析失败保持 endpoint 不可用。
    .map_err(|_| endpoint_error("The interactive endpoint DACL could not be installed."))?;
    // 空描述符不能创建 pipe。
    if descriptor.0.is_null() {
        // 返回封闭 endpoint 错误。
        return Err(endpoint_error(
            // 不公开平台分配状态。
            "The interactive endpoint DACL is unavailable.",
        ));
    }
    // 返回 LocalAlloc 唯一所有者。
    Ok(OwnedSecurityDescriptor(descriptor))
}

// 创建同源 DACL、模式、预算与有界 instance 数量的 server handle。
pub(super) fn create_server_handle(
    // 借用已经校验的 NUL 结尾固定名称。
    name: &[u16],
    // 接收封闭 endpoint 类别。
    kind: FixedLocalEndpointKind,
    // 接收 client 到 server 的输入预算。
    request_frame_bytes: usize,
    // 接收 server 到 client 的输出预算。
    response_frame_bytes: usize,
    // 只允许组合根为真正首实例设置内核门禁。
    first_instance: bool,
) -> AppResult<OwnedPipeHandle> {
    // 构造显式 DACL。
    let descriptor = pipe_security_descriptor()?;
    // 转换安全属性结构长度。
    let length = u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(|_| {
        // 不公开平台尺寸。
        endpoint_error("The interactive endpoint security boundary overflowed.")
    })?;
    // 构造不可继承安全属性。
    let attributes = SECURITY_ATTRIBUTES {
        // 设置结构长度。
        nLength: length,
        // 借用 LocalAlloc 安全描述符。
        lpSecurityDescriptor: descriptor.0.0,
        // 禁止子进程继承 broker endpoint。
        bInheritHandle: false.into(),
    };
    // 只为真正首实例组合内核抢占门禁。
    let first_instance_flag = if first_instance {
        // 首实例必须证明同名 endpoint 尚不存在。
        FILE_FLAG_FIRST_PIPE_INSTANCE.0
    } else {
        // secondary 必须加入已由 owner 占有的同名 pipe。
        0
    };
    // 组合 duplex 与封闭首实例标志。
    let open_mode = windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(
        // 固定双向消息传输。
        PIPE_ACCESS_DUPLEX.0 | first_instance_flag,
    );
    // 组合 message/read-message/local-only 模式。
    let pipe_mode = NAMED_PIPE_MODE(
        // 每次 WriteFile 对应一条完整消息。
        PIPE_TYPE_MESSAGE.0
            // client 与 server 都以消息读取。
            | PIPE_READMODE_MESSAGE.0
            // accept 阶段使用非阻塞轮询以响应进程取消。
            | PIPE_NOWAIT.0
            // 内核拒绝远程 SMB 客户端。
            | PIPE_REJECT_REMOTE_CLIENTS.0,
    );
    // browser 使用有界多实例，其余 endpoint 保持单实例语义。
    let maximum_instances = match kind {
        // browser 必须同时承载在途请求、独立取消与下一 listener。
        FixedLocalEndpointKind::BrowserSession => BROWSER_SESSION_MAXIMUM_PIPE_INSTANCES,
        // 独立交互会话保持既有单连接。
        FixedLocalEndpointKind::InteractiveSession => 1,
        // 长操作保持既有单连接。
        FixedLocalEndpointKind::LongOperation => 1,
    };
    // 创建固定 server instance。
    let handle = unsafe {
        // 调用 Windows named-pipe 创建入口。
        CreateNamedPipeW(
            // 传入 NUL 结尾私有名称。
            PCWSTR(name.as_ptr()),
            // 使用固定双向模式与封闭首实例标志。
            open_mode,
            // 使用本地消息模式。
            pipe_mode,
            // 使用按 endpoint 冻结的有界 instance 数量。
            maximum_instances,
            // 固定输出缓冲区边界。
            u32::try_from(response_frame_bytes).unwrap_or(u32::MAX),
            // 固定输入缓冲区边界。
            u32::try_from(request_frame_bytes).unwrap_or(u32::MAX),
            // 使用固定默认等待时间。
            SERVER_CONNECT_TIMEOUT_MS,
            // 安装显式 DACL。
            Some(&raw const attributes),
        )
    };
    // descriptor 在 CreateNamedPipe 返回后可以释放并接管 server handle。
    OwnedPipeHandle::new(handle)
}

// 保存首实例 owner 的 live session 与关闭线性化门禁。
pub(super) struct BrowserSessionFirstAuthority {
    // 保存只用于固定 pipe 名称的 native session。
    pub(super) session_id: u32,
    // 让 secondary 创建与 owner 关闭互斥。
    pub(super) live: Mutex<bool>,
}

// 返回普通 endpoint 或 live browser secondary 是否仍可服务。
pub(super) fn browser_authority_live(
    // 借用可选 browser owner lease。
    authority: Option<&Arc<BrowserSessionFirstAuthority>>,
) -> bool {
    // 普通 endpoint 不受 browser owner lease 约束。
    authority.is_none_or(|authority| {
        // 锁污染与 owner 关闭都必须失败闭合。
        authority.live.lock().is_ok_and(|live| *live)
    })
}

// 保证 browser secondary 只在首实例 owner live 时执行协议 I/O。
pub(super) fn require_browser_authority_live(
    // 借用可选 browser owner lease。
    authority: Option<&Arc<BrowserSessionFirstAuthority>>,
) -> AppResult<()> {
    // 普通 endpoint 或 live secondary 可以继续当前窄操作。
    if browser_authority_live(authority) {
        // 返回成功而不泄漏 authority。
        return Ok(());
    }
    // 锁污染或 owner 已关闭都返回固定不可用。
    Err(endpoint_error(
        // 不回显原生 endpoint 状态。
        "The browser session endpoint owner is unavailable.",
    ))
}

// 表示不可交给外部连接 worker 的 browser 首实例 guard。
pub(crate) struct BrowserSessionPipeOwner {
    // 保存禁止外部业务进入的首实例 server 端。
    first_server: Option<ConnectedPipe>,
    // 保存内部自连接 client 端，使首实例永久不可接入外部 peer。
    guard_client: Option<ConnectedPipe>,
    // 保存 factory 只能弱引用的 live authority。
    authority: Arc<BrowserSessionFirstAuthority>,
}

// 为 browser 首实例 owner 提供固定创建与 secondary factory。
impl BrowserSessionPipeOwner {
    // 创建并立即内部占用真正首实例。
    pub(crate) fn create(session_id: u32) -> AppResult<Self> {
        // 创建带 first-instance 门禁的固定 browser listener。
        let first = ServerPipe::create_for(FixedLocalEndpointKind::BrowserSession, session_id)?;
        // 冻结一次内部 guard 建立的绝对单调 deadline。
        let deadline = Instant::now() + FIRST_INSTANCE_GUARD_TIMEOUT;
        // 只接受确实连回当前进程首实例的内部 client。
        let guard_client = loop {
            // 在剩余预算内用本进程 client 尝试占用可用 instance。
            let candidate = ConnectedPipe::connect_for_until(
                // 固定连接 browser-session endpoint。
                FixedLocalEndpointKind::BrowserSession,
                // 使用相同 native session 名称后缀。
                session_id,
                // 复用同一启动 deadline，重试不得延长预算。
                deadline,
                // 启动 guard 不接受外部取消输入。
                || false,
            )?;
            // 从内核取得 candidate 的 server PID。
            if candidate.peer_process_id()? == std::process::id() {
                // 只有当前 broker 自身创建的 instance 可以成为内部 guard。
                break candidate;
            }
            // foreign same-principal secondary 只能消耗本次候选并造成有界拒绝服务。
            drop(candidate);
        };
        // server 端确认内部 client 已连接。
        let first_server = first.accept(|| false)?;
        // 创建只由 owner 强持有的 live authority。
        let authority = Arc::new(BrowserSessionFirstAuthority {
            // 冻结固定 session。
            session_id,
            // owner 建立时允许创建 secondary。
            live: Mutex::new(true),
        });
        // 返回不公开首实例 handle 的唯一 owner。
        Ok(Self {
            // 保存内部 server guard。
            first_server: Some(first_server),
            // 保存内部 client guard。
            guard_client: Some(guard_client),
            // 保存 live authority。
            authority,
        })
    }

    // 创建只能在当前 owner live 时派生 secondary 的弱 factory。
    pub(crate) fn secondary_factory(&self) -> BrowserSessionSecondaryFactory {
        // factory 不得延长首实例生命周期。
        BrowserSessionSecondaryFactory {
            // 仅保存不可伪造的弱 authority。
            authority: Arc::downgrade(&self.authority),
        }
    }
}

// owner 关闭时先封住 secondary 创建，再让首实例 server 最后释放。
impl Drop for BrowserSessionPipeOwner {
    // 执行固定关闭次序。
    fn drop(&mut self) {
        // 标记 owner 不再允许创建 secondary。
        if let Ok(mut live) = self.authority.live.lock() {
            // 在关闭任何 guard handle 前封住 factory。
            *live = false;
        }
        // 先关闭内部 client，保留 server 首实例事实。
        drop(self.guard_client.take());
        // 最后关闭真正 first-instance server handle。
        drop(self.first_server.take());
    }
}

// 表示不延长首实例生命周期的 browser secondary factory。
#[derive(Clone)]
pub(crate) struct BrowserSessionSecondaryFactory {
    // 只保存 live owner 的弱 authority。
    authority: Weak<BrowserSessionFirstAuthority>,
}

// 为 secondary factory 提供固定且不可注入的 listener 创建。
impl BrowserSessionSecondaryFactory {
    // 创建一个使用同名、同 DACL、同预算且无 first flag 的 listener。
    pub(crate) fn create_listener(&self) -> AppResult<ServerPipe> {
        // owner 已释放时不得重建无保护 endpoint。
        let authority = self.authority.upgrade().ok_or_else(|| {
            // 返回不泄漏 pipe/session 的固定错误。
            endpoint_error("The browser session endpoint owner is unavailable.")
        })?;
        // 与 owner Drop 线性化 secondary 创建。
        let live = authority.live.lock().map_err(|_| {
            // 锁污染意味着本代 owner 不可信。
            endpoint_error("The browser session endpoint owner is unavailable.")
        })?;
        // owner 关闭后即使弱引用暂时升级也必须失败。
        if !*live {
            // 返回固定不可用。
            return Err(endpoint_error(
                // 不泄漏关闭竞态。
                "The browser session endpoint owner is unavailable.",
            ));
        }
        // 只从冻结 session 创建固定 browser secondary。
        ServerPipe::create_secondary_for(
            // 只使用 authority 冻结的 native session。
            authority.session_id,
            // 让 listener/connection 保存 fail-closed lease 而不拥有首实例 handle。
            Arc::clone(&authority),
        )
    }
}
