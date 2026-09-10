// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 导入统一请求构造器与动词。
use crate::domain::{CommandRequest, Verb};

// 导入被测 accepted 与 final 帧状态机入口。
use super::frames::{
    // 导入封闭步骤结果类别。
    SequenceStepWorkerOutcome,
    // 导入确定成功帧构造器。
    completed_frame,
    // 导入 dispatch accepted 帧构造器。
    dispatch_accepted_frame,
    // 导入 dispatch 后失败或未知帧构造器。
    failed_frame,
    // 导入 dispatch 前拒绝帧构造器。
    not_dispatched_frame,
    // 导入完整或部分 stdout 解析器。
    parse_frame_log,
};
// 导入被测 request/control 协议与共享输出边界。
use super::{
    // 导入输出总字节上限。
    MAXIMUM_OUTPUT_BYTES,
    // 导入单次 control 状态机。
    SequenceStepControlState,
    // 导入协议错误类别。
    SequenceStepProtocolErrorCode,
    // 导入 worker command。
    SequenceStepWorkerCommand,
    // 导入 worker control。
    SequenceStepWorkerControl,
    // 导入 worker request。
    SequenceStepWorkerRequest,
};

// 固定测试使用的 canonical 请求 nonce。
const REQUEST_NONCE: &str = "0123456789abcdef0123456789abcdef";
// 固定关联漂移使用的另一个 canonical nonce。
const OTHER_NONCE: &str = "fedcba9876543210fedcba9876543210";

// 编译期读取内部 JSON Schema，防止文档与运行时边界漂移。
const SCHEMA_TEXT: &str = include_str!(
    // 从 Component 目录定位项目内部契约。
    "../../contracts/internal/sequence-step-worker-v1.schema.json"
);

// 构造不依赖真实窗口的只读状态请求。
fn status_request() -> CommandRequest {
    // 复用统一请求的兼容默认值。
    CommandRequest::read(Verb::Status, "desktop")
}

// 验证内部 schema 与运行时版本、nonce 和 deadline 边界同源。
#[test]
fn schema_matches_runtime_protocol_bounds() -> Result<(), Box<dyn std::error::Error>> {
    // 解析编译期嵌入的内部 schema。
    let schema: Value = serde_json::from_str(SCHEMA_TEXT)?;
    // schema ID 必须固定到内部 v1 文件。
    assert_eq!(
        // 读取固定 schema ID。
        schema["$id"],
        // 对比完整内部契约 URL。
        "https://ai-computer-toolkit.local/contracts/internal/sequence-step-worker-v1.schema.json"
    );
    // 请求协议版本必须与运行时常量逐字一致。
    assert_eq!(
        // 读取 schema 固定版本。
        schema["$defs"]["version"]["const"],
        // 使用运行时同源版本常量。
        super::CONTRACT_VERSION
    );
    // nonce 必须固定为三十二位小写十六进制。
    assert_eq!(schema["$defs"]["nonce"]["pattern"], "^[0-9a-f]{32}$");
    // worker 剩余 deadline 最小值必须为一毫秒。
    assert_eq!(
        schema["$defs"]["request"]["properties"]["timeoutMs"]["minimum"],
        1
    );
    // worker 剩余 deadline 最大值必须为三十秒。
    assert_eq!(
        schema["$defs"]["request"]["properties"]["timeoutMs"]["maximum"],
        30_000
    );
    // 未知 final 必须绑定统一 OutcomeUnknown 错误码。
    assert_eq!(
        schema["$defs"]["unknown"]["properties"]["error"]["allOf"][1]["properties"]["code"]["const"],
        "OUTCOME_UNKNOWN"
    );
    // 确定失败必须引用排除 OutcomeUnknown 的错误定义。
    assert_eq!(
        schema["$defs"]["failed"]["properties"]["error"]["$ref"],
        "#/$defs/determinedError"
    );
    // 测试正常完成。
    Ok(())
}

