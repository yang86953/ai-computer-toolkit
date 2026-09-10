use std::collections::HashMap;

use serde::Serialize;
use windows::{
    Win32::{
        // 导入只读进程时间查询所需的句柄与 FILETIME 类型。
        Foundation::{CloseHandle, E_ACCESSDENIED, FILETIME, HANDLE, HWND, LPARAM, WPARAM},
        // 导入只读 token 完整性查询 API；只公开相对关系，不公开 RID。
        Security::{
            GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation,
            TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenIntegrityLevel,
        },
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            // 导入当前进程的 Windows session 查询 API。
            RemoteDesktop::ProcessIdToSessionId,
            // 导入最小权限的进程创建时间查询 API。
            Threading::{
                GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess,
                OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
            },
            // 导入当前 Windows 用户名查询 API。
            WindowsProgramming::GetUserNameW,
        },
        UI::WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, GetClassNameW, GetForegroundWindow,
            GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
            SMTO_ABORTIFHUNG, SMTO_BLOCK, SendMessageTimeoutW, WM_SETTEXT,
        },
    },
    core::{BOOL, PWSTR},
};

// 导入共享 Windows backend 私有封闭错误码。
use super::windows_error::WindowsErrorCode;

use crate::{
    // 导入与 C++ 对照实现一致的 s2 opaque ID Component。
    components::{
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
        window_target_identity,
    },
    domain::{AppControlError, AppResult, JsonMap},
};

// 表示当前快照读取进程元数据的结果分类。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ProcessMetadataAccess {
    // 表示创建时间与 token 元数据可读。
    Available,
    // 表示 Windows 明确拒绝最小查询权限。
    PermissionBlocked,
    // 表示进程退出或元数据暂不可用。
    Unavailable,
}

// 提供稳定公开枚举文本。
impl ProcessMetadataAccess {
    // 返回语言中立契约值。
    pub const fn as_str(self) -> &'static str {
        // 映射封闭访问分类。
        match self {
            // 输出可用状态。
            Self::Available => "available",
            // 输出权限阻塞状态。
            Self::PermissionBlocked => "permission-blocked",
            // 输出不可用状态。
            Self::Unavailable => "unavailable",
        }
    }
}

// 表示目标进程与当前工具进程的完整性相对关系。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum IntegrityRelation {
    // 表示目标完整性更低。
    Lower,
    // 表示两者完整性相同。
    Same,
    // 表示目标完整性更高。
    Higher,
    // 表示任一侧完整性不可读。
    Unknown,
}

// 提供稳定公开枚举文本。
impl IntegrityRelation {
    // 返回语言中立契约值。
    pub const fn as_str(self) -> &'static str {
        // 映射封闭相对关系。
        match self {
            // 输出较低关系。
            Self::Lower => "lower",
            // 输出相同关系。
            Self::Same => "same",
            // 输出较高关系。
            Self::Higher => "higher",
            // 输出未知关系。
            Self::Unknown => "unknown",
        }
    }
}

// 保存一次只读进程快照中的公开观察事实与私有重新解析身份。
#[derive(Debug, Clone)]
pub struct ProcessRecord {
    // 保存 canonical s2:p 公共目标。
    pub session_id: String,
    // 保存公开安全的可执行文件名。
    pub process_name: String,
    // 标记目标身份是否绑定进程创建时间。
    pub identity_reliable: bool,
    // 保存元数据访问分类。
    pub metadata_access: ProcessMetadataAccess,
    // 保存相对完整性关系。
    pub integrity_relation: IntegrityRelation,
    // 保存当前可见有标题窗口的 canonical s2:w 关系。
    pub window_session_ids: Vec<String>,
    // 保存仅供当前进程重新发现使用的 PID，禁止序列化。
    pub(crate) process_id: u32,
    // 保存仅供抵抗 PID 复用的创建 FILETIME，禁止序列化。
    pub(crate) process_creation_time: u64,
}

