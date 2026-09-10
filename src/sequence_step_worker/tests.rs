//! 验证 sequence step worker 的双阶段投影与输入边界。

// 导入原子布尔与游标输入。
use std::{
    // 提供内存 JSON Lines reader。
    io::{BufReader, Cursor},
    // 提供协议失败标记。
    sync::atomic::AtomicBool,
};

// 导入 JSON 构造器。
use serde_json::json;

// 导入协议 parser、统一错误和被测 worker 核心。
use crate::{
    // 导入帧观察和严格请求。
    components::sequence_step_protocol::{
        // 导入严格请求 parser。
        SequenceStepWorkerRequest,
        // 导入 stdout 状态机 parser。
        frames::{SequenceStepWorkerOutcome, parse_frame_log},
    },
    // 导入统一错误。
    domain::AppControlError,
};

// 导入被测内部函数。
use super::{execute_parsed, read_bounded_line};

// 固定测试关联值。
const NONCE: &str = "0123456789abcdef0123456789abcdef";

// 构造不触碰真实 provider 的严格请求。
fn request() -> SequenceStepWorkerRequest {
    // 构造完整 JSON 请求文本。
    let value = json!({
        // 使用固定协议版本。
        "contractVersion": "act/sequence-step-worker/v1",
        // 使用 canonical nonce。
        "requestNonce": NONCE,
        // 使用有界 deadline。
        "timeoutMs": 1000,
        // 构造 provider-neutral 命令。
        "command": {
            // 使用只读状态动词。
            "verb": "status",
            // 使用稳定 desktop 路由。
            "app": "desktop",
            // 不声明 operation。
            "operation": null,
            // 不声明目标。
            "target": {},
            // 不声明参数。
            "args": {},
            // 使用默认数量边界。
            "maxItems": 50,
            // 使用默认深度边界。
            "maxDepth": 4,
            // 只读请求无需确认。
            "confirmed": false,
            // 不允许前台影响。
            "foregroundConsent": false,
            // 保持标准隔离要求。
            "isolationRequirement": "standard"
        }
    });
    // 严格解析测试请求。
    SequenceStepWorkerRequest::parse(&value.to_string())
        // 固定 fixture 漂移应立即中止测试。
        .unwrap_or_else(|failure| panic!("request fixture failed: {:?}", failure.code()))
}

// 验证成功严格输出 accepted 后 final。
#[test]
fn success_flushes_accepted_before_completed_final() {
    // 保存内存 stdout。
    let mut output = Vec::new();
    // 初始没有 control 协议失败。
    let protocol_failed = AtomicBool::new(false);
    // 执行合成成功 System。
    let exit_code = execute_parsed(
        // 传入严格请求。
        request(),
        // 写入内存 stdout。
        &mut output,
        // 传入稳定协议状态。
        &protocol_failed,
        // 测试不注入取消。
        || false,
        // 合成 System 在返回前触发 hook。
        |_command, hook| {
            // 模拟门禁完成并进入 dispatch。
            hook()?;
            // 返回确定成功结果。
            Ok(json!({ "ok": true }))
        },
    );
    // 成功使用零退出码。
    assert_eq!(exit_code, 0);
    // 输出必须是 UTF-8。
    let text = String::from_utf8(output)
        // 测试 writer 只接收协议 JSON。
        .unwrap_or_else(|error| panic!("worker output was not UTF-8: {error}"));
    // 完整解析两帧状态机。
    let observation = parse_frame_log(&text, NONCE, false)
        // 任意顺序漂移中止测试。
        .unwrap_or_else(|failure| panic!("frame log failed: {:?}", failure.code()));
    // accepted 必须存在。
    assert!(observation.dispatch_accepted());
    // final 必须存在。
    let final_observation = observation
        // 借用 final。
        .final_observation()
        // 缺失 final 中止测试。
        .unwrap_or_else(|| panic!("completed worker omitted final"));
    // 成功结果必须保持 completed。
    assert_eq!(
        final_observation.outcome(),
        SequenceStepWorkerOutcome::Completed
    );
}