// 验证四类协议失败保持稳定文本映射。
#[test]
fn protocol_error_code_strings_are_stable() {
    // 固定完整枚举与文本对照表。
    let mappings = [
        // 普通字段或 deadline 拒绝。
        (
            SequenceStepProtocolErrorCode::InvalidArgument,
            "INVALID_ARGUMENT",
        ),
        // 请求字节上限。
        (
            SequenceStepProtocolErrorCode::RequestTooLarge,
            "WORKER_REQUEST_TOO_LARGE",
        ),
        // 输出字节上限。
        (
            SequenceStepProtocolErrorCode::OutputTooLarge,
            "WORKER_OUTPUT_TOO_LARGE",
        ),
        // 关联、顺序或状态漂移。
        (
            SequenceStepProtocolErrorCode::ProtocolFailed,
            "WORKER_PROTOCOL_FAILED",
        ),
    ];
    // 完整集合必须保持四项。
    assert_eq!(mappings.len(), 4);
    // 逐项核对唯一稳定文本。
    for (code, expected) in mappings {
        // 每个枚举只允许一个公开文本。
        assert_eq!(code.as_str(), expected);
    }
}

// 把协议 JSON Lines 字节转换为 UTF-8 文本。
fn line_text(bytes: Vec<u8>) -> Result<String, Box<dyn std::error::Error>> {
    // 协议构造器必须只产生 UTF-8。
    String::from_utf8(bytes)
        // 把理论上的编码漂移转换为普通测试错误。
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
}

// 从单帧 JSON Lines 文本取得可修改对象。
fn line_value(text: &str) -> Result<Value, Box<dyn std::error::Error>> {
    // 去除唯一末尾换行并解析 JSON。
    serde_json::from_str(text.trim_end())
        // 把解析失败转换为普通测试错误。
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
}

// 验证最终绑定请求能够严格往返且不改变 System 字段。
#[test]
fn request_round_trips_provider_neutral_command() -> Result<(), Box<dyn std::error::Error>> {
    // 构造统一只读请求。
    let mut request = status_request();
    // 增加一个 provider-neutral 目标字段。
    request.target.insert(
        // 使用普通公开字段名。
        "scope".to_owned(),
        // 使用普通字符串值。
        Value::String("fixture".to_owned()),
    );
    // 增加一个 provider-neutral 参数字段。
    request.args.insert(
        // 使用普通公开字段名。
        "mode".to_owned(),
        // 使用普通字符串值。
        Value::String("read".to_owned()),
    );
    // 修改数量边界以证明精确保留。
    request.max_items = 7;
    // 修改深度边界以证明精确保留。
    request.max_depth = 2;
    // 把最终请求转换为协议命令。
    let command = SequenceStepWorkerCommand::from_request(request)
        // 合法命令不得被拒绝。
        .map_err(|failure| {
            std::io::Error::other(format!("command rejected: {:?}", failure.code()))
        })?;
    // 构造固定剩余 deadline 请求。
    let request = SequenceStepWorkerRequest::new(
        // 使用 canonical 请求 nonce。
        REQUEST_NONCE.to_owned(),
        // 使用有界剩余 deadline。
        2_500,
        // 转移已验证命令。
        command,
    )
    // 合法 worker 请求不得被拒绝。
    .map_err(|failure| std::io::Error::other(format!("request rejected: {:?}", failure.code())))?;
    // 序列化为唯一 stdin 帧。
    let line = line_text(
        // 生成有界 JSON Lines。
        request
            .to_line()
            // 合法请求必须可序列化。
            .map_err(|failure| {
                std::io::Error::other(format!("request encoding failed: {:?}", failure.code()))
            })?,
    )?;
    // JSON Lines 必须只有一个末尾换行。
    assert!(line.ends_with('\n'));
    // 严格解析同一请求。
    let parsed = SequenceStepWorkerRequest::parse(&line)
        // 合法帧必须可解析。
        .map_err(|failure| {
            std::io::Error::other(format!("request parsing failed: {:?}", failure.code()))
        })?;
    // 关联值逐字保持。
    assert_eq!(parsed.request_nonce(), REQUEST_NONCE);
    // 剩余 deadline 逐字保持。
    assert_eq!(parsed.timeout_ms(), 2_500);
    // 恢复统一 System 请求。
    let restored = parsed.into_command().into_request();
    // 动词保持只读 status。
    assert_eq!(restored.verb, Verb::Status);
    // app 路由保持不变。
    assert_eq!(restored.app, "desktop");
    // target 保持 provider-neutral 字段。
    assert_eq!(restored.target["scope"], "fixture");
    // args 保持 provider-neutral 字段。
    assert_eq!(restored.args["mode"], "read");
    // 数量边界保持不变。
    assert_eq!(restored.max_items, 7);
    // 深度边界保持不变。
    assert_eq!(restored.max_depth, 2);
    // 测试正常完成。
    Ok(())
}

