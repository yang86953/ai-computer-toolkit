//! 标准 Edit 发现、权限门禁、固定写入与回读验证 Module。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "standard_edit_error.rs"]
mod error_code;

// 导入 JSON 构造工具。
use serde_json::{Value, json};

// 导入当前 Module 私有封闭错误码。
use error_code::StandardEditErrorCode;

// 导入平台 Adapter、权限事实和领域错误。
use crate::{
    // 只通过窄 Adapter 访问 Windows，并复用只读清单事实。
    adapters::{
        // 导入固定消息 Adapter 的封闭结果。
        standard_edit_windows::{self, StandardEditWriteFailure},
        // 导入当前控件与进程只读事实。
        windows::{
            IntegrityRelation, ProcessInventory, ProcessMetadataAccess, ProcessRecord,
            WindowRecord, enumerate_process_inventory, filter_standard_edit_controls,
            foreground_hwnd, opaque_control_session_id,
        },
    },
    // 导入版本化 capability ID。
    capabilities,
    // 导入不透明目标匹配与纯权限评估 Component。
    components::{
        // 导入 fail-closed 唯一目标匹配。
        opaque_id::{OpaqueTargetMatch, match_opaque_target},
        // 导入公开兼容合同要求的静态权限规则。
        static_permission_assessment::{
            StaticIntegrityRelation, StaticMetadataAccess, StaticPermissionAssessment,
            assess_static_background_mutation,
        },
    },
    // 导入稳定 JSON over stdio 领域类型。
    domain::{AppControlError, AppResult, JsonMap},
};

// 固定控件清单扫描上限与公开兼容合同一致。
const INVENTORY_LIMIT: usize = 16_384;
// 固定公开 sessions 最大边界。
const MAXIMUM_SESSION_ITEMS: usize = 4_096;
// 固定 UTF-8 文本字节上限。
pub(crate) const MAXIMUM_UTF8_BYTES: usize = 65_536;
// 固定最小同步消息 deadline。
pub(crate) const MINIMUM_TIMEOUT_MS: u32 = 1;
// 固定最大同步消息 deadline。
pub(crate) const MAXIMUM_TIMEOUT_MS: u32 = 30_000;
// 固定缺省同步消息 deadline。
pub(crate) const DEFAULT_TIMEOUT_MS: u32 = 2_000;

// 将 Windows 元数据访问事实映射为纯 Component 输入。
const fn metadata_access(value: ProcessMetadataAccess) -> StaticMetadataAccess {
    // 显式覆盖全部 Windows 分类。
    match value {
        // 传播可用状态。
        ProcessMetadataAccess::Available => StaticMetadataAccess::Available,
        // 传播权限阻塞状态。
        ProcessMetadataAccess::PermissionBlocked => StaticMetadataAccess::PermissionBlocked,
        // 传播不可用状态。
        ProcessMetadataAccess::Unavailable => StaticMetadataAccess::Unavailable,
    }
}

// 将 Windows 完整性事实映射为纯 Component 输入。
const fn integrity_relation(value: IntegrityRelation) -> StaticIntegrityRelation {
    // 显式覆盖全部 Windows 分类。
    match value {
        // 传播较低关系。
        IntegrityRelation::Lower => StaticIntegrityRelation::Lower,
        // 传播同级关系。
        IntegrityRelation::Same => StaticIntegrityRelation::Same,
        // 传播较高关系。
        IntegrityRelation::Higher => StaticIntegrityRelation::Higher,
        // 传播未知关系。
        IntegrityRelation::Unknown => StaticIntegrityRelation::Unknown,
    }
}

