//! 严格解析浏览器会话生命周期公开输入。

// 导入 JSON 值以验证公开 JSON over stdio 输入。
use serde_json::Value;

// 导入统一公开错误边界。
use crate::domain::{AppControlError, AppResult};

// 声明公开契约冻结的缺省总 deadline。
pub(crate) const DEFAULT_TIMEOUT_MS: u32 = 5_000;

// 声明公开契约冻结的最大总 deadline。
const MAXIMUM_TIMEOUT_MS: u32 = 30_000;

// 保存已验证且不含任何 transport 事实的生命周期输入。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BrowserSessionLifecycleInput {
    // 保存覆盖整个固定 broker 路线的不可扩张总预算。
    timeout_ms: u32,
}

// 为已验证输入提供最小只读投影。
impl BrowserSessionLifecycleInput {
    // 返回固定 broker client 可以消费的总 deadline。
    pub(crate) const fn timeout_ms(self) -> u32 {
        // 复制标量，避免暴露 JSON 原对象。
        self.timeout_ms
    }
}

// 构造不回显调用方 JSON 的稳定输入错误。
fn invalid_input() -> AppControlError {
    // 使用统一公开参数错误码。
    AppControlError::new(
        // 固定为已登记的公开错误码。
        "INVALID_ARGUMENT",
        // 只描述封闭字段和范围。
        "Browser session lifecycle input accepts timeoutMs from 1 through 30000 only.",
    )
}

// 解析严格 object 输入并应用公开缺省值。
pub(crate) fn parse_browser_session_lifecycle_input(
    // 接收 facade 已隔离出的可选 input 字段。
    value: Option<&Value>,
) -> AppResult<BrowserSessionLifecycleInput> {
    // 缺失 input 等价于空 object 并使用契约默认值。
    let Some(value) = value else {
        // 返回唯一缺省配置。
        return Ok(BrowserSessionLifecycleInput {
            // 固定使用五秒总预算。
            timeout_ms: DEFAULT_TIMEOUT_MS,
        });
    };
    // 输入必须是 object，不接受 null、array 或标量。
    let object = value.as_object().ok_or_else(invalid_input)?;
    // 只允许唯一公开 timeout 字段。
    if object.keys().any(|key| key != "timeoutMs") {
        // 未知字段不得进入 broker 或 worker。
        return Err(invalid_input());
    }
    // 缺失 timeout 使用同一公开默认值。
    let Some(timeout) = object.get("timeoutMs") else {
        // 返回缺省预算。
        return Ok(BrowserSessionLifecycleInput {
            // 固定使用五秒总预算。
            timeout_ms: DEFAULT_TIMEOUT_MS,
        });
    };
    // JSON 数字必须能无损表示为 u32。
    let timeout_ms = timeout.as_u64().and_then(|value| u32::try_from(value).ok());
    // 闭合范围，禁止零、负数、浮点和超长预算。
    let Some(timeout_ms) = timeout_ms.filter(|value| (1..=MAXIMUM_TIMEOUT_MS).contains(value))
    else {
        // 非法边界不启动或连接 broker。
        return Err(invalid_input());
    };
    // 返回不带原始 JSON 的已认证标量输入。
    Ok(BrowserSessionLifecycleInput { timeout_ms })
}

// 验证 browser session lifecycle 输入 Component 的封闭字段契约。
#[cfg(test)]
mod tests {
    // 导入 JSON 构造器。
    use serde_json::json;

    // 导入被测解析器与缺省值。
    use super::{DEFAULT_TIMEOUT_MS, parse_browser_session_lifecycle_input};

    // 验证 browser_session_public_route 输入严格性与默认 deadline。
    #[test]
    fn browser_session_public_route_input_is_closed_and_bounded() {
        // 缺失 input 必须使用冻结缺省值。
        assert_eq!(
            // 解析缺失 input。
            parse_browser_session_lifecycle_input(None)
                // 缺省输入必须合法。
                .expect("missing input must use default")
                // 读取已验证 deadline。
                .timeout_ms(),
            // 对比固定契约默认值。
            DEFAULT_TIMEOUT_MS,
        );
        // 允许最小合法 deadline。
        assert_eq!(
            // 解析最小值。
            parse_browser_session_lifecycle_input(Some(&json!({ "timeoutMs": 1 })))
                // 最小值必须合法。
                .expect("minimum timeout must be valid")
                // 读取结果。
                .timeout_ms(),
            // 保留最小值。
            1,
        );
        // 未知字段必须在 broker 前拒绝。
        assert!(parse_browser_session_lifecycle_input(Some(&json!({ "pipe": "x" }))).is_err());
        // 零值必须在 broker 前拒绝。
        assert!(parse_browser_session_lifecycle_input(Some(&json!({ "timeoutMs": 0 }))).is_err());
        // 超过最大预算必须在 broker 前拒绝。
        assert!(
            parse_browser_session_lifecycle_input(Some(&json!({ "timeoutMs": 30_001 }))).is_err()
        );
    }
}
