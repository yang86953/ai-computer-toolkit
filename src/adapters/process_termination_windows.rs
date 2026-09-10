//! 精确进程代际保护、固定终止 dispatch 与句柄等待 Windows Adapter。

// 导入 Windows 句柄生命周期与固定轮询结果。
use windows::{
    // 导入 Win32 进程、会话与消息 API。
    Win32::{
        // 导入句柄、错误与等待常量。
        Foundation::{
            CloseHandle, E_ACCESSDENIED, ERROR_ACCESS_DENIED, ERROR_SUCCESS, FILETIME,
            GetLastError, HANDLE, SetLastError, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        // 导入当前与目标进程 Windows session 查询。
        System::RemoteDesktop::ProcessIdToSessionId,
        // 导入最小进程权限、代际、critical、终止与等待 API。
        System::Threading::{
            GetCurrentProcessId, GetProcessTimes, IsProcessCritical, OpenProcess,
            PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
            TerminateProcess, WaitForSingleObject,
        },
        // 导入固定顶层窗口关闭请求。
        UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE},
    },
    // 导入 Windows 布尔类型。
    core::BOOL,
};

// 导入当前私有进程事实、窗口枚举与风险模式。
use crate::{
    // 平台 Adapter 只复用同层只读 inventory 与窗口身份 Component。
    adapters::{
        // 写前逐窗口核对原生身份。
        window_identity_windows::{native_window, same_window_identity},
        // 只接受 Module 已唯一解析的私有进程记录。
        windows::{ProcessRecord, enumerate_windows},
    },
    // 风险模式由 provider-neutral Component 固定。
    components::process_termination_contract::ProcessTerminationMode,
};

// 固定进程句柄同步等待权限位。
const PROCESS_SYNCHRONIZE: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0010_0000);
// 固定工具发起强制终止时使用的私有非零退出码。
const TOOL_TERMINATED_EXIT_CODE: u32 = 1;

// 表示平台确认的保护目标类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProtectedProcessKind {
    // 当前工具绝不允许自终止。
    CurrentTool,
    // PID 0 与 4 属于固定系统保护目标。
    SystemProcess,
    // Windows 标记的 critical process 不得触碰。
    CriticalProcess,
    // 其他 Windows 登录会话不属于当前授权桌面。
    OtherSession,
}

// 表示进程平台边界的封闭失败分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessTerminationFailure {
    // 目标已经退出或创建代际变化。
    Stale,
    // Windows 明确拒绝所需最小权限。
    PermissionDenied,
    // 目标命中固定保护策略。
    Protected(ProtectedProcessKind),
    // 保护状态无法认证时失败闭合。
    ProtectionCheckFailed,
    // 优雅路径没有可认证顶层窗口。
    Unsupported,
    // dispatch 或枚举发生非权限平台失败。
    OperationFailed,
    // 已接受后无法继续认证同一句柄状态。
    WaitFailed,
}

// 保存已经核对创建代际且拥有最小权限的私有进程句柄。
pub(crate) struct PreparedProcess {
    // 句柄不得进入任何 JSON 或跨 Module 边界。
    handle: HANDLE,
    // 保存私有 PID 只供顶层窗口精确筛选。
    process_id: u32,
    // 保存私有创建 FILETIME 只供所属进程代际筛选。
    creation_time: u64,
}

// 在离开一次同步 Command 时关闭私有进程句柄。
impl Drop for PreparedProcess {
    // 关闭由 OpenProcess 创建的唯一资源。
    fn drop(&mut self) {
        // 句柄关闭失败不改变已经形成的领域结果。
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

// 合并 FILETIME 高低位为 inventory 使用的同一私有代际表示。
const fn filetime_value(value: FILETIME) -> u64 {
    // 保持 Windows FILETIME 的无符号位布局。
    (value.dwHighDateTime as u64) << 32 | value.dwLowDateTime as u64
}

// 判断 Windows 错误是否为稳定访问拒绝。
fn permission_denied(error: &windows::core::Error) -> bool {
    // OpenProcess 与 TerminateProcess 都映射为 E_ACCESSDENIED。
    error.code() == E_ACCESSDENIED
}

// 读取一个进程所属 Windows 登录会话。
fn process_session_id(process_id: u32) -> Result<u32, ProcessTerminationFailure> {
    // 初始化失败时不会误用的会话哨兵。
    let mut session_id = 0_u32;
    // 只读查询失败意味着无法认证当前授权桌面边界。
    unsafe { ProcessIdToSessionId(process_id, &mut session_id) }
        // 隐藏原生错误并收敛为保护检查失败。
        .map_err(|_| ProcessTerminationFailure::ProtectionCheckFailed)?;
    // 返回私有会话 ID。
    Ok(session_id)
}

// 在持有句柄后重新读取进程创建代际。
fn creation_time(handle: HANDLE) -> Result<u64, ProcessTerminationFailure> {
    // 保存创建 FILETIME。
    let mut created = FILETIME::default();
    // 退出时间只用于 API 占位且不公开。
    let mut exited = FILETIME::default();
    // 内核时间只用于 API 占位且不公开。
    let mut kernel = FILETIME::default();
    // 用户时间只用于 API 占位且不公开。
    let mut user = FILETIME::default();
    // 读取失败时无法认证 PID 未复用。
    unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) }
        // 句柄权限已经建立，后续失败按 stale 保守处理。
        .map_err(|_| ProcessTerminationFailure::Stale)?;
    // 返回与 inventory 相同的代际值。
    Ok(filetime_value(created))
}

