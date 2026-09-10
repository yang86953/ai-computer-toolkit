//! Windows 精确窗口捕获预检 Component；只读取元数据，不创建捕获资源。

// 导入 Windows Runtime 捕获 item、DWM 与只读窗口查询 API。
use windows::{
    // 使用公开 WGC 类型系统探测 runtime 与精确窗口 item interop。
    Graphics::Capture::{GraphicsCaptureItem, GraphicsCaptureSession},
    // 使用 Win32 只读元数据 API，不执行激活、输入或像素操作。
    Win32::{
        // 导入权限错误、窗口句柄和矩形类型。
        Foundation::{E_ACCESSDENIED, HWND, RECT},
        // 导入桌面组合与 cloaked 状态查询。
        Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute, DwmIsCompositionEnabled},
        // 导入 WinRT 初始化与 WGC item interop。
        System::WinRT::{
            // 导入精确窗口创建捕获 item 的公开 ABI。
            Graphics::Capture::IGraphicsCaptureItemInterop,
            // 使用多线程 WinRT apartment，不启动捕获。
            RO_INIT_MULTITHREADED,
            RoInitialize,
            RoUninitialize,
        },
        // 导入只读窗口可见性、尺寸与内容保护查询。
        UI::WindowsAndMessaging::{
            GetWindowDisplayAffinity, GetWindowRect, IsIconic, IsWindow, IsWindowVisible,
            WDA_EXCLUDEFROMCAPTURE, WDA_MONITOR, WDA_NONE,
        },
    },
    // 使用类型安全的 WinRT factory 与 HRESULT。
    core::{HRESULT, factory},
};

// 导入语言中立错误边界。
use crate::domain::{AppControlError, AppResult};

// 保存零帧捕获预检可公开的封闭证据。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CapturePreflightEvidence {
    // 标记窗口当前是否可见。
    pub(crate) visible: bool,
    // 标记窗口当前是否最小化。
    pub(crate) minimized: bool,
    // 标记 DWM 是否报告窗口被 cloaked。
    pub(crate) cloaked: bool,
    // 标记窗口矩形是否具有正宽高。
    pub(crate) nonzero_extent: bool,
    // 标记桌面组合是否可用。
    pub(crate) desktop_composition_enabled: bool,
    // 保存封闭内容保护分类。
    pub(crate) content_protection: &'static str,
    // 保存封闭 WGC runtime 分类。
    pub(crate) wgc_runtime: &'static str,
    // 保存封闭 WGC item interop 分类。
    pub(crate) wgc_item_interop: &'static str,
    // 标记捕获 item 是否报告正尺寸，但不公开尺寸。
    pub(crate) capture_item_nonzero_size: bool,
    // 保存按固定优先级计算的 eligibility。
    pub(crate) eligibility: &'static str,
}

// 保存 WGC 探测的内部封闭结果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WgcProbe {
    // 保存 runtime 分类。
    runtime: &'static str,
    // 保存 item interop 分类。
    item_interop: &'static str,
    // 只公开 item 是否具有正尺寸。
    item_nonzero_size: bool,
}

// 对精确原生窗口执行零帧元数据预检。
pub(crate) fn inspect(hwnd: isize) -> AppResult<CapturePreflightEvidence> {
    // 使用自管理 WinRT apartment 的 WGC 探针。
    inspect_with_probe(hwnd, probe_wgc)
}

// 在调用方已初始化 WinRT apartment 时执行零帧元数据预检。
pub(crate) fn inspect_in_initialized_apartment(
    // 接收仅在 adapter 内存在的原生句柄。
    hwnd: isize,
) -> AppResult<CapturePreflightEvidence> {
    // 禁止中途反初始化调用方拥有的 apartment。
    inspect_with_probe(hwnd, probe_wgc_initialized)
}

