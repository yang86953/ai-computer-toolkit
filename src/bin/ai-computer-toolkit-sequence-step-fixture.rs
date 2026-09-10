//! 为 sequence Job runner 提供自包含 Rust 进程生命周期 fixture。

// 导入有界输入、输出和挂起工具。
use std::{
    // 导入标准输入行读取与标准输出写入。
    io::{BufRead, BufReader, Write},
    // 导入测试挂起线程。
    thread,
    // 导入固定挂起时长。
    time::Duration,
};

// 导入 JSON 值和构造器。
use serde_json::{Value, json};

// 固定超出生产 stdout 上限的 fixture 字节数。
const OVERSIZED_OUTPUT_BYTES: usize = 17 * 1024 * 1024;

// 从严格请求首行读取 nonce。
fn request_nonce() -> Option<String> {
    // 建立标准输入缓冲 reader。
    let mut reader = BufReader::new(std::io::stdin());
    // 保存唯一请求行。
    let mut line = String::new();
    // 读取首行失败时返回无关联。
    if reader.read_line(&mut line).ok()? == 0 {
        // EOF 没有请求。
        return None;
    }
    // 解析受测试 runner 控制的 JSON。
    let request = serde_json::from_str::<Value>(&line).ok()?;
    // 只接受字符串 nonce。
    request
        // 读取固定字段。
        .get("requestNonce")
        // 读取字符串。
        .and_then(Value::as_str)
        // 复制小关联值。
        .map(str::to_owned)
}

// 写出并刷新单个 JSON Lines 帧。
fn emit(value: &Value) -> bool {
    // 锁定唯一 stdout。
    let mut stdout = std::io::stdout().lock();
    // 序列化完整 JSON 对象。
    if serde_json::to_writer(&mut stdout, value).is_err() {
        // 序列化或写入失败。
        return false;
    }
    // 写入固定换行并刷新。
    stdout.write_all(b"\n").is_ok() && stdout.flush().is_ok()
}

// 构造 dispatch accepted 帧。
fn accepted(nonce: &str) -> Value {
    // 返回严格双阶段首帧。
    json!({
        // 固定帧类别。
        "kind": "dispatch-accepted",
        // 固定协议版本。
        "contractVersion": "act/sequence-step-worker/v1",
        // 回显可信 nonce。
        "requestNonce": nonce,
        // 标记已 accepted。
        "dispatchAccepted": true,
        // 首帧尚未完成。
        "completed": false
    })
}

// 构造确定成功 final。
fn completed(nonce: &str) -> Value {
    // 返回严格双阶段终帧。
    json!({
        // 固定终帧类别。
        "kind": "final",
        // 固定协议版本。
        "contractVersion": "act/sequence-step-worker/v1",
        // 回显可信 nonce。
        "requestNonce": nonce,
        // provider 已 accepted。
        "dispatchAccepted": true,
        // 已建立确定终态。
        "completed": true,
        // 使用成功结果类别。
        "outcome": "completed",
        // 完成后不得自动重试。
        "retrySafe": false,
        // provider 已接受。
        "acceptedMayHaveOccurred": true,
        // 返回最小结果。
        "result": { "ok": true }
    })
}

// 挂起足够长时间，必须由测试 Job 回收。
fn hang() {
    // 三十秒远大于所有 fixture deadline。
    thread::sleep(Duration::from_secs(30));
}

// 写出超过协议上限且不含换行的字节流。
fn write_oversized_output() -> bool {
    // 锁定 stdout。
    let mut stdout = std::io::stdout().lock();
    // 使用固定小块避免一次大分配。
    let chunk = [b'x'; 8192];
    // 保存累计写入量。
    let mut written = 0_usize;
    // 持续写到超过生产上限。
    while written < OVERSIZED_OUTPUT_BYTES {
        // 写入下一块；Job 回收后的断管属于预期结束。
        if stdout.write_all(&chunk).is_err() {
            // parent 已停止接收。
            return true;
        }
        // 更新累计量。
        written = written.saturating_add(chunk.len());
    }
    // 刷新全部字节。
    stdout.flush().is_ok()
}

// 按私有测试模式运行进程 fixture。
fn main() {
    // 读取唯一 fixture 模式。
    let mode = std::env::args().nth(1).unwrap_or_default();
    // 所有模式先读取父 runner 请求。
    let Some(nonce) = request_nonce() else {
        // 缺失可信请求使用结构化失败退出。
        std::process::exit(2);
    };
    // 按封闭模式建立生命周期。
    let ok = match mode.as_str() {
        // dispatch 前不输出并等待 Job 回收。
        "before-dispatch-hang" => {
            // 挂起直到 parent deadline。
            hang();
            // 理论上不会返回。
            true
        }
        // 输出 accepted 后等待 Job 回收。
        "after-accepted-hang" => {
            // 先刷新 accepted。
            let emitted = emit(&accepted(&nonce));
            // 只有写入成功才挂起。
            if emitted {
                // 等待 parent cancel/deadline。
                hang();
            }
            // 返回写入结果。
            emitted
        }
        // 输出完整成功状态机。
        "completed" => {
            // accepted 必须先成功。
            emit(&accepted(&nonce))
                // final 必须随后成功。
                && emit(&completed(&nonce))
        }
        // 输出无界单行触发 reader 上限。
        "oversized" => write_oversized_output(),
        // 未知模式失败闭合。
        _ => false,
    };
    // 使用稳定成功或 fixture 失败退出码。
    std::process::exit(if ok { 0 } else { 2 });
}
