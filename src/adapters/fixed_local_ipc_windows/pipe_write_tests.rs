//! 验证固定 named-pipe 的有界非阻塞文本写入在对端不读时超时。

// 导入线程同步、独立线程与单调计时类型。
use std::{
    // 使用通道建立连接和关闭的确定同步点。
    sync::mpsc,
    // 让 server peer 独立持有连接但不读取。
    thread,
    // 为 write deadline 和线程回收提供有界计时。
    time::{Duration, Instant},
};

// 导入父测试模块的唯一名称和测试解包帮助。
use super::{TestResultExt, test_pipe_name};
// 导入祖先 pipe Component 的私有测试连接类型。
use super::super::{ConnectedPipe, ServerPipe};

// 固定小型 pipe 双向 buffer 与文本 frame 预算。
const PIPE_BUDGET_BYTES: usize = 4 * 1024;
// 固定非阻塞写入最终 deadline，覆盖系统调度小幅波动。
const WRITE_DEADLINE: Duration = Duration::from_millis(250);
// 固定 server 线程回收等待上限。
const THREAD_SHUTDOWN_DEADLINE: Duration = Duration::from_millis(500);
// 限制填满底层 message buffer 的尝试次数，避免测试无界。
const MAXIMUM_FILL_ATTEMPTS: usize = 32;

