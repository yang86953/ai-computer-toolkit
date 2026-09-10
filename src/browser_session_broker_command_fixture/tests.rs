//! 验证 browser-session broker fixture 的纯 stdin operation 解析边界。

// 导入 JSON 构造宏。
use serde_json::json;

// 导入被测私有命令与纯解析函数。
use super::{FixtureCommand, parse_command};

// 固定 canonical 的公开 browser session identity。
const SESSION_ID: &str = "s2:bs:0123456789abcdef0123456789abcdef";

// 验证精确 open 输入可构造封闭命令，且不触碰 broker。
#[test]
fn accepts_exact_open_command() {
    // 只构造 open 所允许的两个字段。
    let command = parse_command(json!({ "operation": "open", "timeoutMs": 1_000 }));
    // 只接受已经范围校验的 open 命令。
    assert!(matches!(
        command,
        Ok(FixtureCommand::Open { timeout_ms: 1_000 })
    ));
}

// 验证精确 close 输入可保留公开 opaque session 与总预算。
#[test]
fn accepts_exact_close_command() {
    // 只构造 close 所允许的三个字段。
    let command = parse_command(json!({
        "operation": "close",
        "sessionId": SESSION_ID,
        "timeoutMs": 30_000,
    }));
    // 匹配已解析的 close 命令。
    match command {
        // 核对公开 session 与范围上界均被保留。
        Ok(FixtureCommand::Close {
            session_id,
            timeout_ms,
        }) => {
            // session 必须逐字保留。
            assert_eq!(session_id, SESSION_ID);
            // timeout 必须保留为已校验 u32。
            assert_eq!(timeout_ms, 30_000);
        }
        // 其余结果代表 exact close 契约失效。
        _ => panic!("exact close command must parse"),
    }
}

// 验证精确 inspect 输入可保留公开 opaque session 与总预算。
#[test]
fn accepts_exact_inspect_query() {
    // 只构造 inspect 允许的三个字段。
    let command = parse_command(json!({
        // 使用只读查询 operation。
        "operation": "inspect",
        // 绑定 canonical browser-session identity。
        "sessionId": SESSION_ID,
        // 使用协议范围内预算。
        "timeoutMs": 1_000,
    }));
    // 匹配已解析的 inspect 查询。
    match command {
        // 只接受目标和预算逐字保留的封闭查询。
        Ok(FixtureCommand::Inspect {
            // 解构公开 session。
            session_id,
            // 解构总预算。
            timeout_ms,
        }) => {
            // session 不得被 fixture 转换或扩展。
            assert_eq!(session_id, SESSION_ID);
            // timeout 必须保留为已校验 u32。
            assert_eq!(timeout_ms, 1_000);
        }
        // 其余结果代表 inspect 输入边界漂移。
        _ => panic!("exact inspect query must parse"),
    }
}

// 验证额外字段不会穿过 fixture 的精确键集合门禁。
#[test]
fn rejects_extra_input_field() {
    // 添加任何额外字段都必须拒绝。
    let command = parse_command(json!({
        "operation": "open",
        "timeoutMs": 1_000,
        "brokerEpoch": "0123456789abcdef0123456789abcdef",
    }));
    // 不允许得到可执行命令。
    assert!(command.is_err());
}

// 验证零 timeout 在连接或启动 broker 前被拒绝。
#[test]
fn rejects_zero_timeout() {
    // 构造低于协议下界的 timeout。
    let command = parse_command(json!({ "operation": "open", "timeoutMs": 0 }));
    // 不允许得到可执行命令。
    assert!(command.is_err());
}

// 验证超过协议上界的 timeout 在连接或启动 broker 前被拒绝。
#[test]
fn rejects_timeout_above_maximum() {
    // 构造高于协议上界的 timeout。
    let command = parse_command(json!({ "operation": "open", "timeoutMs": 30_001 }));
    // 不允许得到可执行命令。
    assert!(command.is_err());
}

// 验证 timeout 必须是 JSON 整数而不是字符串或浮点数。
#[test]
fn rejects_non_integer_timeout() {
    // 构造字符串 timeout。
    let string_command = parse_command(json!({ "operation": "open", "timeoutMs": "1000" }));
    // 字符串不得穿过 u32 门禁。
    assert!(string_command.is_err());
    // 构造浮点 timeout。
    let decimal_command = parse_command(json!({ "operation": "open", "timeoutMs": 1.5 }));
    // 浮点数不得穿过 u32 门禁。
    assert!(decimal_command.is_err());
}

// 验证大写十六进制 session 后缀不被接受。
#[test]
fn rejects_uppercase_session_id() {
    // 构造包含大写十六进制字符的 session。
    let command = parse_command(json!({
        "operation": "close",
        "sessionId": "s2:bs:0123456789abcdef0123456789abcdeF",
        "timeoutMs": 1_000,
    }));
    // 大写漂移不得进入 Adapter。
    assert!(command.is_err());
}

// 验证 session 后缀长度必须精确为 128 位十六进制。
#[test]
fn rejects_wrong_length_session_id() {
    // 构造少一位的 session 后缀。
    let command = parse_command(json!({
        "operation": "close",
        "sessionId": "s2:bs:0123456789abcdef0123456789abcde",
        "timeoutMs": 1_000,
    }));
    // 长度漂移不得进入 Adapter。
    assert!(command.is_err());
}

// 验证 session 必须使用 browser-session 专属公开前缀。
#[test]
fn rejects_wrong_session_prefix() {
    // 构造使用其他公开 ID 前缀的值。
    let command = parse_command(json!({
        "operation": "close",
        "sessionId": "s2:bp:0123456789abcdef0123456789abcdef",
        "timeoutMs": 1_000,
    }));
    // 前缀漂移不得进入 Adapter。
    assert!(command.is_err());
}
