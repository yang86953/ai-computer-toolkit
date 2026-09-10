//! 验证 browser-session broker 运行时的线性化与无丢失等待语义。

// 导入共享所有权与有界时间控制。
use std::{
    // 在测试中共享运行时。
    sync::Arc,
    // 等待已缩短 deadline 在真实单调时钟上到达。
    thread,
    // 使用极短固定等待。
    time::Duration,
};

// 导入 request builder、响应与执行决定。
use super::super::browser_session_broker_protocol::{
    // 导入完成 outcome 与成功数据。
    response::{
        BrowserSessionBrokerOutcome, BrowserSessionBrokerResponse, BrowserSessionBrokerSuccess,
    },
    // 导入状态机决定与 epoch。
    state::{
        // 导入 cancel receipt 状态。
        BrowserSessionBrokerCancelStatus,
        // 导入 live epoch 与 execution 决定。
        BrowserSessionBrokerEpoch,
        BrowserSessionBrokerExecutionDecision,
    },
    // 导入严格 cancel builder。
    wire::BrowserSessionBrokerCancelFrame,
    // 导入严格 open builder。
    wire::BrowserSessionBrokerRequestFrame,
};

// 导入被测运行时。
use super::{BrowserSessionBrokerRuntime, BrowserSessionBrokerRuntimeWait};

// 固定测试 epoch。
const EPOCH: &str = "11111111111111111111111111111111";
// 固定测试 request nonce。
const NONCE: &str = "22222222222222222222222222222222";

// 构造严格 open request frame。
fn open_frame() -> BrowserSessionBrokerRequestFrame {
    // builder 输出必须通过生产 parser。
    BrowserSessionBrokerRequestFrame::open_confirmed(
        // 使用固定 request nonce。
        NONCE, // 绑定固定 epoch。
        EPOCH, // 使用十秒剩余预算。
        10_000,
    )
    // 合成常量应始终合法。
    .unwrap_or_else(|error| panic!("open frame must parse: {error}"))
}

// 构造已经开始接入的运行时。
fn runtime() -> BrowserSessionBrokerRuntime {
    // 构造 canonical epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())
        // 固定常量必须合法。
        .unwrap_or_else(|error| panic!("epoch must parse: {error}"));
    // 建立空运行时。
    let runtime = BrowserSessionBrokerRuntime::new(epoch);
    // 模拟 endpoint 已发布。
    runtime
        // 打开接入门禁。
        .start_accepting()
        // 空状态不得失败。
        .unwrap_or_else(|error| panic!("runtime must start: {error}"));
    // 返回可观察运行时。
    runtime
}

// 验证同义 attach 不会重新 dispatch 或延长首次 deadline。
#[test]
fn repeated_observe_attaches_without_redispatch_or_deadline_extension() {
    // 建立运行时。
    let runtime = runtime();
    // 构造唯一严格 request。
    let frame = open_frame();
    // 第一次观察必须 dispatch。
    let first = runtime
        // 固定首次服务器时刻。
        .observe_request_at(1_000, frame.request())
        // 合法请求必须进入 ledger。
        .unwrap_or_else(|error| panic!("first observe must succeed: {error}"));
    // 保存首次绝对 deadline。
    let first_deadline = match first.decision() {
        // 首次只能 dispatch。
        BrowserSessionBrokerExecutionDecision::Dispatch { deadline_ms, .. } => *deadline_ms,
        // 其余决定表示重复派发门禁失效。
        decision => panic!("first observe must dispatch: {decision:?}"),
    };
    // 在更晚时刻重送相同 frame。
    let repeated = runtime
        // 固定更晚服务器时刻。
        .observe_request_at(2_000, frame.request())
        // 同义重送必须可恢复。
        .unwrap_or_else(|error| panic!("repeat observe must succeed: {error}"));
    // 断言重送只附着且保留更早 deadline。
    match repeated.decision() {
        // 未接受时 attach 不携带 snapshot。
        BrowserSessionBrokerExecutionDecision::Attach {
            // 读取不会延长的 deadline。
            deadline_ms,
            // 读取当前响应。
            response,
            // 忽略固定 revision。
            ..
        } => {
            // 绝对 deadline 必须保持首次值。
            assert_eq!(*deadline_ms, first_deadline);
            // 业务尚未接受时不得伪造 snapshot。
            assert!(response.is_none());
        }
        // 任何第二次 Dispatch 都会导致重复执行。
        decision => panic!("repeat observe must attach: {decision:?}"),
    }
}

