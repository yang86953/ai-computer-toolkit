//! 组合策略预检、重新定位与 Job worker，执行 provider-neutral 语义元素动作。

// 把错误集合保留为当前 Module 私有类型。
#[path = "semantic_action_error.rs"]
mod error_code;

// 导入 sibling 路径与 deadline 类型。
use std::{
    // 导入 worker 路径。
    path::PathBuf,
    // 导入单调 deadline。
    time::Duration,
};

// 导入严格输入反序列化与 JSON 类型。
use serde_json::{Map, Value, json};

// 导入窗口、capability、契约与 worker Component。
use crate::{
    // 导入窗口重新发现与前景 token。
    adapters::{
        // 导入精确窗口只读边界。
        window::{capture_visible_titled_windows, resolve_window},
        // 导入私有前景身份。
        windows::foreground_hwnd,
    },
    // 导入 capability 单一注册表。
    capabilities,
    // 导入共享输入与 worker Component。
    components::{
        // 导入控制台取消状态。
        cancellation,
        // 导入 provider-neutral 输入契约。
        semantic_action_contract::{SemanticActionInput, SemanticActionInputError},
        // 导入 Job-bounded worker 原语。
        worker_process::{WorkerOutput, run_companion, sibling_companion_path},
    },
    // 导入统一错误与结果类型。
    domain::{AppControlError, AppResult},
    // 导入只读 capability assessment Query。
    modules::capability_assessment,
};

// 导入当前 Module 私有错误类型。
use error_code::SemanticActionErrorCode;

// 固定 semantic action worker 协议版本。
const WORKER_CONTRACT: &str = "act/semantic-action-worker/v1";
// 固定 Rust companion 文件名。
const WORKER_FILE_NAME: &str = "ai-computer-toolkit-semantic-action-worker.exe";
// 限制 worker 结果大小。
const MAXIMUM_OUTPUT_BYTES: usize = 64 * 1024;

// 定位与当前生产 launcher 同目录的固定 companion。
fn worker_path() -> AppResult<PathBuf> {
    // 复用唯一 sibling companion 定位 Component。
    sibling_companion_path(
        // 只允许固定 Rust worker 文件名。
        WORKER_FILE_NAME,
        // 使用不含本机路径的描述。
        "Rust semantic action",
    )
}

// 严格解析并验证公开输入。
fn parse_input(value: &Value) -> AppResult<SemanticActionInput> {
    // 按 deny_unknown_fields 契约反序列化。
    let input = serde_json::from_value::<SemanticActionInput>(value.clone()).map_err(|_| {
        // 不回显 selector 或 Value 文本。
        SemanticActionErrorCode::InvalidArgument.error(
            // 返回固定 schema 诊断。
            "Semantic action input violates schema://ui/element-action/v1.",
        )
    })?;
    // 执行共享硬边界验证。
    if let Err(error) = input.validate() {
        // 将封闭类别映射为安全消息。
        let message = match error {
            // selector 具体内容不得回显。
            SemanticActionInputError::InvalidSelector => {
                // 返回共享 selector 语义。
                "Semantic action selector requires bounded provider-neutral exact-match fields."
            }
            // 说明查询边界。
            SemanticActionInputError::InvalidBounds => {
                // 返回固定范围。
                "Semantic action requires maximumDepth 0..20, maximumItems 1..4096, view control|raw, and timeoutMs 1..30000."
            }
            // 说明 Value 资源上限。
            SemanticActionInputError::InvalidValue => {
                // 不回显原文本。
                "Semantic value action text must not exceed 65536 UTF-8 bytes."
            }
            // 说明 Scroll 必须实际请求变化。
            SemanticActionInputError::EmptyScroll => {
                // 返回封闭滚动诊断。
                "Semantic scroll action requires at least one non-zero relative amount."
            }
        };
        // 返回 Module 自有参数错误。
        return Err(SemanticActionErrorCode::InvalidArgument.error(message));
    }
    // 返回已验证输入。
    Ok(input)
}

