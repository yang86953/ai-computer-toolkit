// 导入被测 Module 私有 helper。
use super::*;
// 导入稳定窗口观察类型。
use crate::adapters::window_lifecycle_windows::{
    WindowRectangle, WindowSize, WindowVisualState, fixture_observation,
};
// 导入窗口坐标空间。
use crate::components::window_lifecycle_contract::{
    WindowLifecycleCoordinateSpace, WindowLifecycleOperation,
};

// 构造不触碰真实桌面的稳定私有窗口记录。
fn window_record(handle: isize) -> WindowRecord {
    // 返回测试专用私有事实。
    WindowRecord {
        // legacy ID 不参与正式匹配。
        session_id: format!("window:{handle}"),
        // 保存测试句柄值。
        hwnd: handle,
        // 使用无敏感含义标题。
        title: "Fixture".to_owned(),
        // 使用无敏感含义类名。
        class_name: "FixtureClass".to_owned(),
        // 使用稳定测试 PID。
        process_id: 42,
        // 使用安全进程名。
        process_name: Some("fixture.exe".to_owned()),
        // 标记夹具可见。
        visible: true,
        // 使用稳定进程代际。
        process_creation_time: 123,
    }
}

// 返回确定性已取消探针。
const fn always_cancelled() -> bool {
    // 固定报告取消。
    true
}

