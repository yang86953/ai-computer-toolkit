// 导入固定 deadline 的 Rust 时间类型。
use std::time::Duration;

// 注册页面动作与截图的窄 System 投影子 Module。
#[path = "browser_session_broker_page_actions.rs"]
mod page_actions;
// 向唯一 broker dispatcher 重导出封闭执行投影。
pub(crate) use page_actions::{
    BrowserSessionBrokerClickExecution, BrowserSessionBrokerScreenshotExecution,
    BrowserSessionBrokerTypeExecution,
};

// 导入 provider-neutral 查询结果构造器和值类型。
use serde_json::{Value, json};

// 导入 broker parser 已验证的 selector 与等待条件。
use crate::components::browser_session_broker_protocol::{
    // 导入 broker provider-neutral selector。
    BrowserSemanticSelector as BrokerSemanticSelector,
    // 导入 broker 有限等待条件。
    BrowserWaitCondition as BrokerWaitCondition,
};

// 导入统一内部错误结果。
use crate::domain::AppResult;
// 导入 Module 的封闭打开结果类别。
use crate::modules::browser_session::{
    // 导入公开元素摘要。
    BrowserElementMatch,
    // 导入导航报告。
    BrowserNavigationReport,
    // 导入 query 成功数据。
    BrowserQueryData,
    // 导入 Module provider-neutral selector。
    BrowserSemanticSelector as ModuleSemanticSelector,
    // 导入 Module 有限等待条件。
    BrowserSemanticWaitCondition as ModuleWaitCondition,
    // 导入 broker close 的有界回收投影。
    BrowserSessionCloseOutcome,
    // 导入页面命令封闭类别。
    BrowserSessionCommandOutcome,
    // 导入页面通用报告。
    BrowserSessionCommandReport,
    // 导入 session inspect 的最小只读事实。
    BrowserSessionInspection,
    // 导入 broker open 的既有投影。
    BrowserSessionOpenOutcome,
    // 导入 broker open 的既有报告。
    BrowserSessionOpenReport,
    // 导入 wait 成功数据。
    BrowserWaitData,
};

// 导入唯一允许协调 Module 的 System。
use super::AppControlService;

// 保存 broker 可安全公开的打开执行投影。
// #2059 后续 dispatcher 接入前保留已冻结的 System 边界。
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerOpenExecution {
    // 保存已创建且仍归 Module 所有的公开 session identity。
    Completed {
        // 保存 opaque session identity。
        session_id: String,
    },
    // 表示已接受后的确定失败。
    Failed,
    // 表示已接受后无法取得可信 final。
    Unknown,
}

// 保存 broker 可安全公开的关闭执行投影。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerCloseExecution {
    // 表示 Module 已移除 registry 并完成 worker/Job 回收。
    Completed,
    // 表示已接受后的确定关闭失败。
    Failed,
    // 表示已接受后无法在本次总预算内证明完整回收。
    Unknown,
}

// 保存 broker 可安全公开的会话查询执行投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerInspectExecution {
    // 表示 Module 在查询线性化点确认目标仍 live。
    Completed {
        // 保存公开 opaque session identity。
        session_id: String,
        // 保存不包含 worker 状态的 live 事实。
        live: bool,
    },
    // 表示 accepted 后发生不属于公开查询事实的内部失败。
    Failed,
}

// 保存 broker 可安全公开的导航执行投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerNavigateExecution {
    // 表示 Module 已换发当前 page identity 并推进代际。
    Completed {
        // 保存公开 opaque page identity。
        page_id: String,
        // 保存从一开始的导航代际。
        generation: u32,
    },
    // 表示 accepted 后取得确定失败。
    Failed,
    // 表示 accepted 后无法取得可信 final。
    Unknown,
}

// 保存 broker 可安全公开的等待执行投影。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerWaitExecution {
    // 表示有限条件已满足。
    Completed {
        // 保存调用时已经预检的当前公开 page identity。
        page_id: String,
        // 保存 Module 在完成报告中确认的正导航代际。
        generation: u32,
    },
    // 表示 accepted 后取得确定失败。
    Failed,
    // 表示 accepted 后无法取得可信 final。
    Unknown,
}

