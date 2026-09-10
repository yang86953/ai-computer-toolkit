//! 定义当前 broker epoch 内固定容量的执行、重放与取消墓碑账本。

// 导入有序 request 映射。
use std::collections::BTreeMap;

// 导入当前 live broker epoch。
use super::BrowserSessionBrokerEpoch;
// 导入协议响应及其封闭 outcome。
use super::super::response::{BrowserSessionBrokerOutcome, BrowserSessionBrokerResponse};
// 导入协议请求与操作类型。
use super::super::{BrowserSessionBrokerOperation, BrowserSessionBrokerRequest};
// 导入协议失败类型。
use super::super::{BrowserSessionBrokerProtocolErrorCode, BrowserSessionBrokerProtocolFailure};
// 导入 canonical nonce 长度。
use super::super::NONCE_LENGTH;

// 固定一个 live epoch 内的 request 与墓碑总容量。
const EXECUTION_LEDGER_CAPACITY: usize = 4_096;
// 固定全部 response replay reservation 总预算。
const REPLAY_PAYLOAD_BUDGET_BYTES: usize = 64 * 1024 * 1024;
// 固定取消先到 final 的小帧预留。
const TOMBSTONE_RESPONSE_RESERVATION_BYTES: usize = 4 * 1024;
// 固定普通 response 的保守 replay reservation。
const STANDARD_RESPONSE_RESERVATION_BYTES: usize = 128 * 1024;
// 固定 query response 的保守 replay reservation。
const QUERY_RESPONSE_RESERVATION_BYTES: usize = 2 * 1024 * 1024;
// 固定 screenshot response 的 schema 最大帧 reservation。
const SCREENSHOT_RESPONSE_RESERVATION_BYTES: usize = 16 * 1024 * 1024 + 128 * 1024;

// 保存一次由响应 Component 验证且与原 request 关联的快照。
#[derive(Clone, Debug, PartialEq)]
struct BrowserSessionBrokerExecutionSnapshot {
    // 保存不可伪造的封闭响应。
    response: BrowserSessionBrokerResponse,
    // 保存响应 Component 计算的实际 replay 字节数。
    replay_payload_bytes: usize,
}

// 为执行快照提供唯一构造与内部只读投影。
impl BrowserSessionBrokerExecutionSnapshot {
    // 从已验证响应构造与原 request 强关联的快照。
    fn from_response(
        // 借用 ledger 首次保存的严格 request。
        request: &BrowserSessionBrokerRequest,
        // 借用 ledger 首次保存的完整 canonical request 语义键。
        request_semantic_key: &str,
        // 借用当前 live epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 接收响应 Component 的封闭值。
        response: BrowserSessionBrokerResponse,
    ) -> Result<Self, BrowserSessionBrokerProtocolFailure> {
        // nonce、operation 与 epoch 必须逐项关联原 request。
        if response.request_nonce() != request.request_nonce()
            // operation 不得跨请求错配。
            || response.operation() != request.operation()
            // replay 不得跨 broker epoch。
            || response.broker_epoch() != epoch.as_str()
            // response 必须逐字绑定 record 首次保存的完整 request 语义。
            || response.request_semantic_key() != Some(request_semantic_key)
        {
            // 拒绝任何无法关联的伪造快照。
            return Err(protocol_failed());
        }
        // 从封闭响应读取实际 wire replay 大小。
        let replay_payload_bytes = response.replay_payload_bytes();
        // 返回只保存响应事实的快照。
        Ok(Self {
            // 保存响应本体。
            response,
            // 保存已验证的 replay 字节数。
            replay_payload_bytes,
        })
    }

    // 返回关联 request nonce。
    fn request_nonce(&self) -> &str {
        // 委托封闭响应。
        self.response.request_nonce()
    }

    // 返回关联 operation。
    const fn operation(&self) -> BrowserSessionBrokerOperation {
        // 委托封闭响应。
        self.response.operation()
    }

    // 返回单调 request revision。
    const fn request_revision(&self) -> u64 {
        // 委托封闭响应。
        self.response.request_revision()
    }

    // 返回 final-wins 事实。
    const fn terminal(&self) -> bool {
        // 任何有 outcome 的响应都是 wire final。
        self.response.outcome().is_some()
    }

