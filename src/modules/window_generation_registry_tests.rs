//! 验证窗口代际 registry 的屏障、回收、序列缺口与永久 poison。

// 导入被测状态机与强类型事件。
use super::{
    // 导入事件和身份键。
    WindowGenerationEvent,
    WindowGenerationKey,
    // 导入 owner 阶段与 poison 分类。
    WindowGenerationOwnerPhase,
    WindowGenerationPoisonReason,
    // 导入 registry 错误与主体。
    WindowGenerationRegistry,
    WindowGenerationRegistryError,
};

// 固定测试 broker epoch。
const EPOCH_A: &str = "00112233445566778899aabbccddeeff";
// 固定另一 broker epoch。
const EPOCH_B: &str = "ffeeddccbbaa99887766554433221100";

// 构造稳定窗口身份键。
fn key(process_id: u32, token: u64, process_generation: u64) -> WindowGenerationKey {
    // 测试只传入非零事实。
    WindowGenerationKey::new(process_id, token, process_generation)
        // 非零夹具必须可建立。
        .unwrap_or_else(|| panic!("fixture key must be valid"))
}

// 从相同验证快照建立 live registry。
fn live_registry(epoch: &str, snapshot: &[WindowGenerationKey]) -> WindowGenerationRegistry {
    // 启动 bootstrap。
    let mut registry = WindowGenerationRegistry::begin(epoch, snapshot)
        // 合法夹具必须可启动。
        .unwrap_or_else(|error| panic!("registry must begin: {error:?}"));
    // 用零事件序列闭合相同快照。
    registry
        // producer 尚无事件。
        .finish_bootstrap(snapshot, 0)
        // 相同屏障必须完成。
        .unwrap_or_else(|error| panic!("bootstrap must finish: {error:?}"));
    // 返回 live owner。
    registry
}

// 验证 bootstrap 可收敛快照重复 create 与顺序 destroy/create。
#[test]
fn bootstrap_barrier_reconciles_queued_events_before_becoming_live() {
    // 初始快照包含旧 token 与不变窗口。
    let old = key(42, 0x1000, 0x2000);
    // 构造不变窗口。
    let stable = key(43, 0x1001, 0x2001);
    // 启动 bootstrapping registry。
    let mut registry = WindowGenerationRegistry::begin(EPOCH_A, &[old, stable])
        // 合法快照必须成功。
        .unwrap_or_else(|error| panic!("registry must begin: {error:?}"));
    // hook 后、快照前的同事实 create 可以重复。
    registry
        // 序列一仍指向不变窗口。
        .apply_event(WindowGenerationEvent::created(1, stable).unwrap_or_else(|| panic!("event")))
        // bootstrap 重复由第二快照认证。
        .unwrap_or_else(|error| panic!("snapshot duplicate must reconcile: {error:?}"));
    // 旧窗口随后销毁。
    registry
        // 序列二移除旧 token。
        .apply_event(WindowGenerationEvent::destroyed(2, 0x1000).unwrap_or_else(|| panic!("event")))
        // 配对 destroy 必须成功。
        .unwrap_or_else(|error| panic!("destroy must apply: {error:?}"));
    // 完全相同 token 由同进程逻辑新窗口回收。
    let replacement = key(42, 0x1000, 0x2000);
    // 新实例 create 必须分配新 generation。
    registry
        // 序列三恢复同 token。
        .apply_event(
            WindowGenerationEvent::created(3, replacement).unwrap_or_else(|| panic!("event")),
        )
        // 已有 destroy 配对时允许回收。
        .unwrap_or_else(|error| panic!("replacement must apply: {error:?}"));
    // 第二快照必须反映 replacement 与 stable。
    registry
        // producer 已产生三条事件。
        .finish_bootstrap(&[replacement, stable], 3)
        // 完整集合一致时进入 live。
        .unwrap_or_else(|error| panic!("barrier must finish: {error:?}"));
    // 核对 live 阶段。
    assert_eq!(registry.phase(), WindowGenerationOwnerPhase::Live);
    // 核对两条 live 记录。
    assert_eq!(registry.live_window_count(), 2);
}

// 验证 live destroy/recreate 为完全相同三字段签发不同 owner generation。
#[test]
fn exact_same_token_reuse_gets_a_new_generation() {
    // 建立单窗口 live registry。
    let window = key(42, 0x1234, 0x5678);
    // 完成初始屏障。
    let mut registry = live_registry(EPOCH_A, &[window]);
    // 解析旧实例。
    let old = registry
        // producer 尚无事件。
        .resolve(window, 0)
        // owner 必须健康。
        .unwrap_or_else(|error| panic!("old resolve must succeed: {error:?}"))
        // 当前窗口必须命中。
        .unwrap_or_else(|| panic!("old window must resolve"));
    // 销毁旧实例。
    registry
        // 应用序列一。
        .apply_event(WindowGenerationEvent::destroyed(1, 0x1234).unwrap_or_else(|| panic!("event")))
        // 已知 token 必须销毁。
        .unwrap_or_else(|error| panic!("destroy must succeed: {error:?}"));
    // 同进程完全相同 token 创建逻辑新实例。
    registry
        // 应用序列二。
        .apply_event(WindowGenerationEvent::created(2, window).unwrap_or_else(|| panic!("event")))
        // 配对回收必须成功。
        .unwrap_or_else(|error| panic!("recreate must succeed: {error:?}"));
    // 解析新实例。
    let replacement = registry
        // producer 已到序列二。
        .resolve(window, 2)
        // owner 必须健康。
        .unwrap_or_else(|error| panic!("replacement resolve must succeed: {error:?}"))
        // replacement 必须命中。
        .unwrap_or_else(|| panic!("replacement must resolve"));
    // owner generation 必须严格增加。
    assert!(replacement.owner_generation() > old.owner_generation());
    // 私有 opaque 材料必须变化。
    assert_ne!(replacement.private_material(), old.private_material());
}