// 保存 broker 可安全公开的元素查询执行投影。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BrowserSessionBrokerQueryExecution {
    // 表示 Module 已签发有界 provider-neutral 元素摘要。
    Completed {
        // 保存已通过 Module 边界收敛的查询对象。
        data: Value,
    },
    // 表示 accepted 后取得确定查询失败。
    Failed,
    // 表示 accepted 后无法取得可信 final。
    Unknown,
}

// 保存业务接受前可安全投影或必须失败闭合的预检错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionBrokerPreflightError {
    // 表示 Module 的 live session registry 已达到固定上限。
    RegistryFull,
    // 表示关闭目标不属于当前 Module 代际。
    StaleSession,
    // 表示页面目标不属于当前 session 的当前导航代际。
    StalePage,
    // 表示元素目标不属于当前 page 的当前导航代际。
    StaleElement,
    // 表示预检返回了不属于冻结业务前拒绝集合的内部错误。
    Internal,
}

// 为预检错误提供稳定代码和拒绝资格判定。
impl BrowserSessionBrokerPreflightError {
    // 返回不含调用方输入或内部细节的稳定代码。
    #[cfg(test)]
    pub(crate) const fn code(self) -> &'static str {
        // 穷举封闭错误集合。
        match self {
            // 返回容量已满的稳定业务前错误码。
            Self::RegistryFull => "BROWSER_SESSION_REGISTRY_FULL",
            // 返回 stale 目标的稳定业务前错误码。
            Self::StaleSession => "STALE_SESSION",
            // 返回 stale page 的稳定业务前错误码。
            Self::StalePage => "STALE_PAGE",
            // 返回 stale element 的稳定业务前错误码。
            Self::StaleElement => "STALE_ELEMENT",
            // 返回只供 broker 失败闭合的内部错误码。
            Self::Internal => "BROKER_PREFLIGHT_INTERNAL",
        }
    }

    // 判断该错误是否允许被 broker 投影为业务前 rejected。
    #[cfg(test)]
    pub(crate) const fn may_reject_before_acceptance(self) -> bool {
        // 只有冻结的三个领域门禁可安全形成可重试拒绝。
        matches!(
            self,
            Self::RegistryFull | Self::StaleSession | Self::StalePage | Self::StaleElement
        )
    }
}

// 仅由 System 协调浏览器会话 Module，不向 broker 暴露 Module 所有权。
impl AppControlService {
    // 在 broker 建立 business accepted 前验证打开容量。
    pub(crate) fn prepare_browser_session_broker_open(
        // 可变借用唯一 System，先回收已完成 close 任务再判定容量。
        &mut self,
    ) -> Result<(), BrowserSessionBrokerPreflightError> {
        // 只把固定容量门禁映射为业务前可拒绝错误。
        project_open_preflight(self.browser_sessions.prepare_open())
    }

    // 在 broker 建立 business accepted 前验证关闭目标仍 live。
    pub(crate) fn prepare_browser_session_broker_close(
        // 借用唯一 System，但不转移 Module 生命周期。
        &self,
        // 借用待检查的 opaque session identity。
        session_id: &str,
    ) -> Result<(), BrowserSessionBrokerPreflightError> {
        // 只把固定 stale 门禁映射为业务前可拒绝错误。
        project_close_preflight(self.browser_sessions.prepare_close(session_id))
    }

    // 在 broker 建立 Query business accepted 前验证精确目标仍 live。
    pub(crate) fn prepare_browser_session_broker_inspect(
        // 只读借用唯一 System，禁止预检改变 Module 生命周期。
        &self,
        // 借用待检查的 opaque session identity。
        session_id: &str,
    ) -> Result<(), BrowserSessionBrokerPreflightError> {
        // 复用同一个 Module 只读查询，并只投影 stale 资格。
        project_inspect_preflight(self.browser_sessions.inspect_session(session_id))
    }

    // 在 broker 建立导航 business accepted 前验证 session 仍 live。
    pub(crate) fn prepare_browser_session_broker_navigate(
        // 只读借用唯一 System，预检不得派发页面命令。
        &self,
        // 借用待导航的公开 session identity。
        session_id: &str,
    ) -> Result<(), BrowserSessionBrokerPreflightError> {
        // 复用 Module 的只读 live registry 查询。
        project_inspect_preflight(self.browser_sessions.inspect_session(session_id))
    }

