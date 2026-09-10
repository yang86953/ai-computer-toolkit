//! 执行 browser-session worker 内的固定页面命令帧。

// 导入标准输出写入和单调期限。
use std::{
    // 导入逐帧 stdout 写入。
    io::Write,
    // 导入命令总期限。
    time::{Duration, Instant},
};

// 导入 JSON 构造和值类型。
use serde_json::{Value, json};

// 导入页面协议、私有 CDP 会话与统一错误。
use crate::{
    // 导入私有组件。
    components::{
        // 导入 CDP 页面所有者。
        browser_cdp_session::BrowserCdpSession,
        // 导入冻结页面命令协议。
        browser_page_protocol::{
            BrowserPageOperation, BrowserPageOperationKind, BrowserPageWorkerInput,
            CONTRACT_VERSION,
        },
    },
    // 导入统一错误。
    domain::AppControlError,
};

// 表示处理一条页面输入后的会话去向。
pub(crate) enum PageInputDisposition {
    // 页面会话可以继续接收命令。
    Continue,
    // 页面协议或连接已不可信，结束整个会话。
    EndSession,
}

// 从一条已验证 command 读取取消关联 nonce。
pub(crate) fn command_nonce(line: &str, expected_session_id: &str) -> Option<String> {
    // 严格解析页面协议。
    let input = BrowserPageWorkerInput::parse_line(line).ok()?;
    // 只允许当前会话的 command 进入并发执行。
    if matches!(input, BrowserPageWorkerInput::Command { .. })
        // 会话身份必须匹配。
        && input.session_id() == expected_session_id
    {
        // 返回关联 nonce 所有权。
        Some(input.request_nonce().to_owned())
    } else {
        // cancel 或 stale 会话无需并发等待。
        None
    }
}

