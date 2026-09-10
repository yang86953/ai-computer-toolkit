//! 执行 Browser Session Module 页面动作并解析 worker 结果。

// 导入单命令总预算。
use std::time::Duration;

// 导入页面操作、父聚合结果与随机身份。
use crate::components::{
    // 导入封闭 worker 页面操作。
    browser_page_protocol::BrowserPageOperation,
    // 导入父进程聚合结果。
    browser_session_process::BrowserPageCommandResult,
    // 导入随机公开身份材料。
    secure_nonce_windows::random_nonce,
};
// 导入统一错误与结果。
use crate::domain::{AppControlError, AppResult};

// 导入同一 Module 的公开动作类型。
use super::actions::{
    // 导入点击数据。
    BrowserClickData,
    // 导入公开元素摘要。
    BrowserElementMatch,
    // 导入查询数据。
    BrowserQueryData,
    // 导入截图数据。
    BrowserScreenshotData,
    // 导入 selector。
    BrowserSemanticSelector,
    // 导入 wait 条件。
    BrowserSemanticWaitCondition,
    // 导入通用命令报告。
    BrowserSessionCommandReport,
    // 导入输入数据。
    BrowserTypeData,
    // 导入文本输入请求。
    BrowserTypeRequest,
    // 导入 wait 数据。
    BrowserWaitData,
};
// 导入同一 Module 的状态与边界。
use super::{
    // 导入公开元素记录。
    BrowserElementEntry,
    // 导入唯一 Module 所有者。
    BrowserSessionModule,
    // 导入元素容量。
    MAXIMUM_PUBLIC_ELEMENTS,
    // 导入元素身份前缀。
    PUBLIC_ELEMENT_PREFIX,
    // 导入内部协议错误。
    internal_protocol_error,
    // 导入命令类别映射。
    map_command_outcome,
    // 导入安全错误投影。
    project_failure,
    // 导入 stale element 错误。
    stale_element_error,
    // 导入 stale page 错误。
    stale_page_error,
    // 导入 stale session 错误。
    stale_session_error,
    // 导入 deadline 验证。
    validate_command_timeout,
};

// 为 Browser Session Module 实现其余五类页面操作。
impl BrowserSessionModule {
    // 等待当前公开页面满足封闭条件。
    pub(crate) fn wait(
        // 可变借用唯一 Module。
        &mut self,
        // 借用公开 session。
        session_id: &str,
        // 借用公开 page。
        page_id: &str,
        // 取得 Module wait 条件。
        condition: BrowserSemanticWaitCondition,
        // 接收单命令总预算。
        timeout: Duration,
        // 接收取消观察。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<BrowserSessionCommandReport<BrowserWaitData>> {
        // 构造封闭 worker 操作。
        let operation = BrowserPageOperation::Wait {
            // 转换 wait 条件。
            condition: condition.into_worker(),
        };
        // 执行当前页面命令。
        let result = self.execute_current_page(
            // 绑定 session。
            session_id, // 绑定 page。
            page_id,    // 传入操作。
            operation,  // 传入总预算。
            timeout,    // 传入取消观察。
            cancelled,
        )?;
        // completed 必须携带固定 conditionMet。
        let data = if result.outcome()
            == crate::components::browser_page_protocol::BrowserPageOutcome::Completed
        {
            // 读取已验证字段。
            let condition_met = required_bool(result.data(), "conditionMet")
                // 漂移时失效会话。
                .map_err(|error| self.invalidate_protocol(session_id, error))?;
            // 建立领域数据。
            Some(BrowserWaitData { condition_met })
        } else {
            // 非成功不携带数据。
            None
        };
        // 投影通用结果。
        Ok(project_report(&result, data))
    }