// 根据当前进程事实执行无主动写探针的权限评估。
fn permission_assessment(process: Option<&ProcessRecord>) -> StaticPermissionAssessment {
    // 缺少可靠进程事实时保持不可用和未知。
    let Some(process) = process else {
        // 返回保守未知结果。
        return assess_static_background_mutation(
            // 标记元数据不可用。
            StaticMetadataAccess::Unavailable,
            // 标记完整性未知。
            StaticIntegrityRelation::Unknown,
        );
    };
    // 将平台事实输入纯决策器。
    assess_static_background_mutation(
        // 映射元数据状态。
        metadata_access(process.metadata_access),
        // 映射相对完整性。
        integrity_relation(process.integrity_relation),
    )
}

// 在同一进程快照中查找控件绑定的进程代际。
fn find_process<'inventory>(
    // 接收完整进程清单。
    inventory: &'inventory ProcessInventory,
    // 接收私有控件事实。
    control: &WindowRecord,
) -> Option<&'inventory ProcessRecord> {
    // 同时匹配 PID 与创建时间以抵抗 PID 复用。
    inventory.records.iter().find(|process| {
        // PID 必须一致。
        process.process_id == control.process_id
            // 进程代际必须一致。
            && process.process_creation_time == control.process_creation_time
    })
}

// 枚举完整标准 Edit 候选，不接受调用方 native 过滤器。
fn enumerate_controls() -> AppResult<Vec<WindowRecord>> {
    // 使用空目标以扫描全部标准 Edit 控件。
    let controls = filter_standard_edit_controls(&JsonMap::new())?;
    // 超出认证清单边界时不得截断后继续唯一匹配。
    if controls.len() > INVENTORY_LIMIT {
        // 返回后台不可用并禁止任取截断候选。
        return Err(
            StandardEditErrorCode::BackgroundOperationUnavailable.with_details(
                // 不公开控件数量或 native 事实。
                "The Standard Edit inventory exceeds the certified bound.",
                // 返回稳定边界原因。
                json!({
                    // 标记清单超界。
                    "reason": "standard-edit-inventory-limit-exceeded",
                    // 禁止自动重试。
                    "safeToRetryAutomatically": false,
                }),
            ),
        );
    }
    // 返回完整且有界的控件清单。
    Ok(controls)
}

// 在当前完整清单中唯一重新解析 canonical s2:c。
fn resolve_control<'records>(
    // 接收调用方已知 opaque 目标。
    session_id: &str,
    // 接收当前扫描记录。
    records: &'records [WindowRecord],
) -> AppResult<&'records WindowRecord> {
    // 使用公共 Component 检测零、一或多命中。
    match match_opaque_target(
        // 传入调用方 opaque 目标。
        session_id,
        // 遍历当前记录引用。
        records.iter(),
        // 从私有事实重新生成 canonical s2:c。
        |control| Some(opaque_control_session_id(control)),
    ) {
        // 唯一命中返回私有记录。
        OpaqueTargetMatch::Unique(control) => Ok(control),
        // 零命中统一返回 stale。
        OpaqueTargetMatch::Missing => Err(StandardEditErrorCode::StaleSession.error(
            // 不公开 native 身份。
            "The exact standard Edit target is unavailable.",
        )),
        // 多命中必须 fail closed。
        OpaqueTargetMatch::Ambiguous => Err(StandardEditErrorCode::AmbiguousTarget.error(
            // 不公开碰撞候选。
            "The exact standard Edit target resolves to multiple controls.",
        )),
    }
}

