//! 为固定集成测试编排 browser-session broker 的 raw wire 恢复场景。

// 导入唯一安全 stdout 输出。
use std::io::Write;
// 导入固定 worker 回收观察等待。
use std::thread;
// 导入单调 deadline 与短暂本地观察等待。
use std::time::{Duration, Instant};

// 导入固定 JSON 输出构造器。
use serde_json::json;

// 导入固定认证连接 Adapter、严格协议值与统一应用错误边界。
use crate::{
    // 只建立生产固定 pipe 的认证 raw 连接。
    adapters::browser_session_broker_windows_fixture::{
        // 保存不泄漏 pipe 的已认证连接。
        CertifiedBrokerFixtureConnection,
        // 建立并完成 server-first ready 的固定连接。
        connect_ready,
    },
    // 借用严格协议的 epoch、结果、取消状态与 wire 编解码器。
    components::browser_session_broker_protocol::{
        // 借用封闭 final outcome 与成功投影。
        response::{BrowserSessionBrokerOutcome, BrowserSessionBrokerSuccess},
        // 借用 live epoch 与取消 receipt 状态。
        state::{BrowserSessionBrokerCancelStatus, BrowserSessionBrokerEpoch},
        // 借用同源 request/cancel builder 与严格 decoder。
        wire::{
            // 构造严格 cancel frame。
            BrowserSessionBrokerCancelFrame,
            // 构造严格 confirmed request frame。
            BrowserSessionBrokerRequestFrame,
            // 解码 request-bound cancel receipt。
            decode_cancel_receipt,
            // 解码 request-bound accepted/final。
            decode_response,
        },
    },
    // 借用统一安全错误类型。
    domain::{AppControlError, AppResult},
};

// 固定认证、写入和完整 request 观察的总预算。
const EXCHANGE_TIMEOUT_MS: u32 = 5_000;
// ready worker accepted 后的固定延迟小于该值。
const LOCAL_READ_TIMEOUT: Duration = Duration::from_millis(80);
// 固定同 nonce replay 场景的 canonical request nonce。
const REPLAY_NONCE: &str = "11111111111111111111111111111111";
// 固定 replay 场景 close 的独立 canonical request nonce。
const REPLAY_CLOSE_NONCE: &str = "66666666666666666666666666666666";
// 固定 tombstone 目标的 canonical request nonce。
const TOMBSTONE_TARGET_NONCE: &str = "22222222222222222222222222222222";
// 固定 tombstone cancel 的 canonical request nonce。
const TOMBSTONE_CANCEL_NONCE: &str = "33333333333333333333333333333333";
// 固定断连后 attach 场景的 canonical request nonce。
const DISCONNECT_NONCE: &str = "44444444444444444444444444444444";
// 固定断连场景 close 的独立 canonical request nonce。
const DISCONNECT_CLOSE_NONCE: &str = "77777777777777777777777777777777";
// 固定本地读超时后 attach 场景的 canonical request nonce。
const READ_TIMEOUT_NONCE: &str = "55555555555555555555555555555555";
// 固定本地读超时场景 close 的独立 canonical request nonce。
const READ_TIMEOUT_CLOSE_NONCE: &str = "88888888888888888888888888888888";
// 固定仅供异义冲突验证使用的公开 opaque session。
const CONFLICT_SESSION_ID: &str = "s2:bs:00000000000000000000000000000000";
// 固定每个场景收敛后的 worker 回收观察窗口。
const WORKER_SETTLE_DELAY: Duration = Duration::from_millis(120);

// 构造不回显 transport、epoch、nonce 或 frame 的统一失败。
fn fixture_failed() -> AppControlError {
    // 返回固定公开错误码和不含内部事实的说明。
    AppControlError::new(
        "FIXTURE_FAILED",
        "The fixed broker protocol fixture failed.",
    )
}

// 将内部严格 codec 或断言失败闭合为唯一安全 fixture 错误。
fn require(condition: bool) -> AppResult<()> {
    // 不变量满足时保持成功。
    if condition {
        // 返回空成功。
        Ok(())
    } else {
        // 不输出失败位置、协议值或 native 事实。
        Err(fixture_failed())
    }
}

