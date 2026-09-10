//! 通过真实生产 worker/runtime 验证 Browser Session Module 全语义聚合。

// 导入单命令总预算。
use std::time::Duration;

// 导入中立 JSON 值和构造宏。
use serde_json::{Value, json};

// 导入统一错误与结果。
use crate::domain::{AppControlError, AppResult};
// 导入 Browser Session Module 领域类型。
use crate::modules::browser_session::{
    // 导入 provider-neutral selector。
    BrowserSemanticSelector,
    // 导入封闭 wait 条件。
    BrowserSemanticWaitCondition,
    // 导入 Module 命令类别。
    BrowserSessionCommandOutcome,
    // 导入 Module 唯一状态所有者。
    BrowserSessionModule,
    // 导入 Module 打开类别。
    BrowserSessionOpenOutcome,
    // 导入确认式文本输入请求。
    BrowserTypeRequest,
};

// 把 Module 打开 outcome 映射为固定测试文本。
fn module_open_outcome_text(outcome: BrowserSessionOpenOutcome) -> &'static str {
    // 穷举 Module 封闭类别。
    match outcome {
        // 映射 ready。
        BrowserSessionOpenOutcome::Ready => "ready",
        // 映射未派发。
        BrowserSessionOpenOutcome::NotDispatched => "not-dispatched",
        // 映射确定失败。
        BrowserSessionOpenOutcome::Failed => "failed",
        // 映射未知。
        BrowserSessionOpenOutcome::Unknown => "unknown",
    }
}

// 把 Module 页面 outcome 映射为固定测试文本。
fn module_command_outcome_text(outcome: BrowserSessionCommandOutcome) -> &'static str {
    // 穷举 Module 封闭类别。
    match outcome {
        // 映射完成。
        BrowserSessionCommandOutcome::Completed => "completed",
        // 映射未派发。
        BrowserSessionCommandOutcome::NotDispatched => "not-dispatched",
        // 映射确定失败。
        BrowserSessionCommandOutcome::Failed => "failed",
        // 映射未知。
        BrowserSessionCommandOutcome::Unknown => "unknown",
    }
}

