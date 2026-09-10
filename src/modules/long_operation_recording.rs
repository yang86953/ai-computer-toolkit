//! 拥有 broker 代际内异步窗口录制任务、取消令牌与线程回收。

// 导入任务索引、panic 边界、共享状态与线程句柄。
use std::{
    // 保存 broker 代际内的任务运行时。
    collections::HashMap,
    // 阻止 worker panic 穿透 broker 生命周期。
    panic::{AssertUnwindSafe, catch_unwind},
    // 提供逐任务取消与 registry 共享所有权。
    sync::{
        // 保存幂等取消信号。
        Arc,
        // 串行化唯一 registry 状态。
        Mutex,
        // 提供无锁取消与 fatal 标记。
        atomic::{AtomicBool, Ordering},
    },
    // 创建并回收命名 worker 线程。
    thread::{self, JoinHandle},
    // 生成持久状态迁移时刻。
    time::{SystemTime, UNIX_EPOCH},
};

// 导入 provider-neutral JSON 输入与结果。
use serde_json::Value;

// 导入进程取消、统一结果、状态机与录制领域 Module。
use crate::{
    // 观察 broker 进程级取消。
    components::cancellation,
    // 传播受控领域失败。
    domain::{AppControlError, AppResult},
    // 复用同一 System 内的领域行为。
    modules::{
        // 读取取消幂等分类。
        long_operation::CancelRequestEffect,
        // 持久化任务事实。
        long_operation_registry::{
            LongOperationFailure, LongOperationRecord, LongOperationRegistry,
            LongOperationRegistryError,
        },
        // 执行 Rust 窗口录制 worker 生命周期。
        window_record,
    },
};

// 固定首个可异步执行的 capability。
const WINDOW_RECORD_CAPABILITY: &str = "window.record@1";
// 固定 worker panic 或线程创建失败的稳定错误码。
const WORKER_UNAVAILABLE_CODE: &str = "ISOLATED_WORKER_UNAVAILABLE";
// 固定不泄漏路径或平台事实的 worker 失败消息。
const WORKER_UNAVAILABLE_MESSAGE: &str = "The long operation worker became unavailable.";
// 固定普通领域失败的安全消息。
const OPERATION_FAILED_MESSAGE: &str = "The long operation failed.";

// 保存单个 broker-owned 任务的运行时所有权。
struct RecordingTaskRuntime {
    // 保存逐任务幂等取消令牌。
    cancelled: Arc<AtomicBool>,
    // 保存必须被 broker 回收的线程句柄。
    worker: JoinHandle<()>,
}

// 表示 worker 是否已经越过持久 dispatch 事实点。
enum DispatchEffect {
    // 已持久进入 running，可以执行领域行为。
    Dispatched,
    // dispatch 前取消已持久收敛为 failed。
    CancelledBeforeDispatch,
}

// 拥有同一 broker 代际内的 registry 与异步录制任务。
pub(crate) struct LongOperationRecordingTasks {
    // 共享唯一持久 registry，串行化全部状态迁移。
    registry: Arc<Mutex<LongOperationRegistry>>,
    // 保存尚未回收的任务运行时。
    tasks: HashMap<String, RecordingTaskRuntime>,
    // 标记任一持久迁移已无法可靠完成。
    fatal: Arc<AtomicBool>,
}

// 为 broker 提供接受、查询、取消与关闭入口。
impl LongOperationRecordingTasks {
    // 接管已完成启动恢复的唯一 registry。
    pub(crate) fn new(registry: LongOperationRegistry) -> Self {
        // 建立空任务代际。
        Self {
            // 只允许本 Module 内部共享 registry。
            registry: Arc::new(Mutex::new(registry)),
            // 启动时不存在本代际 worker。
            tasks: HashMap::new(),
            // 启动恢复成功后状态健康。
            fatal: Arc::new(AtomicBool::new(false)),
        }
    }

