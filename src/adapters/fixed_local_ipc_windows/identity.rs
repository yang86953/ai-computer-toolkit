//! 认证独立交互会话 endpoint 使用的 Windows 进程、主体与 session 私有事实。

// 导入路径、UTF-16 与对齐缓冲区工具。
use std::{
    // 导入原生结构尺寸计算。
    mem::size_of,
    // 导入 Windows 路径比较类型。
    path::{Path, PathBuf},
};

// 导入 Windows 私有身份 API。
use windows::{
    // 只在当前 Adapter 内使用 Win32 类型。
    Win32::{
        // 导入 handle 与配对关闭函数。
        Foundation::{CloseHandle, HANDLE},
        // 导入 token 主体比较接口。
        Security::{
            EqualSid,
            GetSidSubAuthority,
            GetSidSubAuthorityCount,
            GetTokenInformation,
            TOKEN_MANDATORY_LABEL,
            TOKEN_QUERY,
            TOKEN_USER,
            TokenIntegrityLevel,
            // 导入与已打开进程 token 绑定的 session 查询类别。
            TokenSessionId,
            TokenUser,
        },
        // 导入进程、session 与 ToolHelp 接口。
        System::{
            // 导入只读进程快照接口。
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            // 导入 WTS session 枚举与代际信息。
            RemoteDesktop::{
                ProcessIdToSessionId, WTS_CURRENT_SERVER_HANDLE, WTS_SESSION_INFOW, WTSActive,
                WTSEnumerateSessionsW, WTSFreeMemory, WTSINFOW, WTSQuerySessionInformationW,
                WTSSessionInfo,
            },
            // 导入受限进程、token 与完整镜像路径查询。
            Threading::{
                GetCurrentProcess, GetCurrentProcessId, GetProcessId, OpenProcess,
                OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
    },
    // 导入可写 UTF-16 指针。
    core::PWSTR,
};

// 导入统一错误类型。
use crate::domain::{AppControlError, AppResult};

// 限制完整 Windows 镜像路径缓冲区。
const MAXIMUM_IMAGE_PATH_UNITS: usize = 32_768;
// 限制 WTS session 清单，异常数量必须失败闭合。
const MAXIMUM_SESSION_COUNT: usize = 4_096;

// 构造不泄漏 native 身份的 endpoint 认证错误。
fn authentication_error(message: &'static str) -> AppControlError {
    // 使用统一 endpoint 认证失败码。
    AppControlError::new("ENDPOINT_AUTHENTICATION_FAILED", message)
}

// 让私有 Windows handle 在全部返回路径上确定性关闭。
struct OwnedHandle(HANDLE);

// 为有效 handle 建立唯一所有权。
impl OwnedHandle {
    // 接管 Windows API 返回的 handle。
    fn new(handle: HANDLE) -> AppResult<Self> {
        // 拒绝空值与 INVALID_HANDLE_VALUE。
        if handle.is_invalid() {
            // 不公开原生 handle 数值。
            return Err(authentication_error(
                "An endpoint peer handle could not be opened.",
            ));
        }
        // 返回唯一所有者。
        Ok(Self(handle))
    }

    // 返回仅供本 Adapter 调用使用的复制 handle 值。
    const fn raw(&self) -> HANDLE {
        // HANDLE 不跨出当前文件。
        self.0
    }
}

// 在作用域结束时关闭拥有的 handle。
impl Drop for OwnedHandle {
    // 执行配对回收。
    fn drop(&mut self) {
        // 认证结论不依赖关闭错误。
        let _ = unsafe { CloseHandle(self.0) };
    }
}

// 使用 usize 存储保证 TOKEN_USER 结构的对齐。
struct TokenUserBuffer {
    // 保存 GetTokenInformation 写入的对齐存储。
    storage: Vec<usize>,
}

// 为 token 缓冲区提供只读 SID 投影。
impl TokenUserBuffer {
    // 返回只供 EqualSid 和 SDDL 转换使用的私有 SID 指针。
    fn sid(&self) -> windows::Win32::Security::PSID {
        // 存储非空且尺寸已在构造阶段验证。
        let token_user = unsafe {
            // 以对齐指针借用固定头部。
            &*self.storage.as_ptr().cast::<TOKEN_USER>()
        };
        // 返回 token 缓冲区生命周期内有效的 SID。
        token_user.User.Sid
    }
}

// 读取进程 token 的当前用户 SID。
fn token_user(process: HANDLE) -> AppResult<TokenUserBuffer> {
    // 初始化 token handle。
    let mut token = HANDLE::default();
    // 只申请查询权限。
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.map_err(|_| {
        // 隐藏访问失败的 native 细节。
        authentication_error("An endpoint peer token could not be queried.")
    })?;
    // 接管 token handle。
    let token = OwnedHandle::new(token)?;
    // 首次调用只查询所需字节数。
    let mut bytes_needed = 0_u32;
    // 预期缓冲区不足，不依赖该调用的错误文本。
    let _ = unsafe { GetTokenInformation(token.raw(), TokenUser, None, 0, &mut bytes_needed) };
    // 要求至少容纳 TOKEN_USER 且限制异常平台长度。
    if bytes_needed < u32::try_from(size_of::<TOKEN_USER>()).unwrap_or(u32::MAX)
        // 防止异常 token 分配过大内存。
        || bytes_needed > 64 * 1024
    {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "An endpoint peer token returned an invalid identity boundary.",
        ));
    }
    // 计算满足 usize 对齐的元素数量。
    let word_size = size_of::<usize>();
    // 向上取整 token 缓冲区长度。
    let words = usize::try_from(bytes_needed)
        // 转换平台长度。
        .ok()
        // 加上向上取整余量。
        .and_then(|bytes| bytes.checked_add(word_size.saturating_sub(1)))
        // 转换为 usize 元素数量。
        .map(|bytes| bytes / word_size)
        // 任一溢出都失败闭合。
        .ok_or_else(|| authentication_error("An endpoint peer token size overflowed."))?;
    // 分配清零且对齐的 token 存储。
    let mut storage = vec![0_usize; words];
    // 读取完整 TOKEN_USER 与尾随 SID。
    unsafe {
        GetTokenInformation(
            // 使用只读 token handle。
            token.raw(),
            // 只读取用户主体。
            TokenUser,
            // 传入对齐存储首地址。
            Some(storage.as_mut_ptr().cast()),
            // 传入平台报告长度。
            bytes_needed,
            // 允许平台核对最终长度。
            &mut bytes_needed,
        )
    }
    // 映射为不泄漏 SID 的认证失败。
    .map_err(|_| authentication_error("An endpoint peer identity could not be read."))?;
    // 返回拥有尾随 SID 的存储。
    Ok(TokenUserBuffer { storage })
}

