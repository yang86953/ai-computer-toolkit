//! Windows 独立交互会话 pipe 的故障生命周期回归测试。

// 导入线程间同步、唯一编号与单调时钟。
use std::{
    // 为统一测试结果帮助 trait 提供错误诊断边界。
    fmt::Debug,
    // 生成不会与并行测试冲突的进程内编号。
    sync::{
        // 生成不会与并行测试冲突的进程内编号。
        atomic::{AtomicU32, Ordering},
        // 用一次性就绪消息协调 server 创建与 client 连接。
        mpsc,
    },
    // 在独立线程模拟 broker 与 host 两端。
    thread,
    // 为取消、超时和恢复测试提供单调预算。
    time::{Duration, Instant},
};

// 导入 JSON 构造宏。
use serde_json::json;

// 只复用父 Component 的私有命名测试入口。
use super::{
    // 保存 browser 真正首实例且不把 handle 交给业务连接。
    BrowserSessionPipeOwner,
    // 复用固定连接与 listener 类型。
    ConnectedPipe,
    FixedLocalEndpointKind,
    ServerPipe,
    // 复用固定名称生成器。
    pipe_name,
};

// 为不同错误类型提供不触发 expect lint 的统一测试解包。
trait TestResultExt<T> {
    // 成功时返回值，失败时携带调用点消息终止当前测试。
    fn must(self, message: &str) -> T;
}

// 对任意可调试 Result 实现统一测试解包。
impl<T, E: Debug> TestResultExt<T> for Result<T, E> {
    // 保留原始错误诊断且不进入生产路径。
    fn must(self, message: &str) -> T {
        // 测试失败时同时输出稳定调用点与底层错误。
        self.unwrap_or_else(|error| panic!("{message}: {error:?}"))
    }
}

// 为同一测试进程中的每个 pipe 分配唯一后缀。
static NEXT_PIPE_ID: AtomicU32 = AtomicU32::new(1);

// 生成不与真实 Windows session 重叠的固定 browser 测试后缀。
fn browser_test_session() -> u32 {
    // 使用高位测试域与进程内序号隔离并行用例。
    3_000_000_000_u32.saturating_add(NEXT_PIPE_ID.fetch_add(1, Ordering::Relaxed))
}

// 构造仅供测试使用的本机 pipe 名称。
fn test_pipe_name(label: &str) -> Vec<u16> {
    // 取得当前进程内唯一序号。
    let sequence = NEXT_PIPE_ID.fetch_add(1, Ordering::Relaxed);
    // 将固定标签、PID 与序号组合，避免并行测试争抢首实例。
    let name = format!(
        // 使用不与生产 endpoint 重叠的测试前缀。
        r"\\.\pipe\ai-computer-toolkit-interactive-session-test-{}-{label}-{sequence}",
        // 进程 ID 只存在于测试对象名称中。
        std::process::id(),
    );
    // 编码为 Windows API 要求的 NUL 结尾 UTF-16。
    name.encode_utf16().chain(Some(0)).collect()
}

