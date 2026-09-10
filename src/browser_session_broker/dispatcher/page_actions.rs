//! 串行协调页面点击、输入与截图，不拥有 identity 或 worker 生命周期。

// 导入绝对 deadline 的剩余预算类型。
use std::time::Duration;

// 导入严格协议、共享运行时与唯一 System。
use crate::{
    // 导入 broker 领域 request 与响应。
    components::{
        // 导入 strict request 与可关联失败。
        browser_session_broker_protocol::{
            // 导入协议失败边界。
            BrowserSessionBrokerProtocolFailure,
            // 导入严格领域 request。
            BrowserSessionBrokerRequest,
            // 导入 final outcome、response 与 operation success。
            response::{
                // 导入封闭 final outcome。
                BrowserSessionBrokerOutcome,
                // 导入 accepted/final response。
                BrowserSessionBrokerResponse,
                // 导入 operation-specific success。
                BrowserSessionBrokerSuccess,
            },
        },
        // 导入只持有 execution ledger 的共享 runtime。
        browser_session_broker_runtime::BrowserSessionBrokerRuntime,
    },
    // 导入唯一 System 与页面动作执行投影。
    service::{
        // 导入 System 本体。
        AppControlService,
        // 导入封闭业务前错误与执行投影。
        browser_session_broker::{
            // 导入点击执行投影。
            BrowserSessionBrokerClickExecution,
            // 导入业务前错误。
            BrowserSessionBrokerPreflightError,
            // 导入截图执行投影。
            BrowserSessionBrokerScreenshotExecution,
            // 导入输入执行投影。
            BrowserSessionBrokerTypeExecution,
        },
    },
};

// 对页面动作执行无 worker I/O 的 business-preflight。
pub(super) fn preflight(
    // 可变借用唯一 System，只允许其协调 Module。
    system: &mut AppControlService,
    // 借用 strict request。
    request: &BrowserSessionBrokerRequest,
) -> Option<Result<(), BrowserSessionBrokerPreflightError>> {
    // 只匹配本 Component 拥有的三个 operation。
    match request {
        // click 必须在 accepted 前验证三级 identity。
        BrowserSessionBrokerRequest::Click(_, session_id, page_id, element_id) => Some(
            // 委托 System 的纯预检入口。
            system.prepare_browser_session_broker_element(session_id, page_id, element_id),
        ),
        // type 使用与 click 相同的 identity 优先级。
        BrowserSessionBrokerRequest::Type(_, session_id, page_id, element_id, _, _) => Some(
            // 文本已由 parser 验证，预检只验证 target。
            system.prepare_browser_session_broker_element(session_id, page_id, element_id),
        ),
        // screenshot 只绑定当前 session/page。
        BrowserSessionBrokerRequest::Screenshot(_, session_id, page_id) => Some(
            // 委托既有页面纯预检入口。
            system.prepare_browser_session_broker_page(session_id, page_id),
        ),
        // 其他 operation 不属于本 Component。
        _ => None,
    }
}

