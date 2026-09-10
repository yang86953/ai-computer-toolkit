//! 提供 Browser Session Module 的页面身份只读预检。

// 导入统一结果类型。
use crate::domain::AppResult;

// 导入同一 Module 的状态与稳定 stale 错误。
use super::{BrowserSessionModule, stale_element_error, stale_page_error, stale_session_error};

// 为 Browser Session Module 提供不派发 worker I/O 的页面预检。
impl BrowserSessionModule {
    // 验证 session 与 page 同时属于当前导航代际。
    pub(crate) fn prepare_page(&self, session_id: &str, page_id: &str) -> AppResult<()> {
        // 先验证 session 仍由当前 Module 代际拥有。
        let entry = self
            // 查询唯一 live registry。
            .sessions
            // 借用精确 session。
            .get(session_id)
            // 缺失时返回稳定 stale session。
            .ok_or_else(stale_session_error)?;
        // 再验证公开 page 与当前唯一页面逐字相等。
        let current = entry
            // 借用当前页面映射。
            .page
            // 页面尚未创建时同样 stale。
            .as_ref()
            // 缺失时返回稳定 stale page。
            .ok_or_else(stale_page_error)?;
        // 旧代际或其他会话 page 都必须失败闭合。
        if current.public_id != page_id {
            // 返回不回显 page 的稳定错误。
            return Err(stale_page_error());
        }
        // 当前 session/page 对可安全进入 broker accepted 线性化点。
        Ok(())
    }

    // 验证 session、page 与 element 同时属于当前导航代际。
    pub(crate) fn prepare_element(
        // 只读借用唯一 Module。
        &self,
        // 借用公开 session identity。
        session_id: &str,
        // 借用公开 page identity。
        page_id: &str,
        // 借用公开 element identity。
        element_id: &str,
    ) -> AppResult<()> {
        // 先验证 session，固定最高错误优先级。
        let entry = self
            // 查询唯一 live registry。
            .sessions
            // 借用精确 session。
            .get(session_id)
            // 缺失时返回稳定 stale session。
            .ok_or_else(stale_session_error)?;
        // 再验证当前 page，固定第二错误优先级。
        let current = entry
            // 借用当前页面映射。
            .page
            // 页面尚未创建时同样 stale。
            .as_ref()
            // 只接受当前公开 page。
            .filter(|page| page.public_id == page_id)
            // 旧代际统一 stale page。
            .ok_or_else(stale_page_error)?;
        // 最后只检查当前页面代际签发的 element。
        if !current.elements.contains_key(element_id) {
            // 不回显调用方 identity。
            return Err(stale_element_error());
        }
        // 当前三级 identity 可安全进入 broker accepted 线性化点。
        Ok(())
    }
}

// 验证空 Module 的预检错误优先级且不启动 worker。
#[cfg(test)]
mod tests {
    // 导入被测 Module。
    use super::BrowserSessionModule;

    // 空 registry 必须优先返回 stale session。
    #[test]
    fn missing_session_precedes_page_lookup() {
        // 构造不启动 worker 的空 Module。
        let module = BrowserSessionModule::new();
        // 对 canonical 缺失目标执行纯预检。
        let error = module
            // 同时传入缺失 session 与 page。
            .prepare_page(
                // 使用 canonical session 形状。
                "s2:bs:00000000000000000000000000000000",
                // 使用 canonical page 形状。
                "s2:bp:00000000000000000000000000000000",
            )
            // 空 registry 必须失败。
            .expect_err("missing session must fail before page lookup");
        // 保留稳定 session stale 语义。
        assert_eq!(error.code, "STALE_SESSION");
    }
}
