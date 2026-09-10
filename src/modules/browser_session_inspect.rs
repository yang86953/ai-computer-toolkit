//! 提供 Browser Session Module 自有 registry 的无副作用只读查询。

// 导入统一结果与父 Module 私有实现。
use crate::domain::AppResult;

// 导入父 Module 拥有的 registry 与稳定 stale 错误。
use super::{BrowserSessionModule, stale_session_error};

// 保存 Module 向 System 提供的最小 live 会话事实。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionInspection {
    // 保存 Module 已确认仍 live 的 opaque session identity。
    session_id: String,
    // 保存本次只读查询建立的 live 事实。
    live: bool,
}

// 为最小会话事实提供只读投影。
impl BrowserSessionInspection {
    // 返回 Module 已确认的 opaque session identity。
    pub(crate) fn session_id(&self) -> &str {
        // 借用自有 identity，不暴露 registry entry。
        &self.session_id
    }

    // 返回查询时已经建立的 live 事实。
    pub(crate) const fn live(&self) -> bool {
        // 复制不可变布尔事实。
        self.live
    }
}

// 为 Browser Session Module 提供无隐藏副作用的精确身份查询。
impl BrowserSessionModule {
    // 只读检查目标是否仍属于当前 Module 代际。
    pub(crate) fn inspect_session(
        // 只借用 Module，禁止查询改变 registry 或 worker 生命周期。
        &self,
        // 借用待检查的 opaque session identity。
        session_id: &str,
    ) -> AppResult<BrowserSessionInspection> {
        // 只读取 live registry，不取得 entry 内部资源。
        self.sessions
            // 精确身份存在才建立最小 live 事实。
            .contains_key(session_id)
            // 不复制 worker、Job、stdio、页面或私有引用。
            .then(|| BrowserSessionInspection {
                // 仅复制公开 opaque identity。
                session_id: session_id.to_owned(),
                // 查找成功即表示查询线性化点仍 live。
                live: true,
            })
            // 缺失或已关闭身份保持稳定 stale 语义。
            .ok_or_else(stale_session_error)
    }
}

// 验证查询只读且不会启动真实浏览器。
#[cfg(test)]
mod tests {
    // 导入被测 Module。
    use super::BrowserSessionModule;

    // 验证 stale 查询不改变任何 registry 或后台回收所有权。
    #[test]
    fn stale_inspection_is_read_only() {
        // 建立无 worker 的空 Module。
        let module = BrowserSessionModule::new();
        // 保存查询前全部资源计数。
        let before = (
            // 保存 live registry 长度。
            module.sessions.len(),
            // 保存后台 close task 数量。
            module.pending_close_tasks.len(),
            // 保存待重试回收进程数量。
            module.pending_close_processes.len(),
        );
        // 查询不存在的 canonical identity。
        let error = module
            // 只读调用不得创建 worker。
            .inspect_session("s2:bs:00000000000000000000000000000000")
            // 空 registry 必须返回 stale。
            .expect_err("missing session must be stale");
        // 保留公开稳定错误码。
        assert_eq!(error.code, "STALE_SESSION");
        // 查询后资源计数必须逐项不变。
        assert_eq!(
            // 读取查询后的三项所有权计数。
            (
                // 读取 live registry 长度。
                module.sessions.len(),
                // 读取后台 close task 数量。
                module.pending_close_tasks.len(),
                // 读取待重试回收进程数量。
                module.pending_close_processes.len(),
            ),
            // 对比查询前快照。
            before,
        );
    }
}