// 验证首次 execution deadline 从完整 frame 到达时刻建立而非解析结束时重置。
#[test]
fn delayed_parse_cannot_reset_received_frame_deadline() {
    // 建立已经发布 endpoint 的运行时。
    let runtime = runtime();
    // 在模拟解析工作前记录完整 frame 到达时刻。
    let received_at = std::time::Instant::now();
    // 构造只剩五毫秒预算的严格 open request。
    let frame = BrowserSessionBrokerRequestFrame::open_confirmed(
        // 使用固定 request nonce。
        NONCE, // 绑定固定 epoch。
        EPOCH, // 冻结极短剩余预算。
        5,
    )
    // 固定常量必须通过生产 parser。
    .unwrap_or_else(|error| panic!("short open frame must parse: {error}"));
    // 模拟严格解析、operation gate 与连接状态处理已经消耗预算。
    thread::sleep(Duration::from_millis(20));
    // 首次观察必须显式携带物理 frame 到达时刻。
    let observed = runtime
        // 建立不可重置 ledger entry。
        .observe_received_request(received_at, frame.request())
        // 已过期 request 仍先得到唯一 Dispatch record，随后由 dispatcher 收敛 final。
        .unwrap_or_else(|error| panic!("received request must enter ledger: {error}"));
    // 读取首次 deadline。
    let deadline_ms = match observed.decision() {
        // 新 nonce 只能取得一次 Dispatch。
        BrowserSessionBrokerExecutionDecision::Dispatch { deadline_ms, .. } => *deadline_ms,
        // 其他决定表示首次观察被错误重派或附着。
        decision => panic!("received request must dispatch once: {decision:?}"),
    };
    // 解析耗时已超过五毫秒，故 ledger deadline 此刻必须已经到达。
    assert!(deadline_ms <= runtime.now_ms());
    // dispatcher 预检前必须直接建立业务前 expired final。
    assert_eq!(
        // 观察冻结的首次 deadline。
        runtime
            // 不得重新按当前时刻加五毫秒。
            .prepare_dispatch(frame.request())
            // 纯状态推进必须成功。
            .unwrap_or_else(|error| panic!("expired request must settle: {error}")),
        // 已过期 request 不得进入 System 预检。
        super::BrowserSessionBrokerDispatchPreparation::Final,
    );
}