    // 查询当前公开页面并签发公开元素身份。
    pub(crate) fn query(
        // 可变借用唯一 Module。
        &mut self,
        // 借用公开 session。
        session_id: &str,
        // 借用公开 page。
        page_id: &str,
        // 取得 provider-neutral selector。
        selector: BrowserSemanticSelector,
        // 接收结果上限。
        max_results: u16,
        // 接收单命令总预算。
        timeout: Duration,
        // 接收取消观察。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<BrowserSessionCommandReport<BrowserQueryData>> {
        // 构造封闭 worker 操作。
        let operation = BrowserPageOperation::Query {
            // 转换 selector。
            selector: selector.into_worker(),
            // 保存结果上限。
            max_results,
        };
        // 执行当前页面命令。
        let result = self.execute_current_page(
            // 绑定 session。
            session_id, // 绑定 page。
            page_id,    // 传入操作。
            operation,  // 传入总预算。
            timeout,    // 传入取消观察。
            cancelled,
        )?;
        // completed 时转换私有元素引用。
        let data = if result.outcome()
            == crate::components::browser_page_protocol::BrowserPageOutcome::Completed
        {
            // 投影 query 数据并更新当前元素 registry。
            Some(self.project_query(session_id, page_id, result.data())?)
        } else {
            // 非成功不携带数据。
            None
        };
        // 投影通用结果。
        Ok(project_report(&result, data))
    }

    // 点击当前页面代际内的公开元素。
    pub(crate) fn click(
        // 可变借用唯一 Module。
        &mut self,
        // 借用公开 session。
        session_id: &str,
        // 借用公开 page。
        page_id: &str,
        // 借用公开 element。
        element_id: &str,
        // 接收显式确认。
        confirmed: bool,
        // 接收单命令总预算。
        timeout: Duration,
        // 接收取消观察。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<BrowserSessionCommandReport<BrowserClickData>> {
        // 确认必须先于 deadline、目标查找和任何 worker I/O。
        require_confirmation(confirmed)?;
        // deadline 必须在目标查找前验证。
        validate_command_timeout(timeout)?;
        // 解析当前公开元素为私有 worker facts。
        let (page_ref, generation, element_ref) =
            self.current_element_facts(session_id, page_id, element_id)?;
        // 构造固定确认式点击。
        let operation = BrowserPageOperation::Click {
            // worker 必须再次验证确认。
            confirmed: true,
            // 只回传私有元素引用。
            element_ref,
        };
        // 执行已解析页面命令。
        let result = self.execute_bound(
            // 绑定 session。
            session_id, // 传入私有 page ref。
            page_ref,   // 传入当前代际。
            generation, // 传入操作。
            operation,  // 传入总预算。
            timeout,    // 传入取消观察。
            cancelled,
        )?;
        // completed 必须携带 clicked=true。
        let data = if result.outcome()
            == crate::components::browser_page_protocol::BrowserPageOutcome::Completed
        {
            // 读取固定成功事实。
            let clicked = required_bool(result.data(), "clicked")
                // 漂移时失效会话。
                .map_err(|error| self.invalidate_protocol(session_id, error))?;
            // 建立领域数据。
            Some(BrowserClickData { clicked })
        } else {
            // 非成功不携带数据。
            None
        };
        // 投影通用结果。
        Ok(project_report(&result, data))
    }

    // 向当前页面代际内的公开元素输入文本。
    pub(crate) fn type_text(
        // 可变借用唯一 Module。
        &mut self,
        // 借用公开 session。
        session_id: &str,
        // 借用公开 page。
        page_id: &str,
        // 借用公开 element。
        element_id: &str,
        // 取得确认式文本输入请求。
        request: BrowserTypeRequest,
        // 接收单命令总预算。
        timeout: Duration,
        // 接收取消观察。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<BrowserSessionCommandReport<BrowserTypeData>> {
        // 确认必须先于文本验证、目标查找和任何 worker I/O。
        require_confirmation(request.confirmed)?;
        // 用 canonical 占位私有引用先验证文本与 replace 组合。
        let preflight = BrowserPageOperation::Type {
            // 确认已经由 Module 验证。
            confirmed: true,
            // 占位引用仅进入纯验证器，不发送 worker。
            element_ref: "w1:be:00000000000000000000000000000000".to_owned(),
            // 复制有界候选文本。
            text: request.text.clone(),
            // 复制替换要求。
            replace: request.replace,
        };
        // 文本边界必须在目标查找前失败闭合。
        preflight
            .validate()
            .map_err(|_| invalid_operation_error())?;
        // deadline 同样必须先于目标查找。
        validate_command_timeout(timeout)?;
        // 解析当前公开元素为私有 worker facts。
        let (page_ref, generation, element_ref) =
            self.current_element_facts(session_id, page_id, element_id)?;
        // 构造固定确认式输入。
        let operation = BrowserPageOperation::Type {
            // worker 必须再次验证确认。
            confirmed: true,
            // 只回传私有元素引用。
            element_ref,
            // 保存调用方文本。
            text: request.text,
            // 保存替换要求。
            replace: request.replace,
        };
        // 执行已解析页面命令。
        let result = self.execute_bound(
            // 绑定 session。
            session_id, // 传入私有 page ref。
            page_ref,   // 传入当前代际。
            generation, // 传入操作。
            operation,  // 传入总预算。
            timeout,    // 传入取消观察。
            cancelled,
        )?;
        // completed 必须携带 typed 与 utf8Bytes。
        let data = if result.outcome()
            == crate::components::browser_page_protocol::BrowserPageOutcome::Completed
        {
            // 读取输入完成事实。
            let typed = required_bool(result.data(), "typed")
                // 漂移时失效会话。
                .map_err(|error| self.invalidate_protocol(session_id, error))?;
            // 读取有界字节数。
            let utf8_bytes = required_u32(result.data(), "utf8Bytes")
                // 漂移时失效会话。
                .map_err(|error| self.invalidate_protocol(session_id, error))?;
            // 建立领域数据。
            Some(BrowserTypeData { typed, utf8_bytes })
        } else {
            // 非成功不携带数据。
            None
        };
        // 投影通用结果。
        Ok(project_report(&result, data))
    }

