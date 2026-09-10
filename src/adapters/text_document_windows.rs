//! 固定系统 Notepad 的无激活启动与回滚边界。

// 导入路径、宽字符串与短等待工具。
use std::{os::windows::ffi::OsStrExt, path::Path, thread, time::Duration};

// 导入最小 Windows 进程、Job 与前台 API。
use windows::{
    // 导入 Win32 命名空间。
    Win32::{
        // 导入 handle 关闭与等待状态。
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        // 导入 Job 绑定与回滚接口。
        System::{
            // Job 在进程恢复前取得其整棵进程树的回滚所有权。
            JobObjects::{AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject},
            // 导入挂起创建、恢复与等待接口。
            Threading::{
                // 使用 Unicode 环境并先挂起主线程。
                CREATE_SUSPENDED,
                CREATE_UNICODE_ENVIRONMENT,
                // 创建固定进程。
                CreateProcessW,
                // 接收原生进程信息。
                PROCESS_INFORMATION,
                // 恢复主线程。
                ResumeThread,
                // 请求首次显示不激活。
                STARTF_USESHOWWINDOW,
                // 配置启动信息。
                STARTUPINFOW,
                // 绑定前异常时终止仍挂起进程。
                TerminateProcess,
                // 有界等待回滚完成。
                WaitForSingleObject,
            },
        },
        // 只读取前景并使用固定 show mode。
        UI::WindowsAndMessaging::{GetForegroundWindow, SW_SHOWNOACTIVATE},
    },
    // 导入宽字符串指针。
    core::{PCWSTR, PWSTR},
};

// 引入 Text Document Windows 启动 Component 私有封闭错误码实现。
#[path = "text_document_windows_error.rs"]
mod error_code;

// 导入 Text Document Windows 启动 Component 私有封闭错误码。
use error_code::TextDocumentLaunchErrorCode;

// 导入统一结果边界。
use crate::domain::AppResult;

// 保存启动后允许进入公共映射的安全事实。
pub(crate) struct LaunchEvidence {
    // 仅供 legacy 兼容结果保留启动器进程 ID。
    pub(crate) process_id: u32,
    // 固定成功路径必须保持前台不变。
    pub(crate) foreground_unchanged: bool,
}

// 让私有 Win32 handle 在所有退出路径确定性关闭。
struct OwnedHandle(HANDLE);

// 提供私有 handle 所有权操作。
impl OwnedHandle {
    // 接管一个有效 handle。
    fn new(handle: HANDLE, operation: &str) -> AppResult<Self> {
        // 拒绝空值与 INVALID_HANDLE_VALUE。
        if handle.is_invalid() {
            // 返回不泄漏 handle 的稳定错误。
            return Err(TextDocumentLaunchErrorCode::NotepadStartFailed.error(
                // 标明内部失败步骤。
                format!("{operation} returned an invalid handle."),
            ));
        }
        // 接管有效 handle。
        Ok(Self(handle))
    }

    // 仅向本 Adapter 内部 Windows 调用提供原始值。
    const fn raw(&self) -> HANDLE {
        // 原始 handle 不越过当前模块。
        self.0
    }
}

// 作用域结束时关闭单一所有权 handle。
impl Drop for OwnedHandle {
    // 回收内核资源。
    fn drop(&mut self) {
        // 忽略关闭阶段错误，避免覆盖原始领域结果。
        let _ = unsafe { CloseHandle(self.0) };
    }
}

// 把受控路径编码成 nul 结尾 UTF-16。
fn wide_path(path: &Path) -> Vec<u16> {
    // Windows 路径由 OS 字符串直接编码，不经过有损 UTF-8。
    path.as_os_str()
        // 生成 UTF-16 code units。
        .encode_wide()
        // 添加终止 nul。
        .chain(std::iter::once(0))
        // 建立连续缓冲区。
        .collect()
}

// 为受控路径生成 Windows 引号参数。
fn quoted_path(path: &Path) -> AppResult<String> {
    // 本 capability 不接受含引号的路径，避免命令行歧义。
    let value = path.to_string_lossy();
    // 明确拒绝无法安全表示的引号。
    if value.contains('"') {
        // 返回稳定启动错误。
        return Err(TextDocumentLaunchErrorCode::NotepadStartFailed.error(
            // 不回显本机路径。
            "The controlled Notepad path contains an unsupported quote.",
        ));
    }
    // 用 Windows 标准双引号包围完整路径。
    Ok(format!("\"{value}\""))
}

