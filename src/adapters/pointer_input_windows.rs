//! 封装通用指针输入所需的窄 Windows 平台调用。

// 导入 Win32 指针、DPI 与窗口接口。
use windows::Win32::{
    // 导入二维点与矩形。
    Foundation::{POINT, RECT},
    // 导入客户区到屏幕物理坐标转换。
    Graphics::Gdi::ClientToScreen,
    // 导入 UI 平台边界。
    UI::{
        // 导入线程级 Per-Monitor-V2 DPI 上下文。
        HiDpi::{
            // 导入强类型 DPI 上下文。
            DPI_AWARENESS_CONTEXT,
            // 导入 Per-Monitor-V2 固定上下文。
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            // 导入线程上下文切换函数。
            SetThreadDpiAwarenessContext,
        },
        // 导入 SendInput 鼠标结构与封闭事件标志。
        Input::KeyboardAndMouse::{
            // 导入统一输入结构与联合体。
            INPUT,
            INPUT_0,
            // 导入鼠标输入类别。
            INPUT_MOUSE,
            // 导入强类型鼠标事件标志与载荷。
            MOUSE_EVENT_FLAGS,
            // 导入水平滚轮事件。
            MOUSEEVENTF_HWHEEL,
            // 导入左键上下事件。
            MOUSEEVENTF_LEFTDOWN,
            MOUSEEVENTF_LEFTUP,
            // 导入中键上下事件。
            MOUSEEVENTF_MIDDLEDOWN,
            MOUSEEVENTF_MIDDLEUP,
            // 导入右键上下事件。
            MOUSEEVENTF_RIGHTDOWN,
            MOUSEEVENTF_RIGHTUP,
            // 导入垂直滚轮事件。
            MOUSEEVENTF_WHEEL,
            MOUSEINPUT,
            // 导入统一前台输入调用。
            SendInput,
        },
        // 导入精确坐标与前景窗口调用。
        WindowsAndMessaging::{
            // 导入客户区矩形读取。
            GetClientRect,
            // 导入虚拟桌面系统指标读取。
            GetSystemMetrics,
            // 导入窗口关系读取。
            IsChild,
            // 导入虚拟桌面四项指标。
            SM_CXVIRTUALSCREEN,
            SM_CYVIRTUALSCREEN,
            SM_XVIRTUALSCREEN,
            SM_YVIRTUALSCREEN,
            // 导入光标定位请求。
            // 导入当前物理点顶层命中查询。
            WindowFromPoint,
        },
    },
};

// 引入当前 Adapter 私有封闭错误类型。
#[path = "pointer_input_windows_error.rs"]
mod error_code;

// 导入当前 Adapter 私有错误集合。
use error_code::PointerInputWindowsErrorCode;

// 导入窗口私有事实、公开指针契约与统一结果。
use crate::{
    // 导入共享窗口存在性 Component 与私有窗口记录。
    adapters::{
        // 导入共享窗口存在性核对。
        foreground_input_windows::ensure_window_exists,
        // 导入私有窗口记录。
        windows::WindowRecord,
    },
    // 导入 provider-neutral 指针类型。
    components::pointer_input_contract::{
        // 导入按钮类别与坐标空间。
        PointerButton,
        PointerCoordinateSpace,
        // 导入二维点与滚轮轴。
        PointerPoint,
        PointerScrollAxis,
    },
    // 导入统一结果类型。
    domain::AppResult,
};

// Windows 每个滚轮刻度固定使用的增量。
const WINDOWS_WHEEL_DELTA: i32 = 120;

// 保存本次线程替换的 DPI 上下文。
pub(crate) struct PointerDpiGuard {
    // 保存进入前上下文供 Drop 恢复。
    previous: DPI_AWARENESS_CONTEXT,
}

// 提供指针调用专用 DPI 生命周期。
impl PointerDpiGuard {
    // 把当前调用线程切换到 Per-Monitor-V2。
    pub(crate) fn enter() -> AppResult<Self> {
        // 设置物理坐标所需的线程上下文。
        let previous = unsafe {
            // 只改变当前同步指针调用线程。
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
        };
        // 无效旧上下文表示切换失败。
        if previous.is_invalid() {
            // 返回不泄漏 last-error 的稳定缺口。
            return Err(
                PointerInputWindowsErrorCode::CoordinateContextUnavailable.error(
                    "The pointer adapter could not establish Per-Monitor-V2 coordinate semantics.",
                ),
            );
        }
        // 返回拥有恢复责任的守卫。
        Ok(Self { previous })
    }
}

