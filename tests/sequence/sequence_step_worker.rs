#![cfg(target_os = "windows")]

//! 验证固定 sequence step worker 二进制的真实 stdio 路由。

// 导入子进程管道与写入接口。
use std::{
    // 写入 worker stdin。
    io::Write,
    // 创建固定 Cargo 构建产物。
    process::{Command, Stdio},
};

// 导入 JSON 构造器与值类型。
use serde_json::{Value, json};

// 固定测试关联值。
const NONCE: &str = "0123456789abcdef0123456789abcdef";

// 启动 Cargo 提供的固定 worker 二进制并写入完整 stdin。
fn run_worker(input: &[u8]) -> std::process::Output {
    // 创建无参数固定 worker 进程。
    let mut child = Command::new(env!(
        "CARGO_BIN_EXE_ai-computer-toolkit-sequence-step-worker"
    ))
    // 不允许调用方传递任意 argv。
    .stdin(Stdio::piped())
    // 捕获唯一协议 stdout。
    .stdout(Stdio::piped())
    // 捕获必须为空的 stderr。
    .stderr(Stdio::piped())
    // 启动测试 worker。
    .spawn()
    // 固定测试环境缺少二进制时中止。
    .unwrap_or_else(|error| panic!("sequence step worker failed to start: {error}"));
    // 取得唯一 stdin 写端。
    let mut stdin = child
        // 转移可选管道。
        .stdin
        // 缺失管道表示测试装配漂移。
        .take()
        // 中止不完整装配。
        .unwrap_or_else(|| panic!("sequence step worker stdin was unavailable"));
    // 写入完整 JSON Lines 输入。
    stdin
        // 使用调用方提供的测试字节。
        .write_all(input)
        // 管道提前关闭时中止。
        .unwrap_or_else(|error| panic!("sequence step worker stdin failed: {error}"));
    // 关闭 stdin 让 control reader 观察 EOF。
    drop(stdin);
    // 等待固定 worker 退出并收集输出。
    child
        // 使用标准库完整回收子进程。
        .wait_with_output()
        // 等待失败时中止。
        .unwrap_or_else(|error| panic!("sequence step worker wait failed: {error}"))
}

// 构造无目标 desktop 状态请求。
fn valid_request() -> Vec<u8> {
    // 构造严格协议对象。
    let request = json!({
        // 使用固定协议版本。
        "contractVersion": "act/sequence-step-worker/v1",
        // 使用 canonical nonce。
        "requestNonce": NONCE,
        // 使用有界 deadline。
        "timeoutMs": 1000,
        // 使用 provider-neutral 命令。
        "command": {
            // 执行只读状态查询。
            "verb": "status",
            // 选择稳定 desktop provider。
            "app": "desktop",
            // 状态请求没有 operation。
            "operation": null,
            // 状态请求没有目标。
            "target": {},
            // 状态请求没有参数。
            "args": {},
            // 保持默认结果数量。
            "maxItems": 50,
            // 保持默认层级深度。
            "maxDepth": 4,
            // 只读请求无需确认。
            "confirmed": false,
            // 不允许前台影响。
            "foregroundConsent": false,
            // 使用兼容标准隔离。
            "isolationRequirement": "standard"
        }
    });
    // 序列化固定对象。
    let mut bytes = serde_json::to_vec(&request)
        // 固定 JSON 不应序列化失败。
        .unwrap_or_else(|error| panic!("request fixture serialization failed: {error}"));
    // 追加唯一 JSON Lines 换行。
    bytes.push(b'\n');
    // 返回完整 stdin。
    bytes
}

// 验证真实二进制严格输出 accepted 后 completed final。
#[test]
fn fixed_worker_emits_two_phase_frames_for_desktop_status() {
    // 执行无副作用状态请求。
    let output = run_worker(&valid_request());
    // 确定成功必须使用零退出码。
    assert!(output.status.success());
    // worker 不得写入诊断 stderr。
    assert!(output.stderr.is_empty());
    // stdout 必须是 UTF-8 JSON Lines。
    let text = String::from_utf8(output.stdout)
        // 编码漂移中止测试。
        .unwrap_or_else(|error| panic!("worker stdout was not UTF-8: {error}"));
    // 按行读取两帧。
    let frames = text
        // 去除唯一末尾换行。
        .lines()
        // 严格解析每个 JSON 对象。
        .map(|line| {
            // 解析单帧。
            serde_json::from_str::<Value>(line)
                // 非 JSON 中止测试。
                .unwrap_or_else(|error| panic!("worker frame was invalid: {error}"))
        })
        // 收集固定两帧。
        .collect::<Vec<_>>();
    // 只允许 accepted 和 final。
    assert_eq!(frames.len(), 2);
    // 首帧必须是已刷新 accepted。
    assert_eq!(frames[0]["kind"], "dispatch-accepted");
    // 两帧必须保持同一关联值。
    assert_eq!(frames[0]["requestNonce"], NONCE);
    // 终帧必须是确定完成。
    assert_eq!(frames[1]["kind"], "final");
    // 状态查询成功必须是 completed。
    assert_eq!(frames[1]["outcome"], "completed");
}

// 验证无可信 nonce 的请求拒绝不伪造 final。
#[test]
fn malformed_request_exits_without_protocol_frames() {
    // 写入未知字段和无效 nonce。
    let output = run_worker(b"{\"requestNonce\":\"bad\",\"unknown\":true}\n");
    // transport 拒绝必须非零退出。
    assert!(!output.status.success());
    // 不存在可信关联时 stdout 必须为空。
    assert!(output.stdout.is_empty());
    // 拒绝也不得泄漏诊断。
    assert!(output.stderr.is_empty());
}