// 验证三个生产 endpoint 使用封闭、隔离且不可注入的固定名称与预算。
#[test]
fn production_endpoint_kinds_have_distinct_fixed_names() {
    // 读取交互 endpoint 双向预算。
    let interactive_budgets = FixedLocalEndpointKind::InteractiveSession.frame_budgets();
    // 交互 endpoint 保持既有双向 256 KiB 边界。
    assert_eq!(interactive_budgets, (256 * 1024, 256 * 1024));
    // 读取长操作 endpoint 双向预算。
    let long_operation_budgets = FixedLocalEndpointKind::LongOperation.frame_budgets();
    // 长操作 endpoint 双向都可以容纳 1 MiB 结果与固定 envelope。
    assert!(long_operation_budgets.0 > 1024 * 1024);
    // 长操作既有 endpoint 仍保持对称预算。
    assert_eq!(long_operation_budgets.0, long_operation_budgets.1);
    // 读取 browser-session 非对称预算。
    let browser_budgets = FixedLocalEndpointKind::BrowserSession.frame_budgets();
    // browser-session client-to-server 输入严格冻结为 64 KiB。
    assert_eq!(browser_budgets.0, 64 * 1024);
    // browser-session server-to-client 响应冻结为 16 MiB 加 128 KiB。
    assert_eq!(browser_budgets.1, 16 * 1024 * 1024 + 128 * 1024);
    // 为独立交互会话生成固定名称。
    let interactive = pipe_name(FixedLocalEndpointKind::InteractiveSession, 42)
        // 固定合法 session 必须成功。
        .must("interactive endpoint name should be generated");
    // 为长操作 broker 生成固定名称。
    let long_operation = pipe_name(FixedLocalEndpointKind::LongOperation, 42)
        // 固定合法 session 必须成功。
        .must("long operation endpoint name should be generated");
    // 两类 endpoint 不得争抢同一个内核对象。
    assert_ne!(interactive, long_operation);
    // 解码独立交互会话名称供固定前缀断言。
    let interactive = String::from_utf16(&interactive[..interactive.len() - 1])
        // 固定 ASCII 名称必须可解码。
        .must("interactive endpoint name should be valid UTF-16");
    // 解码长操作名称供固定前缀断言。
    let long_operation = String::from_utf16(&long_operation[..long_operation.len() - 1])
        // 固定 ASCII 名称必须可解码。
        .must("long operation endpoint name should be valid UTF-16");
    // 独立交互会话名称保持兼容。
    assert_eq!(
        interactive,
        r"\\.\pipe\ai-computer-toolkit-interactive-session-v1-42"
    );
    // 长操作名称只能来自编译期固定前缀与 native session。
    assert_eq!(
        long_operation,
        r"\\.\pipe\ai-computer-toolkit-long-operation-v1-42"
    );
    // Session 0 对两种 endpoint 都失败闭合。
    assert!(pipe_name(FixedLocalEndpointKind::InteractiveSession, 0).is_err());
    // 长操作同样不得在 Session 0 发布。
    assert!(pipe_name(FixedLocalEndpointKind::LongOperation, 0).is_err());
}

// 验证长操作固定预算可以传输超过交互上限的完整 JSON frame。
#[test]
fn long_operation_budget_carries_result_larger_than_interactive_frame() {
    // 创建不会碰触生产 endpoint 的唯一测试名称。
    let name = test_pipe_name("long-operation-budget");
    // 读取封闭长操作双向帧预算。
    let (request_budget, response_budget) = FixedLocalEndpointKind::LongOperation.frame_budgets();
    // 建立 server 就绪通知通道。
    let (ready_tx, ready_rx) = mpsc::channel();
    // 向 server 线程复制纯名称。
    let server_name = name.clone();
    // 在独立线程拥有 server handle。
    let server_thread = thread::spawn(move || {
        // 使用长操作预算创建测试 endpoint。
        let server = ServerPipe::create_named_with_budgets(
            // 借用唯一测试名称。
            &server_name,
            // server 读取 request 预算。
            request_budget,
            // server 写入 response 预算。
            response_budget,
        )
        // 当前用户 DACL 创建必须成功。
        .must("long operation budget server should be created");
        // 通知主线程 endpoint 已创建。
        ready_tx
            // 只发送无数据就绪信号。
            .send(())
            // 主线程必须仍在等待。
            .must("long operation budget readiness should be delivered");
        // 接受测试 client。
        let connection = server
            // 当前测试不请求取消。
            .accept(|| false)
            // client 连接后必须成功。
            .must("long operation budget connection should be accepted");
        // 读取超过交互上限的完整 frame。
        connection
            // 使用有界测试 deadline。
            .read_text_until(Instant::now() + Duration::from_secs(2), || false)
            // 完整 frame 必须可读。
            .must("long operation frame should be read")
    });
    // 等待 server endpoint 就绪。
    ready_rx
        // 通道关闭表示 server 提前失败。
        .recv()
        // 测试必须在连接前观察就绪。
        .must("long operation budget server should become ready");
    // 使用同一长操作预算连接测试 endpoint。
    let client = ConnectedPipe::connect_named_with_budgets_until(
        // 借用唯一测试名称。
        &name,
        // client 读取 server response 预算。
        response_budget,
        // client 写入 request 预算。
        request_budget,
        // 提供有界连接时间。
        Instant::now() + Duration::from_secs(2),
        // 当前测试不请求取消。
        || false,
    )
    // client 必须成功连接。
    .must("long operation budget client should connect");
    // 构造明显超过 256 KiB 的有界字符串。
    let payload = "x".repeat(300 * 1024);
    // 写入单条完整 JSON frame。
    client
        // 序列化受控测试 payload。
        .write_json(&json!({"result": payload}))
        // 长操作预算必须接受该 frame。
        .must("long operation frame should be written");
    // 等待 server 读取完成。
    let received = server_thread
        // 线程不应 panic。
        .join()
        // 返回完整 frame。
        .must("long operation budget server should finish");
    // 读取结果必须超过既有交互上限。
    assert!(received.len() > 256 * 1024);
}