// 验证 hook 前错误只产生 not-dispatched final。
#[test]
fn pre_dispatch_error_never_emits_accepted() {
    // 保存内存 stdout。
    let mut output = Vec::new();
    // 初始没有 control 协议失败。
    let protocol_failed = AtomicBool::new(false);
    // 合成 System 在门禁阶段拒绝。
    let exit_code = execute_parsed(
        // 传入严格请求。
        request(),
        // 写入内存 stdout。
        &mut output,
        // 传入稳定协议状态。
        &protocol_failed,
        // 测试不注入取消。
        || false,
        // 不调用 hook 并返回参数拒绝。
        |_command, _hook| Err(AppControlError::new("INVALID_ARGUMENT", "rejected")),
    );
    // 结构化拒绝使用二号退出码。
    assert_eq!(exit_code, 2);
    // 解析唯一 final。
    let text = String::from_utf8(output)
        // 测试 writer 只接收协议 JSON。
        .unwrap_or_else(|error| panic!("worker output was not UTF-8: {error}"));
    // 验证完整单帧日志。
    let observation = parse_frame_log(&text, NONCE, false)
        // 任意状态漂移中止测试。
        .unwrap_or_else(|failure| panic!("frame log failed: {:?}", failure.code()));
    // 未触发 hook 就不得 accepted。
    assert!(!observation.dispatch_accepted());
    // final 必须存在。
    let final_observation = observation
        // 借用 final。
        .final_observation()
        // 缺失 final 中止测试。
        .unwrap_or_else(|| panic!("rejected worker omitted final"));
    // 结果必须可判定为未 dispatch。
    assert_eq!(
        final_observation.outcome(),
        SequenceStepWorkerOutcome::NotDispatched
    );
}

// 验证 dispatch 后未知结果保持 accepted 与 unknown。
#[test]
fn post_dispatch_unknown_preserves_uncertainty() {
    // 保存内存 stdout。
    let mut output = Vec::new();
    // 初始没有 control 协议失败。
    let protocol_failed = AtomicBool::new(false);
    // 合成 dispatch 后 OutcomeUnknown。
    let exit_code = execute_parsed(
        // 传入严格请求。
        request(),
        // 写入内存 stdout。
        &mut output,
        // 传入稳定协议状态。
        &protocol_failed,
        // 测试不注入取消。
        || false,
        // 先 accepted 再返回未知错误。
        |_command, hook| {
            // 模拟 System 跨过 dispatch 边界。
            hook()?;
            // 返回明确未知结果。
            Err(AppControlError::new("OUTCOME_UNKNOWN", "unknown"))
        },
    );
    // 未知结果使用结构化失败退出码。
    assert_eq!(exit_code, 2);
    // 解析两帧日志。
    let text = String::from_utf8(output)
        // 测试 writer 只接收协议 JSON。
        .unwrap_or_else(|error| panic!("worker output was not UTF-8: {error}"));
    // 验证完整状态机。
    let observation = parse_frame_log(&text, NONCE, false)
        // 任意状态漂移中止测试。
        .unwrap_or_else(|failure| panic!("frame log failed: {:?}", failure.code()));
    // accepted 必须保留。
    assert!(observation.dispatch_accepted());
    // final 必须存在。
    let final_observation = observation
        // 借用 final。
        .final_observation()
        // 缺失 final 中止测试。
        .unwrap_or_else(|| panic!("unknown worker omitted final"));
    // 未知结果不得降格为普通失败。
    assert_eq!(
        final_observation.outcome(),
        SequenceStepWorkerOutcome::Unknown
    );
    // 未知结果不得允许自动重试。
    assert!(!final_observation.retry_safe());
    // 必须保留可能已接受事实。
    assert!(final_observation.accepted_may_have_occurred());
}

// 验证有界读取拒绝超限帧且保留单行输入。
#[test]
fn bounded_line_reader_rejects_oversized_input() {
    // 构造刚好一行的普通输入。
    let mut valid = BufReader::new(Cursor::new(b"{}\n".to_vec()));
    // 读取上限内帧。
    let line = read_bounded_line(&mut valid, 2)
        // I/O 不应失败。
        .unwrap_or_else(|code| panic!("valid line failed: {code:?}"));
    // 必须去除行尾。
    assert_eq!(line.as_deref(), Some("{}"));
    // 构造超过两字节的输入。
    let mut oversized = BufReader::new(Cursor::new(b"123\n".to_vec()));
    // 超限必须返回错误。
    assert!(read_bounded_line(&mut oversized, 2).is_err());
}