// 读取进程 token 的完整性 RID 并保持数值仅用于私有比较。
fn token_integrity_rid(process: HANDLE) -> AppResult<u32> {
    // 初始化 token handle。
    let mut token = HANDLE::default();
    // 只申请完整性查询所需权限。
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.map_err(|_| {
        // 不公开 token 或完整性数值。
        authentication_error("An endpoint peer integrity token could not be queried.")
    })?;
    // 接管 token handle。
    let token = OwnedHandle::new(token)?;
    // 首次调用只查询所需字节数。
    let mut bytes_needed = 0_u32;
    // 缓冲区不足是预期结果，只使用返回长度。
    let _ = unsafe {
        GetTokenInformation(
            // 使用只读 token。
            token.raw(),
            // 查询 mandatory integrity label。
            TokenIntegrityLevel,
            // 首次不提供缓冲区。
            None,
            // 首次容量为零。
            0,
            // 接收所需字节数。
            &mut bytes_needed,
        )
    };
    // 要求至少容纳结构并限制异常 token 分配。
    if bytes_needed < u32::try_from(size_of::<TOKEN_MANDATORY_LABEL>()).unwrap_or(u32::MAX)
        // 防止异常平台长度扩大内存。
        || bytes_needed > 64 * 1024
    {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "An endpoint peer integrity boundary was invalid.",
        ));
    }
    // 计算满足 usize 对齐的元素数量。
    let word_size = size_of::<usize>();
    // 向上取整 token 缓冲区长度。
    let words = usize::try_from(bytes_needed)
        // 加入向上取整余量。
        .ok()
        // 防止加法溢出。
        .and_then(|bytes| bytes.checked_add(word_size.saturating_sub(1)))
        // 转换为 usize 元素数量。
        .map(|bytes| bytes / word_size)
        // 任一溢出都失败闭合。
        .ok_or_else(|| authentication_error("An endpoint peer integrity size overflowed."))?;
    // 分配清零且对齐的 token 存储。
    let mut storage = vec![0_usize; words];
    // 读取完整 mandatory label 与尾随 SID。
    unsafe {
        GetTokenInformation(
            // 使用只读 token。
            token.raw(),
            // 查询完整性标签。
            TokenIntegrityLevel,
            // 写入对齐存储。
            Some(storage.as_mut_ptr().cast()),
            // 传入平台报告长度。
            bytes_needed,
            // 允许平台核对最终长度。
            &mut bytes_needed,
        )
    }
    // 映射为不泄漏 RID 的认证失败。
    .map_err(|_| authentication_error("An endpoint peer integrity could not be read."))?;
    // 将对齐缓冲区解释为 mandatory label。
    let label = unsafe {
        // 只在 storage 生命周期内借用固定头部。
        &*storage.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()
    };
    // 读取完整性 SID 的子权限数量。
    let count = unsafe { GetSidSubAuthorityCount(label.Label.Sid) };
    // 空指针表示完整性 SID 不可认证。
    if count.is_null() {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "An endpoint peer integrity identity was invalid.",
        ));
    }
    // 复制子权限数量。
    let count = unsafe { *count };
    // mandatory label 必须至少包含一个 RID。
    if count == 0 {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "An endpoint peer integrity identity was empty.",
        ));
    }
    // 读取最后一个子权限即完整性 RID。
    let rid = unsafe { GetSidSubAuthority(label.Label.Sid, u32::from(count - 1)) };
    // 空 RID 指针不能建立认证事实。
    if rid.is_null() {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "An endpoint peer integrity value was unavailable.",
        ));
    }
    // 复制仅供相等比较的 RID。
    Ok(unsafe { *rid })
}