// 验证 accepted 与 final 依次唤醒 attach 并最终只 replay。
#[test]
fn accepted_then_final_becomes_terminal_replay() {
    // 使用共享运行时模拟 dispatcher 与连接线程。
    let runtime = Arc::new(runtime());
    // 构造严格 request。
    let frame = open_frame();
    // 首次观察创建 execution record。
    let first = runtime
        // 使用显式时刻便于稳定断言。
        .observe_request_at(0, frame.request())
        // 首次观察必须成功。
        .unwrap_or_else(|error| panic!("dispatch observe must succeed: {error}"));
    // 构造 revision 零 accepted。
    let accepted = BrowserSessionBrokerResponse::accepted(
        // 绑定同一 request。
        frame.request(),
        // accepted 固定 revision 零。
        0,
        // 绑定 runtime epoch。
        runtime.epoch(),
    );
    // 保存 accepted 并唤醒等待者。
    runtime
        // 应用封闭响应。
        .apply_response(accepted)
        // 合法转换不得失败。
        .unwrap_or_else(|error| panic!("accepted must apply: {error}"));
    // 首次代际必须已经变化。
    assert_eq!(
        // 使用不会阻塞的已变化代际等待。
        runtime.wait_for_change(first.generation(), 10_000),
        // 预期立即观察变化。
        Ok(BrowserSessionBrokerRuntimeWait::Changed)
    );
    // 构造 open completed 数据。
    let success = BrowserSessionBrokerSuccess::Open {
        // 使用 canonical session identity。
        session_id: "s2:bs:33333333333333333333333333333333".to_owned(),
    };
    // 构造 revision 一业务终态。
    let final_response = BrowserSessionBrokerResponse::finished(
        // 绑定原 request。
        frame.request(),
        // accepted 后严格加一。
        1,
        // 绑定 runtime epoch。
        runtime.epoch(),
        // 使用完成终态。
        BrowserSessionBrokerOutcome::Completed,
        // open 必须携带 session data。
        Some(success),
    )
    // 合法 open final 必须可构造。
    .unwrap_or_else(|| panic!("open final must be valid"));
    // 保存业务终态。
    runtime
        // 应用 revision 一 final。
        .apply_response(final_response)
        // 终态转换不得失败。
        .unwrap_or_else(|error| panic!("final must apply: {error}"));
    // 同义重送只能 replay final。
    let replay = runtime
        // 更晚观察不得重派。
        .observe_request_at(5_000, frame.request())
        // terminal 重送必须成功。
        .unwrap_or_else(|error| panic!("replay observe must succeed: {error}"));
    // 验证 terminal replay outcome。
    match replay.decision() {
        // 只允许 replay。
        BrowserSessionBrokerExecutionDecision::Replay { response } => {
            // 重放必须保留 completed。
            assert_eq!(
                response.outcome(),
                Some(BrowserSessionBrokerOutcome::Completed)
            );
            // 重放必须保留 revision 一。
            assert_eq!(response.request_revision(), 1);
        }
        // 任何 attach/dispatch 都违反 terminal-wins。
        decision => panic!("terminal request must replay: {decision:?}"),
    }
}

// 验证同一 request 的 accepted snapshot 只授予一次 System 执行。
#[test]
fn accepted_commit_grants_execution_exactly_once() {
    // 建立运行时。
    let runtime = runtime();
    // 构造严格 open request。
    let frame = open_frame();
    // 首次观察取得唯一 Dispatch。
    runtime
        // 固定服务器时刻。
        .observe_request_at(0, frame.request())
        // 创建 execution record。
        .unwrap_or_else(|error| panic!("request must dispatch: {error}"));
    // dispatcher 预检前只能看到未接受 Attach。
    runtime
        // 验证 cancel/deadline/replay 状态。
        .prepare_dispatch(frame.request())
        // 请求仍应可预检。
        .unwrap_or_else(|error| panic!("dispatch must prepare: {error}"));
    // 构造 revision 零 accepted 候选。
    let accepted = BrowserSessionBrokerResponse::accepted(
        // 绑定原 request。
        frame.request(),
        // accepted 固定 revision 零。
        0,
        // 绑定 live epoch。
        runtime.epoch(),
    );
    // 第一次提交建立唯一 accepted。
    let first = runtime
        // 原子提交 accepted。
        .commit_dispatch_snapshot(frame.request(), accepted.clone())
        // 合法首次提交不得失败。
        .unwrap_or_else(|error| panic!("first accepted must commit: {error}"));
    // 首次 accepted 恰好授予一次执行。
    assert!(first.should_execute());
    // 模拟理论重复 dispatcher 再提交相同 accepted。
    let repeated = runtime
        // 重复提交只能读取已有 snapshot。
        .commit_dispatch_snapshot(frame.request(), accepted)
        // 同义恢复不得破坏 ledger。
        .unwrap_or_else(|error| panic!("repeat accepted must recover: {error}"));
    // 已有 accepted 不得授予第二次执行。
    assert!(!repeated.should_execute());
    // 两次观察都指向同一 revision 零 snapshot。
    assert_eq!(repeated.response().request_revision(), 0);
}