// 验证未读取 peer 填满 message buffer 后，写入会在 deadline 内返回 TIMEOUT 并可回收线程。
#[test]
fn write_text_until_times_out_when_peer_does_not_read() {
    // 创建与生产 endpoint 隔离的唯一测试 pipe 名称。
    let name = test_pipe_name("write-timeout");
    // 建立 server 已发布监听的同步通道。
    let (ready_tx, ready_rx) = mpsc::channel();
    // 建立 server 已接受 client 的同步通道。
    let (accepted_tx, accepted_rx) = mpsc::channel();
    // 建立受控关闭 server peer 的同步通道。
    let (release_tx, release_rx) = mpsc::channel();
    // 建立 server 线程已经退出的回收证据通道。
    let (finished_tx, finished_rx) = mpsc::channel();
    // 复制唯一名称到唯一 server 所有者线程。
    let server_name = name.clone();
    // 在独立线程创建并持有不读取的 server peer。
    let server_thread = thread::spawn(move || {
        // 使用小型固定 buffer 创建独立测试 endpoint。
        let server = ServerPipe::create_named_with_budgets(
            // 使用唯一测试名称。
            &server_name,
            // 限制 client 到 server 的未读取 request buffer。
            PIPE_BUDGET_BYTES,
            // 保持反向 buffer 同样受限。
            PIPE_BUDGET_BYTES,
        )
        // 当前用户 DACL 应允许本机测试 endpoint 创建。
        .must("write timeout server should be created");
        // 通知 client endpoint 已发布。
        ready_tx
            // 只发送无数据就绪事实。
            .send(())
            // 主线程必须仍在等待。
            .must("write timeout readiness should be delivered");
        // 接受唯一 client。
        let connection = server
            // 当前测试不请求取消。
            .accept(|| false)
            // 本机 client 必须能连接。
            .must("write timeout server should accept");
        // 证明 server 已持有连接但尚未读取。
        accepted_tx
            // 只发送无数据 acceptance 事实。
            .send(())
            // 主线程必须仍在等待。
            .must("write timeout acceptance should be delivered");
        // 只等待主线程释放，不调用任何 read API。
        release_rx
            // 线程回收本身必须有界，避免测试泄漏。
            .recv_timeout(Duration::from_secs(2))
            // 主线程必须在断言后释放 server peer。
            .must("write timeout server should be released");
        // 显式关闭未读取 server peer。
        drop(connection);
        // 发布线程已退出的确定证据。
        finished_tx
            // 只发送无数据完成事实。
            .send(())
            // 主线程必须仍在等待回收证据。
            .must("write timeout completion should be delivered");
    });
    // 等待 endpoint 发布，避免 client 在 listener 前连接。
    ready_rx
        // 通道关闭表示 server 线程提前失败。
        .recv_timeout(Duration::from_secs(2))
        // endpoint 必须及时发布。
        .must("write timeout server should become ready");
    // 使用同样的小型双向预算连接测试 endpoint。
    let client = ConnectedPipe::connect_named_with_budgets_and_nonblocking_write_until(
        // 使用唯一测试名称。
        &name,
        // client 读取 server response 的预算。
        PIPE_BUDGET_BYTES,
        // client 写入 server request 的预算。
        PIPE_BUDGET_BYTES,
        // 限制本机连接等待。
        Instant::now() + Duration::from_secs(2),
        // 当前测试不请求取消。
        || false,
    )
    // client 必须成功连接。
    .must("write timeout client should connect");
    // 等待 server 证明连接已建立且不读取。
    accepted_rx
        // 通道关闭表示 server 线程提前失败。
        .recv_timeout(Duration::from_secs(2))
        // server 必须先进入不读取状态。
        .must("write timeout server should accept client");
    // 构造刚好等于固定 write budget 的完整文本 frame。
    let frame = "x".repeat(PIPE_BUDGET_BYTES);
    // 保存最终未读取回压所消耗的单次等待时间。
    let mut timeout_elapsed = None;
    // 有界重复发送，以填满系统可能放大的最小 pipe buffer。
    for _ in 0..MAXIMUM_FILL_ATTEMPTS {
        // 在每次写入前冻结唯一小 deadline。
        let started = Instant::now();
        // 尝试向不读取 peer 发送一条完整 message。
        match client.write_text_until(&frame, started + WRITE_DEADLINE, || false) {
            // 尚有 buffer 空间时继续填充。
            Ok(()) => continue,
            // 只接受回压在 deadline 后返回稳定 TIMEOUT。
            Err(error) if error.error_code() == "TIMEOUT" => {
                // 全帧非阻塞空写超时必须保留 client-local 未派发事实。
                assert!(!error.delivery_may_have_occurred());
                // 保存本次有界等待证据。
                timeout_elapsed = Some(started.elapsed());
                // 已得到唯一目标结果，不再继续写入。
                break;
            }
            // 其他错误不能证明 write deadline 契约。
            Err(error) => panic!("write backpressure returned unexpected error: {error:?}"),
        }
    }
    // 有界填充必须观测到 buffer 已满后的 timeout。
    let timeout_elapsed = timeout_elapsed.expect("unread peer should fill the bounded pipe buffer");
    // 非阻塞回压不得在没有等待时立即误报 timeout。
    assert!(timeout_elapsed >= Duration::from_millis(100));
    // 短轮询调度波动仍不得明显越过 caller deadline。
    assert!(timeout_elapsed <= Duration::from_millis(500));
    // 关闭 client，释放所有待写的本机 handle。
    drop(client);
    // 明确命令 server peer 退出而不是留下后台线程。
    release_tx
        // 只发送无数据释放信号。
        .send(())
        // server 必须仍在等待受控释放。
        .must("write timeout server should receive release");
    // 在固定短上限内取得 server 退出证据。
    finished_rx
        // 超时表示后台线程未被正常回收。
        .recv_timeout(THREAD_SHUTDOWN_DEADLINE)
        // server peer 必须及时退出。
        .must("write timeout server thread should finish");
    // join 只回收已通过完成通道确认的线程。
    server_thread
        // 线程 panic 必须使测试失败。
        .join()
        // 不允许残留后台线程。
        .must("write timeout server thread should join");
}

