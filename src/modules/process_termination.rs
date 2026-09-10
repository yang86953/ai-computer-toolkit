//! 精确进程重新解析、保护门禁与两级终止生命周期 Module。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "process_termination_error.rs"]
mod error_code;

// 导入同步等待所需线程与单调时钟。
use std::{
    // 使用短暂停顿避免忙循环。
    thread,
    // 使用单调时钟拥有整个 Command deadline。
    time::{Duration, Instant},
};

// 导入公开 JSON 值与构造器。
use serde_json::{Value, json};

// 导入只读 inventory、平台 Adapter、纯权限 Component 与领域边界。
use crate::{
    // Module 只通过窄 Windows Adapter 进行进程 mutation。
    adapters::{
        // 导入固定 dispatch、保护检查与句柄状态采样。
        process_termination_windows::{self, ProcessTerminationFailure, ProtectedProcessKind},
        // 导入当前进程与窗口只读事实。
        windows::{
            IntegrityRelation, ProcessInventory, ProcessMetadataAccess, ProcessRecord,
            enumerate_process_inventory, enumerate_windows, foreground_hwnd,
        },
    },
    // 导入两个稳定 capability ID。
    capabilities,
    // 导入取消、opaque、风险输入与静态权限 Component。
    components::{
        // 读取当前 CLI 取消信号。
        cancellation,
        // 验证 canonical 目标类别。
        opaque_id::{OpaqueTargetId, OpaqueTargetKind},
        // 解析 capability 固定风险与有界 deadline。
        process_termination_contract::{
            ProcessTerminationInput, ProcessTerminationMode, parse_process_termination_input,
        },
        // 只读评估元数据与完整性，不执行主动写探针。
        static_permission_assessment::{
            StaticIntegrityRelation, StaticMetadataAccess, StaticPermissionAssessment,
            assess_static_background_mutation,
        },
    },
    // 导入稳定 JSON over stdio 错误与结果。
    domain::{AppControlError, AppResult},
};

// 导入当前 Module 私有错误类型。
use error_code::ProcessTerminationErrorCode;

// 固定完整进程 inventory 认证上限。
const INVENTORY_LIMIT: usize = 16_384;
// 固定可取消轮询间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(25);

// 返回所选风险模式的稳定 capability ID。
const fn capability(mode: ProcessTerminationMode) -> &'static str {
    // capability ID 只能由封闭风险模式映射。
    match mode {
        // 优雅模式映射独立高风险 ID。
        ProcessTerminationMode::Graceful => capabilities::PROCESS_TERMINATE_GRACEFUL,
        // 强制模式映射独立 critical ID。
        ProcessTerminationMode::Force => capabilities::PROCESS_TERMINATE_FORCE,
    }
}

// 验证公开目标是 canonical 进程身份。
fn validate_target(session_id: &str) -> AppResult<()> {
    // 解析 canonical ID 并核对进程类别。
    let valid = OpaqueTargetId::parse(session_id)
        // 只接受 s2:p。
        .is_some_and(|target| target.kind() == OpaqueTargetKind::Process);
    // 合法目标直接通过。
    if valid {
        // 结束目标形状验证。
        return Ok(());
    }
    // legacy、PID 或错误类别目标统一拒绝。
    Err(ProcessTerminationErrorCode::InvalidArgument.error(
        // 不回显调用方原始值。
        "Process termination requires a canonical s2:p target.",
    ))
}

// 枚举完整且有界的当前进程清单。
fn current_processes() -> AppResult<ProcessInventory> {
    // 多取一项以检测超过认证上限的清单。
    let inventory = enumerate_process_inventory(INVENTORY_LIMIT + 1)?;
    // 来源失败或被上限截断时不得任取候选。
    if !inventory.complete || inventory.records.len() > INVENTORY_LIMIT {
        // 返回失败闭合的后台不可用结果。
        return Err(
            ProcessTerminationErrorCode::BackgroundOperationUnavailable.with_details(
                // 不公开实际进程数量。
                "The process inventory is incomplete or exceeds the certified bound.",
                // 提供稳定公开原因。
                json!({
                    // 标记 inventory 无法认证。
                    "reason": "process-inventory-incomplete-or-over-limit",
                    // 明确尚未 dispatch。
                    "accepted": false,
                    // 重新发现后可以由调用方决定是否重试。
                    "retrySafe": true,
                }),
            ),
        );
    }
    // 返回完整清单供唯一匹配。
    Ok(inventory)
}