// 以受限权限打开精确进程。
fn open_process(process_id: u32) -> AppResult<OwnedHandle> {
    // PID 0 和当前 API 无效值不得进入 peer 认证。
    if process_id == 0 {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "The endpoint peer process is unavailable.",
        ));
    }
    // 只请求镜像名与 token 查询所需的有限权限。
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
        // 隐藏权限与 PID 细节。
        .map_err(|_| authentication_error("The endpoint peer process could not be opened."))?;
    // 接管有效进程 handle。
    OwnedHandle::new(handle)
}

// 从已打开的进程对象回读内核 PID，防止后续身份查询重新跟随可复用 PID。
fn process_id_from_handle(process: HANDLE) -> AppResult<u32> {
    // 只从当前持有的进程对象取得 PID。
    let process_id = unsafe { GetProcessId(process) };
    // 零值表示 handle 不能建立稳定进程身份。
    if process_id == 0 {
        // 不公开 handle 或平台失败细节。
        return Err(authentication_error(
            "The endpoint peer process identity could not be bound.",
        ));
    }
    // 返回只在认证边界内使用的稳定 PID。
    Ok(process_id)
}

// 从已打开进程的 token 查询 session，避免重新按 PID 解析另一个进程对象。
fn process_session_id_from_handle(process: HANDLE) -> AppResult<u32> {
    // 初始化只读 token handle。
    let mut token = HANDLE::default();
    // 只申请查询 token session 所需权限。
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.map_err(|_| {
        // 不公开进程或 token 原生事实。
        authentication_error("The endpoint peer session token could not be queried.")
    })?;
    // 接管 token handle 并保持到 session 查询完成。
    let token = OwnedHandle::new(token)?;
    // 初始化不会误报真实 session 的值。
    let mut session_id = u32::MAX;
    // 初始化平台返回的实际字节数。
    let mut bytes_returned = 0_u32;
    // 固定 TokenSessionId 的 u32 缓冲区大小。
    let session_bytes = u32::try_from(size_of::<u32>()).map_err(|_| {
        // 理论尺寸漂移也必须失败闭合。
        authentication_error("The endpoint peer session boundary overflowed.")
    })?;
    // 从同一已打开进程的 token 读取 session ID。
    unsafe {
        GetTokenInformation(
            // 使用与 peer handle 绑定的 token。
            token.raw(),
            // 查询 token 自身的 session 归属。
            TokenSessionId,
            // 写入固定 u32 存储。
            Some((&raw mut session_id).cast()),
            // 传入精确缓冲区大小。
            session_bytes,
            // 接收平台实际写入边界。
            &mut bytes_returned,
        )
    }
    // 查询失败保持认证失败且不泄漏 session。
    .map_err(|_| authentication_error("The endpoint peer session could not be authenticated."))?;
    // 平台必须返回完整且唯一的 u32 session 值。
    if bytes_returned != session_bytes || session_id == 0 || session_id == u32::MAX {
        // Session 0、哨兵或异常长度均不能成为交互 peer。
        return Err(authentication_error(
            "The endpoint peer is not in an interactive session.",
        ));
    }
    // 返回与持有中进程对象绑定的私有 session ID。
    Ok(session_id)
}

