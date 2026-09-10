//! 投影页面点击、输入与截图的 ComputerControlSystem 私有纵切。

// 导入不可扩张的执行预算。
use std::time::Duration;

// 导入 provider-neutral JSON 结果。
use serde_json::{Value, json};

// 导入统一内部结果。
use crate::domain::AppResult;
// 导入 Browser Session Module 的封闭领域类型。
use crate::modules::browser_session::{
    // 导入点击成功事实。
    BrowserClickData,
    // 导入截图成功数据。
    BrowserScreenshotData,
    // 导入页面命令类别。
    BrowserSessionCommandOutcome,
    // 导入通用页面命令报告。
    BrowserSessionCommandReport,
    // 导入输入成功事实。
    BrowserTypeData,
    // 导入输入请求领域值。
    BrowserTypeRequest,
};

// 导入父 System 的业务前错误类型。
use super::BrowserSessionBrokerPreflightError;
// 导入唯一允许协调 Module 的 System。
use super::super::AppControlService;

// 保存 broker 可安全公开的点击执行投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerClickExecution {
    // 表示当前元素已可信完成点击。
    Completed {
        // 保存 request-bound page identity。
        page_id: String,
        // 保存 request-bound element identity。
        element_id: String,
        // 保存 Module 报告的正导航代际。
        generation: u32,
    },
    // 表示 accepted 后取得确定失败。
    Failed,
    // 表示 accepted 后无法取得可信 final。
    Unknown,
}

// 保存 broker 可安全公开的文本输入执行投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerTypeExecution {
    // 表示当前元素已可信完成输入。
    Completed {
        // 保存 request-bound page identity。
        page_id: String,
        // 保存 request-bound element identity。
        element_id: String,
        // 保存 Module 报告的正导航代际。
        generation: u32,
        // 保存已输入的 UTF-8 字节数。
        utf8_bytes: u16,
    },
    // 表示 accepted 后取得确定失败。
    Failed,
    // 表示 accepted 后无法取得可信 final。
    Unknown,
}

// 保存 broker 可安全公开的页面截图执行投影。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BrowserSessionBrokerScreenshotExecution {
    // 表示 Module 已验证并投影有界 PNG。
    Completed {
        // 保存由 Broker response Component 再验证的封闭对象。
        data: Value,
    },
    // 表示 accepted 后取得确定失败。
    Failed,
    // 表示 accepted 后无法取得可信 final。
    Unknown,
}

// 仅由 System 协调 Browser Session Module 的页面动作入口。
impl AppControlService {
    // 在 broker business acceptance 前验证当前三级 identity。
    pub(crate) fn prepare_browser_session_broker_element(
        // 只读借用唯一 System，预检不得触碰 worker I/O。
        &self,
        // 借用公开 session identity。
        session_id: &str,
        // 借用公开 page identity。
        page_id: &str,
        // 借用公开 element identity。
        element_id: &str,
    ) -> Result<(), BrowserSessionBrokerPreflightError> {
        // 只由 Module 读取唯一 identity registry。
        self.browser_sessions
            // 依固定优先级验证三级 target。
            .prepare_element(session_id, page_id, element_id)
            // 只投影冻结 stale 集合。
            .map_err(|error| match error.code {
                // session 缺失必须优先。
                "STALE_SESSION" => BrowserSessionBrokerPreflightError::StaleSession,
                // page 失效必须次优先。
                "STALE_PAGE" => BrowserSessionBrokerPreflightError::StalePage,
                // element 失效只在当前 page 内成立。
                "STALE_ELEMENT" => BrowserSessionBrokerPreflightError::StaleElement,
                // 其他 Module 错误不得伪装为业务前拒绝。
                _ => BrowserSessionBrokerPreflightError::Internal,
            })
    }

