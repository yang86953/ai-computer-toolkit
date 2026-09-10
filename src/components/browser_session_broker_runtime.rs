//! 协调 browser-session broker 当前 epoch 的线性化状态与等待唤醒。

// 导入共享状态、条件变量与单调时钟。
use std::{
    // 使用互斥锁与条件变量隔离短状态推进。
    sync::{Condvar, Mutex},
    // 使用单调时钟计算不可重置的服务器预算。
    time::{Duration, Instant},
};

// 导入严格请求、取消、响应和 epoch 状态机。
use super::browser_session_broker_protocol::{
    // 导入严格取消与协议失败。
    BrowserSessionBrokerCancellationRequest,
    BrowserSessionBrokerProtocolFailure,
    // 导入严格领域请求。
    BrowserSessionBrokerRequest,
    // 导入封闭响应。
    response::BrowserSessionBrokerResponse,
    // 导入 epoch、取消状态、去重决定与唯一状态机。
    state::{
        // 导入 cancel receipt 状态。
        BrowserSessionBrokerCancelStatus,
        // 导入 live epoch。
        BrowserSessionBrokerEpoch,
        // 导入唯一 execution/cancel 状态机。
        BrowserSessionBrokerEpochState,
        // 导入 dispatch、attach 与 replay 决定。
        BrowserSessionBrokerExecutionDecision,
    },
};

// 保存一次 request 观察与同一线性化点的变更代际。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrowserSessionBrokerRuntimeObservation {
    // 保存 dispatch、attach 或 replay 的封闭决定。
    decision: BrowserSessionBrokerExecutionDecision,
    // 保存观察决定时的全局变更代际。
    generation: u64,
}

// 为 request 观察提供只读投影。
impl BrowserSessionBrokerRuntimeObservation {
    // 返回封闭执行决定。
    pub(crate) fn decision(&self) -> &BrowserSessionBrokerExecutionDecision {
        // 借用不可变决定。
        &self.decision
    }

    // 返回用于无丢失等待的变更代际。
    pub(crate) const fn generation(&self) -> u64 {
        // 复制单调代际。
        self.generation
    }
}

// 表示等待响应状态变化的封闭结果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerRuntimeWait {
    // 状态在预算内发生变化。
    Changed,
    // 服务器绝对 deadline 已到达。
    DeadlineReached,
    // broker 已停止接受连接并唤醒等待者。
    Stopping,
}

// 表示唯一 dispatcher 在预检前取得的封闭状态。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BrowserSessionBrokerDispatchPreparation {
    // request 仍可进入无副作用 System 预检。
    Ready,
    // cancel、deadline 或既有终态已经阻止执行。
    Final,
}

// 保存 dispatcher 在业务接受线性化点取得的权威 snapshot。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrowserSessionBrokerDispatchCommit {
    // 保存 accepted、业务前 final 或既有 replay。
    response: BrowserSessionBrokerResponse,
    // 保存同一线性化点读取的不可延长 deadline。
    deadline_ms: u64,
    // 标记本调用是否刚建立唯一 accepted snapshot。
    should_execute: bool,
}

// 为 dispatcher commit 提供只读投影。
impl BrowserSessionBrokerDispatchCommit {
    // 返回必须发送或缓存的权威响应。
    pub(crate) fn response(&self) -> &BrowserSessionBrokerResponse {
        // 借用封闭响应。
        &self.response
    }

    // 返回业务执行使用的绝对 deadline。
    pub(crate) const fn deadline_ms(&self) -> u64 {
        // 复制不可延长 deadline。
        self.deadline_ms
    }

    // 返回 dispatcher 是否应恰好一次调用 System。
    pub(crate) const fn should_execute(&self) -> bool {
        // 只有本调用刚提交 accepted 时为真。
        self.should_execute
    }
}