// 组合窗口事实与调用方选择的 WGC 探针生命周期。
fn inspect_with_probe(
    // 接收私有原生句柄。
    hwnd: isize,
    // 接收封闭 WGC 元数据探针。
    probe: impl FnOnce(HWND) -> WgcProbe,
) -> AppResult<CapturePreflightEvidence> {
    // 构造仅在 Component 内存在的私有原生句柄。
    let native = HWND(hwnd as *mut std::ffi::c_void);
    // 使用时重新验证窗口存在性，防止枚举后的 stale 竞态。
    if !unsafe { IsWindow(Some(native)) }.as_bool() {
        // 返回与 C++ 对照一致的 stale 语义。
        return Err(AppControlError::new(
            // 使用稳定 opaque 目标错误码。
            "STALE_SESSION",
            // 不公开原生句柄。
            "The exact window no longer exists during capture preflight.",
        ));
    }
    // 查询可见性，不显示或激活窗口。
    let visible = unsafe { IsWindowVisible(native) }.as_bool();
    // 查询最小化状态，不恢复窗口。
    let minimized = unsafe { IsIconic(native) }.as_bool();
    // 查询 DWM cloaked 标记；查询失败按未 cloaked 保持 C++ 语义。
    let cloaked = window_is_cloaked(native);
    // 查询窗口矩形，仅公开是否具有正尺寸。
    let nonzero_extent = window_has_nonzero_extent(native);
    // 查询桌面组合状态。
    let desktop_composition_enabled = desktop_composition_enabled();
    // 查询内容保护分类，不公开 affinity 数值。
    let content_protection = content_protection(native);
    // 探测 WGC runtime 与 item interop，不创建帧池或 session。
    let wgc = probe(native);
    // 按 C++ 固定优先级计算 eligibility。
    let eligibility = eligibility(
        // 传入窗口可见性。
        visible,
        // 传入最小化状态。
        minimized,
        // 传入 cloaked 状态。
        cloaked,
        // 传入正尺寸状态。
        nonzero_extent,
        // 传入内容保护分类。
        content_protection,
        // 传入桌面组合状态。
        desktop_composition_enabled,
        // 传入 WGC runtime 分类。
        wgc.runtime,
        // 传入 WGC item interop 分类。
        wgc.item_interop,
        // 传入 item 正尺寸状态。
        wgc.item_nonzero_size,
    );
    // 返回仅含封闭证据的值对象。
    Ok(CapturePreflightEvidence {
        // 保存可见性。
        visible,
        // 保存最小化状态。
        minimized,
        // 保存 cloaked 状态。
        cloaked,
        // 保存正尺寸状态。
        nonzero_extent,
        // 保存组合状态。
        desktop_composition_enabled,
        // 保存内容保护分类。
        content_protection,
        // 保存 WGC runtime 分类。
        wgc_runtime: wgc.runtime,
        // 保存 item interop 分类。
        wgc_item_interop: wgc.item_interop,
        // 保存 item 正尺寸布尔值。
        capture_item_nonzero_size: wgc.item_nonzero_size,
        // 保存 eligibility。
        eligibility,
    })
}

// 查询窗口 cloaked 状态。
fn window_is_cloaked(hwnd: HWND) -> bool {
    // 准备 DWM 输出缓冲区。
    let mut cloaked = 0_u32;
    // 仅请求 DWMWA_CLOAKED 属性。
    let status = unsafe {
        // 调用只读 DWM 属性查询。
        DwmGetWindowAttribute(
            // 传入精确窗口。
            hwnd,
            // 请求 cloaked 属性。
            DWMWA_CLOAKED,
            // 传入固定 u32 输出缓冲区。
            (&mut cloaked as *mut u32).cast(),
            // 传入精确缓冲区字节数。
            u32::try_from(std::mem::size_of::<u32>()).unwrap_or(0),
        )
    };
    // 只有成功且非零才报告 cloaked。
    status.is_ok() && cloaked != 0
}

// 查询窗口是否具有正宽高矩形。
fn window_has_nonzero_extent(hwnd: HWND) -> bool {
    // 准备窗口矩形输出。
    let mut rectangle = RECT::default();
    // 读取矩形；失败时保守报告无可见范围。
    let read = unsafe { GetWindowRect(hwnd, &mut rectangle) };
    // 仅公开正宽高布尔值。
    read.is_ok()
        // 要求右边界大于左边界。
        && rectangle.right > rectangle.left
        // 要求下边界大于上边界。
        && rectangle.bottom > rectangle.top
}

// 查询桌面组合是否启用。
fn desktop_composition_enabled() -> bool {
    // 调用只读 DWM 组合查询并直接读取生成绑定返回的 BOOL。
    unsafe { DwmIsCompositionEnabled() }
        // 成功且值为真才报告可用。
        .is_ok_and(|enabled| enabled.as_bool())
}