// 保存进程枚举结果及快照是否完整。
pub struct ProcessInventory {
    // 保存已枚举记录。
    pub records: Vec<ProcessRecord>,
    // 标记枚举是否自然到达快照末尾。
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowRecord {
    // legacy session 只供当前进程兼容解析，禁止序列化。
    #[serde(skip)]
    pub session_id: String,
    // HWND 只供平台调用，禁止序列化。
    #[serde(skip)]
    pub hwnd: isize,
    pub title: String,
    // Win32 class 只供 provider 识别，禁止序列化。
    #[serde(skip)]
    pub class_name: String,
    // PID 只供当前快照关联，禁止序列化。
    #[serde(skip)]
    pub process_id: u32,
    pub process_name: Option<String>,
    pub visible: bool,
    // 该字段仅参与重新发现身份，禁止进入任何 JSON 边界。
    #[serde(skip)]
    // 保存 Windows FILETIME 形式的进程创建时间。
    pub process_creation_time: u64,
}

// 按 C++ ProcessBackend 的字段顺序生成主机目标。
pub fn opaque_host_session_id(windows_session_id: u32, user_name: &str) -> String {
    // 组合十进制 Windows session ID 与 UTF-8 用户名。
    let identity = format!("{windows_session_id}:{user_name}");
    // 仅返回 canonical s2:h 指纹，禁止公开私有身份片段。
    OpaqueTargetId::new(OpaqueTargetKind::Host, &identity).to_string()
}

// 从当前 Windows 登录会话实时生成主机目标。
pub fn current_host_session_id() -> String {
    // 与 C++ 一致地以 0 作为 session 查询失败哨兵。
    let mut windows_session_id = 0_u32;
    // 读取当前进程 ID，不持有或公开原生进程句柄。
    let process_id = unsafe { GetCurrentProcessId() };
    // 查询失败时保留 0 哨兵，保持跨实现身份输入一致。
    let _ = unsafe { ProcessIdToSessionId(process_id, &mut windows_session_id) };
    // 使用与 C++ 相同的 257 个 UTF-16 code unit 缓冲区。
    let mut user = [0_u16; 257];
    // GetUserNameW 输入容量并在成功时返回包含 NUL 的长度。
    let mut user_length = u32::try_from(user.len()).unwrap_or(0);
    // 只读查询当前 Windows 用户名。
    let user_result = unsafe { GetUserNameW(Some(PWSTR(user.as_mut_ptr())), &mut user_length) };
    // 仅在成功且长度包含至少一个字符和终止符时解码。
    let user_name = if user_result.is_ok() && user_length > 0 {
        // 删除末尾 NUL 后按 UTF-16 解码为 Rust UTF-8 String。
        String::from_utf16_lossy(
            // 将 Win32 返回长度安全转换为切片边界。
            &user[..usize::try_from(user_length - 1).unwrap_or(0)],
        )
    // 查询失败时与 C++ 一致地使用空用户名。
    } else {
        // 返回身份生成所需的空字符串哨兵。
        String::new()
        // 结束用户名查询结果分支。
    };
    // 从当前实时事实生成 canonical s2:h。
    opaque_host_session_id(windows_session_id, &user_name)
}

// 按 C++ DiscoveryBackend 的字段顺序生成精确窗口目标。
pub fn opaque_window_session_id(record: &WindowRecord) -> String {
    // 委托窄 Component 保持既有三字段字节布局与诚实保证边界。
    let identity = window_target_identity::material(
        // 传入私有进程 ID。
        record.process_id,
        // 传入完整当前窗口 token，不公开其数值。
        record.hwnd as usize,
        // 传入进程创建代际；它不是窗口创建时间。
        record.process_creation_time,
    );
    // 仅把 s2 指纹返回给公共调用方。
    OpaqueTargetId::new(OpaqueTargetKind::Window, &identity).to_string()
}

// 从标准 Edit 的私有事实生成与 C++ 对照一致的 canonical s2:c 身份。
pub(crate) fn opaque_control_session_id(record: &WindowRecord) -> String {
    // HWND 按 uintptr_t 十进制表示，保持跨语言输入字节一致。
    let native_control = record.hwnd as usize;
    // 组合 PID、当前控件 token 与进程代际；不承诺同进程 token 回收证明。
    let identity = format!(
        // 固定 C++ StandardEditBackend 使用的三段十进制布局。
        "{}:{}:{}",
        // 输出私有进程 ID。
        record.process_id,
        // 输出私有控件句柄值。
        native_control,
        // 输出私有进程创建 FILETIME。
        record.process_creation_time,
    );
    // 公共边界仅返回不可逆 s2 指纹。
    OpaqueTargetId::new(OpaqueTargetKind::Control, &identity).to_string()
}

pub fn foreground_hwnd() -> isize {
    // 该调用仅读取前景句柄，不会改变窗口状态。
    unsafe { GetForegroundWindow().0 as isize }
}

// 保存一次最小权限进程观察的内部结果。
struct ProcessObservation {
    // 保存进程创建 FILETIME。
    creation_time: u64,
    // 标记创建时间是否可靠。
    identity_reliable: bool,
    // 保存元数据访问分类。
    metadata_access: ProcessMetadataAccess,
    // 保存相对完整性关系。
    integrity_relation: IntegrityRelation,
}

// 读取 token 的完整性 RID；RID 只在函数内比较，绝不进入公共结果。
fn integrity_rid(process: HANDLE) -> Option<u32> {
    // 保存需要由调用方关闭的 token handle。
    let mut token = HANDLE::default();
    // 只申请 TOKEN_QUERY，不调整权限或 token。
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.is_err() {
        // 无法读取时保守返回未知。
        return None;
    }
    // 首次查询只获取所需缓冲区长度。
    let mut required = 0_u32;
    // ERROR_INSUFFICIENT_BUFFER 是预期结果，只使用返回长度。
    let _ = unsafe { GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut required) };
    // 无长度表示 token 信息不可用。
    if required == 0 {
        // 关闭已取得的 token handle。
        let _ = unsafe { CloseHandle(token) };
        // 返回未知完整性。
        return None;
    }
    // 使用 usize 单元保证 TOKEN_MANDATORY_LABEL 所需对齐。
    let word_count = usize::try_from(required)
        // 将字节长度向上取整到 usize 单元。
        .ok()?
        // 执行无溢出的向上整除。
        .div_ceil(std::mem::size_of::<usize>());
    // 分配对齐且零初始化的缓冲区。
    let mut buffer = vec![0_usize; word_count];
    // 读取 token mandatory label。
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            Some(buffer.as_mut_ptr().cast()),
            required,
            &mut required,
        )
    };
    // 查询失败时关闭 token 并返回未知。
    if read.is_err() {
        // 关闭 token handle。
        let _ = unsafe { CloseHandle(token) };
        // 返回未知完整性。
        return None;
    }
    // 将已对齐缓冲区解释为 Windows mandatory label。
    let label = unsafe { &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    // 读取 SID 子权限数量指针。
    let count = unsafe { GetSidSubAuthorityCount(label.Label.Sid) };
    // 空指针表示 SID 无效。
    if count.is_null() {
        // 关闭 token handle。
        let _ = unsafe { CloseHandle(token) };
        // 返回未知完整性。
        return None;
    }
    // 复制子权限数量。
    let count = unsafe { *count };
    // mandatory label 必须至少有一个子权限。
    if count == 0 {
        // 关闭 token handle。
        let _ = unsafe { CloseHandle(token) };
        // 返回未知完整性。
        return None;
    }
    // 读取最后一个子权限即完整性 RID。
    let rid = unsafe { GetSidSubAuthority(label.Label.Sid, u32::from(count - 1)) };
    // 指针有效时复制 RID，避免公开或延长 SID 生命周期。
    let rid = if rid.is_null() {
        // 无效 SID 返回未知。
        None
    } else {
        // 只复制用于相对比较的数值。
        Some(unsafe { *rid })
    };
    // 关闭 token handle。
    let _ = unsafe { CloseHandle(token) };
    // 返回内部比较值。
    rid
}