// 构造不含 native 标识的公开控件 session。
fn session_json(
    // 接收私有控件事实。
    control: &WindowRecord,
    // 接收同快照进程事实。
    process: Option<&ProcessRecord>,
) -> Value {
    // 执行纯静态权限评估。
    let assessment = permission_assessment(process);
    // 只投影安全应用名称，不返回 PID、HWND 或 class。
    json!({
        // 返回 canonical opaque 控件目标。
        "sessionId": opaque_control_session_id(control),
        // 标记公共目标类别。
        "kind": "control",
        // 标记精确 provider-neutral 控件类型。
        "targetKind": "standard-edit-control",
        // 仅返回可执行文件名提示。
        "applicationName": control.process_name.as_deref().unwrap_or(""),
        // 返回当前可见性事实。
        "visible": control.visible,
        // 返回旧观察面使用的单 capability 摘要。
        "capability": {
            // 返回固定 capability ID。
            "id": capabilities::UI_TEXT_INPUT,
            // 标记写风险。
            "risk": "mutation",
            // 标记认证执行域。
            "executionDomain": "same-session-no-focus",
            // 声明逐操作确认。
            "requiresConfirmation": true,
            // 当前 provider 已迁回 Rust。
            "availability": "available",
        },
        // 返回 app facade 可消费的版本化 descriptor。
        "capabilities": [{
            // 返回固定 capability ID。
            "id": capabilities::UI_TEXT_INPUT,
            // 返回版本号。
            "version": 1,
            // 返回统一 app 动词。
            "verb": "apply",
            // 标记当前可用。
            "availability": "available",
            // 标记无焦点同会话路径。
            "execution": "same-session-no-focus",
            // 声明逐操作确认。
            "requiresConfirmation": true,
            // 声明无需前台同意。
            "requiresForegroundConsent": false,
            // 返回固定输入 schema。
            "inputSchema": "schema://ui/text-input/v1",
        }],
        // 返回纯静态 assessment 事实。
        "assessment": {
            // 返回封闭决策。
            "decision": assessment.decision,
            // 永不直接授权执行。
            "safeToExecuteNow": assessment.safe_to_execute_now,
            // 返回确认要求。
            "requiresConfirmation": assessment.requires_confirmation,
            // 返回前台要求。
            "foregroundRequired": assessment.foreground_required,
            // 返回稳定权限关系。
            "permissionRelation": assessment.permission_relation,
            // 明确没有主动试写。
            "activeWriteProbePerformed": assessment.active_write_probe_performed,
        },
    })
}

// 验证只读观察期间前景没有变化。
fn ensure_foreground_unchanged(before: isize, message: &'static str) -> AppResult<()> {
    // 再次读取前景窗口。
    let after = foreground_hwnd();
    // 任何变化均按宿主干扰拒绝结果。
    if before != after {
        // 返回稳定宿主干扰错误。
        return Err(StandardEditErrorCode::HostInterferenceDetected.error(message));
    }
    // 返回成功。
    Ok(())
}

// 返回 Standard Edit runtime 和权限分布的安全只读状态。
pub(crate) fn status() -> AppResult<Value> {
    // 记录扫描前前景。
    let foreground_before = foreground_hwnd();
    // 扫描全部标准 Edit。
    let controls = enumerate_controls()?;
    // 扫描有界进程权限事实。
    let processes = enumerate_process_inventory(INVENTORY_LIMIT)?;
    // 初始化需要确认计数。
    let mut requires_confirmation = 0_usize;
    // 初始化权限阻塞计数。
    let mut permission_blocked = 0_usize;
    // 初始化未知计数。
    let mut indeterminate = 0_usize;
    // 逐控件执行纯静态评估。
    for control in &controls {
        // 获取当前权限决策。
        let assessment = permission_assessment(find_process(&processes, control));
        // 累加封闭分类。
        match assessment.decision {
            // 累加需要确认。
            "requires-confirmation" => requires_confirmation += 1,
            // 累加权限阻塞。
            "permission-blocked" => permission_blocked += 1,
            // 其他状态均保持未知。
            _ => indeterminate += 1,
        }
    }
    // 确认只读扫描没有改变前景。
    ensure_foreground_unchanged(
        // 传入扫描前快照。
        foreground_before,
        // 提供稳定错误消息。
        "Foreground changed during standard Edit status.",
    )?;
    // 返回无 native 信息的状态。
    Ok(json!({
        // 标记 surface。
        "surface": "win32-control",
        // 返回固定 capability。
        "capability": capabilities::UI_TEXT_INPUT,
        // 标记状态只读。
        "readOnly": true,
        // Windows runtime 固定存在。
        "runtimeDetected": true,
        // 标记 Rust 正式执行已开放。
        "rustExecutionEnabled": true,
        // 返回迁移状态。
        "rustStatus": "available-confirmed-opaque-target",
        // 标记后台保证策略。
        "backgroundPolicy": "guaranteed",
        // 标记前景未变。
        "foregroundUnchanged": true,
        // 明确不公开 native 标识。
        "nativeIdentifiersExposed": false,
        // 明确不公开 runtime 路径。
        "runtimePathExposed": false,
        // 标记写入已开放但仍需确认。
        "writesEnabled": true,
        // 状态查询不执行试写。
        "activeWriteProbes": 0,
        // 返回控件总数。
        "controlCount": controls.len(),
        // 返回需要确认数。
        "requiresConfirmationCount": requires_confirmation,
        // 返回权限阻塞数。
        "permissionBlockedCount": permission_blocked,
        // 返回未知数。
        "indeterminateCount": indeterminate,
    }))
}