// 恢复调用前线程 DPI 上下文。
impl Drop for PointerDpiGuard {
    // 执行成对恢复。
    fn drop(&mut self) {
        // Drop 不能因恢复失败 panic。
        let _ = unsafe {
            // 只恢复当前调用线程。
            SetThreadDpiAwarenessContext(self.previous)
        };
    }
}

// 保存平台私有虚拟桌面边界。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VirtualScreenBounds {
    // 保存左边界。
    x: i32,
    // 保存上边界。
    y: i32,
    // 保存宽度。
    width: i32,
    // 保存高度。
    height: i32,
}

// 保存已验证的物理屏幕点。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PhysicalScreenPoint {
    // 保存物理屏幕横坐标。
    pub(crate) x: i32,
    // 保存物理屏幕纵坐标。
    pub(crate) y: i32,
}

// 读取当前虚拟桌面物理边界。
fn virtual_screen_bounds() -> AppResult<VirtualScreenBounds> {
    // 读取带符号左边界。
    let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    // 读取带符号上边界。
    let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    // 读取虚拟桌面宽度。
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    // 读取虚拟桌面高度。
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    // 非正尺寸无法认证物理坐标。
    if width <= 0 || height <= 0 {
        // 返回稳定坐标上下文缺口。
        return Err(PointerInputWindowsErrorCode::CoordinateContextUnavailable
            .error("The virtual screen does not expose a positive physical extent."));
    }
    // 返回私有边界。
    Ok(VirtualScreenBounds {
        // 保存左边界。
        x,
        // 保存上边界。
        y,
        // 保存宽度。
        width,
        // 保存高度。
        height,
    })
}

// 判断点是否位于带符号虚拟桌面内。
fn contains_point(bounds: VirtualScreenBounds, point: PhysicalScreenPoint) -> bool {
    // 使用 i64 避免横向边界加法溢出。
    let right = i64::from(bounds.x) + i64::from(bounds.width);
    // 使用 i64 避免纵向边界加法溢出。
    let bottom = i64::from(bounds.y) + i64::from(bounds.height);
    // 同时核对四条半开边界。
    i64::from(point.x) >= i64::from(bounds.x)
        // 核对右边界。
        && i64::from(point.x) < right
        // 核对上边界。
        && i64::from(point.y) >= i64::from(bounds.y)
        // 核对下边界。
        && i64::from(point.y) < bottom
}