// 在 accepted 后恰好一次调用页面动作 System 入口并构造终态。
pub(super) fn execute(
    // 可变借用唯一 System。
    system: &mut AppControlService,
    // 借用共享 runtime，不持锁调用 Module。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用已 accepted strict request。
    request: &BrowserSessionBrokerRequest,
    // 接收不可延长绝对 deadline。
    deadline_ms: u64,
) -> Option<Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure>> {
    // 只匹配本 Component 拥有的三个 operation。
    match request {
        // 执行 confirmation-first click Command。
        BrowserSessionBrokerRequest::Click(_, session_id, page_id, element_id) => {
            // 复制 nonce 供不持锁取消 closure 查询。
            let request_nonce = request.request_nonce().to_owned();
            // 计算 accepted 后当前剩余预算。
            let timeout = remaining_duration(runtime.now_ms(), deadline_ms);
            // 由唯一 System 调用 Module 的 click 入口。
            let execution = system.execute_browser_session_broker_click(
                // 传入已预检 session。
                session_id,
                // 传入已预检 page。
                page_id,
                // 传入已预检 element。
                element_id,
                // 传入不可延长剩余预算。
                timeout,
                // 每次轮询只短锁读取取消与 deadline 事实。
                || {
                    runtime
                        .execution_should_stop(&request_nonce)
                        .unwrap_or(true)
                },
            );
            // 投影 mutation=true 的封闭终态。
            Some(click_final(runtime, request, execution))
        }
        // 执行 confirmation-first type Command。
        BrowserSessionBrokerRequest::Type(_, session_id, page_id, element_id, text, replace) => {
            // 复制 nonce 供不持锁取消 closure 查询。
            let request_nonce = request.request_nonce().to_owned();
            // 计算 accepted 后当前剩余预算。
            let timeout = remaining_duration(runtime.now_ms(), deadline_ms);
            // 由唯一 System 调用 Module 的 type 入口。
            let execution = system.execute_browser_session_broker_type(
                // 传入已预检 session。
                session_id,
                // 传入已预检 page。
                page_id,
                // 传入已预检 element。
                element_id,
                // 传入 parser 已验证文本。
                text,
                // 保留显式 replace 语义。
                *replace,
                // 传入不可延长剩余预算。
                timeout,
                // 每次轮询只短锁读取取消与 deadline 事实。
                || {
                    runtime
                        .execution_should_stop(&request_nonce)
                        .unwrap_or(true)
                },
            );
            // 投影 mutation=true 的封闭终态。
            Some(type_final(runtime, request, execution))
        }
        // 执行不改变目标的 screenshot Query。
        BrowserSessionBrokerRequest::Screenshot(_, session_id, page_id) => {
            // 复制 nonce 供不持锁取消 closure 查询。
            let request_nonce = request.request_nonce().to_owned();
            // 计算 accepted 后当前剩余预算。
            let timeout = remaining_duration(runtime.now_ms(), deadline_ms);
            // 由唯一 System 调用 Module 的 screenshot 入口。
            let execution = system.execute_browser_session_broker_screenshot(
                // 传入已预检 session。
                session_id,
                // 传入已预检 page。
                page_id,
                // 传入不可延长剩余预算。
                timeout,
                // 每次轮询只短锁读取取消与 deadline 事实。
                || {
                    runtime
                        .execution_should_stop(&request_nonce)
                        .unwrap_or(true)
                },
            );
            // 投影 mutation=false 的封闭终态。
            Some(screenshot_final(runtime, request, execution))
        }
        // 其他 operation 不属于本 Component。
        _ => None,
    }
}

// 将 click 执行投影为封闭 Command 终态。
fn click_final(
    // 借用 current epoch runtime。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用原始 click request。
    request: &BrowserSessionBrokerRequest,
    // 接收 System click 执行投影。
    execution: BrowserSessionBrokerClickExecution,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 穷举点击执行结果。
    match execution {
        // completed 携带 request-bound identity 与正代际。
        BrowserSessionBrokerClickExecution::Completed {
            // 取得当前 page。
            page_id,
            // 取得当前 element。
            element_id,
            // 取得当前导航代际。
            generation,
        } => super::finished(
            // 绑定 runtime。
            runtime,
            // 绑定原 request。
            request,
            // 使用 completed outcome。
            BrowserSessionBrokerOutcome::Completed,
            // 保存 click 专属成功数据。
            Some(BrowserSessionBrokerSuccess::Click {
                // 回显当前 page。
                page_id,
                // 回显当前 element。
                element_id,
                // 保存正代际。
                generation,
            }),
        ),
        // 确定失败不携带成功数据。
        BrowserSessionBrokerClickExecution::Failed => super::finished(
            // 绑定 runtime。
            runtime,
            // 绑定原 request。
            request,
            // 使用 failed outcome。
            BrowserSessionBrokerOutcome::Failed,
            // 禁止伪造成功数据。
            None,
        ),
        // accepted 后无可信 final 必须保留 mutation=true unknown。
        BrowserSessionBrokerClickExecution::Unknown => Ok(BrowserSessionBrokerResponse::unknown(
            // 绑定原 click request。
            request,
            // accepted 后终态固定 revision 一。
            1,
            // 回显当前 epoch。
            runtime.epoch(),
        )),
    }
}

