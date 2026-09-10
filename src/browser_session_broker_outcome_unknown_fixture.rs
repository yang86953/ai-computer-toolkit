//! 以真实固定 broker 验证 accepted 后总 deadline 耗尽的安全 OutcomeUnknown 客户端投影。

// 导入唯一 stdout 输出、有限等待与单调 deadline。
use std::{
    // 只写出单行固定安全 JSON。
    io::Write,
    // 在本地 deadline 前等待已接受执行的最终观察。
    thread,
    // 保存唯一不可延长的总预算。
    time::{Duration, Instant},
};

// 导入固定 JSON 输出构造器。
use serde_json::json;

// 导入认证连接 Adapter、严格 wire codec 与统一错误边界。
use crate::{
    // 只连接真实固定 endpoint 并完成双向 peer 认证。
    adapters::browser_session_broker_windows_fixture::{
        // 保存不泄漏 native pipe 的认证连接。
        CertifiedBrokerFixtureConnection,
        // 在调用方原始绝对 deadline 前连接并读取 ready。
        connect_ready_until,
    },
    // 只使用 broker 已拥有的冻结 request/response 契约。
    components::browser_session_broker_protocol::{
        // 将 ready epoch 转为 response decoder 的严格投影。
        state::BrowserSessionBrokerEpoch,
        // 构造 confirmed request 并解码 request-bound response。
        wire::{BrowserSessionBrokerRequestFrame, decode_response},
    },
    // 复用内部安全失败边界而不泄漏 transport 事实。
    domain::{AppControlError, AppResult},
};

// 固定覆盖首次连接、首次写入、断连、Attach 与本地等待的总预算。
const REQUEST_TIMEOUT_MS: u32 = 5_000;
// 固定且仅在本夹具内部使用的 canonical request nonce。
const OUTCOME_UNKNOWN_NONCE: &str = "99999999999999999999999999999999";
// 固定为生产 Adapter 同源的本地 unknown 安全说明。
const OUTCOME_UNKNOWN_MESSAGE: &str =
    "The browser session broker command outcome is unknown after delivery began.";

// 构造不回显 nonce、epoch、frame、pipe、PID 或路径的统一夹具失败。
fn fixture_failed() -> AppControlError {
    // 返回固定公开错误码和不含内部事实的说明。
    AppControlError::new(
        // 固定夹具失败错误码。
        "FIXTURE_FAILED",
        // 固定安全失败说明。
        "The fixed broker OutcomeUnknown fixture failed.",
    )
}

// 将内部严格 codec、时钟或 transport 失败闭合为唯一安全夹具错误。
fn require(condition: bool) -> AppResult<()> {
    // 只在不变量成立时继续。
    if condition {
        // 返回空成功。
        Ok(())
    } else {
        // 不公开失败位置或协议身份。
        Err(fixture_failed())
    }
}

// 从当前单调时钟构造本次唯一不可延长的请求 deadline。
fn request_deadline() -> AppResult<Instant> {
    // 计算固定总预算对应的 duration。
    let budget = Duration::from_millis(u64::from(REQUEST_TIMEOUT_MS));
    // 拒绝理论时钟溢出而不改变请求状态。
    Instant::now()
        .checked_add(budget)
        .ok_or_else(fixture_failed)
}

// 计算下一次写入前仍可编码进严格 wire 的剩余毫秒预算。
fn remaining_timeout_ms(deadline: Instant) -> AppResult<u32> {
    // 读取不扩张总 deadline 的剩余 duration。
    let remaining = deadline
        // deadline 已经过期时不得构造新的 request frame。
        .checked_duration_since(Instant::now())
        // 将时钟竞争保持为固定夹具失败。
        .ok_or_else(fixture_failed)?;
    // 向下取整以保证 wire deadline 永不超过本地 deadline。
    let milliseconds = remaining.as_millis();
    // 不足一毫秒时不得开始新的 transport 写入。
    if milliseconds == 0 {
        // 返回安全夹具失败。
        return Err(fixture_failed());
    }
    // 仅接受协议已经冻结的 u32 范围。
    u32::try_from(milliseconds).map_err(|_| fixture_failed())
}

// 从认证 ready 保存的文本重新构造 strict response decoder epoch。
fn certified_epoch(
    connection: &CertifiedBrokerFixtureConnection,
) -> AppResult<BrowserSessionBrokerEpoch> {
    // 再次验证 Adapter 只保存 canonical ready epoch。
    BrowserSessionBrokerEpoch::new(connection.epoch().to_owned()).map_err(|_| fixture_failed())
}

