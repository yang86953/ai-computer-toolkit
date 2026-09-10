//! 封装窗口状态、外框几何与最终状态读回所需的私有 Windows 调用。

// 导入窗口矩形、最后错误与消息参数。
use windows::Win32::{
    // 导入平台矩形、错误值与消息参数。
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_SUCCESS, GetLastError, LPARAM, RECT, SetLastError, WPARAM,
    },
    // 导入 DPI 上下文、窗口状态与定位调用。
    UI::{
        // 导入线程级 Per-Monitor-V2 与窗口 DPI 查询。
        HiDpi::{
            // 导入强类型 DPI 上下文。
            DPI_AWARENESS_CONTEXT,
            // 导入 Per-Monitor-V2 固定上下文。
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            // 导入窗口 DPI 与 DPI 感知系统指标。
            GetDpiForWindow,
            GetSystemMetricsForDpi,
            // 导入线程上下文切换函数。
            SetThreadDpiAwarenessContext,
        },
        // 导入固定系统命令、状态、几何与样式查询。
        WindowsAndMessaging::{
            // 导入窗口样式索引与读取。
            GWL_STYLE,
            // 导入窗口外框与虚拟桌面指标读取。
            GetSystemMetrics,
            GetWindowLongPtrW,
            GetWindowRect,
            // 导入最小化与最大化状态读取。
            IsIconic,
            IsZoomed,
            // 导入固定状态命令。
            PostMessageW,
            SC_MAXIMIZE,
            SC_MINIMIZE,
            SC_RESTORE,
            // 导入最小尺寸与虚拟桌面指标。
            SM_CXMINTRACK,
            SM_CXVIRTUALSCREEN,
            SM_CYMINTRACK,
            SM_CYVIRTUALSCREEN,
            SM_XVIRTUALSCREEN,
            SM_YVIRTUALSCREEN,
            // 导入不激活窗口定位调用与标志。
            SWP_ASYNCWINDOWPOS,
            SWP_NOACTIVATE,
            SWP_NOMOVE,
            SWP_NOSIZE,
            SWP_NOZORDER,
            SetWindowPos,
            // 导入窗口样式能力位。
            WM_SYSCOMMAND,
            WS_MAXIMIZEBOX,
            WS_MINIMIZEBOX,
            WS_SIZEBOX,
        },
    },
};

// 导入共享窗口身份、私有记录和 provider-neutral 动作。
use crate::{
    // 只在 Adapter 内恢复并核对原生窗口身份。
    adapters::{
        // 复用窗口 mutation 的当前 token 与进程代际核对。
        window_identity_windows::{native_window, same_window_identity},
        // 只接受重新发现的私有窗口记录。
        windows::WindowRecord,
    },
    // 导入公开协议尺寸上限与封闭动作。
    components::window_lifecycle_contract::{
        MAXIMUM_WINDOW_DIMENSION_PX, WindowLifecycleOperation,
    },
};

// 保存本次线程替换的 DPI 上下文。
struct WindowLifecycleDpiGuard {
    // 保存进入前上下文供 Drop 恢复。
    previous: DPI_AWARENESS_CONTEXT,
}

// 提供窗口外框调用专用 DPI 生命周期。
impl WindowLifecycleDpiGuard {
    // 把当前同步调用线程切换到 Per-Monitor-V2。
    fn enter() -> Result<Self, WindowLifecycleFailure> {
        // 设置物理坐标所需的线程上下文。
        let previous = unsafe {
            // 只改变当前同步窗口生命周期线程。
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)
        };
        // 无效旧上下文表示切换失败。
        if previous.is_invalid() {
            // 返回稳定坐标上下文缺口。
            return Err(WindowLifecycleFailure::CoordinateUnavailable);
        }
        // 返回拥有恢复责任的守卫。
        Ok(Self { previous })
    }
}