// 查询进程完整镜像路径。
fn process_image(process: HANDLE) -> AppResult<PathBuf> {
    // 分配 Windows 文档允许的最大有界路径缓冲区。
    let mut buffer = vec![0_u16; MAXIMUM_IMAGE_PATH_UNITS];
    // 传入可写容量并接收实际长度。
    let mut length = u32::try_from(buffer.len()).map_err(|_| {
        // 理论上的平台长度转换失败保持封闭。
        authentication_error("The endpoint peer image boundary overflowed.")
    })?;
    // 查询完整 DOS 路径且不跟随调用方字符串。
    unsafe {
        QueryFullProcessImageNameW(
            // 使用受限进程 handle。
            process,
            // 使用默认 Win32 路径格式。
            Default::default(),
            // 提供固定缓冲区。
            PWSTR(buffer.as_mut_ptr()),
            // 提供并接收长度。
            &mut length,
        )
    }
    // 映射为不公开路径的认证失败。
    .map_err(|_| authentication_error("The endpoint peer image could not be authenticated."))?;
    // 转换平台返回长度。
    let length = usize::try_from(length).map_err(|_| {
        // 不公开异常长度。
        authentication_error("The endpoint peer image length overflowed.")
    })?;
    // 空值或超出缓冲区均为不可认证。
    if length == 0 || length > buffer.len() {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "The endpoint peer image returned an invalid boundary.",
        ));
    }
    // 使用 Windows 原生 UTF-16 构造私有路径。
    use std::os::windows::ffi::OsStringExt;
    // 返回不进入公共结果的路径。
    Ok(PathBuf::from(std::ffi::OsString::from_wide(
        // 只读取有效平台单元。
        &buffer[..length],
    )))
}

// 以 Windows 路径不区分大小写语义比较固定 sibling。
fn same_windows_path(actual: &Path, expected: &Path) -> bool {
    // 转换为仅在进程内使用的 UTF-16 单元。
    use std::os::windows::ffi::OsStrExt;
    // 取得实际路径的 UTF-16。
    let actual = actual.as_os_str().encode_wide().collect::<Vec<_>>();
    // 取得期望路径的 UTF-16。
    let expected = expected.as_os_str().encode_wide().collect::<Vec<_>>();
    // 简单路径必须长度相同。
    actual.len() == expected.len()
        // 对每个基本 Unicode 单元执行 Windows 路径所需的 ASCII 大小写放宽。
        && actual.iter().zip(expected.iter()).all(|(left, right)| {
            // ASCII 单元使用不区分大小写比较。
            match (u8::try_from(*left), u8::try_from(*right)) {
                // 两个 ASCII 单元按 Windows 常见路径语义比较。
                (Ok(left), Ok(right)) => left.eq_ignore_ascii_case(&right),
                // 非 ASCII 单元必须逐单元一致，避免本地化猜测。
                _ => left == right,
            }
        })
}