// 验证请求未知字段、非法 deadline 和无界对象都在 provider 前拒绝。
#[test]
fn request_rejects_unknown_fields_deadline_and_oversized_maps()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造完整合法请求 JSON。
    let valid = json!({
        // 固定协议版本。
        "contractVersion": "act/sequence-step-worker/v1",
        // 使用 canonical nonce。
        "requestNonce": REQUEST_NONCE,
        // 使用合法 deadline。
        "timeoutMs": 1000,
        // 构造最小统一命令。
        "command": {
            // 使用只读 status。
            "verb": "status",
            // 使用注册 app。
            "app": "desktop",
            // 无 operation。
            "operation": null,
            // 空 provider-neutral 目标。
            "target": {},
            // 空 provider-neutral 参数。
            "args": {},
            // 使用兼容数量边界。
            "maxItems": 50,
            // 使用兼容深度边界。
            "maxDepth": 4,
            // 只读请求无需确认。
            "confirmed": false,
            // 只读请求无需前台同意。
            "foregroundConsent": false,
            // 保持标准隔离要求。
            "isolationRequirement": "standard"
        }
    });
    // 复制并注入顶层未知字段。
    let mut unknown = valid.clone();
    // JSON 宏固定产生对象。
    unknown
        // 取得可变对象。
        .as_object_mut()
        // 静态对象形状失败表示测试构造错误。
        .ok_or_else(|| std::io::Error::other("request fixture is not an object"))?
        // 插入协议未声明字段。
        .insert("nativeHandle".to_owned(), Value::from(7));
    // 未知字段必须在任何执行前拒绝。
    assert_eq!(
        // 解析未知字段请求。
        SequenceStepWorkerRequest::parse(&unknown.to_string())
            // 只取失败类别。
            .err()
            // 缺少错误表示测试失败。
            .ok_or_else(|| std::io::Error::other("unknown request field was accepted"))?
            // 读取封闭类别。
            .code(),
        // 应返回普通参数错误。
        SequenceStepProtocolErrorCode::InvalidArgument
    );
    // 复制并注入零 deadline。
    let mut zero_timeout = valid;
    // JSON 宏固定产生对象。
    zero_timeout["timeoutMs"] = Value::from(0);
    // 零 deadline 必须拒绝。
    assert_eq!(
        // 解析零 deadline 请求。
        SequenceStepWorkerRequest::parse(&zero_timeout.to_string())
            // 只取失败类别。
            .err()
            // 缺少错误表示测试失败。
            .ok_or_else(|| std::io::Error::other("zero timeout was accepted"))?
            // 读取封闭类别。
            .code(),
        // 应返回普通参数错误。
        SequenceStepProtocolErrorCode::InvalidArgument
    );
    // 构造 target 超过单对象边界的统一请求。
    let mut oversized = status_request();
    // 注入超过六十四 KiB 的单值。
    oversized.target.insert(
        // 使用普通字段名。
        "payload".to_owned(),
        // 构造无界测试文本。
        Value::String("x".repeat(64 * 1024)),
    );
    // 无界对象必须在 worker 启动前拒绝。
    assert_eq!(
        // 尝试构造协议命令。
        SequenceStepWorkerCommand::from_request(oversized)
            // 只取失败类别。
            .err()
            // 缺少错误表示测试失败。
            .ok_or_else(|| std::io::Error::other("oversized target was accepted"))?
            // 读取封闭类别。
            .code(),
        // 应返回请求过大。
        SequenceStepProtocolErrorCode::RequestTooLarge
    );
    // 测试正常完成。
    Ok(())
}