// 使用最小进程查询权限读取生命周期与相对完整性。
fn observe_process(process_id: u32, current_integrity: Option<u32>) -> ProcessObservation {
    // 打开只读查询 handle。
    let process = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
    {
        // 保存成功取得的 handle。
        Ok(process) => process,
        // 明确区分权限拒绝与进程退出等其他缺口。
        Err(error) => {
            // 返回不可靠身份与结构化访问分类。
            return ProcessObservation {
                // 缺失创建时间使用 C++ 等价哨兵。
                creation_time: 0,
                // 未绑定创建时间的身份仅对当前快照 best effort。
                identity_reliable: false,
                // 只把明确的 ACCESS_DENIED 分类为权限阻塞。
                metadata_access: if error.code() == E_ACCESSDENIED {
                    // 输出权限阻塞分类。
                    ProcessMetadataAccess::PermissionBlocked
                } else {
                    // 其他失败视为暂不可用。
                    ProcessMetadataAccess::Unavailable
                },
                // 无 token 事实时相对关系未知。
                integrity_relation: IntegrityRelation::Unknown,
            };
        }
    };
    // 准备读取进程时间。
    let mut created = FILETIME::default();
    // 进程退出时间不公开。
    let mut exited = FILETIME::default();
    // 内核时间不公开。
    let mut kernel = FILETIME::default();
    // 用户时间不公开。
    let mut user = FILETIME::default();
    // 执行只读创建时间查询。
    let times =
        unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
    // 在同一 handle 上读取目标完整性。
    let target_integrity = integrity_rid(process);
    // 关闭进程 handle。
    let _ = unsafe { CloseHandle(process) };
    // 创建时间失败时不把 token 成功误报为完整元数据可用。
    if times.is_err() {
        // 返回暂不可用观察。
        return ProcessObservation {
            // 使用不可靠身份哨兵。
            creation_time: 0,
            // 标记 best effort 身份。
            identity_reliable: false,
            // 分类为元数据不可用。
            metadata_access: ProcessMetadataAccess::Unavailable,
            // 不使用孤立 token 结果。
            integrity_relation: IntegrityRelation::Unknown,
        };
    }
    // 仅在两侧 RID 都可读时计算相对关系。
    let integrity_relation = match (target_integrity, current_integrity) {
        // 目标较低。
        (Some(target), Some(current)) if target < current => IntegrityRelation::Lower,
        // 目标较高。
        (Some(target), Some(current)) if target > current => IntegrityRelation::Higher,
        // 两者相同。
        (Some(_), Some(_)) => IntegrityRelation::Same,
        // 任一侧不可读。
        _ => IntegrityRelation::Unknown,
    };
    // 返回完整观察。
    ProcessObservation {
        // 合并 FILETIME 高低位并保持 C++ 无符号布局。
        creation_time: (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
        // 成功读取创建时间时身份绑定进程生命周期。
        identity_reliable: true,
        // 标记元数据可用。
        metadata_access: ProcessMetadataAccess::Available,
        // 保存相对完整性。
        integrity_relation,
    }
}

// 按 C++ ProcessBackend 的 PID、创建时间、名称顺序生成进程目标。
pub(super) fn opaque_process_session_id(
    process_id: u32,
    creation_time: u64,
    process_name: &str,
) -> String {
    // 组合仅供散列的私有进程身份。
    let identity = format!("{process_id}:{creation_time}:{process_name}");
    // 只返回 canonical s2:p 指纹。
    OpaqueTargetId::new(OpaqueTargetKind::Process, &identity).to_string()
}

// 枚举有界进程观察清单并保留快照完整性。
pub(crate) fn enumerate_process_inventory(maximum_items: usize) -> AppResult<ProcessInventory> {
    // 保存已枚举记录。
    let mut records = Vec::new();
    // 创建只读 ToolHelp 快照。
    let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        // 保存有效快照 handle。
        Ok(snapshot) => snapshot,
        // 快照创建失败时返回显式不完整清单。
        Err(_) => {
            // 不以空清单冒充完整发现。
            return Ok(ProcessInventory {
                // 当前没有可信记录。
                records,
                // 标记来源不完整。
                complete: false,
            });
        }
    };
    // 初始化 ToolHelp 结构大小。
    let mut entry = PROCESSENTRY32W {
        // 转换结构长度并保持结构化溢出错误。
        dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).map_err(|_| {
            // 返回稳定快照错误。
            WindowsErrorCode::ProcessSnapshotFailed.error("PROCESSENTRY32W 长度溢出。")
        })?,
        // 其余字段清零。
        ..Default::default()
    };
    // 读取当前工具进程完整性用于相对比较。
    let current_integrity = integrity_rid(unsafe { GetCurrentProcess() });
    // 读取首条进程记录。
    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_err() {
        // 关闭快照 handle。
        let _ = unsafe { CloseHandle(snapshot) };
        // 空快照已自然结束，按 C++ 语义视为完整。
        return Ok(ProcessInventory {
            // 返回空记录。
            records,
            // 标记自然结束。
            complete: true,
        });
    }
    // 默认在达到上限前不完整。
    let mut complete = false;
    // 逐条处理快照记录。
    loop {
        // 读取公开安全的进程名。
        let process_name = utf16_to_string(&entry.szExeFile);
        // 读取生命周期与相对完整性元数据。
        let observation = observe_process(entry.th32ProcessID, current_integrity);
        // 保存公共观察与私有重新解析事实。
        records.push(ProcessRecord {
            // 生成 C++ 等价 canonical s2:p。
            session_id: opaque_process_session_id(
                entry.th32ProcessID,
                observation.creation_time,
                &process_name,
            ),
            // 保存公开进程名。
            process_name,
            // 保存身份可靠性。
            identity_reliable: observation.identity_reliable,
            // 保存元数据访问分类。
            metadata_access: observation.metadata_access,
            // 保存相对完整性。
            integrity_relation: observation.integrity_relation,
            // 窗口关系由上层同一观察流程附加。
            window_session_ids: Vec::new(),
            // 保存私有 PID。
            process_id: entry.th32ProcessID,
            // 保存私有创建时间。
            process_creation_time: observation.creation_time,
        });
        // 继续读取下一条记录。
        let next = unsafe { Process32NextW(snapshot, &mut entry) };
        // 快照自然结束时标记完整。
        if next.is_err() {
            // 记录完整枚举状态。
            complete = true;
            // 退出枚举。
            break;
        }
        // 达到调用方边界且仍有后续记录时保持不完整。
        if records.len() >= maximum_items {
            // 退出有界枚举。
            break;
        }
    }
    // 关闭 ToolHelp 快照。
    let _ = unsafe { CloseHandle(snapshot) };
    // 返回有界清单与完整性。
    Ok(ProcessInventory { records, complete })
}

