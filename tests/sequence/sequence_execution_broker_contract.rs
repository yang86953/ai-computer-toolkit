//! 冻结 sequence execution broker、journal、resume 与重复保护协议。

// 导入 JSON 值类型。
use serde_json::Value;

// 解析固定 broker frame Schema。
fn broker_schema() -> serde_json::Result<Value> {
    // 读取并解析内部 broker v1 Schema。
    serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../../contracts/internal/sequence-execution-broker-v1.schema.json"
    ))
}

// 解析原子 journal record Schema。
fn journal_schema() -> serde_json::Result<Value> {
    // 读取并解析内部 journal v1 Schema。
    serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../../contracts/internal/sequence-execution-journal-v1.schema.json"
    ))
}

// 验证持久身份、记录版本和容量边界不可漂移。
#[test]
fn journal_identity_and_capacity_are_frozen() -> serde_json::Result<()> {
    // 解析 journal Schema。
    let schema = journal_schema()?;
    // journal 根只接受严格 record 定义。
    assert_eq!(schema["$ref"], "#/$defs/journalRecord");
    // 固定持久记录契约版本。
    assert_eq!(
        // 读取版本常量。
        schema["$defs"]["journalRecord"]["properties"]["contractVersion"]["const"],
        // 对比冻结版本。
        "act/sequence-execution-journal/v1"
    );
    // execution identity 使用独立 opaque 类别和 128 位随机文本。
    assert_eq!(
        // 读取 executionId pattern。
        schema["$defs"]["executionId"]["pattern"],
        // 对比 canonical 形状。
        "^s2:q:[0-9a-f]{32}$"
    );
    // step identity 与 execution identity 不混用。
    assert_eq!(
        // 读取 stepId pattern。
        schema["$defs"]["stepId"]["pattern"],
        // 对比 canonical 形状。
        "^s2:qs:[0-9a-f]{32}$"
    );
    // 单个 execution 仍受公开 64 步边界限制。
    assert_eq!(
        // 读取 receipt 最大数量。
        schema["$defs"]["executionSnapshot"]["properties"]["steps"]["maxItems"],
        // 对比公开步骤上限。
        64
    );
    // 显式 forget 的紧凑 tombstone 也有固定容量。
    assert_eq!(
        // 读取 tombstone index 数量上限。
        schema["$defs"]["forgottenIndex"]["properties"]["records"]["maxItems"],
        // 对比固定去重保留容量。
        4096
    );
    // 测试正常完成。
    Ok(())
}

// 验证 execution 与 step 状态机保持封闭且区分未知终态。
#[test]
fn execution_and_step_states_preserve_outcome_truth() -> serde_json::Result<()> {
    // 解析 journal Schema。
    let schema = journal_schema()?;
    // 冻结八个 execution 状态。
    assert_eq!(
        // 读取 execution 状态枚举。
        schema["$defs"]["executionState"]["enum"],
        // 对比完整封闭顺序。
        serde_json::json!([
            // 输入已持久化。
            "created",
            // 当前步骤正在推进。
            "running",
            // 安全检查点等待恢复。
            "awaiting-resume",
            // 为动态确认保留。
            "awaiting-confirmation",
            // 确定成功终态。
            "completed",
            // 确定失败终态。
            "failed",
            // 权威取消终态。
            "cancelled",
            // dispatch 后未知终态。
            "outcome-unknown"
        ])
    );
    // 冻结六个 step receipt 状态。
    assert_eq!(
        // 读取 step 状态枚举。
        schema["$defs"]["stepState"]["enum"],
        // 对比完整封闭顺序。
        serde_json::json!([
            // 尚未物化。
            "pending",
            // 已物化但未 dispatch。
            "prepared",
            // 已建立可能接受事实。
            "dispatching",
            // 确定成功。
            "completed",
            // 确定失败。
            "failed",
            // 终态未知。
            "outcome-unknown"
        ])
    );
    // 测试正常完成。
    Ok(())
}

// 验证 broker 只接受固定动作、边界字段和双 revision 响应。
#[test]
fn broker_frames_are_closed_and_revisioned() -> serde_json::Result<()> {
    // 解析 broker Schema。
    let schema = broker_schema()?;
    // 根 frame 只包含八个封闭类别。
    assert_eq!(
        // 读取 oneOf 分支数量。
        schema["oneOf"].as_array().map(Vec::len),
        // ready、五请求和两响应。
        Some(8)
    );
    // start 输入复用版本化公开 sequence Schema。
    assert_eq!(
        // 读取 input 引用。
        schema["$defs"]["startRequest"]["properties"]["input"]["$ref"],
        // 对比固定公开 Schema 路径：相对本文件所在的 contracts/internal/ 解析，
        // ../v1/ 才落在 contracts/v1/；../../v1/ 会解析到仓库根下的 v1/ 而落空。
        // 同目录的 sequence-execution-journal-v1 用的是同一条相对路径。
        "../v1/sequence-input.schema.json"
    );
    // resume 必须使用 expected execution revision。
    assert!(
        schema["$defs"]["resumeRequest"]["required"]
            // 读取必需字段数组。
            .as_array()
            // 检查 revision 字段。
            .is_some_and(|fields| {
                // 查找精确字段名。
                fields.contains(&Value::String("expectedExecutionRevision".to_owned()))
            })
    );
    // forget 是唯一要求显式 true 确认的清理动作。
    assert_eq!(
        // 读取 confirmed 常量。
        schema["$defs"]["forgetRequest"]["properties"]["confirmed"]["const"],
        // 对比显式确认。
        true
    );
    // business accepted 固定从 request revision 零开始。
    assert_eq!(
        // 读取 accepted revision。
        schema["$defs"]["acceptedResponse"]["properties"]["requestRevision"]["const"],
        // 对比首个 revision。
        0
    );
    // 最终响应只能是业务前 revision 零或 accepted 后 revision 一。
    assert_eq!(
        // 读取 final revision 候选。
        schema["$defs"]["finalResponse"]["properties"]["requestRevision"]["enum"],
        // 对比封闭 revision。
        serde_json::json!([0, 1])
    );
    // wire fingerprint 固定为 64 位小写十六进制。
    assert_eq!(
        // 读取 fingerprint pattern。
        schema["$defs"]["fingerprint"]["pattern"],
        // 对比 16 hex 形状。
        "^[0-9a-f]{16}$"
    );
    // 测试正常完成。
    Ok(())
}
