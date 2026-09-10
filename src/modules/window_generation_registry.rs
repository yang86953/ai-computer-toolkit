//! 拥有单一 broker epoch 内窗口 token 到单调 generation 的领域状态机。

// 导入有界 live 记录索引与集合比较。
use std::collections::{HashMap, HashSet};

// 导入强类型 epoch 与固定容量。
use crate::components::window_generation_protocol::{
    // broker epoch 永不进入公共 JSON。
    BrokerEpoch,
    // live registry 使用统一容量。
    MAXIMUM_TRACKED_WINDOWS,
    // 协议候选提供平台无关整数事实。
    WindowGenerationCandidate,
};

// 表示 owner 当前生命周期阶段。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowGenerationOwnerPhase {
    // 已安装事件源并正在闭合初始快照屏障。
    Bootstrapping,
    // 快照与内部事件序列已经对齐，可解析身份。
    Live,
    // 连续性无法证明，当前 epoch 永久失败闭合。
    Poisoned,
}

// 表示当前 epoch 永久失效的封闭原因。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowGenerationPoisonReason {
    // Adapter 报告事件 hook 不可用。
    EventHookUnavailable,
    // 初始或验证快照不可用。
    SnapshotUnavailable,
    // 快照与已应用顺序事件不一致。
    SnapshotBarrierMismatch,
    // 内部单调事件序列缺失、倒退或未追平 producer。
    EventSequenceGap,
    // 固定事件队列溢出。
    EventQueueOverflow,
    // create 事件无法取得完整 PID 与进程代际。
    EventClassificationUnavailable,
    // live 阶段收到仍存活 token 的重复 create。
    DuplicateLiveCreate,
    // live 阶段收到未知 token 的 destroy。
    UnknownLiveDestroy,
    // token 对应的进程事实发生未配对变化。
    IdentityConflict,
    // live registry 达到固定容量。
    RegistryCapacityExhausted,
    // owner generation 或事件序列发生理论溢出。
    CounterExhausted,
}

// 为 poison 原因提供稳定私有诊断标签。
impl WindowGenerationPoisonReason {
    // 返回不可扩展的 kebab-case 标签。
    pub(crate) const fn as_str(self) -> &'static str {
        // 穷举全部连续性失败原因。
        match self {
            // 映射 hook 失败。
            Self::EventHookUnavailable => "event-hook-unavailable",
            // 映射快照失败。
            Self::SnapshotUnavailable => "snapshot-unavailable",
            // 映射屏障不一致。
            Self::SnapshotBarrierMismatch => "snapshot-barrier-mismatch",
            // 映射事件序列缺口。
            Self::EventSequenceGap => "event-sequence-gap",
            // 映射队列溢出。
            Self::EventQueueOverflow => "event-queue-overflow",
            // 映射事件分类失败。
            Self::EventClassificationUnavailable => "event-classification-unavailable",
            // 映射重复 create。
            Self::DuplicateLiveCreate => "duplicate-live-create",
            // 映射未知 destroy。
            Self::UnknownLiveDestroy => "unknown-live-destroy",
            // 映射身份冲突。
            Self::IdentityConflict => "identity-conflict",
            // 映射 registry 容量。
            Self::RegistryCapacityExhausted => "registry-capacity-exhausted",
            // 映射计数器溢出。
            Self::CounterExhausted => "counter-exhausted",
        }
    }
}

// 表示 owner API 返回的封闭领域错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowGenerationRegistryError {
    // broker epoch 不是 canonical。
    InvalidBrokerEpoch,
    // 输入候选缺字段、重复 token 或超出容量。
    InvalidSnapshot,
    // 初始快照屏障尚未完成。
    BootstrapIncomplete,
    // 当前 epoch 已永久 poison。
    OwnerPoisoned(WindowGenerationPoisonReason),
}