    // 捕获当前公开页面的 provider-neutral PNG。
    pub(crate) fn screenshot(
        // 可变借用唯一 Module。
        &mut self,
        // 借用公开 session。
        session_id: &str,
        // 借用公开 page。
        page_id: &str,
        // 接收单命令总预算。
        timeout: Duration,
        // 接收取消观察。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<BrowserSessionCommandReport<BrowserScreenshotData>> {
        // 执行固定截图操作。
        let result = self.execute_current_page(
            // 绑定 session。
            session_id,
            // 绑定 page。
            page_id,
            // 不接受调用方 CDP 参数。
            BrowserPageOperation::Screenshot {},
            // 传入总预算。
            timeout,
            // 传入取消观察。
            cancelled,
        )?;
        // completed 时转换全部固定截图字段。
        let data = if result.outcome()
            == crate::components::browser_page_protocol::BrowserPageOutcome::Completed
        {
            // 投影截图领域值。
            Some(
                project_screenshot(result.data())
                    // 漂移时失效会话。
                    .map_err(|error| self.invalidate_protocol(session_id, error))?,
            )
        } else {
            // 非成功不携带数据。
            None
        };
        // 投影通用结果。
        Ok(project_report(&result, data))
    }

    // 执行一个需要当前公开 page 的操作。
    fn execute_current_page(
        // 可变借用唯一 Module。
        &mut self,
        // 借用公开 session。
        session_id: &str,
        // 借用公开 page。
        page_id: &str,
        // 取得强类型操作。
        operation: BrowserPageOperation,
        // 接收总预算。
        timeout: Duration,
        // 接收取消观察。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<BrowserPageCommandResult> {
        // 操作与预算必须在目标查找前失败闭合。
        operation
            .validate()
            .map_err(|_| invalid_operation_error())?;
        // 验证 Module 总预算。
        validate_command_timeout(timeout)?;
        // 解析当前页面私有 facts。
        let (page_ref, generation) = self.current_page_facts(session_id, page_id)?;
        // 执行已绑定操作。
        self.execute_bound(
            // 绑定 session。
            session_id, // 传入私有 page ref。
            page_ref,   // 传入代际。
            generation, // 传入操作。
            operation,  // 传入总预算。
            timeout,    // 传入取消观察。
            cancelled,
        )
    }