    // 返回权威 cancelled 事实。
    const fn cancelled(&self) -> bool {
        // 取消事实只能从封闭 outcome 推导。
        matches!(
            self.response.outcome(),
            // accepted 后取消和业务前取消都是权威取消终态。
            Some(BrowserSessionBrokerOutcome::Cancelled)
                | Some(BrowserSessionBrokerOutcome::CancelledBeforeAcceptance)
        )
    }

    // 返回可重放响应。
    fn response(&self) -> &BrowserSessionBrokerResponse {
        // 借用封闭响应。
        &self.response
    }

    // 返回实际 replay payload 字节数。
    const fn replay_payload_bytes(&self) -> usize {
        // 复制响应 Component 给出的值。
        self.replay_payload_bytes
    }
}

// 表示 request 到达 execution ledger 后的唯一处理决定。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BrowserSessionBrokerExecutionDecision {
    // 首次 request 可以且只能 dispatch 一次。
    Dispatch {
        // 返回只可缩短的绝对 deadline。
        deadline_ms: u64,
        // 返回首个可观察 snapshot 的 revision。
        request_revision: u64,
    },
    // 同义在途 request 只附着观察。
    Attach {
        // 返回只可缩短的绝对 deadline。
        deadline_ms: u64,
        // 返回当前最高 revision。
        request_revision: u64,
        // 返回可选 accepted 响应。
        response: Option<BrowserSessionBrokerResponse>,
    },
    // 同义 terminal request 或已绑定墓碑只重放 final。
    Replay {
        // 返回 terminal-wins 响应。
        response: BrowserSessionBrokerResponse,
    },
}

// 表示 cancel 在线性化点观察到的 target execution 状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BrowserSessionBrokerCancelTargetDecision {
    // target 尚未到达，已原子安装墓碑。
    UnknownRequest,
    // target 已 transport accepted，但尚未 business accepted。
    CancelledBeforeAcceptance,
    // target 已 business accepted，只能协作请求停止。
    CancellationRequested,
    // target 已有权威 cancelled final。
    Cancelled,
    // target 已有其他 terminal final。
    TooLate,
}

// 保存已出现 request 的不可变语义、预算和最新状态。
#[derive(Clone, Debug, PartialEq)]
struct RequestRecord {
    // 保存首次解析的严格 request。
    request: BrowserSessionBrokerRequest,
    // 保存完整 canonical semantic key。
    semantic_key: String,
    // 保存只可缩短的绝对 deadline。
    deadline_ms: u64,
    // 保存业务接受前已经原子预留的 replay 字节。
    reserved_bytes: usize,
    // 保存已接受请求的协作取消事实。
    cancellation_requested: bool,
    // 保存最新 accepted 或 final snapshot。
    snapshot: Option<BrowserSessionBrokerExecutionSnapshot>,
}

// 表示 execution ledger 中的 request 或未绑定取消墓碑。
#[derive(Clone, Debug, PartialEq)]
enum ExecutionRecord {
    // 保存已经完整 transport accepted 的 request。
    Request(RequestRecord),
    // 保存只知道 target nonce 的取消先到事实。
    CancellationTombstone {
        // 保存 cancelled-before-acceptance revision。
        request_revision: u64,
        // 保存安装墓碑时预留的小 final 字节。
        reserved_bytes: usize,
    },
}

// 保存当前 epoch 的固定 request execution ledger。
#[derive(Debug, Default)]
pub(super) struct BrowserSessionBrokerExecutionLedger {
    // 保存 request 与墓碑，epoch 结束前不淘汰。
    records: BTreeMap<String, ExecutionRecord>,
    // 保存 business acceptance 前已经预留的 replay 总字节。
    reserved_replay_bytes: usize,
}