// 为既有内部调用方保留完整进程记录便利入口。
pub fn enumerate_processes() -> AppResult<Vec<ProcessRecord>> {
    // 使用最大上限枚举到快照自然结束。
    Ok(enumerate_process_inventory(usize::MAX)?.records)
}

pub fn enumerate_windows() -> AppResult<Vec<WindowRecord>> {
    let names = enumerate_processes()?
        .into_iter()
        .map(|process| (process.process_id, process.process_name))
        .collect::<HashMap<_, _>>();
    let mut records: Vec<WindowRecord> = Vec::new();
    let pointer = (&mut records as *mut Vec<WindowRecord>) as isize;
    // 回调仅在 EnumWindows 同步调用期间解引用该局部 Vec 指针。
    unsafe { EnumWindows(Some(enum_window), LPARAM(pointer)) }
        .map_err(windows_error(WindowsErrorCode::WindowEnumerationFailed))?;
    for record in &mut records {
        record.process_name = names.get(&record.process_id).cloned();
    }
    Ok(records)
}

pub fn filter_windows(target: &JsonMap) -> AppResult<Vec<WindowRecord>> {
    let records = enumerate_windows()?;
    Ok(records
        .into_iter()
        .filter(|record| matches_target(record, target))
        .collect())
}

pub fn filter_standard_edit_controls(target: &JsonMap) -> AppResult<Vec<WindowRecord>> {
    filter_child_controls(target, &["Edit"])
}