// 返回有界、安全、只读的标准 Edit session 清单。
pub(crate) fn sessions(maximum_items: usize) -> AppResult<Value> {
    // 拒绝零或超出公开边界的请求。
    if !(1..=MAXIMUM_SESSION_ITEMS).contains(&maximum_items) {
        // 返回稳定参数错误。
        return Err(StandardEditErrorCode::InvalidArgument.error(
            // 说明允许范围。
            "Standard Edit discovery requires max-items 1..4096.",
        ));
    }
    // 记录扫描前前景。
    let foreground_before = foreground_hwnd();
    // 扫描完整控件清单以保留真实 total。
    let controls = enumerate_controls()?;
    // 扫描进程权限事实。
    let processes = enumerate_process_inventory(INVENTORY_LIMIT)?;
    // 计算返回条数。
    let count = controls.len().min(maximum_items);
    // 投影有界安全 session。
    let public_sessions = controls
        // 遍历引用以保留完整清单。
        .iter()
        // 应用调用方边界。
        .take(count)
        // 从同一快照构造安全结果。
        .map(|control| session_json(control, find_process(&processes, control)))
        // 收集 JSON 数组。
        .collect::<Vec<_>>();
    // 确认只读扫描没有改变前景。
    ensure_foreground_unchanged(
        // 传入扫描前快照。
        foreground_before,
        // 提供稳定错误消息。
        "Foreground changed during standard Edit discovery.",
    )?;
    // 返回安全清单。
    Ok(json!({
        // 返回固定 capability。
        "capability": capabilities::UI_TEXT_INPUT,
        // 标记只读。
        "readOnly": true,
        // 标记结果不是写候选授权。
        "candidateOnly": false,
        // 标记前景未变。
        "foregroundUnchanged": true,
        // 明确不公开 native 标识。
        "nativeIdentifiersExposed": false,
        // 返回有界条数。
        "count": public_sessions.len(),
        // 返回完整总数。
        "total": controls.len(),
        // 标记是否截断。
        "truncated": controls.len() > count,
        // 返回安全 session 数组。
        "sessions": public_sessions,
    }))
}