// 为 execution ledger 提供仅供 EpochState 调用的线性化原语。
impl BrowserSessionBrokerExecutionLedger {
    // 接受 request 并返回唯一处理决定。
    pub(super) fn observe(
        // 可变借用当前 epoch ledger。
        &mut self,
        // 借用当前 broker epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 接收服务器单调时钟毫秒值。
        now_ms: u64,
        // 借用严格 request。
        request: &BrowserSessionBrokerRequest,
    ) -> Result<BrowserSessionBrokerExecutionDecision, BrowserSessionBrokerProtocolFailure> {
        // epoch mismatch 不得写入 ledger。
        if request.expected_broker_epoch() != epoch.as_str() {
            // 返回业务前 stale 拒绝。
            return Err(error(
                BrowserSessionBrokerProtocolErrorCode::StaleBrokerEpoch,
            ));
        }
        // 计算本次候选绝对 deadline。
        let requested_deadline = now_ms
            // 加上 client 给出的剩余预算。
            .checked_add(u64::from(request.remaining_timeout_ms()))
            // 溢出必须失败闭合。
            .ok_or_else(protocol_failed)?;
        // 构造完整 canonical key。
        let semantic_key = request.canonical_semantic_key();
        // 未绑定墓碑在首个迟到 request 到达时原子绑定语义并缓存 final。
        if let Some(ExecutionRecord::CancellationTombstone {
            // 复制墓碑 revision。
            request_revision,
            // 复制墓碑 reservation。
            reserved_bytes,
        }) = self.records.get(request.request_nonce()).cloned()
        {
            // 由响应 Component 构造业务前取消 final。
            let response = BrowserSessionBrokerResponse::cancelled_before_acceptance(
                // 绑定本次首次迟到 request。
                request,
                // 使用墓碑 revision。
                request_revision,
                // 回显当前 live epoch。
                epoch,
            );
            // 只允许从封闭响应建立快照。
            let snapshot = BrowserSessionBrokerExecutionSnapshot::from_response(
                // 绑定本次首次迟到 request。
                request,
                // 绑定即将写入 record 的完整 canonical 语义键。
                &semantic_key,
                // 绑定当前 live epoch。
                epoch,
                // 移入封闭响应。
                response,
            )?;
            // 小 final 必须落在墓碑预留内。
            if snapshot.replay_payload_bytes() > reserved_bytes {
                // 预留算法与响应上界漂移时失败闭合。
                return Err(protocol_failed());
            }
            // 保存将被 replay 的封闭响应。
            let replay = snapshot.response().clone();
            // 将墓碑原子替换为已经绑定完整语义的 terminal record。
            self.records.insert(
                // 仍使用相同 target nonce。
                request.request_nonce().to_owned(),
                // 保存绑定后的 request record。
                ExecutionRecord::Request(RequestRecord {
                    // 保存首次迟到 request。
                    request: request.clone(),
                    // 保存完整语义键以拒绝异义重送。
                    semantic_key,
                    // 保存迟到 frame 的确定 deadline。
                    deadline_ms: requested_deadline,
                    // 继续占用墓碑的小 final reservation。
                    reserved_bytes,
                    // 业务前取消不曾建立协作取消请求。
                    cancellation_requested: false,
                    // 缓存 cancelled-before-acceptance final。
                    snapshot: Some(snapshot),
                }),
            );
            // 首个迟到 request 只获得 replay，绝不 dispatch。
            return Ok(BrowserSessionBrokerExecutionDecision::Replay {
                // 返回缓存 final。
                response: replay,
            });
        }
        // 已见 request 只能 attach 或 replay。
        if let Some(ExecutionRecord::Request(record)) =
            self.records.get_mut(request.request_nonce())
        {
            // 相同 nonce 必须逐字同义。
            if record.semantic_key != semantic_key {
                // 拒绝同 nonce 的第二项工作。
                return Err(error(
                    BrowserSessionBrokerProtocolErrorCode::NonceSemanticConflict,
                ));
            }
            // 重送只能保持或缩短 deadline。
            record.deadline_ms = record.deadline_ms.min(requested_deadline);
            // terminal snapshot 永远只重放。
            if let Some(snapshot) = record
                .snapshot
                .as_ref()
                .filter(|snapshot| snapshot.terminal())
            {
                // 返回拥有型 final 副本。
                return Ok(BrowserSessionBrokerExecutionDecision::Replay {
                    // 复制 terminal 响应。
                    response: snapshot.response().clone(),
                });
            }
            // 读取当前最高 revision。
            let request_revision = record
                // 借用可选 snapshot。
                .snapshot
                // 投影 revision。
                .as_ref()
                // 首个待发 snapshot 从零开始。
                .map_or(0, BrowserSessionBrokerExecutionSnapshot::request_revision);
            // 复制可选 accepted 响应。
            let response = record
                // 借用可选 snapshot。
                .snapshot
                // 借用内部值。
                .as_ref()
                // 克隆封闭 response。
                .map(|snapshot| snapshot.response().clone());
            // 在途重送只附着到现有 execution。
            return Ok(BrowserSessionBrokerExecutionDecision::Attach {
                // 返回当前 deadline。
                deadline_ms: record.deadline_ms,
                // 返回当前最高 revision。
                request_revision,
                // 返回可选 accepted 响应。
                response,
            });
        }
        // 按 operation 最大 response 上限预留 replay 字节。
        let reserved_bytes = reservation_for_operation(request.operation());
        // 新 nonce 必须原子取得 entry 与 replay reservation。
        self.reserve_new_record(reserved_bytes)?;
        // 建立首次 transport-accepted request record。
        self.records.insert(
            // 使用 canonical request nonce 作键。
            request.request_nonce().to_owned(),
            // 保存 request record。
            ExecutionRecord::Request(RequestRecord {
                // 保存严格 request 供后续响应关联。
                request: request.clone(),
                // 保存完整语义键。
                semantic_key,
                // 保存首次绝对 deadline。
                deadline_ms: requested_deadline,
                // 保存预留字节。
                reserved_bytes,
                // 首次 transport 接受时尚未请求协作取消。
                cancellation_requested: false,
                // business 尚未 accepted。
                snapshot: None,
            }),
        );
        // 返回唯一 dispatch 决定。
        Ok(BrowserSessionBrokerExecutionDecision::Dispatch {
            // 返回 deadline。
            deadline_ms: requested_deadline,
            // 首个 wire snapshot revision 为零。
            request_revision: 0,
        })
    }

