//! 验证唯一 dispatcher 的业务前 open/close 协调。

// 导入共享 runtime 与有限等待时长。
use std::{
    // 共享 dispatcher 与测试观察者。
    sync::Arc,
    // 为状态等待设置有界轮询。
    time::{Duration, Instant},
};

// 导入 strict close builder、epoch 与执行决定。
use crate::components::{
    // 导入协议类型。
    browser_session_broker_protocol::{
        // 导入协议失败码。
        BrowserSessionBrokerProtocolErrorCode,
        // 导入可关联协议失败。
        BrowserSessionBrokerProtocolFailure,
        // 导入 outcome、response 与成功数据。
        response::{
            // 导入封闭 outcome。
            BrowserSessionBrokerOutcome,
            // 导入协议响应。
            BrowserSessionBrokerResponse,
            // 导入 operation-specific 成功数据。
            BrowserSessionBrokerSuccess,
        },
        // 导入 execution 决定与 epoch。
        state::{BrowserSessionBrokerEpoch, BrowserSessionBrokerExecutionDecision},
        // 导入严格 close frame builder。
        wire::BrowserSessionBrokerRequestFrame,
    },
    // 导入共享 runtime。
    browser_session_broker_runtime::BrowserSessionBrokerRuntime,
};

// 导入 System 的 session inspect 最小投影。
use crate::service::browser_session_broker::BrowserSessionBrokerInspectExecution;

// 导入被测 dispatcher、Query final 与 accepted 后收敛 helper。
use super::{BrowserSessionBrokerDispatcher, inspect_final, settle_after_acceptance};

// 固定测试 epoch。
const EPOCH: &str = "44444444444444444444444444444444";
// 固定 request nonce。
const NONCE: &str = "55555555555555555555555555555555";
// 固定不存在的 canonical session。
const SESSION: &str = "s2:bs:66666666666666666666666666666666";
// 固定外来 request nonce。
const FOREIGN_NONCE: &str = "77777777777777777777777777777777";

// 验证 stale close 在 accepted 前缓存 rejected 且不启动真实浏览器。
#[test]
fn dispatcher_caches_stale_close_before_business_acceptance() {
    // 构造 live epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())
        // 固定常量必须合法。
        .unwrap_or_else(|error| panic!("epoch must parse: {error}"));
    // 建立共享 runtime。
    let runtime = Arc::new(BrowserSessionBrokerRuntime::new(epoch));
    // 模拟 endpoint 已发布。
    runtime
        // 开放连接接入。
        .start_accepting()
        // 空 runtime 不得失败。
        .unwrap_or_else(|error| panic!("runtime must start: {error}"));
    // 启动唯一 System dispatcher。
    let dispatcher = BrowserSessionBrokerDispatcher::start(Arc::clone(&runtime))
        // 线程启动必须成功。
        .unwrap_or_else(|error| panic!("dispatcher must start: {error}"));
    // 取得不拥有 System 的派发端口。
    let port = dispatcher.port();
    // 构造指向不存在 session 的严格 close。
    let frame = BrowserSessionBrokerRequestFrame::close_confirmed(
        // 使用固定 nonce。
        NONCE,  // 绑定 live epoch。
        EPOCH,  // 使用十秒预算。
        10_000, // 目标不存在，因此只能业务前 stale rejection。
        SESSION,
    )
    // builder 必须通过生产 parser。
    .unwrap_or_else(|error| panic!("close frame must parse: {error}"));
    // 首次观察取得唯一 Dispatch。
    let first = runtime
        // 进入 execution ledger。
        .observe_request(frame.request())
        // 合法 strict request 必须成功。
        .unwrap_or_else(|error| panic!("request must dispatch: {error}"));
    // 证明首次决定不是 Attach/Replay。
    assert!(matches!(
        // 借用首次决定。
        first.decision(),
        // 只允许 Dispatch。
        BrowserSessionBrokerExecutionDecision::Dispatch { .. }
    ));
    // 将纯 request 移交唯一 dispatcher。
    port.dispatch(frame.request().clone())
        // 有界队列必须接收。
        .unwrap_or_else(|error| panic!("dispatch must enqueue: {error}"));
    // 使用短总预算轮询 terminal replay。
    let deadline = Instant::now() + Duration::from_secs(2);
    // 持续观察直到 dispatcher 保存 final。
    loop {
        // 同义观察永远不会重新派发。
        let observed = runtime
            // 重新读取最新 snapshot。
            .observe_request(frame.request())
            // attach/replay 必须成功。
            .unwrap_or_else(|error| panic!("request observation must succeed: {error}"));
        // terminal replay 结束等待。
        if let BrowserSessionBrokerExecutionDecision::Replay { response } = observed.decision() {
            // 必须是业务前 rejected。
            assert_eq!(
                response.outcome(),
                Some(BrowserSessionBrokerOutcome::Rejected)
            );
            // 必须保留 stale session 码。
            assert_eq!(response.error_code(), Some("STALE_SESSION"));
            // 业务绝未接受。
            assert!(!response.business_accepted());
            // 结束轮询。
            break;
        }
        // dispatcher 应在窄预算内完成纯预检。
        assert!(
            Instant::now() < deadline,
            "dispatcher did not publish rejection"
        );
        // 避免测试忙等。
        std::thread::sleep(Duration::from_millis(1));
    }
    // 停止 handler 接入。
    runtime.stop_accepting();
    // drop 最后一个派发端口，保证 Stop 排在全部 Execute 后。
    drop(port);
    // join 线程并在线程内先销毁 System。
    dispatcher
        // 有序关闭。
        .shutdown()
        // 空 Module 关闭不得失败。
        .unwrap_or_else(|error| panic!("dispatcher must stop: {error}"));
}