    // 在 broker accepted 后点击当前公开元素。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_browser_session_broker_click(
        // 可变借用唯一 System。
        &mut self,
        // 借用已预检 session。
        session_id: &str,
        // 借用已预检 page。
        page_id: &str,
        // 借用已预检 element。
        element_id: &str,
        // 接收 broker 截断后的剩余总预算。
        timeout: Duration,
        // 接收 broker 协作停止观察。
        cancelled: impl Fn() -> bool,
    ) -> BrowserSessionBrokerClickExecution {
        // 由 Module 唯一解析 identity 并派发 worker。
        project_click_execution(
            // 绑定 request-known identity 用于成功投影。
            page_id,
            // 绑定 request-known element。
            element_id,
            // Module 内部仍必须再次确认。
            self.browser_sessions
                .click(session_id, page_id, element_id, true, timeout, cancelled),
        )
    }

    // 在 broker accepted 后向当前公开元素输入文本。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_browser_session_broker_type(
        // 可变借用唯一 System。
        &mut self,
        // 借用已预检 session。
        session_id: &str,
        // 借用已预检 page。
        page_id: &str,
        // 借用已预检 element。
        element_id: &str,
        // 借用 parser 已验证的有界文本。
        text: &str,
        // 接收显式替换语义。
        replace: bool,
        // 接收 broker 截断后的剩余总预算。
        timeout: Duration,
        // 接收 broker 协作停止观察。
        cancelled: impl Fn() -> bool,
    ) -> BrowserSessionBrokerTypeExecution {
        // 由 Module 唯一解析 identity 并派发 worker。
        project_type_execution(
            // 绑定 request-known page。
            page_id,
            // 绑定 request-known element。
            element_id,
            // Module 请求保留确认、文本与替换领域值。
            self.browser_sessions.type_text(
                // 传入公开 session。
                session_id,
                // 传入公开 page。
                page_id,
                // 传入公开 element。
                element_id,
                // 构造不含 native 输入事实的领域请求。
                BrowserTypeRequest::new(text.to_owned(), replace, true),
                // 传入剩余预算。
                timeout,
                // 传入协作停止观察。
                cancelled,
            ),
        )
    }

    // 在 broker accepted 后捕获当前页面的有界 PNG。
    pub(crate) fn execute_browser_session_broker_screenshot(
        // 可变借用唯一 System。
        &mut self,
        // 借用已预检 session。
        session_id: &str,
        // 借用已预检 page。
        page_id: &str,
        // 接收 broker 截断后的剩余总预算。
        timeout: Duration,
        // 接收 broker 协作停止观察。
        cancelled: impl Fn() -> bool,
    ) -> BrowserSessionBrokerScreenshotExecution {
        // Module 唯一拥有 page/worker 和 PNG 聚合。
        project_screenshot_execution(
            // 绑定 request-known page。
            page_id,
            // 执行固定无参数截图 Query。
            self.browser_sessions
                .screenshot(session_id, page_id, timeout, cancelled),
        )
    }
}

// 将 Module 点击报告映射为 request-bound broker 投影。
fn project_click_execution(
    // 借用已预检 page。
    page_id: &str,
    // 借用已预检 element。
    element_id: &str,
    // 接收 Module 封闭报告。
    result: AppResult<BrowserSessionCommandReport<BrowserClickData>>,
) -> BrowserSessionBrokerClickExecution {
    // 只在完成位、动作事实和正代际一致时公开成功。
    match result {
        // completed 必须携带 clicked=true。
        Ok(report)
            if report.outcome() == BrowserSessionCommandOutcome::Completed
                && report.completed()
                && report.data().is_some_and(BrowserClickData::clicked) =>
        {
            // 读取可无损收窄的正代际。
            let Some(generation) = positive_generation(&report) else {
                // 内部成功漂移只能保守 unknown。
                return BrowserSessionBrokerClickExecution::Unknown;
            };
            // 返回不含 worker 或 native 事实的成功投影。
            BrowserSessionBrokerClickExecution::Completed {
                // 回显原 page。
                page_id: page_id.to_owned(),
                // 回显原 element。
                element_id: element_id.to_owned(),
                // 保存正代际。
                generation,
            }
        }
        // Module 丢失可信 final 时保持未知。
        Ok(report) if report.outcome() == BrowserSessionCommandOutcome::Unknown => {
            // mutation Command 不得伪造失败或安全重试。
            BrowserSessionBrokerClickExecution::Unknown
        }
        // 确定未派发、失败或同步错误形成 failed。
        Ok(_) | Err(_) => BrowserSessionBrokerClickExecution::Failed,
    }
}

