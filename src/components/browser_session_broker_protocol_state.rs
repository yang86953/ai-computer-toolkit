//! 定义 live broker epoch、deadline 去重、取消和连接状态机。

// 导入有序 nonce 映射。
use std::collections::BTreeMap;

// 导入请求、取消输入与封闭 operation。
use super::{
    BrowserSessionBrokerCancellationRequest, BrowserSessionBrokerOperation,
    BrowserSessionBrokerRequest,
};
// 导入封闭响应值与业务终态。
use super::response::{BrowserSessionBrokerOutcome, BrowserSessionBrokerResponse};
// 导入失败类型。
use super::{BrowserSessionBrokerProtocolErrorCode, BrowserSessionBrokerProtocolFailure};
// 导入 nonce 长度。
use super::NONCE_LENGTH;

// 注册同一状态边界的 execution ledger 实现。
#[path = "browser_session_broker_execution_ledger.rs"]
mod execution_ledger;
// 向协议状态调用方公开只读 request 决定。
pub(crate) use execution_ledger::BrowserSessionBrokerExecutionDecision;
// 仅在 EpochState 内使用 cancel target 决定。
use execution_ledger::BrowserSessionBrokerCancelTargetDecision;
// 仅在 EpochState 内使用固定 execution ledger。
use execution_ledger::BrowserSessionBrokerExecutionLedger;

// 表示运行中 broker 的非持久 epoch。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionBrokerEpoch {
    // 保存 canonical epoch。
    value: String,
}

// 为 epoch 提供构造和访问。
impl BrowserSessionBrokerEpoch {
    // 从主机安全生成的 nonce 构造 epoch。
    pub(crate) fn new(value: String) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // 拒绝非 canonical 值。
        if !hex(&value) {
            // 返回协议错误。
            return Err(failed());
        }
        // 返回 epoch。
        Ok(Self { value })
    }

    // 返回 epoch 文本。
    pub(crate) fn as_str(&self) -> &str {
        // 借用 epoch。
        &self.value
    }
}

// 表示取消的封闭 receipt 状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerCancelStatus {
    // 当前 epoch 未知目标 request，且已经安装 terminal tombstone。
    UnknownRequest,
    // 已登记协作取消但原结果未决。
    CancellationRequested,
    // 原请求已建立非取消终态。
    TooLate,
    // 原请求已确定 cancelled。
    Cancelled,
}

// 保存 cancel nonce 已见语义和最后 receipt。
#[derive(Clone, Debug, Eq, PartialEq)]
struct CancelRecord {
    // 保存完整 cancel 语义键。
    semantic_key: String,
    // 保存关联的唯一 target request nonce。
    target_request_nonce: String,
    // 保存不可倒退的取消状态。
    status: BrowserSessionBrokerCancelStatus,
    // 保存单调 revision。
    revision: u64,
}

// 保存当前 epoch 的 cancel 去重状态。
#[derive(Debug, Default)]
struct BrowserSessionBrokerCancelLedger {
    // 映射 cancel nonce 到单调状态。
    cancels: BTreeMap<String, CancelRecord>,
}

// 为 cancel ledger 提供去重和单调 transition。
impl BrowserSessionBrokerCancelLedger {
    // 在改变 target execution 前验证 epoch、同义性与容量。
    fn preflight(
        &self,
        epoch: &BrowserSessionBrokerEpoch,
        cancel: &BrowserSessionBrokerCancellationRequest,
    ) -> Result<Option<(BrowserSessionBrokerCancelStatus, u64)>, BrowserSessionBrokerProtocolFailure>
    {
        // epoch 不匹配必须以 cancel-rejected 失败闭合。
        if cancel.expected_broker_epoch() != epoch.as_str() {
            // 不得写入 cancel 或 execution ledger。
            return Err(BrowserSessionBrokerProtocolFailure::new(
                // 使用稳定 stale epoch 错误码。
                BrowserSessionBrokerProtocolErrorCode::StaleBrokerEpoch,
            ));
        }
        // 构造完整 cancel 语义。
        let semantic_key = cancel.canonical_semantic_key();
        // 相同 cancel nonce 必须同义。
        if let Some(record) = self.cancels.get(cancel.cancel_request_nonce()) {
            // 拒绝冲突 cancel。
            if record.semantic_key != semantic_key {
                // 返回冲突。
                return Err(BrowserSessionBrokerProtocolFailure::new(
                    BrowserSessionBrokerProtocolErrorCode::NonceSemanticConflict,
                ));
            }
            // 返回已有单调 receipt。
            return Ok(Some((record.status, record.revision)));
        }
        // cancel ledger 使用固定 4096 条容量且 epoch 内不淘汰。
        if self.cancels.len() >= 4_096 {
            // 返回 ledger full。
            return Err(BrowserSessionBrokerProtocolFailure::new(
                BrowserSessionBrokerProtocolErrorCode::BrokerRequestLedgerFull,
            ));
        }
        // 表示可以安全进入 target execution 线性化点。
        Ok(None)
    }

