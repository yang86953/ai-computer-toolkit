//! 拥有公开长操作 status/await Query 与幂等 cancel Command 语义。

// 导入有界轮询与单调 deadline 工具。
use std::{
    // 在两次只读状态查询之间执行短等待。
    thread,
    // 使用单调时钟和固定轮询间隔。
    time::{Duration, Instant},
};

// 导入 provider-neutral JSON 值与构造宏。
use serde_json::{Value, json};

// 导入固定 broker Adapter、协议 Component 与统一错误。
use crate::{
    // 只通过固定 Windows Adapter 触碰进程与 pipe。
    adapters::long_operation_broker_windows,
    // 读取首个异步 capability 的稳定 ID。
    capabilities,
    // 导入请求构造、响应解析与系统随机 nonce。
    components::{
        // 解析 acceptance-aware broker response。
        long_operation_broker_response::{LongOperationClientResponse, parse_response},
        // 构造字段封闭请求。
        long_operation_protocol::LongOperationBrokerRequest,
        // 生成不可预测关联值。
        secure_nonce_windows::random_nonce,
    },
    // 导入统一错误与结果。
    domain::{AppControlError, AppResult},
};

// 固定 await 的只读状态轮询间隔。
const AWAIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

// 提交首个异步窗口录制 Command。
pub(crate) fn start(
    // 接收固定版本化 capability。
    capability_id: &str,
    // 接收 canonical opaque 窗口目标。
    session_id: &str,
    // 接收完整 provider-neutral 输入。
    input: &Value,
    // 接收逐操作显式确认。
    confirmed: bool,
    // 接收总 transport 预算。
    timeout_ms: u32,
) -> AppResult<Value> {
    // 客户端只发布首个冻结异步 capability。
    if capability_id != capabilities::WINDOW_RECORD {
        // 未知 capability 在启动 broker 前失败。
        return Err(AppControlError::new(
            // 使用公共 capability 缺口码。
            "CAPABILITY_GAP",
            // 不回显任意 capability 文本。
            "The requested long operation capability is unavailable.",
        ));
    }
    // 确认必须先于 target 与 input 字段语义。
    if !confirmed {
        // 未确认请求不得启动或连接 broker。
        return Err(AppControlError::new(
            // 使用独立确认错误码。
            "CONFIRMATION_REQUIRED",
            // 不回显 target 或 input。
            "The long operation submission requires explicit confirmation.",
        ));
    }
    // 生成一次性请求关联值。
    let request_nonce = random_nonce()?;
    // 在启动 broker 前执行确认、target 与 input 字段门禁。
    let request = LongOperationBrokerRequest::submit_window_record(
        // 绑定本次请求 nonce。
        request_nonce.clone(),
        // 复制 canonical 窗口目标。
        session_id.to_owned(),
        // 复制完整有界输入。
        input.clone(),
        // 传入逐操作确认事实。
        confirmed,
    )
    // 构造失败不得触碰 endpoint。
    .ok_or_else(|| {
        AppControlError::new(
            // 使用公共参数错误码。
            "INVALID_ARGUMENT",
            // 不回显 target 或 input。
            "The long operation submission requires confirmation, a canonical window target, and an object input.",
        )
    })?;
    // submit 在写入开始后绝不自动重试。
    let text = long_operation_broker_windows::exchange_submit(&request, timeout_ms)?;
    // 严格绑定响应并投影公共结果。
    project_response(&text, &request_nonce)
}

// 执行无副作用 status Query。
pub(crate) fn status(operation_id: &str, timeout_ms: u32) -> AppResult<Value> {
    // 委托共同入口并选择 status 构造器。
    invoke(operation_id, timeout_ms, LongOperationBrokerRequest::status)
}

// 在单调总 deadline 内等待长操作进入权威终态。
pub(crate) fn await_terminal(operation_id: &str, timeout_ms: u32) -> AppResult<Value> {
    // 复用现有 nonce-bound status Query，不新增 broker action。
    await_with_status(operation_id, timeout_ms, |remaining_timeout_ms| {
        // 每次查询只能消费当前总 deadline 的剩余预算。
        status(operation_id, remaining_timeout_ms)
    })
}