// 对持有的同一进程句柄执行一次零等待状态采样。
pub(crate) fn exited(process: &PreparedProcess) -> Result<bool, ProcessTerminationFailure> {
    // 零毫秒采样允许 Module 自己拥有 deadline 与 cancellation。
    match unsafe { WaitForSingleObject(process.handle, 0) } {
        // 句柄 signaled 证明同一进程对象已经退出。
        WAIT_OBJECT_0 => Ok(true),
        // timeout 表示同一进程仍在运行。
        WAIT_TIMEOUT => Ok(false),
        // 其他返回值无法认证最终状态。
        _ => Err(ProcessTerminationFailure::WaitFailed),
    }
}

// 打开并认证一次精确进程代际及固定保护目标。
pub(crate) fn prepare(
    // 接收 Module 已唯一解析的私有进程记录。
    target: &ProcessRecord,
    // 接收 capability 已固定的风险模式。
    mode: ProcessTerminationMode,
) -> Result<PreparedProcess, ProcessTerminationFailure> {
    // 读取当前工具 PID 只用于自保护。
    let current_process_id = unsafe { GetCurrentProcessId() };
    // 当前工具绝不允许终止自身。
    if target.process_id == current_process_id {
        // 返回固定自保护分类。
        return Err(ProcessTerminationFailure::Protected(
            ProtectedProcessKind::CurrentTool,
        ));
    }
    // 固定保护 Idle 与 System PID。
    if matches!(target.process_id, 0 | 4) {
        // 返回固定系统目标分类。
        return Err(ProcessTerminationFailure::Protected(
            ProtectedProcessKind::SystemProcess,
        ));
    }
    // 读取当前工具 Windows 登录会话。
    let current_session = process_session_id(current_process_id)?;
    // 读取目标 Windows 登录会话。
    let target_session = process_session_id(target.process_id)?;
    // 只允许当前授权桌面的同会话进程。
    if current_session != target_session {
        // 返回跨会话保护分类。
        return Err(ProcessTerminationFailure::Protected(
            ProtectedProcessKind::OtherSession,
        ));
    }
    // 查询、代际和同步等待始终需要固定最小权限。
    let mut access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE;
    // 只有明确强制 capability 才请求终止权限。
    if mode == ProcessTerminationMode::Force {
        // 加入不可回滚的强制终止权限。
        access |= PROCESS_TERMINATE;
    }
    // 打开精确 PID 的当前进程对象。
    let handle = unsafe { OpenProcess(access, false, target.process_id) }.map_err(|error| {
        // 明确访问拒绝保持权限分类。
        if permission_denied(&error) {
            // 返回稳定权限失败。
            ProcessTerminationFailure::PermissionDenied
        } else {
            // 退出或 PID 已复用时保守按 stale 处理。
            ProcessTerminationFailure::Stale
        }
    })?;
    // 立即把句柄放入 RAII 资源，任何后续分支都会关闭。
    let process = PreparedProcess {
        // 保存私有句柄。
        handle,
        // 保存当前 PID。
        process_id: target.process_id,
        // 保存已解析创建代际。
        creation_time: target.process_creation_time,
    };
    // 打开句柄后的创建代际必须与 opaque 目标完全一致。
    if creation_time(process.handle)? != process.creation_time {
        // PID 复用在 dispatch 前结构化失败。
        return Err(ProcessTerminationFailure::Stale);
    }
    // 已退出句柄不得进入 dispatch。
    if exited(&process)? {
        // 目标在写前已经 stale。
        return Err(ProcessTerminationFailure::Stale);
    }
    // 保存 Windows critical 标志。
    let mut critical = BOOL::from(false);
    // 无法读取 critical 状态时必须失败闭合。
    unsafe { IsProcessCritical(process.handle, &mut critical) }
        // 不猜测目标可安全终止。
        .map_err(|_| ProcessTerminationFailure::ProtectionCheckFailed)?;
    // critical process 永不 dispatch。
    if critical.as_bool() {
        // 返回固定保护分类。
        return Err(ProcessTerminationFailure::Protected(
            ProtectedProcessKind::CriticalProcess,
        ));
    }
    // 返回已经认证的私有句柄。
    Ok(process)
}