pub fn standard_edit_children(parent: &WindowRecord) -> AppResult<Vec<WindowRecord>> {
    let mut controls = enumerate_child_controls(parent)?;
    controls.retain(|control| control.class_name == "Edit");
    Ok(controls)
}

pub fn set_standard_edit_text(control: &WindowRecord, text: &str) -> AppResult<()> {
    let hwnd = HWND(control.hwnd as *mut std::ffi::c_void);
    if class_name(hwnd) != "Edit" {
        // 拒绝未认证控件类。
        return Err(WindowsErrorCode::BackgroundOperationUnavailable
            .error("set-text 仅认证标准 Edit 控件，拒绝未知窗口类。"));
    }
    let mut utf16 = text.encode_utf16().collect::<Vec<_>>();
    utf16.push(0);
    let mut message_result = 0_usize;
    // WM_SETTEXT 是系统定义消息；此函数只接受字符串内容和固定的 2 秒超时。
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_SETTEXT,
            WPARAM(0),
            LPARAM(utf16.as_ptr() as isize),
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            2_000,
            Some(&mut message_result),
        )
    };
    if sent.0 == 0 {
        // 报告目标挂起或不可用。
        return Err(WindowsErrorCode::TargetHungOrUnavailable
            .error("Edit 控件未在 2000ms 内处理 WM_SETTEXT。"));
    }
    Ok(())
}