// 在完整 inventory 中唯一重新解析 opaque 进程目标。
fn resolve_process<'records>(
    // 接收调用方 canonical s2:p。
    session_id: &str,
    // 接收当前完整进程事实。
    records: &'records [ProcessRecord],
) -> AppResult<&'records ProcessRecord> {
    // 收集全部相同 opaque 指纹以检测理论碰撞。
    let matches = records
        // 遍历当前记录。
        .iter()
        // 只保留同一 canonical session。
        .filter(|record| record.session_id == session_id)
        // 收集引用供数量门禁。
        .collect::<Vec<_>>();
    // 按命中数量明确区分 stale、唯一与歧义。
    match matches.as_slice() {
        // 唯一命中返回私有进程记录。
        [process] => Ok(process),
        // 零命中表示进程退出或代际变化。
        [] => Err(ProcessTerminationErrorCode::StaleSession.error(
            // 不公开 PID、名称或创建时间。
            "The exact process target is stale or unavailable.",
        )),
        // 多命中必须失败闭合。
        _ => Err(ProcessTerminationErrorCode::AmbiguousTarget.error(
            // 不公开碰撞候选。
            "The exact process target resolves to multiple current records.",
        )),
    }
}

// 把 Windows inventory 元数据映射为共享静态权限输入。
const fn metadata_access(value: ProcessMetadataAccess) -> StaticMetadataAccess {
    // 穷举全部访问分类。
    match value {
        // 映射元数据可用。
        ProcessMetadataAccess::Available => StaticMetadataAccess::Available,
        // 映射明确权限拒绝。
        ProcessMetadataAccess::PermissionBlocked => StaticMetadataAccess::PermissionBlocked,
        // 映射暂不可用。
        ProcessMetadataAccess::Unavailable => StaticMetadataAccess::Unavailable,
    }
}

// 把 Windows inventory 完整性映射为共享静态权限输入。
const fn integrity_relation(value: IntegrityRelation) -> StaticIntegrityRelation {
    // 穷举全部相对关系。
    match value {
        // 映射较低完整性。
        IntegrityRelation::Lower => StaticIntegrityRelation::Lower,
        // 映射相同完整性。
        IntegrityRelation::Same => StaticIntegrityRelation::Same,
        // 映射较高完整性。
        IntegrityRelation::Higher => StaticIntegrityRelation::Higher,
        // 映射未知关系。
        IntegrityRelation::Unknown => StaticIntegrityRelation::Unknown,
    }
}

// 执行不含主动写探针的进程权限评估。
fn permission_assessment(process: &ProcessRecord) -> StaticPermissionAssessment {
    // 复用共享静态矩阵，避免 Module 自创权限顺序。
    assess_static_background_mutation(
        // 传播元数据访问分类。
        metadata_access(process.metadata_access),
        // 传播相对完整性分类。
        integrity_relation(process.integrity_relation),
    )
}

// 将静态权限失败映射为稳定公开错误。
fn ensure_permission(
    // 接收已重新解析的精确进程。
    process: &ProcessRecord,
) -> AppResult<StaticPermissionAssessment> {
    // 先执行纯权限评估。
    let assessment = permission_assessment(process);
    // 同级或较低完整性且元数据可用时允许确认后继续。
    if assessment.permission_relation == "no-static-integrity-block-observed" {
        // 返回完整静态证据供成功结果投影。
        return Ok(assessment);
    }
    // 其他状态一律不尝试提权或主动写探针。
    Err(ProcessTerminationErrorCode::PermissionDenied.with_details(
        // 不泄漏 token、RID 或原生访问错误。
        "Static process permission preflight did not authorize termination.",
        // 只公开共享 Component 的稳定原因。
        json!({
            // 提供权限关系原因。
            "reason": assessment.permission_relation,
            // 明确尚未 dispatch。
            "accepted": false,
            // 明确没有主动试写。
            "activeWriteProbePerformed": assessment.active_write_probe_performed,
            // 权限或完整性变化后才适合人工重试。
            "retrySafe": false,
        }),
    ))
}