// 恢复调用前线程 DPI 上下文。
impl Drop for WindowLifecycleDpiGuard {
    // 执行成对恢复。
    fn drop(&mut self) {
        // Drop 不能因恢复失败 panic。
        let _ = unsafe {
            // 只恢复当前调用线程。
            SetThreadDpiAwarenessContext(self.previous)
        };
    }
}

// 表示 provider-neutral 窗口视觉状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowVisualState {
    // 表示非最小化且非最大化状态。
    Normal,
    // 表示最小化状态。
    Minimized,
    // 表示最大化状态。
    Maximized,
}

// 提供稳定公开窗口状态文本。
impl WindowVisualState {
    // 返回 schema 使用的状态名称。
    pub(crate) const fn as_str(self) -> &'static str {
        // 穷举三种窗口状态。
        match self {
            // 映射普通状态。
            Self::Normal => "normal",
            // 映射最小化状态。
            Self::Minimized => "minimized",
            // 映射最大化状态。
            Self::Maximized => "maximized",
        }
    }
}

// 保存 provider-neutral 物理像素窗口外框。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowRectangle {
    // 保存带符号横坐标。
    pub(crate) x: i32,
    // 保存带符号纵坐标。
    pub(crate) y: i32,
    // 保存正外框宽度。
    pub(crate) width: u32,
    // 保存正外框高度。
    pub(crate) height: u32,
}

// 保存 provider-neutral 物理像素尺寸。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowSize {
    // 保存正宽度。
    pub(crate) width: u32,
    // 保存正高度。
    pub(crate) height: u32,
}

// 保存一次经当前 token 与进程代际核对的窗口状态读回。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowLifecycleObservation {
    // 保存当前视觉状态。
    pub(crate) state: WindowVisualState,
    // 保存当前窗口外框。
    pub(crate) bounds: WindowRectangle,
    // 保存窗口当前 DPI。
    pub(crate) dpi: u32,
    // 保存当前 DPI 下系统最小 tracking size。
    pub(crate) minimum_size: WindowSize,
    // 保存当前虚拟桌面边界。
    pub(crate) virtual_screen: WindowRectangle,
    // 标记窗口样式是否允许最小化。
    supports_minimize: bool,
    // 标记窗口样式是否允许最大化。
    supports_maximize: bool,
    // 标记窗口样式是否允许调整大小。
    supports_resize: bool,
}

// 仅为跨 Module 纯测试构造不含平台句柄的观察事实。
#[cfg(test)]
pub(crate) const fn fixture_observation(
    // 接收 provider-neutral 状态。
    state: WindowVisualState,
    // 接收 provider-neutral 外框。
    bounds: WindowRectangle,
    // 接收稳定 DPI。
    dpi: u32,
    // 接收系统最小尺寸。
    minimum_size: WindowSize,
    // 接收虚拟桌面边界。
    virtual_screen: WindowRectangle,
) -> WindowLifecycleObservation {
    // 返回样式能力全部开启的纯测试观察。
    WindowLifecycleObservation {
        // 保存状态。
        state,
        // 保存外框。
        bounds,
        // 保存 DPI。
        dpi,
        // 保存最小尺寸。
        minimum_size,
        // 保存虚拟桌面。
        virtual_screen,
        // 允许最小化。
        supports_minimize: true,
        // 允许最大化。
        supports_maximize: true,
        // 允许缩放。
        supports_resize: true,
    }
}

// 表示窗口生命周期平台边界的封闭失败分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowLifecycleFailure {
    // 当前 token、所属进程或进程代际已经变化。
    Stale,
    // Windows 明确拒绝固定窗口调用。
    PermissionDenied,
    // 目标样式或当前状态不支持请求动作。
    Unsupported,
    // DPI、外框或虚拟桌面物理坐标无法认证。
    CoordinateUnavailable,
    // 请求几何超出当前环境安全边界。
    InvalidGeometry,
    // 其他平台调用失败。
    OperationFailed,
}

