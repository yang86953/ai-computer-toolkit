//! 冻结浏览器会话生命周期公开契约候选的纯 Schema 事实。

// 导入 JSON 构造与值类型。
use serde_json::{Map, Value, json};

// 绑定输入 Schema 文本。
const INPUT_SCHEMA: &str = include_str!(
    // 只读取本测试所有的契约候选。
    "../contracts/v1/browser-session-lifecycle-input.schema.json" // 结束输入 Schema 引用。
);
// 绑定成功结果 Schema 文本。
const RESULT_SCHEMA: &str = include_str!(
    // 只读取本测试所有的成功契约候选。
    "../contracts/v1/browser-session-lifecycle-result.schema.json" // 结束结果 Schema 引用。
);
// 绑定统一公开错误 Envelope Schema 文本。
const ERROR_SCHEMA: &str = include_str!(
    // 只读取已登记公开错误码的统一契约。
    "../contracts/v1/error-envelope.schema.json" // 结束错误 Schema 引用。
);

// 固定合法主机目标。
const HOST_ID: &str = "s2:h:0123456789abcdef";
// 固定合法浏览器会话目标。
const SESSION_ID: &str = "s2:bs:0123456789abcdef0123456789abcdef";

// 核对对象是否只含契约允许的键。
fn exact_keys(
    // 借用待校验对象。
    object: &Map<String, Value>,
    // 借用冻结键集合。
    expected: &[&str],
    // 返回封闭字段事实。
) -> bool {
    // 键数必须精确相等。
    object.len() == expected.len()
        // 每个实际键都必须在冻结集合中。
        && object.keys().all(|key| expected.contains(&key.as_str()))
    // 结束字段集合核对。
}

// 验证公开主机会话形状。
fn canonical_host(value: &str) -> bool {
    // 只允许 s2:h 前缀。
    value.strip_prefix("s2:h:").is_some_and(|suffix| {
        // 后缀必须恰为十六位。
        suffix.len() == 16
            // 只允许小写十六进制。
            && suffix.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        // 结束主机后缀验证。
    })
    // 结束主机目标验证。
}

// 验证公开浏览器会话形状。
fn canonical_session(value: &str) -> bool {
    // 只允许 s2:bs 前缀。
    value.strip_prefix("s2:bs:").is_some_and(|suffix| {
        // 后缀必须恰为三十二位。
        suffix.len() == 32
            // 只允许小写十六进制。
            && suffix.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        // 结束会话后缀验证。
    })
    // 结束浏览器会话验证。
}

// 构造冻结 open success data。
fn open_data() -> Value {
    // 返回无私有路由字段的最小成功投影。
    json!({
        // 固定 open capability。
        "capability":"browser.session.open@1",
        // 固定 open 动作。
        "action":"open",
        // 只表达可信完成。
        "outcome":"completed",
        // 固定完成 dispatch 状态。
        "dispatchState":"completed",
        // 标记已越过业务接受点。
        "accepted":true,
        // 标记已取得可信 final。
        "finalStateReached":true,
        // 生命周期 Command 保守标记已改变。
        "targetMayHaveMutated":true,
        // 新会话必须存活。
        "state":"live",
        // 只返回新 opaque session。
        "sessionId":SESSION_ID,
        // 固定不改变宿主前景。
        "foregroundUnchanged":true,
        // 确认必须在 dispatch 前完成评估。
        "confirmationEvaluatedBeforeDispatch":true,
        // 前景策略即使无需同意也必须被评估。
        "foregroundConsentEvaluatedBeforeDispatch":true,
        // 成功 mutation 不安全重试。
        "retrySafe":false,
        // 禁止自动重派。
        "automaticRetryProhibited":true
    // 结束 open data。
    })
    // 结束 open data 构造。
}

// 构造冻结 close success data。
fn close_data() -> Value {
    // 返回不重复 sessionId 的最小关闭投影。
    json!({
        // 固定 close capability。
        "capability":"browser.session.close@1",
        // 固定 close 动作。
        "action":"close",
        // 只表达可信完成。
        "outcome":"completed",
        // 固定完成 dispatch 状态。
        "dispatchState":"completed",
        // 标记已越过业务接受点。
        "accepted":true,
        // 标记已取得可信 final。
        "finalStateReached":true,
        // 生命周期 Command 保守标记已改变。
        "targetMayHaveMutated":true,
        // 关闭后状态固定。
        "state":"closed",
        // 固定关闭完成事实。
        "closed":true,
        // 固定不改变宿主前景。
        "foregroundUnchanged":true,
        // 确认必须在 dispatch 前完成评估。
        "confirmationEvaluatedBeforeDispatch":true,
        // 前景策略即使无需同意也必须被评估。
        "foregroundConsentEvaluatedBeforeDispatch":true,
        // 成功 mutation 不安全重试。
        "retrySafe":false,
        // 禁止自动重派。
        "automaticRetryProhibited":true
    // 结束 close data。
    })
    // 结束 close data 构造。
}