// 验证 session.inspect 成功终态保持 Query 标志且只回显最小 live 事实。
#[test]
fn inspect_final_is_completed_query_without_mutation_or_unknown() {
    // 建立含 accepted session.inspect snapshot 的纯 runtime。
    let (runtime, frame) = accepted_inspect_runtime();
    // 从 System 最小投影构造 revision 一 Query final。
    let response = inspect_final(
        // 借用 current epoch runtime。
        &runtime,
        // 绑定原严格 Query。
        frame.request(),
        // 模拟 Module 在线性化点确认 live。
        BrowserSessionBrokerInspectExecution::Completed {
            // 回显原 opaque session identity。
            session_id: SESSION.to_owned(),
            // 成功查询固定为 live。
            live: true,
        },
    )
    // 同源 response validator 必须接受。
    .unwrap_or_else(|error| panic!("inspect final must build: {error:?}"));
    // Query 必须形成 completed final。
    assert_eq!(
        response.outcome(),
        Some(BrowserSessionBrokerOutcome::Completed)
    );
    // accepted 后 final 固定 revision 一。
    assert_eq!(response.request_revision(), 1);
    // Query 绝不声明目标可能变更。
    assert!(!response.target_may_have_mutated());
    // Query 确定结果绝不声明 outcome unknown。
    assert!(!response.outcome_unknown());
    // 只允许 session identity 与 live 布尔事实。
    assert!(matches!(
        // 借用成功投影。
        response.success(),
        // 精确匹配最小 Query 事实。
        Some(BrowserSessionBrokerSuccess::SessionInspect { session_id, live })
            // 核对 identity 与 live 值。
            if session_id == SESSION && *live
    ));
}

// 验证 stale session.inspect 在 accepted 前缓存 rejected 且保持 Query 零副作用。
#[test]
fn dispatcher_rejects_stale_inspect_before_business_acceptance() {
    // 构造 live epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())
        // 固定常量必须合法。
        .unwrap_or_else(|error| panic!("epoch must parse: {error}"));
    // 建立共享 runtime。
    let runtime = Arc::new(BrowserSessionBrokerRuntime::new(epoch));
    // 模拟 endpoint 已发布。
    runtime
        // 开放连接接入。
        .start_accepting()
        // 空 runtime 不得失败。
        .unwrap_or_else(|error| panic!("runtime must start: {error}"));
    // 启动唯一 System dispatcher。
    let dispatcher = BrowserSessionBrokerDispatcher::start(Arc::clone(&runtime))
        // 线程启动必须成功。
        .unwrap_or_else(|error| panic!("dispatcher must start: {error}"));
    // 取得不拥有 System 的派发端口。
    let port = dispatcher.port();
    // 构造指向不存在 session 的严格 Query。
    let frame = BrowserSessionBrokerRequestFrame::session_inspect(
        // 使用固定 nonce。
        NONCE,  // 绑定 live epoch。
        EPOCH,  // 使用十秒预算。
        10_000, // 目标不存在，只能业务前 stale rejection。
        SESSION,
    )
    // builder 必须通过生产 parser。
    .unwrap_or_else(|error| panic!("inspect frame must parse: {error}"));
    // 首次观察取得唯一 Dispatch。
    runtime
        // 进入统一 execution ledger。
        .observe_request(frame.request())
        // 合法严格 Query 必须成功。
        .unwrap_or_else(|error| panic!("inspect request must dispatch: {error}"));
    // 将纯 request 移交唯一 dispatcher。
    port.dispatch(frame.request().clone())
        // 有界队列必须接收。
        .unwrap_or_else(|error| panic!("inspect dispatch must enqueue: {error}"));
    // 使用短总预算轮询 terminal replay。
    let deadline = Instant::now() + Duration::from_secs(2);
    // 持续观察直到 dispatcher 保存 final。
    loop {
        // 同义观察永远不会重新派发。
        let observed = runtime
            // 重新读取最新 snapshot。
            .observe_request(frame.request())
            // attach/replay 必须成功。
            .unwrap_or_else(|error| panic!("inspect observation must succeed: {error}"));
        // terminal replay 结束等待。
        if let BrowserSessionBrokerExecutionDecision::Replay { response } = observed.decision() {
            // 必须是业务前 rejected。
            assert_eq!(
                response.outcome(),
                Some(BrowserSessionBrokerOutcome::Rejected)
            );
            // 必须保留 stale session 码。
            assert_eq!(response.error_code(), Some("STALE_SESSION"));
            // Query 业务绝未接受。
            assert!(!response.business_accepted());
            // Query 绝不声明目标变更。
            assert!(!response.target_may_have_mutated());
            // 结束轮询。
            break;
        }
        // dispatcher 应在窄预算内完成只读预检。
        assert!(
            Instant::now() < deadline,
            "dispatcher did not reject stale inspect"
        );
        // 避免测试忙等。
        std::thread::sleep(Duration::from_millis(1));
    }
    // 停止 handler 接入。
    runtime.stop_accepting();
    // drop 最后一个派发端口，保证 Stop 排在全部 Execute 后。
    drop(port);
    // join 线程并在线程内先销毁 System。
    dispatcher
        // 有序关闭。
        .shutdown()
        // 空 Module 关闭不得失败。
        .unwrap_or_else(|error| panic!("dispatcher must stop: {error}"));
}

