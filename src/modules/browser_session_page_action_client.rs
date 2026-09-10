//! 通过固定 Rust Browser Session Broker 投影公开元素动作与页面截图结果。

// 导入 JSON 构造器与值类型。
use serde_json::{Value, json};

// 导入固定 Broker Adapter、严格输入 Component 与稳定 capability。
use crate::{
    // 调用唯一认证 Browser Session Broker 路线。
    adapters::browser_session_broker_windows,
    // 导入稳定 capability ID。
    capabilities,
    // 解析 provider-neutral 页面动作输入。
    components::browser_page_input::{parse_click, parse_screenshot, parse_type},
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

// 点击当前页面元素并投影 request-bound 成功事实。
pub(crate) fn click(
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
            // 绑定元素点击 capability。
            capabilities::BROWSER_ELEMENT_CLICK,
            // 仅在 canonical 时由共享投影回显 target。
            session_id,
        ));
    }
    // 非 canonical Browser Session 在 Broker 前失败闭合。
    require_canonical_browser_session(session_id).map_err(|error| {
        // 补齐未派发事实且不泄漏 identity 布局。
        project_pre_dispatch_error(error, capabilities::BROWSER_ELEMENT_CLICK, session_id)
    })?;
    // 严格解析当前 page、element 与单一总 deadline。
    let input = parse_click(input).map_err(|error| {
        // 输入错误发生在任何 Broker I/O 前。
        project_pre_dispatch_error(error, capabilities::BROWSER_ELEMENT_CLICK, session_id)
    })?;
    // 只经固定认证 Broker 执行 confirmed click。
    let action = browser_session_broker_windows::click_confirmed(
        // 绑定当前 live session。
        session_id,
        // 绑定调用方当前公开 page。
        input.page_id(),
        // 绑定调用方当前公开 element。
        input.element_id(),
        // 传递覆盖完整交换的总预算。
        input.timeout_ms(),
    )
    // 收敛私有 Broker 错误与接受事实。
    .map_err(|error| {
        project_broker_error(error, capabilities::BROWSER_ELEMENT_CLICK, session_id)
    })?;
    // 只在可信 completed final 后输出冻结成功 data。
    Ok(json!({
        // 回显固定 capability。
        "capability": capabilities::BROWSER_ELEMENT_CLICK,
        // 输出固定领域动作。
        "action": "click",
        // 输出可信完成 outcome。
        "outcome": "completed",
        // 固定 dispatch 终态。
        "dispatchState": "completed",
        // 成功必定已越过业务接受点。
        "accepted": true,
        // 成功必定已有可信 final。
        "finalStateReached": true,
        // 点击属于 mutation。
        "targetMayHaveMutated": true,
        // 固定点击完成事实。
        "clicked": true,
        // 回显 request-bound 当前页面 identity。
        "pageId": action.page_id(),
        // 回显 request-bound 当前元素 identity。
        "elementId": action.element_id(),
        // 回显 Module 报告的正导航代际。
        "navigationGeneration": action.generation(),
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

// 向当前页面元素输入文本并投影不含原文的成功事实。
pub(crate) fn type_text(
    // 接收 facade 已传递的逐操作确认事实。
    confirmed: bool,
    // 接收严格公开 input。
    input: Option<&Value>,
    // 接收调用方提供的公开 Browser Session identity。
    session_id: &str,
) -> AppResult<Value> {
    // confirmation 必须先于 target、input 与 Broker 访问。
    if !confirmed {
        // 未确认请求不得读取或验证敏感输入文本。
        return Err(confirmation_required(
            // 绑定元素文本输入 capability。
            capabilities::BROWSER_ELEMENT_TYPE,
            // 仅在 canonical 时由共享投影回显 target。
            session_id,
        ));
    }
    // 非 canonical Browser Session 在 Broker 前失败闭合。
    require_canonical_browser_session(session_id).map_err(|error| {
        // 补齐未派发事实且不泄漏 identity 布局。
        project_pre_dispatch_error(error, capabilities::BROWSER_ELEMENT_TYPE, session_id)
    })?;
    // 严格解析当前 page、element、文本、替换语义与总 deadline。
    let input = parse_type(input).map_err(|error| {
        // 输入错误发生在任何 Broker I/O 前且不得回显原文。
        project_pre_dispatch_error(error, capabilities::BROWSER_ELEMENT_TYPE, session_id)
    })?;
    // 只经固定认证 Broker 执行 confirmed type。
    let action = browser_session_broker_windows::type_confirmed(
        // 绑定当前 live session。
        session_id,
        // 绑定调用方当前公开 page。
        input.page_id(),
        // 绑定调用方当前公开 element。
        input.element_id(),
        // 文本只进入当前认证请求。
        input.text(),
        // 保留调用方显式替换语义。
        input.replace(),
        // 传递覆盖完整交换的总预算。
        input.timeout_ms(),
    )
    // 收敛私有 Broker 错误与接受事实。
    .map_err(|error| project_broker_error(error, capabilities::BROWSER_ELEMENT_TYPE, session_id))?;
    // 只在可信 completed final 后输出冻结成功 data。
    Ok(json!({
        // 回显固定 capability。
        "capability": capabilities::BROWSER_ELEMENT_TYPE,
        // 输出固定领域动作。
        "action": "type",
        // 输出可信完成 outcome。
        "outcome": "completed",
        // 固定 dispatch 终态。
        "dispatchState": "completed",
        // 成功必定已越过业务接受点。
        "accepted": true,
        // 成功必定已有可信 final。
        "finalStateReached": true,
        // 文本输入属于 mutation。
        "targetMayHaveMutated": true,
        // 固定输入完成事实。
        "typed": true,
        // 只回显 request-bound 当前页面 identity。
        "pageId": action.action().page_id(),
        // 只回显 request-bound 当前元素 identity。
        "elementId": action.action().element_id(),
        // 回显 Module 报告的正导航代际。
        "navigationGeneration": action.action().generation(),
        // 只公开 UTF-8 字节数，不公开全部或部分原文。
        "utf8Bytes": action.utf8_bytes(),
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

// 捕获当前页面截图并投影有界 request-bound PNG。
pub(crate) fn screenshot(
    // 接收严格公开 input。
    input: Option<&Value>,
    // 接收调用方提供的公开 Browser Session identity。
    session_id: &str,
) -> AppResult<Value> {
    // Query 仍必须在 Broker 前认证顶层 session 外壳。
    require_canonical_browser_session(session_id).map_err(|error| {
        // 补齐未派发事实且不泄漏 identity 布局。
        project_pre_dispatch_error(error, capabilities::BROWSER_PAGE_SCREENSHOT, session_id)
    })?;
    // 严格解析当前 page 与单一总 deadline。
    let input = parse_screenshot(input).map_err(|error| {
        // 输入错误发生在任何 Broker I/O 前。
        project_pre_dispatch_error(error, capabilities::BROWSER_PAGE_SCREENSHOT, session_id)
    })?;
    // 只经固定认证 Broker 捕获当前 page。
    let screenshot = browser_session_broker_windows::screenshot(
        // 绑定当前 live session。
        session_id,
        // 绑定调用方当前公开 page。
        input.page_id(),
        // 传递覆盖完整交换的总预算。
        input.timeout_ms(),
    )
    // 收敛私有 Broker 错误且保持 targetMayHaveMutated=false。
    .map_err(|error| {
        project_broker_error(error, capabilities::BROWSER_PAGE_SCREENSHOT, session_id)
    })?;
    // 只在可信 completed final 后输出冻结成功 data。
    Ok(json!({
        // 回显固定 capability。
        "capability": capabilities::BROWSER_PAGE_SCREENSHOT,
        // 输出固定领域动作。
        "action": "screenshot",
        // 输出可信完成 outcome。
        "outcome": "completed",
        // 固定 dispatch 终态。
        "dispatchState": "completed",
        // 成功必定已越过业务接受点。
        "accepted": true,
        // 成功必定已有可信 final。
        "finalStateReached": true,
        // 截图 Query 不改变目标。
        "targetMayHaveMutated": false,
        // 固定只读事实。
        "readOnly": true,
        // 回显 request-bound 当前页面 identity。
        "pageId": screenshot.page_id(),
        // 回显 Module 报告的正导航代际。
        "navigationGeneration": screenshot.generation(),
        // 只允许固定 PNG MIME。
        "mimeType": screenshot.mime_type(),
        // 输出 strict Adapter 已验证的有界 Base64。
        "pngBase64": screenshot.png_base64(),
        // 输出 strict Adapter 已验证的原始字节数。
        "pngBytes": screenshot.png_bytes(),
        // 输出 strict Adapter 已验证的 IHDR 宽度。
        "width": screenshot.width(),
        // 输出 strict Adapter 已验证的 IHDR 高度。
        "height": screenshot.height(),
        // 输出稳定且不泄漏内容的短摘要。
        "digest": screenshot.digest(),
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

// 验证动作 client 的 confirmation-first、输入隐私与错误投影边界。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造器。
    use serde_json::json;

    // 导入稳定 capability 与统一错误。
    use crate::{capabilities, domain::AppControlError};

    // 导入被测入口与共享错误投影。
    use super::{
        // 导入父 Module 的公共错误收敛。
        super::project_broker_error,
        // 导入动作入口。
        click,
        screenshot,
        type_text,
    };

    // 固定 canonical Browser Session fixture。
    const SESSION_ID: &str = "s2:bs:0123456789abcdef0123456789abcdef";
    // 固定 canonical 页面 fixture。
    const PAGE_ID: &str = "s2:bp:0123456789abcdef0123456789abcdef";
    // 固定 canonical 元素 fixture。
    const ELEMENT_ID: &str = "s2:be:0123456789abcdef0123456789abcdef";

    // 验证 mutation 确认优先于 target、input 与敏感文本解析。
    #[test]
    fn browser_page_action_public_route_confirmation_precedes_private_input() {
        // 未确认 click 即使输入非法也必须首先返回确认错误。
        let click_error = click(false, Some(&json!({"private":"forbidden"})), SESSION_ID)
            // 未确认请求不得成功。
            .expect_err("unconfirmed click must fail");
        // 必须保持 confirmation-first。
        assert_eq!(click_error.code, "CONFIRMATION_REQUIRED");
        // 未确认 type 不得读取或回显文本。
        let type_error = type_text(
            // 保持未确认。
            false,
            // 同时提供敏感且非法的 input。
            Some(&json!({"text":"do-not-echo"})),
            // 使用 canonical session。
            SESSION_ID,
        )
        // 未确认请求不得成功。
        .expect_err("unconfirmed type must fail");
        // 必须保持 confirmation-first。
        assert_eq!(type_error.code, "CONFIRMATION_REQUIRED");
        // 公开错误消息不得回显输入原文。
        assert!(!type_error.message.contains("do-not-echo"));
        // 公开错误详情同样不得回显输入原文。
        assert!(!type_error.details.to_string().contains("do-not-echo"));
    }

    // 验证业务前非法页面动作只形成未派发结果。
    #[test]
    fn browser_page_action_public_route_rejects_invalid_input_before_broker() {
        // 已确认 click 使用畸形元素 identity。
        let click_error = click(
            // 通过确认门禁。
            true,
            // 提供畸形元素外壳。
            Some(&json!({"pageId":PAGE_ID,"elementId":"s2:be:ABCDEF"})),
            // 使用 canonical session。
            SESSION_ID,
        )
        // 输入不得进入 Broker。
        .expect_err("invalid element must fail");
        // 使用冻结输入错误码。
        assert_eq!(click_error.code, "INVALID_ARGUMENT");
        // 必须证明尚未派发。
        assert_eq!(click_error.details["accepted"], false);
        // screenshot 私有格式控制同样在 Broker 前拒绝。
        let screenshot_error = screenshot(
            // 提供当前页面和禁止字段。
            Some(&json!({"pageId":PAGE_ID,"format":"jpeg"})),
            // 使用 canonical session。
            SESSION_ID,
        )
        // 私有扩张不得进入 Broker。
        .expect_err("private screenshot format must fail");
        // 使用冻结输入错误码。
        assert_eq!(screenshot_error.code, "INVALID_ARGUMENT");
        // Query 输入错误不得声称目标变化。
        assert_eq!(screenshot_error.details["targetMayHaveMutated"], false);
        // 避免未使用固定元素 fixture。
        assert!(ELEMENT_ID.starts_with("s2:be:"));
    }

    // 验证 stale、timeout 与 accepted unknown 按 capability 副作用投影。
    #[test]
    fn browser_page_action_public_route_projects_fault_truth_without_private_data() {
        // 构造业务前 stale element final。
        let stale = project_broker_error(
            // 模拟 strict Broker 已过滤的 stale element。
            AppControlError::with_details(
                // 使用冻结 stale element 码。
                "STALE_ELEMENT",
                // 私有消息不得离开 Module。
                "private element mapping",
                // 明确尚未业务接受。
                json!({"businessAccepted":false,"completed":true,"retrySafe":true,"targetMayHaveMutated":false}),
            ),
            // 绑定 click capability。
            capabilities::BROWSER_ELEMENT_CLICK,
            // 绑定调用方已知 session。
            SESSION_ID,
        );
        // stale element 必须保留已登记公共错误码。
        assert_eq!(stale.code, "STALE_ELEMENT");
        // 业务前拒绝必须明确未接受。
        assert_eq!(stale.details["accepted"], false);
        // stale 映射不得回显私有说明。
        assert!(!stale.message.contains("mapping"));
        // 构造 accepted 后 type 无可信 final。
        let unknown_type = project_broker_error(
            // 模拟 transport 丢失后的 strict unknown。
            AppControlError::with_details(
                // 使用冻结未知结果码。
                "OUTCOME_UNKNOWN",
                // 私有 transport 说明不得公开。
                "private transport loss after text",
                // 保留不可猜测的接受与 mutation 事实。
                json!({"businessAccepted":true,"completed":false,"retrySafe":false,"outcomeUnknown":true,"targetMayHaveMutated":true}),
            ),
            // 绑定 type capability。
            capabilities::BROWSER_ELEMENT_TYPE,
            // 绑定调用方已知 session。
            SESSION_ID,
        );
        // accepted 后缺少 final 只能公开 unknown。
        assert_eq!(unknown_type.code, "OUTCOME_UNKNOWN");
        // unknown 必须保持已接受。
        assert_eq!(unknown_type.details["accepted"], true);
        // unknown 不得伪造可信 final。
        assert_eq!(unknown_type.details["finalStateReached"], false);
        // type unknown 必须保守标记可能修改。
        assert_eq!(unknown_type.details["targetMayHaveMutated"], true);
        // 私有文本说明不得回显。
        assert!(!unknown_type.message.contains("text"));
        // 构造 accepted 后 screenshot 无可信 final。
        let unknown_screenshot = project_broker_error(
            // 模拟只读 Query 的 transport 丢失。
            AppControlError::with_details(
                // 使用冻结未知结果码。
                "OUTCOME_UNKNOWN",
                // 使用不应公开的私有说明。
                "private screenshot transport",
                // Query 始终保持 mutation=false。
                json!({"businessAccepted":true,"completed":false,"retrySafe":false,"outcomeUnknown":true,"targetMayHaveMutated":false}),
            ),
            // 绑定 screenshot capability。
            capabilities::BROWSER_PAGE_SCREENSHOT,
            // 绑定调用方已知 session。
            SESSION_ID,
        );
        // screenshot unknown 仍不得声称目标变化。
        assert_eq!(unknown_screenshot.details["targetMayHaveMutated"], false);
        // 构造 accepted 后 click 的可信 cancelled final。
        let cancelled_click = project_broker_error(
            // 模拟 Broker 已取得可信取消终态。
            AppControlError::with_details(
                // 使用冻结取消错误码。
                "CANCELLED",
                // 私有取消阶段说明不得公开。
                "private click cancellation phase",
                // 保留可信 final 与 mutation 真值。
                json!({"businessAccepted":true,"completed":true,"retrySafe":false,"outcomeUnknown":false,"targetMayHaveMutated":true}),
            ),
            // 绑定 click capability。
            capabilities::BROWSER_ELEMENT_CLICK,
            // 绑定调用方已知 session。
            SESSION_ID,
        );
        // 可信取消必须保留公共 CANCELLED 分类。
        assert_eq!(cancelled_click.code, "CANCELLED");
        // 可信取消必须已有 final。
        assert_eq!(cancelled_click.details["finalStateReached"], true);
        // accepted click 取消仍须保守标记可能 mutation。
        assert_eq!(cancelled_click.details["targetMayHaveMutated"], true);
        // 构造 accepted 后 screenshot 的可信 cancelled final。
        let cancelled_screenshot = project_broker_error(
            // 模拟只读 Query 的可信取消终态。
            AppControlError::with_details(
                // 使用冻结取消错误码。
                "CANCELLED",
                // 使用不应公开的私有说明。
                "private screenshot cancellation phase",
                // screenshot 始终保持 mutation=false。
                json!({"businessAccepted":true,"completed":true,"retrySafe":false,"outcomeUnknown":false,"targetMayHaveMutated":false}),
            ),
            // 绑定 screenshot capability。
            capabilities::BROWSER_PAGE_SCREENSHOT,
            // 绑定调用方已知 session。
            SESSION_ID,
        );
        // 可信 screenshot 取消同样保留公共分类。
        assert_eq!(cancelled_screenshot.code, "CANCELLED");
        // screenshot 取消不得声称目标改变。
        assert_eq!(cancelled_screenshot.details["targetMayHaveMutated"], false);
        // 构造业务前私有 deadline。
        let timeout = project_broker_error(
            // 模拟尚未接受的 Broker deadline。
            AppControlError::new("REQUEST_EXPIRED", "private deadline"),
            // 绑定 screenshot capability。
            capabilities::BROWSER_PAGE_SCREENSHOT,
            // 绑定调用方已知 session。
            SESSION_ID,
        );
        // 私有 deadline 只公开统一 timeout。
        assert_eq!(timeout.code, "TIMEOUT");
        // 业务前 timeout 可以人工安全重试。
        assert_eq!(timeout.details["retrySafe"], true);
        // Query timeout 不得标记 mutation。
        assert_eq!(timeout.details["targetMayHaveMutated"], false);
    }
}