    // 执行已经解析当前 page/element 的操作。
    fn execute_bound(
        // 可变借用唯一 Module。
        &mut self,
        // 借用公开 session。
        session_id: &str,
        // 取得私有 page ref。
        page_ref: String,
        // 接收当前代际。
        generation: u64,
        // 取得强类型操作。
        operation: BrowserPageOperation,
        // 接收总预算。
        timeout: Duration,
        // 接收取消观察。
        cancelled: impl Fn() -> bool,
    ) -> AppResult<BrowserPageCommandResult> {
        // 写操作也必须在 I/O 前复用协议验证器。
        operation
            .validate()
            .map_err(|_| invalid_operation_error())?;
        // 写操作的 deadline 在 I/O 前验证。
        validate_command_timeout(timeout)?;
        // 只允许当前 live session。
        let entry = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(stale_session_error)?;
        // 串行执行私有页面命令。
        let result = match entry.process.execute_page(
            // 只传入私有 page ref。
            Some(&page_ref),
            // 传入当前代际。
            generation,
            // 转移操作。
            operation,
            // 传入总预算。
            timeout,
            // 传入取消观察。
            cancelled,
        ) {
            // 保留可信聚合。
            Ok(result) => result,
            // Component 错误后完整会话失败闭合。
            Err(error) => {
                // 显式结束当前借用。
                let _ = entry;
                // 原子失效会话并把精确资源所有权转入后台回收。
                self.retire_session(session_id);
                // 返回结构化错误。
                return Err(error);
            }
        };
        // 复制连接是否已经不再可信。
        let session_invalidated = result.session_invalidated();
        // 显式结束当前借用。
        let _ = entry;
        // 不可信 worker 必须从 registry 失效并保持异步资源所有权。
        if session_invalidated {
            // 把精确进程、Job 与 profile 析构路径转入 Module 自有后台任务。
            self.retire_session(session_id);
        }
        // 返回聚合结果。
        Ok(result)
    }

    // 解析当前公开 page 为私有 facts。
    fn current_page_facts(&self, session_id: &str, page_id: &str) -> AppResult<(String, u64)> {
        // 只允许当前 live session。
        let entry = self
            .sessions
            .get(session_id)
            .ok_or_else(stale_session_error)?;
        // 只允许当前 page 身份。
        let page = entry
            // 借用当前页面。
            .page
            // 映射可选页面。
            .as_ref()
            // 精确匹配公开身份。
            .filter(|page| page.public_id == page_id)
            // 旧代际统一 stale。
            .ok_or_else(stale_page_error)?;
        // 只向 Module 内部复制私有 facts。
        Ok((page.worker_ref.clone(), page.navigation_generation))
    }

    // 解析当前公开 element 为私有 page 与 element facts。
    fn current_element_facts(
        // 借用唯一 Module。
        &self,
        // 借用公开 session。
        session_id: &str,
        // 借用公开 page。
        page_id: &str,
        // 借用公开 element。
        element_id: &str,
    ) -> AppResult<(String, u64, String)> {
        // 只允许当前 live session。
        let entry = self
            .sessions
            .get(session_id)
            .ok_or_else(stale_session_error)?;
        // 只允许当前 page 身份。
        let page = entry
            // 借用当前页面。
            .page
            // 映射可选页面。
            .as_ref()
            // 精确匹配公开身份。
            .filter(|page| page.public_id == page_id)
            // 旧代际统一 stale page。
            .ok_or_else(stale_page_error)?;
        // 只允许当前页面代际签发的元素。
        let element = page
            // 查找公开元素身份。
            .elements
            // 执行精确查找。
            .get(element_id)
            // 缺失统一 stale element。
            .ok_or_else(stale_element_error)?;
        // 返回仅供 worker 调用的私有 facts。
        Ok((
            // 复制 page ref。
            page.worker_ref.clone(),
            // 复制导航代际。
            page.navigation_generation,
            // 复制 element ref。
            element.worker_ref.clone(),
        ))
    }