// 以固定总预算连接、认证并取得严格 epoch 投影。
fn certified_connection() -> AppResult<CertifiedBrokerFixtureConnection> {
    // 将底层失败折叠为 fixture 安全错误。
    connect_ready(EXCHANGE_TIMEOUT_MS).map_err(|_| fixture_failed())
}

// 从已认证连接复制并再次解析可供 wire decoder 使用的 epoch。
fn certified_epoch(
    // 借用已认证 raw connection。
    connection: &CertifiedBrokerFixtureConnection,
) -> AppResult<BrowserSessionBrokerEpoch> {
    // 重新验证 Adapter 保存的 handshake epoch 没有变成任意文本。
    BrowserSessionBrokerEpoch::new(connection.epoch().to_owned()).map_err(|_| fixture_failed())
}

// 计算本次固定交换使用的唯一绝对 deadline。
fn exchange_deadline() -> Instant {
    // 从当前单调时钟建立不超过冻结 request 预算的 deadline。
    Instant::now() + Duration::from_millis(u64::from(EXCHANGE_TIMEOUT_MS))
}

// 在每个无 live session 场景边界等待 worker 进程完成回收。
fn settle_worker_boundary() {
    // 不读取、输出或持有任何 worker 私有进程事实。
    thread::sleep(WORKER_SETTLE_DELAY);
}

// 将一条已认证 raw frame 写入生产 pipe 并读取一条回应。
fn write_then_read(
    // 借用已认证 raw connection。
    connection: &CertifiedBrokerFixtureConnection,
    // 借用已经严格 builder 生成的文本。
    text: &str,
    // 绑定调用方拥有的绝对 deadline。
    deadline: Instant,
) -> AppResult<String> {
    // 写入唯一完整 frame。
    connection
        // 委托真实 production pipe 的有界写入。
        .write_raw_until(text, deadline)
        // 不公开投递阶段细节。
        .map_err(|_| fixture_failed())?;
    // 读取唯一 response/control frame。
    connection
        // 委托真实 production pipe 的有界读取。
        .read_raw_until(deadline)
        // 不公开 pipe、认证或 deadline 细节。
        .map_err(|_| fixture_failed())
}

// 严格解码并断言 request 的唯一 accepted 首帧。
fn require_accepted(
    // 借用不可信 wire 文本。
    text: &str,
    // 绑定发送过的 strict request。
    request: &BrowserSessionBrokerRequestFrame,
    // 绑定认证握手 current epoch。
    epoch: &BrowserSessionBrokerEpoch,
) -> AppResult<()> {
    // 严格解码 nonce、operation、epoch 与语义指纹绑定的 response。
    let response = decode_response(text, request.request(), epoch).map_err(|_| fixture_failed())?;
    // accepted 必须具有冻结的非终态状态组合。
    require(
        // accepted 没有 final outcome。
        response.outcome().is_none()
            // accepted 固定为首个 revision。
            && response.request_revision() == 0
            // 传输已接受完整 frame。
            && response.transport_accepted()
            // Module 已跨越业务接受点。
            && response.business_accepted()
            // accepted 尚不是完成终态。
            && !response.completed()
            // accepted 后不允许自动重试。
            && !response.retry_safe()
            // accepted 不是本地 OutcomeUnknown。
            && !response.outcome_unknown()
            // open command accepted 后必须保守标记可能 mutation。
            && response.target_may_have_mutated()
            // accepted 不得提前携带 success data 或错误码。
            && response.success().is_none()
            // accepted 不得携带终态错误。
            && response.error_code().is_none(),
    )
}