// 保存当前窗口 token 对应的完整私有身份键。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct WindowGenerationKey {
    // 保存私有 PID。
    process_id: u32,
    // 保存完整当前窗口 token。
    current_window_token: u64,
    // 保存进程创建 FILETIME。
    process_generation: u64,
}

// 为身份键提供严格构造与只读投影。
impl WindowGenerationKey {
    // 从协议候选建立领域键。
    pub(crate) const fn from_candidate(candidate: WindowGenerationCandidate) -> Self {
        // 复制已验证的三个事实。
        Self {
            // 保存 PID。
            process_id: candidate.process_id(),
            // 保存 token。
            current_window_token: candidate.current_window_token(),
            // 保存进程代际。
            process_generation: candidate.process_generation(),
        }
    }

    // 构造全部非零的领域键。
    pub(crate) const fn new(
        // 接收 PID。
        process_id: u32,
        // 接收 token。
        current_window_token: u64,
        // 接收进程代际。
        process_generation: u64,
    ) -> Option<Self> {
        // 复用协议候选的非零边界。
        match WindowGenerationCandidate::new(
            // 传入 PID。
            process_id,
            // 传入 token。
            current_window_token,
            // 传入进程代际。
            process_generation,
        ) {
            // 有效候选转换为领域键。
            Some(candidate) => Some(Self::from_candidate(candidate)),
            // 缺失事实保持失败闭合。
            None => None,
        }
    }

    // 返回完整当前窗口 token。
    pub(crate) const fn current_window_token(self) -> u64 {
        // 复制私有整数。
        self.current_window_token
    }
}

// 表示 Adapter 交付给 Module 的封闭窗口事件。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowGenerationEventKind {
    // 携带完整新窗口身份事实。
    Created(WindowGenerationKey),
    // 销毁事件只依赖当前 token，避免读取已销毁窗口。
    Destroyed(u64),
}

// 表示带 owner 内部单调序列的窗口事件。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WindowGenerationEvent {
    // 保存 Adapter 为每个回调分配的连续序号。
    sequence: u64,
    // 保存封闭事件事实。
    kind: WindowGenerationEventKind,
}

// 为事件提供全部非零的封闭构造。
impl WindowGenerationEvent {
    // 构造完整 create 事件。
    pub(crate) const fn created(sequence: u64, key: WindowGenerationKey) -> Option<Self> {
        // 序列零保留为尚无事件。
        if sequence == 0 {
            // 拒绝非法序列。
            return None;
        }
        // 返回 create 事件。
        Some(Self {
            // 保存序列。
            sequence,
            // 保存完整身份键。
            kind: WindowGenerationEventKind::Created(key),
        })
    }

    // 构造只携带 token 的 destroy 事件。
    pub(crate) const fn destroyed(sequence: u64, current_window_token: u64) -> Option<Self> {
        // 序列和 token 都必须非零。
        if sequence == 0 || current_window_token == 0 {
            // 拒绝非法事件。
            return None;
        }
        // 返回 destroy 事件。
        Some(Self {
            // 保存序列。
            sequence,
            // 保存销毁 token。
            kind: WindowGenerationEventKind::Destroyed(current_window_token),
        })
    }
}

// 保存 live 窗口对应的 owner generation。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LiveWindowGeneration {
    // 保存完整身份键。
    key: WindowGenerationKey,
    // 保存当前 epoch 内单调 generation。
    owner_generation: u64,
}

// 表示 resolve 成功后仅在私有 IPC 内使用的身份材料。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedWindowGeneration {
    // 保存随机 broker epoch。
    broker_epoch: BrokerEpoch,
    // 保存当前 epoch 内单调 generation。
    owner_generation: u64,
}

// 为解析结果提供私有材料投影。
impl ResolvedWindowGeneration {
    // 返回用于 opaque hash 的私有 owner 材料。
    pub(crate) fn private_material(&self) -> String {
        // epoch 与 generation 均不直接进入公共 JSON。
        format!(
            "{}:{:016x}",
            self.broker_epoch.as_str(),
            self.owner_generation
        )
    }