pub fn filter_child_controls(
    target: &JsonMap,
    allowed_classes: &[&str],
) -> AppResult<Vec<WindowRecord>> {
    let process_names = enumerate_processes()?
        .into_iter()
        .map(|process| (process.process_id, process.process_name))
        .collect::<HashMap<_, _>>();
    let mut controls: Vec<WindowRecord> = Vec::new();
    for parent in enumerate_windows()? {
        let pointer = (&raw mut controls) as isize;
        // EnumChildWindows 会同步枚举当前父窗口及其后代控件，不会激活任何窗口。
        // 没有子窗口时 API 会返回 FALSE；这不是扫描失败。
        let _ = unsafe {
            EnumChildWindows(
                Some(HWND(parent.hwnd as *mut std::ffi::c_void)),
                Some(enum_child_control),
                LPARAM(pointer),
            )
        };
    }
    for control in &mut controls {
        control.process_name = process_names.get(&control.process_id).cloned();
    }
    controls.retain(|control| {
        allowed_classes.contains(&control.class_name.as_str()) && matches_target(control, target)
    });
    Ok(controls)
}

fn enumerate_child_controls(parent: &WindowRecord) -> AppResult<Vec<WindowRecord>> {
    let process_names = enumerate_processes()?
        .into_iter()
        .map(|process| (process.process_id, process.process_name))
        .collect::<HashMap<_, _>>();
    let mut controls: Vec<WindowRecord> = Vec::new();
    let pointer = (&mut controls as *mut Vec<WindowRecord>) as isize;
    // 该回调仅在 EnumChildWindows 同步调用期间访问局部集合。
    let _ = unsafe {
        EnumChildWindows(
            Some(HWND(parent.hwnd as *mut std::ffi::c_void)),
            Some(enum_child_control),
            LPARAM(pointer),
        )
    };
    for control in &mut controls {
        control.process_name = process_names.get(&control.process_id).cloned();
    }
    Ok(controls)
}

pub fn unique_window(target: &JsonMap) -> AppResult<WindowRecord> {
    let matches = filter_windows(target)?;
    match matches.len() {
        // 零命中映射为目标不存在。
        0 => Err(WindowsErrorCode::TargetNotFound
            .error("没有匹配的窗口；请先执行 sessions window 或 sessions uia。")),
        // 唯一命中返回目标记录。
        1 => Ok(matches.into_iter().next().ok_or_else(|| {
            // 防御性地报告目标在选择期间失效。
            WindowsErrorCode::TargetNotFound.error("窗口查询结果意外为空。")
        })?),
        // 多命中映射为歧义目标。
        _ => Err(WindowsErrorCode::AmbiguousTarget
            .error("窗口选择器匹配多个结果；请使用精确 sessionId。")),
    }
}

pub fn unique_standard_edit_control(target: &JsonMap) -> AppResult<WindowRecord> {
    unique_child_control(target, &["Edit"], "标准 Edit 控件")
}

pub fn unique_child_control(
    target: &JsonMap,
    allowed_classes: &[&str],
    target_name: &str,
) -> AppResult<WindowRecord> {
    let matches = filter_child_controls(target, allowed_classes)?;
    match matches.len() {
        // 零命中映射为目标不存在。
        0 => Err(WindowsErrorCode::TargetNotFound
            .error(format!("没有匹配的{target_name}；请先执行 sessions。"))),
        // 唯一命中返回目标记录。
        1 => Ok(matches.into_iter().next().ok_or_else(|| {
            // 防御性地报告目标在选择期间失效。
            WindowsErrorCode::TargetNotFound.error(format!("{target_name} 查询结果意外为空。"))
        })?),
        // 多命中映射为歧义目标。
        _ => Err(WindowsErrorCode::AmbiguousTarget.error(format!(
            "{target_name} 选择器匹配多个结果；请使用精确 sessionId。"
        ))),
    }
}