// 查询窗口内容保护并映射到稳定公开枚举。
fn content_protection(hwnd: HWND) -> &'static str {
    // 使用无保护作为初始化缓冲区，不把失败误报为 none。
    let mut affinity = WDA_NONE.0;
    // 调用只读 display affinity 查询。
    let read = unsafe { GetWindowDisplayAffinity(hwnd, &mut affinity) };
    // 查询失败时保持 unknown。
    if read.is_err() {
        // 返回封闭未知分类。
        return "unknown";
    }
    // 排除捕获标记优先于 monitor-only。
    if affinity & WDA_EXCLUDEFROMCAPTURE.0 != WDA_NONE.0 {
        // 返回完整排除分类。
        return "excluded-from-capture";
    }
    // 检查 monitor-only 标记。
    if affinity & WDA_MONITOR.0 != WDA_NONE.0 {
        // 返回仅监视器分类。
        return "monitor-only";
    }
    // 成功且无已知标记表示无保护。
    "none"
}

// 探测公开 WGC runtime 与精确窗口 item interop。
fn probe_wgc(hwnd: HWND) -> WgcProbe {
    // 初始化 WinRT apartment；changed-mode 表示当前线程已有可用 apartment。
    let initialized = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    // 固定 RPC_E_CHANGED_MODE HRESULT，避免把既有 apartment 误报为 runtime 缺失。
    let changed_mode = HRESULT(0x8001_0106_u32 as i32);
    // 记录是否由本函数成功初始化，以决定是否配对反初始化。
    let should_uninitialize = initialized.is_ok();
    // 非 changed-mode 初始化失败时 runtime 不可用。
    if initialized
        // 借用失败值。
        .as_ref()
        // 只保留错误码。
        .err()
        // 判断是否不是允许继续的 changed-mode。
        .is_some_and(|error| error.code() != changed_mode)
    {
        // 返回封闭不可用状态。
        return WgcProbe {
            // runtime 初始化失败。
            runtime: "unavailable",
            // 未进入 item interop。
            item_interop: "unavailable",
            // 未取得 item 尺寸。
            item_nonzero_size: false,
        };
    }
    // 在当前 apartment 内执行 WGC 元数据探针。
    let probe = probe_wgc_initialized(hwnd);
    // 仅对本函数成功初始化的 apartment 配对反初始化。
    if should_uninitialize {
        // 释放当前线程的 WinRT 初始化引用。
        unsafe { RoUninitialize() };
    }
    // 返回封闭探测结果。
    probe
}

// 在已经初始化的 WinRT apartment 内探测 WGC runtime 与 item interop。
fn probe_wgc_initialized(hwnd: HWND) -> WgcProbe {
    // 查询系统是否支持 WGC session；查询失败视为 runtime 不可用。
    let runtime = match GraphicsCaptureSession::IsSupported() {
        // 明确支持。
        Ok(true) => "supported",
        // 明确不支持。
        Ok(false) => "unsupported",
        // 查询失败。
        Err(_) => "unavailable",
    };
    // 仅在 runtime 支持时创建只读 capture item。
    let probe = if runtime == "supported" {
        // 获取公开 item interop factory。
        match factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>() {
            // factory 可用时尝试精确窗口 item。
            Ok(interop) => {
                // 创建 item 本身不会创建帧池或启动捕获 session。
                let item: windows::core::Result<GraphicsCaptureItem> =
                    unsafe { interop.CreateForWindow(hwnd) };
                // 映射 item 结果。
                match item {
                    // item 可用时仅读取 Size。
                    Ok(item) => {
                        // 查询 item 尺寸，不公开数值。
                        let nonzero = item
                            // 调用只读 Size getter。
                            .Size()
                            // 只判断正宽高。
                            .is_ok_and(|size| size.Width > 0 && size.Height > 0);
                        // 返回可用 interop 分类。
                        WgcProbe {
                            // 保留 runtime 分类。
                            runtime,
                            // item 创建成功。
                            item_interop: "available",
                            // 保存封闭尺寸事实。
                            item_nonzero_size: nonzero,
                        }
                    }
                    // 权限拒绝与其他不可用必须区分。
                    Err(error) => WgcProbe {
                        // 保留 runtime 分类。
                        runtime,
                        // 映射稳定 interop 分类。
                        item_interop: if error.code() == E_ACCESSDENIED {
                            // 明确权限阻塞。
                            "permission-blocked"
                        } else {
                            // 其他 item 创建失败。
                            "unavailable"
                        },
                        // 未取得 item 尺寸。
                        item_nonzero_size: false,
                    },
                }
            }
            // factory 不可用时保持 runtime supported 但 interop unavailable。
            Err(_) => WgcProbe {
                // 保留 runtime 分类。
                runtime,
                // factory 缺失。
                item_interop: "unavailable",
                // 未取得 item 尺寸。
                item_nonzero_size: false,
            },
        }
    } else {
        // runtime 不支持或不可用时不尝试 item factory。
        WgcProbe {
            // 保存 runtime 分类。
            runtime,
            // 明确区分 unsupported 与 unavailable。
            item_interop: if runtime == "unsupported" {
                // 系统明确不支持。
                "unsupported"
            } else {
                // runtime 查询不可用。
                "unavailable"
            },
            // 未取得 item 尺寸。
            item_nonzero_size: false,
        }
    };
    // 返回封闭探测结果。
    probe
}

