//! 组合键盘契约、精确窗口、权限与 Windows Adapter 的同步 Command Module。

// 导入单调时钟和有界等待类型。
use std::time::{Duration, Instant};

// 导入公开 JSON 值与构造宏。
use serde_json::{Value, json};

// 引入当前 Module 私有封闭错误类型。
#[path = "keyboard_input_error.rs"]
mod error_code;

// 引入当前 Module 私有中断生命周期控制器。
mod interrupt {
    pub(super) use crate::components::input_deadline::InterruptDeadline;
}

// 导入当前 Module 私有错误集合。
use error_code::KeyboardInputErrorCode;
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
        // 导入私有 Windows 键位与注入 Adapter。
        keyboard_input_windows::{
            // 导入命名键和 Unicode 调度。
            dispatch_key,
            dispatch_unicode_scalar,
            // 导入预调度映射验证。
            validate_key_mapping,
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
    // 导入取消、键盘契约与静态权限 Component。
    components::{
        // 导入进程级取消信号。
        cancellation,
        // 导入 provider-neutral 键盘领域类型。
        keyboard_input_contract::{
            // 导入请求与按键类型。
            KeyboardInput,
            KeyboardKey,
            KeyboardKeyPhase,
            // 导入步骤类型。
            KeyboardStep,
            // 导入解析入口。
            parse_keyboard_input,
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

// 仅在测试构建中记录状态机已经取得的按键释放责任。
#[cfg(test)]
static TEST_ACCEPTED_KEY_DOWNS: std::sync::atomic::AtomicUsize =
    // 初始化为零，不进入生产二进制。
    std::sync::atomic::AtomicUsize::new(0);
// 仅在测试构建中记录 Adapter 已接受的命名键释放。
#[cfg(test)]
static TEST_ACCEPTED_KEY_UPS: std::sync::atomic::AtomicUsize =
    // 初始化为零，不进入生产二进制。
    std::sync::atomic::AtomicUsize::new(0);
// 仅在测试构建中记录已完成的 Unicode scalar 数。
#[cfg(test)]
static TEST_ACCEPTED_UNICODE_SCALARS: std::sync::atomic::AtomicUsize =
    // 初始化为零，不进入生产二进制。
    std::sync::atomic::AtomicUsize::new(0);

// 保存一次同步 Command 的请求内执行状态。
struct ExecutionState {
    input_events_sent: usize,
    // 保存按取得顺序排列且尚未释放的命名键。
    held_keys: Vec<KeyboardKey>,
    // 保存已完成公开步骤的安全摘要。
    completed_steps: Vec<Value>,
    // 保存当前步骤索引。
    current_step: Option<usize>,
    // 保存当前稳定阶段名称。
    current_stage: &'static str,
    // 标记至少一次键盘平台调用已经开始。
    dispatch_attempted: bool,
    // 标记键盘事件已经被 Adapter 确认接受。
    keyboard_effect_seen: bool,
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
            input_events_sent: 0, // 初始没有工具持有按键。
            held_keys: Vec::new(),
            // 初始没有已完成步骤。
            completed_steps: Vec::new(),
            // 初始没有当前步骤。
            current_step: None,
            // 初始处于门禁阶段。
            current_stage: "pre-dispatch-gates",
            // 初始没有平台调用。
            dispatch_attempted: false,
            // 初始没有键盘影响。
            keyboard_effect_seen: false,
            // 初始没有前景影响。
            foreground_effect_started: false,
            // 初始没有恢复目标。
            target_restored: false,
            // 初始没有激活目标。
            target_activated: false,
        }
    }

    // 记录一个公开步骤已经完整结束。
    fn complete_step(&mut self, index: usize, step: &KeyboardStep) {
        // 追加不含文本、键位或 native 事实的稳定摘要。
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
    // 标记是否存在需要释放的命名键。
    attempted: bool,
    // 标记全部命名键是否已确认释放。
    succeeded: bool,
    // 保存释放后仍可能由工具持有的 provider-neutral 键名。
    remaining: Vec<String>,
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
            KeyboardInputErrorCode::CapabilityAssessmentUnavailable.with_details(
                "The keyboard target process inventory exceeds the certified bound.",
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
            "The exact keyboard target process generation is unavailable.",
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
        // 同级或较低完整性在逐操作确认后允许尝试前景输入。
        "requires-confirmation" => Ok(assessment.permission_relation),
        // 明确权限阻塞不得调用 SendInput。
        "permission-blocked" => Err(KeyboardInputErrorCode::PermissionDenied.with_details(
            "Static permission facts block keyboard input to the exact target.",
            // 只公开稳定相对权限原因。
            json!({
                "permissionRelation": assessment.permission_relation,
                "activeWriteProbePerformed": assessment.active_write_probe_performed,
                "retrySafe": false,
            }),
        )),
        // 未知或新增决策不得放宽。
        _ => Err(
            KeyboardInputErrorCode::CapabilityAssessmentUnavailable.with_details(
                "Static permission facts cannot certify keyboard input to the exact target.",
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

// 在任何目标发现和前景影响前验证完整私有键表。
fn prevalidate_key_mappings(input: &KeyboardInput) -> AppResult<()> {
    // 按公开顺序检查每一步中的命名键。
    for step in &input.steps {
        // 按步骤形状选择键集合。
        match step {
            // 单键只验证一个映射。
            KeyboardStep::Key { key, .. } => validate_key_mapping(key)?,
            // 快捷键验证全部有序成员。
            KeyboardStep::Chord { keys, .. } => {
                // 逐键验证私有映射存在。
                for key in keys {
                    // 不公开映射结果。
                    validate_key_mapping(key)?;
                }
            }
            // Unicode 文本不使用命名键表。
            KeyboardStep::Text { .. } => {}
        }
    }
    // 全部映射有效。
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
    // 穷尽重试后保持零键盘事件保证。
    Err(KeyboardInputErrorCode::ForegroundActivationFailed
        .error("The exact keyboard target could not be established as the foreground window."))
}

// 重新发现并确认精确目标仍是前景窗口。
fn ensure_current_foreground(session_id: &str) -> AppResult<()> {
    // 使用时重新发现 canonical 目标。
    let window = current_target(session_id)?;
    // 前景匹配时允许继续。
    if foreground_matches(&window) {
        // 返回成功。
        return Ok(());
    }
    // 前景变化必须停止新输入。
    Err(KeyboardInputErrorCode::HostInterferenceDetected
        .error("The foreground window changed during keyboard input."))
}

// 发送命名键按下并取得请求内释放责任。
fn dispatch_owned_down(key: &KeyboardKey, state: &mut ExecutionState) -> AppResult<()> {
    // 记录平台按下阶段。
    state.current_stage = "key-down";
    // 调用开始后结果必须保守处理。
    state.dispatch_attempted = true;
    // 调用窄 Windows Adapter。
    dispatch_key(key, true)?;
    state.input_events_sent += 1;
    // 成功返回表示按下事件已接受。
    state.keyboard_effect_seen = true;
    // 保存按下顺序供逆序补偿。
    state.held_keys.push(key.clone());
    // 仅向项目自有动态测试发布释放责任已经建立。
    #[cfg(test)]
    TEST_ACCEPTED_KEY_DOWNS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    // 返回成功。
    Ok(())
}

// 发送命名键释放并清除请求内责任。
fn dispatch_owned_up(key: &KeyboardKey, state: &mut ExecutionState) -> AppResult<()> {
    // 记录平台释放阶段。
    state.current_stage = "key-up";
    // 调用开始后结果必须保守处理。
    state.dispatch_attempted = true;
    // 释放优先于取消、deadline 和前景检查。
    dispatch_key(key, false)?;
    state.input_events_sent += 1;
    // 成功返回表示释放事件已接受。
    state.keyboard_effect_seen = true;
    // 找到当前请求持有的对应键。
    if let Some(index) = state.held_keys.iter().rposition(|held| held == key) {
        // 清除唯一释放责任。
        state.held_keys.remove(index);
    }
    // 仅向项目自有动态测试发布 Adapter 已接受释放。
    #[cfg(test)]
    TEST_ACCEPTED_KEY_UPS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    // 返回成功。
    Ok(())
}

// 执行一次成对命名键 press。
fn execute_press(
    // 接收原 canonical 目标。
    check: &dyn Fn() -> AppResult<()>,
    // 接收按键。
    key: &KeyboardKey,
    // 接收持续时间。
    hold_ms: u32,
    // 接收重复次数。
    repeat: u16,
    // 接收重复间隔。
    interval_ms: u32,
    // 接收执行状态。
    state: &mut ExecutionState,
    // 接收中断控制器。
    interrupt: &InterruptDeadline,
) -> AppResult<()> {
    // 按公开重复次数执行成对输入。
    for index in 0..repeat {
        // 新按下前检查中断。
        interrupt.check()?;
        // 每次按下前重新发现并核对前景。
        check()?;
        // 发送按下并取得释放责任。
        dispatch_owned_down(key, state)?;
        // 有持续时间时执行可取消等待。
        if hold_ms != 0 {
            // 等待期间的任何中断由统一补偿释放处理。
            interrupt.pause(Duration::from_millis(u64::from(hold_ms)))?;
        }
        // 正常路径立即优先释放。
        dispatch_owned_up(key, state)?;
        // 释放后核对目标未被切换。
        check()?;
        // 非最后一次重复后执行可取消间隔。
        if index + 1 < repeat && interval_ms != 0 {
            // 等待公开有界间隔。
            interrupt.pause(Duration::from_millis(u64::from(interval_ms)))?;
        }
    }
    // 全部重复完成。
    Ok(())
}

// 执行一次有序快捷键宏。
fn execute_chord(
    // 接收原 canonical 目标。
    check: &dyn Fn() -> AppResult<()>,
    // 接收有序按键集合。
    keys: &[KeyboardKey],
    // 接收持续时间。
    hold_ms: u32,
    // 接收重复次数。
    repeat: u16,
    // 接收重复间隔。
    interval_ms: u32,
    // 接收执行状态。
    state: &mut ExecutionState,
    // 接收中断控制器。
    interrupt: &InterruptDeadline,
) -> AppResult<()> {
    // 按公开重复次数执行整组快捷键。
    for repeat_index in 0..repeat {
        // 按调用方顺序发送全部按下。
        for key in keys {
            // 每个新按下前检查中断。
            interrupt.check()?;
            // 每个新按下前重新发现并核对前景。
            check()?;
            // 取得当前键释放责任。
            dispatch_owned_down(key, state)?;
        }
        // 有持续时间时执行可取消等待。
        if hold_ms != 0 {
            // 等待期间的任何中断由统一逆序补偿处理。
            interrupt.pause(Duration::from_millis(u64::from(hold_ms)))?;
        }
        // 正常路径按按下逆序释放全部键。
        for key in keys.iter().rev() {
            // 释放不得被中断或前景变化阻止。
            dispatch_owned_up(key, state)?;
        }
        // 完整释放后核对目标未被切换。
        check()?;
        // 非最后一次重复后执行可取消间隔。
        if repeat_index + 1 < repeat && interval_ms != 0 {
            // 等待公开有界间隔。
            interrupt.pause(Duration::from_millis(u64::from(interval_ms)))?;
        }
    }
    // 全部快捷键重复完成。
    Ok(())
}

// 执行一个已验证公开步骤。
fn execute_step(
    // 接收原 canonical 目标。
    check: &dyn Fn() -> AppResult<()>,
    // 接收当前步骤。
    step: &KeyboardStep,
    // 接收执行状态。
    state: &mut ExecutionState,
    // 接收单调中断生命周期控制器。
    interrupt: &InterruptDeadline,
) -> AppResult<()> {
    // 按封闭步骤类型调度。
    match step {
        // 单个命名键阶段。
        KeyboardStep::Key {
            key,
            phase,
            hold_ms,
            repeat,
            interval_ms,
        } => match phase {
            // 成对 press 自有完整生命周期。
            KeyboardKeyPhase::Press => execute_press(
                check,
                key,
                *hold_ms,
                *repeat,
                *interval_ms,
                state,
                interrupt,
            ),
            // 显式 down 取得请求内责任。
            KeyboardKeyPhase::Down => {
                // 按下前检查中断。
                interrupt.check()?;
                // 按下前重新发现并核对前景。
                check()?;
                // 调度按下。
                dispatch_owned_down(key, state)
            }
            // 显式 up 优先释放当前请求持有的键。
            KeyboardKeyPhase::Up => {
                // 不允许取消或前景变化阻止释放。
                dispatch_owned_up(key, state)?;
                // 释放后报告前景变化。
                check()
            }
        },
        // 有序快捷键宏。
        KeyboardStep::Chord {
            keys,
            hold_ms,
            repeat,
            interval_ms,
        } => execute_chord(
            check,
            keys,
            *hold_ms,
            *repeat,
            *interval_ms,
            state,
            interrupt,
        ),
        // Unicode 文本步骤。
        KeyboardStep::Text { text, .. } => {
            // 按 Unicode scalar 边界保持可取消性和不拆分字符。
            for character in text.chars() {
                // 每个 scalar 前检查中断。
                interrupt.check()?;
                // 每个 scalar 前重新发现并核对前景。
                check()?;
                // 记录 Unicode 调度阶段。
                state.current_stage = "unicode-text";
                // 调用开始后结果必须保守处理。
                state.dispatch_attempted = true;
                // Adapter 内部对 UTF-16 单元成对调度并补偿。
                dispatch_unicode_scalar(character)?;
                state.input_events_sent += character.len_utf16() * 2;
                // 成功返回表示字符事件已接受。
                state.keyboard_effect_seen = true;
                // 仅向项目自有动态测试发布完整 scalar。
                #[cfg(test)]
                TEST_ACCEPTED_UNICODE_SCALARS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            }
            // 文本完成后再次核对前景。
            check()
        }
    }
}

// 对仍由当前请求持有的命名键执行逆序 best-effort 安全释放。
fn release_held_keys(state: &mut ExecutionState) -> ReleaseReport {
    // 固定释放前是否存在责任。
    let attempted = !state.held_keys.is_empty();
    // 复制逆序键集合以允许修改持有列表。
    let keys = state.held_keys.iter().rev().cloned().collect::<Vec<_>>();
    // 逐键尝试释放且不受前景、取消或 deadline 阻止。
    for key in keys {
        // 只有 Windows 明确接收释放后才清除责任。
        if dispatch_key(&key, false).is_ok() {
            // 找到并清除对应责任。
            if let Some(index) = state.held_keys.iter().rposition(|held| held == &key) {
                // 删除已释放键。
                state.held_keys.remove(index);
            }
            // 仅向项目自有动态测试发布安全释放。
            #[cfg(test)]
            TEST_ACCEPTED_KEY_UPS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
    }
    // 投影仍可能持有的 provider-neutral 键名。
    let remaining = state
        // 访问持有列表。
        .held_keys
        // 按原始按下顺序迭代。
        .iter()
        // 复制公开键名。
        .map(|key| key.as_str().to_owned())
        // 收集有界结果。
        .collect::<Vec<_>>();
    // 返回释放证据。
    ReleaseReport {
        // 保存是否尝试。
        attempted,
        // 空残留表示全部命名键释放成功。
        succeeded: remaining.is_empty(),
        // 保存残留键名。
        remaining,
    }
}

// 构造 dispatch 阶段保守 OutcomeUnknown。
fn outcome_unknown(
    // 接收下层稳定失败类别。
    cause: AppControlError,
    // 接收已验证请求。
    input: &KeyboardInput,
    // 接收并更新执行状态。
    state: &mut ExecutionState,
) -> AppControlError {
    // 始终先尝试释放工具持有命名键。
    let release = release_held_keys(state);
    // 读取 Unicode Adapter 是否已经执行内部释放。
    let unicode_release_attempted = cause
        // 访问下层安全事实。
        .details
        // 读取固定字段。
        .get("safeReleaseAttempted")
        // 转换布尔值。
        .and_then(Value::as_bool)
        // 缺失表示没有 Unicode 残留责任。
        .unwrap_or(false);
    // 读取 Unicode Adapter 的内部释放结果。
    let unicode_release_succeeded = cause
        // 访问下层安全事实。
        .details
        // 读取固定字段。
        .get("safeReleaseSucceeded")
        // 转换布尔值。
        .and_then(Value::as_bool)
        // 未尝试时视为没有 Unicode 残留。
        .unwrap_or(!unicode_release_attempted);
    // 合并命名键和 Unicode 两类释放事实。
    let safe_release_attempted = release.attempted || unicode_release_attempted;
    // 两类释放都必须成功才能给出零残留证明。
    let safe_release_succeeded = release.succeeded && unicode_release_succeeded;
    // 返回 D3/R0 不可自动重试结果。
    KeyboardInputErrorCode::OutcomeUnknown.with_details(
        "The keyboard input outcome could not be determined after foreground execution began.",
        // 只发布 provider-neutral 状态与补偿证据。
        json!({
            "capability": capabilities::UI_INPUT_KEY,
            "outcome": "unknown",
            "causeCode": cause.code,
            "dispatchState": if state.keyboard_effect_seen {
                "accepted-may-have-occurred"
            } else if state.dispatch_attempted {
                "dispatch-attempted"
            } else {
                "foreground-impact-may-have-occurred"
            },
            "acceptedMayHaveOccurred": state.dispatch_attempted,
            "currentStepIndex": state.current_step,
            "currentStage": state.current_stage,
            "completedSteps": state.completed_steps,
        "inputEventsSent": state.input_events_sent,
            "completedStepCount": state.completed_steps.len(),
            "legacyKeyCompatibility": input.legacy_key_compatibility,
            "targetRestored": state.target_restored,
            "targetActivated": state.target_activated,
            "foregroundChanged": cause.code == KeyboardInputErrorCode::HostInterferenceDetected.as_str(),
            "safeReleaseAttempted": safe_release_attempted,
            "safeReleaseSucceeded": safe_release_succeeded,
            "unicodeSafeReleaseAttempted": unicode_release_attempted,
            "unicodeSafeReleaseSucceeded": unicode_release_succeeded,
            "keysHeldByTool": release.remaining,
            "retrySafe": false,
            "automaticRetryProhibited": true,
        }),
    )
}

// 执行完整已验证动作序列。
fn execute_sequence(
    // 接收原 canonical 目标。
    check: &dyn Fn() -> AppResult<()>,
    // 接收已验证请求。
    input: &KeyboardInput,
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
        execute_step(check, step, state, interrupt)?;
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

// 构造已完成且无按键残留的公开结果。
fn success_result(input: &KeyboardInput, state: ExecutionState, permission: &str) -> Value {
    // 返回 provider-neutral 完成证据。
    json!({
        "capability": capabilities::UI_INPUT_KEY,
        "outcome": "completed",
        "dispatchState": "completed",
        "stepCount": input.steps.len(),
        "completedSteps": state.completed_steps,
        "inputEventsSent": state.input_events_sent,
        "legacyKeyCompatibility": input.legacy_key_compatibility,
        "keyOwnership": "request-scoped-balanced",
        "keysHeldByTool": [],
        "safeReleaseAttempted": false,
        "safeReleaseSucceeded": true,
        "providerNeutralKeySet": "complete-key-input-v1",
        "unicodeTextSupported": true,
        "targetReresolvedBeforeEveryEffect": true,
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

// 对精确窗口执行一次已确认通用键盘 Command。
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

// 使用显式取消探针执行同一正式键盘状态机。
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
        return Err(KeyboardInputErrorCode::ConfirmationRequired
            .error("Keyboard input requires explicit per-operation confirmation."));
    }
    // 记录包含解析时间的同步调用起点。
    let started = Instant::now();
    // 严格解析 provider-neutral 输入或旧键兼容形状。
    let input = parse_keyboard_input(value)?;
    // 在目标发现前证明私有键表完整。
    prevalidate_key_mappings(&input)?;
    // 构造请求级单调 deadline 与取消探针。
    let interrupt = InterruptDeadline::new(started, input.timeout_ms, cancelled);
    // 在任何发现前检查取消和 deadline。
    interrupt.check()?;
    // 第一次重新发现精确窗口。
    let initial_window = current_target(session_id)?;
    // 在 provider 输入前完成静态权限预检。
    let permission = permission_preflight(&initial_window)?;
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
        &|| ensure_current_foreground(session_id),
        permission,
    )
}

// 声明不产生真实输入的纯 Module 测试。
#[cfg(test)]
#[path = "keyboard_input_tests.rs"]
mod tests;

// 声明会驱动项目自有窗口夹具的串行动态回归测试。
#[cfg(test)]
#[path = "keyboard_input_dynamic_tests.rs"]
mod dynamic_tests;

fn finish(
    input: &KeyboardInput,
    mut state: ExecutionState,
    interrupt: &InterruptDeadline,
    check: &dyn Fn() -> AppResult<()>,
    permission: &str,
) -> AppResult<Value> {
    // 执行完整动作序列。
    if let Err(error) = execute_sequence(check, input, &mut state, &interrupt) {
        // dispatch 阶段任何失败都执行安全释放并报告 unknown。
        return Err(outcome_unknown(error, input, &mut state));
    }
    // 成功路径仍必须确认没有按键所有权泄漏。
    if !state.held_keys.is_empty() {
        // 构造内部状态漂移并执行安全释放。
        let error = AppControlError::new(
            "OPERATION_FAILED",
            "Keyboard input completed with an unexpected held-key state.",
        );
        // 返回保守不确定结果。
        return Err(outcome_unknown(error, input, &mut state));
    }
    // 最终重新发现并核对前景未在结束前变化。
    if let Err(error) = check() {
        // 执行统一保守失败。
        return Err(outcome_unknown(error, input, &mut state));
    }
    // 返回完成证据。
    Ok(success_result(input, state, permission))
}

pub(crate) fn perform_desktop(
    input: &KeyboardInput,
    cancellation: &crate::components::desktop_session_input_cancellation::DesktopInputCancellation,
    check: &dyn Fn() -> AppResult<()>,
) -> AppResult<Value> {
    prevalidate_key_mappings(input)?;
    let interrupt = InterruptDeadline::new(Instant::now(), input.timeout_ms, || {
        cancellation.is_cancelled() || cancellation::is_cancelled()
    });
    interrupt.check()?;
    check()?;
    let mut result = finish(
        input,
        ExecutionState::new(),
        &interrupt,
        check,
        "desktop-session",
    )?;
    result["targetReresolvedBeforeEveryEffect"] = json!(false);
    Ok(result)
}
