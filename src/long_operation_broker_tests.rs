//! 验证长操作 broker 的接受边界、Query、取消与终态投影。

// 导入 fixture 文件、路径与唯一序列工具。
use std::{
    // 创建并清理精确测试目录。
    fs,
    // 保存 fixture 根路径。
    path::PathBuf,
    // 为并行测试分配唯一目录。
    sync::atomic::{AtomicU64, Ordering},
};

// 导入 JSON fixture 构造宏。
use serde_json::json;

// 导入被测 broker 私有入口。
use super::*;
// 导入原子 journal Component。
use crate::components::long_operation_journal::LongOperationJournal;

// 为同一测试进程分配唯一 fixture 序列。
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// 独占一个 broker registry fixture。
struct BrokerFixture {
    // 保存精确可恢复目录。
    path: PathBuf,
    // 保存被测异步任务 Module。
    tasks: LongOperationRecordingTasks,
    // 保存业务接受前 System 门禁。
    service: AppControlService,
}

// 构造并拥有 broker fixture。
impl BrokerFixture {
    // 创建空 registry。
    fn new(label: &str, now_ms: u64) -> Self {
        // 取得进程内唯一序列。
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        // 构造不会与生产 Known Folder 重叠的临时根。
        let path = std::env::temp_dir().join(format!(
            // 固定测试前缀、进程、标签与序列。
            "act-long-operation-broker-{}-{label}-{sequence}",
            // 只把 PID 用于测试目录隔离。
            std::process::id(),
        ));
        // 清理同名残留测试目录。
        let _ = fs::remove_dir_all(&path);
        // 创建真实父目录。
        fs::create_dir(&path)
            // 失败时保留测试诊断。
            .unwrap_or_else(|error| panic!("broker fixture creation failed: {error}"));
        // 打开原子 journal。
        let journal = LongOperationJournal::open(&path.join("journal"))
            // 失败时保留测试诊断。
            .unwrap_or_else(|error| panic!("broker journal open failed: {error:?}"));
        // 打开空 registry。
        let registry = LongOperationRegistry::open(journal, now_ms)
            // 失败时保留测试诊断。
            .unwrap_or_else(|error| panic!("broker registry open failed: {error:?}"));
        // 返回 fixture 所有者。
        Self {
            // 保存精确测试目录。
            path,
            // 由任务 Module 接管 registry。
            tasks: LongOperationRecordingTasks::new(registry),
            // 使用生产 System 门禁装配。
            service: AppControlService::new(),
        }
    }
}

// 作用域结束时删除精确 fixture。
impl Drop for BrokerFixture {
    // 回收唯一测试目录。
    fn drop(&mut self) {
        // 先持久取消并回收当前 fixture 拥有的 worker。
        let _ = self.tasks.shutdown();
        // 删除仅由当前 fixture 构造的精确路径。
        let _ = fs::remove_dir_all(&self.path);
    }
}

// 解析一条测试请求。
fn request(value: Value) -> LongOperationBrokerRequest {
    // 序列化受控 JSON fixture。
    let text = serde_json::to_string(&value)
        // fixture 理论上必须可序列化。
        .unwrap_or_else(|error| panic!("request fixture serialization failed: {error}"));
    // 使用生产 parser 验证请求。
    LongOperationBrokerRequest::parse(&text)
        // fixture 必须符合冻结协议。
        .unwrap_or_else(|error| panic!("request fixture parsing failed: {error:?}"))
}

// 构造 handle-only 请求。
fn handle_request_fixture(action: &str, operation_id: &str) -> LongOperationBrokerRequest {
    // 使用固定 canonical nonce 与 handle。
    request(json!({
        // 固定 broker 版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用 canonical 请求关联值。
        "requestNonce": "0123456789abcdef0123456789abcdef",
        // 传入受测 action。
        "action": action,
        // 传入 canonical operation handle。
        "operationId": operation_id
    }))
}

