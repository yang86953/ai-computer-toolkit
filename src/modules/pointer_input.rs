//! 组合指针契约、精确窗口、权限与 Windows Adapter 的同步 Command Module。

// 导入请求内按钮集合与单调时钟。
use std::{
    // 导入有序按钮集合。
    collections::BTreeSet,
    // 导入单调时间类型。
    time::{Duration, Instant},
};

// 导入公开 JSON 值与构造宏。
use serde_json::{Value, json};

// 引入当前 Module 私有封闭错误类型。
#[path = "pointer_input_error.rs"]
mod error_code;

// 引入当前 Module 私有中断生命周期控制器。
mod interrupt {
    pub(super) use crate::components::input_deadline::InterruptDeadline;
}

// 导入当前 Module 私有错误集合。
use error_code::PointerInputErrorCode;
// 导入当前 Module 私有中断生命周期控制器。
use interrupt::InterruptDeadline;

// 导入窄 Adapter、契约、权限与统一结果。
use crate::{
    // 导入平台事实和精确窗口重新发现。
    adapters::{
        // 导入键鼠共享的精确窗口生命周期调用。
        foreground_input_windows::{
            // 导入前景事实。
            foreground_matches,
            // 导入窗口恢复和激活调用。
            restore_window,
            try_activate_window,
        },
        // 导入窄 Windows 指针 Adapter。
        pointer_input_windows::{
            // 导入 DPI 生命周期守卫。
            PointerDpiGuard,
            // 导入按钮与滚轮调度。
            dispatch_button,
            dispatch_scroll,
            // 导入光标移动。
            move_pointer,
            // 导入命中测试与当前坐标解析。
            point_targets_window,
            resolve_physical_point,
        },
        // 导入可见窗口快照与 canonical 重解析。
        window::{capture_visible_titled_windows, resolve_window},
        // 导入进程权限事实与私有窗口记录。
        windows::{
            // 导入相对完整性与元数据访问分类。
            IntegrityRelation,
            ProcessMetadataAccess,
            // 导入私有窗口记录。
            WindowRecord,
            // 导入有界进程清单。
            enumerate_process_inventory,
        },
    },
    // 导入版本化 capability ID。
    capabilities,
    // 导入取消、指针契约与静态权限 Component。
    components::{
        // 导入进程级取消信号。
        cancellation,
        // 导入 provider-neutral 指针领域类型。
        pointer_input_contract::{
            // 导入按钮与阶段。
            PointerButton,
            PointerButtonPhase,
            // 导入请求与二维点。
            PointerInput,
            PointerPoint,
            // 导入步骤类型。
            PointerStep,
            // 导入确定性拖拽插值。
            interpolate_pointer_point,
            // 导入解析入口。
            parse_pointer_input,
        },
        // 导入无主动写探针的静态权限评估。
        static_permission_assessment::{
            // 导入 provider-neutral 权限输入。
            StaticIntegrityRelation,
            StaticMetadataAccess,
            // 导入权限纯函数。
            assess_static_background_mutation,
        },
    },
    // 导入统一错误和结果类型。
    domain::{AppControlError, AppResult},
};

// 限制一次权限预检最多枚举的进程数。
const PROCESS_INVENTORY_LIMIT: usize = 65_536;
// 固定前景激活尝试次数。
const FOREGROUND_ACTIVATION_ATTEMPTS: usize = 5;
// 固定窗口恢复后的短等待。
const WINDOW_RESTORE_WAIT: Duration = Duration::from_millis(80);
// 固定前景激活尝试间隔。
const FOREGROUND_ACTIVATION_WAIT: Duration = Duration::from_millis(50);

// 仅在测试构建中记录状态机已经取得的按钮释放责任。
#[cfg(test)]
static TEST_ACCEPTED_BUTTON_DOWNS: std::sync::atomic::AtomicUsize =
    // 初始化为零，不进入生产二进制。
    std::sync::atomic::AtomicUsize::new(0);
// 仅在测试构建中记录 Adapter 已接受的按钮释放。
#[cfg(test)]
static TEST_ACCEPTED_BUTTON_UPS: std::sync::atomic::AtomicUsize =
    // 初始化为零，不进入生产二进制。
    std::sync::atomic::AtomicUsize::new(0);

