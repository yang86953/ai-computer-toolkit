//! 为集成测试运行私有 browser-session Job 客户端。

// 导入单调时间。
use std::time::{Duration, Instant};

// 导入 JSON 构造。
use serde_json::{Value, json};

// 导入私有 Job 客户端和封闭 outcome。
use crate::components::{
    // 导入强类型页面操作与 outcome。
    browser_page_protocol::{BrowserPageOperation, BrowserPageOutcome},
    // 导入客户端与固定模式。
    browser_session_process::{self, BrowserSessionFixtureMode, BrowserSessionOpenResult},
    // 导入 outcome 枚举。
    browser_session_protocol::BrowserSessionOutcome,
};
// 导入统一错误与结果。
use crate::domain::{AppControlError, AppResult};
// 导入独立 Browser Session Module 动态 fixture。
use crate::browser_session_module_fixture;
// 把 outcome 映射为冻结文本。
fn outcome_text(outcome: BrowserSessionOutcome) -> &'static str {
    // 穷举封闭 outcome。
    match outcome {
        // 映射 ready。
        BrowserSessionOutcome::Ready => "ready",
        // 映射未派发。
        BrowserSessionOutcome::NotDispatched => "not-dispatched",
        // 映射确定失败。
        BrowserSessionOutcome::Failed => "failed",
        // 映射未知。
        BrowserSessionOutcome::Unknown => "unknown",
    }
}

// 把页面 outcome 映射为冻结文本。
fn page_outcome_text(outcome: BrowserPageOutcome) -> &'static str {
    // 穷举页面 outcome。
    match outcome {
        // 映射确定完成。
        BrowserPageOutcome::Completed => "completed",
        // 映射未派发。
        BrowserPageOutcome::NotDispatched => "not-dispatched",
        // 映射确定失败。
        BrowserPageOutcome::Failed => "failed",
        // 映射未知。
        BrowserPageOutcome::Unknown => "unknown",
    }
}

// 把打开结果投影为测试 JSON，并按需显式关闭 live 会话。
fn project(result: BrowserSessionOpenResult) -> Value {
    // 先复制结果事实。
    let outcome = result.outcome();
    // 复制完成事实。
    let completed = result.completed();
    // 复制重试事实。
    let retry_safe = result.retry_safe();
    // 复制接受事实。
    let accepted = result.accepted_may_have_occurred();
    // 复制强制回收事实。
    let forced_reap = result.forced_reap();
    // 复制可选错误码。
    let error_code = result
        // 借用错误。
        .error()
        // 读取 code。
        .and_then(|error| error.get("code"))
        // 只接受字符串。
        .and_then(Value::as_str)
        // 保存独立值。
        .map(str::to_owned);
    // 复制可选 opaque ID 形状事实。
    let session_id_valid = result
        // 借用会话 ID。
        .session_id()
        // 核对固定前缀与长度。
        .is_some_and(|value| value.starts_with("s2:bs:") && value.len() == 38);
    // 显式关闭 ready 会话。
    let graceful_close = result
        // 转移 live 所有权与关联 nonce。
        .into_session()
        // 调用真实 close。
        .map(|(session, nonce)| session.close(&nonce));
    // 返回不含原生事实的测试投影。
    json!({
        // 输出 outcome。
        "outcome": outcome_text(outcome),
        // 输出完成事实。
        "completed": completed,
        // 输出重试事实。
        "retrySafe": retry_safe,
        // 输出接受事实。
        "acceptedMayHaveOccurred": accepted,
        // 输出可选错误码。
        "errorCode": error_code,
        // 只输出 ID 形状验证。
        "sessionIdValid": session_id_valid,
        // 输出打开阶段强制回收事实。
        "forcedReap": forced_reap,
        // 输出 ready 后关闭是否优雅。
        "gracefulClose": graceful_close,
    })
}

