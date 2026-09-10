//! 解析并路由长操作 start/status/await/cancel CLI。

// 导入 provider-neutral JSON 目标、输入与结果。
use serde_json::{Map, Value};

// 导入 CLI 错误、统一结果与 System。
use crate::{
    // 使用主 CLI 封闭参数错误码。
    cli::error_code::CliErrorCode,
    // 返回统一结果。
    domain::AppResult,
    // 只协调公开长操作入口。
    service::AppControlService,
};

// 路由固定 broker 的长操作 status/await Query 与 cancel Command。
pub(super) fn route(
    // 借用完整位置参数。
    positionals: &[String],
    // 借用原始 option token。
    option_tokens: &[String],
    // 接收已经有界解析的 timeout。
    timeout_ms: u32,
    // 借用已经解析但尚未触碰 provider 的 target。
    target: &Map<String, Value>,
    // 借用已经有界读取的 JSON 输入。
    input: Option<&Value>,
    // 接收逐操作显式确认。
    confirmed: bool,
    // 借用当前 ComputerControlSystem。
    service: &AppControlService,
) -> AppResult<Value> {
    // 所有公开 action 都使用三个位置参数。
    if positionals.len() != 3 {
        // 任意额外输入在接触 broker 前失败闭合。
        return Err(CliErrorCode::InvalidArgument.error(
            // 给出唯一公开用法且不回显输入。
            "operation requires start <capability@version>, status <s2:o:opaque>, await <s2:o:opaque>, or cancel <s2:o:opaque>.",
        ));
    }
    // 借用已由长度门禁证明存在的 action。
    let action = &positionals[1];
    // 借用已由长度门禁证明存在的 operation handle。
    let operation_id = &positionals[2];
    // 按封闭 action 委托 System。
    match action.as_str() {
        // start 是非幂等 submit Command。
        "start" => {
            // 确认必须先于 capability、target 与 input 语义。
            if !confirmed {
                // 使用独立确认错误码。
                return Err(CliErrorCode::ConfirmationRequired.error(
                    // 不回显 target 或 input。
                    "operation start requires --confirm.",
                ));
            }
            // start 只允许 target、input、确认、deadline 与输出格式。
            if !start_options_are_valid(option_tokens) {
                // 在启动 broker 前拒绝未知 option。
                return Err(CliErrorCode::InvalidArgument.error(
                    // 给出封闭 option 集合。
                    "operation start only accepts --target, --input, --confirm, --timeout-ms, or --pretty.",
                ));
            }
            // target 必须严格只有 sessionId。
            if target.len() != 1 {
                // 拒绝 native 或扩展目标字段。
                return Err(CliErrorCode::InvalidArgument.error(
                    // 不回显目标内容。
                    "operation start requires exactly --target sessionId=<s2:w:opaque>.",
                ));
            }
            // 读取唯一 canonical 窗口目标文本。
            let session_id = target
                // 读取公开 sessionId。
                .get("sessionId")
                // 只接受字符串。
                .and_then(Value::as_str)
                // 拒绝空字符串。
                .filter(|value| !value.is_empty())
                // 缺失或类型错误失败闭合。
                .ok_or_else(|| {
                    CliErrorCode::InvalidArgument.error(
                        // 不回显目标内容。
                        "operation start requires exactly --target sessionId=<s2:w:opaque>.",
                    )
                })?;
            // start 必须携带对象输入。
            let input = input
                // 只接受已经有界读取的 JSON object。
                .filter(|value| value.is_object())
                // 缺失或类型错误失败闭合。
                .ok_or_else(|| {
                    CliErrorCode::InvalidArgument.error(
                        // 不回显输入内容或文件路径。
                        "operation start requires --input <file|-> containing a JSON object.",
                    )
                })?;
            // 委托 System 执行非幂等 acceptance-aware submit。
            service.long_operation_start(
                // 第三个位置参数是固定 capability ID。
                operation_id,
                // 传入 canonical opaque 窗口目标。
                session_id,
                // 传入完整领域输入。
                input,
                // 传入已验证确认事实。
                confirmed,
                // 传入总 transport 预算。
                timeout_ms,
            )
        }
        // status 是无副作用 Query。
        "status" if handle_options_are_valid(option_tokens) => {
            // 委托 handle-only Query。
            service.long_operation_status(operation_id, timeout_ms)
        }
        // await 是由有界 status Query 组成的无副作用终态等待。
        "await" if handle_options_are_valid(option_tokens) => {
            // 委托 handle-only 有界 Query。
            service.long_operation_await(operation_id, timeout_ms)
        }
        // cancel 是幂等 Command。
        "cancel" if handle_options_are_valid(option_tokens) => {
            // 委托 handle-only Command。
            service.long_operation_cancel(operation_id, timeout_ms)
        }
        // 未知 action 不触碰 broker。
        _ => Err(CliErrorCode::InvalidArgument.error(
            // 不接受宽松别名或 option 扩张。
            "operation only accepts start, status, await, or cancel with their closed option sets.",
        )),
    }
}

