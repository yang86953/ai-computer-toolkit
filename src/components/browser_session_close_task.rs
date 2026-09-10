//! 在 Module 所有权内异步回收已从 live registry 移除的浏览器会话。

// 导入有界完成通道。
use std::sync::mpsc::{self, Receiver, TryRecvError};
// 导入唯一后台线程句柄。
use std::thread::{self, JoinHandle};
// 导入总预算时钟。
use std::time::{Duration, Instant};

// 导入 live 进程所有权。
use super::browser_session_process::BrowserSessionProcess;

// 固定 wait 对取消观察的最大轮询粒度。
const CLOSE_TASK_WAIT_SLICE: Duration = Duration::from_millis(10);

// 保存 Module 对后台 close 回收的唯一所有权。
pub(crate) struct BrowserSessionCloseTask {
    // 接收只在完整回收结束后发送的完成事实。
    completed: Receiver<()>,
    // 保存唯一后台 worker 的 join 所有权。
    worker: Option<JoinHandle<()>>,
}

// 为 Module 提供启动、短等待和最终回收。
impl BrowserSessionCloseTask {
    // 转移 live 进程与原始 open nonce 到 Module 自有后台任务。
    pub(crate) fn start(
        // 接收唯一 live 进程所有权。
        process: BrowserSessionProcess,
        // 接收关联协作 cancel 的原始 open nonce。
        request_nonce: String,
    ) -> Result<Self, (BrowserSessionProcess, String)> {
        // 建立只需要一次完成事实的有界通道。
        let (sender, completed) = mpsc::sync_channel(1);
        // 建立在线程成功后才移交进程所有权的有界通道。
        let (process_sender, process_receiver) =
            // 同时转移进程与关联 nonce，避免后台任务猜测身份。
            mpsc::sync_channel::<(BrowserSessionProcess, String)>(1);
        // 在独立线程中持有进程、Job 和 stdio，禁止 dispatcher 阻塞在其回收上。
        let worker = thread::Builder::new()
            // 使用固定且不含调用方事实的线程名。
            .name("browser-session-close-reaper".to_owned())
            // 让后台任务独占所有底层资源。
            .spawn(move || {
                // 只在后台线程已经存在后接收唯一进程所有权。
                let Ok((process, request_nonce)) = process_receiver.recv() else {
                    // 未移交所有权时线程无需执行任何回收。
                    return;
                };
                // 在后台线程发送关联 cancel 并覆盖完整资源收敛预算。
                let graceful = process.close_in_background(&request_nonce);
                // 只有 worker 自行完成析构时才发布可信完成事实。
                if graceful {
                    // 向等待方发布完整回收事实。
                    let _ = sender.send(());
                }
            });
        // 线程创建失败时调用方仍保有进程所有权。
        let worker = match worker {
            // 保存已启动的后台线程。
            Ok(worker) => worker,
            // 归还尚未移交的唯一进程 owner。
            Err(_) => return Err((process, request_nonce)),
        };
        // 在线程已经建立后移交唯一进程所有权。
        if let Err(error) = process_sender.send((process, request_nonce)) {
            // receiver 异常断开时把进程与 nonce 还给调用方。
            return Err(error.0);
        }
        // 返回仍由 Module 持有的任务所有权。
        Ok(Self {
            // 保存完成接收端。
            completed,
            // 保存唯一 join 所有权。
            worker: Some(worker),
        })
    }