    // 在 preflight 与 execution 线性化成功后插入首次 receipt。
    fn insert_initial(
        &mut self,
        cancel: &BrowserSessionBrokerCancellationRequest,
        initial: BrowserSessionBrokerCancelStatus,
    ) -> (BrowserSessionBrokerCancelStatus, u64) {
        // 首次 cancel 从 revision 零建立。
        let record = CancelRecord {
            // 保存完整 canonical cancel key。
            semantic_key: cancel.canonical_semantic_key(),
            // 保存用于业务终态自动结算的 target。
            target_request_nonce: cancel.request_nonce().to_owned(),
            // 保存线性化得到的初态。
            status: initial,
            // 首次可观察 receipt revision 为零。
            revision: 0,
        };
        // 保存 record；preflight 已保证 nonce 尚不存在且容量充足。
        self.cancels
            // 使用 cancel 自身 nonce 作键。
            .insert(cancel.cancel_request_nonce().to_owned(), record);
        // 返回首次 receipt。
        (initial, 0)
    }

    // 预检同一 target 的全部协作取消回执是否都可推进 revision。
    fn preflight_finalize_target(
        // 借用 cancel ledger。
        &self,
        // 借用业务终态关联的 request nonce。
        request_nonce: &str,
    ) -> Result<(), BrowserSessionBrokerProtocolFailure> {
        // 验证全部 revision 可推进，避免 execution 提交后的失败窗口。
        for record in self.cancels.values() {
            // 仅协作取消且同一 target 需要结算。
            if record.status == BrowserSessionBrokerCancelStatus::CancellationRequested
                // target nonce 必须精确关联。
                && record.target_request_nonce == request_nonce
            {
                // 提前验证 revision 溢出。
                record.revision.checked_add(1).ok_or_else(failed)?;
            }
        }
        // 返回 execution 可安全建立业务终态的事实。
        Ok(())
    }

    // 在已完成 revision 预检后不可失败地结算同一 target 的协作取消回执。
    fn commit_finalize_target(
        // 可变借用 cancel ledger。
        &mut self,
        // 借用业务终态关联的 request nonce。
        request_nonce: &str,
        // 接收已建立的权威业务 outcome。
        outcome: BrowserSessionBrokerOutcome,
    ) {
        // cancelled 业务终态是取消成功，其余业务终态均已太晚。
        let status = if outcome == BrowserSessionBrokerOutcome::Cancelled {
            // 保留权威 cancelled receipt。
            BrowserSessionBrokerCancelStatus::Cancelled
        } else {
            // 保留不改写原业务结果的 too-late receipt。
            BrowserSessionBrokerCancelStatus::TooLate
        };
        // 在预检保证的范围内批量推进所有匹配回执。
        for record in self.cancels.values_mut() {
            // 仅协作取消且同一 target 需要结算。
            if record.status == BrowserSessionBrokerCancelStatus::CancellationRequested
                // target nonce 必须精确关联。
                && record.target_request_nonce == request_nonce
            {
                // 保存从业务终态导出的唯一取消终态。
                record.status = status;
                // 预检已证明此处不会溢出，因此直接提交单调 revision。
                record.revision += 1;
            }
        }
    }

