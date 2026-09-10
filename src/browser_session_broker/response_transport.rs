//! 封闭 browser-session broker 的响应编码、绝对截止时间选择与有界 transport 写入。

// 导入单调绝对时间与固定 transport 预算。
use std::time::{Duration, Instant};

// 导入固定 IPC、严格响应协议与共享运行时。
use crate::{
    // 导入不泄漏原生句柄的已连接 pipe owner。
    adapters::fixed_local_ipc_windows::pipe::ConnectedPipe,
    // 导入严格 request、response、连接状态与 wire codec。
    components::{
        // 导入冻结的 browser-session broker 协议。
        browser_session_broker_protocol::{
            // 导入严格 request。
            BrowserSessionBrokerRequest,
            // 导入请求绑定 response。
            response::BrowserSessionBrokerResponse,
            // 导入单连接 revision 状态机。
            state::BrowserSessionBrokerConnection,
            // 导入请求绑定响应编码器。
            wire::encode_response,
        },
        // 导入代际唯一 deadline ledger。
        browser_session_broker_runtime::BrowserSessionBrokerRuntime,
    },
};

// 导入宿主统一停止事实。
use super::should_stop;

// 未绑定请求 ledger 的控制帧仍只能使用固定单帧 transport 上限。
const TRANSPORT_CONNECTION_TIMEOUT: Duration = Duration::from_secs(30);

// 用连接状态机验证并发送一个 strict request-bound response。
pub(super) fn send_response(
    // 借用当前 connected secondary。
    pipe: &ConnectedPipe,
    // 借用原始严格 request。
    request: &BrowserSessionBrokerRequest,
    // 可变借用本连接 response 状态机。
    connection: &mut BrowserSessionBrokerConnection,
    // 借用待发送封闭 response。
    response: &BrowserSessionBrokerResponse,
    // 接收完整 frame 到达时建立的不可重置上限。
    request_deadline: Instant,
    // 借用 current epoch runtime。
    runtime: &BrowserSessionBrokerRuntime,
) -> bool {
    // 先验证 nonce、operation、revision 与 phase。
    if connection.apply_response(response).is_err() {
        // 拒绝状态漂移。
        return false;
    }
    // codec 逐字绑定 strict request 与 current epoch。
    let encoded = match encode_response(
        // 借用封闭 response。
        response,
        // strict 路径必须提供原 request。
        Some(request),
        // 绑定认证 current epoch。
        runtime.epoch(),
    ) {
        // 保存有界 JSON 文本。
        Ok(encoded) => encoded,
        // 编码不变量失败闭合。
        Err(_) => return false,
    };
    // 读取 ledger 当前最短绝对 deadline。
    let ledger_deadline = match runtime.execution_deadline(request.request_nonce()) {
        // 保存存在或尚未预留 entry 的事实。
        Ok(deadline) => deadline,
        // runtime 不可信时不得让 request-bound 回包重置 deadline。
        Err(_) => return false,
    };
    // ledger 拒绝也必须沿用完整 frame 建立的请求预算。
    let deadline = request_response_deadline(ledger_deadline, request_deadline);
    // 在选定的不可延长终点前写出唯一完整 message。
    write_text_until(pipe, &encoded, deadline, runtime)
}

// 为 request-bound 响应选择不可重置的绝对截止时间。
fn request_response_deadline(
    // 接收 ledger 已有 entry 的当前最短截止时间。
    ledger_deadline: Option<Instant>,
    // 接收完整 request frame 建立的物理截止时间。
    request_deadline: Instant,
) -> Instant {
    // 无 entry 时仍使用 request deadline；有 entry 时只允许进一步缩短。
    ledger_deadline.map_or(request_deadline, |deadline| deadline.min(request_deadline))
}

// 在固定 transport 预算内写入一条非 request-bound broker 文本帧。
pub(super) fn write_transport_text(
    // 借用当前连接。
    pipe: &ConnectedPipe,
    // 借用已经严格 codec 生成的文本。
    text: &str,
    // 借用宿主停止事实。
    runtime: &BrowserSessionBrokerRuntime,
) -> bool {
    // 为当前单帧写入建立固定单调上限。
    let Some(deadline) = Instant::now().checked_add(TRANSPORT_CONNECTION_TIMEOUT) else {
        // 理论时钟溢出不得进入无界写入。
        return false;
    };
    // 委托统一有界写入，避免控制帧停止语义漂移。
    write_text_until(pipe, text, deadline, runtime)
}

// 在给定绝对 deadline 内写入一条 broker 文本帧。
fn write_text_until(
    // 借用当前连接。
    pipe: &ConnectedPipe,
    // 借用已经严格 codec 生成的文本。
    text: &str,
    // 接收调用方已选定的绝对单调 deadline。
    deadline: Instant,
    // 借用宿主停止事实。
    runtime: &BrowserSessionBrokerRuntime,
) -> bool {
    // 非阻塞 message 写入在 deadline 或宿主停止时失败闭合。
    pipe.write_text_until(text, deadline, || should_stop(runtime))
        // 只向上层返回写入是否完整。
        .is_ok()
}

// 覆盖不创建 pipe 或 System 的绝对截止时间选择。
#[cfg(test)]
mod tests {
    // 导入单调时间与测试偏移。
    use std::time::{Duration, Instant};

    // 导入被测纯选择函数。
    use super::request_response_deadline;

    // 验证 ledger 满而没有 entry 时不会重新获得 transport 预算。
    #[test]
    fn missing_ledger_entry_uses_received_request_deadline() {
        // 记录稳定测试起点。
        let now = Instant::now();
        // 模拟完整 frame 只剩一毫秒的绝对预算。
        let request_deadline = now + Duration::from_millis(1);
        // 无 ledger entry 必须逐值保留原请求截止时间。
        assert_eq!(
            request_response_deadline(None, request_deadline),
            request_deadline
        );
    }

    // 验证 ledger 与物理 request deadline 始终只选更早者。
    #[test]
    fn ledger_deadline_can_only_shorten_received_request_deadline() {
        // 记录稳定测试起点。
        let now = Instant::now();
        // 建立物理 frame 的原始截止时间。
        let request_deadline = now + Duration::from_millis(20);
        // 建立 ledger 已被同 nonce 重送缩短的截止时间。
        let shortened = now + Duration::from_millis(5);
        // 已缩短 ledger deadline 必须优先。
        assert_eq!(
            // 执行纯选择。
            request_response_deadline(Some(shortened), request_deadline),
            // 保持缩短后的绝对终点。
            shortened,
        );
        // 晚于物理 frame 的 ledger deadline 不得延长请求预算。
        assert_eq!(
            // 模拟理论较晚 ledger 终点。
            request_response_deadline(
                // ledger 晚于 request。
                Some(now + Duration::from_millis(40)),
                // 传入原 request 上限。
                request_deadline,
            ),
            // 仍保持原 request 上限。
            request_deadline,
        );
    }
}
