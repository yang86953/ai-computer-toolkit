//! 精确窗口重新解析、前景门禁与固定关闭 Module。

// 把错误码实现保留为当前 Module 的普通私有类型。
#[path = "window_close_error.rs"]
mod error_code;

// 导入 JSON 值与构造工具。
use serde_json::{Value, json};

// 导入只读发现、平台 Adapter、opaque 匹配与领域错误。
use crate::{
    // 领域 Module 只通过窄 Adapter 触碰 Windows。
    adapters::{
        // 导入固定关闭平台结果。
        window_close_windows::{self, WindowCloseFailure},
        // 导入只读窗口清单与前景事实。
        windows::{
            IntegrityRelation, ProcessMetadataAccess, WindowRecord, enumerate_process_inventory,
            enumerate_windows, foreground_hwnd, opaque_window_session_id,
        },
    },
    // 导入固定 capability ID。
    capabilities,
    // 导入 canonical opaque ID 与唯一匹配 Component。
    components::{
        // 导入 canonical opaque ID 与唯一匹配 Component。
        opaque_id::{OpaqueTargetId, OpaqueTargetKind, OpaqueTargetMatch, match_opaque_target},
        // 导入无主动写探针的静态权限评估。
        static_permission_assessment::{
            StaticIntegrityRelation, StaticMetadataAccess, assess_static_background_mutation,
        },
    },
    // 导入稳定 JSON over stdio 错误类型。
    domain::{AppControlError, AppResult},
};

// 导入当前 Module 私有封闭错误码。
use error_code::WindowCloseErrorCode;

// 固定完整窗口扫描认证上限。
const INVENTORY_LIMIT: usize = 16_384;
// 限制一次权限预检最多枚举的进程数。
const PROCESS_INVENTORY_LIMIT: usize = 65_536;
// 固定契约默认 deadline。
pub(crate) const DEFAULT_TIMEOUT_MS: u32 = 2_000;
// 固定契约最大 deadline。
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

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