// 验证 cancel control 只接受当前请求 nonce 与封闭字段。
#[test]
fn cancel_control_rejects_correlation_drift_and_extensions()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造绑定当前请求的取消帧。
    let control = SequenceStepWorkerControl::cancel(REQUEST_NONCE)
        // canonical nonce 必须可构造。
        .map_err(|failure| {
            std::io::Error::other(format!("control rejected: {:?}", failure.code()))
        })?;
    // 序列化唯一 control 行。
    let line = line_text(
        // 生成 JSON Lines。
        control
            .to_line()
            // 合法 control 必须可序列化。
            .map_err(|failure| {
                std::io::Error::other(format!("control encoding failed: {:?}", failure.code()))
            })?,
    )?;
    // 当前请求 nonce 必须接受。
    assert!(SequenceStepWorkerControl::parse(&line, REQUEST_NONCE).is_ok());
    // 不同请求 nonce 必须作为关联漂移拒绝。
    assert_eq!(
        // 使用另一个期望 nonce 解析同一帧。
        SequenceStepWorkerControl::parse(&line, OTHER_NONCE)
            // 只取失败类别。
            .err()
            // 缺少错误表示测试失败。
            .ok_or_else(|| std::io::Error::other("mismatched cancel nonce was accepted"))?
            // 读取封闭类别。
            .code(),
        // 关联漂移属于协议错误。
        SequenceStepProtocolErrorCode::ProtocolFailed
    );
    // 取得可修改 control 对象。
    let mut extended = line_value(&line)?;
    // JSON 构造必须为对象。
    extended
        // 取得可变对象。
        .as_object_mut()
        // 理论漂移转换为测试错误。
        .ok_or_else(|| std::io::Error::other("control fixture is not an object"))?
        // 注入未声明 native 字段。
        .insert("nativeHandle".to_owned(), Value::from(1));
    // 未知 control 字段必须拒绝。
    assert_eq!(
        // 解析扩展帧。
        SequenceStepWorkerControl::parse(&extended.to_string(), REQUEST_NONCE)
            // 只取失败类别。
            .err()
            // 缺少错误表示测试失败。
            .ok_or_else(|| std::io::Error::other("extended cancel frame was accepted"))?
            // 读取封闭类别。
            .code(),
        // 未知字段属于参数错误。
        SequenceStepProtocolErrorCode::InvalidArgument
    );
    // 创建尚未接受 control 的状态机。
    let mut state = SequenceStepControlState::new();
    // 首个合法 cancel 必须接受。
    state
        // 消费当前请求 control。
        .accept_cancel(&line, REQUEST_NONCE)
        // 合法首帧不得失败。
        .map_err(|failure| {
            std::io::Error::other(format!("first cancel failed: {:?}", failure.code()))
        })?;
    // 状态必须提交为已取消。
    assert!(state.is_cancelled());
    // 同一请求的第二个 cancel 必须失败闭合。
    assert_eq!(
        // 尝试重复消费同一 control。
        state
            .accept_cancel(&line, REQUEST_NONCE)
            // 只取失败类别。
            .err()
            // 缺少错误表示测试失败。
            .ok_or_else(|| std::io::Error::other("duplicate cancel was accepted"))?
            // 读取封闭类别。
            .code(),
        // 重复 control 属于协议状态错误。
        SequenceStepProtocolErrorCode::ProtocolFailed
    );
    // 测试正常完成。
    Ok(())
}