// 断言 accepted 后 open 的 Completed rev1，并返回唯一公开 session identity。
fn completed_open_session(
    // 借用不可信 final 文本。
    text: &str,
    // 绑定已经 accepted 的 strict open request。
    request: &BrowserSessionBrokerRequestFrame,
    // 绑定认证握手 current epoch。
    epoch: &BrowserSessionBrokerEpoch,
) -> AppResult<String> {
    // 严格解码 request-bound final。
    let response = decode_response(text, request.request(), epoch).map_err(|_| fixture_failed())?;
    // final 必须完成一次可能变更 target 的 open。
    require(
        // open 最终只能为 completed。
        response.outcome() == Some(BrowserSessionBrokerOutcome::Completed)
            // accepted 后 final 固定为 revision 一。
            && response.request_revision() == 1
            // 传输与业务均已接受。
            && response.transport_accepted()
            // 业务接受点已经建立。
            && response.business_accepted()
            // final 必须已完成。
            && response.completed()
            // 已执行 mutation 的 final 不得自动重试。
            && !response.retry_safe()
            // 可信 completed 不是 OutcomeUnknown。
            && !response.outcome_unknown()
            // open 可能已创建 target。
            && response.target_may_have_mutated()
            // 成功 final 不得携带 error。
            && response.error_code().is_none(),
    )?;
    // 只接受 operation-matched 的公开 session 成功数据。
    match response.success() {
        // 复制唯一公开 opaque session identity。
        Some(BrowserSessionBrokerSuccess::Open { session_id }) => Ok(session_id.clone()),
        // 其余 success 形状不能证明 open 已完成。
        _ => Err(fixture_failed()),
    }
}

// 断言 accepted 后 close 的 Completed rev1。
fn require_completed_close(
    // 借用不可信 final 文本。
    text: &str,
    // 绑定已经 accepted 的 strict close request。
    request: &BrowserSessionBrokerRequestFrame,
    // 绑定认证握手 current epoch。
    epoch: &BrowserSessionBrokerEpoch,
) -> AppResult<()> {
    // 严格解码 request-bound close final。
    let response = decode_response(text, request.request(), epoch).map_err(|_| fixture_failed())?;
    // close 必须完整建立最终 mutation 事实。
    require(
        // close 最终只能为 completed。
        response.outcome() == Some(BrowserSessionBrokerOutcome::Completed)
            // accepted 后 final 固定为 revision 一。
            && response.request_revision() == 1
            // 传输与业务均必须接受。
            && response.transport_accepted()
            // close 已跨越业务接受点。
            && response.business_accepted()
            // final 必须可信完成。
            && response.completed()
            // 变更 final 不授权自动重试。
            && !response.retry_safe()
            // final 不是 OutcomeUnknown。
            && !response.outcome_unknown()
            // close 可能已经改变 target 生命周期。
            && response.target_may_have_mutated()
            // close success 必须没有 error。
            && response.error_code().is_none()
            // close success 只允许关闭事实。
            && matches!(response.success(), Some(BrowserSessionBrokerSuccess::Close)),
    )
}

// 发送 open 并完整读取 accepted 与 Completed final。
fn send_open_then_completed(
    // 借用认证 raw connection。
    connection: &CertifiedBrokerFixtureConnection,
    // 借用 strict confirmed open frame。
    open: &BrowserSessionBrokerRequestFrame,
    // 绑定认证握手 epoch。
    epoch: &BrowserSessionBrokerEpoch,
) -> AppResult<(String, String)> {
    // 为 accepted 与 final 共用单一 request deadline。
    let deadline = exchange_deadline();
    // 写入 open 并读取 accepted 首帧。
    let accepted_text = write_then_read(connection, open.text(), deadline)?;
    // 只接受严格 accepted。
    require_accepted(&accepted_text, open, epoch)?;
    // 在同一 deadline 内读取 completed final。
    let final_text = connection
        // 委托真实 pipe 读取 terminal response。
        .read_raw_until(deadline)
        // 不公开 pipe 或 timeout 细节。
        .map_err(|_| fixture_failed())?;
    // 严格验证 final 并提取唯一公开 session。
    let session_id = completed_open_session(&final_text, open, epoch)?;
    // 返回 session 与原始 final，供 replay 做逐字比较。
    Ok((session_id, final_text))
}