// 执行幂等 cancel Command。
pub(crate) fn cancel(operation_id: &str, timeout_ms: u32) -> AppResult<Value> {
    // 委托共同入口并选择 cancel 构造器。
    invoke(operation_id, timeout_ms, LongOperationBrokerRequest::cancel)
}

// 构造、投递并投影一个 handle-only broker 请求。
fn invoke(
    // 接收公开 operation handle。
    operation_id: &str,
    // 接收总 transport 预算。
    timeout_ms: u32,
    // 接收封闭请求构造器。
    build_request: fn(String, String) -> Option<LongOperationBrokerRequest>,
) -> AppResult<Value> {
    // 生成一次性请求关联值。
    let request_nonce = random_nonce()?;
    // 在任何 broker 启动前验证 handle 与请求字段组合。
    let request =
        build_request(request_nonce.clone(), operation_id.to_owned()).ok_or_else(|| {
            // 返回普通参数错误且不触碰 endpoint。
            AppControlError::new(
                // 使用公共参数错误码。
                "INVALID_ARGUMENT",
                // 不回显非法 handle。
                "The long operation request requires a canonical operation handle.",
            )
        })?;
    // 通过固定 Adapter 投递同一幂等逻辑请求。
    let text = long_operation_broker_windows::exchange(&request, timeout_ms)?;
    // 严格绑定响应并投影公共结果。
    project_response(&text, &request_nonce)
}

// 构造只终止 waiter 且不改变 operation 的有界等待超时。
fn await_timeout(operation_id: &str, last_observed_status: Option<&str>) -> AppControlError {
    // 返回稳定 Query timeout，不伪造任务失败或取消。
    AppControlError::with_details(
        // 复用公共有界等待错误码。
        "TIMEOUT",
        // 明确 deadline 只属于 await 调用。
        "The long operation did not reach a terminal state before the await deadline.",
        // 只投影调用方已知 handle 与最小等待事实。
        json!({
            // 回显调用方已经提供的 opaque operation handle。
            "operationId": operation_id,
            // 保存最后一次权威非终态；尚未观察时为 null。
            "lastObservedStatus": last_observed_status,
            // await 没有取得终态。
            "terminal": false,
            // 原任务继续由 broker 拥有。
            "operationContinues": true,
            // await timeout 绝不发送 cancel。
            "operationCancelled": false,
            // 所有轮询都是无副作用 Query。
            "statusQueriesAreReadOnly": true
        }),
    )
}

