//! 通过固定 Rust Browser Session Broker 投影公开页面导航、等待与查询结果。

// 导入 JSON 构造器与值类型。
use serde_json::{Value, json};

// 导入固定 Broker Adapter 与严格公开输入 Component。
use crate::{
    // 调用唯一认证 Browser Session Broker 路线。
    adapters::browser_session_broker_windows,
    // 导入稳定 capability ID。
    capabilities,
    // 解析 provider-neutral 页面输入。
    components::browser_page_input::{parse_navigate, parse_query, parse_wait},
    // 导入统一结果边界。
    domain::AppResult,
};

// 复用同一 client Module 的确认、身份与错误投影。
use super::{
    // 构造 confirmation-first 公开错误。
    confirmation_required,
    // 收敛 Broker 私有错误与接受事实。
    project_broker_error,
    // 为本地输入拒绝附加未派发真值。
    project_pre_dispatch_error,
    // 在 Broker 前拒绝非 canonical Browser Session target。
    require_canonical_browser_session,
};

// 构造 Broker success 在本地投影漂移时的接受后失败。
fn invalid_completed_query(session_id: &str) -> crate::domain::AppControlError {
    // 将理论内部漂移保留为已接受、可信失败且不可重试。
    project_broker_error(
        // 构造不泄漏 query 内容的内部协议错误。
        crate::domain::AppControlError::with_details(
            // 使用会收敛为公开操作失败的私有码。
            "BROKER_PROTOCOL_FAILED",
            // 不公开 query 内容。
            "The browser page query result was invalid.",
            // 明确 Broker 已经返回 completed final。
            json!({
                // 业务已经接受。
                "businessAccepted": true,
                // 已取得可信 final。
                "completed": true,
                // 不得自动重试。
                "retrySafe": false,
                // Query 不会改变目标。
                "targetMayHaveMutated": false
            }),
        ),
        // 绑定查询 capability。
        capabilities::BROWSER_PAGE_QUERY,
        // 绑定调用方公开 session。
        session_id,
    )
}

// 导航当前 live Browser Session 并投影新页面 identity 与代际。
pub(crate) fn navigate(
    // 接收 facade 已传递的逐操作确认事实。
    confirmed: bool,
    // 接收严格公开 input。
    input: Option<&Value>,
    // 接收调用方提供的公开 Browser Session identity。
    session_id: &str,
) -> AppResult<Value> {
    // confirmation 必须先于 target、input 与 Broker 访问。
    if !confirmed {
        // 未确认请求只返回业务前公共事实。
        return Err(confirmation_required(
            // 绑定页面导航 capability。
            capabilities::BROWSER_PAGE_NAVIGATE,
            // 仅在 canonical 时由共享投影回显 target。
            session_id,
        ));
    }
    // 非 canonical Browser Session 在 Broker 前失败闭合。
    require_canonical_browser_session(session_id).map_err(|error| {
        // 补齐未派发事实且不泄漏 identity 布局。
        project_pre_dispatch_error(error, capabilities::BROWSER_PAGE_NAVIGATE, session_id)
    })?;
    // 严格解析 URL 与单一总 deadline。
    let input = parse_navigate(input).map_err(|error| {
        // 输入错误发生在任何 Broker I/O 前。
        project_pre_dispatch_error(error, capabilities::BROWSER_PAGE_NAVIGATE, session_id)
    })?;
    // 只经固定认证 Broker 执行 confirmed navigate。
    let navigation = browser_session_broker_windows::navigate_confirmed(
        // 绑定当前 live session。
        session_id,
        // 传递不公开回显的已验证 URL。
        input.url(),
        // 传递覆盖完整交换的总预算。
        input.timeout_ms(),
    )
    // 收敛私有 Broker 错误与 D3/R0 接受事实。
    .map_err(|error| {
        project_broker_error(error, capabilities::BROWSER_PAGE_NAVIGATE, session_id)
    })?;
    // 只在可信 completed final 后输出冻结成功 data。
    Ok(json!({
        // 回显固定 capability。
        "capability": capabilities::BROWSER_PAGE_NAVIGATE,
        // 输出固定领域动作。
        "action": "navigate",
        // 输出可信完成 outcome。
        "outcome": "completed",
        // 固定 dispatch 终态。
        "dispatchState": "completed",
        // 成功必定已越过业务接受点。
        "accepted": true,
        // 成功必定已有可信 final。
        "finalStateReached": true,
        // 页面导航会改变页面代际。
        "targetMayHaveMutated": true,
        // 固定导航完成事实。
        "navigated": true,
        // 只输出 Module 签发的公开页面 identity。
        "pageId": navigation.page_id(),
        // 输出 Module 报告的正导航代际。
        "navigationGeneration": navigation.generation(),
        // 固定 Broker 路线不改变主机前景。
        "foregroundUnchanged": true,
        // confirmation-first 已在 dispatch 前完成。
        "confirmationEvaluatedBeforeDispatch": true,
        // 无前台路线也已完成策略评估。
        "foregroundConsentEvaluatedBeforeDispatch": true,
        // accepted mutation 不可安全重试。
        "retrySafe": false,
        // 禁止 facade 自动重派。
        "automaticRetryProhibited": true,
    }))
}