    // 返回 owner generation 供严格 wire 编码。
    pub(crate) const fn owner_generation(&self) -> u64 {
        // 复制单调整数。
        self.owner_generation
    }
}

// 拥有单一 broker epoch 的快照屏障、事件连续性与 live registry。
pub(crate) struct WindowGenerationRegistry {
    // 保存当前随机 epoch。
    broker_epoch: BrokerEpoch,
    // 保存当前生命周期阶段。
    phase: WindowGenerationOwnerPhase,
    // 保存永久 poison 原因。
    poison_reason: Option<WindowGenerationPoisonReason>,
    // 保存下一个可分配 generation。
    next_owner_generation: u64,
    // 保存最后应用的事件序列。
    last_event_sequence: u64,
    // 以当前 token 唯一索引 live 窗口。
    live_windows: HashMap<u64, LiveWindowGeneration>,
}

// 提供初始屏障、顺序事件、永久 poison 与身份解析。
impl WindowGenerationRegistry {
    // 从 hook 安装后的第一次完整顶层窗口快照开始 bootstrap。
    pub(crate) fn begin(
        // 接收随机 canonical epoch。
        broker_epoch: &str,
        // 接收包含隐藏和无标题窗口的完整快照。
        snapshot: &[WindowGenerationKey],
    ) -> Result<Self, WindowGenerationRegistryError> {
        // 解析 canonical epoch。
        let broker_epoch = BrokerEpoch::parse(broker_epoch)
            // 非 canonical 值不得启动 owner。
            .ok_or(WindowGenerationRegistryError::InvalidBrokerEpoch)?;
        // 初始快照不得超过 live 容量。
        if snapshot.len() > MAXIMUM_TRACKED_WINDOWS {
            // 不截断或驱逐窗口。
            return Err(WindowGenerationRegistryError::InvalidSnapshot);
        }
        // 创建空 bootstrapping registry。
        let mut registry = Self {
            // 保存随机 epoch。
            broker_epoch,
            // 标记屏障未完成。
            phase: WindowGenerationOwnerPhase::Bootstrapping,
            // 尚无 poison。
            poison_reason: None,
            // generation 从一开始，零表示未签发。
            next_owner_generation: 1,
            // 尚未应用事件。
            last_event_sequence: 0,
            // 按固定容量预分配但不超过实际快照。
            live_windows: HashMap::with_capacity(snapshot.len()),
        };
        // 为初始快照逐项签发 generation。
        for key in snapshot {
            // 同一 token 重复表示初始事实歧义。
            if registry
                .live_windows
                .contains_key(&key.current_window_token())
            {
                // 直接拒绝启动而不是任取候选。
                return Err(WindowGenerationRegistryError::InvalidSnapshot);
            }
            // 插入初始 live 记录。
            registry
                // 使用统一单调分配器。
                .insert_new(*key)
                // 理论容量或计数错误仍归为非法初始快照。
                .map_err(|_| WindowGenerationRegistryError::InvalidSnapshot)?;
        }
        // 返回尚不可 resolve 的 registry。
        Ok(registry)
    }

