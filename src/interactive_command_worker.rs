//! 实现独立交互会话固定 command worker 的候选 stdio 入口。

// 导入有界标准输入与单行标准输出接口。
use std::io::{Read, Write};

// 导入 provider-neutral JSON 值与构造宏。
use serde_json::{Value, json};

// 导入固定 broker parent 的 Windows 认证边界。
use crate::adapters::fixed_local_ipc_windows::identity::{
    authenticate_peer_process, current_session_id, parent_process_id, sibling_image_path,
};
// 导入协议、结果投影、统一请求与目标会话 System。
use crate::{
    // 导入 command worker 两个窄 Component。
    components::{
        // 导入封闭协议解析与拒绝响应。
        interactive_command_protocol::{
            // 使用固定协议版本。
            CONTRACT_VERSION,
            // 使用封闭协议错误码。
            InteractiveCommandProtocolErrorCode,
            // 解析严格一次性请求。
            InteractiveCommandRequest,
            // 构造统一 dispatch 前拒绝。
            rejection,
        },
        // 导入业务完成、拒绝与 OutcomeUnknown 投影。
        interactive_command_result,
    },
    // 导入目标 session 内层通用请求。
    domain::{CommandRequest, IsolationRequirement, Verb},
    // 导入唯一 ComputerControlSystem 生产协调器。
    service::AppControlService,
};

// 固定允许创建 command worker 的 broker sibling 文件名。
const BROKER_FILE_NAME: &str = "ai-computer-toolkit-interactive-session-broker.exe";

// 限制 command worker 单次请求为 128KiB。
const MAXIMUM_REQUEST_BYTES: u64 = 128 * 1024;

// 构造不依赖请求内容的序列化失败文本。
fn serialization_failure_text() -> String {
    // 返回单行、固定版本且零平台事实的 JSON。
    format!(
        // 固定 protocol error 响应形状。
        "{{\"ok\":false,\"contractVersion\":\"{CONTRACT_VERSION}\",\"transportAccepted\":false,\"businessAccepted\":false,\"completed\":false,\"outcome\":\"not-dispatched\",\"retrySafe\":true,\"targetMayHaveMutated\":false,\"error\":{{\"code\":\"WORKER_PROTOCOL_ERROR\",\"message\":\"The interactive command worker could not serialize its response.\"}},\"evidence\":{{\"localProviderInvoked\":false,\"targetResolved\":false,\"mutationDispatched\":false,\"foregroundFallbackUsed\":false}}}}"
    )
}

// 把纯协议拒绝转换为统一 JSON 值。
fn rejection_value(
    // 接收可选可信请求 nonce。
    request_nonce: Option<&str>,
    // 接收 transport envelope 接受事实。
    transport_accepted: bool,
    // 接收封闭协议错误码。
    code: InteractiveCommandProtocolErrorCode,
) -> Value {
    // 普通固定结构序列化不应失败。
    serde_json::to_value(rejection(request_nonce, transport_accepted, code)).unwrap_or_else(|_| {
        // 极端失败只返回零副作用固定结构。
        json!({
            // 标记命令失败。
            "ok": false,
            // 固定协议版本。
            "contractVersion": CONTRACT_VERSION,
            // 不声称 transport 已接受。
            "transportAccepted": false,
            // mutation 未接受。
            "businessAccepted": false,
            // mutation 未完成。
            "completed": false,
            // 固定未 dispatch。
            "outcome": "not-dispatched",
            // 修正请求后可重试。
            "retrySafe": true,
            // 目标未改变。
            "targetMayHaveMutated": false,
            // 输出固定序列化错误。
            "error": {
                // 使用协议错误码。
                "code": "WORKER_PROTOCOL_ERROR",
                // 使用安全固定消息。
                "message": "The interactive command worker could not serialize its rejection."
            },
            // 输出零副作用证据。
            "evidence": {
                // 未调用本地 provider。
                "localProviderInvoked": false,
                // 未解析目标。
                "targetResolved": false,
                // 未 dispatch mutation。
                "mutationDispatched": false,
                // 未回退 host 当前桌面。
                "foregroundFallbackUsed": false
            }
        })
    })
}

