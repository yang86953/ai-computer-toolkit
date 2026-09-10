//! 组合精确目标、权限、前景与 Windows Adapter 的同步窗口生命周期 Command Module。

// 导入单调时钟与短轮询时长。
use std::time::{Duration, Instant};

// 导入公开 JSON 值与构造宏。
use serde_json::{Value, json};

// 引入当前 Module 私有封闭错误类型。
#[path = "window_lifecycle_error.rs"]
mod error_code;

// 引入当前 Module 私有中断生命周期控制器。
#[path = "window_lifecycle_interrupt.rs"]
mod interrupt;

// 导入当前 Module 私有错误集合。
use error_code::WindowLifecycleErrorCode;
// 导入当前 Module 私有中断生命周期类型。
use interrupt::{InterruptDeadline, WindowLifecycleInterrupt};

// 导入窄 Adapter、契约、权限与统一结果。
use crate::{
    // 导入精确窗口平台事实和私有调用。
    adapters::{
        // 导入状态、几何、读回与封闭失败分类。
        window_lifecycle_windows::{self, WindowLifecycleFailure, WindowLifecycleObservation},
        // 导入窗口与进程私有事实。
        windows::{
            // 导入相对完整性与元数据访问分类。
            IntegrityRelation,
            ProcessMetadataAccess,
            // 导入私有窗口记录与有界进程清单。
            WindowRecord,
            enumerate_process_inventory,
            // 导入当前顶层窗口与前景事实。
            enumerate_windows,
            foreground_hwnd,
            // 导入 canonical 窗口身份生成器。
            opaque_window_session_id,
        },
    },
    // 导入版本化 capability ID。
    capabilities,
    // 导入取消、opaque、输入和静态权限 Component。
    components::{
        // 导入进程级取消信号。
        cancellation,
        // 导入 canonical opaque 目标解析与碰撞检测。
        opaque_id::{OpaqueTargetId, OpaqueTargetKind, OpaqueTargetMatch, match_opaque_target},
        // 导入无主动写探针的静态权限评估。
        static_permission_assessment::{
            StaticIntegrityRelation, StaticMetadataAccess, assess_static_background_mutation,
        },
        // 导入严格窗口生命周期输入与动作契约。
        window_lifecycle_contract::{
            WindowLifecycleInput, WindowLifecycleOperation, parse_window_lifecycle_input,
        },
        // 导入窗口目标身份强度的唯一公开投影。
        window_target_identity,
    },
    // 导入统一错误和结果类型。
    domain::{AppControlError, AppResult},
};

// 固定完整窗口扫描认证上限。
const WINDOW_INVENTORY_LIMIT: usize = 16_384;
// 限制一次权限预检最多枚举的进程数。
const PROCESS_INVENTORY_LIMIT: usize = 65_536;
// 固定最终状态轮询间隔。
const FINAL_STATE_POLL_INTERVAL: Duration = Duration::from_millis(25);

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

// 验证公开目标是 canonical 窗口 ID。
fn validate_target(session_id: &str) -> AppResult<()> {
    // 解析完整 canonical ID 并核对窗口类别。
    let valid = OpaqueTargetId::parse(session_id)
        // 读取解析出的类别。
        .is_some_and(|target| target.kind() == OpaqueTargetKind::Window);
    // 合法目标直接通过。
    if valid {
        // 结束参数验证。
        return Ok(());
    }
    // 拒绝 legacy、native 或错误类别目标。
    Err(WindowLifecycleErrorCode::InvalidArgument.error(
        // 说明精确目标要求。
        "Window lifecycle requires a canonical s2:w target.",
    ))
}

// 枚举完整且有界的当前顶层窗口清单。
fn current_windows() -> AppResult<Vec<WindowRecord>> {
    // 读取当前顶层窗口私有事实。
    let records = enumerate_windows()?;
    // 超界时不得截断后任取一个碰撞候选。
    if records.len() > WINDOW_INVENTORY_LIMIT {
        // 返回 assessment 缺口并禁止自动重试。
        return Err(
            WindowLifecycleErrorCode::CapabilityAssessmentUnavailable.with_details(
                // 不公开窗口数量或 native 事实。
                "The window inventory exceeds the certified lifecycle bound.",
                // 返回稳定边界原因。
                json!({
                    // 标记完整清单超界。
                    "reason": "window-inventory-limit-exceeded",
                    // 禁止自动重试。
                    "retrySafe": false,
                }),
            ),
        );
    }
    // 返回完整有界清单。
    Ok(records)
}