// 验证 accepted 加完成 final 建立唯一确定成功事实。
#[test]
fn accepted_then_completed_is_the_only_success_sequence() -> Result<(), Box<dyn std::error::Error>>
{
    // 构造 accepted 首帧。
    let accepted = line_text(
        // 生成固定 accepted 帧。
        dispatch_accepted_frame(REQUEST_NONCE)
            // canonical nonce 必须成功。
            .map_err(|failure| {
                std::io::Error::other(format!("accepted frame failed: {:?}", failure.code()))
            })?,
    )?;
    // 构造确定成功 final。
    let completed = line_text(
        // 携带一个小结果对象。
        completed_frame(REQUEST_NONCE, json!({ "ok": true, "value": 7 }))
            // 合法结果必须成功。
            .map_err(|failure| {
                std::io::Error::other(format!("completed frame failed: {:?}", failure.code()))
            })?,
    )?;
    // 拼接两帧 stdout。
    let log = format!("{accepted}{completed}");
    // 完整退出严格解析两帧。
    let observation = parse_frame_log(&log, REQUEST_NONCE, false)
        // 合法状态机必须成功。
        .map_err(|failure| {
            std::io::Error::other(format!("frame log rejected: {:?}", failure.code()))
        })?;
    // accepted 事实必须建立。
    assert!(observation.dispatch_accepted());
    // 完整退出必须提供 final。
    let final_observation = observation
        // 借用 final。
        .final_observation()
        // 缺失 final 表示测试失败。
        .ok_or_else(|| std::io::Error::other("completed log has no final observation"))?;
    // 结果类别必须为确定完成。
    assert_eq!(
        final_observation.outcome(),
        SequenceStepWorkerOutcome::Completed
    );
    // 必须建立确定终态。
    assert!(final_observation.completed());
    // 已完成操作不应自动重试。
    assert!(!final_observation.retry_safe());
    // provider 已经接受操作。
    assert!(final_observation.accepted_may_have_occurred());
    // 完整结果必须保留。
    assert_eq!(
        final_observation.result(),
        Some(&json!({ "ok": true, "value": 7 }))
    );
    // 成功不得携带错误。
    assert!(final_observation.error().is_none());
    // 测试正常完成。
    Ok(())
}

// 验证 dispatch 前拒绝无需 accepted 且保持安全重试事实。
#[test]
fn not_dispatched_final_is_valid_without_accepted() -> Result<(), Box<dyn std::error::Error>> {
    // 构造 policy 拒绝对象。
    let error = json!({ "code": "CONFIRMATION_REQUIRED", "message": "fixture" });
    // 构造单帧 dispatch 前 final。
    let rejected = line_text(
        // 生成未 dispatch 帧。
        not_dispatched_frame(REQUEST_NONCE, error.clone())
            // 合法错误对象必须成功。
            .map_err(|failure| {
                std::io::Error::other(format!("rejection frame failed: {:?}", failure.code()))
            })?,
    )?;
    // 完整退出允许唯一 dispatch 前 final。
    let observation = parse_frame_log(&rejected, REQUEST_NONCE, false)
        // 合法拒绝必须成功。
        .map_err(|failure| {
            std::io::Error::other(format!("rejection log failed: {:?}", failure.code()))
        })?;
    // provider 未被接受。
    assert!(!observation.dispatch_accepted());
    // 取得唯一 final。
    let final_observation = observation
        // 借用 final。
        .final_observation()
        // 缺失 final 表示测试失败。
        .ok_or_else(|| std::io::Error::other("rejection log has no final observation"))?;
    // 结果类别必须为未 dispatch。
    assert_eq!(
        final_observation.outcome(),
        SequenceStepWorkerOutcome::NotDispatched
    );
    // provider 步骤没有启动或完成。
    assert!(!final_observation.completed());
    // 修正输入后可以安全重试。
    assert!(final_observation.retry_safe());
    // provider 不可能已经接受。
    assert!(!final_observation.accepted_may_have_occurred());
    // 错误对象必须完整保留。
    assert_eq!(final_observation.error(), Some(&error));
    // 测试正常完成。
    Ok(())
}

