//! 定义 opaque 窗口目标的身份材料与 provider-neutral 保证强度。

// 导入公开 JSON 构造接口。
use serde_json::{Value, json};

// 固定窗口目标身份强度契约版本。
pub(crate) const CONTRACT_VERSION: &str = "act/window-target-identity/v1";

// 从当前私有窗口事实构造 opaque hash 的稳定材料。
pub(crate) fn material(
    // 接收仅在 Windows Adapter 内存在的进程 ID。
    process_id: u32,
    // 接收仅在 Windows Adapter 内存在的完整当前窗口 token。
    current_window_token: usize,
    // 接收仅在 Windows Adapter 内存在的进程创建代际。
    process_generation: u64,
) -> String {
    // 保持既有跨实现三段十进制字节布局。
    format!("{process_id}:{current_window_token}:{process_generation}")
}

// 投影不含任何原生值的公开身份保证强度。
pub(crate) fn public_assurance() -> Value {
    // 返回严格版本化且保守的能力事实。
    json!({
        // 绑定独立身份强度契约。
        "contractVersion": CONTRACT_VERSION,
        // 说明 opaque hash 使用的 provider-neutral 事实类别。
        "assurance": "process-generation-plus-current-window-token",
        // 进程代际可读时能够区分 PID 回收。
        "processReuse": "detected-when-process-generation-available",
        // 同时存活的窗口具有不同当前 token。
        "simultaneouslyLiveWindows": "distinct-current-window-token",
        // Windows 回收完全相同 token 时当前无窗口级代际证明。
        "sameProcessRecycledWindowToken": "not-guaranteed",
        // 当前短命 launcher 没有持久窗口代际所有者。
        "generationOwner": "none",
        // 任意窗口缺少与生命周期绑定的原子 dispatch，异步 owner 也不能关闭最终 TOCTOU。
        "mutationStopline": "lifetime-bound-atomic-dispatch-unavailable-for-arbitrary-windows"
    })
}

// 声明身份材料与公开保证强度的纯回归测试。
#[cfg(test)]
mod tests {
    // 导入被测纯函数与契约版本。
    use super::{CONTRACT_VERSION, material, public_assurance};

    // 验证进程代际或当前窗口 token 变化会改变身份材料。
    #[test]
    fn material_distinguishes_available_process_and_current_window_facts() {
        // 构造固定当前窗口身份材料。
        let current = material(42, 4660, 123);
        // 不同当前窗口 token 必须改变材料。
        assert_ne!(current, material(42, 4661, 123));
        // 不同进程代际必须改变材料。
        assert_ne!(current, material(42, 4660, 124));
        // 不同 PID 必须改变材料。
        assert_ne!(current, material(43, 4660, 123));
    }

    // 验证同进程完全相同 token 回收无法由当前三字段区分。
    #[test]
    fn recycled_same_process_window_token_has_no_independent_generation_fact() {
        // 表示旧逻辑窗口的三字段材料。
        let previous = material(42, 4660, 123);
        // 表示同一进程内完全相同 token 被新逻辑窗口回收。
        let replacement = material(42, 4660, 123);
        // 当前算法必须诚实暴露两者不可区分。
        assert_eq!(previous, replacement);
        // 公开保证不得声称已解决该回收边界。
        assert_eq!(
            public_assurance()["sameProcessRecycledWindowToken"],
            // 保持保守结论。
            "not-guaranteed"
        );
    }

    // 验证公开保证对象封闭且不包含原生值。
    #[test]
    fn public_assurance_is_versioned_and_provider_neutral() {
        // 取得固定公开保证。
        let assurance = public_assurance();
        // 核对版本。
        assert_eq!(assurance["contractVersion"], CONTRACT_VERSION);
        // mutation 停止线必须明确平台缺少生命周期绑定的原子 dispatch。
        assert_eq!(
            assurance["mutationStopline"],
            // 不把持久异步事件 owner 误报为充分条件。
            "lifetime-bound-atomic-dispatch-unavailable-for-arbitrary-windows"
        );
        // 序列化供隐私扫描。
        let text = assurance.to_string().to_ascii_lowercase();
        // 禁止 HWND 类型名称。
        assert!(!text.contains("hwnd"));
        // 禁止 PID 字段名称。
        assert!(!text.contains("processid"));
        // 禁止任何私有数值进入保证对象。
        for value in ["4660", "42", "123"] {
            // 任一命中都表示测试材料泄漏。
            assert!(!text.contains(value));
        }
    }
}