// 按 C++ 对照的固定优先级计算 eligibility。
#[allow(clippy::too_many_arguments)]
fn eligibility(
    // 接收可见性。
    visible: bool,
    // 接收最小化状态。
    minimized: bool,
    // 接收 cloaked 状态。
    cloaked: bool,
    // 接收正尺寸状态。
    nonzero_extent: bool,
    // 接收内容保护分类。
    content_protection: &str,
    // 接收桌面组合状态。
    desktop_composition_enabled: bool,
    // 接收 WGC runtime 分类。
    wgc_runtime: &str,
    // 接收 item interop 分类。
    wgc_item_interop: &str,
    // 接收 item 正尺寸状态。
    capture_item_nonzero_size: bool,
) -> &'static str {
    // 可见性缺口具有最高优先级。
    if !visible {
        // 返回窗口不可见。
        "window-not-visible"
    // 最小化窗口没有可认证的新合成帧。
    } else if minimized {
        // 返回最小化分类。
        "window-minimized"
    // cloaked 窗口不进入认证捕获路线。
    } else if cloaked {
        // 返回 cloaked 分类。
        "window-cloaked"
    // 无正尺寸窗口无法捕获。
    } else if !nonzero_extent {
        // 返回无可见范围分类。
        "window-has-no-visible-extent"
    // 显式排除捕获必须停止。
    } else if content_protection == "excluded-from-capture" {
        // 返回内容保护分类。
        "capture-excluded-by-window"
    // 无桌面组合时 WGC 路线不可认证。
    } else if !desktop_composition_enabled {
        // 返回组合不可用分类。
        "desktop-composition-unavailable"
    // 内容保护未知时 fail closed。
    } else if content_protection == "unknown" {
        // 返回保护未知分类。
        "capture-protection-unknown"
    // 明确不支持 WGC。
    } else if wgc_runtime == "unsupported" {
        // 返回 runtime 不支持分类。
        "wgc-runtime-unsupported"
    // 其他非 supported runtime 均不可用。
    } else if wgc_runtime != "supported" {
        // 返回 runtime 不可用分类。
        "wgc-runtime-unavailable"
    // 精确 item 被权限阻塞。
    } else if wgc_item_interop == "permission-blocked" {
        // 返回权限阻塞分类。
        "wgc-item-permission-blocked"
    // item interop 或尺寸任一不可用即停止。
    } else if wgc_item_interop != "available" || !capture_item_nonzero_size {
        // 返回 item 不可用分类。
        "wgc-item-unavailable"
    // 所有只读门禁均通过。
    } else {
        // 返回认证路线候选分类。
        "eligible-for-certified-capture-route"
    }
}

// 声明纯 eligibility 顺序测试。
#[cfg(test)]
mod tests {
    // 导入父模块纯函数。
    use super::eligibility;

    // 验证成功与高优先级失败分支保持 C++ 顺序。
    #[test]
    fn eligibility_uses_the_cpp_fail_closed_priority() {
        // 完整可用证据应进入认证路线候选。
        assert_eq!(
            // 传入完整可用夹具。
            eligibility(
                true,
                false,
                false,
                true,
                "none",
                true,
                "supported",
                "available",
                true
            ),
            // 断言稳定成功分类。
            "eligible-for-certified-capture-route"
        );
        // 不可见必须优先于其他同时存在的缺口。
        assert_eq!(
            // 同时注入不可见与最小化。
            eligibility(
                false,
                true,
                true,
                false,
                "unknown",
                false,
                "unavailable",
                "unavailable",
                false
            ),
            // 断言最高优先级分类。
            "window-not-visible"
        );
        // 权限阻塞必须区别于普通 item 不可用。
        assert_eq!(
            // 仅注入 item 权限阻塞。
            eligibility(
                true,
                false,
                false,
                true,
                "none",
                true,
                "supported",
                "permission-blocked",
                false
            ),
            // 断言权限分类。
            "wgc-item-permission-blocked"
        );
    }
}