    // 把 worker query 数据映射为当前公开元素 registry。
    fn project_query(
        // 可变借用唯一 Module。
        &mut self,
        // 借用公开 session。
        session_id: &str,
        // 借用公开 page。
        page_id: &str,
        // 借用已验证成功数据。
        value: Option<&serde_json::Value>,
    ) -> AppResult<BrowserQueryData> {
        // 取得固定 matches 数组。
        let matches = value
            // 读取成功对象。
            .and_then(|value| value.get("matches"))
            // 转换为数组。
            .and_then(serde_json::Value::as_array)
            // 缺失表示内部协议漂移。
            .ok_or_else(|| internal_protocol_error("The browser query matches are missing."))?;
        // 读取固定总命中数。
        let match_count = required_u32(value, "matchCount")?;
        // 读取固定截断事实。
        let truncated = required_bool(value, "truncated")?;
        // 重新取得当前 live session。
        let entry = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(stale_session_error)?;
        // 重新取得同一当前页面，防止错误更新旧代际。
        let page = entry
            // 借用当前页面。
            .page
            // 映射可选页面。
            .as_mut()
            // 精确匹配公开 page。
            .filter(|page| page.public_id == page_id)
            // 旧代际统一 stale。
            .ok_or_else(stale_page_error)?;
        // 建立有界公开结果。
        let mut projected = Vec::with_capacity(matches.len());
        // 逐项转换私有引用。
        for item in matches {
            // 读取 worker 私有 element ref。
            let worker_ref = item
                // 读取固定字段。
                .get("elementRef")
                // 转换为字符串。
                .and_then(serde_json::Value::as_str)
                // 缺失表示内部协议漂移。
                .ok_or_else(|| {
                    internal_protocol_error("The browser query element reference is missing.")
                })?;
            // 相同 worker ref 在当前代际复用同一公开 ID。
            let public_id = if let Some(existing) = page
                // 遍历当前映射。
                .elements
                // 取得键值迭代。
                .iter()
                // 查找同一私有引用。
                .find_map(|(public_id, element)| {
                    // 命中时复制公开身份。
                    (element.worker_ref == worker_ref).then(|| public_id.clone())
                }) {
                // 复用稳定公开身份。
                existing
            } else {
                // 新映射必须遵守容量上限。
                if page.elements.len() >= MAXIMUM_PUBLIC_ELEMENTS {
                    // 返回稳定资源错误且不驱逐旧映射。
                    return Err(AppControlError::new(
                        // 复用已登记元素容量类别。
                        "BROWSER_ELEMENT_REGISTRY_FULL",
                        // 使用安全诊断。
                        "The public browser element registry reached its resource boundary.",
                    ));
                }
                // 生成与 worker ref 无关的随机公开元素 ID。
                let public_id = format!("{PUBLIC_ELEMENT_PREFIX}{}", random_nonce()?);
                // 插入唯一映射。
                page.elements.insert(
                    // 使用公开 ID 作键。
                    public_id.clone(),
                    // 只保存私有引用。
                    BrowserElementEntry {
                        // 复制私有 ref。
                        worker_ref: worker_ref.to_owned(),
                    },
                );
                // 返回新公开身份。
                public_id
            };
            // 投影 provider-neutral 元素摘要。
            projected.push(BrowserElementMatch {
                // 保存公开 identity。
                element_id: public_id,
                // 复制可选 role。
                role: optional_string(item, "role")?,
                // 复制可选名称。
                name: optional_string(item, "name")?,
                // 复制可选文本。
                text: optional_string(item, "text")?,
                // 读取可用事实。
                enabled: item
                    // 读取固定字段。
                    .get("enabled")
                    // 转换为布尔值。
                    .and_then(serde_json::Value::as_bool)
                    // 缺失表示内部协议漂移。
                    .ok_or_else(|| {
                        internal_protocol_error("The browser query enabled fact is missing.")
                    })?,
            });
        }
        // 返回完整 query 领域数据。
        Ok(BrowserQueryData {
            // 保存公开命中。
            matches: projected,
            // 保存总命中数。
            match_count,
            // 保存截断事实。
            truncated,
        })
    }

    // 在结果投影漂移后失效完整会话。
    fn invalidate_protocol(
        // 可变借用唯一 Module。
        &mut self,
        // 借用公开 session。
        session_id: &str,
        // 取得原协议错误。
        error: AppControlError,
    ) -> AppControlError {
        // 原子失效会话并把精确资源所有权转入后台回收。
        self.retire_session(session_id);
        // 返回原安全错误。
        error
    }
}

// 把父聚合结果转换为泛型 Module 报告。
fn project_report<T>(
    // 借用可信父聚合。
    result: &BrowserPageCommandResult,
    // 取得可选领域数据。
    data: Option<T>,
) -> BrowserSessionCommandReport<T> {
    // 构造不含 worker 私有引用的报告。
    BrowserSessionCommandReport {
        // 映射封闭类别。
        outcome: map_command_outcome(result.outcome()),
        // 复制完成事实。
        completed: result.completed(),
        // 复制重试事实。
        retry_safe: result.retry_safe(),
        // 复制可能接受事实。
        accepted_may_have_occurred: result.accepted_may_have_occurred(),
        // 复制导航代际。
        navigation_generation: result.navigation_generation(),
        // 保存领域数据。
        data,
        // 投影安全错误。
        error: project_failure(result.error()),
        // 复制回收事实。
        forced_reap: result.forced_reap(),
    }
}