// 验证 OutcomeUnknown 保留 dispatch 后不确定性与禁止重试证据。
#[test]
fn outcome_unknown_keeps_uncertainty_after_dispatch() -> Result<(), Box<dyn std::error::Error>> {
    // 构造 accepted 首帧。
    let accepted = line_text(
        // 生成固定 accepted 帧。
        dispatch_accepted_frame(REQUEST_NONCE)
            // canonical nonce 必须成功。
            .map_err(|failure| {
                std::io::Error::other(format!("accepted frame failed: {:?}", failure.code()))
            })?,
    )?;
    // 构造 provider OutcomeUnknown 错误。
    let error = json!({
        // 使用统一未知结果错误码。
        "code": "OUTCOME_UNKNOWN",
        // 使用不含目标事实的测试消息。
        "message": "fixture",
        // 保留 provider 已接受事实。
        "acceptedMayHaveOccurred": true,
        // 明确禁止重试。
        "retrySafe": false
    });
    // 构造未知 final。
    let failed = line_text(
        // 让协议从稳定错误码分类未知结果。
        failed_frame(REQUEST_NONCE, error.clone())
            // 合法未知错误必须成功。
            .map_err(|failure| {
                std::io::Error::other(format!("unknown frame failed: {:?}", failure.code()))
            })?,
    )?;
    // 严格解析 accepted 加未知 final。
    let observation = parse_frame_log(&format!("{accepted}{failed}"), REQUEST_NONCE, false)
        // 合法未知状态必须成功。
        .map_err(|failure| {
            std::io::Error::other(format!("unknown log failed: {:?}", failure.code()))
        })?;
    // 取得唯一 final。
    let final_observation = observation
        // 借用 final。
        .final_observation()
        // 缺失 final 表示测试失败。
        .ok_or_else(|| std::io::Error::other("unknown log has no final observation"))?;
    // 结果类别必须保持未知。
    assert_eq!(
        final_observation.outcome(),
        SequenceStepWorkerOutcome::Unknown
    );
    // 未知结果不得宣称完成。
    assert!(!final_observation.completed());
    // 未知结果不得自动重试。
    assert!(!final_observation.retry_safe());
    // provider 可能已经接受。
    assert!(final_observation.accepted_may_have_occurred());
    // 原错误对象必须完整保留。
    assert_eq!(final_observation.error(), Some(&error));
    // 测试正常完成。
    Ok(())
}

// 验证 Job 终止后的部分输出只保留最后可靠 accepted 事实。
#[test]
fn partial_log_distinguishes_before_and_after_dispatch() -> Result<(), Box<dyn std::error::Error>> {
    // 零帧部分输出表示没有可靠 dispatch 事实。
    let before = parse_frame_log("", REQUEST_NONCE, true)
        // 合法零帧部分输出必须成功。
        .map_err(|failure| {
            std::io::Error::other(format!("empty partial log failed: {:?}", failure.code()))
        })?;
    // 不得伪造 accepted。
    assert!(!before.dispatch_accepted());
    // 不得伪造 final。
    assert!(before.final_observation().is_none());
    // 构造 accepted-only 部分输出。
    let accepted = line_text(
        // 生成固定 accepted 帧。
        dispatch_accepted_frame(REQUEST_NONCE)
            // canonical nonce 必须成功。
            .map_err(|failure| {
                std::io::Error::other(format!("accepted frame failed: {:?}", failure.code()))
            })?,
    )?;
    // Job 终止时允许 accepted-only。
    let after = parse_frame_log(&accepted, REQUEST_NONCE, true)
        // 合法部分输出必须成功。
        .map_err(|failure| {
            std::io::Error::other(format!("accepted partial log failed: {:?}", failure.code()))
        })?;
    // 必须保留 dispatch 后事实。
    assert!(after.dispatch_accepted());
    // 不得伪造 final。
    assert!(after.final_observation().is_none());
    // 正常完整退出不允许 accepted-only。
    assert_eq!(
        // 严格解析同一输出。
        parse_frame_log(&accepted, REQUEST_NONCE, false)
            // 只取失败类别。
            .err()
            // 缺少错误表示测试失败。
            .ok_or_else(|| std::io::Error::other("accepted-only complete log was accepted"))?
            // 读取封闭类别。
            .code(),
        // 应返回协议状态错误。
        SequenceStepProtocolErrorCode::ProtocolFailed
    );
    // 测试正常完成。
    Ok(())
}

