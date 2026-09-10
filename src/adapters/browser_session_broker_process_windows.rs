//! 以不继承调用者 handle 的方式启动固定 browser-session broker。

// 导入结构长度与 Windows UTF-16 路径编码。
use std::{
    // 计算 Windows 启动结构长度。
    mem::size_of,
    // 编码固定 sibling 的 UTF-16 路径。
    os::windows::ffi::OsStrExt,
};

// 导入不继承句柄的 Windows 固定进程启动接口。
use windows::{
    // 导入 Windows 进程创建命名空间。
    Win32::{
        // 导入进程与线程 handle 关闭函数。
        Foundation::CloseHandle,
        // 导入固定 broker 进程创建接口与结构。
        System::Threading::{
            // 导入无控制台、独立控制组与 Unicode 环境标志。
            CREATE_NEW_PROCESS_GROUP,
            CREATE_NO_WINDOW,
            CREATE_UNICODE_ENVIRONMENT,
            // 导入固定无参数进程创建函数。
            CreateProcessW,
            // 导入创建结果结构。
            PROCESS_INFORMATION,
            // 导入无 stdio 继承的启动结构。
            STARTUPINFOW,
        },
    },
    // 导入只读 UTF-16 指针。
    core::PCWSTR,
};

// 导入固定 sibling 定位与统一结果。
use crate::{
    // 导入不会接受调用方路径的 sibling 定位器。
    adapters::fixed_local_ipc_windows::identity::sibling_image_path,
    // 导入统一结果类型。
    domain::AppResult,
};

// 导入父 Adapter 的固定文件名与安全错误构造器。
use super::{BROKER_FILE_NAME, broker_unavailable};

// 启动固定无参数 sibling broker，所有 stdio 都不继承 launcher。
pub(super) fn start_fixed_broker() -> AppResult<()> {
    // 从当前已认证 executable 目录定位固定 sibling。
    let image = sibling_image_path(BROKER_FILE_NAME)
        // 路径细节不得进入公共响应。
        .map_err(|_| broker_unavailable("The fixed browser session broker is unavailable."))?;
    // 把固定绝对路径编码为 nul 结尾 UTF-16。
    let image_wide = image
        // 只编码当前已解析的 sibling 路径。
        .as_os_str()
        // 使用 Windows 原生 UTF-16 表示。
        .encode_wide()
        // 添加 CreateProcessW 要求的终止符。
        .chain(Some(0))
        // 保存至进程创建完成。
        .collect::<Vec<_>>();
    // 构造不声明任何 stdio handle 的启动信息。
    let startup = STARTUPINFOW {
        // 写入 Windows 要求的精确结构长度。
        cb: u32::try_from(size_of::<STARTUPINFOW>()).map_err(|_| {
            // 理论长度溢出保持稳定启动失败。
            broker_unavailable("The fixed browser session broker could not be started.")
        })?,
        // 其余字段保持零值，因此不使用调用者标准流。
        ..Default::default()
    };
    // 接收 broker 进程与主线程 handle。
    let mut process = PROCESS_INFORMATION::default();
    // 以不继承任何 launcher handle 的方式启动固定 broker。
    unsafe {
        CreateProcessW(
            // 只允许当前已认证目录中的固定绝对 sibling。
            PCWSTR(image_wide.as_ptr()),
            // broker 不接受命令行参数。
            None,
            // 不继承进程安全描述符。
            None,
            // 不继承线程安全描述符。
            None,
            // 禁止继承 stdout 管道或任何其他 launcher handle。
            false,
            // 隐藏窗口、建立独立控制组并保留 Unicode 环境。
            CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_UNICODE_ENVIRONMENT,
            // 继承当前环境值但不继承环境 handle。
            None,
            // 继承当前工作目录。
            PCWSTR::null(),
            // 传入无 stdio 的固定启动信息。
            &startup,
            // 接收唯一创建结果。
            &mut process,
        )
    }
    // 不公开 Windows 或路径错误。
    .map_err(|_| {
        // 返回稳定启动错误。
        broker_unavailable("The fixed browser session broker could not be started.")
    })?;
    // launcher 不拥有长期 broker 生命周期，立即关闭线程 handle。
    let _ = unsafe { CloseHandle(process.hThread) };
    // 关闭进程 handle，不终止独立 broker。
    let _ = unsafe { CloseHandle(process.hProcess) };
    // 返回固定进程已启动事实。
    Ok(())
}