    // 只读观察已经由物理 request frame 建立的 execution record。
    pub(super) fn observe_existing(
        // 借用当前 epoch ledger。
        &self,
        // 借用当前 broker epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 借用与首次 frame 完整同义的严格 request。
        request: &BrowserSessionBrokerRequest,
    ) -> Result<BrowserSessionBrokerExecutionDecision, BrowserSessionBrokerProtocolFailure> {
        // epoch mismatch 不得读取或认领旧代际 record。
        if request.expected_broker_epoch() != epoch.as_str() {
            // 返回业务前 stale 拒绝。
            return Err(error(
                BrowserSessionBrokerProtocolErrorCode::StaleBrokerEpoch,
            ));
        }
        // 只有首次物理观察才能把 tombstone 或新 nonce 建成 request record。
        let Some(ExecutionRecord::Request(record)) = self.records.get(request.request_nonce())
        else {
            // dispatcher 或同连接重观察缺失 record 表示内部状态失信。
            return Err(protocol_failed());
        };
        // 内部重观察仍必须逐字核对完整 canonical 语义。
        if record.semantic_key != request.canonical_semantic_key() {
            // 相同 nonce 的异义 request 不能认领已有 execution。
            return Err(error(
                BrowserSessionBrokerProtocolErrorCode::NonceSemanticConflict,
            ));
        }
        // terminal snapshot 永远只重放。
        if let Some(snapshot) = record
            .snapshot
            .as_ref()
            .filter(|snapshot| snapshot.terminal())
        {
            // 返回拥有型 final 副本。
            return Ok(BrowserSessionBrokerExecutionDecision::Replay {
                // 复制 terminal 响应。
                response: snapshot.response().clone(),
            });
        }
        // 读取当前最高 revision。
        let request_revision = record
            // 借用可选 snapshot。
            .snapshot
            // 首个待发 snapshot 从零开始。
            .as_ref()
            // 投影 revision。
            .map_or(0, BrowserSessionBrokerExecutionSnapshot::request_revision);
        // 复制可选 accepted 响应。
        let response = record
            // 借用可选 snapshot。
            .snapshot
            // 克隆封闭 response。
            .as_ref()
            // 不把 ledger 内部借用带出线性化边界。
            .map(|snapshot| snapshot.response().clone());
        // 已有但未终结的 execution 只能 Attach。
        Ok(BrowserSessionBrokerExecutionDecision::Attach {
            // 返回首次 frame 建立且重送只可缩短的 deadline。
            deadline_ms: record.deadline_ms,
            // 返回当前最高 revision。
            request_revision,
            // 返回可选 accepted snapshot。
            response,
        })
    }