// 枚举完整且有界的当前窗口清单。
fn current_windows() -> AppResult<Vec<WindowRecord>> {
    // 读取当前顶层窗口私有事实。
    let records = enumerate_windows()?;
    // 超界时不得截断后任取一个碰撞候选。
    if records.len() > INVENTORY_LIMIT {
        // 返回后台不可用并禁止自动重试。
        return Err(
            WindowCloseErrorCode::BackgroundOperationUnavailable.with_details(
                // 不公开窗口数量或 native 事实。
                "The window inventory exceeds the certified bound.",
                // 返回稳定边界原因。
                json!({
                    // 标记完整清单超界。
                    "reason": "window-inventory-limit-exceeded",
                    // 禁止自动重试。
                    "safeToRetryAutomatically": false,
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
        OpaqueTargetMatch::Missing => Err(WindowCloseErrorCode::StaleSession.error(
            // 不公开原生身份。
            "The exact window target is stale or unavailable.",
        )),
        // 多命中必须 fail closed。
        OpaqueTargetMatch::Ambiguous => Err(WindowCloseErrorCode::AmbiguousTarget.error(
            // 明确不会任取窗口。
            "The exact window-close target resolves to multiple windows.",
        )),
    }
}

// 对精确窗口进程执行无主动写探针的静态权限预检。
fn permission_preflight(window: &WindowRecord) -> AppResult<&'static str> {
    // 枚举有界进程权限清单。
    let inventory = enumerate_process_inventory(PROCESS_INVENTORY_LIMIT)?;
    // 截断清单不能证明唯一权限关系。
    if !inventory.complete {
        // 返回明确 assessment 缺口。
        return Err(
            WindowCloseErrorCode::CapabilityAssessmentUnavailable.with_details(
                // 不公开进程数量或 native 事实。
                "The window-close target process inventory exceeds the certified bound.",
                // 返回稳定边界原因。
                json!({
                    // 标记进程清单超界。
                    "reason": "process-inventory-limit-exceeded",
                    // 明确没有主动试写。
                    "activeWriteProbePerformed": false,
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
        return Err(WindowCloseErrorCode::StaleSession.error(
            // 不公开 PID 或创建时间。
            "The exact window-close target process generation is unavailable.",
        ));
    };
    // 执行共享纯静态权限评估。
    let assessment = assess_static_background_mutation(
        // 传播元数据访问事实。
        metadata_access(process.metadata_access),
        // 传播相对完整性事实。
        integrity_relation(process.integrity_relation),
    );
    // 按封闭决策失败闭合。
    match assessment.decision {
        // 同级或较低完整性在逐操作确认后允许固定关闭消息。
        "requires-confirmation" => Ok(assessment.permission_relation),
        // 明确权限阻塞不得调用平台 Adapter。
        "permission-blocked" => Err(WindowCloseErrorCode::PermissionDenied.with_details(
            // 说明静态事实阻塞目标。
            "Static permission facts block closing the exact window target.",
            // 只公开稳定相对权限原因。
            json!({
                // 公开 provider-neutral 权限关系。
                "permissionRelation": assessment.permission_relation,
                // 明确没有主动试写。
                "activeWriteProbePerformed": assessment.active_write_probe_performed,
                // 明确没有发送关闭请求。
                "closeRequested": false,
                // 禁止自动重试。
                "retrySafe": false,
            }),
        )),
        // 未知或新增决策不得放宽。
        _ => Err(
            WindowCloseErrorCode::CapabilityAssessmentUnavailable.with_details(
                // 说明静态事实不足以认证目标。
                "Static permission facts cannot certify closing the exact window target.",
                // 只公开稳定相对权限原因。
                json!({
                    // 公开 provider-neutral 权限关系。
                    "permissionRelation": assessment.permission_relation,
                    // 明确没有主动试写。
                    "activeWriteProbePerformed": assessment.active_write_probe_performed,
                    // 明确没有发送关闭请求。
                    "closeRequested": false,
                    // 禁止自动重试。
                    "retrySafe": false,
                }),
            ),
        ),
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
    Err(WindowCloseErrorCode::InvalidArgument.error(
        // 说明精确目标要求。
        "Window close requires a canonical s2:w target.",
    ))
}

// 验证契约 deadline。
fn validate_timeout(timeout_ms: u32) -> AppResult<()> {
    // 仅接受闭区间 1..30000。
    if (1..=MAXIMUM_TIMEOUT_MS).contains(&timeout_ms) {
        // 合法 deadline 直接通过。
        return Ok(());
    }
    // 拒绝零值或超出认证范围的 deadline。
    Err(WindowCloseErrorCode::InvalidArgument.error(
        // 保持契约错误说明。
        "Window close timeoutMs must be 1..30000.",
    ))
}

// 映射平台封闭失败为稳定领域错误。
fn close_failure(failure: WindowCloseFailure) -> AppControlError {
    // 按封闭枚举返回稳定错误与必要未知结果证据。
    match failure {
        // 写前身份变化是 stale。
        WindowCloseFailure::Stale => WindowCloseErrorCode::StaleSession.error(
            // 不公开句柄或进程。
            "The exact window target is no longer available.",
        ),
        // UIPI 拒绝映射权限错误。
        WindowCloseFailure::PermissionDenied => WindowCloseErrorCode::PermissionDenied.error(
            // 说明系统拒绝固定消息。
            "Windows denied the exact window close request.",
        ),
        // 其他排队失败保持通用错误。
        WindowCloseFailure::OperationFailed => WindowCloseErrorCode::OperationFailed.error(
            // 不包含平台原生错误文本。
            "The exact window rejected the close request.",
        ),
        // 超时必须标记结果未知且禁止重试。
        WindowCloseFailure::Timeout => WindowCloseErrorCode::Timeout.with_details(
            // 说明窗口仍可用但不能推断未处理。
            "The exact window remained available after the close timeout.",
            // 输出固定 outcome-unknown 证据。
            json!({
                // 消息已经排队，结果未知。
                "outcome": "unknown",
                // 禁止自动重试造成重复关闭请求。
                "retrySafe": false,
                // 目标可能稍后关闭。
                "targetMayCloseLater": true,
            }),
        ),
    }
}

// 执行 confirmation-first 的精确后台窗口关闭。
pub(crate) fn close(
    // 接收调用方已知 opaque 目标。
    session_id: &str,
    // 接收逐操作确认。
    confirmed: bool,
    // 接收调用方 deadline。
    timeout_ms: u32,
) -> AppResult<Value> {
    // confirmation 必须先于参数、发现和任何写入。
    if !confirmed {
        // 返回稳定确认错误。
        return Err(WindowCloseErrorCode::ConfirmationRequired.error(
            // 明确关闭是 mutation。
            "Exact window close requires confirmation.",
        ));
    }
    // 验证 canonical s2:w。
    validate_target(session_id)?;
    // 验证 deadline 闭区间。
    validate_timeout(timeout_ms)?;
    // 重新枚举当前完整窗口清单。
    let records = current_windows()?;
    // 唯一解析精确目标。
    let target = resolve_window(session_id, &records)?;
    // 在任何前景读取或平台消息前执行目标静态权限预检。
    let permission_relation = permission_preflight(target)?;
    // 写前读取前景窗口。
    let foreground_before = foreground_hwnd();
    // 当前前景目标关闭必然改变前景，必须在写前拒绝。
    if foreground_before == target.hwnd {
        // 返回后台不可用且明确未请求关闭。
        return Err(WindowCloseErrorCode::BackgroundOperationUnavailable.with_details(
            // 说明拒绝原因但不公开 native 事实。
            "The exact window is currently foreground; closing it would change foreground ownership.",
            // 输出稳定安全证据。
            json!({
                // 标记前景目标原因。
                "reason": "exact-target-currently-foreground",
                // 明确没有发送 WM_CLOSE。
                "closeRequested": false,
                // 禁止调用方自动重试。
                "safeToRetryAutomatically": false,
            }),
        ));
    }
    // 通过窄 Adapter 发送固定 WM_CLOSE 并等待失效。
    let platform_result = window_close_windows::close(target, timeout_ms);
    // 任何消息后结果返回前均复核前景身份。
    let foreground_after = foreground_hwnd();
    // 前景变化时无法认证关闭结果或宿主不干扰。
    if foreground_before != foreground_after {
        // 返回结果未知且禁止自动重试。
        return Err(WindowCloseErrorCode::HostInterferenceDetected.with_details(
            // 说明关闭请求后的前景变化。
            "Foreground changed after the exact window close request.",
            // 输出不可撤销的未知结果证据。
            json!({
                // 目标可能已经关闭或显示确认界面。
                "outcome": "unknown",
                // 禁止自动重试。
                "retrySafe": false,
                // 目标可能已经关闭。
                "targetMayHaveClosed": true,
            }),
        ));
    }
    // 前景稳定后再映射平台结果。
    let evidence = platform_result.map_err(close_failure)?;
    // 返回不含 native 标识的领域成功事实。
    Ok(json!({
        // 返回固定 capability。
        "capability": capabilities::WINDOW_CLOSE,
        // 回显调用方 opaque 目标。
        "targetId": session_id,
        // 标记认证执行域。
        "executionDomain": "same-session-no-focus",
        // 标记逐操作确认已经取得。
        "confirmed": true,
        // 输出无主动写探针的静态权限关系。
        "permissionPreflight": permission_relation,
        // 明确权限评估没有主动试写。
        "activeWriteProbePerformed": false,
        // 固定消息已经成功请求。
        "closeRequested": true,
        // 返回同一窗口身份已经失效。
        "closed": evidence.closed,
        // 标记前景身份保持不变。
        "foregroundUnchanged": true,
        // 明确公共边界无 native 标识。
        "nativeIdentifiersExposed": false,
    }))
}

// 从 provider-neutral input 读取可选 timeout。
pub(crate) fn provider_input(input: &Value) -> AppResult<u32> {
    // 输入必须是对象，允许为空。
    let object = input.as_object().ok_or_else(|| {
        // 返回参数错误。
        WindowCloseErrorCode::InvalidArgument.error(
            // 说明 provider-neutral 输入形状。
            "Window close input must be an object.",
        )
    })?;
    // 只允许固定 timeoutMs 字段。
    if object.keys().any(|key| key != "timeoutMs") {
        // 拒绝任意消息、选择器或其他控制字段。
        return Err(WindowCloseErrorCode::InvalidArgument.error(
            // 明确输入封闭集合。
            "Window close input accepts timeoutMs only.",
        ));
    }
    // 缺失时使用契约默认值。
    let timeout_ms = match object.get("timeoutMs") {
        // 空对象使用 2000ms。
        None => DEFAULT_TIMEOUT_MS,
        // 只接受可转换为 u32 的整数。
        Some(value) => value
            // 读取无符号 JSON 整数。
            .as_u64()
            // 转换为 u32。
            .and_then(|value| u32::try_from(value).ok())
            // 非整数或溢出统一拒绝。
            .ok_or_else(|| {
                // 返回稳定参数错误。
                WindowCloseErrorCode::InvalidArgument.error(
                    // 说明 timeout 类型。
                    "Window close timeoutMs must be an integer.",
                )
            })?,
    };
    // 应用闭区间验证。
    validate_timeout(timeout_ms)?;
    // 返回已验证 deadline。
    Ok(timeout_ms)
}

// 测试拆分到独立文件以保持生产 Module 精简。
#[cfg(test)]
#[path = "window_close_tests.rs"]
mod tests;