// 判断当前前景窗口是否属于精确进程代际。
fn target_owns_foreground(process: &ProcessRecord, foreground: isize) -> AppResult<bool> {
    // 没有前景窗口时目标不占用前景。
    if foreground == 0 {
        // 返回明确否定。
        return Ok(false);
    }
    // 枚举当前顶层窗口供 PID 与代际核对。
    let windows = enumerate_windows()?;
    // 只有同一 HWND、PID 与创建代际才视为目标前景。
    Ok(windows.iter().any(|window| {
        // 核对当前私有前景 HWND。
        window.hwnd == foreground
            // 核对进程 PID。
            && window.process_id == process.process_id
            // 核对进程创建代际。
            && window.process_creation_time == process.process_creation_time
    }))
}

// 映射写前平台失败为稳定领域错误。
fn pre_dispatch_failure(
    // 接收封闭平台失败。
    failure: ProcessTerminationFailure,
    // 接收公开风险模式供详情投影。
    mode: ProcessTerminationMode,
) -> AppControlError {
    // 只处理事实建立前的失败分类。
    match failure {
        // 进程退出或代际变化统一 stale。
        ProcessTerminationFailure::Stale => ProcessTerminationErrorCode::StaleSession
            // 提供不含 native 身份的稳定代际原因。
            .with_details(
                "The exact process generation changed before dispatch.",
                json!({
                    // 区分 PID 复用或写前退出。
                    "reason": "process-exited-or-generation-changed-before-dispatch",
                    // 明确尚未接受。
                    "accepted": false,
                    // 重新发现新目标后可重试。
                    "retrySafe": true,
                }),
            ),
        // 明确 Windows 访问拒绝映射权限错误。
        ProcessTerminationFailure::PermissionDenied => {
            ProcessTerminationErrorCode::PermissionDenied.with_details(
                "Windows denied the exact process termination request.",
                json!({
                    // 隐藏具体访问掩码和原生错误。
                    "reason": "process-access-denied",
                    // 明确尚未接受。
                    "accepted": false,
                    // 权限变化前禁止自动重试。
                    "retrySafe": false,
                }),
            )
        }
        // 固定保护目标映射权限错误与封闭原因。
        ProcessTerminationFailure::Protected(kind) => {
            // 将私有保护类别映射为稳定公开原因。
            let reason = match kind {
                // 当前工具自保护。
                ProtectedProcessKind::CurrentTool => "protected-current-tool-process",
                // PID 0/4 系统保护。
                ProtectedProcessKind::SystemProcess => "protected-system-process",
                // Windows critical 保护。
                ProtectedProcessKind::CriticalProcess => "protected-critical-process",
                // 非当前桌面会话保护。
                ProtectedProcessKind::OtherSession => "protected-other-windows-session",
            };
            // 返回不允许重试的保护错误。
            ProcessTerminationErrorCode::PermissionDenied.with_details(
                "The exact process is protected from termination.",
                json!({
                    // 输出封闭保护原因。
                    "reason": reason,
                    // 明确尚未接受。
                    "accepted": false,
                    // 保护策略不得被自动重试绕过。
                    "retrySafe": false,
                }),
            )
        }
        // 无法认证 critical 或会话状态时按权限保护失败闭合。
        ProcessTerminationFailure::ProtectionCheckFailed => {
            ProcessTerminationErrorCode::PermissionDenied.with_details(
                "The process protection state could not be certified.",
                json!({
                    // 提供稳定保护状态原因。
                    "reason": "process-protection-state-unavailable",
                    // 明确尚未接受。
                    "accepted": false,
                    // 禁止自动重试权限未知目标。
                    "retrySafe": false,
                }),
            )
        }
        // 优雅协议缺失只影响高风险 capability。
        ProcessTerminationFailure::Unsupported => {
            ProcessTerminationErrorCode::CapabilityUnsupported.with_details(
                "The exact process has no certifiable top-level window close protocol.",
                json!({
                    // 标记固定协议缺口。
                    "reason": "no-current-top-level-window-close-target",
                    // 回显调用方所选公开 capability。
                    "capability": capability(mode),
                    // 明确没有强制回退。
                    "gracefulFallbackToForce": false,
                    // 明确尚未接受。
                    "accepted": false,
                }),
            )
        }
        // 平台在接受前的其他失败保持通用错误。
        ProcessTerminationFailure::OperationFailed
        // 写前等待采样失败也尚未建立 mutation 事实。
        | ProcessTerminationFailure::WaitFailed => {
            ProcessTerminationErrorCode::OperationFailed.with_details(
                "The exact process termination request was not accepted.",
                json!({
                    // 标记失败发生在 dispatch 前。
                    "reason": "process-platform-pre-dispatch-failure",
                    // 明确尚未接受。
                    "accepted": false,
                    // 无 mutation 事实时允许重新发现后人工重试。
                    "retrySafe": true,
                }),
            )
        }
    }
}

