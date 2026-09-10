//! 提供 App facade 直接选择 Browser Session provider 的纯路由判定。

// 判断调用是否必须直接选择 provider 而不另做 freshness Query。
pub(super) fn selects_direct_browser_session_provider(capability: &str, _: &str) -> bool {
    // 四项精确 session 操作都由 Broker 业务前预检权威判 stale。
    matches!(
        // 读取已经通过 App registry 解析的 capability。
        capability,
        // 只接受 Browser Session provider 拥有的固定集合。
        crate::capabilities::BROWSER_SESSION_CLOSE
            | crate::capabilities::BROWSER_PAGE_NAVIGATE
            | crate::capabilities::BROWSER_PAGE_WAIT
            | crate::capabilities::BROWSER_PAGE_QUERY
            | crate::capabilities::BROWSER_ELEMENT_CLICK
            | crate::capabilities::BROWSER_ELEMENT_TYPE
            | crate::capabilities::BROWSER_PAGE_SCREENSHOT
    )
}

// 返回 Browser Session capability 的公开精确目标描述。
pub(super) fn targeting_label(capability: &str) -> &'static str {
    // 页面契约明确区分 Browser Session 与通用 App session。
    if matches!(
        // 读取已解析的稳定 capability。
        capability,
        // 三项页面 operation 共用同一公开描述。
        crate::capabilities::BROWSER_PAGE_NAVIGATE
            | crate::capabilities::BROWSER_PAGE_WAIT
            | crate::capabilities::BROWSER_PAGE_QUERY
            | crate::capabilities::BROWSER_ELEMENT_CLICK
            | crate::capabilities::BROWSER_ELEMENT_TYPE
            | crate::capabilities::BROWSER_PAGE_SCREENSHOT
    ) {
        // 回显冻结页面结果 schema 的逐字值。
        return "opaque exact browser session";
    }
    // 生命周期与其他 App capability 保持既有通用描述。
    "opaque exact session"
}

// 验证 browser session close provider 选择不触发 freshness Query。
#[cfg(test)]
mod tests {
    // 导入稳定 capability。
    use crate::capabilities;

    // 导入纯路由判定器。
    use super::{
        // 导入直接 provider 选择判定。
        selects_direct_browser_session_provider,
        // 导入公开目标描述判定。
        targeting_label,
    };

    // 验证 browser_session_public_route close 仅按 capability 与 identity 选择 provider。
    #[test]
    fn browser_session_public_route_close_selection_never_queries_freshness() {
        // canonical browser session 必须直接选择 close provider。
        assert!(selects_direct_browser_session_provider(
            // 传递固定 close capability。
            capabilities::BROWSER_SESSION_CLOSE,
            // 传递合法 browser session identity。
            "s2:bs:0123456789abcdef0123456789abcdef",
        ));
        // open capability 不得误走 close 选择。
        assert!(!selects_direct_browser_session_provider(
            // 使用固定 open capability。
            capabilities::BROWSER_SESSION_OPEN,
            // 即使 identity 形状合法也必须拒绝。
            "s2:bs:0123456789abcdef0123456789abcdef",
        ));
        // wrong-kind target 也必须由专属 provider 给出公开输入错误。
        assert!(selects_direct_browser_session_provider(
            // 使用固定 close capability。
            capabilities::BROWSER_SESSION_CLOSE,
            // 使用普通窗口 opaque identity。
            "s2:w:0123456789abcdef",
        ));
        // 同前缀畸形目标同样不得退化成通用 target-not-found。
        assert!(selects_direct_browser_session_provider(
            // 使用固定 close capability。
            capabilities::BROWSER_SESSION_CLOSE,
            // 使用非法大写后缀锁定公开 INVALID_ARGUMENT 语义。
            "s2:bs:ABCDEF",
        ));
        // 三项页面 operation 都不得先发独立 freshness Query。
        for capability in [
            // 页面导航由 Broker preflight 判 stale。
            capabilities::BROWSER_PAGE_NAVIGATE,
            // 页面等待由 Broker preflight 判 stale page。
            capabilities::BROWSER_PAGE_WAIT,
            // 页面查询由 Broker preflight 判 stale page。
            capabilities::BROWSER_PAGE_QUERY,
            // 元素点击由 Broker preflight 判 stale page/element。
            capabilities::BROWSER_ELEMENT_CLICK,
            // 元素文本输入由 Broker preflight 判 stale page/element。
            capabilities::BROWSER_ELEMENT_TYPE,
            // 页面截图由 Broker preflight 判 stale page。
            capabilities::BROWSER_PAGE_SCREENSHOT,
        ] {
            // 直接 provider 选择不读取 target 或连接 Broker。
            assert!(selects_direct_browser_session_provider(
                // 传递当前页面 capability。
                capability,
                // 目标形状留给 client Module 在确认后验证。
                "s2:bs:0123456789abcdef0123456789abcdef",
            ));
        }
        // 页面结果必须使用专属 Browser Session 描述。
        assert_eq!(
            // 读取页面导航目标描述。
            targeting_label(capabilities::BROWSER_PAGE_NAVIGATE),
            // 对齐冻结公开结果 schema。
            "opaque exact browser session",
        );
        // 生命周期结果必须保持既有通用描述。
        assert_eq!(
            // 读取 close 目标描述。
            targeting_label(capabilities::BROWSER_SESSION_CLOSE),
            // 对齐生命周期结果 schema。
            "opaque exact session",
        );
    }
}