// 重新检查单一标准 Edit opaque 目标。
pub(crate) fn inspect(session_id: &str) -> AppResult<Value> {
    // 拒绝空目标。
    if session_id.is_empty() {
        // 返回参数错误。
        return Err(StandardEditErrorCode::InvalidArgument.error(
            // 说明需要精确 opaque 目标。
            "Standard Edit inspect requires an exact opaque target.",
        ));
    }
    // 记录扫描前前景。
    let foreground_before = foreground_hwnd();
    // 完整重新发现控件。
    let controls = enumerate_controls()?;
    // 唯一解析目标。
    let control = resolve_control(session_id, &controls)?;
    // 获取同快照进程权限事实。
    let processes = enumerate_process_inventory(INVENTORY_LIMIT)?;
    // 构造安全控件结果。
    let public_control = session_json(control, find_process(&processes, control));
    // 确认只读检查没有改变前景。
    ensure_foreground_unchanged(
        // 传入扫描前快照。
        foreground_before,
        // 提供稳定错误消息。
        "Foreground changed during standard Edit inspection.",
    )?;
    // 返回安全精确检查。
    Ok(json!({
        // 返回固定 capability。
        "capability": capabilities::UI_TEXT_INPUT,
        // 标记只读。
        "readOnly": true,
        // 标记不是宽松候选。
        "candidateOnly": false,
        // 标记前景未变。
        "foregroundUnchanged": true,
        // 明确不公开 native 标识。
        "nativeIdentifiersExposed": false,
        // 返回安全控件观察。
        "control": public_control,
    }))
}

// 判断当前清单是否唯一拥有指定 opaque 控件目标。
pub(crate) fn accepts_session(session_id: &str) -> AppResult<bool> {
    // 完整重新发现控件。
    let controls = enumerate_controls()?;
    // 使用唯一匹配 Component 分类。
    match match_opaque_target(
        // 传入调用方目标。
        session_id,
        // 遍历当前候选。
        controls.iter(),
        // 从私有事实重建 canonical ID。
        |control| Some(opaque_control_session_id(control)),
    ) {
        // 唯一命中表示 provider 拥有当前 session。
        OpaqueTargetMatch::Unique(_) => Ok(true),
        // 零命中允许 facade 查询其他 provider。
        OpaqueTargetMatch::Missing => Ok(false),
        // 指纹碰撞必须 fail closed。
        OpaqueTargetMatch::Ambiguous => Err(StandardEditErrorCode::AmbiguousTarget.error(
            // 不公开候选。
            "The exact standard Edit target resolves to multiple controls.",
        )),
    }
}

// 验证 Standard Edit mutation 的 confirmation-first 输入。
fn validate_mutation_input(
    // 接收文本。
    text: &str,
    // 接收逐操作确认。
    confirmed: bool,
    // 接收同步消息 deadline。
    timeout_ms: u32,
) -> AppResult<()> {
    // 确认必须先于发现、权限读取和写入。
    if !confirmed {
        // 返回稳定确认错误。
        return Err(StandardEditErrorCode::ConfirmationRequired.error(
            // 明确 mutation 风险。
            "Standard Edit text mutation requires confirmation.",
        ));
    }
    // 按 UTF-8 bytes 执行固定上限。
    if text.len() > MAXIMUM_UTF8_BYTES
        // deadline 必须位于封闭范围。
        || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&timeout_ms)
    {
        // 返回稳定参数错误。
        return Err(StandardEditErrorCode::InvalidArgument.error(
            // 说明完整边界。
            "Standard Edit mutation requires at most 65536 UTF-8 bytes and timeout-ms 1..30000.",
        ));
    }
    // 返回输入有效。
    Ok(())
}