// 通过真实生产链执行一次页面导航并显式关闭会话。
fn run_production_navigation() -> AppResult<Value> {
    // 打开工具自有隔离会话。
    let opened = browser_session_process::open_isolated(
        // 提供充足打开预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // ready 必须转交 live 会话。
    let (mut session, open_nonce) = opened.into_session().ok_or_else(|| {
        // 返回固定会话不可用错误。
        AppControlError::new(
            // 使用稳定类别。
            "BROWSER_PROTOCOL_UNAVAILABLE",
            // 输出安全诊断。
            "The production browser session did not become ready.",
        )
    })?;
    // 运行一次强类型导航。
    let page = session.execute_page(
        // 初次导航尚无 page ref。
        None,
        // 初始代际为零。
        0,
        // 使用固定 fixture URL。
        BrowserPageOperation::Navigate {
            // 不访问真实网络，只由 runtime fixture 回应。
            url: "https://example.test/page".to_owned(),
        },
        // 提供充足页面预算。
        Duration::from_secs(5),
        // 不触发取消。
        || false,
    )?;
    // 复制页面结果事实。
    let outcome = page.outcome();
    // 读取 opaque page ref 形状。
    let page_ref_valid = page
        // 借用成功数据。
        .data()
        // 定位页面引用。
        .and_then(|data| data.get("pageRef"))
        // 转换为字符串。
        .and_then(Value::as_str)
        // 核对固定私有形状。
        .is_some_and(|value| value.starts_with("w1:bp:") && value.len() == 38);
    // 显式关闭仍可信的 live 会话。
    let graceful_close = session.close(&open_nonce);
    // 返回不含原生事实的测试投影。
    Ok(json!({
        // 输出页面 outcome。
        "outcome": page_outcome_text(outcome),
        // 输出完成事实。
        "completed": page.completed(),
        // 输出重试安全事实。
        "retrySafe": page.retry_safe(),
        // 输出接受事实。
        "acceptedMayHaveOccurred": page.accepted_may_have_occurred(),
        // 输出导航代际。
        "navigationGeneration": page.navigation_generation(),
        // 只输出 page ref 形状验证。
        "pageRefValid": page_ref_valid,
        // 输出可选安全错误码。
        "errorCode": page.error().and_then(|error| error.get("code")).and_then(Value::as_str),
        // 页面完成后不得强制回收。
        "forcedReap": page.forced_reap(),
        // 会话必须仍可优雅关闭。
        "gracefulClose": graceful_close,
    }))
}

// 输出统一测试 envelope 并返回业务退出码。
fn emit(output: &Value) -> i32 {
    // 严格序列化单行 JSON。
    let Ok(text) = serde_json::to_string(output) else {
        // 序列化失败。
        return 2;
    };
    // 输出测试结果。
    println!("{text}");
    // 根据业务结果返回退出码。
    if output.get("ok").and_then(Value::as_bool) == Some(true) {
        // 测试调用成功。
        0
    } else {
        // 测试调用失败。
        2
    }
}

// 执行一个封闭 fixture 模式并向 stdout 输出单行 JSON。
pub fn run_stdio() -> i32 {
    // 读取唯一固定模式。
    let mode = std::env::args().nth(1);
    // 页面导航模式使用 live 会话聚合路径。
    if mode.as_deref() == Some("production-page-navigate") {
        // 投影页面结果或结构化错误。
        let output = match run_production_navigation() {
            // 输出页面结果。
            Ok(result) => json!({ "ok": true, "result": result }),
            // 输出安全错误。
            Err(error) => json!({
                // 标记失败。
                "ok": false,
                // 只输出稳定错误。
                "error": { "code": error.code, "message": error.message },
            }),
        };
        // 输出页面测试 envelope。
        return emit(&output);
    }
    // Module 导航模式验证公开身份、换代和显式关闭。
    if mode.as_deref() == Some("production-module-navigation") {
        // 投影 Module 结果或结构化错误。
        let output = match browser_session_module_fixture::run() {
            // 输出 Module 结果。
            Ok(result) => json!({ "ok": true, "result": result }),
            // 输出安全错误。
            Err(error) => json!({
                // 标记失败。
                "ok": false,
                // 只输出稳定错误。
                "error": { "code": error.code, "message": error.message },
            }),
        };
        // 输出 Module 测试 envelope。
        return emit(&output);
    }
    // 映射固定模式、deadline 与取消策略。
    let result = match mode.as_deref() {
        // production-ready 验证固定 sibling worker 与 runtime 的完整 Job 链。
        Some("production-ready") => browser_session_process::open_isolated(
            // 提供充足总预算。
            Duration::from_secs(5),
            // 不触发取消。
            || false,
        ),
        // ready 验证打开与显式关闭。
        Some("ready") => browser_session_process::open_fixture(
            // 使用 ready fixture。
            BrowserSessionFixtureMode::Ready,
            // 提供充足总预算。
            Duration::from_secs(5),
            // 不触发取消。
            || false,
        ),
        // accepted-only 验证未知聚合。
        Some("accepted-only") => browser_session_process::open_fixture(
            // 使用异常退出 fixture。
            BrowserSessionFixtureMode::AcceptedOnly,
            // 提供充足总预算。
            Duration::from_secs(5),
            // 不触发取消。
            || false,
        ),
        // zero-frame 验证未派发聚合。
        Some("zero-frame") => browser_session_process::open_fixture(
            // 使用零帧 fixture。
            BrowserSessionFixtureMode::ZeroFrame,
            // 提供充足总预算。
            Duration::from_secs(5),
            // 不触发取消。
            || false,
        ),
        // deadline 验证协作宽限后 Job 回收。
        Some("deadline") => browser_session_process::open_fixture(
            // 使用忽略 cancel 的挂起 fixture。
            BrowserSessionFixtureMode::HangAfterAccepted,
            // 使用短总 deadline。
            Duration::from_millis(50),
            // 不触发用户取消。
            || false,
        ),
        // cancel 验证用户取消优先触发 Job 回收。
        Some("cancel") => {
            // 记录取消计时起点。
            let started = Instant::now();
            // 运行挂起 fixture。
            browser_session_process::open_fixture(
                // 使用忽略 cancel 的挂起 fixture。
                BrowserSessionFixtureMode::HangAfterAccepted,
                // 保持 deadline 充足。
                Duration::from_secs(5),
                // 经过固定时长后报告取消。
                || started.elapsed() >= Duration::from_millis(50),
            )
        }
        // race 验证取消和 deadline 在同一轮询窗口同时成立。
        Some("race") => {
            // 记录共同停止时钟。
            let started = Instant::now();
            // 运行挂起 fixture。
            browser_session_process::open_fixture(
                // 使用忽略 cancel 的挂起 fixture。
                BrowserSessionFixtureMode::HangAfterAccepted,
                // deadline 与取消使用同一时长。
                Duration::from_millis(50),
                // 在同一时长报告用户取消。
                || started.elapsed() >= Duration::from_millis(50),
            )
        }
        // 未知模式失败闭合。
        _ => return 2,
    };
    // 把结果或结构化错误投影为 JSON。
    let output = match result {
        // 输出打开结果。
        Ok(result) => json!({ "ok": true, "result": project(result) }),
        // 输出安全错误码与消息。
        Err(error) => json!({
            // 标记失败。
            "ok": false,
            // 只输出稳定错误。
            "error": { "code": error.code, "message": error.message },
        }),
    };
    // 输出统一测试 envelope。
    emit(&output)
}
