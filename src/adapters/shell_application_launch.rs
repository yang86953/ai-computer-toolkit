//! Windows Shell 精确应用启动 Component。

// 导入 C 指针与结构尺寸工具。
use std::{ffi::c_void, mem::size_of, ptr::null_mut};

// 导入公开 Shell、COM 与句柄 API。
use windows::{
    // 导入 Win32 API 分组。
    Win32::{
        // 导入稳定错误码与句柄释放接口。
        Foundation::{CloseHandle, E_ACCESSDENIED, HANDLE, RPC_E_CHANGED_MODE},
        // 导入 COM 生命周期接口。
        System::Com::{
            COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize, IBindCtx,
        },
        // 导入 Shell identity 解析与无参数启动接口。
        UI::Shell::{
            // 导入 ITEMIDLIST 公共类型。
            Common::ITEMIDLIST,
            SEE_MASK_FLAG_NO_UI,
            SEE_MASK_IDLIST,
            SEE_MASK_NOASYNC,
            SEE_MASK_NOCLOSEPROCESS,
            SHELLEXECUTEINFOW,
            SHParseDisplayName,
            ShellExecuteExW,
        },
        // 导入普通显示请求常量。
        UI::WindowsAndMessaging::SW_SHOWNORMAL,
    },
    // 导入只借用 UTF-16 的公共指针类型。
    core::PCWSTR,
};

// 引入 Shell 启动 Component 私有封闭错误码实现。
#[path = "shell_application_launch_error.rs"]
mod error_code;

// 导入 Shell 启动 Component 私有封闭错误码。
use error_code::ShellLaunchErrorCode;

// 导入统一结果边界。
use crate::domain::AppResult;

// 保存 Shell 启动的最小非原生证据。
pub(crate) struct ShellLaunchEvidence {
    // 标记 Shell 已接受启动请求。
    pub(crate) dispatched: bool,
    // 标记 Shell 返回了可关闭的进程句柄。
    pub(crate) process_observed: bool,
}

// 配对管理当前线程的 COM 初始化引用。
struct ComApartment {
    // 标记本 Component 是否需要释放引用。
    should_uninitialize: bool,
}

// 为 COM apartment 实现确定性释放。
impl Drop for ComApartment {
    // 在作用域结束时释放本 Component 的初始化引用。
    fn drop(&mut self) {
        // 只有成功增加引用计数才执行释放。
        if self.should_uninitialize {
            // 与成功的 CoInitializeEx 配对。
            unsafe { CoUninitialize() };
        }
    }
}

// 初始化当前线程为 Shell 需要的 apartment。
fn initialize_com() -> AppResult<ComApartment> {
    // 请求单线程 apartment 以匹配 ShellExecute 对照实现。
    let status = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    // 成功时记录释放责任。
    if status.is_ok() {
        // 返回拥有本次 COM 引用的 guard。
        return Ok(ComApartment {
            // 当前 Component 必须配对释放。
            should_uninitialize: true,
        });
    }
    // 已由调用方选择其他 apartment 时允许继续使用 Shell。
    if status == RPC_E_CHANGED_MODE {
        // 返回不拥有 COM 引用的 guard。
        return Ok(ComApartment {
            // 禁止释放调用方的 COM 引用。
            should_uninitialize: false,
        });
    }
    // 其他初始化失败按 provider 不可用返回。
    Err(ShellLaunchErrorCode::ShellProviderUnavailable.error(
        // 不公开 HRESULT 或本机状态细节。
        "The Windows Shell apartment could not be initialized.",
    ))
}

// 配对管理 Shell 分配的绝对 ITEMIDLIST。
struct ShellItemId(*mut ITEMIDLIST);

// 为 ITEMIDLIST 实现 COM allocator 释放。
impl Drop for ShellItemId {
    // 在所有成功或失败分支释放 PIDL。
    fn drop(&mut self) {
        // 非空 PIDL 才交给 COM allocator。
        if !self.0.is_null() {
            // 与 SHParseDisplayName 的分配语义配对。
            unsafe { CoTaskMemFree(Some(self.0.cast::<c_void>())) };
        }
    }
}

// 关闭 Shell 可选返回的进程句柄。
fn close_process_handle(handle: HANDLE) -> bool {
    // 空句柄表示 Shell 未提供可观察进程。
    if handle.0.is_null() {
        // 返回未观察到进程。
        return false;
    }
    // 关闭只用于证据的进程句柄，不等待也不终止进程。
    let _ = unsafe { CloseHandle(handle) };
    // 返回已观察到进程句柄。
    true
}