// 核对测试样例是否符合冻结成功形状。
fn success_shape(value: &Value) -> bool {
    // 根必须是 JSON 对象。
    let Some(root) = value.as_object() else {
        return false;
    };
    // 根只允许 App facade 核心字段与隔离证明。
    if !exact_keys(
        root,
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
        // 额外字段必须失败闭合。
        return false;
        // 结束根字段门禁。
    }
    // 固定 App facade 公共成功事实。
    if root.get("ok") != Some(&json!(true)) || root.get("app") != Some(&json!("app")) {
        // 拒绝失败 envelope 或其他 surface。
        return false;
        // 结束 App 公共事实门禁。
    }
    // 两个 capability 都必须由隔离 worker 完成并认证。
    if root.get("executionRealm") != Some(&json!("isolated-worker"))
        // 必需执行域必须与实际执行域一致。
        || root.get("requiredExecutionRealm") != Some(&json!("isolated-worker"))
        // System 必须证明执行域，而不是仅回显请求。
        || root.get("executionRealmCertified") != Some(&json!(true))
    {
        // 任一执行域事实漂移都必须失败闭合。
        return false;
        // 结束执行域门禁。
    }
    // 隔离等级与宿主影响策略必须严格配对。
    let isolation_matches = (root.get("isolationRequirement") == Some(&json!("standard"))
        // standard 只允许后台优先。
        && root.get("hostImpactPolicy") == Some(&json!("background-preferred")))
        // strict 构成第二种合法组合。
        || (root.get("isolationRequirement") == Some(&json!("strict"))
            // strict 只允许严格无干扰。
            && root.get("hostImpactPolicy") == Some(&json!("strict-no-interference")));
    // 拒绝交叉搭配的宿主影响策略。
    if !isolation_matches {
        // 返回封闭失败。
        return false;
        // 结束隔离矩阵门禁。
    }
    // 读取不可缺失的公开 target。
    let Some(target) = root.get("targetId").and_then(Value::as_str) else {
        return false;
    };
    // 元数据必须精确证明前景不变。
    if root.get("meta")
        != Some(&json!({"foreground":{"unchanged":true},"targeting":"opaque exact session"}))
    {
        // 拒绝前景或目标语义漂移。
        return false;
        // 结束元数据门禁。
    }
    // 按 capability 封闭区分 open 与 close。
    match root.get("capability").and_then(Value::as_str) {
        // open 必须使用 create、host target 与新 session data。
        Some("browser.session.open@1") => {
            root.get("verb") == Some(&json!("create"))
                && canonical_host(target)
                && root.get("data") == Some(&open_data())
        }
        // close 必须使用 close、browser-session target 与无重复 ID 的 data。
        Some("browser.session.close@1") => {
            root.get("verb") == Some(&json!("close"))
                && canonical_session(target)
                && root.get("data") == Some(&close_data())
        }
        // 拒绝未冻结 capability。
        _ => false,
        // 结束 capability 分流。
    }
    // 结束成功形状核对。
}