// 构造 dispatch 已接受后的统一未知结果。
fn accepted_unknown(
    // 接收公开风险模式。
    mode: ProcessTerminationMode,
    // 接收调用方已知 opaque 目标。
    session_id: &str,
    // 接收封闭未知结果原因。
    reason: &'static str,
) -> AppControlError {
    // 所有接受后失败统一禁止自动重试。
    ProcessTerminationErrorCode::OutcomeUnknown.with_details(
        // 明确只能通过只读重新观察判断后续事实。
        "Windows accepted the process termination request, but final exit could not be certified.",
        // 只输出 provider-neutral 事实。
        json!({
            // 回显公开 capability。
            "capability": capability(mode),
            // 回显调用方原 opaque 目标。
            "targetId": session_id,
            // 输出封闭动作名称。
            "action": mode.action(),
            // 输出封闭未知原因。
            "reason": reason,
            // 平台事实建立点已经越过。
            "accepted": true,
            // dispatch 已提交但最终状态未知。
            "dispatchState": "accepted",
            // 不宣称最终状态成立。
            "finalStateReached": false,
            // 明确优雅路径没有强制回退。
            "gracefulFallbackToForce": false,
            // 指示唯一安全的重新观察入口。
            "reobserveWith": capabilities::PROCESS_DISCOVER,
            // 重新观察期待目标缺失但不预断言。
            "expectedObservation": "target-absent-or-still-running",
            // 禁止调用方自动重复不可回滚请求。
            "retrySafe": false,
            // 冻结更明确的自动重试禁令。
            "automaticRetryProhibited": true,
        }),
    )
}

// 构造写前取消或 deadline 错误。
fn pre_dispatch_interrupt(
    // 标记是取消还是 deadline。
    cancelled: bool,
) -> AppControlError {
    // 根据唯一原因选择稳定错误码与消息。
    if cancelled {
        // 取消发生在任何平台事实建立前。
        return ProcessTerminationErrorCode::Cancelled.with_details(
            "Process termination was cancelled before platform dispatch.",
            json!({ "accepted": false, "retrySafe": true }),
        );
    }
    // deadline 耗尽但尚未 dispatch。
    ProcessTerminationErrorCode::Timeout.with_details(
        "Process termination timed out before platform dispatch.",
        json!({ "accepted": false, "retrySafe": true }),
    )
}

// 在 dispatch 前统一检查取消、deadline 与前景竞争。
fn ensure_pre_dispatch_state(
    // 接收同步 Command 起点。
    started: Instant,
    // 接收已验证 deadline。
    timeout_ms: u32,
    // 接收可测试取消探针。
    cancelled: fn() -> bool,
    // 接收首次认证的私有前景身份。
    foreground_before: isize,
) -> AppResult<()> {
    // 取消优先于 deadline 分类。
    if cancelled() {
        // 返回确定性未接受取消。
        return Err(pre_dispatch_interrupt(true));
    }
    // 同步 deadline 覆盖解析、权限与平台预检。
    if started.elapsed() >= Duration::from_millis(u64::from(timeout_ms)) {
        // 返回确定性未接受 timeout。
        return Err(pre_dispatch_interrupt(false));
    }
    // dispatch 紧邻前景必须仍与首次认证相同。
    if foreground_hwnd() != foreground_before {
        // 不向受竞争的主机状态提交 mutation。
        return Err(
            ProcessTerminationErrorCode::HostInterferenceDetected.with_details(
                "The foreground target changed before process termination dispatch.",
                json!({
                    // 标记主机竞争发生在事实建立前。
                    "reason": "foreground-changed-before-dispatch",
                    // 明确尚未接受。
                    "accepted": false,
                    // 重新发现后可以人工重试。
                    "retrySafe": true,
                }),
            ),
        );
    }
    // 所有写前状态仍可认证。
    Ok(())
}