    // 在 broker 建立页面 Query business accepted 前验证当前导航代际。
    pub(crate) fn prepare_browser_session_broker_page(
        // 只读借用唯一 System，预检不得接触 worker I/O。
        &self,
        // 借用公开 session identity。
        session_id: &str,
        // 借用公开 page identity。
        page_id: &str,
    ) -> Result<(), BrowserSessionBrokerPreflightError> {
        // 只由 Module 读取其唯一 session/page 映射。
        project_page_preflight(self.browser_sessions.prepare_page(session_id, page_id))
    }

    // 在 broker 已接受后由 System 触发隔离会话打开。
    // #2059 后续 dispatcher 接入前保留已冻结的 System 边界。
    #[allow(dead_code)]
    pub(crate) fn execute_browser_session_broker_open(
        // 可变借用唯一 System，以维持对 Module 的唯一调用权。
        &mut self,
        // 接收已被 broker 总 deadline 截断的剩余预算。
        timeout: Duration,
        // 接收 broker 的协作取消观察。
        cancelled: impl Fn() -> bool,
    ) -> BrowserSessionBrokerOpenExecution {
        // 只由 System 调用 Module 的领域打开入口，并投影私有报告。
        project_open_execution(self.browser_sessions.open_isolated(timeout, cancelled))
    }

    // 在 broker 已接受后由 System 触发会话关闭和整树回收。
    // #2059 后续 dispatcher 接入前保留已冻结的 System 边界。
    #[allow(dead_code)]
    pub(crate) fn execute_browser_session_broker_close(
        // 可变借用唯一 System，以维持对 Module 的唯一调用权。
        &mut self,
        // 借用已在接受前验证的 opaque session identity。
        session_id: &str,
        // 接收 broker 已截断的剩余总预算。
        timeout: Duration,
        // 接收 broker 的协作取消观察。
        cancelled: impl Fn() -> bool,
    ) -> BrowserSessionBrokerCloseExecution {
        // 只由 System 调用 Module 的有界异步关闭入口，并隐藏其回收细节。
        project_close_execution(
            // Module 独占后台任务及 live registry 所有权。
            self.browser_sessions
                .close_for_broker(session_id, timeout, cancelled),
        )
    }

    // 在 broker 接受 Query 后由 System 唯一读取 Module live registry。
    pub(crate) fn execute_browser_session_broker_inspect(
        // 只读借用唯一 System，查询不得改变任何 worker 状态。
        &self,
        // 借用已在接受前验证的 opaque session identity。
        session_id: &str,
    ) -> BrowserSessionBrokerInspectExecution {
        // 只由 System 调用 Module 并投影最小公开事实。
        project_inspect_execution(self.browser_sessions.inspect_session(session_id))
    }

    // 在 broker 接受后由 System 唯一执行导航并换发 page identity。
    pub(crate) fn execute_browser_session_broker_navigate(
        // 可变借用唯一 System，以维持 Module 唯一调用权。
        &mut self,
        // 借用已预检的公开 session identity。
        session_id: &str,
        // 借用 parser 已验证的 HTTP(S) URL。
        url: &str,
        // 接收 broker 截断后的剩余总预算。
        timeout: Duration,
        // 接收 broker 协作取消观察。
        cancelled: impl Fn() -> bool,
    ) -> BrowserSessionBrokerNavigateExecution {
        // 由 Module 独占页面代际推进与 worker 私有引用。
        project_navigation_execution(self.browser_sessions.navigate(
            // 传入唯一公开 session。
            session_id,
            // Module 取得 URL 所有权且不会公开回显。
            url.to_owned(),
            // 传入不可扩张剩余预算。
            timeout,
            // 传入短锁取消观察。
            cancelled,
        ))
    }

    // 在 broker 接受 Query 后由 System 唯一执行有限等待。
    pub(crate) fn execute_browser_session_broker_wait(
        // 可变借用唯一 System，以维持 Module 串行领域状态。
        &mut self,
        // 借用已预检的公开 session identity。
        session_id: &str,
        // 借用已预检的当前 page identity。
        page_id: &str,
        // 借用 parser 已验证的有限条件。
        condition: &BrokerWaitCondition,
        // 接收 broker 截断后的剩余总预算。
        timeout: Duration,
        // 接收 broker 协作取消观察。
        cancelled: impl Fn() -> bool,
    ) -> BrowserSessionBrokerWaitExecution {
        // 转换 provider-neutral 条件后调用唯一 Module。
        project_wait_execution(
            page_id,
            self.browser_sessions.wait(
                // 传入公开 session。
                session_id,
                // 传入当前 page。
                page_id,
                // 转换为 Module 自有领域值。
                project_wait_condition(condition),
                // 传入不可扩张剩余预算。
                timeout,
                // 传入短锁取消观察。
                cancelled,
            ),
        )
    }