// 核对公开错误 Envelope 与生命周期真值矩阵。
fn lifecycle_error_shape(value: &Value) -> bool {
    // 根必须是严格错误对象。
    let Some(root) = value.as_object() else {
        return false;
    };
    // 根只允许统一 Envelope 的两个字段。
    if !exact_keys(root, &["ok", "error"]) || root.get("ok") != Some(&json!(false)) {
        // 拒绝额外顶层事实与 success 混用。
        return false;
        // 结束错误根门禁。
    }
    // 读取严格错误对象。
    let Some(error) = root.get("error").and_then(Value::as_object) else {
        return false;
    };
    // 错误对象必须保留 code、message 与 details。
    if !exact_keys(error, &["code", "message", "details"])
        // 消息必须非空。
        || error.get("message").and_then(Value::as_str).is_none_or(str::is_empty)
    {
        // 拒绝不完整错误 Envelope。
        return false;
        // 结束错误对象门禁。
    }
    // 读取生命周期错误详情。
    let Some(details) = error.get("details").and_then(Value::as_object) else {
        return false;
    };
    // capability 必须保持 open 或 close。
    let Some(capability) = details.get("capability").and_then(Value::as_str) else {
        return false;
    };
    // targetId 只在 caller 已提供 canonical 目标时允许出现。
    let target = details.get("targetId").and_then(Value::as_str);
    // 已出现的 target 必须与 capability 种类一致。
    let target_matches = target.is_none_or(|target| match capability {
        // open 保留主机会话目标。
        "browser.session.open@1" => canonical_host(target),
        // close 保留浏览器会话目标。
        "browser.session.close@1" => canonical_session(target),
        // 未冻结 capability 必须失败闭合。
        _ => false,
        // 结束 capability 目标匹配。
    });
    // 拒绝错误的目标种类。
    if !target_matches {
        // 返回封闭失败。
        return false;
        // 结束目标种类门禁。
    }
    // 读取稳定 code 与 outcome 共同冻结业务接受阶段。
    let code = error.get("code").and_then(Value::as_str);
    // 按最后可信 outcome 校验完整真值。
    match details.get("outcome").and_then(Value::as_str) {
        // 明确业务接受前允许已登记且适用于本阶段的公开错误。
        Some("not-dispatched")
            if matches!(
                code,
                Some(
                    "CONFIRMATION_REQUIRED"
                        | "INVALID_ARGUMENT"
                        | "STALE_SESSION"
                        | "BROWSER_SESSION_REGISTRY_FULL"
                        | "CAPABILITY_ASSESSMENT_UNAVAILABLE"
                        | "CAPABILITY_UNAVAILABLE"
                        | "ISOLATED_WORKER_UNAVAILABLE"
                        | "BROKER_UNAVAILABLE"
                        | "TIMEOUT"
                        | "CANCELLED"
                        | "PERMISSION_DENIED"
                )
            ) =>
        {
            // 根据 caller 是否提供 canonical target 选择严格字段集合。
            let keys_match = if target.is_some() {
                // canonical target 必须逐字保留。
                exact_keys(
                    details,
                    &[
                        "capability",
                        "targetId",
                        "outcome",
                        "accepted",
                        "finalStateReached",
                        "retrySafe",
                        "targetMayHaveMutated",
                    ],
                )
            } else {
                // 缺失或畸形 target 不得被捏造。
                exact_keys(
                    details,
                    &[
                        "capability",
                        "outcome",
                        "accepted",
                        "finalStateReached",
                        "retrySafe",
                        "targetMayHaveMutated",
                    ],
                )
                // 结束字段集合分支。
            };
            // 未派发详情必须匹配对应严格字段集合。
            keys_match
                // 结果必须明确未派发。
                && details.get("outcome") == Some(&json!("not-dispatched"))
                // 业务侧不得已经接受。
                && details.get("accepted") == Some(&json!(false))
                // 已取得可信未派发终态。
                && details.get("finalStateReached") == Some(&json!(true))
                // 已证明未派发才允许安全人工重试。
                && details.get("retrySafe") == Some(&json!(true))
                // 未派发不得声称目标可能改变。
                && details.get("targetMayHaveMutated") == Some(&json!(false))
        }
        // 已接受且取得可信失败终态必须禁止重派。
        Some("failed")
            if matches!(
                code,
                Some("OPERATION_FAILED" | "HOST_INTERFERENCE_DETECTED")
            ) =>
        {
            // accepted 后必须已有 canonical target 且固定八个字段。
            target.is_some()
                && exact_keys(details, &["capability", "targetId", "outcome", "accepted", "finalStateReached", "retrySafe", "targetMayHaveMutated", "automaticRetryProhibited"])
                // 业务侧已经接受。
                && details.get("accepted") == Some(&json!(true))
                // 可信失败 final 已经形成。
                && details.get("finalStateReached") == Some(&json!(true))
                // 已接受 mutation 不可安全重试。
                && details.get("retrySafe") == Some(&json!(false))
                // target 可能已经改变。
                && details.get("targetMayHaveMutated") == Some(&json!(true))
                // 自动重派必须显式禁止。
                && details.get("automaticRetryProhibited") == Some(&json!(true))
        }
        // 业务接受后缺少可信 final 只能公开未知结果。
        Some("unknown") if code == Some("OUTCOME_UNKNOWN") => {
            // 未知结果详情只允许八个冻结字段。
            target.is_some()
                && exact_keys(details, &["capability", "targetId", "outcome", "accepted", "finalStateReached", "retrySafe", "targetMayHaveMutated", "automaticRetryProhibited"])
                // 结果必须保持未知。
                && details.get("outcome") == Some(&json!("unknown"))
                // 业务侧必须已经接受。
                && details.get("accepted") == Some(&json!(true))
                // 缺少可信 final 不得伪造完成。
                && details.get("finalStateReached") == Some(&json!(false))
                // 未知 mutation 绝不安全重试。
                && details.get("retrySafe") == Some(&json!(false))
                // 保守声明目标可能改变。
                && details.get("targetMayHaveMutated") == Some(&json!(true))
                // 公共 Adapter 必须禁止自动重派。
                && details.get("automaticRetryProhibited") == Some(&json!(true))
        }
        // 其他阶段或错误组合不属于本生命周期接受矩阵。
        _ => false,
        // 结束公开错误码分流。
    }
    // 结束错误形状核对。
}

