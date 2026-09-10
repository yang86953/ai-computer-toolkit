//! 聚合 ready 浏览器会话中的单个页面命令生命周期。

// 导入通道、deadline 与 JSON 结果工具。
use std::{
    // 导入通道超时分类。
    sync::mpsc::RecvTimeoutError,
    // 导入单调时间。
    time::{Duration, Instant},
};

// 导入中立 JSON 值。
use serde_json::Value;

// 导入冻结页面协议。
use crate::components::browser_page_protocol::{
    // 导入协议模块供输出观察。
    self,
    // 导入强类型页面操作。
    BrowserPageOperation,
    // 导入封闭页面 outcome。
    BrowserPageOutcome,
    // 导入页面 worker 输入。
    BrowserPageWorkerInput,
    // 导入冻结版本。
    CONTRACT_VERSION,
};

// 导入父 Component 的进程所有权与私有原语。
use super::{
    // 导入统一错误与结果。
    AppControlError,
    AppResult,
    // 导入 live 会话所有者。
    BrowserSessionProcess,
    // 导入取消宽限。
    CANCEL_GRACE,
    // 导入 reader 事件。
    ReaderEvent,
    // 导入进程等待状态。
    WAIT_OBJECT_0,
    // 导入轮询粒度。
    WAIT_SLICE,
    // 导入 Windows 等待函数。
    WaitForSingleObject,
    // 导入随机请求 nonce。
    random_nonce,
    // 导入持久输入写入器。
    write_input,
};

// 保存 parent 对一个页面命令的可信聚合结果。
pub(crate) struct BrowserPageCommandResult {
    // 保存封闭 outcome。
    outcome: BrowserPageOutcome,
    // 保存完成事实。
    completed: bool,
    // 保存重试安全事实。
    retry_safe: bool,
    // 保存可能接受事实。
    accepted_may_have_occurred: bool,
    // 保存导航代际。
    navigation_generation: u64,
    // 保存可选成功数据。
    data: Option<Value>,
    // 保存可选安全错误。
    error: Option<Value>,
    // 保存 parent 是否强制回收会话。
    forced_reap: bool,
    // 保存当前会话是否必须从 live registry 失效并转入回收。
    session_invalidated: bool,
}

// 为页面命令结果提供 provider-neutral 只读投影。
impl BrowserPageCommandResult {
    // 返回封闭 outcome。
    pub(crate) const fn outcome(&self) -> BrowserPageOutcome {
        // 复制枚举。
        self.outcome
    }

    // 返回完成事实。
    pub(crate) const fn completed(&self) -> bool {
        // 复制布尔值。
        self.completed
    }

    // 返回重试安全事实。
    pub(crate) const fn retry_safe(&self) -> bool {
        // 复制布尔值。
        self.retry_safe
    }

    // 返回可能接受事实。
    pub(crate) const fn accepted_may_have_occurred(&self) -> bool {
        // 复制布尔值。
        self.accepted_may_have_occurred
    }

    // 返回导航代际。
    pub(crate) const fn navigation_generation(&self) -> u64 {
        // 复制代际。
        self.navigation_generation
    }

    // 返回可选成功数据。
    pub(crate) const fn data(&self) -> Option<&Value> {
        // 借用数据。
        self.data.as_ref()
    }

    // 返回可选安全错误。
    pub(crate) const fn error(&self) -> Option<&Value> {
        // 借用错误。
        self.error.as_ref()
    }

    // 返回 parent 是否强制回收会话。
    pub(crate) const fn forced_reap(&self) -> bool {
        // 复制回收事实。
        self.forced_reap
    }

    // 返回当前会话是否已经不再可信。
    pub(crate) const fn session_invalidated(&self) -> bool {
        // 复制失效事实。
        self.session_invalidated
    }
}