    // 在 broker 接受 Query 后由 System 唯一执行语义元素查询。
    pub(crate) fn execute_browser_session_broker_query(
        // 可变借用唯一 System，以维持元素身份 registry 唯一所有权。
        &mut self,
        // 借用已预检的公开 session identity。
        session_id: &str,
        // 借用已预检的当前 page identity。
        page_id: &str,
        // 借用 parser 已验证的 provider-neutral selector。
        selector: &BrokerSemanticSelector,
        // 接收 parser 已验证的结果上限。
        max_results: u16,
        // 接收 broker 截断后的剩余总预算。
        timeout: Duration,
        // 接收 broker 协作取消观察。
        cancelled: impl Fn() -> bool,
    ) -> BrowserSessionBrokerQueryExecution {
        // 转换 selector 后由 Module 签发公开元素 identity。
        project_query_execution(
            page_id,
            self.browser_sessions.query(
                // 传入公开 session。
                session_id,
                // 传入当前 page。
                page_id,
                // 转换为 Module 自有领域值。
                project_selector(selector),
                // 保留有界结果上限。
                max_results,
                // 传入不可扩张剩余预算。
                timeout,
                // 传入短锁取消观察。
                cancelled,
            ),
        )
    }
}

// 将打开预检映射为冻结的业务前错误集合。
fn project_open_preflight(
    // 接收 Module 的统一预检结果。
    result: AppResult<()>,
) -> Result<(), BrowserSessionBrokerPreflightError> {
    // 仅识别容量门禁，未知错误必须保留为内部失败闭合。
    result.map_err(|error| match error.code {
        // 映射唯一允许的打开业务前拒绝。
        "BROWSER_SESSION_REGISTRY_FULL" => BrowserSessionBrokerPreflightError::RegistryFull,
        // 不把任何其他 Module 错误伪装为可重试拒绝。
        _ => BrowserSessionBrokerPreflightError::Internal,
    })
}

// 将关闭预检映射为冻结的业务前错误集合。
fn project_close_preflight(
    // 接收 Module 的统一预检结果。
    result: AppResult<()>,
) -> Result<(), BrowserSessionBrokerPreflightError> {
    // 仅识别 stale 门禁，未知错误必须保留为内部失败闭合。
    result.map_err(|error| match error.code {
        // 映射唯一允许的关闭业务前拒绝。
        "STALE_SESSION" => BrowserSessionBrokerPreflightError::StaleSession,
        // 不把任何其他 Module 错误伪装为可重试拒绝。
        _ => BrowserSessionBrokerPreflightError::Internal,
    })
}

// 将会话查询预检映射为冻结的业务前 stale 错误。
fn project_inspect_preflight(
    // 接收 Module 只读查询结果。
    result: AppResult<BrowserSessionInspection>,
) -> Result<(), BrowserSessionBrokerPreflightError> {
    // 成功只保留通过事实，失败只允许 stale 形成 wire rejection。
    result
        // 预检不得把查询数据提前泄漏到 response。
        .map(|_| ())
        // 未知错误必须失败闭合。
        .map_err(|error| match error.code {
            // 缺失 live identity 是确定业务前 stale rejection。
            "STALE_SESSION" => BrowserSessionBrokerPreflightError::StaleSession,
            // 其他 Module 错误不属于冻结拒绝白名单。
            _ => BrowserSessionBrokerPreflightError::Internal,
        })
}

// 将页面身份预检映射为冻结的 stale session/page 集合。
fn project_page_preflight(
    // 接收 Module 的纯 registry 预检结果。
    result: AppResult<()>,
) -> Result<(), BrowserSessionBrokerPreflightError> {
    // 只允许两个确定 stale 类别在业务接受前公开。
    result.map_err(|error| match error.code {
        // 缺失 session 保留 session stale。
        "STALE_SESSION" => BrowserSessionBrokerPreflightError::StaleSession,
        // 旧导航代际保留 page stale。
        "STALE_PAGE" => BrowserSessionBrokerPreflightError::StalePage,
        // 其他 Module 错误不属于冻结拒绝白名单。
        _ => BrowserSessionBrokerPreflightError::Internal,
    })
}