// 验证 Schema 本身可解析且冻结已定字段。
#[test]
fn schemas_freeze_lifecycle_ids_targets_and_evidence() {
    // 解析输入 Schema。
    let input: Value = serde_json::from_str(INPUT_SCHEMA).expect("input schema must parse");
    // 解析结果 Schema。
    let result: Value = serde_json::from_str(RESULT_SCHEMA).expect("result schema must parse");
    // 解析统一错误 Schema。
    let error: Value = serde_json::from_str(ERROR_SCHEMA).expect("error schema must parse");
    // 输入契约 ID 必须与未来 registry 同源。
    assert_eq!(input["$id"], "schema://browser/session-lifecycle/v1");
    // 输入对象必须拒绝额外字段。
    assert_eq!(input["additionalProperties"], false);
    // 超时默认值必须固定为五秒。
    assert_eq!(input["properties"]["timeoutMs"]["default"], 5000);
    // 超时上限必须与私有 Broker 总预算一致。
    assert_eq!(input["properties"]["timeoutMs"]["maximum"], 30000);
    // 输入不得重复 capability 和请求 envelope 事实。
    assert!(
        input["properties"].get("action").is_none()
            && input["properties"].get("confirmed").is_none()
    );
    // 结果只有 open 与 close 两个互斥分支。
    assert_eq!(result["oneOf"].as_array().map(Vec::len), Some(2));
    // 成功根必须要求全部五项隔离证明。
    for field in [
        "executionRealm",
        "requiredExecutionRealm",
        "executionRealmCertified",
        "isolationRequirement",
        "hostImpactPolicy",
    ] {
        // 每项证明都不得被 Adapter 省略。
        assert!(
            result["required"]
                .as_array()
                .is_some_and(|required| required.contains(&json!(field)))
        );
        // 结束隔离证明 required 核对。
    }
    // 实际执行域必须固定为隔离 worker。
    assert_eq!(
        result["properties"]["executionRealm"]["const"],
        "isolated-worker"
    );
    // 必需执行域必须同样固定为隔离 worker。
    assert_eq!(
        result["properties"]["requiredExecutionRealm"]["const"],
        "isolated-worker"
    );
    // System 必须认证实际执行域。
    assert_eq!(
        result["properties"]["executionRealmCertified"]["const"],
        true
    );
    // standard 必须映射到后台优先策略。
    assert_eq!(
        result["allOf"][0]["oneOf"][0]["properties"]["hostImpactPolicy"]["const"],
        "background-preferred"
    );
    // strict 必须映射到严格无干扰策略。
    assert_eq!(
        result["allOf"][0]["oneOf"][1]["properties"]["hostImpactPolicy"]["const"],
        "strict-no-interference"
    );
    // open 分支必须绑定 create。
    assert_eq!(result["oneOf"][0]["properties"]["verb"]["const"], "create");
    // close 分支必须绑定 close。
    assert_eq!(result["oneOf"][1]["properties"]["verb"]["const"], "close");
    // open target 必须使用精确主机 session。
    assert_eq!(
        result["$defs"]["hostSessionId"]["pattern"],
        "^s2:h:[0-9a-f]{16}$"
    );
    // 浏览器 session 必须使用三十二位 opaque 后缀。
    assert_eq!(
        result["$defs"]["browserSessionId"]["pattern"],
        "^s2:bs:[0-9a-f]{32}$"
    );
    // close data 不得重复顶层 targetId。
    assert!(
        result["$defs"]["closeData"]["properties"]
            .get("sessionId")
            .is_none()
    );
    // 两个成功分支都必须保守标记 target mutation。
    assert_eq!(
        result["$defs"]["openData"]["properties"]["targetMayHaveMutated"]["const"],
        true
    );
    // close 也不得放宽 mutation 事实。
    assert_eq!(
        result["$defs"]["closeData"]["properties"]["targetMayHaveMutated"]["const"],
        true
    );
    // 读取公开错误码枚举。
    let error_codes = error["properties"]["error"]["properties"]["code"]["enum"]
        .as_array()
        .expect("error code enum");
    // 生命周期矩阵只能引用已登记公开错误码。
    for code in [
        // 确认拒绝属于接受前公开错误。
        "CONFIRMATION_REQUIRED",
        // 输入拒绝属于接受前公开错误。
        "INVALID_ARGUMENT",
        // stale 拒绝属于接受前公开错误。
        "STALE_SESSION",
        // 固定 registry 容量拒绝属于接受前公开错误。
        "BROWSER_SESSION_REGISTRY_FULL",
        // 安全姿态不可认证属于接受前公开错误。
        "CAPABILITY_ASSESSMENT_UNAVAILABLE",
        // capability 缺失属于接受前公开错误。
        "CAPABILITY_UNAVAILABLE",
        // 隔离路线缺失属于接受前公开错误。
        "ISOLATED_WORKER_UNAVAILABLE",
        // broker 缺失属于接受前公开错误。
        "BROKER_UNAVAILABLE",
        // 总预算耗尽属于接受前公开错误。
        "TIMEOUT",
        // 接受前取消属于公开错误。
        "CANCELLED",
        // 权限拒绝属于接受前公开错误。
        "PERMISSION_DENIED",
        // 已接受可信失败收敛为公共操作失败。
        "OPERATION_FAILED",
        // provider 后宿主证据失败必须登记。
        "HOST_INTERFERENCE_DETECTED",
        // 无可信 final 只能公开未知结果。
        "OUTCOME_UNKNOWN",
    ] {
        // 每个矩阵错误码都必须已登记。
        assert!(error_codes.contains(&json!(code)));
        // 结束公开错误码登记核对。
    }
    // 结果 Schema 不得出现私有 transport 字段。
    assert!(
        ![
            "brokerEpoch",
            "requestNonce",
            "requestRevision",
            "pipeName",
            "providerId"
        ]
        .iter()
        .any(|field| RESULT_SCHEMA.contains(field))
    );
    // 结束 Schema 事实测试。
}

