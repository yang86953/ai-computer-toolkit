//! 封闭 Windows browser-session Broker Adapter 可发送的业务字段。

// 导入 broker provider-neutral selector、等待条件与严格 request frame。
use crate::components::browser_session_broker_protocol::{
    // 导入 provider-neutral selector。
    BrowserSemanticSelector,
    // 导入协议失败。
    BrowserSessionBrokerProtocolFailure,
    // 导入有限等待条件。
    BrowserWaitCondition,
    // 导入严格 request frame。
    wire::BrowserSessionBrokerRequestFrame,
};

// 表示 Adapter 允许构造的五个 Command 与四个 Query。
pub(super) enum BrowserSessionExchange<'a> {
    // 表示无目标且已经确认的会话打开。
    OpenConfirmed,
    // 表示绑定公开 opaque session 且已经确认的会话关闭。
    CloseConfirmed(&'a str),
    // 表示绑定公开 opaque session 的只读存活查询。
    SessionInspect(&'a str),
    // 表示绑定公开 session 与 URL 的确认式导航。
    NavigateConfirmed {
        // 借用公开 session identity。
        session_id: &'a str,
        // 借用目标 HTTP(S) URL。
        url: &'a str,
    },
    // 表示绑定当前 page 与有限条件的只读等待。
    Wait {
        // 借用公开 session identity。
        session_id: &'a str,
        // 借用当前公开 page identity。
        page_id: &'a str,
        // 借用有限等待条件。
        condition: &'a BrowserWaitCondition,
    },
    // 表示绑定当前 page 与 provider-neutral selector 的只读查询。
    Query {
        // 借用公开 session identity。
        session_id: &'a str,
        // 借用当前公开 page identity。
        page_id: &'a str,
        // 借用 provider-neutral selector。
        selector: &'a BrowserSemanticSelector,
        // 保存有界结果上限。
        max_results: u16,
    },
    // 表示绑定当前三级 identity 的确认式点击。
    ClickConfirmed {
        // 借用公开 session identity。
        session_id: &'a str,
        // 借用当前公开 page identity。
        page_id: &'a str,
        // 借用当前公开 element identity。
        element_id: &'a str,
    },
    // 表示绑定当前三级 identity 与文本的确认式输入。
    TypeConfirmed {
        // 借用公开 session identity。
        session_id: &'a str,
        // 借用当前公开 page identity。
        page_id: &'a str,
        // 借用当前公开 element identity。
        element_id: &'a str,
        // 借用有界 UTF-8 文本。
        text: &'a str,
        // 保存显式替换语义。
        replace: bool,
    },
    // 表示绑定当前页面的无确认截图 Query。
    Screenshot {
        // 借用公开 session identity。
        session_id: &'a str,
        // 借用当前公开 page identity。
        page_id: &'a str,
    },
}

// 为字段封闭的 exchange 提供角色真值。
impl BrowserSessionExchange<'_> {
    // 返回 operation 是否可能改变目标。
    pub(super) const fn may_mutate_target(&self) -> bool {
        // 只有五个 Command 可能改变目标。
        matches!(
            // 检查封闭 exchange 变体。
            self,
            // open 创建新会话。
            Self::OpenConfirmed
                // close 回收 live 会话。
                | Self::CloseConfirmed(_)
                // navigate 推进页面代际。
                | Self::NavigateConfirmed { .. }
                // click 改变页面交互状态。
                | Self::ClickConfirmed { .. }
                // type 改变当前元素值。
                | Self::TypeConfirmed { .. }
        )
    }
}