// 等待同一私有进程句柄退出并拥有所有接受后语义。
fn await_exit(
    // 接收已经平台接受的私有进程句柄。
    process: &process_termination_windows::PreparedProcess,
    // 接收风险模式供错误与成功映射。
    mode: ProcessTerminationMode,
    // 接收调用方 opaque 目标。
    session_id: &str,
    // 接收整个 Command 起点。
    started: Instant,
    // 接收已验证 deadline。
    timeout_ms: u32,
    // 接收可测试取消探针。
    cancelled: fn() -> bool,
    // 接收写前前景身份。
    foreground_before: isize,
) -> AppResult<()> {
    // 计算固定同步 deadline。
    let deadline = started + Duration::from_millis(u64::from(timeout_ms));
    // 使用短轮询保持取消响应与最终状态认证。
    loop {
        // 接受后的前景变化使宿主不干扰承诺不可认证。
        if foreground_hwnd() != foreground_before {
            // 返回不可自动重试的未知结果。
            return Err(accepted_unknown(
                mode,
                session_id,
                "host-foreground-interference",
            ));
        }
        // 只以同一持有句柄 signaled 作为最终退出事实。
        match process_termination_windows::exited(process) {
            // 同一进程对象已退出，最终状态成立。
            Ok(true) => return Ok(()),
            // 仍运行时继续检查取消与 deadline。
            Ok(false) => {}
            // 接受后句柄状态读取失败必须未知。
            Err(_) => {
                // 禁止将等待失败误报为未执行。
                return Err(accepted_unknown(
                    mode,
                    session_id,
                    "process-handle-wait-failed",
                ));
            }
        }
        // 接受后的取消无法撤回既有请求。
        if cancelled() {
            // 返回不可重试未知结果。
            return Err(accepted_unknown(
                mode,
                session_id,
                "cancelled-after-dispatch",
            ));
        }
        // 接受后的 deadline 不证明目标拒绝或仍将运行。
        if Instant::now() >= deadline {
            // 返回不可重试未知结果。
            return Err(accepted_unknown(
                mode,
                session_id,
                "deadline-after-dispatch",
            ));
        }
        // 只休眠到剩余 deadline 与固定间隔的较小值。
        let pause = deadline
            // 计算尚未耗尽的单调剩余时间。
            .saturating_duration_since(Instant::now())
            // 限制单次不可取消等待。
            .min(POLL_INTERVAL);
        // 短暂休眠避免忙循环。
        thread::sleep(pause);
    }
}

// 执行一次 confirmation-first 的精确进程终止 Command。
pub(crate) fn perform(
    // 接收可能缺失的公开 opaque 目标。
    session_id: Option<&str>,
    // 接收逐操作确认。
    confirmed: bool,
    // 接收可能缺失的 provider-neutral input。
    value: Option<&Value>,
    // 接收 capability 已选择的固定风险模式。
    mode: ProcessTerminationMode,
) -> AppResult<Value> {
    // 使用生产取消信号执行完整状态机。
    perform_with_cancel_probe(
        session_id,
        confirmed,
        value,
        mode,
        cancellation::is_cancelled,
    )
}

