#![cfg(target_os = "windows")]

//! 冻结浏览器页面导航、等待与查询的公开契约候选。

// 导入 JSON 对象、构造器与值类型。
use serde_json::{Map, Value, json};

// 绑定统一公开错误 Schema。
const ERROR_SCHEMA: &str = include_str!(
    // 复用产品唯一 error envelope。
    "../../contracts/v1/error-envelope.schema.json",
);

// 固定合法公开会话目标。
const SESSION_ID: &str = "s2:bs:0123456789abcdef0123456789abcdef";
// 固定第一代公开页面目标。
const PAGE_ID_ONE: &str = "s2:bp:0123456789abcdef0123456789abcdef";
// 固定第二代公开页面目标。
const PAGE_ID_TWO: &str = "s2:bp:fedcba9876543210fedcba9876543210";
// 固定公开元素目标。
const ELEMENT_ID: &str = "s2:be:00112233445566778899aabbccddeeff";

// 核对对象是否只含允许字段。
fn exact_keys(
    // 借用待核对对象。
    object: &Map<String, Value>,
    // 借用允许字段集合。
    expected: &[&str],
) -> bool {
    // 字段数量与名称必须同时完全一致。
    object.len() == expected.len()
        // 拒绝任意未声明字段。
        && object.keys().all(|key| expected.contains(&key.as_str()))
}

