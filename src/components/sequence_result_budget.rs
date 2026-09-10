//! 提供与 provider 无关的 sequence 结果负载字节预算。

// 导入公开 JSON 值类型。
use serde_json::Value;

// 描述一次不能纳入结果预算的负载。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SequenceBudgetExceeded {
    // 保存本次负载序列化后的 UTF-8 字节数。
    pub(crate) attempted_bytes: usize,
    // 保存本次尝试前仍可使用的字节数。
    pub(crate) remaining_bytes: usize,
}

// 跟踪一个 sequence 已接纳的 provider 结果负载字节数。
pub(crate) struct SequenceResultBudget {
    // 保存公开契约允许的最大字节数。
    max_bytes: usize,
    // 保存已经接纳的序列化负载字节数。
    used_bytes: usize,
}

// 实现确定性的结果预算记账。
impl SequenceResultBudget {
    // 创建尚未接纳任何负载的新预算。
    pub(crate) fn new(max_bytes: usize) -> Self {
        // 返回只拥有当前工作流预算的状态。
        Self {
            // 冻结调用方已经验证的最大字节数。
            max_bytes,
            // 初始没有占用结果预算。
            used_bytes: 0,
        }
    }

    // 尝试接纳一个公开 JSON 负载。
    pub(crate) fn reserve(&mut self, payload: &Value) -> Result<usize, SequenceBudgetExceeded> {
        // Value 的显示实现产生与 JSON stdout 相同的紧凑 UTF-8 文本。
        let attempted_bytes = payload.to_string().len();
        // 计算本次尝试前仍可使用的预算。
        let remaining_bytes = self.remaining_bytes();
        // 超出剩余预算时保持已用量不变并返回完整证据。
        if attempted_bytes > remaining_bytes {
            // 返回可由工作流映射的窄错误。
            return Err(SequenceBudgetExceeded {
                // 报告被省略负载的真实序列化字节数。
                attempted_bytes,
                // 报告失败发生前的剩余字节数。
                remaining_bytes,
            });
        }
        // 只在完整负载能够接纳时提交本次占用。
        self.used_bytes += attempted_bytes;
        // 返回本次成功接纳的字节数。
        Ok(attempted_bytes)
    }

    // 返回已经接纳的结果负载字节数。
    pub(crate) const fn used_bytes(&self) -> usize {
        // 暴露只读计数，不交出预算状态。
        self.used_bytes
    }

    // 返回当前仍可使用的结果负载字节数。
    pub(crate) fn remaining_bytes(&self) -> usize {
        // 使用饱和减法防御错误的内部状态。
        self.max_bytes.saturating_sub(self.used_bytes)
    }
}

// 验证负载测量、提交与拒绝语义。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造器和待测预算类型。
    use super::{SequenceBudgetExceeded, SequenceResultBudget};
    // 导入紧凑 JSON 构造宏。
    use serde_json::json;

    // 验证只有完整负载能够接纳时才增加已用量。
    #[test]
    fn budget_commits_only_complete_payloads() {
        // 创建十个字节的确定性预算。
        let mut budget = SequenceResultBudget::new(10);
        // JSON 字符串包含两个引号，因此占用五个字节。
        assert_eq!(budget.reserve(&json!("abc")), Ok(5));
        // 第一次成功后保留五个剩余字节。
        assert_eq!(budget.remaining_bytes(), 5);
        // 六个字节的 JSON 字符串必须被完整拒绝。
        assert_eq!(
            // 尝试接纳超出剩余空间的负载。
            budget.reserve(&json!("abcd")),
            // 核对拒绝证据中的尝试量和剩余量。
            Err(SequenceBudgetExceeded {
                // 四个字符加两个 JSON 引号。
                attempted_bytes: 6,
                // 拒绝前仍有五个字节。
                remaining_bytes: 5,
            })
        );
        // 被拒绝的负载不得改变已用量。
        assert_eq!(budget.used_bytes(), 5);
    }

    // 验证等于剩余空间的负载可以完整接纳。
    #[test]
    fn budget_accepts_exact_remaining_size() {
        // JSON null 的紧凑表示固定为四个字节。
        let mut budget = SequenceResultBudget::new(4);
        // 完整接纳恰好填满预算的负载。
        assert_eq!(budget.reserve(&json!(null)), Ok(4));
        // 填满后剩余量必须为零。
        assert_eq!(budget.remaining_bytes(), 0);
    }
}