    // 应用与 ledger request 关联的封闭响应。
    pub(super) fn apply_response(
        // 可变借用 ledger。
        &mut self,
        // 借用当前 live epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 接收响应 Component 已验证的值。
        response: BrowserSessionBrokerResponse,
    ) -> Result<Option<BrowserSessionBrokerOutcome>, BrowserSessionBrokerProtocolFailure> {
        // 先复制关联 nonce以完成可变查找。
        let request_nonce = response.request_nonce().to_owned();
        // 查找已经 transport accepted 的 request record。
        let Some(ExecutionRecord::Request(record)) = self.records.get_mut(&request_nonce) else {
            // 未绑定墓碑或缺失 record 都不能接收外部响应。
            return Err(protocol_failed());
        };
        // 只允许响应 Component 与保存的原 request 构造快照。
        let snapshot = BrowserSessionBrokerExecutionSnapshot::from_response(
            // 使用 ledger 内不可变 request。
            &record.request,
            // 使用首次 request 保存的完整 canonical 语义键。
            &record.semantic_key,
            // 绑定当前 epoch。
            epoch,
            // 移入封闭响应。
            response,
        )?;
        // operation 必须与首次 request 完全一致。
        if snapshot.operation() != record.request.operation()
            // nonce 也必须逐字一致。
            || snapshot.request_nonce() != record.request.request_nonce()
        {
            // 拒绝跨 request 响应。
            return Err(protocol_failed());
        }
        // final 是 terminal-wins，绝不允许覆盖。
        if record
            .snapshot
            .as_ref()
            .is_some_and(|current| current.terminal())
        {
            // 拒绝 final 后的 accepted 或 final。
            return Err(protocol_failed());
        }
        // 首个 snapshot 只能是 accepted 或业务前 final；accepted 后只能进入业务 final。
        let valid_transition = match record.snapshot.as_ref() {
            // 尚无 snapshot 时，允许 accepted 或业务未接受的确定 final。
            None => {
                // accepted 必须表示业务已接受且尚非 terminal。
                (!snapshot.terminal() && snapshot.response().business_accepted())
                    // pre-acceptance final 必须表示业务尚未接受且已经 terminal。
                    || (snapshot.terminal() && !snapshot.response().business_accepted())
            }
            // 已有非终态只可能是 accepted，下一步必须是业务已接受的 final。
            Some(_) => {
                // 拒绝重复 accepted 或 accepted 后倒退到业务前 final。
                snapshot.terminal() && snapshot.response().business_accepted()
            }
        };
        // 拒绝跳过 accepted 的业务 final 或任何阶段倒退。
        if !valid_transition {
            // 返回协议污染。
            return Err(protocol_failed());
        }
        // 首次 snapshot 必须为 revision 零，后续严格加一。
        let expected_revision = record
            // 借用最新 snapshot。
            .snapshot
            // 投影当前 revision。
            .as_ref()
            // 安全加一。
            .map_or(Some(0), |current| current.request_revision().checked_add(1))
            // u64 溢出必须失败闭合。
            .ok_or_else(protocol_failed)?;
        // 拒绝低 revision、跳号和同 revision 不同内容。
        if snapshot.request_revision() != expected_revision {
            // 返回协议污染。
            return Err(protocol_failed());
        }
        // accepted 后 snapshot 只能使用业务前预留空间。
        if snapshot.replay_payload_bytes() > record.reserved_bytes {
            // 输出超限不能伪造成可信 final。
            return Err(protocol_failed());
        }
        // 仅提取刚建立的业务终态供同一线性化点结算取消回执。
        let business_terminal = if snapshot.terminal() && snapshot.response().business_accepted() {
            // 业务终态必须携带封闭 outcome。
            Some(snapshot.response().outcome().ok_or_else(protocol_failed)?)
        } else {
            // accepted 与业务前终态不驱动取消回执收敛。
            None
        };
        // 保存最新 accepted 或 final snapshot。
        record.snapshot = Some(snapshot);
        // 返回成功。
        Ok(business_terminal)
    }