// 把 Win32 矩形转换为有界 provider-neutral 外框。
fn rectangle_from_win32(rectangle: RECT) -> Result<WindowRectangle, WindowLifecycleFailure> {
    // 使用 i64 计算宽度以避免有符号溢出。
    let width = i64::from(rectangle.right) - i64::from(rectangle.left);
    // 使用 i64 计算高度以避免有符号溢出。
    let height = i64::from(rectangle.bottom) - i64::from(rectangle.top);
    // 宽高必须为正且能进入稳定协议范围。
    let width = u32::try_from(width)
        // 过滤协议上界。
        .ok()
        // 只接受正宽度。
        .filter(|value| (1..=MAXIMUM_WINDOW_DIMENSION_PX).contains(value))
        // 无法表示时返回坐标上下文缺口。
        .ok_or(WindowLifecycleFailure::CoordinateUnavailable)?;
    // 高度执行同一稳定边界检查。
    let height = u32::try_from(height)
        // 过滤协议上界。
        .ok()
        // 只接受正高度。
        .filter(|value| (1..=MAXIMUM_WINDOW_DIMENSION_PX).contains(value))
        // 无法表示时返回坐标上下文缺口。
        .ok_or(WindowLifecycleFailure::CoordinateUnavailable)?;
    // 返回安全外框值。
    Ok(WindowRectangle {
        // 保存左边界。
        x: rectangle.left,
        // 保存上边界。
        y: rectangle.top,
        // 保存正宽度。
        width,
        // 保存正高度。
        height,
    })
}