// 将公开点解析为当前时刻的物理屏幕点。
pub(crate) fn resolve_physical_point(
    // 接收当前重新发现窗口。
    window: &WindowRecord,
    // 接收 provider-neutral 坐标空间。
    coordinate_space: PointerCoordinateSpace,
    // 接收公开点。
    point: PointerPoint,
) -> AppResult<PhysicalScreenPoint> {
    // 确认目标句柄仍存在。
    let native = ensure_window_exists(window)?;
    // 按公开坐标空间转换。
    let physical = match coordinate_space {
        // 屏幕物理像素无需坐标转换。
        PointerCoordinateSpace::ScreenPhysicalPx => PhysicalScreenPoint {
            // 传播横坐标。
            x: point.x,
            // 传播纵坐标。
            y: point.y,
        },
        // 客户区物理像素必须使用当前窗口位置转换。
        PointerCoordinateSpace::WindowClientPhysicalPx => {
            // 初始化客户区矩形。
            let mut client = RECT::default();
            // 读取当前客户区尺寸。
            unsafe { GetClientRect(native, &mut client) }.map_err(|_| {
                // 不公开 Win32 错误内容。
                PointerInputWindowsErrorCode::StaleSession
                    .error("The exact pointer target client area is unavailable.")
            })?;
            // 公开点必须落在客户区半开边界内。
            if point.x < client.left
                // 核对右边界。
                || point.x >= client.right
                // 核对上边界。
                || point.y < client.top
                // 核对下边界。
                || point.y >= client.bottom
            {
                // 返回稳定参数范围错误。
                return Err(PointerInputWindowsErrorCode::InvalidArgument
                    .error("The window-client pointer point is outside the current client area."));
            }
            // 构造客户区点。
            let mut native_point = POINT {
                // 保存客户区横坐标。
                x: point.x,
                // 保存客户区纵坐标。
                y: point.y,
            };
            // 使用当前窗口位置转换到物理屏幕点。
            if !unsafe { ClientToScreen(native, &mut native_point) }.as_bool() {
                // 不公开 Win32 错误内容。
                return Err(PointerInputWindowsErrorCode::StaleSession.error(
                    "The exact pointer target moved or closed during coordinate conversion.",
                ));
            }
            // 返回转换后的物理点。
            PhysicalScreenPoint {
                // 传播屏幕横坐标。
                x: native_point.x,
                // 传播屏幕纵坐标。
                y: native_point.y,
            }
        }
    };
    // 读取当前带符号虚拟桌面边界。
    let bounds = virtual_screen_bounds()?;
    // 最终点必须属于虚拟桌面。
    if !contains_point(bounds, physical) {
        // 返回稳定参数范围错误。
        return Err(PointerInputWindowsErrorCode::InvalidArgument
            .error("The pointer point is outside the current virtual screen."));
    }
    // 返回认证物理点。
    Ok(physical)
}

// 把光标移动到已认证物理点。
pub(crate) fn move_pointer(point: PhysicalScreenPoint) -> AppResult<()> {
    // 调用 Windows 光标定位接口。
    uix_app::platform::windowing::desktop_cursor::set_cursor_position(point.x, point.y).map_err(|_| {
        // 不公开 last-error 或坐标边界。
        PointerInputWindowsErrorCode::PointerDispatchFailed
            .error("Windows did not accept the pointer move.")
    })
}

// 判断物理点当前是否命中精确窗口或其子窗口。
pub(crate) fn point_targets_window(
    // 接收当前重新发现窗口。
    window: &WindowRecord,
    // 接收已验证物理点。
    point: PhysicalScreenPoint,
) -> AppResult<bool> {
    // 核对精确窗口仍存在。
    let target = ensure_window_exists(window)?;
    // 查询该物理点当前最上层命中窗口。
    let hit = unsafe {
        // 使用带符号物理屏幕坐标。
        WindowFromPoint(POINT {
            // 保存横坐标。
            x: point.x,
            // 保存纵坐标。
            y: point.y,
        })
    };
    // 空命中不属于精确目标。
    if hit.is_invalid() {
        // 返回未命中。
        return Ok(false);
    }
    // 精确顶层窗口直接命中。
    if hit == target {
        // 返回命中。
        return Ok(true);
    }
    // 标准子窗口同样属于精确目标。
    Ok(unsafe { IsChild(target, hit) }.as_bool())
}

