//! 只读 Windows 会话与输入桌面安全姿态 Adapter。

// 导入原生缓冲区尺寸计算。
use std::mem::size_of;

// 导入 Windows 错误、handle 与字符串指针类型。
use windows::{
    // 只在私有 Adapter 内使用原生 API 与类型。
    Win32::{
        // 使用访问拒绝 HRESULT 和通用 handle 包装桌面对象。
        Foundation::{E_ACCESSDENIED, HANDLE},
        // 使用只读会话、线程与桌面 API。
        System::{
            // 查询当前进程所属会话及其连接状态。
            RemoteDesktop::{
                ProcessIdToSessionId, WTS_CONNECTSTATE_CLASS, WTS_CURRENT_SERVER_HANDLE, WTSActive,
                WTSConnectState, WTSFreeMemory, WTSQuerySessionInformationW,
            },
            // 查询当前线程桌面、输入桌面与私有名称。
            StationsAndDesktops::{
                CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, GetThreadDesktop,
                GetUserObjectInformationW, HDESK, OpenInputDesktop, UOI_NAME,
            },
            // 查询当前进程与线程的稳定本机 ID。
            Threading::{GetCurrentProcessId, GetCurrentThreadId},
        },
    },
    // 使用 Windows 字符串指针接收 WTS 分配缓冲区。
    core::PWSTR,
};

// 导入 Module 定义的平台端口和封闭事实。
use crate::modules::permission_boundary::{
    DesktopSecurityState, HostSecurityFacts, SecurityContextProbe, SessionSecurityState,
};

// 限制桌面名称只读缓冲区，拒绝异常平台长度。
const MAXIMUM_DESKTOP_NAME_BYTES: u32 = 1024;

// 拥有由 WTS 分配的短生命周期缓冲区。
struct OwnedWtsBuffer(PWSTR);

// 保证所有 WTS 成功分配都被回收。
impl Drop for OwnedWtsBuffer {
    // 在作用域退出时释放平台缓冲区。
    fn drop(&mut self) {
        // WTS 要求使用配对释放函数。
        unsafe { WTSFreeMemory(self.0.0.cast()) };
    }
}

// 拥有由 OpenInputDesktop 返回的桌面 handle。
struct OwnedDesktop(HDESK);

// 保证输入桌面 handle 不跨请求泄漏。
impl Drop for OwnedDesktop {
    // 在作用域退出时关闭拥有的桌面 handle。
    fn drop(&mut self) {
        // 忽略关闭错误，因为探针结论已经完成且 handle 不公开。
        let _ = unsafe { CloseDesktop(self.0) };
    }
}

// 读取当前进程的封闭会话状态。
fn session_state() -> SessionSecurityState {
    // 初始化不会误报真实会话的私有变量。
    let mut session_id = u32::MAX;
    // 将当前进程映射到 Windows 会话。
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session_id) }.is_err() {
        // 映射失败不得假设处于交互会话。
        return SessionSecurityState::Indeterminate;
    }
    // Session 0 永远不进入交互控制路线。
    if session_id == 0 {
        // 返回明确 Session 0 分类。
        return SessionSecurityState::SessionZero;
    }
    // 初始化 WTS 输出指针。
    let mut buffer = PWSTR::null();
    // 初始化实际字节数量。
    let mut bytes_returned = 0_u32;
    // 查询当前会话的连接状态。
    if unsafe {
        WTSQuerySessionInformationW(
            // 使用本机 WTS server。
            Some(WTS_CURRENT_SERVER_HANDLE),
            // 查询当前进程所属会话。
            session_id,
            // 只读取连接状态枚举。
            WTSConnectState,
            // 接收 WTS 分配指针。
            &mut buffer,
            // 接收缓冲区字节数。
            &mut bytes_returned,
        )
    }
    // 平台查询失败保持不可确定。
    .is_err()
    {
        // 返回不可确定会话事实。
        return SessionSecurityState::Indeterminate;
    }
    // 成功调用必须立即建立配对释放所有权。
    let owned_buffer = OwnedWtsBuffer(buffer);
    // 空指针或短缓冲区不得被解引用。
    if owned_buffer.0.0.is_null()
        || bytes_returned < u32::try_from(size_of::<WTS_CONNECTSTATE_CLASS>()).unwrap_or(u32::MAX)
    {
        // 返回不可确定会话事实。
        return SessionSecurityState::Indeterminate;
    }
    // 从 WTS 自有缓冲区复制封闭连接状态。
    let state = unsafe {
        // 使用非对齐读取避免对外部缓冲区布局做额外假设。
        std::ptr::read_unaligned(owned_buffer.0.0.cast::<WTS_CONNECTSTATE_CLASS>())
    };
    // 只有 WTSActive 被认证为当前活动交互会话。
    if state == WTSActive {
        // 返回活动交互分类。
        SessionSecurityState::ActiveInteractive
    } else {
        // 断开、空闲、监听或其他状态统一阻塞。
        SessionSecurityState::NotActive
    }
}