// 将 Module 的打开报告映射为 broker 可缓存的封闭事实。
// #2059 后续 dispatcher 接入前保留已冻结的 System 边界。
#[allow(dead_code)]
fn project_open_execution(
    // 接收 Module 的封闭执行结果。
    result: AppResult<BrowserSessionOpenReport>,
) -> BrowserSessionBrokerOpenExecution {
    // 仅保留不含 worker、Job、stdio 或错误诊断的安全投影。
    match result {
        // 读取 Module 已验证的封闭报告。
        Ok(report) => match report.outcome() {
            // ready 必须同时拥有可信完成事实和公开 identity。
            BrowserSessionOpenOutcome::Ready if report.completed() => report
                // 仅复制 Module 签发的 opaque identity。
                .session_id()
                // 构造 broker completed 投影。
                .map(|session_id| BrowserSessionBrokerOpenExecution::Completed {
                    // 复制公开 identity，绝不泄漏内部 ref。
                    session_id: session_id.to_owned(),
                })
                // ready 缺少 identity 表示不能证明 registry 一致性。
                .unwrap_or(BrowserSessionBrokerOpenExecution::Unknown),
            // ready 却没有可信完成事实同样不能伪造失败。
            BrowserSessionOpenOutcome::Ready => BrowserSessionBrokerOpenExecution::Unknown,
            // inner worker 的确定未派发或失败仍是 broker 接受后的确定失败。
            BrowserSessionOpenOutcome::NotDispatched | BrowserSessionOpenOutcome::Failed => {
                // 投影接受后的确定失败。
                BrowserSessionBrokerOpenExecution::Failed
            }
            // inner worker 已无法产生可信 final 时必须保守 unknown。
            BrowserSessionOpenOutcome::Unknown => BrowserSessionBrokerOpenExecution::Unknown,
        },
        // Module 的同步错误未产生可公开成功身份，按确定失败投影。
        Err(_) => BrowserSessionBrokerOpenExecution::Failed,
    }
}

// 将 Module 的关闭结果映射为 broker 可缓存的封闭事实。
fn project_close_execution(
    // 接收 Module 的有界 close 结果。
    result: AppResult<BrowserSessionCloseOutcome>,
) -> BrowserSessionBrokerCloseExecution {
    // 只在 Module 已证明后台任务完成完整回收时投影完成。
    match result {
        // 可信完成事实允许 broker 缓存 completed。
        Ok(BrowserSessionCloseOutcome::Completed) => BrowserSessionBrokerCloseExecution::Completed,
        // deadline、取消或后台任务异常前无法证明回收必须投影 unknown。
        Ok(BrowserSessionCloseOutcome::Unknown) => BrowserSessionBrokerCloseExecution::Unknown,
        // 仅真正返回结构化错误时才投影确定失败。
        Err(_) => BrowserSessionBrokerCloseExecution::Failed,
    }
}

// 将 Module 的只读会话事实映射为 broker 最小 Query 结果。
fn project_inspect_execution(
    // 接收不含 worker 私有状态的 Module 查询结果。
    result: AppResult<BrowserSessionInspection>,
) -> BrowserSessionBrokerInspectExecution {
    // 只允许完整 live 事实形成 completed。
    match result {
        // 复制公开 opaque identity 与布尔 live 事实。
        Ok(inspection) => BrowserSessionBrokerInspectExecution::Completed {
            // 复制 Module 已确认的公开 identity。
            session_id: inspection.session_id().to_owned(),
            // 复制查询线性化点的 live 事实。
            live: inspection.live(),
        },
        // accepted 后任何结构化错误都只能形成确定 Query failure。
        Err(_) => BrowserSessionBrokerInspectExecution::Failed,
    }
}