// 通过可替换的权威 status Query 执行有界终态等待。
fn await_with_status<F>(
    // 借用公开 operation handle。
    operation_id: &str,
    // 接收完整 await 总预算。
    timeout_ms: u32,
    // 接收每轮剩余预算内的 status Query。
    mut query: F,
) -> AppResult<Value>
where
    // Query 可以维护测试私有观察状态，但不得改变 operation。
    F: FnMut(u32) -> AppResult<Value>,
{
    // 零预算不得查询或启动 broker。
    if timeout_ms == 0 {
        // 返回普通参数错误。
        return Err(AppControlError::new(
            // 使用公共参数错误码。
            "INVALID_ARGUMENT",
            // 不回显 handle。
            "The long operation await timeout must be positive.",
        ));
    }
    // 冻结覆盖全部 status Query 与等待间隔的单调总 deadline。
    let deadline = Instant::now()
        // 转换调用方毫秒预算。
        .checked_add(Duration::from_millis(u64::from(timeout_ms)))
        // 理论溢出失败闭合且不查询 broker。
        .ok_or_else(|| {
            AppControlError::new(
                // 使用稳定 broker 不可用错误码。
                "BROKER_UNAVAILABLE",
                // 不公开时钟或平台细节。
                "The long operation await deadline is unavailable.",
            )
        })?;
    // 在首次权威状态前不猜测任务阶段。
    let mut last_observed_status: Option<String> = None;
    // 每轮只执行一条无副作用 status Query。
    loop {
        // 计算当前剩余总预算。
        let remaining = deadline.saturating_duration_since(Instant::now());
        // deadline 已耗尽时只终止 waiter。
        if remaining.is_zero() {
            // 保留最后权威非终态供调用方判断。
            return Err(await_timeout(
                // 回显公开 handle。
                operation_id,
                // 借用最后状态文本。
                last_observed_status.as_deref(),
            ));
        }
        // 转换为毫秒整数预算，并把亚毫秒尾差收敛为一毫秒。
        let remaining_timeout_ms = u32::try_from(remaining.as_millis().max(1))
            // 公开 timeout 为 u32，上溢时使用其最大值。
            .unwrap_or(u32::MAX);
        // 执行一次 nonce-bound 权威状态查询。
        let result = match query(remaining_timeout_ms) {
            // 保留完整权威状态。
            Ok(result) => result,
            // transport 恰好耗尽 await 总预算时使用 waiter timeout 语义。
            Err(error)
                if matches!(error.code, "TIMEOUT" | "BROKER_UNAVAILABLE")
                    // 只有同一总 deadline 确实到达才重映射。
                    && Instant::now() >= deadline =>
            {
                // 不把 transport 等待失败误写为 operation 终态。
                return Err(await_timeout(
                    // 回显公开 handle。
                    operation_id,
                    // 附加最后权威状态。
                    last_observed_status.as_deref(),
                ));
            }
            // 参数、零命中、权限、取消和 deadline 前 transport 错误原样传播。
            Err(error) => return Err(error),
        };
        // 读取已由 status response Component 验证的 operation 对象。
        let operation = result
            // 读取统一成功 envelope。
            .get("operation")
            // 要求公开状态对象。
            .and_then(Value::as_object)
            // 理论漂移失败闭合且不返回部分状态。
            .ok_or_else(|| {
                AppControlError::new(
                    // 使用稳定 broker 不可用错误码。
                    "BROKER_UNAVAILABLE",
                    // 不回显响应内容。
                    "The long operation await query returned an invalid status.",
                )
            })?;
        // 读取权威 terminal 标志。
        let terminal = operation
            // 访问冻结字段。
            .get("terminal")
            // 要求布尔值。
            .and_then(Value::as_bool)
            // 缺失表示协议漂移。
            .ok_or_else(|| {
                AppControlError::new(
                    // 使用稳定 broker 不可用错误码。
                    "BROKER_UNAVAILABLE",
                    // 不回显响应内容。
                    "The long operation await query omitted its terminal state.",
                )
            })?;
        // 已观察终态时原样返回 status 权威对象。
        if terminal {
            // 不重写 completed、failed 或 outcome-unknown。
            return Ok(result);
        }
        // 保存最后一次权威非终态文本。
        last_observed_status = operation
            // 读取冻结状态字段。
            .get("status")
            // 只接受字符串。
            .and_then(Value::as_str)
            // 建立独立所有权跨过本轮 result。
            .map(str::to_owned);
        // 状态查询结束后重新扣减同一总 deadline。
        let remaining = deadline.saturating_duration_since(Instant::now());
        // deadline 已耗尽时不再查询。
        if remaining.is_zero() {
            // 返回只终止 waiter 的 timeout。
            return Err(await_timeout(
                // 回显公开 handle。
                operation_id,
                // 附加最后权威状态。
                last_observed_status.as_deref(),
            ));
        }
        // 只休眠固定间隔或剩余预算中的较小值。
        thread::sleep(AWAIT_POLL_INTERVAL.min(remaining));
    }
}