// 重送已 accepted open，并允许 server 直接回放 final 或先回 accepted 再回 final。
fn replay_open_to_final(
    // 借用恢复后的认证 raw connection。
    connection: &CertifiedBrokerFixtureConnection,
    // 借用原始 strict open frame。
    open: &BrowserSessionBrokerRequestFrame,
    // 绑定认证握手 epoch。
    epoch: &BrowserSessionBrokerEpoch,
    // 绑定原 request 尚未耗尽的绝对 deadline。
    deadline: Instant,
) -> AppResult<(String, String)> {
    // 发送逐字相同的 strict request 并读取首帧。
    let first_text = write_then_read(connection, open.text(), deadline)?;
    // 根据首帧封闭区分 accepted 或 terminal replay。
    let first_response =
        decode_response(&first_text, open.request(), epoch).map_err(|_| fixture_failed())?;
    // accepted 路径必须再读取 final；terminal 路径则直接验证。
    let final_text = if first_response.outcome().is_none() {
        // 仍然严格验证 accepted flags。
        require_accepted(&first_text, open, epoch)?;
        // 读取同一 deadline 内的 final。
        connection
            // 委托真实 pipe 读取 terminal response。
            .read_raw_until(deadline)
            // 不公开内部 read 失败。
            .map_err(|_| fixture_failed())?
    } else {
        // 直接 final replay 保留首帧文本。
        first_text
    };
    // 仅接受 completed open final。
    let session_id = completed_open_session(&final_text, open, epoch)?;
    // 返回 session 与 terminal wire，供调用方比较重放事实。
    Ok((session_id, final_text))
}

// 用独立 nonce 完整关闭已公开的 session，避免留下 live worker。
fn close_completed(
    // 借用需要关闭的公开 opaque session。
    session_id: &str,
    // 借用已经认证的 current epoch。
    epoch: &BrowserSessionBrokerEpoch,
    // 接收不与任何 open 重用的 canonical close nonce。
    close_nonce: &str,
) -> AppResult<()> {
    // 建立独立认证连接。
    let connection = certified_connection()?;
    // 连接 ready epoch 必须仍是原记录所属 generation。
    let connection_epoch = certified_epoch(&connection)?;
    // 防止在 restart 后关闭错误记录。
    require(connection_epoch == *epoch)?;
    // 构造字段封闭的 confirmed close。
    let close = BrowserSessionBrokerRequestFrame::close_confirmed(
        // 使用独立 close nonce。
        close_nonce,
        // 绑定原 current epoch。
        epoch.as_str(),
        // 保持完整 request 预算。
        EXCHANGE_TIMEOUT_MS,
        // 只传递公开 opaque target。
        session_id,
    )
    // builder 失败不公开 parser 细节。
    .map_err(|_| fixture_failed())?;
    // 建立 accepted 与 final 共用 deadline。
    let deadline = exchange_deadline();
    // 写入 close 并读取 accepted。
    let accepted_text = write_then_read(&connection, close.text(), deadline)?;
    // close accepted 的 flags 与 open accepted 同为 mutation accepted。
    require_accepted(&accepted_text, &close, epoch)?;
    // 在同一 deadline 内读取 close terminal。
    let final_text = connection
        // 委托真实 pipe 读取 terminal response。
        .read_raw_until(deadline)
        // 不公开内部 read 失败。
        .map_err(|_| fixture_failed())?;
    // 只接受 completed close final。
    require_completed_close(&final_text, &close, epoch)
}