// 核对 assessment 在 dispatch 前允许已确认 mutation。
fn permission_preflight(session_id: &str) -> AppResult<()> {
    // 执行只读 capability assessment。
    let assessment = capability_assessment::assess(capabilities::UI_ELEMENT_ACTION, session_id)?;
    // 读取封闭决策。
    let decision = assessment
        // 访问固定字段。
        .get("decision")
        // 要求字符串。
        .and_then(Value::as_str)
        // 缺失视为内部协议失败。
        .ok_or_else(|| {
            // 不回显 assessment 数据。
            SemanticActionErrorCode::WorkerProtocolFailed.error(
                // 说明内部决策缺失。
                "Semantic action permission assessment returned no decision.",
            )
        })?;
    // 权限阻塞必须在 worker 启动前失败。
    if decision == "permission-blocked" {
        // 返回显式权限错误。
        return Err(SemanticActionErrorCode::PermissionDenied.error(
            // 禁止自动提权或替代输入。
            "The exact window is not writable from the current integrity context.",
        ));
    }
    // 当前 provider 不可用不得尝试替代路径。
    if decision == "unavailable" {
        // 返回显式 provider 不可用。
        return Err(SemanticActionErrorCode::AccessibilityUnavailable.error(
            // 禁止静默降级。
            "The exact window provider is currently unavailable for semantic actions.",
        ));
    }
    // 目标或目录不支持时返回 capability 缺口。
    if matches!(decision, "unsupported" | "capability-gap") {
        // 返回稳定 capability 错误。
        return Err(SemanticActionErrorCode::CapabilityUnsupported.error(
            // 指引显式 Workflow 决策。
            "The exact target does not publish the semantic element action capability.",
        ));
    }
    // 已确认 mutation 的静态 assessment 必须正好等待确认。
    if decision != "confirmation-required"
        // 执行域必须与注册表一致。
        || assessment.get("executionRealm").and_then(Value::as_str)
            != Some("same-session-no-focus")
        // mutation 必须声明确认。
        || assessment.get("requiresConfirmation").and_then(Value::as_bool) != Some(true)
        // 该路径不得要求前台输入同意。
        || assessment
            .get("requiresForegroundConsent")
            .and_then(Value::as_bool)
            != Some(false)
        // assessment 必须回显同一 capability。
        || assessment.get("capability").and_then(Value::as_str)
            != Some(capabilities::UI_ELEMENT_ACTION)
        // assessment 必须回显同一 opaque 目标。
        || assessment.get("targetId").and_then(Value::as_str) != Some(session_id)
        // 约束必须禁止 fallback。
        || assessment
            .pointer("/constraints/noFallback")
            .and_then(Value::as_bool)
            != Some(true)
        // mutation 不得误报只读。
        || assessment
            .pointer("/constraints/readOnly")
            .and_then(Value::as_bool)
            != Some(false)
    {
        // 不一致时在 provider 前失败闭合。
        return Err(SemanticActionErrorCode::WorkerProtocolFailed.error(
            // 不回显内部对象。
            "Semantic action permission assessment violated its mutation contract.",
        ));
    }
    // 静态权限、capability 与 realm 门禁通过。
    Ok(())
}

// 从 worker 对象读取必需非空字符串。
fn required_string<'value>(
    // 接收 JSON 对象。
    object: &'value Map<String, Value>,
    // 接收字段名。
    field: &str,
) -> AppResult<&'value str> {
    // 只接受非空字符串。
    object
        // 读取字段。
        .get(field)
        // 要求字符串。
        .and_then(Value::as_str)
        // 拒绝空值。
        .filter(|value| !value.is_empty())
        // 映射协议错误。
        .ok_or_else(|| {
            // 不回显 worker 原始输出。
            SemanticActionErrorCode::WorkerProtocolFailed.error(
                // 指明安全字段名。
                format!("Semantic action worker field '{field}' is missing or invalid."),
            )
        })
}

// 构造 parent 无法确定 worker dispatch 的保守错误。
fn parent_outcome_unknown(action: &str, foreground_unchanged: Option<bool>) -> AppControlError {
    // 返回 D3/R0 保守结果。
    SemanticActionErrorCode::OutcomeUnknown.with_details(
        // 不泄漏进程或协议故障细节。
        "The semantic action worker outcome could not be determined after launch.",
        // 固定不可重试证据。
        json!({
            // 回显公开动作类别。
            "action": action,
            // 明确结果未知。
            "outcome": "unknown",
            // 保守视为可能已接受。
            "dispatchState": "accepted-may-have-occurred",
            // 外部状态可能已改变。
            "acceptedMayHaveOccurred": true,
            // 禁止自动重试。
            "automaticRetryProhibited": true,
            // 没有安全重试证明。
            "retrySafe": false,
            // 保持当前可得的前景证据。
            "foregroundUnchanged": foreground_unchanged,
            // 明确没有静默指针降级。
            "pointerFallbackUsed": false,
        }),
    )
}