// 在已经认证的目标 session 内通过同一个 ComputerControlSystem 执行唯一 command。
fn execute_authenticated(request: &InteractiveCommandRequest) -> (Value, i32) {
    // 协议已经要求领域 input 为对象。
    let Some(mut input) = request.input().as_object().cloned() else {
        // 理论漂移保持 dispatch 前参数拒绝。
        return (
            // 构造带可信 nonce 的拒绝。
            rejection_value(
                // 绑定当前请求。
                Some(request.request_nonce()),
                // 完整协议 envelope 已接受。
                true,
                // 使用普通参数错误。
                InteractiveCommandProtocolErrorCode::InvalidArgument,
            ),
            // 使用结构化业务拒绝退出码。
            2,
        );
    };
    // 恢复各领域 Module 既有的 timeoutMs 输入，同时由外层 Job 保持总 deadline。
    input.insert(
        // 使用既有 camelCase 字段。
        "timeoutMs".to_owned(),
        // 转换已验证有界预算。
        Value::from(request.timeout_ms()),
    );
    // 构造目标 session 内不会递归选择 endpoint 的 generic app 请求。
    let mut command = CommandRequest::read(Verb::Run, "app");
    // 使用协议冻结的唯一 operation。
    command.operation = Some(request.operation().to_owned());
    // 只传入原 canonical 窗口目标。
    command.target.insert(
        // 使用固定字段名。
        "sessionId".to_owned(),
        // 复制已验证 s2:w。
        Value::String(request.session_id().to_owned()),
    );
    // 传入固定版本化 capability。
    command.args.insert(
        // 使用固定字段名。
        "capability".to_owned(),
        // 复制已验证公开 ID。
        Value::String(request.capability().to_owned()),
    );
    // 传入领域输入与唯一 deadline。
    command.args.insert(
        // 使用固定 wrapper 字段。
        "input".to_owned(),
        // 转移独立输入对象。
        Value::Object(input),
    );
    // 保留逐操作确认事实。
    command.confirmed = request.confirmed();
    // 许可仅作用于目标 session 前景。
    command.foreground_consent = request.foreground_consent();
    // worker 内层执行必须走本地 provider，不能递归进入另一个 endpoint。
    command.isolation_requirement = IsolationRequirement::Standard;
    // 使用目标 session 的完整 ComputerControlSystem 执行统一门禁与领域 Module。
    match AppControlService::new().execute(command) {
        // 成功必须投影为确定完成响应。
        Ok(result) => (
            // 删除内层执行计划并输出业务结果。
            interactive_command_result::completed(request.request_nonce(), result),
            // 使用成功退出码。
            0,
        ),
        // 下层失败按接受事实投影为拒绝或 OutcomeUnknown。
        Err(error) => (
            // 使用固定业务状态机转换错误。
            interactive_command_result::failed(request.request_nonce(), &error),
            // 使用结构化业务失败退出码。
            2,
        ),
    }
}