// 保存必须在同一互斥线性化点推进的运行时状态。
#[derive(Debug, Default)]
struct RuntimeState {
    // 保存当前 epoch 的 execution 与 cancel ledger。
    epoch_state: BrowserSessionBrokerEpochState,
    // 保存 accepted/final/cancel 变化的单调代际。
    generation: u64,
    // 标记物理宿主是否仍接受连接等待。
    accepting_connections: bool,
}

// 保存不拥有 System 或 pipe 的共享 epoch 运行时。
#[derive(Debug)]
pub(crate) struct BrowserSessionBrokerRuntime {
    // 保存进程代际唯一且不可持久化的 epoch。
    epoch: BrowserSessionBrokerEpoch,
    // 保存服务器单调时间原点。
    clock_origin: Instant,
    // 保存唯一线性化状态。
    state: Mutex<RuntimeState>,
    // 唤醒 attach、replay 与 cancel 等待者。
    changed: Condvar,
}

// 为共享运行时提供不跨 System 调用持锁的窄操作。
impl BrowserSessionBrokerRuntime {
    // 创建一个尚未发布 endpoint 的 live epoch 运行时。
    pub(crate) fn new(
        // 接收进程代际唯一 canonical epoch。
        epoch: BrowserSessionBrokerEpoch,
    ) -> Self {
        // 构造空 ledger 与单调时钟。
        Self {
            // 保存 canonical epoch。
            epoch,
            // 固定本代际服务器时钟原点。
            clock_origin: Instant::now(),
            // 建立尚未接受连接的初始状态。
            state: Mutex::new(RuntimeState::default()),
            // 建立同一状态边界的条件变量。
            changed: Condvar::new(),
        }
    }

    // 返回当前 live epoch。
    pub(crate) fn epoch(&self) -> &BrowserSessionBrokerEpoch {
        // 借用不可变 epoch。
        &self.epoch
    }

    // 返回从本代际原点开始的单调毫秒值。
    pub(crate) fn now_ms(&self) -> u64 {
        // 读取不会受系统时间回拨影响的耗时。
        let elapsed = self.clock_origin.elapsed().as_millis();
        // 超出 u64 的理论运行时饱和到最大值。
        u64::try_from(elapsed).unwrap_or(u64::MAX)
    }

    // endpoint 发布后允许连接进入等待。
    pub(crate) fn start_accepting(&self) -> Result<(), BrowserSessionBrokerProtocolFailure> {
        // 短锁取得运行时状态。
        let mut state = self.state.lock().map_err(|_| failed())?;
        // 只改变宿主接入事实，不改变协议 ledger。
        state.accepting_connections = true;
        // 唤醒可能等待发布完成的宿主线程。
        self.changed.notify_all();
        // 返回成功。
        Ok(())
    }

    // 停止新连接等待并唤醒全部 attach handler。
    pub(crate) fn stop_accepting(&self) {
        // 锁污染时不再尝试恢复不可信状态。
        if let Ok(mut state) = self.state.lock() {
            // 关闭宿主接入门禁。
            state.accepting_connections = false;
            // 推进代际以解除全部无丢失等待。
            state.generation = state.generation.wrapping_add(1);
            // 广播停止事实。
            self.changed.notify_all();
        }
    }

    // 返回当前宿主是否仍接受连接。
    pub(crate) fn accepting_connections(
        // 借用共享运行时。
        &self,
    ) -> Result<bool, BrowserSessionBrokerProtocolFailure> {
        // 短锁复制接入事实。
        self.state
            // 取得状态锁。
            .lock()
            // 锁污染必须失败闭合。
            .map(|state| state.accepting_connections)
            // 映射为协议失败。
            .map_err(|_| failed())
    }

    // 使用当前单调时钟观察严格 request，仅供不模拟物理读取的纯测试。
    #[cfg(test)]
    pub(crate) fn observe_request(
        // 借用共享运行时。
        &self,
        // 借用严格 request。
        request: &BrowserSessionBrokerRequest,
    ) -> Result<BrowserSessionBrokerRuntimeObservation, BrowserSessionBrokerProtocolFailure> {
        // 委托显式时钟入口以保持同一语义。
        self.observe_request_at(self.now_ms(), request)
    }