// 将 Module 文本输入报告映射为 request-bound broker 投影。
fn project_type_execution(
    // 借用已预检 page。
    page_id: &str,
    // 借用已预检 element。
    element_id: &str,
    // 接收 Module 封闭报告。
    result: AppResult<BrowserSessionCommandReport<BrowserTypeData>>,
) -> BrowserSessionBrokerTypeExecution {
    // 只在完成位、动作事实、字节数和正代际一致时公开成功。
    match result {
        // completed 必须携带 typed=true。
        Ok(report)
            if report.outcome() == BrowserSessionCommandOutcome::Completed
                && report.completed()
                && report.data().is_some_and(BrowserTypeData::typed) =>
        {
            // 读取可无损收窄的正代际。
            let Some(generation) = positive_generation(&report) else {
                // 内部成功漂移只能保守 unknown。
                return BrowserSessionBrokerTypeExecution::Unknown;
            };
            // 读取有界 UTF-8 字节数。
            let Some(utf8_bytes) = report
                // 借用可信数据。
                .data()
                // 将 u32 无损收窄到 Broker u16。
                .and_then(|data| u16::try_from(data.utf8_bytes()).ok())
                // 字节数必须为正。
                .filter(|bytes| *bytes > 0)
            else {
                // 成功数据漂移只能保守 unknown。
                return BrowserSessionBrokerTypeExecution::Unknown;
            };
            // 返回不含原始文本的成功投影。
            BrowserSessionBrokerTypeExecution::Completed {
                // 回显原 page。
                page_id: page_id.to_owned(),
                // 回显原 element。
                element_id: element_id.to_owned(),
                // 保存正代际。
                generation,
                // 保存字节数。
                utf8_bytes,
            }
        }
        // Module 丢失可信 final 时保持未知。
        Ok(report) if report.outcome() == BrowserSessionCommandOutcome::Unknown => {
            // mutation Command 不得伪造失败或安全重试。
            BrowserSessionBrokerTypeExecution::Unknown
        }
        // 确定未派发、失败或同步错误形成 failed。
        Ok(_) | Err(_) => BrowserSessionBrokerTypeExecution::Failed,
    }
}

// 将 Module 截图报告映射为 request-bound broker JSON。
fn project_screenshot_execution(
    // 借用已预检 page。
    page_id: &str,
    // 接收 Module 封闭报告。
    result: AppResult<BrowserSessionCommandReport<BrowserScreenshotData>>,
) -> BrowserSessionBrokerScreenshotExecution {
    // 只在完成位、PNG 数据与正代际一致时公开成功。
    match result {
        // completed 必须携带截图数据。
        Ok(report)
            if report.outcome() == BrowserSessionCommandOutcome::Completed
                && report.completed() =>
        {
            // 读取可信截图数据。
            let Some(data) = report.data() else {
                // 不伪造空 PNG。
                return BrowserSessionBrokerScreenshotExecution::Unknown;
            };
            // 读取可无损收窄的正代际。
            let Some(generation) = positive_generation(&report) else {
                // 内部成功漂移只能保守 unknown。
                return BrowserSessionBrokerScreenshotExecution::Unknown;
            };
            // 返回由 Broker response Component 再验证的封闭对象。
            BrowserSessionBrokerScreenshotExecution::Completed {
                // 只投影公开 PNG 与 request-bound page/代际。
                data: json!({
                    // 回显原 page。
                    "pageId": page_id,
                    // 保存正代际。
                    "navigationGeneration": generation,
                    // MIME 固定来自 Module。
                    "mimeType": data.mime_type(),
                    // 复制有界 Base64。
                    "pngBase64": data.png_base64(),
                    // 保存真实 PNG 字节数。
                    "pngBytes": data.png_bytes(),
                    // 保存 IHDR 宽度。
                    "width": data.width(),
                    // 保存 IHDR 高度。
                    "height": data.height(),
                    // 保存稳定摘要。
                    "digest": data.digest()
                }),
            }
        }
        // Module 丢失可信 final 时保持只读未知。
        Ok(report) if report.outcome() == BrowserSessionCommandOutcome::Unknown => {
            // Query unknown 不得伪造 target mutation。
            BrowserSessionBrokerScreenshotExecution::Unknown
        }
        // 确定未派发、失败或同步错误形成 failed。
        Ok(_) | Err(_) => BrowserSessionBrokerScreenshotExecution::Failed,
    }
}

// 从任意页面命令报告读取可无损收窄的正导航代际。
fn positive_generation<T>(report: &BrowserSessionCommandReport<T>) -> Option<u32> {
    // 将 Module u64 代际无损收窄并拒绝零。
    u32::try_from(report.navigation_generation())
        // 收窄失败返回 None。
        .ok()
        // 零代际不能形成公开成功。
        .filter(|generation| *generation > 0)
}
