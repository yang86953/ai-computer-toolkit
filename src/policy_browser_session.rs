//! 保存 Browser Session 固定 Broker 的 Policy 路由事实。

// 导入稳定 capability、错误与隔离类型。
use crate::{
    // 使用 Browser Session capability 单一注册表。
    capabilities,
    // 返回统一公开错误并读取强类型隔离要求。
    domain::{AppControlError, AppResult, IsolationRequirement},
};

// 固定唯一已认证的 Rust Browser Session Broker sibling。
pub(super) const BROKER_FILE_NAME: &str = "ai-computer-toolkit-browser-session-broker.exe";

// 判断公开 capability 是否由固定 Browser Session Broker 承接。
pub(super) fn uses_fixed_broker(capability: Option<&str>) -> bool {
    // 只允许登记过的会话、页面与元素 capability 使用固定 Broker。
    matches!(
        // 读取调用方已经通过 generic App 解析的 ID。
        capability,
        // 保持八项公开 capability 的封闭集合。
        Some(
            capabilities::BROWSER_SESSION_OPEN
                | capabilities::BROWSER_SESSION_CLOSE
                | capabilities::BROWSER_PAGE_NAVIGATE
                | capabilities::BROWSER_PAGE_WAIT
                | capabilities::BROWSER_PAGE_QUERY
                | capabilities::BROWSER_ELEMENT_CLICK
                | capabilities::BROWSER_ELEMENT_TYPE
                | capabilities::BROWSER_PAGE_SCREENSHOT
        )
    )
}

// 判断公开 capability 是否必须在 Broker 探测前完成确认。
pub(super) fn requires_confirmation_first(capability: Option<&str>) -> bool {
    // 生命周期与页面 mutation Command 都不得让 companion 缺口抢先暴露。
    matches!(
        // 读取调用方已经通过 generic App 解析的 ID。
        capability,
        // 只包含会改变会话或页面状态的五项 Command。
        Some(
            capabilities::BROWSER_SESSION_OPEN
                | capabilities::BROWSER_SESSION_CLOSE
                | capabilities::BROWSER_PAGE_NAVIGATE
                | capabilities::BROWSER_ELEMENT_CLICK
                | capabilities::BROWSER_ELEMENT_TYPE
        )
    )
}

// 判断公开 capability 是否只允许严格零打扰请求。
pub(super) fn requires_strict_isolation(capability: Option<&str>) -> bool {
    // 页面 mutation 成功契约只允许 strict 与 strict-no-interference。
    matches!(
        // 读取已登记 Browser Session capability。
        capability,
        // 导航、点击与输入都禁止 standard 路线。
        Some(
            capabilities::BROWSER_PAGE_NAVIGATE
                | capabilities::BROWSER_ELEMENT_CLICK
                | capabilities::BROWSER_ELEMENT_TYPE
        )
    )
}

// 在任何 target、input 或 Broker 访问前冻结页面导航隔离要求。
pub(super) fn validate_isolation(
    // 接收已经通过 generic App 解析的 capability。
    capability: Option<&str>,
    // 接收调用方强类型隔离要求。
    requirement: IsolationRequirement,
) -> AppResult<()> {
    // standard 页面 mutation 不能形成与冻结结果契约冲突的伪成功。
    if requires_strict_isolation(capability) && requirement != IsolationRequirement::Strict {
        // 以公开参数错误要求调用方显式选择严格零打扰。
        return Err(AppControlError::new(
            // 使用页面错误矩阵允许的公开分类。
            "INVALID_ARGUMENT",
            // 不公开 sibling、provider 或执行环境事实。
            "浏览器页面变更必须使用 strict isolation。",
        ));
    }
    // 其他 Browser Session capability 保持调用方冻结的策略。
    Ok(())
}

// 验证固定 Policy 事实不误接其他浏览器能力。
#[cfg(test)]
mod tests {
    // 导入稳定 capability ID。
    use crate::capabilities;

    // 导入被测封闭判定与固定 sibling。
    use super::{
        // 导入固定 Broker 文件名。
        BROKER_FILE_NAME,
        // 导入确认优先判定。
        requires_confirmation_first,
        // 导入严格隔离判定。
        requires_strict_isolation,
        // 导入固定 Broker 判定。
        uses_fixed_broker,
    };

