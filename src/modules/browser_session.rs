//! 拥有浏览器会话、公开页面身份、导航代际与显式关闭生命周期。
// 导入会话 registry。
use std::collections::HashMap;
// 导入单命令总预算。
use std::time::Duration;
// 导入私有页面协议与会话进程 Component。
use crate::components::{
    // 导入页面操作与封闭结果类别。
    browser_page_protocol::{BrowserPageOperation, BrowserPageOutcome},
    // 导入 Module 自有后台 close task 的公开类型。
    browser_session_close_task::BrowserSessionCloseTask,
    // 导入隔离会话打开入口与 live 进程所有权。
    browser_session_process::{self, BrowserSessionProcess},
    // 导入打开结果类别。
    browser_session_protocol::BrowserSessionOutcome,
    // 导入公开身份随机材料。
    secure_nonce_windows::random_nonce,
};
// 导入统一错误与结果。
use crate::domain::{AppControlError, AppResult};
// 把其余页面语义动作拆到同一 Module 的窄文件。
#[path = "browser_session_actions.rs"]
mod actions;
// 把动作执行和结果解析拆到同一 Module 的窄文件。
#[path = "browser_session_action_runtime.rs"]
mod action_runtime;
// 把 broker close 的异步回收拆到同一 Module 的窄文件。
#[path = "browser_session_close.rs"]
mod close;
// 向 System 边界导出 broker close 的封闭结果类别。
pub(crate) use close::BrowserSessionCloseOutcome;
// 把 broker session inspect 的只读 registry 查询拆到同一 Module 的窄文件。
#[path = "browser_session_inspect.rs"]
mod inspect;
// 向 System 边界导出不含 worker 事实的最小只读投影。
pub(crate) use inspect::BrowserSessionInspection;
// 把 broker 页面身份预检拆到同一 Module 的窄文件。
#[path = "browser_session_preflight.rs"]
mod preflight;
// 导出 Module 页面动作的 provider-neutral 领域类型。
pub(crate) use actions::{
    // 导出点击成功事实供 System 封闭投影。
    BrowserClickData,
    // 导出 provider-neutral 元素摘要。
    BrowserElementMatch,
    // 导出 query 成功数据。
    BrowserQueryData,
    // 导出截图成功数据供 System 封闭投影。
    BrowserScreenshotData,
    // 导出 selector。
    BrowserSemanticSelector,
    // 导出 wait 条件。
    BrowserSemanticWaitCondition,
    // 导出通用页面命令报告。
    BrowserSessionCommandReport,
    // 导出输入成功事实供 System 封闭投影。
    BrowserTypeData,
    // 导出确认式文本输入请求。
    BrowserTypeRequest,
    // 导出 wait 成功数据。
    BrowserWaitData,
};
// 固定单个 Module 代际可拥有的 live 会话上限。
const MAXIMUM_LIVE_SESSIONS: usize = 16;
// 固定 Module 公开页面身份前缀。
const PUBLIC_PAGE_PREFIX: &str = "s2:bp:";
// 固定 Module 当前页面的公开元素映射上限。
const MAXIMUM_PUBLIC_ELEMENTS: usize = 10_000;
// 固定 Module 公开元素身份前缀。
const PUBLIC_ELEMENT_PREFIX: &str = "s2:be:";
// 保存一次打开结果的封闭类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionOpenOutcome {
    // 会话已建立并进入 registry。
    Ready,
    // worker 确定未接受打开请求。
    NotDispatched,
    // worker 接受后确定失败。
    Failed,
    // worker 接受后终态未知。
    Unknown,
}
// 保存一次页面命令的封闭类别。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserSessionCommandOutcome {
    // 命令确定完成。
    Completed,
    // 命令确定未派发。
    NotDispatched,
    // 命令派发后确定失败。
    Failed,
    // 命令派发后终态未知。
    Unknown,
}
// 保存 Module 可公开的安全失败。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionFailure {
    // 保存稳定错误码。
    code: String,
    // 保存不含输入或原生事实的消息。
    message: String,
}
// 为安全失败提供只读投影。
impl BrowserSessionFailure {
    // 返回稳定错误码。
    pub(crate) fn code(&self) -> &str {
        // 借用自有错误码。
        &self.code
    }
    // 返回安全错误消息。
    pub(crate) fn message(&self) -> &str {
        // 借用自有消息。
        &self.message
    }
}
// 保存一次隔离会话打开聚合。
pub(crate) struct BrowserSessionOpenReport {
    // 保存封闭结果类别。
    outcome: BrowserSessionOpenOutcome,
    // 保存可信完成事实。
    completed: bool,
    // 保存安全重试事实。
    retry_safe: bool,
    // 保存 worker 可能接受事实。
    accepted_may_have_occurred: bool,
    // 保存进入 registry 后的公开会话身份。
    session_id: Option<String>,
    // 保存可选安全错误。
    error: Option<BrowserSessionFailure>,
    // 保存 parent 是否执行强制回收。
    forced_reap: bool,
}
// 为打开报告提供 provider-neutral 只读投影。
impl BrowserSessionOpenReport {
    // 返回封闭结果类别。
    pub(crate) const fn outcome(&self) -> BrowserSessionOpenOutcome {
        // 复制枚举。
        self.outcome
    }
    // 返回可信完成事实。
    pub(crate) const fn completed(&self) -> bool {
        // 复制布尔值。
        self.completed
    }
    // 返回安全重试事实。
    pub(crate) const fn retry_safe(&self) -> bool {
        // 复制布尔值。
        self.retry_safe
    }
    // 返回 worker 可能接受事实。
    pub(crate) const fn accepted_may_have_occurred(&self) -> bool {
        // 复制布尔值。
        self.accepted_may_have_occurred
    }
    // 返回可选公开会话身份。
    pub(crate) fn session_id(&self) -> Option<&str> {
        // 借用公开身份。
        self.session_id.as_deref()
    }
    // 返回可选安全错误。
    pub(crate) const fn error(&self) -> Option<&BrowserSessionFailure> {
        // 借用安全错误。
        self.error.as_ref()
    }
    // 返回强制回收事实。
    pub(crate) const fn forced_reap(&self) -> bool {
        // 复制布尔值。
        self.forced_reap
    }
}