// 验证 accepted 后同 nonce 同义重送缩短 deadline 会唤醒旧 waiter 并请求 execution 停止。
#[test]
fn accepted_retry_shortens_live_deadline_and_wakes_waiter() {
    // 建立共享运行时。
    let runtime = runtime();
    // 构造首次十秒预算的严格 request。
    let frame = open_frame();
    // 首次观察建立 execution record。
    let _ = runtime
        // 使用服务器时刻零便于断言绝对 deadline。
        .observe_request_at(0, frame.request())
        // 合法 request 必须成功。
        .unwrap_or_else(|error| panic!("first request must observe: {error}"));
    // 记录首次 ledger 投影出的绝对 deadline。
    let first_deadline = runtime
        // 读取同 nonce 当前的 min-only deadline。
        .execution_deadline(NONCE)
        // 已观察 request 必须有可投影 deadline。
        .unwrap_or_else(|error| panic!("first deadline must project: {error}"))
        // 已观察 request 不得返回空投影。
        .unwrap_or_else(|| panic!("first deadline must exist"));
    // 建立 revision 零 accepted snapshot。
    runtime
        // 先于缩短重送保存业务接受事实。
        .apply_response(BrowserSessionBrokerResponse::accepted(
            // 绑定原 request。
            frame.request(),
            // accepted 固定 revision 零。
            0,
            // 绑定 live epoch。
            runtime.epoch(),
        ))
        // 合法 accepted 必须成功。
        .unwrap_or_else(|error| panic!("accepted must apply: {error}"));
    // 在 accepted 后观察当前 snapshot 与 generation。
    let accepted_observation = runtime
        // 使用不会缩短首次 deadline 的时刻。
        .observe_request_at(0, frame.request())
        // 同义 request 必须 attach。
        .unwrap_or_else(|error| panic!("accepted request must attach: {error}"));
    // 保存缩短前的变更代际。
    let old_generation = accepted_observation.generation();
    // 在更晚时刻使用相同完整语义但更短剩余预算构造重送。
    let shortened = BrowserSessionBrokerRequestFrame::open_confirmed(
        // 复用原 nonce。
        NONCE, // 复用原 epoch。
        EPOCH, // 将服务器绝对 deadline 缩短到 1 毫秒。
        1,
    )
    // 重送 frame 必须仍通过严格 parser。
    .unwrap_or_else(|error| panic!("shortened request must parse: {error}"));
    // 重送只能 attach 且缩短 ledger deadline。
    let observed = runtime
        // 在同一服务器时刻观察更短剩余预算。
        .observe_request_at(0, shortened.request())
        // 同 nonce 同义必须允许 attach。
        .unwrap_or_else(|error| panic!("shortened retry must attach: {error}"));
    // 新观察不得重新 dispatch。
    assert!(matches!(
        // 借用封闭决定。
        observed.decision(),
        // 只允许附着已 accepted execution。
        BrowserSessionBrokerExecutionDecision::Attach { deadline_ms: 1, .. }
    ));
    // 缩短重送后回包路径读取的绝对 deadline 也必须随之提前。
    let shortened_deadline = runtime
        // 读取已更新的 request deadline 投影。
        .execution_deadline(NONCE)
        // 重送后 ledger 仍必须保留 request。
        .unwrap_or_else(|error| panic!("shortened deadline must project: {error}"))
        // 已绑定 request 不得返回空投影。
        .unwrap_or_else(|| panic!("shortened deadline must exist"));
    // 不允许后续 response 写入重新获得首次十秒预算。
    assert!(shortened_deadline < first_deadline);
    // deadline 缩短必须推进 generation 以唤醒旧 waiter。
    assert_ne!(observed.generation(), old_generation);
    // 等待固定两毫秒，使缩短 deadline 在真实单调时钟上确定到达。
    thread::sleep(Duration::from_millis(2));
    // 缩短后的 deadline 已到达，execution closure 必须协作停止。
    assert_eq!(runtime.execution_should_stop(NONCE), Ok(true));
}