// 解析 nonce-bound broker 响应并投影统一结果。
fn project_response(
    // 借用唯一响应文本。
    text: &str,
    // 借用本次请求 nonce。
    request_nonce: &str,
) -> AppResult<Value> {
    // 严格绑定版本、字段与本次 nonce。
    let response = parse_response(text, request_nonce).map_err(|_| {
        // 协议漂移不得向调用方返回部分状态。
        AppControlError::new(
            // 使用 broker 不可用错误码。
            "BROKER_UNAVAILABLE",
            // 不回显响应内容。
            "The long operation broker returned an invalid response.",
        )
    })?;
    // 按业务接受边界投影公共结果。
    match response {
        // 成功返回完整公开 operation 状态。
        LongOperationClientResponse::Success(operation) => Ok(json!({
            // 标记 CLI 调用成功。
            "ok": true,
            // 保留完整公开状态对象。
            "operation": operation
        })),
        // 业务接受前拒绝映射为统一错误 envelope。
        LongOperationClientResponse::Rejected { code, message } => {
            // 把 parser 已封闭的动态文本恢复为静态公共错误码。
            let code = match code.as_str() {
                // 参数错误。
                "INVALID_ARGUMENT" => "INVALID_ARGUMENT",
                // 确认错误。
                "CONFIRMATION_REQUIRED" => "CONFIRMATION_REQUIRED",
                // 容量错误。
                "OPERATION_CAPACITY_EXHAUSTED" => "OPERATION_CAPACITY_EXHAUSTED",
                // 零命中错误。
                "OPERATION_NOT_FOUND" => "OPERATION_NOT_FOUND",
                // 结果预算错误。
                "OPERATION_RESULT_TOO_LARGE" => "OPERATION_RESULT_TOO_LARGE",
                // 权限门禁错误。
                "PERMISSION_DENIED" => "PERMISSION_DENIED",
                // capability 或固定 worker 缺口。
                "CAPABILITY_GAP" => "CAPABILITY_GAP",
                // broker 生命周期错误与理论防御分支。
                _ => "BROKER_UNAVAILABLE",
            };
            // 保留 broker 的稳定错误码与安全消息。
            Err(AppControlError::with_details(
                // 传播静态错误码。
                code,
                // 传播安全消息。
                message,
                // 明确接受阶段而不泄漏 transport 细节。
                json!({
                    // broker 已接受固定 envelope。
                    "transportAccepted": true,
                    // 未建立新的业务事实。
                    "businessAccepted": false
                }),
            ))
        }
    }
}

// 验证 start 的 capability 与确认门禁不会触碰 broker。
#[cfg(test)]
mod start_tests {
    // 导入被测 start 入口。
    use super::start;
    // 导入受控 JSON fixture。
    use serde_json::json;

    // 验证未知 capability 在任何 endpoint 操作前失败。
    #[test]
    fn unknown_start_capability_is_rejected_locally() {
        // 使用故意无效的 target，证明 capability 门禁优先。
        let error = start(
            // 使用未登记 capability。
            "window.unknown@1",
            // 注入非 canonical target。
            "not-a-target",
            // 使用对象输入。
            &json!({}),
            // 显式确认。
            true,
            // 使用正常 deadline。
            5_000,
        )
        // 必须在连接 broker 前失败。
        .err()
        // 意外成功时明确测试失败。
        .unwrap_or_else(|| panic!("unknown start capability unexpectedly succeeded"));
        // 保留 capability 缺口语义。
        assert_eq!(error.code, "CAPABILITY_GAP");
    }

    // 验证未确认 submit 在 endpoint 操作前失败。
    #[test]
    fn unconfirmed_start_is_rejected_locally() {
        // 构造未确认但其余字段合法的请求。
        let error = start(
            // 使用首个冻结 capability。
            "window.record@1",
            // 使用 canonical 窗口目标。
            "s2:w:0000000000000001",
            // 使用对象输入。
            &json!({}),
            // 明确不确认。
            false,
            // 使用正常 deadline。
            5_000,
        )
        // 必须在连接 broker 前失败。
        .err()
        // 意外成功时明确测试失败。
        .unwrap_or_else(|| panic!("unconfirmed start unexpectedly succeeded"));
        // client 入口保留独立确认错误语义。
        assert_eq!(error.code, "CONFIRMATION_REQUIRED");
    }
}

// 验证 await 只读取状态、共享总 deadline 且不伪造取消。
#[cfg(test)]
mod await_tests {
    // 导入被测有界等待器与 JSON 构造宏。
    use super::{await_with_status, json};

    // 构造最小权威状态成功 envelope。
    fn status(state: &str, terminal: bool) -> serde_json::Value {
        // 只提供 await 读取的公开字段。
        json!({
            // 标记 status Query 成功。
            "ok": true,
            // 提供权威 operation 状态。
            "operation": {
                // 保存当前状态。
                "status": state,
                // 保存终态事实。
                "terminal": terminal
            }
        })
    }