// 验证 worker 成功 data 的精确安全形状。
fn success_data(data: &Value, input: &SemanticActionInput) -> AppResult<Value> {
    // 要求对象。
    let object = data.as_object().ok_or_else(|| {
        // worker 可能已 dispatch，协议失败由调用方升级为 unknown。
        SemanticActionErrorCode::WorkerProtocolFailed.error(
            // 不回显原始数据。
            "Semantic action worker data must be an object.",
        )
    })?;
    // 固定完整字段集合。
    let fields = [
        // 动作类别。
        "action",
        // 完成结果。
        "outcome",
        // dispatch 阶段。
        "dispatchState",
        // 接受可能性。
        "acceptedMayHaveOccurred",
        // 自动重试策略。
        "automaticRetryProhibited",
        // 重试安全性。
        "retrySafe",
        // 窗口重解析。
        "windowReResolved",
        // 元素重解析。
        "elementReResolved",
        // selector 语义。
        "selectorSemantics",
        // 有界访问数。
        "visited",
        // 指针 fallback。
        "pointerFallbackUsed",
        // 写调用事实。
        "writePerformed",
    ];
    // 字段集合必须精确匹配。
    if object.len() != fields.len()
        // 每个允许字段必须存在。
        || !fields.iter().all(|field| object.contains_key(*field))
        // 动作必须与调用方一致。
        || object.get("action").and_then(Value::as_str) != Some(input.action.as_str())
        // 只接受明确完成。
        || object.get("outcome").and_then(Value::as_str) != Some("completed")
        // dispatch 必须完成。
        || object.get("dispatchState").and_then(Value::as_str) != Some("completed")
        // mutation 必须已可能发生。
        || object
            .get("acceptedMayHaveOccurred")
            .and_then(Value::as_bool)
            != Some(true)
        // mutation 禁止自动重试。
        || object
            .get("automaticRetryProhibited")
            .and_then(Value::as_bool)
            != Some(true)
        // mutation 不得声称重试安全。
        || object.get("retrySafe").and_then(Value::as_bool) != Some(false)
        // 两级目标都必须重新解析。
        || object.get("windowReResolved").and_then(Value::as_bool) != Some(true)
        // element 必须从 selector 重解析。
        || object.get("elementReResolved").and_then(Value::as_bool) != Some(true)
        // 只允许精确 AND 语义。
        || object.get("selectorSemantics").and_then(Value::as_str) != Some("exact-and")
        // 访问数量必须位于调用方边界。
        || object
            .get("visited")
            .and_then(Value::as_u64)
            .is_none_or(|visited| {
                // 转换边界并核对正数。
                visited == 0
                    || visited > u64::try_from(input.maximum_items).unwrap_or(u64::MAX)
            })
        // 禁止指针 fallback。
        || object.get("pointerFallbackUsed").and_then(Value::as_bool) != Some(false)
        // 必须明确执行了写调用。
        || object.get("writePerformed").and_then(Value::as_bool) != Some(true)
    {
        // 拒绝原生字段或漂移语义。
        return Err(SemanticActionErrorCode::WorkerProtocolFailed.error(
            // 不回显 worker data。
            "Semantic action worker data violated the completion contract.",
        ));
    }
    // 返回安全副本。
    Ok(data.clone())
}