// 在完整清单中唯一重新解析 canonical s2:w。
fn resolve_window<'records>(
    // 接收调用方 opaque 目标。
    session_id: &str,
    // 接收当前完整窗口清单。
    records: &'records [WindowRecord],
) -> AppResult<&'records WindowRecord> {
    // 使用公共匹配 Component 明确区分零、一和多命中。
    match match_opaque_target(
        // 传入精确 opaque 目标。
        session_id,
        // 遍历当前记录引用。
        records.iter(),
        // 每项均从当前私有事实重新生成 canonical ID。
        |record| Some(opaque_window_session_id(record)),
    ) {
        // 唯一命中返回私有窗口记录。
        OpaqueTargetMatch::Unique(record) => Ok(record),
        // 零命中统一返回 stale。
        OpaqueTargetMatch::Missing => Err(WindowLifecycleErrorCode::StaleSession.error(
            // 不公开原生身份。
            "The exact window lifecycle target is stale or unavailable.",
        )),
        // 多命中必须 fail closed。
        OpaqueTargetMatch::Ambiguous => Err(WindowLifecycleErrorCode::AmbiguousTarget.error(
            // 明确不会任取窗口。
            "The exact window lifecycle target resolves to multiple windows.",
        )),
    }
}

// 对目标进程执行无主动写探针的静态权限预检。
fn permission_preflight(window: &WindowRecord) -> AppResult<&'static str> {
    // 枚举有界进程权限清单。
    let inventory = enumerate_process_inventory(PROCESS_INVENTORY_LIMIT)?;
    // 截断清单不能证明唯一权限关系。
    if !inventory.complete {
        // 返回明确 assessment 缺口。
        return Err(
            WindowLifecycleErrorCode::CapabilityAssessmentUnavailable.with_details(
                // 说明有界清单无法认证权限。
                "The lifecycle target process inventory exceeds the certified bound.",
                // 返回稳定不重试事实。
                json!({
                    // 标记进程清单超界。
                    "reason": "process-inventory-limit-exceeded",
                    // 禁止自动重试。
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
        return Err(WindowLifecycleErrorCode::StaleSession.error(
            // 不公开 PID 或创建时间。
            "The exact lifecycle target process generation is unavailable.",
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
        // 同级或较低完整性在逐操作确认后允许固定窗口调用。
        "requires-confirmation" => Ok(assessment.permission_relation),
        // 明确权限阻塞不得调用平台 mutation。
        "permission-blocked" => Err(WindowLifecycleErrorCode::PermissionDenied.with_details(
            // 说明静态事实阻塞目标。
            "Static permission facts block lifecycle control of the exact target.",
            // 只公开稳定相对权限原因。
            json!({
                // 公开 provider-neutral 权限关系。
                "permissionRelation": assessment.permission_relation,
                // 明确没有主动试写。
                "activeWriteProbePerformed": assessment.active_write_probe_performed,
                // 禁止自动重试。
                "retrySafe": false,
            }),
        )),
        // 未知或新增决策不得放宽。
        _ => Err(
            WindowLifecycleErrorCode::CapabilityAssessmentUnavailable.with_details(
                // 说明权限事实不足。
                "Static permission facts cannot certify lifecycle control of the exact target.",
                // 只公开稳定相对权限原因。
                json!({
                    // 公开 provider-neutral 权限关系。
                    "permissionRelation": assessment.permission_relation,
                    // 明确没有主动试写。
                    "activeWriteProbePerformed": assessment.active_write_probe_performed,
                    // 禁止自动重试。
                    "retrySafe": false,
                }),
            ),
        ),
    }
}

// 将写入前中断映射为确定性错误。
fn pre_dispatch_interrupt(interrupt: WindowLifecycleInterrupt) -> AppControlError {
    // 区分取消与 deadline。
    match interrupt {
        // 写入前取消可确定没有平台影响。
        WindowLifecycleInterrupt::Cancelled => WindowLifecycleErrorCode::Cancelled.error(
            // 说明取消发生在平台接受前。
            "Window lifecycle was cancelled before platform dispatch.",
        ),
        // 写入前超时可确定没有平台影响。
        WindowLifecycleInterrupt::TimedOut => WindowLifecycleErrorCode::Timeout.error(
            // 说明 deadline 发生在平台接受前。
            "Window lifecycle reached its deadline before platform dispatch.",
        ),
    }
}

// 将接受前平台失败映射为稳定领域错误。
fn platform_failure_before_acceptance(failure: WindowLifecycleFailure) -> AppControlError {
    // 按封闭平台分类映射。
    match failure {
        // 写前身份变化保持 stale。
        WindowLifecycleFailure::Stale => WindowLifecycleErrorCode::StaleSession.error(
            // 不公开句柄或进程。
            "The exact lifecycle target is no longer available.",
        ),
        // 明确访问拒绝保持权限错误。
        WindowLifecycleFailure::PermissionDenied => WindowLifecycleErrorCode::PermissionDenied
            // 说明固定调用被系统拒绝。
            .error("Windows denied the exact window lifecycle request."),
        // 样式或状态不支持时不得静默恢复或回退输入。
        WindowLifecycleFailure::Unsupported => WindowLifecycleErrorCode::CapabilityUnsupported
            // 说明当前窗口不支持该动作。
            .error("The exact window does not support this lifecycle action in its current state."),
        // DPI 或外框上下文无法认证。
        WindowLifecycleFailure::CoordinateUnavailable => {
            WindowLifecycleErrorCode::CoordinateContextUnavailable.error(
                // 不公开平台错误细节。
                "The exact window does not expose a certified physical coordinate context.",
            )
        }
        // 当前环境几何边界拒绝请求。
        WindowLifecycleFailure::InvalidGeometry => {
            WindowLifecycleErrorCode::InvalidArgument.with_details(
                // 说明环境范围但不公开 native 数据。
                "The requested window geometry is outside the certified current environment bounds.",
                // 返回稳定范围原因。
                json!({
                    // 标记环境几何拒绝。
                    "reason": "window-geometry-out-of-range",
                    // 明确平台未接受动作。
                    "accepted": false,
                    // 禁止调用方不经重新观察自动重试。
                    "retrySafe": false,
                }),
            )
        }
        // 其他调用失败保持确定性接受前错误。
        WindowLifecycleFailure::OperationFailed => WindowLifecycleErrorCode::OperationFailed
            // 说明平台没有确认接受。
            .error("The exact window lifecycle request was not accepted by Windows."),
    }
}

// 返回平台失败的 provider-neutral 稳定原因。
const fn accepted_failure_reason(failure: WindowLifecycleFailure) -> &'static str {
    // 穷举所有读回失败分类。
    match failure {
        // 同代际目标已经不可读。
        WindowLifecycleFailure::Stale => "target-became-stale-after-acceptance",
        // 后续读回遭遇权限变化。
        WindowLifecycleFailure::PermissionDenied => "permission-changed-after-acceptance",
        // 目标能力或状态发生变化。
        WindowLifecycleFailure::Unsupported => "target-state-changed-after-acceptance",
        // 物理坐标上下文发生变化。
        WindowLifecycleFailure::CoordinateUnavailable => {
            "coordinate-context-unavailable-after-acceptance"
        }
        // 最终几何落入未认证范围。
        WindowLifecycleFailure::InvalidGeometry => "geometry-invalid-after-acceptance",
        // 其他读回失败。
        WindowLifecycleFailure::OperationFailed => "final-state-readback-failed",
    }
}

// 构造平台已接受后的不可安全重试未知结果。
fn outcome_unknown(
    // 接收稳定原因文本。
    reason: &'static str,
    // 接收公开消息。
    message: &'static str,
) -> AppControlError {
    // 返回固定未知结果详情。
    WindowLifecycleErrorCode::OutcomeUnknown.with_details(
        // 使用调用方可理解的稳定消息。
        message,
        // 输出 accepted 与禁止重试证据。
        json!({
            // 平台调用已经返回成功接受。
            "accepted": true,
            // 最终状态未能认证。
            "finalStateReached": false,
            // 固定未知结果。
            "outcome": "unknown",
            // 返回 provider-neutral 原因。
            "reason": reason,
            // 禁止自动重试。
            "retrySafe": false,
            // 明确自动重试被禁止。
            "automaticRetryProhibited": true,
        }),
    )
}

// 将接受后中断映射为 OutcomeUnknown。
fn accepted_interrupt(interrupt: WindowLifecycleInterrupt) -> AppControlError {
    // 区分取消请求与 deadline。
    match interrupt {
        // 取消请求不能撤销已经接受的平台调用。
        WindowLifecycleInterrupt::Cancelled => outcome_unknown(
            // 返回稳定取消原因。
            "cancel-requested-after-acceptance",
            // 说明取消不等于已经取消。
            "Cancellation was requested after Windows accepted the lifecycle action.",
        ),
        // deadline 不能证明目标最终未变化。
        WindowLifecycleInterrupt::TimedOut => outcome_unknown(
            // 返回稳定超时原因。
            "deadline-reached-after-acceptance",
            // 说明最终状态未知。
            "The lifecycle deadline expired after Windows accepted the action.",
        ),
    }
}

// 构造只含 provider-neutral 事实的成功结果。
fn success_result(
    // 接收调用方 opaque 目标。
    session_id: &str,
    // 接收原请求动作。
    input: WindowLifecycleInput,
    // 接收最终同代际观察。
    observation: WindowLifecycleObservation,
    // 接收稳定权限关系。
    permission_relation: &'static str,
    // 标记前景身份是否变化。
    foreground_changed: bool,
) -> Value {
    // 返回完整成功数据供 app facade 包装。
    json!({
        // 返回固定 capability。
        "capability": capabilities::WINDOW_LIFECYCLE,
        // 回显调用方 opaque 目标。
        "targetId": session_id,
        // 返回封闭动作名称。
        "action": input.operation.action(),
        // 返回最终完成结果。
        "outcome": "completed",
        // 返回同步状态机完成状态。
        "dispatchState": "completed",
        // 平台调用已经接受。
        "accepted": true,
        // 同代际读回达到最终状态。
        "finalStateReached": true,
        // 返回 provider-neutral 窗口状态。
        "state": observation.state.as_str(),
        // 几何动作回显显式空间，状态动作使用同一最终外框空间。
        "coordinateSpace": input
            // 读取几何动作的显式坐标空间。
            .operation
            // 将可选领域坐标映射为稳定公开文本。
            .coordinate_space()
            // 状态动作的最终外框同样使用虚拟桌面物理像素。
            .map_or("screen-physical-px", |space| space.as_str()),
        // 返回最终窗口外框。
        "bounds": {
            // 返回带符号横坐标。
            "x": observation.bounds.x,
            // 返回带符号纵坐标。
            "y": observation.bounds.y,
            // 返回物理像素宽度。
            "width": observation.bounds.width,
            // 返回物理像素高度。
            "height": observation.bounds.height,
        },
        // 返回最终窗口 DPI。
        "dpi": observation.dpi,
        // 返回当前 DPI 下系统最小 tracking size。
        "minimumSize": {
            // 返回最小宽度。
            "width": observation.minimum_size.width,
            // 返回最小高度。
            "height": observation.minimum_size.height,
        },
        // 返回最终读回使用的虚拟桌面边界。
        "virtualScreen": {
            // 返回虚拟桌面左边界。
            "x": observation.virtual_screen.x,
            // 返回虚拟桌面上边界。
            "y": observation.virtual_screen.y,
            // 返回虚拟桌面宽度。
            "width": observation.virtual_screen.width,
            // 返回虚拟桌面高度。
            "height": observation.virtual_screen.height,
        },
        // 声明 Per-Monitor-V2 物理像素上下文。
        "coordinateContext": "per-monitor-v2-virtual-screen-physical-px",
        // 声明虚拟桌面坐标支持负值。
        "signedVirtualScreenCoordinates": true,
        // 声明写前重新解析精确目标。
        "targetReresolvedBeforeDispatch": true,
        // 如实公开写前与读回采用的窗口身份材料及其停止线。
        "targetIdentityStrength": window_target_identity::public_assurance(),
        // 如实报告前景身份变化。
        "foregroundChangedDuringDispatch": foreground_changed,
        // 返回静态权限预检事实。
        "permissionPreflight": permission_relation,
        // 明确没有主动试写权限探针。
        "activeWriteProbePerformed": false,
        // 声明确认在写前评估。
        "confirmationEvaluatedBeforeDispatch": true,
        // 声明前景同意在写前评估。
        "foregroundConsentEvaluatedBeforeDispatch": true,
        // 成功结果也不授权调用方自动重复 mutation。
        "retrySafe": false,
        // 明确禁止自动重试。
        "automaticRetryProhibited": true,
    })
}

// 判断 dispatch 期间的前景转移是否属于已同意的精确目标生命周期影响。
fn foreground_transition_authorized(
    // 接收调用方请求的封闭动作。
    operation: WindowLifecycleOperation,
    // 接收精确目标私有句柄值。
    target: isize,
    // 接收 dispatch 前宿主前景句柄值。
    before: isize,
    // 接收当前宿主前景句柄值。
    after: isize,
) -> bool {
    // 前景未变化始终符合契约。
    if after == before {
        // 返回授权成立。
        return true;
    }
    // 后台目标只允许前景转移到同一个精确目标。
    if before != target {
        // 第三方窗口变化继续视为宿主干扰。
        return after == target;
    }
    // 前景目标最小化后由 shell 选择其他前景属于已同意影响。
    operation == WindowLifecycleOperation::Minimize
}

// 使用显式取消探针执行正式同步窗口生命周期状态机。
fn perform_with_cancel_probe(
    // 接收调用方可能缺失的 opaque 目标。
    session_id: Option<&str>,
    // 接收逐操作确认。
    confirmed: bool,
    // 接收预先前景影响同意。
    foreground_consent: bool,
    // 接收调用方可能缺失的 provider-neutral input 对象。
    value: Option<&Value>,
    // 接收生产取消信号或测试探针。
    cancelled: fn() -> bool,
) -> AppResult<Value> {
    // 从 Module 入口记录整个 Command 的单调起点。
    let started = Instant::now();
    // confirmation 必须先于 input、目标发现和任何平台调用。
    if !confirmed {
        // 返回稳定确认错误。
        return Err(WindowLifecycleErrorCode::ConfirmationRequired.error(
            // 明确窗口生命周期是 mutation。
            "Window lifecycle control requires explicit per-operation confirmation.",
        ));
    }
    // 前景影响同意必须先于 input、目标发现和任何平台调用。
    if !foreground_consent {
        // 返回稳定前景同意错误。
        return Err(WindowLifecycleErrorCode::ForegroundConsentRequired.error(
            // 明确全部可见状态与几何动作属于前景影响域。
            "Window lifecycle control requires explicit upfront foreground consent.",
        ));
    }
    // confirmation 与前景同意通过后才检查必需输入。
    let value = value.ok_or_else(|| {
        // 返回稳定缺失输入错误。
        WindowLifecycleErrorCode::InvalidArgument.error(
            // 不读取或回显目标信息。
            "args.input is required for window.lifecycle@1.",
        )
    })?;
    // 严格解析单个 provider-neutral 动作。
    let input = parse_window_lifecycle_input(value)?;
    // 构造覆盖整个同步 Command 的中断生命周期。
    let interrupt = InterruptDeadline::new(started, input.timeout_ms, cancelled);
    // 在任何目标解析前执行首次中断检查。
    interrupt.check().map_err(pre_dispatch_interrupt)?;
    // 输入与首次中断检查通过后才读取必需目标。
    let session_id = session_id
        // 空目标与缺失目标使用同一稳定参数错误。
        .filter(|value| !value.is_empty())
        // 构造不泄漏输入或原生事实的错误。
        .ok_or_else(|| {
            // 返回统一缺失目标错误。
            WindowLifecycleErrorCode::InvalidArgument.error(
                // 说明公开 target 字段要求。
                "target.sessionId is required for window.lifecycle@1.",
            )
        })?;
    // 验证 canonical s2:w。
    validate_target(session_id)?;
    // 重新枚举当前完整窗口清单。
    let records = current_windows()?;
    // 唯一解析并复制精确私有目标。
    let target = resolve_window(session_id, &records)?.clone();
    // 执行无主动写探针的静态权限预检。
    let permission_relation = permission_preflight(&target)?;
    // 在平台预检与 dispatch 前再次检查中断。
    interrupt.check().map_err(pre_dispatch_interrupt)?;
    // 记录写前宿主前景身份。
    let foreground_before = foreground_hwnd();
    // 标记调用前目标是否就是前景窗口。
    let target_was_foreground = foreground_before == target.hwnd;
    // 后台目标执行前不允许宿主前景在门禁之间变化。
    if !target_was_foreground && foreground_hwnd() != foreground_before {
        // 明确平台尚未接受调用。
        return Err(
            WindowLifecycleErrorCode::HostInterferenceDetected.with_details(
                // 说明写前宿主干扰。
                "Foreground changed before window lifecycle dispatch.",
                // 返回确定性未接受证据。
                json!({
                    // 平台尚未接受动作。
                    "accepted": false,
                    // 标记写前前景竞争。
                    "reason": "foreground-changed-before-dispatch",
                    // 禁止自动重试。
                    "retrySafe": false,
                }),
            ),
        );
    }
    // 执行恰好一次固定状态或几何平台调用。
    window_lifecycle_windows::dispatch(&target, input.operation)
        // 返回成功即建立 accepted 事实。
        .map_err(platform_failure_before_acceptance)?;
    // 从事实建立点起只允许完成或 OutcomeUnknown。
    loop {
        // 尝试读取同一当前 token 与进程代际下的最终事实。
        let observation = match window_lifecycle_windows::observe(&target) {
            // 保存当前身份材料下的观察。
            Ok(observation) => observation,
            // 接受后的任何读回失败都不能解释为未执行。
            Err(failure) => {
                // 返回不可重试未知结果。
                return Err(outcome_unknown(
                    // 使用稳定平台失败原因。
                    accepted_failure_reason(failure),
                    // 说明最终状态无法认证。
                    "The final lifecycle state could not be read after Windows accepted the action.",
                ));
            }
        };
        // 读取当前前景身份。
        let foreground_after = foreground_hwnd();
        // 只允许无变化、后台目标自身获前景或前景目标最小化三类已同意转移。
        if !foreground_transition_authorized(
            // 传入原请求动作。
            input.operation,
            // 传入精确目标私有句柄。
            target.hwnd,
            // 传入 dispatch 前前景。
            foreground_before,
            // 传入当前前景。
            foreground_after,
        ) {
            // 返回 host interference 的未知结果。
            return Err(
                WindowLifecycleErrorCode::HostInterferenceDetected.with_details(
                    // 说明平台接受后的前景变化。
                    "Foreground changed after Windows accepted the lifecycle action.",
                    // 输出不可撤销的未知结果证据。
                    json!({
                        // 平台调用已经接受。
                        "accepted": true,
                        // 最终状态不能认证。
                        "finalStateReached": false,
                        // 固定未知结果。
                        "outcome": "unknown",
                        // 标记未授权前景变化。
                        "reason": "foreground-changed-after-acceptance",
                        // 禁止自动重试。
                        "retrySafe": false,
                        // 明确自动重试被禁止。
                        "automaticRetryProhibited": true,
                    }),
                ),
            );
        }
        // 精确状态或几何读回成立时完成 Command。
        if window_lifecycle_windows::operation_reached(observation, input.operation) {
            // 返回不含 native 标识的完成事实。
            return Ok(success_result(
                // 回显调用方 opaque 目标。
                session_id,
                // 传播已验证请求。
                input,
                // 传播最终身份材料下的观察。
                observation,
                // 传播权限预检事实。
                permission_relation,
                // 如实报告前景身份变化。
                foreground_after != foreground_before,
            ));
        }
        // 未达到最终状态时检查接受后取消或 deadline。
        interrupt.check().map_err(accepted_interrupt)?;
        // 执行短可取消轮询等待。
        interrupt
            // 不超过全局 deadline 等待一个固定切片。
            .pause(FINAL_STATE_POLL_INTERVAL)
            // 接受后中断必须保持 OutcomeUnknown。
            .map_err(accepted_interrupt)?;
    }
}

// 对精确窗口执行一次已确认通用生命周期 Command。
pub(crate) fn perform(
    // 接收调用方可能缺失的 opaque 窗口目标。
    session_id: Option<&str>,
    // 接收逐操作确认。
    confirmed: bool,
    // 接收预先前景影响同意。
    foreground_consent: bool,
    // 接收调用方可能缺失的严格 input 对象。
    value: Option<&Value>,
) -> AppResult<Value> {
    // 使用进程级取消信号执行正式状态机。
    perform_with_cancel_probe(
        // 传播精确目标。
        session_id,
        // 传播确认事实。
        confirmed,
        // 传播前景同意事实。
        foreground_consent,
        // 传播输入对象。
        value,
        // 注入生产取消探针。
        cancellation::is_cancelled,
    )
}

// 测试拆分到独立文件以保持生产 Module 精简。
#[cfg(test)]
#[path = "window_lifecycle_tests.rs"]
mod tests;

// 声明会改变项目自有可见窗口状态的串行动态回归测试。
#[cfg(test)]
#[path = "window_lifecycle_dynamic_tests.rs"]
mod dynamic_tests;