// 执行同 nonce 同义 replay、异义冲突与最终 close 的真实固定 pipe 场景。
fn nonce_replay_and_conflict() -> AppResult<()> {
    // 建立首次真实认证连接。
    let first = certified_connection()?;
    // 从首次连接取得严格 current epoch。
    let epoch = certified_epoch(&first)?;
    // 构造固定 nonce 的 confirmed open。
    let open = BrowserSessionBrokerRequestFrame::open_confirmed(
        // 绑定固定 replay nonce。
        REPLAY_NONCE,
        // 绑定当前 live epoch。
        epoch.as_str(),
        // 保持完整 request budget。
        EXCHANGE_TIMEOUT_MS,
    )
    // builder 失败不泄漏 schema 细节。
    .map_err(|_| fixture_failed())?;
    // 完整读取首次 accepted 与 completed open。
    let (first_session, first_final) = send_open_then_completed(&first, &open, &epoch)?;
    // 释放首次认证连接。
    drop(first);
    // 建立独立认证连接执行同义 replay。
    let replay = certified_connection()?;
    // replay 必须仍属于同一 broker generation。
    require(certified_epoch(&replay)? == epoch)?;
    // 重送同一 frame 并完整取得 terminal replay。
    let (replay_session, replay_final) = replay_open_to_final(
        // 使用 replay 认证连接。
        &replay,
        // 重送原 strict open。
        &open,
        // 绑定原 record epoch。
        &epoch,
        // 该独立 replay 使用新的完整 request budget。
        exchange_deadline(),
    )?;
    // 同义 replay 必须逐字回放相同 final 与相同公开 session。
    require(replay_session == first_session && replay_final == first_final)?;
    // 释放 replay 认证连接。
    drop(replay);
    // 建立异义冲突认证连接。
    let conflict_connection = certified_connection()?;
    // 冲突必须发生在相同 broker generation。
    require(certified_epoch(&conflict_connection)? == epoch)?;
    // 使用同 nonce 但不同 close 语义构造严格 request。
    let conflict = BrowserSessionBrokerRequestFrame::close_confirmed(
        // 故意复用已绑定 open 的 nonce。
        REPLAY_NONCE,
        // 绑定同一 current epoch。
        epoch.as_str(),
        // 保持完整 request budget。
        EXCHANGE_TIMEOUT_MS,
        // 使用 canonical 但与原 open 不同的公开 target。
        CONFLICT_SESSION_ID,
    )
    // builder 失败不公开 schema 细节。
    .map_err(|_| fixture_failed())?;
    // 写入异义 request 并读取 early final。
    let conflict_text =
        write_then_read(&conflict_connection, conflict.text(), exchange_deadline())?;
    // 严格解码绑定异义 request 的 final。
    let conflict_response = decode_response(&conflict_text, conflict.request(), &epoch)
        // 不把任意文本当作冲突 final。
        .map_err(|_| fixture_failed())?;
    // 语义冲突必须是业务前 rev0 rejection。
    require(
        // 只接受 rejected final。
        conflict_response.outcome() == Some(BrowserSessionBrokerOutcome::Rejected)
            // 业务前 final 固定 revision 零。
            && conflict_response.request_revision() == 0
            // 完整 envelope 已被 transport 接收。
            && conflict_response.transport_accepted()
            // 语义冲突不得进入业务执行。
            && !conflict_response.business_accepted()
            // early rejection 是可信 terminal。
            && conflict_response.completed()
            // 确定未执行故可安全重试。
            && conflict_response.retry_safe()
            // rejection 不是 OutcomeUnknown。
            && !conflict_response.outcome_unknown()
            // 未接受业务前 target 不得变化。
            && !conflict_response.target_may_have_mutated()
            // 错误码必须为冻结冲突 code。
            && conflict_response.error_code() == Some("NONCE_SEMANTIC_CONFLICT")
            // conflict 不得携带 success data。
            && conflict_response.success().is_none(),
    )?;
    // 释放冲突认证连接。
    drop(conflict_connection);
    // 建立第三条认证连接，再次重放原同义 request。
    let after_conflict = certified_connection()?;
    // 原记录不得因异义冲突被替换或清除。
    require(certified_epoch(&after_conflict)? == epoch)?;
    // 重新取得原 nonce 的 terminal replay。
    let (after_conflict_session, after_conflict_final) = replay_open_to_final(
        // 使用异义拒绝后的 replay 认证连接。
        &after_conflict,
        // 重送未被冲突替换的原 strict open。
        &open,
        // 绑定原 record epoch。
        &epoch,
        // 该独立 replay 使用新的完整 request budget。
        exchange_deadline(),
    )?;
    // 原 session 与 final 必须维持不变。
    require(after_conflict_session == first_session && after_conflict_final == first_final)?;
    // 最后显式关闭首次创建的 session。
    close_completed(&first_session, &epoch, REPLAY_CLOSE_NONCE)?;
    // 让外部固定进程见证在下一场景前观察到 worker 回收。
    settle_worker_boundary();
    // 标记已关闭且已收敛的场景成功。
    Ok(())
}