// 验证 transport 前失败不回显 nonce，而 envelope 后失败只回显 canonical nonce。
#[test]
fn protocol_rejections_preserve_transport_acceptance_boundary() {
    // 非法版本在 transport 接受前失败。
    let envelope_failure = match LongOperationBrokerRequest::parse(
        // 即使输入携带 nonce 也不得回显。
        r#"{"contractVersion":"wrong","requestNonce":"0123456789abcdef0123456789abcdef","action":"status","operationId":"s2:o:0000000000000001"}"#,
    ) {
        // 成功表示测试输入不再覆盖拒绝边界。
        Ok(_) => panic!("invalid envelope unexpectedly parsed"),
        // 保存预期协议失败。
        Err(failure) => failure,
    };
    // transport 前失败不保存 nonce。
    assert_eq!(envelope_failure.request_nonce(), None);
    // transport 前失败明确未接受。
    assert!(!envelope_failure.transport_accepted());

    // 缺失 handle 在固定 envelope 接受后失败。
    let semantic_failure = match LongOperationBrokerRequest::parse(
        // 只保留合法版本、nonce 与 action。
        r#"{"contractVersion":"act/long-operation-broker/v1","requestNonce":"0123456789abcdef0123456789abcdef","action":"status"}"#,
    ) {
        // 成功表示测试输入不再覆盖拒绝边界。
        Ok(_) => panic!("invalid handle query unexpectedly parsed"),
        // 保存预期语义失败。
        Err(failure) => failure,
    };
    // 语义失败只保存经过验证的 nonce。
    assert_eq!(
        semantic_failure.request_nonce(),
        Some("0123456789abcdef0123456789abcdef")
    );
    // 固定 envelope 已被接受。
    assert!(semantic_failure.transport_accepted());
}

// 验证非法 submit 在业务接受前关闭且不建立 registry 记录。
#[test]
fn invalid_submit_is_rejected_before_business_acceptance() {
    // 创建空 registry。
    let mut fixture = BrokerFixture::new("submit-disabled", 1_000);
    // 构造已经确认且协议合法的 submit。
    let request = request(json!({
        // 固定 broker 版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用 canonical nonce。
        "requestNonce": "11111111111111111111111111111111",
        // 固定 submit Command。
        "action": "submit",
        // 固定首个 capability。
        "capabilityId": "window.record@1",
        // 使用 opaque 窗口目标。
        "target": {"sessionId": "s2:w:0000000000000001"},
        // 使用合法封闭录制输入形状。
        "input": {"outputPath": "relative.mp4", "frameRate": 30},
        // 显式确认副作用。
        "confirmed": true
    }));
    // 通过生产 broker 路由处理请求。
    let response = handle_request(&mut fixture.tasks, &fixture.service, &request, 1_001)
        // 参数拒绝应以业务响应而非进程错误返回。
        .unwrap_or_else(|error| panic!("invalid submit handling failed: {error:?}"));
    // transport 已被接受。
    assert_eq!(response["transportAccepted"], true);
    // 业务尚未建立任务。
    assert_eq!(response["businessAccepted"], false);
    // 使用稳定参数错误码。
    assert_eq!(response["error"]["code"], "INVALID_ARGUMENT");
    // 禁止在错误响应后留下任务或 worker 所有权。
    assert_eq!(fixture.tasks.registry_for_test().tracked_count(), 0);
}