// 查询指定进程所属 Windows session。
pub(crate) fn process_session_id(process_id: u32) -> AppResult<u32> {
    // 初始化不会误报真实 session 的值。
    let mut session_id = u32::MAX;
    // 查询内核维护的进程 session 归属。
    unsafe { ProcessIdToSessionId(process_id, &mut session_id) }.map_err(|_| {
        // 不公开 PID 或原生错误。
        authentication_error("The endpoint peer session could not be authenticated.")
    })?;
    // Session 0 与未初始化值不能成为交互 endpoint。
    if session_id == 0 || session_id == u32::MAX {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "The endpoint peer is not in an interactive session.",
        ));
    }
    // 返回只在 Adapter 内流转的 native session ID。
    Ok(session_id)
}

// 查询当前进程所属 Windows session。
pub(crate) fn current_session_id() -> AppResult<u32> {
    // 复用同一内核查询边界。
    process_session_id(unsafe { GetCurrentProcessId() })
}

// 认证固定镜像、精确 session 与当前用户主体的对等进程。
pub(crate) fn authenticate_peer_process(
    process_id: u32,
    expected_image: &Path,
    expected_session_id: u32,
) -> AppResult<()> {
    // 先打开 PID 当时指向的进程对象并保持 handle 到认证结束。
    let peer = open_process(process_id)?;
    // 从持有中的进程对象回核稳定 PID。
    let bound_process_id = process_id_from_handle(peer.raw())?;
    // pipe 报告的 PID 必须仍绑定刚打开的进程对象。
    if bound_process_id != process_id {
        // 不公开任一 PID 数值。
        return Err(authentication_error(
            "The endpoint peer process identity changed during authentication.",
        ));
    }
    // 当前进程不能冒充 IPC 对等端。
    if bound_process_id == unsafe { GetCurrentProcessId() } {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "The endpoint peer process is not independent.",
        ));
    }
    // 从同一持有进程的 token 精确核对 session 归属。
    if process_session_id_from_handle(peer.raw())? != expected_session_id {
        // 不公开任一 session number。
        return Err(authentication_error(
            "The endpoint peer session does not match the certified route.",
        ));
    }
    // 核对固定 sibling 完整路径。
    if !same_windows_path(&process_image(peer.raw())?, expected_image) {
        // 不公开实际或期望安装路径。
        return Err(authentication_error(
            "The endpoint peer image does not match the fixed installation.",
        ));
    }
    // 读取当前进程用户主体。
    let current_user = token_user(unsafe { GetCurrentProcess() })?;
    // 读取 peer 用户主体。
    let peer_user = token_user(peer.raw())?;
    // 要求 Windows SID 精确相同。
    unsafe { EqualSid(current_user.sid(), peer_user.sid()) }.map_err(|_| {
        // 不公开任何 SID 文本。
        authentication_error("The endpoint peer principal is not authorized.")
    })?;
    // 读取当前进程完整性 RID。
    let current_integrity = token_integrity_rid(unsafe { GetCurrentProcess() })?;
    // 读取 peer 完整性 RID。
    let peer_integrity = token_integrity_rid(peer.raw())?;
    // 固定 sibling 必须处于相同完整性边界。
    if current_integrity != peer_integrity {
        // 不公开任一 RID 或相对高低。
        return Err(authentication_error(
            "The endpoint peer integrity does not match the fixed route.",
        ));
    }
    // 返回 fixed image、session 与 principal 全部通过。
    Ok(())
}

