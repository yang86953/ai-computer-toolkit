//! 验证 browser PIPE_NOWAIT 连接的直接有界读取语义。

// 导入线程同步、独立线程与单调计时。
use std::{
    // 使用通道建立连接、延迟写入与关闭的确定同步点。
    sync::mpsc,
    // 在独立线程持有或写入 server peer。
    thread,
    // 为读取 deadline 与线程协调提供固定预算。
    time::{Duration, Instant},
};

// 导入父测试模块的唯一名称和测试解包帮助。
use super::{TestResultExt, test_pipe_name};
// 导入祖先 pipe Component 的私有测试连接类型。
use super::super::{ConnectedPipe, ServerPipe};

// 固定测试 pipe 的双向 message buffer 预算。
const PIPE_BUDGET_BYTES: usize = 32 * 1024;
// 固定空管道读取总预算。
const EMPTY_READ_DEADLINE: Duration = Duration::from_millis(150);
// 固定延迟写入时间，确保 client 先进入非阻塞读取。
const DELAYED_WRITE: Duration = Duration::from_millis(60);
// 固定线程同步上限，避免测试留下后台线程。
const THREAD_DEADLINE: Duration = Duration::from_secs(2);

// 验证 browser 非阻塞读取在空管道上按绝对 deadline 返回 TIMEOUT。
#[test]
fn browser_nonblocking_read_empty_pipe_times_out_within_deadline() {
    // 创建与生产 endpoint 隔离的唯一测试 pipe 名称。
    let name = test_pipe_name("browser-read-timeout");
    // 建立 server 已发布监听的同步通道。
    let (ready_tx, ready_rx) = mpsc::channel();
    // 建立 server 已接受 client 的同步通道。
    let (accepted_tx, accepted_rx) = mpsc::channel();
    // 建立受控关闭 server peer 的同步通道。
    let (release_tx, release_rx) = mpsc::channel();
    // 复制唯一名称到 server 所有者线程。
    let server_name = name.clone();
    // 在独立线程创建并持有不写入的 server peer。
    let server_thread = thread::spawn(move || {
        // 使用固定预算创建独立测试 endpoint。
        let server = ServerPipe::create_named_with_budgets(
            // 使用唯一测试名称。
            &server_name,
            // 固定 client 到 server 预算。
            PIPE_BUDGET_BYTES,
            // 固定 server 到 client 预算。
            PIPE_BUDGET_BYTES,
        )
        // 当前用户 DACL 应允许本机测试 endpoint 创建。
        .must("browser read timeout server should be created");
        // 通知 client endpoint 已发布。
        ready_tx
            // 只发送无数据就绪事实。
            .send(())
            // 主线程必须仍在等待。
            .must("browser read timeout readiness should be delivered");
        // 接受唯一 client。
        let connection = server
            // 当前测试不请求取消。
            .accept(|| false)
            // 本机 client 必须能连接。
            .must("browser read timeout server should accept");
        // 证明 server 已持有连接且不会写入。
        accepted_tx
            // 只发送无数据 acceptance 事实。
            .send(())
            // 主线程必须仍在等待。
            .must("browser read timeout acceptance should be delivered");
        // 等待 client 完成 timeout 断言后再释放 peer。
        release_rx
            // 使用固定上限避免线程无界等待。
            .recv_timeout(THREAD_DEADLINE)
            // 主线程必须显式释放 server。
            .must("browser read timeout server should be released");
        // 显式关闭 server 连接。
        drop(connection);
    });
    // 等待 endpoint 发布后再连接。
    ready_rx
        // 使用固定线程协调上限。
        .recv_timeout(THREAD_DEADLINE)
        // server 必须及时发布。
        .must("browser read timeout server should become ready");
    // 使用与生产 browser 相同的连接期 PIPE_NOWAIT 事实连接。
    let client = ConnectedPipe::connect_named_with_budgets_and_nonblocking_write_until(
        // 使用唯一测试名称。
        &name,
        // 固定读取预算。
        PIPE_BUDGET_BYTES,
        // 固定写入预算。
        PIPE_BUDGET_BYTES,
        // 为本机连接提供宽松预算。
        Instant::now() + THREAD_DEADLINE,
        // 当前测试不请求取消。
        || false,
    )
    // client 必须成功连接并安装 PIPE_NOWAIT。
    .must("browser read timeout client should connect");
    // 等待 server 证明内核连接已建立。
    accepted_rx
        // 使用固定线程协调上限。
        .recv_timeout(THREAD_DEADLINE)
        // server 必须先进入不写入状态。
        .must("browser read timeout server should accept client");
    // 记录空管道读取起点。
    let started = Instant::now();
    // 在唯一绝对 deadline 下直接读取空 PIPE_NOWAIT handle。
    let error = match client.read_text_until(started + EMPTY_READ_DEADLINE, || false) {
        // 空管道不得产生协议消息。
        Ok(_) => panic!("empty browser pipe unexpectedly returned a message"),
        // 保留结构化错误供断言。
        Err(error) => error,
    };
    // 空管道必须稳定映射为 TIMEOUT。
    assert_eq!(error.code, "TIMEOUT");
    // 保存实际读取耗时。
    let elapsed = started.elapsed();
    // 短轮询不得明显早于调用预算结束。
    assert!(elapsed >= Duration::from_millis(100));
    // 调度波动仍不得让读取明显越过绝对 deadline。
    assert!(elapsed <= Duration::from_millis(500));
    // 关闭 client 以释放本机 handle。
    drop(client);
    // 命令 server 线程退出。
    release_tx
        // 只发送无数据释放事实。
        .send(())
        // server 必须仍在等待释放。
        .must("browser read timeout server should receive release");
    // 回收已经被显式释放的 server 线程。
    server_thread
        // 等待线程退出。
        .join()
        // 不允许后台线程泄漏或 panic。
        .must("browser read timeout server should join");
}