    // 应用 Adapter 已连续编号的单个 create/destroy 事件。
    pub(crate) fn apply_event(
        // 独占当前状态机。
        &mut self,
        // 接收下一条事件。
        event: WindowGenerationEvent,
    ) -> Result<(), WindowGenerationRegistryError> {
        // poison epoch 永不恢复。
        self.ensure_not_poisoned()?;
        // 计算严格下一序列。
        let expected = self
            // 从最后应用位置递增。
            .last_event_sequence
            // 理论溢出使 epoch 失效。
            .checked_add(1)
            // 使用统一 poison helper。
            .ok_or_else(|| self.poison(WindowGenerationPoisonReason::CounterExhausted))?;
        // 序列缺失或倒退都永久失败闭合。
        if event.sequence != expected {
            // 记录稳定 poison 原因。
            return Err(self.poison(WindowGenerationPoisonReason::EventSequenceGap));
        }
        // 先推进序列，后续任一语义失败都不可重放恢复。
        self.last_event_sequence = event.sequence;
        // 按 bootstrap/live 采用不同的边界收敛规则。
        match self.phase {
            // bootstrap 允许快照与排队事件的同事实重复。
            WindowGenerationOwnerPhase::Bootstrapping => self.apply_bootstrap_event(event.kind),
            // live 对任何未配对变化失败闭合。
            WindowGenerationOwnerPhase::Live => self.apply_live_event(event.kind),
            // 前置检查已经排除 poison。
            WindowGenerationOwnerPhase::Poisoned => {
                // 保持穷举完整并返回既有原因。
                self.ensure_not_poisoned()
            }
        }
    }

    // 用第二次完整快照和 producer 序列闭合初始屏障。
    pub(crate) fn finish_bootstrap(
        // 独占当前状态机。
        &mut self,
        // 接收事件追平后的验证快照。
        validation_snapshot: &[WindowGenerationKey],
        // 接收 Adapter 已产生的最后序列。
        producer_sequence: u64,
    ) -> Result<(), WindowGenerationRegistryError> {
        // poison epoch 永不恢复。
        self.ensure_not_poisoned()?;
        // 只允许一次 bootstrap 完成。
        if self.phase != WindowGenerationOwnerPhase::Bootstrapping {
            // live 重复屏障表示调用顺序错误并使 epoch 失效。
            return Err(self.poison(WindowGenerationPoisonReason::SnapshotBarrierMismatch));
        }
        // producer 与已应用序列必须完全追平。
        self.ensure_producer_synchronized(producer_sequence)?;
        // 验证快照自身必须完整且唯一。
        let validation = snapshot_set(validation_snapshot)
            // 非法快照使当前 epoch 失效。
            .map_err(|reason| self.poison(reason))?;
        // 当前 live 记录也转换为完整键集合。
        let current = self
            // 遍历全部 live 记录。
            .live_windows
            // 只比较完整领域键。
            .values()
            // 复制键。
            .map(|record| record.key)
            // 收集无序集合。
            .collect::<HashSet<_>>();
        // 第二快照必须与排队事件收敛结果逐项相等。
        if validation != current {
            // 不以第二快照覆盖 registry。
            return Err(self.poison(WindowGenerationPoisonReason::SnapshotBarrierMismatch));
        }
        // 仅此事实建立点允许身份解析。
        self.phase = WindowGenerationOwnerPhase::Live;
        // 返回屏障完成。
        Ok(())
    }

    // 解析一个当前窗口键到随机 epoch 与单调 generation。
    pub(crate) fn resolve(
        // 独占状态以同步 producer 序列。
        &mut self,
        // 接收当前 inventory 私有事实。
        key: WindowGenerationKey,
        // 接收 Adapter 已产生的最后序列。
        producer_sequence: u64,
    ) -> Result<Option<ResolvedWindowGeneration>, WindowGenerationRegistryError> {
        // poison epoch 永不恢复。
        self.ensure_not_poisoned()?;
        // bootstrap 完成前不得签发身份。
        if self.phase != WindowGenerationOwnerPhase::Live {
            // 返回可重试的内部启动未完成分类。
            return Err(WindowGenerationRegistryError::BootstrapIncomplete);
        }
        // 请求前必须追平事件 producer。
        self.ensure_producer_synchronized(producer_sequence)?;
        // 按当前 token 查找 live 记录。
        let Some(record) = self.live_windows.get(&key.current_window_token()) else {
            // 未命中表示当前候选不可由 owner 认证。
            return Ok(None);
        };
        // PID 或进程代际变化不得复用旧 generation。
        if record.key != key {
            // 作为 stale 候选返回而不修改 registry。
            return Ok(None);
        }
        // 返回私有 owner 材料。
        Ok(Some(ResolvedWindowGeneration {
            // 复制随机 epoch。
            broker_epoch: self.broker_epoch.clone(),
            // 复制当前 generation。
            owner_generation: record.owner_generation,
        }))
    }