// 保存一次导航的公开结果。
pub(crate) struct BrowserNavigationReport {
    // 保存封闭命令类别。
    outcome: BrowserSessionCommandOutcome,
    // 保存可信完成事实。
    completed: bool,
    // 保存安全重试事实。
    retry_safe: bool,
    // 保存 worker 可能接受事实。
    accepted_may_have_occurred: bool,
    // 保存成功后换发的公开页面身份。
    page_id: Option<String>,
    // 保存 Module 观察的导航代际。
    navigation_generation: u64,
    // 保存可选安全错误。
    error: Option<BrowserSessionFailure>,
    // 保存 parent 是否执行强制回收。
    forced_reap: bool,
}

// 为导航报告提供 provider-neutral 只读投影。
impl BrowserNavigationReport {
    // 返回封闭命令类别。
    pub(crate) const fn outcome(&self) -> BrowserSessionCommandOutcome {
        // 复制枚举。
        self.outcome
    }
    // 返回可信完成事实。
    pub(crate) const fn completed(&self) -> bool {
        // 复制布尔值。
        self.completed
    }
    // 返回安全重试事实。
    pub(crate) const fn retry_safe(&self) -> bool {
        // 复制布尔值。
        self.retry_safe
    }
    // 返回 worker 可能接受事实。
    pub(crate) const fn accepted_may_have_occurred(&self) -> bool {
        // 复制布尔值。
        self.accepted_may_have_occurred
    }

    // 返回成功后换发的公开页面身份。
    pub(crate) fn page_id(&self) -> Option<&str> {
        // 借用公开身份。
        self.page_id.as_deref()
    }

    // 返回 Module 观察的导航代际。
    pub(crate) const fn navigation_generation(&self) -> u64 {
        // 复制代际。
        self.navigation_generation
    }