// 验证已过期绝对 deadline 在任何 WriteFile 前返回 TIMEOUT 且 peer 无数据可读。
#[test]
fn browser_nonblocking_write_expired_deadline_does_not_dispatch() {
    // 创建与生产 endpoint 隔离的唯一测试 pipe 名称。
    let name = test_pipe_name("write-expired");
    // 建立 server 已发布监听的同步通道。
    let (ready_tx, ready_rx) = mpsc::channel();
    // 建立 server 已接受 client 的同步通道。
    let (accepted_tx, accepted_rx) = mpsc::channel();
    // 建立 client 已完成过期写调用的同步通道。
    let (attempted_tx, attempted_rx) = mpsc::channel();
    // 复制唯一名称到 server 所有者线程。
    let server_name = name.clone();
    // 在独立线程验证 peer 没有收到任何 message 字节。
    let server_thread = thread::spawn(move || {
        // 使用小型固定 buffer 创建独立测试 endpoint。
        let server = ServerPipe::create_named_with_budgets(
            // 使用唯一测试名称。
            &server_name,
            // 限制 client 到 server 的 request buffer。
            PIPE_BUDGET_BYTES,
            // 保持反向 buffer 同样受限。
            PIPE_BUDGET_BYTES,
        )
        // 当前用户 DACL 应允许本机测试 endpoint 创建。
        .must("expired write server should be created");
        // 通知 client endpoint 已发布。
        ready_tx
            // 只发送无数据就绪事实。
            .send(())
            // 主线程必须仍在等待。
            .must("expired write readiness should be delivered");
        // 接受唯一 client。
        let connection = server
            // 当前测试不请求取消。
            .accept(|| false)
            // 本机 client 必须能连接。
            .must("expired write server should accept");
        // 证明 server 已持有连接。
        accepted_tx
            // 只发送无数据 acceptance 事实。
            .send(())
            // 主线程必须仍在等待。
            .must("expired write acceptance should be delivered");
        // 等待 client 完成过期调用再观察 pipe。
        attempted_rx
            // 使用固定上限避免线程无界等待。
            .recv_timeout(Duration::from_secs(2))
            // client 必须发布调用完成事实。
            .must("expired write attempt should be observed");
        // 在短预算内读取 peer，任何成功都表示过期调用进入了 WriteFile。
        let error = match connection.read_text_until(
            // 只留足够时间证明 pipe 仍为空。
            Instant::now() + Duration::from_millis(100),
            // 当前测试不请求取消。
            || false,
        ) {
            // peer 不得收到过期调用的 frame。
            Ok(_) => panic!("expired write unexpectedly dispatched a message"),
            // 保留结构化错误供断言。
            Err(error) => error,
        };
        // 空 peer 必须以 TIMEOUT 证明无 message 可读。
        assert_eq!(error.code, "TIMEOUT");
    });
    // 等待 endpoint 发布后再连接。
    ready_rx
        // 使用固定线程协调上限。
        .recv_timeout(Duration::from_secs(2))
        // server 必须及时发布。
        .must("expired write server should become ready");
    // 使用与 browser 相同的创建期 PIPE_NOWAIT 事实连接。
    let client = ConnectedPipe::connect_named_with_budgets_and_nonblocking_write_until(
        // 使用唯一测试名称。
        &name,
        // client 读取 server response 的预算。
        PIPE_BUDGET_BYTES,
        // client 写入 server request 的预算。
        PIPE_BUDGET_BYTES,
        // 为本机连接提供宽松预算。
        Instant::now() + Duration::from_secs(2),
        // 当前测试不请求取消。
        || false,
    )
    // client 必须成功连接。
    .must("expired write client should connect");
    // 等待 server 证明连接已建立。
    accepted_rx
        // 使用固定线程协调上限。
        .recv_timeout(Duration::from_secs(2))
        // server 必须先接受 client。
        .must("expired write server should accept client");
    // 构造明确早于当前时刻的绝对 deadline。
    let expired_deadline = Instant::now()
        // 向过去移动一毫秒以排除时钟边界竞态。
        .checked_sub(Duration::from_millis(1))
        // 单调时钟应支持这一微小回退。
        .expect("expired write deadline should be representable");
    // 尝试用已过期 deadline 写入非空 frame。
    let error = match client.write_text_until("expired", expired_deadline, || false) {
        // 过期调用不得完成写入。
        Ok(()) => panic!("expired write unexpectedly succeeded"),
        // 保留有界写入失败分类。
        Err(error) => error,
    };
    // 过期调用必须返回稳定 TIMEOUT。
    assert_eq!(error.error_code(), "TIMEOUT");
    // 未进入 WriteFile 时必须保持可证明未派发。
    assert!(!error.delivery_may_have_occurred());
    // 通知 server 开始验证 pipe 仍为空。
    attempted_tx
        // 只发送无数据完成事实。
        .send(())
        // server 必须仍在等待该事实。
        .must("expired write completion should be delivered");
    // 回收完成空管道断言的 server 线程。
    server_thread
        // 等待线程退出。
        .join()
        // 不允许后台线程泄漏或 panic。
        .must("expired write server should join");
    // 最后关闭 client handle。
    drop(client);
}