    // 在显式服务器单调时刻线性化 request。
    fn observe_request_at(
        // 借用共享运行时。
        &self,
        // 接收服务器单调毫秒值。
        now_ms: u64,
        // 借用严格 request。
        request: &BrowserSessionBrokerRequest,
    ) -> Result<BrowserSessionBrokerRuntimeObservation, BrowserSessionBrokerProtocolFailure> {
        // 短锁取得唯一 epoch state。
        let mut state = self.state.lock().map_err(|_| failed())?;
        // 停止接入后不得新增或附着连接。
        if !state.accepting_connections {
            // 返回不携带可伪造业务事实的协议失败。
            return Err(failed());
        }
        // 保存重送前已绑定 request 的当前 deadline。
        let previous_deadline = state
            // 借用唯一 epoch state。
            .epoch_state
            // 未见 nonce 返回 None。
            .execution_deadline_ms(request.request_nonce());
        // 在同一线性化点取得去重决定。
        let decision = state
            // 可变借用唯一 epoch state。
            .epoch_state
            // 委托完整 nonce、deadline 与 replay ledger。
            .observe_request(&self.epoch, now_ms, request)?;
        // 读取观察后只可保持或缩短的 deadline。
        let current_deadline = state
            // 借用同一个 epoch state。
            .epoch_state
            // 使用同 nonce 取得最新值。
            .execution_deadline_ms(request.request_nonce());
        // 合法重送缩短 deadline 时必须唤醒旧 waiter 与在途 execution。
        if matches!((previous_deadline, current_deadline), (Some(previous), Some(current)) if current < previous)
        {
            // 推进全局变更代际，让旧 deadline waiter 观察 Changed。
            state.generation = state.generation.wrapping_add(1);
            // 唤醒所有 attach 与 cancel 观察者。
            self.changed.notify_all();
        }
        // 返回决定及不会错过后续唤醒的代际。
        Ok(BrowserSessionBrokerRuntimeObservation {
            // 保存封闭决定。
            decision,
            // 复制当前变更代际。
            generation: state.generation,
        })
    }

    // 以完整 frame 到达时刻首次观察严格 request。
    pub(crate) fn observe_received_request(
        // 借用共享运行时。
        &self,
        // 接收唯一完整 request frame 的单调到达时刻。
        received_at: Instant,
        // 借用严格 request。
        request: &BrowserSessionBrokerRequest,
    ) -> Result<BrowserSessionBrokerRuntimeObservation, BrowserSessionBrokerProtocolFailure> {
        // 把物理到达时刻投影到本 epoch 的同一毫秒坐标。
        let received_ms = self.milliseconds_at(received_at)?;
        // 首次 ledger deadline 必须以 frame 到达而非解析完成时刻为基准。
        self.observe_request_at(received_ms, request)
    }

    // 只读观察已建立 execution 的 snapshot，不重新解释 remaining timeout。
    pub(crate) fn observe_existing_request(
        // 借用共享运行时。
        &self,
        // 借用首次 frame 的同义严格 request。
        request: &BrowserSessionBrokerRequest,
    ) -> Result<BrowserSessionBrokerRuntimeObservation, BrowserSessionBrokerProtocolFailure> {
        // 短锁取得唯一 epoch state。
        let state = self.state.lock().map_err(|_| failed())?;
        // 停止接入后不得继续附着连接。
        if !state.accepting_connections {
            // 返回不携带可伪造业务事实的协议失败。
            return Err(failed());
        }
        // 只读取首次 frame 已建立的 Attach 或 Replay 决定。
        let decision = state
            // 借用唯一 epoch state。
            .epoch_state
            // 禁止内部重观察按当前时刻重算 deadline。
            .observe_existing_request(&self.epoch, request)?;
        // 返回决定及不会错过后续唤醒的代际。
        Ok(BrowserSessionBrokerRuntimeObservation {
            // 保存封闭决定。
            decision,
            // 复制当前变更代际。
            generation: state.generation,
        })
    }