pub fn class_name(hwnd: HWND) -> String {
    let mut buffer = vec![0_u16; 512];
    let written = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if written <= 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&buffer[..usize::try_from(written).unwrap_or_default()])
    }
}

fn matches_target(record: &WindowRecord, target: &JsonMap) -> bool {
    let session_matches = target
        .get("sessionId")
        .and_then(|value| value.as_str())
        .is_none_or(|value| {
            // 正式新核心接受 canonical s2:w。
            value == opaque_window_session_id(record)
                // 旧 direct surface 暂时接受 legacy window session。
                || value == record.session_id
                // 保留旧 UIA 兼容目标输入。
                || value == format!("uia:window:{}", record.hwnd)
                // 保留旧 Win32 control 兼容目标输入。
                || value == format!("win32-control:window:{}", record.hwnd)
        });
    let hwnd_matches = target
        .get("hwnd")
        .and_then(|value| value.as_i64())
        .is_none_or(|value| value == i64::try_from(record.hwnd).unwrap_or_default());
    let title_matches = target
        .get("title")
        .and_then(|value| value.as_str())
        .is_none_or(|value| record.title.contains(value));
    let process_matches = target
        .get("processId")
        .and_then(|value| value.as_u64())
        .is_none_or(|value| value == u64::from(record.process_id));
    let name_matches = target
        .get("processName")
        .and_then(|value| value.as_str())
        .is_none_or(|value| {
            record
                .process_name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(value))
        });
    session_matches && hwnd_matches && title_matches && process_matches && name_matches
}

unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // lparam 来自 enumerate_windows 的有效 Vec 指针，且同步回调不会越过该函数生命周期。
    let records = unsafe { &mut *(lparam.0 as *mut Vec<WindowRecord>) };
    let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
    let mut process_id = 0_u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    records.push(WindowRecord {
        session_id: format!("window:{}", hwnd.0 as isize),
        hwnd: hwnd.0 as isize,
        title: window_title(hwnd),
        class_name: class_name(hwnd),
        process_id,
        process_name: None,
        visible,
        // 在发现快照内绑定当前进程实例的创建时间。
        process_creation_time: process_creation_time(process_id),
    });
    BOOL(1)
}

// 枚举子控件时不读取窗口文本，避免挂起目标阻塞写前发现。
unsafe extern "system" fn enum_child_control(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // lparam 来自同步 EnumChildWindows 调用期间的有效 Vec 指针。
    let records = unsafe { &mut *(lparam.0 as *mut Vec<WindowRecord>) };
    // 只读查询当前可见性，不激活控件。
    let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
    // 初始化私有进程 ID。
    let mut process_id = 0_u32;
    // 只读查询控件所属进程。
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    // 保存不读取 Value/Text 的最小控件事实。
    records.push(WindowRecord {
        // legacy 私有 session 仅供当前 Rust 兼容解析。
        session_id: format!("window:{}", hwnd.0 as isize),
        // 保存私有控件句柄，禁止序列化到正式边界。
        hwnd: hwnd.0 as isize,
        // 子控件发现不读取可能阻塞的窗口文本。
        title: String::new(),
        // 只读取系统类名以识别封闭 allowlist。
        class_name: class_name(hwnd),
        // 保存私有进程 ID 供身份生成与权限关联。
        process_id,
        // 进程名由调用方从同一只读进程清单补齐。
        process_name: None,
        // 保存可见性事实。
        visible,
        // 绑定当前进程代际以区分 PID 回收；不伪造控件创建代际。
        process_creation_time: process_creation_time(process_id),
    });
    // 继续枚举其余子控件。
    BOOL(1)
}

