//! browser-session parent Job 客户端的固定 worker 测试替身。

// 导入有界行读写和等待工具。
use std::{
    // 导入标准输入输出行操作。
    io::{BufRead, Write},
    // 导入固定挂起休眠。
    thread,
    // 导入休眠时长。
    time::Duration,
};

// 导入 JSON 值和构造宏。
use serde_json::{Value, json};

// 固定协议版本。
const CONTRACT_VERSION: &str = "act/browser-session-worker/v1";
// 固定 fixture ready 会话 ID。
const SESSION_ID: &str = "s2:bs:fedcba9876543210fedcba9876543210";

// 向 stdout 写入并 flush 一条帧。
fn write_frame(frame: &Value) -> bool {
    // 严格序列化 fixture 帧。
    let Ok(text) = serde_json::to_string(frame) else {
        // 序列化失败终止 fixture。
        return false;
    };
    // 取得标准输出锁。
    let stdout = std::io::stdout();
    // 锁定当前写入。
    let mut output = stdout.lock();
    // 写入并 flush 完整行。
    writeln!(output, "{text}").is_ok() && output.flush().is_ok()
}

// 从 open 请求提取 canonical nonce。
fn read_nonce() -> Option<String> {
    // 取得标准输入锁。
    let stdin = std::io::stdin();
    // 锁定一行输入。
    let mut input = stdin.lock();
    // 保存 open 行。
    let mut line = String::new();
    // 读取唯一 open。
    if input.read_line(&mut line).ok()? == 0 {
        // 零帧不建立关联。
        return None;
    }
    // 解析 JSON 值。
    let value = serde_json::from_str::<Value>(line.trim_end()).ok()?;
    // 必须是固定 open。
    if value.get("kind").and_then(Value::as_str) != Some("open")
        // 必须使用固定版本。
        || value.get("contractVersion").and_then(Value::as_str) != Some(CONTRACT_VERSION)
    {
        // 拒绝漂移输入。
        return None;
    }
    // 取得请求 nonce。
    value
        // 读取关联字段。
        .get("requestNonce")
        // 只接受字符串。
        .and_then(Value::as_str)
        // 保存独立值。
        .map(str::to_owned)
}

// 构造 accepted 帧。
fn accepted(nonce: &str) -> Value {
    // 返回冻结 accepted 形状。
    json!({
        // 标记 accepted。
        "kind": "open-accepted",
        // 使用固定版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联请求。
        "requestNonce": nonce,
        // 声明 dispatch。
        "dispatchAccepted": true,
        // accepted 未完成。
        "completed": false,
    })
}

// 构造 ready final。
fn ready(nonce: &str) -> Value {
    // 返回冻结 ready 形状。
    json!({
        // 标记 final。
        "kind": "open-final",
        // 使用固定版本。
        "contractVersion": CONTRACT_VERSION,
        // 关联请求。
        "requestNonce": nonce,
        // 声明 ready。
        "outcome": "ready",
        // ready 已完成。
        "completed": true,
        // ready 不可自动重试。
        "retrySafe": false,
        // 资源已接受。
        "acceptedMayHaveOccurred": true,
        // 使用 canonical fixture ID。
        "sessionId": SESSION_ID,
        // ready 无错误。
        "error": Value::Null,
    })
}

// 执行固定 fixture 行为。
fn main() {
    // 读取唯一模式参数。
    let mode = std::env::args().nth(1);
    // 零帧模式不读取 open 直接退出。
    if mode.as_deref() == Some("--mode=zero-frame") {
        // 使用固定异常退出码。
        std::process::exit(7);
    }
    // 其余模式必须取得可信 nonce。
    let Some(nonce) = read_nonce() else {
        // 输入漂移失败。
        std::process::exit(2);
    };
    // 首先输出 accepted。
    if !write_frame(&accepted(&nonce)) {
        // parent 已断开。
        std::process::exit(2);
    }
    // accepted-only 模式立即异常退出。
    if mode.as_deref() == Some("--mode=accepted-only") {
        // 使用固定异常退出码。
        std::process::exit(7);
    }
    // 挂起模式忽略 cancel，迫使 parent Job 回收。
    if mode.as_deref() == Some("--mode=hang-after-accepted") {
        // 永久等待 Job 终止。
        loop {
            // 使用短休眠避免占用 CPU。
            thread::sleep(Duration::from_millis(100));
        }
    }
    // 只允许 ready 模式继续。
    if mode.as_deref() != Some("--mode=ready") {
        // 未知固定参数失败。
        std::process::exit(2);
    }
    // 输出 ready final。
    if !write_frame(&ready(&nonce)) {
        // parent 已断开。
        std::process::exit(2);
    }
    // ready 后等待 cancel 或 EOF。
    let stdin = std::io::stdin();
    // 锁定剩余输入。
    let mut input = stdin.lock();
    // 保存 cancel 行。
    let mut line = String::new();
    // 读取 cancel 或 EOF。
    let _ = input.read_line(&mut line);
}