    // 原子保存 accepted/final 并唤醒全部同 nonce attach。
    pub(crate) fn apply_response(
        // 借用共享运行时。
        &self,
        // 接收已验证封闭响应。
        response: BrowserSessionBrokerResponse,
    ) -> Result<(), BrowserSessionBrokerProtocolFailure> {
        // 短锁取得唯一 epoch state。
        let mut state = self.state.lock().map_err(|_| failed())?;
        // 保存封闭响应并结算关联 cancel receipt。
        state.epoch_state.apply_response(&self.epoch, response)?;
        // 每次成功状态变化只推进一次代际。
        state.generation = state.generation.wrapping_add(1);
        // 唤醒全部 attach 与 cancel 等待者。
        self.changed.notify_all();
        // 返回成功。
        Ok(())
    }

    // 在 System 预检前检查 cancel、replay 与 deadline。
    pub(crate) fn prepare_dispatch(
        // 借用共享运行时。
        &self,
        // 借用已经取得唯一 Dispatch 的 request。
        request: &BrowserSessionBrokerRequest,
    ) -> Result<BrowserSessionBrokerDispatchPreparation, BrowserSessionBrokerProtocolFailure> {
        // 读取当前服务器单调时刻。
        let now_ms = self.now_ms();
        // 短锁取得唯一 epoch state。
        let mut state = self.state.lock().map_err(|_| failed())?;
        // 只读重观察不会重新 dispatch、延长或重算 deadline。
        let decision = state
            // 借用唯一 ledger。
            .epoch_state
            // 使用首次物理 frame 已建立的绝对 deadline。
            .observe_existing_request(&self.epoch, request)?;
        // 按权威 ledger 决定进入预检或直接 replay。
        match decision {
            // 原始 handler 已经取得唯一 Dispatch，因此这里应为 Attach。
            BrowserSessionBrokerExecutionDecision::Attach {
                // 读取不可延长 deadline。
                deadline_ms,
                // 读取可选 accepted snapshot。
                response,
                // revision 由 response 自身携带。
                ..
            } => {
                // 已有 accepted 表示重复 dispatcher，必须失败闭合。
                if response.is_some() {
                    // 不允许第二次领域执行。
                    return Err(failed());
                }
                // deadline 已到时在业务接受前建立唯一 expired final。
                if now_ms >= deadline_ms {
                    // 构造 revision 零 deadline final。
                    let expired = BrowserSessionBrokerResponse::expired_before_acceptance(
                        // 绑定严格 request。
                        request,
                        // 业务前终态固定 revision 零。
                        0,
                        // 回显当前 live epoch。
                        &self.epoch,
                    )
                    // 合法严格 request 必须允许构造 deadline final。
                    .ok_or_else(failed)?;
                    // 保存终态并推进 cancel receipt。
                    state
                        .epoch_state
                        .apply_response(&self.epoch, expired.clone())?;
                    // 推进全局变更代际。
                    advance_generation(&mut state);
                    // 广播新终态。
                    self.changed.notify_all();
                    // 返回直接 replay。
                    return Ok(BrowserSessionBrokerDispatchPreparation::Final);
                }
                // 尚有预算时才允许无副作用预检。
                Ok(BrowserSessionBrokerDispatchPreparation::Ready)
            }
            // cancel tombstone 或既有终态永远直接 replay。
            BrowserSessionBrokerExecutionDecision::Replay { .. } => {
                // 返回 terminal-wins 事实。
                Ok(BrowserSessionBrokerDispatchPreparation::Final)
            }
            // dispatcher 不得取得第二个 Dispatch 决定。
            BrowserSessionBrokerExecutionDecision::Dispatch { .. } => Err(failed()),
        }
    }