// 返回当前进程的固定 sibling 路径并要求文件存在。
pub(crate) fn sibling_image_path(file_name: &str) -> AppResult<PathBuf> {
    // 拒绝目录、空值与路径分隔符，调用方只能提供编译期固定文件名。
    if file_name.is_empty() || file_name.contains(['\\', '/']) {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "The fixed endpoint image name is invalid.",
        ));
    }
    // 读取当前可执行文件的绝对路径。
    let current = std::env::current_exe().map_err(|_| {
        // 不公开安装位置。
        authentication_error("The fixed installation directory could not be resolved.")
    })?;
    // 取得 sibling 目录。
    let directory = current.parent().ok_or_else(|| {
        // 返回封闭认证失败。
        authentication_error("The fixed installation directory is unavailable.")
    })?;
    // 只拼接固定文件名。
    let sibling = directory.join(file_name);
    // 固定 peer 必须实际安装。
    if !sibling.is_file() {
        // 不公开缺失路径。
        return Err(authentication_error(
            "The fixed endpoint peer is not installed beside the current executable.",
        ));
    }
    // 返回仅供进程认证使用的私有路径。
    Ok(sibling)
}

// 查询活动 session 的稳定登录代际并同时验证状态。
pub(crate) fn active_session_generation(session_id: u32) -> AppResult<String> {
    // Session 0 永远不属于独立交互会话。
    if session_id == 0 {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "The endpoint session is not interactive.",
        ));
    }
    // 初始化 WTS 分配缓冲区。
    let mut buffer = PWSTR::null();
    // 初始化实际字节数量。
    let mut bytes_returned = 0_u32;
    // 查询封闭 session 信息。
    unsafe {
        WTSQuerySessionInformationW(
            // 使用本机 WTS server。
            Some(WTS_CURRENT_SERVER_HANDLE),
            // 查询精确 session。
            session_id,
            // 读取包含登录代际的固定结构。
            WTSSessionInfo,
            // 接收 WTS 分配指针。
            &mut buffer,
            // 接收缓冲区字节数。
            &mut bytes_returned,
        )
    }
    // 查询失败保持不可认证。
    .map_err(|_| authentication_error("The endpoint session state could not be read."))?;
    // WTS 成功分配必须在全部路径配对释放。
    struct OwnedWts(PWSTR);
    // 为 WTS 缓冲区实现配对回收。
    impl Drop for OwnedWts {
        // 释放 WTS 分配。
        fn drop(&mut self) {
            // 空指针也由 WTSFreeMemory 安全处理。
            unsafe { WTSFreeMemory(self.0.0.cast()) };
        }
    }
    // 立即接管平台缓冲区。
    let buffer = OwnedWts(buffer);
    // 检查指针与结构长度。
    if buffer.0.0.is_null()
        || bytes_returned < u32::try_from(size_of::<WTSINFOW>()).unwrap_or(u32::MAX)
    {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "The endpoint session returned an invalid state boundary.",
        ));
    }
    // 从 WTS 自有缓冲区复制固定结构。
    let info = unsafe {
        // 使用非对齐读取避免对外部缓冲区额外假设。
        std::ptr::read_unaligned(buffer.0.0.cast::<WTSINFOW>())
    };
    // 要求状态、session 与登录代际同时成立。
    if info.State != WTSActive || info.SessionId != session_id || info.LogonTime <= 0 {
        // 不公开具体状态或登录时间。
        return Err(authentication_error(
            "The endpoint session is not an active authenticated generation.",
        ));
    }
    // 只形成稍后散列的私有代际文本。
    Ok(format!("{}:{}", info.SessionId, info.LogonTime))
}