// 解析并执行一条页面命令或空闲取消。
pub(crate) fn handle_line(
    // 接收有界原始行。
    line: &str,
    // 借用公开会话身份用于关联门禁。
    expected_session_id: &str,
    // 可变借用私有 CDP 会话。
    cdp_session: &mut BrowserCdpSession,
) -> PageInputDisposition {
    // 严格解析冻结协议。
    let Ok(input) = BrowserPageWorkerInput::parse_line(line) else {
        // 非页面协议输入使复用管道不可信。
        return PageInputDisposition::EndSession;
    };
    // 按输入种类处理。
    match input {
        // 空闲时的关联取消是幂等无操作。
        BrowserPageWorkerInput::CancelCommand { session_id, .. } => {
            // 身份漂移时结束会话。
            if session_id != expected_session_id {
                // 防止跨会话取消探测。
                return PageInputDisposition::EndSession;
            }
            // 没有在途命令时继续持有。
            PageInputDisposition::Continue
        }
        // 执行一项固定页面命令。
        BrowserPageWorkerInput::Command {
            // 取得会话身份。
            session_id,
            // 取得请求关联值。
            request_nonce,
            // 取得不重置总期限。
            timeout_ms,
            // 取得可选私有页面引用。
            page_ref,
            // 取得调用方导航代际。
            navigation_generation,
            // 取得强类型操作。
            operation,
            // 版本已由 parser 验证。
            ..
        } => {
            // 保存固定操作种类。
            let operation_kind = operation.kind();
            // 会话身份漂移必须在 dispatch 前失败。
            if session_id != expected_session_id {
                // 输出确定未派发结果。
                let _ = write_not_dispatched(
                    // 关联请求。
                    &request_nonce,
                    // 关联操作。
                    operation_kind,
                    // 保留当前代际。
                    cdp_session.navigation_generation(),
                    // 使用会话 stale 类别。
                    "STALE_SESSION",
                    // 不回显会话身份。
                    "The browser session identity is stale.",
                );
                // 继续服务正确会话。
                return PageInputDisposition::Continue;
            }
            // 从 worker 接受命令时建立唯一总期限。
            let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
            // 在 accepted 前按操作完成页面身份门禁。
            let preflight = match &operation {
                // 导航允许初始空页面引用。
                BrowserPageOperation::Navigate { .. } => cdp_session.validate_navigation(
                    // 借用可选页面引用。
                    page_ref.as_deref(),
                    // 传入观察代际。
                    navigation_generation,
                ),
                // 其余操作必须绑定当前页面。
                BrowserPageOperation::Wait { .. }
                | BrowserPageOperation::Query { .. }
                | BrowserPageOperation::Click { .. }
                | BrowserPageOperation::Type { .. }
                | BrowserPageOperation::Screenshot {} => {
                    // 核对当前页面身份。
                    cdp_session.validate_page(
                        // 借用必需页面引用。
                        page_ref.as_deref(),
                        // 传入观察代际。
                        navigation_generation,
                    )
                }
            };
            // preflight 失败不得输出 accepted。
            if let Err(error) = preflight {
                // 输出确定未派发结果。
                let _ = write_not_dispatched(
                    // 关联请求。
                    &request_nonce,
                    // 关联操作。
                    operation_kind,
                    // 保留当前代际。
                    cdp_session.navigation_generation(),
                    // 透传稳定错误码。
                    error.code,
                    // 透传安全消息。
                    &error.message,
                );
                // 会话仍可信。
                return PageInputDisposition::Continue;
            }
            // 元素动作还必须在 accepted 前解析当前 registry。
            let resolved_element = match &operation {
                // 点击解析 element ref。
                BrowserPageOperation::Click { element_ref, .. }
                // 输入解析相同 element ref。
                | BrowserPageOperation::Type { element_ref, .. } => {
                    // 查找当前代际私有 node ID。
                    match cdp_session.resolve_element(element_ref) {
                        // 保存 node ID。
                        Ok(backend_node_id) => Some(backend_node_id),
                        // stale 不得产生 accepted。
                        Err(error) => {
                            // 输出确定未派发。
                            let _ = write_not_dispatched(
                                // 关联请求。
                                &request_nonce,
                                // 关联操作。
                                operation_kind,
                                // 保留当前代际。
                                cdp_session.navigation_generation(),
                                // 透传稳定错误码。
                                error.code,
                                // 透传安全消息。
                                &error.message,
                            );
                            // 会话仍可信。
                            return PageInputDisposition::Continue;
                        }
                    }
                }
                // 非元素动作不需要 node ID。
                _ => None,
            };
            // accepted 必须在首次 CDP dispatch 前 flush。
            if write_accepted(&request_nonce, operation_kind).is_err() {
                // parent 断开使结果未知。
                return PageInputDisposition::EndSession;
            }
            // 执行封闭页面读取或导航。
            let result = match operation {
                // 执行固定导航。
                BrowserPageOperation::Navigate { url } => cdp_session.navigate(&url, deadline),
                // 执行固定等待。
                BrowserPageOperation::Wait { condition } => cdp_session.wait(&condition, deadline),
                // 执行固定语义查询。
                BrowserPageOperation::Query {
                    // 取得 selector。
                    selector,
                    // 取得结果上限。
                    max_results,
                } => cdp_session.query(&selector, max_results, deadline),
                // 执行固定点击序列。
                BrowserPageOperation::Click { .. } => match resolved_element {
                    // 使用 preflight 已解析 node ID。
                    Some(backend_node_id) => cdp_session.click(backend_node_id, deadline),
                    // 内部状态漂移失败闭合。
                    None => Err(AppControlError::new(
                        // 使用稳定协议码。
                        "BROWSER_PROTOCOL_FAILED",
                        // 输出安全诊断。
                        "The browser element dispatch state was invalid.",
                    )),
                },
                // 执行固定文本输入序列。
                BrowserPageOperation::Type { text, replace, .. } => match resolved_element {
                    // 使用 preflight 已解析 node ID。
                    Some(backend_node_id) => {
                        // 执行固定文本输入。
                        cdp_session.type_text(backend_node_id, &text, replace, deadline)
                    }
                    // 内部状态漂移失败闭合。
                    None => Err(AppControlError::new(
                        // 使用稳定协议码。
                        "BROWSER_PROTOCOL_FAILED",
                        // 输出安全诊断。
                        "The browser element dispatch state was invalid.",
                    )),
                },
                // 执行固定 PNG 截图。
                BrowserPageOperation::Screenshot {} => cdp_session.screenshot(deadline),
            };
            // 投影固定执行结果。
            match result {
                // 输出可信完成结果。
                Ok(data) => {
                    // 写入 completed final。
                    if write_completed(
                        // 关联请求。
                        &request_nonce,
                        // 关联操作。
                        operation_kind,
                        // 返回推进后的代际。
                        cdp_session.navigation_generation(),
                        // 携带 provider-neutral 数据。
                        data,
                    )
                    // 输出断开使终态不可投影。
                    .is_err()
                    {
                        // 结束不可信会话。
                        return PageInputDisposition::EndSession;
                    }
                    // 会话可继续使用。
                    PageInputDisposition::Continue
                }
                // 映射 accepted 后错误。
                Err(error) => write_post_dispatch_error(
                    // 关联请求。
                    &request_nonce,
                    // 关联操作。
                    operation_kind,
                    // 保留当前代际。
                    cdp_session.navigation_generation(),
                    // 借用统一错误。
                    &error,
                ),
            }
        }
    }
}