// 使用查询受限权限读取进程创建时间，不请求写入或提权。
pub(super) fn process_creation_time(process_id: u32) -> u64 {
    // 只申请公开的最小进程查询权限。
    let Ok(process) =
        (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) })
    // 权限不足或进程已退出时以 0 保持与 C++ fail-closed 身份一致。
    else {
        // 返回不可用创建时间哨兵。
        return 0;
        // 结束进程打开失败分支。
    };
    // 保存进程创建 FILETIME。
    let mut created = FILETIME::default();
    // 保存退出 FILETIME 占位。
    let mut exited = FILETIME::default();
    // 保存内核时间占位。
    let mut kernel = FILETIME::default();
    // 保存用户时间占位。
    let mut user = FILETIME::default();
    // 在句柄关闭前完成只读时间查询。
    let status =
        unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
    // 无论查询结果如何都关闭本次私有进程句柄。
    let _ = unsafe { CloseHandle(process) };
    // 查询失败时保持与 C++ 实现相同的 0 哨兵。
    if status.is_err() {
        // 返回不可用创建时间。
        return 0;
        // 结束查询失败分支。
    }
    // 组合 FILETIME 的高低 32 位为无符号 64 位值。
    (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)
}

fn window_title(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) }.max(0);
    let mut buffer = vec![0_u16; usize::try_from(length).unwrap_or_default() + 1];
    let written = unsafe { GetWindowTextW(hwnd, &mut buffer) }.max(0);
    String::from_utf16_lossy(&buffer[..usize::try_from(written).unwrap_or_default()])
}

fn utf16_to_string(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

fn windows_error(code: WindowsErrorCode) -> impl FnOnce(windows::core::Error) -> AppControlError {
    move |error| code.error(error.to_string())
}

// 仅验证私有窗口身份格式与序列化边界。
#[cfg(test)]
// 声明 Windows backend 身份测试集合。
mod tests {
    // 导入当前模块的私有构造能力。
    use super::*;

    // 验证 Rust 的 sessionId:userName 字节布局与 C++ golden 相同。
    #[test]
    // 固定公开结果只包含 canonical s2:h 指纹。
    fn host_s2_identity_matches_cpp_layout() {
        // 使用不依赖真实登录会话的稳定私有身份夹具。
        let target = opaque_host_session_id(12, "fixture-user");
        // 断言与独立 FNV-1a 计算的 C++ s2:h golden 一致。
        assert_eq!(target, "s2:h:43cd964f04d562b7");
        // 断言公共目标不包含用户名。
        assert!(!target.contains("fixture-user"));
        // 断言公共目标不包含原始 session ID 分隔形状。
        assert!(!target.contains("12:"));
        // 结束主机身份 golden 测试。
    }

    // 验证 Rust 的 PID:HWND:creationTime 字节布局与 C++ golden 相同。
    #[test]
    // 同时验证创建时间不进入 JSON。
    fn window_s2_identity_matches_cpp_layout_without_leak() -> Result<(), Box<dyn std::error::Error>>
    {
        // 构造不接触真实桌面的稳定窗口记录。
        let record = WindowRecord {
            // 保留旧直接兼容 session。
            session_id: "window:100".to_owned(),
            // 使用与 golden 私有身份一致的 HWND。
            hwnd: 100,
            // 提供无敏感含义的标题。
            title: "Fixture".to_owned(),
            // 提供旧兼容 className。
            class_name: "FixtureClass".to_owned(),
            // 使用与 golden 私有身份一致的 PID。
            process_id: 42,
            // 提供公开安全进程名。
            process_name: Some("fixture.exe".to_owned()),
            // 标记该夹具窗口可见。
            visible: true,
            // 使用与 golden 私有身份一致的创建时间。
            process_creation_time: 123,
            // 结束稳定窗口记录构造。
        };
        // 断言与独立 FNV-1a 计算的 C++ s2:w golden 一致。
        assert_eq!(opaque_window_session_id(&record), "s2:w:7b416ac5554aad64");
        // 序列化旧兼容窗口记录。
        let serialized = serde_json::to_string(&record)?;
        // 断言私有创建时间不会进入 JSON。
        assert!(!serialized.contains("processCreationTime"));
        // 断言私有创建时间值不会通过字段名泄漏。
        assert!(!serialized.contains("creationTime"));
        // 报告测试成功。
        Ok(())
        // 结束窗口身份与泄漏测试。
    }
    // 结束 Windows backend 测试集合。
}