// 保存一次同步 Command 的请求内执行状态。
struct ExecutionState {
    input_events_sent: usize,
    // 保存当前请求已经按下且尚未释放的按钮。
    held_buttons: BTreeSet<PointerButton>,
    // 保存已完成公开步骤的安全摘要。
    completed_steps: Vec<Value>,
    // 保存当前步骤索引。
    current_step: Option<usize>,
    // 保存当前稳定阶段名称。
    current_stage: &'static str,
    // 标记光标或鼠标事件可能已经被接受。
    pointer_effect_seen: bool,
    // 标记恢复或前景请求可能产生主机影响。
    foreground_effect_started: bool,
    // 标记目标是否被恢复。
    target_restored: bool,
    // 标记目标已经成为前景。
    target_activated: bool,
}

// 提供空执行状态。
impl ExecutionState {
    // 构造调用前状态。
    fn new() -> Self {
        // 返回全部影响标记为假的初始状态。
        Self {
            input_events_sent: 0, // 初始没有工具持有按钮。
            held_buttons: BTreeSet::new(),
            // 初始没有已完成步骤。
            completed_steps: Vec::new(),
            // 初始没有当前步骤。
            current_step: None,
            // 初始处于门禁阶段。
            current_stage: "pre-dispatch-gates",
            // 初始没有指针影响。
            pointer_effect_seen: false,
            // 初始没有前景影响。
            foreground_effect_started: false,
            // 初始没有恢复目标。
            target_restored: false,
            // 初始没有激活目标。
            target_activated: false,
        }
    }

    // 记录一个公开步骤已经完整结束。
    fn complete_step(&mut self, index: usize, step: &PointerStep) {
        // 追加不含坐标或 native 事实的稳定摘要。
        self.completed_steps.push(json!({
            // 保存公开步骤索引。
            "index": index,
            // 保存封闭步骤类别。
            "type": step.kind(),
            // 保存完成状态。
            "status": "completed",
        }));
    }
}

// 保存 best-effort 安全释放结果。
struct ReleaseReport {
    // 标记是否存在需要释放的按钮。
    attempted: bool,
    // 标记全部按钮是否已确认释放。
    succeeded: bool,
    // 保存释放后仍可能由工具持有的按钮名称。
    remaining: Vec<&'static str>,
}

// 将 Windows 元数据访问事实映射为纯 Component 输入。
const fn metadata_access(value: ProcessMetadataAccess) -> StaticMetadataAccess {
    // 穷举全部平台分类。
    match value {
        // 映射可用状态。
        ProcessMetadataAccess::Available => StaticMetadataAccess::Available,
        // 映射明确权限阻塞。
        ProcessMetadataAccess::PermissionBlocked => StaticMetadataAccess::PermissionBlocked,
        // 映射暂不可用状态。
        ProcessMetadataAccess::Unavailable => StaticMetadataAccess::Unavailable,
    }
}

// 将 Windows 完整性关系映射为纯 Component 输入。
const fn integrity_relation(value: IntegrityRelation) -> StaticIntegrityRelation {
    // 穷举全部平台分类。
    match value {
        // 映射较低目标。
        IntegrityRelation::Lower => StaticIntegrityRelation::Lower,
        // 映射同级目标。
        IntegrityRelation::Same => StaticIntegrityRelation::Same,
        // 映射较高目标。
        IntegrityRelation::Higher => StaticIntegrityRelation::Higher,
        // 映射未知关系。
        IntegrityRelation::Unknown => StaticIntegrityRelation::Unknown,
    }
}

// 在当前可见窗口清单中唯一重新解析 canonical s2:w。
fn current_target(session_id: &str) -> AppResult<WindowRecord> {
    // 捕获完整有界窗口快照。
    let windows = capture_visible_titled_windows()?;
    // 要求当前快照唯一命中并复制私有记录。
    Ok(resolve_window(session_id, &windows)?.clone())
}

