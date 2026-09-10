//! 无参数 accepted-only worker 固定 sibling 替身，仅供 broker 恢复集成测试复制使用。

// 导入有界 JSON Lines 读取与刷新写入 trait。
use std::io::{BufRead, Write};

// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 固定真实 worker 协议版本。
const CONTRACT_VERSION: &str = "act/browser-session-worker/v1";
// 固定一次性 request nonce 长度。
const NONCE_LENGTH: usize = 32;
// 固定真实 worker 单行输入资源上限。
const MAXIMUM_INPUT_BYTES: usize = 8 * 1024;

// 验证 JSON 对象只含冻结字段集合。
fn exact_keys(value: &Value, expected: &[&str]) -> bool {
    // 只接受 JSON 对象。
    let Some(object) = value.as_object() else {
        // 非对象不能成为协议帧。
        return false;
    };
    // 字段数量和名称必须同时精确匹配。
    object.len() == expected.len()
        // 禁止任何未知或遗漏字段。
        && object
            // 遍历所有字段名。
            .keys()
            // 只接受冻结字段集合。
            .all(|key| expected.contains(&key.as_str()))
}

// 向 stdout 写入并 flush 一条严格 worker frame。
fn write_frame(frame: &Value) -> bool {
    // 序列化固定 JSON frame。
    let Ok(text) = serde_json::to_string(frame) else {
        // 序列化失败不能继续协议。
        return false;
    };
    // 取得 stdout 唯一写锁。
    let stdout = std::io::stdout();
    // 锁定输出以避免 frame 交错。
    let mut output = stdout.lock();
    // 写入一行并立刻 flush 给真实 parent reader。
    writeln!(output, "{text}").is_ok()
        // 只有成功写入才尝试刷新。
        && output.flush().is_ok()
}

// 验证 worker open request nonce 的 canonical 小写十六进制形状。
fn canonical_nonce(value: &str) -> bool {
    // 只接受 128 位小写十六进制值。
    value.len() == NONCE_LENGTH
        // 拒绝大小写、分隔符、空白和非 ASCII 字符。
        && value
            // 按字节检查 canonical hex。
            .bytes()
            // 仅允许数字和小写 a 到 f。
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 从真实 worker open frame 读取并验证 request nonce。
fn read_open_nonce() -> Option<String> {
    // 取得 stdin 唯一读锁。
    let stdin = std::io::stdin();
    // 锁定首条 worker 输入。
    let mut input = stdin.lock();
    // 保存首条 JSON Lines frame。
    let mut line = String::new();
    // EOF 不能建立 worker session。
    if input.read_line(&mut line).ok()? == 0 {
        // 关闭失败。
        return None;
    }
    // 拒绝超过生产协议资源边界的输入。
    if line.len() > MAXIMUM_INPUT_BYTES {
        // 超限输入不能进入字段解析。
        return None;
    }
    // 解析唯一 JSON 值。
    let value = serde_json::from_str::<Value>(line.trim_end()).ok()?;
    // open 根对象必须只含生产协议五个字段。
    if !exact_keys(
        // 核对完整 open frame。
        &value,
        // 冻结字段顺序只用于审阅，JSON 本身不依赖顺序。
        &[
            "kind",
            "contractVersion",
            "requestNonce",
            "timeoutMs",
            "source",
        ],
    ) {
        // 拒绝 parent wire 扩展或缺失。
        return None;
    }
    // 首条输入必须为固定 open 与真实协议版本。
    if value.get("kind").and_then(Value::as_str) != Some("open")
        // 严格拒绝协议版本漂移。
        || value.get("contractVersion").and_then(Value::as_str) != Some(CONTRACT_VERSION)
    {
        // 协议漂移失败闭合。
        return None;
    }
    // deadline 必须保持生产协议冻结闭区间。
    if !matches!(
        value.get("timeoutMs").and_then(Value::as_u64),
        Some(1..=30_000)
    ) {
        // 拒绝缺失、浮点、负数和越界预算。
        return None;
    }
    // source 必须存在且保持隔离 profile 的封闭对象。
    let source = value.get("source")?;
    // source 只允许唯一 kind 字段。
    if !exact_keys(source, &["kind"])
        // 只接受生产 broker 当前授权的隔离 profile。
        || source.get("kind").and_then(Value::as_str) != Some("isolated-profile")
    {
        // 任意 endpoint、profile 参数或未知来源失败闭合。
        return None;
    }
    // 读取并复制关联 nonce。
    let nonce = value
        .get("requestNonce")
        .and_then(Value::as_str)?
        .to_owned();
    // 只接受真实 parent 生成的 canonical nonce。
    canonical_nonce(&nonce).then_some(nonce)
}

// 构造真实协议 accepted frame。
fn accepted(nonce: &str) -> Value {
    // 返回与 parent parser 对齐的 accepted shape。
    json!({
        // 声明 accepted kind。
        "kind": "open-accepted",
        // 回显协议版本。
        "contractVersion": CONTRACT_VERSION,
        // 回显 canonical request nonce。
        "requestNonce": nonce,
        // 声明 worker 已接受 dispatch。
        "dispatchAccepted": true,
        // accepted 还不是终态。
        "completed": false,
    })
}

// 运行无参数 fixed sibling worker 并等待父 Component 的 Job 强制回收。
fn main() {
    // 专用恢复替身不接受 argv、模式、路径或环境覆盖。
    if std::env::args_os().nth(1).is_some() {
        // 参数漂移使用固定失败码。
        std::process::exit(2);
    }
    // 读取并严格验证唯一 open request。
    let Some(nonce) = read_open_nonce() else {
        // 输入协议失败。
        std::process::exit(2);
    };
    // 先报告真实 accepted 事实。
    if !write_frame(&accepted(&nonce)) {
        // parent 已断开。
        std::process::exit(2);
    }
    // 永久 park，忽略 stdin EOF、cancel 和任何自愿终态路径。
    loop {
        // 只由父 Component 的 Job 关闭负责结束进程。
        std::thread::park();
    }
}