    // 在本次 broker 的剩余总预算内等待可信回收完成。
    pub(crate) fn wait_until(
        // 可变借用任务，以便成功时立即 join。
        &mut self,
        // 接收不可延长的剩余预算。
        timeout: Duration,
        // 接收每轮都重新求值的取消观察。
        cancelled: impl Fn() -> bool,
    ) -> bool {
        // 记录本次等待起点，不把等待时间带入后续调用。
        let started = Instant::now();
        // 在完成、取消或总预算耗尽前短周期等待。
        loop {
            // 先接受已经完成的回收，避免零预算掩盖可信完成事实。
            if self.take_completed_if_ready() {
                // 只有已完成并 join 才允许向上投影 Completed。
                return true;
            }
            // 协作取消只能停止本次等待，后台任务继续由 Module 持有。
            if cancelled() {
                // 取消时不能证明完整回收已在此调用前完成。
                return false;
            }
            // 计算本次总预算尚余的时长。
            let remaining = timeout.saturating_sub(started.elapsed());
            // 预算耗尽时不得阻塞 dispatcher。
            if remaining.is_zero() {
                // 回收任务继续运行，调用方必须投影 Unknown。
                return false;
            }
            // 只等待到取消轮询或总预算中更早的边界。
            let wait = remaining.min(CLOSE_TASK_WAIT_SLICE);
            // 在有界片段内接收完成事实。
            match self.completed.recv_timeout(wait) {
                // 唯一完成事实到达后 join 已结束的线程。
                Ok(()) => {
                    // 线程已发送完成，join 不再等待底层回收。
                    self.join_finished_worker();
                    // 此次调用可证明完整回收。
                    return true;
                }
                // 片段超时后重新检查取消和总 deadline。
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                // worker 未发布完成事实即消失，不能伪造成功。
                Err(mpsc::RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    // 非阻塞检查后台回收是否已经完成，以便 Module 释放陈旧任务。
    pub(crate) fn take_completed_if_ready(&mut self) -> bool {
        // 不得为了清理旧任务阻塞新的 broker command。
        match self.completed.try_recv() {
            // 完成事实到达后 join 已退出的后台线程。
            Ok(()) => {
                // 释放唯一线程所有权。
                self.join_finished_worker();
                // 允许 Module 删除此任务记录。
                true
            }
            // 后台任务仍在回收，必须继续由 Module 持有。
            Err(TryRecvError::Empty) => false,
            // 未见完成事实的断开一律不能报告可信完成。
            Err(TryRecvError::Disconnected) => false,
        }
    }

    // 回收已经确认结束的 worker 线程句柄。
    fn join_finished_worker(&mut self) {
        // 只允许第一个完成观察者取得 join 所有权。
        if let Some(worker) = self.worker.take() {
            // 忽略 panic 载荷；未发送完成的 panic 不会走到此分支。
            let _ = worker.join();
        }
    }

    // 构造不接触原生 worker 的纯有界等待测试任务。
    #[cfg(test)]
    pub(crate) fn start_for_test(completion_delay: Duration) -> Self {
        // 建立只需一次完成事实的测试通道。
        let (sender, completed) = mpsc::sync_channel(1);
        // 启动只等待固定时长的纯测试线程。
        let worker = thread::spawn(move || {
            // 模拟尚未完成的后台回收。
            thread::sleep(completion_delay);
            // 发布已完成事实。
            let _ = sender.send(());
        });
        // 返回仍由测试持有的任务。
        Self {
            // 保存完成接收端。
            completed,
            // 保存唯一 join 所有权。
            worker: Some(worker),
        }
    }

    // 构造已经发布完成事实且不持有后台线程的纯容量测试任务。
    #[cfg(test)]
    pub(crate) fn completed_for_test() -> Self {
        // 建立只需一次完成事实的测试通道。
        let (sender, completed) = mpsc::sync_channel(1);
        // 在构造期发布可信完成事实，不创建任何线程或浏览器资源。
        let _ = sender.send(());
        // 返回可被非阻塞清理路径立即释放的任务。
        Self {
            // 保存已完成接收端。
            completed,
            // 不保留任何待 join 的线程。
            worker: None,
        }
    }
}

// Module 析构时必须等待全部仍挂起的 close task 结束。
impl Drop for BrowserSessionCloseTask {
    // 完成后台资源的最终回收。
    fn drop(&mut self) {
        // 取出仍未 join 的唯一 worker。
        if let Some(worker) = self.worker.take() {
            // 等待后台任务先完成 Job 和 stdio 回收。
            let _ = worker.join();
        }
    }
}

// 覆盖不启动浏览器的后台 close 任务等待与回收边界。
#[cfg(test)]
mod tests {
    // 导入测试计时工具。
    use std::time::{Duration, Instant};
    // 导入纯测试等待线程工具。
    use std::thread;

    // 导入被测任务。
    use super::BrowserSessionCloseTask;

    // 验证短 deadline 不等待后台回收完成。
    #[test]
    fn short_deadline_does_not_block_close_caller() {
        // 构造尚需较长时间才完成的纯任务。
        let mut task = BrowserSessionCloseTask::start_for_test(Duration::from_millis(40));
        // 记录调用计时起点。
        let started = Instant::now();
        // 只给 broker 一毫秒剩余预算。
        let completed = task.wait_until(Duration::from_millis(1), || false);
        // 短预算内不得错误报告完成。
        assert!(!completed);
        // 调用必须远早于模拟回收完成返回。
        assert!(started.elapsed() < Duration::from_millis(30));
        // 等待测试后台任务实际发布完成。
        thread::sleep(Duration::from_millis(50));
        // 非阻塞清理必须释放已经完成的任务。
        assert!(task.take_completed_if_ready());
    }
}