    // 在预检后原子提交 accepted 或业务前 rejection。
    pub(crate) fn commit_dispatch_snapshot(
        // 借用共享运行时。
        &self,
        // 借用原始严格 request。
        request: &BrowserSessionBrokerRequest,
        // 接收 accepted 或业务前 final 候选。
        candidate: BrowserSessionBrokerResponse,
    ) -> Result<BrowserSessionBrokerDispatchCommit, BrowserSessionBrokerProtocolFailure> {
        // 读取当前服务器单调时刻。
        let now_ms = self.now_ms();
        // 短锁覆盖 cancel 与 accepted 的竞态窗口。
        let mut state = self.state.lock().map_err(|_| failed())?;
        // 在同一锁内只读观察最新 tombstone、snapshot 与首次 deadline。
        let decision = state
            // 借用唯一 ledger。
            .epoch_state
            // dispatcher 内部不得把处理耗时重新解释为一次重送。
            .observe_existing_request(&self.epoch, request)?;
        // 只允许原始 Dispatch 对应的未接受 Attach 进入提交。
        match decision {
            // 未接受 request 可提交唯一 snapshot。
            BrowserSessionBrokerExecutionDecision::Attach {
                // 读取不可延长 deadline。
                deadline_ms,
                // 读取当前可选 snapshot。
                response,
                // revision 由候选响应校验。
                ..
            } => {
                // 已有 snapshot 表示其他路径已经越过线性化点。
                if let Some(response) = response {
                    // 返回已有权威 snapshot，绝不执行候选。
                    return Ok(BrowserSessionBrokerDispatchCommit {
                        // 保存 ledger snapshot。
                        response,
                        // 保留当前 deadline。
                        deadline_ms,
                        // 已有 snapshot 绝不允许第二次执行。
                        should_execute: false,
                    });
                }
                // deadline 在业务接受前到达时覆盖 accepted 或 rejection 候选。
                let response = if now_ms >= deadline_ms {
                    // 构造唯一 expired-before-acceptance final。
                    BrowserSessionBrokerResponse::expired_before_acceptance(
                        // 绑定原始 request。
                        request,
                        // 首个终态固定 revision 零。
                        0,
                        // 绑定 live epoch。
                        &self.epoch,
                    )
                    // 严格 request 必须可构造 deadline final。
                    .ok_or_else(failed)?
                } else {
                    // 尚有预算时使用 dispatcher 已验证候选。
                    candidate
                };
                // 仅 freshly committed accepted 可触发一次 System 调用。
                let should_execute = response.outcome().is_none()
                    // accepted 必须明确越过业务接受点。
                    && response.business_accepted();
                // 在 cancel 仍无法插入的同一锁内保存 snapshot。
                state
                    // 可变借用唯一 epoch state。
                    .epoch_state
                    // 委托 response 关联与 revision 门禁。
                    .apply_response(&self.epoch, response.clone())?;
                // 推进一次全局代际。
                advance_generation(&mut state);
                // 广播 accepted 或 final。
                self.changed.notify_all();
                // 返回实际提交的权威 snapshot。
                Ok(BrowserSessionBrokerDispatchCommit {
                    // 保存 accepted/final。
                    response,
                    // 保存执行预算。
                    deadline_ms,
                    // 保存恰好一次执行许可。
                    should_execute,
                })
            }
            // cancel 先到或 preflight 期间已有 terminal 时直接 replay。
            BrowserSessionBrokerExecutionDecision::Replay { response } => {
                // terminal-wins 不写候选。
                Ok(BrowserSessionBrokerDispatchCommit {
                    // 保存权威 final。
                    response,
                    // terminal 不再执行，deadline 只作诊断隔离值。
                    deadline_ms: now_ms,
                    // terminal replay 不得执行。
                    should_execute: false,
                })
            }
            // dispatcher 不得重新取得 Dispatch。
            BrowserSessionBrokerExecutionDecision::Dispatch { .. } => Err(failed()),
        }
    }