// 验证首实例门禁拒绝恶意或重复同名 server。
#[test]
fn first_instance_rejects_duplicate_server_until_owner_drops() {
    // 创建不会碰触生产 endpoint 的唯一测试名称。
    let name = test_pipe_name("first-instance");
    // 首个 server 取得内核对象所有权。
    let owner = ServerPipe::create_named(&name)
        // 当前用户 DACL 创建必须成功。
        .must("first server should own the test endpoint");
    // 同名第二个 server 必须被 FILE_FLAG_FIRST_PIPE_INSTANCE 拒绝。
    assert!(ServerPipe::create_named(&name).is_err());
    // 释放唯一 owner handle。
    drop(owner);
    // owner 释放后同名 endpoint 可以由新代际重建。
    let replacement = ServerPipe::create_named(&name)
        // 新代际必须成功取得首实例。
        .must("replacement server should own the released endpoint");
    // 显式释放 replacement。
    drop(replacement);
}

// 验证 browser 首实例 guard 在两个并发 secondary 连接期间始终保留所有权。
#[test]
fn browser_owner_keeps_first_instance_while_secondary_connections_overlap() {
    // 生成不会碰触真实登录会话的固定测试后缀。
    let session_id = browser_test_session();
    // 创建不可交给业务 worker 的真正首实例 owner。
    let owner = BrowserSessionPipeOwner::create(session_id)
        // 当前用户应能建立受保护的内部 guard。
        .must("browser first-instance owner should be created");
    // 取得不延长 owner 生命周期的 secondary factory。
    let factory = owner.secondary_factory();
    // 建立两个 listener 的统一就绪通道。
    let (ready_tx, ready_rx) = mpsc::channel();
    // 建立两个连接已接受的统一同步通道。
    let (accepted_tx, accepted_rx) = mpsc::channel();
    // 为第一个 accept 线程复制弱 factory。
    let first_factory = factory.clone();
    // 为第二个 accept 线程复制就绪发送端。
    let second_ready_tx = ready_tx.clone();
    // 为第二个 accept 线程复制接受通知发送端。
    let second_accepted_tx = accepted_tx.clone();
    // 在独立线程创建 listener 并等待第一个 client。
    let first_accept = thread::spawn(move || {
        // 当前线程内创建不跨线程移动原生 handle 的 listener。
        let first_listener = first_factory
            // 固定 factory 不接收名称或预算。
            .create_listener()
            // owner live 时必须成功。
            .must("first browser secondary should be created");
        // 通知主线程第一个 listener 已发布。
        ready_tx
            // 只发送无数据就绪事实。
            .send(())
            // 主线程必须仍在等待。
            .must("first browser secondary readiness should be delivered");
        // 接收连接且不请求取消。
        let connection = first_listener
            // 使用 owner-preserving accept 的兼容入口。
            .accept(|| false)
            // 第一个连接必须成功。
            .must("first browser secondary should accept");
        // 通知主线程 server 已取得第一个连接 owner。
        accepted_tx
            // 只发送无数据接受事实。
            .send(())
            // 主线程必须仍在等待。
            .must("first browser secondary acceptance should be delivered");
        // 保持 server 连接 live，直到主线程允许当前 handler 结束。
        thread::sleep(Duration::from_millis(250));
        // 当前线程内关闭不跨线程安全边界的原生 handle。
        drop(connection);
    });
    // 为第二个 accept 线程复制弱 factory。
    let second_factory = factory.clone();
    // 在独立线程创建 listener 并等待第二个 client。
    let second_accept = thread::spawn(move || {
        // 当前线程内创建第二个 listener。
        let second_listener = second_factory
            // 两个 listener 必须可以同时存在。
            .create_listener()
            // 有界 instance 容量必须覆盖独立 cancel。
            .must("second browser secondary should be created");
        // 通知主线程第二个 listener 已发布。
        second_ready_tx
            // 只发送无数据就绪事实。
            .send(())
            // 主线程必须仍在等待。
            .must("second browser secondary readiness should be delivered");
        // 接收连接且不请求取消。
        let connection = second_listener
            // 第二 listener 独立等待。
            .accept(|| false)
            // 第二个连接必须成功。
            .must("second browser secondary should accept");
        // 通知主线程 server 已取得第二个连接 owner。
        second_accepted_tx
            // 只发送无数据接受事实。
            .send(())
            // 主线程必须仍在等待。
            .must("second browser secondary acceptance should be delivered");
        // 保持第二个 server 连接短时 live。
        thread::sleep(Duration::from_millis(250));
        // 当前线程内确定性关闭 handle。
        drop(connection);
    });
    // 等待两个 secondary 都已发布，避免把调度竞态当成传输失败。
    for _ in 0..2 {
        // 接收一个 listener 就绪事实。
        ready_rx
            // 任一线程提前退出都必须失败测试。
            .recv_timeout(Duration::from_secs(2))
            // 固定预算内必须观察就绪。
            .must("browser secondary should become ready");
    }
    // 连接第一个固定 browser instance。
    let first_client = ConnectedPipe::connect_for_until(
        // 只选择 browser-session 固定类别。
        FixedLocalEndpointKind::BrowserSession,
        // 使用相同测试后缀。
        session_id,
        // 限制连接预算。
        Instant::now() + Duration::from_secs(2),
        // 当前测试不取消。
        || false,
    )
    // 第一个 client 必须连接 secondary 而不是内部 guard。
    .must("first browser client should connect");
    // 在第一个连接仍 live 时连接第二个实例。
    let second_client = ConnectedPipe::connect_for_until(
        // 使用同一固定 endpoint 类别。
        FixedLocalEndpointKind::BrowserSession,
        // 使用同一测试后缀。
        session_id,
        // 限制连接预算。
        Instant::now() + Duration::from_secs(2),
        // 当前测试不取消。
        || false,
    )
    // 第二个 client 必须在第一个连接未关闭时成功。
    .must("second browser client should connect concurrently");
    // 等待两个 server 都已接受连接再关闭 client。
    for _ in 0..2 {
        // 接收一个连接已接受事实。
        accepted_rx
            // 任一 accept 线程提前退出都必须失败。
            .recv_timeout(Duration::from_secs(2))
            // 固定预算内必须完成 accept。
            .must("browser secondary connection should be accepted");
    }
    // 两个业务 client live 时重复 first-instance 仍必须失败。
    assert!(ServerPipe::create_for(FixedLocalEndpointKind::BrowserSession, session_id).is_err());
    // 按连接局部顺序关闭 client。
    drop(first_client);
    // 关闭第二个 client。
    drop(second_client);
    // 接收第一个 server 连接。
    first_accept
        // 线程不得 panic。
        .join()
        // 返回 live server 连接。
        .must("first browser accept thread should finish");
    // 接收第二个 server 连接。
    second_accept
        // 线程不得 panic。
        .join()
        // 返回 live server 连接。
        .must("second browser accept thread should finish");
    // 在并发连接关闭后创建用于验证 owner 关闭的闲置 secondary。
    let shutdown_listener = factory
        // factory 仍固定使用同源安全与预算。
        .create_listener()
        // owner live 时必须成功。
        .must("shutdown browser secondary should be created");
    // 最后关闭真正首实例 owner。
    drop(owner);
    // owner 关闭后旧 factory 不得创建无保护 secondary。
    assert!(factory.create_listener().is_err());
    // owner 关闭后既有 secondary 也不得再接受新 peer。
    match shutdown_listener.accept_persistent(|| false) {
        // owner lease 必须把 listener投影为已取消且保留owner供关闭。
        super::PersistentServerAccept::Cancelled(listener) => drop(listener),
        // 任何连接都表示旧 endpoint 仍能解释新业务。
        super::PersistentServerAccept::Connected(_) => {
            panic!("closed browser owner unexpectedly accepted a peer")
        }
        // 稳定关闭不得伪装成平台故障。
        super::PersistentServerAccept::Failed(_, error) => {
            panic!("closed browser owner returned failure: {error:?}")
        }
    }
    // 全部旧实例关闭后新代际可以取得 first-instance。
    let replacement = ServerPipe::create_for(FixedLocalEndpointKind::BrowserSession, session_id)
        // 新代际必须成功。
        .must("replacement browser first instance should be created");
    // 显式释放 replacement。
    drop(replacement);
}