    // 验证终态由 await 原样返回且只查询一次。
    #[test]
    fn terminal_status_returns_without_mutation() {
        // 记录 status Query 次数。
        let mut calls = 0_u32;
        // 执行固定终态查询。
        let result = await_with_status(
            // 使用 canonical operation handle。
            "s2:o:0000000000000001",
            // 使用正常总 deadline。
            1_000,
            // 返回完成终态。
            |_| {
                // 记录唯一查询。
                calls += 1;
                // 返回权威 completed 状态。
                Ok(status("completed", true))
            },
        )
        // 固定 fixture 不得失败。
        .unwrap_or_else(|error| panic!("await terminal fixture failed: {error}"));
        // 终态必须原样保留。
        assert_eq!(result["operation"]["status"], "completed");
        // 不得在终态后继续轮询。
        assert_eq!(calls, 1);
    }

    // 验证多轮 status Query 只能消费递减的同一总预算。
    #[test]
    fn polling_uses_decreasing_remaining_deadline() {
        // 保存每轮传入的剩余预算。
        let mut budgets = Vec::new();
        // 执行一轮 running 后完成的状态序列。
        let result = await_with_status(
            // 使用 canonical operation handle。
            "s2:o:0000000000000002",
            // 为一次短轮询提供充足预算。
            1_000,
            // 依次返回非终态与终态。
            |remaining_timeout_ms| {
                // 保存本轮剩余预算。
                budgets.push(remaining_timeout_ms);
                // 首次返回 running，第二次返回 completed。
                Ok(if budgets.len() == 1 {
                    // 返回非终态。
                    status("running", false)
                } else {
                    // 返回完成终态。
                    status("completed", true)
                })
            },
        )
        // 固定 fixture 不得失败。
        .unwrap_or_else(|error| panic!("await polling fixture failed: {error}"));
        // 最终必须返回 completed。
        assert_eq!(result["operation"]["status"], "completed");
        // 必须执行恰好两次 Query。
        assert_eq!(budgets.len(), 2);
        // 第二轮不能重新获得完整预算。
        assert!(budgets[1] < budgets[0]);
    }

    // 验证 await timeout 只终止 waiter 而不取消任务。
    #[test]
    fn timeout_leaves_operation_running() {
        // 在一毫秒总预算内持续返回 running。
        let error = await_with_status(
            // 使用 canonical operation handle。
            "s2:o:0000000000000003",
            // 使用最小正预算。
            1,
            // 永远返回权威非终态。
            |_| Ok(status("running", false)),
        )
        // 必须以等待 timeout 结束。
        .err()
        // 意外成功时明确失败。
        .unwrap_or_else(|| panic!("await timeout fixture unexpectedly completed"));
        // 使用普通等待 timeout，而不是 OutcomeUnknown。
        assert_eq!(error.code, "TIMEOUT");
        // 原任务必须继续由 broker 拥有。
        assert_eq!(error.details["operationContinues"], true);
        // timeout 绝不伪造取消。
        assert_eq!(error.details["operationCancelled"], false);
        // 保存最后权威状态。
        assert_eq!(error.details["lastObservedStatus"], "running");
    }

    // 验证 status transport 耗尽总预算时仍保持 waiter timeout 语义。
    #[test]
    fn transport_timeout_at_deadline_does_not_cancel_operation() {
        // 让权威 Query 持续到 await 总 deadline 之后。
        let error = await_with_status(
            // 使用 canonical operation handle。
            "s2:o:0000000000000004",
            // 使用最小正预算。
            1,
            // 模拟底层 transport 在 deadline 返回普通 timeout。
            |_| {
                // 跨过本次 await 总 deadline。
                std::thread::sleep(std::time::Duration::from_millis(2));
                // 返回底层 transport timeout。
                Err(crate::domain::AppControlError::new(
                    // 使用底层固定 timeout 码。
                    "TIMEOUT",
                    // 使用不公开 transport 的测试消息。
                    "fixture transport timeout",
                ))
            },
        )
        // 必须以 await timeout 结束。
        .err()
        // 意外成功时明确失败。
        .unwrap_or_else(|| panic!("transport timeout fixture unexpectedly completed"));
        // 仍使用公共 timeout 码。
        assert_eq!(error.code, "TIMEOUT");
        // await 必须明确原任务继续。
        assert_eq!(error.details["operationContinues"], true);
        // await 不得把 transport timeout 变成 cancel。
        assert_eq!(error.details["operationCancelled"], false);
        // 尚未取得权威状态时保持 null。
        assert!(error.details["lastObservedStatus"].is_null());
    }
}
