//! 保存 browser-session Job 客户端的 live 所有权与封闭结果。

// 导入 reader 线程句柄。
use std::thread::JoinHandle;
// 导入 reader 事件接收端。
use std::sync::mpsc::Receiver;

// 导入 JSON 错误值。
use serde_json::Value;

// 导入统一结果和协议 outcome。
use super::{
    // 导入统一结果。
    AppResult,
    // 导入协议输入和 outcome。
    BrowserSessionOutcome,
    BrowserSessionWorkerInput,
    // 导入固定协议版本。
    CONTRACT_VERSION,
    // 导入共用句柄与 reader join。
    OwnedHandle,
    // 导入 reader join、强制回收与 stdin 写入。
    join_reader,
    terminate_and_reap,
    write_input,
};

// 导入固定传统 close 等待和 Windows wait。
use super::{CLOSE_TIMEOUT_MS, WAIT_OBJECT_0, WaitForSingleObject};

// 为后台关闭覆盖浏览器回收与 profile 清理两个五秒阶段并保留一秒余量。
const BACKGROUND_CLOSE_TIMEOUT_MS: u32 = 11_000;

// 保存已经打开且仍由 Job 管理的浏览器会话。
pub(crate) struct BrowserSessionProcess {
    // 保存不泄漏原生 endpoint 的 opaque 会话 ID。
    pub(super) session_id: String,
    // 保存可发送 cancel 的 parent stdin。
    pub(super) stdin: Option<OwnedHandle>,
    // 保存 worker 进程句柄。
    pub(super) process: Option<OwnedHandle>,
    // 保存 kill-on-close Job。
    pub(super) job: Option<OwnedHandle>,
    // 保存 stdout reader。
    pub(super) stdout_reader: Option<JoinHandle<()>>,
    // 保存 stderr reader。
    pub(super) stderr_reader: Option<JoinHandle<AppResult<Vec<u8>>>>,
    // 保存 ready 后页面命令的帧接收端。
    pub(super) frames: Option<Receiver<super::ReaderEvent>>,
}

// 为 live 会话提供封闭身份与显式关闭。
impl BrowserSessionProcess {
    // 构造不持有原生资源的纯 Module 生命周期测试会话。
    #[cfg(test)]
    pub(crate) fn empty_for_test(session_id: String) -> Self {
        // 建立不会触发任何原生回收调用的空资源集合。
        Self {
            // 保存测试用 opaque session identity。
            session_id,
            // 不持有 stdin。
            stdin: None,
            // 不持有 worker process。
            process: None,
            // 不持有 Job。
            job: None,
            // 不持有 stdout reader。
            stdout_reader: None,
            // 不持有 stderr reader。
            stderr_reader: None,
            // 不持有页面帧接收端。
            frames: None,
        }
    }

    // 返回 opaque 会话 ID。
    pub(crate) fn session_id(&self) -> &str {
        // 借用无原生事实的身份。
        &self.session_id
    }

    // 发送关联取消并有界回收完整 Job。
    pub(crate) fn close(self, request_nonce: &str) -> bool {
        // 保持同步调用方既有五秒协作边界。
        self.close_with_timeout(request_nonce, CLOSE_TIMEOUT_MS)
    }

    // 在 Module 自有后台任务中覆盖 worker 的完整资源收敛预算。
    pub(crate) fn close_in_background(self, request_nonce: &str) -> bool {
        // 使用只适用于后台所有权转移的扩展等待边界。
        self.close_with_timeout(request_nonce, BACKGROUND_CLOSE_TIMEOUT_MS)
    }

    // 发送关联取消并在调用方给定的固定预算内回收完整 Job。
    fn close_with_timeout(mut self, request_nonce: &str, timeout_ms: u32) -> bool {
        // 发送协作取消但不依赖 worker 配合。
        if let Some(stdin) = self.stdin.as_ref() {
            // 构造冻结 cancel。
            let cancel = BrowserSessionWorkerInput::Cancel {
                // 使用固定协议版本。
                contract_version: CONTRACT_VERSION.to_owned(),
                // 关联原始 open。
                request_nonce: request_nonce.to_owned(),
            };
            // 尽力写入取消。
            let _ = write_input(stdin, &cancel);
        }
        // 关闭 stdin 使 worker 观察到 parent 生命周期结束。
        self.stdin.take();
        // 在当前所有权路径的固定预算内等待 worker 完整析构。
        let graceful = self.process.as_ref().is_some_and(|process| {
            // 执行不会阻塞 dispatcher 的有界等待。
            (unsafe { WaitForSingleObject(process.raw(), timeout_ms) }) == WAIT_OBJECT_0
        });
        // 无论 worker 是否优雅退出都终止残余 Job 成员。
        self.force_reap();
        // 返回是否观察到优雅退出。
        graceful
    }