    // 返回持久状态是否仍可可靠服务。
    pub(crate) fn healthy(&self) -> bool {
        // 使用 acquire 观察 worker 发布的 fatal 事实。
        !self.fatal.load(Ordering::Acquire)
    }

    // 仅供同 crate 回归夹具构造精确持久状态。
    #[cfg(test)]
    pub(crate) fn registry_for_test(
        // 借用任务 Module。
        &self,
    ) -> std::sync::MutexGuard<'_, LongOperationRegistry> {
        // 测试若造成 poison 应立即保留完整诊断。
        self.registry
            // 取得夹具独占锁。
            .lock()
            // 测试失败不转换为生产错误。
            .unwrap_or_else(|error| panic!("long operation test registry poisoned: {error}"))
    }

    // 接受并启动一个经过 System 预检的窗口录制任务。
    pub(crate) fn submit(
        // 可变借用以登记线程所有权。
        &mut self,
        // 接收 broker 生成的 canonical operation handle。
        operation_id: &str,
        // 接收 canonical opaque 窗口目标。
        session_id: String,
        // 接收已经纯验证的领域输入。
        input: Value,
        // 接收业务接受时刻。
        now_ms: u64,
    ) -> Result<LongOperationRecord, LongOperationRegistryError> {
        // 生产入口固定调用 Rust 录制领域行为。
        self.submit_with(operation_id, now_ms, move |cancelled| {
            // 将逐任务和进程取消合并为 worker 观察函数。
            window_record::record_with_cancellation(&session_id, true, &input, || {
                // 任一取消来源均停止当前任务。
                cancelled.load(Ordering::Acquire) || cancellation::is_cancelled()
            })
        })
    }

    // 查询当前持久 operation 事实。
    pub(crate) fn status(
        // 可变借用以回收到期记录。
        &mut self,
        // 接收 handle-only Query。
        operation_id: &str,
        // 接收查询时刻。
        now_ms: u64,
    ) -> Result<LongOperationRecord, LongOperationRegistryError> {
        // 先回收已经结束的本代际线程。
        self.reap_finished();
        // 独占一次 registry Query。
        self.registry
            // 取得无 panic 锁边界。
            .lock()
            // poison 表示内部事实不再可靠。
            .map_err(|_| LongOperationRegistryError::InvalidRecord)?
            // 委托 registry 拥有到期与查询语义。
            .status(operation_id, now_ms)
    }

    // 幂等持久取消并通知仍存活的任务。
    pub(crate) fn cancel(
        // 可变借用以访问任务索引。
        &mut self,
        // 接收 handle-only Command。
        operation_id: &str,
        // 接收取消接受时刻。
        now_ms: u64,
    ) -> Result<(CancelRequestEffect, LongOperationRecord), LongOperationRegistryError> {
        // 先回收已完成线程，终态记录仍由 registry 保留。
        self.reap_finished();
        // 先原子持久化取消事实。
        let outcome = self
            // 借用唯一 registry。
            .registry
            // 取得无 panic 锁边界。
            .lock()
            // poison 时失败闭合。
            .map_err(|_| LongOperationRegistryError::InvalidRecord)?
            // 由 registry 分类首次、重复与终态取消。
            .request_cancel(operation_id, now_ms)?;
        // 只在取消仍需传播时设置任务令牌。
        if matches!(
            // 读取幂等效果。
            outcome.0,
            // 首次或重复请求都确保令牌为 true。
            CancelRequestEffect::Requested | CancelRequestEffect::AlreadyRequested
        ) {
            // 任务可能已经刚刚结束并等待回收。
            if let Some(runtime) = self.tasks.get(operation_id) {
                // release 发布取消事实给 worker。
                runtime.cancelled.store(true, Ordering::Release);
            }
        }
        // 返回持久快照而不等待 worker 终态。
        Ok(outcome)
    }

    // 停止接受后取消并回收全部本代际 worker。
    pub(crate) fn shutdown(&mut self) -> Result<(), LongOperationRegistryError> {
        // 先尝试持久化全部活动任务的取消意图。
        let persistence = (|| {
            // 取得同一关闭迁移时刻。
            let now_ms = current_unix_milliseconds()?;
            // 独占 registry 以串行记录全部取消。
            let mut registry = self
                // 借用共享 registry。
                .registry
                // 取得无 panic 锁边界。
                .lock()
                // poison 时保留失败闭合分类。
                .map_err(|_| LongOperationRegistryError::InvalidRecord)?;
            // 为仍由本代际拥有的每个任务记录取消。
            for operation_id in self.tasks.keys() {
                // 首次、重复和终态效果均无需覆盖终态。
                registry.request_cancel(operation_id, now_ms)?;
            }
            // 全部取消意图已经持久化。
            Ok(())
        })();
        // 先向全部任务发布取消。
        for runtime in self.tasks.values() {
            // release 保证 worker 后续观察到取消。
            runtime.cancelled.store(true, Ordering::Release);
        }
        // 转移全部句柄，防止 join 时借用索引。
        let tasks = std::mem::take(&mut self.tasks);
        // 逐个等待 Rust worker 结束并释放其 Job 生命周期。
        for (_, runtime) in tasks {
            // worker 内部已经捕获 panic，join 仅负责回收句柄。
            let _ = runtime.worker.join();
        }
        // 只在全部 worker 已回收后返回持久化结果。
        persistence
    }

    // 使用受控执行函数接受并启动任务，供生产与 Module 回归复用。
    fn submit_with<Run>(
        // 可变借用以登记运行时。
        &mut self,
        // 接收 canonical operation handle。
        operation_id: &str,
        // 接收业务接受时刻。
        now_ms: u64,
        // 接收一次性领域执行函数。
        run: Run,
    ) -> Result<LongOperationRecord, LongOperationRegistryError>
    where
        // 执行函数必须可转移进 broker-owned 线程。
        Run: FnOnce(Arc<AtomicBool>) -> AppResult<Value> + Send + 'static,
    {
        // 提交新任务前回收已经结束的线程。
        self.reap_finished();
        // 先持久接受，成功后才允许公开 handle 或启动 worker。
        let accepted = self
            // 借用唯一 registry。
            .registry
            // 取得无 panic 锁边界。
            .lock()
            // poison 时失败闭合。
            .map_err(|_| LongOperationRegistryError::InvalidRecord)?
            // 原子建立 accepted 记录。
            .accept(operation_id, WINDOW_RECORD_CAPABILITY, now_ms)?;
        // 为当前任务创建独立取消令牌。
        let cancelled = Arc::new(AtomicBool::new(false));
        // 克隆线程拥有的取消令牌。
        let worker_cancelled = Arc::clone(&cancelled);
        // 克隆线程拥有的 registry 端口。
        let registry = Arc::clone(&self.registry);
        // 克隆线程发布 fatal 事实的端口。
        let fatal = Arc::clone(&self.fatal);
        // 固定线程内拥有 operation handle。
        let worker_operation_id = operation_id.to_owned();
        // 创建带安全名称的 broker-owned 线程。
        let worker = thread::Builder::new()
            // 名称不包含调用方输入或路径。
            .name("act-long-operation-window-record".to_owned())
            // 在线程内执行全部 dispatch 与终态迁移。
            .spawn(move || {
                // 捕获领域 worker panic 并收敛为稳定失败。
                let execution = catch_unwind(AssertUnwindSafe(|| {
                    // 在同一 registry 锁内解析取消竞争并持久化 dispatch 事实。
                    match transition_dispatch(
                        // 借用共享 registry。
                        &registry,
                        // 绑定当前 operation。
                        &worker_operation_id,
                        // 传入线程取得调度时的取消事实。
                        worker_cancelled.load(Ordering::Acquire),
                    ) {
                        // 只有可靠 dispatch 后才执行领域行为。
                        Ok(DispatchEffect::Dispatched) => {}
                        // dispatch 前取消已经可靠终结。
                        Ok(DispatchEffect::CancelledBeforeDispatch) => return,
                        // 无可靠 dispatch 事实时禁止执行领域行为。
                        Err(_) => {
                            // 要求 broker 失败闭合。
                            fatal.store(true, Ordering::Release);
                            // 直接结束当前线程。
                            return;
                        }
                    }
                    // 执行唯一 Rust 窗口录制领域行为。
                    let result = run(Arc::clone(&worker_cancelled));
                    // 以 worker 证据终结持久记录。
                    if transition_terminal(&registry, &worker_operation_id, result).is_err() {
                        // 终态无法持久化时要求 broker 失败闭合并由下代恢复。
                        fatal.store(true, Ordering::Release);
                    }
                }));
                // panic 必须在当前代际收敛为稳定失败。
                if execution.is_err()
                    // 只有尚可写入终态时才继续。
                    && transition_terminal(
                        // 借用共享 registry。
                        &registry,
                        // 绑定当前 operation。
                        &worker_operation_id,
                        // 使用固定 worker 不可用失败。
                        Err(AppControlError::new(
                            // 使用稳定错误码。
                            WORKER_UNAVAILABLE_CODE,
                            // 使用不泄漏上下文的安全消息。
                            WORKER_UNAVAILABLE_MESSAGE,
                        )),
                    )
                    // 记录无法终结。
                    .is_err()
                {
                    // 要求 broker 失败闭合。
                    fatal.store(true, Ordering::Release);
                }
            });
        // 线程创建失败发生在 accepted 后、dispatch 前。
        let worker = match worker {
            // 保存已启动句柄。
            Ok(worker) => worker,
            // 收敛为可安全重提的 failed 记录。
            Err(_) => {
                // 构造固定失败对象。
                let failure = fixed_failure(WORKER_UNAVAILABLE_CODE, WORKER_UNAVAILABLE_MESSAGE);
                // 持久化并返回已经建立 handle 的终态。
                return self
                    // 借用唯一 registry。
                    .registry
                    // 取得无 panic锁边界。
                    .lock()
                    // poison 时失败闭合。
                    .map_err(|_| LongOperationRegistryError::InvalidRecord)?
                    // dispatch 前失败保持 retry-safe。
                    .fail(operation_id, failure, now_ms);
            }
        };
        // 登记 broker 必须回收的运行时。
        self.tasks.insert(
            // 使用 canonical operation handle 作为索引。
            operation_id.to_owned(),
            // 保存令牌与线程句柄。
            RecordingTaskRuntime { cancelled, worker },
        );
        // 立即返回持久 accepted 快照，不等待 dispatch 或完成。
        Ok(accepted)
    }

    // 回收已经退出的线程而不阻塞 broker 连接循环。
    fn reap_finished(&mut self) {
        // 收集已完成任务的 canonical handle。
        let finished = self
            // 遍历运行时索引。
            .tasks
            // 借用键和值。
            .iter()
            // 只保留已经退出的线程。
            .filter(|(_, runtime)| runtime.worker.is_finished())
            // 克隆待回收的 canonical handle。
            .map(|(operation_id, _)| operation_id.clone())
            // 形成独立列表以解除索引借用。
            .collect::<Vec<_>>();
        // 逐个移除并回收完成线程。
        for operation_id in finished {
            // 从运行时索引转移所有权。
            if let Some(runtime) = self.tasks.remove(&operation_id) {
                // 已完成 join 不会阻塞正常路径。
                let _ = runtime.worker.join();
            }
        }
    }
}