// 等待当前页面满足有限条件并投影页面与代际事实。
pub(crate) fn wait(
    // 接收严格公开 input。
    input: Option<&Value>,
    // 接收调用方提供的公开 Browser Session identity。
    session_id: &str,
) -> AppResult<Value> {
    // Query 仍必须在 Broker 前认证顶层 session 外壳。
    require_canonical_browser_session(session_id).map_err(|error| {
        // 补齐未派发事实且不泄漏 identity 布局。
        project_pre_dispatch_error(error, capabilities::BROWSER_PAGE_WAIT, session_id)
    })?;
    // 严格解析当前 page、有限条件与单一总 deadline。
    let input = parse_wait(input).map_err(|error| {
        // 输入错误发生在任何 Broker I/O 前。
        project_pre_dispatch_error(error, capabilities::BROWSER_PAGE_WAIT, session_id)
    })?;
    // 只经固定认证 Broker 执行无副作用 wait Query。
    let observation = browser_session_broker_windows::wait(
        // 绑定当前 live session。
        session_id,
        // 绑定调用方当前公开 page。
        input.page_id(),
        // 传递有限 provider-neutral 条件。
        input.condition(),
        // 传递覆盖完整交换的总预算。
        input.timeout_ms(),
    )
    // 收敛私有 Broker 错误且保持 targetMayHaveMutated=false。
    .map_err(|error| project_broker_error(error, capabilities::BROWSER_PAGE_WAIT, session_id))?;
    // 只在可信 completed final 后输出冻结成功 data。
    Ok(json!({
        // 回显固定 capability。
        "capability": capabilities::BROWSER_PAGE_WAIT,
        // 输出固定领域动作。
        "action": "wait",
        // 输出可信完成 outcome。
        "outcome": "completed",
        // 固定 dispatch 终态。
        "dispatchState": "completed",
        // 成功必定已越过业务接受点。
        "accepted": true,
        // 成功必定已有可信 final。
        "finalStateReached": true,
        // Query 不改变目标。
        "targetMayHaveMutated": false,
        // 固定只读事实。
        "readOnly": true,
        // 回显 request-bound 当前公开页面 identity。
        "pageId": observation.page_id(),
        // 回显 Module 报告的正导航代际。
        "navigationGeneration": observation.generation(),
        // Broker 只在条件满足时形成 completed。
        "conditionMet": true,
        // 固定 Broker 路线不改变主机前景。
        "foregroundUnchanged": true,
        // Query 不要求确认但策略已经先行评估。
        "confirmationEvaluatedBeforeDispatch": true,
        // 无前台路线也已完成策略评估。
        "foregroundConsentEvaluatedBeforeDispatch": true,
        // accepted Query 只允许同 identity 恢复，不自动新意图。
        "retrySafe": false,
        // 禁止 facade 自动重派。
        "automaticRetryProhibited": true,
    }))
}