    // 返回可选安全错误。
    pub(crate) const fn error(&self) -> Option<&BrowserSessionFailure> {
        // 借用安全错误。
        self.error.as_ref()
    }

    // 返回强制回收事实。
    pub(crate) const fn forced_reap(&self) -> bool {
        // 复制布尔值。
        self.forced_reap
    }
}

// 保存当前公开页面与 worker 私有页面引用的唯一映射。
struct BrowserPageEntry {
    // 保存 Module 换发的公开页面身份。
    public_id: String,
    // 保存仅向页面 worker 回传的私有引用。
    worker_ref: String,
    // 保存该页面绑定的导航代际。
    navigation_generation: u64,
    // 保存当前页面代际的公开元素映射。
    elements: HashMap<String, BrowserElementEntry>,
}

// 保存公开元素身份到 worker 私有引用的唯一映射。
struct BrowserElementEntry {
    // 保存仅向页面 worker 回传的私有元素引用。
    worker_ref: String,
}

// 保存一个 live 会话的完整生命周期责任。
struct BrowserSessionEntry {
    // 拥有 live worker 进程、Job 和 stdio。
    process: BrowserSessionProcess,
    // 保存显式 close 所需原始 open nonce。
    open_nonce: String,
    // 保存当前唯一页面映射。
    page: Option<BrowserPageEntry>,
}

// 拥有同一进程代际内的浏览器会话 registry。
pub(crate) struct BrowserSessionModule {
    // 以公开 session ID 索引唯一 live 会话。
    sessions: HashMap<String, BrowserSessionEntry>,
    // 由 Module 唯一持有已 stale 会话的后台回收任务。
    pending_close_tasks: Vec<BrowserSessionCloseTask>,
    // 线程暂不可用时仍由 Module 保有的待重试回收进程。
    pending_close_processes: Vec<(BrowserSessionProcess, String)>,
}
// 为 Browser Session Module 提供打开、导航、查询和关闭入口。
impl BrowserSessionModule {
    // 建立空 Module 代际。
    pub(crate) fn new() -> Self {
        // 不继承任何旧会话身份。
        Self {
            // 建立有界 registry。
            sessions: HashMap::new(),
            // 建立空的后台回收任务集合。
            pending_close_tasks: Vec::new(),
            // 建立空的待重试进程集合。
            pending_close_processes: Vec::new(),
        }
    }
    // 在业务接受前验证新会话不会超过 Module 自身容量。
    pub(crate) fn prepare_open(&mut self) -> AppResult<()> {
        // 在容量判定前先释放已经完成的后台 close 所有权。
        self.reap_finished_close_tasks();
        // 合并 live registry、后台 close task 与待重试进程的全部 Module 自有资源计数。
        let owned_sessions = self
            // 计算仍可接受新命令的 live session 数量。
            .sessions
            .len()
            // 加入已 stale 但仍持有 Job 的后台任务数量。
            .saturating_add(self.pending_close_tasks.len())
            // 加入尚未能创建后台线程的唯一进程 owner 数量。
            .saturating_add(self.pending_close_processes.len());
        // 只允许 Module 总资源仍在固定上限内时进入业务接受。
        ensure_open_capacity(owned_sessions)
    }
    // 在业务接受前验证关闭目标仍属于当前 Module 代际。
    pub(crate) fn prepare_close(&self, session_id: &str) -> AppResult<()> {
        // 缺失或过期身份不得进入已接受的关闭路径。
        self.sessions
            // 只读取 registry，不转移 worker、Job 或 stdio 所有权。
            .contains_key(session_id)
            // 返回结构化 stale 事实。
            .then_some(())
            // 保持既有安全错误投影。
            .ok_or_else(stale_session_error)
    }
    // 打开工具自有隔离会话并在 ready 后取得所有权。
    pub(crate) fn open_isolated(
        // 可变借用唯一 Module 状态。
        &mut self,
        // 接收打开总预算。
        timeout: Duration,
        // 接收取消观察。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<BrowserSessionOpenReport> {
        // 复用业务接受前同一容量门禁，避免执行路径漂移。
        self.prepare_open()?;
        // 通过唯一生产 Component 打开隔离 worker。
        let opened = browser_session_process::open_isolated(timeout, cancelled)?;
        // 投影打开 outcome。
        let outcome = map_open_outcome(opened.outcome());
        // 复制可信完成事实。
        let completed = opened.completed();
        // 复制安全重试事实。
        let retry_safe = opened.retry_safe();
        // 复制可能接受事实。
        let accepted_may_have_occurred = opened.accepted_may_have_occurred();
        // 复制安全错误。
        let error = project_failure(opened.error());
        // 复制强制回收事实。
        let forced_reap = opened.forced_reap();
        // ready 才允许进入 registry。
        let session_id = if outcome == BrowserSessionOpenOutcome::Ready {
            // 先复制 worker 生成的公开 session ID。
            let session_id = opened
                // 借用 ready 身份。
                .session_id()
                // 缺失身份表示内部契约漂移。
                .ok_or_else(|| {
                    internal_protocol_error("The ready browser session identity is missing.")
                })?
                // 独立保存 registry 键。
                .to_owned();
            // 同一 Module 代际禁止身份覆盖。
            if self.sessions.contains_key(&session_id) {
                // opened 离开作用域时会强制回收重复资源。
                return Err(internal_protocol_error(
                    "The browser session identity was duplicated.",
                ));
            }
            // 取得 live 进程和显式关闭 nonce。
            let (process, open_nonce) = opened
                // 转移唯一资源所有权。
                .into_session()
                // ready 必须携带 live 会话。
                .ok_or_else(|| {
                    internal_protocol_error("The ready browser session resource is missing.")
                })?;
            // 插入完整生命周期记录。
            self.sessions.insert(
                // 使用公开 session ID 作唯一键。
                session_id.clone(),
                // 建立无页面初态。
                BrowserSessionEntry {
                    // 接管 worker 进程。
                    process,
                    // 接管关闭关联 nonce。
                    open_nonce,
                    // 尚未导航时没有页面身份。
                    page: None,
                },
            );
            // 向报告返回公开身份。
            Some(session_id)
        } else {
            // 非 ready 不保留任何会话身份。
            None
        };
        // 返回封闭打开报告。
        Ok(BrowserSessionOpenReport {
            // 保存类别。
            outcome,
            // 保存完成事实。
            completed,
            // 保存重试事实。
            retry_safe,
            // 保存可能接受事实。
            accepted_may_have_occurred,
            // 保存可选 session ID。
            session_id,
            // 保存安全错误。
            error,
            // 保存回收事实。
            forced_reap,
        })
    }