// 把协议 final 复制为独立聚合结果。
fn completed_result(
    // 借用已验证观察。
    observation: &browser_page_protocol::BrowserPageFrameObservation,
) -> AppResult<BrowserPageCommandResult> {
    // final 必须存在。
    let final_observation = observation.final_observation().ok_or_else(|| {
        // 缺失 final 是内部协议漂移。
        page_error(
            // 使用协议失败码。
            "WORKER_PROTOCOL_FAILED",
            // 输出固定诊断。
            "The browser page final observation is missing.",
        )
    })?;
    // 返回独立结果。
    Ok(BrowserPageCommandResult {
        // 保存 outcome。
        outcome: final_observation.outcome(),
        // final 的完成事实由 outcome 决定。
        completed: final_observation.outcome() != BrowserPageOutcome::Unknown,
        // 只有未派发结果可安全重试。
        retry_safe: final_observation.outcome() == BrowserPageOutcome::NotDispatched,
        // 除未派发外都可能已接受。
        accepted_may_have_occurred: final_observation.outcome()
            != BrowserPageOutcome::NotDispatched,
        // 保存已验证代际。
        navigation_generation: final_observation.navigation_generation(),
        // 复制成功数据。
        data: final_observation.data().cloned(),
        // 复制安全错误。
        error: final_observation.error().cloned(),
        // final 本身不要求 parent 强制回收。
        forced_reap: false,
        // unknown final 之后不得复用会话，其余可信 final 保持 live。
        session_invalidated: final_observation.outcome() == BrowserPageOutcome::Unknown,
    })
}

// 聚合 worker 在 final 前失联的保守结果。
fn incomplete_result(
    // 接收是否已经观察 accepted。
    accepted: bool,
    // 接收调用方观察的导航代际。
    navigation_generation: u64,
    // 接收安全错误码。
    code: &'static str,
    // 接收安全诊断。
    message: &'static str,
) -> BrowserPageCommandResult {
    // accepted 后只能报告未知。
    if accepted {
        // 返回保守未知结果。
        return BrowserPageCommandResult {
            // 使用未知 outcome。
            outcome: BrowserPageOutcome::Unknown,
            // 未取得可信完成。
            completed: false,
            // 禁止自动重试。
            retry_safe: false,
            // 明确可能已接受。
            accepted_may_have_occurred: true,
            // 保留调用方已知代际。
            navigation_generation,
            // 未取得成功数据。
            data: None,
            // 使用冻结 unknown 错误。
            error: Some(serde_json::json!({
                // 固定错误码。
                "code": "OUTCOME_UNKNOWN",
                // 固定安全诊断。
                "message": "The browser page command lost its worker after dispatch acceptance.",
            })),
            // 实际强制回收结果由 Module 自有后台任务异步确定。
            forced_reap: false,
            // accepted 后丢失 final 必须立即使 live 会话失效。
            session_invalidated: true,
        };
    }
    // accepted 前失联是确定未派发。
    BrowserPageCommandResult {
        // 使用未派发 outcome。
        outcome: BrowserPageOutcome::NotDispatched,
        // 结果确定完成。
        completed: true,
        // 未派发可安全重试。
        retry_safe: true,
        // 明确没有接受事实。
        accepted_may_have_occurred: false,
        // 保留调用方代际。
        navigation_generation,
        // 未取得成功数据。
        data: None,
        // 保存安全错误。
        error: Some(serde_json::json!({
            // 使用调用方指定错误码。
            "code": code,
            // 使用调用方指定安全诊断。
            "message": message,
        })),
        // 实际强制回收结果由 Module 自有后台任务异步确定。
        forced_reap: false,
        // worker 失联后即使未派发也不得继续复用同一连接。
        session_invalidated: true,
    }
}

