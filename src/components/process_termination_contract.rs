//! 定义 provider-neutral 进程优雅与强制终止风险及有界输入契约。

// 导入严格 JSON 反序列化。
use serde::Deserialize;
// 导入公开 JSON 值。
use serde_json::Value;

// 导入统一结果与错误类型。
use crate::domain::{AppControlError, AppResult};

// 固定同步进程终止请求最短 deadline。
pub(crate) const MINIMUM_PROCESS_TERMINATION_TIMEOUT_MS: u32 = 1;
// 固定同步进程终止请求最长 deadline。
pub(crate) const MAXIMUM_PROCESS_TERMINATION_TIMEOUT_MS: u32 = 30_000;
// 固定同步进程终止请求缺省 deadline。
pub(crate) const DEFAULT_PROCESS_TERMINATION_TIMEOUT_MS: u32 = 5_000;

// 表示调用方明确选择的进程终止风险等级。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProcessTerminationMode {
    // 只请求进程自行退出，绝不升级为强制终止。
    Graceful,
    // 明确使用不可回滚的内核强制终止。
    Force,
}

// 提供稳定 provider-neutral 风险与动作文本。
impl ProcessTerminationMode {
    // 返回公开动作名称。
    pub(crate) const fn action(self) -> &'static str {
        // 穷举两个独立风险级别。
        match self {
            // 映射优雅终止动作。
            Self::Graceful => "terminate-graceful",
            // 映射强制终止动作。
            Self::Force => "terminate-force",
        }
    }

    // 返回公开风险等级。
    pub(crate) const fn risk_level(self) -> &'static str {
        // 风险文本由 capability 固定而非调用方输入决定。
        match self {
            // 优雅请求仍可能触发保存提示或进程退出。
            Self::Graceful => "high",
            // 强制终止可能造成未保存数据丢失。
            Self::Force => "critical",
        }
    }

    // 返回认证平台机制类别。
    pub(crate) const fn mechanism(self) -> &'static str {
        // 只公开 provider-neutral 机制，不公开 API 或句柄。
        match self {
            // 优雅路径只请求顶层窗口关闭。
            Self::Graceful => "top-level-window-close-request",
            // 强制路径只执行内核进程终止。
            Self::Force => "kernel-process-termination",
        }
    }
}

// 保存通过验证的进程终止请求。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProcessTerminationInput {
    // 保存由 capability ID 选择的固定风险模式。
    pub(crate) mode: ProcessTerminationMode,
    // 保存单调 deadline。
    pub(crate) timeout_ms: u32,
}

// 保存公开进程终止 JSON 形状。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessTerminationWire {
    // 保存可选 deadline。
    #[serde(
        default = "default_process_termination_timeout_ms",
        rename = "timeoutMs"
    )]
    timeout_ms: u32,
}

// 返回 serde 使用的缺省 deadline。
const fn default_process_termination_timeout_ms() -> u32 {
    // 复用公开固定常量。
    DEFAULT_PROCESS_TERMINATION_TIMEOUT_MS
}

// 构造不回显原始 JSON 的稳定参数错误。
fn invalid_argument(message: impl Into<String>) -> AppControlError {
    // 返回统一公开错误 envelope。
    AppControlError::new("INVALID_ARGUMENT", message)
}

// 严格解析由 capability 已经选择风险模式的进程终止输入。
pub(crate) fn parse_process_termination_input(
    // 接收不可由 JSON 改写的固定风险模式。
    mode: ProcessTerminationMode,
    // 接收调用方 provider-neutral input。
    value: &Value,
) -> AppResult<ProcessTerminationInput> {
    // Serde 的缺省字段规则会把 null 接受为空输入，因此先固定对象边界。
    if !value.is_object() {
        // 返回不包含原始 JSON 的稳定形状错误。
        return Err(invalid_argument(
            "Process termination input must be a JSON object.",
        ));
    }
    // 严格反序列化仅含可选 deadline 的对象。
    let wire: ProcessTerminationWire = serde_json::from_value(value.clone()).map_err(|_| {
        // 不穿透 serde 可能携带的输入片段。
        invalid_argument("Process termination input does not match process-termination-input-v1.")
    })?;
    // deadline 必须处于认证同步范围。
    if !(MINIMUM_PROCESS_TERMINATION_TIMEOUT_MS..=MAXIMUM_PROCESS_TERMINATION_TIMEOUT_MS)
        // 核对调用方值。
        .contains(&wire.timeout_ms)
    {
        // 返回固定 deadline 边界错误。
        return Err(invalid_argument(format!(
            "Process termination timeoutMs must be within {MINIMUM_PROCESS_TERMINATION_TIMEOUT_MS}..={MAXIMUM_PROCESS_TERMINATION_TIMEOUT_MS}."
        )));
    }
    // 返回由 capability 与严格输入共同形成的领域对象。
    Ok(ProcessTerminationInput {
        // 保存固定风险模式。
        mode,
        // 保存已验证 deadline。
        timeout_ms: wire.timeout_ms,
    })
}