// 枚举当前主机上活动且不同于 host 的候选 session。
pub(crate) fn active_other_session_ids() -> AppResult<Vec<u32>> {
    // 取得 host 当前 session 作为排除项。
    let current = current_session_id()?;
    // 初始化 WTS session 数组指针。
    let mut sessions = std::ptr::null_mut::<WTS_SESSION_INFOW>();
    // 初始化记录数量。
    let mut count = 0_u32;
    // 枚举本机一级 session 信息。
    unsafe {
        WTSEnumerateSessionsW(
            // 使用本机 WTS server。
            Some(WTS_CURRENT_SERVER_HANDLE),
            // 保留参数必须为零。
            0,
            // 固定版本一。
            1,
            // 接收 WTS 分配数组。
            &mut sessions,
            // 接收记录数量。
            &mut count,
        )
    }
    // 枚举失败保持 endpoint 不可用。
    .map_err(|_| authentication_error("Interactive sessions could not be enumerated."))?;
    // 立即为 WTS 数组建立配对释放。
    struct OwnedSessionArray(*mut WTS_SESSION_INFOW);
    // 为 WTS 数组实现回收。
    impl Drop for OwnedSessionArray {
        // 释放平台数组。
        fn drop(&mut self) {
            // 使用 WTS 配对释放接口。
            unsafe { WTSFreeMemory(self.0.cast()) };
        }
    }
    // 接管数组指针。
    let sessions = OwnedSessionArray(sessions);
    // 转换并限制记录数量。
    let count = usize::try_from(count).map_err(|_| {
        // 不公开异常原生数量。
        authentication_error("Interactive session count overflowed.")
    })?;
    // 空数组允许返回无候选。
    if count == 0 {
        // 返回确定空集合。
        return Ok(Vec::new());
    }
    // 非空数量必须具有有效指针且不超过硬上限。
    if sessions.0.is_null() || count > MAXIMUM_SESSION_COUNT {
        // 返回封闭认证失败。
        return Err(authentication_error(
            "Interactive session enumeration exceeded its safety boundary.",
        ));
    }
    // 借用 WTS 固定长度数组。
    let records = unsafe {
        // 数量和非空指针已经验证。
        std::slice::from_raw_parts(sessions.0, count)
    };
    // 保存唯一活动候选。
    let mut result = Vec::new();
    // 遍历平台 session 快照。
    for record in records {
        // 只接受活动、非 Session 0、不同于 host 的 session。
        if record.State == WTSActive && record.SessionId != 0 && record.SessionId != current {
            // 再读取登录代际以拒绝漂移或异常记录。
            active_session_generation(record.SessionId)?;
            // 去重同一 native session。
            if !result.contains(&record.SessionId) {
                // 保存私有候选 ID。
                result.push(record.SessionId);
            }
        }
    }
    // 使用 native ID 排序只为稳定内部探测顺序。
    result.sort_unstable();
    // 返回不跨公共边界的候选集合。
    Ok(result)
}

// 查询当前进程的 ToolHelp parent PID。
pub(crate) fn parent_process_id() -> AppResult<u32> {
    // 创建只读进程快照。
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        // 隐藏快照错误细节。
        .map_err(|_| authentication_error("The command worker parent could not be inspected."))?;
    // 接管快照 handle。
    let snapshot = OwnedHandle::new(snapshot)?;
    // 初始化 ToolHelp 结构大小。
    let mut entry = PROCESSENTRY32W {
        // 写入 Windows 要求的结构长度。
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).map_err(|_| {
            // 返回封闭长度错误。
            authentication_error("The process snapshot boundary overflowed.")
        })?,
        // 其余字段清零。
        ..Default::default()
    };
    // 读取首条记录。
    unsafe { Process32FirstW(snapshot.raw(), &mut entry) }
        // 空或不可读快照不能认证 parent。
        .map_err(|_| authentication_error("The command worker parent is unavailable."))?;
    // 取得当前进程 PID。
    let current = unsafe { GetCurrentProcessId() };
    // 遍历快照直到找到当前进程。
    loop {
        // 精确匹配当前 PID。
        if entry.th32ProcessID == current {
            // parent PID 0 不能成为认证 broker。
            if entry.th32ParentProcessID == 0 {
                // 返回封闭认证失败。
                return Err(authentication_error(
                    "The command worker has no authenticated parent.",
                ));
            }
            // 返回仅供 peer 认证使用的 parent PID。
            return Ok(entry.th32ParentProcessID);
        }
        // 移到下一条进程记录。
        if unsafe { Process32NextW(snapshot.raw(), &mut entry) }.is_err() {
            // 自然结束但未找到当前进程意味着快照过期。
            break;
        }
    }
    // 返回明确不可认证结果。
    Err(authentication_error(
        "The command worker parent could not be resolved uniquely.",
    ))
}