// 在执行领域行为前持久化不可逆 dispatch 事实。
fn transition_dispatch(
    // 借用共享 registry。
    registry: &Arc<Mutex<LongOperationRegistry>>,
    // 接收 canonical operation handle。
    operation_id: &str,
    // 接收 worker 调度前观察到的取消事实。
    cancelled: bool,
) -> Result<DispatchEffect, LongOperationRegistryError> {
    // 取得状态迁移时刻。
    let now_ms = current_unix_milliseconds()?;
    // 独占一次取消竞争与 dispatch 迁移。
    let mut registry = registry
        // 取得无 panic 锁边界。
        .lock()
        // poison 时失败闭合。
        .map_err(|_| LongOperationRegistryError::InvalidRecord)?;
    // 查询持久取消事实并保持同一锁所有权。
    let record = registry.status(operation_id, now_ms)?;
    // 逐任务或持久取消都禁止越过 dispatch 点。
    if cancelled || record.cancel_requested() {
        // 使用固定 dispatch 前取消失败。
        let failure = fixed_failure(
            "CANCELLED",
            "The long operation was cancelled before dispatch.",
        );
        // 持久终结为 retry-safe failed。
        registry.fail(operation_id, failure, now_ms)?;
        // 告知 worker 不得执行领域行为。
        return Ok(DispatchEffect::CancelledBeforeDispatch);
    }
    // 持久化不可逆 dispatch 事实。
    registry.start_dispatch(operation_id, now_ms)?;
    // 允许 worker 执行领域行为。
    Ok(DispatchEffect::Dispatched)
}