// 启动固定 Notepad，并在前台变化时终止本次进程树。
pub(crate) fn launch_no_activate(executable: &Path, artifact: &Path) -> AppResult<LaunchEvidence> {
    // 在创建任何进程前准备无歧义命令行。
    let mut command_line = format!(
        // argv[0] 与唯一 artifact 均使用完整引号。
        "{} {}",
        // 固定 executable。
        quoted_path(executable)?,
        // 工具本次新建的 artifact。
        quoted_path(artifact)?,
    )
    // CreateProcessW 需要可修改的 nul 结尾缓冲区。
    .encode_utf16()
    // 添加终止 nul。
    .chain(std::iter::once(0))
    // 建立连续缓冲区。
    .collect::<Vec<_>>();
    // 单独保留 application name，禁止 PATH 或 shell 解析。
    let executable_wide = wide_path(executable);
    // 在任何启动影响前记录前景。
    let foreground_before = unsafe { GetForegroundWindow() };
    // 先创建临时 Job 作为回滚边界。
    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        // 映射为稳定启动错误。
        .map_err(|_| {
            // 不暴露 Windows 错误或 handle。
            TextDocumentLaunchErrorCode::NotepadStartFailed.error(
                // 描述安全边界未建立。
                "The Notepad rollback boundary could not be created.",
            )
        })?;
    // 接管 Job handle。
    let job = OwnedHandle::new(job, "CreateJobObjectW")?;
    // 配置首次 show 请求为不激活。
    let startup = STARTUPINFOW {
        // 设置结构长度。
        cb: u32::try_from(std::mem::size_of::<STARTUPINFOW>()).map_err(|_| {
            // 返回稳定结构错误。
            TextDocumentLaunchErrorCode::NotepadStartFailed.error("STARTUPINFOW size overflowed.")
        })?,
        // 启用 wShowWindow 字段。
        dwFlags: STARTF_USESHOWWINDOW,
        // 固定无激活显示。
        wShowWindow: u16::try_from(SW_SHOWNOACTIVATE.0).map_err(|_| {
            // 固定常量转换失败时关闭启动路径。
            TextDocumentLaunchErrorCode::NotepadStartFailed
                // 构造固定无激活模式错误。
                .error("The no-activate mode is invalid.")
        })?,
        // 其余启动字段保持零值。
        ..Default::default()
    };
    // 接收进程与线程 handle。
    let mut process = PROCESS_INFORMATION::default();
    // 创建挂起的固定系统进程。
    unsafe {
        CreateProcessW(
            // 固定 application name，不依赖命令行 argv[0] 解析。
            PCWSTR(executable_wide.as_ptr()),
            // 传入含 artifact 的可修改命令行。
            Some(PWSTR(command_line.as_mut_ptr())),
            // 不设置进程安全描述符。
            None,
            // 不设置线程安全描述符。
            None,
            // 不继承调用者 handle。
            false,
            // 绑定 Job 前保持挂起并继承 Unicode 环境。
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            // 继承当前环境。
            None,
            // 继承当前目录。
            PCWSTR::null(),
            // 传入无激活启动信息。
            &startup,
            // 接收进程信息。
            &mut process,
        )
    }
    // 统一映射创建失败。
    .map_err(|_| {
        // 不公开系统路径或原生错误。
        TextDocumentLaunchErrorCode::NotepadStartFailed.error(
            // 固定公开消息。
            "The system Notepad process could not be started.",
        )
    })?;
    // 接管进程 handle。
    let process_handle = OwnedHandle::new(process.hProcess, "CreateProcessW(process)")?;
    // 接管主线程 handle；异常时必须先终止挂起进程。
    let thread_handle = match OwnedHandle::new(process.hThread, "CreateProcessW(thread)") {
        // 保存有效线程 handle。
        Ok(handle) => handle,
        // 无效线程 handle 时回收进程。
        Err(error) => {
            // 进程尚未进入 Job，直接终止。
            let _ = unsafe { TerminateProcess(process_handle.raw(), 1) };
            // 有界等待进程退出。
            let _ = unsafe { WaitForSingleObject(process_handle.raw(), 1_000) };
            // 返回原始稳定错误。
            return Err(error);
        }
    };
    // 在首条应用指令前把进程绑定到回滚 Job。
    if unsafe { AssignProcessToJobObject(job.raw(), process_handle.raw()) }.is_err() {
        // Job 未取得所有权时直接终止挂起进程。
        let _ = unsafe { TerminateProcess(process_handle.raw(), 1) };
        // 有界等待回收。
        let _ = unsafe { WaitForSingleObject(process_handle.raw(), 1_000) };
        // 返回稳定边界失败。
        return Err(TextDocumentLaunchErrorCode::NotepadStartFailed.error(
            // 不暴露 Job 或 handle 细节。
            "The Notepad process could not enter its rollback boundary.",
        ));
    }
    // 恢复主线程并检查失败 sentinel。
    if unsafe { ResumeThread(thread_handle.raw()) } == u32::MAX {
        // 终止 Job 内本次启动树。
        let _ = unsafe { TerminateJobObject(job.raw(), 1) };
        // 等待进程终止。
        let _ = unsafe { WaitForSingleObject(process_handle.raw(), 1_000) };
        // 返回稳定恢复错误。
        return Err(TextDocumentLaunchErrorCode::NotepadStartFailed.error(
            // 表明进程从未离开受控启动边界。
            "The Notepad process could not resume inside its rollback boundary.",
        ));
    }
    // 等待固定短窗口，让首次显示请求与单实例移交完成。
    thread::sleep(Duration::from_millis(250));
    // 只比较前景 handle，不激活或读取窗口内容。
    let foreground_unchanged = unsafe { GetForegroundWindow() } == foreground_before;
    // 前景变化时整个本次 Job 树必须回滚。
    if !foreground_unchanged {
        // 终止本次启动树。
        let _ = unsafe { TerminateJobObject(job.raw(), 1) };
        // 有界等待进程对象进入终止状态。
        let wait = unsafe { WaitForSingleObject(process_handle.raw(), 1_000) };
        // 即使等待状态异常，也不把 artifact 留给上层当成功。
        let _ = wait == WAIT_OBJECT_0;
        // 返回结构化主机干扰错误。
        return Err(TextDocumentLaunchErrorCode::HostInterferenceDetected.error(
            // 明确本次启动已回滚。
            "Notepad changed the foreground target; the launch was rolled back.",
        ));
    }
    // 关闭无 kill-on-close 限制的临时 Job 后允许 Notepad 继续运行。
    drop(job);
    // 返回仅含兼容 PID 与前台事实的内部证据。
    Ok(LaunchEvidence {
        // legacy 结果继续保留启动器 PID。
        process_id: process.dwProcessId,
        // 成功只可能是 true。
        foreground_unchanged,
    })
}