// 解析 worker envelope 并返回成功 data 或白名单错误。
fn worker_data(output: WorkerOutput, input: &SemanticActionInput) -> AppResult<Value> {
    // 要求顶层对象。
    let envelope = output.envelope.as_object().ok_or_else(|| {
        // 返回内部协议错误。
        SemanticActionErrorCode::WorkerProtocolFailed.error(
            // 不回显原始输出。
            "Semantic action worker result must be an object.",
        )
    })?;
    // 顶层只允许固定三字段成功或失败形状。
    if envelope.get("contractVersion").and_then(Value::as_str) != Some(WORKER_CONTRACT) {
        // 拒绝未知版本。
        return Err(SemanticActionErrorCode::WorkerProtocolFailed.error(
            // 不协商未知版本。
            "Semantic action worker contractVersion is invalid.",
        ));
    }
    // 读取成功标志。
    let ok = envelope.get("ok").and_then(Value::as_bool).ok_or_else(|| {
        // 返回协议错误。
        SemanticActionErrorCode::WorkerProtocolFailed.error(
            // 不回显原值。
            "Semantic action worker ok flag is invalid.",
        )
    })?;
    // 成功 envelope 必须对应零退出码。
    if ok {
        // 核对顶层字段与退出码。
        if output.exit_code != 0
            // 成功仅允许 data 字段。
            || envelope.len() != 3
            // data 必须存在。
            || !envelope.contains_key("data")
        {
            // 返回协议失败。
            return Err(SemanticActionErrorCode::WorkerProtocolFailed.error(
                // 不公开退出码。
                "Semantic action worker success envelope is inconsistent.",
            ));
        }
        // 验证成功 data。
        return success_data(&envelope["data"], input);
    }
    // 失败 envelope 必须对应非零退出码和固定字段。
    if output.exit_code == 0 || envelope.len() != 3 || !envelope.contains_key("error") {
        // 返回协议失败。
        return Err(SemanticActionErrorCode::WorkerProtocolFailed.error(
            // 不公开退出码。
            "Semantic action worker failure envelope is inconsistent.",
        ));
    }
    // 要求结构化 error 对象。
    let error = envelope
        // 读取 error 字段。
        .get("error")
        // 要求对象。
        .and_then(Value::as_object)
        // 映射协议失败。
        .ok_or_else(|| {
            // 不回显 worker 内容。
            SemanticActionErrorCode::WorkerProtocolFailed.error(
                // 说明形状错误。
                "Semantic action worker error envelope is invalid.",
            )
        })?;
    // 错误对象固定三字段。
    if error.len() != 3
        || !["code", "message", "details"]
            .iter()
            .all(|field| error.contains_key(*field))
    {
        // 拒绝额外 provider 字段。
        return Err(SemanticActionErrorCode::WorkerProtocolFailed.error(
            // 不回显 error。
            "Semantic action worker error fields are invalid.",
        ));
    }
    // 读取稳定错误码。
    let code = required_string(error, "code")?;
    // 读取安全消息。
    let message = required_string(error, "message")?.to_owned();
    // 只允许封闭白名单。
    let public_code = SemanticActionErrorCode::from_worker_code(code).ok_or_else(|| {
        // 未知 worker 错误不得穿过边界。
        SemanticActionErrorCode::WorkerProtocolFailed.error(
            // 不回显未知码。
            "Semantic action worker returned an unknown error code.",
        )
    })?;
    // 读取固定 details。
    let details = error.get("details").cloned().unwrap_or(Value::Null);
    // 只有 OutcomeUnknown 允许非空 details。
    if public_code != SemanticActionErrorCode::OutcomeUnknown {
        // 普通调用前错误必须没有额外 provider 数据。
        if !details.is_null() {
            // 拒绝隐私漂移。
            return Err(SemanticActionErrorCode::WorkerProtocolFailed.error(
                // 不回显 details。
                "Semantic action worker pre-dispatch error contained unexpected details.",
            ));
        }
        // 返回调用前结构化错误。
        return Err(public_code.error(message));
    }
    // OutcomeUnknown 必须严格验证不可重试证据。
    if details.get("action").and_then(Value::as_str) != Some(input.action.as_str())
        // 结果必须未知。
        || details.get("outcome").and_then(Value::as_str) != Some("unknown")
        // dispatch 必须可能已被接受。
        || details.get("dispatchState").and_then(Value::as_str)
            != Some("accepted-may-have-occurred")
        // 保守 mutation 标记必须为真。
        || details
            .get("acceptedMayHaveOccurred")
            .and_then(Value::as_bool)
            != Some(true)
        // 必须禁止自动重试。
        || details
            .get("automaticRetryProhibited")
            .and_then(Value::as_bool)
            != Some(true)
        // 不得声称重试安全。
        || details.get("retrySafe").and_then(Value::as_bool) != Some(false)
        // 不得发生静默指针降级。
        || details.get("pointerFallbackUsed").and_then(Value::as_bool) != Some(false)
        // worker details 必须固定七字段。
        || details.as_object().is_none_or(|object| object.len() != 7)
    {
        // 不确定结果证据漂移仍按 parent 保守 unknown 处理。
        return Err(parent_outcome_unknown(input.action.as_str(), None));
    }
    // 返回已验证 worker 不确定结果。
    Err(public_code.with_details(message, details))
}