// 验证合法 submit 原子建立 handle 并立即返回业务接受。
#[test]
fn valid_submit_returns_operation_handle_before_worker_terminal() {
    // 创建空 broker 代际与真实临时 journal。
    let mut fixture = BrokerFixture::new("submit-accepted", 1_100);
    // 在 fixture 已存在父目录下构造尚不存在的输出文件。
    let output_path = fixture.path.join("capture.mp4");
    // 构造已经确认且业务预检合法的 submit。
    let request = request(json!({
        // 固定 broker 版本。
        "contractVersion": CONTRACT_VERSION,
        // 使用 canonical nonce。
        "requestNonce": "22222222222222222222222222222222",
        // 固定 submit Command。
        "action": "submit",
        // 固定首个 capability。
        "capabilityId": "window.record@1",
        // 使用 canonical 但不会命中真实窗口的目标。
        "target": {"sessionId": "s2:w:0000000000000001"},
        // 使用纯验证可接受的绝对 MP4 路径。
        "input": {
            // 转换 fixture 路径为 JSON 文本。
            "path": output_path.to_string_lossy(),
            // 使用最小录制时长。
            "durationMs": 1000,
            // 使用最小帧率。
            "fps": 1,
            // 使用最小 worker deadline。
            "timeoutMs": 250
        },
        // 显式确认副作用。
        "confirmed": true
    }));
    // 通过生产 broker 路由处理 submit。
    let response = handle_request(&mut fixture.tasks, &fixture.service, &request, 1_101)
        // 业务接受必须返回正常响应。
        .unwrap_or_else(|error| panic!("valid submit handling failed: {error:?}"));
    // transport 已接受完整 frame。
    assert_eq!(response["transportAccepted"], true);
    // registry 已经原子建立业务事实。
    assert_eq!(response["businessAccepted"], true);
    // 首个公开快照固定为 accepted。
    assert_eq!(response["operation"]["status"], "accepted");
    // 返回 canonical operation handle。
    let operation_id = response["operation"]["operationId"]
        // 只接受字符串 handle。
        .as_str()
        // 缺失时明确测试失败。
        .unwrap_or_else(|| panic!("accepted submit did not return an operation handle"))
        // 克隆供关闭后查询。
        .to_owned();
    // 核对固定 operation opaque 类别。
    assert!(operation_id.starts_with("s2:o:"));
    // broker 关闭必须持久取消并回收测试 worker。
    fixture
        // 使用被测任务 Module。
        .tasks
        // 执行可靠关闭。
        .shutdown()
        // fixture journal 必须保持可写。
        .unwrap_or_else(|error| panic!("accepted submit shutdown failed: {error:?}"));
    // 查询 worker 收敛后的持久终态。
    let terminal = fixture
        // 使用被测 status 入口。
        .tasks
        // 读取同一 operation handle。
        .status(&operation_id, current_unix_milliseconds().unwrap_or(1_102))
        // 终态必须持续可查询。
        .unwrap_or_else(|error| panic!("accepted submit terminal status failed: {error:?}"));
    // 已回收 worker 不得留下非终态。
    assert!(terminal.terminal());
}

// 验证 status Query 与 cancel Command 使用同一持久记录且取消幂等。
#[test]
fn status_and_cancel_share_durable_idempotent_lifecycle() {
    // 创建空 registry。
    let mut fixture = BrokerFixture::new("status-cancel", 2_000);
    // 固定受测 operation handle。
    let operation_id = "s2:o:000000000000002a";
    // 建立业务已接受记录。
    fixture
        // 取得测试 registry 锁。
        .tasks
        // 使用只限测试的夹具入口。
        .registry_for_test()
        // 固定 capability 与时刻。
        .accept(operation_id, "window.record@1", 2_001)
        // fixture 接受必须成功。
        .unwrap_or_else(|error| panic!("fixture accept failed: {error:?}"));

    // 查询初始状态。
    let status = handle_request(
        // 传入唯一 registry。
        &mut fixture.tasks,
        // 传入 System 预检边界。
        &fixture.service,
        // 构造 handle-only Query。
        &handle_request_fixture("status", operation_id),
        // 使用查询时刻。
        2_002,
    )
    // Query 必须成功。
    .unwrap_or_else(|error| panic!("status handling failed: {error:?}"));
    // 初始状态为 accepted。
    assert_eq!(status["operation"]["status"], "accepted");
    // 非终态到期时间必须为 null。
    assert_eq!(status["operation"]["expiresAt"], Value::Null);

    // 首次请求取消。
    let cancelled = handle_request(
        // 传入同一 registry。
        &mut fixture.tasks,
        // 传入 System 预检边界。
        &fixture.service,
        // 构造 cancel Command。
        &handle_request_fixture("cancel", operation_id),
        // 使用单调时刻。
        2_003,
    )
    // 首次取消必须成功。
    .unwrap_or_else(|error| panic!("cancel handling failed: {error:?}"));
    // 状态变为 cancel-requested。
    assert_eq!(cancelled["operation"]["status"], "cancel-requested");
    // 取消事实被公开。
    assert_eq!(cancelled["operation"]["cancelRequested"], true);

    // 记录首次取消后的修订号。
    let revision = fixture
        // 取得测试 registry 锁。
        .tasks
        // 使用只限测试的夹具入口。
        .registry_for_test()
        // 使用后续时刻。
        .status(operation_id, 2_004)
        // 查询必须成功。
        .unwrap_or_else(|error| panic!("post-cancel status failed: {error:?}"))
        // 保存修订号。
        .revision();
    // 重复取消仍返回成功快照。
    let repeated = handle_request(
        // 传入同一 registry。
        &mut fixture.tasks,
        // 传入 System 预检边界。
        &fixture.service,
        // 重复同一 Command。
        &handle_request_fixture("cancel", operation_id),
        // 使用后续时刻。
        2_005,
    )
    // 幂等取消必须成功。
    .unwrap_or_else(|error| panic!("repeated cancel failed: {error:?}"));
    // 重复取消不改变公开状态。
    assert_eq!(repeated["operation"]["status"], "cancel-requested");
    // 重复取消不得增加 revision。
    assert_eq!(
        fixture
            // 取得测试 registry 锁。
            .tasks
            // 使用只限测试的夹具入口。
            .registry_for_test()
            // 使用后续时刻。
            .status(operation_id, 2_006)
            // 查询必须成功。
            .unwrap_or_else(|error| panic!("final status failed: {error:?}"))
            // 读取修订号。
            .revision(),
        revision
    );
}