// 将 Module 导航报告映射为 broker 封闭执行事实。
fn project_navigation_execution(
    // 接收 Module 的导航聚合结果。
    result: AppResult<BrowserNavigationReport>,
) -> BrowserSessionBrokerNavigateExecution {
    // 只在结果类别、完成位、身份与代际全部一致时公开成功。
    match result {
        // completed 报告必须携带 page 与可收窄正代际。
        Ok(report)
            if report.outcome() == BrowserSessionCommandOutcome::Completed
                && report.completed() =>
        {
            // 尝试取得完整成功事实。
            match (
                // 复制公开 page identity。
                report.page_id(),
                // 将 Module 代际无损收窄为 broker u32。
                u32::try_from(report.navigation_generation()).ok(),
            ) {
                // 只有非零代际与 page 同时存在才完成。
                (Some(page_id), Some(generation)) if generation > 0 => {
                    // 返回不含 worker 私有引用的成功投影。
                    BrowserSessionBrokerNavigateExecution::Completed {
                        // 复制公开 page identity。
                        page_id: page_id.to_owned(),
                        // 保存正代际。
                        generation,
                    }
                }
                // 内部成功事实漂移不得伪造成确定失败。
                _ => BrowserSessionBrokerNavigateExecution::Unknown,
            }
        }
        // Module 明确无法取得可信 final 时保守 unknown。
        Ok(report) if report.outcome() == BrowserSessionCommandOutcome::Unknown => {
            // 保留 accepted 后未知结果。
            BrowserSessionBrokerNavigateExecution::Unknown
        }
        // 确定未派发、确定失败或结构化同步错误形成 failed。
        Ok(_) | Err(_) => BrowserSessionBrokerNavigateExecution::Failed,
    }
}

// 将 Module wait 报告映射为 broker 封闭执行事实。
fn project_wait_execution(
    // 借用已经通过业务前预检的当前公开页面 identity。
    page_id: &str,
    // 接收 Module 的 wait 聚合结果。
    result: AppResult<BrowserSessionCommandReport<BrowserWaitData>>,
) -> BrowserSessionBrokerWaitExecution {
    // 只在完成位与 conditionMet 同时可信时公开成功。
    match result {
        // completed 报告必须携带 conditionMet=true。
        Ok(report)
            if report.outcome() == BrowserSessionCommandOutcome::Completed
                && report.completed()
                && report.data().is_some_and(BrowserWaitData::condition_met) =>
        {
            // Module 报告的代际必须可无损收窄且为正数。
            let Some(generation) = u32::try_from(report.navigation_generation())
                // 收窄失败不允许伪造公开成功。
                .ok()
                // 零代际同样不是可信完成。
                .filter(|generation| *generation > 0)
            else {
                // 成功证据漂移时保守投影 unknown。
                return BrowserSessionBrokerWaitExecution::Unknown;
            };
            // 返回当前页面、代际与固定满足事实。
            BrowserSessionBrokerWaitExecution::Completed {
                // 复制调用时已预检的公开页面 identity。
                page_id: page_id.to_owned(),
                // 保存 Module 报告的正代际。
                generation,
            }
        }
        // Module 明确无法取得可信 final 时保守 unknown。
        Ok(report) if report.outcome() == BrowserSessionCommandOutcome::Unknown => {
            // Query unknown 不得伪造完成。
            BrowserSessionBrokerWaitExecution::Unknown
        }
        // 确定未派发、失败或内部同步错误形成 failed。
        Ok(_) | Err(_) => BrowserSessionBrokerWaitExecution::Failed,
    }
}

