//! 保存 Keyboard Input Module 不产生真实输入的纯回归测试。

// 导入父 Module 私有纯原语。
use super::*;

// 验证空执行状态不伪造已接受输入。
#[test]
fn execution_state_starts_without_side_effects() {
    // 构造初始状态。
    let state = ExecutionState::new();
    // 初始没有按键所有权。
    assert!(state.held_keys.is_empty());
    // 初始没有平台调用。
    assert!(!state.dispatch_attempted);
    // 初始没有键盘影响。
    assert!(!state.keyboard_effect_seen);
    // 初始没有前景影响。
    assert!(!state.foreground_effect_started);
}

// 验证无残留状态生成成功释放报告。
#[test]
fn empty_release_report_is_safe() {
    // 构造空执行状态。
    let mut state = ExecutionState::new();
    // 执行无需平台调用的释放。
    let report = release_held_keys(&mut state);
    // 不应伪造释放尝试。
    assert!(!report.attempted);
    // 空状态天然满足零残留。
    assert!(report.succeeded);
    // 不应公开任何残留键。
    assert!(report.remaining.is_empty());
}