    // 在线性化点查询或推进 cancel target。
    pub(super) fn cancel_target(
        // 可变借用 execution ledger。
        &mut self,
        // 借用当前 live epoch。
        epoch: &BrowserSessionBrokerEpoch,
        // 借用原 request nonce。
        request_nonce: &str,
    ) -> Result<BrowserSessionBrokerCancelTargetDecision, BrowserSessionBrokerProtocolFailure> {
        // 只接受 canonical target nonce。
        if !canonical_nonce(request_nonce) {
            // 拒绝无法关联的取消目标。
            return Err(error(
                BrowserSessionBrokerProtocolErrorCode::InvalidArgument,
            ));
        }
        // 已有未绑定墓碑保持 unknown-request terminal 事实。
        if matches!(
            self.records.get(request_nonce),
            // 同一 target 不重复安装墓碑。
            Some(ExecutionRecord::CancellationTombstone { .. })
        ) {
            // 不改变已有墓碑。
            return Ok(BrowserSessionBrokerCancelTargetDecision::UnknownRequest);
        }
        // 读取现有 request 的当前 snapshot 状态。
        if let Some(ExecutionRecord::Request(record)) = self.records.get_mut(request_nonce) {
            // business 尚未 accepted 时直接缓存可信业务前取消 final。
            if record.snapshot.is_none() {
                // 由保存的原 request 构造取消 final。
                let response = BrowserSessionBrokerResponse::cancelled_before_acceptance(
                    // 绑定首次 request。
                    &record.request,
                    // 首个 final revision 为零。
                    0,
                    // 回显当前 epoch。
                    epoch,
                );
                // 构造不可伪造的 snapshot。
                let snapshot = BrowserSessionBrokerExecutionSnapshot::from_response(
                    // 使用 ledger 内 request。
                    &record.request,
                    // 使用首次 request 保存的完整 canonical 语义键。
                    &record.semantic_key,
                    // 绑定当前 epoch。
                    epoch,
                    // 移入封闭响应。
                    response,
                )?;
                // 该 final 必须落在 operation 预留内。
                if snapshot.replay_payload_bytes() > record.reserved_bytes {
                    // 预留漂移必须失败闭合。
                    return Err(protocol_failed());
                }
                // 缓存 terminal final 供同义重送 replay。
                record.snapshot = Some(snapshot);
                // 返回已取消业务前请求。
                return Ok(BrowserSessionBrokerCancelTargetDecision::CancelledBeforeAcceptance);
            }
            // 借用已有 accepted 或 final snapshot。
            let snapshot = record.snapshot.as_ref().ok_or_else(protocol_failed)?;
            // 在途 accepted 只能协作请求停止。
            if !snapshot.terminal() {
                // 持久记录已接受 target 的协作取消事实。
                record.cancellation_requested = true;
                // 返回 cancellation-requested。
                return Ok(BrowserSessionBrokerCancelTargetDecision::CancellationRequested);
            }
            // 已取消 final 可权威回放 cancelled。
            if snapshot.cancelled() {
                // 返回 cancelled。
                return Ok(BrowserSessionBrokerCancelTargetDecision::Cancelled);
            }
            // 其他 final 已经太晚。
            return Ok(BrowserSessionBrokerCancelTargetDecision::TooLate);
        }
        // 未知 target 必须先原子预留墓碑 entry。
        self.reserve_new_record(TOMBSTONE_RESPONSE_RESERVATION_BYTES)?;
        // 安装只按 target nonce 生效的未绑定 cancellation tombstone。
        self.records.insert(
            // 使用 target nonce 作键。
            request_nonce.to_owned(),
            // 保存未知 request 墓碑。
            ExecutionRecord::CancellationTombstone {
                // 后到 request 的 final revision 固定为零。
                request_revision: 0,
                // 保存固定小终态 reservation。
                reserved_bytes: TOMBSTONE_RESPONSE_RESERVATION_BYTES,
            },
        );
        // 返回 unknown-request receipt 事实。
        Ok(BrowserSessionBrokerCancelTargetDecision::UnknownRequest)
    }

    // 返回已接受 request 的协作取消事实。
    pub(super) fn cancellation_requested(
        // 借用 execution ledger。
        &self,
        // 借用目标 request nonce。
        request_nonce: &str,
    ) -> bool {
        // 仅已绑定 request 可保存该事实。
        matches!(
            self.records.get(request_nonce),
            // 读取持久请求记录。
            Some(ExecutionRecord::Request(RequestRecord {
                // 匹配已建立的协作取消事实。
                cancellation_requested: true,
                // 忽略其余私有字段。
                ..
            }))
        )
    }