// 在同一绝对 deadline 前完成认证连接与 server-first ready。
fn certified_connection_until(deadline: Instant) -> AppResult<CertifiedBrokerFixtureConnection> {
    // 将连接、认证或 ready 失败折叠为不泄漏 native 事实的错误。
    connect_ready_until(deadline).map_err(|_| fixture_failed())
}

// 写入一条严格 builder 生成的 frame，并读取唯一首个 broker response。
fn write_then_read(
    // 借用已认证的固定 pipe 投影。
    connection: &CertifiedBrokerFixtureConnection,
    // 借用只可由 strict builder 生成的文本。
    text: &str,
    // 绑定调用方持有的唯一 absolute deadline。
    deadline: Instant,
) -> AppResult<String> {
    // 先提交完整 request frame。
    connection
        // 委托真实固定 pipe 的有界写入。
        .write_raw_until(text, deadline)
        // 不公开 write 阶段或 native 错误。
        .map_err(|_| fixture_failed())?;
    // 再读取同一连接的一条 server response。
    connection
        // 委托真实固定 pipe 的有界读取。
        .read_raw_until(deadline)
        // 不公开 read 阶段或 native 错误。
        .map_err(|_| fixture_failed())
}

// 严格验证本请求唯一允许的 accepted revision 零，而不把它解释为终态。
fn require_accepted(
    // 借用未经信任的 broker wire 文本。
    text: &str,
    // 绑定已发送的完整 strict request。
    request: &BrowserSessionBrokerRequestFrame,
    // 绑定认证 ready 的 current epoch。
    epoch: &BrowserSessionBrokerEpoch,
) -> AppResult<()> {
    // 使用同源 codec 验证 nonce、operation、epoch 与 semantic fingerprint。
    let response = decode_response(text, request.request(), epoch).map_err(|_| fixture_failed())?;
    // 只接受 accepted snapshot 的完整封闭状态组合。
    require(
        // accepted 不是业务终态。
        response.outcome().is_none()
            // accepted 固定为请求第一版。
            && response.request_revision() == 0
            // 完整 frame 已被 transport 接管。
            && response.transport_accepted()
            // broker 已跨越业务接受点。
            && response.business_accepted()
            // accepted 绝不报告完成。
            && !response.completed()
            // 已接受 mutation 禁止自动重试。
            && !response.retry_safe()
            // accepted 本身不是 wire OutcomeUnknown。
            && !response.outcome_unknown()
            // open accepted 必须保守标记 target 可能变化。
            && response.target_may_have_mutated()
            // accepted 不得抢先携带成功数据。
            && response.success().is_none()
            // accepted 不得携带终态错误。
            && response.error_code().is_none(),
    )
}

// 等待直到调用方拥有的 absolute deadline，而不解析或报告任何迟到 wire 终态。
fn wait_until(deadline: Instant) {
    // 读取仍属于 caller 总预算的 duration。
    let remaining = deadline.saturating_duration_since(Instant::now());
    // 只有预算尚存时才执行一次有界本地等待。
    if !remaining.is_zero() {
        // 等待不会延长既有 deadline，只推迟本地输出到该边界。
        thread::sleep(remaining);
    }
}

