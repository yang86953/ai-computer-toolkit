#![cfg(target_os = "windows")]

//! 验证独立交互会话 command worker 候选协议保持失败闭合。

// 导入子进程 stdio 与输出类型。
use std::process::{Command, Output, Stdio};
// 导入向 worker 标准输入写入请求的接口。
use std::io::Write;

// 导入 JSON 构造和值类型。
use serde_json::{Value, json};

// 固定 Cargo 构建的 command worker 候选路径。
const WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-interactive-command-worker");
// 固定内部协议版本。
const CONTRACT_VERSION: &str = "act/interactive-command-worker/v1";

// 构造最小合法的严格键盘请求。
fn valid_request() -> Value {
    // 返回字段封闭的 JSON 对象。
    json!({
        // 使用固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用 canonical 请求 nonce。
        "requestNonce": "0123456789abcdef0123456789abcdef",
        // 使用 canonical endpoint lease nonce。
        "endpointLeaseNonce": "fedcba9876543210fedcba9876543210",
        // 使用通用键盘 capability。
        "capability": "ui.input.key@1",
        // 使用统一 apply operation。
        "operation": "apply",
        // 使用 canonical 窗口目标。
        "sessionId": "s2:w:0000000000000001",
        // 使用 provider-neutral 输入对象。
        "input": { "key": "ENTER" },
        // 提供逐操作确认。
        "confirmed": true,
        // 提供只作用于 worker 会话的前景许可。
        "foregroundConsent": true,
        // 固定严格隔离要求。
        "isolationRequirement": "strict",
        // 固定隔离 worker 外部执行域。
        "requiredExecutionRealm": "isolated-worker",
        // 固定严格零干扰策略。
        "hostImpactPolicy": "strict-no-interference",
        // 使用有界 deadline。
        "timeoutMs": 5000
    })
}

// 运行一次固定 worker 并收集单行响应。
fn run_worker(input: &[u8]) -> Output {
    // 启动 Cargo 构建的固定 Rust 二进制。
    let mut child = Command::new(WORKER)
        // 为请求提供独占管道。
        .stdin(Stdio::piped())
        // 收集唯一 JSON stdout。
        .stdout(Stdio::piped())
        // 收集意外诊断供测试失败分析。
        .stderr(Stdio::piped())
        // 启动候选 worker。
        .spawn()
        // 启动失败时提供固定测试诊断。
        .unwrap_or_else(|error| panic!("interactive worker launch failed: {error}"));
    // 取得请求管道所有权。
    let mut stdin = child
        // 移出可写标准输入。
        .stdin
        // 缺少管道表示夹具配置错误。
        .take()
        // 提供明确测试诊断。
        .unwrap_or_else(|| panic!("interactive worker stdin unavailable"));
    // 写入完整 UTF-8 请求。
    stdin
        // 不附加第二个 JSON 文档。
        .write_all(input)
        // 管道写入失败时终止测试。
        .unwrap_or_else(|error| panic!("interactive worker stdin write failed: {error}"));
    // 关闭标准输入以结束 worker 有界读取。
    drop(stdin);
    // 等待固定短命 worker 结束并收集输出。
    child
        // 消费子进程所有权。
        .wait_with_output()
        // 等待失败时提供明确诊断。
        .unwrap_or_else(|error| panic!("interactive worker wait failed: {error}"))
}