// 验证 browser secondary 使用固定有界 instance 容量并在释放后恢复。
#[test]
fn browser_secondary_instance_capacity_is_bounded_and_recoverable() {
    // 生成独立固定测试后缀。
    let session_id = browser_test_session();
    // 创建占用一个 instance 的真正首实例 guard。
    let owner = BrowserSessionPipeOwner::create(session_id)
        // 当前用户应能创建固定 owner。
        .must("browser capacity owner should be created");
    // 取得 owner 约束的 factory。
    let factory = owner.secondary_factory();
    // 保存能够成功建立的全部 secondary owners。
    let mut listeners = Vec::new();
    // 首实例占用一个名额，因此固定 16 总量还允许 15 个 secondary。
    for _ in 0..15 {
        // 创建下一有界 secondary。
        listeners.push(
            factory
                // factory 使用同源 DACL、模式与预算。
                .create_listener()
                // 固定容量内必须成功。
                .must("browser secondary within capacity should be created"),
        );
    }
    // 第十六个 secondary 会超过包含首实例在内的固定总量。
    assert!(factory.create_listener().is_err());
    // 释放一个 secondary 名额。
    drop(listeners.pop());
    // 同一 owner 下释放的名额可以重新建立 listener。
    let recovered = factory
        // 再次创建固定 secondary。
        .create_listener()
        // 不得把容量耗尽永久化。
        .must("browser secondary capacity should recover after drop");
    // 先关闭恢复的 secondary。
    drop(recovered);
    // 再关闭其余 secondary。
    drop(listeners);
    // 最后关闭真正首实例 owner。
    drop(owner);
}