    // 由 Adapter 主动报告无法恢复的连续性故障。
    pub(crate) fn invalidate(
        // 独占当前状态机。
        &mut self,
        // 接收封闭原因。
        reason: WindowGenerationPoisonReason,
    ) -> WindowGenerationRegistryError {
        // 使用永久 poison helper。
        self.poison(reason)
    }

    // 返回当前 owner 阶段。
    pub(crate) const fn phase(&self) -> WindowGenerationOwnerPhase {
        // 复制无状态枚举。
        self.phase
    }

    // 返回当前 live 记录数量。
    pub(crate) fn live_window_count(&self) -> usize {
        // HashMap 长度受固定容量约束。
        self.live_windows.len()
    }

    // 返回永久 poison 原因。
    pub(crate) const fn poison_reason(&self) -> Option<WindowGenerationPoisonReason> {
        // 复制可选枚举。
        self.poison_reason
    }

    // 应用 bootstrap 阶段排队事件。
    fn apply_bootstrap_event(
        // 独占 registry。
        &mut self,
        // 接收封闭事件。
        kind: WindowGenerationEventKind,
    ) -> Result<(), WindowGenerationRegistryError> {
        // 分派 create/destroy。
        match kind {
            // create 携带完整新身份。
            WindowGenerationEventKind::Created(key) => {
                // 查找快照可能已包含的同一 token。
                if let Some(existing) = self.live_windows.get(&key.current_window_token()) {
                    // 完全相同表示 hook 后、快照前的同事实重复。
                    if existing.key == key {
                        // 保留初始 generation 并等待第二快照验证。
                        return Ok(());
                    }
                    // 同 token 不同进程事实缺少配对 destroy。
                    return Err(self.poison(WindowGenerationPoisonReason::IdentityConflict));
                }
                // 快照没有该窗口时按顺序 create。
                self.insert_new(key)
            }
            // destroy 只携带 token。
            WindowGenerationEventKind::Destroyed(token) => {
                // 快照可能已在该窗口销毁后完成，因此缺失合法。
                self.live_windows.remove(&token);
                // 最终由第二快照认证。
                Ok(())
            }
        }
    }

    // 应用 live 阶段必须严格配对的事件。
    fn apply_live_event(
        // 独占 registry。
        &mut self,
        // 接收封闭事件。
        kind: WindowGenerationEventKind,
    ) -> Result<(), WindowGenerationRegistryError> {
        // 分派 create/destroy。
        match kind {
            // create 必须使用当前未存活 token。
            WindowGenerationEventKind::Created(key) => {
                // 任一 live token 命中都无法区分重复事件或漏失 destroy。
                if self.live_windows.contains_key(&key.current_window_token()) {
                    // 永久拒绝继续签发。
                    return Err(self.poison(WindowGenerationPoisonReason::DuplicateLiveCreate));
                }
                // 为新实例签发新 generation。
                self.insert_new(key)
            }
            // destroy 必须命中当前 live token。
            WindowGenerationEventKind::Destroyed(token) => {
                // 删除当前实例。
                if self.live_windows.remove(&token).is_none() {
                    // 未知 destroy 表示内部事件连续性不可证明。
                    return Err(self.poison(WindowGenerationPoisonReason::UnknownLiveDestroy));
                }
                // 完成已配对销毁。
                Ok(())
            }
        }
    }