// 读取当前虚拟桌面物理边界。
fn virtual_screen_bounds() -> Result<WindowRectangle, WindowLifecycleFailure> {
    // 读取带符号左边界。
    let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    // 读取带符号上边界。
    let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    // 读取虚拟桌面宽度。
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    // 读取虚拟桌面高度。
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    // 转为稳定正宽度。
    let width = u32::try_from(width)
        // 限制公开协议上界。
        .ok()
        // 只接受正且有界宽度。
        .filter(|value| (1..=MAXIMUM_WINDOW_DIMENSION_PX).contains(value))
        // 无法认证时失败闭合。
        .ok_or(WindowLifecycleFailure::CoordinateUnavailable)?;
    // 转为稳定正高度。
    let height = u32::try_from(height)
        // 限制公开协议上界。
        .ok()
        // 只接受正且有界高度。
        .filter(|value| (1..=MAXIMUM_WINDOW_DIMENSION_PX).contains(value))
        // 无法认证时失败闭合。
        .ok_or(WindowLifecycleFailure::CoordinateUnavailable)?;
    // 返回带符号虚拟桌面边界。
    Ok(WindowRectangle {
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

// 读取窗口样式并区分合法零值与平台失败。
fn window_style(target: &WindowRecord) -> Result<u32, WindowLifecycleFailure> {
    // 清空最后错误以区分合法零返回值。
    unsafe { SetLastError(ERROR_SUCCESS) };
    // 读取当前已核对 token 的窗口样式。
    let style = unsafe { GetWindowLongPtrW(native_window(target), GWL_STYLE) };
    // 零样式需要核对最后错误。
    if style == 0 && unsafe { GetLastError() } != ERROR_SUCCESS {
        // 竞态消失必须保持 stale 分类。
        return Err(identity_sensitive_failure(target));
    }
    // 保留 WINDOW_STYLE 的低 32 位位模式。
    Ok(style as u32)
}

// 读取同一当前 token 与进程代际下的状态、外框、DPI 和环境边界。
pub(crate) fn observe(
    // 接收 Module 已唯一解析的私有目标。
    target: &WindowRecord,
) -> Result<WindowLifecycleObservation, WindowLifecycleFailure> {
    // 每次读回都先核对当前 token、PID 与进程代际。
    if !same_window_identity(target) {
        // 禁止读回已检测到变化的 token；完全相同 token 回收仍受公开停止线约束。
        return Err(WindowLifecycleFailure::Stale);
    }
    // 建立物理像素线程上下文并由 Drop 恢复。
    let _dpi_guard = WindowLifecycleDpiGuard::enter()?;
    // 恢复私有窗口句柄。
    let window = native_window(target);
    // 初始化窗口外框载荷。
    let mut rectangle = RECT::default();
    // 读取当前窗口外框。
    unsafe { GetWindowRect(window, &mut rectangle) }
        // 竞态消失与仍存平台失败保持可区分。
        .map_err(|_| identity_sensitive_failure(target))?;
    // 转为稳定正外框。
    let bounds = rectangle_from_win32(rectangle)?;
    // 读取目标窗口当前 DPI。
    let dpi = unsafe { GetDpiForWindow(window) };
    // 零 DPI 无法认证物理几何语义。
    if dpi == 0 || dpi > 960 {
        // DPI 查询期间目标消失时保持 stale 分类。
        if !same_window_identity(target) {
            // 拒绝把竞态过期伪装为坐标缺口。
            return Err(WindowLifecycleFailure::Stale);
        }
        // 返回坐标上下文缺口。
        return Err(WindowLifecycleFailure::CoordinateUnavailable);
    }
    // 读取当前 DPI 下最小 tracking 宽度。
    let minimum_width = unsafe { GetSystemMetricsForDpi(SM_CXMINTRACK, dpi) };
    // 读取当前 DPI 下最小 tracking 高度。
    let minimum_height = unsafe { GetSystemMetricsForDpi(SM_CYMINTRACK, dpi) };
    // 转换最小宽度。
    let minimum_width = u32::try_from(minimum_width)
        // 只接受正且可公开的值。
        .ok()
        // 应用稳定协议范围。
        .filter(|value| (1..=MAXIMUM_WINDOW_DIMENSION_PX).contains(value))
        // 无法认证时失败闭合。
        .ok_or(WindowLifecycleFailure::CoordinateUnavailable)?;
    // 转换最小高度。
    let minimum_height = u32::try_from(minimum_height)
        // 只接受正且可公开的值。
        .ok()
        // 应用稳定协议范围。
        .filter(|value| (1..=MAXIMUM_WINDOW_DIMENSION_PX).contains(value))
        // 无法认证时失败闭合。
        .ok_or(WindowLifecycleFailure::CoordinateUnavailable)?;
    // 读取样式能力位。
    let style = window_style(target)?;
    // 最小化优先于最大化判定。
    let state = if unsafe { IsIconic(window) }.as_bool() {
        // 返回最小化状态。
        WindowVisualState::Minimized
    // 非最小化时核对最大化。
    } else if unsafe { IsZoomed(window) }.as_bool() {
        // 返回最大化状态。
        WindowVisualState::Maximized
    } else {
        // 其余状态统一为普通。
        WindowVisualState::Normal
    };
    // 构造完整 provider-neutral 观察事实。
    let observation = WindowLifecycleObservation {
        // 保存状态。
        state,
        // 保存外框。
        bounds,
        // 保存 DPI。
        dpi,
        // 保存系统最小尺寸。
        minimum_size: WindowSize {
            // 保存最小宽度。
            width: minimum_width,
            // 保存最小高度。
            height: minimum_height,
        },
        // 保存虚拟桌面边界。
        virtual_screen: virtual_screen_bounds()?,
        // 从封闭样式位推导最小化支持。
        supports_minimize: style & WS_MINIMIZEBOX.0 != 0,
        // 从封闭样式位推导最大化支持。
        supports_maximize: style & WS_MAXIMIZEBOX.0 != 0,
        // 从封闭样式位推导缩放支持。
        supports_resize: style & WS_SIZEBOX.0 != 0,
    };
    // 读回结束时再次核对当前 token 与进程代际。
    if !same_window_identity(target) {
        // 禁止返回跨已检测身份变化拼接的事实。
        return Err(WindowLifecycleFailure::Stale);
    }
    // 返回经当前身份材料认证的观察。
    Ok(observation)
}

// 判断两个外框是否至少相交一个物理像素。
fn rectangles_intersect(first: WindowRectangle, second: WindowRectangle) -> bool {
    // 使用 i64 计算第一矩形右边界。
    let first_right = i64::from(first.x) + i64::from(first.width);
    // 使用 i64 计算第一矩形下边界。
    let first_bottom = i64::from(first.y) + i64::from(first.height);
    // 使用 i64 计算第二矩形右边界。
    let second_right = i64::from(second.x) + i64::from(second.width);
    // 使用 i64 计算第二矩形下边界。
    let second_bottom = i64::from(second.y) + i64::from(second.height);
    // 核对两轴半开区间都具有正交集。
    i64::from(first.x) < second_right
        // 核对反向横轴交集。
        && i64::from(second.x) < first_right
        // 核对纵轴交集。
        && i64::from(first.y) < second_bottom
        // 核对反向纵轴交集。
        && i64::from(second.y) < first_bottom
}

// 在任何平台 mutation 前验证样式、状态与环境几何边界。
fn validate_operation(
    // 接收当前身份材料下的观察。
    observation: WindowLifecycleObservation,
    // 接收 provider-neutral 请求动作。
    operation: WindowLifecycleOperation,
) -> Result<(), WindowLifecycleFailure> {
    // 按封闭动作执行专属检查。
    match operation {
        // 恢复命令对三种认证状态均有定义。
        WindowLifecycleOperation::Restore => Ok(()),
        // 最小化要求窗口样式明确支持。
        WindowLifecycleOperation::Minimize if observation.supports_minimize => Ok(()),
        // 最大化要求窗口样式明确支持。
        WindowLifecycleOperation::Maximize if observation.supports_maximize => Ok(()),
        // 移动只认证普通状态，避免隐式恢复或破坏最大化布局。
        WindowLifecycleOperation::Move {
            coordinate_space: _,
            x,
            y,
        } if observation.state == WindowVisualState::Normal => {
            // 构造保持当前尺寸的请求外框。
            let requested = WindowRectangle {
                // 保存调用方横坐标。
                x,
                // 保存调用方纵坐标。
                y,
                // 保持当前宽度。
                width: observation.bounds.width,
                // 保持当前高度。
                height: observation.bounds.height,
            };
            // 请求外框必须仍与任一当前显示区域总边界相交。
            if rectangles_intersect(requested, observation.virtual_screen) {
                // 几何范围有效。
                Ok(())
            } else {
                // 完全移出虚拟桌面时失败闭合。
                Err(WindowLifecycleFailure::InvalidGeometry)
            }
        }
        // 缩放只认证普通且可调整窗口。
        WindowLifecycleOperation::Resize {
            coordinate_space: _,
            width,
            height,
        } if observation.state == WindowVisualState::Normal && observation.supports_resize => {
            // 同时核对系统最小 tracking size 与虚拟桌面跨度。
            if width >= observation.minimum_size.width
                // 核对最小高度。
                && height >= observation.minimum_size.height
                // 核对虚拟桌面宽度上界。
                && width <= observation.virtual_screen.width
                // 核对虚拟桌面高度上界。
                && height <= observation.virtual_screen.height
            {
                // 尺寸范围有效。
                Ok(())
            } else {
                // 环境尺寸不满足时失败闭合。
                Err(WindowLifecycleFailure::InvalidGeometry)
            }
        }
        // 其余样式或状态组合不进行隐式恢复与降级。
        _ => Err(WindowLifecycleFailure::Unsupported),
    }
}

// 在平台失败后按当前 token 与进程代际生成封闭分类。
fn identity_sensitive_failure(target: &WindowRecord) -> WindowLifecycleFailure {
    // 目标竞态消失或可检测的 token 变化必须优先归类 stale。
    if !same_window_identity(target) {
        // 返回精确目标过期。
        WindowLifecycleFailure::Stale
    } else {
        // 身份仍有效时保留通用平台失败。
        WindowLifecycleFailure::OperationFailed
    }
}

// 从紧邻失败调用的最后错误生成封闭 dispatch 分类。
fn dispatch_failure(target: &WindowRecord) -> WindowLifecycleFailure {
    // 读取当前线程最后错误。
    let error = unsafe { GetLastError() };
    // 失败后先重新认证当前 token 与进程代际。
    if !same_window_identity(target) {
        // token 竞态消失或可检测的变化保持 stale。
        return WindowLifecycleFailure::Stale;
    }
    // 明确访问拒绝映射权限失败。
    if error == ERROR_ACCESS_DENIED {
        // 返回权限分类。
        WindowLifecycleFailure::PermissionDenied
    } else {
        // 其余失败保持通用操作错误。
        WindowLifecycleFailure::OperationFailed
    }
}

// 只发送一个固定窗口系统命令。
fn post_system_command(
    // 接收经当前身份材料认证的私有目标。
    target: &WindowRecord,
    // 接收封闭系统命令值。
    command: u32,
) -> Result<(), WindowLifecycleFailure> {
    // 清空最后错误以取得紧邻失败原因。
    unsafe { SetLastError(ERROR_SUCCESS) };
    // 只向精确窗口排队固定 WM_SYSCOMMAND。
    unsafe {
        PostMessageW(
            // 使用精确窗口句柄。
            Some(native_window(target)),
            // 固定系统命令消息。
            WM_SYSCOMMAND,
            // 传入封闭命令值。
            WPARAM(command as usize),
            // 不传递任意平台参数。
            LPARAM(0),
        )
    }
    // 把平台失败压缩为封闭分类。
    .map_err(|_| dispatch_failure(target))
}

// 执行恰好一次已预检窗口状态或几何调用。
pub(crate) fn dispatch(
    // 接收 Module 已唯一解析的私有目标。
    target: &WindowRecord,
    // 接收严格 provider-neutral 动作。
    operation: WindowLifecycleOperation,
) -> Result<(), WindowLifecycleFailure> {
    // 写前最后一次核对当前 token 与进程代际。
    if !same_window_identity(target) {
        // 禁止向已检测到身份变化的 token 写入。
        return Err(WindowLifecycleFailure::Stale);
    }
    // 整个 dispatch 保持 Per-Monitor-V2 线程上下文。
    let _dpi_guard = WindowLifecycleDpiGuard::enter()?;
    // 写前重新读取样式、状态和当前几何边界。
    let observation = observe(target)?;
    // 在事实建立点前完成全部动作检查。
    validate_operation(observation, operation)?;
    // 按封闭动作选择固定平台调用。
    match operation {
        // 恢复只发送固定 SC_RESTORE。
        WindowLifecycleOperation::Restore => post_system_command(target, SC_RESTORE),
        // 最小化只发送固定 SC_MINIMIZE。
        WindowLifecycleOperation::Minimize => post_system_command(target, SC_MINIMIZE),
        // 最大化只发送固定 SC_MAXIMIZE。
        WindowLifecycleOperation::Maximize => post_system_command(target, SC_MAXIMIZE),
        // 移动保持尺寸、Z 序和激活状态。
        WindowLifecycleOperation::Move {
            coordinate_space: _,
            x,
            y,
        } => {
            // 清空最后错误以取得紧邻失败原因。
            unsafe { SetLastError(ERROR_SUCCESS) };
            // 使用异步不激活定位调用。
            unsafe {
                SetWindowPos(
                    // 使用精确窗口句柄。
                    native_window(target),
                    // 不提供 Z 序目标。
                    None,
                    // 设置新横坐标。
                    x,
                    // 设置新纵坐标。
                    y,
                    // 忽略宽度参数。
                    0,
                    // 忽略高度参数。
                    0,
                    // 禁止激活、Z 序和尺寸变化。
                    SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSIZE,
                )
            }
            // 压缩平台失败分类。
            .map_err(|_| dispatch_failure(target))
        }
        // 缩放保持位置、Z 序和激活状态。
        WindowLifecycleOperation::Resize {
            coordinate_space: _,
            width,
            height,
        } => {
            // 已验证协议上界保证可转换为 i32。
            let width =
                i32::try_from(width).map_err(|_| WindowLifecycleFailure::InvalidGeometry)?;
            // 已验证协议上界保证可转换为 i32。
            let height =
                i32::try_from(height).map_err(|_| WindowLifecycleFailure::InvalidGeometry)?;
            // 清空最后错误以取得紧邻失败原因。
            unsafe { SetLastError(ERROR_SUCCESS) };
            // 使用异步不激活定位调用。
            unsafe {
                SetWindowPos(
                    // 使用精确窗口句柄。
                    native_window(target),
                    // 不提供 Z 序目标。
                    None,
                    // 忽略横坐标参数。
                    0,
                    // 忽略纵坐标参数。
                    0,
                    // 设置新外框宽度。
                    width,
                    // 设置新外框高度。
                    height,
                    // 禁止激活、Z 序和位置变化。
                    SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOMOVE,
                )
            }
            // 压缩平台失败分类。
            .map_err(|_| dispatch_failure(target))
        }
    }
}

// 判断当前身份材料下的读回是否达到请求最终状态。
pub(crate) fn operation_reached(
    // 接收当前观察事实。
    observation: WindowLifecycleObservation,
    // 接收原请求动作。
    operation: WindowLifecycleOperation,
) -> bool {
    // 按封闭动作核对最终事实。
    match operation {
        // 恢复必须回到普通状态。
        WindowLifecycleOperation::Restore => observation.state == WindowVisualState::Normal,
        // 最小化必须读回最小化状态。
        WindowLifecycleOperation::Minimize => observation.state == WindowVisualState::Minimized,
        // 最大化必须读回最大化状态。
        WindowLifecycleOperation::Maximize => observation.state == WindowVisualState::Maximized,
        // 移动必须保持请求外框左上角。
        WindowLifecycleOperation::Move { x, y, .. } => {
            // 同时核对两个带符号坐标。
            observation.bounds.x == x && observation.bounds.y == y
        }
        // 缩放必须读回请求外框尺寸。
        WindowLifecycleOperation::Resize { width, height, .. } => {
            // 同时核对两个物理像素维度。
            observation.bounds.width == width && observation.bounds.height == height
        }
    }
}

// 声明无窗口副作用的几何与最终状态纯测试。
#[cfg(test)]
mod tests {
    // 导入被测私有原语。
    use super::*;
    // 导入公开坐标空间。
    use crate::components::window_lifecycle_contract::WindowLifecycleCoordinateSpace;

    // 构造可调整普通窗口观察夹具。
    const fn observation() -> WindowLifecycleObservation {
        // 返回跨越负坐标虚拟桌面的稳定事实。
        WindowLifecycleObservation {
            // 使用普通状态。
            state: WindowVisualState::Normal,
            // 使用稳定窗口外框。
            bounds: WindowRectangle {
                // 设置横坐标。
                x: 10,
                // 设置纵坐标。
                y: 20,
                // 设置宽度。
                width: 800,
                // 设置高度。
                height: 600,
            },
            // 使用标准 DPI。
            dpi: 96,
            // 使用稳定系统最小尺寸。
            minimum_size: WindowSize {
                // 设置最小宽度。
                width: 120,
                // 设置最小高度。
                height: 80,
            },
            // 构造双屏虚拟桌面。
            virtual_screen: WindowRectangle {
                // 左侧显示器使用负坐标。
                x: -1920,
                // 顶边从零开始。
                y: 0,
                // 使用双屏宽度。
                width: 3840,
                // 使用单屏高度。
                height: 1080,
            },
            // 允许最小化。
            supports_minimize: true,
            // 允许最大化。
            supports_maximize: true,
            // 允许调整尺寸。
            supports_resize: true,
        }
    }

    // 验证窗口必须至少保留一个物理像素与虚拟桌面相交。
    #[test]
    fn move_geometry_requires_visible_intersection() {
        // 构造合法负坐标移动。
        let visible = WindowLifecycleOperation::Move {
            // 使用认证物理坐标空间。
            coordinate_space: WindowLifecycleCoordinateSpace::ScreenPhysicalPx,
            // 让窗口部分位于左屏。
            x: -2_000,
            // 保持纵向可见。
            y: 10,
        };
        // 部分相交移动必须通过。
        assert_eq!(validate_operation(observation(), visible), Ok(()));
        // 构造完全离开虚拟桌面的移动。
        let invisible = WindowLifecycleOperation::Move {
            // 使用认证物理坐标空间。
            coordinate_space: WindowLifecycleCoordinateSpace::ScreenPhysicalPx,
            // 完全越过右边界。
            x: 2_000,
            // 保持纵坐标合法。
            y: 10,
        };
        // 完全不可见移动必须结构化拒绝。
        assert_eq!(
            validate_operation(observation(), invisible),
            Err(WindowLifecycleFailure::InvalidGeometry)
        );
    }

    // 验证缩放同时服从系统最小尺寸和虚拟桌面跨度。
    #[test]
    fn resize_geometry_uses_environment_bounds() {
        // 构造合法尺寸。
        let valid = WindowLifecycleOperation::Resize {
            // 使用认证物理坐标空间。
            coordinate_space: WindowLifecycleCoordinateSpace::ScreenPhysicalPx,
            // 使用普通宽度。
            width: 1_024,
            // 使用普通高度。
            height: 768,
        };
        // 合法尺寸必须通过。
        assert_eq!(validate_operation(observation(), valid), Ok(()));
        // 构造小于系统 minimum tracking size 的尺寸。
        let too_small = WindowLifecycleOperation::Resize {
            // 使用认证物理坐标空间。
            coordinate_space: WindowLifecycleCoordinateSpace::ScreenPhysicalPx,
            // 小于系统最小宽度。
            width: 119,
            // 保持高度合法。
            height: 768,
        };
        // 过小尺寸必须失败闭合。
        assert_eq!(
            validate_operation(observation(), too_small),
            Err(WindowLifecycleFailure::InvalidGeometry)
        );
    }

    // 验证最终状态只以精确读回事实为准。
    #[test]
    fn operation_completion_requires_exact_state_or_geometry() {
        // 普通观察满足恢复动作。
        assert!(operation_reached(
            observation(),
            WindowLifecycleOperation::Restore
        ));
        // 普通观察不满足最小化动作。
        assert!(!operation_reached(
            observation(),
            WindowLifecycleOperation::Minimize
        ));
        // 精确当前位置满足移动动作。
        assert!(operation_reached(
            observation(),
            WindowLifecycleOperation::Move {
                // 使用认证物理坐标空间。
                coordinate_space: WindowLifecycleCoordinateSpace::ScreenPhysicalPx,
                // 匹配当前横坐标。
                x: 10,
                // 匹配当前纵坐标。
                y: 20,
            }
        ));
        // 一像素偏差不构成最终状态。
        assert!(!operation_reached(
            observation(),
            WindowLifecycleOperation::Resize {
                // 使用认证物理坐标空间。
                coordinate_space: WindowLifecycleCoordinateSpace::ScreenPhysicalPx,
                // 宽度偏差一像素。
                width: 801,
                // 高度保持一致。
                height: 600,
            }
        ));
    }
}