// 执行 cancel 先到 tombstone、同义 replay 与无 worker 后到 request 场景。
fn cancel_before_target() -> AppResult<()> {
    // 建立发送 cancel 的认证连接。
    let cancel_connection = certified_connection()?;
    // 取得 cancel 所绑定的 current epoch。
    let epoch = certified_epoch(&cancel_connection)?;
    // 构造固定 cancel nonce 与尚未存在的 target nonce。
    let cancel = BrowserSessionBrokerCancelFrame::new(
        // 绑定固定 cancel nonce。
        TOMBSTONE_CANCEL_NONCE,
        // 指向固定但尚未派发的 request nonce。
        TOMBSTONE_TARGET_NONCE,
        // 绑定当前 live epoch。
        epoch.as_str(),
    )
    // builder 失败不公开 schema 细节。
    .map_err(|_| fixture_failed())?;
    // 写入 cancel 并读取首个 receipt。
    let receipt_text = write_then_read(&cancel_connection, cancel.text(), exchange_deadline())?;
    // 严格解码首次 receipt。
    let receipt = decode_cancel_receipt(&receipt_text, cancel.cancel(), &epoch, None)
        // 不把 control codec 失败当成 tombstone 成功。
        .map_err(|_| fixture_failed())?;
    // cancel 先到必须安装 unknown-request tombstone revision 零。
    require(
        // 首次 receipt 固定为 revision 零。
        receipt.cancel_revision() == 0
            // 目标不存在时必须返回 unknown-request。
            && receipt.status() == BrowserSessionBrokerCancelStatus::UnknownRequest,
    )?;
    // 释放首次 cancel 认证连接。
    drop(cancel_connection);
    // 用独立认证连接发送逐字相同 cancel。
    let replay_connection = certified_connection()?;
    // replay 必须仍属于同一 broker generation。
    require(certified_epoch(&replay_connection)? == epoch)?;
    // 写入同义 cancel 并读取 replay receipt。
    let replay_text = write_then_read(&replay_connection, cancel.text(), exchange_deadline())?;
    // receipt replay 必须逐字相同且严格可解码。
    let replay_receipt = decode_cancel_receipt(&replay_text, cancel.cancel(), &epoch, None)
        // 不把不关联的 control frame 当作 replay。
        .map_err(|_| fixture_failed())?;
    // 同义 cancel replay 不得改变 tombstone。
    require(
        // wire replay 必须逐字相同。
        replay_text == receipt_text
            // revision 不得漂移。
            && replay_receipt.cancel_revision() == 0
            // tombstone 状态不得漂移。
            && replay_receipt.status() == BrowserSessionBrokerCancelStatus::UnknownRequest,
    )?;
    // 释放 cancel replay 认证连接。
    drop(replay_connection);
    // 建立发送后到 request 的认证连接。
    let target_connection = certified_connection()?;
    // tombstone 不得跨 restart 错配。
    require(certified_epoch(&target_connection)? == epoch)?;
    // 构造与 tombstone target 逐字匹配的 confirmed open。
    let target = BrowserSessionBrokerRequestFrame::open_confirmed(
        // 使用 tombstone 指向的 target nonce。
        TOMBSTONE_TARGET_NONCE,
        // 绑定同一 live epoch。
        epoch.as_str(),
        // 保持完整 request budget。
        EXCHANGE_TIMEOUT_MS,
    )
    // builder 失败不公开 schema 细节。
    .map_err(|_| fixture_failed())?;
    // 写入后到 request 并读取直接 terminal final。
    let target_text = write_then_read(&target_connection, target.text(), exchange_deadline())?;
    // 严格解码 tombstone 关联 final。
    let response = decode_response(&target_text, target.request(), &epoch)
        // 不能把任意 final 当成 tombstone 响应。
        .map_err(|_| fixture_failed())?;
    // 只接受业务前取消的完整固定状态组合。
    require(
        // terminal 必须表达 cancelled-before-acceptance。
        response.outcome() == Some(BrowserSessionBrokerOutcome::CancelledBeforeAcceptance)
            // tombstone final 固定 revision 零。
            && response.request_revision() == 0
            // 已完整收到 request。
            && response.transport_accepted()
            // 但不得跨过业务接受点。
            && !response.business_accepted()
            // tombstone final 必须完成。
            && response.completed()
            // cancel tombstone 不授权自动重试。
            && !response.retry_safe()
            // tombstone 是可信确定结果。
            && !response.outcome_unknown()
            // request 未执行业务，target 不得改变。
            && !response.target_may_have_mutated()
            // 只接受冻结稳定错误码。
            && response.error_code() == Some("CANCELLED_BEFORE_ACCEPTANCE")
            // 取消终态不得携带 success data。
            && response.success().is_none(),
    )?;
    // 释放后到 request 认证连接。
    drop(target_connection);
    // 建立同义 request replay 的独立认证连接。
    let target_replay = certified_connection()?;
    // replay 仍必须属于同一 epoch。
    require(certified_epoch(&target_replay)? == epoch)?;
    // 重送原 target request 并读取 cached final。
    let replay_final = write_then_read(&target_replay, target.text(), exchange_deadline())?;
    // tombstone final 必须逐字重放。
    require(replay_final == target_text)?;
    // tombstone 不会启动 worker，仍保留统一的外部观察边界。
    settle_worker_boundary();
    // 标记无 worker 的 tombstone 场景成功。
    Ok(())
}