    // 验证 browser_session_public_route 只认证八项固定 Broker capability。
    #[test]
    fn browser_session_public_route_policy_facts_are_closed() {
        // open 必须使用固定生命周期路线。
        assert!(uses_fixed_broker(Some(capabilities::BROWSER_SESSION_OPEN)));
        // close 必须使用同一路线。
        assert!(uses_fixed_broker(Some(capabilities::BROWSER_SESSION_CLOSE)));
        // navigate 必须使用同一个固定 Broker。
        assert!(uses_fixed_broker(Some(capabilities::BROWSER_PAGE_NAVIGATE)));
        // wait 必须使用同一个固定 Broker。
        assert!(uses_fixed_broker(Some(capabilities::BROWSER_PAGE_WAIT)));
        // query 必须使用同一个固定 Broker。
        assert!(uses_fixed_broker(Some(capabilities::BROWSER_PAGE_QUERY)));
        // click 必须使用同一个固定 Broker。
        assert!(uses_fixed_broker(Some(capabilities::BROWSER_ELEMENT_CLICK)));
        // type 必须使用同一个固定 Broker。
        assert!(uses_fixed_broker(Some(capabilities::BROWSER_ELEMENT_TYPE)));
        // 页面截图必须使用同一个固定 Broker。
        assert!(uses_fixed_broker(Some(
            capabilities::BROWSER_PAGE_SCREENSHOT
        )));
        // legacy 截图不得误用长期 Broker。
        assert!(!uses_fixed_broker(Some(capabilities::BROWSER_SCREENSHOT)));
        // 五项 Command 都必须保持 confirmation-first。
        for capability in [
            // 打开会话会创建外部资源。
            capabilities::BROWSER_SESSION_OPEN,
            // 关闭会话会回收外部资源。
            capabilities::BROWSER_SESSION_CLOSE,
            // 导航会改变当前页面代际。
            capabilities::BROWSER_PAGE_NAVIGATE,
            // 点击会改变当前页面状态。
            capabilities::BROWSER_ELEMENT_CLICK,
            // 输入会改变当前页面状态。
            capabilities::BROWSER_ELEMENT_TYPE,
        ] {
            // mutation 必须先确认再探测 Broker。
            assert!(requires_confirmation_first(Some(capability)));
        }
        // 三项 Query 不得错误要求确认。
        for capability in [
            // 页面等待只观察状态。
            capabilities::BROWSER_PAGE_WAIT,
            // 页面查询只返回有界语义结果。
            capabilities::BROWSER_PAGE_QUERY,
            // 页面截图只返回有界 PNG。
            capabilities::BROWSER_PAGE_SCREENSHOT,
        ] {
            // Query 不属于 confirmation-first Command。
            assert!(!requires_confirmation_first(Some(capability)));
        }
        // 页面导航必须使用严格零打扰请求。
        assert!(requires_strict_isolation(Some(
            // 传递唯一严格页面 capability。
            capabilities::BROWSER_PAGE_NAVIGATE,
        )));
        // 元素点击同样必须使用严格零打扰。
        assert!(requires_strict_isolation(Some(
            // 传递确认式点击 capability。
            capabilities::BROWSER_ELEMENT_CLICK,
        )));
        // 元素输入同样必须使用严格零打扰。
        assert!(requires_strict_isolation(Some(
            // 传递确认式输入 capability。
            capabilities::BROWSER_ELEMENT_TYPE,
        )));
        // 页面 Query 保持 standard 可用。
        assert!(!requires_strict_isolation(Some(
            // 传递页面等待 capability。
            capabilities::BROWSER_PAGE_WAIT,
        )));
        // 页面截图保持 standard 可用。
        assert!(!requires_strict_isolation(Some(
            // 传递页面截图 capability。
            capabilities::BROWSER_PAGE_SCREENSHOT,
        )));
        // sibling 文件名必须保持固定。
        assert_eq!(
            // 读取唯一固定文件名。
            BROKER_FILE_NAME,
            // 禁止改成 browser worker 或调用方输入。
            "ai-computer-toolkit-browser-session-broker.exe",
        );
    }
}