// 验证 owner 关闭后既有 browser 连接不能继续认证、写入或 relisten。
#[test]
fn browser_connected_secondary_fails_closed_after_owner_drop() {
    // 生成独立固定测试后缀。
    let session_id = browser_test_session();
    // 创建真正首实例 guard owner。
    let owner = BrowserSessionPipeOwner::create(session_id)
        // 当前用户应能创建固定 owner。
        .must("browser shutdown owner should be created");
    // 取得不延长 owner 生命周期的 factory。
    let factory = owner.secondary_factory();
    // 建立 listener 已发布通知。
    let (ready_tx, ready_rx) = mpsc::channel();
    // 建立连接已接受通知。
    let (accepted_tx, accepted_rx) = mpsc::channel();
    // 建立允许 server 检查关闭事实的门禁。
    let (closed_tx, closed_rx) = mpsc::channel();
    // 在唯一线程内创建、接受与销毁原生 server handle。
    let server_thread = thread::spawn(move || {
        // 创建受 owner lease 约束的 secondary listener。
        let listener = factory
            // 使用固定 factory。
            .create_listener()
            // owner live 时必须成功。
            .must("browser shutdown secondary should be created");
        // 通知主线程 listener 已发布。
        ready_tx
            // 只发送无数据就绪事实。
            .send(())
            // 主线程必须仍在等待。
            .must("browser shutdown readiness should be delivered");
        // 接受固定 browser client。
        let connection = listener
            // 当前测试不取消 accept。
            .accept(|| false)
            // client 连接后必须成功。
            .must("browser shutdown secondary should accept");
        // 通知主线程 server 已取得连接。
        accepted_tx
            // 只发送无数据接受事实。
            .send(())
            // 主线程必须仍在等待。
            .must("browser shutdown acceptance should be delivered");
        // 等待主线程先关闭真正 owner。
        closed_rx
            // 通道关闭表示测试次序失效。
            .recv_timeout(Duration::from_secs(2))
            // 固定预算内必须观察关闭。
            .must("browser owner closure should be delivered");
        // 关闭后不能再取得 peer 身份。
        assert!(connection.peer_process_id().is_err());
        // 关闭后不能发送任何 JSON 文本。
        assert!(connection.write_text("{\"kind\":\"fixture\"}").is_err());
        // 关闭后不能把旧 connection 恢复为 listener。
        match connection.relisten() {
            // owner 关闭必须返回保留 owner 的失败。
            super::PersistentServerRelisten::Failed(connection, _) => drop(connection),
            // 重新监听会让旧代 endpoint 继续接收 peer。
            super::PersistentServerRelisten::Listening(_) => {
                panic!("closed browser owner unexpectedly relistened")
            }
        }
    });
    // 等待 secondary listener 已发布。
    ready_rx
        // 线程提前退出必须失败。
        .recv_timeout(Duration::from_secs(2))
        // 固定预算内必须就绪。
        .must("browser shutdown secondary should become ready");
    // 连接固定 browser secondary。
    let client = ConnectedPipe::connect_for_until(
        // 只选择 browser-session 固定类别。
        FixedLocalEndpointKind::BrowserSession,
        // 使用同一测试后缀。
        session_id,
        // 限制连接预算。
        Instant::now() + Duration::from_secs(2),
        // 当前测试不取消。
        || false,
    )
    // client 必须成功取得 secondary。
    .must("browser shutdown client should connect");
    // 等待 server 已完成 accept。
    accepted_rx
        // server 提前退出必须失败。
        .recv_timeout(Duration::from_secs(2))
        // 固定预算内必须接受。
        .must("browser shutdown connection should be accepted");
    // 在 client 与 server connection 仍 live 时关闭真正 owner。
    drop(owner);
    // 通知 server 验证关闭后的失败闭合。
    closed_tx
        // 只发送无数据关闭事实。
        .send(())
        // server 必须仍在等待。
        .must("browser owner closure should be signalled");
    // 等待 server 完成全部断言。
    server_thread
        // 线程不得 panic。
        .join()
        // 返回单元结果。
        .must("browser shutdown server thread should finish");
    // 最后关闭 client handle。
    drop(client);
}