    // 导航 live 会话并在成功后原子换发公开页面身份。
    pub(crate) fn navigate(
        // 可变借用唯一 Module 状态。
        &mut self,
        // 借用公开 session ID。
        session_id: &str,
        // 接收目标 URL 所有权。
        url: String,
        // 接收单命令总预算。
        timeout: Duration,
        // 接收取消观察。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<BrowserNavigationReport> {
        // 构造封闭导航操作。
        let operation = BrowserPageOperation::Navigate { url };
        // 在查找目标和执行 I/O 前复用协议验证器。
        operation.validate().map_err(|_| {
            // 返回不回显 URL 的稳定错误。
            AppControlError::new(
                // 使用参数错误类别。
                "INVALID_ARGUMENT",
                // 保持消息安全。
                "The browser navigation request was invalid.",
            )
        })?;
        // Module 在目标查找和 worker I/O 前拥有总预算验证。
        validate_command_timeout(timeout)?;
        // 在 dispatch 前预生成候选公开身份，避免成功后随机源失败破坏代际同步。
        let candidate_page_id = format!("{PUBLIC_PAGE_PREFIX}{}", random_nonce()?);
        // 只允许当前 registry 中的精确 session。
        let entry = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(stale_session_error)?;
        // 复制命令前导航代际。
        let previous_generation = entry
            // 借用当前页面。
            .page
            // 映射当前代际。
            .as_ref()
            .map_or(0, |page| page.navigation_generation);
        // 复制当前 worker 私有页面引用，只向 Component 回传。
        let worker_page_ref = entry
            // 借用当前页面。
            .page
            // 借用私有引用。
            .as_ref()
            .map(|page| page.worker_ref.as_str());
        // 串行执行唯一页面命令。
        let result = match entry.process.execute_page(
            // 传入私有页面引用。
            worker_page_ref,
            // 传入 Module 当前代际。
            previous_generation,
            // 转移已验证操作。
            operation,
            // 使用唯一总预算。
            timeout,
            // 传入取消观察。
            cancelled,
        ) {
            // 保留可信聚合结果。
            Ok(result) => result,
            // Component 级错误后保守失效整个 live 会话。
            Err(error) => {
                // 显式结束当前可变借用。
                let _ = entry;
                // 原子失效会话并把精确资源所有权转入后台回收。
                self.retire_session(session_id);
                // 返回原结构化错误。
                return Err(error);
            }
        };
        // 投影封闭命令类别。
        let outcome = map_command_outcome(result.outcome());
        // 复制安全失败。
        let error = project_failure(result.error());
        // 成功导航必须推进恰好一代并换发公开页面身份。
        let page_id = if outcome == BrowserSessionCommandOutcome::Completed {
            // 核对 worker 严格单调推进。
            let expected_generation = previous_generation
                // 防止理论溢出。
                .checked_add(1)
                // u32 上限前不应溢出。
                .ok_or_else(|| {
                    internal_protocol_error("The browser navigation generation overflowed.")
                });
            // 溢出表示 worker 与 Module 已无法安全同步。
            let expected_generation = match expected_generation {
                // 保留有效下一代。
                Ok(value) => value,
                // 失效完整会话后返回错误。
                Err(error) => {
                    // 显式结束当前可变借用。
                    let _ = entry;
                    // 原子失效会话并把精确资源所有权转入后台回收。
                    self.retire_session(session_id);
                    // 返回结构化漂移错误。
                    return Err(error);
                }
            };
            // worker 返回代际必须与 Module 预期一致。
            if result.navigation_generation() != expected_generation {
                // 构造安全漂移错误。
                let error = internal_protocol_error("The browser navigation generation drifted.");
                // 显式结束当前可变借用。
                let _ = entry;
                // 原子失效会话并把精确资源所有权转入后台回收。
                self.retire_session(session_id);
                // 拒绝接受漂移后的身份映射。
                return Err(error);
            }
            // 取得已由 Component 验证的私有 page ref。
            let worker_ref = result
                // 借用成功数据。
                .data()
                // 读取对象字段。
                .and_then(|data| data.get("pageRef"))
                // 转换为字符串。
                .and_then(serde_json::Value::as_str)
                // 缺失表示内部契约漂移。
                .ok_or_else(|| {
                    internal_protocol_error("The browser navigation page reference is missing.")
                })
                // Module 独立拥有映射值。
                .map(str::to_owned);
            // 缺失私有 ref 时失效已推进的 worker 会话。
            let worker_ref = match worker_ref {
                // 保留有效私有映射。
                Ok(value) => value,
                // 失效完整会话后返回错误。
                Err(error) => {
                    // 显式结束当前可变借用。
                    let _ = entry;
                    // 原子失效会话并把精确资源所有权转入后台回收。
                    self.retire_session(session_id);
                    // 返回结构化漂移错误。
                    return Err(error);
                }
            };
            // 使用 dispatch 前预生成且与 worker ref 无关的公开 ID。
            let public_id = candidate_page_id;
            // 原子替换旧页面映射，旧公开身份立即 stale。
            entry.page = Some(BrowserPageEntry {
                // 保存公开 ID。
                public_id: public_id.clone(),
                // 保存私有 ref。
                worker_ref,
                // 保存新代际。
                navigation_generation: expected_generation,
                // 新导航从空元素映射开始。
                elements: HashMap::new(),
            });
            // 报告新公开页面身份。
            Some(public_id)
        } else {
            // 非成功结果不得签发新页面身份。
            None
        };
        // 复制会话回收事实。
        let forced_reap = result.forced_reap();
        // 构造不含 worker 私有引用的结果。
        let report = BrowserNavigationReport {
            // 保存类别。
            outcome,
            // 保存可信完成事实。
            completed: result.completed(),
            // 保存安全重试事实。
            retry_safe: result.retry_safe(),
            // 保存可能接受事实。
            accepted_may_have_occurred: result.accepted_may_have_occurred(),
            // 保存可选公开页面身份。
            page_id,
            // 保存 worker 报告代际。
            navigation_generation: result.navigation_generation(),
            // 保存安全错误。
            error,
            // 保存回收事实。
            forced_reap,
        };
        // 结束 entry 可变借用后再处理 registry。
        let _ = entry;
        // 不可信会话必须从 live registry 失效并保持异步资源所有权。
        if result.session_invalidated() {
            // 把精确进程、Job 与 profile 析构路径转入 Module 自有后台任务。
            self.retire_session(session_id);
        }
        // 返回公开导航报告。
        Ok(report)
    }