    // 返回已绑定 request 当前只可缩短的绝对 deadline。
    pub(super) fn deadline_ms(
        // 借用 execution ledger。
        &self,
        // 借用目标 request nonce。
        request_nonce: &str,
    ) -> Option<u64> {
        // 只有已绑定完整 request 的 record 才具有 deadline。
        match self.records.get(request_nonce) {
            // 返回当前 min-only deadline。
            Some(ExecutionRecord::Request(record)) => Some(record.deadline_ms),
            // 未绑定 cancel tombstone 与缺失 nonce 都没有 request deadline。
            _ => None,
        }
    }

    // 返回当前 retained entry 数量。
    pub(super) fn record_count(&self) -> usize {
        // 投影固定容量占用。
        self.records.len()
    }

    // 返回当前 replay reservation 总字节。
    pub(super) const fn reserved_replay_bytes(&self) -> usize {
        // 投影不可回收预算占用。
        self.reserved_replay_bytes
    }

    // 返回测试所需的只读缓存响应。
    #[cfg(test)]
    pub(super) fn response_for_test(
        &self,
        request_nonce: &str,
    ) -> Option<&BrowserSessionBrokerResponse> {
        // 只从已经绑定的 request record 读取。
        let Some(ExecutionRecord::Request(record)) = self.records.get(request_nonce) else {
            // 未绑定墓碑没有可重放响应。
            return None;
        };
        // 投影可选 snapshot 中的封闭响应。
        record
            .snapshot
            .as_ref()
            .map(BrowserSessionBrokerExecutionSnapshot::response)
    }

    // 为一个新 entry 原子预留容量和 replay bytes。
    fn reserve_new_record(
        // 可变借用 ledger。
        &mut self,
        // 接收 operation 最大 response reservation。
        reserved_bytes: usize,
    ) -> Result<(), BrowserSessionBrokerProtocolFailure> {
        // entry 容量满时不淘汰旧状态。
        if self.records.len() >= EXECUTION_LEDGER_CAPACITY {
            // 返回结构化 ledger full。
            return Err(error(
                BrowserSessionBrokerProtocolErrorCode::BrokerRequestLedgerFull,
            ));
        }
        // 计算预留后的总预算。
        let total = self
            // 读取当前 reservation。
            .reserved_replay_bytes
            // 原子加上新 reservation。
            .checked_add(reserved_bytes)
            // 溢出等价预算不足。
            .ok_or_else(|| error(BrowserSessionBrokerProtocolErrorCode::BrokerRequestLedgerFull))?;
        // 总 reservation 不得超过 64 MiB。
        if total > REPLAY_PAYLOAD_BUDGET_BYTES {
            // 在 business acceptance 前拒绝。
            return Err(error(
                BrowserSessionBrokerProtocolErrorCode::BrokerRequestLedgerFull,
            ));
        }
        // 提交 reservation。
        self.reserved_replay_bytes = total;
        // 返回成功。
        Ok(())
    }
}

// 按 operation 返回业务接受前必须预留的最大 response 字节。
const fn reservation_for_operation(operation: BrowserSessionBrokerOperation) -> usize {
    // 选择封闭 operation 的预算。
    match operation {
        // screenshot 可携带 16 MiB base64 与 envelope。
        BrowserSessionBrokerOperation::Screenshot => SCREENSHOT_RESPONSE_RESERVATION_BYTES,
        // query 最多携带 100 个有界元素投影。
        BrowserSessionBrokerOperation::Query => QUERY_RESPONSE_RESERVATION_BYTES,
        // 其余响应均使用固定小 envelope reservation。
        _ => STANDARD_RESPONSE_RESERVATION_BYTES,
    }
}

// 判断 target nonce 是否是 canonical 小写十六进制。
fn canonical_nonce(value: &str) -> bool {
    // 检查精确长度与字符集。
    value.len() == NONCE_LENGTH
        // 逐字节验证小写十六进制。
        && value
            // 取得字节迭代器。
            .bytes()
            // 只接受数字或 a-f。
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

// 构造指定类别的 ledger 失败。
const fn error(
    // 接收稳定错误码。
    code: BrowserSessionBrokerProtocolErrorCode,
) -> BrowserSessionBrokerProtocolFailure {
    // 返回不泄漏 payload 的失败。
    BrowserSessionBrokerProtocolFailure::new(code)
}

// 构造通用状态机失败。
const fn protocol_failed() -> BrowserSessionBrokerProtocolFailure {
    // 返回封闭协议错误。
    error(BrowserSessionBrokerProtocolErrorCode::ProtocolFailed)
}