// 查询当前页面并投影有界 provider-neutral 元素摘要。
pub(crate) fn query(
    // 接收严格公开 input。
    input: Option<&Value>,
    // 接收调用方提供的公开 Browser Session identity。
    session_id: &str,
) -> AppResult<Value> {
    // Query 仍必须在 Broker 前认证顶层 session 外壳。
    require_canonical_browser_session(session_id).map_err(|error| {
        // 补齐未派发事实且不泄漏 identity 布局。
        project_pre_dispatch_error(error, capabilities::BROWSER_PAGE_QUERY, session_id)
    })?;
    // 严格解析当前 page、selector、结果上限与单一总 deadline。
    let input = parse_query(input).map_err(|error| {
        // 输入错误发生在任何 Broker I/O 前。
        project_pre_dispatch_error(error, capabilities::BROWSER_PAGE_QUERY, session_id)
    })?;
    // 只经固定认证 Broker 执行无副作用 query Query。
    let query = browser_session_broker_windows::query(
        // 绑定当前 live session。
        session_id,
        // 绑定调用方当前公开 page。
        input.page_id(),
        // 传递 provider-neutral selector。
        input.selector(),
        // 传递有界结果上限。
        input.max_results(),
        // 传递覆盖完整交换的总预算。
        input.timeout_ms(),
    )
    // 收敛私有 Broker 错误且保持 targetMayHaveMutated=false。
    .map_err(|error| project_broker_error(error, capabilities::BROWSER_PAGE_QUERY, session_id))?;
    // strict Adapter 已验证 query 为精确对象。
    let query = query
        // 只接受 JSON object。
        .as_object()
        // 理论漂移按接受后失败闭合。
        .ok_or_else(|| invalid_completed_query(session_id))?;
    // 逐项取得 strict Adapter 已验证的当前页面 identity。
    let page_id = query
        // 读取固定字段。
        .get("pageId")
        // 复制公开 JSON 值。
        .cloned()
        // 缺失字段不得形成含 null 的伪成功。
        .ok_or_else(|| invalid_completed_query(session_id))?;
    // 取得正导航代际。
    let navigation_generation = query
        // 读取固定字段。
        .get("navigationGeneration")
        // 复制公开 JSON 值。
        .cloned()
        // 缺失字段失败闭合。
        .ok_or_else(|| invalid_completed_query(session_id))?;
    // 取得有界命中数组。
    let matches = query
        // 读取固定字段。
        .get("matches")
        // 复制公开 JSON 值。
        .cloned()
        // 缺失字段失败闭合。
        .ok_or_else(|| invalid_completed_query(session_id))?;
    // 取得总命中数。
    let match_count = query
        // 读取固定字段。
        .get("matchCount")
        // 复制公开 JSON 值。
        .cloned()
        // 缺失字段失败闭合。
        .ok_or_else(|| invalid_completed_query(session_id))?;
    // 取得截断事实。
    let truncated = query
        // 读取固定字段。
        .get("truncated")
        // 复制公开 JSON 值。
        .cloned()
        // 缺失字段失败闭合。
        .ok_or_else(|| invalid_completed_query(session_id))?;
    // 只在可信 completed final 后输出冻结成功 data。
    Ok(json!({
        // 回显固定 capability。
        "capability": capabilities::BROWSER_PAGE_QUERY,
        // 输出固定领域动作。
        "action": "query",
        // 输出可信完成 outcome。
        "outcome": "completed",
        // 固定 dispatch 终态。
        "dispatchState": "completed",
        // 成功必定已越过业务接受点。
        "accepted": true,
        // 成功必定已有可信 final。
        "finalStateReached": true,
        // Query 不改变目标。
        "targetMayHaveMutated": false,
        // 固定只读事实。
        "readOnly": true,
        // 回显 request-bound 当前公开页面 identity。
        "pageId": page_id,
        // 回显 Module 报告的正导航代际。
        "navigationGeneration": navigation_generation,
        // 输出有界公开元素摘要。
        "matches": matches,
        // 输出 worker 报告的总命中数。
        "matchCount": match_count,
        // 输出是否因 maxResults 截断。
        "truncated": truncated,
        // 固定 Broker 路线不改变主机前景。
        "foregroundUnchanged": true,
        // Query 不要求确认但策略已经先行评估。
        "confirmationEvaluatedBeforeDispatch": true,
        // 无前台路线也已完成策略评估。
        "foregroundConsentEvaluatedBeforeDispatch": true,
        // accepted Query 只允许同 identity 恢复，不自动新意图。
        "retrySafe": false,
        // 禁止 facade 自动重派。
        "automaticRetryProhibited": true,
    }))
}