// 对目标进程执行无主动写探针的静态权限预检。
fn permission_preflight(window: &WindowRecord) -> AppResult<&'static str> {
    // 枚举有界进程权限清单。
    let inventory = enumerate_process_inventory(PROCESS_INVENTORY_LIMIT)?;
    // 截断清单不能证明唯一权限关系。
    if !inventory.complete {
        // 返回明确 assessment 缺口。
        return Err(
            PointerInputErrorCode::CapabilityAssessmentUnavailable.with_details(
                "The pointer target process inventory exceeds the certified bound.",
                // 返回稳定不重试事实。
                json!({
                    "reason": "process-inventory-limit-exceeded",
                    "retrySafe": false,
                }),
            ),
        );
    }
    // 使用 PID 与创建时间关联同一进程代际。
    let process = inventory.records.iter().find(|process| {
        // 核对私有 PID。
        process.process_id == window.process_id
            // 核对私有进程创建时间。
            && process.process_creation_time == window.process_creation_time
    });
    // 缺失进程代际表示窗口已过期。
    let Some(process) = process else {
        // 返回 canonical stale 语义。
        return Err(AppControlError::new(
            "STALE_SESSION",
            "The exact pointer target process generation is unavailable.",
        ));
    };
    // 执行纯静态权限评估。
    let assessment = assess_static_background_mutation(
        // 传播元数据访问事实。
        metadata_access(process.metadata_access),
        // 传播相对完整性事实。
        integrity_relation(process.integrity_relation),
    );
    // 按封闭决策 fail closed。
    match assessment.decision {
        // 同级或较低完整性在逐操作确认后允许尝试前台输入。
        "requires-confirmation" => Ok(assessment.permission_relation),
        // 明确权限阻塞不得调用 SendInput。
        "permission-blocked" => Err(PointerInputErrorCode::PermissionDenied.with_details(
            "Static permission facts block pointer input to the exact target.",
            // 只公开稳定相对权限原因。
            json!({
                "permissionRelation": assessment.permission_relation,
                "activeWriteProbePerformed": assessment.active_write_probe_performed,
                "retrySafe": false,
            }),
        )),
        // 未知或新增决策不得放宽。
        _ => Err(
            PointerInputErrorCode::CapabilityAssessmentUnavailable.with_details(
                "Static permission facts cannot certify pointer input to the exact target.",
                // 只公开稳定相对权限原因。
                json!({
                    "permissionRelation": assessment.permission_relation,
                    "activeWriteProbePerformed": assessment.active_write_probe_performed,
                    "retrySafe": false,
                }),
            ),
        ),
    }
}

// 在任何前景影响前验证全部坐标端点。
fn prevalidate_points(window: &WindowRecord, input: &PointerInput) -> AppResult<()> {
    // 按公开顺序检查每个步骤端点。
    for step in &input.steps {
        // 按步骤形状选择坐标集合。
        match step {
            // 单点移动。
            PointerStep::Move { point }
            // 单点按钮。
            | PointerStep::Button { point, .. }
            // 单点点击。
            | PointerStep::Click { point, .. }
            // 单点滚轮。
            | PointerStep::Scroll { point, .. } => {
                // 只验证坐标范围，不在激活前执行命中测试。
                let _ = resolve_physical_point(window, input.coordinate_space, *point)?;
            }
            // 拖拽验证两个端点。
            PointerStep::Drag { start, end, .. } => {
                // 验证起点。
                let _ = resolve_physical_point(window, input.coordinate_space, *start)?;
                // 验证终点。
                let _ = resolve_physical_point(window, input.coordinate_space, *end)?;
            }
        }
    }
    // 全部端点有效。
    Ok(())
}