// 按领域执行结果持久化唯一终态。
fn transition_terminal(
    // 借用共享 registry。
    registry: &Arc<Mutex<LongOperationRegistry>>,
    // 接收 canonical operation handle。
    operation_id: &str,
    // 接收完整成功结果或统一失败。
    result: AppResult<Value>,
) -> Result<(), LongOperationRegistryError> {
    // 取得终态迁移时刻。
    let now_ms = current_unix_milliseconds()?;
    // 独占一次终态迁移。
    let mut registry = registry
        // 取得无 panic 锁边界。
        .lock()
        // poison 时失败闭合。
        .map_err(|_| LongOperationRegistryError::InvalidRecord)?;
    // 按领域证据选择 completed 或 failed。
    match result {
        // 完整结果由 registry 执行预算门禁。
        Ok(result) => registry.complete(operation_id, result, now_ms).map(|_| ()),
        // 稳定失败不得保存路径、payload 或平台事实。
        Err(error) => registry
            // 构造受预算约束的失败记录。
            .fail(operation_id, operation_failure(&error), now_ms)
            // 只向调用方返回成功分类。
            .map(|_| ()),
    }
}

// 把领域错误收敛为不泄漏上下文的稳定失败。
fn operation_failure(error: &AppControlError) -> LongOperationFailure {
    // 优先保留符合契约的领域错误码。
    LongOperationFailure::new(error.code, OPERATION_FAILED_MESSAGE)
        // 理论上的非法码使用固定保守错误。
        .unwrap_or_else(|| fixed_failure("OPERATION_FAILED", OPERATION_FAILED_MESSAGE))
}