    // 验证公开页面身份仍绑定当前会话和代际。
    pub(crate) fn page_generation(&self, session_id: &str, page_id: &str) -> AppResult<u64> {
        // 只允许当前 live session。
        let entry = self
            .sessions
            .get(session_id)
            .ok_or_else(stale_session_error)?;
        // 只允许当前唯一页面身份。
        let page = entry
            // 借用当前页面。
            .page
            // 映射可选页面。
            .as_ref()
            // 缺失页面统一 stale。
            .filter(|page| page.public_id == page_id)
            // 返回稳定 stale page 错误。
            .ok_or_else(stale_page_error)?;
        // 返回当前代际。
        Ok(page.navigation_generation)
    }

    // 显式关闭并移除一个 live 会话。
    pub(crate) fn close(&mut self, session_id: &str) -> AppResult<bool> {
        // 先复用业务接受前身份门禁，保证 stale 语义一致。
        self.prepare_close(session_id)?;
        // 先从 registry 移除，阻止并发后续查找复用。
        let entry = self
            .sessions
            .remove(session_id)
            .ok_or_else(stale_session_error)?;
        // 使用原始 open nonce 关联协作关闭。
        Ok(entry.process.close(&entry.open_nonce))
    }
}
// 映射 Component 打开类别到 Module 类别。
fn map_open_outcome(outcome: BrowserSessionOutcome) -> BrowserSessionOpenOutcome {
    // 穷举冻结类别。
    match outcome {
        // 映射 ready。
        BrowserSessionOutcome::Ready => BrowserSessionOpenOutcome::Ready,
        // 映射未派发。
        BrowserSessionOutcome::NotDispatched => BrowserSessionOpenOutcome::NotDispatched,
        // 映射确定失败。
        BrowserSessionOutcome::Failed => BrowserSessionOpenOutcome::Failed,
        // 映射未知。
        BrowserSessionOutcome::Unknown => BrowserSessionOpenOutcome::Unknown,
    }
}