// 用当前未派发尝试的剩余预算构造严格 request frame。
pub(super) fn build_request_frame(
    // 借用字段封闭的业务选择。
    command: &BrowserSessionExchange<'_>,
    // 借用 ready 后唯一生成的 secure nonce。
    nonce: &str,
    // 借用唯一允许恢复的 ready epoch。
    epoch: &str,
    // 接收本次 WriteFile 前向下取整的剩余预算。
    remaining_timeout_ms: u32,
) -> Result<BrowserSessionBrokerRequestFrame, BrowserSessionBrokerProtocolFailure> {
    // 依 exchange 角色构造 confirmed Command 或无确认 Query。
    match command {
        // 构造无 target 的 confirmed open frame。
        BrowserSessionExchange::OpenConfirmed => {
            // 委托严格 wire builder。
            BrowserSessionBrokerRequestFrame::open_confirmed(
                // 绑定唯一 nonce。
                nonce,
                // 绑定认证 epoch。
                epoch,
                // 传递当前剩余预算。
                remaining_timeout_ms,
            )
        }
        // 构造绑定公开 session 的 confirmed close frame。
        BrowserSessionExchange::CloseConfirmed(session_id) => {
            // 委托严格 wire builder。
            BrowserSessionBrokerRequestFrame::close_confirmed(
                // 绑定唯一 nonce。
                nonce,
                // 绑定认证 epoch。
                epoch,
                // 传递当前剩余预算。
                remaining_timeout_ms,
                // 只传公开 session identity。
                session_id,
            )
        }
        // 构造不带确认字段的 session.inspect frame。
        BrowserSessionExchange::SessionInspect(session_id) => {
            // 委托严格 wire builder。
            BrowserSessionBrokerRequestFrame::session_inspect(
                // 绑定唯一 nonce。
                nonce,
                // 绑定认证 epoch。
                epoch,
                // 传递当前剩余预算。
                remaining_timeout_ms,
                // 只传公开 session identity。
                session_id,
            )
        }
        // 构造绑定公开 session 与 URL 的 confirmed navigate frame。
        BrowserSessionExchange::NavigateConfirmed { session_id, url } => {
            // 委托严格 wire builder。
            BrowserSessionBrokerRequestFrame::navigate_confirmed(
                // 绑定唯一 nonce。
                nonce,
                // 绑定认证 epoch。
                epoch,
                // 传递当前剩余预算。
                remaining_timeout_ms,
                // 只传公开 session identity。
                session_id,
                // 传递 parser 将复核的 URL。
                url,
            )
        }
        // 构造绑定当前 page 与有限条件的 wait Query。
        BrowserSessionExchange::Wait {
            // 借用公开 session。
            session_id,
            // 借用当前 page。
            page_id,
            // 借用有限条件。
            condition,
        } => {
            // 委托严格 wire builder。
            BrowserSessionBrokerRequestFrame::wait(
                // 绑定唯一 nonce。
                nonce,
                // 绑定认证 epoch。
                epoch,
                // 传递当前剩余预算。
                remaining_timeout_ms,
                // 只传公开 session identity。
                session_id,
                // 只传公开 page identity。
                page_id,
                // 传递有限条件。
                condition,
            )
        }
        // 构造绑定当前 page 与 selector 的 query Query。
        BrowserSessionExchange::Query {
            // 借用公开 session。
            session_id,
            // 借用当前 page。
            page_id,
            // 借用 provider-neutral selector。
            selector,
            // 复制有界结果上限。
            max_results,
        } => {
            // 委托严格 wire builder。
            BrowserSessionBrokerRequestFrame::query(
                // 绑定唯一 nonce。
                nonce,
                // 绑定认证 epoch。
                epoch,
                // 传递当前剩余预算。
                remaining_timeout_ms,
                // 只传公开 session identity。
                session_id,
                // 只传公开 page identity。
                page_id,
                // 传递 provider-neutral selector。
                selector,
                // 传递有界结果上限。
                *max_results,
            )
        }
        // 构造绑定当前三级 target 的 confirmed click frame。
        BrowserSessionExchange::ClickConfirmed {
            // 借用公开 session。
            session_id,
            // 借用当前 page。
            page_id,
            // 借用当前 element。
            element_id,
        } => {
            // 委托严格 wire builder。
            BrowserSessionBrokerRequestFrame::click_confirmed(
                // 绑定唯一 nonce。
                nonce,
                // 绑定认证 epoch。
                epoch,
                // 传递当前剩余预算。
                remaining_timeout_ms,
                // 只传公开 session identity。
                session_id,
                // 只传当前 page identity。
                page_id,
                // 只传当前 element identity。
                element_id,
            )
        }
        // 构造绑定当前三级 target 与文本的 confirmed type frame。
        BrowserSessionExchange::TypeConfirmed {
            // 借用公开 session。
            session_id,
            // 借用当前 page。
            page_id,
            // 借用当前 element。
            element_id,
            // 借用有界文本。
            text,
            // 复制显式替换语义。
            replace,
        } => {
            // 委托严格 wire builder。
            BrowserSessionBrokerRequestFrame::type_confirmed(
                // 绑定唯一 nonce。
                nonce,
                // 绑定认证 epoch。
                epoch,
                // 传递当前剩余预算。
                remaining_timeout_ms,
                // 只传公开 session identity。
                session_id,
                // 只传当前 page identity。
                page_id,
                // 只传当前 element identity。
                element_id,
                // 传递 parser 将复核的文本。
                text,
                // 保留显式替换语义。
                *replace,
            )
        }
        // 构造不含 confirmed 的当前页面 screenshot Query。
        BrowserSessionExchange::Screenshot {
            // 借用公开 session。
            session_id,
            // 借用当前 page。
            page_id,
        } => {
            // 委托严格 wire builder。
            BrowserSessionBrokerRequestFrame::screenshot(
                // 绑定唯一 nonce。
                nonce,
                // 绑定认证 epoch。
                epoch,
                // 传递当前剩余预算。
                remaining_timeout_ms,
                // 只传公开 session identity。
                session_id,
                // 只传当前 page identity。
                page_id,
            )
        }
    }
}
