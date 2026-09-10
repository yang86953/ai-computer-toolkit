//! 覆盖不启动浏览器的 broker close Module 生命周期边界。

// 导入单命令总预算。
use std::time::Duration;

// 导入无原生资源的测试进程 owner。
use crate::components::{
    // 导入测试用后台 close task。
    browser_session_close_task::BrowserSessionCloseTask,
    // 导入无原生资源的测试进程 owner。
    browser_session_process::BrowserSessionProcess,
};

// 导入同一 Module 的私有状态。
use super::{BrowserSessionEntry, BrowserSessionModule, MAXIMUM_LIVE_SESSIONS};

// 验证 broker close 在后台回收完成前立即使 session stale。
#[test]
fn broker_close_removes_session_before_waiting_for_reap() {
    // 建立空 Module。
    let mut module = BrowserSessionModule::new();
    // 构造固定测试 session identity。
    let session_id = "s2:bs:00000000000000000000000000000000".to_owned();
    // 插入不启动 worker 的 live registry 记录。
    module.sessions.insert(
        // 使用会话身份作为唯一键。
        session_id.clone(),
        // 构造无页面的测试会话。
        BrowserSessionEntry {
            // 使用无原生资源的测试进程 owner。
            process: BrowserSessionProcess::empty_for_test(session_id.clone()),
            // 使用不参与测试的固定 nonce。
            open_nonce: "n2:fixture".to_owned(),
            // 尚未建立页面。
            page: None,
        },
    );
    // 以零剩余预算进入 broker close，禁止等待后台回收。
    let _ = module
        // 调用 broker 专用路径。
        .close_for_broker(&session_id, Duration::ZERO, || false)
        // 空资源任务仍必须可被安全接管。
        .expect("broker close must accept the live fixture session");
    // 移除在创建任务前发生，故此刻已无法再通过预检。
    let error = module
        // 复用公开预检验证 stale 投影。
        .prepare_close(&session_id)
        // 会话必须立即 stale。
        .expect_err("accepted close must immediately stale the session");
    // 保持冻结 stale 错误码。
    assert_eq!(error.code, "STALE_SESSION");
}

// 验证 stale 但尚未完成的后台 close 仍占用 Module 固定资源配额。
#[test]
fn pending_close_tasks_count_toward_open_capacity() {
    // 建立空 Module。
    let mut module = BrowserSessionModule::new();
    // 填满仍由 Module 持有的后台 close task 配额。
    for _ in 0..MAXIMUM_LIVE_SESSIONS {
        // 使用短纯延迟避免接触真实 worker。
        module
            // 测试直接模拟已 stale 但尚未被清理的资源。
            .pending_close_tasks
            // 保留 Module 对每一个 task 的唯一所有权。
            .push(BrowserSessionCloseTask::start_for_test(
                // 使用足够长的纯延迟，确保容量预检前任务仍未完成。
                Duration::from_millis(100),
            ));
    }
    // 新 open 预检不得绕过仍占资源的异步回收。
    let error = module
        // 只读取 Module 自有资源计数，不启动 worker。
        .prepare_open()
        // 配额已满必须拒绝。
        .expect_err("pending close tasks must occupy the open capacity");
    // 保持冻结容量错误码。
    assert_eq!(error.code, "BROWSER_SESSION_REGISTRY_FULL");
}

// 验证已完成的后台 close task 会在 open 预检中非阻塞释放容量。
#[test]
fn completed_close_tasks_are_reaped_before_open_capacity_check() {
    // 建立空 Module。
    let mut module = BrowserSessionModule::new();
    // 填满已经发布完成事实的后台任务集合。
    for _ in 0..MAXIMUM_LIVE_SESSIONS {
        // 插入不持有线程或浏览器资源的已完成任务。
        module
            // 直接构造 Module 唯一持有的 stale 回收记录。
            .pending_close_tasks
            // 预先发布完成事实以覆盖非阻塞清理分支。
            .push(BrowserSessionCloseTask::completed_for_test());
    }
    // prepare_open 必须先清理全部完成任务，再计算容量。
    module
        // 不启动 worker，只验证 Module 自有回收与容量边界。
        .prepare_open()
        // 已完成任务不得持续占用新会话配额。
        .expect("completed close tasks must release open capacity");
    // 已完成任务必须从 Module 所有权集合移除。
    assert!(module.pending_close_tasks.is_empty());
}