// 验证确认和前景同意均先于 input 与目标解析。
#[test]
fn policy_gates_precede_input_and_target_resolution() {
    // 未确认请求同时提供非法输入和目标。
    let unconfirmed = match perform(None, false, false, None) {
        // 成功表示确认门禁失效。
        Ok(_) => panic!("unconfirmed lifecycle request must fail"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 确认错误必须最先返回。
    assert_eq!(unconfirmed.code, "CONFIRMATION_REQUIRED");
    // 已确认但未同意前景影响仍不得解析 input。
    let no_consent = match perform(None, true, false, None) {
        // 成功表示前景同意门禁失效。
        Ok(_) => panic!("lifecycle request without foreground consent must fail"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 前景同意错误必须先于参数错误。
    assert_eq!(no_consent.code, "FOREGROUND_CONSENT_REQUIRED");
}

// 验证写前取消不触碰目标发现或平台调用。
#[test]
fn cancellation_before_dispatch_is_deterministic() {
    // 使用合法输入但无效目标，并注入立即取消。
    let error = match perform_with_cancel_probe(
        // 目标本应在后续校验失败。
        None,
        // 提供确认。
        true,
        // 提供前景同意。
        true,
        // 提供合法恢复输入。
        Some(&json!({ "action": "restore" })),
        // 注入确定性取消。
        always_cancelled,
    ) {
        // 成功表示取消门禁失效。
        Ok(_) => panic!("pre-dispatch cancellation must fail"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 写前取消必须是确定性 CANCELLED。
    assert_eq!(error.code, "CANCELLED");
}

// 验证零命中与多命中均 fail closed。
#[test]
fn opaque_window_resolution_is_unique() {
    // 构造一个稳定候选。
    let first = window_record(100);
    // 生成其 canonical 目标。
    let session_id = opaque_window_session_id(&first);
    // 空清单必须返回 stale。
    let missing = match resolve_window(&session_id, &[]) {
        // 成功表示零命中门禁失效。
        Ok(_) => panic!("missing lifecycle target must fail"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对 stale 错误码。
    assert_eq!(missing.code, "STALE_SESSION");
    // 使用相同身份构造第二个碰撞候选。
    let second = first.clone();
    // 多命中必须返回歧义。
    let ambiguous = match resolve_window(&session_id, &[first, second]) {
        // 成功表示碰撞门禁失效。
        Ok(_) => panic!("colliding lifecycle target must fail"),
        // 保存预期错误。
        Err(error) => error,
    };
    // 核对歧义错误码。
    assert_eq!(ambiguous.code, "AMBIGUOUS_TARGET");
}

// 验证 accepted 后中断统一变为禁止重试的未知结果。
#[test]
fn accepted_interrupts_are_outcome_unknown() {
    // 映射接受后取消。
    let cancelled = accepted_interrupt(WindowLifecycleInterrupt::Cancelled);
    // 取消不能伪造已经取消。
    assert_eq!(cancelled.code, "OUTCOME_UNKNOWN");
    // 核对平台已经接受。
    assert_eq!(cancelled.details["accepted"], true);
    // 核对禁止自动重试。
    assert_eq!(cancelled.details["retrySafe"], false);
    // 映射接受后 deadline。
    let timed_out = accepted_interrupt(WindowLifecycleInterrupt::TimedOut);
    // 超时同样保持未知结果。
    assert_eq!(timed_out.code, "OUTCOME_UNKNOWN");
    // 核对最终状态未认证。
    assert_eq!(timed_out.details["finalStateReached"], false);
}

// 验证前景影响同意只授权精确目标自身可归因的前景转移。
#[test]
fn foreground_transition_only_allows_exact_target_effects() {
    // 后台目标未改变前景时保持授权。
    assert!(foreground_transition_authorized(
        // 使用恢复动作。
        WindowLifecycleOperation::Restore,
        // 使用精确目标句柄值。
        20,
        // 使用原前景句柄值。
        10,
        // 保持原前景句柄值。
        10,
    ));
    // 后台恢复只允许前景转移到精确目标。
    assert!(foreground_transition_authorized(
        // 使用恢复动作。
        WindowLifecycleOperation::Restore,
        // 使用精确目标句柄值。
        20,
        // 使用原前景句柄值。
        10,
        // 转移到精确目标。
        20,
    ));
    // 后台动作不得把第三方前景竞争当成已同意影响。
    assert!(!foreground_transition_authorized(
        // 使用恢复动作。
        WindowLifecycleOperation::Restore,
        // 使用精确目标句柄值。
        20,
        // 使用原前景句柄值。
        10,
        // 转移到无关第三方窗口。
        30,
    ));
    // 前景目标最大化时必须保持目标前景。
    assert!(!foreground_transition_authorized(
        // 使用最大化动作。
        WindowLifecycleOperation::Maximize,
        // 使用精确目标句柄值。
        20,
        // 目标原本位于前景。
        20,
        // 转移到无关窗口。
        30,
    ));
    // 前景目标最小化后允许 shell 选择后续前景。
    assert!(foreground_transition_authorized(
        // 使用最小化动作。
        WindowLifecycleOperation::Minimize,
        // 使用精确目标句柄值。
        20,
        // 目标原本位于前景。
        20,
        // shell 选择另一个前景窗口。
        30,
    ));
}

// 验证成功结果只包含 provider-neutral 最终事实。
#[test]
fn success_result_exposes_no_native_identifiers() {
    // 构造合法移动请求。
    let input = WindowLifecycleInput {
        // 使用带符号屏幕物理坐标移动。
        operation: WindowLifecycleOperation::Move {
            // 使用认证坐标空间。
            coordinate_space: WindowLifecycleCoordinateSpace::ScreenPhysicalPx,
            // 使用负横坐标。
            x: -100,
            // 使用正纵坐标。
            y: 50,
        },
        // 使用缺省 deadline。
        timeout_ms: 2_000,
    };
    // 构造最终同代际观察。
    let observation = fixture_observation(
        // 使用普通状态。
        WindowVisualState::Normal,
        // 返回最终外框。
        WindowRectangle {
            // 保存横坐标。
            x: -100,
            // 保存纵坐标。
            y: 50,
            // 保存宽度。
            width: 800,
            // 保存高度。
            height: 600,
        },
        // 使用标准 DPI。
        96,
        // 使用稳定最小尺寸。
        WindowSize {
            // 设置最小宽度。
            width: 120,
            // 设置最小高度。
            height: 80,
        },
        // 使用双屏虚拟桌面。
        WindowRectangle {
            // 设置负左边界。
            x: -1920,
            // 设置上边界。
            y: 0,
            // 设置宽度。
            width: 3840,
            // 设置高度。
            height: 1080,
        },
    );
    // 生成 provider-neutral 成功结果。
    let result = success_result(
        // 使用 canonical 形状目标。
        "s2:w:0000000000000001",
        // 传入动作。
        input,
        // 传入最终观察。
        observation,
        // 传入权限事实。
        "no-static-integrity-block-observed",
        // 标记前景未变化。
        false,
    );
    // 核对 accepted 与 final 区分。
    assert_eq!(result["accepted"], true);
    // 核对最终状态已读回。
    assert_eq!(result["finalStateReached"], true);
    // 核对负坐标保持不变。
    assert_eq!(result["bounds"]["x"], -100);
    // 序列化完整结果检查 native 字段。
    let serialized = result.to_string();
    // 禁止 HWND。
    assert!(!serialized.to_ascii_lowercase().contains("hwnd"));
    // 禁止 PID 字段。
    assert!(!serialized.contains("processId"));
    // 禁止样式字段。
    assert!(!serialized.to_ascii_lowercase().contains("style"));
}