// 验证 browser 非阻塞读取可在延迟写入后拼接完整多块 message。
#[test]
fn browser_nonblocking_read_delayed_write_returns_complete_frame() {
    // 创建与其他测试隔离的唯一 pipe 名称。
    let name = test_pipe_name("browser-read-delayed");
    // 构造超过单个读取块的完整 UTF-8 frame。
    let frame = "测".repeat(6 * 1024);
    // 为 server 线程复制期望 frame。
    let server_frame = frame.clone();
    // 建立 server 已发布监听的同步通道。
    let (ready_tx, ready_rx) = mpsc::channel();
    // 建立 server 已接受 client 的同步通道。
    let (accepted_tx, accepted_rx) = mpsc::channel();
    // 建立 client 已准备读取的同步通道。
    let (start_tx, start_rx) = mpsc::channel();
    // 建立 client 已完成完整读取的同步通道。
    let (release_tx, release_rx) = mpsc::channel();
    // 复制唯一名称到 server 所有者线程。
    let server_name = name.clone();
    // 在独立线程延迟写入完整 message。
    let server_thread = thread::spawn(move || {
        // 使用足以容纳多块 frame 的固定预算创建 endpoint。
        let server = ServerPipe::create_named_with_budgets(
            // 使用唯一测试名称。
            &server_name,
            // 固定 client 到 server 预算。
            PIPE_BUDGET_BYTES,
            // 固定 server 到 client 预算。
            PIPE_BUDGET_BYTES,
        )
        // 当前用户 DACL 应允许创建 endpoint。
        .must("browser delayed read server should be created");
        // 通知 client endpoint 已发布。
        ready_tx
            // 只发送无数据就绪事实。
            .send(())
            // 主线程必须仍在等待。
            .must("browser delayed read readiness should be delivered");
        // 接受唯一 client。
        let connection = server
            // 当前测试不请求取消。
            .accept(|| false)
            // 本机 client 必须能连接。
            .must("browser delayed read server should accept");
        // 通知 client 连接已经建立。
        accepted_tx
            // 只发送无数据 acceptance 事实。
            .send(())
            // 主线程必须仍在等待。
            .must("browser delayed read acceptance should be delivered");
        // 等待 client 即将进入读取循环。
        start_rx
            // 使用固定线程协调上限。
            .recv_timeout(THREAD_DEADLINE)
            // client 必须明确启动读取。
            .must("browser delayed read should be triggered");
        // 固定延迟确保 client 至少观察一次 ERROR_NO_DATA。
        thread::sleep(DELAYED_WRITE);
        // 写入一条超过固定读取块的完整 UTF-8 message。
        connection
            // 使用普通 server 同步写入边界。
            .write_text(&server_frame)
            // client 正在读取时写入必须完成。
            .must("browser delayed frame should be written");
        // 保持 server handle 存活直到 client 确认完整读取。
        release_rx
            // 使用固定线程协调上限。
            .recv_timeout(THREAD_DEADLINE)
            // client 必须显式释放 server peer。
            .must("browser delayed read server should be released");
        // 显式关闭已经完成读取的 server 连接。
        drop(connection);
    });
    // 等待 endpoint 发布后再连接。
    ready_rx
        // 使用固定线程协调上限。
        .recv_timeout(THREAD_DEADLINE)
        // server 必须及时发布。
        .must("browser delayed read server should become ready");
    // 使用与生产 browser 相同的连接期 PIPE_NOWAIT 事实连接。
    let client = ConnectedPipe::connect_named_with_budgets_and_nonblocking_write_until(
        // 使用唯一测试名称。
        &name,
        // 固定读取预算。
        PIPE_BUDGET_BYTES,
        // 固定写入预算。
        PIPE_BUDGET_BYTES,
        // 为本机连接提供宽松预算。
        Instant::now() + THREAD_DEADLINE,
        // 当前测试不请求取消。
        || false,
    )
    // client 必须成功连接并安装 PIPE_NOWAIT。
    .must("browser delayed read client should connect");
    // 等待 server 证明内核连接已建立。
    accepted_rx
        // 使用固定线程协调上限。
        .recv_timeout(THREAD_DEADLINE)
        // server 必须先接受 client。
        .must("browser delayed read server should accept client");
    // 通知 server 在固定延迟后写入。
    start_tx
        // 只发送无数据启动事实。
        .send(())
        // server 必须仍在等待启动。
        .must("browser delayed read server should receive trigger");
    // 在绝对 deadline 内读取延迟到达的完整 message。
    let received = client
        // 使用宽松但有界的总读取预算。
        .read_text_until(Instant::now() + Duration::from_secs(1), || false)
        // ERROR_NO_DATA 重试后必须成功。
        .must("browser delayed frame should be read");
    // 多个 ERROR_MORE_DATA 块必须拼成逐字完整 frame。
    assert_eq!(received, frame);
    // 通知 server client 已经取得完整 frame。
    release_tx
        // 只发送无数据释放事实。
        .send(())
        // server 必须仍在持有连接。
        .must("browser delayed read completion should be delivered");
    // 关闭 client 释放本机 handle。
    drop(client);
    // 回收完成写入的 server 线程。
    server_thread
        // 等待线程退出。
        .join()
        // 不允许后台线程泄漏或 panic。
        .must("browser delayed read server should join");
}