// 验证固定前缀与三十二位小写十六进制身份。
fn canonical_identity(
    // 借用待验证文本。
    value: &str,
    // 借用公开身份前缀。
    prefix: &str,
) -> bool {
    // 只接受指定公开前缀。
    value.strip_prefix(prefix).is_some_and(|suffix| {
        // 后缀必须恰为三十二位。
        suffix.len() == 32
            // 每位只能是小写十六进制。
            && suffix
                // 按字节验证 ASCII 集合。
                .bytes()
                // 拒绝大写与其他字符。
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

// 构造所有成功结果共用的顶层 envelope。
fn success_envelope(
    // 接收 generic verb。
    verb: &str,
    // 接收 capability。
    capability: &str,
    // 接收隔离要求。
    isolation: &str,
    // 接收宿主影响策略。
    host_impact: &str,
    // 接收 operation data。
    data: Value,
) -> Value {
    // 返回只含公开字段的 App facade 结果。
    json!({"executionRealm":"isolated-worker","requiredExecutionRealm":"isolated-worker","executionRealmCertified":true,"isolationRequirement":isolation,"hostImpactPolicy":host_impact,"ok":true,"app":"app","verb":verb,"capability":capability,"targetId":SESSION_ID,"data":data,"meta":{"foreground":{"unchanged":true},"targeting":"opaque exact browser session"}})
}

// 构造 navigate 成功数据。
fn navigate_data(
    // 接收新公开页面身份。
    page_id: &str,
    // 接收新导航代际。
    generation: u64,
) -> Value {
    // 返回确认式 Command 完成事实。
    json!({
        // 固定 capability。
        "capability":"browser.page.navigate@1",
        // 固定内部领域动作。
        "action":"navigate",
        // 固定可信完成。
        "outcome":"completed",
        // 固定 dispatch 完成。
        "dispatchState":"completed",
        // 命令已经业务接受。
        "accepted":true,
        // 已取得可信 final。
        "finalStateReached":true,
        // 导航可能改变页面状态。
        "targetMayHaveMutated":true,
        // 固定导航成功事实。
        "navigated":true,
        // 返回 Module 新签发页面身份。
        "pageId":page_id,
        // 返回当前单调代际。
        "navigationGeneration":generation,
        // 宿主前景必须不变。
        "foregroundUnchanged":true,
        // 确认在 dispatch 前评估。
        "confirmationEvaluatedBeforeDispatch":true,
        // 前景策略在 dispatch 前评估。
        "foregroundConsentEvaluatedBeforeDispatch":true,
        // accepted Command 不安全重试。
        "retrySafe":false,
        // 禁止自动新意图。
        "automaticRetryProhibited":true
    })
}

// 构造 wait 成功数据。
fn wait_data() -> Value {
    // 返回只读 Query 完成事实。
    json!({
        // 固定 capability。
        "capability":"browser.page.wait@1",
        // 固定内部领域动作。
        "action":"wait",
        // 固定可信完成。
        "outcome":"completed",
        // 固定 dispatch 完成。
        "dispatchState":"completed",
        // Query 已业务接受。
        "accepted":true,
        // 已取得可信 final。
        "finalStateReached":true,
        // Query 不改变目标。
        "targetMayHaveMutated":false,
        // 固定只读事实。
        "readOnly":true,
        // 回显当前公开页面。
        "pageId":PAGE_ID_ONE,
        // 返回当前导航代际。
        "navigationGeneration":1,
        // 固定条件满足事实。
        "conditionMet":true,
        // 宿主前景必须不变。
        "foregroundUnchanged":true,
        // Policy 已评估无需确认。
        "confirmationEvaluatedBeforeDispatch":true,
        // 前景策略已评估。
        "foregroundConsentEvaluatedBeforeDispatch":true,
        // accepted Query 只可同 identity 恢复。
        "retrySafe":false,
        // 禁止 Adapter 自动生成新意图。
        "automaticRetryProhibited":true
    })
}

// 构造 query 成功数据。
fn query_data(
    // 接收有界公开命中。
    matches: Value,
    // 接收总命中数。
    match_count: u64,
    // 接收截断事实。
    truncated: bool,
) -> Value {
    // 返回只读 Query 完成事实。
    json!({
        // 固定 capability。
        "capability":"browser.page.query@1",
        // 固定内部领域动作。
        "action":"query",
        // 固定可信完成。
        "outcome":"completed",
        // 固定 dispatch 完成。
        "dispatchState":"completed",
        // Query 已业务接受。
        "accepted":true,
        // 已取得可信 final。
        "finalStateReached":true,
        // Query 不改变目标。
        "targetMayHaveMutated":false,
        // 固定只读事实。
        "readOnly":true,
        // 回显当前公开页面。
        "pageId":PAGE_ID_ONE,
        // 返回当前导航代际。
        "navigationGeneration":1,
        // 保存公开有界命中。
        "matches":matches,
        // 保存 worker 总命中数。
        "matchCount":match_count,
        // 保存截断事实。
        "truncated":truncated,
        // 宿主前景必须不变。
        "foregroundUnchanged":true,
        // Policy 已评估无需确认。
        "confirmationEvaluatedBeforeDispatch":true,
        // 前景策略已评估。
        "foregroundConsentEvaluatedBeforeDispatch":true,
        // accepted Query 只可同 identity 恢复。
        "retrySafe":false,
        // 禁止 Adapter 自动生成新意图。
        "automaticRetryProhibited":true
    })
}

// 验证命中摘要不泄漏私有身份。
fn valid_match(
    // 借用待验证命中。
    value: &Value,
) -> bool {
    // 命中必须是对象。
    let Some(object) = value.as_object() else {
        // 非对象拒绝。
        return false;
    };
    // 实际字段只能来自公开摘要集合。
    if object
        // 遍历全部实际字段。
        .keys()
        // 拒绝 backend/native 字段。
        .any(|key| !["elementId", "role", "name", "text", "enabled"].contains(&key.as_str()))
    {
        // 私有字段失败闭合。
        return false;
    }
    // element 与 enabled 必填。
    object.get("elementId").and_then(Value::as_str).is_some_and(|value| {
        // 只接受公开 s2:be。
        canonical_identity(value, "s2:be:")
    })
        // enabled 必须是布尔值。
        && object.get("enabled").is_some_and(Value::is_boolean)
        // 可选摘要必须是有界字符串。
        && ["role", "name", "text"].into_iter().all(|name| {
            // 缺失允许，存在则不超过 1024 字符。
            object.get(name).is_none_or(|value| {
                // 只接受字符串。
                value.as_str().is_some_and(|text| text.chars().count() <= 1_024)
            })
        })
}

// 验证成功 envelope 与 capability 数据一一对应。
fn valid_success(
    // 借用待验证结果。
    value: &Value,
) -> bool {
    // 根必须是对象。
    let Some(root) = value.as_object() else {
        // 非对象拒绝。
        return false;
    };
    // 根字段集合必须精确。
    if !exact_keys(
        // 传入根对象。
        root,
        // 冻结全部 App facade 成功字段。
        &[
            "executionRealm",
            "requiredExecutionRealm",
            "executionRealmCertified",
            "isolationRequirement",
            "hostImpactPolicy",
            "ok",
            "app",
            "verb",
            "capability",
            "targetId",
            "data",
            "meta",
        ],
    ) {
        // 额外或缺失字段拒绝。
        return false;
    }
    // 验证固定执行与目标事实。
    if root.get("executionRealm") != Some(&json!("isolated-worker"))
        // 必需执行域必须相同。
        || root.get("requiredExecutionRealm") != Some(&json!("isolated-worker"))
        // 执行域必须由 System 认证。
        || root.get("executionRealmCertified") != Some(&json!(true))
        // 只接受成功 App envelope。
        || root.get("ok") != Some(&json!(true))
        // 只接受 App surface。
        || root.get("app") != Some(&json!("app"))
        // target 必须是 canonical s2:bs。
        || !root.get("targetId").and_then(Value::as_str).is_some_and(|value| {
            // 验证公开会话身份。
            canonical_identity(value, "s2:bs:")
        })
        // 元数据必须逐字固定。
        || root.get("meta")
            != Some(&json!({"foreground":{"unchanged":true},"targeting":"opaque exact browser session"}))
    {
        // 任一公共事实漂移都拒绝。
        return false;
    }
    // 取得成功数据对象。
    let Some(data) = root.get("data").and_then(Value::as_object) else {
        // 非对象数据拒绝。
        return false;
    };
    // 按 capability 验证 verb、策略与精确数据。
    match root.get("capability").and_then(Value::as_str) {
        // 验证 navigate Command。
        Some("browser.page.navigate@1") => {
            // generic verb 必须为 apply。
            root.get("verb") == Some(&json!("apply"))
                // mutation 使用 strict 隔离。
                && root.get("isolationRequirement") == Some(&json!("strict"))
                // mutation 使用无干扰宿主策略。
                && root.get("hostImpactPolicy") == Some(&json!("strict-no-interference"))
                // 数据字段必须精确。
                && exact_keys(data, &[
                    "capability", "action", "outcome", "dispatchState", "accepted",
                    "finalStateReached", "targetMayHaveMutated", "navigated", "pageId",
                    "navigationGeneration", "foregroundUnchanged",
                    "confirmationEvaluatedBeforeDispatch",
                    "foregroundConsentEvaluatedBeforeDispatch", "retrySafe",
                    "automaticRetryProhibited",
                ])
                // 数据 capability 必须匹配。
                && data.get("capability") == Some(&json!("browser.page.navigate@1"))
                // 动作必须匹配。
                && data.get("action") == Some(&json!("navigate"))
                // 页面必须是公开身份。
                && data.get("pageId").and_then(Value::as_str).is_some_and(|value| {
                    // 只接受 s2:bp。
                    canonical_identity(value, "s2:bp:")
                })
                // 代际必须为 1..u32::MAX。
                && data.get("navigationGeneration").and_then(Value::as_u64)
                    .is_some_and(|value| (1..=u64::from(u32::MAX)).contains(&value))
                // 验证 mutation 公共真值。
                && command_truth(data)
                // 固定导航完成事实。
                && data.get("navigated") == Some(&json!(true))
        }
        // 验证 wait Query。
        Some("browser.page.wait@1") => {
            // 通用 read verb 必须匹配。
            root.get("verb") == Some(&json!("read"))
                // Query 使用 standard 隔离。
                && root.get("isolationRequirement") == Some(&json!("standard"))
                // Query 使用后台优先策略。
                && root.get("hostImpactPolicy") == Some(&json!("background-preferred"))
                // 数据字段必须精确。
                && exact_keys(data, &[
                    "capability", "action", "outcome", "dispatchState", "accepted",
                    "finalStateReached", "targetMayHaveMutated", "readOnly", "pageId",
                    "navigationGeneration", "conditionMet", "foregroundUnchanged",
                    "confirmationEvaluatedBeforeDispatch",
                    "foregroundConsentEvaluatedBeforeDispatch", "retrySafe",
                    "automaticRetryProhibited",
                ])
                // capability 与动作必须匹配。
                && data.get("capability") == Some(&json!("browser.page.wait@1"))
                // 动作必须为 wait。
                && data.get("action") == Some(&json!("wait"))
                // 验证 Query 公共真值。
                && query_truth(data)
                // 页面必须是公开身份。
                && data.get("pageId").and_then(Value::as_str).is_some_and(|value| {
                    // 只接受 s2:bp。
                    canonical_identity(value, "s2:bp:")
                })
                // 代际必须为正 u32。
                && data.get("navigationGeneration").and_then(Value::as_u64)
                    .is_some_and(|value| (1..=u64::from(u32::MAX)).contains(&value))
                // wait 只返回条件满足。
                && data.get("conditionMet") == Some(&json!(true))
        }
        // 验证 query Query。
        Some("browser.page.query@1") => {
            // 通用 read verb 必须匹配。
            root.get("verb") == Some(&json!("read"))
                // Query 使用 standard 隔离。
                && root.get("isolationRequirement") == Some(&json!("standard"))
                // Query 使用后台优先策略。
                && root.get("hostImpactPolicy") == Some(&json!("background-preferred"))
                // 数据字段必须精确。
                && exact_keys(data, &[
                    "capability", "action", "outcome", "dispatchState", "accepted",
                    "finalStateReached", "targetMayHaveMutated", "readOnly", "pageId",
                    "navigationGeneration", "matches", "matchCount", "truncated",
                    "foregroundUnchanged", "confirmationEvaluatedBeforeDispatch",
                    "foregroundConsentEvaluatedBeforeDispatch", "retrySafe",
                    "automaticRetryProhibited",
                ])
                // capability 与动作必须匹配。
                && data.get("capability") == Some(&json!("browser.page.query@1"))
                // 动作必须为 query。
                && data.get("action") == Some(&json!("query"))
                // 验证 Query 公共真值。
                && query_truth(data)
                // 验证 query 有界命中集合。
                && valid_query_result(data)
        }
        // 拒绝未冻结 capability。
        _ => false,
    }
}

// 验证成功 Command 的公共真值。
fn command_truth(
    // 借用成功数据。
    data: &Map<String, Value>,
) -> bool {
    // 核对 accepted/completed/mutation 矩阵。
    data.get("outcome") == Some(&json!("completed"))
        // dispatch 必须完成。
        && data.get("dispatchState") == Some(&json!("completed"))
        // 业务已经接受。
        && data.get("accepted") == Some(&json!(true))
        // 已取得可信 final。
        && data.get("finalStateReached") == Some(&json!(true))
        // Command 可能改变目标。
        && data.get("targetMayHaveMutated") == Some(&json!(true))
        // 宿主前景必须不变。
        && data.get("foregroundUnchanged") == Some(&json!(true))
        // 两项策略必须在 dispatch 前评估。
        && data.get("confirmationEvaluatedBeforeDispatch") == Some(&json!(true))
        // 前景策略也必须被评估。
        && data.get("foregroundConsentEvaluatedBeforeDispatch") == Some(&json!(true))
        // accepted 后不安全重试。
        && data.get("retrySafe") == Some(&json!(false))
        // 禁止自动重派。
        && data.get("automaticRetryProhibited") == Some(&json!(true))
}

// 验证成功 Query 的公共真值。
fn query_truth(
    // 借用成功数据。
    data: &Map<String, Value>,
) -> bool {
    // 核对 accepted/completed/read-only 矩阵。
    data.get("outcome") == Some(&json!("completed"))
        // dispatch 必须完成。
        && data.get("dispatchState") == Some(&json!("completed"))
        // 业务已经接受。
        && data.get("accepted") == Some(&json!(true))
        // 已取得可信 final。
        && data.get("finalStateReached") == Some(&json!(true))
        // Query 不改变目标。
        && data.get("targetMayHaveMutated") == Some(&json!(false))
        // 固定只读。
        && data.get("readOnly") == Some(&json!(true))
        // 宿主前景必须不变。
        && data.get("foregroundUnchanged") == Some(&json!(true))
        // 两项策略必须在 dispatch 前评估。
        && data.get("confirmationEvaluatedBeforeDispatch") == Some(&json!(true))
        // 前景策略也必须被评估。
        && data.get("foregroundConsentEvaluatedBeforeDispatch") == Some(&json!(true))
        // accepted Query 只可同 identity 恢复。
        && data.get("retrySafe") == Some(&json!(false))
        // 禁止 Adapter 自动新意图。
        && data.get("automaticRetryProhibited") == Some(&json!(true))
}

// 验证 query 的有界命中数据。
fn valid_query_result(
    // 借用 query 数据。
    data: &Map<String, Value>,
) -> bool {
    // 页面必须是公开身份。
    if !data
        .get("pageId")
        .and_then(Value::as_str)
        .is_some_and(|value| {
            // 只接受 s2:bp。
            canonical_identity(value, "s2:bp:")
        })
    {
        // 非公开页面拒绝。
        return false;
    }
    // 代际必须为正 u32。
    if !data
        .get("navigationGeneration")
        .and_then(Value::as_u64)
        .is_some_and(|value| (1..=u64::from(u32::MAX)).contains(&value))
    {
        // 越界代际拒绝。
        return false;
    }
    // matches 必须是最多 100 项数组。
    let Some(matches) = data.get("matches").and_then(Value::as_array) else {
        // 非数组拒绝。
        return false;
    };
    // 数组与每项都必须合法。
    if matches.len() > 100 || !matches.iter().all(valid_match) {
        // 超限或私有命中拒绝。
        return false;
    }
    // 读取总命中数。
    let Some(match_count) = data.get("matchCount").and_then(Value::as_u64) else {
        // 非整数拒绝。
        return false;
    };
    // 读取截断事实。
    let Some(truncated) = data.get("truncated").and_then(Value::as_bool) else {
        // 非布尔值拒绝。
        return false;
    };
    // 总命中不得小于返回集合；未截断时必须相等。
    match_count >= matches.len() as u64 && (truncated || match_count == matches.len() as u64)
}

// 构造公开错误 envelope。
fn error_envelope(
    // 接收稳定错误码。
    code: &str,
    // 接收 capability。
    capability: &str,
    // 接收 outcome。
    outcome: &str,
    // 接收业务接受事实。
    accepted: bool,
    // 接收可信 final 事实。
    final_state_reached: bool,
    // 接收重试事实。
    retry_safe: bool,
    // 接收目标变化事实。
    target_may_have_mutated: bool,
    // 接收是否禁止自动重试。
    automatic_retry_prohibited: bool,
) -> Value {
    // 建立公共 details。
    let mut details = json!({
        // 保存公开 capability。
        "capability":capability,
        // 保存 exact target。
        "targetId":SESSION_ID,
        // 保存封闭 outcome。
        "outcome":outcome,
        // 保存业务接受事实。
        "accepted":accepted,
        // 保存可信 final 事实。
        "finalStateReached":final_state_reached,
        // 保存重试事实。
        "retrySafe":retry_safe,
        // 保存目标变化事实。
        "targetMayHaveMutated":target_may_have_mutated
    });
    // accepted 后必须显式禁止自动重试。
    if automatic_retry_prohibited {
        // 只在相应真值矩阵中加入字段。
        details["automaticRetryProhibited"] = json!(true);
    }
    // 返回统一错误 envelope。
    json!({
        // 失败固定为 false。
        "ok":false,
        // 保存公开安全错误。
        "error":{
            // 保存稳定错误码。
            "code":code,
            // 使用非空安全说明。
            "message":"browser page operation did not complete",
            // 保存公开真值矩阵。
            "details":details
        }
    })
}

// 验证公开错误真值矩阵。
fn valid_page_error(
    // 借用待验证错误。
    value: &Value,
) -> bool {
    // 根字段必须精确。
    let Some(root) = value
        .as_object()
        .filter(|root| exact_keys(root, &["ok", "error"]))
    else {
        // 非统一错误 envelope 拒绝。
        return false;
    };
    // ok 必须固定为 false。
    if root.get("ok") != Some(&json!(false)) {
        // 成功 envelope 不能冒充错误。
        return false;
    }
    // 读取 error 对象。
    let Some(error) = root.get("error").and_then(Value::as_object) else {
        // 非对象拒绝。
        return false;
    };
    // error 只允许三个统一字段。
    if !exact_keys(error, &["code", "message", "details"])
        // message 必须非空。
        || !error.get("message").and_then(Value::as_str).is_some_and(|message| !message.is_empty())
    {
        // 字段漂移拒绝。
        return false;
    }
    // 读取 details。
    let Some(details) = error.get("details").and_then(Value::as_object) else {
        // 非对象拒绝。
        return false;
    };
    // details 只允许公共真值字段。
    if details.keys().any(|key| {
        // 检测私有或成功专用字段。
        ![
            "capability",
            "targetId",
            "outcome",
            "accepted",
            "finalStateReached",
            "retrySafe",
            "targetMayHaveMutated",
            "automaticRetryProhibited",
        ]
        // 核对实际键。
        .contains(&key.as_str())
    }) {
        // 任意私有字段拒绝。
        return false;
    }
    // 目标必须是 canonical session。
    if !details
        .get("targetId")
        .and_then(Value::as_str)
        .is_some_and(|value| {
            // 只允许 s2:bs。
            canonical_identity(value, "s2:bs:")
        })
    {
        // 畸形目标拒绝。
        return false;
    }
    // 读取矩阵字段。
    let Some(code) = error.get("code").and_then(Value::as_str) else {
        // 缺失错误码拒绝。
        return false;
    };
    // 读取 capability。
    let Some(capability) = details.get("capability").and_then(Value::as_str) else {
        // 缺失 capability 拒绝。
        return false;
    };
    // capability 必须属于冻结集合。
    if ![
        "browser.page.navigate@1",
        "browser.page.wait@1",
        "browser.page.query@1",
    ]
    // 核对冻结集合。
    .contains(&capability)
    {
        // 未冻结 capability 拒绝。
        return false;
    }
    // 读取其余真值。
    let truth = (
        // 读取 outcome。
        details.get("outcome").and_then(Value::as_str),
        // 读取 accepted。
        details.get("accepted").and_then(Value::as_bool),
        // 读取 final。
        details.get("finalStateReached").and_then(Value::as_bool),
        // 读取 retrySafe。
        details.get("retrySafe").and_then(Value::as_bool),
        // 读取 mutation。
        details.get("targetMayHaveMutated").and_then(Value::as_bool),
        // 读取自动重试禁止。
        details
            .get("automaticRetryProhibited")
            .and_then(Value::as_bool),
    );
    // 业务接受前使用确定未派发矩阵。
    let preaccepted = matches!(
        // 匹配完整真值。
        truth,
        // 未接受、可信结束、可安全修正输入后重试。
        (
            Some("not-dispatched"),
            Some(false),
            Some(true),
            Some(true),
            Some(false),
            None
        )
    ) && [
        // 允许确认缺失。
        "CONFIRMATION_REQUIRED",
        // 允许参数错误。
        "INVALID_ARGUMENT",
        // 允许 stale session。
        "STALE_SESSION",
        // 允许 stale page。
        "STALE_PAGE",
        // 允许固定 Broker 缺失。
        "BROKER_UNAVAILABLE",
        // 允许业务接受前超时。
        "TIMEOUT",
        // 允许业务接受前取消。
        "CANCELLED",
    ]
    // 核对 preaccepted 错误码。
    .contains(&code);
    // accepted 后确定失败矩阵。
    let accepted_failed = matches!(
        // 匹配完整真值。
        truth,
        // 已接受、可信失败、不安全重试、禁止自动重派。
        (Some("failed"), Some(true), Some(true), Some(false), Some(_), Some(true))
    ) && ["OPERATION_FAILED", "TIMEOUT", "CANCELLED"].contains(&code)
        // navigate 必须保守标记 mutation。
        && (capability != "browser.page.navigate@1"
            // navigate failure 必须可能改变目标。
            || details.get("targetMayHaveMutated") == Some(&json!(true)))
        // Query failure 必须保持只读。
        && (capability == "browser.page.navigate@1"
            // wait/query 不得标记 mutation。
            || details.get("targetMayHaveMutated") == Some(&json!(false)));
    // accepted 后未知矩阵。
    let accepted_unknown = matches!(
        // 匹配完整真值。
        truth,
        // 已接受、无可信 final、不安全重试、禁止自动重派。
        (Some("unknown"), Some(true), Some(false), Some(false), Some(_), Some(true))
    ) && code == "OUTCOME_UNKNOWN"
        // navigate unknown 必须保守标记 mutation。
        && (capability != "browser.page.navigate@1"
            // 核对 mutation 事实。
            || details.get("targetMayHaveMutated") == Some(&json!(true)))
        // Query unknown 仍保持只读目标事实。
        && (capability == "browser.page.navigate@1"
            // 核对不突变。
            || details.get("targetMayHaveMutated") == Some(&json!(false)));
    // 任一封闭矩阵通过即可。
    preaccepted || accepted_failed || accepted_unknown
}

// 验证三项成功结果、导航换代及零/多命中。
#[test]
fn success_results_freeze_generation_and_query_cardinality() {
    // 构造第一代导航结果。
    let first = success_envelope(
        // navigate 使用 apply。
        "apply",
        // 使用冻结 capability。
        "browser.page.navigate@1",
        // mutation 使用 strict。
        "strict",
        // mutation 固定无干扰。
        "strict-no-interference",
        // 签发第一代页面。
        navigate_data(PAGE_ID_ONE, 1),
    );
    // 第一代结果必须通过。
    assert!(valid_success(&first));
    // 构造第二代导航结果。
    let second = success_envelope(
        // navigate 使用 apply。
        "apply",
        // 使用冻结 capability。
        "browser.page.navigate@1",
        // mutation 使用 strict。
        "strict",
        // mutation 固定无干扰。
        "strict-no-interference",
        // 签发第二代页面。
        navigate_data(PAGE_ID_TWO, 2),
    );
    // 第二代结果必须通过。
    assert!(valid_success(&second));
    // 新导航必须换发不同公开 identity。
    assert_ne!(first["data"]["pageId"], second["data"]["pageId"]);
    // 新代际必须恰好递增一。
    assert_eq!(second["data"]["navigationGeneration"].as_u64(), Some(2));
    // wait 成功结果必须通过。
    assert!(valid_success(&success_envelope(
        // wait 使用 read。
        "read",
        // 使用 wait capability。
        "browser.page.wait@1",
        // Query 使用 standard。
        "standard",
        // Query 使用后台优先。
        "background-preferred",
        // 使用固定 wait 数据。
        wait_data(),
    )));
    // 零命中 query 必须是可信成功。
    assert!(valid_success(&success_envelope(
        // query 使用 read。
        "read",
        // 使用 query capability。
        "browser.page.query@1",
        // Query 使用 standard。
        "standard",
        // Query 使用后台优先。
        "background-preferred",
        // 零命中不截断。
        query_data(json!([]), 0, false),
    )));
    // 多命中 query 必须保留有界公开摘要。
    assert!(valid_success(&success_envelope(
        // query 使用 read。
        "read",
        // 使用 query capability。
        "browser.page.query@1",
        // Query 使用 standard。
        "standard",
        // Query 使用后台优先。
        "background-preferred",
        // 返回两个公开元素摘要。
        query_data(
            // 构造命中集合。
            json!([
                // 第一项包含完整摘要。
                {"elementId":ELEMENT_ID,"role":"button","name":"Save","enabled":true},
                // 第二项允许最小摘要。
                {"elementId":"s2:be:ffeeddccbbaa99887766554433221100","enabled":false}
            ]),
            // worker 报告两个命中。
            2,
            // 未发生截断。
            false,
        ),
    )));
}

// 验证统一错误 Schema 与页面操作真值矩阵。
#[test]
fn error_envelope_preserves_acceptance_truth() {
    // 统一 Schema 必须已经登记本批使用的稳定错误码。
    for code in [
        // 参数错误。
        "INVALID_ARGUMENT",
        // 确认缺失。
        "CONFIRMATION_REQUIRED",
        // stale session。
        "STALE_SESSION",
        // stale page。
        "STALE_PAGE",
        // 固定 Broker 缺失。
        "BROKER_UNAVAILABLE",
        // 普通执行失败。
        "OPERATION_FAILED",
        // 显式取消。
        "CANCELLED",
        // 未知结果。
        "OUTCOME_UNKNOWN",
    ] {
        // 每个错误码都必须来自唯一公开 Schema。
        assert!(ERROR_SCHEMA.contains(&format!("\"{code}\"")));
    }
    // navigate 确认缺失必须是业务接受前拒绝。
    assert!(valid_page_error(&error_envelope(
        // 使用确认错误。
        "CONFIRMATION_REQUIRED",
        // 使用 navigate capability。
        "browser.page.navigate@1",
        // 固定未派发。
        "not-dispatched",
        // 未业务接受。
        false,
        // 拒绝本身是可信终态。
        true,
        // 修正确认后可安全新请求。
        true,
        // 目标未变化。
        false,
        // 未接受前不出现自动重试禁止。
        false,
    )));
    // wait stale page 必须在业务接受前失败。
    assert!(valid_page_error(&error_envelope(
        // 使用 stale page。
        "STALE_PAGE",
        // 使用 wait capability。
        "browser.page.wait@1",
        // 固定未派发。
        "not-dispatched",
        // 未业务接受。
        false,
        // 拒绝本身是可信终态。
        true,
        // 可重新发现后新请求。
        true,
        // Query 未变化。
        false,
        // 未接受前不出现自动重试禁止。
        false,
    )));
    // navigate accepted 后普通失败或可信取消都必须保守标记 mutation。
    for code in ["OPERATION_FAILED", "CANCELLED"] {
        assert!(valid_page_error(&error_envelope(
            // 使用普通执行失败或显式取消。
            code,
            // 使用 navigate capability。
            "browser.page.navigate@1",
            // 固定失败终态。
            "failed",
            // 已业务接受。
            true,
            // 已取得可信 final。
            true,
            // accepted 后不安全重试。
            false,
            // 导航可能改变目标。
            true,
            // 禁止自动重派。
            true,
        )));
    }
    // query accepted 后未知必须保持目标只读事实。
    assert!(valid_page_error(&error_envelope(
        // 使用未知结果。
        "OUTCOME_UNKNOWN",
        // 使用 query capability。
        "browser.page.query@1",
        // 固定未知 outcome。
        "unknown",
        // 已业务接受。
        true,
        // 没有可信 final。
        false,
        // 不安全自动重试。
        false,
        // Query 不改变目标。
        false,
        // 禁止 Adapter 自动新意图。
        true,
    )));
}