// 读取一个桌面对象的私有名称。
fn desktop_name(desktop: HDESK) -> Result<String, ()> {
    // 初始化所需字节数。
    let mut bytes_needed = 0_u32;
    // 第一次调用只查询长度，预期不会返回名称。
    let _ = unsafe {
        GetUserObjectInformationW(
            // 把桌面包装为通用用户对象 handle。
            HANDLE(desktop.0),
            // 仅请求私有名称用于等价比较。
            UOI_NAME,
            // 不提供数据缓冲区。
            None,
            // 长度查询使用零字节。
            0,
            // 接收平台要求长度。
            Some(&mut bytes_needed),
        )
    };
    // 拒绝空、奇数字节或异常大的名称缓冲区。
    if bytes_needed < 2
        || !bytes_needed.is_multiple_of(u32::try_from(size_of::<u16>()).unwrap_or(2))
        || bytes_needed > MAXIMUM_DESKTOP_NAME_BYTES
    {
        // 返回不可确定事实且不公开原始长度。
        return Err(());
    }
    // 按 UTF-16 单元数量分配有界私有缓冲区。
    let mut buffer = vec![0_u16; bytes_needed as usize / size_of::<u16>()];
    // 读取完整桌面名称。
    unsafe {
        GetUserObjectInformationW(
            // 使用相同桌面对象。
            HANDLE(desktop.0),
            // 只读取名称。
            UOI_NAME,
            // 传入有界 UTF-16 缓冲区。
            Some(buffer.as_mut_ptr().cast()),
            // 传入平台报告的精确字节数。
            bytes_needed,
            // 不再需要长度输出。
            None,
        )
    }
    // 平台读取失败保持不可确定。
    .map_err(|_| ())?;
    // 找到首个终止空字符。
    let length = buffer
        // 遍历私有 UTF-16 单元。
        .iter()
        // 定位终止符。
        .position(|unit| *unit == 0)
        // 若平台未写终止符则使用完整有界缓冲区。
        .unwrap_or(buffer.len());
    // 严格解码名称，异常 UTF-16 不得用于授权。
    String::from_utf16(&buffer[..length]).map_err(|_| ())
}

// 比较当前进程桌面与活动输入 Default 桌面。
fn desktop_state() -> DesktopSecurityState {
    // 取得当前线程桌面；该 handle 不归调用方所有。
    let thread_desktop = match unsafe { GetThreadDesktop(GetCurrentThreadId()) } {
        // 保存只读借用 handle。
        Ok(desktop) => desktop,
        // 无法取得线程桌面时保持不可确定。
        Err(_) => return DesktopSecurityState::Indeterminate,
    };
    // 只以读取对象权限打开当前输入桌面。
    let input_desktop = match unsafe {
        OpenInputDesktop(
            // 不请求切换或 hook 标志。
            DESKTOP_CONTROL_FLAGS(0),
            // 不允许子进程继承 handle。
            false,
            // 只请求读取对象权限。
            DESKTOP_READOBJECTS,
        )
    } {
        // 成功时立即建立 RAII 所有权。
        Ok(desktop) => OwnedDesktop(desktop),
        // 明确访问拒绝代表受保护输入桌面。
        Err(error) if error.code() == E_ACCESSDENIED => {
            // 返回稳定受保护分类。
            return DesktopSecurityState::ProtectedOrNonInput;
        }
        // 其他平台错误无法可靠分类。
        Err(_) => return DesktopSecurityState::Indeterminate,
    };
    // 读取线程桌面的私有名称。
    let thread_name = match desktop_name(thread_desktop) {
        // 保存有界私有名称。
        Ok(name) => name,
        // 名称不可读时不得猜测。
        Err(()) => return DesktopSecurityState::Indeterminate,
    };
    // 读取输入桌面的私有名称。
    let input_name = match desktop_name(input_desktop.0) {
        // 保存有界私有名称。
        Ok(name) => name,
        // 名称不可读时不得猜测。
        Err(()) => return DesktopSecurityState::Indeterminate,
    };
    // 要求两个桌面相同且输入桌面正是标准 Default。
    if thread_name.eq_ignore_ascii_case(&input_name) && input_name.eq_ignore_ascii_case("Default") {
        // 唯一允许的桌面组合。
        DesktopSecurityState::ActiveDefaultInput
    } else {
        // UAC、锁屏或工具自有非输入桌面统一阻塞。
        DesktopSecurityState::ProtectedOrNonInput
    }
}

// 提供无状态的生产 Windows 安全姿态探针。
pub(crate) struct WindowsSecurityContextProbe;

// 实现 Module 定义的只读平台端口。
impl SecurityContextProbe for WindowsSecurityContextProbe {
    // 取得一次不含敏感原生值的封闭快照。
    fn probe(&self) -> HostSecurityFacts {
        // 组合独立会话与桌面只读事实。
        HostSecurityFacts {
            // 先取得当前进程会话状态。
            session: session_state(),
            // 再比较进程桌面与输入桌面。
            desktop: desktop_state(),
        }
    }
}

// 声明当前生产环境的只读 smoke test。
#[cfg(test)]
mod tests {
    // 导入平台端口以调用生产实现。
    use crate::modules::permission_boundary::SecurityContextProbe;

    // 导入生产探针。
    use super::WindowsSecurityContextProbe;

    // 验证当前测试进程能取得封闭事实且不产生公开原生值。
    #[test]
    fn current_process_security_probe_returns_closed_facts() {
        // 执行只读生产探针。
        let facts = WindowsSecurityContextProbe.probe();
        // Debug 文本只包含封闭枚举名称。
        let text = format!("{facts:?}");
        // 不得出现路径分隔符。
        assert!(!text.contains('\\'));
        // 不得出现 SID 前缀。
        assert!(!text.contains("S-1-"));
        // 不得出现原生十六进制 handle 前缀。
        assert!(!text.contains("0x"));
    }
}
