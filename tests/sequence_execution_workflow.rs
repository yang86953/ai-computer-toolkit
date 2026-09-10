#![cfg(target_os = "windows")]

//! 验证公开 sequence Workflow 的固定 worker 生命周期与 deadline 聚合。

// 导入 System、严格输入与公开 deadline 常量。
use ai_computer_toolkit::{
    // 导入公开 System 协调入口。
    AppControlService,
    // 导入 sequence 输入与预算边界。
    service::{
        // 导入默认步骤预算。
        DEFAULT_SEQUENCE_STEP_TIMEOUT_MS,
        // 导入最小总预算。
        MIN_SEQUENCE_TOTAL_TIMEOUT_MS,
        // 导入严格公开输入。
        SequenceInput,
    },
};
// 导入 JSON 值与构造宏。
use serde_json::{Value, json};

// 强制 Cargo 为 integration Workflow 回归构建固定 sequence step worker。
const _SEQUENCE_STEP_WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-sequence-step-worker");

// 构造一个只读且不依赖真实窗口的 desktop 状态步骤。
fn status_step(name: &str) -> Value {
    // 返回严格 step schema 接受的最小只读输入。
    json!({
        // 使用可断言的调用方名称。
        "name": name,
        // 状态读取不会修改外部状态。
        "verb": "status",
        // desktop 状态由确定性 Rust adapter 提供。
        "app": "desktop",
    })
}

// 验证正常步骤公开固定 worker 的确定完成证据。
#[test]
fn successful_step_reports_worker_execution_evidence() -> Result<(), Box<dyn std::error::Error>> {
    // 构造默认 deadline 下的单步骤只读工作流。
    let input: SequenceInput = serde_json::from_value(json!({
        // 使用确定性 desktop 状态读取。
        "steps": [status_step("status")],
    }))?;
    // 执行工作流并取得聚合结果。
    let result = AppControlService::new().sequence(input)?;
    // 正常步骤必须报告确定完成。
    assert_eq!(result["results"][0]["execution"]["outcome"], "completed");
    // final 证明 provider 已完成。
    assert_eq!(result["results"][0]["execution"]["completed"], true);
    // 已完成操作不得宣称可由框架自动重试。
    assert_eq!(result["results"][0]["execution"]["retrySafe"], false);
    // dispatch accepted 已经发生。
    assert_eq!(
        // 读取可能接受事实。
        result["results"][0]["execution"]["acceptedMayHaveOccurred"],
        // 确定 final 应保持已接受事实。
        true
    );
    // 最后可靠观察来自严格 final 帧。
    assert_eq!(
        // 读取观察阶段。
        result["results"][0]["execution"]["lastReliableObservation"],
        // 对比封闭文本。
        "final"
    );
    // 正常完成没有取消或 deadline 停止原因。
    assert_eq!(result["results"][0]["execution"]["stopReason"], Value::Null);
    // 正常完成无需 parent 强制回收。
    assert_eq!(result["results"][0]["execution"]["forcedReap"], false);
    // 测试正常完成。
    Ok(())
}

// 验证总 deadline 比步骤 deadline 更早时硬停止后续步骤。
#[test]
fn workflow_deadline_stops_sequence_without_fabricating_completion()
-> Result<(), Box<dyn std::error::Error>> {
    // 构造总预算远短于逐步预算的两个只读步骤。
    let input: SequenceInput = serde_json::from_value(json!({
        // 首步用于触发启动阶段 deadline，第二步不得执行。
        "steps": [
            // 逐步预算保持公开默认值。
            { "name": "deadline", "verb": "status", "app": "desktop", "timeoutMs": DEFAULT_SEQUENCE_STEP_TIMEOUT_MS },
            // 后续步骤用于证明硬停止。
            status_step("must-not-run")
        ],
        // 一毫秒总预算稳定早于进程启动和默认步骤预算。
        "totalTimeoutMs": MIN_SEQUENCE_TOTAL_TIMEOUT_MS,
        // 普通 provider 错误继续策略不得放宽 deadline。
        "continueOnError": true,
    }))?;
    // 执行工作流并取得生命周期聚合结果。
    let result = AppControlService::new().sequence(input)?;
    // deadline 使整体工作流失败。
    assert_eq!(result["ok"], false);
    // 仅记录已观察到停止的首步。
    assert_eq!(result["count"], 1);
    // 保留调用方请求的总步骤数。
    assert_eq!(result["total"], 2);
    // 生命周期停止单独计入工作流错误。
    assert_eq!(result["workflowErrorCount"], 1);
    // 总预算耗尽使用封闭停止原因。
    assert_eq!(
        // 读取步骤执行停止原因。
        result["results"][0]["execution"]["stopReason"],
        // 对比公开总 deadline 文本。
        "workflow-deadline-exceeded"
    );
    // deadline 不得伪造步骤完成。
    assert_eq!(result["results"][0]["execution"]["completed"], false);
    // 未知或未 dispatch 都必须保持 outcome 封闭。
    assert!(matches!(
        // 读取 outcome 文本。
        result["results"][0]["execution"]["outcome"].as_str(),
        // 启动竞态允许这两种且都不宣称完成。
        Some("not-dispatched" | "unknown")
    ));
    // 测试正常完成。
    Ok(())
}