// 解析 worker 的唯一 JSON stdout。
fn response(output: &Output) -> Value {
    // 标准输出必须是单个 JSON 对象。
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        // 失败时仅输出 worker 自有 stderr，不回显请求。
        panic!(
            "interactive worker response invalid: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

// 验证完整请求在固定 broker parent 缺失时保持认证失败。
#[test]
fn certified_endpoint_is_required_before_any_target_access() {
    // 运行最小合法请求。
    let output = run_worker(valid_request().to_string().as_bytes());
    // 候选 worker 不得返回成功退出。
    assert!(!output.status.success());
    // 解析封闭失败响应。
    let value = response(&output);
    // 完整协议 transport 已被接受。
    assert_eq!(value["transportAccepted"], true);
    // mutation 尚未被业务接受。
    assert_eq!(value["businessAccepted"], false);
    // 直接启动不能冒充固定 broker parent。
    assert_eq!(value["error"]["code"], "ENDPOINT_AUTHENTICATION_FAILED");
    // 只回显已验证的随机关联值。
    assert_eq!(value["requestNonce"], "0123456789abcdef0123456789abcdef");
    // 未解析目标。
    assert_eq!(value["evidence"]["targetResolved"], false);
    // 未调用本地 provider。
    assert_eq!(value["evidence"]["localProviderInvoked"], false);
    // 未向当前桌面回退。
    assert_eq!(value["evidence"]["foregroundFallbackUsed"], false);
}

// 验证确认缺失优先于非法 target。
#[test]
fn confirmation_is_checked_before_target_semantics() {
    // 构造基础请求。
    let mut request = valid_request();
    // 移除逐操作确认。
    request["confirmed"] = json!(false);
    // 同时提供禁止的 native 目标。
    request["sessionId"] = json!("window:42");
    // 运行组合错误请求。
    let output = run_worker(request.to_string().as_bytes());
    // 解析封闭失败响应。
    let value = response(&output);
    // 必须先返回 confirmation 错误。
    assert_eq!(value["error"]["code"], "CONFIRMATION_REQUIRED");
    // 已验证 envelope 可以回显随机关联值。
    assert_eq!(value["requestNonce"], "0123456789abcdef0123456789abcdef");
    // transport 必须与业务确认拒绝分开表达。
    assert_eq!(value["transportAccepted"], true);
    // mutation 不得派发。
    assert_eq!(value["evidence"]["mutationDispatched"], false);
}

// 验证 command worker 拒绝递归 endpoint 选择。
#[test]
fn recursive_interactive_session_target_is_rejected() {
    // 构造基础请求。
    let mut request = valid_request();
    // 在嵌套输入中注入禁止字段。
    request["input"] = json!({
        // 保留普通 capability 字段。
        "key": "ENTER",
        // 尝试创建第二层隔离路由。
        "routing": { "interactiveSessionId": "s2:i:0000000000000001" }
    });
    // 运行递归路由请求。
    let output = run_worker(request.to_string().as_bytes());
    // 解析封闭失败响应。
    let value = response(&output);
    // 必须返回普通协议参数错误。
    assert_eq!(value["error"]["code"], "INVALID_ARGUMENT");
    // 当前桌面回退必须保持关闭。
    assert_eq!(value["evidence"]["foregroundFallbackUsed"], false);
}

// 验证三个 JSON schema 与 Rust 协议共享封闭字段和状态。
#[test]
fn protocol_schemas_freeze_request_and_response_boundaries() {
    // 读取编译期嵌入的请求 schema。
    let request_schema: Value = serde_json::from_str(include_str!(
        // 使用仓库内部请求契约。
        "../contracts/internal/interactive-command-worker-request-v1.schema.json"
    ))
    // schema 文件必须是合法 JSON。
    .unwrap_or_else(|error| panic!("interactive request schema invalid: {error}"));
    // 请求边界必须拒绝扩展字段。
    assert_eq!(request_schema["additionalProperties"], false);
    // 请求必须固定严格隔离。
    assert_eq!(
        request_schema["properties"]["isolationRequirement"]["const"],
        "strict"
    );
    // 请求目标只允许 canonical 窗口。
    assert_eq!(
        request_schema["properties"]["sessionId"]["pattern"],
        "^s2:w:[0-9a-f]{16}$"
    );
    // 读取编译期嵌入的响应 schema。
    let response_schema: Value = serde_json::from_str(include_str!(
        // 使用仓库内部响应契约。
        "../contracts/internal/interactive-command-worker-response-v1.schema.json"
    ))
    // schema 文件必须是合法 JSON。
    .unwrap_or_else(|error| panic!("interactive response schema invalid: {error}"));
    // 响应必须显式覆盖三种结果类别。
    assert_eq!(
        response_schema["oneOf"]
            // 读取数组长度。
            .as_array()
            // 缺失数组时使用零触发明确断言失败。
            .map_or(0, Vec::len),
        3
    );
    // dispatch 前拒绝必须保持业务未接受。
    assert_eq!(
        response_schema["$defs"]["preDispatchRejection"]["properties"]["businessAccepted"]["const"],
        false
    );
    // outcome unknown 必须禁止重试。
    assert_eq!(
        response_schema["$defs"]["outcomeUnknown"]["properties"]["retrySafe"]["const"],
        false
    );
    // 读取编译期嵌入的 broker wire schema。
    let broker_schema: Value = serde_json::from_str(include_str!(
        // 使用仓库内部 broker 契约。
        "../contracts/internal/interactive-session-broker-v1.schema.json"
    ))
    // schema 文件必须是合法 JSON。
    .unwrap_or_else(|error| panic!("interactive broker schema invalid: {error}"));
    // broker 顶层必须只列出七种封闭 wire 消息。
    assert_eq!(
        broker_schema["oneOf"]
            // 读取固定消息集合。
            .as_array()
            // 缺失数组时使用零触发明确断言失败。
            .map_or(0, Vec::len),
        7
    );
    // endpoint 必须固定独立交互会话执行域。
    assert_eq!(
        broker_schema["$defs"]["endpoint"]["properties"]["executionRealm"]["const"],
        "isolated-worker"
    );
    // command 只能嵌入已经冻结的一次性 worker 请求。
    assert_eq!(
        broker_schema["$defs"]["commandRequest"]["properties"]["command"]["$ref"],
        "interactive-command-worker-request-v1.schema.json"
    );
    // transport 尚未接受时不得回显不可信请求 nonce。
    assert_eq!(
        broker_schema["$defs"]["rejectionBeforeTransport"]["allOf"][1]["properties"]["requestNonce"],
        false
    );
}
