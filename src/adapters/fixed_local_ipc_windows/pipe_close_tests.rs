//! 验证 server 只关闭本端 handle 时保留未读 browser message。

// 导入确定同步、独立 server 线程与单调预算。
use std::{
    // 使用通道锁定 listener 发布与 server handle 关闭时序。
    sync::mpsc,
    // 在独立线程持有生产级 browser secondary。
    thread,
    // 为连接、I/O 与延迟读取提供固定预算。
    time::{Duration, Instant},
};

// 导入父测试模块的唯一 browser session 与失败解包帮助。
use super::{TestResultExt, browser_test_session};
// 导入祖先 pipe Component 的真实 browser 所有者、client 与 endpoint 类别。
use super::super::{BrowserSessionPipeOwner, ConnectedPipe, FixedLocalEndpointKind};

// 固定连接、写入、读取与线程协调的最大预算。
const THREAD_DEADLINE: Duration = Duration::from_secs(2);
// 固定 client 在 server drop 后继续延迟读取的时间。
const DELAYED_READ: Duration = Duration::from_millis(60);
// 使用逐字可比较的非终态 accepted 帧。
const ACCEPTED_FRAME: &str = r#"{"phase":"accepted","sequence":1}"#;
// 使用逐字可比较的终态 final 帧。
const FINAL_FRAME: &str = r#"{"phase":"final","sequence":2}"#;

// 验证 server 写入两帧后立即 CloseHandle，client 仍可在 server drop 后延迟逐字读取。
#[test]
fn server_close_preserves_delayed_unread_browser_frames() {
    // 生成不会触及真实登录会话的固定测试后缀。
    let session_id = browser_test_session();
    // 创建真正 browser 首实例 guard owner。
    let owner = BrowserSessionPipeOwner::create(session_id)
        // 当前用户应能建立受保护的内部 guard。
        .must("browser close owner should be created");
    // 取得受 owner 生命周期约束的 secondary factory。
    let factory = owner.secondary_factory();
    // 建立 secondary listener 已发布的通知。
    let (ready_tx, ready_rx) = mpsc::channel();
    // 建立 server 已写入两帧并释放 connection 所有者的通知。
    let (closed_tx, closed_rx) = mpsc::channel();
    // 在独立线程建立与生产 broker 相同的 PIPE_NOWAIT server 连接。
    let server_thread = thread::spawn(move || {
        // 通过真实 owner factory 创建 browser secondary listener。
        let listener = factory
            // factory 固定同名、DACL、预算与多实例边界。
            .create_listener()
            // owner live 时 secondary 必须可创建。
            .must("browser close secondary should be created");
        // 通知 client 真实 listener 已发布。
        ready_tx
            // 只传递无数据就绪事实。
            .send(())
            // 主线程必须仍在等待。
            .must("browser close readiness should be delivered");
        // 接受唯一 browser client 并在连接期固定 PIPE_NOWAIT。
        let connection = listener
            // 当前回归不请求取消。
            .accept(|| false)
            // 本机 client 必须能完成连接。
            .must("browser close secondary should accept");
        // 先写入 accepted 帧以覆盖真实 broker 两帧顺序。
        connection
            // 使用与生产 response transport 相同的有界非阻塞写入。
            .write_text_until(
                // 写入固定 accepted 内容。
                ACCEPTED_FRAME,
                // 写入不得越过固定单调预算。
                Instant::now() + THREAD_DEADLINE,
                // 当前回归不请求取消。
                || false,
            )
            // accepted 必须以完整 message 写入。
            .must("browser accepted frame should be written");
        // 紧接着写入 final，client 此时尚未读取任何帧。
        connection
            // 复用同一有界非阻塞 response 边界。
            .write_text_until(
                // 写入固定 final 内容。
                FINAL_FRAME,
                // 写入不得越过固定单调预算。
                Instant::now() + THREAD_DEADLINE,
                // 当前回归不请求取消。
                || false,
            )
            // final 必须以完整 message 写入。
            .must("browser final frame should be written");
        // 立即释放 server connection；RAII 析构只执行 CloseHandle。
        drop(connection);
        // 通知 client server handle 已在任何读取前关闭。
        closed_tx
            // 只传递无数据关闭事实。
            .send(())
            // client 必须仍持有 pipe 对端。
            .must("browser server close should be delivered");
    });
    // 等待真实 browser secondary listener 发布。
    ready_rx
        // 使用固定线程协调上限。
        .recv_timeout(THREAD_DEADLINE)
        // server 必须及时就绪。
        .must("browser close secondary should become ready");
    // 使用生产 browser endpoint 连接非阻塞 client。
    let client = ConnectedPipe::connect_for_until(
        // 只选择 browser-session 固定类别。
        FixedLocalEndpointKind::BrowserSession,
        // 使用同一测试 session 后缀。
        session_id,
        // 连接不得越过固定单调预算。
        Instant::now() + THREAD_DEADLINE,
        // 当前回归不请求取消。
        || false,
    )
    // client 必须连接业务 secondary 而非内部 guard。
    .must("browser close client should connect");
    // 等待 server 完整写入两帧并立即关闭本端 handle。
    closed_rx
        // 使用固定线程协调上限。
        .recv_timeout(THREAD_DEADLINE)
        // server 关闭必须在 client 首次读取前完成。
        .must("browser server should close before delayed reads");
    // 故意在 server drop 后继续延迟，确认不依赖析构同步窗口。
    thread::sleep(DELAYED_READ);
    // 在 server handle 已关闭后读取第一条 accepted 帧。
    let accepted = client
        // 使用固定绝对 deadline 读取完整 message。
        .read_text_until(Instant::now() + THREAD_DEADLINE, || false)
        // CloseHandle 不得丢弃已写入的 accepted。
        .must("accepted frame should survive server close");
    // 逐字比较第一条帧。
    assert_eq!(accepted, ACCEPTED_FRAME);
    // 继续在同一已断开 server 的对端上读取 final。
    let final_frame = client
        // 使用独立绝对 deadline 读取第二条完整 message。
        .read_text_until(Instant::now() + THREAD_DEADLINE, || false)
        // CloseHandle 不得丢弃已写入的终态 final。
        .must("final frame should survive server close");
    // 逐字比较终态帧。
    assert_eq!(final_frame, FINAL_FRAME);
    // 释放 client 作为该 pipe instance 的最后一个 handle。
    drop(client);
    // 回收已关闭 server handle 的线程。
    server_thread
        // 等待线程正常退出。
        .join()
        // 不允许后台线程泄漏或 panic。
        .must("browser close server should join");
    // 最后释放真正首实例 owner 与内部 guard。
    drop(owner);
}