// 验证独立 cancel 连接可在 accepted 执行期间发布持久协作停止事实。
#[test]
fn accepted_request_observes_independent_cancel_and_settles_receipt() {
    // 建立共享运行时。
    let runtime = runtime();
    // 构造严格 open request。
    let frame = open_frame();
    // 首次 request 取得唯一 Dispatch。
    runtime
        // 写入 execution ledger。
        .observe_request_at(0, frame.request())
        // 合法请求必须成功。
        .unwrap_or_else(|error| panic!("request must dispatch: {error}"));
    // 保存 revision 零 accepted。
    runtime
        // 应用业务接受响应。
        .apply_response(BrowserSessionBrokerResponse::accepted(
            // 绑定原 request。
            frame.request(),
            // accepted 固定 revision 零。
            0,
            // 回显 live epoch。
            runtime.epoch(),
        ))
        // 合法转换不得失败。
        .unwrap_or_else(|error| panic!("accepted must apply: {error}"));
    // 构造独立 cancel control frame。
    let cancel = BrowserSessionBrokerCancelFrame::new(
        // 使用稳定 cancel nonce。
        "99999999999999999999999999999999",
        // 指向 accepted request。
        NONCE,
        // 绑定同一 live epoch。
        EPOCH,
    )
    // cancel builder 必须通过严格 parser。
    .unwrap_or_else(|error| panic!("cancel must parse: {error}"));
    // 独立 cancel 连接只登记协作停止。
    let first = runtime
        // 在线性化点推进 target。
        .observe_cancel(cancel.cancel())
        // accepted target 必须可取消。
        .unwrap_or_else(|error| panic!("cancel must observe: {error}"));
    // 首个 receipt 是非终态 cancellation-requested revision 零。
    assert_eq!(
        // 读取完整 receipt tuple。
        first,
        // 使用冻结初态。
        (
            // 标记协作停止。
            BrowserSessionBrokerCancelStatus::CancellationRequested,
            // 首次 revision 固定为零。
            0,
        )
    );
    // dispatcher 的取消 closure 必须观察持久事实。
    assert_eq!(runtime.cancellation_requested(NONCE), Ok(true));
    // 构造权威 cancelled 业务终态。
    let cancelled = BrowserSessionBrokerResponse::finished(
        // 绑定原 request。
        frame.request(),
        // accepted 后严格加一。
        1,
        // 回显 live epoch。
        runtime.epoch(),
        // 保存 cancelled outcome。
        BrowserSessionBrokerOutcome::Cancelled,
        // cancelled 不携带成功数据。
        None,
    )
    // 受控响应必须接受该组合。
    .unwrap_or_else(|| panic!("cancelled final must be valid"));
    // 保存终态并自动结算关联 receipt。
    runtime
        // 应用业务终态。
        .apply_response(cancelled)
        // 合法终态不得失败。
        .unwrap_or_else(|error| panic!("cancelled final must apply: {error}"));
    // 相同 cancel nonce 重送只 replay 已结算 receipt。
    let replay = runtime
        // 再次观察同义 cancel。
        .observe_cancel(cancel.cancel())
        // 幂等重送必须成功。
        .unwrap_or_else(|error| panic!("cancel replay must succeed: {error}"));
    // terminal receipt 只推进一次到 revision 一。
    assert_eq!(
        // 读取 replay tuple。
        replay,
        // 使用冻结终态。
        (
            // 原 request 以 cancelled 收敛。
            BrowserSessionBrokerCancelStatus::Cancelled,
            // 状态推进后 revision 为一。
            1,
        )
    );
}