// 验证无客户端时 accept 仍能立即观察取消。
#[test]
fn accept_observes_cancellation_without_client() {
    // 创建不会碰触生产 broker 的独立 server。
    let server = ServerPipe::create_named(&test_pipe_name("accept-cancel"))
        // 测试环境应能安装当前用户 DACL。
        .must("test server pipe should be created");
    // 记录取消观察耗时。
    let started = Instant::now();
    // 第一次轮询即报告调用方取消。
    let error = match server.accept(|| true) {
        // 成功连接意味着取消门禁失效。
        Ok(_) => panic!("cancelled accept unexpectedly connected"),
        // 保留结构化失败供断言。
        Err(error) => error,
    };
    // 取消必须保留公共稳定错误码。
    assert_eq!(error.code, "CANCELLED");
    // 无客户端取消不应等待平台默认超时。
    assert!(started.elapsed() < Duration::from_secs(1));
}

// 注册非阻塞有界写入的独立回压测试，保持本文件行数低于上限。
#[path = "pipe_write_tests.rs"]
mod write_tests;
// 注册 browser 非阻塞读取的独立真实 pipe 回归。
#[path = "pipe_read_tests.rs"]
mod read_tests;
// 注册 server CloseHandle 保留未读 browser 帧的独立真实 pipe 回归。
#[path = "pipe_close_tests.rs"]
mod close_tests;