// 允许测试注入取消探针而不改变生产状态机。
pub(super) fn perform_with_cancel_probe(
    // 接收可能缺失的公开 opaque 目标。
    session_id: Option<&str>,
    // 接收逐操作确认。
    confirmed: bool,
    // 接收可能缺失的公开输入。
    value: Option<&Value>,
    // 接收固定风险模式。
    mode: ProcessTerminationMode,
    // 接收无参数取消探针。
    cancelled: fn() -> bool,
) -> AppResult<Value> {
    // confirmation 必须先于 input、目标、inventory 与平台调用。
    if !confirmed {
        // 返回稳定确认缺口。
        return Err(ProcessTerminationErrorCode::ConfirmationRequired
            .error("Exact process termination requires explicit per-operation confirmation."));
    }
    // 从确认后的第一个步骤开始计算整个同步 deadline。
    let started = Instant::now();
    // 缺失 input 必须在任何目标发现前失败。
    let value = value.ok_or_else(|| {
        // 返回固定输入缺失错误。
        ProcessTerminationErrorCode::InvalidArgument
            .error("args.input is required for process termination.")
    })?;
    // 严格解析仅含可选 timeoutMs 的公开输入。
    let input = parse_process_termination_input(mode, value)?;
    // 写前取消必须先于目标读取与 inventory。
    if cancelled() {
        // 返回确定性未接受取消。
        return Err(pre_dispatch_interrupt(true));
    }
    // 读取非空目标字符串。
    let session_id = session_id
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            // 返回固定目标缺失错误。
            ProcessTerminationErrorCode::InvalidArgument
                .error("target.sessionId is required for process termination.")
        })?;
    // 验证 canonical s2:p 形状。
    validate_target(session_id)?;
    // 读取当前完整进程 inventory。
    let inventory = current_processes()?;
    // 唯一重新解析精确进程代际。
    let target = resolve_process(session_id, &inventory.records)?;
    // best-effort 快照身份不能进入 mutation。
    if !target.identity_reliable {
        // 先区分明确权限阻塞与普通生命周期身份缺口。
        if target.metadata_access == ProcessMetadataAccess::PermissionBlocked {
            // 明确权限拒绝保持权限错误。
            return Err(ProcessTerminationErrorCode::PermissionDenied.with_details(
                "Windows denied process lifetime identity metadata.",
                json!({
                    // 提供稳定权限原因。
                    "reason": "target-metadata-permission-blocked",
                    // 明确尚未接受。
                    "accepted": false,
                    // 禁止无权限变化的自动重试。
                    "retrySafe": false,
                }),
            ));
        }
        // 其他创建时间缺口表示当前 capability 不可认证。
        return Err(
            ProcessTerminationErrorCode::CapabilityUnsupported.with_details(
                "The exact process does not have a process-lifetime identity.",
                json!({
                    // 标记身份新鲜度缺口。
                    "reason": "process-lifetime-identity-unavailable",
                    // 明确尚未接受。
                    "accepted": false,
                    // 重新发现后可以人工重试。
                    "retrySafe": true,
                }),
            ),
        );
    }
    // 执行无主动写探针的权限门禁。
    let permission = ensure_permission(target)?;
    // 保存首次可认证前景身份。
    let foreground_before = foreground_hwnd();
    // 当前前景属于目标时成功必然改变宿主前景，写前拒绝。
    if target_owns_foreground(target, foreground_before)? {
        // 返回后台不干扰能力不可用。
        return Err(
            ProcessTerminationErrorCode::BackgroundOperationUnavailable.with_details(
                "The exact process currently owns the foreground window.",
                json!({
                    // 输出稳定前景原因。
                    "reason": "exact-process-currently-foreground",
                    // 明确尚未接受。
                    "accepted": false,
                    // 禁止前台循环自动重试。
                    "retrySafe": false,
                }),
            ),
        );
    }
    // 在打开进程前检查取消、deadline 与前景竞争。
    ensure_pre_dispatch_state(
        // 传播 Command 起点。
        started,
        // 传播已验证 deadline。
        input.timeout_ms,
        // 传播取消探针。
        cancelled,
        // 传播首次前景身份。
        foreground_before,
    )?;
    // 打开最小权限句柄并重新核对代际、会话与保护目标。
    let process = process_termination_windows::prepare(target, input.mode)
        // 写前平台失败保持确定性分类。
        .map_err(|failure| pre_dispatch_failure(failure, input.mode))?;
    // 紧邻 dispatch 再检查取消、deadline 与前景竞争。
    ensure_pre_dispatch_state(
        // 传播 Command 起点。
        started,
        // 传播已验证 deadline。
        input.timeout_ms,
        // 传播取消探针。
        cancelled,
        // 传播首次前景身份。
        foreground_before,
    )?;
    // 只按 capability 固定模式调用一个平台 dispatch。
    match input.mode {
        // 优雅路径只投递顶层窗口固定关闭请求。
        ProcessTerminationMode::Graceful => {
            // 失败时事实尚未建立，且绝不升级强制终止。
            process_termination_windows::dispatch_graceful(&process)
                .map_err(|failure| pre_dispatch_failure(failure, input.mode))?;
        }
        // 强制路径只调用内核进程终止。
        ProcessTerminationMode::Force => {
            // 失败时事实尚未建立，不尝试其他协议。
            process_termination_windows::dispatch_force(&process)
                .map_err(|failure| pre_dispatch_failure(failure, input.mode))?;
        }
    }
    // 平台接受后只等待同一私有句柄 signaled。
    await_exit(
        // 传入持有的精确进程句柄。
        &process,
        // 传入固定风险模式。
        input.mode,
        // 传入公开 opaque 目标。
        session_id,
        // 传入整个 Command 起点。
        started,
        // 传入已验证 deadline。
        input.timeout_ms,
        // 传入取消探针。
        cancelled,
        // 传入写前前景身份。
        foreground_before,
    )?;
    // 最终成功仍必须保证前景身份不变。
    if foreground_hwnd() != foreground_before {
        // 目标虽已退出但宿主不干扰证据失效。
        return Err(accepted_unknown(
            input.mode,
            session_id,
            "host-foreground-interference-after-exit",
        ));
    }
    // 返回与成功 schema 完全一致的 provider-neutral data。
    Ok(success_result(input, session_id, permission))
}