// 执行 accepted 后客户端断连、重连取得原 final 并 close 的物理场景。
fn accepted_disconnect_reconnect() -> AppResult<()> {
    // 建立首次认证连接。
    let first = certified_connection()?;
    // 取得首次连接的 strict epoch。
    let epoch = certified_epoch(&first)?;
    // 构造该场景专属 fixed confirmed open。
    let open = BrowserSessionBrokerRequestFrame::open_confirmed(
        // 使用专属 canonical nonce。
        DISCONNECT_NONCE,
        // 绑定 current epoch。
        epoch.as_str(),
        // 保持完整 request budget。
        EXCHANGE_TIMEOUT_MS,
    )
    // builder 失败不公开 schema 细节。
    .map_err(|_| fixture_failed())?;
    // 写入 open 并读取唯一 accepted。
    let request_deadline = exchange_deadline();
    // 写入 open 并读取唯一 accepted。
    let accepted_text = write_then_read(&first, open.text(), request_deadline)?;
    // 断连前必须已经收到 strict accepted。
    require_accepted(&accepted_text, &open, &epoch)?;
    // 释放连接，模拟 accepted 后调用方断开。
    drop(first);
    // 建立恢复使用的认证连接。
    let recovered = certified_connection()?;
    // 不允许将旧 nonce 跨 restart 关联。
    require(certified_epoch(&recovered)? == epoch)?;
    // 重送同一 open，允许 direct final 或 accepted 后 final。
    let (session_id, _) = replay_open_to_final(
        // 使用恢复认证连接。
        &recovered,
        // 重送原 strict open。
        &open,
        // 绑定原 record epoch。
        &epoch,
        // 恢复不得越过原 request deadline。
        request_deadline,
    )?;
    // 显式关闭 recovered 的唯一公开 session。
    close_completed(&session_id, &epoch, DISCONNECT_CLOSE_NONCE)?;
    // 让外部固定进程见证在下一场景前观察到 worker 回收。
    settle_worker_boundary();
    // 标记断连恢复场景已关闭并收敛。
    Ok(())
}