// 恢复并有界激活精确窗口。
fn activate_target(
    // 接收原 canonical 目标。
    session_id: &str,
    // 接收执行状态。
    state: &mut ExecutionState,
    // 接收单调中断生命周期控制器。
    interrupt: &InterruptDeadline,
) -> AppResult<WindowRecord> {
    // 在任何前景影响前检查中断。
    interrupt.check()?;
    // 使用时重新发现目标。
    let mut window = current_target(session_id)?;
    // 记录恢复阶段。
    state.current_stage = "restore-target";
    // 仅在最小化或不可见时请求恢复。
    if restore_window(&window)? {
        // 标记已发生可能的前景影响。
        state.foreground_effect_started = true;
        // 保存恢复证据。
        state.target_restored = true;
        // 等待窗口完成异步恢复。
        interrupt.pause(WINDOW_RESTORE_WAIT)?;
    }
    // 尝试有界激活。
    for _attempt in 0..FOREGROUND_ACTIVATION_ATTEMPTS {
        // 每次尝试前检查中断。
        interrupt.check()?;
        // 每次尝试都重新发现 canonical 目标。
        window = current_target(session_id)?;
        // 已是前景时不再请求系统影响。
        if foreground_matches(&window) {
            // 标记目标已激活。
            state.target_activated = true;
            // 返回当前窗口事实。
            return Ok(window);
        }
        // 记录前景请求阶段。
        state.current_stage = "activate-target";
        // SetForegroundWindow 请求可能改变主机前景。
        state.foreground_effect_started = true;
        // 尝试激活并以实际前景事实为准。
        if try_activate_window(&window)? {
            // 标记目标已激活。
            state.target_activated = true;
            // 返回当前窗口事实。
            return Ok(window);
        }
        // 在下一次尝试前执行短可取消等待。
        interrupt.pause(FOREGROUND_ACTIVATION_WAIT)?;
    }
    // 穷尽重试后保持零指针事件保证。
    Err(PointerInputErrorCode::ForegroundActivationFailed
        .error("The exact pointer target could not be established as the foreground window."))
}

// 确认精确目标仍是前景窗口。
fn ensure_target_foreground(window: &WindowRecord) -> AppResult<()> {
    // 前景匹配时允许继续。
    if foreground_matches(window) {
        // 返回成功。
        return Ok(());
    }
    // 前景变化必须停止新输入。
    Err(PointerInputErrorCode::HostInterferenceDetected
        .error("The foreground window changed during pointer input."))
}

// 在当前窗口事实上准备一个精确物理点。
fn prepare_point(
    // 接收原 canonical 目标。
    target: PointTarget,
    // 接收已验证请求。
    input: &PointerInput,
    // 接收公开点。
    point: PointerPoint,
    // 标记该点必须当前命中精确目标。
    require_target_hit: bool,
    // 接收执行状态。
    state: &mut ExecutionState,
    // 接收单调中断生命周期控制器。
    interrupt: &InterruptDeadline,
) -> AppResult<crate::adapters::pointer_input_windows::PhysicalScreenPoint> {
    // 在重新发现前检查中断。
    interrupt.check()?;
    let session_id = match target {
        PointTarget::Desktop(check) => {
            check(Some(point))?;
            return Ok(
                crate::adapters::pointer_input_windows::PhysicalScreenPoint {
                    x: point.x,
                    y: point.y,
                },
            );
        }
        PointTarget::Window(id) => id,
    };
    // 每个点都重新发现目标以适应窗口移动。
    let window = current_target(session_id)?;
    // 禁止在其他窗口成为前景后继续。
    ensure_target_foreground(&window)?;
    // 按当前窗口位置解析物理坐标。
    state.current_stage = "resolve-current-point";
    // 执行 DPI-aware 坐标转换与虚拟桌面范围验证。
    let physical = resolve_physical_point(&window, input.coordinate_space, point)?;
    // 首次接触点必须实际命中目标或其子窗口。
    if require_target_hit && !point_targets_window(&window, physical)? {
        // 返回精确命中失败。
        return Err(PointerInputErrorCode::PointerTargetNotHit
            .error("The pointer point does not currently hit the exact target window."));
    }
    // 在平台调用前再次检查中断与前景。
    interrupt.check()?;
    // 禁止窗口在坐标转换后失去前景。
    ensure_target_foreground(&window)?;
    // 返回已验证物理点。
    Ok(physical)
}

// 移动到公开点并记录已接受影响。
fn move_to(
    // 接收原 canonical 目标。
    target: PointTarget,
    // 接收已验证请求。
    input: &PointerInput,
    // 接收公开点。
    point: PointerPoint,
    // 标记该点必须命中精确目标。
    require_target_hit: bool,
    // 接收执行状态。
    state: &mut ExecutionState,
    // 接收单调中断生命周期控制器。
    interrupt: &InterruptDeadline,
) -> AppResult<()> {
    // 解析当前物理点并核对命中。
    let physical = prepare_point(target, input, point, require_target_hit, state, interrupt)?;
    // 记录平台移动阶段。
    state.current_stage = "move-pointer";
    // 调用窄 Windows Adapter。
    move_pointer(physical)?;
    state.input_events_sent += 1;
    // 成功返回表示光标影响已发生。
    state.pointer_effect_seen = true;
    // 返回成功。
    Ok(())
}