// 验证 handle-only 命令不携带 target、input、确认或启动参数。
fn handle_options_are_valid(tokens: &[String]) -> bool {
    // 从首个 option 开始顺序扫描。
    let mut index = 0;
    // 每次消费一个 flag 或一个 flag/value 对。
    while index < tokens.len() {
        // 只允许纯输出格式与有界 transport deadline。
        match tokens[index].as_str() {
            // pretty 不携带值。
            "--pretty" => index += 1,
            // timeout 必须有一个已由主 parser 验证的值。
            "--timeout-ms" if index + 1 < tokens.len() => index += 2,
            // 任何其他 option 都失败闭合。
            _ => return false,
        }
    }
    // 全部 token 均属于封闭集合。
    true
}

// 验证 start 只携带封闭 submit option 集合。
fn start_options_are_valid(tokens: &[String]) -> bool {
    // 从首个 option 开始顺序扫描。
    let mut index = 0;
    // 跟踪 target 是否已经出现。
    let mut target_seen = false;
    // 跟踪 input 是否已经出现。
    let mut input_seen = false;
    // 跟踪确认是否已经出现。
    let mut confirm_seen = false;
    // 跟踪 deadline 是否已经出现。
    let mut timeout_seen = false;
    // 跟踪输出格式是否已经出现。
    let mut pretty_seen = false;
    // 每次消费一个 flag 或一个 flag/value 对。
    while index < tokens.len() {
        // 只允许 submit 所需字段、输出格式与总 deadline。
        match tokens[index].as_str() {
            // pretty 只允许出现一次且不携带值。
            "--pretty" if !pretty_seen => {
                // 记录唯一出现。
                pretty_seen = true;
                // 消费当前 flag。
                index += 1;
            }
            // confirm 只允许出现一次且不携带值。
            "--confirm" if !confirm_seen => {
                // 记录唯一出现。
                confirm_seen = true;
                // 消费当前 flag。
                index += 1;
            }
            // target 必须唯一且携带一个值。
            "--target" if !target_seen && index + 1 < tokens.len() => {
                // 记录唯一出现。
                target_seen = true;
                // 消费 flag 与值。
                index += 2;
            }
            // input 必须唯一且携带一个值。
            "--input" if !input_seen && index + 1 < tokens.len() => {
                // 记录唯一出现。
                input_seen = true;
                // 消费 flag 与值。
                index += 2;
            }
            // timeout 必须唯一且携带一个值。
            "--timeout-ms" if !timeout_seen && index + 1 < tokens.len() => {
                // 记录唯一出现。
                timeout_seen = true;
                // 消费 flag 与值。
                index += 2;
            }
            // 任何其他 option 都失败闭合。
            _ => return false,
        }
    }
    // 必需 option 各出现一次且全部 token 均已消费。
    target_seen && input_seen && confirm_seen
}