// 验证重复、乱序、关联漂移和非法字段组合都失败闭合。
#[test]
fn frame_log_rejects_duplicates_order_drift_and_invalid_shape()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造 accepted 首帧。
    let accepted = line_text(
        // 生成固定 accepted 帧。
        dispatch_accepted_frame(REQUEST_NONCE)
            // canonical nonce 必须成功。
            .map_err(|failure| {
                std::io::Error::other(format!("accepted frame failed: {:?}", failure.code()))
            })?,
    )?;
    // 构造确定成功 final。
    let completed = line_text(
        // 使用小结果对象。
        completed_frame(REQUEST_NONCE, json!({ "ok": true }))
            // 合法结果必须成功。
            .map_err(|failure| {
                std::io::Error::other(format!("completed frame failed: {:?}", failure.code()))
            })?,
    )?;
    // 两个 accepted 超过封闭状态机。
    assert!(parse_frame_log(&format!("{accepted}{accepted}"), REQUEST_NONCE, true).is_err());
    // final 在 accepted 前违反顺序与 dispatch 事实。
    assert!(parse_frame_log(&format!("{completed}{accepted}"), REQUEST_NONCE, true).is_err());
    // dispatch 后 final 不能缺少 accepted 首帧。
    assert!(parse_frame_log(&completed, REQUEST_NONCE, false).is_err());
    // 当前请求不得接受其他 nonce 的 accepted 帧。
    let other = line_text(
        // 生成另一个请求的 accepted 帧。
        dispatch_accepted_frame(OTHER_NONCE)
            // canonical nonce 必须成功。
            .map_err(|failure| {
                std::io::Error::other(format!("other accepted failed: {:?}", failure.code()))
            })?,
    )?;
    // 关联漂移必须拒绝。
    assert!(parse_frame_log(&other, REQUEST_NONCE, true).is_err());
    // 取得可修改成功 final 对象。
    let mut invalid = line_value(&completed)?;
    // 把成功结果的 completed 伪造为 false。
    invalid["completed"] = Value::Bool(false);
    // 与 accepted 拼接后仍必须拒绝非法字段组合。
    assert!(
        parse_frame_log(
            // 构造 accepted 加非法 final。
            &format!("{accepted}{}\n", invalid),
            // 使用当前请求 nonce。
            REQUEST_NONCE,
            // 要求完整退出。
            false,
        )
        // 状态漂移必须失败。
        .is_err()
    );
    // 构造合法 OutcomeUnknown final 供错误码漂移测试。
    let unknown = line_text(
        // 使用统一未知结果错误码。
        failed_frame(REQUEST_NONCE, json!({ "code": "OUTCOME_UNKNOWN" }))
            // 合法未知结果必须成功。
            .map_err(|failure| {
                std::io::Error::other(format!("unknown frame failed: {:?}", failure.code()))
            })?,
    )?;
    // 取得可修改未知 final 对象。
    let mut mismatched_error = line_value(&unknown)?;
    // 把错误码改成确定失败但保留 unknown outcome。
    mismatched_error["error"]["code"] = Value::String("OPERATION_FAILED".to_owned());
    // 错误码与 outcome 漂移必须拒绝。
    assert!(
        parse_frame_log(
            // 构造 accepted 加漂移 final。
            &format!("{accepted}{}\n", mismatched_error),
            // 使用当前请求 nonce。
            REQUEST_NONCE,
            // 要求完整退出。
            false,
        )
        // 状态漂移必须失败。
        .is_err()
    );
    // dispatch 前拒绝必须携带稳定对象错误码。
    assert_eq!(
        // 尝试用非对象错误构造拒绝。
        not_dispatched_frame(REQUEST_NONCE, Value::String("invalid".to_owned()))
            // 只取失败类别。
            .err()
            // 缺少错误表示测试失败。
            .ok_or_else(|| std::io::Error::other("non-object rejection error was accepted"))?
            // 读取封闭类别。
            .code(),
        // 构造点必须返回协议失败。
        SequenceStepProtocolErrorCode::ProtocolFailed
    );
    // 测试正常完成。
    Ok(())
}

// 验证超大 final 负载在写入 stdout 前被完整拒绝。
#[test]
fn final_frame_enforces_output_budget() -> Result<(), Box<dyn std::error::Error>> {
    // 构造超过全部 stdout 上限的单字符串结果。
    let result = Value::String("x".repeat(MAXIMUM_OUTPUT_BYTES));
    // 结果不得被截断或写出。
    assert_eq!(
        // 尝试构造超大完成帧。
        completed_frame(REQUEST_NONCE, result)
            // 只取失败类别。
            .err()
            // 缺少错误表示测试失败。
            .ok_or_else(|| std::io::Error::other("oversized final frame was accepted"))?
            // 读取封闭类别。
            .code(),
        // 应返回输出过大。
        SequenceStepProtocolErrorCode::OutputTooLarge
    );
    // 测试正常完成。
    Ok(())
}