    // 强制终止 Job 并回收 reader。
    pub(super) fn force_reap(&mut self) {
        // Job 与 process 同时存在时回收完整树。
        if let (Some(job), Some(process)) = (self.job.as_ref(), self.process.as_ref()) {
            // 终止并等待 worker。
            terminate_and_reap(job, process);
        }
        // 关闭 stdin，解除 worker reader。
        self.stdin.take();
        // 关闭 Job 和 process 句柄。
        self.job.take();
        // 关闭进程句柄。
        self.process.take();
        // 丢弃帧接收端使 reader 不再阻塞发送。
        self.frames.take();
        // 等待 stdout reader 退出。
        if let Some(reader) = self.stdout_reader.take() {
            // 忽略 reader panic，生命周期仍已回收。
            let _ = reader.join();
        }
        // 等待 stderr reader 退出。
        if let Some(reader) = self.stderr_reader.take() {
            // 丢弃私有诊断内容。
            let _ = join_reader(reader);
        }
    }
}

// Drop 必须对异常返回同样执行完整 Job 回收。
impl Drop for BrowserSessionProcess {
    // 回收当前会话全部自有资源。
    fn drop(&mut self) {
        // 使用与显式关闭相同的强制兜底。
        self.force_reap();
    }
}

// 保存一次打开握手的封闭结果。
pub(crate) struct BrowserSessionOpenResult {
    // 保存 outcome。
    pub(super) outcome: BrowserSessionOutcome,
    // 保存完成事实。
    pub(super) completed: bool,
    // 保存重试安全事实。
    pub(super) retry_safe: bool,
    // 保存可能接受事实。
    pub(super) accepted_may_have_occurred: bool,
    // 保存可选安全错误。
    pub(super) error: Option<Value>,
    // 保存 ready 时唯一 live 会话。
    pub(super) session: Option<BrowserSessionProcess>,
    // 保存 parent 是否强制回收 Job。
    pub(super) forced_reap: bool,
    // 保存原始请求 nonce，仅供后续 close 关联。
    pub(super) request_nonce: String,
}

// 为打开结果提供只读投影和会话所有权转移。
impl BrowserSessionOpenResult {
    // 返回 outcome。
    pub(crate) const fn outcome(&self) -> BrowserSessionOutcome {
        // 复制封闭枚举。
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

    // 返回可选安全错误。
    pub(crate) const fn error(&self) -> Option<&Value> {
        // 借用错误对象。
        self.error.as_ref()
    }

    // 返回 parent 是否执行强制回收。
    pub(crate) const fn forced_reap(&self) -> bool {
        // 复制回收事实。
        self.forced_reap
    }

    // 返回可选 opaque 会话 ID。
    pub(crate) fn session_id(&self) -> Option<&str> {
        // 从 live 会话借用身份。
        self.session.as_ref().map(BrowserSessionProcess::session_id)
    }

    // 取得 live 会话与原始关联 nonce。
    pub(crate) fn into_session(self) -> Option<(BrowserSessionProcess, String)> {
        // 同时取得会话和 nonce。
        self.session.map(|session| (session, self.request_nonce))
    }
}

// 把不完整输出聚合为零帧未派发或 accepted-only 未知。
pub(super) fn incomplete_result(
    // 接收 accepted 事实。
    accepted: bool,
    // 接收是否强制回收。
    forced_reap: bool,
    // 保存请求 nonce。
    request_nonce: String,
) -> BrowserSessionOpenResult {
    // accepted 后缺失 final 必须保守 unknown。
    if accepted {
        // 返回未知结果。
        return BrowserSessionOpenResult {
            // 使用冻结未知类别。
            outcome: BrowserSessionOutcome::Unknown,
            // 未取得可信完成。
            completed: false,
            // 禁止自动重试。
            retry_safe: false,
            // 明确资源可能已接受。
            accepted_may_have_occurred: true,
            // 使用固定安全错误。
            error: Some(serde_json::json!({
                // 使用唯一未知错误码。
                "code": "OUTCOME_UNKNOWN",
                // 不泄漏 worker 输出。
                "message": "The browser session worker was accepted but no trustworthy final was observed.",
            })),
            // 未建立 live 会话。
            session: None,
            // 保存回收事实。
            forced_reap,
            // 保存关联 nonce。
            request_nonce,
        };
    }
    // 零帧缺失 final 确定尚未 dispatch。
    BrowserSessionOpenResult {
        // 使用未派发类别。
        outcome: BrowserSessionOutcome::NotDispatched,
        // 结果完整。
        completed: true,
        // 可安全重试。
        retry_safe: true,
        // 没有资源接受事实。
        accepted_may_have_occurred: false,
        // 使用固定安全错误。
        error: Some(serde_json::json!({
            // 使用稳定零帧错误码。
            "code": "WORKER_EXITED_BEFORE_DISPATCH",
            // 不泄漏进程细节。
            "message": "The browser session worker exited before dispatch acceptance.",
        })),
        // 没有 live 会话。
        session: None,
        // 保存回收事实。
        forced_reap,
        // 保存关联 nonce。
        request_nonce,
    }
}