// 映射 Component 命令类别到 Module 类别。
fn map_command_outcome(outcome: BrowserPageOutcome) -> BrowserSessionCommandOutcome {
    // 穷举冻结类别。
    match outcome {
        // 映射完成。
        BrowserPageOutcome::Completed => BrowserSessionCommandOutcome::Completed,
        // 映射未派发。
        BrowserPageOutcome::NotDispatched => BrowserSessionCommandOutcome::NotDispatched,
        // 映射确定失败。
        BrowserPageOutcome::Failed => BrowserSessionCommandOutcome::Failed,
        // 映射未知。
        BrowserPageOutcome::Unknown => BrowserSessionCommandOutcome::Unknown,
    }
}

// 把已验证 JSON 错误转为 Module 自有类型。
fn project_failure(value: Option<&serde_json::Value>) -> Option<BrowserSessionFailure> {
    // 只投影同时具有 code 和 message 的安全对象。
    value.and_then(|value| {
        // 读取稳定错误码。
        let code = value.get("code").and_then(serde_json::Value::as_str)?;
        // 读取安全消息。
        let message = value.get("message").and_then(serde_json::Value::as_str)?;
        // 返回独立领域值。
        Some(BrowserSessionFailure {
            // 复制错误码。
            code: code.to_owned(),
            // 复制消息。
            message: message.to_owned(),
        })
    })
}

