//! 保存 Browser Session Module 的 broker close 异步回收路径。

// 导入单命令总预算。
use std::time::Duration;

// 导入 Module 自有后台 close 任务。
use crate::components::{
    // 导入 Module 自有后台 close 任务。
    browser_session_close_task::BrowserSessionCloseTask,
};
// 导入统一结果。
use crate::domain::AppResult;

// 导入同一 Module 的私有状态与封闭结果。
use super::{BrowserSessionModule, stale_session_error};

// 保存 broker close 在本次调用内可证明的封闭结果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionCloseOutcome {
    // 表示后台任务已完成 worker、Job 与 stdio 回收。
    Completed,
    // 表示本次 deadline、取消或任务异常前无法证明回收完成。
    Unknown,
}

// 为 Module 提供 broker 专用的有界 close 执行。
impl BrowserSessionModule {
    // 让不可信 live 会话原子失效并把精确资源转入后台回收。
    pub(super) fn retire_session(&mut self, session_id: &str) {
        // 先从 live registry 移除，禁止任何后续命令复用。
        let Some(entry) = self.sessions.remove(session_id) else {
            // 已经失效的会话无需重复建立回收任务。
            return;
        };
        // 尝试把唯一进程所有权移交给 Module 自有后台任务。
        match BrowserSessionCloseTask::start(entry.process, entry.open_nonce) {
            // 成功时由 Module 持有任务直至资源完整收敛。
            Ok(task) => self.pending_close_tasks.push(task),
            // 线程暂不可用时继续保留精确进程与关联 nonce。
            Err(pending) => self.pending_close_processes.push(pending),
        }
    }

    // 为 broker 关闭先原子失效会话，再由 Module 自有后台任务完成回收。
    pub(crate) fn close_for_broker(
        // 可变借用唯一 Module 状态。
        &mut self,
        // 借用已预检的 opaque session identity。
        session_id: &str,
        // 接收不可延长的剩余总预算。
        timeout: Duration,
        // 接收可在等待期间变化的协作取消观察。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<BrowserSessionCloseOutcome> {
        // 非阻塞释放此前已完成的后台 close 任务。
        self.reap_finished_close_tasks();
        // 复用业务接受前身份门禁，保证 stale 语义一致。
        self.prepare_close(session_id)?;
        // 先从 live registry 原子移除，使全部后续命令立即 stale。
        let entry = self
            // 转移唯一进程所有权给 close task。
            .sessions
            // 目标缺失时保持稳定 stale 语义。
            .remove(session_id)
            // 防御预检和移除间的内部漂移。
            .ok_or_else(stale_session_error)?;
        // 尝试建立只由 Module 持有的后台回收任务。
        let mut task = match BrowserSessionCloseTask::start(entry.process, entry.open_nonce) {
            // 成功时只让本次调用在总预算内等待。
            Ok(task) => task,
            // 线程创建失败时会话仍已 stale，且不能证明回收已在本次调用完成。
            Err((process, request_nonce)) => {
                // 保留进程与关联 nonce，等待下次 Module 调用时重试。
                self.pending_close_processes
                    // 保存完整协作关闭所有权。
                    .push((process, request_nonce));
                // 向 broker 返回保守未知而非伪造失败或完成。
                return Ok(BrowserSessionCloseOutcome::Unknown);
            }
        };
        // 只在剩余总预算和取消边界内等待可信回收。
        if task.wait_until(timeout, cancelled) {
            // 完成事实只会在 worker、Job 与 stdio 回收后发布。
            return Ok(BrowserSessionCloseOutcome::Completed);
        }
        // deadline 或取消前无法证明回收时，Module 继续拥有后台任务。
        self.pending_close_tasks.push(task);
        // 以 Unknown 保持 accepted 后 outcome 的真实性。
        Ok(BrowserSessionCloseOutcome::Unknown)
    }

    // 非阻塞删除已经发布可信完成事实的后台 close task。
    pub(super) fn reap_finished_close_tasks(&mut self) {
        // 取出暂未成功建线程的所有进程 owner。
        let pending_processes = std::mem::take(&mut self.pending_close_processes);
        // 逐个重新尝试建立 Module 自有后台回收任务。
        for (process, request_nonce) in pending_processes {
            // 成功时转入后台任务，失败时继续保留唯一进程 owner。
            match BrowserSessionCloseTask::start(process, request_nonce) {
                // 记录已建立的后台任务。
                Ok(task) => self.pending_close_tasks.push(task),
                // 保留以后再次尝试的完整关闭 owner。
                Err(pending) => self.pending_close_processes.push(pending),
            }
        }
        // 保留尚未完成或无法证明完成的任务所有权。
        self.pending_close_tasks
            // 对每个任务只进行非阻塞完成观察。
            .retain_mut(|task| !task.take_completed_if_ready());
    }
}