// 声明无平台依赖的纯契约回归测试。
#[cfg(test)]
mod tests {
    // 导入被测契约。
    use super::*;
    // 导入 JSON 构造宏。
    use serde_json::json;

    // 验证两个 capability 风险模式互不推断或回退。
    #[test]
    fn graceful_and_force_modes_keep_distinct_contract_facts() -> AppResult<()> {
        // 解析空优雅输入。
        let graceful =
            parse_process_termination_input(ProcessTerminationMode::Graceful, &json!({}))?;
        // 解析空强制输入。
        let force = parse_process_termination_input(ProcessTerminationMode::Force, &json!({}))?;
        // 核对两个请求都使用缺省 deadline。
        assert_eq!(graceful.timeout_ms, DEFAULT_PROCESS_TERMINATION_TIMEOUT_MS);
        // 核对强制路径使用同一有界缺省值。
        assert_eq!(force.timeout_ms, DEFAULT_PROCESS_TERMINATION_TIMEOUT_MS);
        // 优雅动作保持独立公开文本。
        assert_eq!(graceful.mode.action(), "terminate-graceful");
        // 强制动作保持独立公开文本。
        assert_eq!(force.mode.action(), "terminate-force");
        // 优雅风险不能冒充强制风险。
        assert_eq!(graceful.mode.risk_level(), "high");
        // 强制风险必须明确为 critical。
        assert_eq!(force.mode.risk_level(), "critical");
        // 两种机制必须保持不同。
        assert_ne!(graceful.mode.mechanism(), force.mode.mechanism());
        // 完成纯契约验证。
        Ok(())
    }

    // 验证 deadline 闭区间与调用方整数保持不变。
    #[test]
    fn timeout_is_strictly_bounded() -> AppResult<()> {
        // 解析最短合法 deadline。
        let minimum = parse_process_termination_input(
            // 使用优雅模式隔离输入边界。
            ProcessTerminationMode::Graceful,
            // 提供最短整数。
            &json!({ "timeoutMs": 1 }),
        )?;
        // 核对最短值未被改写。
        assert_eq!(minimum.timeout_ms, 1);
        // 解析最长合法 deadline。
        let maximum = parse_process_termination_input(
            // 使用强制模式证明边界一致。
            ProcessTerminationMode::Force,
            // 提供最长整数。
            &json!({ "timeoutMs": 30_000 }),
        )?;
        // 核对最长值未被改写。
        assert_eq!(maximum.timeout_ms, 30_000);
        // 逐项拒绝上下界之外的值。
        for value in [json!({ "timeoutMs": 0 }), json!({ "timeoutMs": 30_001 })] {
            // 解析非法输入并显式区分结果。
            let error = match parse_process_termination_input(ProcessTerminationMode::Force, &value)
            {
                // 成功表示 deadline 门禁失效。
                Ok(_) => panic!("out-of-range process termination timeout must fail"),
                // 保存预期错误。
                Err(error) => error,
            };
            // 核对统一参数错误码。
            assert_eq!(error.code, "INVALID_ARGUMENT");
        }
        // 完成边界验证。
        Ok(())
    }

    // 验证调用方不能用 input 改写风险、平台或原生目标。
    #[test]
    fn risk_native_and_fallback_fields_are_rejected() {
        // 构造全部禁止字段类别。
        let values = [
            // 风险只能由 capability 选择。
            json!({ "mode": "force" }),
            // 不接受平台信号。
            json!({ "signal": 9 }),
            // 不接受调用方退出码。
            json!({ "exitCode": 1 }),
            // 不接受原生 PID。
            json!({ "processId": 42 }),
            // 不接受静默升级开关。
            json!({ "forceOnTimeout": true }),
        ];
        // 逐项验证严格字段集合。
        for value in values {
            // 解析非法输入并显式区分结果。
            let error =
                match parse_process_termination_input(ProcessTerminationMode::Graceful, &value) {
                    // 成功表示封闭字段门禁失效。
                    Ok(_) => panic!("process termination input must reject risk or native fields"),
                    // 保存预期错误。
                    Err(error) => error,
                };
            // 核对稳定参数错误码。
            assert_eq!(error.code, "INVALID_ARGUMENT");
        }
    }

    // 验证输入必须是对象且 timeout 必须是整数。
    #[test]
    fn non_object_and_non_integer_inputs_are_rejected() {
        // 构造非法 JSON 形状。
        let values = [json!(null), json!([]), json!({ "timeoutMs": "5000" })];
        // 逐项验证不发生宽松转换。
        for value in values {
            // 解析非法形状并显式区分结果。
            let error = match parse_process_termination_input(ProcessTerminationMode::Force, &value)
            {
                // 成功表示 serde 边界过宽。
                Ok(_) => panic!("non-object or non-integer termination input must fail: {value}"),
                // 保存预期错误。
                Err(error) => error,
            };
            // 核对统一参数错误码。
            assert_eq!(error.code, "INVALID_ARGUMENT");
        }
    }
}