// 写入页面命令 accepted。
fn write_accepted(request_nonce: &str, operation: BrowserPageOperationKind) -> Result<(), ()> {
    // 输出固定 accepted 字段。
    write_frame(&json!({
        // 标记 accepted 帧。
        "kind": "command-accepted",
        // 使用冻结版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联请求。
        "requestNonce": request_nonce,
        // 关联操作。
        "operation": operation,
        // 声明 dispatch 已接受。
        "dispatchAccepted": true,
        // accepted 尚未完成。
        "completed": false,
    }))
}

// 写入页面命令 completed final。
fn write_completed(
    // 借用请求 nonce。
    request_nonce: &str,
    // 接收操作种类。
    operation: BrowserPageOperationKind,
    // 接收当前导航代际。
    navigation_generation: u64,
    // 取得成功数据。
    data: Value,
) -> Result<(), ()> {
    // 输出固定完成组合。
    write_frame(&json!({
        // 标记 final 帧。
        "kind": "command-final",
        // 使用冻结版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联请求。
        "requestNonce": request_nonce,
        // 关联操作。
        "operation": operation,
        // 声明确定完成。
        "outcome": "completed",
        // 终态可信。
        "completed": true,
        // dispatch 后不可自动重试。
        "retrySafe": false,
        // 明确已经接受。
        "acceptedMayHaveOccurred": true,
        // 返回 worker 当前代际。
        "navigationGeneration": navigation_generation,
        // 返回封闭数据。
        "data": data,
        // 成功不携带错误。
        "error": Value::Null,
    }))
}

// 写入确定未派发 final。
fn write_not_dispatched(
    // 借用请求 nonce。
    request_nonce: &str,
    // 接收操作种类。
    operation: BrowserPageOperationKind,
    // 接收当前导航代际。
    navigation_generation: u64,
    // 借用稳定错误码。
    code: &str,
    // 借用安全消息。
    message: &str,
) -> Result<(), ()> {
    // 输出固定未派发组合。
    write_frame(&json!({
        // 标记 final 帧。
        "kind": "command-final",
        // 使用冻结版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联请求。
        "requestNonce": request_nonce,
        // 关联操作。
        "operation": operation,
        // 声明确定未派发。
        "outcome": "not-dispatched",
        // 终态可信。
        "completed": true,
        // 未派发可安全重试。
        "retrySafe": true,
        // 明确浏览器未接受。
        "acceptedMayHaveOccurred": false,
        // 返回 worker 当前代际。
        "navigationGeneration": navigation_generation,
        // 失败不携带数据。
        "data": Value::Null,
        // 返回安全错误。
        "error": { "code": code, "message": message },
    }))
}