// 判断 worker process 错误能否证明 companion 未启动。
fn definitely_not_dispatched(code: &str) -> bool {
    // 只有路径、调用参数与序列化失败发生在进程执行前。
    matches!(
        // 核对稳定 Component 错误码。
        code,
        // 固定三项安全失败。
        "COMPANION_WORKER_UNAVAILABLE" | "INVALID_ARGUMENT" | "SERIALIZATION_FAILED"
    )
}

// 运行固定 worker 并把所有启动后边界失败升级为 OutcomeUnknown。
fn run_worker(input: &SemanticActionInput, session_id: &str) -> AppResult<Value> {
    // 定位固定 sibling worker。
    let worker = worker_path()?;
    // 构造不含 snapshot element ID 的请求。
    let request = json!({
        // 输出内部协议版本。
        "contractVersion": WORKER_CONTRACT,
        // 指定唯一语义写 operation。
        "operation": "semantic-element-action",
        // 只传原 canonical 窗口 ID。
        "sessionId": session_id,
        // 序列化共享 selector。
        "selector": input.selector,
        // 序列化封闭动作。
        "action": input.action,
        // 传入深度边界。
        "maximumDepth": input.maximum_depth,
        // 传入数量边界。
        "maximumItems": input.maximum_items,
        // 传入封闭 view。
        "view": input.view,
    });
    // 在 Job 中运行一次 worker。
    let output = run_companion(
        // 传入固定路径。
        &worker,
        // 生产 worker 不接受命令行参数。
        &[],
        // 传入版本化请求。
        &request,
        // 传入单调 deadline。
        Duration::from_millis(u64::from(input.timeout_ms)),
        // 传入输出硬上限。
        MAXIMUM_OUTPUT_BYTES,
        // 传入取消轮询。
        cancellation::is_cancelled,
    );
    // 将启动后所有 Component 失败保守升级为 OutcomeUnknown。
    let output = match output {
        // 正常取得 worker envelope。
        Ok(output) => output,
        // 进程未执行的三项错误可原样返回。
        Err(error) if definitely_not_dispatched(error.code) => return Err(error),
        // 其他错误无法证明 dispatch 未发生。
        Err(_) => return Err(parent_outcome_unknown(input.action.as_str(), None)),
    };
    // 协议失败也可能发生在完成动作之后。
    worker_data(output, input).map_err(|error| {
        // 保持 worker 自证的白名单业务错误。
        if error.code != SemanticActionErrorCode::WorkerProtocolFailed.as_str() {
            // 原样返回稳定业务或 unknown 错误。
            return error;
        }
        // 协议失败升级为不可重试 unknown。
        parent_outcome_unknown(input.action.as_str(), None)
    })
}

// 为成功结果追加 System 可验证执行证据。
fn public_success(mut data: Value, input: &SemanticActionInput) -> AppResult<Value> {
    // 要求已验证 data 仍为对象。
    let object = data.as_object_mut().ok_or_else(|| {
        // 防御性返回协议错误。
        SemanticActionErrorCode::WorkerProtocolFailed.error(
            // 不回显数据。
            "Semantic action completion data lost its object shape.",
        )
    })?;
    // 加入前景不变量证据。
    object.insert("foregroundUnchanged".to_owned(), Value::Bool(true));
    // 加入静态权限预检证据。
    object.insert(
        // 使用稳定字段。
        "permissionPreflight".to_owned(),
        // 已确认请求通过只读 assessment。
        Value::String("allowed-after-confirmation".to_owned()),
    );
    // 说明 capability 在 dispatch 前已评估。
    object.insert(
        "capabilityEvaluatedBeforeDispatch".to_owned(),
        Value::Bool(true),
    );
    // 说明确认在输入和 provider 前已评估。
    object.insert(
        "confirmationEvaluatedBeforeDispatch".to_owned(),
        Value::Bool(true),
    );
    // 说明前景影响策略在 dispatch 前已评估。
    object.insert(
        "foregroundImpactEvaluatedBeforeDispatch".to_owned(),
        Value::Bool(true),
    );
    // 说明实际 worker 生命周期隔离。
    object.insert(
        // 使用稳定字段。
        "providerTimeoutIsolation".to_owned(),
        // 使用 Job-bounded worker。
        Value::String("job-bounded-worker".to_owned()),
    );
    // 输出强类型执行域。
    object.insert(
        // 使用稳定字段。
        "executionRealm".to_owned(),
        // mutation 影响同一交互会话但不要求焦点。
        Value::String("same-session-no-focus".to_owned()),
    );
    // 明确不是只读动作。
    object.insert("readOnly".to_owned(), Value::Bool(false));
    // 输出实际 parent deadline。
    object.insert(
        // 使用稳定字段。
        "workerTimeoutMs".to_owned(),
        // 转换无符号整数。
        Value::from(input.timeout_ms),
    );
    // 返回扩展后的安全结果。
    Ok(data)
}