// 为 live 会话提供串行单命令页面协议聚合。
impl BrowserSessionProcess {
    // 执行一个强类型页面命令并保持同一总 deadline。
    pub(crate) fn execute_page(
        // 可变借用唯一 live 会话。
        &mut self,
        // 借用可选当前页面引用。
        page_ref: Option<&str>,
        // 接收 Module 观察的导航代际。
        navigation_generation: u64,
        // 取得强类型操作。
        operation: BrowserPageOperation,
        // 接收单命令总 deadline。
        timeout: Duration,
        // 接收取消观察。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<BrowserPageCommandResult> {
        // deadline 必须转换为冻结毫秒范围。
        let timeout_ms = u32::try_from(timeout.as_millis())
            // 拒绝平台转换溢出。
            .ok()
            // 限制协议范围。
            .filter(|value| (1..=30_000).contains(value))
            // 构造参数失败。
            .ok_or_else(|| {
                // 返回稳定参数错误。
                page_error(
                    // 使用参数类别。
                    "INVALID_ARGUMENT",
                    // 输出固定诊断。
                    "Browser page command timeout must be 1..=30000ms.",
                )
            })?;
        // 保存操作种类用于输出关联。
        let operation_kind = operation.kind();
        // 生成每命令随机关联值。
        let request_nonce = random_nonce()?;
        // 构造冻结页面输入。
        let command = BrowserPageWorkerInput::Command {
            // 使用冻结版本。
            contract_version: CONTRACT_VERSION.to_owned(),
            // 绑定当前 opaque 会话。
            session_id: self.session_id.clone(),
            // 绑定随机请求。
            request_nonce: request_nonce.clone(),
            // 使用不重置总期限。
            timeout_ms,
            // 复制可选页面引用。
            page_ref: page_ref.map(str::to_owned),
            // 传入调用方观察代际。
            navigation_generation,
            // 传入强类型操作。
            operation,
        };
        // 在任何 I/O 前复用协议完整验证。
        command.validate().map_err(|failure| {
            // 映射封闭协议错误。
            page_error(
                // 使用协议稳定码。
                failure.code().as_str(),
                // 不回显输入。
                "The browser page command input was invalid.",
            )
        })?;
        // live 会话必须仍持有 stdin。
        let stdin = self.stdin.as_ref().ok_or_else(|| {
            // 返回确定不可用。
            page_error(
                // 使用断开类别。
                "BROWSER_PROTOCOL_DISCONNECTED",
                // 输出安全诊断。
                "The browser session input channel is closed.",
            )
        })?;
        // 写入完整命令。
        write_input(stdin, &command)?;
        // 从写入前建立总期限起点。
        let started = Instant::now();
        // 保存当前命令输出前缀。
        let mut output = String::new();
        // 保存 accepted 观察。
        let mut accepted = false;
        // 保存是否已经发送取消。
        let mut cancel_sent = false;
        // 保存取消宽限起点。
        let mut cancel_started = None;
        // 等待 final 或会话失联。
        loop {
            // 取消或期限只发送一次关联 cancel。
            if !cancel_sent && (cancelled() || started.elapsed() >= timeout) {
                // 构造冻结取消。
                let cancel = BrowserPageWorkerInput::CancelCommand {
                    // 使用冻结版本。
                    contract_version: CONTRACT_VERSION.to_owned(),
                    // 绑定当前会话。
                    session_id: self.session_id.clone(),
                    // 关联当前命令。
                    request_nonce: request_nonce.clone(),
                };
                // 尽力发送协作取消。
                let _ = write_input(stdin, &cancel);
                // 标记已发送。
                cancel_sent = true;
                // 启动固定协作宽限。
                cancel_started = Some(Instant::now());
            }
            // 协作宽限耗尽后回收不可信会话。
            if cancel_started.is_some_and(|instant| instant.elapsed() >= CANCEL_GRACE) {
                // 返回零帧或 accepted-only 聚合，并由 Module 转移资源所有权。
                return Ok(incomplete_result(
                    // 传入接受事实。
                    accepted,
                    // 保留旧代际。
                    navigation_generation,
                    // accepted 前使用未派发错误。
                    "WORKER_EXITED_BEFORE_DISPATCH",
                    // 输出固定诊断。
                    "The browser page command was cancelled before dispatch acceptance.",
                ));
            }
            // 读取 worker 当前退出事实。
            let process_exited = self.process.as_ref().is_none_or(|process| {
                // 执行零等待探测。
                (unsafe { WaitForSingleObject(process.raw(), 0) }) == WAIT_OBJECT_0
            });
            // 取得下一帧事件。
            let event = self
                // 借用持久帧接收端。
                .frames
                // live 会话必须持有接收端。
                .as_ref()
                // 缺失接收端视为断开。
                .ok_or_else(|| {
                    // 返回结构化断开。
                    page_error(
                        // 使用断开类别。
                        "BROWSER_PROTOCOL_DISCONNECTED",
                        // 输出安全诊断。
                        "The browser session output channel is closed.",
                    )
                })?
                // 使用固定短轮询。
                .recv_timeout(WAIT_SLICE);
            // 按 reader 事件推进。
            match event {
                // 追加完整协议行。
                Ok(ReaderEvent::Line(line)) => {
                    // 命令多帧之间插入唯一 LF。
                    if !output.is_empty() {
                        // 保持 JSON Lines 形状。
                        output.push('\n');
                    }
                    // 追加当前帧。
                    output.push_str(&line);
                    // 验证当前全部前缀。
                    let observation = match browser_page_protocol::observe_output(
                        // 借用当前输出。
                        &output,
                        // 传入关联 nonce。
                        &request_nonce,
                        // 传入操作种类。
                        operation_kind,
                    ) {
                        // 保存合法观察。
                        Ok(observation) => observation,
                        // accepted 后协议漂移必须未知。
                        Err(_) if accepted => {
                            // 返回未知结果并要求 Module 失效会话。
                            return Ok(incomplete_result(
                                // 传入接受事实。
                                true,
                                // 保留旧代际。
                                navigation_generation,
                                // 此码不会进入 accepted 分支结果。
                                "WORKER_PROTOCOL_FAILED",
                                // 输出固定诊断。
                                "The browser page output violated protocol after dispatch.",
                            ));
                        }
                        // accepted 前协议漂移直接失败。
                        Err(_) => {
                            // 返回结构化协议失败，由 Module 转移资源所有权。
                            return Err(page_error(
                                // 使用协议类别。
                                "WORKER_PROTOCOL_FAILED",
                                // 输出固定诊断。
                                "The browser page output violated protocol before dispatch.",
                            ));
                        }
                    };
                    // 保存 accepted 事实。
                    accepted = observation.accepted();
                    // final 到达时完成聚合。
                    if observation.final_observation().is_some() {
                        // 构造独立结果。
                        let result = completed_result(&observation)?;
                        // 返回可信终态。
                        return Ok(result);
                    }
                }
                // EOF、reader 失败或通道断开都按部分事实聚合。
                Ok(ReaderEvent::Eof | ReaderEvent::Failed)
                | Err(RecvTimeoutError::Disconnected) => {
                    // 返回保守聚合并由 Module 转移完整 Job 所有权。
                    return Ok(incomplete_result(
                        // 传入接受事实。
                        accepted,
                        // 保留旧代际。
                        navigation_generation,
                        // 使用稳定退出码。
                        "WORKER_EXITED_BEFORE_DISPATCH",
                        // 输出固定诊断。
                        "The browser page worker exited before a trustworthy final outcome.",
                    ));
                }
                // 已退出进程的周期超时同样按部分事实聚合。
                Err(RecvTimeoutError::Timeout) if process_exited => {
                    // 返回保守聚合并由 Module 转移完整 Job 所有权。
                    return Ok(incomplete_result(
                        // 传入接受事实。
                        accepted,
                        // 保留旧代际。
                        navigation_generation,
                        // 使用稳定退出码。
                        "WORKER_EXITED_BEFORE_DISPATCH",
                        // 输出固定诊断。
                        "The browser page worker exited before a trustworthy final outcome.",
                    ));
                }
                // 周期超时继续检查取消、期限和进程。
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

// 构造统一页面进程错误。
fn page_error(
    // 接收稳定错误码。
    code: &'static str,
    // 接收安全诊断。
    message: &'static str,
) -> AppControlError {
    // 使用产品级错误 envelope。
    AppControlError::new(code, message)
}