// 向精确进程当前顶层窗口投递固定关闭请求。
pub(crate) fn dispatch_graceful(
    // 接收已经通过代际与保护检查的私有句柄。
    process: &PreparedProcess,
) -> Result<(), ProcessTerminationFailure> {
    // dispatch 前再次确认同一进程仍在运行。
    if exited(process)? {
        // 已退出目标按 stale 处理。
        return Err(ProcessTerminationFailure::Stale);
    }
    // 枚举当前全部顶层窗口，不按标题或可见性猜测资格。
    let windows = enumerate_windows().map_err(|_| ProcessTerminationFailure::OperationFailed)?;
    // 只保留属于同一 PID 与创建代际的精确顶层窗口。
    let candidates = windows
        // 消费当前快照。
        .iter()
        // 同时核对 PID 和创建时间。
        .filter(|window| {
            // PID 必须属于准备阶段的目标。
            window.process_id == process.process_id
                // 进程创建代际必须仍完全一致。
                && window.process_creation_time == process.creation_time
        })
        // 收集供固定消息投递。
        .collect::<Vec<_>>();
    // 没有顶层窗口时优雅协议不可用。
    if candidates.is_empty() {
        // 禁止静默升级强制终止。
        return Err(ProcessTerminationFailure::Unsupported);
    }
    // 统计 Windows 已接受的固定关闭请求。
    let mut accepted = 0_usize;
    // 记录是否观察到明确权限拒绝。
    let mut denied = false;
    // 记录是否观察到其他投递失败。
    let mut failed = false;
    // 逐个处理同一进程当前顶层窗口。
    for window in candidates {
        // 写前再次核对 HWND、PID 与进程创建代际。
        if !same_window_identity(window) {
            // 过期窗口不接收任何消息。
            continue;
        }
        // 清空最后错误以便区分 UIPI 拒绝。
        unsafe { SetLastError(ERROR_SUCCESS) };
        // 只投递固定 WM_CLOSE，不发送键鼠或软件专用消息。
        if unsafe {
            PostMessageW(
                // 恢复当前 Adapter 私有 HWND。
                Some(native_window(window)),
                // 固定系统关闭请求。
                WM_CLOSE,
                // 不携带自定义参数。
                Default::default(),
                // 不携带自定义参数。
                Default::default(),
            )
        }
        .is_ok()
        {
            // 至少一个平台接受事实已经建立。
            accepted += 1;
            // 继续请求同一进程的其他顶层窗口自行关闭。
            continue;
        }
        // 紧邻失败读取线程错误。
        let error = unsafe { GetLastError() };
        // 明确访问拒绝单独分类。
        if error == ERROR_ACCESS_DENIED {
            // 记录权限拒绝。
            denied = true;
        } else {
            // 记录其他投递失败。
            failed = true;
        }
    }
    // 任一固定请求已排队就进入同一句柄最终状态等待。
    if accepted > 0 {
        // 返回已接受事实。
        return Ok(());
    }
    // 全部候选均被权限拒绝时返回权限失败。
    if denied {
        // 不泄漏具体窗口或原生错误。
        return Err(ProcessTerminationFailure::PermissionDenied);
    }
    // 其他投递失败保持通用平台错误。
    if failed {
        // 返回未接受的确定性失败。
        return Err(ProcessTerminationFailure::OperationFailed);
    }
    // 所有候选在写前过期表示进程窗口协议不再可用。
    Err(ProcessTerminationFailure::Stale)
}

// 对精确进程句柄提交固定强制终止。
pub(crate) fn dispatch_force(
    // 接收已经请求 PROCESS_TERMINATE 的私有句柄。
    process: &PreparedProcess,
) -> Result<(), ProcessTerminationFailure> {
    // dispatch 前再次确认同一进程仍在运行。
    if exited(process)? {
        // 写前退出保持 stale。
        return Err(ProcessTerminationFailure::Stale);
    }
    // 只使用内部固定非零退出码提交不可回滚终止。
    unsafe { TerminateProcess(process.handle, TOOL_TERMINATED_EXIT_CODE) }.map_err(|error| {
        // 明确访问拒绝映射权限边界。
        if permission_denied(&error) {
            // 返回稳定权限错误。
            ProcessTerminationFailure::PermissionDenied
        } else {
            // 其他未接受失败保持通用分类。
            ProcessTerminationFailure::OperationFailed
        }
    })?;
    // 返回内核已接受终止事实。
    Ok(())
}