    // 仅允许组件内测试注入待结算 cancel 的 revision 边界值。
    #[cfg(test)]
    fn set_pending_revision_for_test(
        // 可变借用 cancel ledger。
        &mut self,
        // 借用待注入的 cancel nonce。
        cancel_nonce: &str,
        // 接收测试专用 revision 值。
        revision: u64,
    ) -> Result<(), BrowserSessionBrokerProtocolFailure> {
        // 只读取已登记的 cancel record。
        let record = self.cancels.get_mut(cancel_nonce).ok_or_else(failed)?;
        // 注入目标必须仍待业务终态结算。
        if record.status != BrowserSessionBrokerCancelStatus::CancellationRequested {
            // 拒绝对终态或未知 cancel 注入状态。
            return Err(failed());
        }
        // 保存仅测试可见的 revision 边界值。
        record.revision = revision;
        // 返回注入成功。
        Ok(())
    }
}

// 保存一个 live epoch 内必须在同一线性化点协调的 execution 与 cancel ledger。
#[derive(Debug, Default)]
pub(crate) struct BrowserSessionBrokerEpochState {
    // 保存 request dispatch、attach、replay 与 tombstone。
    execution: BrowserSessionBrokerExecutionLedger,
    // 保存 cancel nonce 去重与单调 receipt。
    cancellations: BrowserSessionBrokerCancelLedger,
}

// 为 epoch state 提供 request 与 cancel 的单入口状态推进。
impl BrowserSessionBrokerEpochState {
    // 接受 request 并返回唯一 dispatch/attach/replay 决定。
    pub(crate) fn observe_request(
        // 可变借用 epoch state。
        &mut self,
        // 借用当前 epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 接收服务器单调时钟毫秒值。
        now_ms: u64,
        // 借用严格 request。
        request: &BrowserSessionBrokerRequest,
    ) -> Result<BrowserSessionBrokerExecutionDecision, BrowserSessionBrokerProtocolFailure> {
        // 在线性化点观察 request。
        match self.execution.observe(epoch, now_ms, request) {
            // 直接返回成功决定。
            Ok(decision) => Ok(decision),
            // 可安全关联的业务前 ledger 拒绝必须携带 nonce 与 operation。
            Err(failure)
                if matches!(
                    failure.code(),
                    // 同 nonce 异义必须形成可关联拒绝。
                    BrowserSessionBrokerProtocolErrorCode::NonceSemanticConflict
                        // business 前预留失败也必须可关联。
                        | BrowserSessionBrokerProtocolErrorCode::BrokerRequestLedgerFull
                        // 防御性覆盖绕过 epoch-first parser 的调用。
                        | BrowserSessionBrokerProtocolErrorCode::StaleBrokerEpoch
                ) =>
            {
                // 包装为 schema 可投影的 final-rejected 失败。
                Err(BrowserSessionBrokerProtocolFailure::rejected_for_request(
                    // 保留稳定错误码并从严格 request 复制完整公共关联。
                    failure.code(),
                    // 绑定 nonce、operation 与 expected epoch。
                    request,
                ))
            }
            // 内部状态污染保持不可关联失败。
            Err(failure) => Err(failure),
        }
    }

    // 观察已经由物理 request frame 建立的 execution，不重新计算 deadline。
    pub(crate) fn observe_existing_request(
        // 不可变借用 epoch state。
        &self,
        // 借用当前 epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 借用首次 frame 的同义严格 request。
        request: &BrowserSessionBrokerRequest,
    ) -> Result<BrowserSessionBrokerExecutionDecision, BrowserSessionBrokerProtocolFailure> {
        // 委托 execution ledger 的只读 Attach/Replay 入口。
        self.execution.observe_existing(epoch, request)
    }