// 构造不含任何原生进程事实的成功结果。
fn success_result(
    // 接收已经完成的固定风险输入。
    input: ProcessTerminationInput,
    // 接收调用方已知 opaque 目标。
    session_id: &str,
    // 接收静态权限证据。
    permission: StaticPermissionAssessment,
) -> Value {
    // 返回 facade 将提升 targetId 的 data 对象。
    json!({
        // 回显固定 capability ID。
        "capability": capability(input.mode),
        // 回显调用方 opaque 目标供 facade 一致性核对。
        "targetId": session_id,
        // 输出 capability 固定动作。
        "action": input.mode.action(),
        // 输出 capability 固定风险等级。
        "riskLevel": input.mode.risk_level(),
        // 输出 provider-neutral 固定机制。
        "mechanism": input.mode.mechanism(),
        // 最终结果已经完成。
        "outcome": "completed",
        // dispatch 与等待均已完成。
        "dispatchState": "completed",
        // 平台曾接受一次固定请求。
        "accepted": true,
        // 同一句柄最终状态已经成立。
        "finalStateReached": true,
        // 最终状态固定为退出。
        "state": "exited",
        // 写前重新枚举并唯一解析了目标。
        "targetReresolvedBeforeDispatch": true,
        // 打开句柄后再次核对了创建代际。
        "sameProcessGenerationVerified": true,
        // 当前工具、系统、会话与 critical 保护检查已通过。
        "protectedTargetCheck": true,
        // Windows critical 检查已通过。
        "criticalProcessCheck": true,
        // 输出共享静态权限关系。
        "permissionPreflight": permission.permission_relation,
        // 静态评估从未执行主动写探针。
        "activeWriteProbePerformed": permission.active_write_probe_performed,
        // confirmation 在任何输入或目标发现前已经检查。
        "confirmationEvaluatedBeforeDispatch": true,
        // 优雅失败绝不会升级强制终止。
        "gracefulFallbackToForce": false,
        // 指示只读重新观察入口。
        "reobserveWith": capabilities::PROCESS_DISCOVER,
        // 成功后期待原目标缺失。
        "expectedObservation": "target-absent",
        // 最终成功认证宿主前景不变。
        "foregroundUnchanged": true,
        // 不可回滚操作即使成功也不建议重试。
        "retrySafe": false,
        // 明确禁止自动重复请求。
        "automaticRetryProhibited": true,
    })
}

// 纯契约测试拆分到独立文件。
#[cfg(test)]
#[path = "process_termination_tests.rs"]
mod tests;

// 真实项目自有子进程夹具拆分到独立文件。
#[cfg(test)]
#[path = "process_termination_dynamic_tests.rs"]
mod dynamic_tests;