// 执行 accepted 后短读超时、原 deadline 内重连取得 final 并 close 的物理场景。
fn accepted_read_timeout_reconnect() -> AppResult<()> {
    // 建立首次认证连接。
    let first = certified_connection()?;
    // 取得首次连接的 strict epoch。
    let epoch = certified_epoch(&first)?;
    // 构造该场景专属 fixed confirmed open。
    let open = BrowserSessionBrokerRequestFrame::open_confirmed(
        // 使用专属 canonical nonce。
        READ_TIMEOUT_NONCE,
        // 绑定 current epoch。
        epoch.as_str(),
        // 保持完整 request budget。
        EXCHANGE_TIMEOUT_MS,
    )
    // builder 失败不公开 schema 细节。
    .map_err(|_| fixture_failed())?;
    // 为首次 accepted 与本地短读保留同一个 request deadline 上限。
    let request_deadline = exchange_deadline();
    // 写入 open 并读取 accepted。
    let accepted_text = write_then_read(&first, open.text(), request_deadline)?;
    // 短读前必须已收到 strict accepted。
    require_accepted(&accepted_text, &open, &epoch)?;
    // 固定短读 deadline 必须严格早于 request 总 deadline。
    let local_deadline = Instant::now() + LOCAL_READ_TIMEOUT;
    // 短观察只允许在原 request 预算尚未耗尽时发生。
    require(local_deadline < request_deadline)?;
    // ready worker 的固定 250ms 延迟必须使该短读超时。
    require(first.read_raw_until(local_deadline).is_err())?;
    // 释放本地短读超时的认证连接。
    drop(first);
    // 建立仍在原 request deadline 内的恢复认证连接。
    let recovered = certified_connection()?;
    // 只在未 restart 的同一 epoch 内恢复。
    require(certified_epoch(&recovered)? == epoch)?;
    // 重送同 nonce，取得原执行的 final 或其 direct replay。
    let (session_id, _) = replay_open_to_final(
        // 使用恢复认证连接。
        &recovered,
        // 重送原 strict open。
        &open,
        // 绑定原 record epoch。
        &epoch,
        // 恢复不得越过原 request deadline。
        request_deadline,
    )?;
    // 显式关闭恢复得到的唯一公开 session。
    close_completed(&session_id, &epoch, READ_TIMEOUT_CLOSE_NONCE)?;
    // 让外部固定进程见证完成前观察到 worker 回收。
    settle_worker_boundary();
    // 标记短读超时恢复场景已关闭并收敛。
    Ok(())
}

// 写出唯一一行不含协议身份与 native 事实的固定安全结果。
fn write_output(
    // 接收已封闭的 JSON envelope。
    value: serde_json::Value,
) {
    // 序列化固定结果。
    let text = value.to_string();
    // 忽略测试父进程提前关闭 stdout 的写入错误。
    let _ = writeln!(std::io::stdout(), "{text}");
}

// 运行全部固定 raw 协议场景，不读取 argv、stdin、环境或任意外部路径。
pub fn run_stdio() -> i32 {
    // 顺序执行以让每个场景在关闭前独占其固定 nonce。
    let result = nonce_replay_and_conflict()
        // 继续验证 cancel tombstone 和 request replay。
        .and_then(|()| cancel_before_target())
        // 继续验证 accepted 后断连恢复与 close 生命周期。
        .and_then(|()| accepted_disconnect_reconnect())
        // 最后验证短读超时窗口内的恢复与 close 生命周期。
        .and_then(|()| accepted_read_timeout_reconnect());
    // 只输出固定安全布尔结果或固定失败码。
    match result {
        // 四条物理语义全部通过才声明 fixture 成功。
        Ok(()) => {
            // 不回显 nonce、epoch、session、frame、pipe、PID 或路径。
            write_output(json!({
                // 标记固定场景执行成功。
                "ok": true,
                // 只暴露四项非敏感验证事实。
                "result": {
                    // 同 request 同义 replay 已验证。
                    "sameRequestReplay": true,
                    // 同 request identity 的异义语义已被拒绝。
                    "semanticConflict": true,
                    // cancel tombstone 已阻止后到 request。
                    "cancelBeforeTarget": true,
                    // accepted 后断连已在同一 request budget 内恢复。
                    "acceptedDisconnectRecovery": true,
                    // accepted 后短读 timeout 已在同一 request budget 内恢复。
                    "acceptedReadTimeoutRecovery": true
                }
            }));
            // 返回成功退出码。
            0
        }
        // 任一内部失败都闭合为固定公开错误。
        Err(_) => {
            // 不回显原错误文本、来源或协议身份。
            write_output(json!({
                // 标记 fixture 未完成。
                "ok": false,
                // 只输出稳定公开错误码。
                "error": { "code": "FIXTURE_FAILED" }
            }));
            // 返回稳定失败退出码。
            2
        }
    }
}