// 投影截图固定字段。
fn project_screenshot(value: Option<&serde_json::Value>) -> AppResult<BrowserScreenshotData> {
    // 读取成功对象。
    let value =
        value.ok_or_else(|| internal_protocol_error("The browser screenshot data is missing."))?;
    // 构造 provider-neutral 截图数据。
    Ok(BrowserScreenshotData {
        // 复制 MIME。
        mime_type: required_string(Some(value), "mimeType")?,
        // 复制 PNG Base64。
        png_base64: required_string(Some(value), "pngBase64")?,
        // 读取字节数。
        png_bytes: required_u64(Some(value), "pngBytes")?,
        // 读取宽度。
        width: required_u32(Some(value), "width")?,
        // 读取高度。
        height: required_u32(Some(value), "height")?,
        // 复制摘要。
        digest: required_string(Some(value), "digest")?,
    })
}

// 要求显式确认。
fn require_confirmation(confirmed: bool) -> AppResult<()> {
    // 确认成立直接通过。
    if confirmed {
        // 返回成功。
        return Ok(());
    }
    // 未确认在任何目标访问前失败。
    Err(AppControlError::new(
        // 使用统一确认错误码。
        "CONFIRMATION_REQUIRED",
        // 使用安全诊断。
        "The browser page write operation requires explicit confirmation.",
    ))
}

// 构造页面操作参数错误。
fn invalid_operation_error() -> AppControlError {
    // 不回显文本或 selector。
    AppControlError::new(
        // 使用稳定参数类别。
        "INVALID_ARGUMENT",
        // 使用安全诊断。
        "The browser page operation was invalid.",
    )
}

// 读取必需布尔字段。
fn required_bool(value: Option<&serde_json::Value>, key: &str) -> AppResult<bool> {
    // 只接受固定布尔值。
    value
        // 读取字段。
        .and_then(|value| value.get(key))
        // 转换为布尔值。
        .and_then(serde_json::Value::as_bool)
        // 缺失统一内部协议错误。
        .ok_or_else(|| internal_protocol_error("The browser page result boolean fact is missing."))
}

// 读取必需 u32 字段。
fn required_u32(value: Option<&serde_json::Value>, key: &str) -> AppResult<u32> {
    // 先读取 u64。
    let value = required_u64(value, key)?;
    // 有界转换为 u32。
    u32::try_from(value)
        // 溢出统一内部协议错误。
        .map_err(|_| internal_protocol_error("The browser page result integer fact overflowed."))
}

// 读取必需 u64 字段。
fn required_u64(value: Option<&serde_json::Value>, key: &str) -> AppResult<u64> {
    // 只接受非负整数。
    value
        // 读取字段。
        .and_then(|value| value.get(key))
        // 转换为 u64。
        .and_then(serde_json::Value::as_u64)
        // 缺失统一内部协议错误。
        .ok_or_else(|| internal_protocol_error("The browser page result integer fact is missing."))
}

// 读取必需字符串字段。
fn required_string(value: Option<&serde_json::Value>, key: &str) -> AppResult<String> {
    // 只接受字符串并独立保存。
    value
        // 读取字段。
        .and_then(|value| value.get(key))
        // 转换为字符串。
        .and_then(serde_json::Value::as_str)
        // 复制领域值。
        .map(str::to_owned)
        // 缺失统一内部协议错误。
        .ok_or_else(|| internal_protocol_error("The browser page result string fact is missing."))
}

// 读取可选字符串字段。
fn optional_string(value: &serde_json::Value, key: &str) -> AppResult<Option<String>> {
    // 字段必须存在。
    let value = value
        // 读取字段。
        .get(key)
        // 缺失表示内部协议漂移。
        .ok_or_else(|| {
            internal_protocol_error("The browser query optional text fact is missing.")
        })?;
    // null 映射为空。
    if value.is_null() {
        // 返回空值。
        return Ok(None);
    }
    // 字符串映射为独立值。
    value
        // 转换为字符串。
        .as_str()
        // 复制领域值。
        .map(|value| Some(value.to_owned()))
        // 其他类型统一漂移。
        .ok_or_else(|| internal_protocol_error("The browser query optional text fact was invalid."))
}