    // 保存封闭响应并执行 request 关联、revision 与 terminal-wins 门禁。
    pub(crate) fn apply_response(
        // 可变借用 epoch state。
        &mut self,
        // 借用当前 live epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 接收响应 Component 已验证的值。
        response: BrowserSessionBrokerResponse,
    ) -> Result<(), BrowserSessionBrokerProtocolFailure> {
        // 在移动响应前保留其唯一 target nonce。
        let request_nonce = response.request_nonce().to_owned();
        // 仅业务接受终态需要在 execution 提交前预检 cancel revision。
        let requires_cancel_commit = response.outcome().is_some() && response.business_accepted();
        // 先排除任何会阻止全部回执提交的 revision 溢出。
        if requires_cancel_commit {
            // 失败时 execution snapshot 尚未写入。
            self.cancellations
                .preflight_finalize_target(&request_nonce)?;
        }
        // 委托唯一 execution ledger 构造并持久保存内部快照。
        let business_terminal = self.execution.apply_response(epoch, response)?;
        // 业务终态建立后在同一可变线性化点结算全部协作取消。
        if let Some(outcome) = business_terminal {
            // 不跨 Module 调用，直接推进本状态边界拥有的 cancel ledger。
            self.cancellations
                .commit_finalize_target(&request_nonce, outcome);
        }
        // 所有关联状态均已原子建立。
        Ok(())
    }

    // 在线性化点去重 cancel 并原子决定 target execution 状态。
    pub(crate) fn observe_cancel(
        // 可变借用 epoch state。
        &mut self,
        // 借用当前 live epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 借用严格 cancel frame。
        cancel: &BrowserSessionBrokerCancellationRequest,
    ) -> Result<(BrowserSessionBrokerCancelStatus, u64), BrowserSessionBrokerProtocolFailure> {
        // 相同 cancel nonce 重送只 replay receipt，不再次改变 target。
        if let Some(existing) = self.cancellations.preflight(epoch, cancel)? {
            // 返回已有状态与 revision。
            return Ok(existing);
        }
        // 在唯一 execution ledger 中线性化 target cancel。
        let target = self
            // 可变借用 execution ledger。
            .execution
            // 查询或安装 target tombstone。
            .cancel_target(epoch, cancel.request_nonce())?;
        // 把 target execution 状态映射为封闭 cancel receipt。
        let status = match target {
            // 未知 target 已安装 terminal tombstone。
            BrowserSessionBrokerCancelTargetDecision::UnknownRequest => {
                // receipt 保持 unknown-request。
                BrowserSessionBrokerCancelStatus::UnknownRequest
            }
            // business 前 target 已形成可信取消终态。
            BrowserSessionBrokerCancelTargetDecision::CancelledBeforeAcceptance => {
                // cancel receipt 可权威返回 cancelled。
                BrowserSessionBrokerCancelStatus::Cancelled
            }
            // accepted target 只登记协作停止。
            BrowserSessionBrokerCancelTargetDecision::CancellationRequested => {
                // 返回非终态 cancellation-requested。
                BrowserSessionBrokerCancelStatus::CancellationRequested
            }
            // 原 request 已有 cancelled final。
            BrowserSessionBrokerCancelTargetDecision::Cancelled => {
                // 返回 cancelled terminal。
                BrowserSessionBrokerCancelStatus::Cancelled
            }
            // 原 request 已有其他 final。
            BrowserSessionBrokerCancelTargetDecision::TooLate => {
                // 返回 too-late terminal。
                BrowserSessionBrokerCancelStatus::TooLate
            }
        };
        // target 线性化成功后才写入 cancel ledger。
        Ok(self.cancellations.insert_initial(cancel, status))
    }

    // 只读返回 target 是否已登记协作取消。
    pub(crate) fn cancellation_requested(
        // 借用 epoch state。
        &self,
        // 借用目标 request nonce。
        request_nonce: &str,
    ) -> bool {
        // 委托 execution ledger 的持久事实投影。
        self.execution.cancellation_requested(request_nonce)
    }

    // 返回已绑定 request 当前只可缩短的绝对 deadline。
    pub(crate) fn execution_deadline_ms(
        // 借用 epoch state。
        &self,
        // 借用目标 request nonce。
        request_nonce: &str,
    ) -> Option<u64> {
        // 委托 execution ledger 的封闭只读投影。
        self.execution.deadline_ms(request_nonce)
    }

    // 返回测试所需的 retained entry 数量。
    #[cfg(test)]
    pub(crate) fn execution_record_count_for_test(&self) -> usize {
        // 委托只读 ledger 投影。
        self.execution.record_count()
    }

