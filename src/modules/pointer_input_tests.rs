//! 保存 Pointer Input Module 不产生真实输入的纯回归测试。

// 导入父 Module 私有纯原语。
use super::*;

// 验证拖拽插值覆盖负坐标并精确落到终点。
#[test]
fn drag_interpolation_is_deterministic_and_reaches_endpoint() {
    // 构造跨越坐标原点的起点。
    let start = PointerPoint { x: -100, y: 50 };
    // 构造终点。
    let end = PointerPoint { x: 100, y: -50 };
    // 核对中点。
    assert_eq!(
        interpolate_pointer_point(start, end, 2, 4),
        PointerPoint { x: 0, y: 0 }
    );
    // 核对最后采样精确等于终点。
    assert_eq!(interpolate_pointer_point(start, end, 4, 4), end);
}

// 验证空执行状态不伪造已接受输入。
#[test]
fn execution_state_starts_without_side_effects() {
    // 构造初始状态。
    let state = ExecutionState::new();
    // 初始没有按钮所有权。
    assert!(state.held_buttons.is_empty());
    // 初始没有指针影响。
    assert!(!state.pointer_effect_seen);
    // 初始没有前景影响。
    assert!(!state.foreground_effect_started);
}