// 执行首次 accepted、断连、同 nonce Attach 与 deadline 期满的完整物理场景。
fn observe_outcome_unknown() -> AppResult<()> {
    // 在第一次连接前冻结唯一请求总 deadline。
    let deadline = request_deadline()?;
    // 仅在该 deadline 前建立真实 fixed pipe、互认证与 ready。
    let first = certified_connection_until(deadline)?;
    // 保存 first ready 的严格 decoder epoch。
    let epoch = certified_epoch(&first)?;
    // 计算首次 request 写入的剩余预算。
    let first_remaining_timeout_ms = remaining_timeout_ms(deadline)?;
    // 构造固定 confirmed open，不接受 caller nonce、epoch 或 operation 注入。
    let first_request = BrowserSessionBrokerRequestFrame::open_confirmed(
        // 使用仅本夹具可见的固定 nonce。
        OUTCOME_UNKNOWN_NONCE,
        // 绑定首个认证 ready epoch。
        epoch.as_str(),
        // 保留首次写入前的真实剩余预算。
        first_remaining_timeout_ms,
    )
    // builder 失败不公开 schema 细节。
    .map_err(|_| fixture_failed())?;
    // 写入首次 request 并读取 strict accepted rev0。
    let first_response = write_then_read(&first, first_request.text(), deadline)?;
    // 只接受首次执行已经 accepted 的 snapshot。
    require_accepted(&first_response, &first_request, &epoch)?;
    // 主动释放首次连接，client 断线不得隐式取消已 accepted 执行。
    drop(first);
    // 仍只在原始 deadline 前重连、互认证并读取 ready。
    let recovered = certified_connection_until(deadline)?;
    // recovery 必须留在同一 broker epoch，不能跨 restart 关联旧 ledger。
    require(certified_epoch(&recovered)? == epoch)?;
    // 计算重连 Attach 写入时的真实剩余预算。
    let attach_remaining_timeout_ms = remaining_timeout_ms(deadline)?;
    // 恢复写入必须实际缩短 request-bound deadline。
    require(attach_remaining_timeout_ms < first_remaining_timeout_ms)?;
    // 使用同 nonce、同 epoch 和更短预算重建严格 confirmed open。
    let attach_request = BrowserSessionBrokerRequestFrame::open_confirmed(
        // 绝不生成第二个 request identity。
        OUTCOME_UNKNOWN_NONCE,
        // 绝不跨 epoch attach。
        epoch.as_str(),
        // 只将剩余预算缩短。
        attach_remaining_timeout_ms,
    )
    // builder 失败不公开 schema 细节。
    .map_err(|_| fixture_failed())?;
    // 重建仅可改变 deadline，不得改变 ledger semantic identity。
    require(
        // nonce 必须与首次 frame 逐字相同。
        attach_request.request().request_nonce() == first_request.request().request_nonce()
            // semantic fingerprint 也必须保持同义。
            && attach_request.request().semantic_fingerprint()
                == first_request.request().semantic_fingerprint(),
    )?;
    // 写入恢复 frame 并观察同一 execution 的 accepted snapshot。
    let attach_response = write_then_read(&recovered, attach_request.text(), deadline)?;
    // accepted rev0 证明恢复只观察仍在途的同一请求。
    require_accepted(&attach_response, &attach_request, &epoch)?;
    // accepted-only worker 不得提供任何可被客户端解释为终态的 wire frame。
    let final_read = recovered.read_raw_until(deadline);
    // 任何成功读取都可能是 accepted、final 或协议污染，均不得丢弃或解释。
    require(final_read.is_err())?;
    // transport 失败不证明业务结果，因此只等待调用方原始 deadline。
    wait_until(deadline);
    // 本地总预算必须已经耗尽，不能提前输出 OutcomeUnknown。
    require(Instant::now() >= deadline)?;
    // 不解码、断言或向 stdout 宣称任何 wire unknown final。
    // 仅证明客户端无法在总预算内取得可信 completed/failed/cancelled final。
    Ok(())
}

// 写出唯一一行固定安全 JSON，不回显任何协议或 native 事实。
fn write_output(value: serde_json::Value) {
    // 序列化固定 envelope。
    let text = value.to_string();
    // 忽略测试父进程提前关闭 stdout 的写入错误。
    let _ = writeln!(std::io::stdout(), "{text}");
}

// 运行无 argv、无 stdin、无环境或路径注入的 OutcomeUnknown 客户端夹具。
pub fn run_stdio() -> i32 {
    // 预期场景完成后只输出本地安全 OutcomeUnknown envelope。
    if observe_outcome_unknown().is_ok() {
        // 不声称 broker wire 已产生 unknown 终态。
        write_output(json!({
            // 明确这是调用方的非成功观察。
            "ok": false,
            // 只输出固定、安全且无 details 的本地 unknown envelope。
            "error": {
                // 使用冻结的本地 unknown 代码。
                "code": "OUTCOME_UNKNOWN",
                // 保持生产 Adapter 同源安全说明。
                "message": OUTCOME_UNKNOWN_MESSAGE
            }
        }));
        // OutcomeUnknown 是调用失败观察，使用稳定失败退出码。
        return 2;
    }
    // 夹具本身无法完成时只输出独立固定安全失败。
    write_output(json!({ "ok": false, "error": { "code": "FIXTURE_FAILED" } }));
    // 返回稳定夹具失败退出码。
    2
}