// 验证读取超时释放旧连接后，同名 endpoint 可以重建并完成新请求。
#[test]
fn read_timeout_closes_connection_and_new_connection_recovers() {
    // 为超时与恢复阶段保留同一个测试名称。
    let name = test_pipe_name("timeout-recovery");
    // 为第一代 server 建立就绪通知通道。
    let (first_ready_tx, first_ready_rx) = mpsc::channel();
    // 向 broker 线程复制纯 UTF-16 名称而不是移动原生 handle。
    let first_name = name.clone();
    // 在 broker 线程等待一个不发送消息的客户端。
    let timeout_thread = thread::spawn(move || {
        // 在所属线程内创建第一代 server handle。
        let first_server = ServerPipe::create_named(&first_name)
            // 测试环境应能创建受限 pipe。
            .must("first test server pipe should be created");
        // 通知主线程 endpoint 已经创建。
        first_ready_tx
            // 只发送无数据就绪信号。
            .send(())
            // 测试主线程必须仍在等待。
            .must("first test readiness should be delivered");
        // 接受第一代客户端连接。
        let connection = first_server
            // 当前测试不请求取消。
            .accept(|| false)
            // 连接应在测试 client 打开后成功。
            .must("first test connection should be accepted");
        // 对空连接执行有界读取。
        let error = match connection.read_text_until(
            // 使用足够观察轮询又不会拖慢套件的预算。
            Instant::now() + Duration::from_millis(150),
            // 当前测试不请求取消。
            || false,
        ) {
            // 空连接不得产生消息。
            Ok(_) => panic!("empty connection unexpectedly returned a message"),
            // 返回结构化 timeout。
            Err(error) => error,
        };
        // 线程只返回稳定错误码。
        error.code
    });
    // 等待第一代 endpoint 创建完成。
    first_ready_rx
        // 通道关闭表示 broker 线程提前失败。
        .recv()
        // 测试必须在连接前观察就绪。
        .must("first test server should become ready");
    // 连接第一代 server，但故意不发送 frame。
    let first_client = ConnectedPipe::connect_named_until(
        // 使用同一个测试名称。
        &name,
        // 为本机连接提供宽松预算。
        Instant::now() + Duration::from_secs(2),
        // 当前测试不请求取消。
        || false,
    )
    // 第一代 client 应连接成功。
    .must("first test client should connect");
    // 等待 server 侧读取预算结束并释放其 handle。
    let timeout_code = timeout_thread
        // 测试线程不应 panic。
        .join()
        // 将线程 panic 视为测试失败。
        .must("timeout server thread should finish");
    // 空连接必须稳定映射为 timeout。
    assert_eq!(timeout_code, "TIMEOUT");
    // 显式释放已被 server 断开的第一代 client。
    drop(first_client);

    // 为第二代 server 建立就绪通知通道。
    let (second_ready_tx, second_ready_rx) = mpsc::channel();
    // 向新 broker 线程复制纯 UTF-16 名称。
    let second_name = name.clone();
    // 在第二代 broker 线程完成一次 JSON 往返。
    let recovery_thread = thread::spawn(move || {
        // 在所属线程内使用相同名称重建 server，证明首实例已完整回收。
        let second_server = ServerPipe::create_named(&second_name)
            // 同名 endpoint 应能在前代回收后重建。
            .must("second test server pipe should be recreated");
        // 通知主线程第二代 endpoint 已经创建。
        second_ready_tx
            // 只发送无数据就绪信号。
            .send(())
            // 测试主线程必须仍在等待。
            .must("second test readiness should be delivered");
        // 接受第二代客户端。
        let connection = second_server
            // 当前测试不请求取消。
            .accept(|| false)
            // 新连接必须正常建立。
            .must("second test connection should be accepted");
        // 读取恢复请求。
        let request = connection
            // 为完整 roundtrip 提供宽松预算。
            .read_text_until(Instant::now() + Duration::from_secs(2), || false)
            // 恢复连接必须收到完整 frame。
            .must("recovery request should be read");
        // 解析请求以验证 frame 没有跨代污染。
        let request: serde_json::Value = serde_json::from_str(&request)
            // 测试请求必须是合法 JSON。
            .must("recovery request should be valid JSON");
        // 只响应本轮 request ID。
        connection
            // 发送固定响应供 client 验证。
            .write_json(&json!({"requestId": request["requestId"], "ok": true}))
            // 第二代 server 应能写回完整 frame。
            .must("recovery response should be written");
        // 最终响应写入后有界等待 client 读取并关闭连接。
        let error = match connection
            // 不接受最终响应后的额外 client frame。
            .read_text_until(Instant::now() + Duration::from_secs(2), || false)
        {
            // client 不应发送第三条消息。
            Ok(_) => panic!("recovery client unexpectedly sent an extra frame"),
            // client 关闭应解除等待。
            Err(error) => error,
        };
        // client 读完并关闭后应报告 endpoint 已释放。
        assert_eq!(error.code, "ISOLATED_WORKER_UNAVAILABLE");
    });
    // 等待第二代 endpoint 创建完成。
    second_ready_rx
        // 通道关闭表示恢复线程提前失败。
        .recv()
        // 测试必须在连接前观察就绪。
        .must("second test server should become ready");
    // 连接第二代 server。
    let second_client = ConnectedPipe::connect_named_until(
        // 使用与第一代相同的名称。
        &name,
        // 为本机连接提供宽松预算。
        Instant::now() + Duration::from_secs(2),
        // 当前测试不请求取消。
        || false,
    )
    // 第二代 client 应连接成功。
    .must("second test client should connect");
    // 发送唯一恢复请求。
    second_client
        // 单条 WriteFile 保持 message 边界。
        .write_json(&json!({"requestId": "recovered"}))
        // 请求应进入第二代连接。
        .must("recovery request should be written");
    // 读取第二代 server 的响应。
    let response = second_client
        // 使用单调 deadline 防止测试挂死。
        .read_text_until(Instant::now() + Duration::from_secs(2), || false)
        // 恢复响应必须可读。
        .must("recovery response should be read");
    // 解析响应供字段级断言。
    let response: serde_json::Value = serde_json::from_str(&response)
        // server 必须返回合法 JSON。
        .must("recovery response should be valid JSON");
    // 新连接必须对应本轮请求。
    assert_eq!(response["requestId"], "recovered");
    // 新连接必须报告成功。
    assert_eq!(response["ok"], true);
    // 显式关闭 client，确认 server 的最终响应等待可以结束。
    drop(second_client);
    // 等待 server 完整释放第二代连接。
    recovery_thread
        // 测试线程不应 panic。
        .join()
        // 将线程 panic 视为测试失败。
        .must("recovery server thread should finish");
}