// 调度一个按钮阶段并维护请求内所有权。
fn dispatch_tracked_button(
    // 接收按钮。
    button: PointerButton,
    // 接收是否按下。
    pressed: bool,
    // 接收执行状态。
    state: &mut ExecutionState,
) -> AppResult<()> {
    // 记录平台按钮阶段。
    state.current_stage = if pressed {
        // 标记按下阶段。
        "button-down"
    } else {
        // 标记释放阶段。
        "button-up"
    };
    // 单事件调度避免批量部分接收歧义。
    dispatch_button(button, pressed)?;
    state.input_events_sent += 1;
    // 成功返回表示按钮事件已接受。
    state.pointer_effect_seen = true;
    // 按阶段维护请求内持有集合。
    if pressed {
        // 按下后取得释放责任。
        state.held_buttons.insert(button);
        // 仅向项目自有动态测试发布释放责任已经建立。
        #[cfg(test)]
        TEST_ACCEPTED_BUTTON_DOWNS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    } else {
        // 释放后清除责任。
        state.held_buttons.remove(&button);
        // 仅向项目自有动态测试发布 Adapter 已接受释放。
        #[cfg(test)]
        TEST_ACCEPTED_BUTTON_UPS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
    // 返回成功。
    Ok(())
}

// 执行一个已验证公开步骤。
fn execute_step(
    // 接收原 canonical 目标。
    target: PointTarget,
    // 接收完整请求。
    input: &PointerInput,
    // 接收当前步骤。
    step: &PointerStep,
    // 接收执行状态。
    state: &mut ExecutionState,
    // 接收单调中断生命周期控制器。
    interrupt: &InterruptDeadline,
) -> AppResult<()> {
    // 按封闭步骤类型调度。
    match step {
        // 移动允许在当前请求持有按钮期间离开窗口。
        PointerStep::Move { point } => move_to(
            target,
            input,
            *point,
            // 未持有按钮时要求精确命中。
            state.held_buttons.is_empty(),
            state,
            interrupt,
        ),
        // 显式按钮阶段。
        PointerStep::Button {
            button,
            phase,
            point,
        } => {
            // 按下必须命中目标；释放沿用当前请求所有权。
            let require_target_hit = matches!(phase, PointerButtonPhase::Down);
            // 在按钮事件前移动到当前解析点。
            move_to(target, input, *point, require_target_hit, state, interrupt)?;
            // 调度显式按钮阶段。
            dispatch_tracked_button(*button, matches!(phase, PointerButtonPhase::Down), state)
        }
        // 单击或双击宏。
        PointerStep::Click {
            button,
            count,
            interval_ms,
            point,
        } => {
            // 移动到当前命中目标的点。
            move_to(target, input, *point, true, state, interrupt)?;
            // 执行一或两次成对点击。
            for click_index in 0..*count {
                // 每次按下前检查取消和 deadline。
                interrupt.check()?;
                // 重新确认精确目标仍为前景。
                target.check()?;
                // 按下并立即取得释放责任。
                dispatch_tracked_button(*button, true, state)?;
                // 安全释放优先于下一次取消检查。
                dispatch_tracked_button(*button, false, state)?;
                // 双击的第一次之后执行可取消间隔。
                if click_index + 1 < *count {
                    // 等待公开有界间隔。
                    interrupt.pause(Duration::from_millis(u64::from(*interval_ms)))?;
                }
            }
            // 返回点击完成。
            Ok(())
        }
        // 垂直或水平滚轮。
        PointerStep::Scroll { axis, ticks, point } => {
            // 滚轮路由依赖当前物理命中目标。
            move_to(target, input, *point, true, state, interrupt)?;
            // 在滚轮事件前检查取消。
            interrupt.check()?;
            // 禁止前景已变化时滚动。
            target.check()?;
            // 记录滚轮调度阶段。
            state.current_stage = "scroll";
            // 调度一个有界滚轮事件。
            dispatch_scroll(*axis, *ticks)?;
            state.input_events_sent += 1;
            // 成功返回表示滚轮事件已接受。
            state.pointer_effect_seen = true;
            // 返回成功。
            Ok(())
        }
        // 自有完整按钮生命周期的拖拽宏。
        PointerStep::Drag {
            button,
            start,
            end,
            duration_ms,
            samples,
        } => {
            // 起点必须当前命中精确目标。
            move_to(target, input, *start, true, state, interrupt)?;
            // 按下并取得拖拽释放责任。
            dispatch_tracked_button(*button, true, state)?;
            // 记录拖拽采样起始单调时刻。
            let drag_started = Instant::now();
            // 按确定顺序移动到全部采样点。
            for index in 1..=*samples {
                // 计算该采样相对拖拽起点的目标时刻。
                let planned_ms = u64::from(*duration_ms) * u64::from(index)
                    // 平均分配完整持续时间。
                    / u64::from(*samples);
                // 计算已耗时。
                let elapsed = drag_started.elapsed();
                // 只等待尚未经过的计划时间。
                let wait = Duration::from_millis(planned_ms).saturating_sub(elapsed);
                // 按钮持有期间等待仍传播取消和 timeout。
                interrupt.pause(wait)?;
                // 计算当前公开插值点。
                let point = interpolate_pointer_point(*start, *end, index, *samples);
                // 持有按钮时允许物理点离开窗口并沿当前窗口移动。
                move_to(target, input, point, false, state, interrupt)?;
            }
            // 最终移动后立即优先释放按钮。
            dispatch_tracked_button(*button, false, state)
        }
    }
}

// 对仍由当前请求持有的按钮执行 best-effort 安全释放。
fn release_held_buttons(state: &mut ExecutionState) -> ReleaseReport {
    // 固定释放前是否存在责任。
    let attempted = !state.held_buttons.is_empty();
    // 复制稳定按钮顺序以允许修改集合。
    let buttons = state.held_buttons.iter().copied().collect::<Vec<_>>();
    // 逐按钮尝试释放且不受前景或 deadline 阻止。
    for button in buttons {
        // 只有 Windows 明确接收释放后才清除责任。
        if dispatch_button(button, false).is_ok() {
            // 清除已释放按钮。
            state.held_buttons.remove(&button);
            // 仅向项目自有动态测试发布 Adapter 已接受安全释放。
            #[cfg(test)]
            TEST_ACCEPTED_BUTTON_UPS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
    }
    // 投影仍可能持有的公开按钮名称。
    let remaining = state
        // 访问持有集合。
        .held_buttons
        // 按稳定顺序迭代。
        .iter()
        // 映射公开按钮文本。
        .map(|button| button.as_str())
        // 收集有界结果。
        .collect::<Vec<_>>();
    // 返回释放证据。
    ReleaseReport {
        // 保存是否尝试。
        attempted,
        // 空残留表示全部释放成功。
        succeeded: remaining.is_empty(),
        // 保存残留按钮。
        remaining,
    }
}

// 构造 dispatch 阶段保守 OutcomeUnknown。
fn outcome_unknown(
    // 接收下层稳定失败类别。
    cause: AppControlError,
    // 接收已验证请求。
    input: &PointerInput,
    // 接收并更新执行状态。
    state: &mut ExecutionState,
) -> AppControlError {
    // 始终先尝试释放工具持有按钮。
    let release = release_held_buttons(state);
    // 返回 D3/R0 不可自动重试结果。
    PointerInputErrorCode::OutcomeUnknown.with_details(
        "The pointer input outcome could not be determined after foreground execution began.",
        // 只发布 provider-neutral 状态与补偿证据。
        json!({
            "capability": capabilities::UI_INPUT_POINTER,
            "outcome": "unknown",
            "causeCode": cause.code,
            "dispatchState": if state.pointer_effect_seen {
                "accepted-may-have-occurred"
            } else {
                "foreground-impact-may-have-occurred"
            },
            "acceptedMayHaveOccurred": state.pointer_effect_seen,
            "currentStepIndex": state.current_step,
            "currentStage": state.current_stage,
            "completedSteps": state.completed_steps,
        "inputEventsSent": state.input_events_sent,
            "completedStepCount": state.completed_steps.len(),
            "coordinateSpace": input.coordinate_space.as_str(),
            "legacyClickCompatibility": input.legacy_click_compatibility,
            "targetRestored": state.target_restored,
            "targetActivated": state.target_activated,
            "foregroundChanged": cause.code == PointerInputErrorCode::HostInterferenceDetected.as_str(),
            "safeReleaseAttempted": release.attempted,
            "safeReleaseSucceeded": release.succeeded,
            "buttonsHeldByTool": release.remaining,
            "retrySafe": false,
            "automaticRetryProhibited": true,
        }),
    )
}

// 执行完整已验证动作序列。
fn execute_sequence(
    // 接收原 canonical 目标。
    target: PointTarget,
    // 接收已验证请求。
    input: &PointerInput,
    // 接收执行状态。
    state: &mut ExecutionState,
    // 接收单调中断生命周期控制器。
    interrupt: &InterruptDeadline,
) -> AppResult<()> {
    // 按公开顺序执行步骤。
    for (index, step) in input.steps.iter().enumerate() {
        // 保存当前步骤索引供失败报告。
        state.current_step = Some(index);
        // 保存步骤开始阶段。
        state.current_stage = "step-start";
        // 执行一个步骤。
        execute_step(target, input, step, state, interrupt)?;
        // 记录完整结束摘要。
        state.complete_step(index, step);
    }
    // 清除当前步骤索引。
    state.current_step = None;
    // 标记序列完成阶段。
    state.current_stage = "sequence-completed";
    // 返回成功。
    Ok(())
}

// 构造已完成且无按钮残留的公开结果。
fn success_result(input: &PointerInput, state: ExecutionState, permission: &str) -> Value {
    // 返回 provider-neutral 完成证据。
    json!({
        "capability": capabilities::UI_INPUT_POINTER,
        "outcome": "completed",
        "dispatchState": "completed",
        "coordinateSpace": input.coordinate_space.as_str(),
        "stepCount": input.steps.len(),
        "completedSteps": state.completed_steps,
        "inputEventsSent": state.input_events_sent,
        "legacyClickCompatibility": input.legacy_click_compatibility,
        "buttonOwnership": "request-scoped-balanced",
        "buttonsHeldByTool": [],
        "safeReleaseAttempted": false,
        "safeReleaseSucceeded": true,
        "targetReresolvedBeforeEveryPoint": true,
        "targetHitTestedBeforeContact": true,
        "coordinateValidatedAtUse": true,
        "coordinateContext": "per-monitor-v2-physical-px",
        "signedVirtualScreenCoordinates": true,
        "targetRestored": state.target_restored,
        "targetActivated": state.target_activated,
        "foregroundChangedDuringDispatch": false,
        "permissionPreflight": permission,
        "activeWriteProbePerformed": false,
        "confirmationEvaluatedBeforeDispatch": true,
        "foregroundConsentEvaluatedBeforeDispatch": true,
        "retrySafe": false,
        "automaticRetryProhibited": true,
    })
}

// 对精确窗口执行一次已确认通用指针 Command。
pub(crate) fn perform(
    // 接收原 canonical 窗口目标。
    session_id: &str,
    // 接收逐操作确认状态。
    confirmed: bool,
    // 接收公开输入对象。
    value: &Value,
) -> AppResult<Value> {
    // 使用进程级取消信号执行正式生产路径。
    perform_with_cancel_probe(session_id, confirmed, value, cancellation::is_cancelled)
}

// 使用显式取消探针执行同一正式指针状态机。
fn perform_with_cancel_probe(
    // 接收原 canonical 窗口目标。
    session_id: &str,
    // 接收逐操作确认状态。
    confirmed: bool,
    // 接收公开输入对象。
    value: &Value,
    // 接收无状态取消探针供生产或确定性测试使用。
    cancelled: fn() -> bool,
) -> AppResult<Value> {
    // confirmation 必须先于输入、目标、权限与平台解析。
    if !confirmed {
        // 返回统一确认错误。
        return Err(PointerInputErrorCode::ConfirmationRequired
            .error("Pointer input requires explicit per-operation confirmation."));
    }
    // 记录包含解析时间的同步调用起点。
    let started = Instant::now();
    // 严格解析 provider-neutral 输入或旧单击兼容形状。
    let input = parse_pointer_input(value)?;
    // 构造请求级单调 deadline 与取消探针。
    let interrupt = InterruptDeadline::new(started, input.timeout_ms, cancelled);
    // 在任何发现前检查取消和 deadline。
    interrupt.check()?;
    // 第一次重新发现精确窗口。
    let initial_window = current_target(session_id)?;
    // 在 provider 输入前完成静态权限预检。
    let permission = permission_preflight(&initial_window)?;
    // 建立 Per-Monitor-V2 线程坐标上下文。
    let _dpi = PointerDpiGuard::enter()?;
    // 在任何前景影响前验证全部公开端点。
    prevalidate_points(&initial_window, &input)?;
    // 初始化请求内执行状态。
    let mut state = ExecutionState::new();
    // 恢复并激活精确目标。
    if let Err(error) = activate_target(session_id, &mut state, &interrupt) {
        // 已请求前景影响后必须保守报告 unknown。
        if state.foreground_effect_started {
            // 执行安全释放并返回不确定结果。
            return Err(outcome_unknown(error, &input, &mut state));
        }
        // 无前景影响时保留调用前错误。
        return Err(error);
    }
    finish(
        &input,
        state,
        &interrupt,
        PointTarget::Window(session_id),
        permission,
    )
}

// 声明不产生真实输入的纯 Module 测试。
#[cfg(test)]
#[path = "pointer_input_tests.rs"]
mod tests;

// 声明会驱动项目自有窗口夹具的串行动态回归测试。
#[cfg(test)]
#[path = "pointer_input_dynamic_tests.rs"]
mod dynamic_tests;

#[derive(Clone, Copy)]
enum PointTarget<'a> {
    Window(&'a str),
    Desktop(&'a dyn Fn(Option<PointerPoint>) -> AppResult<()>),
}
impl PointTarget<'_> {
    fn check(self) -> AppResult<()> {
        match self {
            Self::Window(id) => ensure_target_foreground(&current_target(id)?),
            Self::Desktop(check) => check(None),
        }
    }
}

fn finish(
    input: &PointerInput,
    mut state: ExecutionState,
    interrupt: &InterruptDeadline,
    target: PointTarget,
    permission: &str,
) -> AppResult<Value> {
    // 执行完整动作序列。
    if let Err(error) = execute_sequence(target, input, &mut state, &interrupt) {
        // dispatch 阶段任何失败都执行安全释放并报告 unknown。
        return Err(outcome_unknown(error, input, &mut state));
    }
    // 成功路径仍必须确认没有按钮所有权泄漏。
    if !state.held_buttons.is_empty() {
        // 构造内部状态漂移并执行安全释放。
        let error = AppControlError::new(
            "OPERATION_FAILED",
            "Pointer input completed with an unexpected held-button state.",
        );
        // 返回保守不确定结果。
        return Err(outcome_unknown(error, input, &mut state));
    }
    if let Err(error) = target.check() {
        return Err(outcome_unknown(error, input, &mut state));
    }
    // 返回完成证据。
    Ok(success_result(input, state, permission))
}

pub(crate) fn perform_desktop(
    input: &PointerInput,
    cancellation: &crate::components::desktop_session_input_cancellation::DesktopInputCancellation,
    check: &dyn Fn(Option<PointerPoint>) -> AppResult<()>,
) -> AppResult<Value> {
    let _dpi = PointerDpiGuard::enter()?;
    let interrupt = InterruptDeadline::new(Instant::now(), input.timeout_ms, || {
        cancellation.is_cancelled() || cancellation::is_cancelled()
    });
    interrupt.check()?;
    let mut result = finish(
        input,
        ExecutionState::new(),
        &interrupt,
        PointTarget::Desktop(check),
        "desktop-session",
    )?;
    result["targetReresolvedBeforeEveryPoint"] = json!(false);
    result["targetHitTestedBeforeContact"] = json!(false);
    Ok(result)
}