// 从标准输入读取一次请求并写出单行 JSON。
pub fn run_stdio() -> i32 {
    // 创建有界 UTF-8 请求缓冲区。
    let mut input = String::new();
    // 最多读取硬上限加一个溢出探针字节。
    let read_result = std::io::stdin()
        // 限制读取量。
        .take(MAXIMUM_REQUEST_BYTES.saturating_add(1))
        // 读取完整文本。
        .read_to_string(&mut input);
    // 按 transport、认证与业务结果构造响应和退出码。
    let (response, exit_code) = match read_result {
        // 超过硬上限时拒绝且不解析任何字段。
        Ok(_) if u64::try_from(input.len()).unwrap_or(u64::MAX) > MAXIMUM_REQUEST_BYTES => {
            // 返回 transport 未接受的参数错误和业务失败退出码。
            (
                // 构造零副作用拒绝。
                rejection_value(
                    // 不回显未经验证的请求关联值。
                    None,
                    // 标记 transport 未接受。
                    false,
                    // 使用封闭参数错误。
                    InteractiveCommandProtocolErrorCode::InvalidArgument,
                ),
                // 使用结构化拒绝退出码。
                2,
            )
        }
        // 成功读取时执行严格协议解析。
        Ok(_) => match InteractiveCommandRequest::parse(&input) {
            // 已验证请求继续认证固定 broker parent。
            Ok(request) => {
                // 读取当前 worker 所属交互 session。
                let worker_session = current_session_id();
                // 读取 ToolHelp 记录的 parent PID。
                let parent = parent_process_id();
                // 定位同安装目录的固定 broker 镜像。
                let broker = sibling_image_path(BROKER_FILE_NAME);
                // 只有三个私有事实都可读时才执行精确 peer 认证。
                let parent_authenticated = match (worker_session, parent, broker) {
                    // 使用精确 session、PID 与固定路径核对 parent。
                    (Ok(session_id), Ok(process_id), Ok(image)) => {
                        // broker 与 worker 必须位于同一目标 session 和用户主体。
                        authenticate_peer_process(process_id, &image, session_id).is_ok()
                    }
                    // 任一平台事实缺失都失败闭合。
                    _ => false,
                };
                // 未认证 parent 不能进入任何目标或 provider 解析。
                if !parent_authenticated {
                    // 返回 endpoint 认证失败、零副作用证据与拒绝退出码。
                    (
                        // 构造认证拒绝。
                        rejection_value(
                            // 只回显已验证的一次性请求 nonce。
                            Some(request.request_nonce()),
                            // 标记协议 transport 已接受。
                            true,
                            // 使用固定 parent 认证错误。
                            InteractiveCommandProtocolErrorCode::EndpointAuthenticationFailed,
                        ),
                        // 使用结构化拒绝退出码。
                        2,
                    )
                } else {
                    // 认证 parent 只能执行一次封闭 ComputerControlSystem command。
                    execute_authenticated(&request)
                }
            }
            // 协议错误不得触碰 target 或本地 provider。
            Err(failure) => {
                // 按 envelope 接受状态返回协议拒绝和失败退出码。
                (
                    // 构造纯协议拒绝。
                    rejection_value(
                        // 只回显已通过 envelope 验证的请求 nonce。
                        failure.request_nonce(),
                        // 传播 transport envelope 接受状态。
                        failure.transport_accepted(),
                        // 输出解析阶段的封闭错误。
                        failure.code(),
                    ),
                    // 使用结构化拒绝退出码。
                    2,
                )
            }
        },
        // 标准输入读取失败时返回普通参数错误和失败退出码。
        Err(_) => (
            // 构造零副作用拒绝。
            rejection_value(
                // 不存在可信请求关联值。
                None,
                // 标记 transport 未接受。
                false,
                // 使用固定参数错误。
                InteractiveCommandProtocolErrorCode::InvalidArgument,
            ),
            // 使用结构化拒绝退出码。
            2,
        ),
    };
    // 序列化普通封闭响应。
    let output = serde_json::to_string(&response)
        // 极端序列化失败时退回固定安全文本。
        .unwrap_or_else(|_| serialization_failure_text());
    // 锁定标准输出以写入唯一一行。
    let mut stdout = std::io::stdout().lock();
    // 写出 JSON 和换行；管道关闭时返回独立退出码。
    if writeln!(stdout, "{output}").is_err() {
        // 表示 worker 无法交付结构化响应。
        return 3;
    }
    // 返回完成或结构化拒绝对应的稳定退出码。
    exit_code
}

// 声明候选 stdio 层的纯序列化测试。
#[cfg(test)]
mod tests {
    // 导入待测固定 fallback。
    use super::*;

    // 验证极端 fallback 不泄漏请求或平台事实。
    #[test]
    fn serialization_fallback_is_fixed_and_fail_closed() {
        // 取得固定 fallback 文本。
        let text = serialization_failure_text();
        // 解析为 JSON 供字段核对。
        let value: serde_json::Value = serde_json::from_str(&text)
            // 固定文本必须始终是合法 JSON。
            .unwrap_or_else(|error| panic!("fallback JSON invalid: {error}"));
        // 固定协议版本必须存在。
        assert_eq!(value["contractVersion"], CONTRACT_VERSION);
        // mutation 不得被接受。
        assert_eq!(value["businessAccepted"], false);
        // 本地 provider 必须保持零调用。
        assert_eq!(value["evidence"]["localProviderInvoked"], false);
    }
}