    // 插入一个此前不存活的窗口并分配 generation。
    fn insert_new(
        // 独占 registry。
        &mut self,
        // 接收完整身份键。
        key: WindowGenerationKey,
    ) -> Result<(), WindowGenerationRegistryError> {
        // 固定容量耗尽时不得驱逐旧实例。
        if self.live_windows.len() >= MAXIMUM_TRACKED_WINDOWS {
            // 当前 epoch 永久失败闭合。
            return Err(self.poison(WindowGenerationPoisonReason::RegistryCapacityExhausted));
        }
        // 取得当前 generation。
        let owner_generation = self.next_owner_generation;
        // 零永不签发且 u64 上限不能继续递增。
        self.next_owner_generation = owner_generation
            // 递增供下一个窗口使用。
            .checked_add(1)
            // 理论溢出永久 poison。
            .ok_or_else(|| self.poison(WindowGenerationPoisonReason::CounterExhausted))?;
        // 插入新 live 记录。
        self.live_windows.insert(
            // 使用完整 token 唯一索引。
            key.current_window_token(),
            // 保存键与 generation。
            LiveWindowGeneration {
                // 保存完整键。
                key,
                // 保存单调 generation。
                owner_generation,
            },
        );
        // 返回成功。
        Ok(())
    }

    // 核对 Adapter producer 与 Module 已应用序列完全一致。
    fn ensure_producer_synchronized(
        // 独占状态以便发现缺口时 poison。
        &mut self,
        // 接收 producer 最新序列。
        producer_sequence: u64,
    ) -> Result<(), WindowGenerationRegistryError> {
        // 相等表示不存在排队或漏失事件。
        if producer_sequence == self.last_event_sequence {
            // 保持健康。
            return Ok(());
        }
        // 任意领先、倒退或末尾丢失都永久失败闭合。
        Err(self.poison(WindowGenerationPoisonReason::EventSequenceGap))
    }

    // poison 后返回稳定错误，健康时继续。
    fn ensure_not_poisoned(&self) -> Result<(), WindowGenerationRegistryError> {
        // 读取可选永久原因。
        match self.poison_reason {
            // 保留首次原因。
            Some(reason) => Err(WindowGenerationRegistryError::OwnerPoisoned(reason)),
            // 健康或 bootstrapping 可继续。
            None => Ok(()),
        }
    }

    // 将当前 epoch 永久转换为 poison。
    fn poison(&mut self, reason: WindowGenerationPoisonReason) -> WindowGenerationRegistryError {
        // 只记录首次连续性失败。
        let stable_reason = *self.poison_reason.get_or_insert(reason);
        // 阶段永久变为 poison。
        self.phase = WindowGenerationOwnerPhase::Poisoned;
        // 返回稳定领域错误。
        WindowGenerationRegistryError::OwnerPoisoned(stable_reason)
    }
}

// 把验证快照转换为严格唯一集合。
fn snapshot_set(
    // 接收完整候选快照。
    snapshot: &[WindowGenerationKey],
) -> Result<HashSet<WindowGenerationKey>, WindowGenerationPoisonReason> {
    // 固定容量不允许截断。
    if snapshot.len() > MAXIMUM_TRACKED_WINDOWS {
        // 返回容量 poison。
        return Err(WindowGenerationPoisonReason::RegistryCapacityExhausted);
    }
    // 建立 token 唯一集合。
    let mut tokens = HashSet::with_capacity(snapshot.len());
    // 建立完整键集合。
    let mut keys = HashSet::with_capacity(snapshot.len());
    // 验证每条记录。
    for key in snapshot {
        // token 重复表示快照不可认证。
        if !tokens.insert(key.current_window_token()) {
            // 返回屏障不一致。
            return Err(WindowGenerationPoisonReason::SnapshotBarrierMismatch);
        }
        // 保存完整键。
        keys.insert(*key);
    }
    // 返回完整集合。
    Ok(keys)
}

// 状态机回归保留在独立文件以控制代码行数。
#[cfg(test)]
#[path = "window_generation_registry_tests.rs"]
mod tests;