// 映射 accepted 后的确定失败或未知结果。
fn write_post_dispatch_error(
    // 借用请求 nonce。
    request_nonce: &str,
    // 接收操作种类。
    operation: BrowserPageOperationKind,
    // 接收当前导航代际。
    navigation_generation: u64,
    // 借用统一错误。
    error: &AppControlError,
) -> PageInputDisposition {
    // 传输断开或 deadline 无法证明浏览器是否执行。
    let unknown = matches!(
        // 检查稳定错误码。
        error.code,
        // deadline 可能在 browser 接受后发生。
        "DEADLINE_EXCEEDED"
            // 断开无法取得可信响应。
            | "BROWSER_PROTOCOL_DISCONNECTED"
            // 帧错误同样破坏关联证据。
            | "BROWSER_PROTOCOL_FAILED"
    );
    // 按证据输出 final。
    let result = if unknown {
        // 输出唯一未知组合。
        write_unknown(request_nonce, operation, navigation_generation)
    } else {
        // 输出 accepted 后确定失败。
        write_failed(
            // 关联请求。
            request_nonce,
            // 关联操作。
            operation,
            // 保留当前代际。
            navigation_generation,
            // 透传稳定码。
            error.code,
            // 透传安全消息。
            &error.message,
        )
    };
    // 输出失败或未知传输错误都结束会话。
    if result.is_err() || unknown {
        // 不再复用不可信连接。
        PageInputDisposition::EndSession
    } else {
        // 确定业务失败允许继续。
        PageInputDisposition::Continue
    }
}

// 写入 accepted 后确定失败 final。
fn write_failed(
    // 借用请求 nonce。
    request_nonce: &str,
    // 接收操作种类。
    operation: BrowserPageOperationKind,
    // 接收当前导航代际。
    navigation_generation: u64,
    // 借用稳定错误码。
    code: &str,
    // 借用安全消息。
    message: &str,
) -> Result<(), ()> {
    // 输出固定失败组合。
    write_frame(&json!({
        // 标记 final 帧。
        "kind": "command-final",
        // 使用冻结版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联请求。
        "requestNonce": request_nonce,
        // 关联操作。
        "operation": operation,
        // 声明确认失败。
        "outcome": "failed",
        // 终态可信。
        "completed": true,
        // dispatch 后不可自动重试。
        "retrySafe": false,
        // 明确可能已接受。
        "acceptedMayHaveOccurred": true,
        // 保留当前代际。
        "navigationGeneration": navigation_generation,
        // 失败不携带数据。
        "data": Value::Null,
        // 返回安全错误。
        "error": { "code": code, "message": message },
    }))
}

// 写入 accepted 后未知 final。
fn write_unknown(
    // 借用请求 nonce。
    request_nonce: &str,
    // 接收操作种类。
    operation: BrowserPageOperationKind,
    // 接收当前导航代际。
    navigation_generation: u64,
) -> Result<(), ()> {
    // 输出固定未知组合。
    write_frame(&json!({
        // 标记 final 帧。
        "kind": "command-final",
        // 使用冻结版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联请求。
        "requestNonce": request_nonce,
        // 关联操作。
        "operation": operation,
        // 声明结果未知。
        "outcome": "unknown",
        // 终态不可信。
        "completed": false,
        // 未知禁止自动重试。
        "retrySafe": false,
        // 明确可能已接受。
        "acceptedMayHaveOccurred": true,
        // 保留当前代际。
        "navigationGeneration": navigation_generation,
        // 未知不携带数据。
        "data": Value::Null,
        // 使用唯一未知错误。
        "error": {
            // 固定错误码。
            "code": "OUTCOME_UNKNOWN",
            // 固定安全消息。
            "message": "The browser page command lost a trustworthy final outcome.",
        },
    }))
}

// 序列化并 flush 一条有界 JSON Lines 帧。
fn write_frame(value: &Value) -> Result<(), ()> {
    // 序列化固定 worker 输出。
    let bytes = serde_json::to_vec(value).map_err(|_| ())?;
    // 取得 stdout 锁。
    let stdout = std::io::stdout();
    // 独占本次写入。
    let mut output = stdout.lock();
    // 写入 JSON、换行并 flush。
    output
        // 写入 JSON 字节。
        .write_all(&bytes)
        // 追加单个换行。
        .and_then(|()| output.write_all(b"\n"))
        // 强制暴露 dispatch 边界。
        .and_then(|()| output.flush())
        // 隐藏管道错误细节。
        .map_err(|_| ())
}