// 通过私有 Shell parsing identity 调度无参数应用启动。
pub(crate) fn launch(identity: &str) -> AppResult<ShellLaunchEvidence> {
    // 空 identity 不是认证 Shell 目标。
    if identity.is_empty() {
        // 在触碰 COM 前返回能力不可用。
        return Err(ShellLaunchErrorCode::CapabilityUnavailable.error(
            // 不说明或回显内部 identity。
            "The exact installed application has no certified public Shell launch identity.",
        ));
    }
    // 保持 COM guard 直到 PIDL 与请求完成。
    let _com = initialize_com()?;
    // 把私有 identity 编码为 NUL 结尾 UTF-16。
    let mut wide_identity = identity.encode_utf16().collect::<Vec<_>>();
    // 追加 Windows 字符串终止符。
    wide_identity.push(0);
    // 初始化 Shell 输出 PIDL 指针。
    let mut item_id = null_mut::<ITEMIDLIST>();
    // 解析当前使用时的精确 Shell identity。
    let parsed = unsafe {
        // 不提供 bind context、属性过滤或原生调用方输出。
        SHParseDisplayName(
            // 只借用本地 UTF-16 缓冲区。
            PCWSTR(wide_identity.as_ptr()),
            // 禁止调用方注入 bind context。
            None::<&IBindCtx>,
            // 接收 Shell 分配的 PIDL。
            &mut item_id,
            // 不请求额外属性。
            0,
            // 不返回原生属性标志。
            None,
        )
    };
    // 解析失败或空 PIDL 都表示目标已经 stale。
    if parsed.is_err() || item_id.is_null() {
        // 返回不含 identity 的稳定 stale 错误。
        return Err(ShellLaunchErrorCode::StaleSession.error(
            // 不泄漏 AUMID、路径或 PIDL。
            "The certified Shell application identity no longer resolves.",
        ));
    }
    // 用 RAII 保证 PIDL 在所有后续分支释放。
    let item_id = ShellItemId(item_id);
    // 一次性初始化 ShellExecute 请求，避免字段赋值遗漏。
    let mut request = SHELLEXECUTEINFOW {
        // 声明结构版本尺寸。
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        // 只允许 PIDL、无 UI、同步调度和可关闭进程句柄。
        fMask: SEE_MASK_IDLIST
            // 请求返回可关闭的进程句柄。
            | SEE_MASK_NOCLOSEPROCESS
            // 禁止 Shell 显示错误界面。
            | SEE_MASK_FLAG_NO_UI
            // 等待同步调度结果。
            | SEE_MASK_NOASYNC,
        // 传入只在当前进程内存在的 PIDL。
        lpIDList: item_id.0.cast::<c_void>(),
        // 请求应用自行决定普通展示方式。
        nShow: SW_SHOWNORMAL.0,
        // 其余可选字段保持零值，不提供 verb、路径字符串、参数或工作目录。
        ..Default::default()
    };
    // 调度 Shell 启动，不提供 verb、路径字符串、参数或工作目录。
    let launched = unsafe { ShellExecuteExW(&mut request) };
    // 无论成功失败都关闭可选进程句柄。
    let process_observed = close_process_handle(request.hProcess);
    // Shell 明确拒绝时返回封闭错误。
    if let Err(error) = launched {
        // 权限拒绝不得降级或提权重试。
        if error.code() == E_ACCESSDENIED {
            // 返回稳定权限错误。
            return Err(ShellLaunchErrorCode::PermissionDenied.error(
                // 不输出 HRESULT 或 native identity。
                "Windows denied the exact installed application launch.",
            ));
        }
        // 其他 Shell 失败保持结构化且不回显系统消息。
        return Err(ShellLaunchErrorCode::ApplicationStartFailed.error(
            // 不泄漏关联或路径细节。
            "Windows Shell rejected the exact installed application launch.",
        ));
    }
    // 返回最小公开证据，禁止句柄与进程 ID 越界。
    Ok(ShellLaunchEvidence {
        // ShellExecuteExW 成功即表示已调度。
        dispatched: true,
        // 只保留是否观察到句柄的布尔事实。
        process_observed,
    })
}

// 声明不启动应用的输入与 stale 门禁测试。
#[cfg(test)]
mod tests {
    // 导入父 Component 的封闭启动入口。
    use super::launch;

    // 验证空 identity 在 COM 或 Shell 调用前失败。
    #[test]
    fn empty_identity_is_not_launchable() {
        // 调用明确空 identity。
        let error = launch("")
            // 空 identity 不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("empty Shell identity must fail"));
        // 必须返回能力不可用而不是启动失败。
        assert_eq!(error.code, "CAPABILITY_UNAVAILABLE");
        // 保持既有安全消息且不回显内部 identity。
        assert_eq!(
            // 读取公开错误消息。
            error.message,
            // 对照既有认证 identity 缺口说明。
            "The exact installed application has no certified public Shell launch identity."
        );
    }

    // 验证不存在的 Shell identity 只返回 stale。
    #[test]
    fn missing_identity_is_stale_without_launch() {
        // 使用不会对应真实目标的解析名称。
        let error = launch("shell:AppsFolder\\Act.Nonexistent.Package_000!Missing")
            // 不存在 identity 不得成功。
            .err()
            // 使用显式 panic 保留失败上下文。
            .unwrap_or_else(|| panic!("missing Shell identity must be stale"));
        // 必须在 ShellExecute 前返回 stale。
        assert_eq!(error.code, "STALE_SESSION");
        // 保持既有不泄漏 identity 的公开消息。
        assert_eq!(
            // 读取公开错误消息。
            error.message,
            // 对照既有使用时解析失败说明。
            "The certified Shell application identity no longer resolves."
        );
    }
}