// 对精确窗口执行一次已确认语义元素动作。
pub(crate) fn perform(
    // 接收原 canonical 窗口 ID。
    session_id: &str,
    // 接收逐操作确认状态。
    confirmed: bool,
    // 接收公开输入对象。
    value: &Value,
) -> AppResult<Value> {
    // confirmation 必须先于输入、target、assessment 与 worker。
    if !confirmed {
        // 返回统一确认错误。
        return Err(SemanticActionErrorCode::ConfirmationRequired.error(
            // 明确语义动作是 mutation。
            "Semantic element action requires explicit confirmation.",
        ));
    }
    // 严格解析 provider-neutral 输入。
    let input = parse_input(value)?;
    // 在 provider dispatch 前评估 capability、权限与执行域。
    permission_preflight(session_id)?;
    // 记录 Module 执行前私有前景身份。
    let foreground_before = foreground_hwnd();
    // 在 worker 启动前重新发现精确窗口。
    let windows = capture_visible_titled_windows()?;
    // fail closed 核对调用方 opaque 目标仍唯一。
    let _window = resolve_window(session_id, &windows)?;
    // 执行一次 Job-bounded worker。
    let result = run_worker(&input, session_id);
    // 记录 worker 返回或回收后的私有前景身份。
    let foreground_after = foreground_hwnd();
    // Worker 失败时保留业务错误；unknown 追加前景证据。
    let data = match result {
        // 成功 data 继续前景不变量门禁。
        Ok(data) => data,
        // 不确定结果补充可得前景事实。
        Err(error) if error.code == SemanticActionErrorCode::OutcomeUnknown.as_str() => {
            // 复制对象 details 或使用 parent 保守形状。
            let mut details = error.details;
            // 仅对象才能追加证据。
            if let Some(object) = details.as_object_mut() {
                // 写入前景比较结果。
                object.insert(
                    // 使用稳定字段。
                    "foregroundUnchanged".to_owned(),
                    // 比较私有身份但只公开布尔值。
                    Value::Bool(foreground_before == foreground_after),
                );
            }
            // 返回仍不可重试的 unknown。
            return Err(SemanticActionErrorCode::OutcomeUnknown.with_details(
                // 保持安全消息。
                error.message,
                // 返回扩展证据。
                details,
            ));
        }
        // 调用前业务错误可直接返回。
        Err(error) => return Err(error),
    };
    // same-session-no-focus 不允许前景身份变化。
    if foreground_before != foreground_after {
        // 已明确完成 mutation 时返回完成但不可重试的干扰错误。
        return Err(
            SemanticActionErrorCode::HostInterferenceDetected.with_details(
                // 不公开 HWND。
                "Foreground changed during the completed semantic element action.",
                // 保留完成事实并禁止重试。
                json!({
                    // 回显动作类别。
                    "action": input.action.as_str(),
                    // 动作调用已明确完成。
                    "outcome": "completed",
                    // mutation 已发生。
                    "acceptedMayHaveOccurred": true,
                    // 禁止自动重试。
                    "automaticRetryProhibited": true,
                    // 没有安全重试证明。
                    "retrySafe": false,
                    // 前景不变量失败。
                    "foregroundUnchanged": false,
                    // 禁止指针 fallback。
                    "pointerFallbackUsed": false,
                }),
            ),
        );
    }
    // 返回追加 System 门禁证据的完成结果。
    public_success(data, &input)
}

// 把纯输入、assessment 与 worker 协议测试拆出生产文件。
#[cfg(test)]
#[path = "semantic_action_tests.rs"]
mod tests;