// 构造静态且已经满足 registry 门禁的失败对象。
fn fixed_failure(code: &str, message: &str) -> LongOperationFailure {
    // 静态常量若漂移应在回归测试中立即暴露。
    LongOperationFailure::new(code, message)
        // 固定值理论上始终有效。
        .unwrap_or_else(|| unreachable!("fixed long operation failure must remain valid"))
}

// 取得当前 Unix 毫秒供异步状态迁移使用。
fn current_unix_milliseconds() -> Result<u64, LongOperationRegistryError> {
    // 读取系统时钟并拒绝理论异常。
    SystemTime::now()
        // 计算 Unix epoch 后时长。
        .duration_since(UNIX_EPOCH)
        // epoch 前时钟不能覆盖持久事实。
        .map_err(|_| LongOperationRegistryError::ClockRegression)
        // 继续投影完整毫秒。
        .and_then(|duration| {
            // 限制为持久 schema 使用的 u64。
            u64::try_from(duration.as_millis())
                // 溢出视为时钟无法可靠表达。
                .map_err(|_| LongOperationRegistryError::ClockRegression)
        })
}

// 声明异步任务接受、取消、panic 与终态回归测试。
#[cfg(test)]
// 将测试夹具放入独立文件控制生产 Module 规模。
#[path = "long_operation_recording_tests.rs"]
mod tests;