// 将平台 Adapter 封闭失败映射为领域错误。
fn write_failure(failure: StandardEditWriteFailure) -> AppControlError {
    // 按固定失败分类映射。
    match failure {
        // stale 不得回退到其他目标。
        StandardEditWriteFailure::Stale => StandardEditErrorCode::StaleSession.error(
            // 不公开 native 变化。
            "The exact standard Edit target identity changed.",
        ),
        // timeout 必须保留 outcome unknown。
        StandardEditWriteFailure::Timeout => StandardEditErrorCode::Timeout.with_details(
            // 说明未在 deadline 内响应。
            "The standard Edit control did not respond before timeout.",
            // 禁止自动重试或声称未写入。
            json!({
                // 同步消息 timeout 后结果未知。
                "outcome": "unknown",
                // 禁止自动重试。
                "retrySafe": false,
                // 目标可能已经变更。
                "targetMayHaveMutated": true,
                // 返回稳定原因。
                "reason": "synchronous-window-message-timeout",
            }),
        ),
        // Windows 明确权限拒绝。
        StandardEditWriteFailure::PermissionDenied => StandardEditErrorCode::PermissionDenied
            .error(
                // 不公开系统错误或 native 目标。
                "Windows denied the certified standard Edit message.",
            ),
        // 其他消息拒绝不得前台降级。
        StandardEditWriteFailure::BackgroundUnavailable => {
            StandardEditErrorCode::BackgroundOperationUnavailable.error(
                // 明确固定消息被拒绝。
                "The standard Edit control rejected the certified message.",
            )
        }
        // 超长回读视为验证失败。
        StandardEditWriteFailure::ReadbackTooLong => StandardEditErrorCode::OperationFailed.error(
            // 说明有界回读失败。
            "Edit readback exceeded the bounded length.",
        ),
        // 无效 UTF-16 视为验证失败。
        StandardEditWriteFailure::InvalidReadback => StandardEditErrorCode::OperationFailed.error(
            // 说明回读编码无效。
            "Standard Edit readback was not valid UTF-16.",
        ),
        // 内容不一致视为验证失败。
        StandardEditWriteFailure::ReadbackMismatch => StandardEditErrorCode::OperationFailed.error(
            // 说明回读不一致。
            "Standard Edit write readback did not match.",
        ),
    }
}

// 对唯一 opaque 控件执行固定 WM_SETTEXT 并验证回读。
pub(crate) fn set_text(
    // 接收 canonical s2:c。
    session_id: &str,
    // 接收 UTF-8 文本。
    text: &str,
    // 接收逐操作确认。
    confirmed: bool,
    // 接收有界 deadline。
    timeout_ms: u32,
) -> AppResult<Value> {
    // 先执行 confirmation 和纯输入边界。
    validate_mutation_input(text, confirmed, timeout_ms)?;
    // 空或畸形目标会在无写入的重新发现中按 stale 拒绝。
    let controls = enumerate_controls()?;
    // 唯一重新解析目标。
    let control = resolve_control(session_id, &controls)?;
    // 读取静态进程权限事实。
    let processes = enumerate_process_inventory(INVENTORY_LIMIT)?;
    // 执行纯静态权限评估。
    let assessment = permission_assessment(find_process(&processes, control));
    // 明确权限阻塞不得调用写 Adapter。
    if assessment.decision == "permission-blocked" {
        // 返回无主动试写的权限错误。
        return Err(StandardEditErrorCode::PermissionDenied.with_details(
            // 说明认证完整性边界。
            "The exact standard Edit target is outside the certified same-integrity mutation boundary.",
            // 只返回稳定权限语义。
            json!({
                // 返回权限关系。
                "permissionRelation": assessment.permission_relation,
                // 明确没有主动试写。
                "activeWriteProbePerformed": false,
                // 禁止自动重试。
                "safeToRetryAutomatically": false,
            }),
        ));
    }
    // 未能证明同级或较低完整性时不得试写。
    if assessment.decision != "requires-confirmation" {
        // 返回后台不可用。
        return Err(
            StandardEditErrorCode::BackgroundOperationUnavailable.with_details(
                // 说明权限关系未知。
                "The exact standard Edit target permission relation is indeterminate.",
                // 只返回稳定权限语义。
                json!({
                    // 返回未知关系。
                    "permissionRelation": assessment.permission_relation,
                    // 明确没有主动试写。
                    "activeWriteProbePerformed": false,
                    // 禁止自动重试。
                    "safeToRetryAutomatically": false,
                }),
            ),
        );
    }
    // 在写 Adapter 前记录前景。
    let foreground_before = foreground_hwnd();
    // 执行固定消息与回读链。
    let result = standard_edit_windows::write_and_readback(control, text, timeout_ms);
    // 写后立即重新读取前景。
    let foreground_after = foreground_hwnd();
    // 前景变化优先报告宿主干扰和潜在 mutation。
    if foreground_before != foreground_after {
        // 返回保守完成语义。
        return Err(
            StandardEditErrorCode::HostInterferenceDetected.with_details(
                // 说明 mutation 期间发生变化。
                "Foreground changed during standard Edit mutation.",
                // 禁止自动重试。
                json!({
                    // 消息调用已经完成或返回。
                    "outcome": "completed",
                    // 禁止自动重试。
                    "retrySafe": false,
                    // 目标可能已变更。
                    "targetMayHaveMutated": true,
                }),
            ),
        );
    }
    // 映射平台封闭失败。
    let evidence = result.map_err(write_failure)?;
    // 返回不含 native 标识的领域成功事实。
    Ok(json!({
        // 返回固定 capability。
        "capability": capabilities::UI_TEXT_INPUT,
        // 回显调用方 opaque 目标。
        "sessionId": session_id,
        // 标记认证执行域。
        "executionDomain": "same-session-no-focus",
        // 标记确认已经取得。
        "confirmed": true,
        // 返回固定回读证据。
        "verifiedByReadback": evidence.verified_by_readback,
        // 只返回 UTF-8 字节数，不回显文本。
        "textBytes": text.len(),
        // 标记前景未变。
        "foregroundUnchanged": true,
        // 明确不公开 native 标识。
        "nativeIdentifiersExposed": false,
    }))
}