// 验证未知 handle 与 completed 终态得到相互独立的公开投影。
#[test]
fn status_distinguishes_unknown_handle_and_completed_result() {
    // 创建空 registry。
    let mut fixture = BrokerFixture::new("terminal", 3_000);
    // 查询不存在的 canonical handle。
    let missing = handle_request(
        // 传入空 registry。
        &mut fixture.tasks,
        // 传入 System 预检边界。
        &fixture.service,
        // 构造合法 Query。
        &handle_request_fixture("status", "s2:o:00000000000000ff"),
        // 使用查询时刻。
        3_001,
    )
    // not-found 是正常 broker 响应。
    .unwrap_or_else(|error| panic!("missing status handling failed: {error:?}"));
    // transport 已被接受。
    assert_eq!(missing["transportAccepted"], true);
    // 未命中不建立业务接受。
    assert_eq!(missing["businessAccepted"], false);
    // 使用统一零命中错误码。
    assert_eq!(missing["error"]["code"], "OPERATION_NOT_FOUND");

    // 固定 completed handle。
    let operation_id = "s2:o:0000000000000007";
    // 原子接受任务。
    fixture
        // 取得测试 registry 锁。
        .tasks
        // 使用只限测试的夹具入口。
        .registry_for_test()
        // 建立 accepted 记录。
        .accept(operation_id, "window.record@1", 3_002)
        // 接受必须成功。
        .unwrap_or_else(|error| panic!("terminal fixture accept failed: {error:?}"));
    // 记录 dispatch 已开始。
    fixture
        // 取得测试 registry 锁。
        .tasks
        // 使用只限测试的夹具入口。
        .registry_for_test()
        // 执行不可逆迁移。
        .start_dispatch(operation_id, 3_003)
        // dispatch 必须成功。
        .unwrap_or_else(|error| panic!("terminal fixture dispatch failed: {error:?}"));
    // 以完整结果终结任务。
    fixture
        // 取得测试 registry 锁。
        .tasks
        // 使用只限测试的夹具入口。
        .registry_for_test()
        // 保存受控 JSON 结果。
        .complete(operation_id, json!({"recordingId": "fixture"}), 3_004)
        // completion 必须成功。
        .unwrap_or_else(|error| panic!("terminal fixture completion failed: {error:?}"));

    // 查询 completed 状态。
    let completed = handle_request(
        // 传入同一 registry。
        &mut fixture.tasks,
        // 传入 System 预检边界。
        &fixture.service,
        // 构造 status Query。
        &handle_request_fixture("status", operation_id),
        // 使用到期前时刻。
        3_005,
    )
    // Query 必须成功。
    .unwrap_or_else(|error| panic!("completed status failed: {error:?}"));
    // 状态明确为 completed。
    assert_eq!(completed["operation"]["status"], "completed");
    // 返回完整结果而非摘要或路径。
    assert_eq!(
        completed["operation"]["result"],
        json!({"recordingId": "fixture"})
    );
    // 终态到期时间使用 UTC RFC 3339 文本。
    assert!(
        completed["operation"]["expiresAt"]
            // 读取字符串。
            .as_str()
            // 固定 UTC 后缀。
            .is_some_and(|value| value.ends_with('Z'))
    );
}