// 验证重复 create、未知 destroy 与序列缺口永久 poison。
#[test]
fn continuity_failures_poison_the_epoch_permanently() {
    // 建立单窗口 live registry。
    let window = key(42, 0x1234, 0x5678);
    // 完成初始屏障。
    let mut registry = live_registry(EPOCH_A, &[window]);
    // live 重复 create 无法区分漏失 destroy。
    let failure = registry
        // 使用下一连续序列。
        .apply_event(WindowGenerationEvent::created(1, window).unwrap_or_else(|| panic!("event")))
        // 必须失败闭合。
        .expect_err("duplicate create must poison");
    // 核对稳定原因。
    assert_eq!(
        failure,
        // 使用永久 poison 分类。
        WindowGenerationRegistryError::OwnerPoisoned(
            // 固定重复 create 原因。
            WindowGenerationPoisonReason::DuplicateLiveCreate
        )
    );
    // 阶段永久 poison。
    assert_eq!(registry.phase(), WindowGenerationOwnerPhase::Poisoned);
    // 后续 resolve 不得恢复或回退。
    assert_eq!(
        registry.resolve(window, 1),
        // 保留首次 poison 原因。
        Err(WindowGenerationRegistryError::OwnerPoisoned(
            // 不被后续调用覆盖。
            WindowGenerationPoisonReason::DuplicateLiveCreate
        ))
    );
    // 另建 registry 验证 producer 末尾缺口。
    let mut gap = live_registry(EPOCH_A, &[window]);
    // producer 声称已有一条尚未应用事件。
    assert_eq!(
        gap.resolve(window, 1),
        // 缺口必须 poison。
        Err(WindowGenerationRegistryError::OwnerPoisoned(
            // 使用序列缺口原因。
            WindowGenerationPoisonReason::EventSequenceGap
        ))
    );
    // 另建 registry 验证未知 destroy。
    let mut unknown = live_registry(EPOCH_A, &[window]);
    // 销毁未跟踪 token。
    assert_eq!(
        unknown.apply_event(
            // 使用合法序列但未知 token。
            WindowGenerationEvent::destroyed(1, 0x9999).unwrap_or_else(|| panic!("event"))
        ),
        // 未知 destroy 必须 poison。
        Err(WindowGenerationRegistryError::OwnerPoisoned(
            // 固定未知 destroy 原因。
            WindowGenerationPoisonReason::UnknownLiveDestroy
        ))
    );
}

// 验证屏障不一致、显式队列溢出与 broker restart 均失败闭合。
#[test]
fn barrier_overflow_and_restart_bound_identity_lifetime() {
    // 构造初始窗口。
    let window = key(42, 0x1234, 0x5678);
    // 启动但不完成屏障。
    let mut mismatch = WindowGenerationRegistry::begin(EPOCH_A, &[window])
        // 合法快照必须开始。
        .unwrap_or_else(|error| panic!("registry must begin: {error:?}"));
    // 不同验证快照不得覆盖初始事实。
    assert_eq!(
        mismatch.finish_bootstrap(&[], 0),
        // 屏障不一致永久 poison。
        Err(WindowGenerationRegistryError::OwnerPoisoned(
            // 固定 mismatch 原因。
            WindowGenerationPoisonReason::SnapshotBarrierMismatch
        ))
    );
    // 建立健康 registry 后显式报告队列溢出。
    let mut overflow = live_registry(EPOCH_A, &[window]);
    // Adapter 容量故障必须永久 poison。
    assert_eq!(
        overflow.invalidate(WindowGenerationPoisonReason::EventQueueOverflow),
        // 返回相同稳定原因。
        WindowGenerationRegistryError::OwnerPoisoned(
            // 固定队列溢出。
            WindowGenerationPoisonReason::EventQueueOverflow
        )
    );
    // 在第一 epoch 解析身份。
    let mut first = live_registry(EPOCH_A, &[window]);
    // 取得第一私有材料。
    let first_material = first
        // 事件序列为零。
        .resolve(window, 0)
        // owner 必须健康。
        .unwrap_or_else(|error| panic!("first resolve: {error:?}"))
        // 当前窗口必须命中。
        .unwrap_or_else(|| panic!("window must resolve"))
        // 投影私有材料。
        .private_material();
    // 模拟 broker restart 建立新随机 epoch。
    let mut second = live_registry(EPOCH_B, &[window]);
    // 取得第二私有材料。
    let second_material = second
        // 新 epoch 也从零事件开始。
        .resolve(window, 0)
        // owner 必须健康。
        .unwrap_or_else(|error| panic!("second resolve: {error:?}"))
        // 当前窗口必须命中。
        .unwrap_or_else(|| panic!("window must resolve"))
        // 投影私有材料。
        .private_material();
    // restart 必须使全部旧 opaque 材料变化。
    assert_ne!(first_material, second_material);
    // poison 标签稳定且不含原生事实。
    assert_eq!(
        WindowGenerationPoisonReason::EventQueueOverflow.as_str(),
        // 使用固定 kebab-case。
        "event-queue-overflow"
    );
}