    // 返回测试所需的 replay reservation 总字节。
    #[cfg(test)]
    pub(crate) const fn execution_reserved_replay_bytes_for_test(&self) -> usize {
        // 委托只读 ledger 投影。
        self.execution.reserved_replay_bytes()
    }

    // 返回测试所需的缓存响应。
    #[cfg(test)]
    pub(crate) fn execution_response_for_test(
        // 借用 epoch state。
        &self,
        // 借用目标 request nonce。
        request_nonce: &str,
    ) -> Option<&BrowserSessionBrokerResponse> {
        // 委托只读 ledger 投影。
        self.execution.response_for_test(request_nonce)
    }
}

// 表示一个连接内唯一 request 的阶段。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerConnectionPhase {
    // 尚无完整 transport request。
    Empty,
    // transport 已接受但业务未接受。
    TransportAccepted,
    // 业务已接受。
    BusinessAccepted,
    // 已建立 final。
    Final,
}

// 保存连接协议状态。
#[derive(Debug)]
pub(crate) struct BrowserSessionBrokerConnection {
    // 保存当前阶段。
    phase: BrowserSessionBrokerConnectionPhase,
    // 保存本连接唯一 request nonce。
    request_nonce: Option<String>,
    // 保存本连接唯一 operation。
    operation: Option<BrowserSessionBrokerOperation>,
    // 保存已由连接观察的最后单调 response revision。
    last_revision: Option<u64>,
    // 保存 command/query mutation 事实。
    target_may_have_mutated: bool,
}

// 为连接提供单调状态转换。
impl Default for BrowserSessionBrokerConnection {
    // 构造空连接。
    fn default() -> Self {
        // 返回无请求状态。
        Self {
            // 连接初始没有 request。
            phase: BrowserSessionBrokerConnectionPhase::Empty,
            // 尚未绑定 request nonce。
            request_nonce: None,
            // 尚未绑定 operation。
            operation: None,
            // 尚未观察任何 response revision。
            last_revision: None,
            // 尚未建立 mutation 事实。
            target_may_have_mutated: false,
        }
    }
}

// 为连接提供状态访问与推进。
impl BrowserSessionBrokerConnection {
    // 建立 transport 接受。
    pub(crate) fn accept_transport(
        &mut self,
        request: &BrowserSessionBrokerRequest,
    ) -> Result<(), BrowserSessionBrokerProtocolFailure> {
        // 每连接只接受一个 request。
        if self.phase != BrowserSessionBrokerConnectionPhase::Empty {
            // 返回连接限额。
            return Err(BrowserSessionBrokerProtocolFailure::new(
                BrowserSessionBrokerProtocolErrorCode::ConnectionRequestLimit,
            ));
        }
        // 保存 immutable mutation 事实。
        self.target_may_have_mutated = request.may_mutate_target();
        // 绑定唯一 request nonce 以拒绝跨 request 响应。
        self.request_nonce = Some(request.request_nonce().to_owned());
        // 绑定唯一 operation 以拒绝跨 operation 响应。
        self.operation = Some(request.operation());
        // transport 接受前不存在可继承的 response revision。
        self.last_revision = None;
        // 推进 transport 接受。
        self.phase = BrowserSessionBrokerConnectionPhase::TransportAccepted;
        // 返回成功。
        Ok(())
    }