    // 在线性化点观察 cancel 并唤醒目标 request handler。
    pub(crate) fn observe_cancel(
        // 借用共享运行时。
        &self,
        // 借用严格 cancel frame。
        cancel: &BrowserSessionBrokerCancellationRequest,
    ) -> Result<(BrowserSessionBrokerCancelStatus, u64), BrowserSessionBrokerProtocolFailure> {
        // 短锁取得唯一 epoch state。
        let mut state = self.state.lock().map_err(|_| failed())?;
        // 停止接入后不再改变 cancel ledger。
        if !state.accepting_connections {
            // 返回失败闭合。
            return Err(failed());
        }
        // 原子去重 cancel 并推进 target 状态。
        let receipt = state.epoch_state.observe_cancel(&self.epoch, cancel)?;
        // cancel 重送也推进观察代际以避免遗漏最新 receipt。
        state.generation = state.generation.wrapping_add(1);
        // 唤醒 target execution 与 control 等待者。
        self.changed.notify_all();
        // 返回封闭 receipt 状态和 revision。
        Ok(receipt)
    }

    // 查询 accepted execution 是否收到协作取消。
    #[cfg(test)]
    pub(crate) fn cancellation_requested(
        // 借用共享运行时。
        &self,
        // 借用 target request nonce。
        request_nonce: &str,
    ) -> Result<bool, BrowserSessionBrokerProtocolFailure> {
        // 短锁只读查询持久 cancel 事实。
        self.state
            // 取得唯一状态锁。
            .lock()
            // 投影 target 取消事实。
            .map(|state| state.epoch_state.cancellation_requested(request_nonce))
            // 锁污染失败闭合。
            .map_err(|_| failed())
    }

    // 按当前 min-only deadline 与持久 cancel 事实判断 execution 是否应协作停止。
    pub(crate) fn execution_should_stop(
        // 借用共享运行时。
        &self,
        // 借用 target request nonce。
        request_nonce: &str,
    ) -> Result<bool, BrowserSessionBrokerProtocolFailure> {
        // 短锁取得 deadline 与 cancel 的同一线性化 snapshot。
        let state = self.state.lock().map_err(|_| failed())?;
        // 取锁后读取当前单调时刻，避免等锁期间越过 deadline。
        let now_ms = self.now_ms();
        // 缺失 request record 意味着 execution 不可再被信任。
        let Some(deadline_ms) = state.epoch_state.execution_deadline_ms(request_nonce) else {
            // 保守请求停止而不伪造任何终态。
            return Ok(true);
        };
        // 显式 cancel 或已缩短 deadline 到达都只请求协作停止。
        Ok(state.epoch_state.cancellation_requested(request_nonce) || now_ms >= deadline_ms)
    }

    // 将已绑定请求的 min-only 单调 deadline 投影为宿主可直接使用的绝对时刻。
    pub(crate) fn execution_deadline(
        // 借用共享运行时。
        &self,
        // 借用目标 request nonce。
        request_nonce: &str,
    ) -> Result<Option<Instant>, BrowserSessionBrokerProtocolFailure> {
        // 短锁复制唯一 ledger 的当前 deadline，绝不跨 pipe 写入持锁。
        let deadline_ms = self
            // 取得唯一状态锁。
            .state
            // 锁污染时拒绝伪造新的传输预算。
            .lock()
            // 投影当前 nonce 的不可延长 deadline。
            .map(|state| state.epoch_state.execution_deadline_ms(request_nonce))
            // 锁污染失败闭合。
            .map_err(|_| failed())?;
        // ledger 前 rejection 没有可绑定的请求 deadline。
        let Some(deadline_ms) = deadline_ms else {
            // 显式区分缺失记录与锁污染等运行时失败。
            return Ok(None);
        };
        // 将 epoch-relative 毫秒转换为同一单调时钟上的绝对时刻。
        self.clock_origin
            // 时钟理论溢出时关闭连接而非延长请求。
            .checked_add(Duration::from_millis(deadline_ms))
            // 返回不泄漏内部时钟原因的失败。
            .map(Some)
            // 理论时钟溢出不得让 request-bound 回包回退到新预算。
            .ok_or_else(failed)
    }