// 验证 open/close 正例与关键反例的封闭结构。
#[test]
fn examples_keep_branches_exact_and_private_fields_closed() {
    // 构造合法 open success。
    let open = json!({"executionRealm":"isolated-worker","requiredExecutionRealm":"isolated-worker","executionRealmCertified":true,"isolationRequirement":"standard","hostImpactPolicy":"background-preferred","ok":true,"app":"app","verb":"create","capability":"browser.session.open@1","targetId":HOST_ID,"data":open_data(),"meta":{"foreground":{"unchanged":true},"targeting":"opaque exact session"}});
    // 合法 open 必须通过冻结形状。
    assert!(success_shape(&open));
    // 构造合法 close success。
    let close = json!({"executionRealm":"isolated-worker","requiredExecutionRealm":"isolated-worker","executionRealmCertified":true,"isolationRequirement":"strict","hostImpactPolicy":"strict-no-interference","ok":true,"app":"app","verb":"close","capability":"browser.session.close@1","targetId":SESSION_ID,"data":close_data(),"meta":{"foreground":{"unchanged":true},"targeting":"opaque exact session"}});
    // 合法 close 必须通过冻结形状。
    assert!(success_shape(&close));
    // 复制 open 以构造错误 target 种类。
    let mut wrong_target = open.clone();
    // open 不得把 browser session 当作原 target。
    wrong_target["targetId"] = json!(SESSION_ID);
    // 错误 target 必须失败闭合。
    assert!(!success_shape(&wrong_target));
    // 复制 close 以构造重复 sessionId。
    let mut duplicate_session = close.clone();
    // 故意向 close data 添加禁止字段。
    duplicate_session["data"]["sessionId"] = json!(SESSION_ID);
    // close data 的额外字段必须被拒绝。
    assert!(!success_shape(&duplicate_session));
    // 复制 open 以构造私有 provider 泄漏。
    let mut leaked_provider = open;
    // 故意在顶层添加私有路由键。
    leaked_provider["providerId"] = json!("private-browser-broker");
    // 私有路由事实必须被封闭对象拒绝。
    assert!(!success_shape(&leaked_provider));
    // 复制 close 以构造交叉隔离矩阵。
    let mut crossed_isolation = close;
    // strict 不得搭配后台优先策略。
    crossed_isolation["hostImpactPolicy"] = json!("background-preferred");
    // 交叉隔离矩阵必须被拒绝。
    assert!(!success_shape(&crossed_isolation));
    // 结束正反例结构测试。
}