// 声明不公开 native 值的身份边界测试。
#[cfg(test)]
mod tests {
    // 导入被测生产接口。
    use super::*;

    // 验证当前 session 具有活动登录代际且不泄漏到错误。
    #[test]
    fn current_session_has_private_active_generation() {
        // 读取当前 session。
        let session_id = current_session_id()
            // 当前测试进程应位于活动交互会话。
            .unwrap_or_else(|error| panic!("current session unavailable: {error}"));
        // 读取仅供散列的登录代际。
        let generation = active_session_generation(session_id)
            // 当前测试会话应具备稳定代际。
            .unwrap_or_else(|error| panic!("current generation unavailable: {error}"));
        // 私有代际非空。
        assert!(!generation.is_empty());
        // 该值不会由任何公开结构直接返回。
        assert!(generation.contains(':'));
    }

    // 验证完整性 RID 可稳定读取且仅在私有认证边界比较。
    #[test]
    fn current_integrity_is_read_consistently() {
        // 读取当前进程首次完整性事实。
        let first = token_integrity_rid(unsafe { GetCurrentProcess() })
            // 当前 Windows 测试进程必须具有 mandatory label。
            .unwrap_or_else(|error| panic!("current integrity unavailable: {error}"));
        // 再次读取同一进程完整性事实。
        let second = token_integrity_rid(unsafe { GetCurrentProcess() })
            // 重复读取必须成功。
            .unwrap_or_else(|error| panic!("repeated integrity unavailable: {error}"));
        // 同一 token 的 RID 必须稳定一致。
        assert_eq!(first, second);
        // mandatory integrity RID 不得是空哨兵。
        assert_ne!(first, 0);
    }

    // 验证已打开进程 handle 同时固定 PID 与 token session 事实。
    #[test]
    fn opened_process_handle_binds_pid_and_session() {
        // 取得当前测试进程 PID。
        let current_process_id = unsafe { GetCurrentProcessId() };
        // 以生产受限权限打开当前进程对象。
        let process = open_process(current_process_id)
            // 当前进程必须可供只读认证测试。
            .unwrap_or_else(|error| panic!("current process unavailable: {error}"));
        // handle 回读 PID 必须与打开时目标一致。
        let bound_process_id = process_id_from_handle(process.raw())
            // 有效进程 handle 必须形成稳定 PID。
            .unwrap_or_else(|error| panic!("bound process identity unavailable: {error}"));
        // 核对 PID 未经第二次名称解析。
        assert_eq!(bound_process_id, current_process_id);
        // 从持有中进程 token 读取 session。
        let bound_session_id = process_session_id_from_handle(process.raw())
            // 当前交互测试进程必须具有 token session。
            .unwrap_or_else(|error| panic!("bound process session unavailable: {error}"));
        // 与现有公开私有 session 查询结果交叉验证。
        let current_session_id = current_session_id()
            // 当前测试进程必须位于交互 session。
            .unwrap_or_else(|error| panic!("current process session unavailable: {error}"));
        // 两条内核路径必须指向同一 session。
        assert_eq!(bound_session_id, current_session_id);
    }

    // 验证固定镜像认证接受当前主程序以外的真实 peer 约束。
    #[test]
    fn invalid_peer_never_exposes_native_identity() {
        // 使用 PID 0 触发确定性认证失败。
        let error = authenticate_peer_process(
            // PID 0 明确无效。
            0,
            // 使用不会被读取的固定路径。
            Path::new("fixed.exe"),
            // 使用当前 session 作为合成期望。
            current_session_id().unwrap_or(u32::MAX),
        )
        // 保存预期错误。
        .err()
        // 意外成功时明确测试失败。
        .unwrap_or_else(|| panic!("PID zero must not authenticate"));
        // 使用稳定认证错误码。
        assert_eq!(error.code, "ENDPOINT_AUTHENTICATION_FAILED");
        // 错误详情不得携带原生值。
        assert!(error.details.is_null());
    }
}