// 验证 accepted 后终态构造失败仍会留下权威 unknown replay。
#[test]
fn accepted_internal_failure_settles_unknown_before_epoch_stop() {
    // 建立含 accepted snapshot 的纯 runtime。
    let (runtime, frame) = accepted_open_runtime();
    // 注入 accepted 后内部失败。
    let result = settle_after_acceptance(
        // 借用 runtime。
        &runtime,
        // 绑定原 request。
        frame.request(),
        // 模拟终态构造失败。
        Err(BrowserSessionBrokerProtocolFailure::new(
            // 使用封闭内部失败码。
            BrowserSessionBrokerProtocolErrorCode::ProtocolFailed,
        )),
    );
    // dispatcher 必须保留失败以停止当前 epoch。
    assert!(result.is_err());
    // 同义重送必须只回放缓存 unknown。
    assert_unknown_replay(&runtime, frame.request());
}

// 验证 accepted 后候选终态与 request 错配时不留下无终态 accepted。
#[test]
fn accepted_invalid_candidate_settles_unknown_without_overwrite() {
    // 建立含 accepted snapshot 的纯 runtime。
    let (runtime, frame) = accepted_open_runtime();
    // 构造不同 nonce 的严格 open frame。
    let foreign = BrowserSessionBrokerRequestFrame::open_confirmed(
        // 使用外来 nonce。
        FOREIGN_NONCE,
        // 绑定相同 epoch。
        EPOCH,
        // 使用相同预算。
        10_000,
    )
    // builder 必须形成可验证的严格 request。
    .unwrap_or_else(|error| panic!("foreign frame must parse: {error}"));
    // 构造合法但与原 ledger record 错配的 unknown。
    let invalid = BrowserSessionBrokerResponse::unknown(
        // 绑定外来 request。
        foreign.request(),
        // accepted 后终态固定 revision 一。
        1,
        // 绑定当前 epoch。
        runtime.epoch(),
    );
    // 试图落账错配候选并触发保守收敛。
    let result = settle_after_acceptance(&runtime, frame.request(), Ok(invalid));
    // 错配必须停止当前 epoch。
    assert!(result.is_err());
    // 原 request 只能回放保守 unknown。
    assert_unknown_replay(&runtime, frame.request());
}