    // 将任意本进程单调时刻投影为本 epoch 的毫秒坐标。
    fn milliseconds_at(
        // 借用共享运行时。
        &self,
        // 接收不早于 runtime 创建的单调时刻。
        instant: Instant,
    ) -> Result<u64, BrowserSessionBrokerProtocolFailure> {
        // 计算与 runtime 时钟原点一致的单调耗时。
        let elapsed = instant
            // 理论早于本代际原点的时刻必须失败闭合。
            .checked_duration_since(self.clock_origin)
            // 不伪造零时刻。
            .ok_or_else(failed)?;
        // 将协议毫秒坐标限制在 u64 范围内。
        u64::try_from(elapsed.as_millis())
            // 理论超长进程寿命不得绕回 deadline。
            .map_err(|_| failed())
    }

    // 等待状态代际改变、deadline 到达或宿主停止。
    pub(crate) fn wait_for_change(
        // 借用共享运行时。
        &self,
        // 接收调用方已观察的状态代际。
        observed_generation: u64,
        // 接收服务器绝对 deadline。
        deadline_ms: u64,
    ) -> Result<BrowserSessionBrokerRuntimeWait, BrowserSessionBrokerProtocolFailure> {
        // 取得与条件变量配对的唯一状态锁。
        let mut state = self.state.lock().map_err(|_| failed())?;
        // 使用循环抵抗虚假唤醒。
        loop {
            // 宿主停止优先解除所有连接等待。
            if !state.accepting_connections {
                // 返回稳定停止事实。
                return Ok(BrowserSessionBrokerRuntimeWait::Stopping);
            }
            // 代际变化证明存在新的 accepted/final/cancel 状态。
            if state.generation != observed_generation {
                // 返回变化事实。
                return Ok(BrowserSessionBrokerRuntimeWait::Changed);
            }
            // 读取当前服务器单调时刻。
            let now_ms = self.now_ms();
            // 到达绝对 deadline 后不得继续等待。
            if now_ms >= deadline_ms {
                // 返回 deadline 事实。
                return Ok(BrowserSessionBrokerRuntimeWait::DeadlineReached);
            }
            // 计算不会越过绝对 deadline 的剩余预算。
            let remaining_ms = deadline_ms.saturating_sub(now_ms);
            // 等待状态改变或预算耗尽。
            let waited = self
                // 使用同一条件变量。
                .changed
                // 将毫秒转换为固定等待时长。
                .wait_timeout(state, Duration::from_millis(remaining_ms))
                // 锁污染失败闭合。
                .map_err(|_| failed())?;
            // 取回互斥状态 guard。
            state = waited.0;
        }
    }
}

// 在锁内推进一次全局状态代际。
fn advance_generation(
    // 可变借用仍在锁内的运行时状态。
    state: &mut RuntimeState,
) {
    // generation 只是等待令牌，环回仍会与上一次观察值不同。
    state.generation = state.generation.wrapping_add(1);
}

// 构造不回显内部锁或时钟事实的协议失败。
fn failed() -> BrowserSessionBrokerProtocolFailure {
    // 使用协议状态失败稳定码。
    BrowserSessionBrokerProtocolFailure::new(
        // 不把内部同步故障伪装为业务拒绝。
        super::browser_session_broker_protocol::BrowserSessionBrokerProtocolErrorCode::ProtocolFailed,
    )
}

// 注册纯运行时协调回归。
#[cfg(test)]
#[path = "browser_session_broker_runtime_tests.rs"]
mod tests;
