//! 封闭 canonical stale 目标可以直接交给独占 provider 的路由集合。

// 导入稳定 capability 注册表与 opaque 目标类别。
use crate::{
    // 使用 capability 单一注册表常量。
    capabilities,
    // 只解析版本化 opaque 目标，不恢复原生值。
    components::opaque_id::{OpaqueTargetId, OpaqueTargetKind},
};

// 判断 stale canonical 目标是否拥有唯一领域 Module 错误语义。
pub(super) fn selects_provider_owned_stale_target(
    // 接收已经过 generic verb 校验的 capability。
    capability: &str,
    // 接收调用方原样 opaque session ID。
    session_id: &str,
) -> bool {
    // 解析版本化公开目标类别。
    OpaqueTargetId::parse(session_id)
        // 只接受已审计的 capability 与目标类别配对。
        .is_some_and(|target| match target.kind() {
            // Standard Edit 只有文本写 Module 独占 s2:c stale 语义。
            OpaqueTargetKind::Control => capability == capabilities::UI_TEXT_INPUT,
            // 窗口类 capability 只允许 Desktop provider 独占集合。
            OpaqueTargetKind::Window => matches!(
                // 对比稳定 capability ID。
                capability,
                // 精确窗口关闭。
                capabilities::WINDOW_CLOSE
                    // 精确窗口关闭等待。
                    | capabilities::WINDOW_CLOSED_WAIT
                    // 语义元素动作。
                    | capabilities::UI_ELEMENT_ACTION
                    // 语义元素定位。
                    | capabilities::UI_ELEMENT_LOCATE
                    // 语义元素等待。
                    | capabilities::UI_ELEMENT_WAIT
                    // 精确窗口截图。
                    | capabilities::WINDOW_SCREENSHOT
                    // 精确窗口录制。
                    | capabilities::WINDOW_RECORD
                    // 窗口状态与几何生命周期。
                    | capabilities::WINDOW_LIFECYCLE
                    // 通用键盘输入。
                    | capabilities::UI_INPUT_KEY
                    // 通用指针输入。
                    | capabilities::UI_INPUT_POINTER
            ),
            // 进程终止 Module 独占两种 s2:p stale 语义。
            OpaqueTargetKind::Process => matches!(
                // 对比稳定终止 capability。
                capability,
                // 先尝试优雅终止。
                capabilities::PROCESS_TERMINATE_GRACEFUL
                    // 显式强制终止。
                    | capabilities::PROCESS_TERMINATE_FORCE
            ),
            // Host session 与其他类别没有当前直达 stale 路由。
            _ => false,
        })
}

// 声明纯路由集合回归测试。
#[cfg(test)]
mod tests {
    // 导入被测规则与 capability 常量。
    use super::{capabilities, selects_provider_owned_stale_target};

    // 验证窗口专属 capability 只接受 canonical s2:w。
    #[test]
    fn window_stale_routes_are_closed_by_target_kind() {
        // 固定全部窗口专属 stale capability。
        let capabilities = [
            // 精确窗口关闭。
            capabilities::WINDOW_CLOSE,
            // 精确窗口关闭等待。
            capabilities::WINDOW_CLOSED_WAIT,
            // 语义元素动作。
            capabilities::UI_ELEMENT_ACTION,
            // 语义元素定位。
            capabilities::UI_ELEMENT_LOCATE,
            // 语义元素等待。
            capabilities::UI_ELEMENT_WAIT,
            // 精确窗口截图。
            capabilities::WINDOW_SCREENSHOT,
            // 精确窗口录制。
            capabilities::WINDOW_RECORD,
            // 窗口生命周期。
            capabilities::WINDOW_LIFECYCLE,
            // 键盘输入。
            capabilities::UI_INPUT_KEY,
            // 指针输入。
            capabilities::UI_INPUT_POINTER,
        ];
        // 逐项核对 canonical 窗口类别。
        for capability in capabilities {
            // s2:w 必须进入领域 Module。
            assert!(selects_provider_owned_stale_target(
                capability,
                // 使用合法形状的固定 opaque 窗口 ID。
                "s2:w:0000000000000000"
            ));
            // 同一 capability 不得误选控件 provider。
            assert!(!selects_provider_owned_stale_target(
                capability,
                // 使用合法形状的固定 opaque 控件 ID。
                "s2:c:0000000000000000"
            ));
        }
    }

    // 验证未审计 capability 与畸形目标保持普通查找失败。
    #[test]
    fn unsupported_or_malformed_stale_routes_are_rejected() {
        // 应用发现不属于精确窗口执行 Module。
        assert!(!selects_provider_owned_stale_target(
            capabilities::APPLICATION_DISCOVER,
            // 使用 canonical 窗口类别。
            "s2:w:0000000000000000"
        ));
        // 畸形 identity 不得通过类别路由。
        assert!(!selects_provider_owned_stale_target(
            capabilities::WINDOW_SCREENSHOT,
            // 缺失十六进制指纹。
            "s2:w:not-canonical"
        ));
    }
}