// 建立只含一个 open accepted snapshot 的纯 runtime。
fn accepted_open_runtime() -> (
    Arc<BrowserSessionBrokerRuntime>,
    BrowserSessionBrokerRequestFrame,
) {
    // 构造固定 live epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())
        // 固定常量必须合法。
        .unwrap_or_else(|error| panic!("epoch must parse: {error}"));
    // 建立共享 runtime。
    let runtime = Arc::new(BrowserSessionBrokerRuntime::new(epoch));
    // 模拟 endpoint 已发布。
    runtime
        // 开放 request 接入。
        .start_accepting()
        // 空 runtime 不得失败。
        .unwrap_or_else(|error| panic!("runtime must start: {error}"));
    // 构造严格 open frame。
    let frame = BrowserSessionBrokerRequestFrame::open_confirmed(
        // 使用固定 nonce。
        NONCE, // 绑定 live epoch。
        EPOCH, // 使用十秒预算。
        10_000,
    )
    // builder 必须通过生产 parser。
    .unwrap_or_else(|error| panic!("open frame must parse: {error}"));
    // 首次观察预留唯一 execution record。
    runtime
        // 进入 execution ledger。
        .observe_request(frame.request())
        // 合法 request 必须取得 Dispatch。
        .unwrap_or_else(|error| panic!("request must dispatch: {error}"));
    // 构造 revision 零 accepted。
    let accepted = BrowserSessionBrokerResponse::accepted(
        // 绑定原 request。
        frame.request(),
        // accepted 固定 revision 零。
        0,
        // 回显 live epoch。
        runtime.epoch(),
    );
    // 原子提交 accepted snapshot。
    let commit = runtime
        // 与 cancel/deadline 线性化。
        .commit_dispatch_snapshot(frame.request(), accepted)
        // 空 ledger 必须允许首次 accepted。
        .unwrap_or_else(|error| panic!("accepted must commit: {error}"));
    // 首次提交必须授予唯一执行权。
    assert!(commit.should_execute());
    // 返回 runtime 与绑定 frame。
    (runtime, frame)
}

// 建立只含一个 session.inspect accepted snapshot 的纯 runtime。
fn accepted_inspect_runtime() -> (
    Arc<BrowserSessionBrokerRuntime>,
    BrowserSessionBrokerRequestFrame,
) {
    // 构造固定 live epoch。
    let epoch = BrowserSessionBrokerEpoch::new(EPOCH.to_owned())
        // 固定常量必须合法。
        .unwrap_or_else(|error| panic!("epoch must parse: {error}"));
    // 建立共享 runtime。
    let runtime = Arc::new(BrowserSessionBrokerRuntime::new(epoch));
    // 模拟 endpoint 已发布。
    runtime
        // 开放 request 接入。
        .start_accepting()
        // 空 runtime 不得失败。
        .unwrap_or_else(|error| panic!("runtime must start: {error}"));
    // 构造严格 session.inspect Query。
    let frame = BrowserSessionBrokerRequestFrame::session_inspect(
        // 使用固定 nonce。
        NONCE,  // 绑定 live epoch。
        EPOCH,  // 使用十秒预算。
        10_000, // 查询固定 canonical session。
        SESSION,
    )
    // builder 必须通过生产 parser。
    .unwrap_or_else(|error| panic!("inspect frame must parse: {error}"));
    // 首次观察预留唯一 Query execution record。
    runtime
        // 进入统一 execution ledger。
        .observe_request(frame.request())
        // 合法 Query 必须取得 Dispatch。
        .unwrap_or_else(|error| panic!("inspect request must dispatch: {error}"));
    // 构造 revision 零 accepted。
    let accepted = BrowserSessionBrokerResponse::accepted(
        // 绑定原 Query。
        frame.request(),
        // accepted 固定 revision 零。
        0,
        // 回显 live epoch。
        runtime.epoch(),
    );
    // 原子提交 accepted snapshot。
    let commit = runtime
        // Query 使用同一 nonce/deadline ledger。
        .commit_dispatch_snapshot(frame.request(), accepted)
        // 首次 accepted 必须成功。
        .unwrap_or_else(|error| panic!("inspect accepted must commit: {error}"));
    // 首次提交必须授予唯一只读执行权。
    assert!(commit.should_execute());
    // 返回纯 runtime 与严格 Query frame。
    (runtime, frame)
}

// 断言原 request 已收敛为 revision 一 unknown replay。
fn assert_unknown_replay(
    // 借用共享 runtime。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用原严格 request。
    request: &crate::components::browser_session_broker_protocol::BrowserSessionBrokerRequest,
) {
    // 同义重送读取权威 snapshot。
    let observed = runtime
        // 重新观察同 nonce 同语义 request。
        .observe_request(request)
        // replay 路径必须成功。
        .unwrap_or_else(|error| panic!("request must replay: {error}"));
    // 只接受 terminal replay。
    let BrowserSessionBrokerExecutionDecision::Replay { response } = observed.decision() else {
        // 其他状态表示 accepted 后留下了无终态 ledger。
        panic!("request must replay unknown");
    };
    // 必须是保守 unknown outcome。
    assert_eq!(
        response.outcome(),
        Some(BrowserSessionBrokerOutcome::Unknown)
    );
    // accepted 后终态固定 revision 一。
    assert_eq!(response.request_revision(), 1);
    // unknown 绝不可安全重试。
    assert!(response.outcome_unknown());
}