// 从 provider-neutral input 读取文本与可选 timeout。
pub(crate) fn provider_input(input: &Value) -> AppResult<(&str, u32)> {
    // 要求 input 为对象。
    let object = input.as_object().ok_or_else(|| {
        // 返回参数错误。
        StandardEditErrorCode::InvalidArgument.error(
            // 说明 provider-neutral 输入形状。
            "Standard Edit input must be an object.",
        )
    })?;
    // 读取必需 UTF-8 文本。
    let text = object
        // 读取固定 text 字段。
        .get("text")
        // 只接受 JSON string。
        .and_then(Value::as_str)
        // 缺失或类型错误统一拒绝。
        .ok_or_else(|| {
            // 返回参数错误。
            StandardEditErrorCode::InvalidArgument.error(
                // 说明必需字段。
                "Standard Edit input.text is required.",
            )
        })?;
    // 读取可选整数 timeout，缺失时使用 2000ms。
    let timeout_ms = match object.get("timeoutMs") {
        // 缺失时使用契约默认。
        None => DEFAULT_TIMEOUT_MS,
        // 只接受可转换为 u32 的 JSON 整数。
        Some(value) => value
            // 读取无符号整数。
            .as_u64()
            // 转换为 u32。
            .and_then(|value| u32::try_from(value).ok())
            // 类型或范围溢出统一拒绝。
            .ok_or_else(|| {
                // 返回参数错误。
                StandardEditErrorCode::InvalidArgument.error(
                    // 说明 timeout 类型。
                    "Standard Edit input.timeoutMs must be an integer.",
                )
            })?,
    };
    // 提前验证文本与 deadline 边界，但不执行 confirmation 门禁。
    if text.len() > MAXIMUM_UTF8_BYTES
        // 验证 timeout 封闭范围。
        || !(MINIMUM_TIMEOUT_MS..=MAXIMUM_TIMEOUT_MS).contains(&timeout_ms)
    {
        // 返回稳定参数错误。
        return Err(StandardEditErrorCode::InvalidArgument.error(
            // 说明完整输入边界。
            "Standard Edit input requires at most 65536 UTF-8 bytes and timeoutMs 1..30000.",
        ));
    }
    // 返回借用文本与已验证 deadline。
    Ok((text, timeout_ms))
}

// 仅在测试构建中验证纯输入与唯一匹配边界。
// 将测试拆分到独立文件，保持生产 Module 低于 900 行。
#[cfg(test)]
#[path = "standard_edit_tests.rs"]
mod tests;