// 构造内部协议漂移错误。
fn internal_protocol_error(message: &'static str) -> AppControlError {
    // 使用稳定 worker 协议类别。
    AppControlError::new("WORKER_PROTOCOL_FAILED", message)
}

// 验证 Module 拥有的单命令总预算。
fn validate_command_timeout(timeout: Duration) -> AppResult<()> {
    // 转换并限制到冻结页面协议范围。
    let valid = u32::try_from(timeout.as_millis())
        // 平台转换溢出视为无效。
        .ok()
        // 只接受 1..=30000 毫秒。
        .is_some_and(|value| (1..=30_000).contains(&value));
    // 合法预算直接通过。
    if valid {
        // 返回成功。
        return Ok(());
    }
    // 无效预算在任何身份查找和 I/O 前失败。
    Err(AppControlError::new(
        // 使用稳定参数类别。
        "INVALID_ARGUMENT",
        // 不回显调用方原值。
        "Browser page command timeout must be 1..=30000ms.",
    ))
}

// 构造 stale session 错误。
fn stale_session_error() -> AppControlError {
    // 不暴露调用方提交的身份。
    AppControlError::new(
        // 使用稳定 stale 类别。
        "STALE_SESSION",
        // 使用安全诊断。
        "The browser session identity is missing or no longer live.",
    )
}

// 验证 live registry 计数仍可接受新的隔离会话。
fn ensure_open_capacity(live_sessions: usize) -> AppResult<()> {
    // 容量已满时不得启动任何 worker 或创建任何 Job。
    if live_sessions >= MAXIMUM_LIVE_SESSIONS {
        // 返回不暴露既有身份的稳定资源耗尽错误。
        return Err(AppControlError::new(
            // 使用统一资源耗尽类别。
            "BROWSER_SESSION_REGISTRY_FULL",
            // 不暴露现有会话身份。
            "The browser session registry reached its live-session limit.",
        ));
    }
    // 容量尚可用时允许进入后续打开路径。
    Ok(())
}

// 构造 stale page 错误。
fn stale_page_error() -> AppControlError {
    // 不暴露调用方提交的身份。
    AppControlError::new(
        // 使用稳定 stale 类别。
        "STALE_PAGE",
        // 使用安全诊断。
        "The browser page identity is missing or belongs to an older navigation generation.",
    )
}

// 构造 stale element 错误。
fn stale_element_error() -> AppControlError {
    // 不暴露调用方提交的身份。
    AppControlError::new(
        // 使用稳定 stale 类别。
        "STALE_ELEMENT",
        // 使用安全诊断。
        "The browser element identity is missing or belongs to an older navigation generation.",
    )
}

// 验证不启动 worker 的纯 Module 门禁。
#[cfg(test)]
mod tests {
    // 导入 Duration。
    use std::time::Duration;

    // 导入被测 Module。
    use super::{
        // 导入受测 Module 类型与页面请求。
        BrowserSessionModule,
        BrowserTypeRequest,
    };

    // 导入 Module 自身的打开容量门禁和固定上限。
    use super::{MAXIMUM_LIVE_SESSIONS, ensure_open_capacity};

    // 验证 broker 可复用的容量预检不会创建 worker。
    #[test]
    fn open_capacity_preflight_rejects_full_registry() {
        // 将计数直接置为固定上限，避免构造真实浏览器进程。
        let result = ensure_open_capacity(MAXIMUM_LIVE_SESSIONS);
        // 容量预检必须返回结构化拒绝。
        let error = result.expect_err("full registry must reject before business acceptance");
        // 保留不泄漏 live identity 的稳定错误码。
        assert_eq!(error.code, "BROWSER_SESSION_REGISTRY_FULL");
    }

