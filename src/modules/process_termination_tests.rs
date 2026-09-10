//! Process Lifecycle Module 的纯边界与无副作用保护测试。

// 导入父 Module 私有 helper。
use super::*;

// 导入当前进程 ID 供自保护夹具。
use windows::Win32::System::Threading::GetCurrentProcessId;

// 构造稳定私有进程记录。
fn process_record(session_id: &str) -> ProcessRecord {
    // 返回不接触真实进程的最小事实。
    ProcessRecord {
        // 保存测试 opaque ID。
        session_id: session_id.to_owned(),
        // 使用无敏感含义进程名。
        process_name: "fixture.exe".to_owned(),
        // 标记创建代际可靠。
        identity_reliable: true,
        // 标记元数据可用。
        metadata_access: ProcessMetadataAccess::Available,
        // 使用同级完整性。
        integrity_relation: IntegrityRelation::Same,
        // 纯测试不需要窗口关系。
        window_session_ids: Vec::new(),
        // 使用稳定私有 PID。
        process_id: 42,
        // 使用稳定私有创建时间。
        process_creation_time: 123,
    }
}

// 固定返回已取消供写前顺序测试。
const fn always_cancelled() -> bool {
    // 返回取消状态。
    true
}

// 验证 confirmation 严格先于 input 与目标。
#[test]
fn confirmation_precedes_input_and_target_parsing() {
    // 使用全部缺失字段执行未确认请求。
    let error = perform_with_cancel_probe(
        // 不提供目标。
        None,
        // 不提供确认。
        false,
        // 不提供 input。
        None,
        // 选择最高风险模式以证明风险不放宽顺序。
        ProcessTerminationMode::Force,
        // 即使取消也应晚于确认。
        always_cancelled,
    )
    // 未确认不得成功。
    .err()
    // 使用显式 panic 保留失败上下文。
    .unwrap_or_else(|| panic!("unconfirmed process termination must fail"));
    // 确认错误必须最先返回。
    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

// 验证严格 input 先于目标 discovery。
#[test]
fn invalid_input_precedes_missing_target() {
    // 使用非法风险改写字段执行已确认请求。
    let error = perform_with_cancel_probe(
        // 目标保持缺失。
        None,
        // 满足确认门禁。
        true,
        // 提供非法 mode 字段。
        Some(&json!({ "mode": "force" })),
        // capability 固定为优雅模式。
        ProcessTerminationMode::Graceful,
        // 取消不参与本测试。
        || false,
    )
    // 非法输入不得成功。
    .err()
    // 使用显式 panic 标记门禁失效。
    .unwrap_or_else(|| panic!("risk override input must fail"));
    // 必须在目标缺失前返回输入错误。
    assert_eq!(error.code, "INVALID_ARGUMENT");
}

// 验证写前取消不会访问目标 inventory。
#[test]
fn cancellation_precedes_target_inventory() {
    // 使用 canonical 合成目标与空 input。
    let error = perform_with_cancel_probe(
        // 提供合法进程目标形状。
        Some("s2:p:0000000000000000"),
        // 满足逐操作确认。
        true,
        // 使用缺省 deadline。
        Some(&json!({})),
        // 选择强制模式。
        ProcessTerminationMode::Force,
        // 立即取消。
        always_cancelled,
    )
    // 取消请求不得成功。
    .err()
    // 使用显式 panic 保留上下文。
    .unwrap_or_else(|| panic!("pre-dispatch cancellation must fail"));
    // 必须是确定性取消而不是 stale。
    assert_eq!(error.code, "CANCELLED");
    // 明确平台未接受。
    assert_eq!(error.details["accepted"], false);
}

// 验证进程重新解析区分零、一和多命中。
#[test]
fn process_resolution_is_unique_or_fail_closed() -> AppResult<()> {
    // 构造唯一候选。
    let first = process_record("s2:p:1111111111111111");
    // 唯一目标必须返回同一记录。
    let resolved = resolve_process(&first.session_id, std::slice::from_ref(&first))?;
    // 核对唯一进程名。
    assert_eq!(resolved.process_name, "fixture.exe");
    // 空清单必须返回 stale。
    let missing = resolve_process(&first.session_id, &[])
        // 提取预期错误。
        .err()
        // 使用显式 panic。
        .unwrap_or_else(|| panic!("missing process must fail"));
    // 核对 stale 分类。
    assert_eq!(missing.code, "STALE_SESSION");
    // 两个相同 opaque 记录形成理论碰撞。
    let duplicates = [first.clone(), first];
    // 多命中必须返回歧义。
    let ambiguous = resolve_process("s2:p:1111111111111111", &duplicates)
        // 提取预期错误。
        .err()
        // 使用显式 panic。
        .unwrap_or_else(|| panic!("duplicate process target must fail"));
    // 核对歧义分类。
    assert_eq!(ambiguous.code, "AMBIGUOUS_TARGET");
    // 返回测试成功。
    Ok(())
}

// 验证权限矩阵允许同级且阻断更高完整性。
#[test]
fn static_permission_preflight_is_fail_closed() -> AppResult<()> {
    // 构造同级可用进程。
    let allowed = process_record("s2:p:1111111111111111");
    // 同级目标通过静态门禁但仍需调用确认。
    let assessment = ensure_permission(&allowed)?;
    // 核对固定权限关系。
    assert_eq!(
        assessment.permission_relation,
        "no-static-integrity-block-observed"
    );
    // 复制夹具并提升目标完整性。
    let mut higher = allowed;
    // 设置更高完整性。
    higher.integrity_relation = IntegrityRelation::Higher;
    // 更高完整性必须被拒绝。
    let error = ensure_permission(&higher)
        // 提取预期错误。
        .err()
        // 使用显式 panic。
        .unwrap_or_else(|| panic!("higher-integrity process must fail"));
    // 核对权限错误码。
    assert_eq!(error.code, "PERMISSION_DENIED");
    // 核对稳定原因且不公开 RID。
    assert_eq!(error.details["reason"], "target-higher-integrity");
    // 返回测试成功。
    Ok(())
}

// 验证当前工具进程由平台 Adapter 在打开句柄前保护。
#[test]
fn current_tool_process_is_intrinsically_protected() {
    // 构造当前 PID 的合成私有记录。
    let mut current = process_record("s2:p:2222222222222222");
    // 写入真实当前 PID 只用于触发首个保护门禁。
    current.process_id = unsafe { GetCurrentProcessId() };
    // 调用平台准备但不会打开当前进程句柄。
    let failure = process_termination_windows::prepare(&current, ProcessTerminationMode::Force)
        // 成功表示自保护门禁失效。
        .err()
        // 使用显式 panic 保留上下文。
        .unwrap_or_else(|| panic!("current tool process must be protected"));
    // 核对精确保护类别。
    assert_eq!(
        failure,
        ProcessTerminationFailure::Protected(ProtectedProcessKind::CurrentTool)
    );
}

// 验证接受后两种风险都映射为不可重试未知结果。
#[test]
fn accepted_failure_is_never_retry_safe() {
    // 逐项覆盖两个独立风险模式。
    for mode in [
        // 优雅终止。
        ProcessTerminationMode::Graceful,
        // 强制终止。
        ProcessTerminationMode::Force,
    ] {
        // 构造接受后 deadline 错误。
        let error = accepted_unknown(
            // 传播当前模式。
            mode,
            // 使用合成 opaque 目标。
            "s2:p:3333333333333333",
            // 使用稳定原因。
            "deadline-after-dispatch",
        );
        // 核对统一未知结果码。
        assert_eq!(error.code, "OUTCOME_UNKNOWN");
        // 平台必须已接受。
        assert_eq!(error.details["accepted"], true);
        // 禁止自动重试。
        assert_eq!(error.details["retrySafe"], false);
        // 优雅路径永不升级。
        assert_eq!(error.details["gracefulFallbackToForce"], false);
        // 禁止公开原生身份字段。
        let text = error.details.to_string().to_ascii_lowercase();
        // 扫描原生事实。
        for forbidden in ["processid", "handle", "creationtime", "exitcode"] {
            // 任一字段出现都表示隐私边界失效。
            assert!(!text.contains(forbidden));
        }
    }
}

// 验证成功投影与 schema 固定字段完全一致。
#[test]
fn success_projection_is_provider_neutral() {
    // 生成同级权限成功证据。
    let permission = permission_assessment(&process_record("s2:p:4444444444444444"));
    // 逐项覆盖两个成功模式。
    for mode in [
        // 优雅终止。
        ProcessTerminationMode::Graceful,
        // 强制终止。
        ProcessTerminationMode::Force,
    ] {
        // 构造已验证输入。
        let input = ProcessTerminationInput {
            // 保存固定模式。
            mode,
            // deadline 不进入成功公开结果。
            timeout_ms: 5_000,
        };
        // 构造成功结果。
        let result = success_result(input, "s2:p:4444444444444444", permission);
        // 核对最终退出事实。
        assert_eq!(result["state"], "exited");
        // 核对同一代际验证。
        assert_eq!(result["sameProcessGenerationVerified"], true);
        // 核对保护门禁证据。
        assert_eq!(result["protectedTargetCheck"], true);
        // 核对重新观察入口。
        assert_eq!(result["reobserveWith"], capabilities::PROCESS_DISCOVER);
        // 公共结果不得包含原生事实。
        let text = result.to_string().to_ascii_lowercase();
        // 扫描禁止字段。
        for forbidden in ["processid", "pid", "handle", "creationtime", "exitcode"] {
            // 任一字段出现都表示 schema 投影失效。
            assert!(!text.contains(forbidden));
        }
    }
}