// 验证统一 error envelope 的业务接受真值矩阵。
#[test]
fn error_envelope_freezes_acceptance_truth_without_identity_guessing() {
    // 构造明确在接受前 stale 的 close 错误。
    let preaccept = json!({"ok":false,"error":{"code":"STALE_SESSION","message":"session is stale","details":{"capability":"browser.session.close@1","targetId":SESSION_ID,"outcome":"not-dispatched","accepted":false,"finalStateReached":true,"retrySafe":true,"targetMayHaveMutated":false}}});
    // 明确未派发错误必须通过冻结矩阵。
    assert!(lifecycle_error_shape(&preaccept));
    // 复制未派发错误以核对 timeout。
    let mut timeout = preaccept.clone();
    // timeout 在明确接受前仍保持相同真值。
    timeout["error"]["code"] = json!("TIMEOUT");
    // 接受前 timeout 必须通过冻结矩阵。
    assert!(lifecycle_error_shape(&timeout));
    // 复制未派发错误以核对 cancel。
    let mut cancelled = preaccept;
    // cancel 在明确接受前仍保持相同真值。
    cancelled["error"]["code"] = json!("CANCELLED");
    // 接受前 cancel 必须通过冻结矩阵。
    assert!(lifecycle_error_shape(&cancelled));
    // 构造 confirmation-first 且尚未提供 target 的未派发错误。
    let confirmation = json!({"ok":false,"error":{"code":"CONFIRMATION_REQUIRED","message":"confirmation required","details":{"capability":"browser.session.close@1","outcome":"not-dispatched","accepted":false,"finalStateReached":true,"retrySafe":true,"targetMayHaveMutated":false}}});
    // 缺失 target 时不得捏造 identity，但矩阵仍必须成立。
    assert!(lifecycle_error_shape(&confirmation));
    // 构造畸形 target 已在 provider 前收敛后的输入错误。
    let invalid = json!({"ok":false,"error":{"code":"INVALID_ARGUMENT","message":"invalid target","details":{"capability":"browser.session.close@1","outcome":"not-dispatched","accepted":false,"finalStateReached":true,"retrySafe":true,"targetMayHaveMutated":false}}});
    // 畸形 target 必须省略 targetId 并保持未派发真值。
    assert!(lifecycle_error_shape(&invalid));
    // 构造 accepted 后取得可信失败 final 的 close 错误。
    let failed = json!({"ok":false,"error":{"code":"OPERATION_FAILED","message":"operation failed","details":{"capability":"browser.session.close@1","targetId":SESSION_ID,"outcome":"failed","accepted":true,"finalStateReached":true,"retrySafe":false,"targetMayHaveMutated":true,"automaticRetryProhibited":true}}});
    // 可信失败不得被洗成未派发或 unknown。
    assert!(lifecycle_error_shape(&failed));
    // 构造 accepted 后缺少可信 final 的 open 错误。
    let unknown = json!({"ok":false,"error":{"code":"OUTCOME_UNKNOWN","message":"final state is unknown","details":{"capability":"browser.session.open@1","targetId":HOST_ID,"outcome":"unknown","accepted":true,"finalStateReached":false,"retrySafe":false,"targetMayHaveMutated":true,"automaticRetryProhibited":true}}});
    // open unknown 必须只保留原 host target。
    assert!(lifecycle_error_shape(&unknown));
    // 复制 unknown 以构造伪造新 session identity。
    let mut guessed_session = unknown.clone();
    // open unknown 不得猜测 sessionId。
    guessed_session["error"]["details"]["sessionId"] = json!(SESSION_ID);
    // 伪造身份必须被封闭详情拒绝。
    assert!(!lifecycle_error_shape(&guessed_session));
    // 复制 unknown 以构造错误的 final 事实。
    let mut false_final = unknown;
    // 缺少可信 final 不得声称完成。
    false_final["error"]["details"]["finalStateReached"] = json!(true);
    // 伪造完成必须被拒绝。
    assert!(!lifecycle_error_shape(&false_final));
    // 结束错误真值矩阵测试。
}