// 通过真实生产链验证 Module 导航换代与 stale page 边界。
pub(crate) fn run() -> AppResult<Value> {
    // 建立唯一 Browser Session Module 代际。
    let mut module = BrowserSessionModule::new();
    // 打开工具自有隔离会话。
    let opened = module.open_isolated(
        // 提供充足打开预算。
        Duration::from_secs(15),
        // 不触发取消。
        || false,
    )?;
    // 复制完整打开聚合事实供测试断言。
    let open_outcome = module_open_outcome_text(opened.outcome());
    // 复制可信完成事实。
    let open_completed = opened.completed();
    // 复制安全重试事实。
    let open_retry_safe = opened.retry_safe();
    // 复制可能接受事实。
    let open_accepted = opened.accepted_may_have_occurred();
    // 复制强制回收事实。
    let open_forced_reap = opened.forced_reap();
    // 投影可选安全打开错误而不泄漏输入。
    let open_error = opened.error().map(|error| {
        // 构造封闭测试错误。
        json!({
            // 输出稳定错误码。
            "code": error.code(),
            // 输出安全消息。
            "message": error.message(),
        })
    });
    // ready 必须返回公开 session ID。
    let session_id = opened
        // 借用公开身份。
        .session_id()
        // 非 ready 在本生产测试中是失败。
        .ok_or_else(|| {
            // 优先保留 Module 已投影的安全失败消息。
            let message = opened
                // 借用可选打开错误。
                .error()
                // 复制安全消息或使用固定诊断。
                .map_or_else(
                    // 缺失错误时使用固定诊断。
                    || "The Browser Session Module did not retain a live session.".to_owned(),
                    // 只复制已验证消息。
                    |error| error.message().to_owned(),
                );
            // 返回稳定不可用错误。
            AppControlError::new(
                // 使用浏览器协议不可用类别。
                "BROWSER_PROTOCOL_UNAVAILABLE",
                // 使用安全诊断。
                message,
            )
        })?
        // 独立保存以解除 opened 借用。
        .to_owned();
    // 执行第一代导航。
    let first = module.navigate(
        // 绑定公开 session。
        &session_id,
        // 使用固定 fixture URL。
        "https://example.test/page".to_owned(),
        // 提供充足命令预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // 第一代必须换发公开 page ID。
    let first_page_id = first
        // 借用公开页面身份。
        .page_id()
        // 成功导航缺失页面是内部错误。
        .ok_or_else(|| {
            // 优先保留 Module 已投影的安全失败消息。
            let message = first
                // 借用可选安全错误。
                .error()
                // 复制安全消息。
                .map_or_else(
                    // 缺失错误时使用固定诊断。
                    || {
                        "The first Module navigation did not issue a public page identity."
                            .to_owned()
                    },
                    // 只复制已验证消息。
                    |error| error.message().to_owned(),
                );
            // 返回协议失败。
            AppControlError::new(
                // 使用稳定 worker 类别。
                "WORKER_PROTOCOL_FAILED",
                // 使用安全诊断。
                message,
            )
        })?
        // 独立保存供第二次导航后验证 stale。
        .to_owned();
    // 读取第一代绑定事实。
    let first_generation = module.page_generation(&session_id, &first_page_id)?;
    // 执行第二代导航并替换页面身份。
    let second = module.navigate(
        // 绑定同一公开 session。
        &session_id,
        // 使用另一固定 fixture URL。
        "https://example.test/page".to_owned(),
        // 提供充足命令预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // 第二代必须换发不同公开 page ID。
    let second_page_id = second
        // 借用公开页面身份。
        .page_id()
        // 缺失表示内部契约漂移。
        .ok_or_else(|| {
            // 返回协议失败。
            AppControlError::new(
                // 使用稳定 worker 类别。
                "WORKER_PROTOCOL_FAILED",
                // 使用安全诊断。
                "The second Module navigation did not issue a public page identity.",
            )
        })?
        // 独立保存供关闭前验证。
        .to_owned();
    // 旧页面必须在第二次导航后结构化 stale。
    let stale_error = module
        // 查找旧公开页面。
        .page_generation(&session_id, &first_page_id)
        // 成功将违反导航换代边界。
        .err()
        // 缺失错误视为协议失败。
        .ok_or_else(|| {
            // 返回内部失败。
            AppControlError::new(
                // 使用稳定 worker 类别。
                "WORKER_PROTOCOL_FAILED",
                // 使用安全诊断。
                "The first public page identity remained live after navigation.",
            )
        })?;
    // 当前页面必须绑定第二代。
    let second_generation = module.page_generation(&session_id, &second_page_id)?;
    // 等待当前文档 ready。
    let wait = module.wait(
        // 绑定公开 session。
        &session_id,
        // 绑定当前公开 page。
        &second_page_id,
        // 使用封闭文档条件。
        BrowserSemanticWaitCondition::DocumentReady,
        // 提供充足命令预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // 读取 wait 成功数据。
    let wait_met = wait
        // 借用可选数据。
        .data()
        // completed 必须携带数据。
        .ok_or_else(|| {
            AppControlError::new("WORKER_PROTOCOL_FAILED", "The Module wait data is missing.")
        })?
        // 读取满足事实。
        .condition_met();
    // 等待固定按钮出现以覆盖 selector wait。
    let element_wait = module.wait(
        // 绑定公开 session。
        &session_id,
        // 绑定当前 page。
        &second_page_id,
        // 使用 provider-neutral role selector。
        BrowserSemanticWaitCondition::ElementPresent(BrowserSemanticSelector::new(
            // 匹配固定按钮 role。
            Some("button".to_owned()),
            // 不限制名称。
            None,
            // 不限制文本。
            None,
            // 使用逐字 role 匹配。
            true,
        )),
        // 提供充足预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // 读取元素等待完成事实。
    let element_wait_met = element_wait
        // 借用成功数据。
        .data()
        // 缺失统一协议失败。
        .ok_or_else(|| {
            AppControlError::new(
                "WORKER_PROTOCOL_FAILED",
                "The Module element wait data is missing.",
            )
        })?
        // 读取满足事实。
        .condition_met();
    // 等待固定可见文本以覆盖文本 wait。
    let text_wait = module.wait(
        // 绑定公开 session。
        &session_id,
        // 绑定当前 page。
        &second_page_id,
        // 使用封闭文本条件。
        BrowserSemanticWaitCondition::TextPresent {
            // 使用 runtime fixture 固定文本。
            text: "Welcome Home".to_owned(),
            // 要求逐字匹配。
            exact: true,
        },
        // 提供充足预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // 读取文本等待完成事实。
    let text_wait_met = text_wait
        // 借用成功数据。
        .data()
        // 缺失统一协议失败。
        .ok_or_else(|| {
            AppControlError::new(
                "WORKER_PROTOCOL_FAILED",
                "The Module text wait data is missing.",
            )
        })?
        // 读取满足事实。
        .condition_met();
    // 查询固定按钮语义。
    let query = module.query(
        // 绑定公开 session。
        &session_id,
        // 绑定当前公开 page。
        &second_page_id,
        // 只使用 provider-neutral role selector。
        BrowserSemanticSelector::new(Some("button".to_owned()), None, None, true),
        // 接受固定 fixture 的全部按钮。
        10,
        // 提供充足命令预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // query completed 必须携带数据。
    let query_data = query
        // 借用可选数据。
        .data()
        // 缺失统一协议失败。
        .ok_or_else(|| {
            AppControlError::new(
                "WORKER_PROTOCOL_FAILED",
                "The Module query data is missing.",
            )
        })?;
    // 至少一个固定按钮必须签发公开元素身份。
    let element = query_data
        // 读取首个命中。
        .matches()
        // 借用第一项。
        .first()
        // 缺失统一协议失败。
        .ok_or_else(|| {
            AppControlError::new(
                "WORKER_PROTOCOL_FAILED",
                "The Module query returned no fixture element.",
            )
        })?;
    // 复制公开元素身份供后续写入。
    let element_id = element.element_id().to_owned();
    // 复制 provider-neutral 摘要。
    let element_role = element.role().map(str::to_owned);
    // 复制可选名称。
    let element_name = element.name().map(str::to_owned);
    // 复制可选文本。
    let element_text = element.text().map(str::to_owned);
    // 复制可用事实。
    let element_enabled = element.enabled();
    // 复制 query 数量事实。
    let query_match_count = query_data.match_count();
    // 复制 query 截断事实。
    let query_truncated = query_data.truncated();
    // 重复同一查询以验证公开元素身份稳定复用。
    let repeated_query = module.query(
        // 绑定公开 session。
        &session_id,
        // 绑定当前 page。
        &second_page_id,
        // 重复相同 role selector。
        BrowserSemanticSelector::new(Some("button".to_owned()), None, None, true),
        // 保持相同结果上限。
        10,
        // 提供充足预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // 读取重复查询首个公开身份。
    let repeated_element_id = repeated_query
        // 借用成功数据。
        .data()
        // 缺失统一协议失败。
        .and_then(|data| data.matches().first())
        // 缺失统一协议失败。
        .ok_or_else(|| {
            AppControlError::new(
                "WORKER_PROTOCOL_FAILED",
                "The repeated Module query returned no fixture element.",
            )
        })?
        // 复制公开 identity。
        .element_id()
        // 保存独立值。
        .to_owned();
    // 执行确定性零命中查询。
    let empty_query = module.query(
        // 绑定公开 session。
        &session_id,
        // 绑定当前 page。
        &second_page_id,
        // 使用 fixture 不存在的 role。
        BrowserSemanticSelector::new(Some("missing-role".to_owned()), None, None, true),
        // 保持合法结果上限。
        10,
        // 提供充足预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // 读取零命中数据。
    let empty_query_data = empty_query
        // 借用成功数据。
        .data()
        // 缺失统一协议失败。
        .ok_or_else(|| {
            AppControlError::new(
                "WORKER_PROTOCOL_FAILED",
                "The empty Module query data is missing.",
            )
        })?;
    // 复制零命中事实。
    let empty_query_count = empty_query_data.match_count();
    // 复制空集合事实。
    let empty_query_is_empty = empty_query_data.matches().is_empty();
    // 未确认点击必须在任何目标解析前失败。
    let confirmation_error = module
        // 使用故意不存在的全部身份。
        .click(
            // 使用 stale session 文本。
            "s2:bs:00000000000000000000000000000000",
            // 使用 stale page 文本。
            "s2:bp:00000000000000000000000000000000",
            // 使用 stale element 文本。
            "s2:be:00000000000000000000000000000000",
            // 明确不确认。
            false,
            // 使用合法预算。
            Duration::from_secs(5),
            // 不触发取消。
            || false,
        )
        // 只取得结构化错误。
        .err()
        // 缺失表示安全顺序漂移。
        .ok_or_else(|| {
            AppControlError::new(
                "WORKER_PROTOCOL_FAILED",
                "The unconfirmed Module click was accepted.",
            )
        })?;
    // 当前页面中的未知元素必须结构化 stale。
    let stale_element = module
        // 使用真实 session/page 和未知 element。
        .click(
            // 绑定公开 session。
            &session_id,
            // 绑定当前 page。
            &second_page_id,
            // 使用 canonical 但未签发的 element。
            "s2:be:00000000000000000000000000000000",
            // 显式确认以进入目标解析。
            true,
            // 使用合法预算。
            Duration::from_secs(5),
            // 不触发取消。
            || false,
        )
        // 只取得结构化错误。
        .err()
        // 缺失表示 stale 边界漂移。
        .ok_or_else(|| {
            AppControlError::new(
                "WORKER_PROTOCOL_FAILED",
                "The stale Module element was accepted.",
            )
        })?;
    // 对已签发元素执行确认式点击。
    let click = module.click(
        // 绑定公开 session。
        &session_id,
        // 绑定当前 page。
        &second_page_id,
        // 绑定公开 element。
        &element_id,
        // 显式确认。
        true,
        // 提供充足预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // 读取点击成功事实。
    let clicked = click
        // 借用可选数据。
        .data()
        // 缺失统一协议失败。
        .ok_or_else(|| {
            AppControlError::new(
                "WORKER_PROTOCOL_FAILED",
                "The Module click data is missing.",
            )
        })?
        // 读取完成事实。
        .clicked();
    // 对同一公开元素执行确认式输入。
    let typed = module.type_text(
        // 绑定公开 session。
        &session_id,
        // 绑定当前 page。
        &second_page_id,
        // 绑定公开 element。
        &element_id,
        // 使用固定文本、替换和显式确认。
        BrowserTypeRequest::new("fixture text".to_owned(), true, true),
        // 提供充足预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // 读取输入成功数据。
    let typed_data = typed
        // 借用可选数据。
        .data()
        // 缺失统一协议失败。
        .ok_or_else(|| {
            AppControlError::new("WORKER_PROTOCOL_FAILED", "The Module type data is missing.")
        })?;
    // 复制输入完成事实。
    let typed_completed = typed_data.typed();
    // 复制 UTF-8 字节事实。
    let typed_bytes = typed_data.utf8_bytes();
    // 捕获当前页面固定 PNG。
    let screenshot = module.screenshot(
        // 绑定公开 session。
        &session_id,
        // 绑定当前 page。
        &second_page_id,
        // 提供充足预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // 读取截图成功数据。
    let screenshot_data = screenshot
        // 借用可选数据。
        .data()
        // 缺失统一协议失败。
        .ok_or_else(|| {
            AppControlError::new(
                "WORKER_PROTOCOL_FAILED",
                "The Module screenshot data is missing.",
            )
        })?;
    // 复制固定 MIME。
    let screenshot_mime = screenshot_data.mime_type().to_owned();
    // 只记录 Base64 非空事实。
    let screenshot_base64_present = !screenshot_data.png_base64().is_empty();
    // 复制 PNG 字节数。
    let screenshot_bytes = screenshot_data.png_bytes();
    // 复制尺寸。
    let screenshot_width = screenshot_data.width();
    // 复制高度。
    let screenshot_height = screenshot_data.height();
    // 复制摘要形状事实。
    let screenshot_digest_valid = screenshot_data.digest().len() == 16;
    // 把动作证据拆成独立对象以保持测试投影有界。
    let action_evidence = json!({
        // 输出 wait 满足事实与类别。
        "waitConditionMet": wait_met, "waitOutcome": module_command_outcome_text(wait.outcome()),
        // 输出两类语义 wait 满足事实。
        "elementWaitMet": element_wait_met, "textWaitMet": text_wait_met,
        // 输出 wait 聚合事实。
        "waitCompleted": wait.completed(), "waitRetrySafe": wait.retry_safe(),
        // 输出 wait 接受与代际事实。
        "waitAccepted": wait.accepted_may_have_occurred(), "waitGeneration": wait.navigation_generation(),
        // 输出 wait 回收与错误事实。
        "waitForcedReap": wait.forced_reap(), "waitError": wait.error().map(|error| error.code()),
        // 输出 query 类别与完成事实。
        "queryOutcome": module_command_outcome_text(query.outcome()), "queryCompleted": query.completed(),
        // 输出 query 重试与接受事实。
        "queryRetrySafe": query.retry_safe(), "queryAccepted": query.accepted_may_have_occurred(),
        // 输出 query 代际与回收事实。
        "queryGeneration": query.navigation_generation(), "queryForcedReap": query.forced_reap(),
        // 输出 query 错误事实。
        "queryError": query.error().map(|error| error.code()),
        // 只输出公开元素 ID 形状事实。
        "elementIdValid": element_id.starts_with("s2:be:") && element_id.len() == 38,
        // 输出元素中立摘要。
        "elementRole": element_role, "elementName": element_name, "elementText": element_text,
        // 输出元素可用与 query 计数事实。
        "elementEnabled": element_enabled, "queryMatchCount": query_match_count,
        // 输出 query 截断事实。
        "queryTruncated": query_truncated,
        // 输出重复查询身份稳定事实。
        "elementIdentityStable": repeated_element_id == element_id,
        // 输出零命中集合与计数事实。
        "emptyQueryIsEmpty": empty_query_is_empty, "emptyQueryCount": empty_query_count,
        // 输出确认与 stale element 顺序证据。
        "confirmationCode": confirmation_error.code, "staleElementCode": stale_element.code,
        // 输出点击与输入结果。
        "clicked": clicked, "typed": typed_completed, "typedBytes": typed_bytes,
        // 输出截图 provider-neutral 事实。
        "screenshotMime": screenshot_mime, "screenshotBase64Present": screenshot_base64_present,
        // 输出截图容器事实。
        "screenshotBytes": screenshot_bytes, "screenshotWidth": screenshot_width,
        // 输出截图尺寸与摘要事实。
        "screenshotHeight": screenshot_height, "screenshotDigestValid": screenshot_digest_valid,
    });
    // 显式关闭并从 registry 移除会话。
    let graceful_close = module.close(&session_id)?;
    // 关闭后的 session 必须结构化 stale。
    let closed_error = module
        // 重新查询已关闭 session。
        .page_generation(&session_id, &second_page_id)
        // 只取结构化错误。
        .err()
        // 缺失错误表示 registry 漏删。
        .ok_or_else(|| {
            // 返回内部失败。
            AppControlError::new(
                // 使用稳定 worker 类别。
                "WORKER_PROTOCOL_FAILED",
                // 使用安全诊断。
                "The closed browser session remained registered.",
            )
        })?;
    // 返回不含 worker 私有引用的 Module 证据。
    Ok(json!({
        // 输出完整打开结果类别。
        "openOutcome": open_outcome,
        // 输出打开完成事实。
        "openCompleted": open_completed,
        // 输出打开重试事实。
        "openRetrySafe": open_retry_safe,
        // 输出打开接受事实。
        "openAcceptedMayHaveOccurred": open_accepted,
        // 输出打开回收事实。
        "openForcedReap": open_forced_reap,
        // 输出可选打开错误。
        "openError": open_error,
        // 只输出 session ID 形状事实。
        "sessionIdValid": session_id.starts_with("s2:bs:") && session_id.len() == 38,
        // 输出第一代结果类别。
        "firstOutcome": module_command_outcome_text(first.outcome()),
        // 输出第一代完成事实。
        "firstCompleted": first.completed(),
        // 输出第一代重试事实。
        "firstRetrySafe": first.retry_safe(),
        // 输出第一代接受事实。
        "firstAcceptedMayHaveOccurred": first.accepted_may_have_occurred(),
        // 输出第一代错误投影。
        "firstError": first.error().map(|error| json!({ "code": error.code(), "message": error.message() })),
        // 输出第一代回收事实。
        "firstForcedReap": first.forced_reap(),
        // 只输出第一代公开页面身份形状。
        "firstPageIdValid": first_page_id.starts_with("s2:bp:") && first_page_id.len() == 38,
        // 输出第一代代际。
        "firstGeneration": first_generation,
        // 输出第一代命令报告代际。
        "firstReportedGeneration": first.navigation_generation(),
        // 输出第二代结果类别。
        "secondOutcome": module_command_outcome_text(second.outcome()),
        // 输出第二代完成事实。
        "secondCompleted": second.completed(),
        // 输出第二代重试事实。
        "secondRetrySafe": second.retry_safe(),
        // 输出第二代接受事实。
        "secondAcceptedMayHaveOccurred": second.accepted_may_have_occurred(),
        // 输出第二代错误投影。
        "secondError": second.error().map(|error| json!({ "code": error.code(), "message": error.message() })),
        // 输出第二代回收事实。
        "secondForcedReap": second.forced_reap(),
        // 只输出第二代公开页面身份形状。
        "secondPageIdValid": second_page_id.starts_with("s2:bp:") && second_page_id.len() == 38,
        // 证明两代公开身份不同。
        "pageIdsDistinct": first_page_id != second_page_id,
        // 输出第二代代际。
        "secondGeneration": second_generation,
        // 输出第二代命令报告代际。
        "secondReportedGeneration": second.navigation_generation(),
        // 输出独立动作证据对象。
        "actions": action_evidence,
        // 输出旧页面结构化 stale。
        "stalePageCode": stale_error.code,
        // 输出显式关闭事实。
        "gracefulClose": graceful_close,
        // 输出关闭后结构化 stale session。
        "closedSessionCode": closed_error.code,
    }))
}