// 将 type 执行投影为封闭 Command 终态。
fn type_final(
    // 借用 current epoch runtime。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用原始 type request。
    request: &BrowserSessionBrokerRequest,
    // 接收 System type 执行投影。
    execution: BrowserSessionBrokerTypeExecution,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 穷举文本输入执行结果。
    match execution {
        // completed 携带 request-bound identity、代际与字节数。
        BrowserSessionBrokerTypeExecution::Completed {
            // 取得当前 page。
            page_id,
            // 取得当前 element。
            element_id,
            // 取得当前导航代际。
            generation,
            // 取得已输入字节数。
            utf8_bytes,
        } => super::finished(
            // 绑定 runtime。
            runtime,
            // 绑定原 request。
            request,
            // 使用 completed outcome。
            BrowserSessionBrokerOutcome::Completed,
            // 保存 type 专属成功数据，不回显文本。
            Some(BrowserSessionBrokerSuccess::Type {
                // 回显当前 page。
                page_id,
                // 回显当前 element。
                element_id,
                // 保存正代际。
                generation,
                // 保存有界字节数。
                utf8_bytes,
            }),
        ),
        // 确定失败不携带成功数据。
        BrowserSessionBrokerTypeExecution::Failed => super::finished(
            // 绑定 runtime。
            runtime,
            // 绑定原 request。
            request,
            // 使用 failed outcome。
            BrowserSessionBrokerOutcome::Failed,
            // 禁止伪造成功数据。
            None,
        ),
        // accepted 后无可信 final 必须保留 mutation=true unknown。
        BrowserSessionBrokerTypeExecution::Unknown => Ok(BrowserSessionBrokerResponse::unknown(
            // 绑定原 type request。
            request,
            // accepted 后终态固定 revision 一。
            1,
            // 回显当前 epoch。
            runtime.epoch(),
        )),
    }
}

// 将 screenshot 执行投影为封闭 Query 终态。
fn screenshot_final(
    // 借用 current epoch runtime。
    runtime: &BrowserSessionBrokerRuntime,
    // 借用原始 screenshot request。
    request: &BrowserSessionBrokerRequest,
    // 接收 System screenshot 执行投影。
    execution: BrowserSessionBrokerScreenshotExecution,
) -> Result<BrowserSessionBrokerResponse, BrowserSessionBrokerProtocolFailure> {
    // 穷举截图执行结果。
    match execution {
        // completed 携带 response Component 将再次验证的 PNG 对象。
        BrowserSessionBrokerScreenshotExecution::Completed { data } => super::finished(
            // 绑定 runtime。
            runtime,
            // 绑定原 request。
            request,
            // 使用 completed outcome。
            BrowserSessionBrokerOutcome::Completed,
            // 保存 screenshot 专属成功数据。
            Some(BrowserSessionBrokerSuccess::Screenshot(data)),
        ),
        // 确定失败不携带成功数据。
        BrowserSessionBrokerScreenshotExecution::Failed => super::finished(
            // 绑定 runtime。
            runtime,
            // 绑定原 request。
            request,
            // 使用 failed outcome。
            BrowserSessionBrokerOutcome::Failed,
            // 禁止伪造成功数据。
            None,
        ),
        // accepted 后无可信 final 保留 Query unknown 与 mutation=false。
        BrowserSessionBrokerScreenshotExecution::Unknown => {
            // 构造 request-bound unknown。
            Ok(BrowserSessionBrokerResponse::unknown(
                // 绑定原 screenshot request。
                request,
                // accepted 后终态固定 revision 一。
                1,
                // 回显当前 epoch。
                runtime.epoch(),
            ))
        }
    }
}

// 将服务器绝对 deadline 转为不会溢出的剩余时长。
fn remaining_duration(
    // 接收当前服务器单调时刻。
    now_ms: u64,
    // 接收不可延长绝对 deadline。
    deadline_ms: u64,
) -> Duration {
    // accepted 后零预算会让取消 closure 立即停止。
    Duration::from_millis(deadline_ms.saturating_sub(now_ms))
}