// 将 Module query 报告映射为 broker provider-neutral JSON。
fn project_query_execution(
    // 借用已经通过业务前预检的当前公开页面 identity。
    page_id: &str,
    // 接收 Module 的 query 聚合结果。
    result: AppResult<BrowserSessionCommandReport<BrowserQueryData>>,
) -> BrowserSessionBrokerQueryExecution {
    // 只在完成位与查询数据同时可信时公开成功。
    match result {
        // completed 报告必须携带查询数据。
        Ok(report)
            if report.outcome() == BrowserSessionCommandOutcome::Completed
                && report.completed() =>
        {
            // 缺失数据表示内部成功事实漂移。
            let Some(data) = report.data() else {
                // 不伪造空查询结果。
                return BrowserSessionBrokerQueryExecution::Unknown;
            };
            // Module 报告的代际必须可无损收窄且为正数。
            let Some(generation) = u32::try_from(report.navigation_generation())
                // 收窄失败不允许伪造公开成功。
                .ok()
                // 零代际同样不是可信完成。
                .filter(|generation| *generation > 0)
            else {
                // 成功证据漂移时保守投影 unknown。
                return BrowserSessionBrokerQueryExecution::Unknown;
            };
            // 把公开元素摘要转换为有界 JSON 数组。
            let matches = data
                // 借用 Module 已限制的命中集合。
                .matches()
                // 逐项转换公开字段。
                .iter()
                // 映射为无 provider/native 事实的对象。
                .map(project_element)
                // 收集为拥有型 JSON 数组。
                .collect::<Vec<_>>();
            // 返回由 response Component 再验证的完整查询对象。
            BrowserSessionBrokerQueryExecution::Completed {
                // 保存精确三字段查询数据。
                data: json!({
                    // 输出当前公开页面 identity。
                    "pageId": page_id,
                    // 输出 Module 报告的当前正代际。
                    "navigationGeneration": generation,
                    // 输出有界公开元素摘要。
                    "matches": matches,
                    // 输出 worker 报告的总命中数。
                    "matchCount": data.match_count(),
                    // 输出是否因 maxResults 截断。
                    "truncated": data.truncated()
                }),
            }
        }
        // Module 明确无法取得可信 final 时保守 unknown。
        Ok(report) if report.outcome() == BrowserSessionCommandOutcome::Unknown => {
            // Query unknown 保持 targetMayHaveMutated=false。
            BrowserSessionBrokerQueryExecution::Unknown
        }
        // 确定未派发、失败或结构化同步错误形成 failed。
        Ok(_) | Err(_) => BrowserSessionBrokerQueryExecution::Failed,
    }
}

// 把 broker selector 转换为 Module 自有领域值。
fn project_selector(selector: &BrokerSemanticSelector) -> ModuleSemanticSelector {
    // 只复制 provider-neutral 字段。
    ModuleSemanticSelector::new(
        // 复制可选 role。
        selector.role().map(str::to_owned),
        // 复制可选名称。
        selector.name().map(str::to_owned),
        // 复制可选文本。
        selector.text().map(str::to_owned),
        // 保留精确匹配标记。
        selector.exact(),
    )
}

// 把 broker wait 条件转换为 Module 自有领域值。
fn project_wait_condition(condition: &BrokerWaitCondition) -> ModuleWaitCondition {
    // 穷举全部有限条件。
    match condition {
        // 映射文档 ready。
        BrokerWaitCondition::DocumentReady => ModuleWaitCondition::DocumentReady,
        // 映射语义元素出现。
        BrokerWaitCondition::ElementPresent(selector) => {
            // 复制 provider-neutral selector。
            ModuleWaitCondition::ElementPresent(project_selector(selector))
        }
        // 映射有界文本出现。
        BrokerWaitCondition::TextPresent { text, exact } => ModuleWaitCondition::TextPresent {
            // 复制目标文本。
            text: text.to_owned(),
            // 保留逐字匹配要求。
            exact: *exact,
        },
    }
}

// 把一个 Module 元素摘要投影为 broker 安全 JSON。
fn project_element(element: &BrowserElementMatch) -> Value {
    // 只输出冻结的 provider-neutral 字段。
    json!({
        // 输出 Module 签发的 opaque element identity。
        "elementId": element.element_id(),
        // 输出可选 role。
        "role": element.role(),
        // 输出可选可访问名称。
        "name": element.name(),
        // 输出可选可见文本。
        "text": element.text(),
        // 输出可用事实。
        "enabled": element.enabled()
    })
}

// 声明不启动真实浏览器的纯 System 边界回归。
#[cfg(test)]
mod tests {
    // 导入统一结果类型。
    use crate::domain::{AppControlError, AppResult};
    // 导入被测 System。
    use super::AppControlService;
    // 导入关闭投影和内部映射。
    use super::{
        // 导入关闭执行投影。
        BrowserSessionBrokerCloseExecution,
        // 导入 inspect 执行投影。
        BrowserSessionBrokerInspectExecution,
        // 导入业务接受前预检错误。
        BrowserSessionBrokerPreflightError,
        // 导入 Module close 的有界结果类别。
        BrowserSessionCloseOutcome,
        // 导入关闭执行投影函数。
        project_close_execution,
        // 导入打开预检错误投影函数。
        project_open_preflight,
    };