    // 验证操作参数与 deadline 在目标查找前失败闭合。
    #[test]
    fn navigation_preflight_precedes_session_lookup() {
        // 建立空 registry。
        let mut module = BrowserSessionModule::new();
        // 无效 URL 必须优先返回参数错误。
        let invalid_url = module
            // 不启动任何 worker。
            .navigate(
                // 使用不存在的 session。
                "s2:bs:00000000000000000000000000000000",
                // 使用被禁止的 scheme。
                "file:///private".to_owned(),
                // 提供合法预算。
                Duration::from_secs(1),
                // 不触发取消。
                || false,
            )
            // 只取得结构化失败。
            .err()
            // 失败必须发生。
            .unwrap_or_else(|| panic!("invalid URL must fail before session lookup"));
        // 核对稳定参数类别。
        assert_eq!(invalid_url.code, "INVALID_ARGUMENT");
        // 无效 deadline 同样必须优先返回参数错误。
        let invalid_timeout = module
            // 不启动任何 worker。
            .navigate(
                // 使用不存在的 session。
                "s2:bs:00000000000000000000000000000000",
                // 使用合法 URL。
                "https://example.test/page".to_owned(),
                // 使用零预算。
                Duration::ZERO,
                // 不触发取消。
                || false,
            )
            // 只取得结构化失败。
            .err()
            // 失败必须发生。
            .unwrap_or_else(|| panic!("invalid timeout must fail before session lookup"));
        // 核对稳定参数类别。
        assert_eq!(invalid_timeout.code, "INVALID_ARGUMENT");
        // 合法输入才允许进入精确身份查找。
        let stale_session = module
            // 不启动任何 worker。
            .navigate(
                // 使用不存在的 session。
                "s2:bs:00000000000000000000000000000000",
                // 使用合法 URL。
                "https://example.test/page".to_owned(),
                // 使用合法预算。
                Duration::from_secs(1),
                // 不触发取消。
                || false,
            )
            // 只取得结构化失败。
            .err()
            // 精确身份必须 stale。
            .unwrap_or_else(|| panic!("missing session must fail closed"));
        // 核对稳定 stale 类别。
        assert_eq!(stale_session.code, "STALE_SESSION");
        // 未确认写入必须先于全部身份查找。
        let confirmation = module
            // 使用不存在的 session/page/element。
            .click(
                // 使用 canonical stale session。
                "s2:bs:00000000000000000000000000000000",
                // 使用 canonical stale page。
                "s2:bp:00000000000000000000000000000000",
                // 使用 canonical stale element。
                "s2:be:00000000000000000000000000000000",
                // 明确不确认。
                false,
                // 使用合法预算。
                Duration::from_secs(1),
                // 不触发取消。
                || false,
            )
            // 只取得结构化错误。
            .err()
            // 失败必须发生。
            .unwrap_or_else(|| panic!("unconfirmed click must fail before target lookup"));
        // 核对确认优先类别。
        assert_eq!(confirmation.code, "CONFIRMATION_REQUIRED");
        // 已确认但空文本必须在身份查找前返回参数错误。
        let invalid_text = module
            // 使用不存在的 session/page/element。
            .type_text(
                // 使用 canonical stale session。
                "s2:bs:00000000000000000000000000000000",
                // 使用 canonical stale page。
                "s2:bp:00000000000000000000000000000000",
                // 使用 canonical stale element。
                "s2:be:00000000000000000000000000000000",
                // 使用已确认但文本无效的请求。
                BrowserTypeRequest::new(String::new(), false, true),
                // 使用合法预算。
                Duration::from_secs(1),
                // 不触发取消。
                || false,
            )
            // 只取得结构化错误。
            .err()
            // 失败必须发生。
            .unwrap_or_else(|| panic!("invalid type text must fail before target lookup"));
        // 核对参数优先类别。
        assert_eq!(invalid_text.code, "INVALID_ARGUMENT");
    }
}

// 声明 broker close 的 Module 私有生命周期回归。
#[cfg(test)]
#[path = "browser_session_close_tests.rs"]
mod close_tests;