// 验证对端在无消息时退出会解除读取等待。
#[test]
fn peer_disconnect_unblocks_wait() {
    // 创建独立测试 endpoint 名称。
    let name = test_pipe_name("peer-disconnect");
    // 为断连 server 建立就绪通知通道。
    let (ready_tx, ready_rx) = mpsc::channel();
    // 为已完成内核连接建立第二个同步通道。
    let (accepted_tx, accepted_rx) = mpsc::channel();
    // 向 broker 线程复制纯 UTF-16 名称。
    let server_name = name.clone();
    // 记录整个断连测试耗时。
    let started = Instant::now();
    // 在 broker 线程等待 client 消息。
    let server_thread = thread::spawn(move || {
        // 在所属线程内创建 server handle。
        let server = ServerPipe::create_named(&server_name)
            // 测试环境应允许创建本机受限 pipe。
            .must("disconnect test server should be created");
        // 通知主线程 endpoint 已经创建。
        ready_tx
            // 只发送无数据就绪信号。
            .send(())
            // 测试主线程必须仍在等待。
            .must("disconnect test readiness should be delivered");
        // 接受即将断开的 client。
        let connection = server
            // 当前测试不请求取消。
            .accept(|| false)
            // client 打开后必须成功连接。
            .must("disconnect test connection should be accepted");
        // 通知主线程 server 已经接管该连接。
        accepted_tx
            // 只发送无数据同步信号。
            .send(())
            // 测试主线程必须仍在等待。
            .must("disconnect acceptance should be delivered");
        // 等待消息或对端断开。
        let error = match connection.read_text_until(
            // 宽松 deadline 用于区分断连与 timeout。
            Instant::now() + Duration::from_secs(2),
            // 当前测试不请求取消。
            || false,
        ) {
            // 已断开的 client 不得产生消息。
            Ok(_) => panic!("disconnected client unexpectedly returned a message"),
            // 返回稳定 endpoint 错误。
            Err(error) => error,
        };
        // 线程只返回公共错误码。
        error.code
    });
    // 等待断连 endpoint 创建完成。
    ready_rx
        // 通道关闭表示 broker 线程提前失败。
        .recv()
        // 测试必须在连接前观察就绪。
        .must("disconnect test server should become ready");
    // 连接 server。
    let client = ConnectedPipe::connect_named_until(
        // 使用本测试的唯一名称。
        &name,
        // 为本机连接提供宽松预算。
        Instant::now() + Duration::from_secs(2),
        // 当前测试不请求取消。
        || false,
    )
    // client 应在 server 监听后连接。
    .must("disconnect test client should connect");
    // 等待 server 确认连接，避免关闭发生在 ConnectNamedPipe 之前。
    accepted_rx
        // 通道关闭表示 broker 线程提前失败。
        .recv()
        // 测试必须先观察已连接状态。
        .must("disconnect test connection should be accepted");
    // 不发送任何 frame 就关闭 client，模拟 host 或 broker 崩溃。
    drop(client);
    // 等待 server 观察内核断连。
    let error_code = server_thread
        // 测试线程不应 panic。
        .join()
        // 将线程 panic 视为测试失败。
        .must("disconnect server thread should finish");
    // 断连必须映射为 endpoint 不可用而非等待到 timeout。
    assert_eq!(error_code, "ISOLATED_WORKER_UNAVAILABLE");
    // 对端退出必须及时解除等待。
    assert!(started.elapsed() < Duration::from_secs(1));
}