    // 验证预检不会启动浏览器且 stale 目标在业务接受前失败。
    #[test]
    fn broker_preflight_is_capacity_and_stale_gate() {
        // 构造空 System，不创建浏览器 worker。
        let mut service = AppControlService::new();
        // 空 registry 必须允许后续打开接受。
        assert!(service.prepare_browser_session_broker_open().is_ok());
        // 不存在的 canonical session 必须在接受前返回 stale。
        let stale = service
            // 调用仅做 registry 查找的预检。
            .prepare_browser_session_broker_close("s2:bs:00000000000000000000000000000000")
            // 空 registry 必须拒绝。
            .expect_err("missing session must be rejected before broker acceptance");
        // 保留 Module 的稳定 stale 错误码。
        assert_eq!(stale.code(), "STALE_SESSION");
        // stale 是冻结业务前拒绝集合的一员。
        assert!(stale.may_reject_before_acceptance());
        // 同一空 registry 的 inspect Query 也必须在接受前返回 stale。
        let inspect_stale = service
            // 调用只读 System 查询预检。
            .prepare_browser_session_broker_inspect("s2:bs:00000000000000000000000000000000")
            // 空 registry 不得接受 Query。
            .expect_err("missing inspected session must be rejected before acceptance");
        // Query 保留相同稳定 stale 错误码。
        assert_eq!(inspect_stale.code(), "STALE_SESSION");
        // stale Query 同样属于冻结业务前拒绝集合。
        assert!(inspect_stale.may_reject_before_acceptance());
    }

    // 验证 Module 已证明回收的关闭投影为完成。
    #[test]
    fn proven_close_reap_is_completed() {
        // 构造 Module 已证明完整回收的结果。
        let result: AppResult<BrowserSessionCloseOutcome> =
            Ok(BrowserSessionCloseOutcome::Completed);
        // 映射不得把可信完成降级为失败。
        assert_eq!(
            // 执行被测投影。
            project_close_execution(result),
            // 关闭仍是可信完成。
            BrowserSessionBrokerCloseExecution::Completed,
        );
    }

    // 验证未在总预算内证明回收时不得伪造 completed。
    #[test]
    fn unproven_close_reap_is_unknown() {
        // 构造 Module 无法在本次调用内证明回收的结果。
        let result: AppResult<BrowserSessionCloseOutcome> = Ok(BrowserSessionCloseOutcome::Unknown);
        // 映射必须保守保留 unknown。
        assert_eq!(
            // 执行被测投影。
            project_close_execution(result),
            // accepted 后不可伪造最终完成。
            BrowserSessionBrokerCloseExecution::Unknown,
        );
    }

    // 验证 accepted 后查询错误不会伪造 live 事实。
    #[test]
    fn failed_inspection_does_not_fabricate_live_session() {
        // 构造空 System，不启动任何 worker。
        let service = AppControlService::new();
        // 绕过 dispatcher 预检直接覆盖 accepted 后防御性投影。
        let result = service.execute_browser_session_broker_inspect(
            // 使用不存在的 canonical session。
            "s2:bs:00000000000000000000000000000000",
        );
        // stale 不得被伪装为 completed live。
        assert_eq!(result, BrowserSessionBrokerInspectExecution::Failed);
    }

    // 验证未冻结的 Module 预检错误不会被伪装为业务前拒绝。
    #[test]
    fn unexpected_preflight_error_fails_closed() {
        // 构造不属于打开容量门禁的内部 Module 错误。
        let result: AppResult<()> = Err(AppControlError::new(
            // 使用不允许形成业务前拒绝的稳定错误码。
            "WORKER_PROTOCOL_FAILED",
            // 使用不含用户输入的固定测试诊断。
            "fixture internal failure",
        ));
        // 执行打开预检投影。
        let error = project_open_preflight(result)
            // 内部错误必须被显式返回给 broker。
            .expect_err("unexpected preflight error must fail closed");
        // 只允许单独内部类别，不可复用业务前 rejected 语义。
        assert_eq!(error, BrowserSessionBrokerPreflightError::Internal);
        // 调用方必须走 protocol/internal fail-closed 路径。
        assert!(!error.may_reject_before_acceptance());
    }
}