// 返回按钮按下或释放对应的平台标志。
fn button_flags(button: PointerButton, pressed: bool) -> MOUSE_EVENT_FLAGS {
    // 穷举按钮与阶段组合。
    match (button, pressed) {
        // 左键按下。
        (PointerButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
        // 左键释放。
        (PointerButton::Left, false) => MOUSEEVENTF_LEFTUP,
        // 右键按下。
        (PointerButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
        // 右键释放。
        (PointerButton::Right, false) => MOUSEEVENTF_RIGHTUP,
        // 中键按下。
        (PointerButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
        // 中键释放。
        (PointerButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
    }
}

// 构造一个平台鼠标事件。
fn mouse_input(flags: MOUSE_EVENT_FLAGS, data: u32) -> INPUT {
    // 返回只含 provider 私有字段的 INPUT。
    INPUT {
        // 固定鼠标输入类别。
        r#type: INPUT_MOUSE,
        // 构造鼠标联合体分支。
        Anonymous: INPUT_0 {
            // 填充鼠标载荷。
            mi: MOUSEINPUT {
                // 坐标移动由 SetCursorPos 单独拥有。
                dx: 0,
                // 坐标移动由 SetCursorPos 单独拥有。
                dy: 0,
                // 保存滚轮数据或零。
                mouseData: data,
                // 保存封闭事件标志。
                dwFlags: flags,
                // 不伪造消息时间。
                time: 0,
                // 不公开或注入额外信息。
                dwExtraInfo: 0,
            },
        },
    }
}

// 发送恰好一个鼠标事件并验证完整接收。
fn send_mouse_event(flags: MOUSE_EVENT_FLAGS, data: u32) -> AppResult<()> {
    // 构造单事件数组以消除批量部分接收歧义。
    let inputs = [mouse_input(flags, data)];
    // 安全转换固定结构尺寸。
    let size = i32::try_from(std::mem::size_of::<INPUT>()).map_err(|_| {
        // 返回稳定平台缺口。
        PointerInputWindowsErrorCode::PointerDispatchFailed
            .error("The Windows INPUT structure size is unsupported.")
    })?;
    // 调度单个事件。
    let sent = unsafe { SendInput(&inputs, size) };
    // 单事件必须完整接收。
    if sent != 1 {
        // 返回不确定平台拒绝。
        return Err(PointerInputWindowsErrorCode::PointerDispatchFailed
            .error("Windows did not accept the pointer event."));
    }
    // 返回调度成功。
    Ok(())
}

// 发送一个显式按钮阶段。
pub(crate) fn dispatch_button(button: PointerButton, pressed: bool) -> AppResult<()> {
    // 使用封闭映射生成平台标志。
    send_mouse_event(button_flags(button, pressed), 0)
}

// 发送一个有界滚轮步骤。
pub(crate) fn dispatch_scroll(axis: PointerScrollAxis, ticks: i32) -> AppResult<()> {
    // 把 provider-neutral 刻度映射到 Windows 固定增量。
    let delta = ticks.saturating_mul(WINDOWS_WHEEL_DELTA);
    // 按轴选择封闭平台标志。
    let flags = match axis {
        // 映射垂直滚轮。
        PointerScrollAxis::Vertical => MOUSEEVENTF_WHEEL,
        // 映射水平滚轮。
        PointerScrollAxis::Horizontal => MOUSEEVENTF_HWHEEL,
    };
    // 负数按 Windows 补码位模式写入 mouseData。
    send_mouse_event(flags, delta as u32)
}

// 声明无输入副作用的坐标纯测试。
#[cfg(test)]
mod tests {
    // 导入被测私有原语。
    use super::*;

    // 验证负坐标多屏边界使用半开区间。
    #[test]
    fn signed_virtual_screen_bounds_are_half_open() {
        // 构造跨越主屏左侧与上侧的虚拟桌面。
        let bounds = VirtualScreenBounds {
            // 保存负左边界。
            x: -1920,
            // 保存负上边界。
            y: -1080,
            // 保存两屏宽度。
            width: 3840,
            // 保存两屏高度。
            height: 2160,
        };
        // 左上角属于边界。
        assert!(contains_point(
            bounds,
            PhysicalScreenPoint { x: -1920, y: -1080 }
        ));
        // 右下最后一个像素属于边界。
        assert!(contains_point(
            bounds,
            PhysicalScreenPoint { x: 1919, y: 1079 }
        ));
        // 右侧半开边界不属于桌面。
        assert!(!contains_point(
            bounds,
            PhysicalScreenPoint { x: 1920, y: 0 }
        ));
        // 上侧外部点不属于桌面。
        assert!(!contains_point(
            bounds,
            PhysicalScreenPoint { x: 0, y: -1081 }
        ));
    }

    // 验证三类按钮上下阶段都有唯一平台映射。
    #[test]
    fn all_button_phases_have_closed_platform_flags() {
        // 核对左键上下阶段不同。
        assert_ne!(
            button_flags(PointerButton::Left, true),
            button_flags(PointerButton::Left, false)
        );
        // 核对右键上下阶段不同。
        assert_ne!(
            button_flags(PointerButton::Right, true),
            button_flags(PointerButton::Right, false)
        );
        // 核对中键上下阶段不同。
        assert_ne!(
            button_flags(PointerButton::Middle, true),
            button_flags(PointerButton::Middle, false)
        );
    }
}