    // 按封闭响应的既有不变量推进连接阶段。
    pub(crate) fn apply_response(
        // 可变借用连接状态。
        &mut self,
        // 借用已被响应 Component 验证的响应。
        response: &BrowserSessionBrokerResponse,
    ) -> Result<(), BrowserSessionBrokerProtocolFailure> {
        // 响应必须关联本连接唯一 request nonce。
        if self.request_nonce.as_deref() != Some(response.request_nonce())
            // 响应 operation 也必须关联本连接唯一 operation。
            || self.operation != Some(response.operation())
            // 所有进入连接状态机的响应都必须已 transport accepted。
            || !response.transport_accepted()
        {
            // 拒绝 foreign request、operation 或 transport 组合。
            return Err(failed());
        }
        // 从封闭 response 推导唯一允许的阶段迁移。
        match self.phase {
            // 尚未绑定 request 时不能观察 response。
            BrowserSessionBrokerConnectionPhase::Empty => Err(failed()),
            // transport 后允许业务 accepted、业务前 final 或 replay 的业务 final。
            BrowserSessionBrokerConnectionPhase::TransportAccepted => {
                // 非终态只能表示 business accepted。
                if response.outcome().is_none()
                    // accepted 必须保持业务接受事实。
                    && response.business_accepted()
                    // transport 后的首个 accepted 固定为 revision 零。
                    && response.request_revision() == 0
                {
                    // 保存业务接受阶段。
                    self.phase = BrowserSessionBrokerConnectionPhase::BusinessAccepted;
                    // 保存已观察的首个 revision。
                    self.last_revision = Some(0);
                    // 返回成功。
                    return Ok(());
                }
                // 业务前终态必须以 revision 零直接终结 transport 阶段。
                if response.outcome().is_some()
                    // 业务前终态不得声称已 business accepted。
                    && !response.business_accepted()
                    // business 接受前的首个 final 固定为 revision 零。
                    && response.request_revision() == 0
                {
                    // 保存终态阶段。
                    self.phase = BrowserSessionBrokerConnectionPhase::Final;
                    // 保存已观察的业务前终态 revision。
                    self.last_revision = Some(0);
                    // 返回成功。
                    return Ok(());
                }
                // 重放业务终态可跳过本连接未观察到的 accepted，但固定只能是 revision 一。
                if response.outcome().is_some()
                    // 重放业务终态必须保持业务已接受事实。
                    && response.business_accepted()
                    // 首个可跳过 accepted 的 terminal revision 固定为一。
                    && response.request_revision() == 1
                {
                    // 保存终态阶段。
                    self.phase = BrowserSessionBrokerConnectionPhase::Final;
                    // 保存已观察的重放终态 revision。
                    self.last_revision = Some(1);
                    // 返回成功。
                    return Ok(());
                }
                // 其余组合违反 response 不变量。
                Err(failed())
            }
            // 业务接受后只允许业务接受的终态。
            BrowserSessionBrokerConnectionPhase::BusinessAccepted => {
                // 读取下一 revision，溢出必须失败闭合。
                let expected_revision = self
                    // 已接受阶段必须保存 accepted revision。
                    .last_revision
                    // 计算唯一允许的下一 revision。
                    .and_then(|revision| revision.checked_add(1))
                    // 缺失或溢出均违反连接状态。
                    .ok_or_else(failed)?;
                // 终态必须保持业务已接受事实并精确匹配下一 revision。
                if response.outcome().is_some()
                    // 终态不得倒退到业务前。
                    && response.business_accepted()
                    // revision 必须严格连续，不接受跳号或重送。
                    && response.request_revision() == expected_revision
                {
                    // 保存终态阶段。
                    self.phase = BrowserSessionBrokerConnectionPhase::Final;
                    // 保存已观察的业务终态 revision。
                    self.last_revision = Some(expected_revision);
                    // 返回成功。
                    return Ok(());
                }
                // 拒绝重复 accepted、业务前 final 与阶段倒退。
                Err(failed())
            }
            // final-wins，任何后续 response 都是协议污染。
            BrowserSessionBrokerConnectionPhase::Final => Err(failed()),
        }
    }

    // 返回当前阶段。
    pub(crate) const fn phase(&self) -> BrowserSessionBrokerConnectionPhase {
        // 复制阶段。
        self.phase
    }

    // 返回 query 永远 false 的 mutation 投影。
    pub(crate) const fn target_may_have_mutated(&self) -> bool {
        // 复制固定事实。
        self.target_may_have_mutated
    }
}

// 判断 canonical nonce。
fn hex(value: &str) -> bool {
    // 检查精确长度与小写 hex。
    value.len() == NONCE_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 构造状态错误。
const fn failed() -> BrowserSessionBrokerProtocolFailure {
    // 返回封闭协议错误。
    BrowserSessionBrokerProtocolFailure::new(BrowserSessionBrokerProtocolErrorCode::ProtocolFailed)
}

// 注册同一状态组件的独立测试模块，避免生产状态文件承担测试职责。
#[cfg(test)]
#[path = "browser_session_broker_protocol_state_tests.rs"]
mod tests;
