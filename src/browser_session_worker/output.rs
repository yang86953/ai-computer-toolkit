//! 负责隔离浏览器会话打开握手的封闭 JSON Lines 输出。

// 导入协议输出写入能力。
use std::io::Write;

// 导入 JSON 构造与值类型。
use serde_json::{Value, json};

// 导入冻结会话协议版本。
use crate::components::browser_session_protocol::CONTRACT_VERSION;

// 向 stdout 写入并 flush 一条协议 JSON 帧。
fn write_frame(frame: &Value) -> Result<(), ()> {
    // 严格序列化为单行 JSON。
    let text = serde_json::to_string(frame).map_err(|_| ())?;
    // 独占当前写入作用域的 stdout 锁。
    let stdout = std::io::stdout();
    // 锁定协议输出。
    let mut output = stdout.lock();
    // 写入一条完整帧。
    writeln!(output, "{text}").map_err(|_| ())?;
    // accepted 必须在 dispatch 前可见。
    output.flush().map_err(|_| ())
}

// 构造并发送未派发 final。
pub(super) fn write_not_dispatched(
    // 关联打开请求。
    request_nonce: &str,
    // 使用封闭错误码。
    code: &str,
    // 使用安全诊断。
    message: &str,
) -> Result<(), ()> {
    // 写入与冻结协议一致的可安全重试终态。
    write_frame(&json!({
        // 标记唯一 final 帧。
        "kind": "open-final",
        // 关联固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联当前请求。
        "requestNonce": request_nonce,
        // 声明尚未 dispatch。
        "outcome": "not-dispatched",
        // 声明终态完整。
        "completed": true,
        // 未 dispatch 可安全重试。
        "retrySafe": true,
        // 明确没有资源接受可能。
        "acceptedMayHaveOccurred": false,
        // 未建立会话身份。
        "sessionId": Value::Null,
        // 只输出封闭错误事实。
        "error": { "code": code, "message": message },
    }))
}

// 构造并发送 accepted 帧。
pub(super) fn write_accepted(
    // 关联打开请求。
    request_nonce: &str,
) -> Result<(), ()> {
    // accepted 必须先于浏览器进程创建。
    write_frame(&json!({
        // 标记 accepted 帧。
        "kind": "open-accepted",
        // 关联固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联当前请求。
        "requestNonce": request_nonce,
        // 声明即将产生资源。
        "dispatchAccepted": true,
        // accepted 不是终态。
        "completed": false,
    }))
}

// 构造并发送 accepted 后的确定失败。
pub(super) fn write_failed(
    // 关联打开请求。
    request_nonce: &str,
    // 使用封闭错误码。
    code: &str,
    // 使用安全诊断。
    message: &str,
) -> Result<(), ()> {
    // 写入不可自动重试的确定失败。
    write_frame(&json!({
        // 标记唯一 final 帧。
        "kind": "open-final",
        // 关联固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联当前请求。
        "requestNonce": request_nonce,
        // 声明 dispatch 后确定失败。
        "outcome": "failed",
        // 声明失败终态完整。
        "completed": true,
        // dispatch 后禁止自动重试。
        "retrySafe": false,
        // 浏览器可能已接受创建。
        "acceptedMayHaveOccurred": true,
        // 失败不建立会话身份。
        "sessionId": Value::Null,
        // 只输出封闭错误事实。
        "error": { "code": code, "message": message },
    }))
}

// 构造并发送 accepted 后失联的保守未知结果。
pub(super) fn write_unknown(
    // 关联打开请求。
    request_nonce: &str,
) -> Result<(), ()> {
    // 写入协议唯一允许的未知组合。
    write_frame(&json!({
        // 标记唯一 final 帧。
        "kind": "open-final",
        // 关联固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联当前请求。
        "requestNonce": request_nonce,
        // 声明结果未知。
        "outcome": "unknown",
        // 未取得可信终态。
        "completed": false,
        // 未知结果禁止自动重试。
        "retrySafe": false,
        // 明确资源可能已经接受。
        "acceptedMayHaveOccurred": true,
        // 未建立可信会话身份。
        "sessionId": Value::Null,
        // 使用冻结的唯一未知错误码。
        "error": {
            // 输出固定错误码。
            "code": "OUTCOME_UNKNOWN",
            // 输出不泄漏连接细节的诊断。
            "message": "The browser session lost its parent before a trustworthy final outcome.",
        },
    }))
}

// 构造并发送 ready final。
pub(super) fn write_ready(
    // 关联打开请求。
    request_nonce: &str,
    // 绑定当前 opaque 会话。
    session_id: &str,
) -> Result<(), ()> {
    // 写入唯一 ready 终态。
    write_frame(&json!({
        // 标记唯一 final 帧。
        "kind": "open-final",
        // 关联固定协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联当前请求。
        "requestNonce": request_nonce,
        // 声明会话已经就绪。
        "outcome": "ready",
        // ready 是可信完整终态。
        "completed": true,
        // 已创建资源禁止自动重试。
        "retrySafe": false,
        // 明确浏览器已接受创建。
        "acceptedMayHaveOccurred": true,
        // 只输出随机 opaque 会话身份。
        "sessionId": session_id,
        // ready 不携带错误。
        "error": Value::Null,
    }))
}
